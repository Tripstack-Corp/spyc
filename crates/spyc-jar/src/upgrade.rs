//! Stage 3 (R33): the jar replaces its own binary while its program runs on.
//!
//! 1. Every thread that may hold bytes in flight parks at a safe point: the
//!    PTY reader between reads, each connection's reader between frames and
//!    its writer between batches, the acceptor and the child waiter.
//! 2. The jar's state is encoded as a [`handover`](crate::handover) blob, and
//!    the new binary must accept it (`validate-handover`) within a deadline.
//! 3. `BINARY hold-handover` takes copies of the listener and every
//!    connection, and the blob with the write end of a pipe, and holds the
//!    sockets until the new image has its own. macOS flushes a unix socket
//!    whose every reference is in flight, as these are from the `exec` until
//!    the new image receives them, whenever its collector runs (any unix
//!    socket closing anywhere sets it off), and both ends then read
//!    end-of-file. The collector looks only at sockets that were once in
//!    flight themselves, so the socketpair that carries them doesn't count as
//!    holding them; the holder's copies do. The holder writes the blob into
//!    the pipe, so the blob is only ever in memory (R19).
//! 4. The pipe's read end, the listener, the PTY master and every connection
//!    travel as `SCM_RIGHTS` over a socketpair whose other end becomes the new
//!    image's standard input, so the new image receives them as owned
//!    descriptors with nothing `unsafe`.
//! 5. The jar `exec`s `BINARY resume-handover`: same PID, same session, same
//!    child. It takes its descriptors, reads the blob from the pipe, then ends
//!    the holder and reaps it.
//!
//! Any failure before the `exec` unparks everything and refuses the upgrade:
//! the old jar runs on as if nothing happened.
//!
//! Every step runs one file (decision L, [`crate::store`]): the named binary
//! when it's an entry of the jar's store, or the jar's own copy of it, made
//! before anything parks. The named path is never looked up again, so a
//! file replaced there meanwhile never runs. The validator must print its
//! proof line, since its exit status alone proves nothing, and it has one
//! deadline for its exit and its output, after which its whole process
//! group is ended.
//!
//! Lookups and connections yet to say `hello` are neither waited for nor
//! carried: they hold nothing, and are closed at the `exec` (review finding
//! 5). Input the program hasn't taken can't be carried either, so an upgrade
//! waits until there is none.

use std::io::{BufRead, IoSlice, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags};

use crate::handover;
use crate::jar::{Conn, Shared, Who};
use crate::proto::{Msg, Role, reason};
use crate::state::ConnState;

/// The message that travels with the descriptors.
pub(crate) const FDS_TAG: &[u8; 8] = b"SPYCJARF";
/// The tag, then the holder's pid.
const HEAD: usize = 8 + 4;
/// The largest blob a holder or a new image takes.
const MAX_BLOB: u64 = 1 << 30;
/// How long a holder waits for the new image before it gives up.
const HOLD_LIMIT: Duration = Duration::from_secs(30);
/// How long a connection may take its last frames before the handover.
const FLUSH_DEADLINE: Duration = Duration::from_secs(1);
const PARK_DEADLINE: Duration = Duration::from_secs(3);
const VALIDATE_DEADLINE: Duration = Duration::from_secs(5);

/// The proof line's first field: a validator prints it, the blob's instance
/// and the bytes it decoded.
pub const PROOF: &str = "spyc-jar-handover-ok";

/// Start an upgrade to `binary`, asked for on `reply`, which hears why if it
/// fails. On success nothing is sent from here: the new image announces
/// itself with `Upgraded` on every connection it carried.
pub(crate) fn begin(shared: &Arc<Shared>, reply: Arc<Conn>, binary: String) {
    if shared.upgrading.swap(true, Ordering::SeqCst) {
        reply.send(
            shared,
            &Msg::Refused {
                reason: reason::UPGRADE_BUSY,
                why: "an upgrade is already under way".into(),
            },
        );
        return;
    }
    let shared = Arc::clone(shared);
    std::thread::spawn(move || {
        let (why, err) = match one_file(&shared, &binary) {
            Ok(image) => run(&shared, &image),
            Err(e) => e,
        };
        shared.upgrading.store(false, Ordering::SeqCst);
        shared.park_cv.notify_all();
        shared.cv.notify_all();
        reply.send(
            &shared,
            &Msg::Refused {
                reason: why,
                why: format!("upgrade refused: {err:#}"),
            },
        );
    });
}

/// The file this jar runs from: an entry of its store, its own copy, or for
/// a jar started from anywhere else, that path.
pub(crate) fn own_image() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// Where a jar with no store keeps its copies: beside its socket.
pub(crate) fn copies_dir(shared: &Shared) -> PathBuf {
    shared
        .cfg
        .socket
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .join(crate::store::COPIES)
}

/// The one file an upgrade to `binary` runs (decision L, part 1).
fn one_file(shared: &Shared, binary: &str) -> Result<PathBuf, (u16, anyhow::Error)> {
    let named = Path::new(binary);
    if !named.is_absolute() {
        return Err((reason::INVALID, anyhow!("{binary} isn't an absolute path")));
    }
    let store = crate::store::store_of(&own_image());
    // SPYC-TRAP(jar-upgrade-one-file): every step after this runs the file it
    // returns; looking `named` up again would run whatever is there by then.
    crate::store::one_file(named, store.as_deref(), &copies_dir(shared))
        .map_err(|e| (reason::UPGRADE_REJECTED, e))
}

/// Park, validate, hand over and `exec`. Returns only on failure, with its
/// `proto::reason`.
fn run(shared: &Shared, binary: &Path) -> (u16, anyhow::Error) {
    match try_run(shared, binary) {
        Ok(never) => match never {},
        Err(e) => e,
    }
}

#[expect(
    clippy::significant_drop_tightening,
    reason = "the listener is handed over under its lock"
)]
fn try_run(
    shared: &Shared,
    binary: &Path,
) -> Result<std::convert::Infallible, (u16, anyhow::Error)> {
    let busy = |e: anyhow::Error| (reason::UPGRADE_BUSY, e);
    let failed = |e: anyhow::Error| (reason::UPGRADE_FAILED, e);
    shared.cv.notify_all();
    shared.inbox_cv.notify_all();
    wait_parked(shared).map_err(busy)?;
    if shared.input_pending() {
        return Err(busy(anyhow!(
            "the program hasn't read the input it was sent"
        )));
    }
    let st = shared.lock();
    let conns = shared.conns();
    let mut ids: Vec<u64> = conns
        .iter()
        .filter(|(_, c)| c.role != Role::Lookup)
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    // Every writer is parked: what waits in an outbox goes out now, ahead of
    // `Upgraded`, or the connection stays behind.
    ids.retain(|id| flushed(&conns[id]));
    let outs: Vec<&UnixStream> = ids.iter().map(|id| &*conns[id].sock).collect();
    let states: Vec<ConnState> = ids
        .iter()
        .map(|id| {
            let c = &conns[id];
            ConnState {
                id: *id,
                role: c.role,
                cursor: *c.cursor.lock().unwrap_or_else(PoisonError::into_inner),
                exit_sent: c.exit_sent.load(Ordering::SeqCst),
                fg_sent: c.fg_sent.load(Ordering::SeqCst),
            }
        })
        .collect();
    if states.len() > handover::MAX_CONNS {
        return Err(busy(anyhow!(
            "{} connections, more than a handover carries",
            states.len()
        )));
    }
    let blob = handover::encode_parts(&shared.cfg, shared.child_pid, &st, &states);
    validate(binary, &blob, shared.cfg.instance).map_err(|e| (reason::UPGRADE_REJECTED, e))?;

    let (blob_read, blob_write) = pipe().map_err(failed)?;
    let (to_image, image_stdin) = UnixStream::pair().context("socketpair").map_err(failed)?;
    // Held across the handover: the holder and the new image take this
    // listener, and no connection may be accepted in between.
    let listener = shared
        .listener
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let held: Vec<BorrowedFd<'_>> = [blob_write.as_fd(), listener.as_fd()]
        .into_iter()
        .chain(outs.iter().map(|s| s.as_fd()))
        .collect();
    // SPYC-TRAP(jar-upgrade-holder): without these copies, macOS flushes
    // every connection while it is in flight across the `exec`.
    let mut holder = hold(binary, &held, &blob).map_err(failed)?;
    drop(blob_write);
    let mut fds: Vec<BorrowedFd<'_>> =
        vec![blob_read.as_fd(), listener.as_fd(), shared.master.as_fd()];
    fds.extend(outs.iter().map(|s| s.as_fd()));
    let mut space =
        vec![std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(fds.len()))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        let _ = holder.kill();
        let _ = holder.wait();
        return Err(failed(anyhow!("the descriptors don't fit one message")));
    }
    let pid = holder.id().to_be_bytes();
    let iov = [IoSlice::new(FDS_TAG), IoSlice::new(&pid)];
    if let Err(e) = rustix::net::sendmsg(&to_image, &iov, &mut control, SendFlags::empty()) {
        let _ = holder.kill();
        let _ = holder.wait();
        return Err(failed(anyhow!("send the descriptors: {e}")));
    }
    drop(to_image);
    // What stays behind is closed rather than left open in the new image.
    for (_, c) in conns.iter().filter(|(id, _)| !ids.contains(id)) {
        c.close();
    }
    for (_, sock) in shared
        .tokenless
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
    {
        let _ = sock.shutdown(std::net::Shutdown::Both);
    }
    // `PREVIOUS`: the image this one runs from, for a new image that can't
    // resume to switch back to.
    let err = Command::new(binary)
        .arg("resume-handover")
        .arg(own_image())
        .stdin(Stdio::from(OwnedFd::from(image_stdin)))
        .exec();
    // The `exec` failed, and std has already put the socketpair on standard
    // input, where it would keep a copy of every descriptor.
    if let Ok(null) = std::fs::File::open("/dev/null") {
        let _ = rustix::stdio::dup2_stdin(&null);
    }
    let _ = holder.kill();
    let _ = holder.wait();
    drop(outs);
    drop(conns);
    drop(st);
    Err(failed(anyhow!("exec {}: {err}", binary.display())))
}

/// Write what waits in `conn`'s outbox, its writer being parked. False when
/// the client won't take it in time: that connection is closed, not carried.
fn flushed(conn: &Conn) -> bool {
    let bytes = conn.take_outbox();
    if bytes.is_empty() {
        return true;
    }
    let _ = conn.sock.set_write_timeout(Some(FLUSH_DEADLINE));
    if (&*conn.sock).write_all(&bytes).is_ok() {
        return true;
    }
    conn.close();
    false
}

/// A pipe whose ends don't cross an `exec` unless handed over.
fn pipe() -> anyhow::Result<(OwnedFd, OwnedFd)> {
    let (r, w) = rustix::pipe::pipe().context("pipe")?;
    rustix::io::fcntl_setfd(&r, rustix::io::FdFlags::CLOEXEC)?;
    rustix::io::fcntl_setfd(&w, rustix::io::FdFlags::CLOEXEC)?;
    Ok((r, w))
}

/// Start `binary` holding `held` (step 3): the blob pipe's write end first,
/// then the sockets. It reads the blob after them, and answers once it has
/// both.
fn hold(
    binary: &Path,
    held: &[BorrowedFd<'_>],
    blob: &[u8],
) -> anyhow::Result<std::process::Child> {
    let (ours, theirs) = UnixStream::pair().context("socketpair")?;
    crate::jar::no_sigpipe(&ours);
    let mut child = Command::new(binary)
        .arg("hold-handover")
        .stdin(Stdio::from(OwnedFd::from(theirs)))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("run {} hold-handover", binary.display()))?;
    let given = (|| -> anyhow::Result<()> {
        let mut space =
            vec![std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(held.len()))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !control.push(SendAncillaryMessage::ScmRights(held)) {
            bail!("the sockets don't fit one message");
        }
        rustix::net::sendmsg(
            &ours,
            &[IoSlice::new(b"h")],
            &mut control,
            SendFlags::empty(),
        )?;
        (&ours).write_all(&(blob.len() as u64).to_be_bytes())?;
        (&ours).write_all(blob)?;
        ours.set_read_timeout(Some(VALIDATE_DEADLINE))?;
        let mut ack = [0u8; 1];
        (&ours)
            .read_exact(&mut ack)
            .context("no word from the holder")?;
        Ok(())
    })();
    if let Err(e) = given {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e.context(format!("{} didn't hold the handover", binary.display())));
    }
    Ok(child)
}

/// `spyc-jar hold-handover`: keep an upgrade's sockets alive across `exec`.
///
/// It takes the blob pipe and the sockets on standard input, then the blob;
/// says so; writes the blob into the pipe for the new image; and keeps the
/// sockets until the new image ends this process, or [`HOLD_LIMIT`].
pub fn hold_stdin() -> anyhow::Result<()> {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags};
    let sock = UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned()?);
    let mut space = vec![
        std::mem::MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(handover::MAX_CONNS + 2))
    ];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let mut byte = [0u8; 1];
    rustix::net::recvmsg(
        &sock,
        &mut [std::io::IoSliceMut::new(&mut byte)],
        &mut control,
        RecvFlags::empty(),
    )
    .context("receive the sockets")?;
    let mut held: Vec<OwnedFd> = control
        .drain()
        .filter_map(|m| match m {
            RecvAncillaryMessage::ScmRights(r) => Some(r.collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    if held.is_empty() {
        bail!("no blob pipe");
    }
    let pipe = std::fs::File::from(held.remove(0));
    let blob = read_blob(&mut &sock)?;
    (&sock).write_all(b"k").context("answer")?;
    // The new image reads the blob as the holder writes it; a pipe holds far
    // less than a blob.
    std::thread::spawn(move || {
        let mut pipe = pipe;
        let _ = pipe.write_all(&(blob.len() as u64).to_be_bytes());
        let _ = pipe.write_all(&blob);
    });
    std::thread::sleep(HOLD_LIMIT);
    drop(held);
    Ok(())
}

/// A blob as a holder and a new image take it: its length (u64), then its
/// bytes.
fn read_blob(r: &mut impl Read) -> anyhow::Result<Vec<u8>> {
    let mut len = [0u8; 8];
    r.read_exact(&mut len).context("the blob's length")?;
    let len = u64::from_be_bytes(len);
    if len > MAX_BLOB {
        bail!("a blob of {len} bytes is over the limit");
    }
    let mut blob = vec![0u8; usize::try_from(len)?];
    r.read_exact(&mut blob).context("the blob")?;
    Ok(blob)
}

/// Wait until every thread that may hold bytes in flight has parked.
fn wait_parked(shared: &Shared) -> anyhow::Result<()> {
    let until = Instant::now() + PARK_DEADLINE;
    let mut parked = shared.parked.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        let mut want = vec![Who::Acceptor];
        if !shared.pty_done.load(Ordering::SeqCst) {
            want.push(Who::Pty);
        }
        if !shared.waiter_done.load(Ordering::SeqCst) {
            want.push(Who::Waiter);
        }
        want.push(Who::Input);
        for (id, c) in shared
            .conns()
            .iter()
            .filter(|(_, c)| c.role != Role::Lookup)
        {
            want.push(Who::ConnRead(*id));
            if !c.writer_done.load(Ordering::SeqCst) {
                want.push(Who::ConnWrite(*id));
            }
        }
        let missing: Vec<_> = want.into_iter().filter(|w| !parked.contains(w)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        if Instant::now() >= until {
            bail!("not every thread reached a safe point: {missing:?}");
        }
        parked = shared
            .park_cv
            .wait_timeout(parked, Duration::from_millis(20))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// Have `binary` check the blob before anything is handed to it: it must
/// print the proof line for this blob within the deadline, which covers its
/// exit and its output. One that runs out of time is ended with its whole
/// process group. The blob goes over a socketpair, not a pipe: a validator
/// that refuses without reading it leaves a write that answers `EPIPE`, never
/// `SIGPIPE`.
fn validate(binary: &Path, blob: &[u8], instance: u64) -> anyhow::Result<()> {
    let (ours, theirs) = UnixStream::pair().context("socketpair")?;
    crate::jar::no_sigpipe(&ours);
    let mut child = Command::new(binary)
        .arg("validate-handover")
        .process_group(0)
        .stdin(Stdio::from(OwnedFd::from(theirs)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("run {} validate-handover", binary.display()))?;
    let group = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw);
    let end_group = || {
        if let Some(g) = group {
            let _ = rustix::process::kill_process_group(g, rustix::process::Signal::KILL);
        }
    };
    let blob_owned = blob.to_vec();
    std::thread::spawn(move || {
        let _ = (&ours).write_all(&blob_owned);
        let _ = ours.shutdown(std::net::Shutdown::Write);
    });
    // Read on threads of their own: a child of the validator may hold either
    // stream open long after the validator has gone.
    let (line_tx, line_rx) = std::sync::mpsc::channel();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut first = String::new();
            let _ = std::io::BufReader::new(out.take(4096)).read_line(&mut first);
            let _ = line_tx.send(first);
        });
    }
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut why = String::new();
            let _ = err.take(4096).read_to_string(&mut why);
            let _ = err_tx.send(why);
        });
    }
    let until = Instant::now() + VALIDATE_DEADLINE;
    let left = || until.saturating_duration_since(Instant::now());
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if left().is_zero() {
            end_group();
            let _ = child.wait();
            bail!(
                "{} didn't answer within {VALIDATE_DEADLINE:?}",
                binary.display()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        let why = err_rx.recv_timeout(left()).unwrap_or_default();
        bail!("{} rejected the handover: {}", binary.display(), why.trim());
    }
    let line = line_rx.recv_timeout(left()).unwrap_or_default();
    if !proves(&line, instance, blob.len()) {
        bail!(
            "{} exited without proving it took the handover (it printed {:?})",
            binary.display(),
            line.trim()
        );
    }
    Ok(())
}

/// Whether `line` is the proof line for the blob of `instance` and `len`
/// bytes. A later validator may add fields after these three.
fn proves(line: &str, instance: u64, len: usize) -> bool {
    let mut f = line.split_whitespace();
    f.next() == Some(PROOF)
        && f.next() == Some(format!("{instance:016x}").as_str())
        && f.next() == Some(len.to_string().as_str())
}

/// `spyc-jar validate-handover`: the blob on standard input; exit 0 and print
/// the proof line if this binary would take over from it.
pub fn validate_stdin() -> anyhow::Result<()> {
    let mut blob = Vec::new();
    std::io::stdin()
        .take(1 << 30)
        .read_to_end(&mut blob)
        .context("read the blob")?;
    let h = handover::decode(&blob)?;
    println!("{PROOF} {:016x} {}", h.cfg.instance, blob.len());
    Ok(())
}

/// `spyc-jar resume-handover`: take over from the jar that just `exec`ed this
/// binary, from the descriptors waiting on standard input.
pub fn resume() -> anyhow::Result<()> {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags};
    let sock = UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned()?);
    let mut head = [0u8; HEAD];
    let mut space = vec![
        std::mem::MaybeUninit::uninit();
        rustix::cmsg_space!(ScmRights(handover::MAX_CONNS + 3))
    ];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let got = rustix::net::recvmsg(
        &sock,
        &mut [std::io::IoSliceMut::new(&mut head)],
        &mut control,
        RecvFlags::empty(),
    )
    .context("receive the descriptors")?;
    let mut sock = sock;
    if got.bytes < head.len() {
        sock.read_exact(&mut head[got.bytes..])
            .context("the handover's head")?;
    }
    if &head[..8] != FDS_TAG {
        bail!("no handover on standard input");
    }
    let holder = u32::from_be_bytes(head[8..12].try_into().expect("4 bytes"));
    let mut fds: Vec<OwnedFd> = Vec::new();
    for m in control.drain() {
        if let RecvAncillaryMessage::ScmRights(r) = m {
            fds.extend(r);
        }
    }
    drop(sock);
    for fd in &fds {
        rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC)?;
    }
    let mut fds = fds.into_iter();
    let blob = read_blob(&mut std::fs::File::from(
        fds.next().context("no blob pipe")?,
    ))?;
    // Every socket is ours now, and the blob: the holder can go.
    if let Some(pid) = i32::try_from(holder)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        let _ = rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty());
    }
    let h = handover::decode(&blob)?;
    let listener = std::os::unix::net::UnixListener::from(fds.next().context("no listener")?);
    let master = fds.next().context("no PTY master")?;
    if !rustix::termios::isatty(&master) {
        bail!("the PTY master isn't a terminal");
    }
    let streams: Vec<UnixStream> = fds.map(UnixStream::from).collect();
    if streams.len() != h.conns.len() {
        bail!(
            "{} connections in the blob, {} descriptors",
            h.conns.len(),
            streams.len()
        );
    }
    let conns = h.conns.into_iter().zip(streams).collect();
    crate::jar::start(h.cfg, h.state, h.child_pid, master, listener, conns)
}

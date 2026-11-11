//! Stage 3 (R33): the jar replaces its own binary while its program runs on.
//!
//! 1. Every thread that may hold bytes in flight parks at a safe point: the
//!    PTY reader between reads, each connection's reader between frames and
//!    its writer between batches, the acceptor and the child waiter.
//! 2. The jar's state is encoded as a [`handover`](crate::handover) blob, and
//!    the new binary must accept it (`validate-handover`) within a deadline.
//! 3. The blob is sealed with AES-256-GCM under a fresh key and goes
//!    into an unlinked file (a `memfd` on Linux), so only ciphertext can reach
//!    a disk (R19). That file, the listener, the PTY master and every
//!    connection travel as `SCM_RIGHTS` over a socketpair whose other end
//!    becomes the new image's standard input, with the key and nonce in the
//!    same message, so the new image receives them as owned descriptors with
//!    nothing `unsafe`.
//! 4. The jar `exec`s `BINARY resume-handover`: same PID, same session, same
//!    child. In-flight descriptors keep the PTY and the connections open
//!    across the moment the old image's own copies close.
//!
//! Any failure before the `exec` unparks everything and refuses the upgrade:
//! the old jar runs on as if nothing happened.

use std::io::{IoSlice, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags};

use crate::handover;
use crate::jar::{ConnState, Shared, Who};
use crate::proto::{self, Msg};

/// The message that travels with the descriptors.
pub(crate) const FDS_TAG: &[u8; 8] = b"SPYCJARF";
const PARK_DEADLINE: Duration = Duration::from_secs(3);
const VALIDATE_DEADLINE: Duration = Duration::from_secs(5);

/// Start an upgrade to `binary`, asked for on the connection writing to
/// `reply`, which hears why if it fails. On success nothing is sent from here:
/// the new image announces itself with `Upgraded` on every connection.
pub(crate) fn begin(shared: &Arc<Shared>, reply: Arc<Mutex<std::os::unix::net::UnixStream>>, binary: String) {
    if shared.upgrading.swap(true, Ordering::SeqCst) {
        let mut s = reply.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = proto::write_msg(&mut *s, &Msg::Refused {
            why: "an upgrade is already under way".into(),
        });
        return;
    }
    let shared = Arc::clone(shared);
    std::thread::spawn(move || {
        let err = run(&shared, &binary);
        shared.upgrading.store(false, Ordering::SeqCst);
        shared.park_cv.notify_all();
        shared.cv.notify_all();
        let mut s = reply.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = proto::write_msg(&mut *s, &Msg::Refused {
            why: format!("upgrade refused: {err:#}"),
        });
    });
}

/// Park, validate, hand over and `exec`. Returns only on failure.
fn run(shared: &Shared, binary: &str) -> anyhow::Error {
    match try_run(shared, binary) {
        Ok(never) => match never {},
        Err(e) => e,
    }
}

enum Never {}

fn try_run(shared: &Shared, binary: &str) -> anyhow::Result<Never> {
    shared.cv.notify_all();
    wait_parked(shared)?;
    let st = shared.lock();
    let conns = shared.conns();
    let mut ids: Vec<u64> = conns.keys().copied().collect();
    ids.sort_unstable();
    let outs: Vec<_> = ids
        .iter()
        .map(|id| conns[id].out.lock().unwrap_or_else(PoisonError::into_inner))
        .collect();
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
        bail!("{} connections, more than a handover carries", states.len());
    }
    let blob = handover::encode_parts(&shared.cfg, shared.child_pid, &st, &states);
    validate(binary, &blob)?;

    let (key, nonce, sealed) = seal(&blob)?;
    let blob_file = blob_file(shared, &sealed)?;
    let (ours, theirs) = UnixStream::pair().context("socketpair")?;
    let mut fds: Vec<BorrowedFd<'_>> = vec![blob_file.as_fd(), shared.listener.as_fd(), shared.master.as_fd()];
    fds.extend(outs.iter().map(|s| s.as_fd()));
    let mut space = vec![std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(fds.len()))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        bail!("the descriptors don't fit one message");
    }
    let iov = [IoSlice::new(FDS_TAG), IoSlice::new(&key), IoSlice::new(&nonce)];
    rustix::net::sendmsg(&ours, &iov, &mut control, SendFlags::empty())
        .context("send the descriptors")?;
    drop(ours);
    let err = Command::new(binary)
        .arg("resume-handover")
        .stdin(Stdio::from(OwnedFd::from(theirs)))
        .exec();
    drop(outs);
    drop(conns);
    drop(st);
    Err(anyhow!("exec {binary}: {err}"))
}

/// The blob sealed under a fresh random key and nonce, used once.
fn seal(blob: &[u8]) -> anyhow::Result<([u8; 32], [u8; 12], Vec<u8>)> {
    use aes_gcm::aead::{Aead, KeyInit};
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 12];
    let mut rng = std::fs::File::open("/dev/urandom").context("open /dev/urandom")?;
    rng.read_exact(&mut key)?;
    rng.read_exact(&mut nonce)?;
    let sealed = aes_gcm::Aes256Gcm::new((&key).into())
        .encrypt((&nonce).into(), blob)
        .map_err(|_| anyhow!("seal the handover blob"))?;
    Ok((key, nonce, sealed))
}

fn open_sealed(sealed: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> anyhow::Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    aes_gcm::Aes256Gcm::new(key.into())
        .decrypt(nonce.into(), sealed)
        .map_err(|_| anyhow!("the handover blob doesn't open with its key"))
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
        for (id, c) in shared.conns().iter() {
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

/// Have `binary` check the blob before anything is handed to it.
fn validate(binary: &str, blob: &[u8]) -> anyhow::Result<()> {
    let mut child = Command::new(binary)
        .arg("validate-handover")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("run {binary} validate-handover"))?;
    let mut stdin = child.stdin.take().context("the validator's input")?;
    let blob = blob.to_vec();
    let feeder = std::thread::spawn(move || {
        let _ = stdin.write_all(&blob);
    });
    let until = Instant::now() + VALIDATE_DEADLINE;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{binary} didn't answer within {VALIDATE_DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let _ = feeder.join();
    if !status.success() {
        let mut why = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut why);
        }
        bail!("{binary} rejected the handover: {}", why.trim());
    }
    Ok(())
}

/// The blob in memory only (R19): a `memfd`, which never has a name on any
/// filesystem.
#[cfg(target_os = "linux")]
fn blob_file(_shared: &Shared, blob: &[u8]) -> anyhow::Result<std::fs::File> {
    let fd = rustix::fs::memfd_create("spyc-jar-handover", rustix::fs::MemfdFlags::CLOEXEC)
        .context("memfd_create")?;
    let mut f = std::fs::File::from(fd);
    f.write_all(blob).context("write the handover blob")?;
    std::io::Seek::rewind(&mut f)?;
    Ok(f)
}

/// macOS has no `memfd`, and its POSIX shared memory can't be written, only
/// mapped: the sealed blob goes in a file only this process can reach,
/// created beside the socket in the jar's `0700` directory, readable by the
/// owner alone, and unlinked before it's handed on. Whatever of it reaches
/// the disk is ciphertext, its key only ever in the socket's buffer.
#[cfg(not(target_os = "linux"))]
fn blob_file(shared: &Shared, blob: &[u8]) -> anyhow::Result<std::fs::File> {
    let path = shared.cfg.socket.with_extension("handover");
    let _ = std::fs::remove_file(&path);
    let mut f = std::fs::File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("create {}", path.display()))?;
    let _ = std::fs::remove_file(&path);
    f.write_all(blob).context("write the handover blob")?;
    std::io::Seek::rewind(&mut f)?;
    Ok(f)
}

/// `spyc-jar validate-handover`: the blob on standard input, exit 0 if this
/// binary would take over from it.
pub fn validate_stdin() -> anyhow::Result<()> {
    let mut blob = Vec::new();
    std::io::stdin()
        .take(1 << 30)
        .read_to_end(&mut blob)
        .context("read the blob")?;
    handover::validate(&blob)?;
    Ok(())
}

/// `spyc-jar resume-handover`: take over from the jar that just `exec`ed this
/// binary, from the descriptors waiting on standard input.
pub fn resume() -> anyhow::Result<()> {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags};
    let sock = UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned()?);
    let mut head = [0u8; 52];
    let mut space = vec![std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(handover::MAX_CONNS + 3))];
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
        sock.read_exact(&mut head[got.bytes..]).context("the handover's key")?;
    }
    if &head[..8] != FDS_TAG {
        bail!("no handover on standard input");
    }
    let key: [u8; 32] = head[8..40].try_into().expect("32 bytes");
    let nonce: [u8; 12] = head[40..52].try_into().expect("12 bytes");
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
    let mut sealed = Vec::new();
    std::fs::File::from(fds.next().context("no blob")?)
        .take(1 << 30)
        .read_to_end(&mut sealed)?;
    let blob = open_sealed(&sealed, &key, &nonce)?;
    let h = handover::decode(&blob)?;
    let listener = std::os::unix::net::UnixListener::from(fds.next().context("no listener")?);
    let master = fds.next().context("no PTY master")?;
    if !rustix::termios::isatty(&master) {
        bail!("the PTY master isn't a terminal");
    }
    let streams: Vec<UnixStream> = fds.map(UnixStream::from).collect();
    if streams.len() != h.conns.len() {
        bail!("{} connections in the blob, {} descriptors", h.conns.len(), streams.len());
    }
    let conns = h.conns.into_iter().zip(streams).collect();
    crate::jar::start(h.cfg, h.state, h.child_pid, master, listener, conns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_blob_hides_its_bytes_and_opens_only_whole_with_its_key() {
        let blob = handover::sample_blob();
        let (key, nonce, sealed) = seal(&blob).expect("seal");
        for plain in [&handover::MAGIC[..], b"echo hi", b"/tmp/spyc/jars/sample.sock"] {
            assert!(
                !sealed.windows(plain.len()).any(|w| w == plain),
                "{:?} is readable in what reaches a file",
                String::from_utf8_lossy(plain)
            );
            assert!(blob.windows(plain.len()).any(|w| w == plain), "the check would see it");
        }
        assert_eq!(open_sealed(&sealed, &key, &nonce).expect("open"), blob);
        let mut tampered = sealed.clone();
        tampered[20] ^= 1;
        assert!(open_sealed(&tampered, &key, &nonce).is_err(), "a changed byte is refused");
        let mut other = key;
        other[0] ^= 1;
        assert!(open_sealed(&sealed, &other, &nonce).is_err(), "another key is refused");
    }
}

//! The owner's end of a pane's jar, as `portable-pty`'s traits: [`JarMaster`]
//! and [`JarChild`] stand in for the PTY master and the child, so `PtyHost`
//! drives a jarred program as it drives a local one.
//!
//! One reader thread takes the jar's frames. Output goes to `PtyHost`'s reader
//! through a bounded channel, whose backpressure paces the program (R11), and
//! is acknowledged once handed over. Everything spyc sends goes through the
//! connection's [`Outbox`], whose own thread does the writing and the pings
//! (finding 4). The foreground process group (R4) and the exit arrive as
//! frames and are kept, so no call here waits on the jar.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::time::Duration;

use anyhow::Context;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use spyc_jar::client::{Outbox, Receiver};
use spyc_jar::proto::{JarSignal, Msg};

use super::offsets::Offsets;
use super::place::{SocketState, socket_state};

/// What every half of one jarred pane shares.
pub(super) struct Link {
    pub out: Outbox,
    pub pid: u32,
    /// The foreground process group the jar last reported (R4); 0 unknown.
    pub fg: AtomicU32,
    pub exit: Mutex<Option<ExitStatus>>,
    pub exited: Condvar,
    /// Which jar offset each byte handed to the pane came from (finding 8).
    pub offsets: Mutex<Offsets>,
    /// When the terminal's replies may reach the program again (R15a).
    pub gate: ReplayGate,
}

/// How long after a replayed tail is handed over its queries may still be in
/// the parser.
const REPLY_GRACE: Duration = Duration::from_millis(500);

/// R15a: a replayed tail makes the engine see the program's old queries
/// again, and the program must never get a second answer to a question it
/// asked before the crash. Its replies are dropped until the tail has been handed over, and a grace for
/// the parser after that.
pub(super) struct ReplayGate {
    /// The jar's head when this spyc took over: the bytes before it are
    /// replay. `None`: nothing to replay, open from the start.
    live_from: Option<u64>,
    caught_up: Mutex<Option<std::time::Instant>>,
}

impl ReplayGate {
    /// A gate with nothing to replay.
    pub(super) const fn open() -> Self {
        Self {
            live_from: None,
            caught_up: Mutex::new(None),
        }
    }

    /// A gate closed until the bytes before `head` are handed over.
    pub(super) const fn until(head: u64) -> Self {
        Self {
            live_from: Some(head),
            caught_up: Mutex::new(None),
        }
    }

    pub(super) fn handed_over(&self, end: u64) {
        let mut at = self
            .caught_up
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if at.is_none() && self.live_from.is_some_and(|head| end >= head) {
            *at = Some(std::time::Instant::now());
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.live_from.is_none()
            || self
                .caught_up
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some_and(|t| t.elapsed() >= REPLY_GRACE)
    }
}

impl Link {
    fn set_exit(&self, status: ExitStatus) {
        self.exit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(status);
        self.exited.notify_all();
    }

    pub(super) fn signal(&self, signal: JarSignal) -> io::Result<()> {
        self.out.send(&Msg::Signal {
            signal: signal as i32,
        })
    }
}

/// Cancel (`CAN`): the byte that returns a VT parser to its ground state from
/// inside any sequence, handed to the pane before the first byte after a gap,
/// which may begin inside one.
pub(super) const RESYNC: u8 = 0x18;

/// How long [`JarChild::wait`] waits for the exit of a program it was told
/// to kill: a jar that never reports one can't hold spyc's shutdown.
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// Run the reader thread for the connection `rx`, handing output to `bytes`.
pub(super) fn read_frames(
    mut rx: Receiver,
    link: &Link,
    bytes: &mpsc::SyncSender<Vec<u8>>,
    socket: &std::path::Path,
) {
    loop {
        match rx.recv(Duration::from_secs(3600)) {
            Ok(Some(Msg::Output { offset, bytes: b })) => {
                let end = offset + b.len() as u64;
                let jumped = link
                    .offsets
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .hand(offset, b.len() as u64);
                if jumped && bytes.send(vec![RESYNC]).is_err() {
                    break;
                }
                if bytes.send(b).is_err() {
                    break;
                }
                link.gate.handed_over(end);
                let _ = link.out.send(&Msg::Ack { offset: end });
            }
            Ok(Some(Msg::Exited { code, signaled })) => {
                link.set_exit(if signaled {
                    ExitStatus::with_signal(&format!("signal {code}"))
                } else {
                    ExitStatus::with_exit_code(u32::try_from(code).unwrap_or(1))
                });
                let _ = link.out.send(&Msg::AckExit);
                break;
            }
            Ok(Some(Msg::Foreground { pgid })) => link.fg.store(pgid, Ordering::Relaxed),
            Ok(_) => {}
            Err(e) => {
                let lost = match socket_state(socket) {
                    SocketState::Live | SocketState::Unknown => Lost::Held,
                    SocketState::Stale | SocketState::Missing => Lost::Gone,
                };
                crate::spyc_debug!("jar {}: {} after {e:#}", socket.display(), lost.label());
                link.set_exit(ExitStatus::with_signal(lost.label()));
                break;
            }
        }
    }
    // Every way out above has an exit, but one that ended the loop without
    // one still mustn't leave a `wait` hanging.
    link.set_exit(ExitStatus::with_signal(Lost::Gone.label()));
}

/// Why a pane's connection to its jar ended without an exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lost {
    /// The jar still runs: it ended this owner's lease (decision G).
    Held,
    /// The jar is gone, and its program with it.
    Gone,
}

impl Lost {
    /// What the pane's exit status carries, and its tab shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Held => "detached",
            Self::Gone => "jar lost",
        }
    }
}

/// The program's output, as `PtyHost`'s reader reads it.
pub(super) struct JarReader {
    pub rx: mpsc::Receiver<Vec<u8>>,
    pub buf: Vec<u8>,
    pub pos: usize,
}

impl Read for JarReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Input, as `Input` frames.
struct JarWriter(Arc<Link>);

impl Write for JarWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.out.send(&Msg::Input {
            bytes: buf.to_vec(),
        })?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The terminal's answers to the program's queries, as `Reply` frames (R15).
pub(super) struct JarReplies(pub Arc<Link>);

impl Write for JarReplies {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.0.gate.is_open() {
            self.0.out.send(&Msg::Reply {
                bytes: buf.to_vec(),
            })?;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct JarMaster {
    pub link: Arc<Link>,
    pub size: Mutex<PtySize>,
    pub reader: Mutex<Option<JarReader>>,
}

impl MasterPty for JarMaster {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        *self.size.lock().unwrap_or_else(PoisonError::into_inner) = size;
        self.link.out.send(&Msg::Resize {
            rows: size.rows,
            cols: size.cols,
            px_w: size.pixel_width,
            px_h: size.pixel_height,
        })?;
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(*self.size.lock().unwrap_or_else(PoisonError::into_inner))
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        let reader = self
            .reader
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        Ok(Box::new(reader.context("a jar's output has one reader")?))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(JarWriter(Arc::clone(&self.link))))
    }
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        match self.link.fg.load(Ordering::Relaxed) {
            0 => None,
            pgid => libc::pid_t::try_from(pgid).ok(),
        }
    }
    fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        None
    }
    fn tty_name(&self) -> Option<PathBuf> {
        None
    }
}

/// The program in the jar. Its pid is the jar's report, for reading what the
/// process is doing; every signal goes through the jar (finding 10).
pub(super) struct JarChild(pub Arc<Link>);

impl std::fmt::Debug for JarChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JarChild({})", self.0.pid)
    }
}

impl ChildKiller for JarChild {
    fn kill(&mut self) -> io::Result<()> {
        self.0.signal(JarSignal::Kill)
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(JarKiller(Arc::clone(&self.0)))
    }
}

impl Child for JarChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(self
            .0
            .exit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }
    /// The exit, or `TimedOut` once [`EXIT_WAIT`] passes without one.
    fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self
            .0
            .exited
            .wait_timeout_while(
                self.0.exit.lock().unwrap_or_else(PoisonError::into_inner),
                EXIT_WAIT,
                |s| s.is_none(),
            )
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .clone();
        status.ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the jar reported no exit"))
    }
    fn process_id(&self) -> Option<u32> {
        Some(self.0.pid)
    }
}

struct JarKiller(Arc<Link>);

impl std::fmt::Debug for JarKiller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JarKiller({})", self.0.pid)
    }
}

impl ChildKiller for JarKiller {
    fn kill(&mut self) -> io::Result<()> {
        self.0.signal(JarSignal::Kill)
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Self(Arc::clone(&self.0)))
    }
}

/// A detached tab's stand-ins: its jar runs on for another spyc, so here
/// there is no output, no input and no program, only the exit that labels
/// the tab `[detached]`.
pub(super) fn detached(socket: &std::path::Path, (rows, cols): (u16, u16)) -> super::Jarred {
    super::Jarred {
        master: Box::new(GoneMaster(Mutex::new(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }))),
        child: Box::new(GoneChild),
        replies: Box::new(io::sink()),
        signals: None,
        snapshots: None,
        socket: socket.to_path_buf(),
        adoption: None,
    }
}

struct GoneMaster(Mutex<PtySize>);

impl MasterPty for GoneMaster {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = size;
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(*self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(io::empty()))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(io::sink()))
    }
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }
    fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        None
    }
    fn tty_name(&self) -> Option<PathBuf> {
        None
    }
}

#[derive(Debug)]
struct GoneChild;

impl ChildKiller for GoneChild {
    fn kill(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Self)
    }
}

impl Child for GoneChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(Some(ExitStatus::with_signal(Lost::Held.label())))
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        Ok(ExitStatus::with_signal(Lost::Held.label()))
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
}

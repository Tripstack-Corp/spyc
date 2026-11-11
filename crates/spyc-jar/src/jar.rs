//! The jar: one program on a PTY, an event log of what it printed and how big
//! its terminal was, and at most one owner.
//!
//! Threads, each blocking on one thing:
//! - the PTY reader appends output to the log, and waits while a live owner is
//!   a window behind (R11);
//! - the child waiter records the exit;
//! - one reader and one writer per connection;
//! - the reaper ends the jar once its program has ended and the owner
//!   acknowledged it, or nobody came back within the linger time.
//!
//! The log never parses a byte. A client starts each connection with a
//! `Replay { from }` and gets every event at or after that offset, after a
//! `Gap` when `from` has already been dropped.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::proto::{self, JarSignal, Msg, Role, Snapshot};

pub struct Config {
    pub socket: PathBuf,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub rows: u16,
    pub cols: u16,
    /// Bytes of output the log holds at most.
    pub ring_cap: usize,
    /// How far a live owner may fall behind before the program waits (R11).
    pub window: u64,
    /// How long an owner may stay silent before its lease lapses (R21).
    pub deadline: Duration,
    /// How long an ended program's jar waits for an owner to collect the exit.
    pub linger_after_exit: Duration,
}

enum Event {
    Out(u64, Vec<u8>),
    Resized(u64, u16, u16),
}

impl Event {
    const fn offset(&self) -> u64 {
        match self {
            Self::Out(o, _) | Self::Resized(o, _, _) => *o,
        }
    }
}

/// The event log. `seq` numbers events so a resize at an offset already sent
/// is still sent once; offsets number bytes.
struct Log {
    events: VecDeque<(u64, Event)>,
    next_seq: u64,
    /// Offset of the next byte the program prints.
    head: u64,
    /// Offset of the oldest byte still held; earlier ones are gone.
    base: u64,
    held: usize,
    cap: usize,
}

impl Log {
    fn push(&mut self, event: Event) {
        if let Event::Out(_, bytes) = &event {
            self.head += bytes.len() as u64;
            self.held += bytes.len();
        }
        self.events.push_back((self.next_seq, event));
        self.next_seq += 1;
        self.trim(0);
    }

    /// Drop events wholly below `keep_from`, then the oldest until the log
    /// fits its cap.
    fn trim(&mut self, keep_from: u64) {
        while let Some((_, front)) = self.events.front() {
            // An output is wholly before `keep_from` once it ends at or before
            // it; a resize only when it happened strictly before it, since one
            // at `keep_from` lays out every byte from there on.
            let below = match front {
                Event::Out(o, b) => o + b.len() as u64 <= keep_from,
                Event::Resized(o, ..) => *o < keep_from,
            };
            if !below && self.held <= self.cap {
                break;
            }
            if let Some((_, Event::Out(o, b))) = self.events.pop_front() {
                self.held -= b.len();
                self.base = o + b.len() as u64;
            }
        }
        if self.events.is_empty() {
            self.base = self.head;
        }
    }

    /// The first sequence number to send to a client replaying from `from`.
    fn seq_for(&self, from: u64) -> u64 {
        self.events
            .iter()
            .find(|(_, e)| match e {
                Event::Out(o, b) => o + b.len() as u64 > from,
                Event::Resized(o, ..) => *o >= from,
            })
            .map_or(self.next_seq, |(seq, _)| *seq)
    }
}

struct State {
    log: Log,
    owner: Option<u64>,
    acked: u64,
    /// The latest snapshot of each format (R17a): the binary one for the same
    /// build, the portable one for an upgrade across a pin bump.
    snapshots: Vec<Snapshot>,
    exit: Option<(i32, bool)>,
    exit_acked: bool,
    pty_eof: bool,
    rows: u16,
    cols: u16,
    next_conn: u64,
    ownerless_since: Option<Instant>,
    /// The PTY's foreground process group, 0 until read.
    fg: u32,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
    master: Mutex<Box<dyn MasterPty + Send>>,
    input: Mutex<Box<dyn Write + Send>>,
    child_pid: u32,
    cfg: Config,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Spawn the program and serve the socket until the jar ends. Never returns
/// on success: the reaper ends the process.
pub fn run(cfg: Config, listener: UnixListener) -> anyhow::Result<()> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: cfg.rows,
            cols: cfg.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open a PTY")?;
    let mut cmd = CommandBuilder::new(&cfg.argv[0]);
    cmd.args(&cfg.argv[1..]);
    if let Some(cwd) = &cfg.cwd {
        cmd.cwd(cwd);
    }
    let reader = pair.master.try_clone_reader().context("PTY reader")?;
    let input = pair.master.take_writer().context("PTY writer")?;
    let mut child = pair.slave.spawn_command(cmd).context("spawn the program")?;
    drop(pair.slave);
    let child_pid = child.process_id().unwrap_or(0);

    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            log: Log {
                events: VecDeque::new(),
                next_seq: 0,
                head: 0,
                base: 0,
                held: 0,
                cap: cfg.ring_cap,
            },
            owner: None,
            acked: 0,
            snapshots: Vec::new(),
            exit: None,
            exit_acked: false,
            pty_eof: false,
            rows: cfg.rows,
            cols: cfg.cols,
            next_conn: 1,
            ownerless_since: Some(Instant::now()),
            fg: 0,
        }),
        cv: Condvar::new(),
        master: Mutex::new(pair.master),
        input: Mutex::new(input),
        child_pid,
        cfg,
    });

    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || pump_pty(&shared, reader));
    }
    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let status = child.wait();
            let mut st = shared.lock();
            st.exit = Some(match status {
                Ok(s) => match s.signal() {
                    Some(_) => (-1, true),
                    None => (i32::try_from(s.exit_code()).unwrap_or(i32::MAX), false),
                },
                Err(_) => (-1, false),
            });
            shared.cv.notify_all();
        });
    }
    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || reap(&shared));
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let shared = Arc::clone(&shared);
        thread::spawn(move || serve(&shared, stream));
    }
    Ok(())
}

/// Copy the program's output into the log, pausing while a live owner is a
/// window behind.
fn pump_pty(shared: &Shared, mut reader: Box<dyn Read + Send>) {
    // The window is checked before each read, so a live owner can be a
    // window plus one read behind: keep reads small.
    let mut buf = vec![0u8; 8192];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut st = shared.lock();
        while st.owner.is_some() && st.log.head - st.acked >= shared.cfg.window {
            st = shared.cv.wait(st).unwrap_or_else(PoisonError::into_inner);
        }
        let at = st.log.head;
        st.log.push(Event::Out(at, buf[..n].to_vec()));
        if let Some(keep) = st.snapshots.iter().map(|s| s.offset).min() {
            st.log.trim(keep);
        }
        shared.cv.notify_all();
    }
    shared.lock().pty_eof = true;
    shared.cv.notify_all();
}

/// End the jar once its program has ended and the exit was collected, or
/// nobody came back for it within the linger time.
fn reap(shared: &Shared) {
    let mut st = shared.lock();
    loop {
        // R4: the foreground process group, read here because only the
        // master's holder can. A job starting needn't print anything.
        let fg = shared
            .master
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .process_group_leader()
            .and_then(|p| u32::try_from(p).ok())
            .unwrap_or(0);
        if fg != st.fg {
            st.fg = fg;
            shared.cv.notify_all();
        }
        if st.exit.is_some() {
            let alone_too_long = st
                .ownerless_since
                .is_some_and(|t| t.elapsed() >= shared.cfg.linger_after_exit);
            if st.exit_acked || alone_too_long {
                let _ = std::fs::remove_file(&shared.cfg.socket);
                std::process::exit(0);
            }
        }
        st = shared
            .cv
            .wait_timeout(st, Duration::from_millis(200))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// One connection: its `hello`, a writer thread streaming the log, and this
/// thread reading what the client sends.
fn serve(shared: &Arc<Shared>, stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut read_half = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let out = Arc::new(Mutex::new(stream));
    let send = |msg: &Msg| {
        let mut s = out.lock().unwrap_or_else(PoisonError::into_inner);
        proto::write_msg(&mut *s, msg)
    };
    let Ok(Some(Msg::Hello { version, role })) = proto::read_msg(&mut read_half) else {
        let _ = send(&Msg::Refused {
            why: "expected a hello".into(),
        });
        return;
    };
    if version != proto::VERSION {
        let _ = send(&Msg::Refused {
            why: format!("jar speaks version {}", proto::VERSION),
        });
        return;
    }
    let id = {
        let mut st = shared.lock();
        if role == Role::Owner && st.owner.is_some() {
            drop(st);
            let _ = send(&Msg::Refused {
                why: "the jar already has an owner".into(),
            });
            return;
        }
        let id = st.next_conn;
        st.next_conn += 1;
        if role == Role::Owner {
            st.owner = Some(id);
            st.ownerless_since = None;
            st.acked = st.log.base.max(st.acked.min(st.log.head));
        }
        let welcome = Msg::Welcome {
            version: proto::VERSION,
            child_pid: shared.child_pid,
            head: st.log.head,
            base: st.log.base,
            rows: st.rows,
            cols: st.cols,
        };
        drop(st);
        if send(&welcome).is_err() {
            release(shared, id);
            return;
        }
        id
    };
    let owner = role == Role::Owner;
    let _ = read_half.set_read_timeout(Some(if owner {
        shared.cfg.deadline
    } else {
        Duration::from_secs(3600)
    }));

    // `cursor`: where the writer is in the log; `None` until the first Replay.
    let cursor: Arc<Mutex<Option<(u64, u64)>>> = Arc::new(Mutex::new(None));
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let shared = Arc::clone(shared);
        let out = Arc::clone(&out);
        let cursor = Arc::clone(&cursor);
        let closed = Arc::clone(&closed);
        thread::spawn(move || stream_log(&shared, &out, &cursor, &closed))
    };

    loop {
        let msg = match proto::read_msg(&mut read_half) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(proto::ReadError::UnknownCritical(kind)) => {
                let _ = send(&Msg::Unsupported { kind });
                continue;
            }
            Err(_) => break,
        };
        match msg {
            Msg::Replay { from: asked } => {
                let st = shared.lock();
                let gap_to = (asked < st.log.base).then_some(st.log.base);
                let from = asked.max(st.log.base).min(st.log.head);
                let seq = st.log.seq_for(from);
                drop(st);
                if let Some(to) = gap_to {
                    let _ = send(&Msg::Gap { from: asked, to });
                }
                *cursor.lock().unwrap_or_else(PoisonError::into_inner) = Some((seq, from));
                shared.cv.notify_all();
            }
            Msg::Ping { n } => {
                let _ = send(&Msg::Pong { n });
            }
            // Every snapshot held, then `Snapshot(None)` to end the list.
            Msg::FetchSnapshot => {
                let snaps = shared.lock().snapshots.clone();
                for s in snaps {
                    let _ = send(&Msg::Snapshot(Some(s)));
                }
                let _ = send(&Msg::Snapshot(None));
            }
            _ if !owner => {
                let _ = send(&Msg::Refused {
                    why: "a reader can only watch".into(),
                });
            }
            Msg::Input { bytes } | Msg::Reply { bytes } => {
                if let Some(path) = std::env::var_os("SPYC_JAR_TRACE") {
                    use std::io::Write as _;
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        let _ = writeln!(
                            f,
                            "{} {:?}",
                            shared.child_pid,
                            String::from_utf8_lossy(&bytes)
                        );
                    }
                }
                let mut w = shared.input.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = w.write_all(&bytes);
                let _ = w.flush();
            }
            Msg::Resize {
                rows,
                cols,
                px_w,
                px_h,
            } => {
                let size = PtySize {
                    rows,
                    cols,
                    pixel_width: px_w,
                    pixel_height: px_h,
                };
                let resized = shared
                    .master
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .resize(size)
                    .is_ok();
                if resized {
                    let mut st = shared.lock();
                    st.rows = rows;
                    st.cols = cols;
                    let at = st.log.head;
                    st.log.push(Event::Resized(at, rows, cols));
                    shared.cv.notify_all();
                }
            }
            Msg::Ack { offset } => {
                let mut st = shared.lock();
                st.acked = st.acked.max(offset.min(st.log.head));
                shared.cv.notify_all();
            }
            Msg::Signal { signal } => deliver(shared.child_pid, signal),
            Msg::StoreSnapshot(snap) => {
                let mut st = shared.lock();
                st.snapshots.retain(|s| s.format != snap.format);
                st.snapshots.push(snap);
                // Keep the bytes after the oldest snapshot: either may be used.
                let keep = st.snapshots.iter().map(|s| s.offset).min().unwrap_or(0);
                let keep = keep.min(st.log.head);
                st.log.trim(keep);
            }
            Msg::AckExit => {
                shared.lock().exit_acked = true;
                shared.cv.notify_all();
            }
            other => {
                let _ = send(&Msg::Refused {
                    why: format!("not a client message: {other:?}"),
                });
            }
        }
    }
    closed.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(s) = out.lock() {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    release(shared, id);
    let _ = writer.join();
}

/// A connection ended: if it owned the jar, the jar has no owner now.
fn release(shared: &Shared, id: u64) {
    let mut st = shared.lock();
    if st.owner == Some(id) {
        st.owner = None;
        st.ownerless_since = Some(Instant::now());
    }
    shared.cv.notify_all();
}

fn deliver(pid: u32, signal: i32) {
    use rustix::process::Signal;
    let Some(sig) = JarSignal::from_wire(signal) else {
        return;
    };
    let sig = match sig {
        JarSignal::Interrupt => Signal::INT,
        JarSignal::Terminate => Signal::TERM,
        JarSignal::Kill => Signal::KILL,
        JarSignal::Stop => Signal::STOP,
        JarSignal::Continue => Signal::CONT,
        JarSignal::Hangup => Signal::HUP,
    };
    if let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(pid, sig);
    }
}

/// Stream the log to one connection from its cursor, then the exit once
/// everything before it has gone out.
fn stream_log(
    shared: &Shared,
    out: &Mutex<UnixStream>,
    cursor: &Mutex<Option<(u64, u64)>>,
    closed: &std::sync::atomic::AtomicBool,
) {
    let mut exit_sent = false;
    let mut fg_sent = 0u32;
    loop {
        let mut st = shared.lock();
        let batch = loop {
            if closed.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let pos = *cursor.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((seq, from)) = pos {
                let pending = seq < st.log.next_seq;
                let exit_due = !exit_sent && st.exit.is_some() && st.pty_eof && !pending;
                let fg_due = st.fg != fg_sent;
                if pending || exit_due || fg_due {
                    let (mut frames, next) =
                        collect(&st, seq, from, exit_due.then(|| st.exit).flatten());
                    if fg_due {
                        frames.insert(0, Msg::Foreground { pgid: st.fg });
                        fg_sent = st.fg;
                    }
                    break (frames, next);
                }
            }
            st = shared
                .cv
                .wait_timeout(st, Duration::from_millis(500))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        };
        drop(st);
        let (frames, next) = batch;
        {
            let mut c = cursor.lock().unwrap_or_else(PoisonError::into_inner);
            if c.is_some_and(|(seq, _)| seq <= next.0) {
                *c = Some(next);
            }
        }
        // One write for the whole batch.
        let mut bytes = Vec::new();
        for f in &frames {
            if let Msg::Exited { .. } = f {
                exit_sent = true;
            }
            bytes.extend(proto::encode(f));
        }
        let mut s = out.lock().unwrap_or_else(PoisonError::into_inner);
        if s.write_all(&bytes).and_then(|()| s.flush()).is_err() {
            return;
        }
    }
}

/// The frames from `seq` (at byte offset `from`) to the end of the log, and
/// where the cursor stands after them.
fn collect(st: &State, seq: u64, from: u64, exit: Option<(i32, bool)>) -> (Vec<Msg>, (u64, u64)) {
    let mut frames = Vec::new();
    let mut at = from;
    for (s, e) in &st.log.events {
        if *s < seq {
            continue;
        }
        match e {
            Event::Out(o, b) => {
                let end = o + b.len() as u64;
                if end <= at {
                    continue;
                }
                let skip = usize::try_from(at.saturating_sub(*o)).unwrap_or(0);
                // Adjacent output goes out as one frame: a PTY hands over
                // small chunks, and a frame each costs the reader syscalls.
                if let Some(Msg::Output { offset, bytes }) = frames.last_mut()
                    && *offset + bytes.len() as u64 == at.max(*o)
                    && bytes.len() < 1 << 20
                {
                    bytes.extend_from_slice(&b[skip..]);
                } else {
                    frames.push(Msg::Output {
                        offset: at.max(*o),
                        bytes: b[skip..].to_vec(),
                    });
                }
                at = end;
            }
            Event::Resized(o, rows, cols) => {
                frames.push(Msg::Resized {
                    offset: *o,
                    rows: *rows,
                    cols: *cols,
                });
                at = at.max(e.offset());
            }
        }
    }
    if let Some((code, signaled)) = exit {
        frames.push(Msg::Exited { code, signaled });
    }
    (frames, (st.log.next_seq, at))
}

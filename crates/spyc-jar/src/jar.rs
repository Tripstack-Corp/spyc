//! The jar: one program on a PTY, an event log of what it printed and how big
//! its terminal was, and at most one owner.
//!
//! Threads, each blocking on one thing:
//! - the PTY reader appends output to the log, and pauses while a live owner
//!   is a window behind (R11);
//! - the child waiter records the exit;
//! - the acceptor takes new connections;
//! - one reader and one writer per connection;
//! - the reaper ends the jar once its program has ended and the owner
//!   acknowledged it, or nobody came back within the linger time.
//!
//! The log never parses a byte. A client starts each connection with a
//! `Replay { from }` and gets every event at or after that offset, after a
//! `Gap` when `from` has already been dropped.
//!
//! Stage 3 (R33): every thread that holds bytes in flight parks at a safe point
//! when an upgrade starts (`upgrade.rs`), so the whole jar can cross `exec` as
//! a [`Handover`](crate::handover::Handover) and its descriptors.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use rustix::process::{Pid, WaitOptions};

use crate::proto::{self, JarSignal, Msg, Role, Snapshot};

#[derive(Clone)]
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
    /// Stage 2b: send the program `SIGUSR1` when an owner connects, so a spyc
    /// running in this jar knows a new terminal attached.
    pub attach_signal: bool,
    /// R32a: how long the program runs on with no owner before the jar ends
    /// it, as a closed terminal would; `None` waits for an owner however long.
    pub linger: Option<Duration>,
    /// R31: this jar's instance, random at its start and kept across
    /// upgrades, and when it started, in seconds since the epoch.
    pub instance: u64,
    pub created: u64,
}

/// How long a jar with a finite linger waits for its first owner: the spyc
/// that spawned it connects within milliseconds.
const FIRST_OWNER_GRACE: Duration = Duration::from_secs(10);

pub(crate) enum Event {
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
pub(crate) struct Log {
    pub(crate) events: VecDeque<(u64, Event)>,
    pub(crate) next_seq: u64,
    /// Offset of the next byte the program prints.
    pub(crate) head: u64,
    /// Offset of the oldest byte still held; earlier ones are gone.
    pub(crate) base: u64,
    pub(crate) held: usize,
    pub(crate) cap: usize,
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

pub(crate) struct State {
    pub(crate) log: Log,
    pub(crate) owner: Option<u64>,
    pub(crate) acked: u64,
    /// The latest snapshot of each format (R17a): the binary one for the same
    /// build, the portable one for an upgrade across a pin bump.
    pub(crate) snapshots: Vec<Snapshot>,
    pub(crate) exit: Option<(i32, bool)>,
    pub(crate) exit_acked: bool,
    pub(crate) pty_eof: bool,
    pub(crate) rows: u16,
    pub(crate) cols: u16,
    pub(crate) next_conn: u64,
    pub(crate) ownerless_since: Option<Instant>,
    /// The PTY's foreground process group, 0 until read.
    pub(crate) fg: u32,
    /// Whether any owner has connected yet.
    pub(crate) had_owner: bool,
    /// R22: the token a released jar is held for, and until when.
    pub(crate) reserved: Option<(Vec<u8>, Instant)>,
    /// R18: the owner's key/value store.
    pub(crate) info: Vec<(String, String)>,
}

/// How long a released jar waits for its named successor before any owner
/// may take it.
const SUCCESSOR_WAIT: Duration = Duration::from_secs(60);

/// What a connection is, apart from its socket: enough to resume it.
pub(crate) struct ConnState {
    pub(crate) id: u64,
    pub(crate) role: Role,
    pub(crate) cursor: Option<(u64, u64)>,
    pub(crate) exit_sent: bool,
    pub(crate) fg_sent: u32,
}

/// A live connection, shared by its reader and writer threads.
pub(crate) struct Conn {
    pub(crate) role: Role,
    pub(crate) out: Arc<Mutex<UnixStream>>,
    /// Where the writer is in the log; `None` until the first `Replay`.
    pub(crate) cursor: Mutex<Option<(u64, u64)>>,
    pub(crate) exit_sent: AtomicBool,
    pub(crate) fg_sent: AtomicU32,
    pub(crate) closed: AtomicBool,
    pub(crate) writer_done: AtomicBool,
}

/// A thread that may hold bytes in flight, and so parks for an upgrade.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Who {
    Pty,
    Waiter,
    Acceptor,
    ConnRead(u64),
    ConnWrite(u64),
}

pub(crate) struct Shared {
    pub(crate) state: Mutex<State>,
    pub(crate) cv: Condvar,
    pub(crate) master: OwnedFd,
    pub(crate) input: Mutex<File>,
    pub(crate) listener: UnixListener,
    pub(crate) child_pid: u32,
    pub(crate) cfg: Config,
    pub(crate) conns: Mutex<HashMap<u64, Arc<Conn>>>,
    pub(crate) upgrading: AtomicBool,
    pub(crate) parked: Mutex<HashSet<Who>>,
    pub(crate) park_cv: Condvar,
    pub(crate) pty_done: AtomicBool,
    pub(crate) waiter_done: AtomicBool,
}

impl Shared {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn conns(&self) -> MutexGuard<'_, HashMap<u64, Arc<Conn>>> {
        self.conns.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wait out an upgrade as `who`: counted as parked until it ends, which
    /// it does only by failing, since one that succeeds never returns.
    pub(crate) fn park(&self, who: Who) {
        let mut p = self.parked.lock().unwrap_or_else(PoisonError::into_inner);
        p.insert(who);
        self.park_cv.notify_all();
        while self.upgrading.load(Ordering::SeqCst) {
            p = self.park_cv.wait(p).unwrap_or_else(PoisonError::into_inner);
        }
        p.remove(&who);
    }

    fn upgrading(&self) -> bool {
        self.upgrading.load(Ordering::SeqCst)
    }
}

/// Whether `fd` has something to read within `wait`.
fn readable(fd: impl AsFd, wait: Duration) -> bool {
    let mut fds = [rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN)];
    let ts = rustix::event::Timespec {
        tv_sec: 0,
        tv_nsec: i64::try_from(wait.as_nanos()).unwrap_or(0),
    };
    matches!(rustix::event::poll(&mut fds, Some(&ts)), Ok(n) if n > 0)
}

const TICK: Duration = Duration::from_millis(100);

/// Spawn the program and serve the socket until the jar ends. Never returns
/// on success: the reaper ends the process.
pub fn run(cfg: Config, listener: UnixListener) -> anyhow::Result<()> {
    let (master, child_pid) = crate::pty::spawn(&cfg)?;
    let state = State {
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
        had_owner: false,
        reserved: None,
        info: Vec::new(),
    };
    start(cfg, state, child_pid, master, listener, Vec::new())
}

/// Run a jar from `state`: a fresh one, or one taking over across `exec` with
/// its connections in `conns`.
pub(crate) fn start(
    cfg: Config,
    state: State,
    child_pid: u32,
    master: OwnedFd,
    listener: UnixListener,
    conns: Vec<(ConnState, UnixStream)>,
) -> anyhow::Result<()> {
    let reader = File::from(master.try_clone().context("PTY reader")?);
    let input = File::from(master.try_clone().context("PTY writer")?);
    let exited = state.exit.is_some();
    let pty_eof = state.pty_eof;
    let shared = Arc::new(Shared {
        state: Mutex::new(state),
        cv: Condvar::new(),
        master,
        input: Mutex::new(input),
        listener,
        child_pid,
        cfg,
        conns: Mutex::new(HashMap::new()),
        upgrading: AtomicBool::new(false),
        parked: Mutex::new(HashSet::new()),
        park_cv: Condvar::new(),
        pty_done: AtomicBool::new(pty_eof),
        waiter_done: AtomicBool::new(exited),
    });
    for (cs, stream) in conns {
        let conn = register(&shared, cs.id, cs.role, stream)?;
        *conn.cursor.lock().unwrap_or_else(PoisonError::into_inner) = cs.cursor;
        conn.exit_sent.store(cs.exit_sent, Ordering::SeqCst);
        conn.fg_sent.store(cs.fg_sent, Ordering::SeqCst);
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let build = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let _ = send_on(&conn.out, &Msg::Upgraded { build });
            run_conn(&shared, cs.id, &conn);
        });
    }
    if !pty_eof {
        let shared = Arc::clone(&shared);
        thread::spawn(move || pump_pty(&shared, reader));
    }
    if !exited {
        let shared = Arc::clone(&shared);
        thread::spawn(move || wait_child(&shared));
    }
    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || reap(&shared));
    }
    shared.listener.set_nonblocking(true)?;
    loop {
        if shared.upgrading() {
            shared.park(Who::Acceptor);
            continue;
        }
        if !readable(&shared.listener, TICK) {
            continue;
        }
        let Ok((stream, _)) = shared.listener.accept() else {
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let shared = Arc::clone(&shared);
        thread::spawn(move || serve(&shared, stream));
    }
}

/// Copy the program's output into the log, pausing while a live owner is a
/// window behind.
fn pump_pty(shared: &Shared, mut reader: File) {
    // The window is checked before each read, so a live owner can be a
    // window plus one read behind: keep reads small.
    let mut buf = vec![0u8; 8192];
    loop {
        if shared.upgrading() {
            shared.park(Who::Pty);
            continue;
        }
        {
            let st = shared.lock();
            if st.owner.is_some() && st.log.head - st.acked >= shared.cfg.window {
                let _ = shared.cv.wait_timeout(st, TICK);
                continue;
            }
        }
        if !readable(&reader, TICK) {
            continue;
        }
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut st = shared.lock();
        let at = st.log.head;
        st.log.push(Event::Out(at, buf[..n].to_vec()));
        if let Some(keep) = st.snapshots.iter().map(|s| s.offset).min() {
            st.log.trim(keep);
        }
        shared.cv.notify_all();
    }
    shared.lock().pty_eof = true;
    shared.pty_done.store(true, Ordering::SeqCst);
    shared.cv.notify_all();
}

/// Record the program's exit, polled so the waiter can park for an upgrade.
fn wait_child(shared: &Shared) {
    let pid = i32::try_from(shared.child_pid).ok().and_then(Pid::from_raw);
    loop {
        if shared.upgrading() {
            shared.park(Who::Waiter);
            continue;
        }
        let exit = match rustix::process::waitpid(pid, WaitOptions::NOHANG) {
            Ok(None) => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Ok(Some((_, status))) => match (status.exit_status(), status.terminating_signal()) {
                (Some(code), _) => (code, false),
                (None, Some(_)) => (-1, true),
                _ => (-1, false),
            },
            Err(_) => (-1, false),
        };
        let mut st = shared.lock();
        st.exit = Some(exit);
        shared.waiter_done.store(true, Ordering::SeqCst);
        shared.cv.notify_all();
        return;
    }
}

/// End the jar once its program has ended and the exit was collected, or
/// nobody came back for it within the linger time.
fn reap(shared: &Shared) {
    let mut st = shared.lock();
    loop {
        // R4: the foreground process group, read here because only the
        // master's holder can. A job starting needn't print anything.
        let fg = rustix::termios::tcgetpgrp(&shared.master)
            .ok()
            .and_then(|p| u32::try_from(p.as_raw_nonzero().get()).ok())
            .unwrap_or(0);
        if fg != st.fg {
            st.fg = fg;
            shared.cv.notify_all();
        }
        if st.exit.is_none() && !shared.upgrading() && lingered(shared, &st) {
            drop(st);
            end_program(shared);
            st = shared.lock();
            continue;
        }
        if st.exit.is_some() && !shared.upgrading() {
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

/// Whether the program has been without an owner for longer than its linger
/// policy allows (R32a).
fn lingered(shared: &Shared, st: &State) -> bool {
    let (Some(linger), Some(since)) = (shared.cfg.linger, st.ownerless_since) else {
        return false;
    };
    let allowed = if st.had_owner {
        linger
    } else {
        linger.max(FIRST_OWNER_GRACE)
    };
    since.elapsed() >= allowed
}

/// End the program as closing its terminal would: `SIGHUP` to its process
/// group, then `SIGKILL` if it hasn't gone within two seconds. The reaper ends
/// the jar once the exit is recorded, no owner being there to collect it.
fn end_program(shared: &Shared) {
    use rustix::process::Signal;
    let Some(pid) = i32::try_from(shared.child_pid).ok().and_then(Pid::from_raw) else {
        return;
    };
    let _ = rustix::process::kill_process_group(pid, Signal::HUP);
    let until = Instant::now() + Duration::from_secs(2);
    while shared.lock().exit.is_none() && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    if shared.lock().exit.is_none() {
        let _ = rustix::process::kill_process_group(pid, Signal::KILL);
    }
    // No owner will come for the exit: don't linger for one.
    shared.lock().exit_acked = true;
}

/// R31: what this jar is, for `spyc --jars` and a spyc looking for its jars.
fn describe(shared: &Shared) -> Vec<(String, String)> {
    let st = shared.lock();
    let id = shared
        .cfg
        .socket
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut f = vec![
        ("id", id),
        ("instance", format!("{:016x}", shared.cfg.instance)),
        ("created", shared.cfg.created.to_string()),
        ("jar", format!("{} {}", env!("CARGO_PKG_VERSION"), std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default())),
        ("pid", std::process::id().to_string()),
        ("program", shared.cfg.argv.join(" ")),
        ("program_pid", shared.child_pid.to_string()),
        ("cwd", shared.cfg.cwd.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
        ("size", format!("{}x{}", st.rows, st.cols)),
        ("overflow", "drop".into()),
        ("linger", shared.cfg.linger.map_or("until an owner returns".into(), |d| format!("{} ms", d.as_millis()))),
        ("owner", if st.owner.is_some() { "connected".into() } else { st.ownerless_since.map_or("none".into(), |t| format!("none for {} s", t.elapsed().as_secs())) }),
        ("log", format!("{} of {} bytes held, {} printed, from {}", st.log.held, st.log.cap, st.log.head, st.log.base)),
        ("snapshots", st.snapshots.iter().map(|s| format!("{}@{}", s.format, s.offset)).collect::<Vec<_>>().join(" ")),
    ];
    if let Some((code, signaled)) = st.exit {
        f.push(("exit", if signaled { "signal".into() } else { code.to_string() }));
    }
    if st.reserved.as_ref().is_some_and(|(_, until)| Instant::now() < *until) {
        f.push(("held", "for the spyc it was handed to".into()));
    }
    let mut f: Vec<(String, String)> = f.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    f.extend(st.info.iter().cloned());
    f
}

fn send_on(out: &Mutex<UnixStream>, msg: &Msg) -> std::io::Result<()> {
    let mut s = out.lock().unwrap_or_else(PoisonError::into_inner);
    proto::write_msg(&mut *s, msg)
}

fn register(shared: &Shared, id: u64, role: Role, stream: UnixStream) -> anyhow::Result<Arc<Conn>> {
    let conn = Arc::new(Conn {
        role,
        out: Arc::new(Mutex::new(stream)),
        cursor: Mutex::new(None),
        exit_sent: AtomicBool::new(false),
        fg_sent: AtomicU32::new(0),
        closed: AtomicBool::new(false),
        writer_done: AtomicBool::new(false),
    });
    shared.conns().insert(id, Arc::clone(&conn));
    Ok(conn)
}

/// One new connection: its `hello`, then [`run_conn`].
fn serve(shared: &Arc<Shared>, stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut read_half = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let refuse = |why: String| {
        let mut s = &stream;
        let _ = proto::write_msg(&mut s, &Msg::Refused { why });
    };
    let Ok(Some(Msg::Hello {
        version,
        role,
        claim,
    })) = proto::read_msg(&mut read_half)
    else {
        refuse("expected a hello".into());
        return;
    };
    if version != proto::VERSION {
        refuse(format!("jar speaks version {}", proto::VERSION));
        return;
    }
    let mut st = shared.lock();
    if role == Role::Owner && st.owner.is_some() {
        drop(st);
        refuse("the jar already has an owner".into());
        return;
    }
    if role == Role::Owner
        && let Some((token, until)) = st.reserved.take()
        && Instant::now() < until
        && token != claim
    {
        st.reserved = Some((token, until));
        drop(st);
        refuse("the jar is held for the spyc it was handed to".into());
        return;
    }
    let id = st.next_conn;
    st.next_conn += 1;
    if role == Role::Owner {
        if shared.cfg.attach_signal
            && let Some(pid) = i32::try_from(shared.child_pid).ok().and_then(Pid::from_raw)
        {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::USR1);
        }
        st.owner = Some(id);
        st.had_owner = true;
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
    let Ok(conn) = register(shared, id, role, stream) else {
        return;
    };
    drop(st);
    if send_on(&conn.out, &welcome).is_err() {
        release(shared, id);
        return;
    }
    run_conn(shared, id, &conn);
}

/// A connection after its `hello`: a writer thread streaming the log, and
/// this thread reading what the client sends.
fn run_conn(shared: &Arc<Shared>, id: u64, conn: &Arc<Conn>) {
    let mut read_half = match conn
        .out
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .try_clone()
    {
        Ok(s) => s,
        Err(_) => {
            release(shared, id);
            return;
        }
    };
    // A frame that has begun arrives whole within this; silence between
    // frames is timed by `last_heard` instead.
    let _ = read_half.set_read_timeout(Some(Duration::from_secs(5)));
    let owner = conn.role == Role::Owner;
    let writer = {
        let shared = Arc::clone(shared);
        let conn = Arc::clone(conn);
        thread::spawn(move || {
            stream_log(&shared, id, &conn);
            conn.writer_done.store(true, Ordering::SeqCst);
            shared.park_cv.notify_all();
        })
    };
    let send = |msg: &Msg| send_on(&conn.out, msg);
    let mut last_heard = Instant::now();
    loop {
        if shared.upgrading() {
            shared.park(Who::ConnRead(id));
            last_heard = Instant::now();
            continue;
        }
        if !readable(&read_half, TICK) {
            if owner && last_heard.elapsed() >= shared.cfg.deadline {
                break;
            }
            continue;
        }
        let msg = match proto::read_msg(&mut read_half) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(proto::ReadError::UnknownCritical(kind)) => {
                let _ = send(&Msg::Unsupported { kind });
                continue;
            }
            Err(_) => break,
        };
        last_heard = Instant::now();
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
                *conn.cursor.lock().unwrap_or_else(PoisonError::into_inner) = Some((seq, from));
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
            Msg::Upgrade { binary } => crate::upgrade::begin(shared, Arc::clone(&conn.out), binary),
            Msg::Release { token } if owner => {
                let mut st = shared.lock();
                st.reserved = Some((token, Instant::now() + SUCCESSOR_WAIT));
                drop(st);
                // Nothing this connection sends after its release may act on
                // the jar: whatever follows, a `Kill` from its tab closing
                // among them, is never read.
                break;
            }
            Msg::SetInfo { key, value } if owner => {
                let mut st = shared.lock();
                st.info.retain(|(k, _)| *k != key);
                st.info.push((key, value));
            }
            Msg::Describe => {
                let _ = send(&Msg::Description {
                    fields: describe(shared),
                });
            }
            _ if !owner => {
                let _ = send(&Msg::Refused {
                    why: "a reader can only watch".into(),
                });
            }
            Msg::Input { bytes } | Msg::Reply { bytes } => {
                if let Some(path) = std::env::var_os("SPYC_JAR_TRACE") {
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
                let size = rustix::termios::Winsize {
                    ws_row: rows,
                    ws_col: cols,
                    ws_xpixel: px_w,
                    ws_ypixel: px_h,
                };
                if rustix::termios::tcsetwinsize(&shared.master, size).is_ok() {
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
    conn.closed.store(true, Ordering::SeqCst);
    if let Ok(s) = conn.out.lock() {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    release(shared, id);
    shared.cv.notify_all();
    let _ = writer.join();
}

/// A connection ended: it leaves the registry, and if it owned the jar, the
/// jar has no owner now.
fn release(shared: &Shared, id: u64) {
    shared.conns().remove(&id);
    let mut st = shared.lock();
    if st.owner == Some(id) {
        st.owner = None;
        st.ownerless_since = Some(Instant::now());
        st.info.retain(|(k, _)| k != "owner_mcp");
    }
    shared.cv.notify_all();
    shared.park_cv.notify_all();
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
    if let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) {
        let _ = rustix::process::kill_process_group(pid, sig);
    }
}

/// Stream the log to one connection from its cursor, then the exit once
/// everything before it has gone out.
fn stream_log(shared: &Shared, id: u64, conn: &Conn) {
    loop {
        let mut st = shared.lock();
        let batch = loop {
            if conn.closed.load(Ordering::SeqCst) {
                return;
            }
            if shared.upgrading() {
                drop(st);
                shared.park(Who::ConnWrite(id));
                st = shared.lock();
                continue;
            }
            let pos = *conn.cursor.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((seq, from)) = pos {
                let pending = seq < st.log.next_seq;
                let exit_due = !conn.exit_sent.load(Ordering::SeqCst)
                    && st.exit.is_some()
                    && st.pty_eof
                    && !pending;
                let fg_due = st.fg != conn.fg_sent.load(Ordering::SeqCst);
                if pending || exit_due || fg_due {
                    let (mut frames, next) =
                        collect(&st, seq, from, exit_due.then(|| st.exit).flatten());
                    if fg_due {
                        frames.insert(0, Msg::Foreground { pgid: st.fg });
                        conn.fg_sent.store(st.fg, Ordering::SeqCst);
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
        let (frames, next) = batch;
        // One write for the whole batch, under the connection's lock and
        // before the cursor moves, so an upgrade never sees a cursor past
        // what went out.
        let mut bytes = Vec::new();
        for f in &frames {
            bytes.extend(proto::encode(f));
        }
        let mut s = conn.out.lock().unwrap_or_else(PoisonError::into_inner);
        drop(st);
        if s.write_all(&bytes).and_then(|()| s.flush()).is_err() {
            return;
        }
        if frames.iter().any(|f| matches!(f, Msg::Exited { .. })) {
            conn.exit_sent.store(true, Ordering::SeqCst);
        }
        let mut c = conn.cursor.lock().unwrap_or_else(PoisonError::into_inner);
        if c.is_some_and(|(seq, _)| seq <= next.0) {
            *c = Some(next);
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

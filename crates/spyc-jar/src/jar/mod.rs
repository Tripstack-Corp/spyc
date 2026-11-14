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
//! Every thread that holds bytes in flight parks at a safe point when an
//! upgrade starts (`upgrade.rs`), so the whole jar can cross `exec` as a
//! [`Handover`](crate::handover::Handover) and its descriptors.
//!
//! - [`life`]: the program's life: its output, its exit, its linger.
//! - [`accept`]: a new connection and its `hello`.
//! - [`conn`]: a connection's loop, and the writer streaming it the log.
//! - [`describe`]: what the jar says about itself (R31).

mod accept;
mod conn;
mod describe;
mod life;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;

use crate::proto::{self, Msg, Role};
use crate::state::{Config, ConnState, Log, State};

pub(crate) use accept::no_sigpipe;
pub use conn::MIN_TOKEN;

/// A live connection, shared by its reader and writer threads.
pub(crate) struct Conn {
    pub(crate) role: Role,
    /// The connecting process, 0 when the platform can't say.
    pub(crate) peer_pid: u32,
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
    /// Replaced when its path goes missing ([`keep_socket`]).
    pub(crate) listener: Mutex<UnixListener>,
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

    pub(super) fn upgrading(&self) -> bool {
        self.upgrading.load(Ordering::SeqCst)
    }
}

/// Whether `fd` has something to read within `wait`.
pub(super) fn readable(fd: impl AsFd, wait: Duration) -> bool {
    let mut fds = [rustix::event::PollFd::new(
        &fd,
        rustix::event::PollFlags::IN,
    )];
    let ts = rustix::event::Timespec {
        tv_sec: 0,
        tv_nsec: i64::try_from(wait.as_nanos()).unwrap_or(0),
    };
    matches!(rustix::event::poll(&mut fds, Some(&ts)), Ok(n) if n > 0)
}

pub(super) const TICK: Duration = Duration::from_millis(100);

/// Spawn the program and serve the socket until the jar ends. Never returns
/// on success: the reaper ends the process.
pub fn run(cfg: Config, listener: UnixListener, token: Vec<u8>) -> anyhow::Result<()> {
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
        token,
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
        listener: Mutex::new(listener),
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
        let conn = accept::register(&shared, cs.id, cs.role, stream);
        *conn.cursor.lock().unwrap_or_else(PoisonError::into_inner) = cs.cursor;
        conn.exit_sent.store(cs.exit_sent, Ordering::SeqCst);
        conn.fg_sent.store(cs.fg_sent, Ordering::SeqCst);
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let build =
                std::env::current_exe().map_or_else(|_| String::new(), |p| p.display().to_string());
            let _ = send_on(&conn.out, &Msg::Upgraded { build });
            conn::run_conn(&shared, cs.id, &conn);
        });
    }
    if !pty_eof {
        let shared = Arc::clone(&shared);
        thread::spawn(move || life::pump_pty(&shared, reader));
    }
    if !exited {
        let shared = Arc::clone(&shared);
        thread::spawn(move || life::wait_child(&shared));
    }
    {
        let shared = Arc::clone(&shared);
        thread::spawn(move || life::reap(&shared));
    }
    shared
        .listener
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .set_nonblocking(true)?;
    let mut checked = Instant::now();
    loop {
        if shared.upgrading() {
            shared.park(Who::Acceptor);
            continue;
        }
        if checked.elapsed() >= SOCKET_CHECK {
            checked = Instant::now();
            keep_socket(&shared);
        }
        let listener = shared
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !readable(&*listener, TICK) {
            continue;
        }
        let accepted = listener.accept();
        drop(listener);
        let Ok((stream, _)) = accepted else {
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let shared = Arc::clone(&shared);
        thread::spawn(move || accept::serve(&shared, stream));
    }
}

/// How often the jar checks that its socket's path is still there.
const SOCKET_CHECK: Duration = Duration::from_secs(2);

/// The socket's path is the jar's one address, and a cleaner may delete it:
/// macOS removes what's left untouched in `$TMPDIR` for days, and a
/// listening socket's file is never touched. A jar whose path has gone binds
/// a new socket there; one whose path holds something else leaves it be.
fn keep_socket(shared: &Shared) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let path = &shared.cfg.socket;
    if path.symlink_metadata().is_ok() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir);
    }
    let Ok(fresh) = UnixListener::bind(path) else {
        return;
    };
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    if fresh.set_nonblocking(true).is_ok() {
        *shared
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = fresh;
    }
}
pub(super) fn send_on(out: &Mutex<UnixStream>, msg: &Msg) -> std::io::Result<()> {
    let mut s = out.lock().unwrap_or_else(PoisonError::into_inner);
    proto::write_msg(&mut *s, msg)
}

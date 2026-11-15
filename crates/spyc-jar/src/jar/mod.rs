//! The jar: one program on a PTY, an event log of what it printed and how big
//! its terminal was, and at most one owner.
//!
//! Threads, each blocking on one thing:
//! - the PTY reader appends output to the log, and pauses while a live owner
//!   is a window behind (R11);
//! - the child waiter records the exit;
//! - the acceptor takes new connections;
//! - one reader and one writer per connection, and a lookup's one thread;
//! - the input writer feeds the PTY what the owner sends;
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
//! No thread blocks on a peer while it holds a lock, and none blocks for good
//! on one that has stopped reading (review finding 4): only a connection's
//! writer writes to its socket, with a timeout; anything else it is to send
//! waits in its [`outbox`]; a socket is shut down through the connection's
//! one handle, with no lock; and the program's input has a thread of its own
//! ([`input`]). A connection without the token can't hold the jar (finding
//! 5): a frame arrives whole by a deadline, few such connections are open at
//! once, and none waits for or crosses an upgrade.
//!
//! - [`life`]: the program's life: its output, its exit, its linger.
//! - [`accept`]: a new connection and its `hello`.
//! - [`conn`]: a connection's loop, and the writer streaming it the log.
//! - [`outbox`]: what a connection's writer sends ahead of the log.
//! - [`input`]: the program's input, written off every connection's thread.
//! - [`describe`]: what the jar says about itself (R31).

mod accept;
mod conn;
mod describe;
mod input;
mod life;
mod outbox;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;

use crate::proto::{Msg, Role};
use crate::state::{Config, ConnState, Log, State};

pub(crate) use accept::{no_sigpipe, peer_is};
pub use conn::MIN_TOKEN;
pub(crate) use input::Inbox;
pub(crate) use outbox::Outbox;

/// A live connection, shared by its reader and writer threads.
pub(crate) struct Conn {
    pub(crate) role: Role,
    /// The connecting process, 0 when the platform can't say.
    pub(crate) peer_pid: u32,
    /// The connection's one descriptor: its reader reads it, its writer (a
    /// lookup's one thread) writes it, and anyone may shut it down.
    pub(crate) sock: Arc<UnixStream>,
    pub(crate) outbox: Mutex<Outbox>,
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
    Input,
    ConnRead(u64),
    ConnWrite(u64),
}

pub(crate) struct Shared {
    pub(crate) state: Mutex<State>,
    pub(crate) cv: Condvar,
    pub(crate) master: OwnedFd,
    /// What the owner sent the program that the PTY hasn't taken yet.
    pub(crate) inbox: Mutex<Inbox>,
    pub(crate) inbox_cv: Condvar,
    /// Replaced when its path goes missing ([`keep_socket`]).
    pub(crate) listener: Mutex<UnixListener>,
    /// The program's pid; 0 while a jar started on its owner waits for one.
    pub(crate) child_pid: std::sync::atomic::AtomicU32,
    /// The PTY's slave, held for a program not yet started.
    pub(crate) pending: Mutex<Option<File>>,
    pub(crate) cfg: Config,
    pub(crate) conns: Mutex<HashMap<u64, Arc<Conn>>>,
    /// Connections that have shown no token: before their `hello`, and
    /// lookups. The oldest goes when there are too many ([`accept::admit`]).
    pub(crate) tokenless: Mutex<VecDeque<(u64, Arc<UnixStream>)>>,
    pub(crate) next_ticket: AtomicU64,
    /// An upgrade is under way: a second is refused.
    pub(crate) upgrade_claimed: AtomicBool,
    /// Every thread is to park for the handover. Set only once the new
    /// binary has passed its first validation.
    pub(crate) upgrading: AtomicBool,
    pub(crate) parked: Mutex<HashSet<Who>>,
    pub(crate) park_cv: Condvar,
    pub(crate) pty_done: AtomicBool,
    pub(crate) waiter_done: AtomicBool,
}

impl Shared {
    /// The program's pid; 0 until a jar started on its owner has one.
    pub(crate) fn child_pid(&self) -> u32 {
        self.child_pid.load(Ordering::SeqCst)
    }

    /// Start the program held for the first owner, under the state's lock.
    /// Nothing to do once it has started.
    pub(crate) fn start_program(&self) -> anyhow::Result<()> {
        let Some(slave) = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return Ok(());
        };
        let pid = crate::pty::start(&self.cfg, slave)?;
        self.child_pid.store(pid, Ordering::SeqCst);
        self.cv.notify_all();
        Ok(())
    }

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
    ready(fd, rustix::event::PollFlags::IN, wait)
}

/// Whether `fd` is ready for `flags` within `wait`.
pub(crate) fn ready(fd: impl AsFd, flags: rustix::event::PollFlags, wait: Duration) -> bool {
    let mut fds = [rustix::event::PollFd::new(&fd, flags)];
    let ts = rustix::event::Timespec {
        tv_sec: i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(wait.subsec_nanos()),
    };
    matches!(rustix::event::poll(&mut fds, Some(&ts)), Ok(n) if n > 0)
}

pub(super) const TICK: Duration = Duration::from_millis(100);

/// The most the PTY reader takes at once. A live owner can be a window plus
/// one read behind, so the window must leave this much of the ring free.
pub(crate) const PTY_READ: usize = 8192;

/// How long a frame that has begun may take to arrive whole.
pub(super) const FRAME_DEADLINE: Duration = Duration::from_secs(5);

/// Reads from a socket that give up at `until`: a frame arrives whole by its
/// deadline however its bytes trickle, since a peer that sends a byte at a
/// time would otherwise keep its reader from ever reaching a safe point.
pub(super) struct By<'a> {
    pub(super) sock: &'a UnixStream,
    pub(super) until: Instant,
}

impl<'a> By<'a> {
    pub(super) fn frame(sock: &'a UnixStream) -> Self {
        Self {
            sock,
            until: Instant::now() + FRAME_DEADLINE,
        }
    }
}

impl Read for By<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() || !readable(self.sock, left) {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        let mut sock = self.sock;
        sock.read(buf)
    }
}

/// Spawn the program and serve the socket until the jar ends. Never returns
/// on success: the reaper ends the process.
pub fn run(cfg: Config, listener: UnixListener, token: Vec<u8>) -> anyhow::Result<()> {
    let (master, child_pid, pending) = if cfg.start_on_owner.is_some() {
        let (master, slave) = crate::pty::open(&cfg)?;
        (master, 0, Some(slave))
    } else {
        let (master, pid) = crate::pty::spawn(&cfg)?;
        (master, pid, None)
    };
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
    start(
        cfg,
        state,
        (child_pid, pending),
        master,
        listener,
        Vec::new(),
    )
}

/// Run a jar from `state`: a fresh one, or one taking over across `exec` with
/// its connections in `conns`.
pub(crate) fn start(
    cfg: Config,
    state: State,
    child: (u32, Option<File>),
    master: OwnedFd,
    listener: UnixListener,
    conns: Vec<(ConnState, UnixStream)>,
) -> anyhow::Result<()> {
    prepare(cfg, state, child, master, listener, conns)?.serve()
}

/// A jar set up and not yet running: no thread started, so a new image that
/// fails here still holds everything that crossed (`held.rs`).
pub(crate) struct Ready {
    shared: Arc<Shared>,
    reader: File,
    input: File,
    conns: Vec<(u64, Arc<Conn>)>,
}

/// Set a jar up from `state`: everything [`start`] does that can fail, and
/// nothing that starts a thread.
pub(crate) fn prepare(
    cfg: Config,
    state: State,
    (child_pid, pending): (u32, Option<File>),
    master: OwnedFd,
    listener: UnixListener,
    conns: Vec<(ConnState, UnixStream)>,
) -> anyhow::Result<Ready> {
    // Neither the PTY's reader nor its writer may block on it: each polls,
    // and a write the program has no room for waits its turn.
    let flags = rustix::fs::fcntl_getfl(&master).context("the PTY's flags")?;
    rustix::fs::fcntl_setfl(&master, flags | rustix::fs::OFlags::NONBLOCK)
        .context("the PTY, non-blocking")?;
    let reader = File::from(master.try_clone().context("PTY reader")?);
    let input = File::from(master.try_clone().context("PTY writer")?);
    listener.set_nonblocking(true)?;
    let exited = state.exit.is_some();
    let pty_eof = state.pty_eof;
    let shared = Arc::new(Shared {
        state: Mutex::new(state),
        cv: Condvar::new(),
        master,
        inbox: Mutex::new(Inbox::default()),
        inbox_cv: Condvar::new(),
        listener: Mutex::new(listener),
        child_pid: std::sync::atomic::AtomicU32::new(child_pid),
        pending: Mutex::new(pending),
        cfg,
        conns: Mutex::new(HashMap::new()),
        tokenless: Mutex::new(VecDeque::new()),
        next_ticket: AtomicU64::new(1),
        upgrade_claimed: AtomicBool::new(false),
        upgrading: AtomicBool::new(false),
        parked: Mutex::new(HashSet::new()),
        park_cv: Condvar::new(),
        pty_done: AtomicBool::new(pty_eof),
        waiter_done: AtomicBool::new(exited),
    });
    let conns = conns
        .into_iter()
        .map(|(cs, stream)| {
            let conn = accept::register(&shared, cs.id, cs.role, Arc::new(stream));
            *conn.cursor.lock().unwrap_or_else(PoisonError::into_inner) = cs.cursor;
            conn.exit_sent.store(cs.exit_sent, Ordering::SeqCst);
            conn.fg_sent.store(cs.fg_sent, Ordering::SeqCst);
            (cs.id, conn)
        })
        .collect();
    Ok(Ready {
        shared,
        reader,
        input,
        conns,
    })
}

impl Ready {
    /// Start the jar's threads and serve its socket until the jar ends.
    /// Never returns on success: the reaper ends the process.
    pub(crate) fn serve(self) -> anyhow::Result<()> {
        let Self {
            shared,
            reader,
            input,
            conns,
        } = self;
        for (id, conn) in conns {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                let build = crate::store::build_of(
                    &crate::upgrade::own_image(),
                    &crate::upgrade::copies_dir(&shared),
                );
                conn.send(&shared, &Msg::Upgraded { build });
                conn::run_conn(&shared, id, &conn);
            });
        }
        if !shared.pty_done.load(Ordering::SeqCst) {
            let shared = Arc::clone(&shared);
            thread::spawn(move || life::pump_pty(&shared, reader));
        }
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || input::feed_pty(&shared, input));
        }
        if !shared.waiter_done.load(Ordering::SeqCst) {
            let shared = Arc::clone(&shared);
            thread::spawn(move || life::wait_child(&shared));
        }
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || life::reap(&shared));
        }
        if let Some(wait) = shared.cfg.start_on_owner
            && shared.child_pid() == 0
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || life::await_first_owner(&shared, wait));
        }
        accept_loop(&shared)
    }
}

/// Take new connections until the jar ends.
fn accept_loop(shared: &Arc<Shared>) -> ! {
    let mut checked = Instant::now();
    loop {
        if shared.upgrading() {
            shared.park(Who::Acceptor);
            continue;
        }
        if checked.elapsed() >= SOCKET_CHECK {
            checked = Instant::now();
            keep_socket(shared);
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
            // Out of descriptors, the listener stays readable: wait a tick
            // rather than spin.
            thread::sleep(TICK);
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let sock = Arc::new(stream);
        let ticket = accept::admit(shared, &sock);
        let shared = Arc::clone(shared);
        thread::spawn(move || accept::serve(&shared, sock, ticket));
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

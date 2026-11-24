//! A new connection: who is at the other end (R26), its `hello`, and the
//! lease an owner takes with it.
//!
//! Until its `hello` shows the token, and for as long as it lasts if it is a
//! lookup, a connection is tokenless. At most [`MAX_TOKENLESS`] are open at
//! once, the oldest closed to admit another, so they can't take the threads
//! and descriptors that the jar's owner, its readers and an upgrade need
//! (review finding 5).

use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::conn::{LOOKUP_DEADLINE, run_conn};
use super::{By, Conn, Outbox, Shared, State};
use crate::proto::{self, Msg, Role, reason};

/// The most tokenless connections open at once.
const MAX_TOKENLESS: usize = 16;

pub(super) fn register(shared: &Shared, id: u64, role: Role, sock: Arc<UnixStream>) -> Arc<Conn> {
    no_sigpipe(&sock);
    if role == Role::Lookup {
        let _ = sock.set_write_timeout(Some(LOOKUP_DEADLINE));
    }
    let conn = Arc::new(Conn {
        role,
        peer_pid: peer_pid(&sock).unwrap_or(0),
        sock,
        outbox: Mutex::new(Outbox::default()),
        cursor: Mutex::new(None),
        exit_sent: AtomicBool::new(false),
        fg_sent: AtomicU32::new(0),
        closed: AtomicBool::new(false),
        // A lookup has no writer.
        writer_done: AtomicBool::new(role == Role::Lookup),
    });
    shared.conns().insert(id, Arc::clone(&conn));
    conn
}

/// Count a new connection as tokenless, closing the oldest past
/// [`MAX_TOKENLESS`]. Returns its ticket.
pub(super) fn admit(shared: &Shared, sock: &Arc<UnixStream>) -> u64 {
    let ticket = shared.next_ticket.fetch_add(1, Ordering::SeqCst);
    let mut open = shared
        .tokenless
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    open.push_back((ticket, Arc::clone(sock)));
    while open.len() > MAX_TOKENLESS {
        if let Some((_, oldest)) = open.pop_front() {
            let _ = oldest.shutdown(std::net::Shutdown::Both);
        }
    }
    ticket
}

/// A tokenless connection's place among them, given up when it is dropped:
/// once its `hello` shows the token, or once it ends.
struct Ticket<'a>(&'a Shared, u64);

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        let_go(self.0, self.1);
    }
}

/// Give up `ticket`'s place among the tokenless.
pub(super) fn let_go(shared: &Shared, ticket: u64) {
    shared
        .tokenless
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|(t, _)| *t != ticket);
}

/// R26: the user at the other end of `stream`.
#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    Ok(rustix::net::sockopt::socket_peercred(stream)?.uid.as_raw())
}

/// R26: the user at the other end of `stream`.
#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    Ok(nix::unistd::getpeereid(stream)?.0.as_raw())
}

/// R26: no way to tell who connected, so nobody may.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_uid(_stream: &UnixStream) -> std::io::Result<u32> {
    Err(std::io::Error::other(
        "no peer credentials on this platform",
    ))
}

/// R26: whether the peer on `stream` is `uid`. One that can't be told is not.
pub fn peer_is(stream: &UnixStream, uid: u32) -> bool {
    peer_uid(stream).is_ok_and(|p| p == uid)
}

/// The process at the other end of `stream`, as the kernel recorded it when
/// it connected.
#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> std::io::Result<u32> {
    let pid = rustix::net::sockopt::socket_peercred(stream)?.pid;
    Ok(pid.as_raw_nonzero().get().unsigned_abs())
}

/// The process at the other end of `stream`, as the kernel recorded it when
/// it connected.
#[cfg(target_os = "macos")]
fn peer_pid(stream: &UnixStream) -> std::io::Result<u32> {
    let pid = nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::LocalPeerPid)?;
    Ok(pid.unsigned_abs())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_pid(_stream: &UnixStream) -> std::io::Result<u32> {
    Err(std::io::Error::other(
        "no peer credentials on this platform",
    ))
}

/// Writing to a socket whose peer has gone answers `EPIPE` rather than raising
/// `SIGPIPE`, whatever the jar's disposition: a failed `exec` leaves it at
/// the default (review finding A1).
// SPYC-TRAP(jar-no-sigpipe): every socket the jar writes is made so here or
// by std, and every `sendmsg` carries [`NO_SIGNAL`]; one write that isn't
// ends the jar and its program after a failed upgrade.
#[cfg(target_os = "macos")]
pub fn no_sigpipe(stream: &UnixStream) {
    let _ = rustix::net::sockopt::set_socket_nosigpipe(stream, true);
}

/// Linux's std writes to a socket with `MSG_NOSIGNAL` already, and the jar's
/// own `sendmsg` calls carry [`NO_SIGNAL`].
#[cfg(not(target_os = "macos"))]
pub const fn no_sigpipe(_stream: &UnixStream) {}

/// What every `sendmsg` of the jar's carries: on Linux `MSG_NOSIGNAL`, which
/// `rustix` adds only when asked. macOS has no such flag; [`no_sigpipe`] on
/// the socket does it there.
#[cfg(target_os = "linux")]
pub const NO_SIGNAL: rustix::net::SendFlags = rustix::net::SendFlags::NOSIGNAL;

/// What every `sendmsg` of the jar's carries: nothing, [`no_sigpipe`] on the
/// socket does it.
#[cfg(not(target_os = "linux"))]
pub const NO_SIGNAL: rustix::net::SendFlags = rustix::net::SendFlags::empty();

/// A connected pair of sockets that never raise `SIGPIPE`: std makes no
/// socketpair so on macOS, as it does every other socket.
pub fn pair() -> std::io::Result<(UnixStream, UnixStream)> {
    let (a, b) = UnixStream::pair()?;
    no_sigpipe(&a);
    no_sigpipe(&b);
    Ok((a, b))
}

/// Queue for every connection of the program's own (role `program`) that
/// `msg` happened: an owner attached or went. The caller holds the state's
/// lock and wakes the writers once it lets go.
// SPYC-TRAP(jar-program-hears-each-owner-once): who is told is settled under
// the state's lock, as the owner changes; read after it, a program connecting
// beside an owner heard it twice, and an owner's going could follow the next's
// coming.
pub(super) fn queue_for_program(shared: &Shared, msg: &Msg) {
    let programs: Vec<Arc<Conn>> = shared
        .conns()
        .values()
        .filter(|c| c.role == Role::Program)
        .cloned()
        .collect();
    for c in programs {
        c.queue(msg);
    }
}

/// One new connection, holding `ticket` among the tokenless: its `hello`,
/// then [`run_conn`].
pub(super) fn serve(shared: &Arc<Shared>, sock: Arc<UnixStream>, ticket: u64) {
    serve_as(shared, sock, ticket, rustix::process::geteuid().as_raw());
}

/// [`serve`], for a jar whose user is `uid`.
fn serve_as(shared: &Arc<Shared>, sock: Arc<UnixStream>, ticket: u64, uid: u32) {
    let ticket = Ticket(shared, ticket);
    // Another user's process is refused before a byte of it is read, whatever
    // token it holds; the jar's directory being `0700` is the first fence.
    if !peer_is(&sock, uid) {
        return;
    }
    let _ = sock.set_write_timeout(Some(LOOKUP_DEADLINE));
    let refuse = |reason: u16, why: String| {
        let mut s = &*sock;
        let _ = proto::write_msg(&mut s, &Msg::Refused { reason, why });
    };
    let Ok(Some(Msg::Hello {
        version,
        role,
        token,
    })) = proto::read_msg_max(&mut By::frame(&sock), proto::MAX_HELLO)
    else {
        refuse(reason::INVALID, "expected a hello".into());
        return;
    };
    // A hello names the highest version its client speaks; the welcome
    // answers with the one this connection speaks.
    if version == 0 {
        refuse(
            reason::VERSION_NOT_SERVED,
            format!("jar speaks version {}", proto::VERSION),
        );
        return;
    }
    let mut st = shared.lock();
    let allowed = match role {
        // The program is known by its pid, which the kernel recorded as it
        // connected; it holds no token, since it runs as whatever the user
        // started in the jar.
        Role::Program => {
            let ours = st.exit.is_none()
                && peer_pid(&sock).is_ok_and(|p| p != 0 && p == shared.child_pid());
            if ours {
                Ok(())
            } else {
                Err((reason::ROLE_NOT_PERMITTED, "not this jar's program"))
            }
        }
        Role::Owner | Role::Reader if !same_token(&st.token, &token) => {
            Err((reason::WRONG_TOKEN, "wrong token"))
        }
        Role::Owner if st.owner.is_some() => {
            Err((reason::OWNER_PRESENT, "the jar already has an owner"))
        }
        Role::Owner | Role::Reader | Role::Lookup => Ok(()),
    };
    if let Err((r, why)) = allowed {
        drop(st);
        refuse(r, why.into());
        return;
    }
    let id = st.next_conn;
    st.next_conn += 1;
    // Decision M: the program held for its first owner starts now, before
    // the welcome, which carries its pid.
    if role == Role::Owner
        && let Err(e) = shared.start_program()
    {
        drop(st);
        refuse(
            reason::UNSPECIFIED,
            format!("the program didn't start: {e:#}"),
        );
        super::end::end(&shared.cfg.socket, 1);
    }
    if role == Role::Owner {
        st.info.retain(|(k, _)| k != crate::state::OWNER_LEFT);
        st.owner = Some(id);
        st.had_owner = true;
        st.ownerless_since = None;
        st.acked = st.log.base.max(st.acked.min(st.log.head));
    }
    let welcome = Msg::Welcome {
        version: version.min(proto::VERSION),
        child_pid: shared.child_pid(),
        head: st.log.head,
        base: st.log.base,
        rows: st.rows,
        cols: st.cols,
    };
    let _ = sock.set_write_timeout(None);
    let conn = register(shared, id, role, sock);
    if role == Role::Lookup {
        drop(st);
        for msg in greeting(welcome) {
            conn.send(shared, &msg);
        }
    } else {
        for msg in greeting(welcome) {
            conn.queue(&msg);
        }
        tell_program_if_owned(shared, &st, &conn);
        drop(st);
        shared.cv.notify_all();
        drop(ticket);
    }
    run_conn(shared, id, &conn);
}

/// What a connection hears first: its `welcome`, and in a jar built to
/// differ, a frame version 1 doesn't know.
fn greeting(welcome: Msg) -> Vec<Msg> {
    #[cfg_attr(
        not(feature = "differ"),
        expect(unused_mut, reason = "only the jar built to differ adds")
    )]
    let mut msgs = vec![welcome];
    #[cfg(feature = "differ")]
    msgs.push(Msg::Differ {
        note: b"a jar built to differ".to_vec(),
    });
    msgs
}

/// Queue the program's `attached` for `conn`, just registered under `st`:
/// when it is an owner, or the program connecting while an owner is there.
fn tell_program_if_owned(shared: &Shared, st: &State, conn: &Conn) {
    match conn.role {
        Role::Owner => queue_for_program(shared, &Msg::Attached),
        Role::Program if st.owner.is_some() => {
            conn.queue(&Msg::Attached);
        }
        _ => {}
    }
}

/// Compare tokens in time that doesn't depend on where they first differ.
pub(super) fn same_token(want: &[u8], got: &[u8]) -> bool {
    want.len() == got.len() && want.iter().zip(got).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
}

#[cfg(test)]
mod peer_tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn a_peer_is_known_by_its_user() {
        let (a, _b) = UnixStream::pair().expect("socketpair");
        let me = rustix::process::geteuid().as_raw();
        assert!(peer_is(&a, me), "this process's own socket is this user's");
        assert!(!peer_is(&a, me.wrapping_add(1)), "and no other user's");
    }

    /// A directory under `/tmp` (a socket's path is short), removed when
    /// it is dropped.
    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A jar set up and nothing started: no thread, no program.
    fn a_jar(dir: &Dir) -> Arc<Shared> {
        let cfg = crate::state::Config {
            socket: dir.0.join("j.sock"),
            argv: vec!["/bin/sh".into()],
            cwd: None,
            rows: 24,
            cols: 80,
            ring_cap: 1 << 16,
            window: 1 << 12,
            deadline: Duration::from_secs(10),
            linger_after_exit: Duration::from_secs(1),
            linger: None,
            instance: 1,
            created: 0,
            start_on_owner: Some(Duration::from_secs(60)),
        };
        let (master, slave) = crate::pty::open(&cfg).expect("a PTY");
        let listener = UnixListener::bind(&cfg.socket).expect("bind");
        let state = State {
            log: crate::state::Log {
                events: std::collections::VecDeque::new(),
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
            ownerless_since: None,
            fg: 0,
            had_owner: false,
            token: b"0123456789abcdef".to_vec(),
            info: Vec::new(),
        };
        super::super::prepare(cfg, state, (0, Some(slave)), master, listener, Vec::new())
            .expect("a jar")
            .shared
    }

    /// Review finding F6 (R26): a connection is served only when its peer is
    /// the jar's user. Served as a jar of another user, the test's own
    /// connection must be closed before a byte of it is read; served as its
    /// own, the same connection is welcomed. Without the check both are.
    #[test]
    fn a_connection_from_another_user_is_closed_unheard() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos();
        let dir = Dir(PathBuf::from(format!(
            "/tmp/jar-peer-{}-{nanos}",
            std::process::id()
        )));
        std::fs::create_dir(&dir.0).expect("a directory");
        let shared = a_jar(&dir);
        let me = rustix::process::geteuid().as_raw();
        let ask = |uid: u32| {
            let (mut client, jar) = UnixStream::pair().expect("socketpair");
            let hello = Msg::Hello {
                version: 1,
                role: Role::Lookup,
                token: Vec::new(),
            };
            client.write_all(&proto::encode(&hello)).expect("hello");
            client
                .write_all(&proto::encode(&Msg::Ping { n: 1 }))
                .expect("ping");
            client
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("a timeout");
            let shared = Arc::clone(&shared);
            let serving = std::thread::spawn(move || serve_as(&shared, Arc::new(jar), 0, uid));
            let answer = proto::read_msg(&mut client);
            serving.join().expect("served");
            answer
        };
        let own = ask(me);
        assert!(
            matches!(own, Ok(Some(Msg::Welcome { .. }))),
            "a connection of the jar's user is welcomed: {own:?}"
        );
        let other = ask(me.wrapping_add(1));
        assert!(
            !matches!(other, Ok(Some(_))),
            "one the jar's user didn't make is closed unheard: {other:?}"
        );
    }
}

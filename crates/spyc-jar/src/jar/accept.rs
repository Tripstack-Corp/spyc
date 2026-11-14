//! A new connection: who is at the other end (R26), its `hello`, and the
//! lease an owner takes with it.

use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::conn::{release, run_conn};
use super::{Conn, Shared, send_on};
use crate::proto::{self, Msg, Role, reason};

pub(super) fn register(shared: &Shared, id: u64, role: Role, stream: UnixStream) -> Arc<Conn> {
    no_sigpipe(&stream);
    let conn = Arc::new(Conn {
        role,
        peer_pid: peer_pid(&stream).unwrap_or(0),
        out: Arc::new(Mutex::new(stream)),
        cursor: Mutex::new(None),
        exit_sent: AtomicBool::new(false),
        fg_sent: AtomicU32::new(0),
        closed: AtomicBool::new(false),
        writer_done: AtomicBool::new(false),
    });
    shared.conns().insert(id, Arc::clone(&conn));
    conn
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
fn peer_is(stream: &UnixStream, uid: u32) -> bool {
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
/// the default.
#[cfg(target_os = "macos")]
pub fn no_sigpipe(stream: &UnixStream) {
    let _ = rustix::net::sockopt::set_socket_nosigpipe(stream, true);
}

/// Linux's std writes to a socket with `MSG_NOSIGNAL` already.
#[cfg(not(target_os = "macos"))]
pub const fn no_sigpipe(_stream: &UnixStream) {}

/// Tell every connection of the program's own (role `program`) that `msg`
/// happened: an owner attached or went.
pub(super) fn tell_program(shared: &Shared, msg: &Msg) {
    let programs: Vec<Arc<Conn>> = shared
        .conns()
        .values()
        .filter(|c| c.role == Role::Program)
        .cloned()
        .collect();
    for c in programs {
        let _ = send_on(&c.out, msg);
    }
}

/// One new connection: its `hello`, then [`run_conn`].
pub(super) fn serve(shared: &Arc<Shared>, stream: UnixStream) {
    // Another user's process is refused before a byte of it is read, whatever
    // token it holds; the jar's directory being `0700` is the first fence.
    if !peer_is(&stream, rustix::process::geteuid().as_raw()) {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(mut read_half) = stream.try_clone() else {
        return;
    };
    let refuse = |reason: u16, why: String| {
        let mut s = &stream;
        let _ = proto::write_msg(&mut s, &Msg::Refused { reason, why });
    };
    let Ok(Some(Msg::Hello {
        version,
        role,
        token,
    })) = proto::read_msg_max(&mut read_half, proto::MAX_HELLO)
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
            let ours = st.exit.is_none() && peer_pid(&stream).is_ok_and(|p| p == shared.child_pid);
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
    if role == Role::Owner {
        st.owner = Some(id);
        st.had_owner = true;
        st.ownerless_since = None;
        st.acked = st.log.base.max(st.acked.min(st.log.head));
    }
    let welcome = Msg::Welcome {
        version: version.min(proto::VERSION),
        child_pid: shared.child_pid,
        head: st.log.head,
        base: st.log.base,
        rows: st.rows,
        cols: st.cols,
    };
    let conn = register(shared, id, role, stream);
    drop(st);
    if send_on(&conn.out, &welcome).is_err() {
        release(shared, id);
        return;
    }
    let owned = shared.lock().owner.is_some();
    match role {
        Role::Owner => tell_program(shared, &Msg::Attached),
        Role::Program if owned => {
            let _ = send_on(&conn.out, &Msg::Attached);
        }
        _ => {}
    }
    run_conn(shared, id, &conn);
}

/// Compare tokens in time that doesn't depend on where they first differ.
pub(super) fn same_token(want: &[u8], got: &[u8]) -> bool {
    want.len() == got.len() && want.iter().zip(got).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
}

#[cfg(test)]
mod peer_tests {
    use super::*;

    #[test]
    fn a_peer_is_known_by_its_user() {
        let (a, _b) = UnixStream::pair().expect("socketpair");
        let me = rustix::process::geteuid().as_raw();
        assert!(peer_is(&a, me), "this process's own socket is this user's");
        assert!(!peer_is(&a, me.wrapping_add(1)), "and no other user's");
    }
}

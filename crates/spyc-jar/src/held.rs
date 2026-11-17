//! A new image that took over an upgrade and couldn't resume holds what it
//! was handed, rather than lose the program with itself (`JAR_PROTOCOL.md` →
//! "Upgrading: where each check runs", the new image's containment).
//!
//! What crossed the `exec` stays open: the PTY master, drained so the
//! program never blocks on a full terminal; the listener; and every
//! connection. Each connection that crossed hears why once, as reason 7
//! ([`reason::UPGRADE_FAILED`]), then has `Ping` and `Describe` answered and
//! everything else refused the same way. A new connection may look the jar
//! up; any other role is refused with reason 7, since nothing can attach.
//!
//! The jar ends with its program. Once no connection that crossed is left,
//! nobody can reach the program again, so the jar ends it as a closed
//! terminal would and goes.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crate::proto::{self, Msg, Role, reason};

/// What crossed the `exec`, in the order the old image sent it.
pub struct Crossed {
    pub listener: Option<UnixListener>,
    pub master: Option<OwnedFd>,
    pub conns: Vec<UnixStream>,
}

struct Held {
    why: String,
    socket: Option<PathBuf>,
    master: Option<OwnedFd>,
    /// Bytes the program printed that nobody will read.
    drained: AtomicU64,
    /// Connections that crossed and are still open.
    open: Mutex<usize>,
    /// No child is left to reap.
    childless: Mutex<bool>,
    childless_cv: Condvar,
    lookups: Mutex<VecDeque<Arc<UnixStream>>>,
}

/// How long a connection's write, and a lookup's question, may take.
const WAIT: Duration = Duration::from_secs(5);
/// How long the program has to go after `SIGHUP` before `SIGKILL`.
const HANGUP_GRACE: Duration = Duration::from_secs(2);
/// The most lookups open at once; the oldest is closed to admit another.
const MAX_LOOKUPS: usize = 16;

/// Hold `crossed` after a resume that failed with `cause`, until the program
/// ends or nobody is left to reach it. Never returns.
pub fn hold(crossed: Crossed, cause: &str) -> ! {
    let Crossed {
        listener,
        master,
        conns,
    } = crossed;
    let socket = listener
        .as_ref()
        .and_then(|l| l.local_addr().ok())
        .and_then(|a| a.as_pathname().map(Path::to_path_buf));
    let held = Arc::new(Held {
        why: format!(
            "the upgrade's new binary couldn't resume ({cause}); the jar holds its program and serves nothing else"
        ),
        socket,
        master,
        drained: AtomicU64::new(0),
        open: Mutex::new(conns.len()),
        childless: Mutex::new(false),
        childless_cv: Condvar::new(),
        lookups: Mutex::new(VecDeque::new()),
    });
    if let Some(pty) = held.master.as_ref().and_then(|m| m.try_clone().ok()) {
        let held = Arc::clone(&held);
        thread::spawn(move || drain(&held, File::from(pty)));
    }
    {
        let held = Arc::clone(&held);
        thread::spawn(move || reap(&held));
    }
    if conns.is_empty() {
        end(&held);
    }
    for sock in conns {
        let held = Arc::clone(&held);
        thread::spawn(move || carried(&held, &sock));
    }
    if let Some(listener) = listener {
        accept(&held, &listener);
    }
    loop {
        thread::park();
    }
}

/// Read the program's output and drop it, so the program never waits on a
/// terminal nobody reads.
fn drain(held: &Held, mut pty: File) {
    let mut buf = vec![0u8; 8192];
    loop {
        if !crate::jar::ready(&pty, rustix::event::PollFlags::IN, WAIT) {
            continue;
        }
        match pty.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                held.drained.fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            // The program's side has closed.
            Err(_) => return,
        }
    }
}

/// Reap the program, and end the jar once no child is left.
fn reap(held: &Held) {
    while let Ok(_) | Err(rustix::io::Errno::INTR) =
        rustix::process::wait(rustix::process::WaitOptions::empty())
    {}
    *held
        .childless
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = true;
    held.childless_cv.notify_all();
    finish(held);
}

/// One connection that crossed: told why, then answered until it goes.
fn carried(held: &Held, sock: &UnixStream) {
    crate::jar::no_sigpipe(sock);
    let _ = sock.set_nonblocking(false);
    let _ = sock.set_read_timeout(None);
    let _ = sock.set_write_timeout(Some(WAIT));
    let refused = || Msg::Refused {
        reason: reason::UPGRADE_FAILED,
        why: held.why.clone(),
    };
    let mut s = sock;
    if proto::write_msg(&mut s, &refused()).is_ok() {
        loop {
            let reply = match proto::read_msg(&mut s) {
                Ok(Some(Msg::Ping { n })) => Msg::Pong { n },
                Ok(Some(Msg::Describe)) => Msg::Description {
                    fields: describe(held),
                },
                Ok(Some(_)) => refused(),
                Ok(None) => continue,
                Err(proto::ReadError::UnknownCritical(kind)) => Msg::Unsupported { kind },
                Err(_) => break,
            };
            if proto::write_msg(&mut s, &reply).is_err() {
                break;
            }
        }
    }
    let _ = sock.shutdown(std::net::Shutdown::Both);
    let mut open = held.open.lock().unwrap_or_else(PoisonError::into_inner);
    *open = open.saturating_sub(1);
    if *open == 0 {
        drop(open);
        end(held);
    }
}

/// Take new connections: lookups are answered, nothing else is let in.
fn accept(held: &Arc<Held>, listener: &UnixListener) -> ! {
    // The old image left it non-blocking for its own acceptor.
    let _ = listener.set_nonblocking(false);
    loop {
        let Ok((sock, _)) = listener.accept() else {
            thread::sleep(Duration::from_millis(100));
            continue;
        };
        let sock = Arc::new(sock);
        {
            let mut open = held.lookups.lock().unwrap_or_else(PoisonError::into_inner);
            open.push_back(Arc::clone(&sock));
            while open.len() > MAX_LOOKUPS {
                if let Some(oldest) = open.pop_front() {
                    let _ = oldest.shutdown(std::net::Shutdown::Both);
                }
            }
        }
        let held = Arc::clone(held);
        thread::spawn(move || {
            lookup(&held, &sock);
            held.lookups
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|s| !Arc::ptr_eq(s, &sock));
        });
    }
}

/// A new connection: its `hello`, then one question if it is a lookup.
fn lookup(held: &Held, sock: &UnixStream) {
    if !crate::jar::peer_is(sock, rustix::process::geteuid().as_raw()) {
        return;
    }
    crate::jar::no_sigpipe(sock);
    let _ = sock.set_nonblocking(false);
    let _ = sock.set_read_timeout(Some(WAIT));
    let _ = sock.set_write_timeout(Some(WAIT));
    let mut s = sock;
    let answer = |msg: &Msg| proto::write_msg(&mut &*sock, msg).is_ok();
    let Ok(Some(Msg::Hello { version, role, .. })) = proto::read_msg_max(&mut s, proto::MAX_HELLO)
    else {
        answer(&Msg::Refused {
            reason: reason::INVALID,
            why: "expected a hello".into(),
        });
        return;
    };
    if version == 0 {
        answer(&Msg::Refused {
            reason: reason::VERSION_NOT_SERVED,
            why: format!("jar speaks version {}", proto::VERSION),
        });
        return;
    }
    if role != Role::Lookup {
        answer(&Msg::Refused {
            reason: reason::UPGRADE_FAILED,
            why: held.why.clone(),
        });
        return;
    }
    let (rows, cols) = held
        .master
        .as_ref()
        .and_then(|m| rustix::termios::tcgetwinsize(m).ok())
        .map_or((0, 0), |w| (w.ws_row, w.ws_col));
    let welcomed = answer(&Msg::Welcome {
        version: version.min(proto::VERSION),
        child_pid: 0,
        head: 0,
        base: 0,
        rows,
        cols,
    });
    if !welcomed {
        return;
    }
    let reply = match proto::read_msg(&mut s) {
        Ok(Some(Msg::Ping { n })) => Msg::Pong { n },
        Ok(Some(Msg::Describe)) => Msg::Description {
            fields: describe(held),
        },
        Ok(Some(_)) => Msg::Refused {
            reason: reason::ROLE_NOT_PERMITTED,
            why: "a lookup asks only what the jar is".into(),
        },
        _ => return,
    };
    answer(&reply);
}

/// What a held jar says of itself: enough to be told apart and to keep its
/// binary's store entry, and why it holds.
fn describe(held: &Held) -> Vec<(String, String)> {
    let id = held
        .socket
        .as_deref()
        .and_then(Path::file_stem)
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let copies = held
        .socket
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("/"))
        .join(crate::store::COPIES);
    let image = crate::upgrade::own_image();
    [
        ("id", id),
        ("held", held.why.clone()),
        (
            "jar",
            format!("{} {}", crate::PACKAGE_VERSION, image.display()),
        ),
        ("build", crate::store::build_of(&image, &copies)),
        ("commit", crate::cli::commit().to_string()),
        ("pid", std::process::id().to_string()),
        (
            "drained",
            format!("{} bytes", held.drained.load(Ordering::Relaxed)),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Nobody can reach the program again: hang it up as a closed terminal
/// would, kill it if it lingers, and go.
fn end(held: &Held) -> ! {
    use rustix::process::{Signal, kill_process_group};
    if let Some(group) = held
        .master
        .as_ref()
        .and_then(|m| rustix::termios::tcgetpgrp(m).ok())
    {
        let _ = kill_process_group(group, Signal::HUP);
        let gone = *held
            .childless_cv
            .wait_timeout_while(
                held.childless
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
                HANGUP_GRACE,
                |c| !*c,
            )
            .unwrap_or_else(PoisonError::into_inner)
            .0;
        if !gone {
            let _ = kill_process_group(group, Signal::KILL);
        }
    }
    finish(held)
}

/// End the jar, taking its socket with it. Once only: the reaper and the
/// last connection may both get here.
fn finish(held: &Held) -> ! {
    static ENDING: AtomicBool = AtomicBool::new(false);
    if ENDING.swap(true, Ordering::SeqCst) {
        loop {
            thread::park();
        }
    }
    if let Some(socket) = &held.socket {
        let _ = std::fs::remove_file(socket);
        crate::lock::forget(socket);
    }
    std::process::exit(0)
}

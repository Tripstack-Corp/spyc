#![cfg(unix)]
//! B12: a read of a jar's connection that a signal interrupts goes on
//! waiting, as an uninterrupted one does. An interrupted read is no lost
//! connection: Linux fails a socket read that has a timeout with `EINTR`,
//! whatever `SA_RESTART` says, both when a handled signal reaches the
//! reading thread and when the process is stopped and continued; macOS
//! restarts it. Each test reads from a jar of its own that welcomes the
//! client and then says nothing until told.
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use spyc_jar::client::Client;
use spyc_jar::proto::{self, Msg, Role};

/// How long each test's read waits: far past what the test takes.
const READ_WAIT: Duration = Duration::from_secs(20);

/// A jar's end of one connection: it welcomes the client, then sends a
/// ping once told. Its socket, its directory, and the telling.
fn quiet_jar() -> (PathBuf, tempfile::TempDir, mpsc::Sender<()>) {
    // A socket's path is capped near 104 bytes: the directory is short.
    let dir = tempfile::Builder::new()
        .prefix("ji")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("j.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (tell, told) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let _hello = proto::read_msg(&mut conn).unwrap();
        proto::write_msg(
            &mut conn,
            &Msg::Welcome {
                version: proto::VERSION,
                child_pid: 1,
                head: 0,
                base: 0,
                rows: 24,
                cols: 80,
            },
        )
        .unwrap();
        if told.recv().is_ok() {
            proto::write_msg(&mut conn, &Msg::Ping { n: 1 }).unwrap();
        }
        // Open until the test ends: a closed connection is a lost one.
        let _ = told.recv();
    });
    (socket, dir, tell)
}

fn connect(socket: &std::path::Path) -> Client {
    Client::connect(socket, Role::Owner, Duration::from_secs(10), b"token")
        .unwrap()
        .0
}

/// A handled signal on the reading thread, as spyc's own handlers can take
/// one: the read goes on, and returns the frame the jar sends after.
#[test]
fn a_read_a_handled_signal_interrupts_goes_on_waiting() {
    use std::os::unix::thread::JoinHandleExt;
    let handled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register(
        signal_hook::consts::SIGUSR1,
        std::sync::Arc::clone(&handled),
    )
    .unwrap();
    let (socket, _dir, tell) = quiet_jar();
    let mut client = connect(&socket);
    let reader = std::thread::spawn(move || client.recv(READ_WAIT).map_err(|e| format!("{e:#}")));
    // Signals until one has surely found the read waiting.
    for _ in 0..20 {
        if reader.is_finished() {
            break;
        }
        // `RawPthread` is `pthread_t` on macOS and an integer on Linux musl.
        #[allow(clippy::unnecessary_cast, reason = "a cast only on some targets")]
        // SAFETY: `reader` isn't joined yet, so its pthread handle still
        // names that thread; SIGUSR1 has the handler registered above.
        unsafe {
            libc::pthread_kill(reader.as_pthread_t() as libc::pthread_t, libc::SIGUSR1);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        handled.load(std::sync::atomic::Ordering::SeqCst),
        "premise: a signal was handled"
    );
    tell.send(()).unwrap();
    assert_eq!(
        reader.join().unwrap(),
        Ok(Some(Msg::Ping { n: 1 })),
        "an interrupted read ended the connection"
    );
}

/// This process stopped and continued from outside while the read waits:
/// the read goes on, and returns the frame the jar sends after.
#[test]
fn a_read_through_a_stop_and_continue_goes_on_waiting() {
    let (socket, _dir, tell) = quiet_jar();
    let mut client = connect(&socket);
    let reader = std::thread::spawn(move || client.recv(READ_WAIT).map_err(|e| format!("{e:#}")));
    let me = std::process::id();
    // Stops and continues until one has surely found the read waiting: the
    // first may come before the reader has begun to.
    for _ in 0..5 {
        if reader.is_finished() {
            break;
        }
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                &format!("kill -STOP {me}; sleep 0.2; kill -CONT {me}"),
            ])
            .status()
            .unwrap();
        assert!(status.success(), "premise: stopped and continued");
    }
    tell.send(()).unwrap();
    assert_eq!(
        reader.join().unwrap(),
        Ok(Some(Msg::Ping { n: 1 })),
        "an interrupted read ended the connection"
    );
}

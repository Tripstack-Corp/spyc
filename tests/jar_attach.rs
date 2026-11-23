#![cfg(unix)]
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

//! The attach client (`spyc-jar attach`) on a real terminal: what it relays,
//! and what it doesn't. Driven through the binary, on a PTY of the test's own,
//! to a jar running a program that answers each line.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const WAIT: Duration = Duration::from_secs(30);
const TOKEN: &[u8] = b"attach-test-token-0123456789";

/// A jar running a program that answers each line, ended with the test.
struct Jar {
    dir: tempfile::TempDir,
    socket: PathBuf,
    process: Child,
}

impl Drop for Jar {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl Jar {
    fn start() -> Self {
        Self::start_with(&[])
    }

    /// [`Self::start`], with the jar's flags after `--wire 1`.
    fn start_with(flags: &[&str]) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("jt")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let socket = dir.path().join("j.sock");
        let mut process = Command::new(env!("CARGO_BIN_EXE_spyc-jar"))
            .args(["--wire", "1"])
            .args(flags)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(
                &spyc_jar::cli::Start {
                    token: TOKEN.to_vec(),
                    socket: socket.clone(),
                    cwd: None,
                    rows: 24,
                    cols: 80,
                    hold: None,
                    exit_linger: None,
                    argv: ["/bin/sh", "-c", "while read l; do echo got=$l; done"]
                        .map(String::from)
                        .to_vec(),
                }
                .encode(),
            )
            .unwrap();
        let until = Instant::now() + WAIT;
        while spyc_jar::client::describe(&socket).is_err() {
            assert!(Instant::now() < until, "the jar never answered");
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            dir,
            socket,
            process,
        }
    }

    fn described(&self, key: &str) -> Option<String> {
        spyc_jar::client::describe(&self.socket)
            .ok()?
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
}

/// Keys typed before the attach client could show the program were typed
/// blind, and would land wherever the program is now: the client drops them,
/// and relays what is typed once it's attached.
#[test]
fn keys_typed_before_the_client_attaches_are_dropped() {
    let jar = Jar::start();
    let token_file = jar.dir.path().join("token");
    std::fs::write(&token_file, TOKEN).unwrap();

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut keys = pair.master.take_writer().unwrap();
    // In the terminal's input before the client starts.
    keys.write_all(b"early\r").unwrap();
    keys.flush().unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_spyc-jar"));
    cmd.args(["attach"]);
    cmd.arg(&jar.socket);
    cmd.arg(&token_file);
    let mut client = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let seen = Arc::new(Mutex::new(Vec::new()));
    {
        let seen = Arc::clone(&seen);
        let mut reader = pair.master.try_clone_reader().unwrap();
        std::thread::spawn(move || {
            let _master = pair.master;
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                seen.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
    }
    let shown = || String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
    let until = Instant::now() + WAIT;
    while jar.described("owner_pid") != client.process_id().map(|p| p.to_string()) {
        assert!(
            Instant::now() < until,
            "the client never attached: {}",
            shown()
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    keys.write_all(b"late\r").unwrap();
    keys.flush().unwrap();
    let until = Instant::now() + WAIT;
    while !shown().contains("got=late") {
        assert!(
            Instant::now() < until,
            "keys typed now never arrived: {}",
            shown()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !shown().contains("got=early"),
        "keys typed before the client attached reached the program: {}",
        shown()
    );
    let _ = client.kill();
}

/// A jar says why its owner left (`owner_left`), so a client that sees its
/// connection close can ask: one that sent nothing for the jar's deadline
/// went silent, and one that closed its connection closed it. An owner that
/// connects clears it.
#[test]
fn a_jar_says_why_its_owner_left() {
    use spyc_jar::client::Client;
    use spyc_jar::proto::Role;
    let jar = Jar::start_with(&["--deadline-ms", "1000"]);
    let owner_gone = |what: &str| {
        let until = Instant::now() + WAIT;
        loop {
            if let Some(why) = jar.described("owner_left") {
                return why;
            }
            assert!(Instant::now() < until, "{what}: the owner never left");
            std::thread::sleep(Duration::from_millis(20));
        }
    };

    // Connected, then nothing more.
    let (silent, _) = Client::connect(&jar.socket, Role::Owner, WAIT, TOKEN).unwrap();
    assert_eq!(owner_gone("silent"), "silent");
    drop(silent);

    let (closing, _) = Client::connect(&jar.socket, Role::Owner, WAIT, TOKEN).unwrap();
    assert_eq!(
        jar.described("owner_left"),
        None,
        "an owner connected, and the last one's reason stayed"
    );
    closing.drop_connection();
    assert_eq!(owner_gone("closed"), "closed");
}

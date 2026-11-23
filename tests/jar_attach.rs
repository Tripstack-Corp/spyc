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
        Self::start_running(flags, "while read l; do echo got=$l; done")
    }

    /// A jar with `flags` after `--wire 1`, running `script` in `/bin/sh`.
    fn start_running(flags: &[&str], script: &str) -> Self {
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
                    argv: ["/bin/sh", "-c", script].map(String::from).to_vec(),
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

    // The jar names its owner as the client connects, a moment before the
    // client relays keys: type until the program answers.
    let until = Instant::now() + WAIT;
    while !shown().contains("got=late") {
        assert!(
            Instant::now() < until,
            "keys typed now never arrived: {}",
            shown()
        );
        keys.write_all(b"late\r").unwrap();
        keys.flush().unwrap();
        std::thread::sleep(Duration::from_millis(250));
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

/// `spyc-jar attach` on a PTY of the test's own, which reads what the client
/// writes to it unless paused, as a terminal that has stopped reading.
struct Attached {
    client: Box<dyn portable_pty::Child + Send + Sync>,
    keys: Box<dyn Write + Send>,
    seen: Arc<Mutex<Vec<u8>>>,
    paused: Arc<std::sync::atomic::AtomicBool>,
}

impl Attached {
    fn start(jar: &Jar) -> Self {
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
        let keys = pair.master.take_writer().unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_spyc-jar"));
        cmd.args(["attach"]);
        cmd.arg(&jar.socket);
        cmd.arg(&token_file);
        let client = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let (seen, paused) = (Arc::clone(&seen), Arc::clone(&paused));
            let mut reader = pair.master.try_clone_reader().unwrap();
            std::thread::spawn(move || {
                let _master = pair.master;
                let mut buf = [0u8; 16384];
                loop {
                    while paused.load(std::sync::atomic::Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => seen.lock().unwrap().extend_from_slice(&buf[..n]),
                    }
                }
            });
        }
        let mut attached = Self {
            client,
            keys,
            seen,
            paused,
        };
        attached.wait_attached(jar);
        attached.type_until_shown("ready");
        attached
    }

    fn pid(&self) -> u32 {
        self.client.process_id().unwrap()
    }

    fn shown(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }

    fn running(&mut self) -> bool {
        self.client.try_wait().unwrap().is_none()
    }

    fn pause(&self, paused: bool) {
        self.paused
            .store(paused, std::sync::atomic::Ordering::SeqCst);
    }

    fn type_keys(&mut self, keys: &[u8]) {
        self.keys.write_all(keys).unwrap();
        self.keys.flush().unwrap();
    }

    /// Type `line` until the program answers it. The jar names its owner as
    /// the client connects, a moment before the client relays keys, and keys
    /// typed before then go nowhere.
    fn type_until_shown(&mut self, line: &str) {
        let answer = format!("got={line}");
        let until = Instant::now() + WAIT;
        while !self.shown().contains(&answer) {
            assert!(
                Instant::now() < until,
                "never shown: {answer}; the terminal showed: {}",
                self.shown()
            );
            self.type_keys(format!("{line}\r").as_bytes());
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Wait for the jar to have this client as its owner.
    fn wait_attached(&self, jar: &Jar) {
        let until = Instant::now() + WAIT;
        while jar.described("owner_pid") != Some(self.pid().to_string()) {
            assert!(
                Instant::now() < until,
                "the client never attached: {}",
                self.shown()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_shown(&self, needle: &str) {
        let until = Instant::now() + WAIT;
        while !self.shown().contains(needle) {
            let shown = self.shown();
            assert!(
                Instant::now() < until,
                "never shown: {needle}; the terminal showed: {}",
                &shown[shown.len().saturating_sub(2000)..]
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        let _ = self.client.kill();
    }
}

/// A terminal slow to read what the program prints keeps its session: the
/// client's writes to it wait, and meanwhile the client still answers its
/// jar, so the jar never takes it for gone. A terminal app paused while its
/// computer sleeps reads nothing.
#[test]
fn a_terminal_slow_to_read_keeps_its_session() {
    let jar = Jar::start_running(
        &["--deadline-ms", "4000"],
        "while read l; do if [ \"$l\" = flood ]; then yes | head -c 2000000; fi; echo got=$l; done",
    );
    let mut term = Attached::start(&jar);
    term.pause(true);
    term.type_keys(b"flood\r");
    // Past the jar's deadline, reading none of it.
    std::thread::sleep(Duration::from_secs(6));
    assert_eq!(
        jar.described("owner_left"),
        None,
        "the jar dropped the client of a terminal slow to read"
    );
    term.pause(false);
    term.wait_shown("got=flood");
    term.type_keys(b"after\r");
    term.wait_shown("got=after");
    assert!(term.running(), "the client ended: {}", term.shown());
}

/// A client stopped past the jar's deadline, as a computer's sleep can stop
/// it, is dropped by the jar. Once it runs again it attaches again by
/// itself, and what is typed then reaches the program; what was typed while
/// it was dropped was typed blind, and doesn't.
#[test]
fn a_client_stopped_past_the_deadline_attaches_again() {
    use rustix::process::{Pid, Signal, kill_process};
    let jar = Jar::start_with(&["--deadline-ms", "3000"]);
    let mut term = Attached::start(&jar);
    let pid = Pid::from_raw(i32::try_from(term.pid()).unwrap()).unwrap();
    kill_process(pid, Signal::STOP).unwrap();
    let until = Instant::now() + WAIT;
    while jar.described("owner_left").as_deref() != Some("silent") {
        assert!(
            Instant::now() < until,
            "the jar never dropped the stopped client"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    term.type_keys(b"blind\r");
    kill_process(pid, Signal::CONT).unwrap();
    term.wait_attached(&jar);
    term.type_until_shown("back");
    assert!(term.running(), "the client ended: {}", term.shown());
    assert!(
        !term.shown().contains("got=blind"),
        "keys typed while the client was dropped reached the program: {}",
        term.shown()
    );
}

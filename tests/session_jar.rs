#![cfg(unix)]
//! spyc in its own jar (S4): a session's jar lives in the sessions directory,
//! and spyc running in one sets each terminal that attaches up again, probes
//! it and repaints, while a spyc that is a pane jar's program does none of it.
//! Driven through the binaries: a real `spyc-jar` running spyc, and the test
//! as the terminal that attaches, answering spyc's queries as a terminal would.
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use spyc_jar::client::{Client, Sender};
use spyc_jar::proto::{Msg, Role};

const WAIT: Duration = Duration::from_secs(30);
const TOKEN: &[u8] = b"session-jar-test-token-0123";
const NAME: &str = "PAPRIKA_TEST";

/// The queries spyc makes of its terminal: what a terminal with the kitty
/// keyboard protocol answers, and one without it.
const KITTY: &[(&[u8], &[u8])] = &[
    (b"\x1b[?u", b"\x1b[?0u"),
    (b"\x1b[16t", b"\x1b[6;16;8t"),
    (b"\x1b[c", b"\x1b[?62;22c"),
    (b"\x1b[5n", b"\x1b[0n"),
    (b"\x1b[6n", b"\x1b[1;1R"),
];
const PLAIN: &[(&[u8], &[u8])] = &[
    (b"\x1b[16t", b"\x1b[6;16;8t"),
    (b"\x1b[c", b"\x1b[?62;22c"),
    (b"\x1b[5n", b"\x1b[0n"),
    (b"\x1b[6n", b"\x1b[1;1R"),
];

const ALT_SCREEN: &str = "\x1b[?1049h";
const BRACKETED_PASTE: &str = "\x1b[?2004h";
const KEYBOARD_PROBE: &str = "\x1b[?u";
const IMAGE_PROBE: &str = "\x1b[16t";
const KITTY_PUSH: &str = "\x1b[>1u";

struct Dirs {
    root: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    config: PathBuf,
    runtime: PathBuf,
    project: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::Builder::new()
            .prefix("sj")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let at = |name: &str| {
            let d = root.path().join(name);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let d = Self {
            home: at("h"),
            state: at("s"),
            config: at("c"),
            runtime: at("r"),
            project: at("p"),
            root,
        };
        for dir in ["spyc", "spyc/sessions", "spyc/jars"] {
            let dir = d.runtime.join(dir);
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        d
    }

    fn env(&self) -> [(&'static str, &std::ffi::OsStr); 8] {
        [
            ("HOME", self.home.as_os_str()),
            ("XDG_STATE_HOME", self.state.as_os_str()),
            ("XDG_CONFIG_HOME", self.config.as_os_str()),
            ("XDG_RUNTIME_DIR", self.runtime.as_os_str()),
            ("TMPDIR", self.runtime.as_os_str()),
            ("PATH", "/usr/bin:/bin".as_ref()),
            ("SHELL", "/bin/sh".as_ref()),
            ("TERM", "xterm-256color".as_ref()),
        ]
    }

    /// Whether spyc's context file, which it writes for its MCP tools, has
    /// its cursor on `name`.
    fn context_shows(&self, name: &str) -> bool {
        let needle = format!("\"cursor_file\": \"{name}\"");
        std::fs::read_dir(self.state.join("spyc"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".spyc-context-")
            })
            .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| t.contains(&needle)))
    }

    /// The test's own directory's name, which every path spyc shows holds.
    fn mark(&self) -> String {
        self.root
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }
}

/// A jar at `socket` running spyc, started as spyc starts one. Its program
/// and the jar are ended when it drops, however the test ended.
struct Jar {
    socket: PathBuf,
    process: std::process::Child,
}

impl Jar {
    fn start(dirs: &Dirs, socket: PathBuf) -> Self {
        let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
        let binary = spyc.with_file_name("spyc-jar");
        let mut process = Command::new(&binary)
            .args(["--wire", &spyc_jar::proto::VERSION.to_string()])
            .env_clear()
            .envs(dirs.env())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = spyc_jar::cli::Start {
            token: TOKEN.to_vec(),
            socket: socket.clone(),
            cwd: Some(dirs.project.clone()),
            rows: 24,
            cols: 80,
            hold: None,
            exit_linger: None,
            argv: vec![spyc.display().to_string()],
        };
        let mut stdin = process.stdin.take().unwrap();
        stdin.write_all(&start.encode()).unwrap();
        drop(stdin);
        let jar = Self { socket, process };
        eventually("the jar answers", || {
            spyc_jar::client::answers_now(&jar.socket)
        });
        jar
    }

    fn described(&self, key: &str) -> Option<String> {
        spyc_jar::client::describe(&self.socket)
            .ok()?
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// How much its program has printed.
    fn printed(&self) -> u64 {
        let log = self.described("log").unwrap();
        let head = log.split(" printed").next().unwrap();
        head.rsplit(' ').next().unwrap().parse().unwrap()
    }

    /// Wait for spyc to have written to its terminal: it links to its own
    /// jar before it touches the terminal, so it has linked by then.
    fn wait_for_spyc(&self) {
        eventually("spyc writes to its terminal", || {
            self.described("log")
                .is_some_and(|log| !log.contains(" 0 printed"))
        });
    }
}

impl Drop for Jar {
    fn drop(&mut self) {
        let pid = |key| self.described(key).and_then(|p| p.parse().ok());
        if let Some(p) = pid("program_pid").and_then(rustix::process::Pid::from_raw) {
            let _ = rustix::process::kill_process(p, rustix::process::Signal::KILL);
        }
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// A terminal attached to a jar: what it was sent since it attached, and
/// spyc's queries answered as `answers` says.
struct Terminal {
    seen: Arc<Mutex<Vec<u8>>>,
    exited: Arc<Mutex<bool>>,
    tx: Sender,
}

impl Terminal {
    fn attach(
        jar: &Jar,
        size: (u16, u16),
        answers: &'static [(&'static [u8], &'static [u8])],
    ) -> Self {
        Self::attach_from(jar, None, size, answers)
    }

    /// [`Self::attach`], sent what spyc printed from `from` on rather than
    /// only what it prints next.
    fn attach_from(
        jar: &Jar,
        from: Option<u64>,
        (rows, cols): (u16, u16),
        answers: &'static [(&'static [u8], &'static [u8])],
    ) -> Self {
        let (client, welcome) = Client::connect(&jar.socket, Role::Owner, WAIT, TOKEN).unwrap();
        let Msg::Welcome { head, .. } = welcome else {
            panic!("expected a welcome, got {welcome:?}");
        };
        let (mut rx, tx) = client.split();
        tx.send(&Msg::Replay {
            from: from.unwrap_or(head),
        })
        .unwrap();
        tx.send(&Msg::Resize {
            rows,
            cols,
            px_w: 0,
            px_h: 0,
        })
        .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let exited = Arc::new(Mutex::new(false));
        {
            let (seen, exited, tx) = (Arc::clone(&seen), Arc::clone(&exited), tx.clone());
            std::thread::spawn(move || {
                while let Ok(msg) = rx.recv(Duration::from_millis(200)) {
                    match msg {
                        Some(Msg::Output { offset, bytes }) => {
                            for (query, answer) in answers {
                                let asked =
                                    bytes.windows(query.len()).filter(|w| w == query).count();
                                for _ in 0..asked {
                                    let _ = tx.send(&Msg::Input {
                                        bytes: answer.to_vec(),
                                    });
                                }
                            }
                            seen.lock().unwrap().extend_from_slice(&bytes);
                            let _ = tx.send(&Msg::Ack {
                                offset: offset + bytes.len() as u64,
                            });
                        }
                        Some(Msg::Exited { .. }) => {
                            *exited.lock().unwrap() = true;
                            let _ = tx.send(&Msg::AckExit);
                        }
                        _ => {}
                    }
                }
            });
        }
        Self { seen, exited, tx }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }

    fn type_keys(&self, keys: &[u8]) {
        self.tx
            .send(&Msg::Input {
                bytes: keys.to_vec(),
            })
            .unwrap();
    }

    fn wait_for(&self, what: &str, needle: &str) {
        let until = Instant::now() + WAIT;
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < until,
                "never: {what}; the terminal was sent:\n{:?}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The terminal leaves: the jar's connection drops, as when a terminal
    /// is closed.
    fn close(self) {
        self.tx.close();
    }
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < until, "never: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What came before `needle`'s first appearance in `text`.
fn before<'a>(text: &'a str, needle: &str) -> &'a str {
    text.find(needle).map_or(text, |at| &text[..at])
}

#[test]
fn each_terminal_that_attaches_to_spycs_own_jar_is_set_up_probed_and_drawn_on() {
    let dirs = Dirs::new();
    let jar = Jar::start(
        &dirs,
        dirs.runtime
            .join("spyc/sessions")
            .join(format!("{NAME}.sock")),
    );
    jar.wait_for_spyc();

    // A terminal with the kitty keyboard protocol.
    let first = Terminal::attach(&jar, (30, 100), KITTY);
    first.wait_for("the session's name drawn", NAME);
    let drawn = before(&first.text(), NAME).to_string();
    for (what, sequence) in [
        ("the alternate screen", ALT_SCREEN),
        ("bracketed paste", BRACKETED_PASTE),
        ("the keyboard probe", KEYBOARD_PROBE),
        ("the image probe", IMAGE_PROBE),
        ("the kitty keyboard flag", KITTY_PUSH),
    ] {
        assert!(
            drawn.contains(sequence),
            "{what} not set before the first frame: {drawn:?}"
        );
    }
    first.close();

    // With no terminal attached, spyc has something new to show and draws
    // none of it: what it printed meanwhile is sent to the next terminal,
    // and none of that is the new file.
    let detached_at = jar.printed();
    std::fs::write(dirs.project.join("detached-marker"), "").unwrap();
    eventually("spyc sees the new file", || {
        dirs.context_shows("detached-marker")
    });

    // Another, smaller, without it.
    let second = Terminal::attach_from(&jar, Some(detached_at), (20, 70), PLAIN);
    second.wait_for("the session's name drawn again", NAME);
    let drawn = before(&second.text(), NAME).to_string();
    for (what, sequence) in [
        ("the alternate screen", ALT_SCREEN),
        ("bracketed paste", BRACKETED_PASTE),
        ("the keyboard probe", KEYBOARD_PROBE),
        ("the image probe", IMAGE_PROBE),
    ] {
        assert!(
            drawn.contains(sequence),
            "{what} not set again before the frame: {drawn:?}"
        );
    }
    assert!(
        !drawn.contains(KITTY_PUSH),
        "the kitty keyboard flag pushed on a terminal without it: {drawn:?}"
    );
    assert!(
        before(&second.text(), "detached-marker").contains(ALT_SCREEN),
        "drawn with no terminal attached: {:?}",
        second.text()
    );

    // Keys reach spyc after both probes: none were taken by them.
    second.type_keys(b":q\r");
    second.wait_for("spyc asks for a second `:q`", "press again to quit");
    second.type_keys(b":q\r");
    eventually("spyc quits when told to", || *second.exited.lock().unwrap());
}

#[test]
fn a_spyc_in_a_panes_jar_takes_it_for_a_panes_not_its_own() {
    let dirs = Dirs::new();
    let jar = Jar::start(&dirs, dirs.runtime.join("spyc/jars/0123456789abcdef.sock"));
    jar.wait_for_spyc();

    let terminal = Terminal::attach(&jar, (30, 100), KITTY);
    // A plain spyc redraws for the new size, setting nothing up again.
    terminal.wait_for("spyc drawn at the new size", &dirs.mark());
    let drawn = before(&terminal.text(), &dirs.mark()).to_string();
    assert!(
        !drawn.contains(ALT_SCREEN) && !drawn.contains(KEYBOARD_PROBE),
        "a pane's jar taken for spyc's own: {drawn:?}"
    );
    assert!(
        !terminal.text().contains(NAME),
        "named after a jar that isn't its own"
    );
}

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
/// What `:q` asks while quitting would end something.
const QUIT_ASKS: &str = "quit anyway? [y/N]";
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

/// The instrumented build's `LLVM_PROFILE_FILE`, kept through each spawned
/// process's cleared environment: without it an instrumented spyc writes its
/// coverage data into the directory it runs in, the test's project among them.
fn profile_file() -> Option<&'static std::ffi::OsStr> {
    static FILE: std::sync::OnceLock<Option<std::ffi::OsString>> = std::sync::OnceLock::new();
    FILE.get_or_init(|| std::env::var_os("LLVM_PROFILE_FILE"))
        .as_deref()
}

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

    fn env(&self) -> Vec<(&'static str, &std::ffi::OsStr)> {
        let mut env = vec![
            ("HOME", self.home.as_os_str()),
            ("XDG_STATE_HOME", self.state.as_os_str()),
            ("XDG_CONFIG_HOME", self.config.as_os_str()),
            ("XDG_RUNTIME_DIR", self.runtime.as_os_str()),
            ("TMPDIR", self.runtime.as_os_str()),
            ("PATH", "/usr/bin:/bin".as_ref()),
            ("SHELL", "/bin/sh".as_ref()),
            ("TERM", "xterm-256color".as_ref()),
        ];
        env.extend(profile_file().map(|file| ("LLVM_PROFILE_FILE", file)));
        env
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
        Self::start_with(dirs, socket, &[])
    }

    /// [`Self::start`], with the jar's flags after `--wire`.
    fn start_with(dirs: &Dirs, socket: PathBuf, flags: &[&str]) -> Self {
        let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
        let binary = spyc.with_file_name("spyc-jar");
        let mut process = Command::new(&binary)
            .args(["--wire", &spyc_jar::proto::VERSION.to_string()])
            .args(flags)
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

/// Quit with `:q`, as the user would: spyc quits at once with nothing to
/// lose, and otherwise asks once, which a `y` answers. `quit` says it has
/// gone.
fn quit_with_q(
    what: &str,
    mut type_keys: impl FnMut(&[u8]),
    mut asked: impl FnMut() -> bool,
    mut quit: impl FnMut() -> bool,
) {
    type_keys(b":q\r");
    let until = Instant::now() + WAIT;
    let mut answered = false;
    while !quit() {
        if !answered && asked() {
            type_keys(b"y");
            answered = true;
        }
        assert!(Instant::now() < until, "never: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What came before `needle`'s first appearance in `text`.
fn before<'a>(text: &'a str, needle: &str) -> &'a str {
    text.find(needle).map_or(text, |at| &text[..at])
}

/// The second session the tests that need two run.
const OTHER: &str = "SAFFRON_TEST";

/// Where a session's own jar listens.
fn session_socket(dirs: &Dirs, name: &str) -> PathBuf {
    dirs.runtime
        .join("spyc/sessions")
        .join(format!("{name}.sock"))
}

/// The token `spyc attach` presents, kept where spyc keeps a session jar's.
fn keep_token(dirs: &Dirs, name: &str) {
    let tokens = dirs.state.join("spyc/jar-tokens");
    std::fs::create_dir_all(&tokens).unwrap();
    std::fs::write(tokens.join(name), TOKEN).unwrap();
}

/// A running session's own jar, its token kept for `spyc attach`.
fn session(dirs: &Dirs, name: &str) -> Jar {
    let jar = Jar::start(dirs, session_socket(dirs, name));
    keep_token(dirs, name);
    jar.wait_for_spyc();
    jar
}

/// `spyc ARGS` on a PTY of the test's own: the user's terminal, answering
/// spyc's queries through the jar as a terminal would.
struct Attached {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    seen: Arc<Mutex<Vec<u8>>>,
    keys: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Attached {
    fn start(dirs: &Dirs, args: &[&str]) -> Self {
        use portable_pty::{CommandBuilder, PtySize, native_pty_system};
        use std::io::Read;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_spyc"));
        cmd.args(args);
        cmd.cwd(&dirs.project);
        cmd.env_clear();
        for (k, v) in dirs.env() {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let keys: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        {
            let (seen, keys) = (Arc::clone(&seen), Arc::clone(&keys));
            std::thread::spawn(move || {
                let _master = pair.master;
                let mut buf = [0u8; 8192];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let chunk = &buf[..n];
                    for (query, answer) in KITTY {
                        let asked = chunk.windows(query.len()).filter(|w| w == query).count();
                        for _ in 0..asked {
                            let _ = keys.lock().unwrap().write_all(answer);
                        }
                    }
                    seen.lock().unwrap().extend_from_slice(chunk);
                }
            });
        }
        Self { child, seen, keys }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }

    fn wait_for(&self, what: &str, needle: &str) {
        let until = Instant::now() + WAIT;
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < until,
                "never: {what}; the terminal showed:\n{:?}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Wait for spyc to have drawn `name` on its screen: after the
    /// alternate screen, so a mention before it (a list, a message) isn't
    /// taken for the frame.
    fn wait_drawn(&self, name: &str) {
        let until = Instant::now() + WAIT;
        loop {
            let text = self.text();
            if text
                .rfind(ALT_SCREEN)
                .is_some_and(|at| text[at..].contains(name))
            {
                return;
            }
            assert!(
                Instant::now() < until,
                "never drew {name}; the terminal showed:\n{text:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn type_keys(&self, keys: &[u8]) {
        let mut w = self.keys.lock().unwrap();
        w.write_all(keys).unwrap();
        w.flush().unwrap();
    }

    /// Wait for it to exit, and say how.
    fn exit(&mut self) -> portable_pty::ExitStatus {
        let until = Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < until,
                "never exited; the terminal showed:\n{:?}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Quit the spyc it shows, as the user would.
    fn quit_spyc(&mut self) {
        let keys = Arc::clone(&self.keys);
        let seen = Arc::clone(&self.seen);
        let child = &mut self.child;
        let mut status = None;
        quit_with_q(
            "spyc quits when told to",
            |typed| {
                let mut w = keys.lock().unwrap();
                // A key racing the exit meets a closed pty: nothing to tell.
                let _ = w.write_all(typed).and_then(|()| w.flush());
            },
            || String::from_utf8_lossy(&seen.lock().unwrap()).contains(QUIT_ASKS),
            || {
                status = child.try_wait().unwrap();
                status.is_some()
            },
        );
        assert!(status.is_some_and(|s| s.success()), "{:?}", self.text());
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn spyc_attach_shows_the_session_and_a_second_attach_takes_it_over() {
    let dirs = Dirs::new();
    let _jar = session(&dirs, NAME);

    let mut first = Attached::start(&dirs, &["attach", NAME]);
    first.wait_drawn(NAME);

    // Attached at a desk, then again from elsewhere: the second takes it.
    let mut second = Attached::start(&dirs, &["attach"]);
    second.wait_for("the takeover said", "taking it over");
    second.wait_drawn(NAME);
    let ended = first.exit();
    assert_eq!(ended.exit_code(), 128 + 15, "the first ended by SIGTERM");
    let shown = first.text();
    let last_frame = shown.rfind(NAME).unwrap();
    assert!(
        shown[last_frame..].contains("\x1b[?1049l"),
        "the first terminal left on spyc's screen: {:?}",
        &shown[last_frame..]
    );

    second.quit_spyc();
}

/// D3 of the 3.0 review: `spyc attach` looks at the session again after its
/// question and takes it over from the terminal attached then, not the one
/// attached when it asked, which may have gone and its pid with it.
#[test]
fn a_takeover_after_the_question_ends_the_client_attached_then() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    let _other = session(&dirs, OTHER);
    let mut desk = Attached::start(&dirs, &["attach", NAME]);
    desk.wait_drawn(NAME);
    let mut asking = Attached::start(&dirs, &["attach"]);
    asking.wait_for("the choice", "Attach to which?");
    let listed = asking.text();
    assert!(
        listed.contains(&format!("  1  {NAME}  (attached elsewhere)")),
        "{listed:?}"
    );

    // While it asks, another terminal takes the session from the desk.
    let mut elsewhere = Attached::start(&dirs, &["attach", NAME]);
    let _ = desk.exit();
    let pid = elsewhere.child.process_id().unwrap().to_string();
    eventually("the session's client is the one from elsewhere", || {
        jar.described("owner_pid").as_deref() == Some(pid.as_str())
    });

    asking.type_keys(b"1\r");
    asking.wait_drawn(NAME);
    let ended = elsewhere.exit();
    assert_eq!(
        ended.exit_code(),
        128 + 15,
        "the client attached then wasn't ended by SIGTERM"
    );
    asking.quit_spyc();
}

/// `^d` in a session reached by `spyc attach` ends the client: it attaches
/// again only to a jar that dropped it for silence, never one spyc detached.
#[test]
fn a_detach_in_spyc_ends_spyc_attach() {
    let dirs = Dirs::new();
    let _jar = session(&dirs, NAME);
    let mut term = Attached::start(&dirs, &["attach", NAME]);
    term.wait_drawn(NAME);
    term.type_keys(b"\x04");
    let status = term.exit();
    assert!(status.success(), "{:?}", term.text());
    assert!(
        term.text().ends_with("[detached]\r\n"),
        "the client didn't say it was detached: {:?}",
        term.text()
    );
}

/// Every session jar under `Dirs`, shut down when the test ends however it
/// ended: one a test made by detaching has no handle of its own.
struct EndSessions<'a>(&'a Dirs);

impl Drop for EndSessions<'_> {
    fn drop(&mut self) {
        let Ok(entries) = std::fs::read_dir(self.0.runtime.join("spyc/sessions")) else {
            return;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.extension().is_some_and(|x| x == "sock") {
                let name = path.file_stem().unwrap().to_string_lossy().into_owned();
                let _ = Command::new(env!("CARGO_BIN_EXE_spyc"))
                    .args(["shutdown", "-y", &name])
                    .env_clear()
                    .envs(self.0.env())
                    .stdin(Stdio::null())
                    .output();
            }
        }
    }
}

/// The first `^d` in a plain spyc frees the terminal as any other does: the
/// spyc it hands the session to runs on in its own jar with no terminal, and
/// `spyc attach` brings it back.
#[test]
fn the_first_detach_in_a_plain_spyc_frees_the_terminal() {
    let dirs = Dirs::new();
    let _sessions = EndSessions(&dirs);
    let mut plain = Attached::start(&dirs, &[]);
    plain.wait_drawn(&dirs.mark());
    plain.type_keys(b"\x04");
    let status = plain.exit();
    assert!(status.success(), "{:?}", plain.text());
    let sessions: Vec<PathBuf> = std::fs::read_dir(dirs.runtime.join("spyc/sessions"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sock"))
        .collect();
    assert_eq!(sessions.len(), 1, "one session jar: {sessions:?}");
    let name = sessions[0]
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        plain.text().contains(&format!("spyc attach {name}")),
        "the terminal wasn't told how to come back: {:?}",
        plain.text()
    );

    let mut back = Attached::start(&dirs, &["attach", &name]);
    back.wait_drawn(&dirs.mark());
    back.quit_spyc();
}

#[test]
fn a_bare_spyc_attach_asks_which_of_several_sessions() {
    let dirs = Dirs::new();
    let _first = session(&dirs, NAME);
    let _second = session(&dirs, OTHER);

    let mut terminal = Attached::start(&dirs, &["attach"]);
    terminal.wait_for("the choice", "Attach to which?");
    let listed = terminal.text();
    assert!(
        listed.contains(&format!("  1  {NAME}")) && listed.contains(&format!("  2  {OTHER}")),
        "{listed:?}"
    );
    terminal.type_keys(b"2\r");
    terminal.wait_drawn(OTHER);
    terminal.quit_spyc();
}

#[test]
fn spyc_attach_says_what_runs_when_no_session_matches() {
    let dirs = Dirs::new();
    let attach = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_spyc"))
            .args(args)
            .env_clear()
            .envs(dirs.env())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let said = attach(&["attach"]);
    assert!(
        said.contains("no session runs in a jar of its own"),
        "{said}"
    );

    let _jar = session(&dirs, NAME);
    let said = attach(&["attach", "SAGE"]);
    assert!(
        said.contains("no session named SAGE") && said.contains(NAME),
        "{said}"
    );
}

/// `spyc attach` from inside spyc, a pane or a shell a spyc started, would
/// show the session inside itself: it refuses, unless forced. A jar long gone
/// says nothing.
#[test]
fn spyc_attach_from_inside_spyc_is_refused_unless_forced() {
    let dirs = Dirs::new();
    let _jar = session(&dirs, NAME);
    let live = session_socket(&dirs, NAME);
    let gone = dirs.runtime.join("spyc/jars/gone.sock");
    let attach = |inside: (&str, &std::ffi::OsStr), args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_spyc"))
            .args(args)
            .env_clear()
            .envs(dirs.env())
            .env(inside.0, inside.1)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let pane = ("SPYC_PANE_ID", std::ffi::OsStr::new("a-pane"));
    let said = attach(pane, &["attach", NAME]);
    assert!(
        said.contains("runs in a spyc pane") && said.contains("--force"),
        "{said}"
    );
    let said = attach(("SPYC_JAR_SOCKET", live.as_os_str()), &["attach", NAME]);
    assert!(said.contains("runs in a spyc jar"), "{said}");
    // Past the check, to a session that doesn't run.
    let said = attach(pane, &["attach", "--force", "SAGE"]);
    assert!(said.contains("no session named SAGE"), "{said}");
    let said = attach(("SPYC_JAR_SOCKET", gone.as_os_str()), &["attach", "SAGE"]);
    assert!(said.contains("no session named SAGE"), "{said}");
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
    quit_with_q(
        "spyc quits when told to",
        |typed| second.type_keys(typed),
        || second.text().contains(QUIT_ASKS),
        || *second.exited.lock().unwrap(),
    );
}

/// D1 of the 3.0 review, in the session's own jar (#1328's rename): `:name`
/// refuses a name whose socket couldn't be bound, though this jar keeps its
/// own socket, since the name is the next jar's address after `spyc -r`.
#[test]
fn a_name_too_long_for_a_socket_is_refused_in_the_sessions_own_jar() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    let term = Terminal::attach(&jar, (30, 100), KITTY);
    term.wait_for("the session's name drawn", NAME);
    let log = dirs.state.join(format!("spyc/events/{NAME}.log"));
    let logged = |needle: &str| std::fs::read_to_string(&log).is_ok_and(|l| l.contains(needle));
    term.type_keys(format!(":name {}\r", "A".repeat(120)).as_bytes());
    eventually("spyc refuses the name", || logged("too long"));
    assert!(
        !session_socket(&dirs, NAME).with_extension("name").exists(),
        "the name was written beside the socket"
    );
    quit_with_q(
        "spyc quits when told to",
        |typed| term.type_keys(typed),
        || term.text().contains(QUIT_ASKS),
        || *term.exited.lock().unwrap(),
    );
}

/// The session's event log says why each terminal left spyc's own jar: one
/// whose connection closed, one that went silent past the jar's deadline, and
/// one spyc detached, which says nothing more.
#[test]
fn the_event_log_says_why_a_terminal_left() {
    let dirs = Dirs::new();
    let jar = Jar::start_with(
        &dirs,
        dirs.runtime
            .join("spyc/sessions")
            .join(format!("{NAME}.sock")),
        &["--deadline-ms", "2000"],
    );
    jar.wait_for_spyc();
    let log = dirs.state.join(format!("spyc/events/{NAME}.log"));
    let detaches = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            // What happened, before the tab that leads its place and ids.
            .map(|l| l.split('\t').next().unwrap_or(l))
            .filter(|l| l.contains("terminal detached"))
            .map(|l| l.split_once("terminal detached").unwrap().1.to_string())
            .collect::<Vec<_>>()
    };
    let next_detach = |seen: usize| {
        let until = Instant::now() + WAIT;
        loop {
            if let Some(line) = detaches().get(seen) {
                return line.clone();
            }
            assert!(
                Instant::now() < until,
                "no detach noted after {seen}: {:?}",
                std::fs::read_to_string(&log)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    let closing = Terminal::attach(&jar, (30, 100), PLAIN);
    closing.wait_for("the session drawn", NAME);
    closing.close();
    assert_eq!(next_detach(0), ", its connection closed: spyc runs on");

    // Drawn on, then nothing from it.
    let silent = Terminal::attach(&jar, (30, 100), PLAIN);
    silent.wait_for("the session drawn again", NAME);
    assert_eq!(
        next_detach(1),
        ", silent past the jar's deadline: spyc runs on"
    );
    assert_eq!(jar.described("owner_left").as_deref(), Some("silent"));

    let detached = Terminal::attach(&jar, (30, 100), PLAIN);
    detached.wait_for("the session drawn a third time", NAME);
    detached.type_keys(b"\x04");
    assert_eq!(next_detach(2), ": spyc runs on");
    eventually("the jar says spyc detached it", || {
        jar.described("owner_left").as_deref() == Some("detached")
    });
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

/// Where `spyc attach` finds a session's token (`keep_token`).
fn token_file(dirs: &Dirs, name: &str) -> PathBuf {
    dirs.state.join("spyc/jar-tokens").join(name)
}

/// A session shut down forgets its token (the 3.0 review, E7): once
/// `spyc shutdown` has seen it end, no token file is left for it.
#[test]
fn a_session_shut_down_forgets_its_token() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    assert!(
        token_file(&dirs, NAME).exists(),
        "premise: the session's token is kept"
    );
    // The socket file is there before spyc listens on it, and `spyc shutdown`
    // connects once: wait until it accepts.
    let spyc = jar.described("program_pid").expect("the session's spyc");
    let mcp = dirs.state.join(format!("spyc/mcp-{spyc}.sock"));
    eventually("spyc's MCP socket accepts", || {
        std::os::unix::net::UnixStream::connect(&mcp).is_ok()
    });
    let out = Command::new(env!("CARGO_BIN_EXE_spyc"))
        .args(["shutdown", NAME, "-y"])
        .env_clear()
        .envs(dirs.env())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(
        !token_file(&dirs, NAME).exists(),
        "the ended session's token is still kept"
    );
}

/// A session quit from the terminal attached to it forgets its token.
#[test]
fn a_session_quit_from_its_terminal_forgets_its_token() {
    let dirs = Dirs::new();
    let _jar = session(&dirs, NAME);
    let mut term = Attached::start(&dirs, &["attach", NAME]);
    term.wait_drawn(NAME);
    term.quit_spyc();
    assert!(
        !token_file(&dirs, NAME).exists(),
        "the ended session's token is still kept"
    );
}

/// A session quit from a terminal that isn't `spyc attach` (a raw client of
/// its jar) forgets its token by its own spyc, as it quits for good.
#[test]
fn a_session_quit_by_its_own_spyc_forgets_its_token() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    let terminal = Terminal::attach(&jar, (30, 100), PLAIN);
    terminal.wait_for("the session drawn", NAME);
    quit_with_q(
        "spyc quits when told to",
        |keys| terminal.type_keys(keys),
        || terminal.text().contains(QUIT_ASKS),
        || jar.described("exit").is_some() || !jar.socket.exists(),
    );
    assert!(
        !token_file(&dirs, NAME).exists(),
        "the ended session's token is still kept"
    );
}

/// A session whose spyc died while a terminal was attached forgets its token
/// once that terminal's `spyc attach` learns its jar holds only the exit.
#[test]
fn a_session_whose_spyc_died_forgets_its_token_through_its_terminal() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    let mut term = Attached::start(&dirs, &["attach", NAME]);
    term.wait_drawn(NAME);
    let spyc = jar
        .described("program_pid")
        .and_then(|p| p.parse().ok())
        .and_then(rustix::process::Pid::from_raw)
        .expect("the session's spyc");
    rustix::process::kill_process(spyc, rustix::process::Signal::KILL).unwrap();
    term.exit();
    assert!(
        !token_file(&dirs, NAME).exists(),
        "the ended session's token is still kept"
    );
}

/// Make `dir` open to everyone, as a directory another user made first
/// under `/tmp/spyc-<uid>` would be.
fn open_to_others(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o777)).unwrap();
}

/// `spyc attach` trusts no sessions directory another user could have made
/// or can write in (the 3.0 review, E2, and its addendum's P3-9): it says
/// why and attaches to nothing, so no token and no key leaves this user, and
/// it signals no pid it read there. The session here has a client attached,
/// whose pid its jar names, as a planted socket names a pid of its choosing:
/// that client must not be ended.
#[test]
fn spyc_attach_refuses_a_sessions_directory_open_to_others() {
    let dirs = Dirs::new();
    let _jar = session(&dirs, NAME);
    let mut first = Attached::start(&dirs, &["attach", NAME]);
    first.wait_drawn(NAME);
    open_to_others(&dirs.runtime.join("spyc/sessions"));
    let mut term = Attached::start(&dirs, &["attach", NAME]);
    let until = Instant::now() + WAIT;
    let status = loop {
        if let Some(ended) = first.child.try_wait().unwrap() {
            panic!(
                "the client named by a socket in a directory open to others was ended: {ended:?}; \
                 the second attach showed {:?}",
                term.text()
            );
        }
        if let Some(status) = term.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < until,
            "the second attach never ended: {:?}",
            term.text()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(!status.success(), "{:?}", term.text());
    assert!(term.text().contains("open to others"), "{:?}", term.text());
    assert!(
        first.child.try_wait().unwrap().is_none(),
        "the client attached before was ended"
    );
}

/// `spyc shutdown` asks no spyc named by a socket in a sessions directory
/// open to others (the 3.0 review, E2).
#[test]
fn spyc_shutdown_refuses_a_sessions_directory_open_to_others() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    open_to_others(&dirs.runtime.join("spyc/sessions"));
    let out = Command::new(env!("CARGO_BIN_EXE_spyc"))
        .args(["shutdown", NAME, "-y"])
        .env_clear()
        .envs(dirs.env())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{out:?}");
    assert!(said.contains("open to others"), "{said}");
    assert_eq!(jar.described("exit"), None, "the session ended");
}

/// `spyc --jars` lists no jar from a directory open to others, and says
/// why (the 3.0 review, E2).
#[test]
fn spyc_jars_refuses_a_directory_open_to_others() {
    let dirs = Dirs::new();
    let jar = Jar::start(&dirs, dirs.runtime.join("spyc/jars/0123456789abcdef.sock"));
    jar.wait_for_spyc();
    open_to_others(&dirs.runtime.join("spyc/jars"));
    let out = Command::new(env!("CARGO_BIN_EXE_spyc"))
        .arg("--jars")
        .env_clear()
        .envs(dirs.env())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(said.contains("open to others"), "{said}");
    assert!(!said.contains("0123456789abcdef"), "a jar listed: {said}");
}

/// Send the spyc of `pid` `method` with `params` on its MCP socket, as
/// another process of this user's can: its reply, for a request.
fn ask_spyc(
    dirs: &Dirs,
    pid: u32,
    method: &str,
    params: serde_json::Value,
    request: bool,
) -> Option<serde_json::Value> {
    use std::io::{BufRead, Read};
    let sock = dirs.state.join(format!("spyc/mcp-{pid}.sock"));
    // The socket file is there before spyc listens on it: connect until it
    // accepts.
    let mut stream = None;
    eventually("spyc's MCP socket accepts", || {
        stream = std::os::unix::net::UnixStream::connect(&sock).ok();
        stream.is_some()
    });
    let mut stream = stream.expect("connected");
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut msg = serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params });
    if request {
        msg["id"] = serde_json::json!(1);
    }
    let body = msg.to_string();
    write!(stream, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    if !request {
        return None;
    }
    let mut reader = std::io::BufReader::new(stream);
    let mut len = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length:") {
            len = n.trim().parse().unwrap();
        }
    }
    let mut reply = vec![0; len];
    reader.read_exact(&mut reply).unwrap();
    Some(serde_json::from_slice(&reply).unwrap())
}

/// The pid of the spyc a session's jar runs.
fn spyc_of(jar: &Jar) -> u32 {
    jar.described("program_pid")
        .and_then(|p| p.parse().ok())
        .expect("the session's spyc")
}

/// The session's token, as `spyc attach` and `spyc shutdown` read it.
fn token_text() -> String {
    String::from_utf8(TOKEN.to_vec()).unwrap()
}

/// `spyc/shutdown` ends a session only with the session's token (the 3.0
/// review, E1): any process of this user's reaches spyc's MCP socket, and
/// one request would otherwise end spyc and every tab.
#[test]
fn a_shutdown_without_the_sessions_token_is_refused() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    let spyc = spyc_of(&jar);
    for params in [
        serde_json::json!({}),
        serde_json::json!({ "token": "not-the-token" }),
    ] {
        let reply = ask_spyc(&dirs, spyc, "spyc/shutdown", params.clone(), true).unwrap();
        assert!(
            reply["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("token")),
            "{params}: {reply}"
        );
    }
    assert_eq!(jar.described("exit"), None, "the session ended");
    let reply = ask_spyc(
        &dirs,
        spyc,
        "spyc/shutdown",
        serde_json::json!({ "token": token_text() }),
        true,
    )
    .unwrap();
    assert_eq!(reply["result"], serde_json::json!({}), "{reply}");
    eventually("the session ends", || jar.described("exit").is_some());
}

/// `spyc/release` lets a session go only with the session's token (E1).
#[test]
fn a_release_without_the_sessions_token_is_refused() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    let reply = ask_spyc(
        &dirs,
        spyc_of(&jar),
        "spyc/release",
        serde_json::json!({}),
        true,
    )
    .unwrap();
    assert!(
        reply["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("token")),
        "{reply}"
    );
    assert_eq!(jar.described("exit"), None, "the session ended");
}

/// `spyc/upgrade` is acted on only with the session's token (E1), and is
/// answered either way (H7): without the token, refused for it; with it, by
/// what `:upgrade` needs, here a newer spyc this test never installs.
#[test]
fn an_upgrade_without_the_sessions_token_is_refused() {
    let dirs = Dirs::new();
    let jar = session(&dirs, NAME);
    let reply = ask_spyc(
        &dirs,
        spyc_of(&jar),
        "spyc/upgrade",
        serde_json::json!({}),
        true,
    )
    .unwrap();
    let why = reply["error"]["message"].as_str().unwrap_or("");
    assert!(
        why.contains("token") && !why.contains("no newer spyc"),
        "{reply}"
    );
    let reply = ask_spyc(
        &dirs,
        spyc_of(&jar),
        "spyc/upgrade",
        serde_json::json!({ "token": token_text() }),
        true,
    )
    .unwrap();
    let why = reply["error"]["message"].as_str().unwrap_or("");
    assert!(why.contains("no newer spyc"), "{reply}");
}

/// A spyc that runs in no session jar of its own takes no control request,
/// whatever it carries (E1).
#[test]
fn a_spyc_outside_a_session_jar_takes_no_control_request() {
    let dirs = Dirs::new();
    let mut plain = Attached::start(&dirs, &[]);
    plain.wait_drawn(&dirs.mark());
    let pid = plain.child.process_id().expect("spyc's pid");
    let reply = ask_spyc(
        &dirs,
        pid,
        "spyc/shutdown",
        serde_json::json!({ "token": token_text() }),
        true,
    )
    .unwrap();
    assert!(
        reply["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("no session jar")),
        "{reply}"
    );
    plain.quit_spyc();
}

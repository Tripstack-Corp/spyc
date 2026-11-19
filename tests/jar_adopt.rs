#![cfg(unix)]
//! `kill -9` of spyc, then `spyc -r`: a tab's program, running in its jar,
//! is taken back by the new spyc rather than started again (`[pane] jars`).
//! And a spyc with no usable jar offers to run its tabs without one.
//! Driven through the binary on a PTY of the test's own, with a stand-in
//! agent, and every directory the test's own.
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const WAIT: Duration = Duration::from_secs(30);

/// One spyc on a PTY: everything it prints, kept, and the terminal's answers
/// to its queries, given as a terminal would.
struct Spyc {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    seen: Arc<Mutex<Vec<u8>>>,
    keys: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Where it logs its standard error, one file a run.
    stderr: PathBuf,
}

/// The queries spyc makes of its terminal, and what this one answers.
const ANSWERS: &[(&[u8], &[u8])] = &[
    (b"\x1b[6n", b"\x1b[1;1R"),
    (b"\x1b[5n", b"\x1b[0n"),
    (b"\x1b[16t", b"\x1b[6;16;8t"),
    (b"\x1b[?u", b"\x1b[?0u"),
    (b"\x1b[c", b"\x1b[?62;22c"),
    (b"\x1b[0c", b"\x1b[?62;22c"),
];

impl Spyc {
    fn start(dirs: &Dirs, args: &[&str]) -> Self {
        Self::start_from(Path::new(env!("CARGO_BIN_EXE_spyc")), dirs, args)
    }

    /// [`Self::start`], running the spyc at `binary`.
    fn start_from(binary: &Path, dirs: &Dirs, args: &[&str]) -> Self {
        warm(binary);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(binary);
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
            // Kept reading until the PTY ends: a killed spyc's exit waits on
            // its terminal being drained.
            std::thread::spawn(move || {
                let _master = pair.master;
                let mut buf = [0u8; 8192];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let chunk = &buf[..n];
                    // Each query answered as often as it's asked: spyc's
                    // image probe asks twice and waits for both, and a probe
                    // left waiting takes the next key typed.
                    for (query, answer) in ANSWERS {
                        let asked = chunk.windows(query.len()).filter(|w| w == query).count();
                        for _ in 0..asked {
                            let _ = keys.lock().unwrap().write_all(answer);
                        }
                    }
                    seen.lock().unwrap().extend_from_slice(chunk);
                }
            });
        }
        Self {
            child,
            seen,
            keys,
            stderr: dirs.state.join("spyc").join("stderr"),
        }
    }

    fn pid(&self) -> i32 {
        i32::try_from(self.child.process_id().unwrap()).unwrap()
    }

    /// [`eventually`], saying what this spyc last drew when it never does.
    fn eventually(&self, what: &str, mut done: impl FnMut() -> bool) {
        let until = Instant::now() + WAIT;
        while !done() {
            if Instant::now() >= until {
                let seen = String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned();
                let at = seen.len().saturating_sub(3000);
                let at = (at..seen.len())
                    .find(|&i| seen.is_char_boundary(i))
                    .unwrap_or(0);
                let logs: String = std::fs::read_dir(&self.stderr)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
                    .collect();
                panic!(
                    "never: {what}; spyc's standard error:\n{logs}\nspyc last drew:\n{}",
                    &seen[at..]
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn shows(&self, needle: &str) -> bool {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).contains(needle)
    }

    fn type_keys(&self, keys: &[u8]) {
        let mut w = self.keys.lock().unwrap();
        w.write_all(keys).unwrap();
        w.flush().unwrap();
    }

    /// Whether it has exited.
    fn exited(&mut self) -> bool {
        self.child.try_wait().unwrap().is_some()
    }

    /// What it printed after the last time it entered the alternate screen.
    fn since_last_screen(&self) -> String {
        let seen = String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned();
        seen.rfind("\x1b[?1049h")
            .map_or_else(String::new, |at| seen[at..].to_string())
    }

    /// How many times it has entered the alternate screen.
    fn screens(&self) -> usize {
        String::from_utf8_lossy(&self.seen.lock().unwrap())
            .matches("\x1b[?1049h")
            .count()
    }

    /// `kill -9`, then reap it.
    fn crash(mut self) {
        kill(self.pid());
        let _ = self.child.wait();
    }
}

/// macOS scans an executable on its first run, which takes seconds for a
/// debug build this size, and longer while a gate runs every test at once;
/// every rebuild makes it new again. Run `binary` once, untimed, so the waits
/// below are spent on spyc rather than the scan: a gate's failures here were
/// spycs that had drawn nothing and logged nothing in 30 s.
fn warm(binary: &Path) {
    static WARMED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    let mut warmed = WARMED.lock().unwrap();
    if !warmed.iter().any(|b| b == binary) {
        let _ = std::process::Command::new(binary)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        warmed.push(binary.to_path_buf());
    }
}

fn kill(pid: i32) {
    if let Some(p) = rustix::process::Pid::from_raw(pid) {
        let _ = rustix::process::kill_process(p, rustix::process::Signal::KILL);
    }
}

fn alive(pid: i32) -> bool {
    rustix::process::Pid::from_raw(pid)
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < until, "never: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct Dirs {
    root: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    config: PathBuf,
    runtime: PathBuf,
    project: PathBuf,
    pidfile: PathBuf,
}

impl Dirs {
    /// A spyc's whole environment: every directory the test's own.
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

    /// What `spyc --jars` prints here.
    fn jars_listed(&self) -> String {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_spyc"))
            .arg("--jars")
            .env_clear()
            .envs(self.env())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap()
    }
}

/// Every jar still answering under the test's directories is ended with its
/// program, however the test ended: a pane's jar waits for an owner for
/// ever, holding a pty.
impl Drop for Dirs {
    fn drop(&mut self) {
        let mut dirs = vec![self.root.path().to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                use std::os::unix::fs::FileTypeExt;
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    dirs.push(entry.path());
                } else if kind.is_socket() {
                    let pid = |key| described(&entry.path(), key).and_then(|p| p.parse().ok());
                    if let Some(p) = pid("program_pid").and_then(rustix::process::Pid::from_raw) {
                        let _ =
                            rustix::process::kill_process_group(p, rustix::process::Signal::KILL);
                    }
                    if let Some(jar) = pid("pid") {
                        kill(jar);
                    }
                }
            }
        }
    }
}

fn dirs() -> Dirs {
    dirs_running("printf ready-mark; exec cat")
}

/// Directories for a spyc whose one tab notes its pid, then runs `rest`.
fn dirs_running(rest: &str) -> Dirs {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::Builder::new()
        .prefix("ja")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let at = |name: &str| {
        let d = root.path().join(name);
        std::fs::create_dir(&d).unwrap();
        d
    };
    let d = Dirs {
        home: at("h"),
        state: at("s"),
        config: at("c"),
        runtime: at("r"),
        project: at("p"),
        pidfile: root.path().join("agent.pid"),
        root,
    };
    std::fs::write(
        d.home.join(".spycrc.toml"),
        format!(
            "[pane]\njars = true\ntabs = [\"sh -c 'echo $$ > {}; {rest}'\"]\n",
            d.pidfile.display()
        ),
    )
    .unwrap();
    d
}

/// Make the saved tab whose jar is at `socket` a Claude tab in a named
/// conversation, so a restore that started it again would type
/// `/resume jar-thread` into it.
fn as_claude_tab(state: &Path, socket: &Path) {
    fn mark(v: &mut serde_json::Value, socket: &str) {
        match v {
            serde_json::Value::Object(o) => {
                if o.get("jar_socket").and_then(|s| s.as_str()) == Some(socket) {
                    o.insert("agent_kind".into(), "claude".into());
                    o.insert("agent_session_id".into(), "jar-thread".into());
                }
                o.values_mut().for_each(|v| mark(v, socket));
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|v| mark(v, socket)),
            _ => {}
        }
    }
    let sessions = state.join("spyc").join("sessions");
    for entry in std::fs::read_dir(sessions).unwrap() {
        let path = entry.unwrap().path();
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        mark(&mut v, &socket.display().to_string());
        std::fs::write(&path, serde_json::to_string(&v).unwrap()).unwrap();
    }
}

/// The `jar_socket` a saved session records, once one does.
fn saved_jar_socket(state: &Path) -> Option<PathBuf> {
    let sessions = state.join("spyc").join("sessions");
    std::fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .find_map(|text| {
            let at = text.find("\"jar_socket\"")?;
            let rest = &text[at + "\"jar_socket\"".len()..];
            let start = rest.find('"')? + 1;
            let end = start + rest[start..].find('"')?;
            Some(PathBuf::from(&rest[start..end]))
        })
}

fn described(socket: &Path, key: &str) -> Option<String> {
    spyc_jar::client::describe(socket)
        .ok()?
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// Ends whatever the test leaves running, even when it fails.
struct Leftovers(Vec<i32>);

impl Drop for Leftovers {
    fn drop(&mut self) {
        for pid in &self.0 {
            kill(*pid);
        }
    }
}

#[test]
fn spyc_r_after_a_kill_takes_the_running_program_back() {
    let dirs = dirs();
    let mut left = Leftovers(Vec::new());
    let first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || dirs.pidfile.exists());
    let agent: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    left.0.push(agent);
    eventually("the tab's output shows", || first.shows("ready-mark"));
    let mut socket = None;
    eventually("the session records the tab's jar", || {
        socket = saved_jar_socket(&dirs.state);
        socket.is_some()
    });
    let socket = socket.unwrap();
    let build = described(&socket, "build").unwrap_or_default();
    let hash = build.strip_prefix(&format!("{}-", spyc_jar::PACKAGE_VERSION));
    assert!(
        hash.is_some_and(|h| h.len() == 12 && h.bytes().all(|b| b.is_ascii_hexdigit())),
        "the pane's jar runs from a store entry, not {build:?}"
    );
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    first.crash();
    assert!(alive(agent), "the program died with spyc");
    as_claude_tab(&dirs.state, &socket);

    let second = Spyc::start(&dirs, &["-r"]);
    left.0.push(second.pid());
    eventually("the restore picker", || second.shows("Enter restore"));
    // Enter restores the newest session. A startup probe still waiting on
    // its answer may take a key typed meanwhile, so Enter goes again until
    // the restore shows; once restored, a stray one reaches the tab's `cat`.
    let mut typed: Option<Instant> = None;
    eventually("the new spyc owns the jar", || {
        if typed.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            second.type_keys(b"\r");
            typed = Some(Instant::now());
        }
        described(&socket, "owner_pid") == Some(second.pid().to_string())
    });
    eventually("the restored tab shows the program's output", || {
        second.shows("ready-mark")
    });
    assert!(alive(agent), "the program was ended");
    let now: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(now, agent, "the program was started again");
    // A restore types `/resume` into a Claude tab it starts once its banner
    // has settled (2 s); one it took back is still in its conversation.
    std::thread::sleep(Duration::from_secs(5));
    assert!(
        !second.shows("/resume"),
        "a restore typed /resume into the running agent"
    );
}

/// An agent's MCP proxy, `spyc --mcp`, run with the environment spyc gave the
/// agent in its tab: requests go in on its standard input, and each reply is
/// read from its output within the test's wait.
struct Proxy {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    replies: std::sync::mpsc::Receiver<String>,
}

impl Proxy {
    fn start(dirs: &Dirs, agent_env: &[(String, String)]) -> Self {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_spyc"))
            .arg("--mcp")
            .current_dir(&dirs.project)
            .env_clear()
            .envs(dirs.env())
            .envs(agent_env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, replies) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stdin,
            replies,
        }
    }

    fn send(&mut self, message: &str) {
        self.stdin.write_all(message.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
    }

    /// The reply to a request, or `None` when the proxy ended without one.
    fn reply(&self) -> Option<serde_json::Value> {
        let line = self.replies.recv_timeout(WAIT).ok()?;
        Some(serde_json::from_str(&line).unwrap())
    }

    /// The pid of the spyc that answers `get_spyc_context` as request `id`.
    fn answering_spyc(&mut self, id: u32) -> Result<i64, String> {
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"get_spyc_context\",\"arguments\":{{}}}}}}"
        ));
        let reply = self.reply().ok_or("the proxy ended without a reply")?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .ok_or_else(|| format!("no context in {reply}"))?;
        let context: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        context["pid"]
            .as_i64()
            .ok_or_else(|| format!("no pid in {context}"))
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// R30a: an agent's MCP proxy follows its pane's jar. After a `kill -9` of
/// spyc and `spyc -r`, the next tool call through the same proxy is answered
/// by the spyc that took the jar back: the proxy asks its jar where its owner's
/// MCP socket is now, rather than ending with the spyc that launched it. An
/// agent mid-turn keeps spyc's tools across a crash, and across the handover
/// `^d` and an upgrade make.
#[test]
fn an_agents_mcp_proxy_follows_its_jar_to_the_spyc_that_takes_it_back() {
    let probe = tempfile::NamedTempFile::new_in("/tmp").unwrap();
    let env_file = probe.path().to_path_buf();
    let dirs = dirs_running(&format!(
        "env > {}; printf ready-mark; exec cat",
        env_file.display()
    ));
    let mut left = Leftovers(Vec::new());
    let first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || dirs.pidfile.exists());
    eventually("the tab's output shows", || first.shows("ready-mark"));
    left.0.push(
        std::fs::read_to_string(&dirs.pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap(),
    );
    let agent_env: Vec<(String, String)> = std::fs::read_to_string(&env_file)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| matches!(*k, "SPYC_MCP_SOCK" | "SPYC_PANE_ID" | "SPYC_JAR_SOCKET"))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(
        agent_env.len(),
        3,
        "the tab's program sees its spyc, pane and jar: {agent_env:?}"
    );
    let mut socket = None;
    eventually("the session records the tab's jar", || {
        socket = saved_jar_socket(&dirs.state);
        socket.is_some()
    });
    let socket = socket.unwrap();
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }

    let mut proxy = Proxy::start(&dirs, &agent_env);
    proxy.send("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}");
    assert!(proxy.reply().is_some(), "the proxy initialized");
    proxy.send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");
    assert_eq!(proxy.answering_spyc(2), Ok(i64::from(first.pid())));

    let first_pid = first.pid();
    first.crash();
    let second = Spyc::start(&dirs, &["-r"]);
    left.0.push(second.pid());
    eventually("the restore picker", || second.shows("Enter restore"));
    let mut typed: Option<Instant> = None;
    eventually("the new spyc owns the jar", || {
        if typed.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            second.type_keys(b"\r");
            typed = Some(Instant::now());
        }
        described(&socket, "owner_pid") == Some(second.pid().to_string())
    });
    assert_ne!(first_pid, second.pid());
    assert_eq!(
        proxy.answering_spyc(3),
        Ok(i64::from(second.pid())),
        "the agent's next tool call reaches the spyc that took its jar back"
    );

    // The agent's status hook reaches it too, from a directory no spyc is
    // rooted at, where only the jar knows it.
    let hook = std::process::Command::new(env!("CARGO_BIN_EXE_spyc"))
        .args(["--report-status", "working", "--status-trace"])
        .current_dir(&dirs.home)
        .env_clear()
        .envs(dirs.env())
        .envs(agent_env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(hook.status.success(), "{hook:?}");
    let log = std::fs::read_to_string(dirs.state.join("spyc/mcp.log")).unwrap_or_default();
    assert!(
        log.contains("report-status: sent state=working"),
        "the hook's report reached no spyc: {}",
        log.lines()
            .filter(|l| l.contains("report-status"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The session jar under `dirs`, once one runs: its name and socket.
fn session_jar(dirs: &Dirs) -> Option<(String, PathBuf)> {
    let sessions = dirs.runtime.join("spyc/sessions");
    std::fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "sock") && described(p, "pid").is_some())
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), p))
}

/// S4: `^d` in a plain spyc hands it over to a spyc in its session's own jar,
/// and the terminal shows the same session: the same tab, its program never
/// restarted. A second `^d` leaves that spyc running with no terminal, and
/// `spyc attach` from another brings it back. `:q` quits it, ending its tabs.
#[test]
fn ctrl_d_hands_spyc_to_its_jar_and_spyc_attach_brings_it_back() {
    let dirs = dirs();
    let mut left = Leftovers(Vec::new());
    let mut first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || dirs.pidfile.exists());
    let agent: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    left.0.push(agent);
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
    let mut tab_socket = None;
    eventually("the session records the tab's jar", || {
        tab_socket = saved_jar_socket(&dirs.state);
        tab_socket.is_some()
    });
    let tab_socket = tab_socket.unwrap();
    if let Some(jar) = described(&tab_socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }

    // The handover: the same terminal, now a client of the session's jar.
    let screens = first.screens();
    first.type_keys(b"\x04");
    let mut session = None;
    first.eventually("a session jar", || {
        session = session_jar(&dirs);
        session.is_some()
    });
    let (name, socket) = session.unwrap();
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    first.eventually("the session drawn again, from its jar", || {
        first.screens() > screens && first.since_last_screen().contains(&name)
    });
    let jar_spyc = described(&socket, "program_pid").unwrap();
    left.0.push(jar_spyc.parse().unwrap());
    first.eventually("the jar's spyc owns the tab's jar", || {
        described(&tab_socket, "owner_pid").as_deref() == Some(jar_spyc.as_str())
    });
    first.eventually("the tab's output shows again", || {
        first.since_last_screen().contains("ready-mark")
    });
    assert!(!first.exited(), "the terminal's spyc became `spyc attach`");
    let now: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(now, agent, "the tab's program was started again");

    // A second `^d`: the terminal goes, reset, and spyc runs on.
    first.type_keys(b"\x04");
    eventually("the client leaves", || first.exited());
    let shown = String::from_utf8_lossy(&first.seen.lock().unwrap()).into_owned();
    let last = shown.rfind("\x1b[?1049h").unwrap();
    // The title's restore is spyc's own reset; the client's fixed one has none.
    assert!(
        shown[last..].contains("\x1b[23;0t") && shown[last..].contains("\x1b[?1049l"),
        "spyc didn't reset the terminal it left: {:?}",
        &shown[last..]
    );
    assert_eq!(
        described(&socket, "program_pid").as_deref(),
        Some(jar_spyc.as_str())
    );
    assert!(alive(agent), "the tab's program ended with the terminal");

    // From another terminal, the same spyc.
    let back = Spyc::start(&dirs, &["attach"]);
    back.eventually("the session drawn", || {
        back.shows(&name) && back.shows("ready-mark")
    });
    back.type_keys(b":q\r");
    back.eventually("spyc asks for a second `:q`", || {
        back.shows("press again to quit")
    });
    back.type_keys(b":q\r");
    eventually("quitting ends the tab's program", || !alive(agent));
}

/// `text` without its escape sequences or spaces: what a line says, however
/// the drawing moved the cursor between its words.
fn squeezed(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    for c in chars.by_ref() {
                        if c == '\x07' {
                            break;
                        }
                    }
                }
                _ => {}
            },
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

/// S4: `:upgrade` in spyc's own jar hands the session to a newly installed
/// spyc in place: the same jar and pid, the terminal still attached, the
/// session restored and its tab taken back, its program never restarted. With
/// nothing new installed it says so and does nothing.
#[test]
fn upgrade_in_a_session_jar_runs_the_new_spyc_with_the_same_tabs() {
    let dirs = dirs();
    // A spyc at a path of the test's own, which an install can replace.
    let bin = dirs.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let built = Path::new(env!("CARGO_BIN_EXE_spyc"));
    let spyc = bin.join("spyc");
    std::fs::copy(built, &spyc).unwrap();
    std::fs::copy(built.with_file_name("spyc-jar"), bin.join("spyc-jar")).unwrap();
    let mut left = Leftovers(Vec::new());
    let first = Spyc::start_from(&spyc, &dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || dirs.pidfile.exists());
    let agent: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    left.0.push(agent);
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
    let mut tab_socket = None;
    eventually("the session records the tab's jar", || {
        tab_socket = saved_jar_socket(&dirs.state);
        tab_socket.is_some()
    });
    let tab_socket = tab_socket.unwrap();
    if let Some(jar) = described(&tab_socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }

    // Into the session's jar.
    let screens = first.screens();
    first.type_keys(b"\x04");
    let mut session = None;
    first.eventually("a session jar", || {
        session = session_jar(&dirs);
        session.is_some()
    });
    let (name, socket) = session.unwrap();
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    first.eventually("the session drawn again, from its jar", || {
        first.screens() > screens && first.since_last_screen().contains(&name)
    });
    let jar_spyc = described(&socket, "program_pid").unwrap();
    left.0.push(jar_spyc.parse().unwrap());
    first.eventually("the jar's spyc owns the tab's jar", || {
        described(&tab_socket, "owner_pid").as_deref() == Some(jar_spyc.as_str())
    });

    // Nothing new installed: nothing happens.
    first.type_keys(b":upgrade\r");
    first.eventually("told no newer spyc is installed", || {
        squeezed(&first.since_last_screen()).contains("nonewerspycisinstalled")
    });

    // An install: a new file at the path, run once so macOS has scanned it.
    let next = bin.join("spyc.new");
    std::fs::copy(built, &next).unwrap();
    std::fs::rename(&next, &spyc).unwrap();
    let _ = std::process::Command::new(&spyc).arg("--version").output();
    let before = first.seen.lock().unwrap().len();
    let screens = first.screens();
    first.type_keys(b":upgrade\r");
    first.eventually("the new spyc draws the session", || {
        first.screens() > screens && first.since_last_screen().contains(&name)
    });
    // The old spyc left the terminal as it found it, then the new one took it.
    let since = String::from_utf8_lossy(&first.seen.lock().unwrap()[before..]).into_owned();
    let left_at = since
        .find("\x1b[?1049l")
        .expect("the old spyc restored the terminal");
    assert!(
        since[left_at..].contains("\x1b[?1049h"),
        "the new spyc set the terminal up"
    );
    assert_eq!(
        described(&socket, "program_pid").as_deref(),
        Some(jar_spyc.as_str()),
        "the upgrade kept the jar's program: the same pid"
    );
    first.eventually("the new spyc has the tab back", || {
        described(&tab_socket, "owner_pid").as_deref() == Some(jar_spyc.as_str())
            && first.since_last_screen().contains("ready-mark")
    });
    let now: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(now, agent, "the tab's program was started again");
    assert!(alive(agent));

    first.type_keys(b":q\r");
    first.eventually("spyc asks for a second `:q`", || {
        squeezed(&first.since_last_screen()).contains("pressagaintoquit")
    });
    first.type_keys(b":q\r");
    eventually("quitting ends the tab's program", || !alive(agent));
}

/// S4: `spyc attach` from an install the session's spyc hasn't taken offers
/// the upgrade, and a yes has that spyc `:upgrade` in place before the
/// terminal attaches. With nothing new installed it asks nothing.
#[test]
fn spyc_attach_from_a_newer_install_offers_the_upgrade() {
    let dirs = dirs();
    let bin = dirs.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let built = Path::new(env!("CARGO_BIN_EXE_spyc"));
    let spyc = bin.join("spyc");
    std::fs::copy(built, &spyc).unwrap();
    std::fs::copy(built.with_file_name("spyc-jar"), bin.join("spyc-jar")).unwrap();
    let mut left = Leftovers(Vec::new());
    let mut first = Spyc::start_from(&spyc, &dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || dirs.pidfile.exists());
    let agent: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    left.0.push(agent);
    first.eventually("the tab's output shows", || first.shows("ready-mark"));

    // Into the session's jar, then detached.
    let screens = first.screens();
    first.type_keys(b"\x04");
    let mut session = None;
    first.eventually("a session jar", || {
        session = session_jar(&dirs);
        session.is_some()
    });
    let (name, socket) = session.unwrap();
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    first.eventually("the session drawn again, from its jar", || {
        first.screens() > screens && first.since_last_screen().contains(&name)
    });
    let jar_spyc = described(&socket, "program_pid").unwrap();
    left.0.push(jar_spyc.parse().unwrap());
    first.type_keys(b"\x04");
    eventually("the client leaves", || first.exited());

    // Nothing new installed: it attaches without asking.
    let mut plain = Spyc::start_from(&spyc, &dirs, &["attach"]);
    plain.eventually("the session drawn", || {
        plain.since_last_screen().contains(&name)
    });
    assert!(
        !plain.shows("Upgrade it?"),
        "asked with nothing new installed"
    );
    plain.type_keys(b"\x04");
    eventually("the client leaves", || plain.exited());

    // An install the session's spyc hasn't taken. A start time is whole
    // seconds, so the install has to come more than a second after it.
    std::thread::sleep(Duration::from_secs(2));
    let next = bin.join("spyc.new");
    std::fs::copy(built, &next).unwrap();
    std::fs::rename(&next, &spyc).unwrap();
    let _ = std::process::Command::new(&spyc).arg("--version").output();
    let second = Spyc::start_from(&spyc, &dirs, &["attach"]);
    second.eventually("the offer", || second.shows("Upgrade it? [Y/n]"));
    second.type_keys(b"y\r");
    second.eventually("the session drawn", || {
        second.since_last_screen().contains(&name)
            && second.since_last_screen().contains("ready-mark")
    });
    assert_eq!(
        described(&socket, "program_pid").as_deref(),
        Some(jar_spyc.as_str()),
        "upgraded in place: the same pid"
    );
    // The session's spyc is the new one: there's nothing newer for it now.
    second.eventually("the new spyc has the tab back", || {
        described(&tab_jar(&dirs), "owner_pid").as_deref() == Some(jar_spyc.as_str())
    });
    second.type_keys(b":upgrade\r");
    second.eventually("told no newer spyc is installed", || {
        squeezed(&second.since_last_screen()).contains("nonewerspycisinstalled")
    });
    let now: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(now, agent, "the tab's program was started again");
    second.type_keys(b":q\r");
    second.eventually("spyc asks for a second `:q`", || {
        squeezed(&second.since_last_screen()).contains("pressagaintoquit")
    });
    second.type_keys(b":q\r");
    eventually("quitting ends the tab's program", || !alive(agent));
}

/// The tab's jar, as the saved session records it.
fn tab_jar(dirs: &Dirs) -> PathBuf {
    saved_jar_socket(&dirs.state).expect("the session records the tab's jar")
}

/// S2b: a spyc stopped past its jars' deadline loses their leases; resumed, it
/// takes its tabs back, the same program, and what it printed meanwhile shows,
/// since it fit the jar's ring.
#[test]
fn a_spyc_stopped_past_the_deadline_takes_its_tabs_back() {
    let go = std::env::temp_dir().join(format!("jar-go-{}", std::process::id()));
    let dirs = dirs_running(&format!(
        "printf ready-mark; while [ ! -e {} ]; do sleep 0.1; done; printf while-stopped-mark; exec cat",
        go.display()
    ));
    let mut left = Leftovers(Vec::new());
    let spyc = Spyc::start(&dirs, &[]);
    left.0.push(spyc.pid());
    spyc.eventually("the tab's program starts", || dirs.pidfile.exists());
    let agent: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    left.0.push(agent);
    eventually("the tab's output shows", || spyc.shows("ready-mark"));
    let mut socket = None;
    eventually("the session records the tab's jar", || {
        socket = saved_jar_socket(&dirs.state);
        socket.is_some()
    });
    let socket = socket.unwrap();
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    eventually("spyc owns the jar", || {
        described(&socket, "owner_pid") == Some(spyc.pid().to_string())
    });

    let pid = rustix::process::Pid::from_raw(spyc.pid()).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::STOP).unwrap();
    eventually("the jar ends a stopped spyc's lease", || {
        described(&socket, "owner").is_some_and(|o| o.starts_with("none"))
    });
    let before = printed(&socket);
    std::fs::write(&go, "").unwrap();
    eventually("the program prints while spyc is stopped", || {
        printed(&socket) > before
    });
    assert!(
        !spyc.shows("while-stopped-mark"),
        "premise: spyc was stopped"
    );
    rustix::process::kill_process(pid, rustix::process::Signal::CONT).unwrap();

    eventually("the resumed spyc owns the jar again", || {
        described(&socket, "owner_pid") == Some(spyc.pid().to_string())
    });
    eventually("what it printed meanwhile shows", || {
        spyc.shows("while-stopped-mark")
    });
    let _ = std::fs::remove_file(&go);
    assert!(alive(agent), "the program was ended");
    let now: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(now, agent, "the program was started again");
}

/// How many bytes the jar at `socket` says its program printed.
fn printed(socket: &Path) -> usize {
    described(socket, "log")
        .and_then(|l| {
            l.split(" printed")
                .next()
                .and_then(|s| s.rsplit(' ').next())
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(0)
}

/// The pids the tabs' programs noted in `pidfile`, one a line.
fn noted(pidfile: &Path) -> Vec<i32> {
    std::fs::read_to_string(pidfile)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

/// Every jar socket under `dir`.
fn sockets_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "sock") {
                found.push(p);
            }
        }
    }
    found
}

/// Tabs a `[pane] tabs` list starts together each get a jar of their own,
/// though ids made together share their leading digits (a v7 UUID's are its
/// millisecond and a counter): both programs run, and neither tab is left
/// `[not started]` by a jar that took the other's socket.
#[test]
fn startup_tabs_each_get_a_jar_of_their_own() {
    let dirs = dirs();
    let tab = format!("sh -c 'echo $$ >> {}; exec cat'", dirs.pidfile.display());
    std::fs::write(
        dirs.home.join(".spycrc.toml"),
        format!("[pane]\njars = true\ntabs = [\"{tab}\", \"{tab}\"]\n"),
    )
    .unwrap();
    let mut left = Leftovers(Vec::new());
    let spyc = Spyc::start(&dirs, &[]);
    left.0.push(spyc.pid());
    let refused = || spyc.shows("not started") || spyc.shows("wrong token");
    eventually("both tabs' programs run, or one is refused", || {
        noted(&dirs.pidfile).len() == 2 || refused()
    });
    left.0.extend(noted(&dirs.pidfile));
    for socket in sockets_under(&dirs.runtime) {
        if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
            left.0.push(jar);
        }
    }
    assert!(!refused(), "a tab was left not started");
    assert_eq!(noted(&dirs.pidfile).len(), 2, "both programs ran");
}

/// Jars are on by default; a spyc installed without its `spyc-jar` starts
/// no tab's program, says to check its installation, and offers to run tabs
/// without jars. Taken, the tab's program runs.
#[test]
fn a_spyc_with_no_spyc_jar_beside_it_offers_to_run_tabs_without_jars() {
    let dirs = dirs();
    std::fs::write(
        dirs.home.join(".spycrc.toml"),
        format!(
            "[pane]\ntabs = [\"sh -c 'echo $$ > {}; exec cat'\"]\n",
            dirs.pidfile.display()
        ),
    )
    .unwrap();
    // The spyc this build made, alone in a directory: linked, so macOS
    // doesn't scan a new copy on its first run.
    let alone = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let binary = alone.path().join("spyc");
    if std::fs::hard_link(env!("CARGO_BIN_EXE_spyc"), &binary).is_err() {
        std::fs::copy(env!("CARGO_BIN_EXE_spyc"), &binary).unwrap();
    }
    let mut left = Leftovers(Vec::new());
    let spyc = Spyc::start_from(&binary, &dirs, &[]);
    left.0.push(spyc.pid());
    eventually("spyc offers to run tabs without jars", || {
        spyc.shows("run without jars")
    });
    assert!(spyc.shows("installation"), "says to check the installation");
    assert!(!dirs.pidfile.exists(), "a program ran before the answer");
    spyc.type_keys(b"y");
    spyc.eventually("the tab's program runs", || {
        !noted(&dirs.pidfile).is_empty()
    });
    left.0.extend(noted(&dirs.pidfile));
    assert!(
        sockets_under(&dirs.runtime.join("spyc").join("jars")).is_empty(),
        "no jar was started"
    );
}

/// `spyc --jars` lists this user's jars by what each holds (R18): a tab's
/// by its label and project, with its owner; after a `kill -9` of spyc, with
/// none, so a jar no spyc takes back can be found.
#[test]
fn spyc_jars_lists_each_jar_by_its_tab_and_owner() {
    let dirs = dirs();
    let mut left = Leftovers(Vec::new());
    let spyc = Spyc::start(&dirs, &[]);
    left.0.push(spyc.pid());
    spyc.eventually("the tab's program runs", || {
        !noted(&dirs.pidfile).is_empty()
    });
    left.0.extend(noted(&dirs.pidfile));
    for socket in sockets_under(&dirs.runtime) {
        if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
            left.0.push(jar);
        }
    }
    eventually("--jars names the tab, its project and owner", || {
        let listed = dirs.jars_listed();
        listed.contains("pane, other in p)") && listed.contains("owner connected")
    });
    spyc.crash();
    eventually("--jars shows it has no owner", || {
        dirs.jars_listed().contains("owner none")
    });
}

/// The jar socket of the first capture or task a saved session records.
fn saved_task_socket(state: &Path) -> Option<PathBuf> {
    let sessions = state.join("spyc").join("sessions");
    std::fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .find_map(|v| Some(PathBuf::from(v["tasks"][0]["jar_socket"].as_str()?)))
}

/// S2c: a `!` capture running in a jar when spyc is killed comes back with
/// `spyc -r` as a background task: its jar taken back by the new spyc, the
/// same program still running, never started again.
#[test]
fn spyc_r_after_a_kill_takes_a_running_capture_back() {
    let dirs = dirs();
    std::fs::write(dirs.home.join(".spycrc.toml"), "[pane]\njars = true\n").unwrap();
    let mut left = Leftovers(Vec::new());
    let capture = format!(
        "!echo $$ >> {}; printf capture-mark; exec cat",
        dirs.pidfile.display()
    );
    let first = Spyc::start(&dirs, &["-c", &capture]);
    left.0.push(first.pid());
    eventually("the capture's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    let program = noted(&dirs.pidfile)[0];
    left.0.push(program);
    let mut socket = None;
    eventually("the session records the capture's jar", || {
        socket = saved_task_socket(&dirs.state);
        socket.is_some()
    });
    let socket = socket.unwrap();
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    first.crash();
    assert!(alive(program), "the capture's program died with spyc");

    let second = Spyc::start(&dirs, &["-r"]);
    left.0.push(second.pid());
    eventually("the restore picker", || second.shows("Enter restore"));
    let mut typed: Option<Instant> = None;
    eventually("the new spyc owns the capture's jar", || {
        if typed.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            second.type_keys(b"\r");
            typed = Some(Instant::now());
        }
        described(&socket, "owner_pid") == Some(second.pid().to_string())
    });
    assert!(alive(program), "the capture's program was ended");
    assert_eq!(
        noted(&dirs.pidfile),
        [program],
        "the program was started again"
    );
}

/// S2c: a tab's jar a `kill -9` of spyc left, which no `spyc -r` took back,
/// is offered at the next spyc's start; `r` takes it back, the same program
/// running, never started again.
#[test]
fn the_next_spyc_offers_a_jar_no_spyc_owns_and_takes_it_back() {
    let dirs = dirs();
    let mut left = Leftovers(Vec::new());
    let first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    eventually("the tab's program starts", || dirs.pidfile.exists());
    let program = noted(&dirs.pidfile)[0];
    left.0.push(program);
    eventually("the tab's output shows", || first.shows("ready-mark"));
    let socket = sockets_under(&dirs.runtime.join("spyc").join("jars"))
        .pop()
        .expect("the tab's jar");
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    eventually("the jar knows its tab", || {
        described(&socket, "is").as_deref() == Some("pane")
    });
    first.crash();

    std::fs::write(dirs.home.join(".spycrc.toml"), "[pane]\n").unwrap();
    let second = Spyc::start(&dirs, &[]);
    left.0.push(second.pid());
    eventually("the offer", || second.shows("no spyc owns"));
    second.type_keys(b"r");
    eventually("the new spyc owns the jar", || {
        described(&socket, "owner_pid") == Some(second.pid().to_string())
    });
    assert!(alive(program), "the program was ended");
    assert_eq!(
        noted(&dirs.pidfile),
        [program],
        "the program was started again"
    );
}

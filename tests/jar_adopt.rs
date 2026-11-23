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

    /// [`Self::start`], with `env` added to its environment.
    fn start_env(dirs: &Dirs, args: &[&str], env: &[(&str, &std::ffi::OsStr)]) -> Self {
        Self::start_from_env(Path::new(env!("CARGO_BIN_EXE_spyc")), dirs, args, env)
    }

    /// [`Self::start`], running the spyc at `binary`.
    fn start_from(binary: &Path, dirs: &Dirs, args: &[&str]) -> Self {
        Self::start_from_env(binary, dirs, args, &[])
    }

    fn start_from_env(
        binary: &Path,
        dirs: &Dirs,
        args: &[&str],
        extra: &[(&str, &std::ffi::OsStr)],
    ) -> Self {
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
        for (k, v) in extra {
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
    pidfile: PathBuf,
}

impl Dirs {
    /// A spyc's whole environment: every directory the test's own.
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
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
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
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
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

    // A report that names no pane reaches no spyc, though the jar knows one:
    // it would land on whatever tab is focused.
    let log_at = std::fs::read_to_string(dirs.state.join("spyc/mcp.log"))
        .unwrap_or_default()
        .len();
    let unnamed = std::process::Command::new(env!("CARGO_BIN_EXE_spyc"))
        .args(["--report-status", "working", "--status-trace"])
        .current_dir(&dirs.home)
        .env_clear()
        .envs(dirs.env())
        .envs(
            agent_env
                .iter()
                .filter(|(k, _)| k != "SPYC_PANE_ID")
                .map(|(k, v)| (k, v)),
        )
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(unnamed.status.success(), "{unnamed:?}");
    let log = std::fs::read_to_string(dirs.state.join("spyc/mcp.log")).unwrap_or_default();
    assert!(
        !log[log_at..].contains("report-status: sent"),
        "a report naming no pane reached a spyc: {}",
        &log[log_at..]
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
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
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
    back.eventually("spyc asks whether to quit", || {
        squeezed(&back.since_last_screen()).contains("quitanyway?[y/N]")
    });
    back.type_keys(b"y");
    eventually("quitting ends the tab's program", || !alive(agent));
}

/// [`dirs_running`], its one tab saying it hosts Claude (`[[pane.tab]] agent`),
/// so its status reports count.
fn dirs_with_agent(rest: &str) -> Dirs {
    let d = dirs_running(rest);
    std::fs::write(
        d.home.join(".spycrc.toml"),
        format!(
            "[pane]\njars = true\n\n[[pane.tab]]\ncommand = \"sh -c 'echo $$ > {}; {rest}'\"\nagent = \"claude\"\n",
            d.pidfile.display()
        ),
    )
    .unwrap();
    d
}

/// S5: `:detach project NAME` moves a project into a session of its own. Its
/// tab's program runs on, taken back by the spyc the new session's jar
/// starts, and the spyc it left runs on without it. The agent's MCP proxy
/// follows its jar there, its scope claim goes with it, and so does the
/// `blocked` it reported before the move, which it won't report again. Then
/// `:attach project NAME` brings it all back, the session's spyc gone and its
/// file with it.
#[test]
fn detach_project_moves_a_project_and_its_agent_into_a_session_of_its_own() {
    let probe = tempfile::NamedTempFile::new_in("/tmp").unwrap();
    let env_file = probe.path().to_path_buf();
    let dirs = dirs_with_agent(&format!(
        "env > {}; printf ready-mark; exec cat",
        env_file.display()
    ));
    let elsewhere = dirs.root.path().join("elsewhere-proj");
    std::fs::create_dir(&elsewhere).unwrap();
    let mut left = Leftovers(Vec::new());
    let first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
    // spyc asks once whether to wire Claude up; no.
    first.eventually("the agent-wiring question", || {
        squeezed(&first.since_last_screen()).contains("[y]yes[n]no")
    });
    first.type_keys(b"n");
    let agent = noted(&dirs.pidfile)[0];
    left.0.push(agent);
    let mut tab_socket = None;
    eventually("the session records the tab's jar", || {
        tab_socket = saved_jar_socket(&dirs.state);
        tab_socket.is_some()
    });
    let tab_socket = tab_socket.unwrap();
    if let Some(jar) = described(&tab_socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    let agent_env: Vec<(String, String)> = std::fs::read_to_string(&env_file)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| matches!(*k, "SPYC_MCP_SOCK" | "SPYC_PANE_ID" | "SPYC_JAR_SOCKET"))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // The agent claims a file and blocks on a prompt.
    let mut proxy = Proxy::start(&dirs, &agent_env);
    proxy.send("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}");
    assert!(proxy.reply().is_some(), "the proxy initialized");
    proxy.send("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");
    proxy.send(
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"register_scope\",\"arguments\":{\"paths\":[\"src/moved.rs\"],\"intent\":\"editing\"}}}",
    );
    let claimed = proxy.reply().unwrap();
    assert!(claimed.get("error").is_none(), "{claimed}");
    let blocked = std::process::Command::new(env!("CARGO_BIN_EXE_spyc"))
        .args(["--report-status", "blocked"])
        .current_dir(&dirs.home)
        .env_clear()
        .envs(dirs.env())
        .envs(agent_env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(blocked.status.success(), "{blocked:?}");

    // A second project, then the first detached by its name.
    first.type_keys(b" n");
    first.eventually("the open-project prompt", || first.shows("open project:"));
    // The prompt starts at the directory in view: cleared first.
    first.type_keys(format!("\x15{}\r", elsewhere.display()).as_bytes());
    first.eventually("the second project opens", || {
        std::fs::read_dir(dirs.state.join("spyc"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".spyc-context-")
            })
            .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| t.contains("elsewhere-proj")))
    });
    let project = dirs
        .project
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    first.type_keys(format!(":detach project {project}\r").as_bytes());
    let mut session = None;
    first.eventually("a session jar", || {
        session = session_jar(&dirs);
        session.is_some()
    });
    let (name, socket) = session.unwrap();
    if let Some(jar) = described(&socket, "pid").and_then(|p| p.parse().ok()) {
        left.0.push(jar);
    }
    let mut moved_spyc = None;
    first.eventually("the session's spyc runs", || {
        moved_spyc = described(&socket, "program_pid");
        moved_spyc.is_some()
    });
    let moved_spyc = moved_spyc.unwrap();
    left.0.push(moved_spyc.parse().unwrap());
    first.eventually("the session's spyc owns the tab's jar", || {
        described(&tab_socket, "owner_pid").as_deref() == Some(moved_spyc.as_str())
    });
    first.eventually("the spyc it left says where it went", || {
        squeezed(&first.since_last_screen()).contains(&format!("runsonassession{name}"))
    });
    assert!(alive(agent), "the tab's program ended with the move");
    assert_eq!(
        described(&tab_socket, "program_pid"),
        Some(agent.to_string()),
        "the tab's program was started again"
    );
    assert!(alive(first.pid()), "the spyc it left runs on");

    // The agent's next tool call reaches the new spyc, which has its claim.
    assert_eq!(
        proxy.answering_spyc(3),
        Ok(moved_spyc.parse::<i64>().unwrap()),
        "the agent's MCP followed its jar"
    );
    proxy.send(
        "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"list_scopes\",\"arguments\":{}}}",
    );
    let scopes = proxy.reply().unwrap().to_string();
    assert!(
        scopes.contains("src/moved.rs"),
        "the claim stayed behind: {scopes}"
    );

    // And it knows the agent is blocked, though the agent said so only before.
    let back = Spyc::start(&dirs, &["attach", &name]);
    left.0.push(back.pid());
    back.eventually("the session drawn, its tab with it", || {
        let shown = squeezed(&back.since_last_screen());
        shown.contains(&name) && shown.contains("ready-mark")
    });
    back.type_keys(b":activity dump\r");
    back.eventually("the tab's dot is blocked", || {
        squeezed(&back.since_last_screen()).contains("dot=blocked")
    });

    // And back: `:attach` lists the session, and `:attach project NAME`
    // brings its project home, its tab, claim and blocked agent with it.
    first.type_keys(b":attach\r");
    first.eventually("the session listed, attached", || {
        squeezed(&first.since_last_screen()).contains(&format!("{name}attached"))
    });
    first.type_keys(b"q");
    first.type_keys(format!(":attach project {name}\r").as_bytes());
    let first_pid = first.pid().to_string();
    first.eventually("this spyc owns the tab's jar again", || {
        described(&tab_socket, "owner_pid").as_deref() == Some(first_pid.as_str())
    });
    let moved_pid: i32 = moved_spyc.parse().unwrap();
    eventually("the session's spyc has gone", || !alive(moved_pid));
    assert_eq!(
        described(&tab_socket, "program_pid"),
        Some(agent.to_string()),
        "the tab's program was started again"
    );
    assert_eq!(
        proxy.answering_spyc(5),
        Ok(i64::from(first.pid())),
        "the agent's MCP followed its jar home"
    );
    proxy.send(
        "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"list_scopes\",\"arguments\":{}}}",
    );
    let scopes = proxy.reply().unwrap().to_string();
    assert!(
        scopes.contains("src/moved.rs"),
        "the claim stayed behind: {scopes}"
    );
    let dumped = squeezed(&first.since_last_screen())
        .matches("dot=blocked")
        .count();
    first.type_keys(b":activity dump\r");
    first.eventually("the tab's dot is blocked here too", || {
        squeezed(&first.since_last_screen())
            .matches("dot=blocked")
            .count()
            > dumped
    });
    let named = format!("\"name\": \"{name}\"");
    let left_behind: Vec<_> = std::fs::read_dir(dirs.state.join("spyc/sessions"))
        .unwrap()
        .flatten()
        .filter(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| t.contains(&named)))
        .map(|e| e.path())
        .collect();
    assert!(
        left_behind.is_empty(),
        "{name}'s file stayed: {left_behind:?}"
    );
}

/// #904: a tab reaches the SSH agent of the terminal its spyc has now. It is
/// given a link of its own as its `SSH_AUTH_SOCK`, and after `^d` and a
/// `spyc attach` from a login with another agent, the same link, in the same
/// running program's environment, reaches that one.
#[test]
fn a_tabs_ssh_agent_follows_the_terminal_that_attaches() {
    use std::os::unix::fs::PermissionsExt;
    let probe = tempfile::NamedTempFile::new_in("/tmp").unwrap();
    let seen = probe.path().to_path_buf();
    let dirs = dirs_running(&format!(
        "echo $SSH_AUTH_SOCK > {}; printf ready-mark; exec cat",
        seen.display()
    ));
    let agents = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let (a, b) = (agents.path().join("a"), agents.path().join("b"));
    let _first_agent = std::os::unix::net::UnixListener::bind(&a).unwrap();
    let second_agent = std::os::unix::net::UnixListener::bind(&b).unwrap();
    let real = |p: &Path| std::fs::canonicalize(p).ok();
    let mut left = Leftovers(Vec::new());

    let mut first = Spyc::start_env(&dirs, &[], &[("SSH_AUTH_SOCK", a.as_os_str())]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    left.0.push(noted(&dirs.pidfile)[0]);
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
    let tab_agent = PathBuf::from(std::fs::read_to_string(&seen).unwrap().trim());
    assert_ne!(
        tab_agent, a,
        "the tab was given the agent, not a link to it"
    );
    assert_eq!(real(&tab_agent), real(&a));
    let mut tab_socket = None;
    eventually("the session records the tab's jar", || {
        tab_socket = saved_jar_socket(&dirs.state);
        tab_socket.is_some()
    });
    let tab_socket = tab_socket.unwrap();

    // `^d` into the session's own jar, then `^d` again: the terminal goes.
    first.type_keys(b"\x04");
    let mut session = None;
    first.eventually("a session jar", || {
        session = session_jar(&dirs);
        session.is_some()
    });
    let (name, socket) = session.unwrap();
    first.eventually("the session drawn again, from its jar", || {
        let shown = first.since_last_screen();
        shown.contains(&name) && shown.contains("ready-mark")
    });
    let jar_spyc = described(&socket, "program_pid").unwrap();
    left.0.push(jar_spyc.parse().unwrap());
    first.eventually("the session's spyc owns the tab's jar", || {
        described(&tab_socket, "owner_pid").as_deref() == Some(jar_spyc.as_str())
    });
    first.type_keys(b"\x04");
    eventually("the terminal leaves", || first.exited());

    // From another login, with another agent.
    let back = Spyc::start_env(&dirs, &["attach"], &[("SSH_AUTH_SOCK", b.as_os_str())]);
    left.0.push(back.pid());
    back.eventually("the session drawn", || {
        squeezed(&back.since_last_screen()).contains("ready-mark")
    });
    eventually("the tab's link reaches the new agent", || {
        real(&tab_agent) == real(&b)
    });
    std::os::unix::net::UnixStream::connect(&tab_agent).unwrap();
    second_agent.set_nonblocking(true).unwrap();
    assert!(
        second_agent.accept().is_ok(),
        "the tab's link reached another agent"
    );
}

/// #904: in its own jar, spyc sends what goes to a terminal by the terminal
/// attached now, not the one it started in. A session started locally and
/// attached from an SSH login copies a path back through that terminal (OSC
/// 52), as a spyc started over SSH does, rather than into a clipboard on a
/// machine the user isn't at.
#[test]
fn a_session_attached_over_ssh_copies_through_the_terminal() {
    let dirs = dirs();
    std::fs::write(dirs.project.join("copied-file.txt"), "").unwrap();
    let mut left = Leftovers(Vec::new());
    let mut first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    left.0.push(noted(&dirs.pidfile)[0]);
    first.eventually("the tab's output shows", || first.shows("ready-mark"));

    // `^d` into the session's own jar, then `^d` again: the terminal goes.
    let screens = first.screens();
    first.type_keys(b"\x04");
    let mut session = None;
    first.eventually("a session jar", || {
        session = session_jar(&dirs);
        session.is_some()
    });
    let (name, socket) = session.unwrap();
    // A new screen, drawn from the jar: the one before `^d` showed both too.
    first.eventually("the session drawn again, from its jar", || {
        let shown = first.since_last_screen();
        first.screens() > screens && shown.contains(&name) && shown.contains("ready-mark")
    });
    // The jar reports 0 while it holds its program for the first owner.
    let mut jar_spyc = None;
    first.eventually("the session's spyc runs", || {
        jar_spyc = described(&socket, "program_pid").filter(|p| p != "0");
        jar_spyc.is_some()
    });
    left.0.push(jar_spyc.unwrap().parse().unwrap());
    first.type_keys(b"\x04");
    eventually("the terminal leaves", || first.exited());

    let ssh = std::ffi::OsStr::new("192.0.2.1 50000 192.0.2.2 22");
    let back = Spyc::start_env(&dirs, &["attach"], &[("SSH_CONNECTION", ssh)]);
    left.0.push(back.pid());
    back.eventually("the session drawn", || {
        squeezed(&back.since_last_screen()).contains("copied-file.txt")
    });
    back.type_keys(b"\x01k");
    std::thread::sleep(Duration::from_millis(300));
    back.type_keys(b"yf");
    back.eventually("the path comes back as OSC 52", || {
        back.since_last_screen().contains("\x1b]52;")
    });
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
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
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
    first.eventually("spyc asks whether to quit", || {
        squeezed(&first.since_last_screen()).contains("quitanyway?[y/N]")
    });
    first.type_keys(b"y");
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
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
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

    // Upgraded in place, the spyc keeps its pid and so its start time: the
    // next attach asks nothing, for there's nothing newer.
    let mut second = second;
    second.type_keys(b"\x04");
    eventually("the client leaves", || second.exited());
    let third = Spyc::start_from(&spyc, &dirs, &["attach"]);
    third.eventually("the session drawn", || {
        third.since_last_screen().contains(&name)
    });
    assert!(
        !third.shows("Upgrade it?"),
        "offered the upgrade the session's spyc already took"
    );
    third.type_keys(b":q\r");
    third.eventually("spyc asks whether to quit", || {
        squeezed(&third.since_last_screen()).contains("quitanyway?[y/N]")
    });
    third.type_keys(b"y");
    eventually("quitting ends the tab's program", || !alive(agent));
}

/// `spyc shutdown` ends a session as `:q` would, its tabs with it, from
/// outside it. It names what ends and asks first, and a no leaves it running;
/// `-y` doesn't ask.
#[test]
fn spyc_shutdown_ends_a_detached_session_and_its_tabs() {
    let dirs = dirs();
    let mut left = Leftovers(Vec::new());
    let mut first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    let agent: i32 = std::fs::read_to_string(&dirs.pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    left.0.push(agent);
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
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
    let jar_spyc: i32 = described(&socket, "program_pid").unwrap().parse().unwrap();
    left.0.push(jar_spyc);
    first.eventually("the jar's spyc owns the tab's jar", || {
        described(&tab_jar(&dirs), "owner_pid") == Some(jar_spyc.to_string())
    });
    first.type_keys(b"\x04");
    eventually("the client leaves", || first.exited());

    let shutdown = |args: &[&str], answer: &str| {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_spyc"))
            .arg("shutdown")
            .args(args)
            .current_dir(&dirs.home)
            .env_clear()
            .envs(dirs.env())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(answer.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), text)
    };

    // Named, and answered no: it says what would end, and nothing does.
    let (_, said) = shutdown(&[&name], "n\n");
    assert!(
        said.contains(&name) && said.contains("[y/N]") && said.contains("1 tab"),
        "it names the session and its tab, and asks: {said}"
    );
    assert!(alive(agent) && alive(jar_spyc), "a no ended something");
    assert_eq!(
        described(&socket, "program_pid"),
        Some(jar_spyc.to_string()),
        "a no left the session running"
    );

    // The only session, without asking.
    let (ok, said) = shutdown(&["-y"], "");
    assert!(ok, "{said}");
    assert!(!said.contains("[y/N]"), "`-y` asked: {said}");
    eventually("the session's spyc ends", || !alive(jar_spyc));
    eventually("its tab's program ends", || !alive(agent));
    // Its jar keeps the exit a while for an owner, and runs nothing.
    eventually("its jar holds only the exit", || {
        described(&socket, "pid").is_none() || described(&socket, "exit").is_some()
    });
    let (ok, said) = shutdown(&["-y"], "");
    assert!(
        !ok && said.contains("no session runs"),
        "with none left it says so: {said}"
    );
}

/// S3: agents started on one install run on under the next. A spyc started
/// after an install takes each tab's jar back and upgrades it to its own
/// build: the same jar and the same program, now running the new build.
#[test]
fn spyc_upgrades_the_jars_it_takes_back_to_its_own_build() {
    let dirs = dirs();
    let bin = dirs.root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let built = Path::new(env!("CARGO_BIN_EXE_spyc"));
    let spyc = bin.join("spyc");
    std::fs::copy(built, &spyc).unwrap();
    // Two builds of the jar: bytes past the end change its hash, so its
    // store entry and the build it names, and nothing it does.
    let install_jar = |mark: &[u8]| {
        let next = bin.join("spyc-jar.new");
        std::fs::copy(built.with_file_name("spyc-jar"), &next).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&next)
            .unwrap()
            .write_all(mark)
            .unwrap();
        std::fs::rename(&next, bin.join("spyc-jar")).unwrap();
    };
    install_jar(b"build-a");
    let mut left = Leftovers(Vec::new());
    let first = Spyc::start_from(&spyc, &dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    let agent = noted(&dirs.pidfile)[0];
    left.0.push(agent);
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
    eventually("the session records the tab's jar", || {
        saved_jar_socket(&dirs.state).is_some()
    });
    let socket = tab_jar(&dirs);
    let jar: i32 = described(&socket, "pid").unwrap().parse().unwrap();
    left.0.push(jar);
    let a = described(&socket, "build").unwrap();
    assert!(!a.is_empty(), "the jar runs from the store");

    first.crash();
    install_jar(b"build-b");
    let second = Spyc::start_from(&spyc, &dirs, &["-r"]);
    left.0.push(second.pid());
    second.eventually("the restore picker", || second.shows("Enter restore"));
    let mut typed: Option<Instant> = None;
    second.eventually("the new spyc owns the jar", || {
        if typed.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            second.type_keys(b"\r");
            typed = Some(Instant::now());
        }
        described(&socket, "owner_pid") == Some(second.pid().to_string())
    });
    let mut now = None;
    second.eventually("the tab's jar runs the new build", || {
        now = described(&socket, "build").filter(|b| !b.is_empty() && *b != a);
        now.is_some()
    });
    assert_eq!(
        described(&socket, "pid"),
        Some(jar.to_string()),
        "upgraded in place: the same jar"
    );
    assert_eq!(
        described(&socket, "program_pid"),
        Some(agent.to_string()),
        "the same program, never restarted"
    );
    assert!(alive(agent), "the program ran on");
    assert_eq!(
        described(&socket, "owner_pid"),
        Some(second.pid().to_string()),
        "spyc still owns it"
    );
    let now = now.unwrap();
    assert!(
        dirs.jars_listed().contains(&format!("build {now}")),
        "`spyc --jars` shows the new build: {}",
        dirs.jars_listed()
    );
}

/// `:events`: a flash gone from the status row is in the session's event
/// log, on disk as it happens, and the next spyc on the session shows it.
#[test]
fn a_flash_is_in_the_sessions_event_log_and_outlives_spyc() {
    let dirs = dirs();
    let mut left = Leftovers(Vec::new());
    let first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    left.0.extend(noted(&dirs.pidfile));
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
    first.type_keys(b":upgrade\r");
    first.eventually("the flash", || {
        squeezed(&first.since_last_screen()).contains("nonewerspycisinstalled")
    });
    let events = dirs.state.join("spyc/events");
    let logged = || {
        std::fs::read_dir(&events)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
            .collect::<String>()
    };
    eventually("the session's file has the flash", || {
        logged().contains("upgrade: no newer spyc is installed")
    });
    assert!(logged().contains("started, pid"), "{}", logged());

    // `spyc events NAME` prints it from a shell; `-n 1` only the last line;
    // `-f` goes on with what comes after.
    let name = std::fs::read_dir(&events)
        .unwrap()
        .flatten()
        .find_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap();
    let events_cmd = |args: &[&str]| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_spyc"));
        cmd.arg("events")
            .args(args)
            .current_dir(&dirs.home)
            .env_clear()
            .envs(dirs.env());
        cmd
    };
    let printed = events_cmd(&[&name]).output().unwrap();
    let printed = String::from_utf8_lossy(&printed.stdout).into_owned();
    assert!(
        printed.contains("upgrade: no newer spyc is installed") && printed.contains("started, pid"),
        "{printed}"
    );
    let last = events_cmd(&[&name, "-n", "1"]).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&last.stdout).lines().count(),
        1,
        "{last:?}"
    );
    let mut follow = events_cmd(&[&name, "-f"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    left.0.push(i32::try_from(follow.id()).unwrap());
    let mut out = follow.stdout.take().unwrap();
    let (more_tx, more) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = std::io::Read::read(&mut out, &mut buf) {
            if n == 0 {
                break;
            }
            let _ = more_tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
        }
    });
    first.type_keys(b":whoami\r");
    let mut seen = String::new();
    let until = Instant::now() + WAIT;
    while !seen.contains('@') {
        assert!(
            Instant::now() < until,
            "`-f` never printed the new flash: {seen}"
        );
        if let Ok(chunk) = more.recv_timeout(Duration::from_millis(100)) {
            seen.push_str(&chunk);
        }
    }
    let _ = follow.kill();
    let _ = follow.wait();
    eventually("the session is saved", || {
        saved_jar_socket(&dirs.state).is_some()
    });

    first.crash();
    let second = Spyc::start(&dirs, &["-r"]);
    left.0.push(second.pid());
    second.eventually("the restore picker", || second.shows("Enter restore"));
    second.type_keys(b"\r");
    second.eventually("the session restored", || second.shows("ready-mark"));
    second.type_keys(b":events\r");
    second.eventually("the last spyc's flash in `:events`", || {
        squeezed(&second.since_last_screen()).contains("upgrade:nonewerspycisinstalled")
    });
}

/// The session's event log follows it through `^d`'s handover into its own
/// jar and out through `:q`, each spyc's events written before it goes.
#[test]
fn the_event_log_follows_a_session_through_a_handover_and_a_quit() {
    let dirs = dirs();
    let mut left = Leftovers(Vec::new());
    let mut first = Spyc::start(&dirs, &[]);
    left.0.push(first.pid());
    first.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
    left.0.extend(noted(&dirs.pidfile));
    first.eventually("the tab's output shows", || first.shows("ready-mark"));
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
    if let Some(spyc) = described(&socket, "program_pid").and_then(|p| p.parse().ok()) {
        left.0.push(spyc);
    }
    first.type_keys(b":q\r");
    first.eventually("spyc asks whether to quit", || {
        squeezed(&first.since_last_screen()).contains("quitanyway?[y/N]")
    });
    first.type_keys(b"y");
    eventually("the client leaves", || first.exited());
    let log = dirs.state.join(format!("spyc/events/{name}.log"));
    let wanted = [
        "started, pid",
        "detached: spyc moves into",
        "started, pid",
        "terminal attached",
        "quit: the session saved",
    ];
    let in_order = |text: &str| {
        let mut rest = text;
        wanted.iter().all(|w| match rest.find(w) {
            Some(at) => {
                rest = &rest[at + w.len()..];
                true
            }
            None => false,
        })
    };
    let until = Instant::now() + WAIT;
    let mut text = String::new();
    while Instant::now() < until {
        text = std::fs::read_to_string(&log).unwrap_or_default();
        if in_order(&text) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the session's log, not in order: {text}");
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
    spyc.eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
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
    eventually("the tab's program starts", || {
        !noted(&dirs.pidfile).is_empty()
    });
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

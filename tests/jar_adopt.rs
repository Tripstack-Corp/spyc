#![cfg(unix)]
//! `kill -9` of spyc, then `spyc -r`: a tab's program, running in its jar,
//! is taken back by the new spyc rather than started again (`[pane] jars`).
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
        for (k, v) in [
            ("HOME", dirs.home.as_os_str()),
            ("XDG_STATE_HOME", dirs.state.as_os_str()),
            ("XDG_CONFIG_HOME", dirs.config.as_os_str()),
            ("XDG_RUNTIME_DIR", dirs.runtime.as_os_str()),
            ("TMPDIR", dirs.runtime.as_os_str()),
            ("PATH", "/usr/bin:/bin".as_ref()),
            ("SHELL", "/bin/sh".as_ref()),
            ("TERM", "xterm-256color".as_ref()),
        ] {
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
        Self { child, seen, keys }
    }

    fn pid(&self) -> i32 {
        i32::try_from(self.child.process_id().unwrap()).unwrap()
    }

    fn shows(&self, needle: &str) -> bool {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).contains(needle)
    }

    fn type_keys(&self, keys: &[u8]) {
        let mut w = self.keys.lock().unwrap();
        w.write_all(keys).unwrap();
        w.flush().unwrap();
    }

    /// `kill -9`, then reap it.
    fn crash(mut self) {
        kill(self.pid());
        let _ = self.child.wait();
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
    _root: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    config: PathBuf,
    runtime: PathBuf,
    project: PathBuf,
    pidfile: PathBuf,
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
        _root: root,
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
    eventually("the tab's program starts", || dirs.pidfile.exists());
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
    eventually("the tab's program starts", || dirs.pidfile.exists());
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

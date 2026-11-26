#![cfg(unix)]
//! #1309: spyc stopped from outside (`kill -STOP`) and handed back by its
//! shell's `fg` takes its terminal back: it redraws with no key typed, and
//! reads keys one at a time again, though the shell put the terminal in its
//! own line mode meanwhile; and, its tabs' programs in jars, it keeps its
//! link to each through the stop. Driven through the binary, started from an
//! interactive `/bin/sh` on a PTY of the test's own, every directory the
//! test's own.
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const WAIT: Duration = Duration::from_secs(30);

/// The queries spyc makes of its terminal, and what this one answers.
const ANSWERS: &[(&[u8], &[u8])] = &[
    (b"\x1b[6n", b"\x1b[1;1R"),
    (b"\x1b[5n", b"\x1b[0n"),
    (b"\x1b[16t", b"\x1b[6;16;8t"),
    (b"\x1b[?u", b"\x1b[?0u"),
    (b"\x1b[c", b"\x1b[?62;22c"),
    (b"\x1b[0c", b"\x1b[?62;22c"),
];

/// An interactive shell on a PTY: everything it and its jobs print, kept,
/// and spyc's queries answered as a terminal would.
struct Shell {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    seen: Arc<Mutex<Vec<u8>>>,
    keys: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Shell {
    fn start(home: &Path, project: &Path, runtime: &Path) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-i");
        cmd.cwd(project);
        cmd.env_clear();
        for (k, v) in [
            ("HOME", home.as_os_str()),
            ("XDG_STATE_HOME", home.join("state").as_os_str()),
            ("XDG_CONFIG_HOME", home.join("config").as_os_str()),
            ("XDG_RUNTIME_DIR", runtime.as_os_str()),
            ("TMPDIR", runtime.as_os_str()),
        ] {
            cmd.env(k, v);
        }
        cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
        cmd.env("TERM", "xterm-256color");
        cmd.env("SHELL", "/bin/sh");
        cmd.env("PS1", "PROMPT> ");
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

    fn printed(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// What it printed from `at` on.
    fn since(&self, at: usize) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()[at..]).into_owned()
    }

    fn type_keys(&self, keys: &[u8]) {
        let mut w = self.keys.lock().unwrap();
        w.write_all(keys).unwrap();
        w.flush().unwrap();
    }

    fn eventually(&self, what: &str, from: usize, done: impl Fn(&str) -> bool) {
        let until = Instant::now() + WAIT;
        while !done(&self.since(from)) {
            if Instant::now() >= until {
                let seen = self.since(from);
                let at = seen.len().saturating_sub(3000);
                let at = (at..seen.len())
                    .find(|&i| seen.is_char_boundary(i))
                    .unwrap_or(0);
                panic!("never: {what}; the terminal last showed:\n{}", &seen[at..]);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The shell's one child, spyc.
fn job_of(shell: &Shell) -> String {
    let pid = shell.child.process_id().unwrap().to_string();
    let until = Instant::now() + WAIT;
    loop {
        let out = std::process::Command::new("pgrep")
            .args(["-P", &pid])
            .output()
            .unwrap();
        let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !found.is_empty() {
            return found.lines().next().unwrap().to_string();
        }
        assert!(Instant::now() < until, "the shell started nothing");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn signal(pid: &str, sig: &str) {
    let status = std::process::Command::new("kill")
        .args([sig, pid])
        .status()
        .unwrap();
    assert!(status.success(), "kill {sig} {pid}");
}

#[test]
fn spyc_stopped_from_outside_and_handed_back_redraws_and_reads_keys_again() {
    let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
    // macOS scans a new executable on its first run: not inside the waits.
    let _ = std::process::Command::new(spyc).arg("--version").output();
    let home = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(home.path()).unwrap();
    let project = home.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("marker-file.txt"), "x").unwrap();
    // A socket's path is capped near 104 bytes: the runtime directory is short.
    let runtime = tempfile::Builder::new()
        .prefix("sc")
        .tempdir_in("/tmp")
        .unwrap();
    let shell = Shell::start(&home, &project, runtime.path());
    shell.eventually("the shell's prompt", 0, |s| s.contains("PROMPT> "));
    let at = shell.printed();
    shell.type_keys(format!("'{}'\n", spyc.display()).as_bytes());
    shell.eventually("spyc draws its listing", at, |s| {
        s.contains("marker-file.txt")
    });
    let job = job_of(&shell);

    let at = shell.printed();
    signal(&job, "-STOP");
    shell.eventually("the shell takes the terminal back", at, |s| {
        s.contains("PROMPT> ")
    });
    let at = shell.printed();
    shell.type_keys(b"fg\n");
    shell.eventually("spyc redraws, no key typed", at, |s| {
        s.contains("marker-file.txt")
    });
    // In the shell's line mode a `?` would wait in the line for a newline.
    let at = shell.printed();
    shell.type_keys(b"?");
    shell.eventually("one key reaches spyc: its help", at, |s| {
        s.contains("Key tiers")
    });
    signal(&job, "-KILL");
}

/// Every jar socket the sessions saved under `state` record.
fn saved_jar_sockets(state: &Path) -> Vec<PathBuf> {
    fn walk(v: &serde_json::Value, out: &mut Vec<PathBuf>) {
        match v {
            serde_json::Value::Object(o) => {
                if let Some(s) = o.get("jar_socket").and_then(|s| s.as_str()) {
                    out.push(PathBuf::from(s));
                }
                o.values().for_each(|v| walk(v, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(state.join("spyc").join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if let Ok(v) = std::fs::read_to_string(entry.path())
            .map_err(|_| ())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|_| ()))
        {
            walk(&v, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn described(socket: &Path, key: &str) -> Option<String> {
    spyc_jar::client::describe(socket)
        .ok()?
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// Ends each jar it names, with its program, however the test ended: a
/// pane's jar waits for an owner for ever.
struct Jars(Vec<PathBuf>);

impl Drop for Jars {
    fn drop(&mut self) {
        for socket in &self.0 {
            let pid = |key| described(socket, key).and_then(|p| p.parse().ok());
            if let Some(p) = pid("program_pid").and_then(rustix::process::Pid::from_raw) {
                let _ = rustix::process::kill_process_group(p, rustix::process::Signal::KILL);
            }
            if let Some(p) = pid("pid").and_then(rustix::process::Pid::from_raw) {
                let _ = rustix::process::kill_process(p, rustix::process::Signal::KILL);
            }
        }
    }
}

/// What spyc's debug log says, in `state`.
fn debug_log(state: &Path) -> String {
    std::fs::read_dir(state.join("spyc"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("spyc-debug-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect()
}

/// P1 and B12: on Linux a stop and continue of spyc, however short, ended
/// every jarred tab's link at once (each reader's wait failed with `EINTR`),
/// and sent each tab through a retake that raced the connection it hadn't
/// yet closed. A jarred tab's link outlasts a short stop: none ends, none is
/// left `[detached]`, and spyc stays each jar's owner.
#[test]
fn a_short_stop_and_continue_leaves_every_jarred_tab_its_link() {
    const TABS: usize = 3;
    let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
    let _ = std::process::Command::new(spyc).arg("--version").output();
    let home = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(home.path()).unwrap();
    let project = home.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("marker-file.txt"), "x").unwrap();
    let pids = home.join("pids");
    let tab = format!("\"sh -c 'echo $$ >> {}; exec cat'\"", pids.display());
    std::fs::write(
        home.join(".spycrc.toml"),
        format!(
            "[pane]\njars = true\ntabs = [{}]\n",
            vec![tab; TABS].join(", ")
        ),
    )
    .unwrap();
    let runtime = tempfile::Builder::new()
        .prefix("sc")
        .tempdir_in("/tmp")
        .unwrap();
    let shell = Shell::start(&home, &project, runtime.path());
    shell.eventually("the shell's prompt", 0, |s| s.contains("PROMPT> "));
    let at = shell.printed();
    shell.type_keys(format!("'{}' --debug\n", spyc.display()).as_bytes());
    shell.eventually("spyc draws its listing", at, |s| {
        s.contains("marker-file.txt")
    });
    let job = job_of(&shell);
    let state = home.join("state");
    let mut jars = Jars(Vec::new());
    let until = Instant::now() + WAIT;
    loop {
        jars.0 = saved_jar_sockets(&state);
        let owned = jars
            .0
            .iter()
            .filter(|s| described(s, "owner_pid").as_deref() == Some(job.as_str()))
            .count();
        if jars.0.len() == TABS && owned == TABS {
            break;
        }
        assert!(
            Instant::now() < until,
            "never: spyc owns its {TABS} tabs' jars; it owns {owned} of {:?}",
            jars.0
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let at = shell.printed();
    signal(&job, "-STOP");
    shell.eventually("the shell takes the terminal back", at, |s| {
        s.contains("PROMPT> ")
    });
    // However short: a stop inside every jar's deadline.
    let at = shell.printed();
    shell.type_keys(b"fg\n");
    shell.eventually("spyc redraws", at, |s| s.contains("marker-file.txt"));
    let until = Instant::now() + WAIT;
    let owned = |jars: &Jars| {
        jars.0
            .iter()
            .all(|s| described(s, "owner_pid").as_deref() == Some(job.as_str()))
    };
    while !owned(&jars) {
        assert!(
            Instant::now() < until,
            "a stop of spyc lost it a jar: {}",
            debug_log(&state)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let ended: Vec<String> = debug_log(&state)
        .lines()
        .filter(|l| l.starts_with("jar ") && l.contains(" after "))
        .map(str::to_string)
        .collect();
    assert!(
        ended.is_empty(),
        "a stop of spyc ended its tabs' links: {ended:#?}"
    );
    assert!(
        !shell.since(at).contains("[detached]"),
        "a stop of spyc left a tab detached"
    );
    let started = std::fs::read_to_string(&pids).unwrap();
    assert_eq!(
        started.lines().count(),
        TABS,
        "a program started twice: {started}"
    );
    signal(&job, "-KILL");
}

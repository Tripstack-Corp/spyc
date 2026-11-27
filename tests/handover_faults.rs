#![cfg(unix)]
//! G2 of the 3.0 review: spyc killed at each named point of a handover
//! (`handover_fault`: a debug build's `SPYC_HANDOVER_FAULT`) loses no jar,
//! starts no program twice, and leaves its session to come back whole, each
//! tab on the jar it had: by `spyc -r`, or, where the session's own jar
//! runs, by the spyc there, which `spyc attach` reaches. Driven through the
//! binaries, every directory the test's own. Each tab's program adds its pid
//! to a file as it starts, so a second start shows.
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use spyc_jar::client::{Client, Sender};
use spyc_jar::proto::{Msg, Role};

/// The bound on every wait for a state. Only a bound: no wait concludes.
const WAIT: Duration = Duration::from_secs(60);

/// The queries spyc makes of its terminal, and what this one answers.
const ANSWERS: &[(&[u8], &[u8])] = &[
    (b"\x1b[6n", b"\x1b[1;1R"),
    (b"\x1b[5n", b"\x1b[0n"),
    (b"\x1b[16t", b"\x1b[6;16;8t"),
    (b"\x1b[?u", b"\x1b[?0u"),
    (b"\x1b[c", b"\x1b[?62;22c"),
    (b"\x1b[0c", b"\x1b[?62;22c"),
];

const ALT_SCREEN: &str = "\x1b[?1049h";

/// The tabs a session under test runs.
const TABS: usize = 2;

/// The instrumented build's `LLVM_PROFILE_FILE`, kept through each spawned
/// process's cleared environment.
fn profile_file() -> Option<std::ffi::OsString> {
    std::env::var_os("LLVM_PROFILE_FILE")
}

/// Wait for `done`, bounded by [`WAIT`]; `what` and `seen` say what
/// didn't happen.
fn eventually(what: &str, mut done: impl FnMut() -> bool, seen: impl Fn() -> String) {
    let until = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < until, "never: {what}; {}", seen());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The test's directories, in one short one under `/tmp` (a socket's path
/// is capped near 104 bytes). Every jar and session jar under it is ended
/// with its program when the test ends, however it ended.
struct World {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("hf")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        for d in ["s", "c", "d", "k", "r"] {
            std::fs::create_dir(root.join(d)).unwrap();
        }
        for d in ["r/spyc", "r/spyc/sessions", "r/spyc/jars"] {
            let d = root.join(d);
            std::fs::create_dir(&d).unwrap();
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self { root, _dir: dir }
    }

    fn state(&self) -> PathBuf {
        self.root.join("s")
    }

    fn runtime(&self) -> PathBuf {
        self.root.join("r")
    }

    /// The file each tab's program adds its pid to as it starts.
    fn starts_file(&self) -> PathBuf {
        self.root.join("starts")
    }

    /// Each program start so far: its pid and the directory it started in.
    fn starts(&self) -> Vec<(i32, PathBuf)> {
        std::fs::read_to_string(self.starts_file())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (pid, dir) = l.trim().split_once(' ')?;
                Some((pid.parse().ok()?, PathBuf::from(dir)))
            })
            .collect()
    }

    /// A home whose `.spycrc.toml` runs tabs in jars, and opens `tabs` of
    /// them as each project opens, each running a program that marks its
    /// start and where.
    fn home(&self, name: &str, tabs: usize) -> PathBuf {
        let home = self.root.join(name);
        std::fs::create_dir(&home).unwrap();
        let tab = format!(
            "\"sh -c 'echo $$ $(pwd -P) >> {}; exec cat'\"",
            self.starts_file().display()
        );
        let tabs = if tabs > 0 {
            format!("tabs = [{}]\n", vec![tab; tabs].join(", "))
        } else {
            String::new()
        };
        std::fs::write(
            home.join(".spycrc.toml"),
            format!("[pane]\njars = true\n{tabs}"),
        )
        .unwrap();
        home
    }

    /// A project directory, with a file whose name a frame of it shows.
    fn project(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}-files.txt")), "x").unwrap();
        dir
    }

    fn env(&self, home: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
        let at = |d: &str| self.root.join(d).into_os_string();
        let mut env = vec![
            ("HOME", home.as_os_str().to_owned()),
            ("XDG_STATE_HOME", at("s")),
            ("XDG_CONFIG_HOME", at("c")),
            ("XDG_DATA_HOME", at("d")),
            ("XDG_CACHE_HOME", at("k")),
            ("XDG_RUNTIME_DIR", at("r")),
            ("TMPDIR", at("r")),
            ("PATH", "/usr/bin:/bin".into()),
            ("SHELL", "/bin/sh".into()),
            ("TERM", "xterm-256color".into()),
        ];
        env.extend(profile_file().map(|f| ("LLVM_PROFILE_FILE", f)));
        env
    }

    /// Every tab jar's socket.
    fn jars(&self) -> Vec<PathBuf> {
        sockets_in(&self.runtime().join("spyc/jars"))
    }

    /// Every session jar's socket.
    fn sessions(&self) -> Vec<PathBuf> {
        sockets_in(&self.runtime().join("spyc/sessions"))
    }

    /// Every session saved.
    fn saved(&self) -> Vec<Saved> {
        std::fs::read_dir(self.state().join("spyc/sessions"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let text = std::fs::read_to_string(e.path()).ok()?;
                Saved::parse(&serde_json::from_str(&text).ok()?)
            })
            .collect()
    }

    /// Whether spyc `pid` said it died at `point`: on `terminal`, or in
    /// the log its standard error went to while it drew.
    fn fault_said(&self, pid: i32, point: &str, terminal: &str) -> bool {
        let point = if point.contains(':') {
            point.to_string()
        } else {
            format!("{point}:1")
        };
        let said = format!("handover fault at {point},");
        terminal.contains(&said)
            || std::fs::read_to_string(self.state().join(format!("spyc/stderr/{pid}.log")))
                .is_ok_and(|log| log.contains(&said))
    }
}

impl Drop for World {
    fn drop(&mut self) {
        for socket in self.jars().iter().chain(&self.sessions()) {
            let pid = |key| described(socket, key).and_then(|p| p.parse().ok());
            if let Some(p) = pid("program_pid").and_then(rustix::process::Pid::from_raw) {
                let _ = rustix::process::kill_process_group(p, rustix::process::Signal::KILL);
                let _ = rustix::process::kill_process(p, rustix::process::Signal::KILL);
            }
            if let Some(p) = pid("pid").and_then(rustix::process::Pid::from_raw) {
                let _ = rustix::process::kill_process(p, rustix::process::Signal::KILL);
            }
        }
    }
}

fn sockets_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sock"))
        .collect();
    out.sort();
    out
}

fn described(socket: &Path, key: &str) -> Option<String> {
    spyc_jar::client::describe(socket)
        .ok()?
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

fn pid_of(socket: &Path, key: &str) -> Option<i32> {
    described(socket, key).and_then(|p| p.parse().ok())
}

fn alive(pid: i32) -> bool {
    rustix::process::Pid::from_raw(pid)
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// A saved session, as far as these tests read it.
#[derive(Debug)]
struct Saved {
    name: String,
    pid: Option<i32>,
    epoch: u64,
    jars: Vec<PathBuf>,
    /// How many tabs and tasks it holds, with a jar or without.
    held: usize,
}

impl Saved {
    fn parse(v: &serde_json::Value) -> Option<Self> {
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
        // Its tabs' jars and its tasks'.
        let mut jars = Vec::new();
        walk(v.get("projects")?, &mut jars);
        if let Some(tasks) = v.get("tasks") {
            walk(tasks, &mut jars);
        }
        let tabs: usize = v
            .get("projects")?
            .as_array()?
            .iter()
            .filter_map(|p| p.get("tabs")?.as_array().map(Vec::len))
            .sum();
        let tasks = v
            .get("tasks")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        Some(Self {
            name: v.get("name")?.as_str()?.to_string(),
            pid: v
                .get("pid")
                .and_then(serde_json::Value::as_i64)
                .and_then(|p| i32::try_from(p).ok()),
            epoch: v.get("epoch_secs")?.as_u64()?,
            jars,
            held: tabs + tasks,
        })
    }

    /// Whether the spyc it names runs.
    fn running(&self) -> bool {
        self.pid.is_some_and(alive)
    }
}

/// `spyc ARGS` on a PTY of the test's own, answering spyc's queries as a
/// terminal would. Killed when it drops.
struct Spyc {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    seen: Arc<Mutex<Vec<u8>>>,
    keys: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Spyc {
    fn start(
        world: &World,
        exe: &Path,
        (home, cwd): (&Path, &Path),
        args: &[&str],
        fault: Option<&str>,
    ) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap_or_else(|e| panic!("no PTY for spyc: {e:#}"));
        let mut cmd = CommandBuilder::new(exe);
        cmd.args(args);
        cmd.cwd(cwd);
        cmd.env_clear();
        for (k, v) in world.env(home) {
            cmd.env(k, v);
        }
        if let Some(point) = fault {
            cmd.env("SPYC_HANDOVER_FAULT", point);
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

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }

    /// The last of what it showed.
    fn tail(&self) -> String {
        let text = self.text();
        let at = text.len().saturating_sub(2000);
        let at = (at..text.len())
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(0);
        format!("the terminal last showed:\n{:?}", &text[at..])
    }

    fn type_keys(&self, keys: &[u8]) {
        let mut w = self.keys.lock().unwrap();
        // A key racing an exit meets a closed PTY: nothing to tell.
        let _ = w.write_all(keys).and_then(|()| w.flush());
    }

    /// Wait for it to draw `needle` after it last took the screen.
    fn wait_drawn(&self, needle: &str) {
        eventually(
            &format!("spyc draws {needle}"),
            || {
                let text = self.text();
                text.rfind(ALT_SCREEN)
                    .is_some_and(|at| text[at..].contains(needle))
            },
            || self.tail(),
        );
    }

    /// Wait for it to exit, and say how.
    fn exit(&mut self) -> portable_pty::ExitStatus {
        let mut status = None;
        let seen = Arc::clone(&self.seen);
        let child = &mut self.child;
        eventually(
            "spyc exits",
            || {
                status = child.try_wait().unwrap();
                status.is_some()
            },
            || {
                let text = String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
                let at = text.len().saturating_sub(2000);
                let at = (at..text.len())
                    .find(|&i| text.is_char_boundary(i))
                    .unwrap_or(0);
                format!("the terminal last showed:\n{:?}", &text[at..])
            },
        );
        status.unwrap()
    }

    /// Wait for it to die at `point`: killed, as a crash would end it, and
    /// saying it was the point.
    fn killed_at(&mut self, world: &World, point: &str) {
        let pid = self.pid();
        let status = self.exit();
        assert!(
            status.signal().is_some_and(|s| s.starts_with("Killed")),
            "spyc wasn't killed at {point}: {status:?}; {}",
            self.tail()
        );
        assert!(
            world.fault_said(pid, point, &self.text()),
            "spyc died, but not at {point}; {}",
            self.tail()
        );
    }
}

impl Drop for Spyc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A terminal attached to a session's own jar, as `spyc attach` attaches
/// one: what it was sent, and spyc's queries answered.
struct Terminal {
    seen: Arc<Mutex<Vec<u8>>>,
    tx: Sender,
}

impl Terminal {
    fn attach(world: &World, socket: &Path) -> Self {
        let stem = socket.file_stem().unwrap();
        let token =
            spyc_jar::client::read_token_file(&world.state().join("spyc/jar-tokens").join(stem))
                .unwrap();
        let (client, welcome) = Client::connect(socket, Role::Owner, WAIT, &token).unwrap();
        let Msg::Welcome { head, .. } = welcome else {
            panic!("expected a welcome, got {welcome:?}");
        };
        let (mut rx, tx) = client.split();
        tx.send(&Msg::Replay { from: head }).unwrap();
        tx.send(&Msg::Resize {
            rows: 30,
            cols: 120,
            px_w: 0,
            px_h: 0,
        })
        .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        {
            let (seen, tx) = (Arc::clone(&seen), tx.clone());
            std::thread::spawn(move || {
                while let Ok(msg) = rx.recv(Duration::from_millis(200)) {
                    match msg {
                        Some(Msg::Output { offset, bytes }) => {
                            for (query, answer) in ANSWERS {
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
                            let _ = tx.send(&Msg::AckExit);
                        }
                        _ => {}
                    }
                }
            });
        }
        Self { seen, tx }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }

    fn type_keys(&self, keys: &[u8]) {
        let _ = self.tx.send(&Msg::Input {
            bytes: keys.to_vec(),
        });
    }
}

/// What ran before the fault: every jar and the pid of its program, every
/// program start, and the spycs that still run after it and own jars.
struct Before {
    jars: BTreeMap<PathBuf, i32>,
    starts: Vec<(i32, PathBuf)>,
    owners: Vec<i32>,
}

/// Wait for each spyc in `owning` to own its number of jars, and no other
/// jar to run, each program started once, and for each a saved session to
/// name all its jars (its last autosave, which a crash leaves behind): what
/// runs before the fault. Every jar under the test's directory is in it, the
/// ones a handover moves and the ones that stay.
fn running(world: &World, owning: &[(i32, usize)]) -> Before {
    let total: usize = owning.iter().map(|(_, n)| n).sum();
    let owned = || {
        let jars = world.jars();
        jars.len() == total
            && world.starts().len() == total
            && owning.iter().all(|&(owner, n)| {
                let mine: Vec<&PathBuf> = jars
                    .iter()
                    .filter(|s| pid_of(s, "owner_pid") == Some(owner))
                    .collect();
                mine.len() == n
                    && world
                        .saved()
                        .iter()
                        .any(|m| mine.iter().all(|j| m.jars.contains(j)))
            })
    };
    eventually(&format!("{owning:?} own their jars, saved"), owned, || {
        let owners: Vec<_> = world.jars().iter().map(|s| described(s, "owner")).collect();
        format!(
            "jars {:?}, owners {owners:?}, starts {:?}",
            world.jars(),
            std::fs::read_to_string(world.starts_file())
        )
    });
    let jars = world
        .jars()
        .into_iter()
        .map(|s| {
            let program = pid_of(&s, "program_pid").unwrap();
            (s, program)
        })
        .collect();
    let mut starts = world.starts();
    starts.sort();
    Before {
        jars,
        starts,
        owners: Vec::new(),
    }
}

/// No jar lost, no program started twice: every program that ran still
/// runs, in the jar it had, and no program has started since, anywhere.
fn nothing_lost(world: &World, before: &Before, when: &str) {
    for (socket, program) in &before.jars {
        assert_eq!(
            pid_of(socket, "program_pid"),
            Some(*program),
            "{when}: the jar at {} lost its program",
            socket.display()
        );
        assert!(alive(*program), "{when}: program {program} ended");
    }
    let mut starts = world.starts();
    starts.sort();
    assert_eq!(starts, before.starts, "{when}: a program started again");
}

/// Wait for every jar in `jars` to be owned by `owner`.
fn wait_owned(world: &World, jars: &[PathBuf], owner: i32) {
    eventually(
        &format!("spyc {owner} takes back {jars:?}"),
        || jars.iter().all(|s| pid_of(s, "owner_pid") == Some(owner)),
        || {
            let owners: Vec<_> = jars.iter().map(|s| described(s, "owner")).collect();
            format!("owners {owners:?}; jars now {:?}", world.jars())
        },
    );
}

/// Wait for spyc `pid` to have settled what it holds after taking back
/// `took`: its own session saved, naming them, which it saves only after
/// the restore made every tab and task it will hold. Then that save holds
/// nothing but jars that ran before the fault: a tab or task beyond them
/// (one started afresh, a startup tab seeded under a restore) is a start.
fn settled(world: &World, pid: i32, took: &[PathBuf], before: &Before) {
    let mut own = None;
    eventually(
        &format!("spyc {pid} saves the session it took back"),
        || {
            own = world
                .saved()
                .into_iter()
                .find(|m| m.pid == Some(pid) && took.iter().all(|j| m.jars.contains(j)));
            own.is_some()
        },
        || format!("{:?}", world.saved()),
    );
    let own = own.unwrap();
    let foreign: Vec<&PathBuf> = own
        .jars
        .iter()
        .filter(|j| !before.jars.contains_key(*j))
        .collect();
    assert!(
        own.held == own.jars.len() && foreign.is_empty(),
        "spyc {pid} holds more than it took back: {} tabs and tasks, jars {:?}, \
         not from before the fault {foreign:?}",
        own.held,
        own.jars
    );
}

/// The number the `-r` picker shows on session `name`'s row: the `[N]`
/// nearest before the name's last appearance. Read off what was drawn, where
/// a cell left blank may be a cursor move rather than a space.
fn picker_number(text: &str, name: &str) -> Option<u8> {
    let at = text.rfind(name)?;
    let row = &text.as_bytes()[at.saturating_sub(64)..at];
    (2..row.len())
        .rev()
        .find(|&i| row[i] == b']' && row[i - 1].is_ascii_digit() && row[i - 2] == b'[')
        .map(|i| row[i - 1])
}

/// How the session comes back once spyc has died at the point.
#[derive(Clone, Copy, Debug)]
enum Back {
    /// `spyc -r` takes every jar back.
    Restore,
    /// The session's own jar runs, and its spyc takes back the jars its
    /// session names; `spyc -r` takes any other back.
    Attach,
}

/// The session whose own jar runs takes back the jars it names, and
/// `spyc attach` reaches it, drawing `shown`: the spyc there. The client is
/// ended, not detached: a pop-up of that spyc's (the jars a crash left
/// running) may be up, and would take the `^d`.
fn attach_back(world: &World, shown: &str, before: &Before) -> i32 {
    eventually(
        "one session jar runs",
        || world.sessions().len() == 1 && pid_of(&world.sessions()[0], "program_pid").is_some(),
        || format!("{:?}", world.sessions()),
    );
    let sessions = world.sessions();
    let socket = &sessions[0];
    let name = socket.file_stem().unwrap().to_string_lossy().into_owned();
    let spyc = pid_of(socket, "program_pid").expect("a spyc runs in the session's jar");
    let saved = world.saved();
    let named: Vec<PathBuf> = saved
        .iter()
        .filter(|m| m.name == name)
        .max_by_key(|m| m.epoch)
        .map_or_default(|m| m.jars.clone());
    assert!(!named.is_empty(), "session {name} names no jar: {saved:?}");
    wait_owned(world, &named, spyc);
    settled(world, spyc, &named, before);
    let home = world.home("attach-home", 0);
    let attach = Spyc::start(
        world,
        Path::new(env!("CARGO_BIN_EXE_spyc")),
        (&home, &world.root),
        &["attach", &name],
        None,
    );
    attach.wait_drawn(shown);
    drop(attach);
    wait_owned(world, &named, spyc);
    spyc
}

/// Bring every jar of `before` back, as `back` says: the spycs `spyc -r`
/// started, which own them while they run. Each runs from a home that opens
/// startup tabs, which a restore must not start: a start shows.
fn bring_back(world: &World, before: &Before, back: Back) -> Vec<Spyc> {
    let mut owners: Vec<i32> = before.owners.clone();
    if matches!(back, Back::Attach) {
        owners.push(attach_back(world, "alpha-files.txt", before));
    }
    let mut restored = Vec::new();
    for round in 0.. {
        let unowned: Vec<PathBuf> = before
            .jars
            .keys()
            .filter(|s| !pid_of(s, "owner_pid").is_some_and(|o| owners.contains(&o)))
            .cloned()
            .collect();
        if unowned.is_empty() {
            break;
        }
        assert!(round < 4, "jars nobody takes back: {unowned:?}");
        let saved = world.saved();
        let pick = saved
            .iter()
            .filter(|m| !m.running())
            .max_by_key(|m| {
                (
                    unowned.iter().filter(|j| m.jars.contains(j)).count(),
                    m.epoch,
                )
            })
            .filter(|m| unowned.iter().any(|j| m.jars.contains(j)))
            .unwrap_or_else(|| panic!("no saved session names {unowned:?}: {saved:?}"));
        let home = world.home(&format!("restore-{round}"), TABS);
        let started = world.starts().len();
        let r = Spyc::start(
            world,
            Path::new(env!("CARGO_BIN_EXE_spyc")),
            (&home, &world.root),
            &["-r"],
            None,
        );
        let mut number = None;
        eventually(
            &format!("the -r picker lists {}", pick.name),
            || {
                number = picker_number(&r.text(), &pick.name);
                number.is_some()
            },
            || r.tail(),
        );
        // What it decided at its start is in the frame that draws the
        // picker: startup tabs it opened flash "opened N/M startup tab(s)".
        // Those are waited for, so their start shows however soon the
        // restore closes them.
        if r.text().contains("tab(s)") {
            eventually(
                "the startup tabs the -r spyc opened start",
                || world.starts().len() > started,
                || r.tail(),
            );
        }
        r.type_keys(&[number.unwrap()]);
        let taking: Vec<PathBuf> = unowned
            .iter()
            .filter(|j| pick.jars.contains(j))
            .cloned()
            .collect();
        wait_owned(world, &taking, r.pid());
        settled(world, r.pid(), &taking, before);
        owners.push(r.pid());
        restored.push(r);
    }
    restored
}

/// The handover a case crashes, and what its spyc does to get there.
#[derive(Clone, Copy, Debug)]
enum Handover {
    /// `^d` in a plain spyc.
    Detach,
    /// `^d` in a plain spyc whose session's jar can't start: its socket is
    /// taken.
    DetachRefused,
    /// `:upgrade` in spyc's own jar, a newer spyc installed.
    Upgrade,
    /// `:detach project`, its project one of two, each with its tabs.
    DetachProject,
    /// `:detach tab 1`.
    DetachTab,
    /// `:attach project`, the fault in the spyc that gives its session up.
    AttachProjectGiver,
    /// `:attach project`, the fault in the spyc that takes it.
    AttachProjectTaker,
    /// `:attach tab`, the fault in the spyc that takes it.
    AttachTabTaker,
}

/// What the crashed spyc runs besides its tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Runs {
    /// Its tabs only.
    Tabs,
    /// A background task as well, in a jar of its own.
    Task,
}

/// A row of the table: crash `handover` at `point` (`SPYC_HANDOVER_FAULT`'s
/// value), and bring the session back as `back` says.
struct Case {
    handover: Handover,
    point: &'static str,
    back: Back,
    runs: Runs,
}

impl Case {
    /// The point spyc is killed at; none for a case whose handover runs to
    /// its end, the control that sees what only a finished handover does
    /// (a moved program ended after the crash points have passed).
    fn fault(&self) -> Option<&'static str> {
        Some(self.point).filter(|p| !p.is_empty())
    }

    /// Wait for the crashed spyc to die at the point, or, with none, for the
    /// handover to have run.
    fn ends(&self, world: &World, a: &mut Spyc) {
        if self.fault().is_some() {
            a.killed_at(world, self.point);
        }
    }
}

/// How many cases run at once. Each holds about ten PTYs (its spycs' and
/// its jars'), and macOS has 511 for all of the user's processes: the whole
/// table at once ran out of them under a loaded gate.
const AT_ONCE: usize = 6;

/// The cases running now, and the wake for one waiting its turn.
static RUNNING: Mutex<usize> = Mutex::new(0);
static FREED: std::sync::Condvar = std::sync::Condvar::new();

/// A case's turn, given back when it ends, however it ends.
struct Turn;

impl Turn {
    fn take() -> Self {
        let mut running = RUNNING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *running >= AT_ONCE {
            running = FREED
                .wait(running)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *running += 1;
        Self
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        *RUNNING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) -= 1;
        FREED.notify_one();
    }
}

/// Run a case: start the session, crash it at the point, and check that no
/// jar was lost, no program started twice, and the session comes back.
fn run(case: &Case) {
    let _turn = Turn::take();
    let world = World::new();
    let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
    // macOS scans a new executable on its first run: not inside the waits.
    let _ = std::process::Command::new(spyc).arg("--version").output();
    let (before, _alive) = match case.handover {
        Handover::Detach | Handover::DetachRefused => crash_detach(&world, case),
        Handover::Upgrade => crash_upgrade(&world, case),
        Handover::DetachProject | Handover::DetachTab => crash_move(&world, case),
        Handover::AttachProjectGiver | Handover::AttachProjectTaker | Handover::AttachTabTaker => {
            crash_attach(&world, case)
        }
    };
    let when = case.fault().unwrap_or("the end of the handover");
    nothing_lost(&world, &before, &format!("killed at {when}"));
    let _back = bring_back(&world, &before, case.back);
    nothing_lost(&world, &before, &format!("back after {when}"));
}

/// A background task in `a`, run as a capture and sent to the background
/// with `^Z` once it has started, and shown there: a program that marks its
/// start, as each tab's does.
fn start_task(world: &World, a: &Spyc) {
    let started = world.starts().len();
    a.type_keys(
        format!(
            "!echo $$ $(pwd -P) >> {}; exec cat\r",
            world.starts_file().display()
        )
        .as_bytes(),
    );
    eventually(
        "the task's program starts",
        || world.starts().len() > started,
        || a.tail(),
    );
    a.type_keys(b"\x1a");
    // In the background once the pane's divider shows it there, `[1+]` or
    // `[1●]`: until then a key typed would reach the capture's program.
    eventually(
        "spyc draws the task in the background",
        || {
            let text = a.text();
            text.rfind(ALT_SCREEN)
                .is_some_and(|at| text[at..].contains("[1+]") || text[at..].contains("[1\u{25cf}]"))
        },
        || a.tail(),
    );
}

/// A plain spyc in `project` with [`TABS`] tabs, and a task if `runs` says,
/// whose crash is at `fault`: it and how many jars it runs.
fn plain(
    world: &World,
    exe: &Path,
    (home, project): (&str, &str),
    runs: Runs,
    fault: Option<&str>,
) -> (Spyc, usize) {
    let home = world.home(home, TABS);
    let dir = world.project(project);
    let a = Spyc::start(world, exe, (&home, &dir), &[], fault);
    a.wait_drawn(&format!("{project}-files.txt"));
    let mut jars = TABS;
    if runs == Runs::Task {
        start_task(world, &a);
        jars += 1;
    }
    (a, jars)
}

/// `^d` in a plain spyc running [`TABS`] tabs, killed at the point.
fn crash_detach(world: &World, case: &Case) -> (Before, Vec<Spyc>) {
    let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
    let (mut a, jars) = plain(world, spyc, ("ha", "alpha"), case.runs, case.fault());
    let before = running(world, &[(a.pid(), jars)]);
    let _taken = matches!(case.handover, Handover::DetachRefused).then(|| {
        let name = world
            .saved()
            .into_iter()
            .find(|m| m.pid == Some(a.pid()))
            .expect("the session saved")
            .name;
        let socket = world.runtime().join(format!("spyc/sessions/{name}.sock"));
        std::os::unix::net::UnixListener::bind(socket).unwrap()
    });
    a.type_keys(b"\x04");
    if case.fault().is_none() {
        assert!(a.exit().success(), "the ^d failed: {}", a.tail());
    }
    case.ends(world, &mut a);
    (before, Vec::new())
}

/// `:upgrade` in the own jar of a session that `^d` started from a spyc
/// installed in the test's own directory, killed at the point once a newer
/// spyc is installed there.
fn crash_upgrade(world: &World, case: &Case) -> (Before, Vec<Spyc>) {
    let bin = world.root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let installed = bin.join("spyc");
    std::fs::copy(env!("CARGO_BIN_EXE_spyc"), &installed).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_spyc-jar"), bin.join("spyc-jar")).unwrap();
    let _ = std::process::Command::new(&installed)
        .arg("--version")
        .output();
    let (mut a, jars) = plain(world, &installed, ("ha", "alpha"), case.runs, case.fault());
    let before = running(world, &[(a.pid(), jars)]);
    a.type_keys(b"\x04");
    assert!(a.exit().success(), "the ^d failed: {}", a.tail());
    eventually(
        "the session's jar runs",
        || world.sessions().len() == 1,
        String::new,
    );
    let socket = world.sessions().remove(0);
    let own = pid_of(&socket, "program_pid").expect("a spyc in the session's jar");
    let jars: Vec<PathBuf> = before.jars.keys().cloned().collect();
    wait_owned(world, &jars, own);
    // A newer install: another file at the path spyc runs from.
    let file = std::fs::File::open(&installed).unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    let terminal = Terminal::attach(world, &socket);
    eventually(
        "spyc draws on the attached terminal",
        || terminal.text().contains("alpha-files.txt"),
        || terminal.text(),
    );
    terminal.type_keys(b":upgrade\r");
    eventually(
        &format!("the session's spyc dies at {}", case.point),
        || !alive(own),
        || terminal.text(),
    );
    assert!(
        world.fault_said(own, case.point, &terminal.text()),
        "the session's spyc died, but not at {}: {:?}",
        case.point,
        terminal.text()
    );
    (before, Vec::new())
}

/// `:detach project` or `:detach tab 1` in a plain spyc, killed at the point.
/// For a project, the spyc runs two, each with [`TABS`] tabs: what stays is
/// checked as what moves is.
fn crash_move(world: &World, case: &Case) -> (Before, Vec<Spyc>) {
    let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
    let (mut a, mut jars) = plain(world, spyc, ("ha", "alpha"), case.runs, case.fault());
    if matches!(case.handover, Handover::DetachProject) {
        let beta = world.project("beta");
        // `Space n`, its offer cleared, and the second project's path: each
        // key taken in turn, the prompt open before the next. Its startup
        // tabs open as the first project's did.
        a.type_keys(b" n\x15");
        a.type_keys(format!("{}\r", beta.display()).as_bytes());
        a.wait_drawn("beta-files.txt");
        jars += TABS;
    }
    let before = running(world, &[(a.pid(), jars)]);
    if matches!(case.handover, Handover::DetachProject) {
        a.type_keys(b":detach project alpha\r");
    } else {
        a.type_keys(b":detach tab 1\r");
    }
    if case.fault().is_none() {
        // The spyc runs on, owning what stays.
        let mut before = before;
        before.owners.push(a.pid());
        return (before, vec![a]);
    }
    case.ends(world, &mut a);
    (before, Vec::new())
}

/// `:attach project` or `:attach tab` of a session `^d` detached with
/// [`TABS`] tabs, by a spyc running [`TABS`] tabs of its own, killed at the
/// point in the spyc that gives the session up or the one that takes it.
/// The spycs that still run.
fn crash_attach(world: &World, case: &Case) -> (Before, Vec<Spyc>) {
    let spyc = Path::new(env!("CARGO_BIN_EXE_spyc"));
    let giver = matches!(case.handover, Handover::AttachProjectGiver);
    let (mut b, _) = plain(
        world,
        spyc,
        ("hb", "beta"),
        Runs::Tabs,
        if giver { case.fault() } else { None },
    );
    let theirs = running(world, &[(b.pid(), TABS)]);
    b.type_keys(b"\x04");
    assert!(b.exit().success(), "the ^d failed: {}", b.tail());
    eventually(
        "the session's jar runs",
        || world.sessions().len() == 1,
        String::new,
    );
    let socket = world.sessions().remove(0);
    let name = socket.file_stem().unwrap().to_string_lossy().into_owned();
    let own = pid_of(&socket, "program_pid").expect("a spyc in the session's jar");
    let jars: Vec<PathBuf> = theirs.jars.keys().cloned().collect();
    wait_owned(world, &jars, own);

    let (mut a, _) = plain(
        world,
        spyc,
        ("ha", "alpha"),
        Runs::Tabs,
        if giver { None } else { case.fault() },
    );
    let mut before = running(world, &[(own, TABS), (a.pid(), TABS)]);
    let what = if matches!(case.handover, Handover::AttachTabTaker) {
        "tab"
    } else {
        "project"
    };
    a.type_keys(format!(":attach {what} {name}\r").as_bytes());
    if case.fault().is_none() {
        // The taking spyc runs on, owning every jar; the giving one quits.
        let all: Vec<PathBuf> = before.jars.keys().cloned().collect();
        wait_owned(world, &all, a.pid());
        eventually("the giving spyc quits", || !alive(own), || a.tail());
        before.owners.push(a.pid());
        return (before, vec![a]);
    }
    if giver {
        eventually(
            &format!("the giving spyc dies at {}", case.point),
            || !alive(own),
            || a.tail(),
        );
        assert!(
            world.fault_said(own, case.point, ""),
            "the giving spyc died, but not at {}",
            case.point
        );
        // The taking spyc runs on, owning its own tabs' jars.
        before.owners.push(a.pid());
        (before, vec![a])
    } else {
        case.ends(world, &mut a);
        (before, Vec::new())
    }
}

/// One test per row of the table, each named for its case.
macro_rules! cases {
    ($($(#[$attr:meta])* $name:ident: $handover:ident, $point:literal, $back:ident, $runs:ident;)*) => {
        $(
            $(#[$attr])*
            #[test]
            fn $name() {
                run(&Case {
                    handover: Handover::$handover,
                    point: $point,
                    back: Back::$back,
                    runs: Runs::$runs,
                });
            }
        )*
    };
}

cases! {
    detach_saved: Detach, "detach.saved", Restore, Tabs;
    detach_token_saved: Detach, "session.token-saved", Restore, Tabs;
    detach_jar_started: Detach, "detach.jar-started", Attach, Tabs;
    detach_first_released: Detach, "detach.released:1", Attach, Tabs;
    detach_last_released: Detach, "detach.released:2", Attach, Tabs;
    detach_kept: DetachRefused, "detach.kept", Restore, Tabs;
    detach_task_saved: Detach, "detach.saved", Restore, Task;
    detach_task_released: Detach, "detach.released:3", Attach, Task;
    upgrade_saved: Upgrade, "upgrade.saved", Restore, Tabs;
    upgrade_first_released: Upgrade, "upgrade.released:1", Restore, Tabs;
    upgrade_last_released: Upgrade, "upgrade.released:2", Restore, Tabs;
    upgrade_task_released: Upgrade, "upgrade.released:3", Restore, Task;
    move_saved: DetachProject, "move.saved", Restore, Tabs;
    move_first_released: DetachProject, "move.released:1", Restore, Tabs;
    move_last_released: DetachProject, "move.released:2", Restore, Tabs;
    move_resaved: DetachProject, "move.resaved", Restore, Tabs;
    move_token_saved: DetachProject, "session.token-saved", Restore, Tabs;
    move_jar_started: DetachProject, "move.jar-started", Attach, Tabs;
    move_task_resaved: DetachProject, "move.resaved", Restore, Task;
    move_task_jar_started: DetachProject, "move.jar-started", Attach, Task;
    move_tab_saved: DetachTab, "move-tab.saved", Restore, Tabs;
    move_tab_released: DetachTab, "move-tab.released", Restore, Tabs;
    move_tab_resaved: DetachTab, "move-tab.resaved", Restore, Tabs;
    move_tab_jar_started: DetachTab, "move.jar-started", Attach, Tabs;
    release_saved: AttachProjectGiver, "release.saved", Restore, Tabs;
    release_token_forgotten: AttachProjectGiver, "release.token-forgotten", Restore, Tabs;
    release_first_released: AttachProjectGiver, "release.released:1", Restore, Tabs;
    release_last_released: AttachProjectGiver, "release.released:2", Restore, Tabs;
    take_saved: AttachProjectTaker, "take.saved", Restore, Tabs;
    take_removed: AttachProjectTaker, "take.removed", Restore, Tabs;
    take_tabs_saved: AttachTabTaker, "take-tabs.saved", Restore, Tabs;
    take_tabs_removed: AttachTabTaker, "take-tabs.removed", Restore, Tabs;
    // Controls: each handover run to its end, nothing killed.
    detach_done: Detach, "", Attach, Task;
    move_done: DetachProject, "", Attach, Task;
    move_tab_done: DetachTab, "", Attach, Tabs;
    attach_done: AttachProjectTaker, "", Restore, Tabs;
    attach_tab_done: AttachTabTaker, "", Restore, Tabs;
}

#[test]
fn the_picker_number_is_read_off_the_row_that_names_the_session() {
    let text = "x\u{1b}[?1049hsessions\u{1b}[1;10Hj/k\u{1b}[2;3H[n]\u{1b}[2;8Hnew\
                \u{1b}[3;3H[1]\u{1b}[3;8H\u{1b}[1mCUMIN_CLOVE\u{1b}[4;3H[2]\u{1b}[4;8HANISE_MACE";
    assert_eq!(picker_number(text, "CUMIN_CLOVE"), Some(b'1'));
    assert_eq!(picker_number(text, "ANISE_MACE"), Some(b'2'));
    assert_eq!(picker_number(text, "NUTMEG"), None);
}

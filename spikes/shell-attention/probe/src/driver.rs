//! The run: each case in a fresh shell on a fresh PTY, observed at 250 ms, 2 s
//! and 10 s after its output goes quiet, from the ancestor's position (this
//! process, holding the master) and an unrelated one (a child of this process
//! in its own session, so not an ancestor of the program, reading by pid).

use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{Value, json};

use crate::corpus::{CASES, Case, Os, Prep};
use crate::obs::{self, From};

pub struct Cfg {
    pub fix: String,
    pub home: String,
    pub work: String,
    pub scope: String,
    pub only: Option<Vec<String>>,
    pub port: u16,
}

const LABELS: [(u64, &str); 3] = [(250, "250ms"), (2000, "2s"), (10_000, "10s")];
/// Past this, new output no longer restarts the quiet clock.
const RESTART_WINDOW: Duration = Duration::from_secs(15);
const CAP: Duration = Duration::from_secs(35);
const PAIR: Duration = Duration::from_millis(100);

pub fn os_ok(o: Os) -> bool {
    match o {
        Os::Both => true,
        Os::Linux => cfg!(target_os = "linux"),
        Os::Macos => cfg!(target_os = "macos"),
    }
}

pub fn which(p: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {p} >/dev/null")])
        .status()
        .is_ok_and(|s| s.success())
}

pub fn shell_env(home: &str) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        ),
        ("HOME".to_string(), home.to_string()),
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("PS1".to_string(), "$ ".to_string()),
        ("HISTFILE".to_string(), "/dev/null".to_string()),
        (
            "BASH_SILENCE_DEPRECATION_WARNING".to_string(),
            "1".to_string(),
        ),
        ("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string()),
        ("GIT_TERMINAL_PROMPT".to_string(), "1".to_string()),
        (
            "LANG".to_string(),
            if cfg!(target_os = "macos") {
                "en_US.UTF-8"
            } else {
                "C.UTF-8"
            }
            .to_string(),
        ),
    ];
    for k in ["USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(k) {
            env.push((k.to_string(), v));
        }
    }
    env
}

pub fn shell_argv() -> Vec<String> {
    vec!["bash".into(), "--noprofile".into(), "--norc".into()]
}

/// The master's output: what arrived, when it last did, and the replies a
/// terminal owes to the queries programs send at start (a program waiting on
/// an answer would otherwise look blocked on the terminal for the wrong reason).
pub struct Io {
    pub master: OwnedFd,
    pub buf: Vec<u8>,
    pub last_out: Instant,
}

impl Io {
    pub fn pump(&mut self, wait: Duration) -> bool {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        let mut got = false;
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let ts = Timespec {
                tv_sec: left.as_secs() as i64,
                tv_nsec: i64::from(left.subsec_nanos()),
            };
            let mut fds = [PollFd::new(&self.master, PollFlags::IN)];
            let n = poll(&mut fds, Some(&ts)).unwrap_or(0);
            if n == 0 {
                return got;
            }
            let mut chunk = [0u8; 8192];
            let r = rustix::io::read(&self.master, &mut chunk);
            match r {
                Ok(0) | Err(_) => return got,
                Ok(k) => {
                    let start = self.buf.len().saturating_sub(16);
                    self.buf.extend_from_slice(&chunk[..k]);
                    self.last_out = Instant::now();
                    got = true;
                    self.answer(start);
                }
            }
            if Instant::now() >= until {
                return got;
            }
        }
    }

    fn answer(&mut self, from: usize) {
        let tail = self.buf[from..].to_vec();
        let has = |p: &[u8]| tail.windows(p.len()).any(|w| w == p);
        let mut reply = Vec::new();
        if has(b"\x1b[6n") {
            reply.extend_from_slice(b"\x1b[1;1R");
        }
        if has(b"\x1b[c") || has(b"\x1b[0c") {
            reply.extend_from_slice(b"\x1b[?62;22c");
        }
        if has(b"\x1b[>c") || has(b"\x1b[>0c") {
            reply.extend_from_slice(b"\x1b[>1;10;0c");
        }
        if has(b"\x1b]11;?") {
            reply.extend_from_slice(b"\x1b]11;rgb:0000/0000/0000\x1b\\");
        }
        if has(b"\x1b]10;?") {
            reply.extend_from_slice(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
        }
        if !reply.is_empty() {
            self.write(&reply);
        }
    }

    pub fn write(&self, b: &[u8]) {
        let _ = rustix::io::write(&self.master, b);
    }

    /// S4: output quiet, and the bytes after the last newline, escape
    /// sequences dropped, hold something visible.
    pub fn s4(&self) -> Value {
        let last_nl = self
            .buf
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        let tail = strip_escapes(&self.buf[last_nl..]);
        let visible = tail.iter().any(|b| b.is_ascii_graphic() || *b >= 0x80);
        let shown: String = String::from_utf8_lossy(&tail)
            .chars()
            .rev()
            .take(60)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        json!({"unterminated": visible, "tail": shown, "quiet_ms": self.last_out.elapsed().as_millis() as u64})
    }
}

fn strip_escapes(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b {
            i += 1;
            match b.get(i) {
                Some(b'[') => {
                    i += 1;
                    while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                        i += 1;
                    }
                    i += 1;
                }
                Some(b']') | Some(b'P') | Some(b'_') => {
                    while i < b.len()
                        && b[i] != 0x07
                        && !(b[i] == 0x1b && b.get(i + 1) == Some(&b'\\'))
                    {
                        i += 1;
                    }
                    i += if b.get(i) == Some(&0x07) { 1 } else { 2 };
                }
                _ => i += 1,
            }
        } else {
            if b[i] == b'\r' {
                out.clear();
            } else if b[i] >= 0x20 || b[i] == b'\t' {
                out.push(b[i]);
            }
            i += 1;
        }
    }
    out
}

fn subst(s: &str, cfg: &Cfg) -> String {
    s.replace("{SSH}", crate::corpus::SSH)
        .replace("{FIX}", &cfg.fix)
        .replace("{HOME}", &cfg.home)
        .replace("{WORK}", &cfg.work)
        .replace("{PORT}", &cfg.port.to_string())
}

fn prep(p: Prep, cfg: &Cfg) -> anyhow::Result<Vec<Child>> {
    let home = &cfg.home;
    let port = cfg.port.to_string();
    let mut kids = Vec::new();
    match p {
        Prep::None => {}
        Prep::Http401 | Prep::Stall => {
            let mode = if p == Prep::Http401 { "401" } else { "stall" };
            kids.push(
                Command::new("python3")
                    .args(["-I", &format!("{}/servers.py", cfg.fix), mode, &port])
                    .stdin(Stdio::null())
                    .spawn()?,
            );
            std::thread::sleep(Duration::from_millis(500));
        }
        Prep::Sshd => {
            let dir = format!("{home}/sshd");
            std::fs::create_dir_all(&dir)?;
            let key = format!("{dir}/host_ed25519");
            if !std::path::Path::new(&key).exists() {
                Command::new("ssh-keygen")
                    .args(["-q", "-t", "ed25519", "-N", "", "-f", &key])
                    .status()?;
            }
            let conf = format!("{dir}/sshd_config");
            std::fs::write(
                &conf,
                format!(
                    "ListenAddress 127.0.0.1\nPort {port}\nHostKey {key}\nPasswordAuthentication yes\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nPidFile none\n"
                ),
            )?;
            let sshd = if std::path::Path::new("/usr/sbin/sshd").exists() {
                "/usr/sbin/sshd"
            } else {
                "sshd"
            };
            kids.push(
                Command::new(sshd)
                    .args(["-D", "-e", "-f", &conf])
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?,
            );
            std::thread::sleep(Duration::from_millis(700));
        }
        Prep::Fifo => {
            let _ = std::fs::remove_file(format!("{home}/fifo"));
            Command::new("mkfifo")
                .arg(format!("{home}/fifo"))
                .status()?;
        }
        Prep::BigFile => {
            let mut f = std::fs::File::create(format!("{home}/big.bin"))?;
            let block = vec![0x5au8; 1 << 20];
            for _ in 0..3072 {
                f.write_all(&block)?;
            }
            f.sync_all()?;
        }
        Prep::BigC => {
            let mut s = String::new();
            for i in 0..crate::BIG_C_FUNCTIONS {
                s.push_str(&format!(
                    "unsigned f{i}(unsigned x){{for(int j=0;j<64;j++){{x=x*2654435761u+{i}u;x^=x>>13;if(x&1)x+=j*{i}u;}}return x;}}\n"
                ));
            }
            std::fs::write(format!("{home}/big.c"), s)?;
        }
        Prep::Notes => {
            std::fs::write(format!("{home}/notes.txt"), "a note\n")?;
            let long: String = (1..5000).map(|i| format!("line {i}\n")).collect();
            std::fs::write(format!("{home}/long.txt"), long)?;
        }
    }
    Ok(kids)
}

fn unprep(p: Prep, cfg: &Cfg) {
    let home = &cfg.home;
    match p {
        Prep::BigFile => {
            let _ = std::fs::remove_file(format!("{home}/big.bin"));
            let _ = std::fs::remove_file(format!("{home}/big2.bin"));
        }
        Prep::BigC => {
            let _ = std::fs::remove_file(format!("{home}/big.o"));
        }
        _ => {}
    }
    let _ = std::fs::remove_file(format!("{}/dd.bin", cfg.work));
}

/// The unrelated position: this binary's `observe`, which leaves this
/// process's session first.
fn observe_unrelated(shell: u32, slave: &str, with_ps: bool) -> Value {
    let mut cmd = Command::new(std::env::current_exe().expect("own binary"));
    cmd.args(["observe", "--shell", &shell.to_string(), "--slave", slave]);
    if with_ps {
        cmd.arg("--ps");
    }
    match cmd.stdin(Stdio::null()).output() {
        Ok(o) => serde_json::from_slice(&o.stdout).unwrap_or_else(
            |e| json!({"error": format!("{e}: {}", String::from_utf8_lossy(&o.stderr))}),
        ),
        Err(e) => json!({"error": e.to_string()}),
    }
}

pub fn kill_all(io: &Io, shell: u32, tty: &obs::os::Tty) {
    use rustix::process::{Pid, Signal, kill_process, kill_process_group};
    if let Ok(fg) = rustix::termios::tcgetpgrp(&io.master) {
        if fg.as_raw_nonzero().get() as u32 != shell {
            let _ = kill_process_group(fg, Signal::KILL);
        }
    }
    for p in obs::os::session(tty) {
        if let Some(pid) = p["pid"].as_u64().and_then(|p| Pid::from_raw(p as i32)) {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
}

fn run_case(case: &Case, cfg: &Cfg, platform: &str) -> anyhow::Result<Vec<Value>> {
    let kids = prep(case.prep, cfg)?;
    let pty = crate::spawn::spawn(&shell_argv(), &shell_env(&cfg.home), &cfg.home)?;
    let shell = pty.child.id();
    let slave = pty.slave_path.clone();
    let tty = obs::open_tty(&slave)?;
    let mut child = pty.child;
    let mut io = Io {
        master: pty.master,
        buf: Vec::new(),
        last_out: Instant::now(),
    };
    // The prompt: quiet for 300 ms, within 5 s.
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        io.pump(Duration::from_millis(50));
        if !io.buf.is_empty() && io.last_out.elapsed() >= Duration::from_millis(300) {
            break;
        }
    }
    let cmd = subst(case.cmd, cfg);
    if !cmd.is_empty() {
        io.write(format!("{cmd}\r").as_bytes());
    }
    let start = Instant::now();
    io.pump(Duration::from_millis(30));
    let mut rows: Vec<Value> = Vec::new();
    let mut idx = 0;
    let mut restarts = 0;
    let mut last_sample = Instant::now();
    let mut prev: [Option<Value>; 2] = [None, None];
    loop {
        io.pump(Duration::from_millis(20));
        // Output that arrives while the job still holds the foreground (a
        // prompt that came late) restarts the clock; the shell's own prompt
        // after the job ended doesn't, so a job's working rows are kept.
        let job_holds = rustix::termios::tcgetpgrp(&io.master)
            .is_ok_and(|p| p.as_raw_nonzero().get() as u32 != shell);
        if idx > 0
            && io.last_out > last_sample
            && start.elapsed() < RESTART_WINDOW
            && restarts < 4
            && job_holds
        {
            rows.clear();
            prev = [None, None];
            idx = 0;
            restarts += 1;
        }
        let quiet = io.last_out.elapsed();
        let timed_out = start.elapsed() > CAP;
        if quiet >= Duration::from_millis(LABELS[idx].0) || timed_out {
            let label = LABELS[idx].1;
            let s4 = io.s4();
            let since_cmd = start.elapsed().as_millis() as u64;
            let with_ps = label == "2s";
            let anc = obs::snapshot(From::Master(io.master.as_fd()), &tty, PAIR, with_ps);
            let unrel = observe_unrelated(shell, &slave, false);
            for (i, (pos, snap)) in [("ancestor", anc), ("unrelated", unrel)]
                .into_iter()
                .enumerate()
            {
                let since_prev = prev[i].as_ref().map(|p| obs::delta(&p["s3"], &snap["s3"]));
                prev[i] = Some(snap.clone());
                let running = snap["fg"].as_u64().is_some_and(|f| f as u32 != shell);
                rows.push(json!({
                    "id": format!("{platform}/{}/{}/{}/{pos}", cfg.scope, case.id, label),
                    "platform": platform, "scope": cfg.scope, "case": case.id, "cat": case.cat,
                    "expect_blocked": case.expect, "label": label, "reached": !timed_out,
                    "since_cmd_ms": since_cmd, "restarts": restarts, "position": pos,
                    "shell_pid": shell, "cmd_running": running, "s4": s4,
                    "s3_since_prev": since_prev, "obs": snap,
                }));
            }
            last_sample = Instant::now();
            idx += 1;
            if idx == LABELS.len() {
                break;
            }
        }
    }
    kill_all(&io, shell, &tty);
    let _ = rustix::process::kill_process(
        rustix::process::Pid::from_raw(shell as i32).context("pid")?,
        rustix::process::Signal::KILL,
    );
    drop(io);
    let _ = child.wait();
    for mut k in kids {
        let _ = k.kill();
        let _ = k.wait();
    }
    unprep(case.prep, cfg);
    Ok(rows)
}

pub fn run(cfg: &Cfg, out: &str) -> anyhow::Result<()> {
    let platform = if cfg!(target_os = "linux") {
        "linux"
    } else {
        "macos"
    };
    let mut rows = Vec::new();
    let mut not_covered = Vec::new();
    std::fs::create_dir_all(&cfg.home)?;
    for case in CASES {
        if cfg
            .only
            .as_ref()
            .is_some_and(|o| !o.iter().any(|x| x == case.id))
        {
            continue;
        }
        if !os_ok(case.os) {
            if !CASES.iter().any(|c| c.id == case.id && os_ok(c.os)) {
                not_covered.push(json!({"case": case.id, "why": "not this platform"}));
            }
            continue;
        }
        if let Some(m) = case.needs.iter().find(|p| !which(p)) {
            not_covered.push(json!({"case": case.id, "why": format!("{m} not installed")}));
            continue;
        }
        if case.id == "go-read"
            && !std::path::Path::new(&format!("{}/bin/goread", cfg.fix)).exists()
        {
            not_covered.push(json!({"case": case.id, "why": "goread not built"}));
            continue;
        }
        eprintln!("case {}", case.id);
        match run_case(case, cfg, platform) {
            Ok(r) => rows.extend(r),
            Err(e) => not_covered.push(json!({"case": case.id, "why": format!("{e:#}")})),
        }
    }
    let doc = json!({"meta": crate::meta(&cfg.scope), "not_covered": not_covered, "rows": rows});
    std::fs::write(out, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

#[allow(dead_code)]
pub fn read_all(r: &mut impl Read) -> String {
    let mut s = String::new();
    let _ = r.read_to_string(&mut s);
    s
}

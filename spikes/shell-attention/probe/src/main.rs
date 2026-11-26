//! The shell-attention probe: can a PTY's holder tell that a shell command is
//! waiting on the user at the terminal, working, or waiting on something else?
//! `README.md` is the report; `run.sh` reruns everything.
//!
//! - `run`: the corpus, every signal at three quiet points from two positions.
//! - `yama`: which `/proc` (or `proc_info`) reads each position may make, per
//!   target.
//! - `cost`: what one observation of a group of 1, 10 and 100 processes costs.
//! - `observe`, `access`, `pty-child`: steps the others spawn.

#![allow(dead_code)]

mod corpus;
mod count;
mod driver;
mod obs;
#[cfg(target_os = "linux")]
pub mod obs_linux;
#[cfg(target_os = "macos")]
pub mod obs_macos;
mod spawn;
mod term;

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{Value, json};

/// The size of `compile`'s generated C file, chosen so `cc -O2` runs past
/// the 10 s point on both machines.
pub const BIG_C_FUNCTIONS: usize = 40_000;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn sh(cmd: &str) -> String {
    std::process::Command::new("sh")
        .args(["-c", cmd])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn meta(scope: &str) -> Value {
    json!({
        "uname": sh("uname -srm"),
        "date": sh("date -u +%Y-%m-%dT%H:%M:%SZ"),
        "uid": rustix::process::getuid().as_raw(),
        "scope": scope,
        "yama_ptrace_scope": std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope").map(|s| s.trim().to_string()).unwrap_or_else(|_| "n/a".into()),
        // SAFETY: `sysconf` reads a constant.
        "clk_tck": unsafe { libc::sysconf(libc::_SC_CLK_TCK) },
        "versions": {
            "bash": sh("bash --version | head -1"),
            "sudo": sh("sudo -V 2>/dev/null | head -1"),
            "ssh": sh("ssh -V 2>&1"),
            "git": sh("git --version"),
            "python3": sh("python3 --version"),
            "node": sh("node --version"),
            "zsh": sh("zsh --version"),
            "vim": sh("vim --version | head -1"),
            "less": sh("less --version | head -1"),
            "fzf": sh("fzf --version"),
            "gum": sh("gum --version"),
            "cc": sh("cc --version | head -1"),
        },
    })
}

fn wait_prompt(io: &mut driver::Io) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        io.pump(Duration::from_millis(50));
        if !io.buf.is_empty() && io.last_out.elapsed() >= Duration::from_millis(300) {
            break;
        }
    }
}

struct Tab {
    io: driver::Io,
    shell: u32,
    slave: String,
    tty: obs::os::Tty,
    child: std::process::Child,
}

fn tab(home: &str, cmd: &str, settle: Duration) -> anyhow::Result<Tab> {
    std::fs::create_dir_all(home)?;
    let pty = spawn::spawn(&driver::shell_argv(), &driver::shell_env(home), home)?;
    let tty = obs::open_tty(&pty.slave_path)?;
    let mut io = driver::Io {
        master: pty.master,
        buf: Vec::new(),
        last_out: Instant::now(),
    };
    wait_prompt(&mut io);
    io.write(format!("{cmd}\r").as_bytes());
    io.pump(settle);
    Ok(Tab {
        io,
        shell: pty.child.id(),
        slave: pty.slave_path,
        tty,
        child: pty.child,
    })
}

fn close(mut t: Tab) {
    driver::kill_all(&t.io, t.shell, &t.tty);
    let _ = t.child.kill();
    drop(t.io);
    let _ = t.child.wait();
}

fn fg_members(t: &Tab) -> Vec<Value> {
    let fg = rustix::termios::tcgetpgrp(&t.io.master)
        .map(|p| p.as_raw_nonzero().get() as u32)
        .unwrap_or(0);
    obs::group(fg, &t.tty, false).0
}

fn unrelated_access(pid: u32) -> Value {
    let out = std::process::Command::new(std::env::current_exe().expect("own binary"))
        .args(["access", "--pid", &pid.to_string()])
        .output();
    out.ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or(json!({"error": "observer failed"}))
}

fn yama_target(
    rows: &mut Vec<Value>,
    scope: &str,
    home: &str,
    target: &str,
    cmd: &str,
    pick: &dyn Fn(&Tab) -> Option<Value>,
) {
    let t = match tab(home, cmd, Duration::from_millis(1500)) {
        Ok(t) => t,
        Err(e) => {
            rows.push(json!({"scope": scope, "target": target, "error": format!("{e:#}")}));
            return;
        }
    };
    match pick(&t) {
        Some(m) => {
            let pid = m["pid"].as_u64().unwrap_or(0) as u32;
            let info = json!({"pid": pid, "comm": m["comm"], "uid": m["uid"], "ppid": m["ppid"], "shell": t.shell, "prober": std::process::id()});
            rows.push(json!({"scope": scope, "target": target, "position": "ancestor", "target_info": info, "cells": obs::os::access(pid)}));
            rows.push(json!({"scope": scope, "target": target, "position": "unrelated", "target_info": info, "cells": unrelated_access(pid)}));
        }
        None => rows.push(json!({"scope": scope, "target": target, "error": "target not found", "screen": String::from_utf8_lossy(&t.io.buf).to_string()})),
    }
    close(t);
}

fn yama(scope: &str, home: &str, out: &str) -> anyhow::Result<()> {
    let mut rows = Vec::new();
    // The foreground group's leader: the job's first process, whatever the
    // caller may read of it (a setuid program's name can be refused).
    let first = |_name: &'static str| {
        move |t: &Tab| {
            let fg = rustix::termios::tcgetpgrp(&t.io.master)
                .map(|p| p.as_raw_nonzero().get() as u32)
                .unwrap_or(0);
            fg_members(t)
                .into_iter()
                .find(|m| m["pid"].as_u64() == Some(u64::from(fg)))
        }
    };
    let orphan = |t: &Tab| {
        // A process whose parent exited: found by its controlling terminal.
        obs::os::session(&t.tty)
            .into_iter()
            .find(|p| p["comm"] == "sleep")
            .map(|p| {
                let pid = p["pid"].as_u64().unwrap_or(0) as u32;
                #[cfg(target_os = "linux")]
                return obs::os::member(pid, &t.tty);
                #[cfg(target_os = "macos")]
                return obs::os::member(pid, &t.tty, false);
            })
    };
    yama_target(
        &mut rows,
        scope,
        home,
        "same-user",
        "sleep 300",
        &first("sleep"),
    );
    if driver::which("sudo") {
        yama_target(
            &mut rows,
            scope,
            home,
            "root-owned",
            "sudo -k true",
            &first("sudo"),
        );
    }
    yama_target(
        &mut rows,
        scope,
        home,
        "reparented",
        "sh -c 'sleep 301 & exit 0'",
        &orphan,
    );
    #[cfg(target_os = "linux")]
    {
        // The jar could make itself the subreaper: an orphan then comes to it.
        rustix::process::set_child_subreaper(Some(rustix::process::getpid()))
            .context("PR_SET_CHILD_SUBREAPER")?;
        yama_target(
            &mut rows,
            scope,
            home,
            "reparented-to-subreaper",
            "sh -c 'sleep 301 & exit 0'",
            &orphan,
        );
    }
    let doc = json!({"meta": meta(scope), "rows": rows});
    std::fs::write(out, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

fn percentile(v: &mut [u64], p: f64) -> u64 {
    v.sort_unstable();
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

/// The syscalls one unrelated `observe` makes between its marks, by
/// `strace`, where it is installed: a check on the instrumented count.
fn strace_count(shell: u32, slave: &str, n: usize) -> Value {
    if !driver::which("strace") {
        return json!(null);
    }
    let file = format!("/work/strace-{n}.txt");
    let exe = std::env::current_exe().expect("own binary");
    let _ = std::process::Command::new("strace")
        .args(["-f", "-o", &file])
        .arg(&exe)
        .args(["observe", "--shell", &shell.to_string(), "--slave", slave])
        .env("SA_MARK", "1")
        .output();
    let Ok(text) = std::fs::read_to_string(&file) else {
        return json!(null);
    };
    let lines: Vec<&str> = text.lines().collect();
    let begin = lines.iter().position(|l| l.contains("/sa-mark-begin"));
    let end = lines.iter().position(|l| l.contains("/sa-mark-end"));
    match (begin, end) {
        (Some(b), Some(e)) => {
            let between = &lines[b + 1..e];
            let mut by: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
            for l in between {
                let name = l
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.split('(').next())
                    .unwrap_or("?")
                    .to_string();
                *by.entry(name).or_default() += 1;
            }
            json!({"total": between.len(), "by_syscall": by})
        }
        _ => json!(null),
    }
}

fn cost(home: &str, out: &str) -> anyhow::Result<()> {
    let mut rows = Vec::new();
    for n in [1usize, 10, 100] {
        let cmd = if n == 1 {
            "sleep 300".to_string()
        } else {
            format!(
                "sh -c 'for i in $(seq 1 {}); do sleep 300 & done; sleep 300'",
                n - 2
            )
        };
        let t = tab(home, &cmd, Duration::from_millis(500))?;
        let until = Instant::now() + Duration::from_secs(15);
        while fg_members(&t).len() < n && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
        let members = fg_members(&t).len();
        let mut full = Vec::new();
        let mut known = Vec::new();
        let (mut calls_full, mut calls_known, mut scanned) = (0, 0, 0);
        let fg = rustix::termios::tcgetpgrp(&t.io.master)
            .map(|p| p.as_raw_nonzero().get() as u32)
            .unwrap_or(0);
        let pids = obs::os::group_pids(fg).0;
        for i in 0..53 {
            count::take();
            let t0 = Instant::now();
            count::add(1);
            let _ = rustix::termios::tcgetpgrp(&t.io.master);
            let _ = term::settings(t.io.master.as_fd());
            let (_, s) = obs::group(fg, &t.tty, false);
            let us = t0.elapsed().as_micros() as u64;
            let c = count::take();
            count::take();
            let t1 = Instant::now();
            for p in &pids {
                #[cfg(target_os = "linux")]
                let _ = obs::os::member(*p, &t.tty);
                #[cfg(target_os = "macos")]
                let _ = obs::os::member(*p, &t.tty, false);
            }
            let us_known = t1.elapsed().as_micros() as u64;
            let ck = count::take();
            if i >= 3 {
                full.push(us);
                known.push(us_known);
                (calls_full, calls_known, scanned) = (c, ck, s);
            }
        }
        let strace = strace_count(t.shell, &t.slave, n);
        rows.push(json!({
            "n": n, "members": members, "strace_calls_unrelated_first_read": strace, "proc_entries_scanned": scanned,
            "full_us_median": percentile(&mut full, 0.5), "full_us_p90": percentile(&mut full, 0.9), "full_calls": calls_full,
            "members_only_us_median": percentile(&mut known, 0.5), "members_only_us_p90": percentile(&mut known, 0.9), "members_only_calls": calls_known,
        }));
        close(t);
    }
    let doc = json!({"meta": meta("n/a"), "rows": rows});
    std::fs::write(out, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(String::as_str).unwrap_or("");
    match sub {
        "pty-child" => spawn::child(&args[3..]),
        "observe" => {
            // Out of the prober's session: an unrelated reader by pid.
            let _ = rustix::process::setsid();
            let shell: u32 = arg(&args, "--shell").context("--shell")?.parse()?;
            let slave = arg(&args, "--slave").context("--slave")?;
            let tty = obs::open_tty(&slave)?;
            let snap = obs::snapshot::<std::os::fd::BorrowedFd<'_>>(
                obs::From::Pid {
                    shell,
                    slave: &slave,
                },
                &tty,
                Duration::from_millis(100),
                args.iter().any(|a| a == "--ps"),
            );
            println!("{snap}");
            Ok(())
        }
        "access" => {
            let _ = rustix::process::setsid();
            let pid: u32 = arg(&args, "--pid").context("--pid")?.parse()?;
            println!("{}", obs::os::access(pid));
            Ok(())
        }
        "run" => {
            let cfg = driver::Cfg {
                fix: arg(&args, "--fix").context("--fix")?,
                home: arg(&args, "--home").context("--home")?,
                work: arg(&args, "--work").unwrap_or_else(|| "/tmp".into()),
                scope: arg(&args, "--scope").unwrap_or_else(|| "n/a".into()),
                only: arg(&args, "--only").map(|s| s.split(',').map(str::to_string).collect()),
                port: arg(&args, "--port").map_or(Ok(47123), |p| p.parse())?,
            };
            driver::run(&cfg, &arg(&args, "--out").context("--out")?)
        }
        "yama" => yama(
            &arg(&args, "--scope").unwrap_or_else(|| "n/a".into()),
            &arg(&args, "--home").context("--home")?,
            &arg(&args, "--out").context("--out")?,
        ),
        "cost" => cost(
            &arg(&args, "--home").context("--home")?,
            &arg(&args, "--out").context("--out")?,
        ),
        _ => anyhow::bail!("usage: probe run|yama|cost|observe|access|pty-child …"),
    }
}

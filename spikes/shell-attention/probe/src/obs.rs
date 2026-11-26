//! One observation of the foreground group from either position, and the S3
//! summary over its members.

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[cfg(target_os = "linux")]
pub use crate::obs_linux as os;
#[cfg(target_os = "macos")]
pub use crate::obs_macos as os;

pub fn open_tty(path: &str) -> anyhow::Result<os::Tty> {
    os::tty(path)
}

/// Every member of group `pgid`.
pub fn group(pgid: u32, tty: &os::Tty, with_ps: bool) -> (Vec<Value>, usize) {
    let (pids, scanned) = os::group_pids(pgid);
    #[cfg(target_os = "linux")]
    let _ = with_ps;
    #[cfg(target_os = "linux")]
    let members = pids.iter().map(|p| os::member(*p, tty)).collect();
    #[cfg(target_os = "macos")]
    let members = pids.iter().map(|p| os::member(*p, tty, with_ps)).collect();
    (members, scanned)
}

fn n(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| v.as_u64().map(|u| u as i64))
        .unwrap_or(0)
}

/// CPU, disk and the other counters, summed over the members; `unreadable`
/// names a member whose counter couldn't be read.
pub fn s3(members: &[Value]) -> Value {
    let (mut cpu, mut run, mut disk, mut chars, mut sys, mut csw, mut instr) =
        (0i64, 0i64, 0i64, 0i64, 0i64, 0i64, 0i64);
    let mut unreadable = Vec::new();
    for m in members {
        if cfg!(target_os = "linux") {
            // utime … cstime are clock ticks of 1/USER_HZ = 10 ms.
            cpu +=
                (n(&m["utime"]) + n(&m["stime"]) + n(&m["cutime"]) + n(&m["cstime"])) * 10_000_000;
            run += n(&m["run_ns"]);
            if m["io"].get("error").is_some() {
                unreadable.push(json!({"pid": m["pid"], "io": m["io"]["error"]}));
            }
            disk += n(&m["io"]["read_bytes"]) + n(&m["io"]["write_bytes"]);
            chars += n(&m["io"]["rchar"]) + n(&m["io"]["wchar"]);
            sys += n(&m["io"]["syscr"]) + n(&m["io"]["syscw"]);
            csw += n(&m["vcsw"]) + n(&m["nvcsw"]);
        } else {
            let r = &m["rusage"];
            if r.get("error").is_some() {
                unreadable.push(json!({"pid": m["pid"], "rusage": r["error"]}));
            }
            cpu += n(&r["user_ns"])
                + n(&r["system_ns"])
                + n(&r["child_user_ns"])
                + n(&r["child_system_ns"]);
            run += n(&m["task"]["total_user_ns"]) + n(&m["task"]["total_system_ns"]);
            disk += n(&r["diskio_read"]) + n(&r["diskio_written"]);
            chars += n(&r["logical_writes"]);
            instr += n(&r["instructions"]);
            sys += n(&m["task"]["syscalls_unix"]) + n(&m["task"]["syscalls_mach"]);
            csw += n(&m["task"]["csw"]);
        }
    }
    json!({"cpu_ns": cpu, "run_ns": run, "disk_bytes": disk, "chars": chars, "syscalls": sys,
           "csw": csw, "instructions": instr, "unreadable": unreadable})
}

/// With `SA_MARK` set, a failing `access` on `path`: a line `strace` shows,
/// so the cost table can count the syscalls between two marks exactly.
fn mark(path: &str) {
    if std::env::var_os("SA_MARK").is_some() {
        let _ = rustix::fs::access(path, rustix::fs::Access::EXISTS);
    }
}

pub fn delta(a: &Value, b: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for k in [
        "cpu_ns",
        "run_ns",
        "disk_bytes",
        "chars",
        "syscalls",
        "csw",
        "instructions",
    ] {
        out.insert(k.to_string(), json!(n(&b[k]) - n(&a[k])));
    }
    Value::Object(out)
}

/// Where the foreground group and the terminal settings come from: the
/// master, held only by the ancestor, or by pid and slave path.
pub enum From<'a, F: AsFd> {
    Master(F),
    Pid { shell: u32, slave: &'a str },
}

/// The foreground group, the settings, every member, the session's
/// processes, and an S3 pair taken `pair` apart; the first read's cost.
pub fn snapshot<F: AsFd>(from: From<'_, F>, tty: &os::Tty, pair: Duration, with_ps: bool) -> Value {
    crate::count::take();
    mark("/sa-mark-begin");
    let t0 = Instant::now();
    let (fg, s1, extra) = match &from {
        From::Master(m) => {
            crate::count::add(1);
            let fg = rustix::termios::tcgetpgrp(m)
                .map(|p| json!(p.as_raw_nonzero().get()))
                .unwrap_or_else(|e| json!(crate::count::errno(&e.into())));
            (fg, crate::term::settings(m), json!({}))
        }
        From::Pid { shell, slave } => {
            let fg = os::tpgid(*shell)
                .map(|p| json!(p))
                .unwrap_or_else(|e| json!(e));
            crate::count::add(3);
            match rustix::fs::open(
                *slave,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NOCTTY
                    | rustix::fs::OFlags::NONBLOCK,
                rustix::fs::Mode::empty(),
            ) {
                Ok(fd) => {
                    let pg = rustix::termios::tcgetpgrp(&fd)
                        .map(|p| json!(p.as_raw_nonzero().get()))
                        .unwrap_or_else(|e| json!(crate::count::errno(&e.into())));
                    (
                        fg,
                        crate::term::settings(&fd),
                        json!({"tcgetpgrp_on_slave": pg}),
                    )
                }
                Err(e) => (
                    fg,
                    json!({"error": crate::count::errno(&e.into())}),
                    json!({}),
                ),
            }
        }
    };
    let pgid = fg.as_u64().unwrap_or(0) as u32;
    let (mut members, scanned) = if pgid > 0 {
        group(pgid, tty, false)
    } else {
        (Vec::new(), 0)
    };
    let first_us = t0.elapsed().as_micros() as u64;
    mark("/sa-mark-end");
    let calls = crate::count::take();
    if with_ps {
        // What `ps` makes of the wait channel, outside the timed read.
        for m in &mut members {
            let pid = m["pid"].to_string();
            let out = std::process::Command::new("ps")
                .args(["-o", "wchan=,state=", "-p", &pid])
                .output();
            m["ps_wchan_state"] = json!(
                out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default()
            );
        }
    }
    let session = os::session(tty);
    let a = s3(&members);
    std::thread::sleep(pair);
    let (members_b, _) = if pgid > 0 {
        group(pgid, tty, false)
    } else {
        (Vec::new(), 0)
    };
    let b = s3(&members_b);
    json!({
        "fg": fg, "s1": s1, "extra": extra, "members": members, "session": session,
        "s3": a, "s3_pair_delta": delta(&a, &b), "pair_ms": pair.as_millis() as u64,
        "cost": {"us": first_us, "calls": calls, "proc_scanned": scanned},
    })
}

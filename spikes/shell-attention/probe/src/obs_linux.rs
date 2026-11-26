//! S2 and S3 on Linux, read by pid from `/proc`: no `ptrace`, no
//! `/proc/<pid>/mem`, and not `cmdline` or `environ` either, which the
//! kernel serves by reading the process's memory.

use serde_json::{Value, json};

use crate::count::{list_dir, read_file, readlink};

/// The PTY's slave as `/proc` names it: its path, and its `tty_nr` as
/// `/proc/<pid>/stat` encodes a device.
pub struct Tty {
    pub path: String,
    pub tty_nr: u64,
}

pub fn tty(path: &str) -> anyhow::Result<Tty> {
    let st = rustix::fs::stat(path)?;
    let (maj, min) = (
        u64::from(rustix::fs::major(st.st_rdev)),
        u64::from(rustix::fs::minor(st.st_rdev)),
    );
    Ok(Tty {
        path: path.to_string(),
        tty_nr: (min & 0xff) | (maj << 8) | ((min & !0xff) << 12),
    })
}

/// The fields of a `stat` file after `comm`, numbered from 0 at `state`
/// (field 3 in proc(5)), and `comm`.
fn stat(path: &str) -> Result<(String, Vec<String>), String> {
    let s = read_file(path)?;
    let open = s.find('(').ok_or("no comm")?;
    let close = s.rfind(')').ok_or("no comm")?;
    let comm = s[open + 1..close].to_string();
    Ok((
        comm,
        s[close + 1..]
            .split_whitespace()
            .map(str::to_string)
            .collect(),
    ))
}

fn num(f: &[String], i: usize) -> u64 {
    f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// The foreground group of `pid`'s terminal, as any process may read it.
pub fn tpgid(pid: u32) -> Result<u32, String> {
    let (_, f) = stat(&format!("/proc/{pid}/stat"))?;
    Ok(num(&f, 5) as u32)
}

/// Every pid whose group is `pgid`, and how many `/proc` entries were read
/// to find them.
pub fn group_pids(pgid: u32) -> (Vec<u32>, usize) {
    let mut scanned = 0;
    let mut out = Vec::new();
    for name in list_dir("/proc").unwrap_or_default() {
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        scanned += 1;
        if let Ok((_, f)) = stat(&format!("/proc/{pid}/stat")) {
            if num(&f, 2) == u64::from(pgid) {
                out.push(pid);
            }
        }
    }
    (out, scanned)
}

/// Every process whose controlling terminal is the slave: a stopped
/// background job is found here, never in the foreground group.
pub fn session(tty: &Tty) -> Vec<Value> {
    let mut out = Vec::new();
    for name in list_dir("/proc").unwrap_or_default() {
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if let Ok((comm, f)) = stat(&format!("/proc/{pid}/stat")) {
            if num(&f, 4) == tty.tty_nr {
                out.push(json!({"pid": pid, "comm": comm, "state": f[0], "pgrp": num(&f, 2)}));
            }
        }
    }
    out
}

fn sys_name(nr: i64) -> &'static str {
    #[cfg(target_arch = "aarch64")]
    let table: &[(i64, &str)] = &[
        (63, "read"),
        (64, "write"),
        (65, "readv"),
        (67, "pread64"),
        (72, "pselect6"),
        (73, "ppoll"),
        (22, "epoll_pwait"),
        (441, "epoll_pwait2"),
        (101, "nanosleep"),
        (115, "clock_nanosleep"),
        (260, "wait4"),
        (95, "waitid"),
        (98, "futex"),
        (202, "accept"),
        (242, "accept4"),
        (207, "recvfrom"),
        (212, "recvmsg"),
        (56, "openat"),
        (133, "rt_sigsuspend"),
        (137, "rt_sigtimedwait"),
        (25, "fcntl"),
        (32, "flock"),
        (203, "connect"),
        (29, "ioctl"),
        (285, "copy_file_range"),
        (71, "sendfile"),
    ];
    #[cfg(target_arch = "x86_64")]
    let table: &[(i64, &str)] = &[
        (0, "read"),
        (1, "write"),
        (19, "readv"),
        (17, "pread64"),
        (23, "select"),
        (7, "poll"),
        (270, "pselect6"),
        (271, "ppoll"),
        (232, "epoll_wait"),
        (281, "epoll_pwait"),
        (441, "epoll_pwait2"),
        (35, "nanosleep"),
        (230, "clock_nanosleep"),
        (61, "wait4"),
        (247, "waitid"),
        (202, "futex"),
        (43, "accept"),
        (288, "accept4"),
        (45, "recvfrom"),
        (47, "recvmsg"),
        (257, "openat"),
        (2, "open"),
        (34, "pause"),
        (130, "rt_sigsuspend"),
        (128, "rt_sigtimedwait"),
        (72, "fcntl"),
        (73, "flock"),
        (42, "connect"),
        (16, "ioctl"),
        (326, "copy_file_range"),
        (40, "sendfile"),
    ];
    table.iter().find(|(n, _)| *n == nr).map_or("?", |(_, s)| s)
}

/// Whether a descriptor's target is this terminal: the slave, or `/dev/tty`
/// (what `ssh`, `getpass` and `less` open).
fn is_tty(target: &str, tty: &Tty) -> bool {
    target == tty.path || target == "/dev/tty"
}

/// `/proc/<pid>/task/<tid>/syscall`, parsed, with what can be learnt about
/// its descriptor argument from `fd/` and `fdinfo/` without memory.
fn syscall(pid: u32, tid: &str, tty: &Tty) -> Value {
    let raw = match read_file(&format!("/proc/{pid}/task/{tid}/syscall")) {
        Ok(r) => r.trim().to_string(),
        Err(e) => return json!({"error": e}),
    };
    if raw == "running" {
        return json!({"raw": raw, "name": "running"});
    }
    let parts: Vec<&str> = raw.split_whitespace().collect();
    let nr: i64 = parts.first().and_then(|v| v.parse().ok()).unwrap_or(-1);
    let arg = |i: usize| -> u64 {
        parts
            .get(i + 1)
            .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0)
    };
    let name = if nr < 0 { "none" } else { sys_name(nr) };
    let mut v = json!({"nr": nr, "name": name, "arg0": arg(0), "arg1": arg(1)});
    match name {
        "read" | "readv" | "pread64" => {
            let t = readlink(&format!("/proc/{pid}/fd/{}", arg(0)));
            v["fd_target"] = json!(t.clone().unwrap_or_else(|e| e));
            v["fd_is_tty"] = json!(t.is_ok_and(|t| is_tty(&t, tty)));
        }
        "select" | "pselect6" => {
            // nfds is the highest descriptor plus one: what the process holds
            // below it is the most the set can contain.
            v["nfds"] = json!(arg(0));
            let below: Vec<Value> = (0..arg(0).min(64))
                .filter_map(|fd| {
                    readlink(&format!("/proc/{pid}/fd/{fd}"))
                        .ok()
                        .map(|t| json!({"fd": fd, "tty": is_tty(&t, tty), "target": t}))
                })
                .collect();
            v["fds_below_nfds"] = json!(below);
        }
        "poll" | "ppoll" => v["nfds"] = json!(arg(1)),
        "epoll_wait" | "epoll_pwait" | "epoll_pwait2" => {
            v["epfd"] = json!(arg(0));
            v["epoll_targets"] = epoll_targets(pid, arg(0), tty);
        }
        _ => {}
    }
    v
}

fn epoll_targets(pid: u32, epfd: u64, tty: &Tty) -> Value {
    let info = match read_file(&format!("/proc/{pid}/fdinfo/{epfd}")) {
        Ok(i) => i,
        Err(e) => return json!({"error": e}),
    };
    let mut out = Vec::new();
    for line in info.lines().filter(|l| l.starts_with("tfd:")) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let tfd = w.get(1).copied().unwrap_or("?");
        let events = w.get(3).copied().unwrap_or("?");
        let t = readlink(&format!("/proc/{pid}/fd/{tfd}")).unwrap_or_else(|e| e);
        out.push(json!({"tfd": tfd, "events": events, "tty": is_tty(&t, tty), "target": t}));
    }
    json!(out)
}

/// Everything S2 and S3 read for one process.
pub fn member(pid: u32, tty: &Tty) -> Value {
    let mut v = json!({"pid": pid});
    match stat(&format!("/proc/{pid}/stat")) {
        Ok((comm, f)) => {
            v["comm"] = json!(comm);
            v["state"] = json!(f[0]);
            v["ppid"] = json!(num(&f, 1));
            v["utime"] = json!(num(&f, 11));
            v["stime"] = json!(num(&f, 12));
            v["cutime"] = json!(num(&f, 13));
            v["cstime"] = json!(num(&f, 14));
        }
        Err(e) => v["stat_error"] = json!(e),
    }
    match read_file(&format!("/proc/{pid}/status")) {
        Ok(s) => {
            for l in s.lines() {
                let mut kv = l.splitn(2, ':');
                let (k, val) = (kv.next().unwrap_or(""), kv.next().unwrap_or("").trim());
                match k {
                    "Uid" => v["uid"] = json!(val.split_whitespace().nth(1).unwrap_or("?")),
                    "voluntary_ctxt_switches" => v["vcsw"] = json!(val.parse::<u64>().unwrap_or(0)),
                    "nonvoluntary_ctxt_switches" => {
                        v["nvcsw"] = json!(val.parse::<u64>().unwrap_or(0))
                    }
                    _ => {}
                }
            }
        }
        Err(e) => v["status_error"] = json!(e),
    }
    match read_file(&format!("/proc/{pid}/io")) {
        Ok(s) => {
            let mut io = serde_json::Map::new();
            for l in s.lines() {
                if let Some((k, val)) = l.split_once(':') {
                    io.insert(k.to_string(), json!(val.trim().parse::<u64>().unwrap_or(0)));
                }
            }
            v["io"] = Value::Object(io);
        }
        Err(e) => v["io"] = json!({"error": e}),
    }
    match read_file(&format!("/proc/{pid}/schedstat")) {
        Ok(s) => {
            v["run_ns"] = json!(
                s.split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(0)
            )
        }
        Err(e) => v["run_ns_error"] = json!(e),
    }
    match list_dir(&format!("/proc/{pid}/fd")) {
        Ok(fds) => {
            let mut tty_fds = Vec::new();
            for fd in &fds {
                if let Ok(t) = readlink(&format!("/proc/{pid}/fd/{fd}")) {
                    if is_tty(&t, tty) {
                        tty_fds.push(json!({"fd": fd, "target": t}));
                    }
                }
            }
            v["fds"] = json!({"count": fds.len(), "tty": tty_fds});
        }
        Err(e) => v["fds"] = json!({"error": e}),
    }
    let mut threads = Vec::new();
    for tid in list_dir(&format!("/proc/{pid}/task")).unwrap_or_default() {
        let state = stat(&format!("/proc/{pid}/task/{tid}/stat"))
            .map(|(_, f)| f[0].clone())
            .unwrap_or_else(|e| e);
        let wchan =
            read_file(&format!("/proc/{pid}/task/{tid}/wchan")).unwrap_or_else(|e| format!("!{e}"));
        threads.push(
            json!({"tid": tid, "state": state, "wchan": wchan, "syscall": syscall(pid, &tid, tty)}),
        );
    }
    v["threads"] = json!(threads);
    v
}

/// What each `/proc` file the signals use answers this caller: the Yama
/// matrix's cells.
pub fn access(pid: u32) -> Value {
    let try_read = |p: &str| read_file(p).map(|_| "ok".to_string()).unwrap_or_else(|e| e);
    let try_link = |p: &str| readlink(p).map(|_| "ok".to_string()).unwrap_or_else(|e| e);
    let try_dir = |p: &str| list_dir(p).map(|_| "ok".to_string()).unwrap_or_else(|e| e);
    json!({
        "stat": try_read(&format!("/proc/{pid}/stat")),
        "status": try_read(&format!("/proc/{pid}/status")),
        "wchan": try_read(&format!("/proc/{pid}/wchan")),
        "syscall": try_read(&format!("/proc/{pid}/syscall")),
        "task_syscall": try_read(&format!("/proc/{pid}/task/{pid}/syscall")),
        "fd_list": try_dir(&format!("/proc/{pid}/fd")),
        "fd0_link": try_link(&format!("/proc/{pid}/fd/0")),
        "fdinfo0": try_read(&format!("/proc/{pid}/fdinfo/0")),
        "io": try_read(&format!("/proc/{pid}/io")),
        "schedstat": try_read(&format!("/proc/{pid}/schedstat")),
    })
}

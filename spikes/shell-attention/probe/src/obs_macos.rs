//! S2m and S3 on macOS, read by pid through `libproc`'s `proc_info` calls,
//! the only process introspection a same-user, unprivileged, untraced caller
//! has: no `task_for_pid`, no dtrace, no stackshot. Each call below is one
//! `proc_info` syscall. The probe calls them through `libc`; the jar, which
//! forbids `unsafe`, would go through the `libproc` crate (README, item 5).

use std::ffi::CStr;

use serde_json::{Value, json};

use crate::count::add;

const PROC_PGRP_ONLY: u32 = 2;
const PROC_TTY_ONLY: u32 = 3;
const PROC_PIDLISTFDS: i32 = 1;
const PROC_PIDTASKALLINFO: i32 = 2;
const PROC_PIDTBSDINFO: i32 = 3;
const PROC_PIDTHREADINFO: i32 = 5;
const PROC_PIDLISTTHREADS: i32 = 6;
const PROC_PIDFDVNODEPATHINFO: i32 = 2;
const PROC_PIDFDKQUEUE_EXTINFO: i32 = 9;
const PROX_FDTYPE_VNODE: u32 = 1;
const PROX_FDTYPE_KQUEUE: u32 = 5;

#[repr(C)]
struct ProcFileinfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: i64,
    fi_type: i32,
    fi_guardflags: u32,
}

/// `struct vnode_fdinfowithpath`, which `libc` doesn't declare.
#[repr(C)]
struct VnodeFdInfoWithPath {
    pfi: ProcFileinfo,
    pvip: libc::vnode_info_path,
}

/// `struct kevent_extinfo`: one knote of a kqueue.
#[repr(C)]
#[derive(Clone, Copy)]
struct KeventExtinfo {
    ident: u64,
    filter: i16,
    flags: u16,
    qos: i32,
    udata: u64,
    fflags: u32,
    xflags: u32,
    data: i64,
    ext: [u64; 4],
    sdata: u64,
    status: i32,
    sfflags: i32,
    reserved: [u64; 2],
}

fn errno() -> String {
    crate::count::errno(&std::io::Error::last_os_error())
}

fn pidinfo<T>(pid: i32, flavor: i32, arg: u64) -> Result<T, String> {
    add(1);
    let mut v = std::mem::MaybeUninit::<T>::zeroed();
    let size = std::mem::size_of::<T>() as i32;
    // SAFETY: `v` is `size` writable bytes; the kernel writes at most that.
    let r = unsafe { libc::proc_pidinfo(pid, flavor, arg, v.as_mut_ptr().cast(), size) };
    if r <= 0 {
        return Err(errno());
    }
    // SAFETY: zeroed plain-old-data the kernel filled; every bit pattern is valid.
    Ok(unsafe { v.assume_init() })
}

fn list<T: Copy>(pid: i32, flavor: i32, max: usize) -> Result<Vec<T>, String> {
    add(1);
    let mut buf: Vec<T> = Vec::with_capacity(max);
    let size = (max * std::mem::size_of::<T>()) as i32;
    // SAFETY: `buf` has room for `max` items, `size` bytes.
    let r = unsafe { libc::proc_pidinfo(pid, flavor, 0, buf.as_mut_ptr().cast(), size) };
    if r <= 0 {
        return Err(errno());
    }
    // SAFETY: the kernel filled `r` bytes of plain-old-data.
    unsafe { buf.set_len((r as usize / std::mem::size_of::<T>()).min(max)) };
    Ok(buf)
}

fn listpids(kind: u32, typeinfo: u32) -> Vec<u32> {
    add(1);
    let mut buf = vec![0i32; 4096];
    // SAFETY: `buf` is 4096 writable pids.
    let r = unsafe {
        libc::proc_listpids(
            kind,
            typeinfo,
            buf.as_mut_ptr().cast(),
            (buf.len() * 4) as i32,
        )
    };
    let n = (r.max(0) as usize / 4).min(buf.len());
    buf[..n]
        .iter()
        .filter(|p| **p > 0)
        .map(|p| *p as u32)
        .collect()
}

fn cstr(bytes: &[libc::c_char]) -> String {
    // SAFETY: a fixed kernel buffer, NUL-terminated within its length or full.
    let b: &[u8] = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len()) };
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

pub struct Tty {
    pub path: String,
    pub tdev: u32,
}

pub fn tty(path: &str) -> anyhow::Result<Tty> {
    let st = rustix::fs::stat(path)?;
    Ok(Tty {
        path: path.to_string(),
        tdev: st.st_rdev as u32,
    })
}

fn bsd(pid: u32) -> Result<libc::proc_bsdinfo, String> {
    pidinfo::<libc::proc_bsdinfo>(pid as i32, PROC_PIDTBSDINFO, 0)
}

pub fn tpgid(pid: u32) -> Result<u32, String> {
    bsd(pid).map(|b| b.e_tpgid)
}

pub fn group_pids(pgid: u32) -> (Vec<u32>, usize) {
    let p = listpids(PROC_PGRP_ONLY, pgid);
    let n = p.len();
    (p, n)
}

pub fn session(tty: &Tty) -> Vec<Value> {
    listpids(PROC_TTY_ONLY, tty.tdev)
        .into_iter()
        .map(|pid| match bsd(pid) {
            Ok(b) => json!({"pid": pid, "comm": cstr(&b.pbi_comm), "state": status_name(b.pbi_status), "pgrp": b.pbi_pgid}),
            Err(e) => json!({"pid": pid, "error": e}),
        })
        .collect()
}

/// `p_stat`: `SIDL`, `SRUN`, `SSLEEP`, `SSTOP`, `SZOMB`.
fn status_name(s: u32) -> &'static str {
    match s {
        1 => "idl",
        2 => "run",
        3 => "sleep",
        4 => "stop",
        5 => "zomb",
        _ => "?",
    }
}

/// `pth_run_state`: `TH_STATE_RUNNING` … `TH_STATE_HALTED`.
fn run_state(s: i32) -> &'static str {
    match s {
        1 => "running",
        2 => "stopped",
        3 => "waiting",
        4 => "uninterruptible",
        5 => "halted",
        _ => "?",
    }
}

fn is_tty(path: &str, tty: &Tty) -> bool {
    path == tty.path || path == "/dev/tty"
}

/// Mach absolute time units to nanoseconds: on Apple silicon `rusage_info`'s
/// times are in ticks of 125/3 ns, not nanoseconds.
#[allow(deprecated)]
fn to_ns(t: u64) -> u64 {
    let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: `tb` is a writable `mach_timebase_info`.
    #[allow(deprecated)]
    unsafe {
        libc::mach_timebase_info(&mut tb)
    };
    if tb.denom == 0 {
        return t;
    }
    (u128::from(t) * u128::from(tb.numer) / u128::from(tb.denom)) as u64
}

fn kqueue_knotes(pid: i32, fd: i32, tty_fds: &[i32]) -> Value {
    add(1);
    let mut buf = vec![
        KeventExtinfo {
            ident: 0,
            filter: 0,
            flags: 0,
            qos: 0,
            udata: 0,
            fflags: 0,
            xflags: 0,
            data: 0,
            ext: [0; 4],
            sdata: 0,
            status: 0,
            sfflags: 0,
            reserved: [0; 2]
        };
        64
    ];
    let size = (buf.len() * std::mem::size_of::<KeventExtinfo>()) as i32;
    // SAFETY: `buf` is `size` writable bytes.
    let r = unsafe {
        libc::proc_pidfdinfo(
            pid,
            fd,
            PROC_PIDFDKQUEUE_EXTINFO,
            buf.as_mut_ptr().cast(),
            size,
        )
    };
    if r < 0 {
        return json!({"error": errno()});
    }
    // The call answers a count of knotes, which may exceed what fit.
    let n = (r as usize).min(buf.len());
    let knotes: Vec<Value> = buf[..n]
        .iter()
        .map(|k| json!({"ident": k.ident, "filter": k.filter, "tty": k.filter == -1 && tty_fds.contains(&(k.ident as i32))}))
        .collect();
    json!({"returned": r, "knotes": knotes})
}

/// Everything S2m and S3 read for one process.
pub fn member(pid: u32, tty: &Tty, _with_ps: bool) -> Value {
    let p = pid as i32;
    let mut v = json!({"pid": pid});
    match pidinfo::<libc::proc_taskallinfo>(p, PROC_PIDTASKALLINFO, 0) {
        Ok(t) => {
            v["comm"] = json!(cstr(&t.pbsd.pbi_comm));
            v["state"] = json!(status_name(t.pbsd.pbi_status));
            v["ppid"] = json!(t.pbsd.pbi_ppid);
            v["uid"] = json!(t.pbsd.pbi_uid);
            v["task"] = json!({
                "total_user_ns": to_ns(t.ptinfo.pti_total_user),
                "total_system_ns": to_ns(t.ptinfo.pti_total_system),
                "syscalls_unix": t.ptinfo.pti_syscalls_unix,
                "syscalls_mach": t.ptinfo.pti_syscalls_mach,
                "csw": t.ptinfo.pti_csw,
                "threadnum": t.ptinfo.pti_threadnum,
                "numrunning": t.ptinfo.pti_numrunning,
            });
        }
        Err(e) => {
            v["task_error"] = json!(e);
            match bsd(pid) {
                Ok(b) => {
                    v["comm"] = json!(cstr(&b.pbi_comm));
                    v["state"] = json!(status_name(b.pbi_status));
                    v["uid"] = json!(b.pbi_uid);
                }
                Err(e) => v["bsd_error"] = json!(e),
            }
        }
    }
    let mut threads = Vec::new();
    match list::<u64>(p, PROC_PIDLISTTHREADS, 256) {
        Ok(handles) => {
            for h in handles {
                match pidinfo::<libc::proc_threadinfo>(p, PROC_PIDTHREADINFO, h) {
                    Ok(t) => threads.push(json!({
                        "run_state": run_state(t.pth_run_state),
                        "flags": t.pth_flags,
                        "sleep_time": t.pth_sleep_time,
                        "name": cstr(&t.pth_name),
                    })),
                    Err(e) => threads.push(json!({"error": e})),
                }
            }
            v["threads"] = json!(threads);
        }
        Err(e) => v["threads"] = json!({"error": e}),
    }
    match list::<libc::proc_fdinfo>(p, PROC_PIDLISTFDS, 1024) {
        Ok(fds) => {
            let mut tty_fds = Vec::new();
            let mut kqs = Vec::new();
            for f in &fds {
                if f.proc_fdtype == PROX_FDTYPE_VNODE {
                    add(1);
                    let mut vi = std::mem::MaybeUninit::<VnodeFdInfoWithPath>::zeroed();
                    let size = std::mem::size_of::<VnodeFdInfoWithPath>() as i32;
                    // SAFETY: `vi` is `size` writable bytes.
                    let r = unsafe {
                        libc::proc_pidfdinfo(
                            p,
                            f.proc_fd,
                            PROC_PIDFDVNODEPATHINFO,
                            vi.as_mut_ptr().cast(),
                            size,
                        )
                    };
                    if r > 0 {
                        // SAFETY: filled by the kernel; plain-old-data.
                        let vi = unsafe { vi.assume_init() };
                        let path_ptr = std::ptr::addr_of!(vi.pvip.vip_path).cast::<libc::c_char>();
                        // SAFETY: `vip_path` is a NUL-terminated MAXPATHLEN buffer.
                        let path = unsafe { CStr::from_ptr(path_ptr) }
                            .to_string_lossy()
                            .into_owned();
                        if is_tty(&path, tty) {
                            tty_fds.push(f.proc_fd);
                        }
                    }
                } else if f.proc_fdtype == PROX_FDTYPE_KQUEUE {
                    kqs.push(f.proc_fd);
                }
            }
            let kq: Vec<Value> = kqs
                .iter()
                .map(|fd| json!({"fd": fd, "extinfo": kqueue_knotes(p, *fd, &tty_fds)}))
                .collect();
            v["fds"] = json!({"count": fds.len(), "tty": tty_fds, "kqueues": kq});
        }
        Err(e) => v["fds"] = json!({"error": e}),
    }
    add(1);
    let mut ri = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: `ri` is a writable `rusage_info_v4`, the version asked for.
    let r = unsafe { libc::proc_pid_rusage(p, libc::RUSAGE_INFO_V4, ri.as_mut_ptr().cast()) };
    if r == 0 {
        // SAFETY: filled by the kernel.
        let ri = unsafe { ri.assume_init() };
        v["rusage"] = json!({
            "user_ns": to_ns(ri.ri_user_time),
            "system_ns": to_ns(ri.ri_system_time),
            "child_user_ns": to_ns(ri.ri_child_user_time),
            "child_system_ns": to_ns(ri.ri_child_system_time),
            "diskio_read": ri.ri_diskio_bytesread,
            "diskio_written": ri.ri_diskio_byteswritten,
            "logical_writes": ri.ri_logical_writes,
            "instructions": ri.ri_instructions,
            "cycles": ri.ri_cycles,
            "pkg_idle_wkups": ri.ri_pkg_idle_wkups,
            "interrupt_wkups": ri.ri_interrupt_wkups,
        });
    } else {
        v["rusage"] = json!({"error": errno()});
    }
    v
}

pub fn access(pid: u32) -> Value {
    let p = pid as i32;
    let r = |x: Result<(), String>| x.map(|()| "ok".to_string()).unwrap_or_else(|e| e);
    add(1);
    let mut ri = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: as in `member`.
    let rc = unsafe { libc::proc_pid_rusage(p, libc::RUSAGE_INFO_V4, ri.as_mut_ptr().cast()) };
    let rusage = if rc == 0 { "ok".to_string() } else { errno() };
    json!({
        "bsdinfo": r(bsd(pid).map(|_| ())),
        "taskallinfo": r(pidinfo::<libc::proc_taskallinfo>(p, PROC_PIDTASKALLINFO, 0).map(|_| ())),
        "listthreads": r(list::<u64>(p, PROC_PIDLISTTHREADS, 64).map(|_| ())),
        "listfds": r(list::<libc::proc_fdinfo>(p, PROC_PIDLISTFDS, 64).map(|_| ())),
        "rusage": rusage,
    })
}

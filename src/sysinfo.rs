//! Process & environment info — what `D`, `V`, `I` show.

/// Current UTC date/time as `YYYY-MM-DD HH:MM:SS UTC`.
pub fn format_now() -> String {
    let dt = jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC);
    dt.strftime("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Seconds since the Unix epoch. Centralized so callers don't each
/// re-roll `SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, ..)`.
/// Uses `jiff::Timestamp` for monotonicity guarantees and a clean API.
pub fn epoch_secs() -> u64 {
    jiff::Timestamp::now().as_second().max(0) as u64
}

/// Whether `pid` names a live process, via a signal-0 existence test
/// (`rustix::process::test_kill_process` — no `unsafe`, no `/proc` dependency,
/// works on macOS + Linux). Used by the startup orphan sweeps to decide whether
/// a `.spyc-context-<pid>.json` / dead-socket MCP entry belongs to a still-
/// running spyc.
///
/// Treats a process as alive UNLESS the kernel says *no such process*
/// (`ESRCH`): a `Pid::from_raw` that fails (pid 0 / negative) or any other
/// errno (e.g. `EPERM` — exists but not ours to signal) counts as alive, so the
/// sweep only ever reaps an entry whose owner is *definitely* gone. PID reuse
/// therefore can't cause a wrongful delete (a reused-and-live PID reads alive).
pub fn pid_alive(pid: u32) -> bool {
    let Ok(raw) = i32::try_from(pid) else {
        return true;
    };
    let Some(rpid) = rustix::process::Pid::from_raw(raw) else {
        return true;
    };
    !matches!(
        rustix::process::test_kill_process(rpid),
        Err(rustix::io::Errno::SRCH)
    )
}

/// Nanoseconds since the Unix epoch. Same shape as `epoch_secs` but
/// for hot-path id generators that want sub-second resolution.
pub fn epoch_nanos() -> u128 {
    let ts = jiff::Timestamp::now();
    let secs = ts.as_second().max(0) as u128;
    let subsec = ts.subsec_nanosecond().max(0) as u128;
    secs * 1_000_000_000 + subsec
}

/// Resident set size in kilobytes, or None if we can't determine it.
pub fn rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                return rest.split_whitespace().next()?.parse::<u64>().ok();
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        // Read via `ps -o rss=` — avoids pulling in libc/mach just for
        // this. The subprocess is short and runs in the background of the
        // TUI (stdout piped), so we don't need the terminal teardown dance.
        let pid = std::process::id();
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        std::str::from_utf8(&out.stdout)
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// `(rss_kb, thread_count)` for the current process, read in-process:
/// RSS via the `sysinfo` crate, thread count via `libproc`
/// `proc_pidinfo` on macOS / `tasks()` on Linux. Best-effort; `None`/0
/// on failure or unsupported platform. The sysinfo/libproc refresh
/// isn't free, so call it OFF the main/render thread (see
/// `App::refresh_process_stats`).
pub fn proc_rss_threads() -> Option<(u64, u32)> {
    use sysinfo_crate::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = Pid::from_u32(std::process::id());
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    let proc = sys.process(pid)?;
    // `memory()` is bytes (sysinfo 0.30+); the HUD wants KB.
    let rss_kb = proc.memory() / 1024;
    // Thread count: sysinfo's `tasks()` is Linux-only (`None` on macOS), so on
    // macOS read it from `proc_pidinfo` via libproc; on Linux use `tasks()`.
    // 0 = unavailable.
    let threads = mac_thread_count()
        .or_else(|| proc.tasks().map(|t| u32::try_from(t.len()).unwrap_or(0)))
        .unwrap_or(0);
    Some((rss_kb, threads))
}

/// macOS thread count via `proc_pidinfo(PROC_PIDTASKINFO).pti_threadnum`
/// (safe `libproc` wrapper). `None` on failure / non-macOS.
#[cfg(target_os = "macos")]
fn mac_thread_count() -> Option<u32> {
    use libproc::libproc::proc_pid::pidinfo;
    use libproc::libproc::task_info::TaskInfo;
    let pid = i32::try_from(std::process::id()).ok()?;
    let info = pidinfo::<TaskInfo>(pid, 0).ok()?;
    u32::try_from(info.pti_threadnum).ok()
}

#[cfg(not(target_os = "macos"))]
const fn mac_thread_count() -> Option<u32> {
    None
}

#[cfg(test)]
mod proc_stats_tests {
    #[test]
    fn proc_rss_threads_reports_live_stats() {
        let (rss_kb, threads) =
            super::proc_rss_threads().expect("proc_rss_threads should read the current process");
        assert!(rss_kb > 0, "RSS should be > 0 (got {rss_kb} KB)");
        // macOS thread count comes from libproc (sysinfo's tasks() is
        // Linux-only); a live process always has >= 1 thread.
        #[cfg(target_os = "macos")]
        assert!(
            threads > 0,
            "macOS thread count should be > 0 (got {threads})"
        );
        let _ = threads; // used by the macOS assert; silence unused elsewhere
    }
}

/// Human-readable RSS, e.g. `12.3 MB`.
pub fn format_rss() -> String {
    match rss_kb() {
        Some(kb) if kb < 1024 => format!("{kb} KB"),
        Some(kb) => format!("{:.1} MB", kb as f64 / 1024.0),
        None => "unavailable".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_now_has_correct_shape() {
        let s = format_now();
        // YYYY-MM-DD HH:MM:SS UTC → 23 chars, with known positions.
        assert_eq!(s.len(), 23);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], " ");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
        assert!(s.ends_with(" UTC"));
    }
}

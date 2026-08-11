//! Cross-platform "current working directory of another process" lookup.
//!
//! Used by the pane status line to surface the *live* cwd of the
//! subprocess (a `bash` tab where the user `cd`'d, etc.) instead of
//! always showing the spawn-time cwd.
//!
//! - **Linux**: `readlink /proc/<pid>/cwd`. Always available, sub-ms.
//! - **macOS**: `proc_pidinfo(PROC_PIDVNODEPATHINFO).pvi_cdir`, reached through
//!   the `sysinfo` crate's process-`cwd` support. In-process, ~0.1ms.
//!
//! The macOS side used to shell out to `lsof -a -p <pid> -d cwd -Fn`, on the
//! reasoning that Darwin's `struct proc_vnodepathinfo` layout (vinfo_stat /
//! fsid_t padding) shifts between versions and getting it wrong silently slices
//! the path. That risk is real but it isn't ours to carry: the struct is defined
//! by the `libc` crate and the call is made by `sysinfo`, both of which spyc
//! already depends on for the activity HUD's RSS/thread numbers. So this needs
//! no new dependency and no `unsafe` here, and it drops a ~40ms process spawn
//! (measured) from a once-per-second poll.
//!
//! (`libproc`, also already in the tree, is *not* the route: its `pidcwd` is
//! `Err("pidcwd is not implemented for macos")` and it exposes no
//! `PROC_PIDVNODEPATHINFO` flavor.)
//!
//! Returns `None` on any failure (process gone, permission denied, etc.).

use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub fn cwd_for_pid(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "macos")]
pub fn cwd_for_pid(pid: u32) -> Option<PathBuf> {
    use sysinfo_crate::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    // Scope the refresh to this one pid and to `cwd` alone — the default
    // `everything()` would walk every process and read argv/env per process.
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
    );
    sys.process(pid)?.cwd().map(std::path::Path::to_path_buf)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn cwd_for_pid(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::cwd_for_pid;

    /// Resolve a live child's cwd — the real shape of the call (the pane asks
    /// about its *subprocess*, never itself), and the case a wrong struct
    /// layout would silently mangle rather than fail.
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn cwd_of_a_child_resolves_the_dir_it_was_spawned_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let expected = std::fs::canonicalize(dir.path()).expect("canonicalize");
        // `sleep` holds the cwd open long enough to be interrogated; it is
        // reaped below so the test leaves no stray process.
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&expected)
            .spawn()
            .expect("spawn sleep");

        let got = cwd_for_pid(child.id());
        let _ = child.kill();
        let _ = child.wait();

        let got = got.expect("a live child's cwd must resolve");
        assert_eq!(
            std::fs::canonicalize(&got).unwrap_or(got),
            expected,
            "resolved cwd must be the whole spawn dir, not a truncated prefix"
        );
    }

    /// A pid we cannot inspect must yield `None`, not a wrong path — pid 1
    /// (`launchd` / `init`) is owned by root and its cwd is not readable by a
    /// normal user. Either outcome is acceptable *except* a bogus path.
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn an_unreadable_pid_degrades_to_none() {
        match cwd_for_pid(1) {
            None => {}
            Some(p) => assert!(
                p.is_absolute() && p.exists(),
                "pid 1 yielded a non-path: {p:?}"
            ),
        }
    }

    /// A pid that cannot exist resolves to nothing rather than to some other
    /// process's directory.
    #[test]
    fn a_dead_pid_resolves_to_nothing() {
        assert_eq!(cwd_for_pid(u32::MAX), None);
    }

    #[test]
    fn cwd_of_self_matches_current_dir() {
        // Best-effort cross-platform sanity check. On unsupported
        // platforms cwd_for_pid returns None; skip the assertion.
        let Some(cwd) = cwd_for_pid(std::process::id()) else {
            return;
        };
        let actual = std::env::current_dir().unwrap();
        // macOS resolves /var → /private/var on the lookup side, so
        // canonicalize both before comparing.
        let expected = std::fs::canonicalize(&actual).unwrap_or(actual);
        let got = std::fs::canonicalize(&cwd).unwrap_or(cwd);
        assert_eq!(got, expected);
    }
}

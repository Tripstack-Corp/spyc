//! Cross-platform "current working directory of another process" lookup.
//!
//! Used by the pane status line to surface the *live* cwd of the
//! subprocess (a `bash` tab where the user `cd`'d, etc.) instead of
//! always showing the spawn-time cwd.
//!
//! - **Linux**: `readlink /proc/<pid>/cwd`. Always available, sub-ms.
//! - **macOS**: shell out to `lsof -a -p <pid> -d cwd -Fn`. We avoided
//!   the libproc FFI route — Darwin's `struct proc_vnodepathinfo`
//!   layout (vinfo_stat / fsid_t padding) shifts between versions and
//!   getting it wrong silently slices the path. `lsof` is built-in on
//!   macOS, costs ~5ms per call, and we only poll once per second.
//!
//! Returns `None` on any failure (process gone, permission denied, etc.).

use std::path::PathBuf;

#[cfg(target_os = "linux")]
pub fn cwd_for_pid(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "macos")]
pub fn cwd_for_pid(pid: u32) -> Option<PathBuf> {
    // -Fn → field output: each line is a single field. The cwd line
    // starts with 'n' and contains the path. Other lines (process
    // header 'p<pid>', fd 'fcwd', etc.) are skipped.
    let output = std::process::Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    for line in std::str::from_utf8(&output.stdout).ok()?.lines() {
        if let Some(path) = line.strip_prefix('n')
            && !path.is_empty()
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn cwd_for_pid(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::cwd_for_pid;

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

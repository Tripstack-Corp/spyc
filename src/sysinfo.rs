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

/// Nanoseconds since the Unix epoch. Same shape as `epoch_secs` but
/// for hot-path id generators that want sub-second resolution.
pub fn epoch_nanos() -> u128 {
    let ts = jiff::Timestamp::now();
    let secs = ts.as_second().max(0) as u128;
    let subsec = ts.subsec_nanosecond().max(0) as u128;
    secs * 1_000_000_000 + subsec
}

/// Pure-parser half of [`git_file_statuses`]: turns raw `git status
/// --porcelain` output (plus the dir-relative prefix) into the
/// basename-keyed map the list view consumes. Split out so we can unit
/// test the path-mapping rules without spawning `git`.
///
/// This is now a thin composition of the two shared stages in
/// [`crate::git::status`]: decode the porcelain text into per-path
/// [`StatusEntry`](crate::git::status::StatusEntry)s, then map those onto
/// the listing dir. The gix backend produces the same intermediate, so both
/// paths share the path-mapping logic (`map_to_listing`).
pub fn parse_porcelain_statuses(
    porcelain: &str,
    prefix: &str,
) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
    let entries = crate::git::status::decode_porcelain(porcelain);
    crate::git::status::map_to_listing(&entries, prefix)
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

/// `(rss_kb, thread_count)` for the current process via `ps` — macOS
/// `thcount`, Linux `nlwp`. Best-effort; `None` on any failure or
/// unsupported platform. Spawns a subprocess (a fork-exec), so call it
/// OFF the main/render thread (see `App::refresh_process_stats`).
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
    use crate::ui::list_view::GitChange;

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

    #[test]
    fn deep_modification_does_not_dirty_same_basename_at_root() {
        // Regression: a root listing of `git status` showing
        // `content-acquisition/AGENTS.md` modified must NOT mark a
        // separate root-level `AGENTS.md` as modified.
        let porcelain = " M content-acquisition/AGENTS.md\n";
        let map = parse_porcelain_statuses(porcelain, "");
        // The deep file's basename is NOT a root entry.
        assert!(!map.contains_key("AGENTS.md"));
        // The parent dir IS marked dirty (unstaged-Modified).
        let dir_status = map.get("content-acquisition/").unwrap();
        assert_eq!(dir_status.unstaged, Some(GitChange::Modified));
        assert!(dir_status.staged.is_none());
        assert!(!dir_status.untracked);
    }

    #[test]
    fn root_modification_marks_basename() {
        // ` M` = unstaged-only modify.
        let map = parse_porcelain_statuses(" M AGENTS.md\n", "");
        let s = map.get("AGENTS.md").unwrap();
        assert_eq!(s.unstaged, Some(GitChange::Modified));
        assert!(s.staged.is_none());
        assert!(!s.untracked);
    }

    #[test]
    fn root_and_deep_same_basename_uses_root_status() {
        // Both a root file and a sibling-named deep file are dirty.
        // The root entry must reflect the root file's actual status,
        // not the deep file's.
        let porcelain = "?? new.md\n M sub/new.md\n";
        let map = parse_porcelain_statuses(porcelain, "");
        let new_md = map.get("new.md").unwrap();
        assert!(new_md.untracked);
        assert!(new_md.staged.is_none() && new_md.unstaged.is_none());
        assert_eq!(map.get("sub/").unwrap().unstaged, Some(GitChange::Modified));
    }

    #[test]
    fn prefix_strips_listing_dir() {
        // Listing `sub/` under a repo root: only entries under `sub/`
        // contribute, and they're keyed relative to the listing dir.
        let porcelain = " M sub/foo.txt\n M other/bar.txt\n";
        let map = parse_porcelain_statuses(porcelain, "sub");
        assert_eq!(
            map.get("foo.txt").unwrap().unstaged,
            Some(GitChange::Modified)
        );
        assert!(!map.contains_key("bar.txt"));
    }

    #[test]
    fn untracked_surfaces_in_subdirectory_listing() {
        // Regression for the `-uno` huge-tree suppression: viewing
        // `docs/` with untracked files in it must surface them as
        // basename-keyed untracked entries. (Previously huge trees
        // ran `git status -uno` and these `??` lines never existed;
        // now we always pass `-unormal`.)
        let porcelain = "?? docs/PATH_HANDOFF_PLAN.md\n?? docs/TEST_IMPROVEMENT_PLAN.md\n";
        let map = parse_porcelain_statuses(porcelain, "docs");
        let a = map.get("PATH_HANDOFF_PLAN.md").unwrap();
        assert!(a.untracked);
        assert!(a.staged.is_none() && a.unstaged.is_none());
        assert!(map.get("TEST_IMPROVEMENT_PLAN.md").unwrap().untracked);
    }

    #[test]
    fn untracked_only_subdir_collapses_to_untracked_dir() {
        // A subtree whose only change is untracked content marks the
        // intermediate directory `?` (untracked), not `~` (modified).
        // The deep file itself is not surfaced as a basename here.
        let map = parse_porcelain_statuses("?? docs/drafts/notes.md\n", "docs");
        let dir = map.get("drafts/").unwrap();
        assert!(dir.untracked);
        assert!(dir.staged.is_none() && dir.unstaged.is_none());
        assert!(!map.contains_key("notes.md"));
    }

    #[test]
    fn mixed_subdir_prefers_modified_over_untracked() {
        // A dir containing both a tracked modification and an untracked
        // file reads as changed (`~`), regardless of which row git
        // emits first — tracked outranks untracked and never downgrades.
        let untracked_first = parse_porcelain_statuses("?? sub/new.md\n M sub/old.md\n", "");
        let modified_first = parse_porcelain_statuses(" M sub/old.md\n?? sub/new.md\n", "");
        for map in [untracked_first, modified_first] {
            let dir = map.get("sub/").unwrap();
            assert_eq!(dir.unstaged, Some(GitChange::Modified));
            assert!(!dir.untracked);
        }
    }

    #[test]
    fn rename_takes_new_name() {
        // `R ` = staged rename, working tree clean.
        let porcelain = "R  old.md -> new.md\n";
        let map = parse_porcelain_statuses(porcelain, "");
        let s = map.get("new.md").unwrap();
        assert_eq!(s.staged, Some(GitChange::Renamed));
        assert!(s.unstaged.is_none());
        assert!(!map.contains_key("old.md"));
    }

    #[test]
    fn staged_only_modify() {
        let map = parse_porcelain_statuses("M  foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Modified));
        assert!(s.unstaged.is_none());
    }

    #[test]
    fn partially_staged_modify() {
        // `MM` — staged modify + further unstaged edits. Both halves set.
        let map = parse_porcelain_statuses("MM foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Modified));
        assert_eq!(s.unstaged, Some(GitChange::Modified));
    }

    #[test]
    fn conflict_marks_both_halves() {
        // `UU` — both sides unmerged. We collapse to Conflicted on both.
        let map = parse_porcelain_statuses("UU foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Conflicted));
        assert_eq!(s.unstaged, Some(GitChange::Conflicted));
    }
}

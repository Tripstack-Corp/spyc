//! Process & environment info — what `D`, `V`, `I` show.
//!
//! We deliberately don't pull in a date-formatting crate; the 20-line
//! Hinnant algorithm below formats UTC timestamps correctly for any
//! Gregorian date we care about.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current UTC date/time as `YYYY-MM-DD HH:MM:SS UTC`.
pub fn format_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    let hour = (secs / 3600) % 24;
    let minute = (secs / 60) % 60;
    let second = secs % 60;
    format!("{y:04}-{m:02}-{d:02} {hour:02}:{minute:02}:{second:02} UTC")
}

/// Howard Hinnant's `civil_from_days`: converts days-since-1970-01-01
/// (Unix epoch) to (year, month, day). Handles years 0000–9999 and
/// beyond; we don't care about edge cases.
const fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146_096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32)
}

/// Git status for a directory: branch name + dirty flag.
/// Returns e.g. `"main*"` (dirty) or `"main"` (clean), or `None` if
/// the directory isn't in a git repo.
pub fn git_status(dir: &std::path::Path) -> Option<String> {
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !branch.status.success() {
        return None;
    }
    let branch = std::str::from_utf8(&branch.stdout).ok()?.trim().to_string();

    let porcelain = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let dirty = porcelain.status.success() && !porcelain.stdout.is_empty();

    Some(if dirty { format!("{branch}*") } else { branch })
}

/// Per-file git status for the current directory. Returns a map from
/// filename (not full path) to status. Only includes files that are
/// modified, new, deleted, etc. — clean files are omitted.
pub fn git_file_statuses(
    dir: &std::path::Path,
) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
    use crate::ui::list_view::GitFileStatus;
    let mut map = std::collections::HashMap::new();
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain", "-uall"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let Ok(output) = output else { return map };
    if !output.status.success() {
        return map;
    }
    // Compute the prefix to strip from repo-relative paths so we get
    // paths relative to the current listing directory.
    let prefix = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| {
            let repo_root = std::path::PathBuf::from(s.trim());
            dir.strip_prefix(&repo_root)
                .unwrap_or(std::path::Path::new(""))
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default();

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.len() < 4 {
            continue;
        }
        let xy = &line[..2];
        let path_str = &line[3..];
        // For renames ("R  old -> new"), take the new name.
        let raw_path = path_str.rsplit(" -> ").next().unwrap_or(path_str);
        // Strip the directory prefix to get a path relative to the
        // current listing dir (git status gives repo-relative paths).
        let filename = if prefix.is_empty() {
            raw_path
        } else {
            let pfx = if prefix.ends_with('/') {
                prefix.as_str()
            } else {
                &format!("{prefix}/")
            };
            match raw_path.strip_prefix(pfx) {
                Some(rest) => rest,
                None => continue, // not under this directory
            }
        };
        let name = std::path::Path::new(filename)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Top component relative to THIS directory.
        let top_component = filename.split('/').next().unwrap_or(filename).to_string();
        let status = match xy {
            "??" => GitFileStatus::Untracked,
            "!!" => continue, // ignored
            s if s.contains('U') || s == "DD" || s == "AA" => GitFileStatus::Conflicted,
            s if s.starts_with('R') || s.ends_with('R') => GitFileStatus::Renamed,
            s if s.starts_with('A') || s.ends_with('A') => GitFileStatus::Added,
            s if s.starts_with('D') || s.ends_with('D') => GitFileStatus::Deleted,
            _ => GitFileStatus::Modified,
        };
        if !name.is_empty() {
            map.entry(name).or_insert(status);
        }
        // Mark parent directories as modified too.
        if top_component != filename && !top_component.is_empty() {
            map.entry(format!("{top_component}/"))
                .or_insert(GitFileStatus::Modified);
        }
    }
    map
}

// ---- Git worktree helpers ---------------------------------------------------

/// A parsed git worktree entry.
pub struct Worktree {
    pub path: std::path::PathBuf,
    /// Short commit hash.
    pub head: String,
    /// Branch name, "(detached)", or "(bare)".
    pub branch: String,
}

/// Parse `git worktree list --porcelain` output.
pub fn git_worktree_list(dir: &std::path::Path) -> Option<Vec<Worktree>> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?;
    let mut worktrees = Vec::new();
    let mut path: Option<String> = None;
    let mut head = String::new();
    let mut branch = String::new();

    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(p) = path.take() {
                let b = std::mem::take(&mut branch);
                worktrees.push(Worktree {
                    path: std::path::PathBuf::from(p),
                    head: std::mem::take(&mut head),
                    branch: if b.is_empty() { "(detached)".into() } else { b },
                });
            }
            head.clear();
            branch.clear();
            continue;
        }
        if let Some(rest) = line.strip_prefix("worktree ") {
            path = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            head = rest[..7.min(rest.len())].to_string();
        } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            branch = rest.to_string();
        } else if line == "bare" {
            branch = "(bare)".to_string();
        } else if line == "detached" {
            branch = "(detached)".to_string();
        }
    }
    if worktrees.is_empty() {
        None
    } else {
        Some(worktrees)
    }
}

/// Create a new worktree as a sibling of the repo root.
/// Tries to check out an existing branch first; creates with `-b` if needed.
pub fn git_worktree_add(
    dir: &std::path::Path,
    branch: &str,
) -> std::io::Result<std::path::PathBuf> {
    let root_output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()?;
    let root = std::str::from_utf8(&root_output.stdout)
        .unwrap_or("")
        .trim()
        .to_string();
    let root_path = std::path::PathBuf::from(&root);
    let parent = root_path.parent().unwrap_or(&root_path);
    let target = parent.join(branch);

    // Try existing branch first.
    let status = std::process::Command::new("git")
        .args(["worktree", "add", &target.display().to_string(), branch])
        .current_dir(dir)
        .stderr(std::process::Stdio::piped())
        .output()?;
    if status.status.success() {
        return Ok(target);
    }
    // Branch doesn't exist — create it.
    let status = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            branch,
            &target.display().to_string(),
        ])
        .current_dir(dir)
        .stderr(std::process::Stdio::piped())
        .output()?;
    if status.status.success() {
        Ok(target)
    } else {
        let msg = std::str::from_utf8(&status.stderr).unwrap_or("unknown error");
        Err(std::io::Error::other(msg.trim().to_string()))
    }
}

/// Remove a worktree by path.
pub fn git_worktree_remove(path: &std::path::Path) -> std::io::Result<()> {
    let output = std::process::Command::new("git")
        .args(["worktree", "remove", &path.display().to_string()])
        .stderr(std::process::Stdio::piped())
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        let msg = std::str::from_utf8(&output.stderr).unwrap_or("unknown error");
        Err(std::io::Error::other(msg.trim().to_string()))
    }
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
    fn epoch_is_1970() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn y2k_is_30th_anniversary() {
        // 2000-01-01 is day 10957 since 1970-01-01.
        assert_eq!(civil_from_days(10957), (2000, 1, 1));
    }

    #[test]
    fn pre_epoch() {
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

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

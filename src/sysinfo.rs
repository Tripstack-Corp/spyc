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
    let raw = std::str::from_utf8(&porcelain.stdout).unwrap_or("<utf8>");
    crate::spyc_debug!(
        "git_status({}): branch={branch:?} dirty={dirty} porcelain={:?}",
        dir.display(),
        raw,
    );

    Some(if dirty { format!("{branch}*") } else { branch })
}

/// Per-file git status for the current directory. Returns a map from
/// filename (not full path) to status. Only includes files that are
/// modified, new, deleted, etc. — clean files are omitted.
pub fn git_file_statuses(
    dir: &std::path::Path,
) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
    let map = std::collections::HashMap::new();
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain", "-unormal"])
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
    parse_porcelain_statuses(&text, &prefix)
}

/// Pure-parser half of [`git_file_statuses`]: turns raw `git status
/// --porcelain` output (plus the dir-relative prefix) into the
/// basename-keyed map the list view consumes. Split out so we can unit
/// test the path-mapping rules without spawning `git`.
fn parse_porcelain_statuses(
    porcelain: &str,
    prefix: &str,
) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
    use crate::ui::list_view::GitFileStatus;
    let mut map = std::collections::HashMap::new();
    for line in porcelain.lines() {
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
                prefix.to_string()
            } else {
                format!("{prefix}/")
            };
            match raw_path.strip_prefix(&pfx) {
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
        let in_this_dir = top_component == filename;
        let status = match xy {
            "??" => GitFileStatus::Untracked,
            "!!" => continue, // ignored
            s if s.contains('U') || s == "DD" || s == "AA" => GitFileStatus::Conflicted,
            s if s.starts_with('R') || s.ends_with('R') => GitFileStatus::Renamed,
            s if s.starts_with('A') || s.ends_with('A') => GitFileStatus::Added,
            s if s.starts_with('D') || s.ends_with('D') => GitFileStatus::Deleted,
            _ => GitFileStatus::Modified,
        };
        // Only file rows in THIS directory get a basename entry.
        // Otherwise a deep entry like `content-acquisition/AGENTS.md`
        // would write `AGENTS.md → Modified` and dirty the unrelated
        // root-level `AGENTS.md` row.
        if in_this_dir && !name.is_empty() {
            map.entry(name).or_insert(status);
        }
        // Mark parent directory as modified for entries in subtrees.
        if !in_this_dir && !top_component.is_empty() {
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
    use crate::ui::list_view::GitFileStatus;

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
        // The parent dir IS marked modified.
        assert_eq!(
            map.get("content-acquisition/"),
            Some(&GitFileStatus::Modified)
        );
    }

    #[test]
    fn root_modification_marks_basename() {
        let map = parse_porcelain_statuses(" M AGENTS.md\n", "");
        assert_eq!(map.get("AGENTS.md"), Some(&GitFileStatus::Modified));
    }

    #[test]
    fn root_and_deep_same_basename_uses_root_status() {
        // Both a root file and a sibling-named deep file are dirty.
        // The root entry must reflect the root file's actual status,
        // not the deep file's.
        let porcelain = "?? new.md\n M sub/new.md\n";
        let map = parse_porcelain_statuses(porcelain, "");
        assert_eq!(map.get("new.md"), Some(&GitFileStatus::Untracked));
        assert_eq!(map.get("sub/"), Some(&GitFileStatus::Modified));
    }

    #[test]
    fn prefix_strips_listing_dir() {
        // Listing `sub/` under a repo root: only entries under `sub/`
        // contribute, and they're keyed relative to the listing dir.
        let porcelain = " M sub/foo.txt\n M other/bar.txt\n";
        let map = parse_porcelain_statuses(porcelain, "sub");
        assert_eq!(map.get("foo.txt"), Some(&GitFileStatus::Modified));
        assert!(!map.contains_key("bar.txt"));
    }

    #[test]
    fn rename_takes_new_name() {
        let porcelain = "R  old.md -> new.md\n";
        let map = parse_porcelain_statuses(porcelain, "");
        assert_eq!(map.get("new.md"), Some(&GitFileStatus::Renamed));
        assert!(!map.contains_key("old.md"));
    }
}

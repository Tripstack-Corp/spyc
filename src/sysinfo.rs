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

/// Resolve `<repo_root>/.git` to the actual gitdir on disk. For a
/// normal repo this is just `<repo_root>/.git`; for a worktree or
/// submodule, `.git` is a *file* whose content is `gitdir: <path>`
/// — we follow that pointer.
///
/// Returns `None` if `.git` is missing or the gitfile is malformed.
/// Pure filesystem (no subprocess); used to bypass
/// `git rev-parse --git-dir` on every chdir.
pub fn resolve_gitdir(repo_root: &std::path::Path) -> Option<std::path::PathBuf> {
    let dot_git = repo_root.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git);
    }
    // gitfile: `gitdir: /abs/or/rel/path\n`
    let contents = std::fs::read_to_string(&dot_git).ok()?;
    let line = contents.lines().find(|l| l.starts_with("gitdir:"))?;
    let path_str = line.trim_start_matches("gitdir:").trim();
    let p = std::path::PathBuf::from(path_str);
    Some(if p.is_absolute() {
        p
    } else {
        repo_root.join(p)
    })
}

/// Read `<gitdir>/HEAD` and return a branch display string —
/// `main` for an attached branch, `abc1234` for a detached HEAD
/// (short hash), or `None` if HEAD can't be read. Pure filesystem;
/// replaces `git rev-parse --abbrev-ref HEAD` on the chdir hot
/// path.
pub fn read_head_branch(gitdir: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let trimmed = contents.trim();
    if let Some(rest) = trimmed.strip_prefix("ref: ") {
        // `refs/heads/main` → `main`. For non-heads refs (e.g.
        // `refs/remotes/origin/foo` from a weird checkout) just
        // strip the longest known prefix we recognize, else show
        // the bare ref name.
        let name = rest
            .strip_prefix("refs/heads/")
            .or_else(|| rest.strip_prefix("refs/"))
            .unwrap_or(rest);
        Some(name.to_string())
    } else if trimmed.len() >= 7 {
        // Detached HEAD — raw commit hash. Show first 7 chars,
        // matching `git rev-parse --short` default.
        Some(trimmed[..7].to_string())
    } else {
        None
    }
}

/// Spawn `git status --porcelain` and return the raw stdout. Split
/// out from [`git_file_statuses`] so callers (e.g. the chdir path)
/// can cache the raw text across navigations within the same repo —
/// the index walk is the expensive part of the spawn and produces
/// identical output for every dir under one repo root.
///
/// Returns `None` if the spawn fails or git exits non-zero.
pub fn git_status_porcelain_raw(dir: &std::path::Path, huge: bool) -> Option<String> {
    let untracked_flag = if huge { "-uno" } else { "-unormal" };
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain", untracked_flag])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Pure-parser half of [`git_file_statuses`]: turns raw `git status
/// --porcelain` output (plus the dir-relative prefix) into the
/// basename-keyed map the list view consumes. Split out so we can unit
/// test the path-mapping rules without spawning `git`.
pub fn parse_porcelain_statuses(
    porcelain: &str,
    prefix: &str,
) -> std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> {
    use crate::ui::list_view::{GitChange, GitFileStatus};
    /// Decode one porcelain XY half (X = index/staged, Y = working tree)
    /// into a `GitChange`. ` ` (and `?`/`!`) yield None — those are
    /// handled by the caller via the special-case markers.
    const fn decode_half(c: char) -> Option<GitChange> {
        match c {
            'M' | 'T' => Some(GitChange::Modified),
            'A' => Some(GitChange::Added),
            'D' => Some(GitChange::Deleted),
            'R' | 'C' => Some(GitChange::Renamed),
            'U' => Some(GitChange::Conflicted),
            _ => None,
        }
    }
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
        // Build a structured status. Both halves are decoded
        // independently; a porcelain like `MM` means staged-modified
        // AND further-modified-unstaged. Conflicts (`UU`, `DD`, `AA`)
        // collapse to Conflicted on both halves so the marker reads
        // `!!` and stands out.
        let status = if xy == "??" {
            GitFileStatus {
                untracked: true,
                ..GitFileStatus::clean()
            }
        } else if xy == "!!" {
            continue; // ignored
        } else if xy.contains('U') || xy == "DD" || xy == "AA" {
            GitFileStatus {
                staged: Some(GitChange::Conflicted),
                unstaged: Some(GitChange::Conflicted),
                untracked: false,
            }
        } else {
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or(' ');
            let y = chars.next().unwrap_or(' ');
            GitFileStatus {
                staged: decode_half(x),
                unstaged: decode_half(y),
                untracked: false,
            }
        };
        // Only file rows in THIS directory get a basename entry.
        // Otherwise a deep entry like `content-acquisition/AGENTS.md`
        // would write `AGENTS.md → Modified` and dirty the unrelated
        // root-level `AGENTS.md` row.
        if in_this_dir && !name.is_empty() {
            map.entry(name).or_insert(status);
        }
        // Mark parent directory as dirty for entries in subtrees.
        // Use the unstaged-Modified shape since directories don't
        // have a meaningful per-half staging concept.
        if !in_this_dir && !top_component.is_empty() {
            map.entry(format!("{top_component}/"))
                .or_insert_with(|| GitFileStatus::unstaged(GitChange::Modified));
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

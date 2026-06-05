//! Git worktree list / add / remove (subprocess backend).
//!
//! Relocated verbatim from `sysinfo.rs`. The `Worktree` shape is the
//! facade's public output so the `W l` pager formatting stays untouched
//! when the gix backend swaps in (PR 6).

/// A parsed git worktree entry.
pub struct Worktree {
    pub path: std::path::PathBuf,
    /// Short commit hash.
    pub head: String,
    /// Branch name, "(detached)", or "(bare)".
    pub branch: String,
}

/// Parse `git worktree list --porcelain` output.
pub fn list(dir: &std::path::Path) -> Option<Vec<Worktree>> {
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
pub fn add(dir: &std::path::Path, branch: &str) -> std::io::Result<std::path::PathBuf> {
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
pub fn remove(path: &std::path::Path) -> std::io::Result<()> {
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

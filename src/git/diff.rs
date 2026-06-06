//! Git diff / show / blame byte producers (subprocess backend).
//!
//! These pipe git's own `--color=always` output straight through; they were
//! the pager's source before PR 8b. As of PR 8b the live views build the
//! structured-model counterparts (straight from gix, for the in-house
//! renderer) in the sibling [`crate::git::diff_model`] (diff/show) and
//! [`crate::git::blame`] (blame), so these subprocess producers are now
//! unused — kept compiled (`#[allow(dead_code)]`) until PR 9 deletes them.

use std::path::Path;
use std::process::{Command, Output};

/// `git diff --color=always [--cached | HEAD] -- <paths>` → raw stdout
/// bytes. `cached` selects the staged ("what would commit") view;
/// otherwise diff-vs-HEAD (staged + unstaged). Exit status is ignored —
/// `git diff` prints to stdout regardless — so the caller renders
/// whatever it produced.
#[allow(dead_code)] // removed in PR 9
pub fn working(dir: &Path, paths: &[String], cached: bool) -> std::io::Result<Vec<u8>> {
    let mut args: Vec<&str> = vec!["diff", "--color=always"];
    if cached {
        args.push("--cached");
    } else {
        args.push("HEAD");
    }
    args.push("--");
    for s in paths {
        args.push(s);
    }
    let output = Command::new("git").args(&args).current_dir(dir).output()?;
    Ok(output.stdout)
}

/// Render an "added" diff for every untracked file under `paths`.
/// Two-step: list with `git ls-files --others --exclude-standard`,
/// then `git diff --no-index /dev/null <file>` per result. Returns the
/// concatenated colored diff bytes (empty if no untracked files match).
#[allow(dead_code)] // removed in PR 9
pub fn untracked_bytes(cwd: &Path, paths: &[String]) -> Vec<u8> {
    let mut args: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "--"];
    for s in paths {
        args.push(s);
    }
    let listing = match Command::new("git").args(&args).current_dir(cwd).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in listing.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(file) = std::str::from_utf8(line) else {
            continue;
        };
        // --no-index exits 1 when files differ — that's the success
        // case for us. Just take whatever it printed.
        if let Ok(o) = Command::new("git")
            .args([
                "diff",
                "--no-index",
                "--color=always",
                "--",
                "/dev/null",
                file,
            ])
            .current_dir(cwd)
            .output()
        {
            out.extend(o.stdout);
        }
    }
    out
}

/// `git show --color=always <sha>` → the full `Output` so the caller can
/// branch on `status.success()` / empty stdout / stderr (the matched-SHA
/// commit-discussion pager).
#[allow(dead_code)] // removed in PR 9
pub fn show(dir: &Path, sha: &str) -> std::io::Result<Output> {
    Command::new("git")
        .args(["show", "--color=always", sha])
        .current_dir(dir)
        .output()
}

/// `git blame --color-lines -- <path>` → the full `Output` (caller
/// branches on success / empty / stderr).
#[allow(dead_code)] // removed in PR 9
pub fn blame(dir: &Path, path: &str) -> std::io::Result<Output> {
    Command::new("git")
        .args(["blame", "--color-lines", "--", path])
        .current_dir(dir)
        .output()
}

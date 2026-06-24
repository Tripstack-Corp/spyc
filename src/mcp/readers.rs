//! Readers that pull picks / inventory / cwd / file content from the spyc
//! context file, for the MCP tool handlers. Split out of mcp.rs verbatim.
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub(super) fn search_root(ctx_path: &Path) -> PathBuf {
    if let Ok(text) = std::fs::read_to_string(ctx_path)
        && let Ok(v) = serde_json::from_str::<Value>(&text)
    {
        // `search_root` is the focused column's worktree root (spyc resolves
        // it via `tool_root`), so MCP search follows the worktree the user is
        // working in. Older context files predate the field — fall back to
        // `project_home`, then `cwd`.
        if let Some(root) = v["search_root"].as_str()
            && !root.is_empty()
        {
            return PathBuf::from(root);
        }
        if let Some(home) = v["project_home"].as_str()
            && !home.is_empty()
        {
            return PathBuf::from(home);
        }
        if let Some(cwd) = v["cwd"].as_str() {
            return PathBuf::from(cwd);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// Read picks from the context file as absolute paths (resolved
/// against `cwd`). Returns the picks plus the cwd to use as the
/// display-relative root for match formatting. Picks list may be
/// empty (no picks selected); that's a valid state -- search_picks
/// just returns no matches.
pub(super) fn read_picks_from_context(ctx_path: &Path) -> (Vec<PathBuf>, Option<PathBuf>) {
    let Ok(text) = std::fs::read_to_string(ctx_path) else {
        return (Vec::new(), None);
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return (Vec::new(), None);
    };
    let cwd = v["cwd"].as_str().map(PathBuf::from);
    let picks = v["picks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str())
                .map(|s| {
                    let p = Path::new(s);
                    if p.is_absolute() {
                        p.to_path_buf()
                    } else if let Some(c) = &cwd {
                        c.join(p)
                    } else {
                        p.to_path_buf()
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    (picks, cwd)
}

/// Read inventory paths from the context file. Inventory entries
/// are stored as absolute paths in the persistent state, so no
/// resolution against cwd is needed.
pub(super) fn read_inventory_from_context(ctx_path: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(ctx_path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v["inventory"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Render a slice of grep matches as a JSON array of objects with
/// `{path, line, col, text}` shape. Used by all three content-search
/// tools so the response shape is uniform.
pub(super) fn grep_matches_to_json(hits: &[crate::fs::grep::GrepMatch]) -> Value {
    let arr: Vec<Value> = hits
        .iter()
        .map(|m| {
            json!({
                "path": m.path.to_string_lossy(),
                "line": m.line,
                "col": m.col,
                "text": m.text,
            })
        })
        .collect();
    Value::Array(arr)
}

/// Read the cwd from the context file (for resolving relative paths).
pub(super) fn read_cwd_from_context(ctx_path: &Path) -> PathBuf {
    if let Ok(text) = std::fs::read_to_string(ctx_path)
        && let Ok(v) = serde_json::from_str::<Value>(&text)
        && let Some(cwd) = v["cwd"].as_str()
    {
        return PathBuf::from(cwd);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// Read file content (up to 100KB, text only).
pub(super) fn read_file_content(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    if meta.len() > 100 * 1024 {
        return Err(format!(
            "{}: file too large ({} KB, limit 100 KB)",
            path.display(),
            meta.len() / 1024
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Reject binary files (null bytes in first 8KB).
    let check_len = bytes.len().min(8192);
    if bytes[..check_len].contains(&0) {
        return Err(format!("{}: binary file", path.display()));
    }
    String::from_utf8(bytes).map_err(|_| format!("{}: not valid UTF-8", path.display()))
}

pub(super) fn read_context_or_empty(ctx_path: &Path) -> String {
    std::fs::read_to_string(ctx_path).unwrap_or_else(|_| {
        json!({
            "cwd": null,
            "cursor_file": null,
            "picks": [],
            "inventory": [],
            "filter": null,
            "git_branch": null,
            "project_home": null,
            "session_name": ""
        })
        .to_string()
    })
}

/// JSON inventory of the focused column's worktrees, for the `list_worktrees`
/// MCP tool: one object per worktree — branch, short HEAD, a dirty-file
/// breakdown (`repo_status` counts), whether it's the worktree the user is
/// currently in, and `ahead`/`behind`/`merged` against the repo's integration
/// base (the "is this safe to remove?" signal; `null` when the tip or base
/// can't be resolved — a bare/unborn entry, or a repo with no base branch).
/// Runs on the socket thread — pure git + the context file's cwd, no `App`.
/// Returns `[]` outside a repo. (Column-`b` flags still need context the
/// snapshot doesn't yet expose.)
pub(super) fn list_worktrees_json(ctx_path: &Path) -> String {
    let cwd = read_cwd_from_context(ctx_path);
    let cwd_canon = std::fs::canonicalize(&cwd).ok();
    let Some(worktrees) = crate::git::worktree::list(&cwd) else {
        return "[]".to_string();
    };
    // Resolve the integration base once, from the MAIN worktree — always first
    // (`worktree::list` ordering) and a clean repo root, unlike the column's
    // possibly-subdir cwd. Each entry's ahead/behind/merged is computed against
    // it via the shared object DB.
    let repo_root = worktrees.first().map(|w| w.path.clone());
    let base = repo_root
        .as_deref()
        .and_then(crate::git::branch::default_base);
    let arr: Vec<Value> = worktrees
        .iter()
        .map(|wt| {
            let (staged, unstaged, untracked) =
                crate::git::status::repo_status(&wt.path).map_or((0, 0, 0), |entries| {
                    (
                        entries.iter().filter(|e| e.staged.is_some()).count(),
                        entries.iter().filter(|e| e.unstaged.is_some()).count(),
                        entries.iter().filter(|e| e.untracked).count(),
                    )
                });
            let is_current =
                cwd_canon.is_some() && std::fs::canonicalize(&wt.path).ok() == cwd_canon;
            let status = repo_root
                .as_deref()
                .zip(base.as_deref())
                .filter(|_| !wt.head.is_empty())
                .and_then(|(root, base)| crate::git::branch::branch_status(root, &wt.head, base));
            json!({
                "path": wt.path.display().to_string(),
                "branch": wt.branch,
                "head": wt.head,
                "is_current": is_current,
                "dirty": { "staged": staged, "unstaged": unstaged, "untracked": untracked },
                "ahead": status.map(|s| s.ahead),
                "behind": status.map(|s| s.behind),
                "merged": status.map(|s| s.merged),
            })
        })
        .collect();
    Value::Array(arr).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support::run_git;

    #[test]
    fn reports_inventory_dirty_and_current() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "c1"]);
        // A linked worktree on a new branch, with one untracked file.
        let wt = crate::git::worktree::add(&repo, "feature", None).unwrap();
        std::fs::write(wt.join("scratch.txt"), "x\n").unwrap();
        // Context file points cwd at the main repo (the focused column).
        let ctx = tmp.path().join("ctx.json");
        std::fs::write(
            &ctx,
            json!({ "cwd": repo.display().to_string() }).to_string(),
        )
        .unwrap();

        let v: Value = serde_json::from_str(&list_worktrees_json(&ctx)).unwrap();
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 2, "main + the linked worktree");
        let main = arr
            .iter()
            .find(|e| e["branch"] == "main")
            .expect("main entry");
        assert_eq!(main["is_current"], json!(true));
        let feat = arr
            .iter()
            .find(|e| e["branch"] == "feature")
            .expect("feature entry");
        assert_eq!(feat["is_current"], json!(false));
        assert_eq!(feat["dirty"]["untracked"], json!(1));
        // No commits past main → fully merged, nothing would be lost on removal.
        assert_eq!(feat["merged"], json!(true));
        assert_eq!(feat["ahead"], json!(0));
        assert_eq!(main["merged"], json!(true));
    }

    #[test]
    fn reports_ahead_behind_for_unmerged_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "c1"]);
        // A worktree on a branch one commit ahead of main.
        let wt = crate::git::worktree::add(&repo, "feature", None).unwrap();
        std::fs::write(wt.join("b.txt"), "b\n").unwrap();
        run_git(&wt, &["add", "."]);
        run_git(&wt, &["commit", "-q", "-m", "c2"]);
        let ctx = tmp.path().join("ctx.json");
        std::fs::write(
            &ctx,
            json!({ "cwd": repo.display().to_string() }).to_string(),
        )
        .unwrap();

        let v: Value = serde_json::from_str(&list_worktrees_json(&ctx)).unwrap();
        let arr = v.as_array().expect("array");
        let feat = arr
            .iter()
            .find(|e| e["branch"] == "feature")
            .expect("feature entry");
        assert_eq!(feat["ahead"], json!(1));
        assert_eq!(feat["behind"], json!(0));
        assert_eq!(feat["merged"], json!(false));
    }

    #[test]
    fn empty_outside_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = tmp.path().join("ctx.json");
        std::fs::write(
            &ctx,
            json!({ "cwd": tmp.path().display().to_string() }).to_string(),
        )
        .unwrap();
        assert_eq!(list_worktrees_json(&ctx), "[]");
    }
}

// ── Framing helpers ─────────────────────────────────────────────

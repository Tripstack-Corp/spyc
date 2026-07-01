//! Readers that pull picks / inventory / cwd / file content from the spyc
//! context file, for the MCP tool handlers. Split out of mcp.rs verbatim.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::git::model::{DiffKind, DiffModel, FileStatus, Hunk, LineOrigin};

/// The effective root for a read tool: an explicit `root` argument when the
/// agent passes one (it's working in a different worktree than the user's
/// focused column — the scoping gap that otherwise forces a Bash fallback),
/// else the focused column's [`search_root`]. The override must be an existing
/// directory; it is **not** confined to the repo (the agent already has shell
/// reach, so this grants no new capability — it just lets the structured tools
/// follow the agent's actual working tree). Returns the reason string on a bad
/// override so the handler can surface it.
pub(super) fn effective_root(args: &Value, ctx_path: &Path) -> Result<PathBuf, String> {
    if let Some(root) = args["root"].as_str()
        && !root.is_empty()
    {
        let p = PathBuf::from(root);
        if !p.is_dir() {
            return Err(format!("root: not a directory: {root}"));
        }
        return Ok(p);
    }
    Ok(search_root(ctx_path))
}

/// Pick the search root: prefer `search_root` from the context file (the
/// focused commander's worktree root), then `project_home`, then `cwd`. The
/// MCP search tools scope themselves to it, matching the in-TUI `F` / `:grep`.
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
pub fn read_file_content(path: &Path) -> Result<String, String> {
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
/// currently in, `ahead`/`behind`/`merged` against the repo's integration
/// base (the "is this safe to remove?" signal; `null` when the tip or base
/// can't be resolved — a bare/unborn entry, or a repo with no base branch),
/// and `locked`/`lock_reason` (the `claim_worktree` lease — a cooperating
/// session's `remove`/`clean` refuses a locked worktree).
/// Runs on the socket thread — pure git + the context file's cwd, no `App`.
/// Returns `[]` outside a repo. (Column-`b` flags still need context the
/// snapshot doesn't yet expose.)
pub(super) fn list_worktrees_json(ctx_path: &Path) -> String {
    list_worktrees_json_at(&read_cwd_from_context(ctx_path))
}

/// Root-based core of [`list_worktrees_json`]: the same worktree inventory, but
/// keyed off an explicit `cwd` (the focused column's worktree root) instead of
/// re-reading it from a context file. The Lua worker holds the snapshot struct,
/// not a ctx file, so it calls this directly; the MCP handler resolves `cwd`
/// from the context file and delegates here.
pub fn list_worktrees_json_at(cwd: &Path) -> String {
    let cwd_canon = std::fs::canonicalize(cwd).ok();
    let Some(worktrees) = crate::git::worktree::list(cwd) else {
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
            let lock_reason = crate::git::worktree::lock_reason(&wt.path);
            json!({
                "path": wt.path.display().to_string(),
                "branch": wt.branch,
                "head": wt.head,
                "is_current": is_current,
                "dirty": { "staged": staged, "unstaged": unstaged, "untracked": untracked },
                "ahead": status.map(|s| s.ahead),
                "behind": status.map(|s| s.behind),
                "merged": status.map(|s| s.merged),
                "locked": lock_reason.is_some(),
                "lock_reason": lock_reason,
            })
        })
        .collect();
    Value::Array(arr).to_string()
}

/// Resolve a worktree path arg for the lease tools: absolute as-is, else
/// relative to the context cwd (the focused column). The lease tools run on the
/// socket thread — pure fs on the worktree's admin dir, no `App`.
fn resolve_worktree_path(ctx_path: &Path, path_arg: &str) -> PathBuf {
    let raw = PathBuf::from(path_arg);
    if raw.is_absolute() {
        raw
    } else {
        read_cwd_from_context(ctx_path).join(raw)
    }
}

/// `claim_worktree`: lock the worktree at `path_arg` with `reason` (git's
/// native lock), so a cooperating session's `remove_worktree`/`clean_worktree`
/// refuses to tear it down. Returns a confirmation message or an error string.
pub(super) fn claim_worktree_result(
    ctx_path: &Path,
    path_arg: &str,
    reason: &str,
) -> Result<String, String> {
    let target = resolve_worktree_path(ctx_path, path_arg);
    let reason = reason.trim();
    let reason = if reason.is_empty() {
        "in use by an MCP client"
    } else {
        reason
    };
    crate::git::worktree::lock(&target, reason)
        .map(|()| format!("claimed worktree {} — locked: {reason}", target.display()))
        .map_err(|e| format!("claim {}: {e}", target.display()))
}

/// `release_worktree`: clear the lease on the worktree at `path_arg`.
pub(super) fn release_worktree_result(ctx_path: &Path, path_arg: &str) -> Result<String, String> {
    let target = resolve_worktree_path(ctx_path, path_arg);
    crate::git::worktree::unlock(&target)
        .map(|()| format!("released worktree {} (lease cleared)", target.display()))
        .map_err(|e| format!("release {}: {e}", target.display()))
}

/// One-word label for a `GitChange` (for the `git_status` JSON).
const fn change_label(c: crate::ui::list_view::GitChange) -> &'static str {
    use crate::ui::list_view::GitChange;
    match c {
        GitChange::Modified => "modified",
        GitChange::Added => "added",
        GitChange::Deleted => "deleted",
        GitChange::Renamed => "renamed",
        GitChange::Conflicted => "conflicted",
    }
}

/// `git_status`: the focused worktree's working-tree status as a JSON array,
/// one object per changed path — `{path, staged, unstaged, untracked}` (staged
/// / unstaged are the change kind or null). Socket-thread, pure git on the
/// search root; `[]` when clean or outside a repo.
pub fn git_status_json(root: &Path) -> String {
    let Some(entries) = crate::git::status::repo_status(root) else {
        return "[]".to_string();
    };
    let arr: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "path": e.rela_path,
                "staged": e.staged.map(change_label),
                "unstaged": e.unstaged.map(change_label),
                "untracked": e.untracked,
            })
        })
        .collect();
    Value::Array(arr).to_string()
}

/// `git_log`: the most recent `limit` commits reachable from `root`'s HEAD,
/// newest first — `{short_id, author, time, subject}` per entry. Socket-thread,
/// pure git; `[]` outside a repo / unborn HEAD. `root` is the effective root
/// (focused column, or the agent's explicit `root` override).
pub fn git_log_json(root: &Path, limit: usize) -> String {
    let arr: Vec<Value> = crate::git::log::recent(root, limit)
        .iter()
        .map(|c| {
            json!({
                "short_id": c.short_id,
                "author": c.author,
                "time": c.time,
                "subject": c.subject,
            })
        })
        .collect();
    Value::Array(arr).to_string()
}

/// Which two sides the `git_diff` tool compares (mirrors the TUI's `gd`/`gD`/`gu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffMode {
    /// `git diff HEAD`: the working tree (staged + unstaged + untracked) vs `HEAD`.
    HeadToWorktree,
    /// `git diff --cached`: the index vs `HEAD` (staged only).
    Cached,
    /// `git diff`: the index vs the working tree (unstaged only — what changed
    /// since you staged).
    Unstaged,
}

/// `git_diff`: unified-diff text for `root`'s changes at the requested
/// [`DiffMode`]. `paths` (repo-relative, forward-slash) optionally restricts the
/// result; empty means everything. In-process gix via the `diff_model` builders
/// (no `git` subprocess — the production guard forbids it). Returns `""` when
/// there's nothing to show or `root` isn't a repo. A capped result ends with a
/// truncation note.
pub(super) fn git_diff_text(root: &Path, mode: DiffMode, paths: &[String]) -> String {
    let model = match mode {
        DiffMode::Cached => crate::git::diff_model::diff_cached(root, paths),
        DiffMode::Unstaged => crate::git::diff_model::diff_index_to_worktree(root, paths),
        DiffMode::HeadToWorktree => crate::git::diff_model::diff_head_to_worktree(root, paths),
    };
    model
        .as_ref()
        .map(diff_model_to_unified)
        .unwrap_or_default()
}

/// Serialize a [`DiffModel`] to `git diff`-style unified text: a `diff --git`
/// header + rename/copy/binary/submodule lines + `@@` hunks per file. The
/// `DiffModel` already carries 1-based hunk line numbers and marker-free line
/// text (the renderer's job is to add markers), so this is a faithful
/// reconstruction an agent can read or feed to `git apply`.
fn diff_model_to_unified(model: &DiffModel) -> String {
    let mut out = String::new();
    for f in &model.files {
        let old = f.old_path.as_deref();
        let new = f.new_path.as_deref();
        // `diff --git a/X b/Y`: the new path on both sides for a modify/add, the
        // old path on both for a delete, the pair for a rename/copy.
        let a_name = old.or(new).unwrap_or("");
        let b_name = new.or(old).unwrap_or("");
        let _ = writeln!(out, "diff --git a/{a_name} b/{b_name}");
        match f.status {
            FileStatus::Renamed { similarity } => {
                let _ = writeln!(out, "similarity index {similarity}%");
                if let Some(o) = old {
                    let _ = writeln!(out, "rename from {o}");
                }
                if let Some(n) = new {
                    let _ = writeln!(out, "rename to {n}");
                }
            }
            FileStatus::Copied { similarity } => {
                let _ = writeln!(out, "similarity index {similarity}%");
                if let Some(o) = old {
                    let _ = writeln!(out, "copy from {o}");
                }
                if let Some(n) = new {
                    let _ = writeln!(out, "copy to {n}");
                }
            }
            _ => {}
        }
        match &f.kind {
            DiffKind::Binary => {
                let _ = writeln!(
                    out,
                    "Binary files a/{} and b/{} differ",
                    old.unwrap_or("/dev/null"),
                    new.unwrap_or("/dev/null")
                );
            }
            DiffKind::Submodule { old: o, new: n } => {
                let _ = writeln!(
                    out,
                    "Submodule {} {}..{}",
                    new.or(old).unwrap_or(""),
                    if o.is_empty() { "0000000" } else { o },
                    if n.is_empty() { "0000000" } else { n },
                );
            }
            DiffKind::Error(msg) => {
                let _ = writeln!(out, "diff error: {msg}");
            }
            // A pure rename / mode change carries no hunks — git emits only the
            // rename headers above, no `---`/`+++`. So skip the file headers and
            // body when there are no hunks.
            DiffKind::Text(hunks) if !hunks.is_empty() => {
                match f.status {
                    FileStatus::Added => {
                        let _ = writeln!(out, "--- /dev/null");
                        let _ = writeln!(out, "+++ b/{}", new.unwrap_or(""));
                    }
                    FileStatus::Deleted => {
                        let _ = writeln!(out, "--- a/{}", old.unwrap_or(""));
                        let _ = writeln!(out, "+++ /dev/null");
                    }
                    _ => {
                        let _ = writeln!(out, "--- a/{}", old.or(new).unwrap_or(""));
                        let _ = writeln!(out, "+++ b/{}", new.or(old).unwrap_or(""));
                    }
                }
                for h in hunks {
                    let _ = writeln!(out, "{}", hunk_header(h));
                    for line in &h.lines {
                        let marker = match line.origin {
                            LineOrigin::Context => ' ',
                            LineOrigin::Add => '+',
                            LineOrigin::Remove => '-',
                        };
                        let _ = writeln!(out, "{marker}{}", line.text);
                    }
                }
            }
            DiffKind::Text(_) => {}
        }
    }
    if model.truncated {
        let _ = writeln!(
            out,
            "(diff truncated — exceeded the line budget; narrow with `paths` or use the TUI `|` view)"
        );
    }
    out
}

/// The `@@ -old_start,old_lines +new_start,new_lines @@` header. git omits the
/// `,count` when a side spans exactly one line; match that for fidelity.
fn hunk_header(h: &Hunk) -> String {
    fn span(start: u32, lines: u32) -> String {
        if lines == 1 {
            format!("{start}")
        } else {
            format!("{start},{lines}")
        }
    }
    format!(
        "@@ -{} +{} @@",
        span(h.old_start, h.old_lines),
        span(h.new_start, h.new_lines)
    )
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

    #[test]
    fn claim_release_round_trip_reflected_in_listing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "c1"]);
        let wt = crate::git::worktree::add(&repo, "feature", None).unwrap();
        let ctx = tmp.path().join("ctx.json");
        std::fs::write(
            &ctx,
            json!({ "cwd": repo.display().to_string() }).to_string(),
        )
        .unwrap();

        let feat = |ctx: &Path| -> Value {
            let v: Value = serde_json::from_str(&list_worktrees_json(ctx)).unwrap();
            v.as_array()
                .unwrap()
                .iter()
                .find(|e| e["branch"] == "feature")
                .expect("feature entry")
                .clone()
        };

        // Initially unlocked.
        let before = feat(&ctx);
        assert_eq!(before["locked"], json!(false));
        assert_eq!(before["lock_reason"], json!(null));

        // Claim → listing shows the lock + reason.
        claim_worktree_result(&ctx, &wt.display().to_string(), "agent A").expect("claim");
        let claimed = feat(&ctx);
        assert_eq!(claimed["locked"], json!(true));
        assert_eq!(claimed["lock_reason"], json!("agent A"));

        // Release → back to unlocked.
        release_worktree_result(&ctx, &wt.display().to_string()).expect("release");
        assert_eq!(feat(&ctx)["locked"], json!(false));
    }

    #[test]
    fn git_status_and_log_reflect_the_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "first"]);
        // One tracked modification + one untracked file.
        std::fs::write(repo.join("a.txt"), "a\nb\n").unwrap();
        std::fs::write(repo.join("new.txt"), "x\n").unwrap();
        // search_root falls back to project_home → cwd; point cwd at the repo.
        let ctx = tmp.path().join("ctx.json");
        std::fs::write(
            &ctx,
            json!({ "cwd": repo.display().to_string() }).to_string(),
        )
        .unwrap();

        let status: Value = serde_json::from_str(&git_status_json(&search_root(&ctx))).unwrap();
        let arr = status.as_array().expect("status array");
        let a = arr
            .iter()
            .find(|e| e["path"] == "a.txt")
            .expect("a.txt entry");
        assert_eq!(a["unstaged"], json!("modified"));
        let n = arr
            .iter()
            .find(|e| e["path"] == "new.txt")
            .expect("new.txt entry");
        assert_eq!(n["untracked"], json!(true));

        let log: Value = serde_json::from_str(&git_log_json(&search_root(&ctx), 10)).unwrap();
        let log = log.as_array().expect("log array");
        assert_eq!(log.len(), 1, "one commit");
        assert_eq!(log[0]["subject"], json!("first"));
    }

    #[test]
    fn effective_root_prefers_explicit_root_else_search_root() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let ctx = dir.join("ctx.json");
        std::fs::write(
            &ctx,
            json!({ "cwd": dir.display().to_string() }).to_string(),
        )
        .unwrap();

        // No `root` arg → the focused column's search_root (here, cwd).
        assert_eq!(effective_root(&json!({}), &ctx).unwrap(), dir);

        // Explicit existing dir → that dir (the agent's own worktree).
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(
            effective_root(&json!({ "root": sub.display().to_string() }), &ctx).unwrap(),
            sub
        );

        // Explicit non-directory → a clear error, not a silent fallback.
        let bogus = dir.join("nope");
        let err =
            effective_root(&json!({ "root": bogus.display().to_string() }), &ctx).unwrap_err();
        assert!(err.contains("not a directory"), "got {err}");
    }

    #[test]
    fn git_diff_text_shows_worktree_and_staged_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "first"]);
        // Unstaged modification + an untracked file.
        std::fs::write(repo.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(repo.join("new.txt"), "fresh\n").unwrap();

        let diff = git_diff_text(&repo, DiffMode::HeadToWorktree, &[]);
        assert!(
            diff.contains("diff --git a/a.txt b/a.txt"),
            "header:\n{diff}"
        );
        assert!(diff.contains("-two"), "removed line:\n{diff}");
        assert!(diff.contains("+TWO"), "added line:\n{diff}");
        // An untracked file comes through as an all-addition.
        assert!(
            diff.contains("diff --git a/new.txt b/new.txt") && diff.contains("+fresh"),
            "untracked addition:\n{diff}"
        );

        // `paths` restricts the diff.
        let only_a = git_diff_text(&repo, DiffMode::HeadToWorktree, &["a.txt".to_string()]);
        assert!(
            only_a.contains("a.txt") && !only_a.contains("new.txt"),
            "path filter:\n{only_a}"
        );

        // Cached is the staged-vs-HEAD view: nothing staged yet → empty.
        assert_eq!(
            git_diff_text(&repo, DiffMode::Cached, &[]),
            "",
            "nothing staged"
        );
        run_git(&repo, &["add", "a.txt"]);
        let staged = git_diff_text(&repo, DiffMode::Cached, &[]);
        assert!(
            staged.contains("a.txt") && staged.contains("+TWO"),
            "staged view:\n{staged}"
        );

        // Unstaged (index↔worktree) after staging a.txt: it now matches the
        // index, so plain `git diff` shows nothing for it — only the still-
        // untracked new.txt remains.
        let unstaged = git_diff_text(&repo, DiffMode::Unstaged, &[]);
        assert!(
            !unstaged.contains("a.txt") && unstaged.contains("new.txt"),
            "unstaged view should drop the now-staged a.txt:\n{unstaged}"
        );
    }

    #[test]
    fn unified_serializer_renders_each_status() {
        use crate::git::model::{DiffLine, FileDiff};
        let model = DiffModel {
            files: vec![
                FileDiff {
                    old_path: None,
                    new_path: Some("added.rs".into()),
                    status: FileStatus::Added,
                    kind: DiffKind::Text(vec![Hunk {
                        old_start: 0,
                        old_lines: 0,
                        new_start: 1,
                        new_lines: 1,
                        lines: vec![DiffLine {
                            origin: LineOrigin::Add,
                            text: "hello".into(),
                        }],
                    }]),
                    lang_hint: "rs".into(),
                },
                FileDiff {
                    old_path: Some("gone.txt".into()),
                    new_path: None,
                    status: FileStatus::Deleted,
                    kind: DiffKind::Text(vec![Hunk {
                        old_start: 1,
                        old_lines: 1,
                        new_start: 0,
                        new_lines: 0,
                        lines: vec![DiffLine {
                            origin: LineOrigin::Remove,
                            text: "bye".into(),
                        }],
                    }]),
                    lang_hint: String::new(),
                },
                // Pure rename (no content change) → rename headers only, no ---/+++.
                FileDiff {
                    old_path: Some("old.rs".into()),
                    new_path: Some("new.rs".into()),
                    status: FileStatus::Renamed { similarity: 95 },
                    kind: DiffKind::Text(vec![]),
                    lang_hint: "rs".into(),
                },
                FileDiff {
                    old_path: Some("img.png".into()),
                    new_path: Some("img.png".into()),
                    status: FileStatus::Modified,
                    kind: DiffKind::Binary,
                    lang_hint: "png".into(),
                },
            ],
            truncated: false,
        };
        let out = diff_model_to_unified(&model);
        // Addition: /dev/null old side, `@@ -0,0 +1 @@` (count-1 omitted on the new side).
        assert!(out.contains("diff --git a/added.rs b/added.rs"), "{out}");
        assert!(out.contains("--- /dev/null"), "{out}");
        assert!(out.contains("+++ b/added.rs"), "{out}");
        assert!(out.contains("@@ -0,0 +1 @@"), "{out}");
        assert!(out.contains("+hello"), "{out}");
        // Deletion: /dev/null new side.
        assert!(out.contains("--- a/gone.txt"), "{out}");
        assert!(out.contains("+++ /dev/null"), "{out}");
        assert!(out.contains("-bye"), "{out}");
        // Pure rename: headers only, no file-content markers.
        assert!(out.contains("rename from old.rs"), "{out}");
        assert!(out.contains("rename to new.rs"), "{out}");
        assert!(out.contains("similarity index 95%"), "{out}");
        // Binary marker.
        assert!(
            out.contains("Binary files a/img.png and b/img.png differ"),
            "{out}"
        );
    }
}

// ── Framing helpers ─────────────────────────────────────────────

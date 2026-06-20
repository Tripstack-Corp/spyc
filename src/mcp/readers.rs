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

// ── Framing helpers ─────────────────────────────────────────────

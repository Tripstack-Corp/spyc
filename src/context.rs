//! Context file writer for MCP integration.
//!
//! Writes a JSON snapshot of spyc's current state to a well-known file
//! so that `spyc --mcp` (or any external tool) can serve it to Claude.
//! The file is written atomically (write-to-temp + rename) to avoid
//! partial reads.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Snapshot of spyc's user-visible state, serialized to JSON.
#[derive(Debug, Clone, Serialize)]
pub struct SpycContext {
    /// Current working directory shown in the file list.
    pub cwd: PathBuf,
    /// Name of the file/dir under the cursor (if any).
    pub cursor_file: Option<String>,
    /// Picked files in the current directory.
    pub picks: Vec<PathBuf>,
    /// Global inventory (cross-directory).
    pub inventory: Vec<PathBuf>,
    /// Active limit filter (e.g. "*.rs"), if any.
    pub filter: Option<String>,
    /// Git branch name, if in a repo.
    pub git_branch: Option<String>,
    /// Sticky project root (target of `gh`, default cwd for new panes).
    pub project_home: Option<PathBuf>,
    /// Spice-pair session name (e.g. `SAFFRON_CUMIN`).
    pub session_name: String,
}

/// Environment variable that tells `spyc --mcp` where to find the
/// context file. Set in each pane's environment so Claude CLI's MCP
/// server reads context from the correct spyc instance.
pub const CONTEXT_ENV_VAR: &str = "SPYC_CONTEXT";

/// Return the context file path for a given project root.
/// Includes the PID so multiple spyc instances don't collide.
pub fn context_path(project_root: &Path) -> PathBuf {
    let pid = std::process::id();
    project_root.join(format!(".spyc-context-{pid}.json"))
}

/// Write context atomically: write to a temp file in the same directory,
/// then rename over the target. This prevents readers from seeing partial
/// JSON.
pub fn write_context_file(path: &Path, ctx: &SpycContext) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(ctx)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".spyc-context-{}.tmp", std::process::id()));
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Remove the context file (best-effort, called on quit).
pub fn remove_context_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_context() {
        let tmp = tempfile::tempdir().unwrap();
        let path = context_path(tmp.path());
        let ctx = SpycContext {
            cwd: PathBuf::from("/home/user/project"),
            cursor_file: Some("main.rs".into()),
            picks: vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")],
            inventory: vec![PathBuf::from("/tmp/notes.txt")],
            filter: Some("*.rs".into()),
            git_branch: Some("main".into()),
            project_home: Some(PathBuf::from("/home/user/project")),
            session_name: "SAFFRON_CUMIN".into(),
        };
        write_context_file(&path, &ctx).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["cwd"], "/home/user/project");
        assert_eq!(parsed["cursor_file"], "main.rs");
        assert_eq!(parsed["picks"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["inventory"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["filter"], "*.rs");
        assert_eq!(parsed["git_branch"], "main");
        assert_eq!(parsed["project_home"], "/home/user/project");
        assert_eq!(parsed["session_name"], "SAFFRON_CUMIN");
    }

    #[test]
    fn context_with_none_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let path = context_path(tmp.path());
        let ctx = SpycContext {
            cwd: PathBuf::from("/tmp"),
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: None,
            session_name: String::new(),
        };
        write_context_file(&path, &ctx).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(parsed["cursor_file"].is_null());
        assert!(parsed["filter"].is_null());
        assert!(parsed["git_branch"].is_null());
        assert!(parsed["project_home"].is_null());
        assert_eq!(parsed["session_name"], "");
    }

    #[test]
    fn remove_context_is_best_effort() {
        let tmp = tempfile::tempdir().unwrap();
        let path = context_path(tmp.path());
        // Removing a file that doesn't exist should not panic.
        remove_context_file(&path);
    }
}

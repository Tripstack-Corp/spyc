//! Context file writer for MCP integration.
//!
//! Writes a JSON snapshot of spyc's current state to a well-known file
//! so that `spyc --mcp` (or any external tool) can serve it to Claude.
//! The file is written atomically (write-to-temp + rename) to avoid
//! partial reads.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Snapshot of spyc's user-visible state, serialized to JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Root that project-scoped MCP search tools (`search_paths` /
    /// `search_content`) walk: the **focused column's worktree root** when it's
    /// in a repo, else `project_home`, else `cwd`. Lets MCP search follow the
    /// focused worktree the same way grep `F` / find / harpoon do. `None` when
    /// no better root than `cwd` is known.
    pub search_root: Option<PathBuf>,
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
    let json = serde_json::to_string_pretty(ctx).map_err(std::io::Error::other)?;
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

/// Reap orphaned `.spyc-context-<pid>.json` files in `dir` whose owning process
/// is dead. Each spyc removes its own context file on a clean exit
/// ([`remove_context_file`]), but a SIGKILL / panic / `kill -9` leaves it
/// behind, so they accumulate in the project root over time. Called once at
/// startup (in the launch dir, where every instance's context file lands).
///
/// Conservative: only removes a file whose embedded PID is **definitely** dead
/// ([`crate::sysinfo::pid_alive`] — ESRCH only) and isn't `our_pid` (we write
/// ours moments later). A live or reused PID is left alone. Returns the count
/// removed. Best-effort — an unreadable dir or unparsable name is skipped.
pub fn sweep_orphan_context_files(dir: &Path, our_pid: u32) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix(".spyc-context-"))
            .and_then(|rest| rest.strip_suffix(".json"))
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == our_pid || crate::sysinfo::pid_alive(pid) {
            continue;
        }
        if std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
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
            search_root: Some(PathBuf::from("/home/user/project")),
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
            search_root: None,
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

    /// The orphan sweep reaps a dead-PID context file but spares our own and a
    /// live PID — and leaves unrelated files (and a malformed name) untouched.
    #[test]
    fn sweep_reaps_dead_pid_context_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let our_pid = std::process::id();
        // PID 999999999 is (effectively) never live → an orphan. Our own PID
        // is alive, covering the "spare the live owner" case.
        let dead = dir.join(".spyc-context-999999999.json");
        let ours = dir.join(format!(".spyc-context-{our_pid}.json"));
        let unrelated = dir.join("README.md");
        let malformed = dir.join(".spyc-context-notapid.json");
        for f in [&dead, &ours, &unrelated, &malformed] {
            std::fs::write(f, b"{}").unwrap();
        }

        let removed = sweep_orphan_context_files(dir, our_pid);
        assert_eq!(removed, 1, "only the dead-PID file is reaped");
        assert!(!dead.exists(), "dead-PID orphan removed");
        assert!(ours.exists(), "our own context file is spared");
        assert!(unrelated.exists(), "unrelated files untouched");
        assert!(malformed.exists(), "unparsable name skipped");
    }

    /// `App::write_context` dedups by comparing the snapshot struct instead
    /// of its serialized JSON. That's only valid if equal structs serialize
    /// identically and unequal ones differ — lock that invariant here.
    #[test]
    fn struct_equality_tracks_serialized_json() {
        let base = SpycContext {
            cwd: PathBuf::from("/p"),
            cursor_file: Some("a.rs".into()),
            picks: vec![PathBuf::from("src/lib.rs")],
            inventory: vec![],
            filter: None,
            git_branch: Some("main".into()),
            project_home: None,
            search_root: None,
            session_name: "SAFFRON_CUMIN".into(),
        };
        let same = base.clone();
        let mut different = base.clone();
        different.cursor_file = Some("b.rs".into());

        let j = |c: &SpycContext| serde_json::to_string_pretty(c).unwrap();
        assert_eq!(base, same);
        assert_eq!(j(&base), j(&same), "equal structs must serialize equally");
        assert_ne!(base, different);
        assert_ne!(
            j(&base),
            j(&different),
            "changed struct must serialize differently"
        );
    }
}

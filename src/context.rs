//! Context file writer for MCP integration.
//!
//! Writes a JSON snapshot of spyc's current state to a well-known file
//! so that `spyc --mcp` (or any external tool) can serve it to Claude.
//! The file is written atomically (write-to-temp + rename) to avoid
//! partial reads.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::project_id::ProjectId;

/// Snapshot of spyc's user-visible state, serialized to JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpycContext {
    /// The session's root: `start_dir`, which `spyc -r` moves to the restored
    /// session's directory. The read tools trust it and the repo's worktrees
    /// as `root` arguments (`mcp::readers::allowed_roots`), and the
    /// `mcp-<pid>.root` sidecar records it for discovery.
    pub root: PathBuf,
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
    /// PID of the running spyc TUI that wrote this context. Surfaced so an MCP
    /// client can name the exact process to restart (the writable tools forward
    /// to *this* instance over its socket).
    pub pid: u32,
    /// Build identity of the running spyc — crate version + short git SHA
    /// (e.g. `1.59.0 (25abd0a)`, from [`crate::VERSION`]). The SHA changes every
    /// commit, so a client can compare it against the repo HEAD to detect the
    /// running spyc predates a tool it expects and prompt a restart.
    pub version: String,
    /// Archives currently mounted, so a read tool can tell a member path from a
    /// path on disk. Empty in the overwhelmingly common case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archive_mounts: Vec<ArchiveMountRef>,
}

/// A mounted archive, as much as an out-of-process reader needs: where the
/// container is, and where its extracted bytes go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchiveMountRef {
    /// The archive file's own path, which is also the mount root: a member is
    /// addressed beneath it (`/src/pkg.zip/a.txt`).
    pub root: PathBuf,
    /// Directory holding whatever has been extracted so far.
    pub staging: PathBuf,
    /// The file the bytes are read out of, when that isn't `root` — a nested
    /// archive is addressed at its member path but read from a staged copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
}

/// Environment variable that tells `spyc --mcp` where to find the
/// context file. Set in each pane's environment so Claude CLI's MCP
/// server reads context from the correct spyc instance.
pub const CONTEXT_ENV_VAR: &str = "SPYC_CONTEXT";

/// The context file in `dir`, named for this process so instances don't
/// collide.
pub fn context_path(dir: &Path) -> PathBuf {
    let pid = std::process::id();
    dir.join(format!(".spyc-context-{pid}.json"))
}

/// Where this process keeps `project`'s context: the owner-private state dir,
/// so nothing lands in the directory the user works in, and the path stays put
/// when the root moves. Each project has its own file,
/// `.spyc-context-<pid>-<project>.json` (#99). With no state dir there is no
/// MCP socket either, and the file goes in `launch_dir` for `SPYC_CONTEXT`'s
/// direct mode.
pub fn process_context_path(launch_dir: &Path, project: ProjectId) -> PathBuf {
    let dir = crate::state::state_root().unwrap_or_else(|| launch_dir.to_path_buf());
    dir.join(format!(
        ".spyc-context-{}-{project}.json",
        std::process::id()
    ))
}

/// Write context atomically: write to a temp file in the same directory,
/// then rename over the target. This prevents readers from seeing partial
/// JSON.
pub fn write_context_file(path: &Path, ctx: &SpycContext) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(ctx).map_err(std::io::Error::other)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    // Named after its target, so two projects' writes never share one.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Remove the context file (best-effort, called on quit).
pub fn remove_context_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// The pid in a context file's name: `.spyc-context-<pid>.json`, a project's
/// `.spyc-context-<pid>-<project>.json` (#99), or either's `.tmp`. `None` for
/// any other name.
pub fn context_file_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(".spyc-context-")?;
    let stem = rest
        .strip_suffix(".json")
        .or_else(|| rest.strip_suffix(".tmp"))?;
    stem.split_once('-')
        .map_or(stem, |(pid, _)| pid)
        .parse()
        .ok()
}

/// Reap orphaned context files in `dir` whose owning process is dead:
/// `.spyc-context-<pid>.json`, each project's `.spyc-context-<pid>-<project>.json`
/// (#99), and a write's leftover `.tmp`. Each spyc removes its own on a clean
/// exit ([`remove_context_file`]), but a SIGKILL / panic / `kill -9` leaves
/// them behind. Called once at startup, in the state dir and in the launch dir,
/// where older spycs wrote theirs.
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
        let Some(pid) = name.to_str().and_then(context_file_pid) else {
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

    /// #523: the context lives in the owner-private state dir, never in the
    /// directory the user launched spyc from. #99: one file per project, named
    /// for this process and the project.
    #[test]
    fn the_context_file_lives_in_the_state_dir() {
        let state = tempfile::tempdir().unwrap();
        let launch = tempfile::tempdir().unwrap();
        crate::state::with_state_root(state.path(), || {
            let (a, b) = (ProjectId::new(), ProjectId::new());
            let path = process_context_path(launch.path(), a);
            assert_eq!(path.parent(), Some(state.path()));
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(
                name,
                format!(".spyc-context-{}-{a}.json", std::process::id())
            );
            assert_ne!(process_context_path(launch.path(), b), path);
            assert_eq!(context_file_pid(name), Some(std::process::id()));
        });
    }

    #[test]
    fn round_trip_context() {
        let tmp = tempfile::tempdir().unwrap();
        let path = context_path(tmp.path());
        let ctx = SpycContext {
            root: PathBuf::from("/home/user/project"),
            cwd: PathBuf::from("/home/user/project"),
            cursor_file: Some("main.rs".into()),
            picks: vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")],
            inventory: vec![PathBuf::from("/tmp/notes.txt")],
            filter: Some("*.rs".into()),
            git_branch: Some("main".into()),
            project_home: Some(PathBuf::from("/home/user/project")),
            search_root: Some(PathBuf::from("/home/user/project")),
            session_name: "SAFFRON_CUMIN".into(),
            pid: 4242,
            version: "1.59.0 (deadbee)".into(),
            archive_mounts: Vec::new(),
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
        assert_eq!(parsed["pid"], 4242);
        assert_eq!(parsed["version"], "1.59.0 (deadbee)");
    }

    #[test]
    fn context_with_none_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let path = context_path(tmp.path());
        let ctx = SpycContext {
            root: tmp.path().to_path_buf(),
            cwd: PathBuf::from("/tmp"),
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
            project_home: None,
            search_root: None,
            session_name: String::new(),
            pid: 0,
            version: String::new(),
            archive_mounts: Vec::new(),
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

    /// #99: each project has its own file, `.spyc-context-<pid>-<project>.json`,
    /// and a dead spyc's are reaped like its one file was.
    #[test]
    fn sweep_reaps_a_dead_spycs_project_context_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let our_pid = std::process::id();
        let project = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5a6b";
        let dead = dir.join(format!(".spyc-context-999999999-{project}.json"));
        let ours = dir.join(format!(".spyc-context-{our_pid}-{project}.json"));
        for f in [&dead, &ours] {
            std::fs::write(f, b"{}").unwrap();
        }

        let removed = sweep_orphan_context_files(dir, our_pid);
        assert!(!dead.exists(), "a dead spyc's project file is reaped");
        assert!(ours.exists(), "our own project file is spared");
        assert_eq!(removed, 1);
    }

    /// `App::write_context` dedups by comparing the snapshot struct instead
    /// of its serialized JSON. That's only valid if equal structs serialize
    /// identically and unequal ones differ — lock that invariant here.
    #[test]
    fn struct_equality_tracks_serialized_json() {
        let base = SpycContext {
            root: PathBuf::from("/p"),
            cwd: PathBuf::from("/p"),
            cursor_file: Some("a.rs".into()),
            picks: vec![PathBuf::from("src/lib.rs")],
            inventory: vec![],
            filter: None,
            git_branch: Some("main".into()),
            project_home: None,
            search_root: None,
            session_name: "SAFFRON_CUMIN".into(),
            pid: 4242,
            version: "1.59.0 (deadbee)".into(),
            archive_mounts: Vec::new(),
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

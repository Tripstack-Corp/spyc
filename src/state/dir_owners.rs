//! Which live spyc instances relied on a spyc file shared in a directory, as
//! older spycs recorded it before an agent's wiring moved to its user
//! configuration (#619). Two kinds were shared: the agent status hooks, and the
//! agents' MCP `spyc` entry, each `{dir: [pid, ...]}` in the state dir.
//!
//! Read only now: the directories name projects that may still hold a
//! per-directory copy, and a live owner is an older spyc still relying on it,
//! whose directory the scrub leaves alone (`crate::wiring::legacy`). A recycled
//! pid can keep a dead owner looking alive, which only leaves a copy in place a
//! while longer.

pub mod hooks;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

type Owners = HashMap<String, Vec<u32>>;

/// A spyc file one directory's instances shared.
#[derive(Clone, Copy, Debug)]
pub enum Shared {
    /// The agents' status hooks (`.claude/settings.json` and kin).
    StatusHooks,
    /// The agents' MCP `spyc` entry (`.mcp.json` and kin).
    McpEntry,
}

fn disk_path(what: Shared) -> Option<PathBuf> {
    let file = match what {
        Shared::StatusHooks => "hook_owners.json",
        Shared::McpEntry => "mcp_owners.json",
    };
    crate::state::state_root().map(|d| d.join(file))
}

fn key(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

fn load(what: Shared) -> Owners {
    disk_path(what)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Every directory the store names, its owners alive or not.
#[must_use]
pub fn recorded_dirs(what: Shared) -> Vec<PathBuf> {
    load(what).into_keys().map(PathBuf::from).collect()
}

/// Whether a live instance other than `pid` relies on the `what` in `dir`.
pub fn claimed_by_another(what: Shared, dir: &Path, pid: u32) -> bool {
    load(what).get(&key(dir)).is_some_and(|owners| {
        owners
            .iter()
            .any(|p| *p != pid && crate::sysinfo::pid_alive(*p))
    })
}

/// Record that `pid` relies on the `what` in `dir`, as an older spyc did.
#[cfg(test)]
pub fn claim(what: Shared, dir: &Path, pid: u32) {
    let mut map = load(what);
    let owners = map.entry(key(dir)).or_default();
    if !owners.contains(&pid) {
        owners.push(pid);
    }
    let path = disk_path(what).expect("a test pins the state root");
    std::fs::create_dir_all(path.parent().expect("a state file has a parent"))
        .expect("create the state dir");
    let text = serde_json::to_string(&map).expect("serialize owners");
    crate::fs::write_atomic(&path, text.as_bytes()).expect("write owners");
}

#[cfg(test)]
mod tests {
    use super::Shared::{McpEntry, StatusHooks};
    use super::{claim, claimed_by_another, recorded_dirs};
    use std::path::{Path, PathBuf};

    /// Your own claim is not another's, a dead owner is no owner, and each kind
    /// is its own store.
    #[test]
    fn only_another_live_instance_counts() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = Path::new("/repo");
            let me = std::process::id();
            claim(McpEntry, dir, me);
            assert!(!claimed_by_another(McpEntry, dir, me));
            let mut gone = std::process::Command::new("true").spawn().unwrap();
            let dead = gone.id();
            gone.wait().unwrap();
            claim(McpEntry, dir, dead);
            assert!(!claimed_by_another(McpEntry, dir, me));
            claim(McpEntry, dir, 1);
            assert!(claimed_by_another(McpEntry, dir, me));
            assert!(!claimed_by_another(StatusHooks, dir, me));
            assert_eq!(recorded_dirs(McpEntry), [PathBuf::from("/repo")]);
        });
    }
}

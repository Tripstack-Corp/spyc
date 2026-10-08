//! Which live spyc instances rely on a spyc file shared in a directory.
//!
//! Two kinds are shared rather than owned: the agent status hooks, and the
//! agents' MCP `spyc` entry. Neither names an instance — the hook reporter and
//! the `spyc --mcp` proxy both use whichever socket the **pane's** env names —
//! so one copy serves every spyc there. Writing is therefore safe to repeat,
//! but removing is not. Without this registry a second spyc quitting deleted
//! the hooks out from under the first one's live panes, dropping every dot to
//! output-timing for the rest of that session with no signal. Teardown consults
//! [`release`] so it removes them only when nobody else is left. Each kind is
//! counted separately, because `:hooks off` drops a hooks claim on purpose and
//! must not take the MCP entry with it.
//!
//! `{dir: [pid, ...]}` per kind in the XDG state dir, pruned of dead pids on
//! every write. Hook cleanup uses the stricter per-agent leases in [`hooks`];
//! this directory-wide registry also protects reporters from older instances.
//! The MCP/compatibility counters are best-effort: a lost concurrent
//! update either strands a pid (the next prune drops it) or loses ours (the
//! drift re-heal in `app::status_hooks` puts the hooks back, and the next agent
//! launch rewrites the MCP entry). A recycled pid can keep a dead owner looking
//! alive, which only leaves a file in place a while longer.

pub mod hooks;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

type Owners = HashMap<String, Vec<u32>>;

/// A spyc file one directory's instances share.
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

fn save(what: Shared, map: &Owners) {
    let Some(path) = disk_path(what) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(map) {
        let _ = crate::fs::write_atomic(&path, text.as_bytes());
    }
}

/// Drop owners whose process is gone, then dirs left with none. A spyc killed
/// with `SIGKILL` never releases, so this is what keeps the store from pinning
/// a file forever.
fn prune(map: &mut Owners) {
    map.retain(|_, pids| {
        pids.retain(|p| crate::sysinfo::pid_alive(*p));
        !pids.is_empty()
    });
}

/// Record that `pid` relies on the `what` in `dir`. Idempotent.
pub fn claim(what: Shared, dir: &Path, pid: u32) {
    let mut map = load(what);
    prune(&mut map);
    let owners = map.entry(key(dir)).or_default();
    if !owners.contains(&pid) {
        owners.push(pid);
    }
    save(what, &map);
}

/// Whether a live instance other than `pid` relies on the `what` in `dir`.
pub fn claimed_by_another(what: Shared, dir: &Path, pid: u32) -> bool {
    let mut map = load(what);
    prune(&mut map);
    map.get(&key(dir))
        .is_some_and(|owners| owners.iter().any(|p| *p != pid))
}

/// Drop `pid`'s claim on the `what` in `dir`. **Returns whether no live owner
/// remains** — i.e. the caller is the last one out and may remove it. A dir
/// nobody ever claimed reads as "last out" (there is nothing to protect).
#[must_use]
pub fn release(what: Shared, dir: &Path, pid: u32) -> bool {
    let mut map = load(what);
    prune(&mut map);
    let k = key(dir);
    let remaining = match map.get_mut(&k) {
        Some(owners) => {
            owners.retain(|p| *p != pid);
            owners.len()
        }
        None => 0,
    };
    if remaining == 0 {
        map.remove(&k);
    }
    save(what, &map);
    remaining == 0
}

#[cfg(test)]
mod tests {
    use super::Shared::{McpEntry, StatusHooks};
    use super::{Shared, claimed_by_another};
    use std::path::Path;

    fn claim(dir: &Path, pid: u32) {
        super::claim(StatusHooks, dir, pid);
    }

    fn release(dir: &Path, pid: u32) -> bool {
        super::release(StatusHooks, dir, pid)
    }

    /// The invariant the whole module exists for: a second instance leaving does
    /// NOT hand the first one's hooks to the reaper.
    #[test]
    fn only_the_last_live_owner_may_clean() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = Path::new("/repo");
            let me = std::process::id();
            // Two owners: us and another live process (pid 1 — always alive).
            claim(dir, me);
            claim(dir, 1);
            assert!(!release(dir, me), "a live sibling must block the cleanup");
            assert!(release(dir, 1), "the last owner out may clean");
        });
    }

    /// Claims are per-dir, and a dir nobody claimed is free to clean — otherwise
    /// an upgrade from a spyc that predates the registry would strand its hooks.
    #[test]
    fn claims_are_scoped_per_dir_and_unclaimed_dirs_are_free() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            claim(Path::new("/a"), 1);
            assert!(release(Path::new("/b"), std::process::id()));
            assert!(!release(Path::new("/a"), std::process::id()));
        });
    }

    /// A `SIGKILL`ed spyc leaves its pid behind; liveness — not the release
    /// call it never made — is what frees the dir.
    ///
    /// The dead pid comes from a reaped child rather than a large constant:
    /// `pid_alive` fails SAFE (anything but `ESRCH` reads as alive), so an
    /// out-of-range number would be reported live and prove nothing.
    #[test]
    fn a_dead_owner_does_not_pin_the_hooks() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn a child to reap");
        let dead = child.id();
        child.wait().expect("reap it");

        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = Path::new("/repo");
            claim(dir, dead);
            claim(dir, std::process::id());
            assert!(release(dir, std::process::id()));
        });
    }

    /// Claiming twice from one process must not double-count, or that process
    /// could never release itself to zero.
    #[test]
    fn claiming_twice_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = Path::new("/repo");
            let me = std::process::id();
            claim(dir, me);
            claim(dir, me);
            assert!(release(dir, me));
        });
    }

    /// The two kinds are counted apart: dropping a hooks claim, which `:hooks off`
    /// does on purpose, leaves the MCP claim standing.
    #[test]
    fn hooks_and_mcp_entries_are_claimed_separately() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = Path::new("/repo");
            let me = std::process::id();
            super::claim(StatusHooks, dir, 1);
            super::claim(McpEntry, dir, 1);
            assert!(super::release(StatusHooks, dir, 1));
            assert!(
                claimed_by_another(McpEntry, dir, me),
                "pid 1 still owns the entry"
            );
            assert!(!claimed_by_another(Shared::StatusHooks, dir, me));
        });
    }

    /// Your own claim is not another's.
    #[test]
    fn only_another_live_instance_counts_as_another() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = Path::new("/repo");
            let me = std::process::id();
            super::claim(McpEntry, dir, me);
            assert!(!claimed_by_another(McpEntry, dir, me));
            super::claim(McpEntry, dir, 1);
            assert!(claimed_by_another(McpEntry, dir, me));
        });
    }
}

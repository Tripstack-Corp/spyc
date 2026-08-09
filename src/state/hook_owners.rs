//! Which live spyc instances rely on the agent status hooks installed in a dir.
//!
//! The hooks are *shared*: one `.claude/settings.json` serves every spyc,
//! because the reporter it installs targets whichever socket the **pane's** env
//! names. Installing is therefore safe to repeat — but removing is not. Without
//! this registry a second spyc quitting deleted the hooks out from under the
//! first one's live panes, dropping every dot to output-timing for the rest of
//! that session with no signal. Teardown consults [`release`] so it removes
//! them only when nobody else is left.
//!
//! `{dir: [pid, ...]}` in the XDG state dir, pruned of dead pids on every write.
//! Best-effort like the other state stores: a lost concurrent update either
//! strands a pid (the next prune drops it) or loses ours (the drift re-heal in
//! `app::status_hooks` puts the hooks back). A recycled pid can keep a dead
//! owner looking alive, which only leaves hooks installed a while longer —
//! harmless, since the reporter is fail-soft.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

type Owners = HashMap<String, Vec<u32>>;

fn disk_path() -> Option<PathBuf> {
    crate::state::state_root().map(|d| d.join("hook_owners.json"))
}

fn key(dir: &Path) -> String {
    dir.to_string_lossy().into_owned()
}

fn load() -> Owners {
    disk_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(map: &Owners) {
    let Some(path) = disk_path() else {
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
/// hooks forever.
fn prune(map: &mut Owners) {
    map.retain(|_, pids| {
        pids.retain(|p| crate::sysinfo::pid_alive(*p));
        !pids.is_empty()
    });
}

/// Record that `pid` relies on the status hooks installed in `dir`. Idempotent.
pub fn claim(dir: &Path, pid: u32) {
    let mut map = load();
    prune(&mut map);
    let owners = map.entry(key(dir)).or_default();
    if !owners.contains(&pid) {
        owners.push(pid);
    }
    save(&map);
}

/// Drop `pid`'s claim on `dir`. **Returns whether no live owner remains** — i.e.
/// the caller is the last one out and may remove the hooks. A dir nobody ever
/// claimed reads as "last out" (there is nothing to protect).
#[must_use]
pub fn release(dir: &Path, pid: u32) -> bool {
    let mut map = load();
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
    save(&map);
    remaining == 0
}

#[cfg(test)]
mod tests {
    use super::{claim, release};
    use std::path::Path;

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
}

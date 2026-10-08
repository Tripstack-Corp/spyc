//! Per-agent hook leases: borrowing reporters protects them without owning cleanup.
//!
//! The stable advisory lock is nonblocking. Missing/corrupt state or contention
//! preserves hooks; a cleanup guard holds the lock through the file removal so
//! a new instance cannot register between the last-owner check and cleanup.

use super::{Shared, key};
use crate::state::sessions::AgentKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct HookClaim {
    pub dir: PathBuf,
    pub kind: AgentKind,
    pub managed: bool,
}

#[derive(Serialize, Deserialize)]
struct Lease {
    pid: u32,
    kind: AgentKind,
    managed: bool,
}

type Leases = HashMap<String, Vec<Lease>>;

struct Registry {
    lock: File,
    path: PathBuf,
    leases: Leases,
}

fn read<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Option<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(T::default()),
        Err(_) => None,
    }
}

impl Registry {
    fn open() -> Option<Self> {
        let root = crate::state::state_root()?;
        std::fs::create_dir_all(&root).ok()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("hook_leases.lock"))
            .ok()?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).ok()?;
        let path = root.join("hook_leases.json");
        let mut leases: Leases = read(&path)?;
        if leases.values().flatten().any(|owner| {
            crate::agent::profile_for(owner.kind)
                .status_hooks()
                .is_none()
        }) {
            return None;
        }
        leases.retain(|_, owners| {
            owners.retain(|owner| crate::sysinfo::pid_alive(owner.pid));
            !owners.is_empty()
        });
        Some(Self { lock, path, leases })
    }

    fn save(&self) -> Option<()> {
        let bytes = serde_json::to_vec(&self.leases).ok()?;
        crate::fs::write_atomic(&self.path, &bytes).ok()
    }
}

/// Register a borrow or successful installation. A repeated refusal revokes
/// this instance's earlier cleanup authority for that agent and directory.
pub fn claim(dir: &Path, kind: AgentKind, pid: u32, managed: bool) -> bool {
    // Keep older instances' directory-wide cleanup from deleting live reporters.
    super::claim(Shared::StatusHooks, dir, pid);
    let Some(mut registry) = Registry::open() else {
        return false;
    };
    let owners = registry.leases.entry(key(dir)).or_default();
    if let Some(owner) = owners
        .iter_mut()
        .find(|owner| owner.pid == pid && owner.kind == kind)
    {
        owner.managed = managed;
    } else {
        owners.push(Lease { pid, kind, managed });
    }
    registry.save().is_some()
}

/// Holds the registry lock through authorized teardown cleanup.
pub struct CleanupGuard {
    _lock: File,
}

/// Remove this instance's agent lease. Cleanup requires a recorded successful
/// installation and no remaining same-agent owner. Unknown older instances
/// conservatively protect every agent's hooks in their directory.
pub fn release(claim: &HookClaim, pid: u32) -> Option<CleanupGuard> {
    let mut registry = Registry::open()?;
    let dir = key(&claim.dir);
    let owners = registry.leases.get_mut(&dir)?;
    let departing = owners
        .iter()
        .find(|owner| owner.pid == pid && owner.kind == claim.kind)?;
    // SPYC-TRAP(hook-cleanup-needs-managed-lease): marker presence grants no teardown authority.
    let managed = claim.managed && departing.managed;
    let legacy: super::Owners = read(&super::disk_path(Shared::StatusHooks)?)?;
    let unknown_owner = legacy.get(&dir).is_some_and(|pids| {
        pids.iter().any(|other| {
            *other != pid
                && crate::sysinfo::pid_alive(*other)
                && !owners.iter().any(|owner| owner.pid == *other)
        })
    });
    owners.retain(|owner| owner.pid != pid || owner.kind != claim.kind);
    let same_agent = owners.iter().any(|owner| owner.kind == claim.kind);
    if owners.is_empty() {
        registry.leases.remove(&dir);
    }
    registry.save()?;
    (managed && !same_agent && !unknown_owner).then_some(CleanupGuard {
        _lock: registry.lock,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(test: impl FnOnce(&Path)) {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || test(tmp.path()));
    }

    fn owned(dir: &Path, kind: AgentKind) -> HookClaim {
        HookClaim {
            dir: dir.to_path_buf(),
            kind,
            managed: true,
        }
    }

    #[test]
    fn borrowed_last_owner_and_missing_claim_never_authorize_cleanup() {
        fixture(|dir| {
            let me = std::process::id();
            let claim_info = owned(dir, AgentKind::Codex);
            assert!(release(&claim_info, me).is_none());
            assert!(claim(dir, AgentKind::Codex, me, false));
            assert!(release(&claim_info, me).is_none());
        });
    }

    #[test]
    fn same_agent_sibling_protects_hooks_until_last_managed_owner() {
        fixture(|dir| {
            let me = std::process::id();
            assert!(claim(dir, AgentKind::Codex, me, true));
            assert!(claim(dir, AgentKind::Codex, 1, true));
            assert!(release(&owned(dir, AgentKind::Codex), me).is_none());
            let _ = super::super::release(Shared::StatusHooks, dir, me);
            assert!(release(&owned(dir, AgentKind::Codex), 1).is_some());
        });
    }

    #[test]
    fn other_agent_sibling_does_not_prevent_cleanup() {
        fixture(|dir| {
            let me = std::process::id();
            assert!(claim(dir, AgentKind::Codex, me, true));
            assert!(claim(dir, AgentKind::Claude, 1, true));
            assert!(release(&owned(dir, AgentKind::Codex), me).is_some());
            let _ = super::super::release(Shared::StatusHooks, dir, me);
            assert!(release(&owned(dir, AgentKind::Claude), 1).is_some());
        });
    }

    #[test]
    fn refusal_revokes_previous_managed_lease() {
        fixture(|dir| {
            let me = std::process::id();
            assert!(claim(dir, AgentKind::Codex, me, true));
            assert!(claim(dir, AgentKind::Codex, me, false));
            assert!(release(&owned(dir, AgentKind::Codex), me).is_none());
        });
    }

    #[test]
    fn unknown_legacy_owner_conservatively_protects_hooks() {
        fixture(|dir| {
            let me = std::process::id();
            assert!(claim(dir, AgentKind::Codex, me, true));
            super::super::claim(Shared::StatusHooks, dir, 1);
            assert!(release(&owned(dir, AgentKind::Codex), me).is_none());
        });
    }

    #[test]
    fn registry_contention_is_nonblocking_and_preserves_hooks() {
        fixture(|dir| {
            let me = std::process::id();
            assert!(claim(dir, AgentKind::Codex, me, true));
            let guard = release(&owned(dir, AgentKind::Codex), me).unwrap();
            let start = std::time::Instant::now();
            assert!(!claim(dir, AgentKind::Codex, 1, true));
            assert!(start.elapsed() < std::time::Duration::from_secs(1));
            drop(guard);
            assert!(claim(dir, AgentKind::Codex, 1, true));
        });
    }

    #[test]
    fn corrupt_registry_does_not_grant_cleanup_authority() {
        fixture(|dir| {
            let me = std::process::id();
            assert!(claim(dir, AgentKind::Codex, me, true));
            std::fs::write(
                crate::state::state_root().unwrap().join("hook_leases.json"),
                "{broken",
            )
            .unwrap();
            assert!(release(&owned(dir, AgentKind::Codex), me).is_none());
            assert!(!claim(dir, AgentKind::Codex, me, true));
        });
    }

    #[test]
    fn unknown_agent_kind_in_registry_preserves_hooks() {
        fixture(|dir| {
            let me = std::process::id();
            assert!(claim(dir, AgentKind::Codex, me, true));
            let path = crate::state::state_root().unwrap().join("hook_leases.json");
            let bytes = std::fs::read_to_string(&path)
                .unwrap()
                .replace("codex", "unknown-agent");
            std::fs::write(path, bytes).unwrap();
            assert!(release(&owned(dir, AgentKind::Codex), me).is_none());
            assert!(!claim(dir, AgentKind::Codex, me, true));
        });
    }
}

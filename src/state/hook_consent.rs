//! Persisted, per-project consent for installing Claude status hooks.
//!
//! Writing `report_status` hooks into a project's `.claude/settings.json` makes
//! `claude` execute a command on every turn — more invasive than the `.mcp.json`
//! it merely reads — so spyc asks the user first (a `[Y/n]` popup) and remembers
//! the answer **per project root, forever**. This is that store: a tiny
//! `{root_path: allowed}` JSON map in the XDG state dir, keyed by the project
//! root (so every dir/worktree under a consented repo shares one decision).
//!
//! `None` = never asked (→ prompt), `Some(true)` = allowed (→ install hooks),
//! `Some(false)` = denied (→ never install). Best-effort like the other state
//! files: a missing/corrupt store reads as "never asked".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn disk_path() -> Option<PathBuf> {
    crate::state::state_root().map(|d| d.join("hook_consent.json"))
}

fn load() -> HashMap<String, bool> {
    disk_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The recorded consent for installing status hooks in `root`:
/// `Some(true)` = allowed, `Some(false)` = denied, `None` = never asked.
#[must_use]
pub fn consent_for(root: &Path) -> Option<bool> {
    load().get(&root.to_string_lossy().into_owned()).copied()
}

/// Persist the user's per-project answer (best-effort; a write failure just
/// means we ask again next time, never an error to the user).
pub fn set_consent(root: &Path, allow: bool) {
    let mut map = load();
    map.insert(root.to_string_lossy().into_owned(), allow);
    let Some(path) = disk_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&map) {
        let _ = std::fs::write(&path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::{consent_for, set_consent};
    use std::path::Path;

    #[test]
    fn consent_round_trips_per_project_and_defaults_to_none() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let a = Path::new("/repo/a");
            let b = Path::new("/repo/b");
            // Unasked projects read as None.
            assert_eq!(consent_for(a), None);
            // A grant and a denial persist independently, keyed by root.
            set_consent(a, true);
            set_consent(b, false);
            assert_eq!(consent_for(a), Some(true));
            assert_eq!(consent_for(b), Some(false));
            assert_eq!(consent_for(Path::new("/repo/c")), None);
            // A later answer overwrites the earlier one.
            set_consent(a, false);
            assert_eq!(consent_for(a), Some(false));
        });
    }
}

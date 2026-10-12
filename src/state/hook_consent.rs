//! The per-project answers spyc recorded before an agent's wiring moved to its
//! user configuration (#619): a `{root: allowed}` JSON map in the state dir.
//! Read only to find the projects that may still hold a per-directory copy,
//! and retired at startup: an older spyc still running reads this store before
//! it writes a project its hooks, so every answer becomes a no.

use std::collections::HashMap;
use std::path::PathBuf;

fn disk_path() -> Option<PathBuf> {
    crate::state::state_root().map(|d| d.join("hook_consent.json"))
}

fn load() -> HashMap<String, bool> {
    disk_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(map: &HashMap<String, bool>) {
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

/// Every project root that has answered.
#[must_use]
pub fn recorded_roots() -> Vec<PathBuf> {
    load().into_keys().map(PathBuf::from).collect()
}

/// Make every recorded answer a no, so an older spyc stops writing projects
/// their hooks. The roots stay, for [`recorded_roots`].
pub fn retire() {
    let mut map = load();
    if map.values().all(|allowed| !allowed) {
        return;
    }
    for allowed in map.values_mut() {
        *allowed = false;
    }
    save(&map);
}

/// Record `root`'s answer, as older spycs did.
#[cfg(test)]
pub fn set_consent(root: &std::path::Path, allow: bool) {
    let mut map = load();
    map.insert(root.to_string_lossy().into_owned(), allow);
    save(&map);
}

#[cfg(test)]
mod tests {
    use super::{load, recorded_roots, retire, set_consent};
    use std::path::{Path, PathBuf};

    #[test]
    fn retiring_turns_every_answer_into_a_no_and_keeps_the_roots() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            set_consent(Path::new("/repo/a"), true);
            set_consent(Path::new("/repo/b"), false);
            retire();
            assert!(load().values().all(|allowed| !allowed));
            let mut roots = recorded_roots();
            roots.sort();
            assert_eq!(roots, [PathBuf::from("/repo/a"), PathBuf::from("/repo/b")]);
        });
    }
}

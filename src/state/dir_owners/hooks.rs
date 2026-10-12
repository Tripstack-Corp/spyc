//! The per-agent hook leases older spycs recorded in `hook_leases.json`: which
//! directories they wrote status hooks into. Read only, to find the projects
//! that may still hold a per-directory copy (`crate::wiring::legacy`). An older
//! spyc still running there is found through the `StatusHooks` registry its
//! lease also wrote.

use std::collections::HashMap;
use std::path::PathBuf;

/// Every directory a lease names, its owners alive or not. Only the keys are
/// read, so whatever a lease holds parses.
#[must_use]
pub fn recorded_dirs() -> Vec<PathBuf> {
    crate::state::state_root()
        .and_then(|root| std::fs::read(root.join("hook_leases.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<HashMap<String, serde_json::Value>>(&bytes).ok())
        .map(|leases| leases.into_keys().map(PathBuf::from).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::recorded_dirs;
    use std::path::PathBuf;

    #[test]
    fn every_leased_directory_is_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            assert!(recorded_dirs().is_empty());
            std::fs::write(
                tmp.path().join("hook_leases.json"),
                r#"{"/repo/a":[{"pid":1,"kind":"Claude","managed":true}],"/repo/b":[]}"#,
            )
            .unwrap();
            let mut dirs = recorded_dirs();
            dirs.sort();
            assert_eq!(dirs, [PathBuf::from("/repo/a"), PathBuf::from("/repo/b")]);
        });
    }
}

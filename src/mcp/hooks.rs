//! Claude Code status hooks: spyc installs three command hooks into a launched
//! claude pane's project `.claude/settings.json` so the agent auto-reports its
//! activity (`spyc --report-status <state>`) on lifecycle events, driving the
//! per-tab activity dot WITHOUT the agent having to call the `report_status`
//! tool itself. The reporter reads `SPYC_MCP_SOCK` + `SPYC_PANE_ID` from the
//! env spyc injects into the pane (inherited by the hook).
//!
//! Event → state (verified against the Claude Code hooks docs):
//! * `UserPromptSubmit`  → `working` (the agent just got a prompt)
//! * `PermissionRequest` → `blocked` (Claude is asking to use a tool — the
//!   real-time "which agent needs me" signal; fires the instant the permission
//!   prompt appears)
//! * `Notification`      → `blocked` (Claude's "waiting for your input" idle
//!   notification — a slower backstop. It does NOT fire on permission prompts,
//!   so `PermissionRequest` is what carries the immediate blocked signal.)
//! * `Stop`              → `done`    (the agent finished its turn)
//!
//! Merge/cleanup mirrors the `.mcp.json` policy in [`super::config`]: merge our
//! entries into an existing file without disturbing the user's other hooks or
//! settings; on teardown remove only the entries we wrote (identified by the
//! `--report-status` command), deleting an emptied file/dir; never touch a
//! git-tracked `settings.json` (don't dirty something the user committed).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::config::ConfigCleanup;

/// The (event, reported-state) hooks spyc installs. Two events map to `blocked`:
/// `PermissionRequest` (the real-time permission-prompt signal) and
/// `Notification` (the slower idle "waiting for input" backstop).
const STATUS_HOOKS: [(&str, &str); 4] = [
    ("UserPromptSubmit", "working"),
    ("PermissionRequest", "blocked"),
    ("Notification", "blocked"),
    ("Stop", "done"),
];

/// True if `group` (a matcher-group `{matcher, hooks:[...]}`) holds a command
/// hook that is one of OURS — i.e. its command runs `--report-status`. The
/// spyc-specific flag is a sound "we wrote this" proxy (the user is not
/// expected to author their own `--report-status` hooks).
fn group_is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| {
            handlers.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains("--report-status"))
            })
        })
}

/// Write/merge the spyc status hooks into `<dir>/.claude/settings.json`,
/// preserving any existing settings + the user's own hooks. Idempotent: a prior
/// spyc hook group for each event is replaced (not duplicated) on re-launch.
/// Returns whether we wrote the file (so teardown knows to clean it). No-op
/// (returns `false`) if the file is git-tracked — we never dirty a committed
/// config.
pub fn ensure_claude_status_hooks(dir: &Path) -> bool {
    let path = dir.join(".claude").join("settings.json");
    if crate::git::discovery::is_tracked(&path) {
        return false;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));
    let exe = exe.to_string_lossy();

    // Start from the existing object (if parseable), else a fresh `{}`.
    let mut root = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let Some(obj) = root.as_object_mut() else {
        return false;
    };
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks_obj) = hooks.as_object_mut() else {
        return false;
    };
    for (event, state) in STATUS_HOOKS {
        let group = json!({
            "matcher": "",
            "hooks": [ { "type": "command", "command": format!("{exe} --report-status {state}") } ],
        });
        let arr = hooks_obj.entry(event).or_insert_with(|| json!([]));
        let Some(list) = arr.as_array_mut() else {
            continue;
        };
        // Drop a stale spyc group from a prior launch, keep the user's, append ours.
        list.retain(|g| !group_is_ours(g));
        list.push(group);
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&root) {
        Ok(out) => crate::fs::write_atomic(&path, (out + "\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// Teardown counterpart: remove only the hook entries spyc wrote from
/// `<dir>/.claude/settings.json`, preserving the user's other hooks/settings.
/// Empties cascade: an event whose array becomes empty is dropped, then the
/// `hooks` key if empty, then the whole file (and the `.claude/` dir) if that
/// leaves it empty. Refuses a git-tracked file. Best-effort.
pub fn cleanup_claude_status_hooks(dir: &Path) -> ConfigCleanup {
    let path = dir.join(".claude").join("settings.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ConfigCleanup::NothingToDo;
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&text) else {
        return ConfigCleanup::NothingToDo;
    };
    // Is any of ours present? (read-only scan, so a tracked file with nothing of
    // ours reports NothingToDo rather than SkippedTracked.)
    let has_ours = root
        .pointer("/hooks")
        .and_then(Value::as_object)
        .is_some_and(|h| {
            STATUS_HOOKS.iter().any(|(event, _)| {
                h.get(*event)
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().any(group_is_ours))
            })
        });
    if !has_ours {
        return ConfigCleanup::NothingToDo;
    }
    if crate::git::discovery::is_tracked(&path) {
        return ConfigCleanup::SkippedTracked;
    }

    let Some(obj) = root.as_object_mut() else {
        return ConfigCleanup::NothingToDo;
    };
    if let Some(hooks_obj) = obj.get_mut("hooks").and_then(Value::as_object_mut) {
        for (event, _) in STATUS_HOOKS {
            if let Some(list) = hooks_obj.get_mut(event).and_then(Value::as_array_mut) {
                list.retain(|g| !group_is_ours(g));
            }
        }
        // Drop emptied event arrays.
        hooks_obj.retain(|_, v| !v.as_array().is_some_and(Vec::is_empty));
    }
    // Drop the `hooks` key if it's now empty.
    if obj
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(serde_json::Map::is_empty)
    {
        obj.remove("hooks");
    }

    if obj.is_empty() {
        let _ = std::fs::remove_file(&path);
        // Remove `.claude/` too, but only if now empty (no-op otherwise).
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
        return ConfigCleanup::Cleaned;
    }
    if let Ok(out) = serde_json::to_string_pretty(&root) {
        let _ = crate::fs::write_atomic(&path, (out + "\n").as_bytes());
    }
    ConfigCleanup::Cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn writes_status_hooks_then_cleans_them_leaving_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(ensure_claude_status_hooks(dir));
        let path = dir.join(".claude/settings.json");
        let v = read(&path);
        // All three events present, each running --report-status with its state.
        for (event, state) in STATUS_HOOKS {
            let cmd = v
                .pointer(&format!("/hooks/{event}/0/hooks/0/command"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(
                cmd.contains("--report-status") && cmd.contains(state),
                "{event} → {state}: got {cmd:?}"
            );
        }
        // Cleanup removes everything → file (and .claude/) gone.
        assert!(matches!(
            cleanup_claude_status_hooks(dir),
            ConfigCleanup::Cleaned
        ));
        assert!(
            !path.exists(),
            "settings.json should be removed when emptied"
        );
    }

    #[test]
    fn merge_preserves_user_settings_and_hooks_and_cleanup_leaves_them() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        // A pre-existing settings.json with a user theme + a user Stop hook.
        std::fs::write(
            dir.join(".claude/settings.json"),
            r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo bye"}]}]}}"#,
        )
        .unwrap();
        assert!(ensure_claude_status_hooks(dir));
        let path = dir.join(".claude/settings.json");
        let v = read(&path);
        assert_eq!(v["theme"], "dark", "user setting preserved");
        // Stop now has BOTH the user's echo + our report-status group.
        let stop = v.pointer("/hooks/Stop").unwrap().as_array().unwrap();
        assert_eq!(stop.len(), 2, "user hook kept, ours appended: {stop:?}");

        // Idempotent re-write doesn't duplicate ours.
        assert!(ensure_claude_status_hooks(dir));
        let stop2 = read(&path)["hooks"]["Stop"].as_array().unwrap().len();
        assert_eq!(stop2, 2, "re-launch replaces our group, no dupe");

        // Cleanup removes ONLY ours; the user's theme + echo hook survive.
        assert!(matches!(
            cleanup_claude_status_hooks(dir),
            ConfigCleanup::Cleaned
        ));
        let after = read(&path);
        assert_eq!(after["theme"], "dark");
        let stop_after = after.pointer("/hooks/Stop").unwrap().as_array().unwrap();
        assert_eq!(stop_after.len(), 1, "only the user's echo hook remains");
        assert_eq!(stop_after[0]["hooks"][0]["command"], "echo bye");
    }

    #[test]
    fn cleanup_is_a_noop_when_nothing_of_ours() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/settings.json"), r#"{"theme":"dark"}"#).unwrap();
        assert!(matches!(
            cleanup_claude_status_hooks(dir),
            ConfigCleanup::NothingToDo
        ));
        assert!(dir.join(".claude/settings.json").exists());
    }
}

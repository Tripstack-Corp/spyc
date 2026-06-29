//! Claude Code status hooks: spyc installs command hooks into a launched
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
//! * `PreToolUse`        → `blocked` (matched to `AskUserQuestion`/`ExitPlanMode`
//!   — the agent asking a question or requesting plan approval. These are
//!   mid-turn tools that fire none of the above, so without this the dot keeps
//!   pulsing `working` while the agent actually waits on the user.)
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

/// The (event, matcher, reported-state) hooks spyc installs. `matcher` is the
/// tool-name pattern for the per-tool events (`""` = all / not-a-tool-event).
/// Three things map to `blocked`: `PermissionRequest` (the real-time
/// permission-prompt signal), `Notification` (the slower idle "waiting for
/// input" backstop), and a `PreToolUse` matching `AskUserQuestion`/`ExitPlanMode`
/// — the agent asking a question or requesting plan approval, which fire no
/// `PermissionRequest`/`Notification`/`Stop` of their own (they're mid-turn
/// tools), so without this the dot keeps the working pulse while the agent
/// waits on you.
const STATUS_HOOKS: [(&str, &str, &str); 5] = [
    ("UserPromptSubmit", "", "working"),
    ("PermissionRequest", "", "blocked"),
    ("Notification", "", "blocked"),
    ("PreToolUse", "AskUserQuestion|ExitPlanMode", "blocked"),
    ("Stop", "", "done"),
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
/// preserving any existing settings + the user's own hooks. Idempotent at the
/// byte level: when the file already holds exactly the merged content the write
/// is **skipped**, so re-launching a pane in a cwd whose hooks are already
/// installed doesn't touch the file. (All agent panes in a repo share one
/// settings.json and Claude reloads it live — an identical rewrite still bumps
/// mtime and would trip every sibling agent's hook reload for nothing.)
/// Returns whether our hooks are present in a file we own (so teardown knows to
/// clean it) — `true` both when we wrote and when an already-current file let us
/// skip. `false` if the file is git-tracked (we never dirty a committed config)
/// or its `hooks` value is a non-object we won't clobber.
pub fn ensure_claude_status_hooks(dir: &Path) -> bool {
    let path = dir.join(".claude").join("settings.json");
    if crate::git::discovery::is_tracked(&path) {
        return false;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));
    let existing = std::fs::read_to_string(&path).ok();
    let Some(out) = merged_status_hooks_json(existing.as_deref(), &exe.to_string_lossy()) else {
        return false;
    };
    // Already current → skip the write (and its mtime bump), but still report
    // `true`: our hooks ARE present, so teardown must track this dir.
    if existing.as_deref() == Some(out.as_str()) {
        return true;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    crate::fs::write_atomic(&path, out.as_bytes()).is_ok()
}

/// Merge spyc's status hooks into `existing` settings.json content (or a fresh
/// `{}` when absent/unparseable), returning the serialized result with a
/// trailing newline. `None` when the existing `hooks` value is a non-object we
/// refuse to clobber, or on a serialization failure. Pure (no I/O) so the merge
/// — and its byte-level idempotency, which is what makes the write skippable —
/// is unit-testable.
fn merged_status_hooks_json(existing: Option<&str>, exe: &str) -> Option<String> {
    let mut root = existing
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let obj = root.as_object_mut()?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks_obj = hooks.as_object_mut()?;
    for (event, matcher, state) in STATUS_HOOKS {
        let group = json!({
            "matcher": matcher,
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
    serde_json::to_string_pretty(&root)
        .ok()
        .map(|out| out + "\n")
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
            STATUS_HOOKS.iter().any(|(event, _, _)| {
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
        for (event, _, _) in STATUS_HOOKS {
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
        // Every event present, each running --report-status with its state and
        // carrying its tool-name matcher (`""` for the non-tool events).
        for (event, matcher, state) in STATUS_HOOKS {
            let cmd = v
                .pointer(&format!("/hooks/{event}/0/hooks/0/command"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(
                cmd.contains("--report-status") && cmd.contains(state),
                "{event} → {state}: got {cmd:?}"
            );
            let m = v
                .pointer(&format!("/hooks/{event}/0/matcher"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert_eq!(m, matcher, "{event} matcher");
        }
        // The PreToolUse hook targets the user-facing ask/approve tools.
        assert_eq!(
            v.pointer("/hooks/PreToolUse/0/matcher")
                .and_then(Value::as_str),
            Some("AskUserQuestion|ExitPlanMode")
        );
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

    #[test]
    fn merge_is_byte_idempotent() {
        // Applying the merge to its own output must reproduce it byte-for-byte —
        // this is the invariant that lets `ensure_claude_status_hooks` skip the
        // write (and the mtime bump) on a re-launch instead of churning the
        // shared settings.json.
        let once = merged_status_hooks_json(None, "spyc").expect("fresh merge");
        let twice = merged_status_hooks_json(Some(&once), "spyc").expect("re-merge");
        assert_eq!(once, twice, "re-applying the merge changed a byte");
        // It really installed our hooks (guards against a vacuous equality).
        assert!(once.contains("--report-status blocked"));
        // A user's own settings/hooks survive the round-trip unchanged too.
        let with_user = merged_status_hooks_json(
            Some(r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo bye"}]}]}}"#),
            "spyc",
        )
        .expect("merge over user config");
        assert_eq!(
            with_user,
            merged_status_hooks_json(Some(&with_user), "spyc").expect("re-merge over user config"),
            "merge over a user config is not idempotent"
        );
        assert!(with_user.contains("echo bye"), "user hook dropped");
    }

    #[test]
    fn relaunch_does_not_rewrite_an_already_current_file() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(ensure_claude_status_hooks(dir));
        let path = dir.join(".claude/settings.json");

        // Backdate mtime far enough that a rewrite is unambiguous regardless of
        // the filesystem's timestamp resolution.
        let backdated = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(backdated)
            .unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();

        // Second launch: hooks already current → must skip the write...
        assert!(
            ensure_claude_status_hooks(dir),
            "still reports present (track for cleanup)"
        );
        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            before, after,
            "an already-current settings.json was rewritten (mtime bumped → would trip Claude's live-reload)"
        );
    }
}

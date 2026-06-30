//! Agent status hooks: spyc installs command hooks into a launched agent
//! pane's project config so the agent auto-reports its activity
//! (`spyc --report-status <state>`) on lifecycle events, driving the per-tab
//! activity dot WITHOUT the agent having to call the `report_status` tool
//! itself. The reporter reads `SPYC_MCP_SOCK` + `SPYC_PANE_ID` from the env
//! spyc injects into the pane (inherited by the hook).
//!
//! Three agents are wired (all share the lifecycle-event → reported-state idea):
//! * **claude** — `.claude/settings.json` (JSON), reloaded live → hooks take
//!   effect on the next turn (the functions below).
//! * **codex** — inline `[[hooks.<Event>]]` arrays in the SAME
//!   `.codex/config.toml` we write the MCP entry into; read once at startup, so
//!   they must be written BEFORE the pane spawns (see the codex section).
//! * **agy** (Antigravity) — a `spyc-status` named set in `.agents/hooks.json`;
//!   PARTIAL (working/done only — agy has no approval event for `blocked`). See
//!   the agy section.
//!
//! Event → state (verified against the Claude Code hooks docs):
//! * `UserPromptSubmit`  → `working` (the agent just got a prompt)
//! * `PermissionRequest` → `blocked` (Claude is asking to use a tool — the
//!   real-time "which agent needs me" signal; fires the instant the permission
//!   prompt appears)
//! * `Notification`      → `blocked` (configured), but **payload-dependent**:
//!   Claude fires Notification with a `notification_type` of either
//!   `permission_prompt` ("Claude needs your permission" — keep `blocked`) or
//!   `idle_prompt` ("Claude is waiting for your input" after ~60s idle). The
//!   reporter (`mcp::effective_report_state`) reads the hook's stdin and
//!   **downgrades an `idle_prompt` Notification to `done`** — an idle agent is
//!   finished-and-waiting, not "needs me", so it must not flip to the red
//!   blocked square. (Verified empirically via `--status-trace`: Notification
//!   DOES fire on permission prompts, contra an earlier belief — but
//!   `PermissionRequest` is the primary, instant permission signal.)
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
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use super::config::ConfigCleanup;

/// Set once at launch from the `--status-trace` CLI flag. When on, the hook
/// commands spyc installs get a baked `--status-trace` arg so the one-shot
/// reporter subprocess logs each invocation — and crucially the arg rides in the
/// command string, so it survives even if Claude runs hooks with a sanitized
/// env. Off by default (the reporter fires every agent turn; always-on logging
/// would spam `mcp.log`). Mirrors the `--key-trace` debug-flag pattern.
static STATUS_TRACE: AtomicBool = AtomicBool::new(false);

/// Enable baking `--status-trace` into installed hook commands. Called once at
/// startup from `spyc --status-trace`.
pub fn set_status_trace(on: bool) {
    STATUS_TRACE.store(on, Ordering::Relaxed);
}

fn status_trace_enabled() -> bool {
    STATUS_TRACE.load(Ordering::Relaxed)
}

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
    let Some(out) = merged_status_hooks_json(
        existing.as_deref(),
        &exe.to_string_lossy(),
        status_trace_enabled(),
    ) else {
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
fn merged_status_hooks_json(existing: Option<&str>, exe: &str, trace: bool) -> Option<String> {
    let mut root = existing
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let obj = root.as_object_mut()?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks_obj = hooks.as_object_mut()?;
    // `--status-trace` rides in the command string (not env) so the reporter
    // logs even when Claude sanitizes the hook env.
    let suffix = if trace { " --status-trace" } else { "" };
    for (event, matcher, state) in STATUS_HOOKS {
        let group = json!({
            "matcher": matcher,
            "hooks": [ { "type": "command", "command": format!("{exe} --report-status {state}{suffix}") } ],
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

// ── Codex status hooks ────────────────────────────────────────────────
//
// Codex ships an event-hooks system parallel to claude's, but its config lives
// as inline `[[hooks.<Event>]]` arrays-of-tables in the SAME
// `.codex/config.toml` we write the MCP server into (see [`super::config`]),
// not a separate file. The event names overlap with claude's:
// `UserPromptSubmit` → working, `PermissionRequest` → blocked (the instant
// "needs me" approval signal), `Stop` → done. Codex has no `Notification`/idle
// event, so there's no idle-downgrade to apply — `effective_report_state` is
// inert for codex payloads. Timing differs: codex reads its config ONCE at
// startup (no live reload), so for an already-consented repo the hooks are
// written before the pane spawns; a first-launch `yes` only takes effect on
// codex's next launch.

/// Codex's (event, reported-state). No matcher: these events aren't
/// tool-scoped, and the `--report-status` command string is the "ours" marker
/// for cleanup (a user isn't expected to author their own).
const CODEX_STATUS_HOOKS: [(&str, &str); 3] = [
    ("UserPromptSubmit", "working"),
    ("PermissionRequest", "blocked"),
    ("Stop", "done"),
];

/// TOML counterpart of [`group_is_ours`]: a `{ hooks = [{ command = … }] }`
/// group is ours when a handler's `command` runs `--report-status`.
fn codex_group_is_ours(group: &toml::Value) -> bool {
    group
        .get("hooks")
        .and_then(toml::Value::as_array)
        .is_some_and(|handlers| {
            handlers.iter().any(|h| {
                h.get("command")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|c| c.contains("--report-status"))
            })
        })
}

/// Codex counterpart of [`ensure_claude_status_hooks`]: merge spyc's status
/// hooks into `<dir>/.codex/config.toml` (the same file as the MCP entry),
/// preserving everything else. The byte-idempotent skip, the git-tracked guard,
/// and the return contract all mirror the claude version.
pub fn ensure_codex_status_hooks(dir: &Path) -> bool {
    let path = dir.join(".codex").join("config.toml");
    if crate::git::discovery::is_tracked(&path) {
        return false;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));
    let existing = std::fs::read_to_string(&path).ok();
    let Some(out) = merged_codex_status_hooks_toml(
        existing.as_deref(),
        &exe.to_string_lossy(),
        status_trace_enabled(),
    ) else {
        return false;
    };
    if existing.as_deref() == Some(out.as_str()) {
        return true;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    crate::fs::write_atomic(&path, out.as_bytes()).is_ok()
}

/// Pure merge for codex's `config.toml`: add a `[[hooks.<Event>]]` group for
/// each [`CODEX_STATUS_HOOKS`] entry into `existing` (or a fresh table),
/// dropping a stale spyc group from a prior launch and preserving the user's
/// own config (including our `[mcp_servers.spyc]`). `None` on a non-table
/// existing value or a serialization failure. Pure (no I/O) so the merge — and
/// its byte-level idempotency, which lets the write be skipped — is testable.
fn merged_codex_status_hooks_toml(
    existing: Option<&str>,
    exe: &str,
    trace: bool,
) -> Option<String> {
    let mut root = existing
        .and_then(|t| toml::from_str::<toml::Value>(t).ok())
        .filter(toml::Value::is_table)
        .unwrap_or_else(|| toml::Value::Table(toml::Table::new()));
    let obj = root.as_table_mut()?;
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let hooks_obj = hooks.as_table_mut()?;
    // `--status-trace` rides in the command string (not env) so the reporter
    // logs even when codex sanitizes the hook env.
    let suffix = if trace { " --status-trace" } else { "" };
    for (event, state) in CODEX_STATUS_HOOKS {
        let mut handler = toml::Table::new();
        handler.insert("type".into(), toml::Value::String("command".into()));
        handler.insert(
            "command".into(),
            toml::Value::String(format!("{exe} --report-status {state}{suffix}")),
        );
        let mut group = toml::Table::new();
        group.insert(
            "hooks".into(),
            toml::Value::Array(vec![toml::Value::Table(handler)]),
        );
        let arr = hooks_obj
            .entry(event)
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        let Some(list) = arr.as_array_mut() else {
            continue;
        };
        // Drop a stale spyc group from a prior launch, keep the user's, append ours.
        list.retain(|g| !codex_group_is_ours(g));
        list.push(toml::Value::Table(group));
    }
    toml::to_string_pretty(&root).ok()
}

/// Teardown counterpart: remove only spyc's hook groups from
/// `<dir>/.codex/config.toml`, preserving the user's config and our MCP entry
/// (cleaned separately by [`super::config::cleanup_codex_config`]). Empties
/// cascade as in the claude version; the file (and `.codex/`) is deleted only
/// when nothing else remains. Refuses a git-tracked file.
pub fn cleanup_codex_status_hooks(dir: &Path) -> ConfigCleanup {
    let codex_dir = dir.join(".codex");
    let path = codex_dir.join("config.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ConfigCleanup::NothingToDo;
    };
    let Ok(mut root) = toml::from_str::<toml::Value>(&text) else {
        return ConfigCleanup::NothingToDo;
    };
    let has_ours = root
        .get("hooks")
        .and_then(toml::Value::as_table)
        .is_some_and(|h| {
            CODEX_STATUS_HOOKS.iter().any(|(event, _)| {
                h.get(*event)
                    .and_then(toml::Value::as_array)
                    .is_some_and(|a| a.iter().any(codex_group_is_ours))
            })
        });
    if !has_ours {
        return ConfigCleanup::NothingToDo;
    }
    if crate::git::discovery::is_tracked(&path) {
        return ConfigCleanup::SkippedTracked;
    }
    let Some(obj) = root.as_table_mut() else {
        return ConfigCleanup::NothingToDo;
    };
    if let Some(hooks_obj) = obj.get_mut("hooks").and_then(toml::Value::as_table_mut) {
        for (event, _) in CODEX_STATUS_HOOKS {
            if let Some(list) = hooks_obj.get_mut(event).and_then(toml::Value::as_array_mut) {
                list.retain(|g| !codex_group_is_ours(g));
            }
        }
        // Drop emptied event arrays.
        hooks_obj.retain(|_, v| !v.as_array().is_some_and(Vec::is_empty));
    }
    // Drop the `hooks` key if it's now empty.
    if obj
        .get("hooks")
        .and_then(toml::Value::as_table)
        .is_some_and(toml::Table::is_empty)
    {
        obj.remove("hooks");
    }
    if obj.is_empty() {
        let _ = std::fs::remove_file(&path);
        // Remove `.codex/` too, but only if now empty (no-op otherwise).
        let _ = std::fs::remove_dir(&codex_dir);
        return ConfigCleanup::Cleaned;
    }
    if let Ok(out) = toml::to_string_pretty(&root) {
        let _ = crate::fs::write_atomic(&path, out.as_bytes());
    }
    ConfigCleanup::Cleaned
}

// ── Agy (Antigravity CLI) status hooks ────────────────────────────────
//
// Agy's JSON hooks live in `<dir>/.agents/hooks.json`. Its root is a map of
// *named hook-sets*; the matcher-less lifecycle events (`PreInvocation` /
// `Stop`) take a FLAT list of handlers directly under the event key (unlike
// claude/codex's `{hooks:[…]}` group + matcher, which agy uses only for
// PreToolUse). spyc owns one named set, `spyc-status`:
//
// ```json
// { "spyc-status": {
//     "PreInvocation": [{ "type": "command", "command": "spyc --report-status working" }],
//     "Stop":          [{ "type": "command", "command": "spyc --report-status done" }] } }
// ```
//
// **Partial** coverage: only `working` (turn start) + `done` (turn end). Agy
// exposes NO permission/approval event, so there's no `blocked` ("needs me")
// signal — the dot is accurate except it never shows the red waiting square.
// (Revisit if agy adds an approval hook.) Read at startup → written pre-spawn
// like codex (`live_reload: false`). The exact agy schema is derived from
// Google's hooks docs + community guides rather than a verified live install,
// so it may need a tweak as agy stabilizes.

/// Agy's (event, reported-state). No `blocked`: agy has no approval/permission
/// event to hang it on.
const AGY_STATUS_HOOKS: [(&str, &str); 2] = [("PreInvocation", "working"), ("Stop", "done")];

/// The named hook-set spyc owns in agy's `hooks.json` (our namespace there).
const AGY_HOOK_SET: &str = "spyc-status";

/// True when `set` (a named hook-set's value) is one of ours — any handler in
/// any of its event arrays runs `--report-status`. Belt-and-suspenders on top
/// of owning the `spyc-status` key, mirroring the claude/codex "ours" marker.
fn agy_set_is_ours(set: &Value) -> bool {
    set.as_object().is_some_and(|events| {
        events.values().any(|arr| {
            arr.as_array().is_some_and(|handlers| {
                handlers.iter().any(|h| {
                    h.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|c| c.contains("--report-status"))
                })
            })
        })
    })
}

/// Agy counterpart of [`ensure_claude_status_hooks`]: merge spyc's status hooks
/// into `<dir>/.agents/hooks.json`. Byte-idempotent skip, git-tracked guard,
/// and return contract mirror the claude version.
pub fn ensure_agy_status_hooks(dir: &Path) -> bool {
    let path = dir.join(".agents").join("hooks.json");
    if crate::git::discovery::is_tracked(&path) {
        return false;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));
    let existing = std::fs::read_to_string(&path).ok();
    let Some(out) = merged_agy_status_hooks_json(
        existing.as_deref(),
        &exe.to_string_lossy(),
        status_trace_enabled(),
    ) else {
        return false;
    };
    if existing.as_deref() == Some(out.as_str()) {
        return true;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    crate::fs::write_atomic(&path, out.as_bytes()).is_ok()
}

/// Pure merge for agy's `hooks.json`: own the `spyc-status` named set (replace
/// any prior ours, preserving the user's other sets). `None` on a non-object
/// existing value or a serialization failure. Pure → its byte-idempotency,
/// which lets the write be skipped, is testable.
fn merged_agy_status_hooks_json(existing: Option<&str>, exe: &str, trace: bool) -> Option<String> {
    let mut root = existing
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let obj = root.as_object_mut()?;
    let suffix = if trace { " --status-trace" } else { "" };
    let mut set = serde_json::Map::new();
    for (event, state) in AGY_STATUS_HOOKS {
        set.insert(
            event.to_string(),
            json!([{ "type": "command", "command": format!("{exe} --report-status {state}{suffix}") }]),
        );
    }
    // We own the whole `spyc-status` key, so an insert replaces a prior ours
    // outright — idempotent — without disturbing the user's other named sets.
    obj.insert(AGY_HOOK_SET.to_string(), Value::Object(set));
    serde_json::to_string_pretty(&root)
        .ok()
        .map(|out| out + "\n")
}

/// Teardown counterpart: remove only spyc's `spyc-status` set from
/// `<dir>/.agents/hooks.json`, preserving the user's other sets; delete the
/// file (and `.agents/` if now empty) when nothing else remains. Refuses a
/// git-tracked file.
pub fn cleanup_agy_status_hooks(dir: &Path) -> ConfigCleanup {
    let agents_dir = dir.join(".agents");
    let path = agents_dir.join("hooks.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ConfigCleanup::NothingToDo;
    };
    let Ok(mut root) = serde_json::from_str::<Value>(&text) else {
        return ConfigCleanup::NothingToDo;
    };
    if !root.get(AGY_HOOK_SET).is_some_and(agy_set_is_ours) {
        return ConfigCleanup::NothingToDo;
    }
    if crate::git::discovery::is_tracked(&path) {
        return ConfigCleanup::SkippedTracked;
    }
    let Some(obj) = root.as_object_mut() else {
        return ConfigCleanup::NothingToDo;
    };
    obj.remove(AGY_HOOK_SET);
    if obj.is_empty() {
        let _ = std::fs::remove_file(&path);
        // Remove `.agents/` too, but only if now empty (no-op otherwise — it
        // may hold skills / mcp_config.json).
        let _ = std::fs::remove_dir(&agents_dir);
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
        let once = merged_status_hooks_json(None, "spyc", false).expect("fresh merge");
        let twice = merged_status_hooks_json(Some(&once), "spyc", false).expect("re-merge");
        assert_eq!(once, twice, "re-applying the merge changed a byte");
        // It really installed our hooks (guards against a vacuous equality).
        assert!(once.contains("--report-status blocked"));
        assert!(
            !once.contains("--status-trace"),
            "trace off → no baked flag"
        );
        // A user's own settings/hooks survive the round-trip unchanged too.
        let with_user = merged_status_hooks_json(
            Some(r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo bye"}]}]}}"#),
            "spyc",
            false,
        )
        .expect("merge over user config");
        assert_eq!(
            with_user,
            merged_status_hooks_json(Some(&with_user), "spyc", false)
                .expect("re-merge over user config"),
            "merge over a user config is not idempotent"
        );
        assert!(with_user.contains("echo bye"), "user hook dropped");

        // `--status-trace` on bakes the flag into every command, still idempotently.
        let traced = merged_status_hooks_json(None, "spyc", true).expect("traced merge");
        assert!(traced.contains("--report-status blocked --status-trace"));
        assert_eq!(
            traced,
            merged_status_hooks_json(Some(&traced), "spyc", true).expect("re-merge traced"),
            "traced merge is not idempotent"
        );
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

    // ── codex (TOML, shares config.toml with the MCP entry) ───────────────

    fn read_toml(path: &Path) -> toml::Value {
        toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// The first handler `command` for a codex hook `event`, or "".
    fn codex_cmd(v: &toml::Value, event: &str) -> String {
        v.get("hooks")
            .and_then(|h| h.get(event))
            .and_then(toml::Value::as_array)
            .and_then(|a| a.first())
            .and_then(|g| g.get("hooks"))
            .and_then(toml::Value::as_array)
            .and_then(|a| a.first())
            .and_then(|h| h.get("command"))
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn writes_codex_status_hooks_then_cleans_them_leaving_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(ensure_codex_status_hooks(dir));
        let path = dir.join(".codex/config.toml");
        let v = read_toml(&path);
        for (event, state) in CODEX_STATUS_HOOKS {
            let cmd = codex_cmd(&v, event);
            assert!(
                cmd.contains("--report-status") && cmd.contains(state),
                "{event} → {state}: got {cmd:?}"
            );
        }
        assert!(matches!(
            cleanup_codex_status_hooks(dir),
            ConfigCleanup::Cleaned
        ));
        assert!(!path.exists(), "config.toml should be removed when emptied");
        assert!(
            !dir.join(".codex").exists(),
            "empty .codex dir should be removed"
        );
    }

    #[test]
    fn codex_hooks_coexist_with_the_mcp_entry_and_cleanup_leaves_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(".codex")).unwrap();
        let path = dir.join(".codex/config.toml");
        // A pre-existing MCP entry (as the codex MCP writer leaves it) + a user key.
        std::fs::write(
            &path,
            "model = \"gpt-5\"\n\n[mcp_servers.spyc]\ncommand = \"spyc\"\nargs = [\"--mcp\"]\n",
        )
        .unwrap();
        assert!(ensure_codex_status_hooks(dir));
        let v = read_toml(&path);
        assert!(v.get("hooks").is_some(), "hooks added");
        assert!(v.get("mcp_servers").is_some(), "MCP entry preserved");
        assert_eq!(v.get("model").and_then(toml::Value::as_str), Some("gpt-5"));

        // Idempotent re-write doesn't disturb the file.
        assert!(ensure_codex_status_hooks(dir));

        // Cleanup removes ONLY our hooks; the MCP entry + user key survive.
        assert!(matches!(
            cleanup_codex_status_hooks(dir),
            ConfigCleanup::Cleaned
        ));
        let after = read_toml(&path);
        assert!(after.get("hooks").is_none(), "our hooks removed");
        assert!(after.get("mcp_servers").is_some(), "MCP entry preserved");
        assert_eq!(
            after.get("model").and_then(toml::Value::as_str),
            Some("gpt-5")
        );
    }

    #[test]
    fn cleanup_codex_hooks_is_a_noop_when_nothing_of_ours() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(".codex")).unwrap();
        std::fs::write(dir.join(".codex/config.toml"), "model = \"gpt-5\"\n").unwrap();
        assert!(matches!(
            cleanup_codex_status_hooks(dir),
            ConfigCleanup::NothingToDo
        ));
        assert!(dir.join(".codex/config.toml").exists());
    }

    #[test]
    fn codex_merge_is_byte_idempotent() {
        // The invariant that lets `ensure_codex_status_hooks` skip a re-write:
        // re-applying the merge to its own output reproduces it byte-for-byte.
        let once = merged_codex_status_hooks_toml(None, "spyc", false).expect("fresh merge");
        let twice = merged_codex_status_hooks_toml(Some(&once), "spyc", false).expect("re-merge");
        assert_eq!(once, twice, "re-applying the merge changed a byte");
        assert!(once.contains("--report-status blocked"));
        assert!(
            !once.contains("--status-trace"),
            "trace off → no baked flag"
        );

        // A user's own codex config (incl. our MCP entry) survives the round-trip.
        let with_user = merged_codex_status_hooks_toml(
            Some("model = \"gpt-5\"\n[mcp_servers.spyc]\ncommand = \"spyc\"\n"),
            "spyc",
            false,
        )
        .expect("merge over user config");
        assert_eq!(
            with_user,
            merged_codex_status_hooks_toml(Some(&with_user), "spyc", false)
                .expect("re-merge over user config"),
            "merge over a user config is not idempotent"
        );
        assert!(with_user.contains("gpt-5"), "user config dropped");
        assert!(with_user.contains("mcp_servers"), "MCP entry dropped");

        // `--status-trace` on bakes the flag into every command, still idempotently.
        let traced = merged_codex_status_hooks_toml(None, "spyc", true).expect("traced merge");
        assert!(traced.contains("--report-status blocked --status-trace"));
        assert_eq!(
            traced,
            merged_codex_status_hooks_toml(Some(&traced), "spyc", true).expect("re-merge traced"),
            "traced merge is not idempotent"
        );
    }

    // ── agy (.agents/hooks.json, named hook-set, flat handler lists) ──────

    #[test]
    fn writes_agy_status_hooks_then_cleans_them_leaving_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(ensure_agy_status_hooks(dir));
        let path = dir.join(".agents/hooks.json");
        let v = read(&path);
        for (event, state) in AGY_STATUS_HOOKS {
            // Flat handler list directly under the event key (no matcher/group).
            let cmd = v
                .pointer(&format!("/{AGY_HOOK_SET}/{event}/0/command"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(
                cmd.contains("--report-status") && cmd.contains(state),
                "{event} → {state}: got {cmd:?}"
            );
            assert_eq!(
                v.pointer(&format!("/{AGY_HOOK_SET}/{event}/0/type"))
                    .and_then(Value::as_str),
                Some("command")
            );
        }
        assert!(matches!(
            cleanup_agy_status_hooks(dir),
            ConfigCleanup::Cleaned
        ));
        assert!(!path.exists(), "hooks.json should be removed when emptied");
        assert!(
            !dir.join(".agents").exists(),
            "empty .agents dir should be removed"
        );
    }

    #[test]
    fn agy_hooks_preserve_user_sets_and_cleanup_leaves_them() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(".agents")).unwrap();
        let path = dir.join(".agents/hooks.json");
        // A pre-existing user hook-set.
        std::fs::write(
            &path,
            r#"{"linter":{"PostToolUse":[{"matcher":"write_to_file","hooks":[{"type":"command","command":"./lint.sh"}]}]}}"#,
        )
        .unwrap();
        assert!(ensure_agy_status_hooks(dir));
        let v = read(&path);
        assert!(v.get("linter").is_some(), "user set preserved");
        assert!(v.get(AGY_HOOK_SET).is_some(), "our set added");

        // Idempotent re-write.
        assert!(ensure_agy_status_hooks(dir));

        // Cleanup removes ONLY our set; the user's linter set survives.
        assert!(matches!(
            cleanup_agy_status_hooks(dir),
            ConfigCleanup::Cleaned
        ));
        let after = read(&path);
        assert!(after.get(AGY_HOOK_SET).is_none(), "our set removed");
        assert!(after.get("linter").is_some(), "user set preserved");
        assert_eq!(
            after.pointer("/linter/PostToolUse/0/hooks/0/command"),
            Some(&Value::String("./lint.sh".into()))
        );
    }

    #[test]
    fn cleanup_agy_hooks_is_a_noop_when_nothing_of_ours() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(".agents")).unwrap();
        std::fs::write(dir.join(".agents/hooks.json"), r#"{"linter":{}}"#).unwrap();
        assert!(matches!(
            cleanup_agy_status_hooks(dir),
            ConfigCleanup::NothingToDo
        ));
        assert!(dir.join(".agents/hooks.json").exists());
    }

    #[test]
    fn agy_merge_is_byte_idempotent() {
        let once = merged_agy_status_hooks_json(None, "spyc", false).expect("fresh merge");
        let twice = merged_agy_status_hooks_json(Some(&once), "spyc", false).expect("re-merge");
        assert_eq!(once, twice, "re-applying the merge changed a byte");
        assert!(once.contains("--report-status working"));
        assert!(once.contains("--report-status done"));
        assert!(
            !once.contains("blocked"),
            "agy has no blocked signal (no approval event)"
        );
        assert!(
            !once.contains("--status-trace"),
            "trace off → no baked flag"
        );

        let with_user = merged_agy_status_hooks_json(
            Some(r#"{"linter":{"Stop":[{"type":"command","command":"./x.sh"}]}}"#),
            "spyc",
            false,
        )
        .expect("merge over user config");
        assert_eq!(
            with_user,
            merged_agy_status_hooks_json(Some(&with_user), "spyc", false).expect("re-merge"),
            "merge over a user config is not idempotent"
        );
        assert!(with_user.contains("./x.sh"), "user set dropped");

        let traced = merged_agy_status_hooks_json(None, "spyc", true).expect("traced merge");
        assert!(traced.contains("--report-status working --status-trace"));
        assert_eq!(
            traced,
            merged_agy_status_hooks_json(Some(&traced), "spyc", true).expect("re-merge traced"),
        );
    }
}

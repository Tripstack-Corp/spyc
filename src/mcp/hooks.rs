//! Agent status hooks: spyc installs command hooks into a launched agent
//! pane's project config so the agent auto-reports its activity
//! (`spyc --report-status <state>`) on lifecycle events, driving the per-tab
//! activity dot WITHOUT the agent having to call the `report_status` tool
//! itself. The reporter reads `SPYC_MCP_SOCK` + `SPYC_PANE_ID` from the env
//! spyc injects into the pane (inherited by the hook).
//!
//! The reporter binary is resolved PATH-first (a bare `spyc` when one is on
//! `$PATH`, else the running binary's absolute path), and the command is
//! fail-soft (`… 2>/dev/null || true`), so a hook survives the binary MOVING
//! (a cleaned build dir, `~/.local/bin` → Homebrew) and a moved/uninstalled
//! spyc degrades to a no-op instead of erroring every turn. See
//! [`reporter_binary`] / [`reporter_command`].
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

use std::path::Path;
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

/// The spyc binary to bake into an installed status-hook command. Prefers a
/// `spyc` discoverable on `$PATH` so the hook survives the binary MOVING (a
/// cleaned build dir, `~/.local/bin` → Homebrew, an apt↔brew switch): the
/// reporter only needs *some* spyc that speaks `--report-status`, not the exact
/// running binary. Falls back to the running binary's absolute path when spyc
/// isn't on PATH. `None` only when neither is a UTF-8 string embeddable in the
/// command (mirrors the prior skip-on-non-UTF-8-exe behavior).
fn reporter_binary() -> Option<String> {
    if spyc_on_path() {
        return Some("spyc".to_string());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.to_str().map(str::to_owned))
}

/// Is an executable `spyc` resolvable on `$PATH`?
fn spyc_on_path() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable_file(&dir.join("spyc")))
}

/// A regular file with any execute bit set.
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Build the shell command an installed status hook runs: the (shell-quoted)
/// reporter binary, `--report-status <state>`, `--status-trace` when tracing, and
/// a fail-soft tail. The reporter is fire-and-forget (it just pings the pane's
/// MCP socket), so a moved/uninstalled binary must degrade to a no-op — WITHOUT
/// the `2>/dev/null || true`, a missing binary's nonzero exit surfaces as a
/// per-turn hook error in the agent (the "No such file or directory" report a
/// stale absolute path produced). The `--report-status` token stays in the
/// string: it's the "ours" marker cleanup keys on.
fn reporter_command(exe: &str, state: &str, trace: bool) -> String {
    let trace = if trace { " --status-trace" } else { "" };
    // Shell-quote so a spaced install path stays one token or the hook never fires.
    let exe = crate::shell::shell_quote(exe);
    format!("{exe} --report-status {state}{trace} 2>/dev/null || true")
}

/// The (event, matcher, reported-state) hooks spyc installs. `matcher` is the
/// tool-name pattern for per-tool events (`""` = all / not-a-tool event). Three
/// events map to `blocked`: `PermissionRequest`, `Notification` (idle backstop),
/// and `PreToolUse` for `AskUserQuestion`/`ExitPlanMode` (mid-turn tools that
/// fire none of the other events). See the module doc for the full mapping.
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
    // Resolve the reporter binary (PATH-preferring, so the hook survives the
    // binary moving); skip on a non-UTF-8 path a shell would mis-exec.
    let Some(exe) = reporter_binary() else {
        return false;
    };
    let existing = std::fs::read_to_string(&path).ok();
    let Some(out) = merged_status_hooks_json(existing.as_deref(), &exe, status_trace_enabled())
    else {
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
    for (event, matcher, state) in STATUS_HOOKS {
        let group = json!({
            "matcher": matcher,
            "hooks": [ { "type": "command", "command": reporter_command(exe, state, trace) } ],
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
// Codex's hooks are inline `[[hooks.<Event>]]` tables in the same
// `.codex/config.toml` as the MCP entry (see [`super::config`]), not a separate
// file. `UserPromptSubmit` → working, `PermissionRequest` → blocked, `Stop` →
// done; codex has no Notification/idle event. It reads config once at startup
// (no live reload), so hooks are written pre-spawn; a first-launch `yes` only
// takes effect on codex's next launch.

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
    let Some(exe) = reporter_binary() else {
        return false;
    };
    let existing = std::fs::read_to_string(&path).ok();
    let Some(out) =
        merged_codex_status_hooks_toml(existing.as_deref(), &exe, status_trace_enabled())
    else {
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
    for (event, state) in CODEX_STATUS_HOOKS {
        let mut handler = toml::Table::new();
        handler.insert("type".into(), toml::Value::String("command".into()));
        handler.insert(
            "command".into(),
            toml::Value::String(reporter_command(exe, state, trace)),
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
// Agy's JSON hooks live in `<dir>/.agents/hooks.json` as named hook-sets; the
// lifecycle events (`PreInvocation` / `Stop`) take a flat handler list under
// the event key (agy uses the claude/codex `{hooks,matcher}` group only for
// PreToolUse). spyc owns one set, `spyc-status`: PreInvocation → working, Stop
// → done. PARTIAL — agy exposes no approval event, so there's no `blocked`.
// Read once at startup (written pre-spawn like codex). The schema is derived
// from docs, not a verified live install.

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
    let Some(exe) = reporter_binary() else {
        return false;
    };
    let existing = std::fs::read_to_string(&path).ok();
    let Some(out) = merged_agy_status_hooks_json(existing.as_deref(), &exe, status_trace_enabled())
    else {
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
    let mut set = serde_json::Map::new();
    for (event, state) in AGY_STATUS_HOOKS {
        set.insert(
            event.to_string(),
            json!([{ "type": "command", "command": reporter_command(exe, state, trace) }]),
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
    fn hook_command_shell_quotes_spaced_exe_path() {
        // An install path with a space must be single-quoted so the shell execs
        // the whole path — an unquoted `/Users/My User/spyc` would exec `/Users/My`
        // and the hook would silently never fire. All three writers share the guard.
        let exe = "/Users/My User/bin/spyc";
        let quoted = "'/Users/My User/bin/spyc' --report-status";
        let claude = merged_status_hooks_json(None, exe, false).expect("claude merge");
        assert!(claude.contains(quoted), "claude not quoted: {claude}");
        let codex = merged_codex_status_hooks_toml(None, exe, false).expect("codex merge");
        assert!(codex.contains(quoted), "codex not quoted: {codex}");
        let agy = merged_agy_status_hooks_json(None, exe, false).expect("agy merge");
        assert!(agy.contains(quoted), "agy not quoted: {agy}");
    }

    #[test]
    fn reporter_command_is_fail_soft_and_quoted() {
        // A moved/uninstalled binary must no-op, not error the agent turn: every
        // installed command ends in the fail-soft tail; `--status-trace` rides
        // before it.
        // `shell_quote` always single-quotes; a quoted word with no slash is
        // still PATH-resolved by the shell, so the bare-name preference holds.
        assert_eq!(
            reporter_command("spyc", "done", false),
            "'spyc' --report-status done 2>/dev/null || true"
        );
        assert_eq!(
            reporter_command("spyc", "working", true),
            "'spyc' --report-status working --status-trace 2>/dev/null || true"
        );
        // A spaced install path stays one shell token, tail still present, and the
        // `--report-status` "ours" marker survives (cleanup keys on it).
        let q = reporter_command("/opt/my spyc/spyc", "blocked", false);
        assert!(
            q.starts_with("'/opt/my spyc/spyc' --report-status blocked"),
            "not quoted: {q}"
        );
        assert!(
            q.ends_with(" 2>/dev/null || true"),
            "no fail-soft tail: {q}"
        );
        assert!(q.contains("--report-status"));
    }

    #[test]
    fn is_executable_file_requires_the_exec_bit() {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("spyc");
        std::fs::write(&f, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&f, Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable_file(&f), "a non-exec file must not resolve");
        std::fs::set_permissions(&f, Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable_file(&f), "an exec file resolves");
        // A directory (even +x) is not an executable file, and a missing path is not.
        let d = tmp.path().join("dir");
        std::fs::create_dir(&d).unwrap();
        assert!(!is_executable_file(&d), "a dir is not an executable file");
        assert!(!is_executable_file(&tmp.path().join("missing")));
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

use std::path::Path;

use super::{
    ConfigCleanup, merge_root_toml, reporter_binary, reporter_command, status_trace_enabled,
};

// ── Codex status hooks ────────────────────────────────────────────────
//
// Codex's hooks are inline `[[hooks.<Event>]]` tables in `.codex/config.toml`.
// The app passes the root-checkout source Codex consumes; this writer operates
// on that literal directory. Ordinary MCP config can remain worktree-local.
// Codex also loads `.codex/hooks.json`; spyc migrates its reporters out of it.
// `UserPromptSubmit` → working, `Stop` → done, `Interrupt` → idle.
// `PermissionRequest` retains its wire command for hook-trust stability; a
// metadata-capable host records it observationally because it precedes both
// automatic and human review. It reads config once at startup
// (no live reload), so hooks are written pre-spawn; a first-launch `yes` only
// takes effect on codex's next launch.

/// Lifecycle events are unfiltered; tool hooks match only `request_user_input`.
/// The `--report-status` command string identifies owned handlers for cleanup.
const CODEX_STATUS_HOOKS: [(&str, &str); 6] = [
    ("UserPromptSubmit", "working"),
    ("PermissionRequest", "blocked"),
    ("Stop", "done"),
    ("Interrupt", "idle"),
    ("PreToolUse", crate::agent::codex_recovery::QUESTION_START),
    ("PostToolUse", crate::agent::codex_recovery::QUESTION_END),
];

/// TOML counterpart of [`super::group_is_ours`]: a `{ hooks = [{ command = … }] }`
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

/// Codex counterpart of [`super::ensure_claude_status_hooks`]: merge spyc's status
/// hooks into `<dir>/.codex/config.toml` at the caller's resolved hook source,
/// preserving everything else. Legacy JSON reporters are removed only after
/// the canonical file is durable; an unsafe legacy source refuses installation.
/// Other handlers survive even when they share a matcher group with a reporter.
pub fn ensure_codex_status_hooks(dir: &Path) -> bool {
    let path = dir.join(".codex").join("config.toml");
    if crate::git::discovery::is_tracked(&path) {
        return false;
    }
    let Some(exe) = reporter_binary() else {
        return false;
    };
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return false,
    };
    let Some(out) =
        merged_codex_status_hooks_toml(existing.as_deref(), &exe, status_trace_enabled())
    else {
        return false;
    };
    let Ok(legacy) = legacy_edit(dir) else {
        return false;
    };
    if existing.as_deref() != Some(out.as_str()) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if crate::fs::write_atomic(&path, out.as_bytes()).is_err() {
            return false;
        }
    }
    // The canonical file is durable before removing a legacy reporter.
    // Codex executes both representations; unchanged TOML still needs migration.
    apply_legacy_edit(dir, legacy).is_ok()
}

/// Pure merge for codex's `config.toml`: add a `[[hooks.<Event>]]` group for
/// each [`CODEX_STATUS_HOOKS`] entry into `existing` (or a fresh table),
/// dropping stale spyc handlers from a prior launch and preserving the user's
/// own config (including our `[mcp_servers.spyc]`). `None` when we refuse: an
/// unparseable existing file, a non-table root or `hooks` value, or a
/// serialization failure — see [`merge_root_toml`]. Pure (no I/O) so the merge — and
/// its byte-level idempotency, which lets the write be skipped — is testable.
pub(super) fn merged_codex_status_hooks_toml(
    existing: Option<&str>,
    exe: &str,
    trace: bool,
) -> Option<String> {
    let mut root = merge_root_toml(existing)?;
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
        if matches!(event, "PreToolUse" | "PostToolUse") {
            group.insert(
                "matcher".into(),
                toml::Value::String("^request_user_input$".into()),
            );
        }
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
        // Drop stale spyc handlers, preserve user handlers, append ours.
        prune_toml_groups(list);
        list.push(toml::Value::Table(group));
    }
    toml::to_string_pretty(&root).ok()
}

/// Teardown counterpart: remove spyc's handlers from both Codex hook sources,
/// preserving the user's config and our MCP entry (cleaned separately by
/// [`crate::mcp::config::cleanup_codex_config`]). Empties
/// cascade as in the claude version; the file (and `.codex/`) is deleted only
/// when nothing else remains. Refuses a git-tracked file.
pub fn cleanup_codex_status_hooks(dir: &Path) -> ConfigCleanup {
    let legacy = match legacy_edit(dir) {
        Ok(edit) => match apply_legacy_edit(dir, edit) {
            Ok(true) => ConfigCleanup::Cleaned,
            _ => ConfigCleanup::NothingToDo,
        },
        Err(LegacyError::Tracked) => ConfigCleanup::SkippedTracked,
        Err(_) => ConfigCleanup::NothingToDo,
    };
    let canonical = cleanup_codex_toml(dir);
    match (legacy, canonical) {
        (ConfigCleanup::SkippedTracked, _) | (_, ConfigCleanup::SkippedTracked) => {
            ConfigCleanup::SkippedTracked
        }
        (ConfigCleanup::Cleaned, _) | (_, ConfigCleanup::Cleaned) => ConfigCleanup::Cleaned,
        _ => ConfigCleanup::NothingToDo,
    }
}

fn cleanup_codex_toml(dir: &Path) -> ConfigCleanup {
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
                prune_toml_groups(list);
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

/// Remove individual reporters so a matcher group shared with user hooks survives.
fn prune_toml_groups(groups: &mut Vec<toml::Value>) {
    groups.retain_mut(|group| {
        if !codex_group_is_ours(group) {
            return true;
        }
        let Some(handlers) = group.get_mut("hooks").and_then(toml::Value::as_array_mut) else {
            return true;
        };
        handlers.retain(|handler| {
            !handler
                .get("command")
                .and_then(toml::Value::as_str)
                .is_some_and(|command| command.contains("--report-status"))
        });
        !handlers.is_empty()
    });
}

enum LegacyEdit {
    Keep,
    Write(String),
    Remove,
}

enum LegacyError {
    Invalid,
    Unreadable,
    Tracked,
}

/// Plan a precise removal from Codex's second, independently loaded hook source.
/// Invalid and tracked sources are preserved before any canonical-file write.
fn legacy_edit(dir: &Path) -> Result<LegacyEdit, LegacyError> {
    let path = dir.join(".codex/hooks.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(LegacyEdit::Keep),
        Err(_) => return Err(LegacyError::Unreadable),
    };
    let edit = pruned_legacy_hooks_json(&text)?;
    if !matches!(edit, LegacyEdit::Keep) && crate::git::discovery::is_tracked(&path) {
        return Err(LegacyError::Tracked);
    }
    Ok(edit)
}

fn pruned_legacy_hooks_json(text: &str) -> Result<LegacyEdit, LegacyError> {
    let mut root: serde_json::Value =
        serde_json::from_str(text).map_err(|_| LegacyError::Invalid)?;
    let object = root.as_object_mut().ok_or(LegacyError::Invalid)?;
    let Some(hooks) = object.get_mut("hooks") else {
        return Ok(LegacyEdit::Keep);
    };
    let hooks = hooks.as_object_mut().ok_or(LegacyError::Invalid)?;
    let mut changed = false;
    for groups in hooks.values_mut() {
        let groups = groups.as_array_mut().ok_or(LegacyError::Invalid)?;
        groups.retain_mut(|group| {
            if !super::group_is_ours(group) {
                return true;
            }
            let Some(handlers) = group
                .get_mut("hooks")
                .and_then(serde_json::Value::as_array_mut)
            else {
                return true;
            };
            handlers.retain(|handler| {
                !handler
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|command| command.contains("--report-status"))
            });
            changed = true;
            !handlers.is_empty()
        });
    }
    if !changed {
        return Ok(LegacyEdit::Keep);
    }
    hooks.retain(|_, groups| !groups.as_array().is_some_and(Vec::is_empty));
    if hooks.is_empty() {
        object.remove("hooks");
    }
    if object.is_empty() {
        return Ok(LegacyEdit::Remove);
    }
    serde_json::to_string_pretty(&root)
        .map(|text| LegacyEdit::Write(text + "\n"))
        .map_err(|_| LegacyError::Invalid)
}

fn apply_legacy_edit(dir: &Path, edit: LegacyEdit) -> std::io::Result<bool> {
    let path = dir.join(".codex/hooks.json");
    match edit {
        LegacyEdit::Keep => Ok(false),
        LegacyEdit::Write(text) => crate::fs::write_atomic(&path, text.as_bytes()).map(|()| true),
        LegacyEdit::Remove => {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            }
            let _ = std::fs::remove_dir(dir.join(".codex"));
            Ok(true)
        }
    }
}

pub fn codex_legacy_hook_diagnostic(dir: &Path) -> Option<&'static str> {
    match legacy_edit(dir) {
        Ok(LegacyEdit::Keep) => None,
        Ok(_) => Some(
            "additional spyc reporters in .codex/hooks.json; run `:hooks on`, then restart Codex",
        ),
        Err(LegacyError::Tracked) => Some(
            "spyc reporters in git-tracked .codex/hooks.json; remove those reporters by hand, then restart Codex",
        ),
        Err(LegacyError::Invalid) => Some(
            "legacy .codex/hooks.json is malformed; repair it before `:hooks on` and restarting Codex",
        ),
        Err(LegacyError::Unreadable) => Some(
            "legacy .codex/hooks.json is unreadable; restore access before `:hooks on` and restarting Codex",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

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

    fn legacy_file(dir: &Path, hooks: Value) -> std::path::PathBuf {
        std::fs::create_dir_all(dir.join(".codex")).unwrap();
        let path = dir.join(".codex/hooks.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&hooks).unwrap()).unwrap();
        path
    }

    fn own_handler(state: &str) -> Value {
        json!({"type":"command", "command":format!("spyc --report-status {state} 2>/dev/null || true")})
    }

    #[test]
    fn codex_migration_removes_duplicate_reporters_and_preserves_mixed_user_groups() {
        let tmp = tempfile::tempdir().unwrap();
        let user = json!({"type":"command", "command":"user-reporter", "timeout":7});
        let path = legacy_file(
            tmp.path(),
            json!({"extra":"preserve", "hooks":{
                "UserPromptSubmit":[{"hooks":[own_handler("working")]}],
                "PermissionRequest":[{"matcher":"Bash", "extra":"group", "hooks":[own_handler("blocked"), user]}],
                "Stop":[{"hooks":[own_handler("done")]}],
                "PreToolUse":[{"matcher":"AskUserQuestion|ExitPlanMode", "hooks":[own_handler("blocked")]}],
                "FutureEvent":[{"hooks":[user]}]
            }}),
        );
        assert!(ensure_codex_status_hooks(tmp.path()));
        let left: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            left,
            json!({"extra":"preserve", "hooks":{
                "PermissionRequest":[{"matcher":"Bash", "extra":"group", "hooks":[user]}],
                "FutureEvent":[{"hooks":[user]}]
            }})
        );
        let installed = read_toml(&tmp.path().join(".codex/config.toml"));
        for event in ["UserPromptSubmit", "PermissionRequest", "Stop"] {
            let handlers = installed["hooks"][event].as_array().unwrap();
            assert_eq!(handlers.len(), 1, "one canonical reporter for {event}");
        }
        let once = std::fs::read(&path).unwrap();
        assert!(ensure_codex_status_hooks(tmp.path()));
        assert_eq!(std::fs::read(&path).unwrap(), once);
    }

    #[test]
    fn codex_migration_runs_even_when_toml_is_already_current() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(ensure_codex_status_hooks(tmp.path()));
        let toml_path = tmp.path().join(".codex/config.toml");
        let before = std::fs::read(&toml_path).unwrap();
        let json_path = legacy_file(
            tmp.path(),
            json!({"hooks":{
                "Stop":[{"hooks":[own_handler("done")]}]
            }}),
        );
        assert!(ensure_codex_status_hooks(tmp.path()));
        assert!(
            !json_path.exists(),
            "empty legacy hook file must be removed"
        );
        assert_eq!(std::fs::read(toml_path).unwrap(), before);
    }

    #[test]
    fn codex_migration_refuses_malformed_or_unreadable_legacy_files_without_installing_duplicates()
    {
        for content in ["", "   ", "{broken", r#"{"hooks":42}"#, "[1,2]"] {
            let tmp = tempfile::tempdir().unwrap();
            let path = legacy_file(tmp.path(), json!({}));
            std::fs::write(&path, content).unwrap();
            assert!(!ensure_codex_status_hooks(tmp.path()), "refuse {content}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
            assert!(!tmp.path().join(".codex/config.toml").exists());
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".codex/hooks.json")).unwrap();
        assert!(!ensure_codex_status_hooks(tmp.path()));
        assert!(!tmp.path().join(".codex/config.toml").exists());
    }

    #[test]
    fn codex_migration_refuses_tracked_reporters_and_leaves_user_only_files_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let path = legacy_file(
            tmp.path(),
            json!({"hooks":{
                "Stop":[{"hooks":[own_handler("done")]}]
            }}),
        );
        crate::git::test_support::run_git(tmp.path(), &["init", "-q"]);
        crate::git::test_support::run_git(tmp.path(), &["add", ".codex/hooks.json"]);
        let before = std::fs::read(&path).unwrap();
        assert!(!ensure_codex_status_hooks(tmp.path()));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(!tmp.path().join(".codex/config.toml").exists());
        assert!(matches!(
            cleanup_codex_status_hooks(tmp.path()),
            ConfigCleanup::SkippedTracked
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);

        std::fs::write(
            &path,
            "{\"hooks\": {\"Stop\": [{\"hooks\": [{\"command\": \"user-reporter\"}]}]}}\n",
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(
            ensure_codex_status_hooks(tmp.path()),
            "tracked user hooks do not block installation"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        cleanup_codex_status_hooks(tmp.path());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn codex_cleanup_removes_legacy_only_reporters_without_requiring_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let path = legacy_file(
            tmp.path(),
            json!({"hooks":{
                "PreToolUse":[{"matcher":"AskUserQuestion|ExitPlanMode", "hooks":[own_handler("blocked")]}]
            }}),
        );
        assert!(matches!(
            cleanup_codex_status_hooks(tmp.path()),
            ConfigCleanup::Cleaned
        ));
        assert!(!path.exists());
        assert!(!tmp.path().join(".codex").exists());
    }

    #[test]
    fn codex_migration_preserves_legacy_reporters_if_the_canonical_config_is_invalid() {
        let tmp = tempfile::tempdir().unwrap();
        let path = legacy_file(
            tmp.path(),
            json!({"hooks":{
                "Stop":[{"hooks":[own_handler("done")]}]
            }}),
        );
        let before = std::fs::read(&path).unwrap();
        let config = tmp.path().join(".codex/config.toml");
        std::fs::write(&config, "model = \n").unwrap();
        assert!(!ensure_codex_status_hooks(tmp.path()));
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert_eq!(std::fs::read_to_string(config).unwrap(), "model = \n");
    }

    #[test]
    fn codex_merge_and_cleanup_preserve_user_handlers_sharing_a_toml_group() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".codex")).unwrap();
        let path = tmp.path().join(".codex/config.toml");
        std::fs::write(&path, "[[hooks.Stop]]\nmatcher = 'keep-group'\n[[hooks.Stop.hooks]]\ncommand = 'user-reporter'\n[[hooks.Stop.hooks]]\ncommand = 'spyc --report-status done'\n[hooks.state.user]\ntrusted_hash = 'unchanged'\n").unwrap();
        assert!(ensure_codex_status_hooks(tmp.path()));
        let installed = read_toml(&path);
        assert_eq!(codex_cmd(&installed, "Stop"), "user-reporter");
        assert_eq!(
            installed["hooks"]["state"]["user"]["trusted_hash"].as_str(),
            Some("unchanged")
        );
        cleanup_codex_status_hooks(tmp.path());
        let left = read_toml(&path);
        assert_eq!(left["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(codex_cmd(&left, "Stop"), "user-reporter");
        assert_eq!(
            left["hooks"]["Stop"][0]["matcher"].as_str(),
            Some("keep-group")
        );
        assert_eq!(
            left["hooks"]["state"]["user"]["trusted_hash"].as_str(),
            Some("unchanged")
        );
    }

    #[test]
    fn codex_legacy_diagnostics_explain_refusals_and_clear_after_migration() {
        let tmp = tempfile::tempdir().unwrap();
        let path = legacy_file(
            tmp.path(),
            json!({"hooks":{
                "Stop":[{"hooks":[own_handler("done")]}]
            }}),
        );
        assert!(
            codex_legacy_hook_diagnostic(tmp.path())
                .unwrap()
                .contains("additional spyc reporters")
        );
        assert!(ensure_codex_status_hooks(tmp.path()));
        assert!(codex_legacy_hook_diagnostic(tmp.path()).is_none());
        std::fs::write(&path, "{broken").unwrap();
        assert!(
            codex_legacy_hook_diagnostic(tmp.path())
                .unwrap()
                .contains("malformed")
        );
    }
}

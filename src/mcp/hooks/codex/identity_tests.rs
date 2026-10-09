use super::*;
use serde_json::json;

fn legacy_reporter() -> serde_json::Value {
    json!({"type":"command", "command":"spyc --report-status done"})
}

fn user_handler() -> serde_json::Value {
    json!({"type":"command", "command":"printf user-hook", "timeout":7})
}

#[test]
fn legacy_migration_and_cleanup_refuse_to_move_user_hook_trust_positions() {
    for groups in [
        json!([{ "hooks": [legacy_reporter(), user_handler()] }]),
        json!([{ "hooks": [legacy_reporter()] }, { "hooks": [user_handler()] }]),
    ] {
        let temp = tempfile::tempdir().unwrap();
        // A current canonical file must not be rewritten before unsafe legacy
        // pruning is detected; cleanup must not partially remove the sources.
        assert!(ensure_codex_status_hooks(temp.path()));
        let canonical = temp.path().join(".codex/config.toml");
        let canonical_before = std::fs::read(&canonical).unwrap();
        let path = temp.path().join(".codex/hooks.json");
        let before = serde_json::to_vec_pretty(&json!({"hooks":{"Stop":groups}})).unwrap();
        std::fs::write(&path, &before).unwrap();
        assert!(
            !ensure_codex_status_hooks(temp.path()),
            "migration moved a user hook"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read(&canonical).unwrap(), canonical_before);
        assert!(matches!(
            cleanup_codex_status_hooks(temp.path()),
            ConfigCleanup::NothingToDo
        ));
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert_eq!(std::fs::read(canonical).unwrap(), canonical_before);
        let diagnostic = codex_legacy_hook_diagnostic(temp.path()).unwrap();
        assert!(
            diagnostic.contains("user hook") && diagnostic.contains("trust"),
            "{diagnostic}"
        );
    }
}

#[test]
fn inline_install_and_cleanup_refuse_to_move_user_hook_trust_positions() {
    for hooks in [
        "[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'spyc --report-status done'\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'printf user-hook'\ntimeout = 7\n",
        "[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'spyc --report-status done'\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'printf user-hook'\ntimeout = 7\n",
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".codex")).unwrap();
        let path = temp.path().join(".codex/config.toml");
        std::fs::write(&path, hooks).unwrap();
        let legacy = temp.path().join(".codex/hooks.json");
        let legacy_before =
            serde_json::to_vec(&json!({"hooks":{"Stop":[{"hooks":[legacy_reporter()]}]}})).unwrap();
        std::fs::write(&legacy, &legacy_before).unwrap();
        assert!(
            !ensure_codex_status_hooks(temp.path()),
            "installation moved a user hook"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), hooks);
        assert!(matches!(
            cleanup_codex_status_hooks(temp.path()),
            ConfigCleanup::NothingToDo
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap(), hooks);
        assert_eq!(
            std::fs::read(legacy).unwrap(),
            legacy_before,
            "unsafe inline cleanup must not remove the legacy source first"
        );
        let diagnostic = codex_legacy_hook_diagnostic(temp.path()).unwrap();
        assert!(
            diagnostic.contains("user hook") && diagnostic.contains("trust"),
            "{diagnostic}"
        );
    }
}

#[test]
fn safe_trailing_prunes_leave_user_positions_and_metadata_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".codex")).unwrap();
    let path = temp.path().join(".codex/hooks.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({"hooks":{"Stop":[
            {"matcher":"keep-group", "hooks":[user_handler(), legacy_reporter()]},
            {"hooks":[legacy_reporter()]}
        ]}}))
        .unwrap(),
    )
    .unwrap();
    assert!(ensure_codex_status_hooks(temp.path()));
    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(migrated["hooks"]["Stop"][0]["hooks"][0], user_handler());
    assert_eq!(migrated["hooks"]["Stop"][0]["matcher"], "keep-group");
}

fn unrelated_reporter_mentions() -> Vec<&'static str> {
    vec![
        "printf '%s' '--report-status'",
        "echo 'spyc --report-status done'",
        "other-tool --report-status done",
        "spyc --report-status done; printf user-hook",
        "spyc --report-status done-custom",
        "env spyc --report-status done",
        "$(echo spyc) --report-status done",
    ]
}

#[test]
fn legacy_migration_preserves_unrelated_flag_mentions_and_user_positions() {
    for command in unrelated_reporter_mentions() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".codex")).unwrap();
        let user = json!({"type":"command", "command":command, "timeout":7});
        let expected =
            json!({"extra":"keep", "hooks":{"Stop":[{"matcher":"keep-group", "hooks":[user]}]}});
        let mut before = expected.clone();
        before["hooks"]["Stop"][0]["hooks"]
            .as_array_mut()
            .unwrap()
            .push(legacy_reporter());
        let path = temp.path().join(".codex/hooks.json");
        std::fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
        assert!(ensure_codex_status_hooks(temp.path()), "{command}");
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual, expected, "migration discarded user hook: {command}");
        cleanup_codex_status_hooks(temp.path());
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual, expected, "cleanup discarded user hook: {command}");
    }
}

#[test]
fn inline_install_and_cleanup_preserve_unrelated_flag_mentions() {
    for command in unrelated_reporter_mentions() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".codex")).unwrap();
        let user: toml::Value =
            toml::Value::try_from(json!({"type":"command", "command":command, "timeout":7}))
                .unwrap();
        let mut root: toml::Value = toml::Value::try_from(
            json!({"hooks":{"Stop":[{"matcher":"keep-group", "hooks":[user]}]}}),
        )
        .unwrap();
        root["hooks"]["Stop"][0]["hooks"]
            .as_array_mut()
            .unwrap()
            .push(toml::Value::try_from(legacy_reporter()).unwrap());
        let path = temp.path().join(".codex/config.toml");
        std::fs::write(&path, toml::to_string(&root).unwrap()).unwrap();
        assert!(ensure_codex_status_hooks(temp.path()), "{command}");
        let installed =
            toml::from_str::<toml::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            installed["hooks"]["Stop"][0]["hooks"][0], user,
            "installation discarded user hook: {command}"
        );
        assert_eq!(
            installed["hooks"]["Stop"][0]["matcher"].as_str(),
            Some("keep-group")
        );
        cleanup_codex_status_hooks(temp.path());
        let cleaned =
            toml::from_str::<toml::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            cleaned["hooks"]["Stop"][0]["hooks"][0], user,
            "cleanup discarded user hook: {command}"
        );
        assert_eq!(cleaned["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn legacy_user_only_flag_mentions_are_byte_identical_after_install_and_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".codex")).unwrap();
    let before = serde_json::to_vec(
        &json!({"hooks":{"Stop":[{"hooks":[{"command":"echo 'spyc --report-status done'"}]}]}}),
    )
    .unwrap();
    let path = temp.path().join(".codex/hooks.json");
    std::fs::write(&path, &before).unwrap();
    assert!(ensure_codex_status_hooks(temp.path()));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    cleanup_codex_status_hooks(temp.path());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn codex_preserves_noncommand_handlers_with_reporter_shaped_text() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".codex")).unwrap();
    let user = json!({"type":"prompt", "command":"spyc --report-status done"});
    let content = json!({"hooks":{"Stop":[{"hooks":[user]}]}});
    let legacy = temp.path().join(".codex/hooks.json");
    let before = serde_json::to_vec(&content).unwrap();
    std::fs::write(&legacy, &before).unwrap();
    let canonical = temp.path().join(".codex/config.toml");
    std::fs::write(
        &canonical,
        toml::to_string(&toml::Value::try_from(&content).unwrap()).unwrap(),
    )
    .unwrap();
    assert!(ensure_codex_status_hooks(temp.path()));
    assert_eq!(std::fs::read(&legacy).unwrap(), before);
    cleanup_codex_status_hooks(temp.path());
    assert_eq!(std::fs::read(legacy).unwrap(), before);
    let cleaned: toml::Value =
        toml::from_str(&std::fs::read_to_string(canonical).unwrap()).unwrap();
    assert_eq!(
        cleaned["hooks"]["Stop"][0]["hooks"][0],
        toml::Value::try_from(user).unwrap()
    );
}

#[test]
fn renamed_current_reporter_stays_idempotent_without_changing_command_bytes() {
    let exe = "/opt/custom/reporter";
    let once = merged_codex_status_hooks_toml(None, exe, false).unwrap();
    let twice = merged_codex_status_hooks_toml(Some(&once), exe, false).unwrap();
    assert_eq!(once, twice);
    let root: toml::Value = toml::from_str(&once).unwrap();
    assert_eq!(
        root["hooks"]["Stop"][0]["hooks"][0]["command"].as_str(),
        Some(reporter_command(exe, "done", false).as_str())
    );
}

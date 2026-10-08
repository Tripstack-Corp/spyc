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

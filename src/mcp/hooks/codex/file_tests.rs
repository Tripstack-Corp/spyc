//! Filesystem regressions for legacy JSON migration and teardown.
use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn legacy_fixture() -> &'static str {
    r#"{"user-setting":"keep", "hooks":{"Stop":[{"hooks":[{"command":"printf user-hook"}]},{"hooks":[{"command":"spyc --report-status done"}]}]}}"#
}

#[test]
fn legacy_symlink_refuses_install_and_cleanup_without_changing_either_file() {
    for dangling in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir(dir.join(".codex")).unwrap();
        let target = dir.join("shared.json");
        if !dangling {
            std::fs::write(&target, legacy_fixture()).unwrap();
        }
        let link = dir.join(".codex/hooks.json");
        symlink(&target, &link).unwrap();
        assert!(
            !ensure_codex_status_hooks(dir),
            "a symlink must not be migrated"
        );
        assert!(!dir.join(".codex/config.toml").exists());
        let canonical = merged_codex_status_hooks_toml(None, "spyc", false).unwrap();
        std::fs::write(dir.join(".codex/config.toml"), &canonical).unwrap();
        assert!(matches!(
            cleanup_codex_status_hooks(dir),
            ConfigCleanup::NothingToDo
        ));
        assert_eq!(
            std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap(),
            canonical
        );
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        if dangling {
            assert!(!target.exists());
        } else {
            assert_eq!(std::fs::read_to_string(&target).unwrap(), legacy_fixture());
        }
        assert!(
            codex_legacy_hook_diagnostic(dir)
                .unwrap()
                .contains("symlink")
        );
    }
}

#[test]
fn legacy_migration_preserves_regular_file_permissions_and_user_content() {
    for mode in [0o640, 0o644] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir(dir.join(".codex")).unwrap();
        let path = dir.join(".codex/hooks.json");
        std::fs::write(&path, legacy_fixture()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(ensure_codex_status_hooks(dir));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            mode
        );
        let migrated: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(migrated["user-setting"], "keep");
        assert_eq!(
            migrated["hooks"]["Stop"][0]["hooks"][0]["command"],
            "printf user-hook"
        );
        assert!(!migrated.to_string().contains("--report-status"));
        assert!(matches!(
            cleanup_codex_status_hooks(dir),
            ConfigCleanup::Cleaned
        ));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            mode
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap(),
            migrated
        );
    }
}

#[test]
fn a_symlink_created_after_preflight_is_preserved_at_the_apply_boundary() {
    for text in [
        legacy_fixture(),
        r#"{"hooks":{"Stop":[{"hooks":[{"command":"spyc --report-status done"}]}]}}"#,
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir(dir.join(".codex")).unwrap();
        let path = dir.join(".codex/hooks.json");
        std::fs::write(&path, text).unwrap();
        let edit = legacy_edit(dir).unwrap_or_else(|_| panic!("valid legacy preflight"));
        let target = dir.join("shared.json");
        std::fs::write(&target, legacy_fixture()).unwrap();
        std::fs::remove_file(&path).unwrap();
        symlink(&target, &path).unwrap();
        assert!(apply_legacy_edit(dir, edit).is_err());
        assert_eq!(std::fs::read_link(path).unwrap(), target);
        assert_eq!(std::fs::read_to_string(target).unwrap(), legacy_fixture());
    }
}

#[test]
fn legacy_cleanup_also_keeps_regular_file_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir(dir.join(".codex")).unwrap();
    let path = dir.join(".codex/hooks.json");
    std::fs::write(&path, legacy_fixture()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(
        cleanup_codex_status_hooks(dir),
        ConfigCleanup::Cleaned
    ));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(
        !std::fs::read_to_string(path)
            .unwrap()
            .contains("--report-status")
    );
}

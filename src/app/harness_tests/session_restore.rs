//! Restore refusals are isolated to the affected saved tab.

use super::*;

fn saved_session(
    dir: &std::path::Path,
    commands: &[String],
    active_tab: usize,
) -> crate::state::sessions::Session {
    serde_json::from_value(serde_json::json!({
        "id": 991, "name": "RESTORE_ISOLATION", "saved_at": "", "epoch_secs": 0, "cwd": dir,
        "tabs": commands.iter().enumerate().map(|(index, command)| serde_json::json!({
            "command": command, "label": format!("saved-{index}"), "cwd": dir,
            "agent_kind": "codex", "agent_session_id": "019e8b21-9e7c-7553-a118-d1cdada725fd",
            "claim_owner": format!("saved-owner-{index}")
        })).collect::<Vec<_>>(),
        "active_tab": active_tab, "pane_height_pct": 75, "pane_focused": true
    }))
    .unwrap()
}

#[test]
fn mixed_restore_opens_valid_tabs_and_keeps_active_tab_identity() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let saved = saved_session(
            &dir,
            &[
                format!("{command} --future-option"),
                command.clone(),
                command,
            ],
            1,
        );
        let mut app = App::test_app(dir.clone());
        app.restore_session(&saved);
        let tabs = app
            .runtime
            .pane_tabs
            .as_ref()
            .expect("supported tabs must restore");
        assert_eq!(tabs.tabs().len(), 2);
        assert_eq!(tabs.active_index(), 0);
        assert_eq!(tabs.tabs()[0].info.label, "saved-1");
        assert_eq!(tabs.tabs()[1].info.claim_owner, "saved-owner-2");
        assert_eq!(app.state.session_id, Some(saved.id));
        assert_eq!(app.state.pane.pane_height_pct, 75);
        assert!(
            agent_argv(&dir, "codex")
                .iter()
                .all(|arg| arg != "--future-option")
        );
        assert!(app.flash_text().unwrap().contains("kept saved"));
    });
}

fn read_saved(state_root: &std::path::Path, id: u64) -> crate::state::sessions::Session {
    serde_json::from_slice(
        &std::fs::read(state_root.join("sessions").join(format!("{id}.json"))).unwrap(),
    )
    .unwrap()
}

#[test]
fn partial_restore_preserves_refused_metadata_on_save_and_second_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let original = saved_session(
            &dir,
            &[
                format!("{command} --future-option"),
                command.clone(),
                command,
            ],
            1,
        );
        let mut app = App::test_app(dir.clone());
        app.restore_session(&original);
        agent_argv(&dir, "codex");
        app.save_session();
        let first_save = read_saved(&state_root, original.id);
        assert_eq!(first_save.tabs.len(), 3);
        assert_eq!(first_save.active_tab, 1);
        assert_eq!(
            serde_json::to_value(&first_save.tabs[0]).unwrap(),
            serde_json::to_value(&original.tabs[0]).unwrap()
        );
        app.show_session_info();
        let text = app
            .view
            .pager
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();
        assert!(text.contains("unopened tabs (kept saved)"));
        assert!(text.contains("saved tab 1 (saved-0)"));
        assert!(text.contains("unsupported Codex option"));
        drop(app);
        let mut again = App::test_app(dir);
        again.restore_session(&first_save);
        let tabs = again.runtime.pane_tabs.as_ref().unwrap();
        assert_eq!(tabs.tabs().len(), 2);
        assert_eq!(tabs.active_index(), 0);
        assert_eq!(tabs.tabs()[0].info.label, "saved-1");
        again.save_session();
        let second_save = read_saved(&state_root, original.id);
        assert_eq!(second_save.active_tab, 1);
        assert_eq!(
            serde_json::to_value(&second_save.tabs[0]).unwrap(),
            serde_json::to_value(&original.tabs[0]).unwrap()
        );
        // Closing all live panes must not drop the unopened records.
        again.runtime.pane_tabs = None;
        again.save_session();
        let last_save = read_saved(&state_root, original.id);
        assert_eq!(last_save.tabs.len(), 1);
        assert_eq!(
            serde_json::to_value(&last_save.tabs[0]).unwrap(),
            serde_json::to_value(&original.tabs[0]).unwrap()
        );
    });
}

#[test]
fn a_refused_selected_tab_falls_forward_and_new_restore_clears_old_refusals() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let original = saved_session(
            &dir,
            &[
                command.clone(),
                format!("{command} --future-option"),
                command.clone(),
            ],
            1,
        );
        let mut app = App::test_app(dir.clone());
        app.restore_session(&original);
        assert_eq!(app.runtime.pane_tabs.as_ref().unwrap().active_index(), 1);
        assert_eq!(app.state.deferred_tabs.len(), 1);
        let supported = saved_session(&dir, &[command], 0);
        app.restore_session(&supported);
        assert!(app.state.deferred_tabs.is_empty());
        assert_eq!(app.runtime.pane_tabs.as_ref().unwrap().tabs().len(), 1);
        assert_eq!(app.flash_text(), Some("session restored"));
    });
}

#[test]
fn unopened_tabs_survive_debounced_autosave_without_live_panes() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let original = saved_session(&dir, &[format!("{command} --future-option"), command], 1);
        let mut app = App::test_app(dir);
        app.restore_session(&original);
        app.runtime.pane_tabs = None;
        let mut ctx = RunCtx::for_test();
        let now = std::time::Instant::now();
        app.settle_autosave(now, &mut ctx);
        app.settle_autosave(now + std::time::Duration::from_secs(3), &mut ctx);
        let saved = read_saved(&state_root, original.id);
        assert_eq!(saved.tabs.len(), 1);
        assert_eq!(
            serde_json::to_value(&saved.tabs[0]).unwrap(),
            serde_json::to_value(&original.tabs[0]).unwrap()
        );
        // A change to unopened metadata must dirty the autosave baseline too.
        app.state.deferred_tabs[0].tab.label = "renamed deferred".into();
        app.settle_autosave(now + std::time::Duration::from_secs(4), &mut ctx);
        app.settle_autosave(now + std::time::Duration::from_secs(7), &mut ctx);
        assert_eq!(
            read_saved(&state_root, original.id).tabs[0].label,
            "renamed deferred"
        );
    });
}

#[test]
fn a_refused_tab_does_not_prevent_layout_or_scope_restore() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let right = dir.join("right");
        std::fs::create_dir(&right).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let mut saved = saved_session(&dir, &[format!("{command} --future-option"), command], 1);
        saved.vsplit = Some(crate::state::sessions::SavedVsplit {
            width_pct: 65,
            full_height: true,
            focus_right: true,
            preview_path: None,
            right_cwd: Some(right.clone()),
        });
        saved
            .scope_claims
            .push(crate::state::scope_registry::ScopeClaim {
                id: 22,
                owner: "saved-owner-0".into(),
                owner_label: "unopened".into(),
                paths: vec!["src/app/session.rs".into()],
                intent: crate::state::scope_registry::ScopeIntent::Editing,
                pr: None,
                note: Some("keep orphan claim".into()),
                claimed_at_secs: 0,
            });
        let mut app = App::test_app(dir);
        app.restore_session(&saved);
        assert_eq!(app.state.right.as_ref().unwrap().listing.dir, right);
        let split = app.state.vsplit.unwrap();
        assert_eq!(split.width_pct, 65);
        assert_eq!(split.mode, super::super::state::VsplitMode::FullHeight);
        assert_eq!(split.focus, super::super::state::Side::Right);
        assert_eq!(app.state.scope_registry, saved.scope_claims);
        assert_eq!(
            app.runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .claim_owner,
            "saved-owner-1"
        );
    });
}

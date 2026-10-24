//! Image gallery and paste capture through the pane's actual App routes.
use super::*;
use crate::app::image_ops;
use crate::app::{PaneInput, PaneTarget};

const SID: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn launch(app: &mut App, cwd: &std::path::Path, home: &std::path::Path) {
    std::fs::create_dir_all(home).unwrap();
    let command = fake_agent(cwd, "codex").display().to_string();
    assert!(app.open_pane_tab_in_home(&command, cwd, Some(home)));
    agent_argv(cwd, "codex");
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .active_info_mut()
        .codex_session_id = Some(SID.into());
    app.state.active_project_mut().focus = state::Focus::Pane;
}

#[test]
fn codex_image_paste_is_captured_alongside_the_original_key() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &cwd.join("home"));
        let paste = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        let effects = app.handle_key(paste).unwrap();
        assert_eq!(effects.pane_key(), Some(paste));
        assert_eq!(effects.len(), 2);
        assert_eq!(
            effects.clipboard_capture_tab(),
            Some(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .id
                    .as_str()
            )
        );
        let alternate = KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        let effects = app.handle_key(alternate).unwrap();
        assert_eq!(effects.pane_key(), Some(alternate));
        assert!(effects.clipboard_capture_tab().is_some());
        app.state
            .active_project_mut()
            .config
            .pane
            .preview_pasted_images = false;
        let effects = app.handle_key(paste).unwrap();
        assert_eq!(effects.pane_key(), Some(paste));
        assert!(effects.clipboard_capture_tab().is_none());
        app.state
            .active_project_mut()
            .config
            .pane
            .preview_pasted_images = true;
        assert!(
            app.handle_key(key('v'))
                .unwrap()
                .clipboard_capture_tab()
                .is_none()
        );
    });
}

#[test]
fn codex_gallery_indexes_only_the_pinned_home_and_opens_its_attachment() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("correct");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        for (home, label) in [(&home, "correct"), (&cwd.join("other"), "wrong")] {
            let day = home.join("sessions/2026/10/12");
            std::fs::create_dir_all(&day).unwrap();
            let records = [
                serde_json::json!({"type":"session_meta","payload":{"id":SID,"cwd":cwd}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"one","content":[{"type":"text","text":label},{"type":"image","image_url":format!("data:image/png;base64,{PNG}")}]}}}),
            ];
            std::fs::write(
                day.join(format!("rollout-now-{SID}.jsonl")),
                records
                    .iter()
                    .map(serde_json::Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n",
            )
            .unwrap();
        }
        let effects = app.open_image_gallery();
        let op = effects
            .transcript_index()
            .expect("Codex supports received images")
            .clone();
        let outcome = image_ops::index_transcript(op);
        app.file_image_outcome(outcome);
        app.apply_image_outcomes();
        let gallery = app.view.image_gallery.as_ref().unwrap();
        assert_eq!(gallery.received.len(), 1);
        assert_eq!(gallery.received[0].prompt_excerpt, "correct");
        let bytes = crate::state::transcript_images::load(
            gallery.transcript_path.as_deref().unwrap(),
            &gallery.received[0],
        )
        .unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
    });
}

fn land_capture(app: &mut App, tab_id: &str, epoch: u64) {
    app.file_image_outcome(image_ops::ImageOutcome::Captured {
        tab_id: tab_id.into(),
        epoch,
        encoded: vec![1, 2, 3],
        dims: (1, 1),
    });
    app.apply_image_outcomes();
}

#[test]
fn a_late_clipboard_read_cannot_restore_unsent_images_after_enter_or_tab_close() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &cwd.join("home"));
        let tab_id = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        land_capture(&mut app, &tab_id, 0);
        assert_eq!(
            app.state.active_project().pane.pending_images[&tab_id].len(),
            1
        );
        app.open_image_gallery();
        assert_eq!(
            app.gallery_counts(),
            (1, 0),
            "captured image is visible before submission"
        );
        app.close_image_gallery();
        app.execute_pane_input(
            PaneTarget::Active,
            PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None,
            None,
        );
        assert!(
            !app.state
                .active_project()
                .pane
                .pending_images
                .contains_key(&tab_id)
        );
        land_capture(&mut app, &tab_id, 0);
        assert!(
            !app.state
                .active_project()
                .pane
                .pending_images
                .contains_key(&tab_id),
            "late read belongs to the submitted draft"
        );
        land_capture(&mut app, &tab_id, 1);
        assert_eq!(
            app.state.active_project().pane.pending_images[&tab_id].len(),
            1,
            "next draft may capture again"
        );
        app.close_active_tab_now();
        land_capture(&mut app, &tab_id, 1);
        assert!(
            !app.state
                .active_project()
                .pane
                .pending_images
                .contains_key(&tab_id),
            "closed pane owns no clipboard bytes"
        );
    });
}

fn write_attachment(home: &std::path::Path, cwd: &std::path::Path) {
    let day = home.join("sessions/2026/10/12");
    std::fs::create_dir_all(&day).unwrap();
    let records = [
        serde_json::json!({"type":"session_meta","payload":{"id":SID,"cwd":cwd}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"one","content":[{"type":"text","text":"captured home"},{"type":"image","image_url":format!("data:image/png;base64,{PNG}")}]}}}),
    ];
    std::fs::write(
        day.join(format!("rollout-now-{SID}.jsonl")),
        records
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
}

#[test]
fn gallery_worker_resolves_after_dispatch_using_the_captured_home() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("home");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        let effects = app.open_image_gallery();
        let op = effects
            .transcript_index()
            .expect("dispatch plans a worker without resolving the missing rollout")
            .clone();
        write_attachment(&home, &cwd);
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .codex_home = Some(cwd.join("replacement"));
        let outcome = image_ops::index_transcript(op);
        app.file_image_outcome(outcome);
        app.apply_image_outcomes();
        assert_eq!(
            app.view.image_gallery.as_ref().unwrap().received[0].prompt_excerpt,
            "captured home"
        );
    });
}

#[test]
fn gallery_worker_result_from_a_closed_popup_cannot_fill_a_reopened_one() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("home");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        write_attachment(&home, &cwd);
        let old = app.open_image_gallery().transcript_index().unwrap().clone();
        app.close_image_gallery();
        let current = app.open_image_gallery().transcript_index().unwrap().clone();
        app.file_image_outcome(image_ops::index_transcript(old));
        app.apply_image_outcomes();
        assert!(
            app.view.image_gallery.as_ref().unwrap().loading,
            "old result must not end the new request's loading state"
        );
        assert_eq!(app.gallery_counts(), (0, 0));
        app.file_image_outcome(image_ops::index_transcript(current));
        app.apply_image_outcomes();
        assert_eq!(app.gallery_counts(), (0, 1));
        assert!(!app.view.image_gallery.as_ref().unwrap().loading);
    });
}

#[test]
fn gallery_worker_miss_keeps_unsent_images_and_reports_only_for_its_live_popup() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("home");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        let tab_id = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .id
            .clone();
        land_capture(&mut app, &tab_id, 0);
        let pending = app.open_image_gallery().transcript_index().unwrap().clone();
        app.file_image_outcome(image_ops::index_transcript(pending));
        app.apply_image_outcomes();
        assert_eq!(
            app.gallery_counts(),
            (1, 0),
            "a missing rollout cannot hide unsent clipboard bytes"
        );
        assert!(!app.view.image_gallery.as_ref().unwrap().loading);
        app.close_image_gallery();
        app.execute_pane_input(
            PaneTarget::Active,
            PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None,
            None,
        );
        let missing = app.open_image_gallery().transcript_index().unwrap().clone();
        app.file_image_outcome(image_ops::index_transcript(missing.clone()));
        app.apply_image_outcomes();
        assert!(app.view.image_gallery.is_none());
        assert!(app.flash_text().unwrap().contains("no transcript"));
        app.state.flash_info("keep this status");
        app.file_image_outcome(image_ops::index_transcript(missing));
        app.apply_image_outcomes();
        assert_eq!(
            app.flash_text(),
            Some("keep this status"),
            "a retired index request cannot flash a late error"
        );
    });
}

#[test]
fn image_routing_follows_the_agent_in_a_shells_foreground() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("home");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        write_attachment(&home, &cwd);
        let info = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut();
        info.command = "zsh".into();
        info.spawn_epoch_secs = 100;
        info.running = Some(crate::pane::RunningAgent {
            kind: crate::state::sessions::AgentKind::Codex,
            pid: 12345,
            started_epoch_secs: 500,
            backgrounded: false,
            on_shared_daemon: false,
        });
        let paste = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        let effects = app.handle_key(paste).unwrap();
        assert_eq!(effects.pane_key(), Some(paste));
        assert!(
            effects.clipboard_capture_tab().is_some(),
            "the foreground Codex owns image paste even though the tab launched zsh"
        );
        let op = app
            .open_image_gallery()
            .transcript_index()
            .expect("the foreground agent supplies its gallery reader")
            .clone();
        assert_eq!(
            op.query.spawn_epoch_secs, 500,
            "lookup belongs to this agent's start, not the shell's"
        );
        app.file_image_outcome(image_ops::index_transcript(op));
        app.apply_image_outcomes();
        assert_eq!(app.gallery_counts(), (0, 1));
        app.close_image_gallery();
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .running
            .as_mut()
            .unwrap()
            .backgrounded = true;
        let effects = app.handle_key(paste).unwrap();
        assert_eq!(effects.pane_key(), Some(paste));
        assert!(
            effects.clipboard_capture_tab().is_none(),
            "the shell's quoted-insert cannot read the clipboard while Codex is backgrounded"
        );
        assert!(app.open_image_gallery().is_empty());
        assert!(app.view.image_gallery.is_none());
    });
}

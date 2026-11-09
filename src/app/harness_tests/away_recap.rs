//! #919: a tab whose agent finished a turn while it was out of view says, as it
//! comes back into view, what happened meanwhile, in a flash the next key
//! clears: `1 turn · 3 tools (Edit 2, Bash 1) · 1 file changed · done 0s ago`.

use super::claude_home::tick;
use super::*;
use crate::agent::status_hook::{StatusHookEvent, read_payload};
use crate::mcp_cmd::{McpCommand, McpResponse};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SID: &str = "c1a0de00-0000-4000-8000-0000000a3a1e";

/// A git repository in `tmp` with one commit, and a Claude Code home whose
/// conversation for it has one record from before.
fn workspace(tmp: &Path) -> (PathBuf, PathBuf) {
    let dir = std::fs::canonicalize(tmp).unwrap().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    let run_git = crate::git::test_support::run_git;
    run_git(&dir, &["init", "-q", "--initial-branch=main"]);
    std::fs::write(dir.join("f.txt"), "v1\n").unwrap();
    std::fs::write(dir.join("g.txt"), "v1\n").unwrap();
    run_git(&dir, &["add", "f.txt", "g.txt"]);
    run_git(&dir, &["commit", "-q", "-m", "v1"]);
    let home = tmp.join("claude-home");
    let project = home
        .join("projects")
        .join(crate::state::sessions::project_slug(&dir));
    std::fs::create_dir_all(&project).unwrap();
    let before = json!({"type": "assistant", "sessionId": SID, "timestamp": "2026-01-01T00:00:00Z",
        "message": {"id": "msg-0", "content": [
            {"type": "tool_use", "id": "toolu_old", "name": "Read", "input": {}}]}});
    std::fs::write(project.join(format!("{SID}.jsonl")), format!("{before}\n")).unwrap();
    (dir, home)
}

/// Two tabs in `dir`: Claude, its conversation in `home`, in view, and another
/// beside it. No notification fires: the test is about the recap. The file
/// list stays in the process's own cwd: the loop's settle moves the process to
/// the focused column, and a temporary directory gone after the test would
/// leave every later test without one.
fn two_tabs(dir: &Path, home: &Path) -> App {
    let mut app = App::test_app(std::env::current_dir().unwrap());
    app.view.term_size = (100, 32);
    let project = app.state.active_project_mut();
    project.config.mouse.capture = false;
    project.config.notify.desktop = false;
    project.config.notify.bell = false;
    project.config.notify.visual = false;
    project.pane.pane_height_pct = 50;
    for command in ["claude", "cat"] {
        let wake = app.make_pane_wake();
        let pane =
            crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.project().context_path, wake)
                .expect("spawn cat");
        let mut info = crate::pane::tabs::TabInfo::new(command, dir.to_path_buf());
        info.label = command.into();
        if command == "claude" {
            info.claude_home = Some(home.to_owned());
            info.live_session_id = Some(SID.into());
        }
        let entry = crate::pane::tabs::TabEntry::new(pane, info);
        let tabs = &mut app.runtime.project_mut().pane_tabs;
        match tabs.as_mut() {
            Some(tabs) => tabs.push(entry),
            None => *tabs = Some(crate::pane::tabs::PaneTabs::new(entry)),
        }
    }
    // A pushed tab becomes the active one: bring Claude's back.
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .switch_to(0);
    app
}

/// What spyc's status hook sends Claude's tab.
fn hook(app: &mut App, status: &str, payload: &serde_json::Value) {
    let kept = read_payload(payload.to_string().as_bytes()).expect("a hook payload");
    let response = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane: Some(1),
        status: status.into(),
        ttl_ms: None,
        note: None,
        session_id: Some(SID.into()),
        claude_home: None,
        claude_credential: None,
        hook_event: StatusHookEvent::from_payload(&kept),
    });
    assert!(matches!(response, McpResponse::Ok { .. }), "{response:?}");
}

/// One turn of Claude's, start to finish, as its hooks report it.
fn turn(app: &mut App, ctx: &mut RunCtx) {
    hook(
        app,
        "working",
        &json!({"hook_event_name": "UserPromptSubmit"}),
    );
    tick(app, ctx);
    hook(app, "done", &json!({"hook_event_name": "Stop"}));
    tick(app, ctx);
}

/// Claude's calls, written to its conversation now.
fn calls(home: &Path, dir: &Path, tools: &[(&str, &str)]) {
    let path = home
        .join("projects")
        .join(crate::state::sessions::project_slug(dir))
        .join(format!("{SID}.jsonl"));
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    for (id, tool) in tools {
        let record = json!({"type": "assistant", "sessionId": SID,
            "timestamp": jiff::Timestamp::now().to_string(),
            "message": {"id": format!("msg-{id}"), "content": [
                {"type": "tool_use", "id": id, "name": tool,
                 "input": {"file_path": "private-path", "command": "private-command"}}]}});
        std::io::Write::write_all(&mut file, format!("{record}\n").as_bytes()).unwrap();
    }
}

/// Whether Claude's tab has a finished turn not yet seen.
fn unseen_done(app: &App) -> bool {
    app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
        .info
        .unseen_done
        .is_some()
}

fn show_tab(app: &mut App, ctx: &mut RunCtx, index: usize) {
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .switch_to(index);
    tick(app, ctx);
}

/// The flash once the recap lands, or the latest one after a few seconds.
fn recap(app: &mut App, ctx: &mut RunCtx) -> Option<String> {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        tick(app, ctx);
        let flash = app.flash_text().map(String::from);
        if flash.as_deref().is_some_and(|f| f.contains(" turn")) || Instant::now() > end {
            return flash;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The reported case: Claude finished a turn while its tab was out of view;
/// back in view, the tab says how many turns, which tools, how many files
/// changed and how long ago it finished. Never what the tools acted on.
#[test]
fn coming_back_to_a_tab_that_finished_out_of_view_recaps_it() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (dir, home) = workspace(tmp.path());
        let mut app = two_tabs(&dir, &home);
        let mut ctx = RunCtx::for_test();
        tick(&mut app, &mut ctx);
        assert_eq!(
            app.in_view_tab(),
            Some(0),
            "premise: Claude's tab is in view"
        );
        // Changed before the tab was left: not the agent's doing meanwhile.
        std::fs::write(dir.join("g.txt"), "v2\n").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        show_tab(&mut app, &mut ctx, 1);
        std::thread::sleep(Duration::from_millis(20));

        calls(
            &home,
            &dir,
            &[("e1", "Edit"), ("e2", "Edit"), ("b1", "Bash")],
        );
        std::fs::write(dir.join("f.txt"), "v2\n").unwrap();
        turn(&mut app, &mut ctx);
        assert!(unseen_done(&app), "premise: it finished out of view");
        show_tab(&mut app, &mut ctx, 0);

        let recap = recap(&mut app, &mut ctx).unwrap_or_default();
        assert!(
            recap.starts_with("1 turn · 3 tools (Edit 2, Bash 1) · 1 file changed · done "),
            "{recap}"
        );
        assert!(recap.ends_with(" ago"), "{recap}");
        assert!(
            !recap.contains("private") && !recap.contains("Read"),
            "{recap}"
        );
    });
}

/// Each turn finished out of view counts.
#[test]
fn every_turn_finished_out_of_view_counts() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (dir, home) = workspace(tmp.path());
        let mut app = two_tabs(&dir, &home);
        let mut ctx = RunCtx::for_test();
        tick(&mut app, &mut ctx);
        assert_eq!(
            app.in_view_tab(),
            Some(0),
            "premise: Claude's tab is in view"
        );
        show_tab(&mut app, &mut ctx, 1);
        turn(&mut app, &mut ctx);
        turn(&mut app, &mut ctx);
        assert!(unseen_done(&app), "premise: it finished out of view");
        show_tab(&mut app, &mut ctx, 0);
        let recap = recap(&mut app, &mut ctx).unwrap_or_default();
        assert!(recap.starts_with("2 turns · "), "{recap}");
    });
}

/// A turn that finished in view was seen: leaving and coming back recaps
/// nothing.
#[test]
fn a_finish_seen_in_view_has_no_recap() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (dir, home) = workspace(tmp.path());
        let mut app = two_tabs(&dir, &home);
        let mut ctx = RunCtx::for_test();
        tick(&mut app, &mut ctx);
        assert_eq!(
            app.in_view_tab(),
            Some(0),
            "premise: Claude's tab is in view"
        );
        turn(&mut app, &mut ctx);
        show_tab(&mut app, &mut ctx, 1);
        show_tab(&mut app, &mut ctx, 0);
        let recap = recap(&mut app, &mut ctx);
        assert!(
            !recap.as_deref().is_some_and(|f| f.contains(" turn")),
            "{recap:?}"
        );
    });
}

/// A second absence recaps its own turns, not the first one's again.
#[test]
fn each_absence_recaps_its_own_turns() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (dir, home) = workspace(tmp.path());
        let mut app = two_tabs(&dir, &home);
        let mut ctx = RunCtx::for_test();
        tick(&mut app, &mut ctx);
        for _ in 0..2 {
            show_tab(&mut app, &mut ctx, 1);
            turn(&mut app, &mut ctx);
            show_tab(&mut app, &mut ctx, 0);
            let recap = recap(&mut app, &mut ctx).unwrap_or_default();
            assert!(recap.starts_with("1 turn · "), "{recap}");
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap();
            assert!(
                app.flash_text().is_none(),
                "the next key clears it: {:?}",
                app.flash_text()
            );
        }
    });
}

/// A recap that lands once the tab has gone out of view again is dropped:
/// the status bar would be speaking of a tab no longer shown.
#[test]
fn a_recap_for_a_tab_left_again_is_dropped() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (dir, home) = workspace(tmp.path());
        let mut app = two_tabs(&dir, &home);
        let mut ctx = RunCtx::for_test();
        tick(&mut app, &mut ctx);
        show_tab(&mut app, &mut ctx, 1);
        turn(&mut app, &mut ctx);
        assert!(unseen_done(&app), "premise: it finished out of view");
        show_tab(&mut app, &mut ctx, 0);
        show_tab(&mut app, &mut ctx, 1);
        let recap = recap(&mut app, &mut ctx);
        assert!(
            !recap.as_deref().is_some_and(|f| f.contains(" turn")),
            "{recap:?}"
        );
    });
}

//! #917: a Claude tab reads the Claude Code home it runs with, its
//! `CLAUDE_CONFIG_DIR`, for its HUD, `^a v`, and the session save and restore.
//! The status hook's report wins, the pane's launch record names it until then,
//! and spyc's own home is the default. Every tab here is started by a shell
//! that sets `CLAUDE_CONFIG_DIR`, so none inherits the developer's.

use super::*;
use crate::agent::tail::Availability;
use crate::app::state::agent_hud::Binding;
use crate::mcp::test_support::Conn;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

/// The conversation the tab runs, filed under the home its hook reports.
const SID: &str = "c1a0de00-0000-4000-8000-0000000000b0";
/// Another conversation in the same directory, under the default home.
const DECOY: &str = "c1a0de00-0000-4000-8000-00000000dec0";
/// The conversation the launch record's home holds.
const LAUNCHED: &str = "c1a0de00-0000-4000-8000-0000000000a0";

/// A conversation `sid` in `home` for `cwd` that says `said`, with one usage
/// record for the HUD. With `running`, also Claude's record of the process
/// running it, started now, which is what `^a v` and the save match a tab to
/// before its hook names the conversation.
fn conversation(home: &Path, cwd: &Path, sid: &str, said: &str, running: bool) {
    let project = home
        .join("projects")
        .join(crate::state::sessions::project_slug(cwd));
    std::fs::create_dir_all(&project).unwrap();
    let records = [
        json!({"type": "user", "sessionId": sid, "message": {"content": said}}),
        json!({"type": "assistant", "sessionId": sid, "timestamp": "2026-11-04T12:00:00Z",
               "message": {"id": "msg-1", "model": "claude-opus-5-5", "content": [],
                           "usage": {"input_tokens": 1, "output_tokens": 5}}}),
    ];
    let body = records.map(|r| r.to_string()).join("\n") + "\n";
    std::fs::write(project.join(format!("{sid}.jsonl")), body).unwrap();
    if running {
        let sessions = home.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let started = crate::sysinfo::epoch_secs() * 1000;
        let record = json!({"pid": 4242, "sessionId": sid, "cwd": cwd, "startedAt": started});
        std::fs::write(sessions.join(format!("{sid}.json")), record.to_string()).unwrap();
    }
}

/// Run `body` with every pane shell exporting `CLAUDE_CONFIG_DIR=home` after its
/// rc, as a profile's rc would; an empty `home` names none.
fn with_rc_home<T>(dir: &Path, home: &str, body: impl FnOnce() -> T) -> T {
    use std::os::unix::fs::PermissionsExt;
    let shell = dir.join("rc-shell");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nexport CLAUDE_CONFIG_DIR={}\nexec /bin/sh \"$@\"\n",
            crate::shell::shell_quote(home)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    crate::shell::with_test_shell(shell.to_str().unwrap(), body)
}

/// A test app in `cwd` whose own Claude home, spyc's `CLAUDE_CONFIG_DIR` or
/// `~/.claude`, is `default`, with `^a v` reading Claude's transcript.
fn app_with_default(cwd: &Path, default: &Path) -> App {
    let mut app = App::test_app(cwd.to_path_buf());
    let project = app.state.active_project_mut();
    project.config.pane.claude_transcript_scrollback = true;
    project.pane.agent_hud.claude_home = Some(default.to_path_buf());
    app
}

/// A Claude tab in `dir`, once its agent has started, so its launch record is
/// written. Returns its pane id.
fn claude_tab(app: &mut App, dir: &Path) -> String {
    let command = fake_agent(dir, "claude").display().to_string();
    assert!(app.open_pane_tab_in(&command, dir));
    agent_argv(dir, "claude");
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .unwrap()
        .active_info()
        .id
        .clone()
}

/// A status hook's `report_status`, sent over the socket protocol as the
/// reporter sends it and run by the loop's own drain.
fn hook_reports(app: &mut App, args: serde_json::Value) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut conn = Conn::open(app.view.project().context_path.clone(), tx);
    let mut ctx = RunCtx::for_test();
    std::thread::scope(|s| {
        let client = s.spawn(move || conn.tool("report_status", args));
        while !client.is_finished() {
            if let Ok(req) = rx.recv_timeout(Duration::from_millis(5)) {
                ctx.mcp_pending.push(req);
                app.drain_mcp_pending(&mut ctx);
            }
        }
        let reply = client.join().expect("the hook's call returns");
        assert!(reply.to_string().contains("set for pane"), "{reply}");
    });
}

/// What `^a v` shows for the active tab: the transcript it mounts, else the
/// flash that says it found none.
fn scrollback(app: &mut App) -> String {
    app.close_pane_scroll_pager();
    app.apply(&Action::PaneScrollEnter).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        app.drain_pager_stream();
        match app.view.project().scroll_pager.as_ref() {
            Some(pager) if !pager.streaming => {
                return pager
                    .lines
                    .iter()
                    .flat_map(|line| line.spans.iter())
                    .map(|span| span.content.as_ref())
                    .collect();
            }
            None => return app.flash_text().unwrap_or_default().to_string(),
            Some(_) => {}
        }
        assert!(Instant::now() < end, "the transcript never landed");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The tab a save of `app` records, as the session file has it.
fn saved_tab(app: &mut App) -> serde_json::Value {
    app.save_session();
    let manifest = crate::state::sessions::load_manifests()
        .into_iter()
        .next()
        .expect("the session is saved");
    serde_json::to_value(&manifest.projects[0].tabs[0]).unwrap()
}

/// One pass of the loop's drain and settle, as `App::run` makes before it waits.
pub(super) fn tick(app: &mut App, ctx: &mut RunCtx) {
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 100, 32)),
        },
    )
    .unwrap();
    let fg = crate::app::ForegroundExec {
        park: std::sync::Arc::default(),
        acked: std::sync::Arc::default(),
        reader_done: std::sync::Arc::default(),
        reader: std::thread::current(),
    };
    app.drain_landed_work(ctx, &mut terminal, &fg);
    app.settle_before_recv(ctx, &mut terminal, &fg);
}

/// Settle the loop until the agent HUD reads the reported session's transcript
/// under `home`.
fn wait_hud_reads(app: &mut App, home: &Path) {
    let mut ctx = RunCtx::for_test();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        tick(app, &mut ctx);
        let bound = match &app.state.active_project().pane.agent_hud.binding {
            Some(Binding::Claude(b)) => Some(b.claude_home.clone()),
            _ => None,
        };
        let read = app
            .active_agent_hud_update()
            .map(crate::app::state::agent_hud::Update::availability);
        if bound.as_deref() == Some(home) && read == Some(Availability::Available) {
            return;
        }
        assert!(
            Instant::now() < end,
            "the HUD bound {bound:?} ({read:?}), not {}",
            home.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The issue's case: spyc names no `CLAUDE_CONFIG_DIR`, its own home holds a
/// decoy conversation in the tab's directory, and the tab's hook reports that
/// Claude runs from profile `b`. The HUD and `^a v` both read `b`'s.
#[test]
fn a_claude_tab_reads_the_home_its_hook_reports_not_the_default() {
    // The loop's settle sets the process-wide mouse mode.
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let (default, profile_b) = (dir.join("default-claude"), dir.join("profile-b"));
        conversation(&default, &dir, DECOY, "the decoy in the default home", true);
        conversation(&profile_b, &dir, SID, "profile b's conversation", false);
        with_rc_home(&dir, "", || {
            // The loop's settle moves the process into the focused column, so
            // the app stays where the test process already is.
            let mut app = app_with_default(&std::env::current_dir().unwrap(), &default);
            app.view.term_size = (100, 32);
            app.state.active_project_mut().config.mouse.capture = false;
            app.state.active_project_mut().pane.pane_height_pct = 50;
            let pane = claude_tab(&mut app, &dir);
            app.cmd_agent_hud("full");
            hook_reports(
                &mut app,
                json!({"status": "working", "pane_id": pane, "session_id": SID,
                       "claude_home": profile_b}),
            );
            let text = scrollback(&mut app);
            assert!(text.contains("profile b's conversation"), "{text}");
            assert!(!text.contains("decoy"), "{text}");
            wait_hud_reads(&mut app, &profile_b);
        });
    });
}

/// Before its first hook report a tab reads the home its launch record names,
/// as a profile's rc sets it, never the default's decoy; a relative home the
/// hook reports names no place and changes nothing, and an absolute one wins.
#[test]
fn a_claude_tab_reads_its_launch_home_until_its_hook_reports_one() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let default = dir.join("default-claude");
        let (launched, profile_b) = (dir.join("launched"), dir.join("profile-b"));
        conversation(&default, &dir, DECOY, "the decoy in the default home", true);
        conversation(&launched, &dir, LAUNCHED, "launched conversation", true);
        conversation(&profile_b, &dir, SID, "profile b's conversation", false);
        with_rc_home(&dir, launched.to_str().unwrap(), || {
            let mut app = app_with_default(&dir, &default);
            let pane = claude_tab(&mut app, &dir);
            let text = scrollback(&mut app);
            assert!(text.contains("launched conversation"), "{text}");
            assert!(!text.contains("decoy"), "{text}");

            hook_reports(
                &mut app,
                json!({"status": "working", "pane_id": pane, "session_id": SID,
                       "claude_home": "profile-b"}),
            );
            let text = scrollback(&mut app);
            assert!(text.contains("launched conversation"), "{text}");

            hook_reports(
                &mut app,
                json!({"status": "working", "pane_id": pane, "session_id": SID,
                       "claude_home": profile_b}),
            );
            let text = scrollback(&mut app);
            assert!(text.contains("profile b's conversation"), "{text}");
        });
    });
}

/// spyc started with `CLAUDE_CONFIG_DIR` reads that home for a tab that names
/// none of its own: its `^a v`, and the conversation a save records.
#[test]
fn a_claude_tab_that_names_no_home_reads_spycs_own() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let own = dir.join("spyc-claude-config");
        conversation(&own, &dir, SID, "spyc's own home's conversation", true);
        with_rc_home(&dir, "", || {
            let mut app = app_with_default(&dir, &own);
            claude_tab(&mut app, &dir);
            let text = scrollback(&mut app);
            assert!(text.contains("spyc's own home's conversation"), "{text}");
            assert_eq!(saved_tab(&mut app)["agent_session_id"], SID);
        });
    });
}

/// A save records the home a tab's hook reported, and `-r` resumes the
/// conversation from it and reads it for `^a v`, though spyc's own home holds
/// no such conversation.
#[test]
fn a_restored_claude_tab_resumes_from_the_home_it_was_saved_with() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let (default, profile_b) = (dir.join("default-claude"), dir.join("profile-b"));
        std::fs::create_dir_all(&default).unwrap();
        conversation(&profile_b, &dir, SID, "profile b's conversation", false);
        with_rc_home(&dir, "", || {
            let mut app = app_with_default(&dir, &default);
            let pane = claude_tab(&mut app, &dir);
            hook_reports(
                &mut app,
                json!({"status": "working", "pane_id": pane, "session_id": SID,
                       "claude_home": profile_b}),
            );
            let saved = saved_tab(&mut app);
            assert_eq!(saved["agent_session_id"], SID);
            assert_eq!(saved["claude_home"], json!(profile_b), "{saved}");

            // The session's own cwd is the crate root, where the process runs:
            // a restore moves the process there.
            let session = parsed(json!({
                "id": 917, "saved_at": "", "epoch_secs": 0, "cwd": env!("CARGO_MANIFEST_DIR"),
                "tabs": [saved], "active_tab": 0, "pane_height_pct": 50, "pane_focused": true
            }));
            let mut restored = app_with_default(&dir, &default);
            restored.restore_session(&session);
            let info = restored
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .expect("the tab restores")
                .active_info();
            assert!(
                matches!(
                    &info.pending_resume_send,
                    Some(crate::pane::tabs::PendingResumeSend::Text { sid, .. }) if sid == SID
                ),
                "the restore types no `/resume {SID}`"
            );
            let text = scrollback(&mut restored);
            assert!(text.contains("profile b's conversation"), "{text}");
        });
    });
}

/// `^a R` and `^a F` run the tab's command again, so its claude files under the
/// same home: the new tab reads it before its own first report, never the
/// default's decoy.
#[test]
fn a_restarted_or_forked_claude_tab_keeps_its_reported_home() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let (default, profile_b) = (dir.join("default-claude"), dir.join("profile-b"));
        conversation(&default, &dir, DECOY, "the decoy in the default home", true);
        conversation(&profile_b, &dir, SID, "profile b's conversation", true);
        with_rc_home(&dir, "", || {
            let mut app = app_with_default(&dir, &default);
            let pane = claude_tab(&mut app, &dir);
            hook_reports(
                &mut app,
                json!({"status": "working", "pane_id": pane, "session_id": SID,
                       "claude_home": profile_b}),
            );

            std::fs::remove_file(dir.join("claude.argv")).unwrap();
            app.restart_active_tab_now();
            agent_argv(&dir, "claude");
            let text = scrollback(&mut app);
            assert!(text.contains("profile b's conversation"), "^a R: {text}");

            // A report that names no home pins the session `^a F` branches.
            let restarted = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .id
                .clone();
            hook_reports(
                &mut app,
                json!({"status": "working", "pane_id": restarted, "session_id": SID}),
            );
            std::fs::remove_file(dir.join("claude.argv")).unwrap();
            app.fork_active_tab();
            agent_argv(&dir, "claude");
            let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
            assert_eq!((tabs.tabs().len(), tabs.active_index()), (2, 1));
            let text = scrollback(&mut app);
            assert!(text.contains("profile b's conversation"), "^a F: {text}");
        });
    });
}

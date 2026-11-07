//! #918: a blocked agent says what it asks for. The ping reads "claude needs
//! you: Bash", the HUD's activity row ends in the label, and `Space !` flashes
//! "tab 2: Bash" where it lands. The label comes from the hook's metadata only:
//! the tool's name, `question` or `plan` for the tools that ask, `mcp` for
//! another server's tool, or why a failed turn stopped. Never tool input.

use super::agent_readiness::{agent_app, dump, why_status};
use super::project_tabs::agent_tab_in;
use super::*;
use crate::agent::codex_recovery::QUESTION_START;
use crate::agent::status_hook::{StatusHookEvent, read_payload};
use crate::app::effect::{PaneInput, PaneTarget};
use crate::config::AgentHudMode;
use crate::mcp_cmd::{McpCommand, McpResponse};
use ratatui::layout::Rect;
use serde_json::json;
use std::time::{Duration, Instant};

/// An agent tab named for `name`, as spyc labels one it launched.
fn agent(dir: &std::path::Path, name: &str) -> App {
    let mut app = agent_app(dir, name);
    let tabs = app.runtime.project_mut().pane_tabs.as_mut().unwrap();
    tabs.active_info_mut().label = name.into();
    app
}

/// What spyc's status hook sends tab `pane` (1-based, else the active one):
/// the session id, and the metadata the reporter keeps of the agent's whole
/// payload, with the state it resolved.
fn hook_to(app: &mut App, pane: Option<usize>, status: &str, payload: &serde_json::Value) {
    let kept = read_payload(payload.to_string().as_bytes()).expect("a hook payload");
    let response = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane,
        status: status.into(),
        ttl_ms: None,
        note: None,
        session_id: Some("session-1".into()),
        claude_home: None,
        hook_event: StatusHookEvent::from_payload(&kept),
    });
    assert!(matches!(response, McpResponse::Ok { .. }), "{response:?}");
}

fn hook(app: &mut App, status: &str, payload: &serde_json::Value) {
    hook_to(app, None, status, payload);
}

fn prompt() -> serde_json::Value {
    json!({"hook_event_name":"UserPromptSubmit", "prompt":"private-prompt"})
}

/// Claude asking to use `tool`, with the input it would run.
fn permission(tool: &str) -> serde_json::Value {
    json!({
        "hook_event_name":"PermissionRequest", "tool_name":tool,
        "tool_input":{"command":"private-command", "description":"private-description"}
    })
}

/// Settle the dots, and the summary of the desktop ping that fired, if any.
fn ping(app: &mut App) -> Option<String> {
    let (_, effects) = app.settle_agent_activity(Instant::now(), &mut RunCtx::for_test());
    summary(&effects)
}

/// The summary of the desktop ping among `effects`, if one fired.
fn summary(effects: &[Effect]) -> Option<String> {
    effects.iter().find_map(|fx| match fx {
        Effect::Notify {
            system: Some((summary, _)),
            ..
        } => Some(summary.clone()),
        Effect::Notify {
            osc9: Some(line), ..
        } => line.split(" — ").next().map(String::from),
        _ => None,
    })
}

/// The HUD's activity row, drawn 160 columns wide.
fn hud_row(app: &mut App) -> String {
    app.view.term_size = (160, 40);
    let project = app.state.active_project_mut();
    project.config.pane.agent_hud = AgentHudMode::Compact;
    project.pane.pane_height_pct = 50;
    let rect = app
        .frame_layout(Rect::new(0, 0, 160, 40))
        .agent_hud
        .expect("the strip is drawn");
    app.agent_hud_lines(rect)[0].to_string()
}

/// A permission request names its tool in the ping, and at the end of the
/// HUD's activity row.
#[test]
fn a_bash_permission_request_says_claude_needs_you_bash() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        hook(&mut app, "working", &prompt());
        assert_eq!(ping(&mut app), None);
        hook(&mut app, "blocked", &permission("Bash"));
        assert_eq!(ping(&mut app).as_deref(), Some("claude needs you: Bash"));
        let row = hud_row(&mut app);
        assert!(row.contains("needs you"), "{row}");
        assert!(row.trim_end().ends_with("Bash"), "{row}");
    });
}

/// The tools that put a question to the user read `question`, in every
/// agent, and Claude's plan approval reads `plan`.
#[test]
fn the_tools_that_ask_read_question_and_plan() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let asking = |tool: &str| {
            json!({
                "hook_event_name":"PreToolUse", "tool_name":tool,
                "turn_id":"turn-1", "tool_use_id":"call-1",
                "tool_input":{"questions":["private-question"], "plan":"private-plan"}
            })
        };
        for (name, state, tool, label) in [
            ("claude", "blocked", "AskUserQuestion", "question"),
            ("claude", "blocked", "ExitPlanMode", "plan"),
            ("codex", QUESTION_START, "request_user_input", "question"),
            ("agy", "blocked", "ask_question", "question"),
        ] {
            let mut app = agent(tmp.path(), name);
            hook(&mut app, "working", &prompt());
            ping(&mut app);
            hook(&mut app, state, &asking(tool));
            assert_eq!(
                ping(&mut app),
                Some(format!("{name} needs you: {label}")),
                "{name} {tool}"
            );
        }
    });
}

/// Another MCP server's tool reads `mcp`: a server's name can be private.
/// spyc's own tools keep theirs, as in the HUD's tools row.
#[test]
fn another_servers_tool_reads_mcp() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (tool, label) in [
            ("mcp__private__tool", "mcp"),
            ("mcp__spyc__navigate_to", "mcp__spyc__navigate_to"),
        ] {
            let mut app = agent(tmp.path(), "claude");
            hook(&mut app, "working", &prompt());
            ping(&mut app);
            hook(&mut app, "blocked", &permission(tool));
            assert_eq!(
                ping(&mut app),
                Some(format!("claude needs you: {label}")),
                "{tool}"
            );
            let row = hud_row(&mut app);
            assert!(row.trim_end().ends_with(label), "{row}");
            assert!(!row.contains("private"), "{row}");
        }
    });
}

/// Claude's `permission_prompt` notification names no tool. It follows the
/// request it is about, so the tab keeps that request's label, and pings
/// no second time.
#[test]
fn a_permission_prompt_keeps_the_label_of_the_request_before_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        hook(&mut app, "working", &prompt());
        ping(&mut app);
        hook(&mut app, "blocked", &permission("Edit"));
        assert_eq!(ping(&mut app).as_deref(), Some("claude needs you: Edit"));
        let prompt_shown = json!({
            "hook_event_name":"Notification", "notification_type":"permission_prompt",
            "message":"Claude needs your permission to use Edit"
        });
        hook(&mut app, "blocked", &prompt_shown);
        assert_eq!(ping(&mut app), None, "the same wait");
        let row = hud_row(&mut app);
        assert!(row.trim_end().ends_with("Edit"), "{row}");
    });
}

/// The label goes with the `blocked` that carried it: once the user answers,
/// in the pane or in the agent's own prompt, a later block says only what it
/// names itself.
#[test]
fn the_label_goes_with_the_blocked_that_carried_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        hook(&mut app, "working", &prompt());
        ping(&mut app);
        hook(&mut app, "blocked", &permission("Bash"));
        assert_eq!(ping(&mut app).as_deref(), Some("claude needs you: Bash"));
        app.execute_pane_input(
            PaneTarget::Active,
            PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None,
            None,
        );
        ping(&mut app);
        let row = hud_row(&mut app);
        assert!(!row.contains("Bash"), "answered in the pane: {row}");

        hook(&mut app, "working", &prompt());
        ping(&mut app);
        hook(&mut app, "blocked", &permission("Bash"));
        ping(&mut app);
        // Answered where spyc sees no keystroke: the hooks carry the turn on.
        hook(&mut app, "done", &json!({"hook_event_name":"Stop"}));
        hook(&mut app, "working", &prompt());
        ping(&mut app);
        let unnamed =
            json!({"hook_event_name":"Notification", "notification_type":"permission_prompt"});
        hook(&mut app, "blocked", &unnamed);
        assert_eq!(ping(&mut app).as_deref(), Some("claude needs you"));
        let row = hud_row(&mut app);
        assert!(!row.contains("Bash"), "a new wait: {row}");
    });
}

/// A failed turn shows its failure where a permission shows its tool.
#[test]
fn a_failed_turn_says_why_where_a_permission_names_its_tool() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        hook(&mut app, "working", &prompt());
        ping(&mut app);
        let failed = json!({
            "hook_event_name":"StopFailure", "error":"rate_limit",
            "error_details":"private-details", "last_assistant_message":"private-reply"
        });
        hook(&mut app, "blocked", &failed);
        assert_eq!(
            ping(&mut app).as_deref(),
            Some("claude needs you: stopped: rate limit")
        );
        let row = hud_row(&mut app);
        assert!(row.trim_end().ends_with("stopped: rate limit"), "{row}");
    });
}

/// `Space !` says where it landed and what the agent there asks for.
#[test]
fn space_bang_says_which_tab_and_what_it_asks_for() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        let here = app.state.active;
        agent_tab_in(&mut app, here, tmp.path(), "claude");
        let tabs = app.runtime.project_mut().pane_tabs.as_mut().unwrap();
        tabs.switch_to(0);
        hook_to(&mut app, Some(2), "blocked", &permission("Bash"));
        ping(&mut app);
        app.state.active_project_mut().focus = state::Focus::FileList;
        app.handle_key(key(' ')).unwrap();
        app.handle_key(key('!')).unwrap();
        let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
        assert_eq!(tabs.active_index(), 1, "it lands on the blocked tab");
        assert_eq!(app.flash_text(), Some("tab 2: Bash"));
    });
}

/// A `waiting` agent's note still ends the HUD's activity row, where a blocked
/// one's label goes.
#[test]
fn a_waiting_agents_note_still_ends_the_hud_row() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        hook(&mut app, "working", &prompt());
        ping(&mut app);
        let stop = json!({
            "hook_event_name":"Stop", "session_crons":[],
            "background_tasks":[{"id":"t1", "command":"private-command"}]
        });
        hook(&mut app, "waiting", &stop);
        ping(&mut app);
        let row = hud_row(&mut app);
        assert!(row.contains("waiting"), "{row}");
        assert!(row.trim_end().ends_with("1 background task"), "{row}");
    });
}

/// The agent's own words on a `blocked` it reports stay out of the desktop
/// ping, which can leave the machine: only hook metadata names the wait.
#[test]
fn the_agents_own_note_stays_out_of_the_ping() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        let response = app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: "blocked".into(),
            ttl_ms: None,
            note: Some("private-note".into()),
            session_id: None,
            claude_home: None,
            hook_event: None,
        });
        assert!(matches!(response, McpResponse::Ok { .. }), "{response:?}");
        assert_eq!(ping(&mut app).as_deref(), Some("claude needs you"));
    });
}

/// No tool input, prompt or reply enters spyc's state through a blocked
/// report: not the tab's reports, its hook events, the HUD, the ping or the
/// diagnostics.
#[test]
fn no_tool_input_enters_spycs_state() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent(tmp.path(), "claude");
        hook(&mut app, "working", &prompt());
        ping(&mut app);
        hook(&mut app, "blocked", &permission("Bash"));
        let summary = ping(&mut app).unwrap_or_default();
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        let kept = format!(
            "{:?} {:?} {:?}",
            info.reported, info.last_reported, info.recent_hook_events
        );
        let seen = [
            kept,
            summary,
            hud_row(&mut app),
            why_status(&mut app),
            dump(&mut app),
        ];
        for text in seen {
            assert!(!text.contains("private"), "{text}");
        }
    });
}

// ── a wait spyc reads off the screen ──

/// A tab running `name` whose screen shows `fixture`, a wait the agent draws
/// with no hook of its own, behind the `working` its last prompt reported.
fn showing(dir: &std::path::Path, name: &str, fixture: &str) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    super::agent_approval::draw_fixture(&mut app, dir, name, fixture);
    let info = app
        .runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .active_info_mut();
    info.label = name.into();
    if name == "codex" {
        info.codex_home = Some(dir.join("codex-home"));
    }
    hook(&mut app, "working", &prompt());
    ping(&mut app);
    app
}

/// The screen read once its output has gone quiet, then the dots: the summary
/// of the ping that fired, if any.
fn ping_after_reading_the_screen(app: &mut App) -> Option<String> {
    let quiet_since = Instant::now();
    {
        let info = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut();
        info.last_output_at = Some(quiet_since);
        info.scrape_dirty = true;
    }
    let at = quiet_since + Duration::from_secs(1);
    app.settle_scrape_quiet(at, &mut RunCtx::for_test());
    let (_, effects) = app.settle_agent_activity(at, &mut RunCtx::for_test());
    summary(&effects)
}

/// A wait spyc reads off codex's screen names the form it shows, as a hook's
/// names its tool: in the ping and at the end of the HUD's activity row.
#[test]
fn a_wait_read_off_codexs_screen_names_its_form() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (fixture, label) in [
            (
                include_str!("../../../tests/fixtures/codex-approval-exec.txt"),
                "command",
            ),
            (
                include_str!("../../../tests/fixtures/codex-approval-edit.txt"),
                "file edit",
            ),
            (
                include_str!("../../../tests/fixtures/codex-approval-mcp.txt"),
                "mcp",
            ),
            (
                include_str!("../../../tests/fixtures/codex-approval-network.txt"),
                "network",
            ),
            (
                include_str!("../../../tests/fixtures/codex-questions/expanded-options.txt"),
                "question",
            ),
        ] {
            let mut app = showing(tmp.path(), "codex", fixture);
            assert_eq!(
                ping_after_reading_the_screen(&mut app),
                Some(format!("codex needs you: {label}")),
                "{label}"
            );
            let row = hud_row(&mut app);
            assert!(row.trim_end().ends_with(label), "{label}: {row}");
        }
    });
}

/// agy's dialogs, the same: each names what its question asks to allow.
#[test]
fn a_wait_read_off_agys_screen_names_its_form() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (fixture, label) in [
            (
                include_str!("../../../tests/fixtures/agy-output/approval-command.txt"),
                "command",
            ),
            (
                include_str!("../../../tests/fixtures/agy-output/approval-command-editor.txt"),
                "command",
            ),
            (
                include_str!("../../../tests/fixtures/agy-output/approval-edit.txt"),
                "file edit",
            ),
            (
                include_str!("../../../tests/fixtures/agy-output/approval-file.txt"),
                "file access",
            ),
            (
                include_str!("../../../tests/fixtures/agy-output/approval-file-access.txt"),
                "file access",
            ),
            (
                include_str!("../../../tests/fixtures/agy-output/approval-mcp.txt"),
                "mcp",
            ),
        ] {
            let mut app = showing(tmp.path(), "agy", fixture);
            assert_eq!(
                ping_after_reading_the_screen(&mut app),
                Some(format!("agy needs you: {label}")),
                "{label}"
            );
            let row = hud_row(&mut app);
            assert!(row.trim_end().ends_with(label), "{label}: {row}");
        }
    });
}

/// `Space !` names a wait read off the screen where it lands, as it names a
/// hook's.
#[test]
fn space_bang_names_a_wait_read_off_the_screen() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let fixture = include_str!("../../../tests/fixtures/codex-approval-exec.txt");
        let mut app = showing(tmp.path(), "codex", fixture);
        ping_after_reading_the_screen(&mut app);
        app.state.active_project_mut().focus = state::Focus::FileList;
        app.handle_key(key(' ')).unwrap();
        app.handle_key(key('!')).unwrap();
        assert_eq!(app.flash_text(), Some("tab 1: command"));
    });
}

/// The label goes with the form that showed it: answered in the pane, a later
/// block the agent reports itself names nothing.
#[test]
fn a_forms_label_goes_once_it_is_answered() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let fixture = include_str!("../../../tests/fixtures/agy-output/approval-command.txt");
        let mut app = showing(tmp.path(), "agy", fixture);
        assert_eq!(
            ping_after_reading_the_screen(&mut app).as_deref(),
            Some("agy needs you: command")
        );
        app.execute_pane_input(
            PaneTarget::Active,
            PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None,
            None,
        );
        ping(&mut app);
        let response = app.execute_mcp_command(McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: "blocked".into(),
            ttl_ms: None,
            note: None,
            session_id: None,
            claude_home: None,
            hook_event: None,
        });
        assert!(matches!(response, McpResponse::Ok { .. }), "{response:?}");
        assert_eq!(ping(&mut app).as_deref(), Some("agy needs you"));
    });
}

// ── `[notify] blocked_detail`: what the tool acts on ──

/// Turn on `[notify] blocked_detail`, as a `.spycrc.toml` does.
fn with_detail(app: &mut App, dir: &std::path::Path) {
    let rc = dir.join("detail.spycrc.toml");
    std::fs::write(&rc, "[notify]\nblocked_detail = true\n").unwrap();
    app.state.active_project_mut().config = crate::config::Config::load_from(&[Some(&rc)]).unwrap();
}

/// Claude asking to use `tool` with `input`.
fn asking_with(tool: &str, input: serde_json::Value) -> serde_json::Value {
    json!({"hook_event_name":"PermissionRequest", "tool_name":tool, "tool_input":input})
}

/// A Claude tab, its option on, that has started a turn.
fn detailed_claude(dir: &std::path::Path) -> App {
    let mut app = agent(dir, "claude");
    with_detail(&mut app, dir);
    hook(&mut app, "working", &prompt());
    ping(&mut app);
    app
}

/// With the option on, a `Bash` request says what it runs in the words the
/// model gave it, never the command: in the ping and at the end of the HUD's
/// activity row.
#[test]
#[ignore = "red: no [notify] blocked_detail"]
fn with_detail_on_a_bash_request_says_what_it_runs() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = detailed_claude(tmp.path());
        let input = json!({"command":"private-command", "description":"Run the test suite"});
        hook(&mut app, "blocked", &asking_with("Bash", input));
        let pinged = ping(&mut app).unwrap_or_default();
        assert_eq!(pinged, "claude needs you: Bash · Run the test suite");
        let row = hud_row(&mut app);
        assert!(
            row.trim_end().ends_with("Bash · Run the test suite"),
            "{row}"
        );
        assert!(!row.contains("private"), "{row}");
    });
}

/// An edit or a write names the file it is for, by its name alone.
#[test]
#[ignore = "red: no [notify] blocked_detail"]
fn with_detail_on_an_edit_names_its_file() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for tool in ["Edit", "Write"] {
            let mut app = detailed_claude(tmp.path());
            let input = json!({
                "file_path":"/work/private-dir/src/main.rs",
                "old_string":"private-old", "new_string":"private-new", "content":"private-content"
            });
            hook(&mut app, "blocked", &asking_with(tool, input));
            assert_eq!(
                ping(&mut app).as_deref(),
                Some(format!("claude needs you: {tool} · main.rs").as_str())
            );
        }
    });
}

/// A detail is one clean line: its first, without control characters, cut
/// short when long.
#[test]
#[ignore = "red: no [notify] blocked_detail"]
fn a_detail_is_one_clean_line() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = detailed_claude(tmp.path());
        let input = json!({"description":"Run \u{1b}[31mthe tests\u{7}\nand then private-more"});
        hook(&mut app, "blocked", &asking_with("Bash", input));
        let pinged = ping(&mut app).unwrap_or_default();
        assert_eq!(pinged, "claude needs you: Bash · Run [31mthe tests");

        let mut app = detailed_claude(tmp.path());
        let long = "word ".repeat(60);
        hook(
            &mut app,
            "blocked",
            &asking_with("Bash", json!({"description": long})),
        );
        let pinged = ping(&mut app).unwrap_or_default();
        let detail = pinged
            .strip_prefix("claude needs you: Bash · ")
            .unwrap_or_else(|| panic!("{pinged}"));
        assert!(detail.ends_with('…'), "{detail}");
        assert!(detail.chars().count() < long.len() / 2, "{detail}");
    });
}

/// `Space !` says it where it lands.
#[test]
#[ignore = "red: no [notify] blocked_detail"]
fn with_detail_space_bang_says_what_the_tool_acts_on() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = detailed_claude(tmp.path());
        let input = json!({"description":"Run the test suite"});
        hook(&mut app, "blocked", &asking_with("Bash", input));
        ping(&mut app);
        app.state.active_project_mut().focus = state::Focus::FileList;
        app.handle_key(key(' ')).unwrap();
        app.handle_key(key('!')).unwrap();
        assert_eq!(app.flash_text(), Some("tab 1: Bash · Run the test suite"));
    });
}

/// Claude's `permission_prompt` notification names nothing, so it keeps the
/// detail of the request before it, as it keeps that request's label.
#[test]
#[ignore = "red: no [notify] blocked_detail"]
fn a_permission_prompt_keeps_the_detail_of_the_request_before_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = detailed_claude(tmp.path());
        let input = json!({"file_path":"/work/src/main.rs"});
        hook(&mut app, "blocked", &asking_with("Edit", input));
        ping(&mut app);
        let prompt_shown =
            json!({"hook_event_name":"Notification", "notification_type":"permission_prompt"});
        hook(&mut app, "blocked", &prompt_shown);
        let row = hud_row(&mut app);
        assert!(row.trim_end().ends_with("Edit · main.rs"), "{row}");
    });
}

/// Even with the option on, the detail is held for the ping, the HUD and
/// `Space !` alone: never in the diagnostics, the tab's hook history or a
/// debug print of its reports.
#[test]
#[ignore = "red: no [notify] blocked_detail"]
fn with_detail_on_it_stays_out_of_the_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = detailed_claude(tmp.path());
        let input = json!({"description":"private-description"});
        hook(&mut app, "blocked", &asking_with("Bash", input));
        let pinged = ping(&mut app).unwrap_or_default();
        assert!(pinged.ends_with("private-description"), "premise: {pinged}");
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        let kept = format!(
            "{:?} {:?} {:?}",
            info.reported, info.last_reported, info.recent_hook_events
        );
        for text in [kept, why_status(&mut app), dump(&mut app)] {
            assert!(!text.contains("private"), "{text}");
        }
    });
}

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
use std::time::Instant;

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

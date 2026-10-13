//! `waiting`: an agent that ended its turn with background work still running
//! shows `○` until that work wakes it, without the `done` ping in between.

use super::agent_readiness::{agent_app, dump, why_status};
use super::*;
use crate::agent::status_hook::StatusHookEvent;
use crate::app::effect::{PaneInput, PaneTarget};
use crate::mcp_cmd::{McpCommand, McpResponse};
use crate::pane::{AgentActivity, ReportedStatus};
use serde_json::json;
use std::time::{Duration, Instant};

/// What spyc's status hook sends: the agent's session id and the event's
/// metadata, with the state the reporter already resolved.
fn hook(app: &mut App, status: &str, event: &serde_json::Value) -> McpResponse {
    app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane: None,
        status: status.into(),
        ttl_ms: None,
        note: None,
        session_id: Some("session-1".into()),
        hook_event: StatusHookEvent::from_value(event),
    })
}

/// The agent's own `report_status` call.
fn agent(app: &mut App, status: &str, note: Option<&str>, ttl_ms: Option<u64>) {
    let response = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane: None,
        status: status.into(),
        ttl_ms,
        note: note.map(String::from),
        session_id: None,
        hook_event: None,
    });
    assert!(matches!(response, McpResponse::Ok { .. }), "{response:?}");
}

fn reported(app: &App) -> Option<ReportedStatus> {
    app.runtime
        .pane_tabs
        .as_ref()
        .unwrap()
        .active_info()
        .reported
        .clone()
}

/// Settle the dot at `at`: its state, and whether a notification fired.
fn settle(app: &mut App, at: Instant) -> (AgentActivity, bool) {
    let (_, effects) = app.settle_agent_activity(at, &mut RunCtx::for_test());
    let activity = app
        .runtime
        .pane_tabs
        .as_ref()
        .unwrap()
        .active_info()
        .activity;
    let pinged = effects.iter().any(|fx| matches!(fx, Effect::Notify { .. }));
    (activity, pinged)
}

/// Claude Code's `Stop` lists the work still running; the reporter turns that
/// turn end into `waiting`. The dot holds through the idle nudge, follows the
/// turn the finished work starts, and pings only when the agent is done.
#[test]
fn a_turn_ending_with_background_work_waits_without_a_ping_until_the_work_wakes_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        hook(
            &mut app,
            "working",
            &json!({"hook_event_name":"UserPromptSubmit"}),
        );
        assert_eq!(settle(&mut app, Instant::now()).0, AgentActivity::Working);

        hook(
            &mut app,
            "waiting",
            &json!({"hook_event_name":"Stop", "background_tasks":1, "session_crons":0}),
        );
        assert_eq!(
            settle(&mut app, Instant::now()),
            (AgentActivity::Waiting, false),
            "entering waiting is not a notification"
        );
        assert_eq!(
            reported(&app).unwrap().note.as_deref(),
            Some("1 background task")
        );

        let nudge = hook(
            &mut app,
            "done",
            &json!({"hook_event_name":"Notification", "notification_type":"idle_prompt"}),
        );
        assert!(
            matches!(&nudge, McpResponse::Ok { message } if message.contains("not applied")),
            "{nudge:?}"
        );
        assert_eq!(
            settle(&mut app, Instant::now()),
            (AgentActivity::Waiting, false)
        );

        hook(
            &mut app,
            "working",
            &json!({"hook_event_name":"UserPromptSubmit", "source":"system"}),
        );
        assert_eq!(settle(&mut app, Instant::now()).0, AgentActivity::Working);
        hook(
            &mut app,
            "done",
            &json!({"hook_event_name":"Stop", "background_tasks":0, "session_crons":0}),
        );
        assert_eq!(
            settle(&mut app, Instant::now()),
            (AgentActivity::Done, true),
            "the finished work's turn end pings as done"
        );
    });
}

/// Codex's `Stop` can't say whether work is pending, and agy's names no event
/// at all, so a self-reported `waiting` absorbs the turn end that follows it.
/// A second one means the agent woke and finished without reporting again.
#[test]
fn a_self_reported_wait_outlasts_its_own_turn_end_but_not_the_next() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (name, stop) in [
            (
                "codex",
                json!({"hook_event_name":"Stop", "turn_id":"turn-1"}),
            ),
            ("agy", json!({})),
        ] {
            let mut app = agent_app(tmp.path(), name);
            agent(&mut app, "waiting", Some("CI on #641"), None);
            assert_eq!(
                settle(&mut app, Instant::now()),
                (AgentActivity::Waiting, false),
                "{name}"
            );

            let first = hook(&mut app, "done", &stop);
            assert!(
                matches!(&first, McpResponse::Ok { message }
                    if message.contains("ends the turn that reported waiting")),
                "{name}: {first:?}"
            );
            assert_eq!(
                settle(&mut app, Instant::now()),
                (AgentActivity::Waiting, false),
                "{name}"
            );
            assert_eq!(
                reported(&app).unwrap().note.as_deref(),
                Some("CI on #641"),
                "{name}"
            );

            hook(&mut app, "done", &stop);
            assert_eq!(
                settle(&mut app, Instant::now()),
                (AgentActivity::Done, true),
                "{name}"
            );
        }
    });
}

/// A CI run outlasts the five-minute cap on other reports, so `waiting` holds
/// for an hour; past that the agent has missed a re-report, and timing takes
/// over. A larger request is clamped to the hour, and the others keep theirs.
#[test]
fn waiting_holds_for_an_hour() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        let lifetime = |app: &App| {
            let r = reported(app).unwrap();
            r.expiry.duration_since(r.at)
        };
        agent(&mut app, "working", None, None);
        assert_eq!(lifetime(&app), Duration::from_secs(300));
        agent(&mut app, "waiting", None, Some(36_000_000));
        assert_eq!(lifetime(&app), Duration::from_secs(3600));
        agent(&mut app, "waiting", None, None);
        assert_eq!(lifetime(&app), Duration::from_secs(3600));

        let at = reported(&app).unwrap().at;
        assert_eq!(
            settle(&mut app, at + Duration::from_secs(59 * 60)).0,
            AgentActivity::Waiting
        );
        assert_eq!(
            settle(&mut app, at + Duration::from_secs(61 * 60)).0,
            AgentActivity::Idle
        );
    });
}

/// Enter starts the user's own turn, which takes over from the wait. Esc and
/// `^c` interrupt the agent, which stops nothing running in the background.
#[test]
fn a_new_prompt_retires_waiting_and_an_interrupt_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        agent(&mut app, "waiting", None, None);
        for key in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            app.execute_pane_input(PaneTarget::Active, PaneInput::Key(key), None, None);
            assert_eq!(
                reported(&app).map(|r| r.status),
                Some(AgentActivity::Waiting),
                "{key:?}"
            );
        }
        app.execute_pane_input(
            PaneTarget::Active,
            PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None,
            None,
        );
        assert!(reported(&app).is_none());
    });
}

#[test]
fn diagnostics_say_what_a_waiting_agent_waits_on() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        agent(&mut app, "waiting", Some("CI on #641"), None);
        settle(&mut app, Instant::now());
        let why = why_status(&mut app);
        assert!(
            why.contains("waiting (self-reported) — CI on #641"),
            "{why}"
        );
        let dump = dump(&mut app);
        assert!(
            dump.contains("source: SELF-REPORT status=waiting"),
            "{dump}"
        );
        assert!(dump.contains("    note: CI on #641"), "{dump}");
    });
}

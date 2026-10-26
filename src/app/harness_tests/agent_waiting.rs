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
        .project()
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
        .project()
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

/// agy's `Stop` reports `fullyIdle: false` while background tasks, subagents or
/// scheduled timers are in flight; the reporter turns that `done` into
/// `waiting`, and the next invocation's `Stop` with `fullyIdle: true` finishes
/// the turn as `done`.
#[test]
fn an_agy_turn_ending_with_background_work_waits_until_the_next_invocation_finishes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut agy = agent_app(tmp.path(), "agy");
        hook(
            &mut agy,
            "working",
            &json!({"hook_event_name":"PreInvocation"}),
        );
        assert_eq!(settle(&mut agy, Instant::now()).0, AgentActivity::Working);
        hook(
            &mut agy,
            "waiting",
            &json!({
                "hook_event_name":"Stop",
                "termination_reason":"NO_TOOL_CALL",
                "fully_idle":false
            }),
        );
        assert_eq!(
            settle(&mut agy, Instant::now()),
            (AgentActivity::Waiting, false),
            "agy: entering waiting is not a notification"
        );
        assert_eq!(
            reported(&agy).unwrap().note.as_deref(),
            Some("background work"),
            "agy"
        );
        hook(
            &mut agy,
            "working",
            &json!({"hook_event_name":"PreInvocation"}),
        );
        assert_eq!(settle(&mut agy, Instant::now()).0, AgentActivity::Working);
        hook(
            &mut agy,
            "done",
            &json!({
                "hook_event_name":"Stop",
                "termination_reason":"NO_TOOL_CALL",
                "fully_idle":true
            }),
        );
        assert_eq!(
            settle(&mut agy, Instant::now()),
            (AgentActivity::Done, true),
            "agy: the finished work's turn end pings as done"
        );
        let dump = dump(&mut agy);
        assert!(
            dump.contains("event=PreInvocation status=working"),
            "{dump}"
        );
        assert!(dump.contains("event=Stop status=waiting"), "{dump}");
        assert!(dump.contains("event=Stop status=done"), "{dump}");
    });
}

/// Codex's `Stop` can't say whether work is pending, and neither could an older
/// agy `Stop` that omitted `fullyIdle`, so a self-reported `waiting` absorbs
/// the turn end that follows it. A second one means the agent woke and finished
/// without reporting again.
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

/// `waiting` is how a turn ends. Reported mid-turn (an agent starting a
/// background watch and carrying on), the agent is still working: the dot
/// says so until the turn ends, and the wait shows from then, in the agent's
/// words. Claude Code's and agy's `Stop` name the pending work; codex's can't.
#[test]
fn a_wait_reported_mid_turn_shows_working_until_the_turn_ends() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (name, turn_start, turn_end_state, turn_end) in [
            (
                "claude",
                "UserPromptSubmit",
                "waiting",
                json!({"hook_event_name":"Stop", "background_tasks":1, "session_crons":0}),
            ),
            (
                "agy",
                "PreInvocation",
                "waiting",
                json!({"hook_event_name":"Stop", "termination_reason":"NO_TOOL_CALL", "fully_idle":false}),
            ),
            (
                "codex",
                "UserPromptSubmit",
                "done",
                json!({"hook_event_name":"Stop", "turn_id":"turn-1"}),
            ),
        ] {
            let mut app = agent_app(tmp.path(), name);
            hook(&mut app, "working", &json!({"hook_event_name":turn_start}));
            agent(&mut app, "waiting", Some("release run"), None);
            assert_eq!(
                settle(&mut app, Instant::now()),
                (AgentActivity::Working, false),
                "{name}: the turn is still going"
            );

            hook(&mut app, turn_end_state, &turn_end);
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .activity,
                AgentActivity::Waiting,
                "{name}: shown as the turn ends, not at the next settle"
            );
            assert_eq!(
                settle(&mut app, Instant::now()),
                (AgentActivity::Waiting, false),
                "{name}: the turn ended waiting"
            );
            assert_eq!(
                reported(&app).unwrap().note.as_deref(),
                Some("release run"),
                "{name}"
            );
        }
    });
}

/// A held wait is the agent's word about how its turn will end, and a later
/// word beats it: a turn end that says nothing is pending, the agent's own
/// newer report, or a new prompt from the user.
#[test]
fn a_held_wait_gives_way_to_anything_newer() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let open = |app: &mut App, turn_start: &str| {
            hook(app, "working", &json!({"hook_event_name":turn_start}));
            agent(app, "waiting", Some("release run"), None);
        };
        for (name, turn_start, stop_nothing_pending) in [
            (
                "claude",
                "UserPromptSubmit",
                json!({"hook_event_name":"Stop", "background_tasks":0, "session_crons":0}),
            ),
            (
                "agy",
                "PreInvocation",
                json!({"hook_event_name":"Stop", "termination_reason":"NO_TOOL_CALL", "fully_idle":true}),
            ),
        ] {
            let mut app = agent_app(tmp.path(), name);
            open(&mut app, turn_start);
            hook(&mut app, "done", &stop_nothing_pending);
            assert_eq!(
                settle(&mut app, Instant::now()),
                (AgentActivity::Done, true),
                "{name}: Stop says nothing will wake it"
            );
        }

        let mut app = agent_app(tmp.path(), "codex");
        open(&mut app, "UserPromptSubmit");
        agent(&mut app, "working", None, None);
        hook(
            &mut app,
            "done",
            &json!({"hook_event_name":"Stop", "turn_id":"turn-1"}),
        );
        assert_eq!(
            settle(&mut app, Instant::now()),
            (AgentActivity::Done, true),
            "the agent took its wait back before the turn ended"
        );

        let mut app = agent_app(tmp.path(), "codex");
        open(&mut app, "UserPromptSubmit");
        app.execute_pane_input(
            PaneTarget::Active,
            PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None,
            None,
        );
        hook(
            &mut app,
            "done",
            &json!({"hook_event_name":"Stop", "turn_id":"turn-1"}),
        );
        assert_eq!(
            settle(&mut app, Instant::now()).0,
            AgentActivity::Done,
            "a new prompt takes over from the wait"
        );
    });
}

/// The user cancels a turn: Esc in the pane, then the agent's cancel hook.
fn cancel(app: &mut App, status: &str, event: &serde_json::Value) -> McpResponse {
    app.execute_pane_input(
        PaneTarget::Active,
        PaneInput::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        None,
        None,
    );
    hook(app, status, event)
}

/// A cancel ends the turn. Cancelling stops nothing running in the
/// background, and Codex's `Interrupt` can't say whether anything is, so the
/// wait the agent held shows from the cancel, in its words and without a
/// ping, as it does at Codex's `Stop`.
#[test]
#[ignore = "red: Codex's Interrupt leaves the turn open, so the wait stays held and the dot shows idle"]
fn a_codex_cancel_ends_the_turn_on_the_wait_it_held() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        hook(
            &mut app,
            "working",
            &json!({"hook_event_name":"UserPromptSubmit", "turn_id":"turn-1"}),
        );
        agent(&mut app, "waiting", Some("release run"), None);

        let interrupt = cancel(
            &mut app,
            "idle",
            &json!({"hook_event_name":"Interrupt", "turn_id":"turn-1"}),
        );
        assert!(
            matches!(&interrupt, McpResponse::Ok { message }
                if message.contains("ends the turn that reported waiting")),
            "{interrupt:?}"
        );
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Waiting,
            "shown at the cancel, not at the next settle"
        );
        assert_eq!(
            settle(&mut app, Instant::now()),
            (AgentActivity::Waiting, false)
        );
        assert_eq!(reported(&app).unwrap().note.as_deref(), Some("release run"));
        let why = why_status(&mut app);
        assert!(!why.contains("shown once this turn ends"), "{why}");
    });
}

/// agy's cancelled `Stop` says whether work is still pending, and that
/// decides the wait the agent held: with nothing pending the cancel shows
/// `idle`, and with background work in flight the reporter makes it
/// `waiting`, in the agent's words.
#[test]
fn an_agy_cancel_shows_what_its_stop_says_is_pending() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (fully_idle, state, shown, note) in [
            (true, "idle", AgentActivity::Idle, None),
            (
                false,
                "waiting",
                AgentActivity::Waiting,
                Some("release run"),
            ),
        ] {
            let mut app = agent_app(tmp.path(), "agy");
            hook(
                &mut app,
                "working",
                &json!({"hook_event_name":"PreInvocation"}),
            );
            agent(&mut app, "waiting", Some("release run"), None);
            cancel(
                &mut app,
                state,
                &json!({
                    "hook_event_name":"Stop",
                    "termination_reason":"USER_CANCELED",
                    "fully_idle":fully_idle
                }),
            );
            assert_eq!(
                settle(&mut app, Instant::now()),
                (shown, false),
                "fullyIdle: {fully_idle}"
            );
            assert_eq!(
                reported(&app).and_then(|r| r.note),
                note.map(String::from),
                "fullyIdle: {fully_idle}"
            );
        }
    });
}

/// Whatever a cancel makes of the wait the agent held, the wait belongs to
/// the cancelled turn: a later turn, however it started, ends the way its
/// own end says.
#[test]
#[ignore = "red: a cancel leaves the turn open, so the held wait takes over at a later turn's end"]
fn a_wait_held_by_a_cancelled_turn_never_outlives_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (name, turn_start, interrupt, end_state, end, shown, note) in [
            (
                "codex",
                json!({"hook_event_name":"UserPromptSubmit"}),
                json!({"hook_event_name":"Interrupt", "turn_id":"turn-1"}),
                "done",
                json!({"hook_event_name":"Stop", "turn_id":"turn-2"}),
                (AgentActivity::Done, true),
                None,
            ),
            (
                "agy",
                json!({"hook_event_name":"PreInvocation"}),
                json!({"hook_event_name":"Stop", "termination_reason":"USER_CANCELED", "fully_idle":true}),
                "waiting",
                json!({"hook_event_name":"Stop", "termination_reason":"NO_TOOL_CALL", "fully_idle":false}),
                (AgentActivity::Waiting, false),
                Some("background work"),
            ),
        ] {
            let mut app = agent_app(tmp.path(), name);
            hook(&mut app, "working", &turn_start);
            agent(&mut app, "waiting", Some("release run"), None);
            cancel(&mut app, "idle", &interrupt);
            settle(&mut app, Instant::now());

            hook(&mut app, "working", &turn_start);
            assert_eq!(
                settle(&mut app, Instant::now()).0,
                AgentActivity::Working,
                "{name}"
            );
            hook(&mut app, end_state, &end);
            assert_eq!(settle(&mut app, Instant::now()), shown, "{name}");
            assert_eq!(
                reported(&app).and_then(|r| r.note),
                note.map(String::from),
                "{name}: the later turn's own note"
            );
        }
    });
}

/// A wait held for its turn's end is visible in the diagnostics, so a working
/// dot doesn't hide that the agent said it will wait.
#[test]
fn diagnostics_name_a_wait_held_for_its_turn_end() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "claude");
        hook(
            &mut app,
            "working",
            &json!({"hook_event_name":"UserPromptSubmit"}),
        );
        agent(&mut app, "waiting", Some("release run"), None);
        settle(&mut app, Instant::now());
        let why = why_status(&mut app);
        assert!(
            why.contains("not applied: held until this turn ends"),
            "{why}"
        );
        assert!(why.contains("release run"), "{why}");
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

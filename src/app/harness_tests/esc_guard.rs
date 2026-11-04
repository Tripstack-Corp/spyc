//! The `Esc` guard (#925). With Claude Code's `/vim` on, an `Esc` in NORMAL
//! mode interrupts the running turn, so a bare `Esc` to a working Claude tab
//! whose prompt isn't in INSERT mode is dropped and flashed, while `Shift-Esc`
//! and `^a Esc` send a plain `Esc`. Each case presses its keys through
//! `handle_key`, with the mode landed as `spyc --statusline` forwards it.

use super::agent_readiness::agent_app;
use super::*;
use crate::agent::claude_statusline::{Statusline, project_bytes};
use crate::app::effect::PaneTarget;
use crate::mcp_cmd::McpCommand;
use crate::pane::AgentActivity;
use serde_json::{Value, json};

fn esc() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

fn shift_esc() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::SHIFT)
}

fn ctrl_a() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)
}

fn active(app: &App) -> &crate::pane::tabs::TabInfo {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .expect("a pane tab")
        .active_info()
}

/// A tab running `agent` with the keyboard, its `status` reported and
/// settled. `cat` is no agent: it takes no report.
fn agent_tab(dir: &std::path::Path, agent: &str, status: &str) -> App {
    let mut app = agent_app(dir, agent);
    app.set_pane_focus(true);
    app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane: None,
        status: status.into(),
        ttl_ms: Some(120_000),
        note: Some("a build".into()),
        session_id: None,
        hook_event: None,
    });
    app.settle_agent_activity(std::time::Instant::now(), &mut RunCtx::for_test());
    let expected = if agent == "cat" {
        AgentActivity::Unknown
    } else {
        AgentActivity::reportable(status).expect("a reportable status")
    };
    assert_eq!(active(&app).activity, expected, "premise: {agent} {status}");
    app
}

/// A Claude tab with the keyboard, its `status` reported and settled.
fn claude(dir: &std::path::Path, status: &str) -> App {
    agent_tab(dir, "claude", status)
}

/// Claude Code's statusline `input` on the active tab, as `spyc --statusline`
/// forwards it: projected, across the socket, and landed.
fn statusline(app: &mut App, input: &Value) {
    let projected = project_bytes(input.to_string().as_bytes());
    let wire = serde_json::to_value(&projected).expect("the projection serializes");
    let received: Statusline = serde_json::from_value(wire).expect("the server reads it");
    let pane_id = active(app).id.clone();
    app.execute_mcp_command(McpCommand::Statusline {
        pane_id,
        statusline: received.bounded(),
    });
}

/// A statusline input with vim mode on, in `mode`.
fn vim(mode: &str) -> Value {
    json!({"context_window": {"context_window_size": 200_000}, "vim": {"mode": mode}})
}

/// A statusline input with vim mode off.
fn no_vim() -> Value {
    json!({"context_window": {"context_window_size": 200_000}})
}

fn blocked_flash(mode: &str, through: &str) -> String {
    format!("`Esc` blocked: claude is working in vim {mode} mode. `{through}` interrupts it.")
}

/// A working Claude tab outside INSERT mode: a bare `Esc` would interrupt the
/// turn, so nothing reaches the pane, and the flash names the key that does.
#[test]
#[ignore = "red: no guard drops the `Esc`"]
fn a_bare_esc_to_a_working_claude_outside_insert_mode_is_dropped_and_flashed() {
    for mode in ["NORMAL", "VISUAL", "VISUAL LINE"] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = claude(tmp.path(), "working");
            statusline(&mut app, &vim(mode));
            let fx = app.handle_key(esc()).unwrap();
            assert_eq!(fx.pane_bytes(), None, "{mode}: {fx:?}");
            assert_eq!(
                app.flash_text(),
                Some(blocked_flash(mode, "^a Esc").as_str()),
                "{mode}"
            );
        });
    }
}

/// The guard fails open: in INSERT mode, with vim off or in a mode spyc
/// doesn't know, in any state but `working`, and in any other agent's tab, a
/// bare `Esc` goes through as typed.
#[test]
fn esc_goes_through_wherever_it_cannot_interrupt_a_vim_normal_turn() {
    let cases = [
        ("claude", "working", Some("INSERT")),
        ("claude", "working", None),
        ("claude", "working", Some("REPLACE")),
        ("claude", "idle", Some("NORMAL")),
        ("claude", "done", Some("NORMAL")),
        ("claude", "blocked", Some("NORMAL")),
        ("claude", "waiting", Some("NORMAL")),
        ("codex", "working", Some("NORMAL")),
        ("cat", "working", Some("NORMAL")),
    ];
    for case @ (agent, status, mode) in cases {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = agent_tab(tmp.path(), agent, status);
            if let Some(mode) = mode {
                statusline(&mut app, &vim(mode));
            }
            let fx = app.handle_key(esc()).unwrap();
            assert_eq!(fx.pane_bytes(), Some(vec![0x1b]), "{case:?}");
            assert_eq!(app.flash_text(), None, "{case:?}");
        });
    }
}

/// The mode is the newest report's: one with vim on sets it, and one without
/// clears it.
#[test]
#[ignore = "red: the statusline's vim mode is not kept"]
fn the_newest_statusline_report_decides_the_mode() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = claude(tmp.path(), "working");
        for (input, passes) in [
            (vim("NORMAL"), false),
            (vim("INSERT"), true),
            (vim("NORMAL"), false),
            (no_vim(), true),
        ] {
            statusline(&mut app, &input);
            let fx = app.handle_key(esc()).unwrap();
            assert_eq!(fx.pane_bytes().is_some(), passes, "after {input}");
        }
    });
}

/// `[pane] claude_esc_guard = false` turns the guard off.
#[test]
#[ignore = "red: `[pane] claude_esc_guard` is not a key"]
fn claude_esc_guard_false_lets_esc_through() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let rc = tmp.path().join("spycrc.toml");
        for (on, passes) in [(false, true), (true, false)] {
            std::fs::write(&rc, format!("[pane]\nclaude_esc_guard = {on}\n")).unwrap();
            let mut app = claude(tmp.path(), "working");
            app.state.active_project_mut().config =
                crate::config::Config::load_from(&[Some(&rc)]).unwrap();
            statusline(&mut app, &vim("NORMAL"));
            let fx = app.handle_key(esc()).unwrap();
            assert_eq!(fx.pane_bytes().is_some(), passes, "claude_esc_guard = {on}");
        }
    });
}

/// Deliver `fx`'s pane input the way the executor does.
fn execute(app: &mut App, fx: Vec<Effect>) {
    for effect in fx {
        if let Effect::SendToPane {
            target: PaneTarget::Active,
            input,
            ..
        } = effect
        {
            app.execute_pane_input(PaneTarget::Active, input, None, None);
        }
    }
}

/// `Shift-Esc` gets past the guard as a plain `Esc`, and spyc counts it as the
/// interrupt it is: the tab's `working` report is retired.
#[test]
#[ignore = "red: `Shift-Esc` goes out as a modified key, not an interrupt"]
fn shift_esc_sends_a_plain_esc_that_interrupts_the_turn() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = claude(tmp.path(), "working");
        statusline(&mut app, &vim("NORMAL"));
        let fx = app.handle_key(shift_esc()).unwrap();
        assert_eq!(fx.pane_bytes(), Some(vec![0x1b]), "{fx:?}");
        assert_eq!(app.flash_text(), None);
        execute(&mut app, fx);
        assert!(
            active(&app).reported.is_none(),
            "the interrupt retires the working report"
        );
    });
}

/// `^a Esc` sends a plain `Esc` on every terminal and to every tab: past the
/// guard to a working Claude in NORMAL mode, and to a shell.
#[test]
#[ignore = "red: `^a Esc` is unbound"]
fn ctrl_a_esc_sends_a_plain_esc_to_any_tab() {
    for agent in ["claude", "cat"] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = agent_tab(tmp.path(), agent, "working");
            statusline(&mut app, &vim("NORMAL"));
            assert!(app.handle_key(ctrl_a()).unwrap().is_empty(), "the prefix");
            let fx = app.handle_key(esc()).unwrap();
            assert_eq!(fx.pane_bytes(), Some(vec![0x1b]), "{agent}: {fx:?}");
            assert_eq!(app.flash_text(), None, "{agent}");
            execute(&mut app, fx);
            assert!(
                active(&app).reported.is_none(),
                "{agent}: the interrupt retires any working report"
            );
        });
    }
}

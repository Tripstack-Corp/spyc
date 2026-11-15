//! A program reports its own status (OSC 7501, #927), end to end through a real
//! pane: a shell tab running a script that says it is blocked shows the red
//! square and pings with what it waits on, a reported error shows the `✕` and
//! pings, and the user's Enter acknowledges both. Nothing in these tabs is an
//! agent spyc knows, so before #927 they had no dot at all.

use super::*;
use crate::app::effect::{PaneInput, PaneTarget};
use crate::pane::AgentActivity;
use std::time::{Duration, Instant};

/// A shell tab running `report`, a `printf` format, then sleeping.
fn reporting_tab(dir: &std::path::Path, report: &str) -> App {
    let script = dir.join("report.sh");
    std::fs::write(&script, format!("printf '{report}'\nexec sleep 30\n")).unwrap();
    let mut app = App::test_app(dir.to_path_buf());
    assert!(app.open_pane_tab_in(&format!("sh {}", script.display()), dir));
    app
}

fn activity(app: &App) -> AgentActivity {
    app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
        .info
        .activity
}

/// Drain and settle until the tab shows `want`, returning every desktop ping's
/// summary on the way.
fn settle_until(app: &mut App, want: AgentActivity) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut pings = Vec::new();
    loop {
        app.drain_pane_output();
        let (_, effects) = app.settle_agent_activity(Instant::now(), &mut RunCtx::for_test());
        pings.extend(effects.iter().filter_map(|fx| match fx {
            Effect::Notify {
                system: Some((summary, _)),
                ..
            } => Some(summary.clone()),
            Effect::Notify {
                osc9: Some(line), ..
            } => line.split(" — ").next().map(String::from),
            _ => None,
        }));
        if activity(app) == want {
            return pings;
        }
        assert!(
            Instant::now() < deadline,
            "never {want:?}: {:?}",
            activity(app)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn enter(app: &mut App) {
    app.execute_pane_input(
        PaneTarget::Active,
        PaneInput::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        None,
        None,
    );
}

#[test]
fn a_shell_tab_shows_a_blocked_report_and_pings_until_answered() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = reporting_tab(
            &dir,
            r"\033]7501;state=blocked:kind=permission:app=deploy\033\\",
        );
        let pings = settle_until(&mut app, AgentActivity::Blocked);
        assert_eq!(pings, ["sh needs you: permission"]);

        enter(&mut app);
        let pings = settle_until(&mut app, AgentActivity::Unknown);
        assert!(pings.is_empty(), "{pings:?}");
    });
}

#[test]
fn a_shell_tab_shows_a_reported_error_and_pings() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = reporting_tab(&dir, r"\033]7501;state=error:app=build\033\\");
        let pings = settle_until(&mut app, AgentActivity::Error);
        assert_eq!(pings, ["sh failed"]);

        enter(&mut app);
        settle_until(&mut app, AgentActivity::Unknown);
    });
}

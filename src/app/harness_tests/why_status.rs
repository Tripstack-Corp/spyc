//! `:why-status` in the pager (#545): the active tab's dot explained where it
//! can be read, searched, copied and saved, rather than in the status line.

use super::agent_readiness::{agent_app, why_status};
use super::*;

/// The verdict comes first, then the tab's evidence, with each hook fact on a
/// line of its own: "a reporter is installed" stays apart from "a hook was
/// seen to run", however narrow the pane.
#[test]
fn why_status_opens_a_saveable_pager_with_the_verdict_first() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = agent_app(&dir, "claude");
        let text = why_status(&mut app);
        let view = app.view.pager.as_ref().expect("a pager opened");
        assert_eq!(view.title, "why-status");
        assert!(view.saveable, "its report can be saved");
        assert!(
            !app.flash_text().unwrap_or_default().contains("why-status"),
            "the diagnostic is not in the status line: {:?}",
            app.flash_text()
        );

        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("why-status @ "), "{text}");
        assert!(
            lines[2].ends_with("(output-timing)"),
            "verdict first: {text}"
        );
        assert_eq!(lines[3], "no output yet", "{text}");
        assert!(
            lines[5].starts_with("*[1] "),
            "then the tab's report: {text}"
        );

        let hooks = lines.iter().position(|l| *l == "    hooks:").expect(&text);
        assert!(lines[hooks + 1].starts_with("      MISSING"), "{text}");
        assert!(
            lines.contains(&"      hook execution unverified"),
            "unverified execution is its own fact: {text}"
        );
    });
}

/// A plain shell tab has no dot, and says so, with its report beneath.
#[test]
fn why_status_on_a_tab_that_is_not_an_agent_says_it_has_no_dot() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("cat", &dir));
        let text = why_status(&mut app);
        assert!(
            text.contains("\"cat\" is not a known agent, so it has no dot"),
            "{text}"
        );
        assert!(text.contains("    command: cat"), "{text}");
        assert!(!text.contains("hooks:"), "no hooks to speak of: {text}");
    });
}

/// With no pane there is nothing to explain, so no pager.
#[test]
fn why_status_with_no_pane_says_so_in_the_status_line() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let mut app = App::test_app(std::fs::canonicalize(tmp.path()).unwrap());
        app.dispatch_command("why-status");
        assert!(app.view.pager.is_none());
        assert_eq!(app.flash_text(), Some("why-status: no pane open"));
    });
}

/// A codex a shell started without `--no-daemon` may run on its shared
/// daemon, whose hooks can't reach the tab: `:why-status` says why the dot
/// follows output, and how to get hook status.
#[test]
fn why_status_explains_a_shell_codex_on_the_shared_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("sh", &dir));
        app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0]
            .info
            .running = Some(crate::pane::RunningAgent {
            kind: crate::state::sessions::AgentKind::Codex,
            pid: 1,
            started_epoch_secs: 1,
            backgrounded: false,
            on_shared_daemon: true,
        });
        let text = why_status(&mut app);
        assert!(text.contains("Start it with `codex --no-daemon`"), "{text}");
        assert!(
            text.contains("`codex agents` and `codex queue` can't reach it"),
            "names the cost: {text}"
        );
        assert!(
            text.contains("without --no-daemon: hooks can't name this tab"),
            "{text}"
        );
    });
}

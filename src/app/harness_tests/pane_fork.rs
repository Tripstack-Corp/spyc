//! `^a F` forks the active tab's conversation into a new tab and leaves the
//! original where it was.

use super::*;

const PARENT: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";

/// An app whose one tab runs the fake `name` agent in `dir`, started and with
/// its argv file cleared, so the next start recorded is the fork's.
fn agent_tab(dir: &std::path::Path, name: &str) -> App {
    let cmd = fake_agent(dir, name).display().to_string();
    let mut app = App::test_app(dir.to_path_buf());
    assert!(app.open_pane_tab_in(&cmd, dir));
    agent_argv(dir, name);
    std::fs::remove_file(dir.join(format!("{name}.argv"))).unwrap();
    app
}

fn tabs(app: &App) -> &crate::pane::tabs::PaneTabs {
    app.runtime.pane_tabs.as_ref().unwrap()
}

#[test]
fn a_codex_tab_forks_its_conversation_into_a_new_tab() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = agent_tab(&dir, "codex");
        let source = app.runtime.pane_tabs.as_mut().unwrap().tabs_mut();
        source[0].info.codex_session_id = Some(PARENT.into());
        // The pane has wandered; the branch still starts where codex did.
        source[0].set_live_cwd(dir.join("bin"));

        app.apply(&Action::PaneForkTab).unwrap();

        assert_eq!(agent_argv(&dir, "codex"), ["--no-daemon", "fork", PARENT]);
        let tabs = tabs(&app);
        assert_eq!(tabs.tabs().len(), 2);
        assert_eq!(tabs.active_index(), 1, "the fork is where you land");
        let fork = &tabs.tabs()[1].info;
        assert_eq!(fork.cwd, dir);
        assert_eq!(
            fork.codex_session_id, None,
            "the fork is not its parent's conversation"
        );
        assert_eq!(
            tabs.tabs()[0].info.codex_session_id.as_deref(),
            Some(PARENT),
            "the original keeps its own"
        );
        assert_eq!(app.flash_text(), Some("forked tab 1 into tab 2"));
    });
}

/// Two tabs can run one conversation (`codex resume X` twice). Each is still
/// that conversation's own tab, so either one forks it.
#[test]
fn a_conversation_two_tabs_share_still_forks() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = agent_tab(&dir, "codex");
        let cmd = tabs(&app).tabs()[0].info.command.clone();
        assert!(app.open_pane_tab_in(&cmd, &dir));
        agent_argv(&dir, "codex");
        std::fs::remove_file(dir.join("codex.argv")).unwrap();
        for tab in app.runtime.pane_tabs.as_mut().unwrap().tabs_mut() {
            tab.info.codex_session_id = Some(PARENT.into());
        }

        app.apply(&Action::PaneForkTab).unwrap();

        assert_eq!(agent_argv(&dir, "codex"), ["--no-daemon", "fork", PARENT]);
        assert_eq!(tabs(&app).tabs().len(), 3);
    });
}

#[test]
fn a_codex_tab_with_no_conversation_yet_is_not_forked() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = agent_tab(&dir, "codex");

        app.apply(&Action::PaneForkTab).unwrap();

        assert_eq!(tabs(&app).tabs().len(), 1);
        assert!(
            app.flash_text().unwrap().contains("nothing to branch yet"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// agy can resume its conversation but not branch it. A second tab on the same
/// conversation would be two clients of one session, so `^a F` opens nothing.
#[test]
fn an_agent_that_cannot_branch_opens_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = agent_tab(&dir, "agy");
        app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0]
            .info
            .live_session_id = Some(PARENT.into());

        app.apply(&Action::PaneForkTab).unwrap();

        assert_eq!(tabs(&app).tabs().len(), 1);
        assert!(
            app.flash_text().unwrap().contains("agy can't branch"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// A shell has no conversation, so its fork is a copy, opened where the shell
/// is now rather than where it started.
#[test]
fn a_shell_tab_forks_into_a_copy_at_its_live_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let moved = dir.join("moved");
        std::fs::create_dir(&moved).unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in("cat", &dir));
        app.runtime.pane_tabs.as_mut().unwrap().tabs_mut()[0].set_live_cwd(moved.clone());

        app.apply(&Action::PaneForkTab).unwrap();

        let tabs = tabs(&app);
        assert_eq!(tabs.tabs().len(), 2);
        assert_eq!(tabs.tabs()[1].info.command, "cat");
        assert_eq!(tabs.tabs()[1].info.cwd, moved);
    });
}

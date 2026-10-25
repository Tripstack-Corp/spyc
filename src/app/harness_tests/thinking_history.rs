//! `^a t`, an agy tab's thinking history: which tabs it opens on, and what
//! `r`, `T` and `t` do inside it.

use super::*;

/// A pane tab running `cat` under `command`'s name, so it is detected as
/// that agent and spawns in a test.
fn agent_tab(app: &mut App, dir: &std::path::Path, command: &str) {
    let wake = app.make_pane_wake();
    let pane = crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.project().context_path, wake)
        .expect("spawn cat");
    let entry = crate::pane::tabs::TabEntry::new(
        pane,
        crate::pane::tabs::TabInfo::new(command, dir.to_path_buf()),
    );
    app.runtime.project_mut().pane_tabs = Some(crate::pane::tabs::PaneTabs::new(entry));
}

fn ctrl_a(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
        .unwrap();
    press(app, c);
}

/// A key, then the loop top's refresh of what routes the next one.
fn press(app: &mut App, c: char) {
    app.handle_key(key(c)).unwrap();
    app.snapshot_pane_routing();
}

/// The scrollback's own flash: a pager drawn over the status bar takes the
/// flashes its keys raise.
fn scroll_flash(app: &App) -> Option<String> {
    app.view
        .project()
        .scroll_pager
        .as_ref()
        .and_then(|v| v.flash.as_ref())
        .map(ToString::to_string)
}

fn scroll_title(app: &App) -> Option<String> {
    app.view
        .project()
        .scroll_pager
        .as_ref()
        .map(|v| v.title.clone())
}

#[test]
fn ctrl_a_t_opens_an_agy_tabs_thinking_and_its_keys_keep_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir.clone());
        agent_tab(&mut app, &dir, "agy");

        ctrl_a(&mut app, 't');
        let thinking = Some(" agy (thinking)".to_owned());
        assert_eq!(scroll_title(&app), thinking);

        // `r` reads the thinking again, not the transcript `^a v` shows.
        press(&mut app, 'r');
        assert_eq!(scroll_title(&app), thinking);

        // `T` and `t` belong to `^a v`'s views; here they only say so.
        press(&mut app, 'T');
        assert_eq!(
            scroll_flash(&app).as_deref(),
            Some("source flip: ^a v's scrollback only")
        );
        assert_eq!(scroll_title(&app), thinking);
        press(&mut app, 't');
        assert_eq!(
            scroll_flash(&app).as_deref(),
            Some("tool calls: ^a v's transcript only")
        );
        assert_eq!(scroll_title(&app), thinking);
    });
}

#[test]
fn ctrl_a_t_on_another_agents_tab_opens_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        let mut app = App::test_app(dir.clone());
        agent_tab(&mut app, &dir, "claude");

        ctrl_a(&mut app, 't');
        assert_eq!(app.flash_text(), Some("thinking history: agy tabs only"));
        assert_eq!(scroll_title(&app), None);
    });
}

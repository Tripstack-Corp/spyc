//! The key a click names, and the hint's fade. A left click on a status-bar
//! project names the key that does the same, as a menu row's click does; every
//! key-fired action counts toward its hint's fade, the pane's own `^z` among
//! them, and a menu click never does. The status-bar and divider menus take
//! `↓` and `Enter` as an entry's does. Each case goes through `handle_key` or
//! `handle_mouse` and aims at what the frame draws.

use super::divider_menu_tests::{
    app_with_split, app_with_tabs, on_the_split, past_the_tabs, split_mode,
};
use super::menu_tests::{app_with_tab, ctrl_a, flash, right_click};
use super::status_menu_tests::{esc, on_bar, project_list_open, projects, read_git, repo_app};
use super::tests::{FRAME, centre, click, drawn_row, press};
use crate::app::state::ZoomTarget;
use crate::app::{App, Effect, Mode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

/// Run `body` over a fresh state root, in a canonical temp dir, with mouse
/// reporting on, as a click needs it.
fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let state = base.join("state");
    std::fs::create_dir_all(&state).unwrap();
    crate::state::with_state_root(&state, || {
        crate::set_mouse_capture_for_test(true);
        body(&base);
        crate::set_mouse_capture_for_test(false);
    });
}

fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(code, modifiers)).unwrap()
}

/// The leader, then `c`: `Space c`.
fn space(app: &mut App, c: char) {
    press(app, ' ');
    press(app, c);
}

fn names(app: &App, key: &str) -> bool {
    flash(app).is_some_and(|f| f.starts_with(&format!("{key} ")))
}

// ── a status-bar project's click names its key ──

/// A click on a project's label goes there and names `Space N`, as a menu
/// row's click names its key.
#[test]
#[ignore = "red: a project label's click names no key"]
fn a_click_on_a_project_label_names_its_key() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo"], 120);
        let label = on_bar(&mut app, "bravo");

        click(&mut app, label);

        assert_eq!(app.state.active, ids[1], "premise: bravo is in view");
        assert_eq!(flash(&app).as_deref(), Some("Space 2  go to project 2"));
    });
}

/// Once `Space N` has been pressed three times, a label's click names it no
/// more: two presses, and it still does.
#[test]
#[ignore = "red: a project label's click names no key"]
fn a_project_labels_key_fades_as_space_n_is_learned() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo"], 120);
        space(&mut app, '2');
        space(&mut app, '1');
        assert_eq!(app.state.active, ids[0], "premise: the keys went there");

        let label = on_bar(&mut app, "bravo");
        click(&mut app, label);
        assert!(names(&app, "Space 2"), "two presses: {:?}", flash(&app));

        space(&mut app, '1');
        let label = on_bar(&mut app, "bravo");
        click(&mut app, label);

        assert_eq!(app.state.active, ids[1], "the click still goes there");
        assert!(!names(&app, "Space 2"), "three presses: {:?}", flash(&app));
    });
}

/// A click on the collapsed count lists the projects and names `Space j`.
#[test]
#[ignore = "red: the collapsed count's click names no key"]
fn a_click_on_the_collapsed_count_names_space_j() {
    with_mouse(|base| {
        let labels = ["alpha", "bravo", "charlie", "delta"];
        let (mut app, _) = projects(base, &labels, 60);
        let count = on_bar(&mut app, "1/4");

        click(&mut app, count);

        assert!(project_list_open(&app), "premise: the list is open");
        assert_eq!(
            flash(&app).as_deref(),
            Some("Space j  list the open projects")
        );
    });
}

// ── every key counts ──

/// The active tab's menu, by a right-click on the pane; then a click on its
/// row reading `row`. What the click asked the loop to do.
fn click_tab_row(app: &mut App, row: &str) -> Vec<Effect> {
    let pane = app.frame_layout(FRAME).pane.expect("a pane is open");
    right_click(app, centre(pane));
    let label = app
        .runtime
        .project()
        .pane_tabs
        .as_ref()
        .expect("a tab")
        .active_info()
        .label
        .clone();
    let at = drawn_row(app, &label, row);
    click(app, at)
}

const SUSPEND: &str = "suspend / resume pane tab";

/// The pane's own `^z` counts toward its hint's fade, as every key-fired
/// action does: after three presses a click on the menu's suspend row still
/// suspends or resumes, and names `^z` no more.
#[test]
#[ignore = "red: the pane's `^z` dispatches without counting toward the fade"]
fn the_panes_ctrl_z_counts_toward_its_hint_fading() {
    with_mouse(|base| {
        let mut app = app_with_tab(base, "sleep 30");
        let ctrl_z = |app: &mut App| key(app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        let fx = ctrl_z(&mut app);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::SignalPane { resume: false, .. })),
            "premise: `^z` suspends the tab: {fx:?}"
        );
        ctrl_z(&mut app);
        click_tab_row(&mut app, SUSPEND);
        assert!(names(&app, "^z"), "two presses: {:?}", flash(&app));

        let fx = ctrl_z(&mut app);
        assert!(
            fx.iter().any(|e| matches!(e, Effect::SaveLearned(_))),
            "the third press is counted and written: {fx:?}"
        );
        let fx = click_tab_row(&mut app, SUSPEND);

        assert!(
            fx.iter().any(|e| matches!(e, Effect::SignalPane { .. })),
            "the row still fires: {fx:?}"
        );
        assert!(!names(&app, "^z"), "three presses: {:?}", flash(&app));
    });
}

const RESTART: &str = "restart pane tab command";

/// `^a R` restarts the tab, after a confirm while it runs, which `Esc`
/// declines.
fn restart_by_key(app: &mut App) {
    ctrl_a(app);
    press(app, 'R');
    assert!(
        matches!(app.state.mode, Mode::Prompting(_)),
        "premise: `^a R` asks first"
    );
    esc(app);
}

/// The restart row's hint (criterion 7): any number of menu clicks never fade
/// it, two `^a R` presses don't, and the third does.
#[test]
fn the_restart_hint_fades_after_three_presses_and_never_after_clicks() {
    with_mouse(|base| {
        let mut app = app_with_tab(base, "sleep 30");
        for n in 1..=5 {
            click_tab_row(&mut app, RESTART);
            assert!(names(&app, "^a R"), "click {n}: {:?}", flash(&app));
            esc(&mut app);
        }

        restart_by_key(&mut app);
        restart_by_key(&mut app);
        click_tab_row(&mut app, RESTART);
        assert!(names(&app, "^a R"), "two presses: {:?}", flash(&app));
        esc(&mut app);

        restart_by_key(&mut app);
        click_tab_row(&mut app, RESTART);

        assert!(
            matches!(app.state.mode, Mode::Prompting(_)),
            "the row still fires"
        );
        assert!(!names(&app, "^a R"), "three presses: {:?}", flash(&app));
    });
}

// ── `↓` and `Enter` in the status-bar and divider menus ──

/// In the open menu, `↓` from the top down to the row whose label holds
/// `text`, then `Enter`.
fn arrow_to_and_enter(app: &mut App, text: &str) -> Vec<Effect> {
    let index = app
        .view
        .chord_hint
        .as_ref()
        .expect("a menu is open")
        .rows
        .iter()
        .position(|(_, label)| label.contains(text))
        .unwrap_or_else(|| panic!("no row {text:?} in the open menu"));
    for _ in 0..=index {
        key(app, KeyCode::Down, KeyModifiers::NONE);
    }
    key(app, KeyCode::Enter, KeyModifiers::NONE)
}

/// The projects menu, from the collapsed count, and a project's own, from its
/// label: `↓` and `Enter` step to the next project and to the previous.
#[test]
fn arrows_and_enter_fire_the_project_menus_rows() {
    with_mouse(|base| {
        let labels = ["alpha", "bravo", "charlie", "delta"];
        let (mut app, ids) = projects(base, &labels, 60);
        let count = on_bar(&mut app, "1/4");
        right_click(&mut app, count);

        arrow_to_and_enter(&mut app, "go to the next project");

        assert_eq!(app.state.active, ids[1], "the next project is in view");
        assert!(app.view.chord_hint.is_none(), "and the menu closed");

        let (mut app, ids) = projects(&base.join("two"), &["alpha", "bravo"], 120);
        let label = on_bar(&mut app, "alpha");
        right_click(&mut app, label);

        arrow_to_and_enter(&mut app, "go to the previous project");

        assert_eq!(app.state.active, ids[1], "the previous project, wrapping");
    });
}

/// The git segment's menu: `Enter` on "new git worktree" asks for its branch.
#[test]
fn arrows_and_enter_fire_the_git_menus_rows() {
    with_mouse(|base| {
        let dir = base.join("repo");
        let mut app = repo_app(&dir);
        read_git(&mut app, &dir);
        let segment = on_bar(&mut app, "\u{e0a0}");
        right_click(&mut app, segment);

        arrow_to_and_enter(&mut app, "new git worktree");

        let Mode::Prompting(prompt) = &app.state.mode else {
            panic!("no prompt opened");
        };
        assert_eq!(prompt.prefix, "worktree branch: ");
    });
}

/// The session segment's menu: `Enter` on its one row shows the session.
#[test]
fn arrows_and_enter_fire_the_session_menus_row() {
    with_mouse(|base| {
        let dir = base.join("work");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.view.term_size = (120, 24);
        app.state.session_name = Some("SAFFRON_CUMIN".to_string());
        let segment = on_bar(&mut app, "SAFFRON_CUMIN");
        right_click(&mut app, segment);

        arrow_to_and_enter(&mut app, "session info");

        assert!(
            app.view.project().pager.is_some(),
            "the session's info is open"
        );
    });
}

/// The pane divider's `pane tabs` menu: `Enter` on "zoom pane" zooms it.
#[test]
fn arrows_and_enter_fire_the_pane_tabs_menus_rows() {
    with_mouse(|base| {
        let mut app = app_with_tabs(base, 1);
        let point = past_the_tabs(&mut app);
        right_click(&mut app, point);

        arrow_to_and_enter(&mut app, "zoom pane");

        assert_eq!(
            app.state.active_project().pane.zoom,
            ZoomTarget::BottomPane,
            "the row zoomed the pane"
        );
        assert!(app.view.chord_hint.is_none(), "and the menu closed");
    });
}

/// The split's menu: `Enter` on its height row flips the split.
#[test]
fn arrows_and_enter_fire_the_split_menus_rows() {
    with_mouse(|base| {
        let mut app = app_with_split(base);
        let shape = split_mode(&app);
        let point = on_the_split(&mut app);
        right_click(&mut app, point);

        arrow_to_and_enter(&mut app, "full-height / top-only");

        let flipped = split_mode(&app);
        assert!(flipped.is_some(), "the split stays open");
        assert_ne!(flipped, shape, "the row flipped the split's height");
    });
}

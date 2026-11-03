//! A menu's hint fades as its key is learned. Once an action has been fired by
//! its own key `[ui] menu_hints_fade_after` times, a click on its row no longer
//! names the key. The counts persist in `learned.json`, `:hints` lists what has
//! faded, and the status bar shows `. menu` until an object's menu first
//! opens. Each case goes through `handle_key`, `handle_mouse`, a `:` command,
//! a drawn frame or a new `App` over the same state root.

use super::menu_tests::{app_over_entries, cursor_on, flash};
use super::tests::{FRAME, click, drawn, drawn_row, press};
use crate::app::{App, Effect};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use std::time::{Duration, Instant};

const TOP_HINT: &str = "gg  top of column";

/// Run `body` over a fresh state root, with mouse reporting on, as a click
/// needs it.
fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::set_mouse_capture_for_test(true);
        body(tmp.path());
        crate::set_mouse_capture_for_test(false);
    });
}

/// A new spyc over the `work/` that [`app_over_entries`] made: what the next
/// launch sees of the state root.
fn restart(tmp: &Path) -> App {
    let mut app = App::test_app(tmp.join("work"));
    app.view.term_size = (FRAME.width, FRAME.height);
    app.state.refresh_listing();
    app
}

fn key(app: &mut App, c: char) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
        .unwrap()
}

/// Type `gg` `n` times; what the keys asked the loop to do.
fn type_gg(app: &mut App, n: usize) -> Vec<Effect> {
    let mut fx = Vec::new();
    for _ in 0..n {
        fx.extend(key(app, 'g'));
        fx.extend(key(app, 'g'));
    }
    fx
}

/// Open `g`'s which-key menu and click its "top of column" row.
fn click_top_of_column(app: &mut App) -> Vec<Effect> {
    press(app, 'g');
    app.settle_chord_hint(
        Instant::now() + Duration::from_secs(60),
        &mut crate::app::RunCtx::for_test(),
    );
    let row = drawn_row(app, "g", "top of column");
    click(app, row)
}

fn named_gg(app: &App) -> bool {
    flash(app).as_deref() == Some(TOP_HINT)
}

/// Make the writes to `learned.json` among `fx`, as `run_effects` does, and
/// count them. No effect writes it yet.
fn persist(fx: Vec<Effect>) -> usize {
    drop(fx);
    0
}

/// Load `toml` as the user's config, through the config loader, and put it in
/// force in the active project.
fn with_config(app: &mut App, tmp: &Path, toml: &str) {
    let rc = tmp.join("rc.toml");
    std::fs::write(&rc, toml).unwrap();
    let config = crate::config::Config::load_from(&[Some(&rc)]).unwrap();
    let project = app.state.active_project_mut();
    project.user_keymap = crate::keymap::user::UserKeymap::from_bindings(config.bindings.clone());
    project.config = config;
}

fn learned_json(tmp: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(tmp.join("learned.json")).ok()?;
    Some(serde_json::from_str(&text).expect("learned.json is JSON"))
}

/// The count `learned.json` holds for the action named `name`.
fn count_on_disk(tmp: &Path, name: &str) -> Option<u64> {
    learned_json(tmp)?.get("counts")?.get(name)?.as_u64()
}

fn status_row(app: &mut App) -> String {
    let buf = drawn(app);
    (0..FRAME.width).map(|x| buf[(x, 0)].symbol()).collect()
}

// ── the hint fades ──

/// Three `gg`s and the hint is learned: a click on `g`'s "top of column" row
/// still moves the cursor, and names no key. Two, and it still does.
#[test]
#[ignore = "red: a key press counts toward nothing, so the hint never fades"]
fn the_hint_fades_after_three_key_presses() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        type_gg(&mut app, 2);
        click_top_of_column(&mut app);
        assert!(named_gg(&app), "two presses: {:?}", flash(&app));

        type_gg(&mut app, 1);
        app.state.cur_mut().cursor.index = 2;
        click_top_of_column(&mut app);

        assert_eq!(app.state.cur().cursor.index, 0, "the row still fires");
        assert!(!named_gg(&app), "three presses: {:?}", flash(&app));
    });
}

/// A click is not a key: clicking the row again and again never silences the
/// hint it is there to teach.
#[test]
fn menu_clicks_never_count() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        for click_number in 1..=5 {
            let fx = click_top_of_column(&mut app);
            persist(fx);
            assert!(named_gg(&app), "click {click_number}: {:?}", flash(&app));
        }
    });
}

/// A row's key typed while its menu is open is a key, and counts.
#[test]
#[ignore = "red: a key typed in a menu counts toward nothing"]
fn a_rows_key_typed_in_its_menu_counts() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        for _ in 0..3 {
            cursor_on(&mut app, "notes.txt");
            key(&mut app, '.');
            key(&mut app, 't');
        }
        cursor_on(&mut app, "notes.txt");
        key(&mut app, '.');
        let row = drawn_row(&mut app, "notes.txt", "toggle pick");

        click(&mut app, row);

        assert!(
            !flash(&app).is_some_and(|f| f.starts_with("t  ")),
            "a learned key: {:?}",
            flash(&app)
        );
    });
}

/// A key of the user's own that fires the action teaches it as well as the
/// default does.
#[test]
#[ignore = "red: a user binding's action counts toward nothing"]
fn a_user_bindings_action_counts() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        with_config(&mut app, tmp, "keymap = [\"map X goto_first\"]\n");
        for _ in 0..3 {
            key(&mut app, 'X');
        }

        click_top_of_column(&mut app);

        assert!(!named_gg(&app), "learned by X: {:?}", flash(&app));
    });
}

/// `menu_hints = "always"` never fades.
#[test]
#[ignore = "red: spyc reads no `[ui]`"]
fn always_keeps_the_hint() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        with_config(&mut app, tmp, "[ui]\nmenu_hints = \"always\"\n");
        persist(type_gg(&mut app, 3));

        click_top_of_column(&mut app);

        assert!(named_gg(&app), "always: {:?}", flash(&app));
    });
}

/// `menu_hints = "off"` names no key, counts nothing and writes nothing.
#[test]
#[ignore = "red: spyc reads no `[ui]`"]
fn off_names_nothing_and_writes_nothing() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        with_config(&mut app, tmp, "[ui]\nmenu_hints = \"off\"\n");

        let writes = persist(type_gg(&mut app, 3));
        click_top_of_column(&mut app);
        assert!(flash(&app).is_none(), "off: {:?}", flash(&app));
        cursor_on(&mut app, "notes.txt");
        let writes = writes + persist(key(&mut app, '.'));

        assert_eq!(writes, 0, "no write");
        assert!(learned_json(tmp).is_none(), "no learned.json");
    });
}

/// `menu_hints_fade_after` is how many presses it takes.
#[test]
#[ignore = "red: spyc reads no `[ui]`"]
fn fade_after_sets_how_many_presses() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        with_config(&mut app, tmp, "[ui]\nmenu_hints_fade_after = 1\n");
        persist(type_gg(&mut app, 1));

        click_top_of_column(&mut app);

        assert!(!named_gg(&app), "one press: {:?}", flash(&app));
    });
}

/// A setting outside what `[ui]` allows fails the file, as any bad value does.
#[test]
#[ignore = "red: `[ui]` is skipped as an unknown key, its values unchecked"]
fn a_bad_ui_value_is_a_config_error() {
    let tmp = tempfile::tempdir().unwrap();
    for bad in ["menu_hints = \"sometimes\"", "menu_hints_fade_after = 0"] {
        let rc = tmp.path().join("rc.toml");
        std::fs::write(&rc, format!("[ui]\n{bad}\n")).unwrap();
        assert!(
            crate::config::Config::load_from_allowing_unknown(&[Some(&rc)]).is_err(),
            "{bad} loads"
        );
    }
}

// ── the counts persist ──

/// The counts outlive the run: a new spyc over the same state root has the
/// hint faded already.
#[test]
#[ignore = "red: no count is kept, in memory or in `learned.json`"]
fn counts_persist_across_a_restart() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        persist(type_gg(&mut app, 3));
        drop(app);

        let mut app = restart(tmp);
        click_top_of_column(&mut app);

        assert!(!named_gg(&app), "after a restart: {:?}", flash(&app));
    });
}

/// A name this spyc doesn't know, a newer spyc's action, is kept as it was.
#[test]
#[ignore = "red: spyc reads and writes no `learned.json`"]
fn an_unknown_name_survives_a_write() {
    with_mouse(|tmp| {
        std::fs::write(
            tmp.join("learned.json"),
            r#"{"counts": {"a_future_action": 2, "goto_first": 3}, "menu_seen": false}"#,
        )
        .unwrap();
        let mut app = app_over_entries(tmp);
        click_top_of_column(&mut app);
        assert!(!named_gg(&app), "the file's count: {:?}", flash(&app));

        let writes = persist(key(&mut app, 'G'));

        assert_eq!(writes, 1, "G's first press is written");
        assert_eq!(count_on_disk(tmp, "goto_last"), Some(1));
        assert_eq!(count_on_disk(tmp, "a_future_action"), Some(2));
        assert_eq!(count_on_disk(tmp, "goto_first"), Some(3));
    });
}

/// A `learned.json` that isn't JSON starts over empty: spyc starts, the hint
/// shows, and the startup check says so.
#[test]
#[ignore = "red: the startup check doesn't read `learned.json`"]
fn a_corrupt_file_loads_as_empty() {
    with_mouse(|tmp| {
        std::fs::write(tmp.join("learned.json"), "{ not json").unwrap();
        let mut app = app_over_entries(tmp);

        click_top_of_column(&mut app);

        assert!(named_gg(&app), "an empty count: {:?}", flash(&app));
        let warnings = crate::state::health::check(tmp).warnings;
        assert!(
            warnings.iter().any(|w| w.contains("learned.json")),
            "{warnings:?}"
        );
    });
}

/// A key pressed again and again writes only until its hint fades: the count
/// stops at the threshold.
#[test]
#[ignore = "red: no key press writes `learned.json`"]
fn writes_stop_at_the_threshold() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);

        let writes = persist(type_gg(&mut app, 13));

        assert_eq!(writes, 3, "one write per press until it fades");
        assert_eq!(count_on_disk(tmp, "goto_first"), Some(3));
    });
}

// ── `:hints` ──

/// The pager `:hints` opens, as text, closed again by `q` once read.
fn hints_pager(app: &mut App) -> String {
    app.dispatch_command("hints");
    let pager = app
        .view
        .project()
        .pager
        .as_ref()
        .expect(":hints opens a pager");
    let text = pager
        .lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect::<Vec<String>>()
        .join("\n");
    key(app, 'q');
    assert!(app.view.project().pager.is_none(), "q closed it");
    text
}

/// `:hints` lists what has faded, and `:hints reset` brings every hint back.
#[test]
#[ignore = "red: `:hints` is no command"]
fn hints_lists_the_faded_and_reset_brings_them_back() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        let before = hints_pager(&mut app);
        assert!(!before.contains("top of column"), "{before}");
        persist(type_gg(&mut app, 3));

        let listed = hints_pager(&mut app);
        assert!(listed.contains("top of column"), "{listed}");
        assert!(listed.contains("goto_first"), "{listed}");

        persist(app.dispatch_command("hints reset"));
        click_top_of_column(&mut app);

        assert!(named_gg(&app), "after a reset: {:?}", flash(&app));
        assert_eq!(count_on_disk(tmp, "goto_first"), None, "the file forgot");
    });
}

// ── the status bar's `. menu` ──

/// A fresh install's status bar shows `. menu` until `.` opens a menu, and
/// never again, in a later run too.
#[test]
#[ignore = "red: the status bar doesn't show `. menu`"]
fn the_status_bar_teaches_the_menu_key_until_a_menu_opens() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        assert!(status_row(&mut app).contains(". menu"), "a fresh install");

        cursor_on(&mut app, "notes.txt");
        persist(key(&mut app, '.'));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert!(!status_row(&mut app).contains(". menu"), "a menu opened");
        let mut app = restart(tmp);
        assert!(!status_row(&mut app).contains(". menu"), "the next run");
    });
}

/// With `menu_hints = "off"` the bar teaches nothing either.
#[test]
#[ignore = "red: spyc reads no `[ui]`"]
fn off_hides_the_status_bars_menu_key() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        with_config(&mut app, tmp, "[ui]\nmenu_hints = \"off\"\n");

        assert!(!status_row(&mut app).contains(". menu"));
    });
}

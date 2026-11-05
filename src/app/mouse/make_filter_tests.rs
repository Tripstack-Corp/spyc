//! Typing in the make menu filters its targets: each character but `?` goes
//! to a filter the title shows, the rows narrow to the targets it matches,
//! and the highlight sits on the first. Each case opens the menu with
//! `Space m`, types through `handle_key`, and reads the menu as the frame
//! draws it.

use super::make_menu_tests::{
    app_over, key, land_make_menu, make_menu_drawn, make_row_at, pager_flash, pager_title, space_m,
    special, with_mouse,
};
use super::menu_tests::app_over_entries;
use super::tests::{FRAME, click, press};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use std::fmt::Write as _;
use std::path::Path;

const MAKEFILE: &str = "\
build: ## Build it
\t@true
check: ## Run the checks
\t@true
chmod-all: ## Make every script executable
\t@true
";

/// An App over `work/`, holding `makefile`, with its make menu open.
fn make_menu_over(tmp: &Path, makefile: &str) -> App {
    let mut app = app_over(&tmp.join("work"), &[("Makefile", makefile)]);
    let fx = space_m(&mut app);
    land_make_menu(&mut app, fx);
    app
}

/// Type `text` into the open make menu, which stays open.
fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, c);
        assert!(app.view.chord_hint.is_some(), "{c:?} closed the make menu");
    }
}

/// The open make menu's title and the rows it draws, on a `frame`-sized
/// screen.
fn shown(app: &mut App, frame: Rect) -> (String, Vec<String>) {
    assert!(app.view.chord_hint.is_some(), "the make menu closed");
    let (_, title, lines) = make_menu_drawn(app, frame);
    let rows = lines
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    (title, rows)
}

/// The targets the drawn rows run.
fn targets(rows: &[String]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| r.strip_prefix(":make ")?.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// `ch` leaves the two targets it matches, `check` and `chmod-all`, the title
/// shows it, and `Enter` right away runs the first.
#[test]
fn typing_narrows_the_targets_and_enter_runs_the_first_match() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);

        typed(&mut app, "ch");

        let (title, rows) = shown(&mut app, FRAME);
        assert!(title.contains(" make · Makefile · ch "), "{title}");
        assert_eq!(targets(&rows), ["check", "chmod-all"], "{rows:#?}");
        special(&mut app, KeyCode::Enter);
        let title = pager_title(&app).unwrap_or_default();
        assert!(
            title.contains("! make check"),
            "the first match ran: {title}"
        );
    });
}

/// The arrows move among the matches.
#[test]
fn the_arrows_move_among_the_matches() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);
        typed(&mut app, "ch");

        special(&mut app, KeyCode::Down);
        special(&mut app, KeyCode::Enter);

        let title = pager_title(&app).unwrap_or_default();
        assert!(
            title.contains("! make chmod-all"),
            "the second match: {title}"
        );
    });
}

/// `Backspace` takes a character off the filter, widening the match, back to
/// every target; with nothing left to take off, it leaves the menu as it is.
#[test]
fn backspace_widens_the_match() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);
        typed(&mut app, "chm");
        let (title, rows) = shown(&mut app, FRAME);
        assert!(title.contains(" make · Makefile · chm "), "{title}");
        assert_eq!(targets(&rows), ["chmod-all"], "{rows:#?}");

        special(&mut app, KeyCode::Backspace);
        let (title, rows) = shown(&mut app, FRAME);
        assert!(title.contains(" make · Makefile · ch "), "{title}");
        assert_eq!(targets(&rows), ["check", "chmod-all"], "{rows:#?}");

        special(&mut app, KeyCode::Backspace);
        special(&mut app, KeyCode::Backspace);
        let all = shown(&mut app, FRAME);
        assert!(all.0.contains(" make · Makefile ─"), "no filter: {}", all.0);
        assert_eq!(targets(&all.1), ["build", "check", "chmod-all"]);

        special(&mut app, KeyCode::Backspace);
        assert_eq!(shown(&mut app, FRAME), all, "an empty filter stays empty");
    });
}

/// `Esc` with a filter clears it, back to every target, and a second `Esc`
/// closes the menu.
#[test]
fn esc_clears_the_filter_and_a_second_esc_closes() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);
        typed(&mut app, "ch");

        special(&mut app, KeyCode::Esc);

        let (title, rows) = shown(&mut app, FRAME);
        assert!(title.contains(" make · Makefile ─"), "no filter: {title}");
        assert_eq!(targets(&rows), ["build", "check", "chmod-all"]);
        special(&mut app, KeyCode::Esc);
        assert!(app.view.chord_hint.is_none(), "the second `Esc` closed it");
    });
}

/// A filter no target matches leaves one row that says so and runs nothing.
#[test]
fn a_filter_matching_nothing_shows_one_inert_row() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);

        typed(&mut app, "zz");

        let (_, rows) = shown(&mut app, FRAME);
        assert_eq!(rows, ["make: no target matches \"zz\""]);
        special(&mut app, KeyCode::Enter);
        assert!(app.runtime.pending_capture.is_none(), "nothing runs");
        assert!(pager_title(&app).is_none(), "no pager opens");
    });
}

/// Forty targets don't fit an 80×24 frame, but the filter searches all of
/// them: one the fitted list cut off is found, and the closing row counts
/// the matches left off, not the targets.
#[test]
fn the_filter_finds_a_target_the_fitted_list_cut_off() {
    with_mouse(|tmp| {
        let mut text = String::new();
        for i in 1..=30 {
            writeln!(text, "build-{i:02}: ## Build part {i:02}\n\t@true").unwrap();
        }
        for i in 1..=10 {
            writeln!(text, "test-{i:02}: ## Test part {i:02}\n\t@true").unwrap();
        }
        let mut app = app_over(&tmp.join("work"), &[("Makefile", &text)]);
        let small = Rect::new(0, 0, 80, 24);
        app.view.term_size = (small.width, small.height);
        let fx = space_m(&mut app);
        land_make_menu(&mut app, fx);
        let (_, rows) = shown(&mut app, small);
        assert!(
            !targets(&rows).contains(&"test-07".to_string()),
            "premise: the fitted list cut it off: {rows:#?}"
        );

        typed(&mut app, "build");
        let (_, rows) = shown(&mut app, small);
        let more = rows
            .last()
            .and_then(|l| l.strip_prefix("… "))
            .and_then(|l| l.split_once(" more: :make <target>"))
            .and_then(|(n, _)| n.parse::<usize>().ok())
            .unwrap_or_else(|| panic!("premise: thirty don't fit: {rows:#?}"));
        assert_eq!(targets(&rows).len() + more, 30, "{rows:#?}");

        special(&mut app, KeyCode::Esc);
        typed(&mut app, "test-07");
        let (_, rows) = shown(&mut app, small);
        assert_eq!(targets(&rows), ["test-07"], "{rows:#?}");
        special(&mut app, KeyCode::Enter);
        let title = pager_title(&app).unwrap_or_default();
        assert!(title.contains("! make test-07"), "{title}");
    });
}

/// A click on a filtered row runs its target and names `:make <target>`, as a
/// click on any make row does.
#[test]
fn a_click_on_a_filtered_row_names_its_command() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);
        typed(&mut app, "chm");

        let at = make_row_at(&mut app, ":make chmod-all");
        click(&mut app, at);

        let title = pager_title(&app).unwrap_or_default();
        assert!(title.contains("! make chmod-all"), "{title}");
        let hint = pager_flash(&app).unwrap_or_default();
        assert!(hint.contains(":make chmod-all"), "the hint: {hint:?}");
    });
}

/// `?` is no target's character, so in the make menu it still opens the key
/// reference.
#[test]
fn a_question_mark_in_the_make_menu_opens_the_key_reference() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);

        key(&mut app, '?');

        assert!(app.help_is_open(), "the key reference is open");
        assert!(app.view.chord_hint.is_none(), "and the menu closed");
    });
}

/// Another menu has no filter: a letter typed in the entry menu closes it and
/// does what it always does, `j` moving the cursor.
#[test]
fn a_letter_in_the_entry_menu_closes_it_and_acts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = app_over_entries(tmp.path());
        app.state.cur_mut().cursor.index = 0;
        assert!(app.state.cur().rows.len() > 1, "premise: a row to move to");
        press(&mut app, '.');
        assert!(app.view.chord_hint.is_some(), "premise: the menu is open");

        press(&mut app, 'j');

        assert!(app.view.chord_hint.is_none(), "the menu closed");
        assert_eq!(app.state.cur().cursor.index, 1, "and `j` moved the cursor");
    });
}

/// The filter matches a name as the list's `=` filter does: anywhere in it,
/// in any case, so `MOD` finds `chmod-all`.
#[test]
fn the_filter_matches_as_the_lists_filter_does() {
    with_mouse(|tmp| {
        let mut app = make_menu_over(tmp, MAKEFILE);

        typed(&mut app, "MOD");

        let (_, rows) = shown(&mut app, FRAME);
        assert_eq!(targets(&rows), ["chmod-all"], "{rows:#?}");
    });
}

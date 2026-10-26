//! `]p` / `[p`: the cursor to the next / previous picked row, pressed as keys
//! in a listed directory. Mirrors `]g` / `[g`: wraps end to end, walks only
//! the rows on screen, and flashes when there is nothing to land on.

use super::*;
use crate::app::state::Side;

const FILES: [&str; 5] = [
    "alpha.txt",
    "bravo.txt",
    "charlie.txt",
    "delta.md",
    "echo.txt",
];

fn listed(dir: &std::path::Path) -> App {
    std::fs::create_dir_all(dir).unwrap();
    for name in FILES {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let mut app = App::test_app(dir.to_path_buf());
    app.state.refresh_listing(); // `test_app` doesn't list its dir
    app
}

fn press(app: &mut App, keys: &str) {
    for c in keys.chars() {
        app.handle_key(key(c)).unwrap();
    }
}

fn index_of(app: &App, name: &str) -> usize {
    app.state
        .cur()
        .rows
        .iter()
        .position(|r| r.display == name)
        .unwrap_or_else(|| panic!("{name} is not a row"))
}

/// The focused column's cursor row, by name.
fn at(app: &App) -> &str {
    let col = app.state.cur();
    &col.rows[col.cursor.index].display
}

fn put_cursor_on(app: &mut App, name: &str) {
    let i = index_of(app, name);
    app.state.cur_mut().cursor.index = i;
}

/// Pick `name` as a user does: cursor onto it, then `t`.
fn pick(app: &mut App, name: &str) {
    put_cursor_on(app, name);
    press(app, "t");
    assert!(
        app.state
            .cur()
            .picks
            .contains(&app.state.cur().listing.dir.join(name)),
        "`t` picked {name}"
    );
}

#[test]
fn bracket_p_moves_to_each_pick_below_the_cursor_in_turn() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = listed(&tmp.path().join("work"));
        pick(&mut app, "bravo.txt");
        pick(&mut app, "delta.md");
        put_cursor_on(&mut app, "alpha.txt");

        press(&mut app, "]p");
        assert_eq!(at(&app), "bravo.txt");
        // From a picked row, the next pick, not the same one.
        press(&mut app, "]p");
        assert_eq!(at(&app), "delta.md");
        assert_eq!(app.flash_text(), None, "a jump that lands flashes nothing");
    });
}

#[test]
fn bracket_p_wraps_from_below_the_last_pick_to_the_first() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = listed(&tmp.path().join("work"));
        pick(&mut app, "bravo.txt");
        pick(&mut app, "delta.md");
        put_cursor_on(&mut app, "echo.txt");

        press(&mut app, "]p");
        assert_eq!(at(&app), "bravo.txt");
    });
}

#[test]
fn open_bracket_p_moves_to_each_pick_above_and_wraps_to_the_last() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = listed(&tmp.path().join("work"));
        pick(&mut app, "bravo.txt");
        pick(&mut app, "delta.md");
        put_cursor_on(&mut app, "echo.txt");

        press(&mut app, "[p");
        assert_eq!(at(&app), "delta.md");
        press(&mut app, "[p");
        assert_eq!(at(&app), "bravo.txt");
        press(&mut app, "[p");
        assert_eq!(
            at(&app),
            "delta.md",
            "wraps from the first pick to the last"
        );
    });
}

#[test]
fn with_no_picks_the_chord_flashes_and_leaves_the_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = listed(&tmp.path().join("work"));
        put_cursor_on(&mut app, "charlie.txt");

        for chord in ["]p", "[p"] {
            app.state.flash = None;
            press(&mut app, chord);
            assert_eq!(at(&app), "charlie.txt", "{chord} left the cursor");
            assert_eq!(
                app.flash_text(),
                Some("no picks in this directory"),
                "{chord} flashes"
            );
        }
    });
}

/// In a vsplit the chord walks the focused column `b`'s rows and picks, and
/// `a`'s cursor stays where it was.
#[test]
fn in_column_b_the_chord_walks_bs_picks() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = listed(&tmp.path().join("a"));
        pick(&mut app, "charlie.txt");
        put_cursor_on(&mut app, "alpha.txt");
        let b = tmp.path().join("b");
        std::fs::create_dir(&b).unwrap();
        for name in FILES {
            std::fs::write(b.join(name), name).unwrap();
        }
        app.open_second_commander_at(&b);
        assert_eq!(app.focused_side(), Side::Right);
        pick(&mut app, "delta.md");
        put_cursor_on(&mut app, "alpha.txt");

        press(&mut app, "]p");
        assert_eq!(at(&app), "delta.md", "b's own pick, not a's charlie.txt");
        press(&mut app, "[p");
        assert_eq!(at(&app), "delta.md", "b's only pick, from either side");
        let a = app.state.col(Side::Left);
        assert_eq!(
            a.rows[a.cursor.index].display, "alpha.txt",
            "a's cursor never moved"
        );
    });
}

/// A pick the `=` filter hides is off screen, so it is no jump target, as a
/// git change the filter hides is none for `]g`.
#[test]
fn a_pick_the_filter_hides_is_not_a_jump_target() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = listed(&tmp.path().join("work"));
        pick(&mut app, "bravo.txt");
        pick(&mut app, "delta.md");
        press(&mut app, "=*.txt");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();
        let rows: Vec<&str> = app
            .state
            .cur()
            .rows
            .iter()
            .map(|r| r.display.as_str())
            .collect();
        assert_eq!(rows, ["alpha.txt", "bravo.txt", "charlie.txt", "echo.txt"]);
        put_cursor_on(&mut app, "charlie.txt");

        press(&mut app, "]p");
        assert_eq!(at(&app), "bravo.txt", "past the hidden delta.md, wrapping");

        // With the only visible pick dropped, the hidden one leaves nothing.
        press(&mut app, "t");
        press(&mut app, "]p");
        assert_eq!(at(&app), "bravo.txt");
        assert_eq!(app.flash_text(), Some("no picks in this directory"));
    });
}

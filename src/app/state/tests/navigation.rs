#![allow(clippy::wildcard_imports)]
use super::*;

#[test]
fn pane_focused_is_true_only_for_pane_variant() {
    let mut s = test_state();
    s.focus = Focus::Pane;
    assert!(s.pane_focused());
    for f in [
        Focus::FileList,
        Focus::Overlay,
        Focus::Pager(crate::ui::pager::Mount::Overlay),
        Focus::Pager(crate::ui::pager::Mount::TopPane),
        Focus::Pager(crate::ui::pager::Mount::LowerPane),
    ] {
        s.focus = f;
        assert!(!s.pane_focused(), "{f:?} must not be pane-focused");
    }
}

// ── cursor_move_vertical ──────────────────────────────────────

#[test]
fn vertical_move_wraps_forward() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    s.cursor.index = 2;
    s.cursor_move_vertical(1, 3, 3);
    assert_eq!(s.cursor.index, 0);
}

#[test]
fn vertical_move_wraps_backward() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    s.cursor.index = 0;
    s.cursor_move_vertical(-1, 3, 3);
    assert_eq!(s.cursor.index, 2);
}

#[test]
fn vertical_move_no_op_on_empty() {
    let mut s = test_state();
    s.cursor_move_vertical(1, 1, 0);
    assert_eq!(s.cursor.index, 0);
}

#[test]
fn vertical_move_multi_step() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
    s.cursor.index = 1;
    s.cursor_move_vertical(3, 5, 5);
    assert_eq!(s.cursor.index, 4);
}

// ── goto_col_top / goto_col_bottom ────────────────────────────

#[test]
fn goto_col_top_first_column() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
    s.cursor.index = 2; // last in first column
    s.goto_col_top(3);
    assert_eq!(s.cursor.index, 0);
}

#[test]
fn goto_col_top_second_column() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
    s.cursor.index = 4; // second column, row 1
    s.goto_col_top(3);
    assert_eq!(s.cursor.index, 3); // top of second column
}

#[test]
fn goto_col_bottom_first_column() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
    s.cursor.index = 0;
    s.goto_col_bottom(3, 5);
    assert_eq!(s.cursor.index, 2); // last in first column (3 rows)
}

#[test]
fn goto_col_bottom_partial_column() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
    s.cursor.index = 3; // second column
    s.goto_col_bottom(3, 5);
    assert_eq!(s.cursor.index, 4); // last entry in partial column
}

// ── cursor_move_columns ───────────────────────────────────────

#[test]
fn column_move_right() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
    s.cursor.index = 1; // col 0, row 1
    s.cursor_move_columns(1, 3, 6);
    assert_eq!(s.cursor.index, 4); // col 1, row 1
}

#[test]
fn column_move_left() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
    s.cursor.index = 4; // col 1, row 1
    s.cursor_move_columns(-1, 3, 6);
    assert_eq!(s.cursor.index, 1); // col 0, row 1
}

#[test]
fn column_move_wraps_at_edge() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
    s.cursor.index = 4; // col 1, row 1
    s.cursor_move_columns(1, 3, 6); // wraps to col 0
    assert_eq!(s.cursor.index, 1); // col 0, row 1
}

#[test]
fn column_move_single_column_noop() {
    let mut s = state_with_rows(&["a", "b"]);
    s.cursor.index = 0;
    s.cursor_move_columns(1, 10, 2);
    assert_eq!(s.cursor.index, 0); // no-op
}

// ── ensure_cursor_visible ─────────────────────────────────────

#[test]
fn ensure_visible_snaps_view_top() {
    let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f", "g", "h"]);
    s.grid_dims = GridDims {
        cols: 1,
        rows_per_col: 3,
    }; // 3 items per page
    s.cursor.index = 5; // page 1 (items 3-5)
    s.ensure_cursor_visible();
    assert_eq!(s.cursor.view_top, 3);
}

#[test]
fn ensure_visible_first_page() {
    let mut s = state_with_rows(&["a", "b", "c", "d"]);
    s.grid_dims = GridDims {
        cols: 1,
        rows_per_col: 3,
    };
    s.cursor.index = 1;
    s.ensure_cursor_visible();
    assert_eq!(s.cursor.view_top, 0);
}

// ── find_match ────────────────────────────────────────────────

#[test]
fn find_prefix_match() {
    let s = state_with_rows(&["alpha", "beta", "gamma"]);
    assert_eq!(s.find_match("b", 0, false), Some(1));
}

#[test]
fn find_wraps_around() {
    // Pick names with no shared substrings so the wrap behavior is
    // unambiguous under substring matching: only `foo` contains `f`.
    let s = state_with_rows(&["foo", "bar", "baz"]);
    assert_eq!(s.find_match("f", 1, false), Some(0)); // wraps from bar/baz back to foo
}

#[test]
fn find_backward() {
    let s = state_with_rows(&["alpha", "beta", "gamma"]);
    assert_eq!(s.find_match("b", 2, true), Some(1));
}

#[test]
fn find_no_match() {
    let s = state_with_rows(&["alpha", "beta"]);
    assert_eq!(s.find_match("xyz", 0, false), None);
}

#[test]
fn find_glob_pattern() {
    let s = state_with_rows(&["foo.rs", "bar.txt", "baz.rs"]);
    assert_eq!(s.find_match("*.rs", 0, false), Some(0));
    assert_eq!(s.find_match("*.rs", 1, false), Some(2));
}

#[test]
fn find_empty_rows() {
    let s = test_state();
    assert_eq!(s.find_match("a", 0, false), None);
}

/// Regression: `/env` used to anchor at the start of the name,
/// so dot-prefixed files (`.env`, `.envrc`) were unreachable
/// without typing the dot. Now substring — `env` finds them all.
#[test]
fn find_substring_matches_dot_prefixed_file() {
    let s = state_with_rows(&[".env", ".envrc", "main.rs", "environment.toml"]);
    assert_eq!(s.find_match("env", 0, false), Some(0));
    assert_eq!(s.find_match("env", 1, false), Some(1));
    assert_eq!(s.find_match("env", 2, false), Some(3));
}

/// Substring match is case-insensitive on both sides.
#[test]
fn find_substring_is_case_insensitive() {
    let s = state_with_rows(&["README.md", "src", "Cargo.toml"]);
    assert_eq!(s.find_match("readme", 0, false), Some(0));
    assert_eq!(s.find_match("CARGO", 0, false), Some(2));
}

/// Globs are still anchor-aware (no implicit substring) so the
/// power-user escape hatch keeps working: `env*` only matches
/// names *starting* with `env`, hiding `.env` etc.
#[test]
fn find_glob_remains_anchored() {
    let s = state_with_rows(&[".env", "envoy", "main.rs"]);
    assert_eq!(s.find_match("env*", 0, false), Some(1));
}

// ── flash ─────────────────────────────────────────────────────

#[test]
fn jump_next_git_change_skips_clean_rows() {
    let mut s = dirty_state(&["a", "b", "c", "d"], &["c"]);
    s.cursor.index = 0;
    assert!(s.jump_to_git_change(true));
    assert_eq!(s.cursor.index, 2); // landed on `c`
}

#[test]
fn jump_next_git_change_wraps_around() {
    let mut s = dirty_state(&["a", "b", "c", "d"], &["a"]);
    s.cursor.index = 2; // past the only dirty row
    assert!(s.jump_to_git_change(true));
    assert_eq!(s.cursor.index, 0); // wrapped back to `a`
}

#[test]
fn jump_prev_git_change_wraps_around() {
    let mut s = dirty_state(&["a", "b", "c", "d"], &["d"]);
    s.cursor.index = 1; // before the only dirty row in reverse
    assert!(s.jump_to_git_change(false));
    assert_eq!(s.cursor.index, 3); // wrapped to `d`
}

#[test]
fn jump_advances_off_the_current_dirty_row() {
    // From a dirty row, pressing `]g` should land on the *next*
    // dirty row, not stay put.
    let mut s = dirty_state(&["a", "b", "c", "d"], &["a", "c"]);
    s.cursor.index = 0;
    assert!(s.jump_to_git_change(true));
    assert_eq!(s.cursor.index, 2);
}

#[test]
fn jump_returns_false_when_no_changes() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    assert!(!s.jump_to_git_change(true));
    assert!(!s.jump_to_git_change(false));
}

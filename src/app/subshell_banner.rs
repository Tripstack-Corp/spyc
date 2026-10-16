//! The escape sequences that mark spyc's `$` subshell handoff (#686):
//! DECSTBM reserves the top row of the real host terminal so a reverse-video
//! marker survives a `clear` run inside the subshell — `clear` erases screen
//! content (ED) but never the scroll region. `run_child_in_foreground`
//! writes [`enter`] right after `suspend_tui` and [`leave`] right before
//! `resume_tui`; both are pure string builders so the byte sequences are
//! unit-tested without a real terminal.

/// Shown in the reserved row and reused as the window title while the
/// subshell runs — `run_child_in_foreground` sets both from this one string.
pub const MARKER: &str = "🌶️ spyc subshell — exit or ^D to return";

/// Sequence to write immediately after `suspend_tui`, before the child
/// spawns: set the scroll region to rows `2..=rows` (DECSTBM also homes the
/// cursor, hence the explicit `1;1H`), paint [`MARKER`] across row 1 in
/// reverse video, then park the cursor at the top of the region now carved
/// out for the child's own prompt. Empty when there's no row to spare.
pub fn enter(cols: u16, rows: u16) -> String {
    if rows < 2 {
        return String::new();
    }
    let text = crate::ui::display_truncate(MARKER, cols as usize);
    format!("\x1b[2;{rows}r\x1b[1;1H\x1b[7m{text}\x1b[K\x1b[0m\x1b[2;1H")
}

/// Sequence to write once the child exits, before `resume_tui`: drop the
/// scroll region back to the whole screen. `CSI r` with no parameters is
/// the full-screen default, so spyc's own repaint isn't left confined to
/// the child's carved-out area.
pub const fn leave() -> &'static str {
    "\x1b[r"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_carves_top_row_and_parks_cursor_below_it() {
        let seq = enter(60, 24);
        assert!(seq.starts_with("\x1b[2;24r"), "{seq:?}");
        assert!(seq.contains(MARKER));
        assert!(seq.ends_with("\x1b[2;1H"), "{seq:?}");
    }

    #[test]
    fn enter_fills_the_rest_of_the_row_before_resetting_style() {
        let seq = enter(60, 24);
        // Reverse video turns on before the text and the row is erased
        // (filling with that same attribute) before it turns back off.
        let on = seq.find("\x1b[7m").expect("reverse video on");
        let erase = seq.find("\x1b[K").expect("erase to end of line");
        let off = seq.find("\x1b[0m").expect("reverse video off");
        assert!(on < erase && erase < off, "{seq:?}");
    }

    #[test]
    fn enter_truncates_the_marker_to_the_terminal_width() {
        let seq = enter(5, 24);
        assert!(!seq.contains("exit or"), "{seq:?}");
    }

    #[test]
    fn enter_is_empty_below_two_rows() {
        assert_eq!(enter(80, 1), "");
        assert_eq!(enter(80, 0), "");
    }

    #[test]
    fn leave_resets_to_the_full_screen_region() {
        assert_eq!(leave(), "\x1b[r");
    }
}

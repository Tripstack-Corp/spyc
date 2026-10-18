//! The marker that opens spyc's `$` subshell (#686): one reverse-video line
//! naming the subshell, which scrolls away with the shell's own output. What
//! lasts is the window title and `SPYC_SUBSHELL=1`, which a prompt can show.
//! Nothing reserves screen space: the scroll region is terminal-wide and
//! survives a screen switch, so a reserved row confined every program the
//! subshell ran (`less` froze its top line, `clear` pinned a stale one).
//! `run_child_in_foreground` writes [`enter`] right after `suspend_tui`; it is
//! a pure string builder, so its bytes are unit-tested without a terminal.

/// Printed as the subshell's first line and reused as the window title while
/// it runs — `run_child_in_foreground` sets both from this one string.
pub const MARKER: &str = "🌶️ spyc subshell — exit or ^D to return";

/// The line to write right after `suspend_tui`, which leaves the cursor home
/// on a cleared screen: [`MARKER`] in reverse video, padded with spaces to the
/// full width (an erase-to-end-of-line fills with the default background on
/// xterm and ghostty, not with reverse video), then a line break for the
/// shell's prompt. Empty below two rows, where the prompt would scroll the
/// line away before it could be read.
pub fn enter(cols: u16, rows: u16) -> String {
    if rows < 2 {
        return String::new();
    }
    let cols = usize::from(cols);
    let text = crate::ui::display_truncate(MARKER, cols);
    let pad = cols.saturating_sub(crate::ui::display_width(text));
    format!("\x1b[7m{text}{}\x1b[0m\r\n", " ".repeat(pad))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The visible text of `seq`: every CSI sequence and line break removed.
    fn visible(seq: &str) -> String {
        let mut out = String::new();
        let mut chars = seq.chars();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                '\r' | '\n' => {}
                c => out.push(c),
            }
        }
        out
    }

    #[test]
    fn enter_is_one_reverse_video_line() {
        let seq = enter(60, 24);
        assert!(seq.starts_with("\x1b[7m"), "{seq:?}");
        assert!(seq.contains(MARKER), "{seq:?}");
        assert!(seq.ends_with("\x1b[0m\r\n"), "{seq:?}");
        assert_eq!(seq.matches('\n').count(), 1, "{seq:?}");
    }

    #[test]
    fn enter_pads_the_bar_to_the_full_width() {
        let marker = crate::ui::display_width(MARKER);
        for cols in [60, 120, marker, 5] {
            let shown = visible(&enter(u16::try_from(cols).expect("small"), 24));
            assert_eq!(
                crate::ui::display_width(&shown),
                cols,
                "{cols} columns: {shown:?}"
            );
        }
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

    /// The scroll region is terminal-wide and survives a screen switch, so a
    /// region reserved for the marker confines every program the subshell
    /// runs: `less` freezes its top line and can't scroll back, and `clear`
    /// pins a stale line above the region.
    #[test]
    fn entering_the_subshell_sets_no_scroll_region() {
        let seq = enter(80, 24);
        let sets_region = seq.match_indices("\x1b[").any(|(at, csi)| {
            let params = &seq[at + csi.len()..];
            let end = params
                .find(|c: char| !(c.is_ascii_digit() || c == ';'))
                .unwrap_or(params.len());
            params[end..].starts_with('r')
        });
        assert!(!sets_region, "{seq:?}");
    }
}

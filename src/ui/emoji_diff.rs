//! Keeps ratatui's diff from drifting the cursor right after an emoji
//! presentation sequence.
//!
//! When a cell holding a VS16 sequence (`🌶️`, `❤️`) changes, ratatui's diff
//! also rewrites the column the emoji covers if that column changed, for
//! terminals that draw the sequence one column wide. The crossterm backend
//! sends that write with no cursor move, since it directly follows the emoji.
//! A terminal that draws the sequence two wide, as Ghostty does, is already
//! past that column, so the write and the rest of its run land one column
//! right. The next frame blanks only the columns ratatui thinks it wrote, and
//! the run's last glyph stays on screen.
//!
//! [`pin_emoji_widths`] gives each such cell its own width as a
//! `ForcedWidth`, which sends it down the path a CJK glyph takes: the covered
//! column is skipped, so the next write starts with a cursor move. spyc counts
//! the sequence as two columns everywhere else (`display_width`, the pane
//! engine's mode 2027), so this is the same bet.

use std::num::NonZeroU16;

use ratatui::buffer::{Buffer, CellDiffOption, CellWidth};

/// Mark every wide VS16 cell in `buf` so the diff skips the column it covers.
/// Runs once per frame on the finished buffer. A cell that already carries a
/// diff option (an image's `Skip`) is left alone.
pub fn pin_emoji_widths(buf: &mut Buffer) {
    for cell in &mut buf.content {
        if cell.diff_option != CellDiffOption::None || !cell.symbol().contains('\u{fe0f}') {
            continue;
        }
        // The same measure, and the same `> 1`, as the diff's own VS16 test.
        if let Some(width) = NonZeroU16::new(cell.symbol().cell_width()).filter(|w| w.get() > 1) {
            cell.set_diff_option(CellDiffOption::ForcedWidth(width));
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use ratatui::backend::{Backend, CrosstermBackend};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use unicode_segmentation::UnicodeSegmentation;

    use crate::pane::engine::{Engine, TerminalScreen, Wide};
    use crate::pane::engine_ghostty::GhosttyEngine;

    const W: u16 = 40;

    /// The bytes the backend wrote, readable while it still owns the writer.
    #[derive(Clone, Default)]
    struct Tty(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

    impl std::io::Write for Tty {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// One row as a reader sees it: the glyph at each column that starts one,
    /// `None` where a wide glyph's right half sits. Both sides are read into
    /// this shape, with a blank spelled one way.
    type Row = Vec<Option<String>>;

    fn glyph(s: &str) -> String {
        if s.is_empty() { " " } else { s }.to_owned()
    }

    fn buffer_row(buf: &Buffer) -> Row {
        let mut row = Row::new();
        let mut covered = 0;
        for x in 0..W {
            if covered > 0 {
                covered -= 1;
                row.push(None);
                continue;
            }
            let sym = buf[(x, 0)].symbol();
            covered = crate::ui::display_width(sym).saturating_sub(1);
            row.push(Some(glyph(sym)));
        }
        row
    }

    fn screen_row(engine: &GhosttyEngine) -> Row {
        let screen = engine.screen();
        let mut text = String::new();
        (0..W)
            .map(|col| {
                let style = screen.cell_style(0, col).expect("inside the grid");
                if style.wide == Wide::Tail {
                    return None;
                }
                text.clear();
                screen.cell_text(0, col, &mut text);
                Some(glyph(&text))
            })
            .collect()
    }

    /// Draw each frame the way `Terminal::flush` does, a diff against the last
    /// one through the backend spyc runs, into a terminal that lays a VS16
    /// sequence out two columns wide. Returns (buffer, screen) after each.
    ///
    /// A frame is its graphemes, each with a style: a style-only change is
    /// what makes the diff rewrite an unchanged blank, and a background is
    /// what makes it force-clear a replaced wide glyph's columns.
    fn replay(frames: &[Vec<(&str, u8)>]) -> Vec<(Row, Row)> {
        let area = Rect::new(0, 0, W, 1);
        let tty = Tty::default();
        let mut backend = CrosstermBackend::new(tty.clone());
        let mut engine = <GhosttyEngine as Engine>::new(1, W, 0);
        let mut prev = Buffer::empty(area);
        let mut seen = Vec::new();
        for frame in frames {
            let mut next = Buffer::empty(area);
            let mut x = 0;
            for &(g, style) in frame {
                let style = match style % 3 {
                    0 => Style::default(),
                    1 => Style::default().add_modifier(Modifier::BOLD),
                    _ => Style::default().bg(Color::Blue),
                };
                (x, _) = next.set_stringn(x, 0, g, usize::from(W - x), style);
            }
            super::pin_emoji_widths(&mut next);
            backend
                .draw(prev.diff(&next).into_iter())
                .expect("a Vec never fails a write");
            engine.process(&tty.0.take());
            seen.push((buffer_row(&next), screen_row(&engine)));
            prev = next;
        }
        seen
    }

    /// Bold throughout, as a flash is.
    fn graphemes(s: &str) -> Vec<(&str, u8)> {
        s.graphemes(true).map(|g| (g, 1)).collect()
    }

    /// `gV` typed slowly enough for the chord hint `g-` to paint first, then
    /// any key that clears the flash: the chilli's covered column was written
    /// one to the right, the rest of the flash with it, and its `)` stayed.
    #[test]
    fn a_version_flash_over_the_chord_hint_clears_completely() {
        let frames = [
            graphemes("g-"),
            graphemes("\u{1f336}\u{fe0f} spyc <x.y.z> (6c513994)"),
            graphemes(""),
        ];
        for (i, (buffer, screen)) in replay(&frames).iter().enumerate() {
            assert_eq!(screen, buffer, "frame {i}");
        }
    }

    proptest! {
        /// The terminal shows what the buffer holds after every frame, for
        /// any run of narrow, CJK and VS16 glyphs over any other.
        #[test]
        fn the_screen_matches_the_buffer_after_every_frame(
            frames in prop::collection::vec(
                prop::collection::vec(
                    (
                        prop::sample::select(vec![
                            "a", "-", " ", "\u{3042}", "\u{2705}",
                            "\u{1f336}\u{fe0f}", "\u{2764}\u{fe0f}",
                        ]),
                        any::<u8>(),
                    ),
                    0..14,
                ),
                1..6,
            )
        ) {
            for (i, (buffer, screen)) in replay(&frames).iter().enumerate() {
                prop_assert_eq!(screen, buffer, "frame {}", i);
            }
        }
    }

    /// The tests above call the pass directly, so they stay green if the frame
    /// loop stops calling it.
    #[test]
    fn the_frame_loop_runs_the_pass() {
        let run = crate::guard_support::production_half(include_str!("../app/run.rs"));
        assert!(
            run.contains("emoji_diff::pin_emoji_widths(frame.buffer_mut())"),
            "the draw closure in app/run.rs must pin emoji widths on every frame"
        );
    }
}

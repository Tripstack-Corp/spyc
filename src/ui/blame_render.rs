//! Pure renderer: a [`BlameModel`] → styled `ratatui` lines.
//!
//! The in-house replacement for piping `git blame` bytes through the pager.
//! Each file line gets a fixed-width gutter — short commit id, author, date —
//! followed by the syntax-highlighted line content. The gutter is colored by a
//! stable hash of the commit id, so consecutive lines from the same commit
//! share a color (mirroring `git blame --color-lines`, making churn visible at
//! a glance). Line numbers come free from the pager's own line-number toggle.
//!
//! Pure: `model + &Theme → lines`, no IO, no gix. Wired into the pager by
//! PR 8b (via the git-view session in `app/git_view_session.rs`).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::git::model::{BlameLine, BlameModel};
use crate::ui::theme::Theme;
use crate::ui::{display_pad_right, display_truncate};

/// Author-name column width in the blame gutter.
const AUTHOR_W: usize = 12;
/// Date column width (`YYYY-MM-DD`).
const DATE_W: usize = 10;

/// Distinguishable hues for per-commit gutter coloring (Tokyo-Night palette).
/// A commit's id hashes to one of these; same commit → same color.
const BLAME_PALETTE: [Color; 6] = [
    Color::Rgb(0x7a, 0xa2, 0xf7), // blue
    Color::Rgb(0x9e, 0xce, 0x6a), // green
    Color::Rgb(0xe0, 0xaf, 0x68), // amber
    Color::Rgb(0xbb, 0x9a, 0xf7), // lavender
    Color::Rgb(0x73, 0xda, 0xca), // teal
    Color::Rgb(0xf7, 0x76, 0x8e), // red
];

/// Render a whole-file blame to styled lines: one line per file line, each a
/// `short-id author date │ <highlighted content>` row.
pub fn render_blame(model: &BlameModel, theme: &Theme) -> Vec<Line<'static>> {
    if model.lines.is_empty() {
        return vec![Line::styled("No blame data.", theme.diff_meta_style())];
    }

    // Highlight the full file once (syntect is stateful across lines).
    let content = model
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let hl = crate::ui::syntax::highlight_to_lines(&model.path, &content);

    let mut out = Vec::with_capacity(model.lines.len());
    for (i, bl) in model.lines.iter().enumerate() {
        let mut spans = vec![blame_gutter(bl, theme)];
        match hl.as_ref().and_then(|lines| lines.get(i)) {
            Some(line) => spans.extend(line.spans.iter().cloned()),
            None => spans.push(Span::raw(bl.text.clone())),
        }
        out.push(Line::from(spans));
    }
    if model.truncated {
        out.push(Line::styled(
            "… blame truncated (file too large to display in full)",
            theme.diff_meta_style(),
        ));
    }
    out
}

/// The fixed-width `short-id author date │ ` gutter span for one blame line.
fn blame_gutter(bl: &BlameLine, theme: &Theme) -> Span<'static> {
    let author = display_pad_right(display_truncate(&bl.author, AUTHOR_W), AUTHOR_W);
    let date = display_pad_right(display_truncate(&bl.date, DATE_W), DATE_W);
    Span::styled(
        format!("{:<7} {author} {date} │ ", bl.short_id),
        blame_style(theme, &bl.short_id),
    )
}

/// Gutter style for a commit: a palette color keyed by the id hash, or a dim
/// terminal default in `mono`.
fn blame_style(theme: &Theme, short_id: &str) -> Style {
    if theme.mono {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(blame_color(short_id))
    }
}

/// Map a commit short id to a stable palette color via FNV-1a, so all lines
/// from one commit share a hue.
fn blame_color(short_id: &str) -> Color {
    let mut hash: u32 = 0x811c_9dc5;
    for b in short_id.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    BLAME_PALETTE[hash as usize % BLAME_PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::render_blame;
    use crate::git::model::{BlameLine, BlameModel};
    use crate::ui::theme::Theme;
    use ratatui::text::Line;

    fn line(short: &str, author: &str, date: &str, no: u32, text: &str) -> BlameLine {
        BlameLine {
            short_id: short.into(),
            author: author.into(),
            date: date.into(),
            lineno: no,
            text: text.into(),
        }
    }

    fn sample() -> BlameModel {
        BlameModel {
            path: "f.rs".into(),
            lines: vec![
                line("abc1234", "Ada", "2026-01-02", 1, "fn main() {"),
                line("abc1234", "Ada", "2026-01-02", 2, "    let x = 1;"),
                line("def5678", "Bob", "2026-03-04", 3, "    let y = 2;"),
                line("abc1234", "Ada", "2026-01-02", 4, "}"),
            ],
            truncated: false,
        }
    }

    fn text(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn blame_gutter_layout() {
        let out = render_blame(&sample(), &Theme::default());
        assert_eq!(
            text(&out),
            "abc1234 Ada          2026-01-02 │ fn main() {\n\
             abc1234 Ada          2026-01-02 │     let x = 1;\n\
             def5678 Bob          2026-03-04 │     let y = 2;\n\
             abc1234 Ada          2026-01-02 │ }"
        );
    }

    #[test]
    fn gutter_color_groups_by_commit() {
        let theme = Theme::default();
        let out = render_blame(&sample(), &theme);
        let gutter_fg = |i: usize| out[i].spans[0].style.fg;
        // Lines 0, 1, 3 are Ada's commit → same color; line 2 is Bob's → differs.
        assert_eq!(gutter_fg(0), gutter_fg(1));
        assert_eq!(gutter_fg(0), gutter_fg(3));
        assert_ne!(gutter_fg(0), gutter_fg(2));
    }

    #[test]
    fn mono_gutter_is_dim_no_color() {
        let theme = Theme::default().toggled(); // mono
        let out = render_blame(&sample(), &theme);
        assert_eq!(out[0].spans[0].style.fg, None);
    }

    #[test]
    fn empty_blame_message() {
        let theme = Theme::default();
        let out = render_blame(
            &BlameModel {
                path: "x".into(),
                lines: Vec::new(),
                truncated: false,
            },
            &theme,
        );
        assert_eq!(text(&out), "No blame data.");
    }

    #[test]
    fn truncated_model_appends_note() {
        let mut model = sample();
        model.truncated = true;
        let out = render_blame(&model, &Theme::default());
        assert_eq!(out.len(), 5, "4 lines + 1 truncation note");
        let last = out.last().unwrap();
        let note: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            note.contains("truncated"),
            "truncation note missing: {note:?}"
        );
    }
}

//! Draws a [`crate::fs::disk_tree`] layout under the directory's own
//! long-listing lines: one header line, then a line per row with its sizes,
//! its share of its parent, its file count, and its name on the tree's
//! branches.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::Theme;
use crate::fs::disk_tree::{Row, RowKind, Usage};

/// The columns before the names, and the header over them.
const HEADER: &str = "   SIZE  ON DISK  SHARE              FILES  ";

/// Cells in a share bar.
const BAR: usize = 8;

/// `listing`, its first line the header, then a blank line, the tree's
/// header line and one line per row.
pub fn render(listing: &[String], rows: &[Row], theme: &Theme) -> Vec<Line<'static>> {
    let style = |colour| {
        if theme.mono {
            Style::default()
        } else {
            Style::default().fg(colour)
        }
    };
    let mut out = Vec::with_capacity(listing.len() + rows.len() + 2);
    for (i, line) in listing.iter().enumerate() {
        let colour = if i == 0 {
            theme.diff_meta_fg
        } else {
            theme.file
        };
        let mut style = style(colour);
        if i == 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        out.push(Line::from(Span::styled(line.clone(), style)));
    }
    if !listing.is_empty() {
        out.push(Line::default());
    }
    out.push(Line::from(vec![
        Span::styled(
            HEADER,
            style(theme.diff_meta_fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "NAME",
            style(theme.diff_meta_fg).add_modifier(Modifier::BOLD),
        ),
    ]));
    for row in rows {
        let columns = format!(
            "{:>7}  {:>7}  {:>5} ",
            super::format_size(row.usage.apparent),
            super::format_size(row.usage.disk),
            percent(row.share),
        );
        let mut spans = vec![
            Span::styled(columns, style(theme.file)),
            Span::styled(bar(row.share), style(theme.pick)),
            Span::styled(
                format!(" {:>9}  ", count(row.usage.files)),
                style(theme.file),
            ),
            Span::styled(branches(row), style(theme.other)),
        ];
        match row.kind {
            RowKind::Dir { open, empty, .. } => {
                spans.push(Span::styled(format!("{}/", row.name), style(theme.dir)));
                if !open && !empty {
                    spans.push(Span::styled(" ▸", style(theme.other)));
                }
            }
            RowKind::File => spans.push(Span::styled(row.name.clone(), style(theme.file))),
            RowKind::More { entries } => spans.push(Span::styled(
                format!("… {} more", count(entries)),
                style(theme.other),
            )),
        }
        out.push(Line::from(spans));
    }
    out
}

/// The pager's title: where, how much, and how deep it is open.
pub fn title(
    root: &str,
    total: Usage,
    unreadable: u64,
    depth: Option<usize>,
    done: bool,
) -> String {
    let files = count(total.files);
    let size = if done {
        let disk = super::format_size(total.disk);
        format!("{disk} on disk, {files} files")
    } else {
        format!("scanning… {files} files")
    };
    let unread = if unreadable > 0 {
        format!(", {} unreadable", count(unreadable))
    } else {
        String::new()
    };
    let depth = depth.map_or_else(
        || "every level (zm)".to_string(),
        |depth| format!("depth {depth} (zr / zm)"),
    );
    format!("tree — {root} — {size}{unread} — {depth}")
}

/// A share in thousandths as a percentage: a decimal under 10%.
fn percent(share: u32) -> String {
    if share >= 100 {
        format!("{}%", share / 10)
    } else {
        format!("{}.{}%", share / 10, share % 10)
    }
}

/// A share in thousandths as a bar of [`BAR`] cells, in eighths.
fn bar(share: u32) -> String {
    const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    let eighths = (share.min(1000) as usize * BAR * 8).div_ceil(1000);
    let mut out = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        out.push(EIGHTHS[eighths % 8 - 1]);
    }
    let cells = out.chars().count();
    out.push_str(&" ".repeat(BAR - cells));
    out
}

/// A count with its thousands set apart: `1,234,567`.
fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The branch lines before a row's name.
fn branches(row: &Row) -> String {
    if row.parent.is_none() {
        return String::new();
    }
    let mut out = String::new();
    for &goes_on in &row.rails {
        out.push_str(if goes_on { "│  " } else { "   " });
    }
    out.push_str(if row.last { "└─ " } else { "├─ " });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn counts_and_shares_read_as_people_write_them() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(percent(1000), "100%");
        assert_eq!(percent(942), "94%");
        assert_eq!(percent(42), "4.2%");
        assert_eq!(bar(1000), "████████");
        assert_eq!(bar(0), "        ");
        assert_eq!(bar(500), "████    ");
        assert_eq!(bar(1).trim_end(), "▏", "a sliver still shows");
    }

    #[test]
    fn a_row_shows_its_sizes_share_files_and_branches() {
        let usage = Usage {
            apparent: 3 * 1024,
            disk: 4 * 1024,
            files: 1_500,
        };
        let row = Row {
            rails: vec![true],
            last: true,
            parent: Some(1),
            kind: RowKind::Dir {
                id: 2,
                open: false,
                empty: false,
            },
            name: "target".into(),
            usage,
            share: 500,
        };
        let listing = ["NAME  MODE".to_string(), "src/  drwxr-xr-x".to_string()];
        let lines = render(&listing, &[row], &Theme::default());
        assert_eq!(text(&lines[0]), "NAME  MODE");
        assert_eq!(text(&lines[1]), "src/  drwxr-xr-x");
        assert_eq!(text(&lines[2]), "");
        assert!(text(&lines[3]).ends_with("NAME"));
        assert_eq!(
            text(&lines[4]),
            "   3.0K     4.0K    50% ████         1,500  │  └─ target/ ▸"
        );
    }

    #[test]
    fn the_title_says_how_far_the_scan_has_got_and_how_deep_it_is_open() {
        let total = Usage {
            apparent: 10,
            disk: 2048,
            files: 12_345,
        };
        assert_eq!(
            title("~/src", total, 0, Some(2), false),
            "tree — ~/src — scanning… 12,345 files — depth 2 (zr / zm)"
        );
        assert_eq!(
            title("~/src", total, 3, None, true),
            "tree — ~/src — 2.0K on disk, 12,345 files, 3 unreadable — every level (zm)"
        );
    }
}

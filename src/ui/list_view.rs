//! The multi-column file list that is the heart of spyc's UI.
//!
//! Layout is **column-major** (`ls -C` / spy style): items fill the first
//! column top-to-bottom before spilling into the next column. Rows per
//! column always equals the screen height, so the first column is used
//! fully before the second begins. When the columns don't all fit across
//! the screen the remainder paginates.
//!
//! Each column's width is the longest display string in that column plus
//! the 2-char marker prefix, so one long filename only widens its own
//! column. Column widths are computed per page.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};

use crate::fs::EntryKind;
use crate::ui::theme::Theme;

/// Git status for a single file in the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitFileStatus {
    #[default]
    Clean,
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
    Conflicted,
}

pub struct Row {
    pub display: String,
    pub kind: EntryKind,
    pub picked: bool,
    pub taken: bool,
    pub git_status: GitFileStatus,
}

pub struct ListView<'a> {
    pub rows: &'a [Row],
    pub cursor: usize,
    pub view_top: usize,
    pub empty_marker: bool,
    pub focused: bool,
    pub theme: &'a Theme,
}

/// Geometry of the rendered grid for the currently visible page.
#[derive(Debug, Clone)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    /// Width of each column (includes the 2-char marker prefix).
    pub col_widths: Vec<u16>,
}

impl Grid {
    pub const fn items_per_page(&self) -> usize {
        self.cols as usize * self.rows as usize
    }

    /// Left-edge x-coordinate (relative to list area) of each column.
    pub fn col_x_offsets(&self, gap: u16) -> Vec<u16> {
        let mut xs = Vec::with_capacity(self.col_widths.len());
        let mut x = 0u16;
        for (i, w) in self.col_widths.iter().enumerate() {
            xs.push(x);
            x = x.saturating_add(*w);
            if i + 1 < self.col_widths.len() {
                x = x.saturating_add(gap);
            }
        }
        xs
    }
}

const COL_GAP: u16 = 2;
const MARKER_W: u16 = 2;
const MIN_NAME_WIDTH: u16 = 8;

impl ListView<'_> {
    /// Compute the grid for the page that starts at `self.view_top`.
    pub fn grid(&self, area: Rect) -> Grid {
        let height = area.height.max(1) as usize;
        let width = area.width as usize;

        if self.rows.is_empty() {
            return Grid {
                cols: 1,
                rows: height as u16,
                col_widths: vec![MIN_NAME_WIDTH + MARKER_W],
            };
        }

        // Columns are always R = screen_height rows deep. Walk columns
        // starting at view_top, adding columns while the total width still
        // fits on screen. What doesn't fit paginates.
        let rows_per_col = height;
        let tail_start = self.view_top.min(self.rows.len());
        let tail = &self.rows[tail_start..];
        let widths: Vec<usize> = tail.iter().map(|row| row.display.chars().count()).collect();
        let count = widths.len();

        let mut col_widths: Vec<u16> = Vec::new();
        let mut total = 0usize;
        let mut col_idx = 0usize;
        loop {
            let start = col_idx * rows_per_col;
            if start >= count {
                break;
            }
            let end = (start + rows_per_col).min(count);
            let col_w = widths[start..end].iter().copied().max().unwrap_or(0) + MARKER_W as usize;
            let addition = col_w + if col_idx > 0 { COL_GAP as usize } else { 0 };
            if total + addition > width {
                break;
            }
            total += addition;
            col_widths.push(col_w as u16);
            col_idx += 1;
        }
        if col_idx == 0 {
            // Screen narrower than one cell. Render one clamped column.
            let widest = widths
                .iter()
                .copied()
                .max()
                .unwrap_or(MIN_NAME_WIDTH as usize);
            col_widths.push((widest + MARKER_W as usize).min(width.max(1)) as u16);
            col_idx = 1;
        }
        Grid {
            cols: col_idx as u16,
            rows: rows_per_col as u16,
            col_widths,
        }
    }
}

impl Widget for ListView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.rows.is_empty() {
            if self.empty_marker {
                let style = Style::default().fg(self.theme.empty_marker);
                buf.set_string(area.x, area.y, "<Empty>", style);
            }
            return;
        }

        let grid = self.grid(area);
        let x_offsets = grid.col_x_offsets(COL_GAP);
        let rows_per_col = grid.rows as usize;
        let per_page = grid.items_per_page();

        let start = self.view_top.min(self.rows.len());
        let end = (start + per_page).min(self.rows.len());
        let slice = &self.rows[start..end];

        for (i, row) in slice.iter().enumerate() {
            let col_idx = i / rows_per_col;
            let row_idx = i % rows_per_col;
            if col_idx >= grid.cols as usize {
                break;
            }
            let cell_w = grid.col_widths[col_idx];
            let x = area.x + x_offsets[col_idx];
            let y = area.y + row_idx as u16;

            let marker = if row.picked {
                '*'
            } else if row.taken {
                '+'
            } else {
                ' '
            };
            let marker_style = if row.picked {
                self.theme.pick_style()
            } else if row.taken {
                self.theme.take_style()
            } else {
                Style::default()
            };

            let name_style = row_style(row.kind, row.git_status, self.theme);
            let highlighted = (start + i) == self.cursor;
            let (marker_style, name_style) = if highlighted {
                // On the cursor row, force a bright white foreground so the
                // text remains legible on the saturated cursor bar and you
                // can see the selected row from across the room.
                // When unfocused (pane has focus), dim the cursor bar.
                let bg = if self.focused {
                    self.theme.cursor_bg
                } else {
                    self.theme.cursor_bg_dim
                };
                let bold = if self.focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                };
                (
                    marker_style.bg(bg).add_modifier(bold),
                    Style::default()
                        .fg(self.theme.cursor_fg)
                        .bg(bg)
                        .add_modifier(bold),
                )
            } else {
                (marker_style, name_style)
            };

            buf.set_string(x, y, format!("{marker} "), marker_style);

            let name_w = cell_w.saturating_sub(MARKER_W) as usize;
            let drawn: String = row.display.chars().take(name_w).collect();
            let drawn_chars = drawn.chars().count() as u16;
            buf.set_string(x + MARKER_W, y, &drawn, name_style);

            let used = MARKER_W + drawn_chars;
            if used < cell_w {
                let pad: String = " ".repeat((cell_w - used) as usize);
                buf.set_string(x + used, y, &pad, name_style);
            }
        }
    }
}

fn row_style(kind: EntryKind, git: GitFileStatus, theme: &Theme) -> Style {
    let base = match kind {
        EntryKind::Dir => theme.dir_style(),
        EntryKind::Executable => theme.exec_style(),
        EntryKind::Symlink => theme.symlink_style(),
        EntryKind::File => theme.file_style(),
        EntryKind::Other => theme.other_style(),
    };
    // Tint based on git status — override fg for modified/new files.
    match git {
        GitFileStatus::Clean => base,
        GitFileStatus::Modified => base.fg(theme.pick),  // amber
        GitFileStatus::Added | GitFileStatus::Untracked => base.fg(theme.exec), // green
        GitFileStatus::Deleted => base.fg(theme.cursor_bg).add_modifier(Modifier::DIM), // red-ish dim
        GitFileStatus::Renamed => base.fg(theme.symlink), // lavender
        GitFileStatus::Conflicted => base.fg(theme.cursor_bg).add_modifier(Modifier::BOLD), // red bold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str) -> Row {
        Row {
            display: name.to_string(),
            kind: EntryKind::File,
            picked: false,
            taken: false,
            git_status: GitFileStatus::Clean,
        }
    }

    fn grid_for(names: &[&str], w: u16, h: u16) -> Grid {
        let theme = Theme::default();
        let rows: Vec<Row> = names.iter().map(|n| row(n)).collect();
        let lv = ListView {
            rows: &rows,
            cursor: 0,
            view_top: 0,
            empty_marker: false,
            focused: true,
            theme: &theme,
        };
        lv.grid(Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        })
    }

    #[test]
    fn rows_always_match_screen_height() {
        // 12 items on a 10-row screen → 2 columns of up to 10 each.
        let names: Vec<String> = (0..12).map(|i| format!("f{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let g = grid_for(&refs, 80, 10);
        assert_eq!(g.rows, 10);
        assert_eq!(g.cols, 2);
    }

    #[test]
    fn few_items_use_one_tall_column() {
        // 5 items on a 30-row screen should use one column (fill vertically).
        let g = grid_for(&["a", "b", "c", "d", "e"], 80, 30);
        assert_eq!(g.cols, 1);
        assert_eq!(g.rows, 30);
    }

    #[test]
    fn one_long_name_does_not_stretch_other_columns() {
        // With R=4, items split into [a,b,c,d] and [long,e,f,g]. The column
        // containing the long name should be much wider than the other.
        let g = grid_for(
            &[
                "a",
                "b",
                "c",
                "d",
                "this_is_a_very_long_filename_indeed",
                "e",
                "f",
                "g",
            ],
            80,
            4,
        );
        assert_eq!(g.rows, 4);
        assert!(g.col_widths.len() >= 2);
        let short_col = g.col_widths.iter().min().copied().unwrap();
        let long_col = g.col_widths.iter().max().copied().unwrap();
        assert!(
            long_col > short_col + 5,
            "expected long col to be wider: {:?}",
            g.col_widths
        );
    }

    #[test]
    fn narrow_screen_falls_back_to_one_column() {
        let g = grid_for(&["apple", "banana", "cherry"], 10, 10);
        assert_eq!(g.cols, 1);
    }
}

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

/// One half of a git porcelain XY pair — either the index/staged side
/// or the working-tree/unstaged side. `Untracked` is special: it
/// applies to a file as a whole (porcelain `??`), not to either side
/// in isolation, so it lives on `GitFileStatus` itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChange {
    Modified,
    Added,
    Deleted,
    Renamed,
    Conflicted,
}

/// Git status for a single file in the listing. Models the full XY
/// porcelain shape so the renderer can show "staged but also further
/// modified", "staged-only", "unstaged-only", etc. as distinct states
/// — previously collapsed to one marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GitFileStatus {
    pub staged: Option<GitChange>,
    pub unstaged: Option<GitChange>,
    pub untracked: bool,
}

impl GitFileStatus {
    pub const fn clean() -> Self {
        Self {
            staged: None,
            unstaged: None,
            untracked: false,
        }
    }

    pub const fn is_clean(self) -> bool {
        self.staged.is_none() && self.unstaged.is_none() && !self.untracked
    }

    /// Convenience for callers (tests, parent-dir aggregation) that
    /// just want a generic "this row has changes" marker.
    pub const fn unstaged(change: GitChange) -> Self {
        Self {
            staged: None,
            unstaged: Some(change),
            untracked: false,
        }
    }

    /// True when either half marks this path as deleted — used to surface
    /// git-deleted files (gone from disk) as struck-through "ghost" rows.
    pub const fn is_deleted(self) -> bool {
        matches!(self.staged, Some(GitChange::Deleted))
            || matches!(self.unstaged, Some(GitChange::Deleted))
    }
}

pub struct Row {
    pub display: String,
    pub kind: EntryKind,
    pub picked: bool,
    pub taken: bool,
    /// A git-deleted file that no longer exists on disk — rendered
    /// struck-through. The path is its would-be location (for restore).
    pub deleted: bool,
    pub git_status: GitFileStatus,
    /// True while this entry is among the targets of an active
    /// `RemoveConfirm` prompt. Drives the warning-color row
    /// highlight: the user sees exactly which files the next `y`
    /// keystroke will affect. Always `false` outside the prompt.
    pub pending_delete: bool,
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

/// MVU Phase 5: the geometry subset of [`Grid`] that update-time cursor /
/// page math needs — `cols` × `rows_per_col` and the derived
/// `items_per_page`. The render-only `col_widths` / `col_x_offsets` are
/// deliberately excluded so they never leak into `AppState` as read-back
/// state (the "render cache read back as state" smell). Render keeps the
/// full `Grid` local for drawing and stores only this slice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridDims {
    pub cols: u16,
    pub rows_per_col: u16,
}

impl GridDims {
    pub const fn items_per_page(self) -> usize {
        self.cols as usize * self.rows_per_col as usize
    }
}

impl Grid {
    pub const fn items_per_page(&self) -> usize {
        self.cols as usize * self.rows as usize
    }

    /// The update-time geometry slice (drops the render-only column
    /// widths). MVU Phase 5.
    pub const fn dims(&self) -> GridDims {
        GridDims {
            cols: self.cols,
            rows_per_col: self.rows,
        }
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
        let count = tail.len();

        // Max display width of column `col`'s rows, measured lazily. Only the
        // columns that actually fit on screen are measured below, so the work
        // is O(items_per_page + one overflow column) — NOT O(rows to the end
        // of the listing). The old code collected display_width for *every*
        // tail row on *every* frame: a 100k-entry dir near the top meant 100k
        // width calls + an ~800 KB Vec per frame (typing jank / agent-stream
        // redraw cost that grew linearly with directory size).
        let col_max_width = |col: usize| -> usize {
            let start = col * rows_per_col;
            let end = (start + rows_per_col).min(count);
            tail[start..end]
                .iter()
                .map(|r| super::display_width(&r.display))
                .max()
                .unwrap_or(0)
        };

        let mut col_widths: Vec<u16> = Vec::new();
        let mut total = 0usize;
        let mut col_idx = 0usize;
        loop {
            let start = col_idx * rows_per_col;
            if start >= count {
                break;
            }
            let col_w = col_max_width(col_idx) + MARKER_W as usize;
            let addition = col_w + if col_idx > 0 { COL_GAP as usize } else { 0 };
            if total + addition > width {
                break;
            }
            total += addition;
            col_widths.push(col_w as u16);
            col_idx += 1;
        }
        if col_idx == 0 {
            // Screen narrower than one cell. Render one clamped column —
            // measured from the first column's rows only (what's drawn), not
            // the whole tail.
            let widest = col_max_width(0).max(MIN_NAME_WIDTH as usize);
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

        // When the list isn't the input target (pane focused), fade
        // the non-cursor rows so the user can tell where focus lives
        // at a glance. The cursor row already gets its own dimmed
        // treatment via `cursor_bg_dim` below; this dims everything
        // else. SGR 2 (Modifier::DIM) is rendered as ~50% lightness
        // by every supported terminal.
        let dim = if self.focused {
            Modifier::empty()
        } else {
            Modifier::DIM
        };

        for (i, row) in slice.iter().enumerate() {
            let col_idx = i / rows_per_col;
            let row_idx = i % rows_per_col;
            if col_idx >= grid.cols as usize {
                break;
            }
            let cell_w = grid.col_widths[col_idx];
            let x = area.x + x_offsets[col_idx];
            let y = area.y + row_idx as u16;

            // Pick / take override the git pair entirely (single-char
            // marker + space). Otherwise we render the staged + unstaged
            // pair from `git_marker_pair`, with each char carrying its
            // own style so green-staged-vs-amber-unstaged is legible at
            // a glance.
            let (chars_styles, single_marker) = if row.picked {
                (
                    [(' ', Style::default()); 2],
                    Some(('*', self.theme.pick_style())),
                )
            } else if row.taken {
                (
                    [(' ', Style::default()); 2],
                    Some(('+', self.theme.take_style())),
                )
            } else {
                (git_marker_pair(row.git_status, self.theme), None)
            };

            let name_style = row_style(row.kind, row.git_status, self.theme);
            let highlighted = (start + i) == self.cursor;

            // Cursor row — force the bright bar bg + fg over both
            // marker cells and the filename. DIM doesn't apply to the
            // cursor row; the existing `cursor_bg_dim` handles the
            // unfocused case.
            //
            // A row pending delete (active `RemoveConfirm` prompt)
            // overrides the cursor bg with the warning color so the
            // user sees the consequence of the next `y` keystroke.
            // The warning wins even when the row is also the cursor.
            let (cursor_bg, cursor_bold) = if row.pending_delete {
                (Some(self.theme.delete_warning), Modifier::BOLD)
            } else if highlighted {
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
                (Some(bg), bold)
            } else {
                (None, Modifier::empty())
            };

            let apply_row_style = |s: Style| -> Style {
                if let Some(bg) = cursor_bg {
                    s.bg(bg).add_modifier(cursor_bold)
                } else {
                    s.add_modifier(dim)
                }
            };

            // Marker cells: two chars, two independent styles.
            if let Some((marker, marker_style)) = single_marker {
                // Pick / take: write the single char + a space.
                let style = apply_row_style(marker_style);
                buf.set_string(x, y, format!("{marker} "), style);
            } else {
                let [(c0, s0), (c1, s1)] = chars_styles;
                let s0 = apply_row_style(s0);
                let s1 = apply_row_style(s1);
                buf.set_string(x, y, c0.to_string(), s0);
                buf.set_string(x + 1, y, c1.to_string(), s1);
            }

            let final_name_style = if highlighted || row.pending_delete {
                // Pending-delete rows always force the bright fg +
                // bold treatment over the warning bg so the
                // filename stays readable against the strong color.
                // Cursor rows do the same against the cursor bg.
                Style::default()
                    .fg(self.theme.cursor_fg)
                    .bg(cursor_bg.unwrap_or_default())
                    .add_modifier(cursor_bold)
            } else {
                name_style.add_modifier(dim)
            };

            let name_w = cell_w.saturating_sub(MARKER_W) as usize;
            let drawn = super::display_truncate(&row.display, name_w);
            let drawn_w = super::display_width(drawn) as u16;
            // A git-deleted "ghost" row (the file is gone from disk) strikes
            // through ONLY the filename glyphs — not the trailing pad — so the
            // line stops at the name. SGR 9 on the pad spaces would otherwise
            // run the strikethrough out to the column edge.
            let drawn_style = if row.deleted {
                final_name_style.add_modifier(Modifier::CROSSED_OUT)
            } else {
                final_name_style
            };
            buf.set_string(x + MARKER_W, y, drawn, drawn_style);

            let used = MARKER_W + drawn_w;
            if used < cell_w {
                let pad: String = " ".repeat((cell_w - used) as usize);
                buf.set_string(x + used, y, &pad, final_name_style);
            }
        }
    }
}

fn row_style(kind: EntryKind, _git: GitFileStatus, theme: &Theme) -> Style {
    match kind {
        EntryKind::Dir => theme.dir_style(),
        EntryKind::Executable => theme.exec_style(),
        EntryKind::Symlink => theme.symlink_style(),
        EntryKind::File => theme.file_style(),
        EntryKind::Other => theme.other_style(),
    }
}

/// Glyph + color for a single XY half. Returns the same per-change
/// glyph for either side; the *position* in the marker column tells
/// the user which half it is (column 0 = staged, column 1 = unstaged).
/// Color encodes the kind (modified/added/etc.).
fn change_glyph(change: GitChange, theme: &Theme) -> (char, Style) {
    match change {
        GitChange::Modified => ('~', Style::default().fg(theme.pick)),
        GitChange::Added => ('+', Style::default().fg(theme.exec)),
        GitChange::Deleted => ('-', Style::default().fg(theme.cursor_bg)),
        GitChange::Renamed => ('>', Style::default().fg(theme.symlink)),
        GitChange::Conflicted => (
            '!',
            Style::default()
                .fg(theme.cursor_bg)
                .add_modifier(Modifier::BOLD),
        ),
    }
}

/// Two-character marker for the left gutter: column 0 = staged side,
/// column 1 = unstaged side. Mirrors `git status -s`. Untracked files
/// have no staged side (porcelain `??`) and render as ` ?` so the `?`
/// lines up with the unstaged column.
///
/// Each char carries its own style — staged and unstaged colors don't
/// have to match, which is the whole point: at a glance the user can
/// tell `M ` (ready to commit) from ` M` (still working) from `MM`
/// (partially staged + further edits).
fn git_marker_pair(git: GitFileStatus, theme: &Theme) -> [(char, Style); 2] {
    if git.is_clean() {
        return [(' ', Style::default()), (' ', Style::default())];
    }
    if git.untracked {
        // Untracked: no staged side; the `?` in the unstaged column.
        return [
            (' ', Style::default()),
            ('?', Style::default().fg(theme.exec)),
        ];
    }
    let staged = git
        .staged
        .map_or((' ', Style::default()), |c| change_glyph(c, theme));
    let unstaged = git
        .unstaged
        .map_or((' ', Style::default()), |c| change_glyph(c, theme));
    [staged, unstaged]
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
            deleted: false,
            git_status: GitFileStatus::clean(),
            pending_delete: false,
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

    #[test]
    fn offscreen_wide_row_does_not_affect_visible_columns() {
        // height 2 → 2 rows/col. Only column 0 ([aa, bb]) fits in width 20;
        // the huge name lands in column 1, which doesn't fit. The visible
        // column's width must come from column 0 alone — and with lazy
        // measurement the huge (off-screen) name is never even measured.
        let huge = "x".repeat(100);
        let g = grid_for(&["aa", "bb", &huge, "cc"], 20, 2);
        assert_eq!(g.cols, 1);
        assert_eq!(g.col_widths, vec![2 + MARKER_W]); // "aa"/"bb" = 2 cols + marker
    }

    // ── snapshot tests (TestBackend) ──────────────────────────────
    //
    // These capture the rendered glyphs (no styling) for the file list
    // at a fixed geometry. Mirrors the pattern in `ui::status::tests`:
    // a regression that changes layout, marker prefixes, or row
    // truncation will diff visibly.

    use ratatui::{Terminal, backend::TestBackend};

    fn render_list_to_string(rows: &[Row], cursor: usize, focused: bool, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, w, h);
                let lv = ListView {
                    rows,
                    cursor,
                    view_top: 0,
                    empty_marker: true,
                    focused,
                    theme: &theme,
                };
                f.render_widget(lv, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf.cell((x, y)).map_or(" ", |c| c.symbol()));
            }
            out.push('\n');
        }
        out.trim_end().to_string()
    }

    #[test]
    fn snapshot_list_basic_focused() {
        let rows = vec![row("README.md"), row("Cargo.toml"), row("src")];
        let out = render_list_to_string(&rows, 0, true, 30, 4);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_list_picks_and_takes() {
        let rows = vec![
            Row {
                display: "alpha.txt".into(),
                kind: EntryKind::File,
                picked: true,
                taken: false,
                deleted: false,
                git_status: GitFileStatus::clean(),
                pending_delete: false,
            },
            Row {
                display: "beta.txt".into(),
                kind: EntryKind::File,
                picked: false,
                taken: true,
                deleted: false,
                git_status: GitFileStatus::clean(),
                pending_delete: false,
            },
            Row {
                display: "gamma".into(),
                kind: EntryKind::Dir,
                picked: false,
                taken: false,
                deleted: false,
                git_status: GitFileStatus::unstaged(GitChange::Modified),
                pending_delete: false,
            },
        ];
        let out = render_list_to_string(&rows, 1, true, 30, 4);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_list_empty() {
        let rows: Vec<Row> = Vec::new();
        let out = render_list_to_string(&rows, 0, true, 30, 4);
        insta::assert_snapshot!(out);
    }

    /// A deleted "ghost" row strikes through ONLY the filename glyphs, not the
    /// trailing pad — SGR 9 on the pad would run the line out to the column
    /// edge (the bug the first cut shipped). Renders a non-cursor deleted row
    /// and checks the CROSSED_OUT modifier per cell.
    #[test]
    fn deleted_row_strikes_filename_only_not_pad() {
        use ratatui::style::Modifier;
        let rows = vec![
            row("keep.md"),
            Row {
                display: "gone.txt".into(), // 8 chars at x = MARKER_W(2)..10
                kind: EntryKind::File,
                picked: false,
                taken: false,
                deleted: true,
                git_status: GitFileStatus {
                    staged: None,
                    unstaged: Some(GitChange::Deleted),
                    untracked: false,
                },
                pending_delete: false,
            },
        ];
        let backend = TestBackend::new(30, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::default();
        terminal
            .draw(|f| {
                let lv = ListView {
                    rows: &rows,
                    cursor: 0, // cursor on keep.md, so the ghost row isn't highlighted
                    view_top: 0,
                    empty_marker: true,
                    focused: true,
                    theme: &theme,
                };
                f.render_widget(lv, Rect::new(0, 0, 30, 4));
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let y = 1; // the ghost row
        let crossed = |x: u16| {
            buf.cell((x, y))
                .unwrap()
                .modifier
                .contains(Modifier::CROSSED_OUT)
        };
        // Filename glyphs (x = 2..10) are struck …
        for x in 2u16..10 {
            assert!(crossed(x), "filename cell {x} should be struck through");
        }
        // … but the pad past the name is not.
        for x in 12u16..30 {
            assert!(!crossed(x), "pad cell {x} must not be struck through");
        }
    }
}

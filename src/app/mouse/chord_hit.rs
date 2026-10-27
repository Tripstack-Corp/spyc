//! The which-key popup's geometry: where its box sits and which entry each of
//! its body lines draws.
//!
//! **`chord_hint_layout` is the single source of truth for the popup's
//! geometry**, consumed by both the renderer (`render/overlays.rs`) and the
//! mouse, which gives a click on the box to the menu instead of to whatever the
//! layout says lies beneath it. The rule [`super::tab_hit`] states for the tab
//! bar holds here: a hit-test that re-derived the box would drift from the one
//! drawn, and a click aimed at the menu would land beside it.

use ratatui::layout::Rect;

use crate::app::render::overlays::{place_clear_of_line, wrap_label};

/// The label column's cap: a verbose entry wraps instead of widening the box
/// past a comfortable size.
const LABEL_MAX: usize = 38;

/// One body line of the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintLine {
    /// The entry this line draws: an index into the hint's rows.
    pub entry: usize,
    /// Whether this is the entry's first line, the one that carries its key.
    pub first: bool,
    /// This line's part of the entry's word-wrapped label.
    pub label: String,
}

/// Where the popup is drawn, and what each of its body lines holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordHintLayout {
    /// The box, border included.
    pub rect: Rect,
    /// The widest key.
    pub key_w: usize,
    /// The label column: the widest label, capped at `LABEL_MAX`.
    pub label_w: usize,
    /// One cell's width, ` <key>  <label> `.
    pub col_w: usize,
    /// The body's columns, each its lines top to bottom.
    pub columns: Vec<Vec<HintLine>>,
}

/// Lay `rows` (`(keys, label)` per entry) out in a box centred in `area`, its
/// border nudged off the pane divider's row and the split's column, since a box
/// edge on a divider merges the two rules into one line that reads as broken.
/// `None` when there is nothing to list or no room for a box: then nothing is
/// drawn, and nothing is there to hit.
pub fn chord_hint_layout(
    rows: &[(&str, &str)],
    area: Rect,
    h_divider_row: Option<u16>,
    v_divider_col: Option<u16>,
) -> Option<ChordHintLayout> {
    if rows.is_empty() || area.width < 24 || area.height < 8 {
        return None;
    }
    let key_w = rows
        .iter()
        .map(|(k, _)| crate::ui::display_width(k))
        .max()
        .unwrap_or(0);
    let label_w = rows
        .iter()
        .map(|(_, l)| crate::ui::display_width(l))
        .max()
        .unwrap_or(0)
        .clamp(1, LABEL_MAX);
    let col_w = key_w + label_w + 4;
    // One cell per entry: its label word-wrapped to `label_w` across as many
    // lines as it needs.
    let cells: Vec<Vec<HintLine>> = rows
        .iter()
        .enumerate()
        .map(|(entry, (_, label))| {
            wrap_label(label, label_w)
                .into_iter()
                .enumerate()
                .map(|(i, label)| HintLine {
                    entry,
                    first: i == 0,
                    label,
                })
                .collect()
        })
        .collect();

    let body_h = (area.height as usize).saturating_sub(2).max(1);
    let max_cols = ((area.width as usize).saturating_sub(2) / col_w.max(1)).max(1);
    let columns = flow_columns(cells, body_h, max_cols);

    let n = columns.len().max(1);
    let rows_tall = columns.iter().map(Vec::len).max().unwrap_or(0);
    let width = ((n * col_w) as u16 + 2).min(area.width);
    let height = (rows_tall as u16 + 2).min(area.height);
    let cx = area.x + area.width.saturating_sub(width) / 2;
    let cy = area.y + area.height.saturating_sub(height) / 2;
    let rect = Rect {
        x: place_clear_of_line(cx, width, v_divider_col, area.x, area.right()),
        y: place_clear_of_line(cy, height, h_divider_row, area.y, area.bottom()),
        width,
        height,
    };
    Some(ChordHintLayout {
        rect,
        key_w,
        label_w,
        col_w,
        columns,
    })
}

/// Flow cells into columns, balancing by rendered-line count so a tall menu (or
/// one with wrapped labels) splits into columns rather than overflowing the
/// screen height. An entry never splits across columns.
fn flow_columns<T>(cells: Vec<Vec<T>>, body_h: usize, max_cols: usize) -> Vec<Vec<T>> {
    let total_lines: usize = cells.iter().map(Vec::len).sum();
    let n_cols = total_lines.div_ceil(body_h).clamp(1, max_cols);
    let target = total_lines.div_ceil(n_cols).max(1);

    let mut columns = Vec::new();
    let mut cur = Vec::new();
    for cell in cells {
        if !cur.is_empty() && cur.len() + cell.len() > target {
            columns.push(std::mem::take(&mut cur));
        }
        cur.extend(cell);
    }
    if !cur.is_empty() {
        columns.push(cur);
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0, 0, 120, 24);
    const ROWS: [(&str, &str); 2] = [("a", "first"), ("bb", "second entry")];

    #[test]
    fn the_box_is_centred_and_sized_to_its_entries() {
        let layout = chord_hint_layout(&ROWS, AREA, None, None).expect("room for a box");
        assert_eq!((layout.key_w, layout.label_w), (2, "second entry".len()));
        assert_eq!(layout.col_w, 2 + 12 + 4);
        // One column of two lines, plus the border.
        assert_eq!(layout.rect, Rect::new((120 - 20) / 2, (24 - 4) / 2, 20, 4));
    }

    #[test]
    fn a_wrapped_label_keeps_its_key_on_its_first_line_only() {
        let long = "word ".repeat(12);
        let layout = chord_hint_layout(&[("x", long.trim())], AREA, None, None).expect("a box");
        let lines: Vec<&HintLine> = layout.columns.iter().flatten().collect();
        assert!(lines.len() > 1, "a label past the cap wraps");
        assert!(lines[0].first);
        assert!(lines[1..].iter().all(|l| !l.first && l.entry == 0));
    }

    #[test]
    fn the_box_steps_off_a_divider_its_edge_would_land_on() {
        let centred = chord_hint_layout(&ROWS, AREA, None, None)
            .expect("a box")
            .rect;
        let nudged = chord_hint_layout(&ROWS, AREA, Some(centred.y), Some(centred.x))
            .expect("a box")
            .rect;
        assert_ne!(nudged.x, centred.x, "off the split's column");
        assert_ne!(nudged.y, centred.y, "off the divider's row");
        assert_eq!(
            (nudged.width, nudged.height),
            (centred.width, centred.height)
        );
    }

    #[test]
    fn there_is_no_box_without_rows_or_room() {
        assert!(chord_hint_layout(&[], AREA, None, None).is_none());
        assert!(chord_hint_layout(&ROWS, Rect::new(0, 0, 23, 24), None, None).is_none());
        assert!(chord_hint_layout(&ROWS, Rect::new(0, 0, 120, 7), None, None).is_none());
    }
}

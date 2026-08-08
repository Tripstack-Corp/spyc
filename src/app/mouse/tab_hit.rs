//! Pure divider tab-bar layout: column widths, and which tab a pointer column
//! falls in.
//!
//! Sibling of [`super::route`]'s `region_at` — that hit-tests the pointer
//! against `FrameLayout`'s rects, this resolves the finer question of *which
//! tab* within the divider's one row.
//!
//! **`tab_widths` is the single source of truth for the tab bar's geometry**,
//! consumed by both the renderer (`render/chrome.rs`, for its advance and
//! overflow break) and the hit-test here. Deriving the widths twice is the
//! obvious shortcut and it silently breaks: any drift makes a click land on the
//! neighbouring tab, and only for the tabs *after* the one that drifted — which
//! reads as "clicks are off by one sometimes" rather than as a width bug.

use crate::pane::tabs::PaneTabs;

/// A tab's clickable extent on the divider: absolute screen columns
/// `start..end`, end exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabSpan {
    pub index: usize,
    pub start: u16,
    pub end: u16,
}

/// Per-tab width in display columns, in tab order.
///
/// Mirrors what `render_divider` paints for each tab: the `─` separator, the
/// `[N]` bracket, exactly one status cell, and the space-padded label.
///
/// The status cell is one column for every tab except a suspended one, whose
/// 💤 is two columns wide — the one deliberate width difference in the bar (a
/// sticky toggle, unlike the per-frame flicker the reserved blank prevents).
pub fn tab_widths(tabs: &PaneTabs, is_scrolling: bool) -> Vec<u16> {
    let active = tabs.active_index();
    tabs.tabs()
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            // The active tab's label is uppercased in scroll mode, and case can
            // change display width (ß → SS), so measure the label spyc will
            // actually paint rather than the stored one.
            let label = if i == active && is_scrolling {
                entry.info.label.to_uppercase()
            } else {
                entry.info.label.clone()
            };
            let cell = if entry.info.suspended { 2 } else { 1 };
            let w = 1 // "─" separator
                + crate::ui::display_width(&format!("[{}]", i + 1))
                + cell
                + crate::ui::display_width(&format!(" {label} "));
            u16::try_from(w).unwrap_or(u16::MAX)
        })
        .collect()
}

/// Lay the tab bar out left-to-right from `origin_x`, dropping tabs that would
/// overflow `width` — the same overflow rule the renderer applies, so a tab
/// scrolled off the divider is not clickable either.
pub fn tab_spans(origin_x: u16, width: u16, widths: &[u16]) -> Vec<TabSpan> {
    let mut spans = Vec::with_capacity(widths.len());
    let mut used: u16 = 0;
    for (index, &w) in widths.iter().enumerate() {
        // Saturating: a pathological label can't wrap the budget into "fits".
        if used.saturating_add(w) > width {
            break;
        }
        spans.push(TabSpan {
            index,
            start: origin_x.saturating_add(used),
            end: origin_x.saturating_add(used).saturating_add(w),
        });
        used = used.saturating_add(w);
    }
    spans
}

/// Which tab column `x` falls in, if any.
///
/// The leading `─` separator counts as part of its tab: it is one column, and
/// excluding it would leave a dead stripe between adjacent labels that reads as
/// a missed click.
pub fn tab_at(x: u16, spans: &[TabSpan]) -> Option<usize> {
    spans
        .iter()
        .find(|s| x >= s.start && x < s.end)
        .map(|s| s.index)
}

/// Which tab the pointer is over, given the divider's rect.
///
/// `None` when there is no divider (pane closed), the pointer is on another row,
/// or it is past the last tab — all of which must stay chrome-selectable.
pub fn tab_at_point(
    divider: Option<ratatui::layout::Rect>,
    widths: &[u16],
    col: u16,
    row: u16,
) -> Option<usize> {
    let d = divider?;
    if row < d.y || row >= d.y.saturating_add(d.height) {
        return None;
    }
    tab_at(col, &tab_spans(d.x, d.width, widths))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    const fn divider(y: u16) -> Rect {
        Rect {
            x: 0,
            y,
            width: 80,
            height: 1,
        }
    }

    #[test]
    fn a_pointer_on_another_row_is_not_a_tab_hit() {
        assert_eq!(tab_at_point(Some(divider(10)), &[6, 6], 2, 10), Some(0));
        assert_eq!(
            tab_at_point(Some(divider(10)), &[6, 6], 2, 9),
            None,
            "row above"
        );
        assert_eq!(
            tab_at_point(Some(divider(10)), &[6, 6], 2, 11),
            None,
            "row below"
        );
    }

    /// No pane → no divider → the tab bar can't be hit at all.
    #[test]
    fn no_divider_is_never_a_tab_hit() {
        assert_eq!(tab_at_point(None, &[6], 2, 0), None);
    }

    /// The divider's tail (cwd, empty space) must fall through so it stays
    /// chrome-selectable — the whole row activating a tab would make the path
    /// uncopyable.
    #[test]
    fn the_dividers_tail_is_not_a_tab_hit() {
        assert_eq!(tab_at_point(Some(divider(0)), &[6, 6], 11, 0), Some(1));
        assert_eq!(tab_at_point(Some(divider(0)), &[6, 6], 12, 0), None, "tail");
        assert_eq!(
            tab_at_point(Some(divider(0)), &[6, 6], 79, 0),
            None,
            "far tail"
        );
    }

    #[test]
    fn spans_are_contiguous_from_the_origin() {
        let spans = tab_spans(10, 100, &[8, 6, 7]);
        assert_eq!(
            spans,
            vec![
                TabSpan {
                    index: 0,
                    start: 10,
                    end: 18
                },
                TabSpan {
                    index: 1,
                    start: 18,
                    end: 24
                },
                TabSpan {
                    index: 2,
                    start: 24,
                    end: 31
                },
            ]
        );
    }

    /// A tab the renderer dropped for overflow must not be clickable — else a
    /// click in the divider's empty tail activates an invisible tab.
    #[test]
    fn overflow_drops_the_tabs_the_renderer_drops() {
        let spans = tab_spans(0, 15, &[8, 6, 7]);
        assert_eq!(spans.len(), 2, "third tab overflows 15 cols: {spans:?}");
        assert_eq!(spans[1].end, 14);
        assert_eq!(tab_at(14, &spans), None, "past the last tab: no hit");
    }

    #[test]
    fn hit_test_maps_every_column_of_a_tab_including_its_separator() {
        let spans = tab_spans(5, 100, &[4, 4]);
        for x in 5..9 {
            assert_eq!(tab_at(x, &spans), Some(0), "col {x}");
        }
        for x in 9..13 {
            assert_eq!(tab_at(x, &spans), Some(1), "col {x}");
        }
        assert_eq!(tab_at(4, &spans), None, "left of the bar");
        assert_eq!(tab_at(13, &spans), None, "right of the bar");
    }

    #[test]
    fn no_tabs_means_no_hits() {
        let spans = tab_spans(0, 80, &[]);
        assert!(spans.is_empty());
        assert_eq!(tab_at(0, &spans), None);
    }

    /// Zero width (a fully collapsed divider) must not produce a span that
    /// swallows the whole row.
    #[test]
    fn zero_width_yields_nothing() {
        assert!(tab_spans(0, 0, &[4]).is_empty());
    }
}

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

/// One tab's fitted geometry: its total width in display columns and the
/// exact label text (padding spaces included, as they survived the fit) the
/// renderer must paint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabCell {
    pub width: u16,
    pub label_text: String,
}

/// Fit every tab into `bar_width` display columns — the bar never overflows
/// and never silently drops a tab.
///
/// Mirrors what `render_divider` paints for each tab: the `─` separator, the
/// `[N]` bracket, exactly one status cell, and the (possibly cropped) label.
///
/// The status cell is one column for every tab except a suspended one, whose
/// 💤 is two columns wide — the one deliberate width difference in the bar (a
/// sticky toggle, unlike the per-frame flicker the reserved blank prevents).
///
/// When the natural layout overflows, columns are reclaimed in two stages:
/// 1. **Padding spaces first** (trailing then leading, rightmost tab first) —
///    uneven spacing across tabs is accepted; only as many spaces go as needed.
/// 2. **Then letters from the longest label**, one column at a time — which
///    converges the labels toward equal length, then shrinks them together.
///
/// Only if the fixed chrome alone (`─[N]` + status cells) exceeds the bar do
/// tabs still drop, via the overflow break in `tab_spans` / the renderer.
pub fn tab_layout(tabs: &PaneTabs, is_scrolling: bool, bar_width: u16) -> Vec<TabCell> {
    let active = tabs.active_index();
    let items = tabs
        .tabs()
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
            let fixed = 1 // "─" separator
                + crate::ui::display_width(&format!("[{}]", i + 1))
                + cell;
            (fixed, label)
        })
        .collect();
    fit_tabs(items, bar_width)
}

/// The pure fit itself: `items` is each tab's fixed chrome width (`─[N]` +
/// status cell) and its painted label. Split out from [`tab_layout`] so the
/// algorithm is unit-testable without spawning a pty behind a `PaneTabs`.
fn fit_tabs(items: Vec<(usize, String)>, bar_width: u16) -> Vec<TabCell> {
    struct Fit {
        fixed: usize,
        label: String,
        lpad: bool,
        rpad: bool,
    }
    let mut fits: Vec<Fit> = items
        .into_iter()
        .map(|(fixed, label)| Fit {
            fixed,
            label,
            lpad: true,
            rpad: true,
        })
        .collect();
    let total = |fits: &[Fit]| -> usize {
        fits.iter()
            .map(|f| {
                f.fixed
                    + usize::from(f.lpad)
                    + crate::ui::display_width(&f.label)
                    + usize::from(f.rpad)
            })
            .sum()
    };
    let bar = bar_width as usize;
    // Stage 1: crop padding spaces, trailing before leading, right to left.
    'spaces: for trailing in [true, false] {
        for i in (0..fits.len()).rev() {
            if total(&fits) <= bar {
                break 'spaces;
            }
            if trailing {
                fits[i].rpad = false;
            } else {
                fits[i].lpad = false;
            }
        }
    }
    // Stage 2: shave one display column off the currently-longest label until
    // it fits (or every label is gone — fixed chrome alone overflows).
    // Eligibility and the cut are both in COLUMNS: a label that is non-empty
    // in bytes but zero columns wide (a lone ZWSP) can't shrink, and filtering
    // on `is_empty` would select it forever. Cutting on grapheme clusters
    // (`display_truncate`) keeps the width the fit sums equal to the width
    // the terminal draws.
    while total(&fits) > bar {
        let Some(longest) = fits
            .iter_mut()
            .filter(|f| crate::ui::display_width(&f.label) > 0)
            .max_by_key(|f| crate::ui::display_width(&f.label))
        else {
            break;
        };
        let target = crate::ui::display_width(&longest.label).saturating_sub(1);
        longest.label = crate::ui::display_truncate(&longest.label, target).to_string();
    }
    fits.into_iter()
        .map(|f| {
            let label_text = format!(
                "{}{}{}",
                if f.lpad { " " } else { "" },
                f.label,
                if f.rpad { " " } else { "" }
            );
            TabCell {
                width: u16::try_from(f.fixed + crate::ui::display_width(&label_text))
                    .unwrap_or(u16::MAX),
                label_text,
            }
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

    /// Single-digit tab: `─` + `[N]` + one status cell = 5 fixed columns.
    fn item(label: &str) -> (usize, String) {
        (5, label.to_string())
    }

    fn bar_total(cells: &[TabCell]) -> u16 {
        cells.iter().map(|c| c.width).sum()
    }

    #[test]
    fn fit_keeps_padding_when_there_is_room() {
        let cells = fit_tabs(vec![item("claude"), item("bash")], 80);
        assert_eq!(cells[0].label_text, " claude ");
        assert_eq!(cells[1].label_text, " bash ");
        assert_eq!(cells[0].width, 13);
        assert_eq!(cells[1].width, 11);
    }

    /// Two columns over: only padding spaces go (trailing first, rightmost
    /// tab first) — every letter survives, spacing ends up uneven.
    #[test]
    fn fit_crops_spaces_before_letters() {
        let cells = fit_tabs(vec![item("claude"), item("bash")], 22);
        assert_eq!(
            cells[1].label_text, " bash",
            "rightmost trailing space first"
        );
        assert_eq!(cells[0].label_text, " claude", "then the next tab's");
        assert_eq!(bar_total(&cells), 22);
    }

    /// Deep overflow: after the spaces, letters come off whichever label is
    /// currently longest, converging the labels toward equal length.
    #[test]
    fn fit_shaves_longest_label_first() {
        let cells = fit_tabs(vec![item("claude"), item("bash")], 16);
        assert_eq!(cells[0].label_text, "cla");
        assert_eq!(cells[1].label_text, "bas");
        assert_eq!(bar_total(&cells), 16);
    }

    /// The bar must never overflow, whatever the label pressure — that is the
    /// whole point of the fit (tabs used to silently drop instead).
    #[test]
    fn fit_never_overflows_the_bar() {
        let labels = [
            "coordinator",
            "discipline",
            "watercooler",
            "martlet",
            "topo-oceans",
            "codex-dev",
            "system",
            "shell",
        ];
        for bar in [200u16, 120, 80, 60, 48, 41] {
            let cells = fit_tabs(labels.iter().map(|l| item(l)).collect(), bar);
            assert_eq!(cells.len(), labels.len(), "no tab dropped at {bar}");
            assert!(
                bar_total(&cells) <= bar,
                "overflow at bar={bar}: {}",
                bar_total(&cells)
            );
        }
    }

    /// Below the fixed chrome floor (8 tabs × 5 cols = 40) the labels are gone
    /// and the old overflow-drop in `tab_spans` remains the backstop.
    #[test]
    fn fit_gives_up_at_the_fixed_chrome_floor() {
        let cells = fit_tabs(vec![item("claude"), item("bash")], 8);
        assert!(cells.iter().all(|c| c.label_text.is_empty()));
        assert_eq!(bar_total(&cells), 10, "the fixed chrome itself remains");
    }

    /// A label that is non-empty in bytes but zero display columns (a lone
    /// zero-width space, a variation selector) must not wedge the shave loop:
    /// it can't be shrunk, so the fit has to look past it — and return.
    /// Reachable from `[[pane.tab]] label = "..."`, which isn't validated.
    #[test]
    fn fit_terminates_on_zero_width_label() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let cells = fit_tabs(vec![item("\u{200b}"), item("claude")], 8);
            let _ = tx.send(cells);
        });
        let cells = rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("fit_tabs never returned on a zero-width label");
        assert!(bar_total(&cells) <= 10, "fixed chrome floor holds");
    }

    /// Shaving must cut on grapheme clusters: a ZWJ emoji is one drawn glyph,
    /// and a per-char pop would leave a half-cluster whose width the terminal
    /// and the fit disagree on.
    #[test]
    fn fit_shaves_whole_clusters() {
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        // `ab` + family = 4 columns on 5 fixed; a bar of 6 leaves one column
        // of label. The family (2 wide) must go as one unit, not one scalar
        // at a time.
        let cells = fit_tabs(vec![item(&format!("ab{family}"))], 6);
        assert_eq!(cells[0].label_text, "a");
        assert_eq!(bar_total(&cells), 6);
        // And a cluster at the head with a 1-column budget drops whole rather
        // than leaving a fragment.
        let cells = fit_tabs(vec![item(&format!("{family}a"))], 6);
        assert!(!cells[0].label_text.contains('\u{200d}'), "no ZWJ fragment");
        assert!(bar_total(&cells) <= 6);
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

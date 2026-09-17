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
/// 2. **Then the widest label is cropped**, one step at a time, by the first
///    of these that applies and frees at least one column:
///    - **tail-segment drop** at the label's last seam (`topo-oceans` →
///      `topo…`), refused when the result would equal another tab's label;
///    - **head-segment drop** when the first segment is shared with another
///      tab (`watercooler-cloud` beside `watercooler` → `…cloud`) — the shared
///      head carried no information, the tail is what tells them apart;
///    - **character shave** with a trailing `…` (`codex` → `cod…`);
///    - **floor**: a label of [`LABEL_FLOOR`] painted columns or fewer is
///      cleared outright — `[N]` still names the tab, and an empty label reads
///      as deliberate where `c…` reads as broken.
///
/// The **active tab is cropped last**: it leaves the pool only once every
/// other label is empty, so moderate pressure never touches the name the user
/// is reading, and extreme pressure yields `[1][2]…[9] bash`.
///
/// Every step frees at least one column or is skipped, so the loop always
/// reaches the fixed-chrome floor. Only if the fixed chrome alone (`─[N]` +
/// status cells) exceeds the bar do tabs still drop, via the overflow break in
/// `tab_spans` / the renderer.
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
    fit_tabs(items, Some(active), bar_width)
}

/// The pure fit itself: `items` is each tab's fixed chrome width (`─[N]` +
/// status cell) and its painted label. Split out from [`tab_layout`] so the
/// algorithm is unit-testable without spawning a pty behind a `PaneTabs`.
fn fit_tabs(items: Vec<(usize, String)>, active: Option<usize>, bar_width: u16) -> Vec<TabCell> {
    let mut fits: Vec<Fit> = items
        .into_iter()
        .map(|(fixed, label)| Fit {
            fixed,
            original: label.clone(),
            core: label,
            head_elided: false,
            tail_cut: false,
            lpad: true,
            rpad: true,
        })
        .collect();
    let total = |fits: &[Fit]| -> usize { fits.iter().map(Fit::width).sum() };
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
    // Stage 2: crop the widest eligible label one step at a time. The active
    // tab is eligible only once every other label is already empty.
    while total(&fits) > bar {
        let others_alive = fits
            .iter()
            .enumerate()
            .any(|(i, f)| Some(i) != active && !f.core.is_empty());
        // `max_by_key` keeps the LAST maximum, so ties crop the rightmost tab.
        let Some(idx) = (0..fits.len())
            .filter(|&i| {
                let alive = !fits[i].core.is_empty();
                let spared = Some(i) == active && others_alive;
                alive && !spared
            })
            .max_by_key(|&i| fits[i].painted_width())
        else {
            break;
        };
        crop_one(&mut fits, idx);
    }
    fits.into_iter()
        .map(|f| {
            let label_text = f.label_text();
            TabCell {
                width: u16::try_from(f.fixed + crate::ui::display_width(&label_text))
                    .unwrap_or(u16::MAX),
                label_text,
            }
        })
        .collect()
}

/// Marks a cropped label: the column it costs is what tells the reader the
/// name continues, so `codex-` no longer reads as the tab's actual name.
const ELLIPSIS: &str = "\u{2026}";

/// A label painted at this many columns or fewer (`co…`) carries nothing the
/// `[N]` bracket doesn't; the next crop clears it instead of shaving on.
const LABEL_FLOOR: usize = 3;

/// One tab mid-fit: its fixed chrome width plus the label in its current
/// cropped form. `original` is kept for the head-segment comparison, which
/// asks whether the label *as named* shares its first segment with a
/// neighbour — an answer cropping must not change.
struct Fit {
    fixed: usize,
    original: String,
    core: String,
    head_elided: bool,
    tail_cut: bool,
    lpad: bool,
    rpad: bool,
}

impl Fit {
    /// The label's painted core: ellipses included, padding excluded. An empty
    /// core paints nothing at all — no orphaned `…`.
    fn painted(&self) -> String {
        if self.core.is_empty() {
            return String::new();
        }
        let head = if self.head_elided { ELLIPSIS } else { "" };
        let tail = if self.tail_cut { ELLIPSIS } else { "" };
        format!("{head}{}{tail}", self.core)
    }

    fn painted_width(&self) -> usize {
        crate::ui::display_width(&self.painted())
    }

    fn label_text(&self) -> String {
        format!(
            "{}{}{}",
            if self.lpad { " " } else { "" },
            self.painted(),
            if self.rpad { " " } else { "" }
        )
    }

    fn width(&self) -> usize {
        self.fixed + crate::ui::display_width(&self.label_text())
    }

    fn clear(&mut self) {
        self.core.clear();
        self.head_elided = false;
        self.tail_cut = false;
    }
}

/// A separator a label can be cut before. Space is included because a custom
/// `^a r` name can carry one; `/` because a cwd-derived name might.
const fn is_seam_sep(c: char) -> bool {
    matches!(c, '-' | '_' | '.' | '/' | ' ')
}

/// Byte offsets at which `s` may be cut: before a separator run that has
/// text on both sides, and at a lowercase→uppercase transition
/// (`TradingAgents` → `Trading` | `Agents`). Ascending.
fn seams(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(p) = prev {
            let sep_run_start = is_seam_sep(c) && !is_seam_sep(p);
            let text_after = || s[i..].chars().any(|c| !is_seam_sep(c));
            let camel = !is_seam_sep(c) && c.is_uppercase() && p.is_lowercase();
            if (sep_run_start && text_after()) || camel {
                out.push(i);
            }
        }
        prev = Some(c);
    }
    out
}

/// `s` up to its first seam — the whole of it when there is none.
fn first_segment(s: &str) -> &str {
    seams(s).first().map_or(s, |&i| &s[..i])
}

/// Crop `fits[idx]` by one step: tail-segment drop, else head-segment drop,
/// else character shave, else clear. See [`tab_layout`] for the rationale
/// behind the order. Every step either frees at least one column or falls
/// through to the next, which is what keeps the caller's loop finite.
fn crop_one(fits: &mut [Fit], idx: usize) {
    let before = fits[idx].painted_width();
    if before <= LABEL_FLOOR {
        fits[idx].clear();
        return;
    }
    let others: Vec<(String, String, String)> = fits
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != idx)
        .map(|(_, f)| {
            (
                f.original.to_lowercase(),
                f.core.to_lowercase(),
                first_segment(&f.original).to_lowercase(),
            )
        })
        .collect();
    // A crop that lands on a neighbour's name (as named, or as currently
    // painted) is refused: two tabs reading the same defeats the label.
    let collides = |cand: &str| {
        let cand = cand.to_lowercase();
        others
            .iter()
            .any(|(orig, core, _)| *orig == cand || *core == cand)
    };
    let fit = &mut fits[idx];
    let seams = seams(&fit.core);
    if let Some(&cut) = seams.last() {
        let cand = &fit.core[..cut];
        if !collides(cand) {
            let cand = cand.to_string();
            let prev = std::mem::replace(&mut fit.core, cand);
            let was_cut = std::mem::replace(&mut fit.tail_cut, true);
            if fit.painted_width() < before {
                return;
            }
            fit.core = prev;
            fit.tail_cut = was_cut;
        }
    }
    if let Some(&cut) = seams.first() {
        let head = fit.core[..cut].to_lowercase();
        let shared = others.iter().any(|(_, _, first)| *first == head);
        let cand = fit.core[cut..].trim_start_matches(is_seam_sep);
        if shared && !cand.is_empty() && !collides(cand) {
            let cand = cand.to_string();
            let prev = std::mem::replace(&mut fit.core, cand);
            let was_elided = std::mem::replace(&mut fit.head_elided, true);
            if fit.painted_width() < before {
                return;
            }
            fit.core = prev;
            fit.head_elided = was_elided;
        }
    }
    // Shave: the first pop only pays for the `…`, so it keeps popping until a
    // column is actually freed.
    fit.tail_cut = true;
    while fit.painted_width() >= before {
        if fit.core.pop().is_none() {
            fit.clear();
            return;
        }
    }
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
        let cells = fit_tabs(vec![item("claude"), item("bash")], None, 80);
        assert_eq!(cells[0].label_text, " claude ");
        assert_eq!(cells[1].label_text, " bash ");
        assert_eq!(cells[0].width, 13);
        assert_eq!(cells[1].width, 11);
    }

    /// Two columns over: only padding spaces go (trailing first, rightmost
    /// tab first) — every letter survives, spacing ends up uneven.
    #[test]
    fn fit_crops_spaces_before_letters() {
        let cells = fit_tabs(vec![item("claude"), item("bash")], None, 22);
        assert_eq!(
            cells[1].label_text, " bash",
            "rightmost trailing space first"
        );
        assert_eq!(cells[0].label_text, " claude", "then the next tab's");
        assert_eq!(bar_total(&cells), 22);
    }

    /// Deep overflow with seamless labels: letters come off whichever label is
    /// currently widest, converging the labels toward equal length, and every
    /// shaved label wears the `…` that says it was cut.
    #[test]
    fn fit_shaves_longest_label_first() {
        let cells = fit_tabs(vec![item("claude"), item("bash")], None, 16);
        assert_eq!(cells[0].label_text, "cl…");
        assert_eq!(cells[1].label_text, "ba…");
        assert_eq!(bar_total(&cells), 16);
    }

    /// A shave costs the `…` its column, so the first shave pops two letters
    /// to free one — `codex` never paints as `code…` (same width as before).
    #[test]
    fn fit_shave_marks_the_cut_and_still_frees_a_column() {
        let cells = fit_tabs(vec![item("codex")], None, 9);
        assert_eq!(cells[0].label_text, "cod…");
        assert_eq!(bar_total(&cells), 9);
    }

    /// A label with a seam loses its last segment whole rather than letters
    /// from the middle of a word: `topo-oc` told the reader nothing.
    #[test]
    fn fit_cuts_at_the_last_seam_before_shaving() {
        let cells = fit_tabs(vec![item("topo-oceans"), item("shell")], None, 22);
        assert_eq!(cells[0].label_text, "topo…");
        assert_eq!(
            cells[1].label_text, "shell",
            "the shorter label is untouched"
        );
        assert!(bar_total(&cells) <= 22);
    }

    /// camelCase is a seam too.
    #[test]
    fn fit_treats_a_case_transition_as_a_seam() {
        let cells = fit_tabs(vec![item("TradingAgents"), item("shell")], None, 24);
        assert_eq!(cells[0].label_text, "Trading…");
    }

    /// Siblings sharing a head must stay distinct: cutting the tail off
    /// `watercooler-cloud` would land on the `watercooler` tab's own name, so
    /// the shared head goes instead and the distinguishing tail survives.
    #[test]
    fn fit_keeps_siblings_distinct_by_dropping_the_shared_head() {
        let cells = fit_tabs(
            vec![
                item("watercooler"),
                item("watercooler-cloud"),
                item("watercooler-dashboard"),
            ],
            None,
            41,
        );
        assert_eq!(cells[1].label_text, "…cloud");
        assert_eq!(cells[2].label_text, "…dashboard");
        assert_eq!(cells[0].label_text, "watercool…", "seamless: shaved last");
        assert!(bar_total(&cells) <= 41);
    }

    /// Below [`LABEL_FLOOR`] the label is cleared, not shaved to `c…`.
    #[test]
    fn fit_clears_a_label_at_the_floor_instead_of_shaving_further() {
        let at_floor = fit_tabs(vec![item("bash")], None, 8);
        assert_eq!(at_floor[0].label_text, "ba…");
        let below = fit_tabs(vec![item("bash")], None, 7);
        assert_eq!(below[0].label_text, "", "cleared, no orphan `…`");
        assert_eq!(bar_total(&below), 5, "fixed chrome only");
    }

    /// The active tab is the one being read: it keeps its full name while any
    /// other tab still has a label, and is cropped only once they are gone.
    #[test]
    fn fit_crops_the_active_tab_last() {
        let items = || vec![item("coordinator"), item("discipline"), item("shell")];
        let moderate = fit_tabs(items(), Some(0), 37);
        assert_eq!(moderate[0].label_text, "coordinator", "active untouched");
        assert_eq!(moderate[1].label_text, "disci…", "the widest other paid");
        assert_eq!(moderate[2].label_text, "shell");
        assert!(bar_total(&moderate) <= 37);

        let extreme = fit_tabs(items(), Some(0), 18);
        assert_eq!(extreme[1].label_text, "");
        assert_eq!(extreme[2].label_text, "");
        assert_eq!(
            extreme[0].label_text, "co…",
            "active crops only after the rest"
        );
        assert_eq!(bar_total(&extreme), 18);
    }

    /// Across every bar width the fit never overflows and no two painted
    /// labels above the floor read the same. Seam-aware cropping guarantees
    /// distinctness for segment cuts; a pure character shave of two names
    /// that differ only past the cut (`claude1`/`claude2`) can still collide,
    /// which is why this set has none such — the `[N]` bracket is the
    /// backstop there.
    #[test]
    fn fit_stays_distinct_and_within_the_bar_at_every_width() {
        let labels = [
            "coordinator",
            "discipline",
            "watercooler",
            "watercooler-cloud",
            "watercooler-dashboard",
            "martlet-ops",
            "topo-oceans",
            "codex",
            "codex-dev",
            "system",
            "shell",
            "bash",
        ];
        let floor = u16::try_from(labels.len() * 5).unwrap();
        for bar in (floor..=220).rev() {
            let cells = fit_tabs(labels.iter().map(|l| item(l)).collect(), Some(11), bar);
            assert!(bar_total(&cells) <= bar, "overflow at bar={bar}: {cells:?}");
            let painted: Vec<&str> = cells
                .iter()
                .map(|c| c.label_text.trim())
                .filter(|t| crate::ui::display_width(t) > LABEL_FLOOR)
                .collect();
            for (i, a) in painted.iter().enumerate() {
                for b in &painted[i + 1..] {
                    assert_ne!(a, b, "duplicate label at bar={bar}: {cells:?}");
                }
            }
        }
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
            let cells = fit_tabs(labels.iter().map(|l| item(l)).collect(), None, bar);
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
        let cells = fit_tabs(vec![item("claude"), item("bash")], None, 8);
        assert!(cells.iter().all(|c| c.label_text.is_empty()));
        assert_eq!(bar_total(&cells), 10, "the fixed chrome itself remains");
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

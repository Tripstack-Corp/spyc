//! The status bar's `projects` segment while several projects are open: a
//! chip for each, its label and the glyph of what its agents most need, and
//! the steps the segment narrows by when the bar is short. The `Space j` rows
//! show the same glyphs.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::ui::theme::Theme;

/// A blocked agent's red, on its tab's dot and on its project's chip.
pub const BLOCKED_RED: Color = Color::Rgb(0xF7, 0x76, 0x8E);

/// What a project's agents most need from the user, least urgent first. A
/// project whose agents are all idle, or that has none, has none of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectAttention {
    /// Waiting on background work that will wake it.
    Waiting,
    Working,
    /// Finished while the user wasn't looking at its tab.
    UnseenDone,
    /// Waiting on the user.
    Blocked,
}

/// One open project in the segment.
pub struct ProjectChip {
    pub label: String,
    pub attention: Option<ProjectAttention>,
    /// The project in view.
    pub active: bool,
}

/// Every open project's chip in switcher order, and the colour of the working
/// glyph this frame, the tab dots' pulse.
pub struct ProjectChips {
    pub chips: Vec<ProjectChip>,
    pub pulse: Color,
}

/// The glyph for `attention`, the tab dots' own, in mono too: a red `■`
/// blocked, a teal `■` a done not yet seen, a `●` in `pulse` working and a
/// hollow `○` waiting, which never pulses and is uncoloured in mono.
pub fn glyph(attention: ProjectAttention, pulse: Color, theme: &Theme) -> Span<'static> {
    let bold = |fg| Style::default().fg(fg).add_modifier(Modifier::BOLD);
    match attention {
        ProjectAttention::Blocked => Span::styled("\u{25a0}", bold(BLOCKED_RED)),
        ProjectAttention::UnseenDone => Span::styled("\u{25a0}", bold(theme.take)),
        ProjectAttention::Working => Span::styled("\u{25cf}", bold(pulse)),
        ProjectAttention::Waiting if theme.mono => Span::raw("\u{25cb}"),
        ProjectAttention::Waiting => Span::styled("\u{25cb}", Style::default().fg(theme.dir)),
    }
}

/// The narrowest a label is cut to, its ellipsis included.
const MIN_CUT: usize = 4;

/// What parts the chips: powerline's thin chevron, or a `|` in mono, where the
/// status bar draws no powerline glyphs. Both are as wide.
const PARTING: &str = " \u{e0b1} ";
const MONO_PARTING: &str = " | ";

impl ProjectChips {
    /// The segment in at most `room` columns, the first that fits of: every
    /// chip; every chip with the longer labels cut to one length, no shorter
    /// than [`MIN_CUT`]; the project in view with its place among them
    /// (`2/4`) and the others' most urgent glyph, its label cut to fit. Else
    /// the project in view's label cut to the room, with a red `■` while
    /// another project is blocked: a blocked agent elsewhere is what the
    /// segment is for. However short the room, that label is kept to
    /// [`MIN_CUT`] columns, and `most`, where the bar ends, is cut to before
    /// the `■` is lost.
    ///
    /// Each step is as wide whatever the agents are doing, so what follows
    /// the segment never moves as they work, block and finish: each glyph has
    /// its cell, blank while there is nothing to show, and which step is
    /// drawn doesn't depend on them.
    pub fn spans(&self, room: usize, most: usize, theme: &Theme) -> Vec<Span<'static>> {
        if let Some(cap) = self.label_cap(room, theme) {
            return self.every_chip(cap, theme);
        }
        self.collapsed(room, theme)
            .unwrap_or_else(|| self.alone(room, most, theme))
    }

    /// The longest a label may be for every chip to fit `room`: the widest
    /// label's width when they all fit whole, else the longest cut, no
    /// shorter than [`MIN_CUT`], that fits. `None` when even that doesn't.
    fn label_cap(&self, room: usize, theme: &Theme) -> Option<usize> {
        let count = self.chips.len();
        // A space and a cell after each label, and a parting between chips.
        let partings = super::display_width(parting(theme)) * count.saturating_sub(1);
        let budget = room.checked_sub(2 * count + partings)?;
        let widths: Vec<usize> = self
            .chips
            .iter()
            .map(|c| super::display_width(&c.label))
            .collect();
        let widest = widths.iter().copied().max().unwrap_or(0);
        (widest.min(MIN_CUT)..=widest)
            .rev()
            .find(|&cap| widths.iter().map(|w| (*w).min(cap)).sum::<usize>() <= budget)
    }

    /// Every chip as `label glyph`, each label in at most `cap` columns,
    /// [`PARTING`]s between them: the one in view bold, the others dim, and
    /// the parting neither, to stand out from both.
    fn every_chip(&self, cap: usize, theme: &Theme) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for (i, chip) in self.chips.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(parting(theme)));
            }
            spans.push(label(chip, cap));
            spans.push(Span::raw(" "));
            spans.push(cell(chip.attention, self.pulse, theme));
        }
        spans
    }

    /// `label 2/4 ■` in `room`, the label cut to fit, or `None` when that
    /// would cut it shorter than [`MIN_CUT`].
    fn collapsed(&self, room: usize, theme: &Theme) -> Option<Vec<Span<'static>>> {
        // The project in view is always one of the open ones.
        let at = self.chips.iter().position(|c| c.active).unwrap_or(0);
        let place = format!(" {}/{} ", at + 1, self.chips.len());
        let cap = room.checked_sub(super::display_width(&place) + 1)?;
        if cap < self.active_width().min(MIN_CUT) {
            return None;
        }
        let mut spans = self.active_label(cap);
        spans.push(Span::raw(place));
        let others = self.others().filter_map(|c| c.attention).max();
        spans.push(cell(others, self.pulse, theme));
        Some(spans)
    }

    fn alone(&self, room: usize, most: usize, theme: &Theme) -> Vec<Span<'static>> {
        let cap = room.max(MIN_CUT + 2).min(most).saturating_sub(2);
        let mut spans = self.active_label(cap);
        let blocked = self
            .others()
            .any(|c| c.attention == Some(ProjectAttention::Blocked));
        spans.push(Span::raw(" "));
        let blocked = blocked.then_some(ProjectAttention::Blocked);
        spans.push(cell(blocked, self.pulse, theme));
        spans
    }

    /// The project in view's label's columns.
    fn active_width(&self) -> usize {
        self.chips
            .iter()
            .find(|c| c.active)
            .map_or(0, |c| super::display_width(&c.label))
    }

    /// The project in view's label, in at most `cap` columns.
    fn active_label(&self, cap: usize) -> Vec<Span<'static>> {
        self.chips
            .iter()
            .find(|c| c.active)
            .map(|c| label(c, cap))
            .into_iter()
            .collect()
    }

    fn others(&self) -> impl Iterator<Item = &ProjectChip> {
        self.chips.iter().filter(|c| !c.active)
    }
}

/// The parting `theme` draws between chips.
const fn parting(theme: &Theme) -> &'static str {
    if theme.mono { MONO_PARTING } else { PARTING }
}

/// The glyph for `attention`, or a blank in its place, so a glyph coming and
/// going moves nothing beside it.
pub fn cell(attention: Option<ProjectAttention>, pulse: Color, theme: &Theme) -> Span<'static> {
    attention.map_or_else(|| Span::raw(" "), |a| glyph(a, pulse, theme))
}

/// `chip`'s label in at most `cap` columns ([`cut`]), bold for the project in
/// view and dim for the others.
fn label(chip: &ProjectChip, cap: usize) -> Span<'static> {
    let modifier = if chip.active {
        Modifier::BOLD
    } else {
        Modifier::DIM
    };
    Span::styled(
        cut(&chip.label, cap),
        Style::default().add_modifier(modifier),
    )
}

/// `label` in at most `columns`, its middle given up for an ellipsis when it
/// doesn't fit: a third for the head and the rest for the tail, since a
/// project's siblings often share its start (`web-api`, `web-app`).
fn cut(label: &str, columns: usize) -> String {
    if super::display_width(label) <= columns {
        return label.to_owned();
    }
    let Some(room) = columns.checked_sub(1) else {
        return String::new();
    };
    let head = room / 3;
    format!(
        "{}\u{2026}{}",
        super::display_truncate(label, head),
        super::display_truncate_tail(label, room - head)
    )
}

/// How many columns `spans` take.
pub fn width(spans: &[Span]) -> usize {
    spans.iter().map(|s| super::display_width(&s.content)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chips(list: &[(&str, Option<ProjectAttention>, bool)]) -> ProjectChips {
        ProjectChips {
            chips: list
                .iter()
                .map(|(label, attention, active)| ProjectChip {
                    label: (*label).to_string(),
                    attention: *attention,
                    active: *active,
                })
                .collect(),
            pulse: Color::Red,
        }
    }

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    use ProjectAttention::{Blocked, UnseenDone, Waiting, Working};

    /// Each step is taken only when the one before doesn't fit, and the last
    /// is drawn whatever the room: every chip, the longer labels cut, the
    /// project in view collapsed with its label cut to fit, then alone.
    #[test]
    fn the_segment_narrows_one_step_at_a_time() {
        let theme = Theme::default();
        let c = chips(&[
            ("alpha", Some(Blocked), false),
            ("bravo", None, true),
            ("charlie", Some(Working), false),
            ("delta", None, false),
        ]);
        let at = |room| text(&c.spans(room, 100, &theme));
        let p = "\u{e0b1}";
        let every = format!("alpha ■ {p} bravo   {p} charlie ● {p} delta  ");
        assert_eq!(at(100), every);
        assert_eq!(at(39), every);
        assert_eq!(
            at(38),
            format!("alpha ■ {p} bravo   {p} c…rlie ● {p} delta  ")
        );
        assert_eq!(at(33), format!("a…ha ■ {p} b…vo   {p} c…ie ● {p} d…ta  "));
        assert_eq!(at(32), "bravo 2/4 ■");
        assert_eq!(at(11), "bravo 2/4 ■");
        assert_eq!(at(10), "b…vo 2/4 ■");
        assert_eq!(at(9), "bravo ■");
        assert_eq!(at(0), "b…vo ■");
    }

    /// Collapsed, the glyph is the others' most urgent; alone, only a block
    /// elsewhere is kept, and the project in view's own state isn't drawn
    /// in either, since its tab bar shows it.
    #[test]
    fn collapsed_and_alone_speak_for_the_others() {
        let theme = Theme::default();
        let c = chips(&[
            ("x", Some(Waiting), false),
            ("y", Some(Blocked), true),
            ("z", Some(UnseenDone), false),
        ]);
        let collapsed = c.collapsed(100, &theme).expect("room to collapse");
        assert_eq!(text(&collapsed), "y 2/3 ■");
        assert_eq!(collapsed.last().map(|s| s.style.fg), Some(Some(theme.take)));
        assert_eq!(text(&c.alone(100, 100, &theme)), "y  ");
        let quiet = chips(&[("x", None, false), ("y", None, true)]);
        let quiet = quiet.collapsed(100, &theme).expect("room to collapse");
        assert_eq!(text(&quiet), "y 2/2  ");
    }

    /// Narrowest, the label is cut to the room, kept to `MIN_CUT` columns,
    /// and cut further only where the bar ends short of it and its `■`, so
    /// the `■` is kept; with nothing blocked elsewhere, the label is cut
    /// alike and its cell left blank, so the step holds its width.
    #[test]
    fn the_narrowest_step_cuts_the_label_not_the_block() {
        let theme = Theme::default();
        let c = chips(&[("a-long-project", None, true), ("x", Some(Blocked), false)]);
        assert_eq!(text(&c.spans(9, 100, &theme)), "a-\u{2026}ject ■");
        assert_eq!(text(&c.spans(0, 100, &theme)), "a\u{2026}ct ■");
        assert_eq!(text(&c.spans(0, 5, &theme)), "\u{2026}ct ■");
        assert_eq!(text(&c.spans(0, 3, &theme)), "\u{2026} ■");
        assert_eq!(text(&c.spans(0, 2, &theme)), " ■");
        let quiet = chips(&[("a-long-project", None, true), ("x", Some(Working), false)]);
        assert_eq!(text(&quiet.spans(9, 100, &theme)), "a-\u{2026}ject  ");
    }

    /// The glyphs are the tab dots': shape and colour, and in mono only the
    /// waiting `○` drops its colour, as on the dot.
    #[test]
    fn each_attention_has_the_tab_dots_glyph() {
        let theme = Theme::default();
        let cases = [
            (Blocked, "■", BLOCKED_RED),
            (UnseenDone, "■", theme.take),
            (Working, "●", Color::Red),
            (Waiting, "○", theme.dir),
        ];
        for (attention, shape, fg) in cases {
            let span = glyph(attention, Color::Red, &theme);
            assert_eq!((span.content.as_ref(), span.style.fg), (shape, Some(fg)));
        }
        let mono = Theme {
            mono: true,
            ..Theme::default()
        };
        let span = glyph(Blocked, Color::Red, &mono);
        assert_eq!(
            span.style.fg,
            Some(BLOCKED_RED),
            "kept in mono, as on the dot"
        );
        let span = glyph(Waiting, Color::Red, &mono);
        assert_eq!((span.content.as_ref(), span.style.fg), ("○", None));
    }

    /// The segment is as wide whatever the agents are doing, at every step it
    /// narrows through and with each project in view, so what follows it on
    /// the bar never moves as they work, block and finish.
    #[test]
    fn the_segment_holds_its_width_whatever_the_agents_do() {
        let theme = Theme::default();
        let states = [
            None,
            Some(Waiting),
            Some(Working),
            Some(UnseenDone),
            Some(Blocked),
        ];
        let labels = ["web", "spyc", "a-long-project", "api"];
        for (room, most) in [(100, 100), (40, 100), (20, 100), (12, 100), (0, 9)] {
            for active in 0..labels.len() {
                let mut widths = std::collections::BTreeSet::new();
                for shift in 0..states.len() {
                    let list: Vec<_> = labels
                        .iter()
                        .enumerate()
                        .map(|(k, label)| (*label, states[(shift + k) % states.len()], k == active))
                        .collect();
                    widths.insert(width(&chips(&list).spans(room, most, &theme)));
                }
                let label = labels[active];
                assert_eq!(
                    widths.len(),
                    1,
                    "room={room} most={most} in {label}: {widths:?}"
                );
            }
        }
    }

    /// The segment never widens as the bar narrows, and keeps to the room
    /// but for the narrowest label it is cut to: each step is no wider than
    /// the one before it.
    #[test]
    fn the_segment_never_widens_as_the_bar_narrows() {
        let theme = Theme::default();
        let sets = [
            chips(&[
                ("spyc", Some(Blocked), false),
                ("tripstack_platform", None, true),
            ]),
            chips(&[
                ("alpha", Some(Blocked), false),
                ("bravo", None, true),
                ("charlie", Some(Working), false),
                ("delta", None, false),
            ]),
            chips(&[
                ("web-api", None, true),
                ("web-app", Some(UnseenDone), false),
            ]),
        ];
        for c in &sets {
            let mut before = usize::MAX;
            for room in (0..=60).rev() {
                let drawn = c.spans(room, 200, &theme);
                let w = width(&drawn);
                assert!(w <= before, "room={room}: {} after {before}", text(&drawn));
                assert!(w <= room.max(MIN_CUT + 2), "room={room}: {}", text(&drawn));
                before = w;
            }
        }
    }

    /// Where every chip doesn't fit whole, each keeps its chip, the longer
    /// labels cut to one length in the middle, so siblings sharing a start
    /// stay told apart; only below a cut of `MIN_CUT` columns do the chips
    /// collapse, the label in view cut to fit.
    #[test]
    fn a_middle_width_cuts_every_label_to_one_length() {
        let theme = Theme::default();
        let c = chips(&[
            ("spyc", Some(Blocked), false),
            ("tripstack_platform", None, true),
            ("fika", Some(Working), false),
        ]);
        let at = |room| text(&c.spans(room, 100, &theme));
        let p = "\u{e0b1}";
        assert_eq!(
            at(38),
            format!("spyc ■ {p} tripstack_platform   {p} fika ●")
        );
        assert_eq!(at(30), format!("spyc ■ {p} tri\u{2026}atform   {p} fika ●"));
        assert_eq!(at(24), format!("spyc ■ {p} t\u{2026}rm   {p} fika ●"));
        assert_eq!(at(23), "trips\u{2026}ck_platform 2/3 ■");
        let siblings = chips(&[
            ("web-api", None, false),
            ("web-app", None, true),
            ("docs", None, false),
        ]);
        let cut = text(&siblings.spans(25, 100, &theme));
        assert_eq!(cut, format!("w\u{2026}pi   {p} w\u{2026}pp   {p} docs  "));
    }

    /// Projects are parted by a chevron, powerline's own between the parts of
    /// one segment, or by a bar in mono, where the bar draws no powerline
    /// glyphs.
    #[test]
    fn the_chips_are_parted_by_a_chevron() {
        let c = chips(&[("web", Some(Blocked), false), ("spyc", None, true)]);
        let theme = Theme::default();
        assert_eq!(
            text(&c.spans(100, 100, &theme)),
            "web \u{25a0} \u{e0b1} spyc  "
        );
        let mono = Theme {
            mono: true,
            ..Theme::default()
        };
        assert_eq!(text(&c.spans(100, 100, &mono)), "web \u{25a0} | spyc  ");
    }

    /// The order the segment ranks by: the user's turn above the agent's.
    #[test]
    fn blocked_outranks_an_unseen_done_outranks_work() {
        assert!(Blocked > UnseenDone && UnseenDone > Working && Working > Waiting);
    }
}

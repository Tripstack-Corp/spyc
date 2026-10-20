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

impl ProjectChips {
    /// The segment in at most `room` columns, the first that fits of: every
    /// chip; the project in view and those with something to say, the rest
    /// counted as `+N`; the project in view with its place among them (`2/4`)
    /// and the others' most urgent glyph. Else the project in view's label,
    /// with a red `■` while another project is blocked, however short the
    /// room: a blocked agent elsewhere is what the segment is for. `most` is
    /// where the bar ends, so the label is cut rather than the `■`.
    pub fn spans(&self, room: usize, most: usize, theme: &Theme) -> Vec<Span<'static>> {
        [
            self.every_chip(theme),
            self.loud_chips(theme),
            self.collapsed(theme),
        ]
        .into_iter()
        .find(|form| width(form) <= room)
        .unwrap_or_else(|| self.alone(most, theme))
    }

    fn every_chip(&self, theme: &Theme) -> Vec<Span<'static>> {
        self.joined(self.chips.iter(), theme)
    }

    fn loud_chips(&self, theme: &Theme) -> Vec<Span<'static>> {
        let (shown, quiet): (Vec<&ProjectChip>, Vec<&ProjectChip>) = self
            .chips
            .iter()
            .partition(|c| c.active || c.attention.is_some());
        let mut spans = self.joined(shown.into_iter(), theme);
        if !quiet.is_empty() {
            spans.push(dim(format!(" +{}", quiet.len())));
        }
        spans
    }

    fn collapsed(&self, theme: &Theme) -> Vec<Span<'static>> {
        // The project in view is always one of the open ones.
        let at = self.chips.iter().position(|c| c.active).unwrap_or(0);
        let mut spans = self.active_label();
        spans.push(Span::raw(format!(" {}/{}", at + 1, self.chips.len())));
        let others = self.others().filter_map(|c| c.attention).max();
        if let Some(attention) = others {
            spans.push(Span::raw(" "));
            spans.push(glyph(attention, self.pulse, theme));
        }
        spans
    }

    fn alone(&self, most: usize, theme: &Theme) -> Vec<Span<'static>> {
        let mut spans = self.active_label();
        if self
            .others()
            .any(|c| c.attention == Some(ProjectAttention::Blocked))
        {
            let keep = most.saturating_sub(2);
            for label in &mut spans {
                if super::display_width(&label.content) > keep {
                    let cut = super::display_truncate(&label.content, keep.saturating_sub(1));
                    let cut = if keep == 0 {
                        String::new()
                    } else {
                        format!("{cut}\u{2026}")
                    };
                    label.content = cut.into();
                }
            }
            spans.push(Span::raw(" "));
            spans.push(glyph(ProjectAttention::Blocked, self.pulse, theme));
        }
        spans
    }

    /// `chips` as `label glyph`, joined by ` · `: the one in view bold, the
    /// others dim.
    fn joined<'c>(
        &self,
        chips: impl Iterator<Item = &'c ProjectChip>,
        theme: &Theme,
    ) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for (i, chip) in chips.enumerate() {
            if i > 0 {
                spans.push(dim(" \u{b7} ".to_string()));
            }
            spans.push(label(chip));
            if let Some(attention) = chip.attention {
                spans.push(Span::raw(" "));
                spans.push(glyph(attention, self.pulse, theme));
            }
        }
        spans
    }

    fn active_label(&self) -> Vec<Span<'static>> {
        self.chips
            .iter()
            .find(|c| c.active)
            .map(label)
            .into_iter()
            .collect()
    }

    fn others(&self) -> impl Iterator<Item = &ProjectChip> {
        self.chips.iter().filter(|c| !c.active)
    }
}

fn label(chip: &ProjectChip) -> Span<'static> {
    let modifier = if chip.active {
        Modifier::BOLD
    } else {
        Modifier::DIM
    };
    Span::styled(chip.label.clone(), Style::default().add_modifier(modifier))
}

fn dim(text: String) -> Span<'static> {
    Span::styled(text, Style::default().add_modifier(Modifier::DIM))
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

    /// `x` blocked, `y` in view, `z` working and `v` quiet.
    fn four() -> ProjectChips {
        chips(&[
            ("x", Some(Blocked), false),
            ("y", None, true),
            ("z", Some(Working), false),
            ("v", None, false),
        ])
    }

    /// Each step is taken only when the one before doesn't fit, and the last
    /// is drawn whatever the room.
    #[test]
    fn the_segment_narrows_one_step_at_a_time() {
        let theme = Theme::default();
        let c = four();
        let at = |room| text(&c.spans(room, 100, &theme));
        assert_eq!(at(100), "x ■ · y · z ● · v");
        assert_eq!(at(17), "x ■ · y · z ● · v");
        assert_eq!(at(16), "x ■ · y · z ● +1");
        assert_eq!(at(15), "y 2/4 ■");
        assert_eq!(at(7), "y 2/4 ■");
        assert_eq!(at(6), "y ■");
        assert_eq!(at(0), "y ■");
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
        let collapsed = c.collapsed(&theme);
        assert_eq!(text(&collapsed), "y 2/3 ■");
        assert_eq!(collapsed.last().map(|s| s.style.fg), Some(Some(theme.take)));
        assert_eq!(text(&c.alone(100, &theme)), "y");
        let quiet = chips(&[("x", None, false), ("y", None, true)]);
        assert_eq!(text(&quiet.collapsed(&theme)), "y 2/2");
    }

    /// Where the bar ends short of the label and its `■`, the label is cut
    /// and the `■` kept; with nothing blocked elsewhere, the label is whole.
    #[test]
    fn the_narrowest_step_cuts_the_label_not_the_block() {
        let theme = Theme::default();
        let c = chips(&[("a-long-project", None, true), ("x", Some(Blocked), false)]);
        assert_eq!(text(&c.spans(0, 16, &theme)), "a-long-project ■");
        assert_eq!(text(&c.spans(0, 9, &theme)), "a-long… ■");
        assert_eq!(text(&c.spans(0, 3, &theme)), "… ■");
        assert_eq!(text(&c.spans(0, 2, &theme)), " ■");
        let quiet = chips(&[("a-long-project", None, true), ("x", Some(Working), false)]);
        assert_eq!(text(&quiet.spans(0, 9, &theme)), "a-long-project");
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

    /// The order the segment ranks by: the user's turn above the agent's.
    #[test]
    fn blocked_outranks_an_unseen_done_outranks_work() {
        assert!(Blocked > UnseenDone && UnseenDone > Working && Working > Waiting);
    }
}

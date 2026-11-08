//! Where each segment of the drawn status bar is: one span per segment
//! (`ui::status`), and within the `projects` segment one per project label and
//! the collapsed count (`ui::project_chips`). The pass that draws the bar
//! measures it, and the mouse reads the spans recorded with the drawn row, so
//! a click and the text it lands on can't disagree about where a label is.

/// What a span of the status bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSegment {
    /// `PROJECT_HOME`'s basename, while one project is open.
    Project,
    /// Every open project's chip, while several are.
    Projects,
    /// Open project `n`'s label among the chips, 0-based in switcher order.
    ProjectLabel(usize),
    /// The collapsed chips' place of the project in view (`2/4`).
    ProjectCount,
    Session,
    Path,
    /// A folder in the path, this many levels above the one the column is in.
    PathUp(usize),
    Git,
    /// The active pane tab's agent.
    Agent,
    /// The column's state (`[picks:2]`).
    Suffix,
}

/// A segment's display columns `start..end` from the bar's left edge, end
/// exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSpan {
    pub segment: StatusSegment,
    pub start: usize,
    pub end: usize,
}

/// The segment drawn at display column `col` of the bar: the narrowest span
/// there, so a label wins over the `projects` segment it is part of.
pub fn segment_at(spans: &[SegmentSpan], col: usize) -> Option<StatusSegment> {
    spans
        .iter()
        .filter(|s| (s.start..s.end).contains(&col))
        .min_by_key(|s| s.end - s.start)
        .map(|s| s.segment)
}

/// Where each folder of `full` is drawn, as [`StatusSegment::PathUp`] spans,
/// when the bar draws it as `drawn` from column `from`: `full` whole, or cut in
/// its middle at `cut` (`ui::status::truncate_middle`), or only its tail. The
/// folder the column is in has none, and nor does a name the cut shortened. A
/// leading `/` is the root's.
pub fn path_folder_spans(full: &str, drawn: &str, cut: &str, from: usize) -> Vec<SegmentSpan> {
    // `full`'s bytes drawn whole before the cut (`..head`) and after it
    // (`tail..`), and the column the part after the cut starts at.
    let (head, tail, tail_col) = if drawn == full {
        (full.len(), full.len(), 0)
    } else if let Some((h, t)) = drawn
        .split_once(cut)
        .filter(|(h, t)| full.starts_with(h) && full.ends_with(t))
    {
        let (head, tail) = (h.len(), full.len() - t.len());
        (
            head,
            tail,
            super::display_width(h) + super::display_width(cut),
        )
    } else if full.ends_with(drawn) {
        (0, full.len() - drawn.len(), 0)
    } else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut at = 0;
    for name in full.split('/') {
        names.push((at, at + name.len()));
        at += name.len() + 1;
    }
    let total = names.len();
    let mut spans = Vec::new();
    for (i, &(a, b)) in names.iter().enumerate() {
        let up = total - 1 - i;
        let (a, b) = match (a == b, i) {
            (false, _) => (a, b),
            (true, 0) if full.starts_with('/') => (0, 1),
            (true, _) => continue,
        };
        let col = if b <= head {
            super::display_width(&full[..a])
        } else if a >= tail {
            tail_col + super::display_width(&full[tail..a])
        } else {
            continue;
        };
        if up > 0 {
            let start = from + col;
            spans.push(SegmentSpan {
                segment: StatusSegment::PathUp(up),
                start,
                end: start + super::display_width(&full[a..b]),
            });
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::{SegmentSpan, StatusSegment, path_folder_spans, segment_at};
    use crate::ui::project_chips::{ProjectAttention, ProjectChip, ProjectChips};
    use crate::ui::status::StatusBar;
    use crate::ui::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::widgets::Paragraph;

    /// A label wins over the segment it is drawn in; off every span is
    /// nothing.
    #[test]
    fn the_narrowest_span_answers() {
        let spans = [
            SegmentSpan {
                segment: StatusSegment::Projects,
                start: 4,
                end: 20,
            },
            SegmentSpan {
                segment: StatusSegment::ProjectLabel(1),
                start: 10,
                end: 14,
            },
        ];
        assert_eq!(segment_at(&spans, 3), None);
        assert_eq!(segment_at(&spans, 4), Some(StatusSegment::Projects));
        assert_eq!(segment_at(&spans, 10), Some(StatusSegment::ProjectLabel(1)));
        assert_eq!(segment_at(&spans, 13), Some(StatusSegment::ProjectLabel(1)));
        assert_eq!(segment_at(&spans, 14), Some(StatusSegment::Projects));
        assert_eq!(segment_at(&spans, 20), None);
    }

    fn chips(labels: &[&str]) -> ProjectChips {
        ProjectChips {
            chips: labels
                .iter()
                .enumerate()
                .map(|(i, label)| ProjectChip {
                    label: (*label).to_string(),
                    attention: (i == 1).then_some(ProjectAttention::Blocked),
                    active: i == 0,
                })
                .collect(),
            pulse: Color::Red,
        }
    }

    /// The bar's cells as a terminal draws `bar` at `width`, and its spans.
    fn drawn(bar: &StatusBar, width: u16) -> (Vec<String>, Vec<SegmentSpan>) {
        let area = Rect::new(0, 0, width, 1);
        let status = bar.build(area);
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|f| f.render_widget(Paragraph::new(status.line.clone()), area))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let cells = (0..width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        (cells, status.segments)
    }

    /// Whether `drawn` is `label`, or `label` cut in its middle.
    fn is_label(drawn: &str, label: &str) -> bool {
        drawn == label
            || drawn.split_once('\u{2026}').is_some_and(|(head, tail)| {
                !drawn.is_empty() && label.starts_with(head) && label.ends_with(tail)
            })
    }

    /// At every width the chips narrow through (every chip whole, the labels
    /// cut, collapsed to `1/4`, the project in view alone), in colour and in
    /// mono, each label's span covers its label as drawn, no more and no
    /// less, and the count's covers `1/4`; the git, agent and session
    /// segments' spans hold their text.
    #[test]
    fn each_span_covers_exactly_what_is_drawn_there() {
        let labels = ["alpha", "bravo", "charlie", "delta"];
        let projects = chips(&labels);
        for mono in [false, true] {
            let theme = Theme {
                mono,
                ..Theme::default()
            };
            let mut seen = std::collections::BTreeSet::new();
            for width in [200u16, 120, 90, 60, 45, 30] {
                let bar = StatusBar {
                    project_home: Some("alpha"),
                    session_name: Some("SAFFRON_CUMIN"),
                    projects: Some(&projects),
                    path: "~/src/alpha",
                    suffix: "[picks:1]",
                    suffix_short: None,
                    git_info: Some("main*"),
                    agent_info: Some("claude"),
                    theme: &theme,
                    plain_logo: false,
                    stderr_notice: Some("\u{26a0}2"),
                    teach_menu: true,
                };
                let (cells, spans) = drawn(&bar, width);
                let text = |s: &SegmentSpan| cells[s.start..s.end].concat();
                let row = cells.concat();
                let mut shape = (0, false);
                for span in &spans {
                    match span.segment {
                        StatusSegment::ProjectLabel(i) => {
                            shape.0 += 1;
                            assert!(
                                is_label(&text(span), labels[i]),
                                "mono={mono} width={width}: {span:?} holds {:?}: {row:?}",
                                text(span)
                            );
                            let before = cells[span.start - 1].as_str();
                            let after = cells.get(span.end).map_or(" ", String::as_str);
                            assert_eq!(
                                (before, after),
                                (" ", " "),
                                "mono={mono} width={width}: {span:?} ends at the label: {row:?}"
                            );
                        }
                        StatusSegment::ProjectCount => {
                            shape.1 = true;
                            assert_eq!(text(span), "1/4", "mono={mono} width={width}");
                        }
                        _ => {}
                    }
                }
                seen.insert(shape);
                if !mono {
                    for (segment, holds) in [
                        (StatusSegment::Git, "main*"),
                        (StatusSegment::Agent, "claude"),
                    ] {
                        // A segment past the bar's end is cut by the terminal.
                        let shown = |s: &&SegmentSpan| s.segment == segment && s.end <= cells.len();
                        if let Some(span) = spans.iter().find(shown) {
                            assert!(
                                text(span).contains(holds),
                                "mono={mono} width={width}: {span:?}: {row:?}"
                            );
                        }
                    }
                }
            }
            assert!(
                seen.contains(&(4, false)),
                "mono={mono}: every chip: {seen:?}"
            );
            assert!(
                seen.contains(&(1, true)),
                "mono={mono}: collapsed: {seen:?}"
            );
            assert!(seen.contains(&(1, false)), "mono={mono}: alone: {seen:?}");
        }
    }

    /// With one project open its segment and the session's are spans of
    /// their own, in colour and in mono.
    #[test]
    fn one_projects_segment_and_the_session_hold_their_text() {
        for mono in [false, true] {
            let theme = Theme {
                mono,
                ..Theme::default()
            };
            let bar = StatusBar {
                project_home: Some("alpha"),
                session_name: Some("SAFFRON_CUMIN"),
                projects: None,
                path: "~/src/alpha",
                suffix: "",
                suffix_short: None,
                git_info: None,
                agent_info: None,
                theme: &theme,
                plain_logo: true,
                stderr_notice: None,
                teach_menu: false,
            };
            let (cells, spans) = drawn(&bar, 100);
            let at = |segment| {
                let span = spans
                    .iter()
                    .find(|s| s.segment == segment)
                    .unwrap_or_else(|| panic!("mono={mono}: no {segment:?} in {spans:?}"));
                cells[span.start..span.end].concat()
            };
            assert_eq!(
                at(StatusSegment::Project)
                    .trim_end_matches('\u{e0b0}')
                    .trim(),
                "alpha"
            );
            assert_eq!(
                at(StatusSegment::Session)
                    .trim_end_matches('\u{e0b0}')
                    .trim(),
                "SAFFRON_CUMIN"
            );
            assert!(
                at(StatusSegment::Path).contains("~/src/alpha"),
                "mono={mono}"
            );
        }
    }

    /// Each folder's `(text, levels up)`, read back off `drawn` by its spans.
    fn folders(full: &str, drawn: &str) -> Vec<(String, usize)> {
        let cols: Vec<char> = drawn.chars().collect();
        path_folder_spans(full, drawn, "...", 0)
            .into_iter()
            .map(|s| {
                let StatusSegment::PathUp(up) = s.segment else {
                    panic!("{s:?}")
                };
                (cols[s.start..s.end].iter().collect(), up)
            })
            .collect()
    }

    fn owned(want: &[(&str, usize)]) -> Vec<(String, usize)> {
        want.iter().map(|(t, n)| ((*t).to_string(), *n)).collect()
    }

    /// Every folder but the last goes up as far as it is from the end; the
    /// root is the leading `/`, and `~` is one folder however deep home is.
    #[test]
    fn each_folder_goes_up_as_far_as_it_is_from_the_end() {
        let full = "/usr/local/lib";
        assert_eq!(
            folders(full, full),
            owned(&[("/", 3), ("usr", 2), ("local", 1)])
        );
        let home = "~/src/spyc";
        assert_eq!(folders(home, home), owned(&[("~", 2), ("src", 1)]));
        assert_eq!(folders("~", "~"), owned(&[]));
        assert_eq!(folders("<INVENTORY>", "<INVENTORY>"), owned(&[]));
    }

    /// Cut in the middle, the folders whole on either side of the cut keep
    /// their levels, and the one the cut shortened has none.
    #[test]
    fn a_cut_path_keeps_the_folders_it_draws_whole() {
        let full = "/alpha/bravo/charlie/delta/echo";
        assert_eq!(
            folders(full, "/alpha/br...ie/delta/echo"),
            owned(&[("/", 5), ("alpha", 4), ("delta", 1)])
        );
        // Only the tail.
        assert_eq!(folders(full, "lie/delta/echo"), owned(&[("delta", 1)]));
        // A drawing that isn't `full` cut has none.
        assert_eq!(folders(full, "/other/echo"), owned(&[]));
    }

    /// A wide name moves the columns of what follows it by its width.
    #[test]
    fn columns_are_display_columns() {
        let full = "/日本/docs/x";
        let spans = path_folder_spans(full, full, "...", 10);
        let docs = spans
            .iter()
            .find(|s| s.segment == StatusSegment::PathUp(1))
            .unwrap();
        assert_eq!((docs.start, docs.end), (10 + 6, 10 + 10));
    }
}

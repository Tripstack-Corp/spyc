use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::ui::theme;

pub struct StatusBar<'a> {
    pub user_host: &'a str,
    pub path: &'a str,
    /// Optional trailing state, e.g. `[picks:2 inv:5 m1:on m2:on]`.
    pub suffix: &'a str,
}

const ELLIPSIS: &str = "...";

impl StatusBar<'_> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let avail = area.width as usize;

        let host_w = self.user_host.chars().count() + 2; // ": "
        let suffix_w = if self.suffix.is_empty() {
            0
        } else {
            2 + self.suffix.chars().count()
        };

        let path_budget = avail.saturating_sub(host_w + suffix_w);
        let (path_disp, suffix_disp) = if path_budget >= 8 {
            (
                truncate_middle(self.path, path_budget),
                self.suffix.to_string(),
            )
        } else {
            let budget = avail.saturating_sub(host_w);
            (truncate_middle(self.path, budget), String::new())
        };

        let mut spans = vec![
            Span::styled(
                format!("{}: ", self.user_host),
                Style::default()
                    .fg(theme::STATUS_USER)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(path_disp, Style::default().fg(theme::STATUS_PATH)),
        ];
        if !suffix_disp.is_empty() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                suffix_disp,
                Style::default().fg(theme::STATUS_SUFFIX),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

pub fn truncate_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    if n <= max {
        return s.to_string();
    }
    if max <= ELLIPSIS.chars().count() {
        return chars[n - max..].iter().collect();
    }
    let budget = max - ELLIPSIS.chars().count();
    let head = budget / 3;
    let tail = budget - head;
    let mut out = String::with_capacity(max);
    out.extend(chars[..head].iter());
    out.push_str(ELLIPSIS);
    out.extend(chars[n - tail..].iter());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_path_unchanged() {
        assert_eq!(truncate_middle("/a/b", 20), "/a/b");
    }

    #[test]
    fn middle_truncation_favours_tail() {
        let s = "/Users/derek/src/cspy/a/b/c/very_long_directory_name";
        let out = truncate_middle(s, 25);
        assert_eq!(out.chars().count(), 25);
        assert!(out.starts_with("/User"));
        assert!(out.contains("..."));
        assert!(out.ends_with("directory_name") || out.ends_with("ctory_name"));
    }

    #[test]
    fn degenerate_budget_keeps_tail() {
        let out = truncate_middle("/very/long/path", 3);
        assert_eq!(out, "ath");
    }
}

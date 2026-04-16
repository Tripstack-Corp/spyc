use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::ui::theme::Theme;

pub struct StatusBar<'a> {
    pub user_host: &'a str,
    pub path: &'a str,
    /// Optional trailing state, e.g. `[picks:2 inv:5 m1:on m2:on]`.
    pub suffix: &'a str,
    /// Git branch + dirty flag, e.g. `"main*"` or `None` if not in a repo.
    pub git_info: Option<&'a str>,
    pub theme: &'a Theme,
}

/// Powerline right-pointing triangle (requires a Nerd Font or powerline-patched font).
const PL_SEP: &str = "\u{e0b0}";

const ELLIPSIS: &str = "...";

impl StatusBar<'_> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if self.theme.mono {
            self.render_plain(frame, area);
            return;
        }

        let avail = area.width as usize;

        // Segment background/foreground colors.
        let host_bg = self.theme.status_user;                 // lavender
        let host_fg = Color::Rgb(0x1a, 0x1b, 0x26);          // tokyo night bg

        let path_bg = Color::Rgb(0x3b, 0x40, 0x61);          // muted indigo
        let path_fg = self.theme.status_path;                  // near-white

        let git_bg = Color::Rgb(0x2a, 0x2e, 0x45);           // slightly lighter than suffix
        let git_fg = self.theme.exec;                          // soft green

        let suffix_bg = Color::Rgb(0x24, 0x28, 0x3b);        // darker
        let suffix_fg = self.theme.status_suffix;

        let term_bg = Color::Reset;

        // Build fixed-width segments first so path gets the remainder.
        let host_text = format!(" {} ", self.user_host);

        let git_text = self.git_info.map(|g| format!(" \u{e0a0} {g} ")); // branch icon
        let git_w = git_text.as_ref().map_or(0, |s| s.len() + 1); // +1 for sep

        let has_suffix = !self.suffix.is_empty();
        let suffix_text = if has_suffix {
            format!(" {} ", self.suffix)
        } else {
            String::new()
        };
        let suffix_w = if has_suffix { suffix_text.len() + 1 } else { 0 };

        // Count separators: host→path + path→(git|suffix|term) + optional git→suffix + optional suffix→term
        let sep_count = 1 + 1
            + if git_text.is_some() && has_suffix { 1 } else { 0 }
            + if git_text.is_some() && !has_suffix { 0 } else { 0 };
        let fixed = host_text.len() + git_w + suffix_w + sep_count;
        let path_budget = avail.saturating_sub(fixed + 1); // +1 for path→next sep
        let path_text = format!(" {} ", truncate_middle(self.path, path_budget.saturating_sub(2)));

        let mut spans: Vec<Span> = Vec::new();

        // Segment 1: user@host
        spans.push(Span::styled(
            &host_text,
            Style::default().fg(host_fg).bg(host_bg).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(PL_SEP, Style::default().fg(host_bg).bg(path_bg)));

        // Segment 2: path
        let path_next_bg = if git_text.is_some() { git_bg }
            else if has_suffix { suffix_bg }
            else { term_bg };
        spans.push(Span::styled(
            &path_text,
            Style::default().fg(path_fg).bg(path_bg),
        ));
        spans.push(Span::styled(PL_SEP, Style::default().fg(path_bg).bg(path_next_bg)));

        // Segment 3 (optional): git branch
        if let Some(ref gt) = git_text {
            let git_next_bg = if has_suffix { suffix_bg } else { term_bg };
            spans.push(Span::styled(
                gt.as_str(),
                Style::default().fg(git_fg).bg(git_bg).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(PL_SEP, Style::default().fg(git_bg).bg(git_next_bg)));
        }

        // Segment 4 (optional): suffix (picks/inv/masks)
        if has_suffix {
            spans.push(Span::styled(
                &suffix_text,
                Style::default().fg(suffix_fg).bg(suffix_bg),
            ));
            spans.push(Span::styled(PL_SEP, Style::default().fg(suffix_bg).bg(term_bg)));
        }

        // Fill remaining width.
        let used: usize = host_text.len() + 1 + path_text.len() + 1
            + git_text.as_ref().map_or(0, |s| s.len() + 1)
            + if has_suffix { suffix_text.len() + 1 } else { 0 };
        if used < avail {
            spans.push(Span::styled(
                " ".repeat(avail - used),
                Style::default().bg(term_bg),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// Fallback plain rendering for mono mode.
    fn render_plain(&self, frame: &mut Frame, area: Rect) {
        let avail = area.width as usize;

        let host_w = self.user_host.chars().count() + 2;
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
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(path_disp),
        ];
        if !suffix_disp.is_empty() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                suffix_disp,
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

use ratatui::style::Color;

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

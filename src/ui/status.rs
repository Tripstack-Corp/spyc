use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
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
    #[allow(clippy::similar_names)]
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if self.theme.mono {
            self.render_plain(frame, area);
            return;
        }

        let avail = area.width as usize;

        // Segment background/foreground colors.
        let host_bg = self.theme.status_user; // lavender
        let host_fg = Color::Rgb(0x1a, 0x1b, 0x26); // tokyo night bg

        let path_bg = Color::Rgb(0x3b, 0x40, 0x61); // muted indigo
        let path_fg = self.theme.status_path; // near-white

        let git_bg = Color::Rgb(0x2a, 0x2e, 0x45); // slightly lighter than suffix
        let git_fg = self.theme.exec; // soft green

        let suffix_bg = Color::Rgb(0x24, 0x28, 0x3b); // darker
        let suffix_fg = self.theme.status_suffix;

        let term_bg = Color::Reset;

        // Build fixed-width segments first so path gets the remainder.
        let host_text = format!(" \u{1f336}\u{fe0f} {} ", self.user_host);

        let git_text = self.git_info.map(|g| format!(" \u{e0a0} {g} ")); // branch icon
        let git_w = git_text.as_ref().map_or(0, |s| dw(s) + 1); // +1 for sep

        let has_suffix = !self.suffix.is_empty();
        let suffix_text = if has_suffix {
            format!(" {} ", self.suffix)
        } else {
            String::new()
        };
        let suffix_w = if has_suffix { dw(&suffix_text) + 1 } else { 0 };

        // Count separators: host→path + path→(git|suffix|term) + optional git→suffix
        let sep_count = 1 + 1 + usize::from(git_text.is_some() && has_suffix);
        let fixed = dw(&host_text) + git_w + suffix_w + sep_count;
        let path_budget = avail.saturating_sub(fixed + 1); // +1 for path→next sep
        let path_text = format!(
            " {} ",
            truncate_middle(self.path, path_budget.saturating_sub(2))
        );

        let mut spans: Vec<Span> = Vec::new();

        // Segment 1: user@host
        spans.push(Span::styled(
            &host_text,
            Style::default()
                .fg(host_fg)
                .bg(host_bg)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            PL_SEP,
            Style::default().fg(host_bg).bg(path_bg),
        ));

        // Segment 2: path
        let path_next_bg = if git_text.is_some() {
            git_bg
        } else if has_suffix {
            suffix_bg
        } else {
            term_bg
        };
        spans.push(Span::styled(
            &path_text,
            Style::default().fg(path_fg).bg(path_bg),
        ));
        spans.push(Span::styled(
            PL_SEP,
            Style::default().fg(path_bg).bg(path_next_bg),
        ));

        // Segment 3 (optional): git branch
        if let Some(ref gt) = git_text {
            let git_next_bg = if has_suffix { suffix_bg } else { term_bg };
            spans.push(Span::styled(
                gt.as_str(),
                Style::default()
                    .fg(git_fg)
                    .bg(git_bg)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                PL_SEP,
                Style::default().fg(git_bg).bg(git_next_bg),
            ));
        }

        // Segment 4 (optional): suffix (picks/inv/masks)
        if has_suffix {
            spans.push(Span::styled(
                &suffix_text,
                Style::default().fg(suffix_fg).bg(suffix_bg),
            ));
            spans.push(Span::styled(
                PL_SEP,
                Style::default().fg(suffix_bg).bg(term_bg),
            ));
        }

        // Fill remaining width.
        let used: usize = dw(&host_text)
            + 1
            + dw(&path_text)
            + 1
            + git_text.as_ref().map_or(0, |s| dw(s) + 1)
            + if has_suffix { dw(&suffix_text) + 1 } else { 0 };
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

        let host_w = dw(self.user_host) + 2 + 3; // +3 for "🌶️ "
        let suffix_w = if self.suffix.is_empty() {
            0
        } else {
            2 + dw(self.suffix)
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
                format!("\u{1f336}\u{fe0f} {}: ", self.user_host),
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

pub fn truncate_middle(s: &str, max: usize) -> String {
    if dw(s) <= max {
        return s.to_string();
    }
    let ellipsis_w = dw(ELLIPSIS);
    if max <= ellipsis_w {
        // Degenerate: just return the tail that fits.
        return display_take_tail(s, max);
    }
    let budget = max - ellipsis_w;
    let head_budget = budget / 3;
    let tail_budget = budget - head_budget;
    let head = super::display_truncate(s, head_budget);
    let tail = display_take_tail(s, tail_budget);
    format!("{head}{ELLIPSIS}{tail}")
}

/// Take the last `max` display columns from a string.
fn display_take_tail(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut width = 0;
    let mut start = chars.len();
    for i in (0..chars.len()).rev() {
        let cw = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(0);
        if width + cw > max {
            break;
        }
        width += cw;
        start = i;
    }
    chars[start..].iter().collect()
}

/// Shorthand for `super::display_width(s)`.
fn dw(s: &str) -> usize {
    super::display_width(s)
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
        let s = "/Users/derek/src/spyc/a/b/c/very_long_directory_name";
        let out = truncate_middle(s, 25);
        assert_eq!(dw(&out), 25);
        assert!(out.starts_with("/User"));
        assert!(out.contains("..."));
        assert!(out.ends_with("directory_name") || out.ends_with("ctory_name"));
    }

    #[test]
    fn degenerate_budget_keeps_tail() {
        let out = truncate_middle("/very/long/path", 3);
        assert_eq!(out, "ath");
    }

    // ── snapshot tests (TestBackend) ──────────────────────────────

    use ratatui::{Terminal, backend::TestBackend};

    fn render_status_to_string(
        user_host: &str,
        path: &str,
        suffix: &str,
        git_info: Option<&str>,
        mono: bool,
        width: u16,
    ) -> String {
        let backend = TestBackend::new(width, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = if mono {
            let mut t = Theme::default();
            t.mono = true;
            t
        } else {
            Theme::default()
        };
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, width, 1);
                let bar = StatusBar {
                    user_host,
                    path,
                    suffix,
                    git_info,
                    theme: &theme,
                };
                bar.render(f, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for x in 0..buf.area.width {
            out.push_str(buf.cell((x, 0)).map_or(" ", |c| c.symbol()));
        }
        out.trim_end().to_string()
    }

    #[test]
    fn snapshot_status_mono_basic() {
        let out = render_status_to_string("derek@mac", "/tmp", "", None, true, 60);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_mono_with_suffix() {
        let out = render_status_to_string(
            "derek@mac",
            "/home/derek/src",
            "[picks:2 m1:on]",
            None,
            true,
            60,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_basic() {
        let out = render_status_to_string("derek@mac", "/tmp", "", None, false, 80);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_with_git() {
        let out = render_status_to_string(
            "derek@mac",
            "/home/src",
            "[picks:1]",
            Some("main*"),
            false,
            80,
        );
        insta::assert_snapshot!(out);
    }
}

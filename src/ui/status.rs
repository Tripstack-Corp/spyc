use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::theme::Theme;

pub struct StatusBar<'a> {
    /// Basename of `PROJECT_HOME` if set (e.g. `spyc`). Hidden when `None`.
    pub project_home: Option<&'a str>,
    /// Session display name (e.g. `SAFFRON_CUMIN`). Hidden when `None`.
    pub session_name: Option<&'a str>,
    pub path: &'a str,
    /// Optional trailing state, e.g. `[picks:2 inv:5 m1:on m2:on]`.
    pub suffix: &'a str,
    /// Git branch + dirty flag, e.g. `"main*"` or `None` if not in a repo.
    pub git_info: Option<&'a str>,
    /// Active pane's agent identity (e.g. `"claude:76422c62"` /
    /// `"gemini:4c130f82"` / `"codex"`). `None` when no pane is open
    /// or its command isn't a known agent. Rendered as its own
    /// segment between `git` and `suffix` so it sits in roughly the
    /// same visual band as related state.
    pub agent_info: Option<&'a str>,
    pub theme: &'a Theme,
}

/// Powerline right-pointing triangle (requires a Nerd Font or powerline-patched font).
const PL_SEP: &str = "\u{e0b0}";

const ELLIPSIS: &str = "...";

fn push_segment(spans: &mut Vec<Span>, text: &str, fg: Color, bg: Color, next_bg: Color) {
    spans.push(Span::styled(
        text.to_string(),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(PL_SEP, Style::default().fg(bg).bg(next_bg)));
}

impl StatusBar<'_> {
    #[allow(clippy::similar_names)]
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if self.theme.mono {
            self.render_plain(frame, area);
            return;
        }

        let avail = area.width as usize;

        // Segment background/foreground colors.
        let proj_bg = Color::Rgb(0x7a, 0x4d, 0x3a); // warm brown (pepper-ish)
        let proj_fg = Color::Rgb(0xf2, 0xe8, 0xcf); // cream

        let sess_bg = self.theme.status_user; // lavender (reused)
        let sess_fg = Color::Rgb(0x1a, 0x1b, 0x26); // tokyo night bg

        let path_bg = Color::Rgb(0x3b, 0x40, 0x61); // muted indigo
        let path_fg = self.theme.status_path;

        let git_bg = Color::Rgb(0x2a, 0x2e, 0x45); // slightly lighter than suffix
        let git_fg = self.theme.exec; // soft green

        // Agent segment sits between git and suffix. Uses the pick
        // accent so it visually distinguishes from the
        // git/file-state band.
        let agent_bg = Color::Rgb(0x32, 0x36, 0x52);
        let agent_fg = self.theme.pick;

        let suffix_bg = Color::Rgb(0x24, 0x28, 0x3b); // darker
        let suffix_fg = self.theme.status_suffix;

        let term_bg = Color::Reset;

        // Pepper emoji gets its own span with terminal-default bg so the
        // emoji color rendering isn't clipped by the segment background.
        let emoji_text = " \u{1f336}\u{fe0f} ";
        let emoji_w = 4; // space + 🌶️ (2 cols) + space

        let project_text = self.project_home.map(|p| format!(" {p} "));
        let session_text = self.session_name.map(|n| format!(" {n} "));
        let git_text = self.git_info.map(|g| format!(" \u{e0a0} {g} "));
        let agent_text = self.agent_info.map(|a| format!(" \u{f120} {a} ")); // 󰰠 terminal-shell-like glyph
        let suffix_text = (!self.suffix.is_empty()).then(|| format!(" {} ", self.suffix));

        let width_of = |t: &Option<String>| t.as_deref().map_or(0, |s| dw(s) + 1);
        let project_w = width_of(&project_text);
        let session_w = width_of(&session_text);
        let git_w = width_of(&git_text);
        let agent_w = width_of(&agent_text);
        let suffix_w = width_of(&suffix_text);

        // path_budget = avail − (emoji + optional segments + path-sep).
        let fixed = emoji_w + project_w + session_w + git_w + agent_w + suffix_w + 1;
        let path_budget = avail.saturating_sub(fixed);
        let path_text = format!(
            " {} ",
            truncate_middle(self.path, path_budget.saturating_sub(2))
        );

        let path_next_bg = if git_text.is_some() {
            git_bg
        } else if agent_text.is_some() {
            agent_bg
        } else if suffix_text.is_some() {
            suffix_bg
        } else {
            term_bg
        };

        let mut spans: Vec<Span> = Vec::new();

        // Pepper (no background).
        spans.push(Span::styled(emoji_text, Style::default().bg(term_bg)));

        if let Some(ref text) = project_text {
            let next_bg = if session_text.is_some() {
                sess_bg
            } else {
                path_bg
            };
            push_segment(&mut spans, text, proj_fg, proj_bg, next_bg);
        }
        if let Some(ref text) = session_text {
            push_segment(&mut spans, text, sess_fg, sess_bg, path_bg);
        }

        // Path (always present).
        spans.push(Span::styled(
            &path_text,
            Style::default().fg(path_fg).bg(path_bg),
        ));
        spans.push(Span::styled(
            PL_SEP,
            Style::default().fg(path_bg).bg(path_next_bg),
        ));

        if let Some(ref text) = git_text {
            let next_bg = if agent_text.is_some() {
                agent_bg
            } else if suffix_text.is_some() {
                suffix_bg
            } else {
                term_bg
            };
            push_segment(&mut spans, text, git_fg, git_bg, next_bg);
        }
        if let Some(ref text) = agent_text {
            let next_bg = suffix_text.as_ref().map_or(term_bg, |_| suffix_bg);
            push_segment(&mut spans, text, agent_fg, agent_bg, next_bg);
        }
        if let Some(ref text) = suffix_text {
            push_segment(&mut spans, text, suffix_fg, suffix_bg, term_bg);
        }

        // Fill remaining width.
        let used: usize =
            emoji_w + project_w + session_w + dw(&path_text) + 1 + git_w + agent_w + suffix_w;
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

        let parts: Vec<&str> = [self.project_home, self.session_name]
            .into_iter()
            .flatten()
            .collect();
        let prefix = if parts.is_empty() {
            String::new()
        } else {
            format!("{}: ", parts.join(" "))
        };
        let pre_w = dw(&prefix);
        let suffix_w = if self.suffix.is_empty() {
            0
        } else {
            2 + dw(self.suffix)
        };

        let path_budget = avail.saturating_sub(pre_w + suffix_w);
        let (path_disp, suffix_disp) = if path_budget >= 8 {
            (
                truncate_middle(self.path, path_budget),
                self.suffix.to_string(),
            )
        } else {
            let budget = avail.saturating_sub(pre_w);
            (truncate_middle(self.path, budget), String::new())
        };

        let mut spans = vec![
            Span::styled(prefix, Style::default().add_modifier(Modifier::BOLD)),
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
        let s = "/Users/x/src/spyc/a/b/c/very_long_directory_name";
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

    #[allow(clippy::too_many_arguments)]
    fn render_status_to_string(
        project_home: Option<&str>,
        session_name: Option<&str>,
        path: &str,
        suffix: &str,
        git_info: Option<&str>,
        agent_info: Option<&str>,
        mono: bool,
        width: u16,
    ) -> String {
        let backend = TestBackend::new(width, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = if mono {
            Theme {
                mono: true,
                ..Theme::default()
            }
        } else {
            Theme::default()
        };
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, width, 1);
                let bar = StatusBar {
                    project_home,
                    session_name,
                    path,
                    suffix,
                    git_info,
                    agent_info,
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
        let out = render_status_to_string(
            None,
            Some("SAFFRON_CUMIN"),
            "/tmp",
            "",
            None,
            None,
            true,
            60,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_mono_with_suffix() {
        let out = render_status_to_string(
            Some("spyc"),
            Some("SAFFRON_CUMIN"),
            "/home/user/src",
            "[picks:2 m1:on]",
            None,
            None,
            true,
            60,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_basic() {
        let out = render_status_to_string(
            None,
            Some("SAFFRON_CUMIN"),
            "/tmp",
            "",
            None,
            None,
            false,
            80,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_with_git() {
        let out = render_status_to_string(
            Some("spyc"),
            Some("SAFFRON_CUMIN"),
            "/home/src",
            "[picks:1]",
            Some("main*"),
            None,
            false,
            80,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_with_agent() {
        // New segment: agent identity (claude/codex/gemini with short
        // session-id when known) renders between `git` and `suffix`.
        let out = render_status_to_string(
            Some("spyc"),
            Some("SAFFRON_CUMIN"),
            "/home/src",
            "[picks:1]",
            Some("main*"),
            Some("claude:76422c62"),
            false,
            96,
        );
        insta::assert_snapshot!(out);
    }
}

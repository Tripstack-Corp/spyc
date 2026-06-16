//! Quick Select (`^a u`): snapshot the visible pane, label every
//! matched URL / path / git-sha / IP / custom pattern, and install a
//! key-intercepting overlay that yanks (or, with uppercase intent,
//! opens) the picked match. Extracted verbatim from `app/mod.rs` (the
//! impl-extraction sweep). The open / key-handler / overlay-render
//! entry points are `pub` (called from `actions` / `key_dispatch` /
//! `render`); the dispatch / yank / url helpers are module-internal.

use ratatui::Frame;

use super::{App, Effect};

impl App {
    /// `^a u` — enter Quick Select. Snapshot the visible pane,
    /// scan for matches across the built-in + user patterns,
    /// assign labels, and install the picker as a key-intercepting
    /// overlay. Bails with a flash if there's nothing pickable.
    pub fn open_quick_select(&mut self) {
        use crate::pane::quick_select::{QuickSelect, assign_labels, build_patterns, scan};
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            self.state.flash_error("quick select: pane is closed");
            return;
        };
        // Always scan the *visible* viewport — labels must land on
        // text the user can see. Scroll mode falls out of this for
        // free since `visible_lines()` honors the user's current
        // scroll position.
        let lines = tabs.active().visible_lines();
        let patterns = build_patterns(&self.state.config.scan_patterns);
        let mut matches = scan(&lines, &patterns);
        if matches.is_empty() {
            self.state.flash_info("quick select: no matches in view");
            return;
        }
        let all_two_letter = assign_labels(&mut matches);
        self.view.quick_select = Some(QuickSelect {
            matches,
            pending_first: None,
            all_two_letter,
            open_intent: false,
        });
        self.view.needs_full_repaint = true;
    }

    /// Key handler for the Quick Select overlay. Owns input until
    /// the picker exits. Bindings:
    ///   `q` / `Esc`            — exit, no action
    ///   one-letter labels      — commit immediately
    ///   uppercase one-letter   — commit with "open" intent
    ///   two-letter labels      — first key narrows, second commits;
    ///                            uppercase anywhere = open intent
    ///   any other key          — clears any narrowing buffer (so a
    ///                            stray keystroke doesn't strand the
    ///                            user; they can still type a label)
    pub fn handle_quick_select_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Effect> {
        use crossterm::event::KeyCode;
        let Some(qs) = self.view.quick_select.as_mut() else {
            return Vec::new();
        };

        let close = |this: &mut Self| {
            this.view.quick_select = None;
            this.view.needs_full_repaint = true;
        };

        let c = match key.code {
            KeyCode::Esc => {
                close(self);
                return Vec::new();
            }
            KeyCode::Char(c) => c,
            _ => return Vec::new(),
        };

        // `q`/`Q` always exits — labels never use it (alphabet check
        // covered in unit test) so this is unambiguous.
        if c.eq_ignore_ascii_case(&'q') && qs.pending_first.is_none() {
            close(self);
            return Vec::new();
        }

        let is_upper = c.is_ascii_uppercase();
        let lower = c.to_ascii_lowercase();

        if qs.all_two_letter {
            match qs.pending_first {
                None => {
                    // First keystroke: must be the prefix of some label.
                    let any_match = qs.matches.iter().any(|m| m.label.starts_with(lower));
                    if !any_match {
                        return Vec::new(); // no narrowing possible — ignore
                    }
                    qs.pending_first = Some(lower);
                    if is_upper {
                        qs.open_intent = true;
                    }
                }
                Some(first) => {
                    let combined = format!("{first}{lower}");
                    let open = qs.open_intent || is_upper;
                    let m = qs.matches.iter().find(|m| m.label == combined).cloned();
                    close(self);
                    if let Some(m) = m {
                        self.dispatch_quick_select(&m, open);
                    }
                }
            }
        } else {
            // 1-letter labels. Uppercase commits with open intent.
            let m = qs
                .matches
                .iter()
                .find(|m| m.label == lower.to_string())
                .cloned();
            close(self);
            if let Some(m) = m {
                self.dispatch_quick_select(&m, is_upper);
            }
        }
        Vec::new()
    }

    /// Route a picked match to the right action, given user
    /// intent. See action matrix in `FEATURES.md` ("Quick Select").
    fn dispatch_quick_select(&mut self, m: &crate::pane::quick_select::Match, open_intent: bool) {
        let kind_label = m.kind.label().to_string();
        // The kind × intent decision is the pure `quick_select_action`; this
        // method only executes the chosen (impure) action.
        match quick_select_action(m, open_intent) {
            QsAction::Yank => self.yank_quick_select(&m.text, &kind_label),
            QsAction::OpenUrl(url) => self.open_url_or_flash(&url),
            QsAction::JumpPath(path) => self.jump_to_pane_path(&path),
            QsAction::GitShow(sha) => self.open_git_show_pager(&sha),
            // IPv4 and template-less Custom: fall back to yank with a hint
            // that explains why nothing else happened.
            QsAction::YankNoHandler => {
                self.yank_quick_select(&m.text, &kind_label);
                self.state
                    .flash_info(format!("yanked {kind_label} (no open handler)"));
            }
        }
    }

    fn yank_quick_select(&mut self, text: &str, kind_label: &str) {
        match crate::clipboard::copy(text) {
            Ok(()) => {
                let preview: String = text.chars().take(60).collect();
                let ellipsis = if text.len() > 60 { "…" } else { "" };
                self.state
                    .flash_info(format!("yanked {kind_label}: {preview}{ellipsis}"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
    }

    /// Hand `target` to the system handler via the `open` crate
    /// (cross-platform: macOS `open`, Linux `xdg-open`, Windows
    /// `start`). The crate spawns the launcher as a detached child
    /// and returns immediately, so the system handler never blocks
    /// our event loop.
    fn open_url_or_flash(&mut self, url: &str) {
        match open::that_detached(url) {
            Ok(()) => {
                let preview: String = url.chars().take(80).collect();
                let ellipsis = if url.len() > 80 { "…" } else { "" };
                self.state
                    .flash_info(format!("opening: {preview}{ellipsis}"));
            }
            Err(e) => self.state.flash_error(format!("open: {e}")),
        }
    }

    /// Render label overlay on top of the pane. Drawn after the
    /// pane widget so labels paint over the live vt100 grid; small
    /// inverted-color cells next to each match's start position.
    pub fn render_quick_select_overlay(&self, frame: &mut Frame, pane_rect: ratatui::layout::Rect) {
        use ratatui::{
            style::{Color, Modifier, Style},
            widgets::Paragraph,
        };
        let Some(qs) = self.view.quick_select.as_ref() else {
            return;
        };
        let label_style = Style::default()
            .fg(Color::Black)
            .bg(self.view.theme.pick)
            .add_modifier(Modifier::BOLD);
        let pending_style = Style::default()
            .fg(Color::Black)
            .bg(self.view.theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        for m in &qs.matches {
            // Skip labels that would render outside the pane rect.
            // (Matches whose row exceeded the pane height are
            // possible if the snapshot happened to be longer than
            // the visible region — defensive.)
            if m.row >= pane_rect.height as usize || m.col >= pane_rect.width as usize {
                continue;
            }
            // 2-letter narrowing: dim labels whose first letter
            // doesn't match the buffered keystroke; highlight
            // those that do (the user sees their narrowing land).
            let style = if let Some(first) = qs.pending_first {
                if m.label.starts_with(first) {
                    pending_style
                } else {
                    Style::default().fg(self.view.theme.status_suffix)
                }
            } else {
                label_style
            };
            let text = if let Some(first) = qs.pending_first {
                if m.label.starts_with(first) {
                    // Show only the *second* letter, since the
                    // first is already committed.
                    m.label.chars().nth(1).map(|c| c.to_string())
                } else {
                    None
                }
            } else {
                Some(m.label.clone())
            };
            let Some(text) = text else { continue };
            let label_rect = ratatui::layout::Rect {
                x: pane_rect.x + m.col as u16,
                y: pane_rect.y + m.row as u16,
                width: text.len() as u16,
                height: 1,
            };
            // Clamp to pane rect.
            if label_rect.x + label_rect.width > pane_rect.x + pane_rect.width
                || label_rect.y >= pane_rect.y + pane_rect.height
            {
                continue;
            }
            frame.render_widget(
                Paragraph::new(ratatui::text::Span::styled(text, style)),
                label_rect,
            );
        }
    }
}

/// What a picked Quick Select match should do, given the user's intent
/// (lowercase = yank, uppercase = open). Split out of `dispatch_quick_select`
/// as a pure decision so the whole action matrix is unit-testable without
/// touching the clipboard / OS opener / git.
#[derive(Debug, PartialEq, Eq)]
enum QsAction {
    /// Copy the match text to the clipboard.
    Yank,
    /// Hand a URL to the system opener.
    OpenUrl(String),
    /// Navigate spyc to a filesystem path.
    JumpPath(String),
    /// Open `git show <sha>` in the pager.
    GitShow(String),
    /// No opener for this kind (IPv4, template-less custom) — yank + hint.
    YankNoHandler,
}

/// The Quick Select action matrix. Lowercase intent always yanks; uppercase
/// opens per kind — URL → opener, path → navigate, SHA → git show, custom
/// with a `url_template` → the filled URL — and kinds with no opener fall
/// back to a yank-with-hint.
fn quick_select_action(m: &crate::pane::quick_select::Match, open_intent: bool) -> QsAction {
    use crate::pane::quick_select::MatchKind;
    if !open_intent {
        return QsAction::Yank;
    }
    match &m.kind {
        MatchKind::Url => QsAction::OpenUrl(m.text.clone()),
        MatchKind::Path => QsAction::JumpPath(m.text.clone()),
        MatchKind::GitSha => QsAction::GitShow(m.text.clone()),
        MatchKind::Custom {
            url_template: Some(t),
            ..
        } => QsAction::OpenUrl(t.replace("{}", &m.text)),
        MatchKind::Ipv4 | MatchKind::Custom { .. } => QsAction::YankNoHandler,
    }
}

#[cfg(test)]
mod tests {
    use super::{QsAction, quick_select_action};
    use crate::pane::quick_select::{Match, MatchKind};

    fn m(kind: MatchKind, text: &str) -> Match {
        Match {
            text: text.to_string(),
            kind,
            label: "a".to_string(),
            row: 0,
            col: 0,
        }
    }

    #[test]
    fn lowercase_intent_always_yanks() {
        for kind in [
            MatchKind::Url,
            MatchKind::Path,
            MatchKind::GitSha,
            MatchKind::Ipv4,
        ] {
            assert_eq!(quick_select_action(&m(kind, "x"), false), QsAction::Yank);
        }
    }

    #[test]
    fn uppercase_opens_per_kind() {
        assert_eq!(
            quick_select_action(&m(MatchKind::Url, "https://e.com"), true),
            QsAction::OpenUrl("https://e.com".to_string())
        );
        assert_eq!(
            quick_select_action(&m(MatchKind::Path, "/tmp/x"), true),
            QsAction::JumpPath("/tmp/x".to_string())
        );
        assert_eq!(
            quick_select_action(&m(MatchKind::GitSha, "abc123"), true),
            QsAction::GitShow("abc123".to_string())
        );
    }

    #[test]
    fn uppercase_custom_with_template_fills_the_url() {
        let kind = MatchKind::Custom {
            name: "JIRA".to_string(),
            url_template: Some("https://j/browse/{}".to_string()),
        };
        assert_eq!(
            quick_select_action(&m(kind, "PROJ-42"), true),
            QsAction::OpenUrl("https://j/browse/PROJ-42".to_string())
        );
    }

    #[test]
    fn uppercase_kinds_without_an_opener_yank_with_hint() {
        assert_eq!(
            quick_select_action(&m(MatchKind::Ipv4, "1.2.3.4"), true),
            QsAction::YankNoHandler
        );
        let kind = MatchKind::Custom {
            name: "X".to_string(),
            url_template: None,
        };
        assert_eq!(
            quick_select_action(&m(kind, "v"), true),
            QsAction::YankNoHandler
        );
    }
}

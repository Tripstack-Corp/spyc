use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::action::Action;
use super::user::{BoundAction, UserKeymap};

/// State carried between keystrokes while we wait for a multi-key sequence
/// to complete (count prefix, `gg`, future operator-pending, marks, search).
#[derive(Debug, Default, Clone)]
pub struct Resolver {
    count: Option<u32>,
    pending: PendingSeq,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PendingSeq {
    #[default]
    Normal,
    /// Seen a `g`, waiting for a second one (`gg`).
    G,
    /// Seen `m`, waiting for a letter `a-z` to set that mark.
    Mark,
    /// Seen `'`, waiting for a letter `a-z` to jump to that mark.
    JumpMark,
    /// Seen `Ctrl-W`, waiting for a pane-command letter
    /// (`j`/`k`/`s`/`+`/`-`/`\\`/`c`).
    W,
    /// Seen uppercase `W`, waiting for a worktree sub-command (l/n/d).
    Worktree,
    /// Seen `y`, waiting for: `y` = take (inventory yank), `p` = yank pane.
    Yank,
    /// Seen uppercase `H`, waiting for a harpoon sub-command:
    /// `1`..`9` = jump to slot, `a` = append, `x` = remove, `h` = open menu.
    Harpoon,
}

/// What the resolver produced from the latest keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolverOutcome {
    /// An action is ready to execute.
    Action(Action),
    /// A user-defined binding carrying inline data (unix cmd, preset pattern, …).
    User(BoundAction),
    /// Waiting for more input (e.g. count digits, or first `g` of `gg`).
    Pending,
    /// Unknown key, no effect.
    Ignored,
}

impl Resolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Non-zero count supplied by user, else 1.
    fn take_count(&mut self) -> usize {
        let n = self.count.take().unwrap_or(0);
        if n == 0 { 1 } else { n as usize }
    }

    const fn reset(&mut self) {
        self.count = None;
        self.pending = PendingSeq::Normal;
    }

    /// True while a multi-key sequence (gg, ma, 'a, Ctrl-W…) is in progress.
    /// Used by the App to decide whether to intercept or forward keys when
    /// the pty pane is focused.
    pub const fn is_pending(&self) -> bool {
        !matches!(self.pending, PendingSeq::Normal)
    }

    /// Display string for the current pending sequence, or `None` when idle.
    /// Shown in the prompt line so the user knows spyc is waiting for more input.
    pub fn pending_display(&self) -> Option<String> {
        let prefix = self.count.map(|n| n.to_string()).unwrap_or_default();
        let seq = match self.pending {
            PendingSeq::Normal if self.count.is_some() => "",
            PendingSeq::Normal => return None,
            PendingSeq::G => "g-",
            PendingSeq::Mark => "m-",
            PendingSeq::JumpMark => "'-",
            PendingSeq::W => "^a-",
            PendingSeq::Worktree => "W-",
            PendingSeq::Yank => "y-",
            PendingSeq::Harpoon => "H-",
        };
        Some(format!("{prefix}{seq}"))
    }

    /// Feed a key through the resolver, first consulting the user keymap
    /// and falling through to the built-in default bindings.
    pub fn feed(&mut self, ev: KeyEvent, user: &UserKeymap) -> ResolverOutcome {
        // User bindings always win. We still reset any pending multi-key
        // state so `g` followed by a user-bound key doesn't trigger `gg`.
        if let Some(action) = user.find(&ev) {
            self.reset();
            return ResolverOutcome::User(action.clone());
        }
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);

        // Control-codes take priority and reset any pending state.
        if ctrl {
            let out = match ev.code {
                KeyCode::Char('d' | 'D') => ResolverOutcome::Action(Action::Quit),
                KeyCode::Char('l' | 'L') => ResolverOutcome::Action(Action::Redraw),
                KeyCode::Char('b' | 'B') => ResolverOutcome::Action(Action::PageUp),
                KeyCode::Char('f' | 'F') => ResolverOutcome::Action(Action::PageDown),
                KeyCode::Char('t' | 'T') => ResolverOutcome::Action(Action::PickToggleAll),
                // Ctrl-W starts the pane-command prefix. We used to bind
                // it to `chmod +w`; that went to `!chmod +w %` to free the
                // key for split-nav. Note the match arm below starts the
                // pending sequence and falls through after resetting.
                KeyCode::Char('w' | 'W' | 'a' | 'A') => {
                    self.pending = PendingSeq::W;
                    return ResolverOutcome::Pending;
                }
                KeyCode::Char('x' | 'X') => ResolverOutcome::Action(Action::ChmodAdd('x')),
                KeyCode::Char('r' | 'R') => ResolverOutcome::Action(Action::ReloadConfig),
                // Ctrl-backslash toggles the split pane. Some terminals
                // deliver this as `Char('\\')` with CONTROL (handled
                // here); others as the raw 0x1c / FS byte (handled at
                // the top-level match below).
                KeyCode::Char('\\') => ResolverOutcome::Action(Action::TogglePane),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: `g` already seen.
        if self.pending == PendingSeq::G {
            let out = match ev.code {
                KeyCode::Char('g') => ResolverOutcome::Action(Action::GotoFirst),
                KeyCode::Char('d') => ResolverOutcome::Action(Action::GitDiff),
                KeyCode::Char('D') => ResolverOutcome::Action(Action::GitDiffCached),
                KeyCode::Char('b') => ResolverOutcome::Action(Action::GitBlame),
                KeyCode::Char('f') => ResolverOutcome::Action(Action::GotoFile),
                KeyCode::Char('F') => ResolverOutcome::Action(Action::GotoFileLine),
                KeyCode::Char('V') => ResolverOutcome::Action(Action::Version),
                KeyCode::Char('h') => ResolverOutcome::Action(Action::JumpProjectHome),
                KeyCode::Char('P') => ResolverOutcome::Action(Action::SetProjectHomeHere),
                KeyCode::Char('S') => ResolverOutcome::Action(Action::SetStartDirHere),
                KeyCode::Char('U') => ResolverOutcome::Action(Action::ShowUserHost),
                KeyCode::Char('B') => ResolverOutcome::Action(Action::OpenTaskViewer),
                KeyCode::Char('p') => ResolverOutcome::Action(Action::ReopenLastBuffer),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: Ctrl-A / Ctrl-W prefix waiting for a pane command.
        // Combines screen(1)-style (^a n=next, ^a p=prev, ^a c=new, ^a k=kill)
        // with vim-style (j/k focus, +/- resize).
        if self.pending == PendingSeq::W {
            let out = match ev.code {
                // Focus switching — vim-style j/J, plus screen-style ^a ^a
                // which sends literal ^a to the pane (grouped here to shut
                // up clippy::match_same_arms).
                KeyCode::Char('j' | 'J' | 'a' | 'A') => {
                    ResolverOutcome::Action(Action::PaneFocusDown)
                }
                KeyCode::Char('k') => ResolverOutcome::Action(Action::PaneFocusUp),
                // Tab navigation (screen-style + vim bracket style).
                KeyCode::Char('n' | ']') => ResolverOutcome::Action(Action::PaneNextTab),
                KeyCode::Char('p' | '[') => ResolverOutcome::Action(Action::PanePrevTab),
                KeyCode::Char('c') => ResolverOutcome::Action(Action::PaneNewTab),
                KeyCode::Char('K' | 'x' | 'X') => ResolverOutcome::Action(Action::PaneCloseTab),
                KeyCode::Char(c @ '1'..='9') => {
                    ResolverOutcome::Action(Action::PaneTabByIndex(c as u8 - b'0'))
                }
                KeyCode::Char('r') => ResolverOutcome::Action(Action::PaneRenameTab),
                KeyCode::Char('R') => ResolverOutcome::Action(Action::PaneRestartTab),
                // Pane toggle / resize / scroll.
                KeyCode::Char('\\' | 'C') => ResolverOutcome::Action(Action::TogglePane),
                KeyCode::Char('+' | '=') => ResolverOutcome::Action(Action::PaneGrow),
                KeyCode::Char('-' | '_') => ResolverOutcome::Action(Action::PaneShrink),
                KeyCode::Char('z' | 'Z') => ResolverOutcome::Action(Action::TogglePaneZoom),
                KeyCode::Char('v' | 'V') => ResolverOutcome::Action(Action::PaneScrollEnter),
                // Send / pipe content to pane.
                KeyCode::Char('s' | 'S') => ResolverOutcome::Action(Action::PaneSendSelection),
                KeyCode::Char('P') => ResolverOutcome::Action(Action::PanePipeContent),
                KeyCode::Char('i' | 'I') => ResolverOutcome::Action(Action::PanePipeInventory),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: uppercase W prefix waiting for a worktree command.
        if self.pending == PendingSeq::Worktree {
            let out = match ev.code {
                KeyCode::Char('l' | 'L') => ResolverOutcome::Action(Action::WorktreeList),
                KeyCode::Char('n' | 'N') => ResolverOutcome::Action(Action::WorktreeNew),
                KeyCode::Char('d' | 'D') => ResolverOutcome::Action(Action::WorktreeDelete),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: `H` (harpoon) prefix waiting for a sub-command.
        if self.pending == PendingSeq::Harpoon {
            let out = match ev.code {
                KeyCode::Char(c @ '1'..='9') => {
                    ResolverOutcome::Action(Action::HarpoonJump(c as u8 - b'0'))
                }
                KeyCode::Char('a' | 'A') => ResolverOutcome::Action(Action::HarpoonAppend),
                KeyCode::Char('x' | 'X') => ResolverOutcome::Action(Action::HarpoonRemove),
                KeyCode::Char('h') => ResolverOutcome::Action(Action::HarpoonOpenMenu),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: `y` prefix waiting for a yank sub-command.
        if self.pending == PendingSeq::Yank {
            let out = match ev.code {
                KeyCode::Char('y') => ResolverOutcome::Action(Action::Take),
                KeyCode::Char('p') => ResolverOutcome::Action(Action::YankPrompt),
                KeyCode::Char('P') => ResolverOutcome::Action(Action::YankLastPrompt),
                KeyCode::Char('a') => ResolverOutcome::Action(Action::YankScrollback),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: `m` (set mark) or `'` (jump to mark) waiting for a letter.
        if matches!(self.pending, PendingSeq::Mark | PendingSeq::JumpMark) {
            let is_set = self.pending == PendingSeq::Mark;
            let out = match ev.code {
                KeyCode::Char(c @ 'a'..='z') => ResolverOutcome::Action(if is_set {
                    Action::SetMark(c)
                } else {
                    Action::JumpMark(c)
                }),
                // '' (single-quote twice) = jump to previous directory.
                KeyCode::Char('\'') if !is_set => ResolverOutcome::Action(Action::JumpPrevDir),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Pane toggle alternate paths. Ctrl-\ on some terminals comes
        // through as the raw FS byte 0x1c rather than `Char('\\')` with
        // CONTROL, so we match both. F10 is an unambiguous fallback for
        // terminals that swallow Ctrl-\ entirely.
        if matches!(ev.code, KeyCode::Char('\x1c') | KeyCode::F(10)) {
            self.reset();
            return ResolverOutcome::Action(Action::TogglePane);
        }
        // F11 opens the pane with `claude --resume` to continue the last
        // conversation.
        if matches!(ev.code, KeyCode::F(9)) {
            self.reset();
            return ResolverOutcome::Action(Action::ResumePane);
        }

        match ev.code {
            // Count prefix. Leading zero is a motion (home column) in vi; here
            // we only accept digits after something non-zero.
            KeyCode::Char(c @ '0'..='9') => {
                let digit = u32::from(c as u8 - b'0');
                if digit == 0 && self.count.is_none() {
                    // Treat bare `0` as "start of line" — not meaningful yet; ignore.
                    ResolverOutcome::Ignored
                } else {
                    self.count = Some(self.count.unwrap_or(0) * 10 + digit);
                    ResolverOutcome::Pending
                }
            }

            // Motion — vi.
            KeyCode::Char('h') | KeyCode::Left => {
                let n = self.take_count();
                ResolverOutcome::Action(Action::Left(n))
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.take_count();
                ResolverOutcome::Action(Action::Down(n))
            }
            KeyCode::Enter | KeyCode::Char('d') => {
                self.reset();
                ResolverOutcome::Action(Action::EnterOrDisplay)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let n = self.take_count();
                ResolverOutcome::Action(Action::Up(n))
            }
            KeyCode::Char('l' | ' ') | KeyCode::Right => {
                let n = self.take_count();
                ResolverOutcome::Action(Action::Right(n))
            }

            KeyCode::PageUp => {
                self.reset();
                ResolverOutcome::Action(Action::PageUp)
            }
            KeyCode::PageDown => {
                self.reset();
                ResolverOutcome::Action(Action::PageDown)
            }

            // gg / G.
            KeyCode::Char('g') => {
                self.pending = PendingSeq::G;
                ResolverOutcome::Pending
            }
            KeyCode::Char('G') => {
                self.reset();
                ResolverOutcome::Action(Action::GotoLast)
            }

            // Navigation.
            KeyCode::Char('e' | 'v') => {
                self.reset();
                ResolverOutcome::Action(Action::EnterOrEdit)
            }
            KeyCode::Char('u' | '-') => {
                self.reset();
                ResolverOutcome::Action(Action::Climb)
            }
            // `~` and the Home key both still jump to `$HOME`. `H` was
            // formerly an alias here but is now the harpoon chord prefix
            // (`H1`..`H9`, `Ha`, `Hx`, `Hh`); muscle-memory falls back
            // to `gh` (PROJECT_HOME) for the common case anyway.
            KeyCode::Char('~') | KeyCode::Home => {
                self.reset();
                ResolverOutcome::Action(Action::Home)
            }

            // Picks.
            KeyCode::Char('t') => {
                self.reset();
                ResolverOutcome::Action(Action::TogglePick)
            }
            KeyCode::Char('T') => {
                self.reset();
                ResolverOutcome::Action(Action::PickPatternPrompt)
            }

            // Inventory / yank prefix (yy = take, yp = yank pane).
            KeyCode::Char('y') => {
                self.pending = PendingSeq::Yank;
                ResolverOutcome::Pending
            }
            KeyCode::Char('Y') => {
                self.reset();
                ResolverOutcome::Action(Action::Untake)
            }
            KeyCode::Char('p') => {
                self.reset();
                ResolverOutcome::Action(Action::Drop)
            }
            KeyCode::Char('i') => {
                self.reset();
                ResolverOutcome::Action(Action::ToggleInventoryView)
            }
            KeyCode::Char('z') => {
                self.reset();
                ResolverOutcome::Action(Action::EmptyInventory)
            }

            // Ignore mask toggles.
            KeyCode::Char('a') => {
                self.reset();
                ResolverOutcome::Action(Action::ToggleMask(1))
            }
            KeyCode::Char('o') => {
                self.reset();
                ResolverOutcome::Action(Action::ToggleMask(2))
            }

            // Shell-out. `!` captures output into the in-app pager (with
            // ANSI colors preserved); `;` runs in the foreground for
            // interactive tools (vim, htop, etc.).
            KeyCode::Char('!') => {
                self.reset();
                ResolverOutcome::Action(Action::ShellCapturedPrompt)
            }
            KeyCode::Char(';') => {
                self.reset();
                ResolverOutcome::Action(Action::ShellForegroundPrompt)
            }
            KeyCode::Char('$') => {
                self.reset();
                ResolverOutcome::Action(Action::StartShell)
            }

            // Search.
            KeyCode::Char('/') => {
                self.reset();
                ResolverOutcome::Action(Action::SearchPrompt)
            }
            KeyCode::Char('n') => {
                self.reset();
                ResolverOutcome::Action(Action::SearchNext)
            }
            KeyCode::Char('N') => {
                self.reset();
                ResolverOutcome::Action(Action::SearchPrev)
            }

            // Filter.
            KeyCode::Char('=') => {
                self.reset();
                ResolverOutcome::Action(Action::LimitPrompt)
            }

            // Command line.
            KeyCode::Char(':') => {
                self.reset();
                ResolverOutcome::Action(Action::CommandPrompt)
            }

            // Jump.
            KeyCode::Char('J') => {
                self.reset();
                ResolverOutcome::Action(Action::JumpPrompt)
            }

            // Find file (project-wide fuzzy filename picker).
            KeyCode::Char('F') => {
                self.reset();
                ResolverOutcome::Action(Action::FindFile)
            }

            // File operations.
            KeyCode::Char('c') => {
                self.reset();
                ResolverOutcome::Action(Action::CopyPrompt)
            }
            KeyCode::Char('`') => {
                self.reset();
                ResolverOutcome::Action(Action::JumpStartDir)
            }
            KeyCode::Char('m') => {
                // Start of `m{a-z}` set-mark sequence.
                self.pending = PendingSeq::Mark;
                ResolverOutcome::Pending
            }
            KeyCode::Char('\'') => {
                // Start of `'{a-z}` jump-to-mark sequence.
                self.pending = PendingSeq::JumpMark;
                ResolverOutcome::Pending
            }
            KeyCode::Char('R') => {
                self.reset();
                ResolverOutcome::Action(Action::RemovePrompt)
            }
            KeyCode::Char('M') => {
                // `m` is the set-mark prefix (vi); move takes uppercase.
                self.reset();
                ResolverOutcome::Action(Action::MovePrompt)
            }
            KeyCode::Char('+') => {
                // Spy uses `N` for mkdir but we reserve `N` for vi's
                // reverse-search. `+` reads intuitively as "add a dir".
                self.reset();
                ResolverOutcome::Action(Action::MakeDirPrompt)
            }
            KeyCode::Char('O') => {
                self.reset();
                ResolverOutcome::Action(Action::NewFilePrompt)
            }
            KeyCode::Char('L') => {
                self.reset();
                ResolverOutcome::Action(Action::LongList)
            }
            KeyCode::Char('f') => {
                self.reset();
                ResolverOutcome::Action(Action::FileType)
            }

            // Help overlay.
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.reset();
                ResolverOutcome::Action(Action::Help)
            }

            // Info commands.
            KeyCode::Char('D') => {
                self.reset();
                ResolverOutcome::Action(Action::Date)
            }
            KeyCode::Char('V') => {
                self.reset();
                ResolverOutcome::Action(Action::EditInPane)
            }
            KeyCode::Char('I') => {
                self.reset();
                ResolverOutcome::Action(Action::ShowMemory)
            }
            KeyCode::Char('C') => {
                self.reset();
                ResolverOutcome::Action(Action::ColorToggle)
            }
            KeyCode::Char('A') => {
                self.reset();
                ResolverOutcome::Action(Action::ToggleActivity)
            }
            KeyCode::Char('s') => {
                self.reset();
                ResolverOutcome::Action(Action::SetEnvPrompt)
            }

            // Git worktree prefix.
            KeyCode::Char('W') => {
                self.pending = PendingSeq::Worktree;
                ResolverOutcome::Pending
            }

            // Harpoon prefix (`H1`..`H9`, `Ha`, `Hx`, `Hh`).
            KeyCode::Char('H') => {
                self.pending = PendingSeq::Harpoon;
                ResolverOutcome::Pending
            }

            // Quit. Lowercase `q` is reserved for future macro recording
            // (see MacroRecordReserved); only `Q` quits via this binding.
            // `^D` and `:q` also quit.
            KeyCode::Char('Q') => {
                self.reset();
                ResolverOutcome::Action(Action::Quit)
            }
            KeyCode::Char('q') => {
                self.reset();
                ResolverOutcome::Action(Action::MacroRecordReserved)
            }

            KeyCode::Esc => {
                self.reset();
                ResolverOutcome::Pending
            }

            _ => {
                self.reset();
                ResolverOutcome::Ignored
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn special(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn empty_keymap() -> UserKeymap {
        UserKeymap::default()
    }

    fn feed(r: &mut Resolver, ev: KeyEvent) -> ResolverOutcome {
        r.feed(ev, &empty_keymap())
    }

    // ── count accumulation ────────────────────────────────────────

    #[test]
    fn bare_zero_is_ignored() {
        let mut r = Resolver::new();
        assert_eq!(feed(&mut r, key('0')), ResolverOutcome::Ignored);
    }

    #[test]
    fn single_digit_starts_count() {
        let mut r = Resolver::new();
        assert_eq!(feed(&mut r, key('3')), ResolverOutcome::Pending);
    }

    #[test]
    fn multi_digit_count() {
        let mut r = Resolver::new();
        assert_eq!(feed(&mut r, key('1')), ResolverOutcome::Pending);
        assert_eq!(feed(&mut r, key('2')), ResolverOutcome::Pending);
        // 12j should move down 12
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(12))
        );
    }

    #[test]
    fn count_with_trailing_zero() {
        let mut r = Resolver::new();
        feed(&mut r, key('1'));
        feed(&mut r, key('0'));
        assert_eq!(
            feed(&mut r, key('k')),
            ResolverOutcome::Action(Action::Up(10))
        );
    }

    #[test]
    fn count_resets_after_action() {
        let mut r = Resolver::new();
        feed(&mut r, key('5'));
        feed(&mut r, key('j'));
        // Next motion without count should default to 1
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(1))
        );
    }

    #[test]
    fn count_applies_to_all_motions() {
        let mut r = Resolver::new();
        feed(&mut r, key('3'));
        assert_eq!(
            feed(&mut r, key('h')),
            ResolverOutcome::Action(Action::Left(3))
        );

        feed(&mut r, key('7'));
        assert_eq!(
            feed(&mut r, key('l')),
            ResolverOutcome::Action(Action::Right(7))
        );
    }

    #[test]
    fn count_resets_on_non_motion_key() {
        let mut r = Resolver::new();
        feed(&mut r, key('5'));
        // 't' is toggle pick — doesn't use count, resets
        assert_eq!(
            feed(&mut r, key('t')),
            ResolverOutcome::Action(Action::TogglePick)
        );
        // Count should be gone
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(1))
        );
    }

    #[test]
    fn large_count() {
        let mut r = Resolver::new();
        for c in "999".chars() {
            feed(&mut r, key(c));
        }
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(999))
        );
    }

    // ── gg sequence ───────────────────────────────────────────────

    #[test]
    fn g_enters_pending() {
        let mut r = Resolver::new();
        assert_eq!(feed(&mut r, key('g')), ResolverOutcome::Pending);
        assert!(r.is_pending());
    }

    #[test]
    fn gg_goes_to_first() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(
            feed(&mut r, key('g')),
            ResolverOutcome::Action(Action::GotoFirst)
        );
        assert!(!r.is_pending());
    }

    #[test]
    fn gd_is_git_diff() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(
            feed(&mut r, key('d')),
            ResolverOutcome::Action(Action::GitDiff)
        );
    }

    #[test]
    fn gb_is_git_blame() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(
            feed(&mut r, key('b')),
            ResolverOutcome::Action(Action::GitBlame)
        );
    }

    #[test]
    fn g_cap_d_is_git_diff_cached() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(
            feed(&mut r, key('D')),
            ResolverOutcome::Action(Action::GitDiffCached)
        );
    }

    #[test]
    fn gf_is_goto_file() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(
            feed(&mut r, key('f')),
            ResolverOutcome::Action(Action::GotoFile)
        );
    }

    #[test]
    fn g_cap_f_is_goto_file_line() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(
            feed(&mut r, key('F')),
            ResolverOutcome::Action(Action::GotoFileLine)
        );
    }

    #[test]
    fn g_followed_by_unknown_is_ignored() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(feed(&mut r, key('x')), ResolverOutcome::Ignored);
        assert!(!r.is_pending());
    }

    #[test]
    fn cap_g_goes_to_last() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, key('G')),
            ResolverOutcome::Action(Action::GotoLast)
        );
    }

    // ── marks ─────────────────────────────────────────────────────

    #[test]
    fn m_enters_mark_pending() {
        let mut r = Resolver::new();
        assert_eq!(feed(&mut r, key('m')), ResolverOutcome::Pending);
        assert!(r.is_pending());
    }

    #[test]
    fn m_a_sets_mark() {
        let mut r = Resolver::new();
        feed(&mut r, key('m'));
        assert_eq!(
            feed(&mut r, key('a')),
            ResolverOutcome::Action(Action::SetMark('a'))
        );
    }

    #[test]
    fn m_z_sets_mark() {
        let mut r = Resolver::new();
        feed(&mut r, key('m'));
        assert_eq!(
            feed(&mut r, key('z')),
            ResolverOutcome::Action(Action::SetMark('z'))
        );
    }

    #[test]
    fn m_nonletter_is_ignored() {
        let mut r = Resolver::new();
        feed(&mut r, key('m'));
        assert_eq!(feed(&mut r, key('1')), ResolverOutcome::Ignored);
    }

    #[test]
    fn quote_a_jumps_to_mark() {
        let mut r = Resolver::new();
        feed(&mut r, key('\''));
        assert_eq!(
            feed(&mut r, key('a')),
            ResolverOutcome::Action(Action::JumpMark('a'))
        );
    }

    #[test]
    fn quote_quote_jumps_prev_dir() {
        let mut r = Resolver::new();
        feed(&mut r, key('\''));
        assert_eq!(
            feed(&mut r, key('\'')),
            ResolverOutcome::Action(Action::JumpPrevDir)
        );
    }

    #[test]
    fn quote_nonletter_is_ignored() {
        let mut r = Resolver::new();
        feed(&mut r, key('\''));
        assert_eq!(feed(&mut r, key('3')), ResolverOutcome::Ignored);
    }

    // ── Ctrl-W pane commands ──────────────────────────────────────

    #[test]
    fn ctrl_w_enters_pane_pending() {
        let mut r = Resolver::new();
        assert_eq!(feed(&mut r, ctrl('w')), ResolverOutcome::Pending);
        assert!(r.is_pending());
    }

    #[test]
    fn ctrl_w_j_focuses_down() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::PaneFocusDown)
        );
    }

    #[test]
    fn ctrl_w_k_focuses_up() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('k')),
            ResolverOutcome::Action(Action::PaneFocusUp)
        );
    }

    #[test]
    fn ctrl_w_plus_grows_pane() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('+')),
            ResolverOutcome::Action(Action::PaneGrow)
        );
    }

    #[test]
    fn ctrl_w_minus_shrinks_pane() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('-')),
            ResolverOutcome::Action(Action::PaneShrink)
        );
    }

    #[test]
    fn ctrl_w_n_next_tab() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('n')),
            ResolverOutcome::Action(Action::PaneNextTab)
        );
    }

    #[test]
    fn ctrl_a_c_new_tab() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('a'));
        assert_eq!(
            feed(&mut r, key('c')),
            ResolverOutcome::Action(Action::PaneNewTab)
        );
    }

    #[test]
    fn ctrl_w_x_close_tab() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('x')),
            ResolverOutcome::Action(Action::PaneCloseTab)
        );
    }

    #[test]
    fn ctrl_w_digit_switches_tab() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('3')),
            ResolverOutcome::Action(Action::PaneTabByIndex(3))
        );
    }

    #[test]
    fn ctrl_w_bracket_navigates_tabs() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key(']')),
            ResolverOutcome::Action(Action::PaneNextTab)
        );

        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('[')),
            ResolverOutcome::Action(Action::PanePrevTab)
        );
    }

    #[test]
    fn ctrl_w_s_sends_selection() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('s')),
            ResolverOutcome::Action(Action::PaneSendSelection)
        );
    }

    #[test]
    fn ctrl_w_v_enters_scroll() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('v')),
            ResolverOutcome::Action(Action::PaneScrollEnter)
        );
    }

    #[test]
    fn ctrl_w_backslash_toggles_pane() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('\\')),
            ResolverOutcome::Action(Action::TogglePane)
        );
    }

    #[test]
    fn ctrl_w_r_renames_tab() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('r')),
            ResolverOutcome::Action(Action::PaneRenameTab)
        );
    }

    #[test]
    fn ctrl_w_p_prev_tab() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('p')),
            ResolverOutcome::Action(Action::PanePrevTab)
        );
    }

    #[test]
    fn ctrl_a_shift_p_pipes_content() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('a'));
        assert_eq!(
            feed(&mut r, key('P')),
            ResolverOutcome::Action(Action::PanePipeContent)
        );
    }

    #[test]
    fn ctrl_w_i_pipes_inventory() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('i')),
            ResolverOutcome::Action(Action::PanePipeInventory)
        );
    }

    #[test]
    fn ctrl_w_z_zooms_pane() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(
            feed(&mut r, key('z')),
            ResolverOutcome::Action(Action::TogglePaneZoom)
        );
    }

    #[test]
    fn ctrl_w_unknown_is_ignored() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('w'));
        assert_eq!(feed(&mut r, key('q')), ResolverOutcome::Ignored);
    }

    // ── W (worktree) prefix ───────────────────────────────────────

    #[test]
    fn cap_w_enters_worktree_pending() {
        let mut r = Resolver::new();
        assert_eq!(feed(&mut r, key('W')), ResolverOutcome::Pending);
        assert!(r.is_pending());
    }

    #[test]
    fn w_l_lists_worktrees() {
        let mut r = Resolver::new();
        feed(&mut r, key('W'));
        assert_eq!(
            feed(&mut r, key('l')),
            ResolverOutcome::Action(Action::WorktreeList)
        );
    }

    #[test]
    fn w_n_creates_worktree() {
        let mut r = Resolver::new();
        feed(&mut r, key('W'));
        assert_eq!(
            feed(&mut r, key('n')),
            ResolverOutcome::Action(Action::WorktreeNew)
        );
    }

    #[test]
    fn w_d_deletes_worktree() {
        let mut r = Resolver::new();
        feed(&mut r, key('W'));
        assert_eq!(
            feed(&mut r, key('d')),
            ResolverOutcome::Action(Action::WorktreeDelete)
        );
    }

    #[test]
    fn w_unknown_is_ignored() {
        let mut r = Resolver::new();
        feed(&mut r, key('W'));
        assert_eq!(feed(&mut r, key('z')), ResolverOutcome::Ignored);
    }

    // ── control codes ─────────────────────────────────────────────

    #[test]
    fn ctrl_d_quits() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, ctrl('d')),
            ResolverOutcome::Action(Action::Quit)
        );
    }

    #[test]
    fn ctrl_l_redraws() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, ctrl('l')),
            ResolverOutcome::Action(Action::Redraw)
        );
    }

    #[test]
    fn ctrl_b_page_up() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, ctrl('b')),
            ResolverOutcome::Action(Action::PageUp)
        );
    }

    #[test]
    fn ctrl_f_page_down() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, ctrl('f')),
            ResolverOutcome::Action(Action::PageDown)
        );
    }

    #[test]
    fn ctrl_t_toggles_all_picks() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, ctrl('t')),
            ResolverOutcome::Action(Action::PickToggleAll)
        );
    }

    #[test]
    fn ctrl_x_chmod() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, ctrl('x')),
            ResolverOutcome::Action(Action::ChmodAdd('x'))
        );
    }

    #[test]
    fn ctrl_r_reloads_config() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, ctrl('r')),
            ResolverOutcome::Action(Action::ReloadConfig)
        );
    }

    #[test]
    fn ctrl_resets_pending_state() {
        let mut r = Resolver::new();
        feed(&mut r, key('g')); // pending G
        assert!(r.is_pending());
        assert_eq!(
            feed(&mut r, ctrl('d')),
            ResolverOutcome::Action(Action::Quit)
        );
        assert!(!r.is_pending());
    }

    // ── simple single-key actions ─────────────────────────────────

    #[test]
    fn basic_motions_default_count_1() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(1))
        );
        assert_eq!(
            feed(&mut r, key('k')),
            ResolverOutcome::Action(Action::Up(1))
        );
        assert_eq!(
            feed(&mut r, key('h')),
            ResolverOutcome::Action(Action::Left(1))
        );
        assert_eq!(
            feed(&mut r, key('l')),
            ResolverOutcome::Action(Action::Right(1))
        );
    }

    #[test]
    fn arrow_keys_work() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::Down)),
            ResolverOutcome::Action(Action::Down(1))
        );
        assert_eq!(
            feed(&mut r, special(KeyCode::Up)),
            ResolverOutcome::Action(Action::Up(1))
        );
        assert_eq!(
            feed(&mut r, special(KeyCode::Left)),
            ResolverOutcome::Action(Action::Left(1))
        );
        assert_eq!(
            feed(&mut r, special(KeyCode::Right)),
            ResolverOutcome::Action(Action::Right(1))
        );
    }

    #[test]
    fn shell_prompts() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, key('!')),
            ResolverOutcome::Action(Action::ShellCapturedPrompt)
        );
        assert_eq!(
            feed(&mut r, key(';')),
            ResolverOutcome::Action(Action::ShellForegroundPrompt)
        );
        assert_eq!(
            feed(&mut r, key('$')),
            ResolverOutcome::Action(Action::StartShell)
        );
    }

    #[test]
    fn search_keys() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, key('/')),
            ResolverOutcome::Action(Action::SearchPrompt)
        );
        assert_eq!(
            feed(&mut r, key('n')),
            ResolverOutcome::Action(Action::SearchNext)
        );
        assert_eq!(
            feed(&mut r, key('N')),
            ResolverOutcome::Action(Action::SearchPrev)
        );
    }

    #[test]
    fn quit_keys() {
        let mut r = Resolver::new();
        // Lowercase q is reserved for future macro recording; only Q quits.
        assert_eq!(
            feed(&mut r, key('q')),
            ResolverOutcome::Action(Action::MacroRecordReserved)
        );
        assert_eq!(
            feed(&mut r, key('Q')),
            ResolverOutcome::Action(Action::Quit)
        );
    }

    #[test]
    fn navigation_keys() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, key('u')),
            ResolverOutcome::Action(Action::Climb)
        );
        assert_eq!(
            feed(&mut r, key('-')),
            ResolverOutcome::Action(Action::Climb)
        );
        // `H` is the harpoon chord prefix (was `Home` alias; freed
        // for `H1`..`H9`, `Ha`, `Hx`, `Hh`). `~` and the Home key
        // remain the bindings for jumping to `$HOME`.
        assert_eq!(feed(&mut r, key('H')), ResolverOutcome::Pending);
        // `Hh` opens the harpoon menu (recovers from the pending state).
        assert_eq!(
            feed(&mut r, key('h')),
            ResolverOutcome::Action(Action::HarpoonOpenMenu)
        );
        assert_eq!(
            feed(&mut r, key('~')),
            ResolverOutcome::Action(Action::Home)
        );
    }

    #[test]
    fn harpoon_chord_jumps_to_slot() {
        let mut r = Resolver::new();
        feed(&mut r, key('H'));
        assert_eq!(
            feed(&mut r, key('3')),
            ResolverOutcome::Action(Action::HarpoonJump(3))
        );
    }

    #[test]
    fn harpoon_chord_append_remove() {
        let mut r = Resolver::new();
        feed(&mut r, key('H'));
        assert_eq!(
            feed(&mut r, key('a')),
            ResolverOutcome::Action(Action::HarpoonAppend)
        );
        feed(&mut r, key('H'));
        assert_eq!(
            feed(&mut r, key('x')),
            ResolverOutcome::Action(Action::HarpoonRemove)
        );
    }

    #[test]
    fn inventory_keys() {
        let mut r = Resolver::new();
        // y enters pending, yy = take
        assert_eq!(feed(&mut r, key('y')), ResolverOutcome::Pending);
        assert_eq!(
            feed(&mut r, key('y')),
            ResolverOutcome::Action(Action::Take)
        );
        // yp = yank prompt
        assert_eq!(feed(&mut r, key('y')), ResolverOutcome::Pending);
        assert_eq!(
            feed(&mut r, key('p')),
            ResolverOutcome::Action(Action::YankPrompt)
        );
        assert_eq!(
            feed(&mut r, key('Y')),
            ResolverOutcome::Action(Action::Untake)
        );
        assert_eq!(
            feed(&mut r, key('p')),
            ResolverOutcome::Action(Action::Drop)
        );
        assert_eq!(
            feed(&mut r, key('i')),
            ResolverOutcome::Action(Action::ToggleInventoryView)
        );
        assert_eq!(
            feed(&mut r, key('z')),
            ResolverOutcome::Action(Action::EmptyInventory)
        );
    }

    #[test]
    fn file_operation_keys() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, key('c')),
            ResolverOutcome::Action(Action::CopyPrompt)
        );
        assert_eq!(
            feed(&mut r, key('R')),
            ResolverOutcome::Action(Action::RemovePrompt)
        );
        assert_eq!(
            feed(&mut r, key('M')),
            ResolverOutcome::Action(Action::MovePrompt)
        );
        assert_eq!(
            feed(&mut r, key('+')),
            ResolverOutcome::Action(Action::MakeDirPrompt)
        );
    }

    // ── Esc resets ────────────────────────────────────────────────

    #[test]
    fn esc_resets_count_and_pending() {
        let mut r = Resolver::new();
        feed(&mut r, key('5'));
        feed(&mut r, key('g'));
        assert!(r.is_pending());
        // Esc hits the catch-all `_ =>` arm which resets and returns Ignored
        // (g-pending intercepts first, so Esc after g is "unknown in g-seq")
        let out = feed(&mut r, special(KeyCode::Esc));
        assert_eq!(out, ResolverOutcome::Ignored);
        // State is fully reset — next key is fresh
        assert!(!r.is_pending());
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(1))
        );
    }

    #[test]
    fn esc_from_normal_resets_count() {
        let mut r = Resolver::new();
        feed(&mut r, key('5'));
        // Esc from normal (non-pending) state returns Pending but resets count
        let out = feed(&mut r, special(KeyCode::Esc));
        assert_eq!(out, ResolverOutcome::Pending);
        assert!(!r.is_pending());
        assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(1))
        );
    }

    // ── pending display ───────────────────────────────────────────

    #[test]
    fn pending_display_idle() {
        let r = Resolver::new();
        assert!(r.pending_display().is_none());
    }

    #[test]
    fn pending_display_count_only() {
        let mut r = Resolver::new();
        feed(&mut r, key('5'));
        assert_eq!(r.pending_display(), Some("5".to_string()));
    }

    #[test]
    fn pending_display_g() {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(r.pending_display(), Some("g-".to_string()));
    }

    #[test]
    fn pending_display_count_plus_g() {
        let mut r = Resolver::new();
        feed(&mut r, key('5'));
        feed(&mut r, key('g'));
        // count is 5 but g clears count context — actually let's check
        // Actually looking at the code: g doesn't go through count path,
        // it sets pending = G. The count stays.
        // Wait, actually 'g' enters PendingSeq::G but doesn't touch count.
        // So pending_display should be "5g-"
        assert_eq!(r.pending_display(), Some("5g-".to_string()));
    }

    #[test]
    fn pending_display_mark() {
        let mut r = Resolver::new();
        feed(&mut r, key('m'));
        assert_eq!(r.pending_display(), Some("m-".to_string()));
    }

    #[test]
    fn pending_display_jump_mark() {
        let mut r = Resolver::new();
        feed(&mut r, key('\''));
        assert_eq!(r.pending_display(), Some("'-".to_string()));
    }

    #[test]
    fn pending_display_ctrl_a() {
        let mut r = Resolver::new();
        feed(&mut r, ctrl('a'));
        assert_eq!(r.pending_display(), Some("^a-".to_string()));
    }

    #[test]
    fn pending_display_worktree() {
        let mut r = Resolver::new();
        feed(&mut r, key('W'));
        assert_eq!(r.pending_display(), Some("W-".to_string()));
    }

    // ── user keymap override ──────────────────────────────────────

    #[test]
    fn user_binding_wins_over_builtin() {
        let mut r = Resolver::new();
        let user = UserKeymap::from_bindings(vec![super::super::user::UserBinding {
            chord: super::super::user::KeyChord::Char('j'),
            action: BoundAction::UnixCmd("my-cmd".to_string()),
        }]);
        let out = r.feed(key('j'), &user);
        assert_eq!(
            out,
            ResolverOutcome::User(BoundAction::UnixCmd("my-cmd".to_string()))
        );
    }

    #[test]
    fn user_binding_resets_pending() {
        let mut r = Resolver::new();
        feed(&mut r, key('g')); // enter pending G
        assert!(r.is_pending());
        let user = UserKeymap::from_bindings(vec![super::super::user::UserBinding {
            chord: super::super::user::KeyChord::Char('g'),
            action: BoundAction::Plain(Action::Noop),
        }]);
        r.feed(key('g'), &user);
        assert!(!r.is_pending());
    }

    // ── special keys ──────────────────────────────────────────────

    #[test]
    fn f1_is_help() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::F(1))),
            ResolverOutcome::Action(Action::Help)
        );
    }

    #[test]
    fn f9_resumes_pane() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::F(9))),
            ResolverOutcome::Action(Action::ResumePane)
        );
    }

    #[test]
    fn f10_toggles_pane() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::F(10))),
            ResolverOutcome::Action(Action::TogglePane)
        );
    }

    #[test]
    fn page_up_down() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::PageUp)),
            ResolverOutcome::Action(Action::PageUp)
        );
        assert_eq!(
            feed(&mut r, special(KeyCode::PageDown)),
            ResolverOutcome::Action(Action::PageDown)
        );
    }

    #[test]
    fn home_key() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::Home)),
            ResolverOutcome::Action(Action::Home)
        );
    }

    #[test]
    fn enter_is_enter_or_display() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::Enter)),
            ResolverOutcome::Action(Action::EnterOrDisplay)
        );
    }

    #[test]
    fn unknown_key_is_ignored() {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, special(KeyCode::F(20))),
            ResolverOutcome::Ignored
        );
    }
}

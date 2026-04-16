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
        if n == 0 {
            1
        } else {
            n as usize
        }
    }

    fn reset(&mut self) {
        self.count = None;
        self.pending = PendingSeq::Normal;
    }

    /// True while a multi-key sequence (gg, ma, 'a, Ctrl-W…) is in progress.
    /// Used by the App to decide whether to intercept or forward keys when
    /// the pty pane is focused.
    pub const fn is_pending(&self) -> bool {
        !matches!(self.pending, PendingSeq::Normal)
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
                KeyCode::Char('w' | 'W') => {
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
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: Ctrl-W prefix waiting for a pane command.
        if self.pending == PendingSeq::W {
            let out = match ev.code {
                KeyCode::Char('j' | 'J' | 'k' | 'K') => {
                    ResolverOutcome::Action(Action::PaneFocusToggle)
                }
                KeyCode::Char('s' | 'S') => {
                    ResolverOutcome::Action(Action::PaneSendSelection)
                }
                KeyCode::Char('+' | '=') => ResolverOutcome::Action(Action::PaneGrow),
                KeyCode::Char('-' | '_') => ResolverOutcome::Action(Action::PaneShrink),
                KeyCode::Char('\\' | 'c' | 'C') => {
                    ResolverOutcome::Action(Action::TogglePane)
                }
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
            KeyCode::Char('H' | '~') | KeyCode::Home => {
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

            // Inventory (take / drop / view / empty).
            KeyCode::Char('y' | 'Y') => {
                self.reset();
                ResolverOutcome::Action(Action::Take)
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

            // Jump.
            KeyCode::Char('J') => {
                self.reset();
                ResolverOutcome::Action(Action::JumpPrompt)
            }

            // File operations.
            KeyCode::Char('c') => {
                self.reset();
                ResolverOutcome::Action(Action::CopyPrompt)
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
                ResolverOutcome::Action(Action::Version)
            }
            KeyCode::Char('I') => {
                self.reset();
                ResolverOutcome::Action(Action::ShowMemory)
            }
            KeyCode::Char('C') => {
                self.reset();
                ResolverOutcome::Action(Action::ColorToggle)
            }
            KeyCode::Char('s') => {
                self.reset();
                ResolverOutcome::Action(Action::SetEnvPrompt)
            }

            // Quit.
            KeyCode::Char('Q' | 'q') => {
                self.reset();
                ResolverOutcome::Action(Action::Quit)
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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::action::Action;

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
}

/// What the resolver produced from the latest keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolverOutcome {
    /// An action is ready to execute.
    Action(Action),
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

    pub fn feed(&mut self, ev: KeyEvent) -> ResolverOutcome {
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);

        // Control-codes take priority and reset any pending state.
        if ctrl {
            let out = match ev.code {
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    ResolverOutcome::Action(Action::Quit)
                }
                KeyCode::Char('l') | KeyCode::Char('L') => {
                    ResolverOutcome::Action(Action::Redraw)
                }
                KeyCode::Char('b') | KeyCode::Char('B') => {
                    ResolverOutcome::Action(Action::PageUp)
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    ResolverOutcome::Action(Action::PageDown)
                }
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    ResolverOutcome::Action(Action::PickToggleAll)
                }
                KeyCode::Char('w') | KeyCode::Char('W') => {
                    ResolverOutcome::Action(Action::ChmodAdd('w'))
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    ResolverOutcome::Action(Action::ChmodAdd('x'))
                }
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

        match ev.code {
            // Count prefix. Leading zero is a motion (home column) in vi; here
            // we only accept digits after something non-zero.
            KeyCode::Char(c @ '0'..='9') => {
                let digit = (c as u8 - b'0') as u32;
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
            KeyCode::Enter => {
                self.reset();
                ResolverOutcome::Action(Action::EnterOrDisplay)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let n = self.take_count();
                ResolverOutcome::Action(Action::Up(n))
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
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
            KeyCode::Char('d') => {
                self.reset();
                ResolverOutcome::Action(Action::EnterOrDisplay)
            }
            KeyCode::Char('e') | KeyCode::Char('v') => {
                self.reset();
                ResolverOutcome::Action(Action::EnterOrEdit)
            }
            KeyCode::Char('u') | KeyCode::Char('-') => {
                self.reset();
                ResolverOutcome::Action(Action::Climb)
            }
            KeyCode::Char('H') | KeyCode::Char('~') => {
                self.reset();
                ResolverOutcome::Action(Action::Home)
            }
            KeyCode::Home => {
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
            KeyCode::Char('y') | KeyCode::Char('Y') => {
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

            // Shell-out.
            KeyCode::Char('!') | KeyCode::Char(';') => {
                self.reset();
                ResolverOutcome::Action(Action::ShellPrompt)
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

            // Quit.
            KeyCode::Char('Q') | KeyCode::Char('q') => {
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

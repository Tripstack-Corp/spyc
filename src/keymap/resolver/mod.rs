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
    /// Seen `]`, waiting for a "next" sub-command (`g` = next git change).
    NextBracket,
    /// Seen `[`, waiting for a "prev" sub-command (`g` = prev git change).
    PrevBracket,
    /// Seen `d`, waiting for the second `d` of the vim-style
    /// `dd` (or `Ndd`) delete chord. Cancels on any other key.
    D,
    /// Seen uppercase `Z`, waiting for the second `Z` of the
    /// vim-style `ZZ` quit chord. Cancels on any other key.
    Z,
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
            PendingSeq::NextBracket => "]-",
            PendingSeq::PrevBracket => "[-",
            PendingSeq::D => "d-",
            PendingSeq::Z => "Z-",
        };
        Some(format!("{prefix}{seq}"))
    }

    /// Feed a key through the resolver, first consulting the user keymap
    /// and falling through to the built-in default bindings.
    pub fn feed(&mut self, ev: KeyEvent, user: &UserKeymap) -> ResolverOutcome {
        // User bindings win at the top level (and for the `g` chord —
        // see below). For the explicit chord prefixes (`^a`, `[`, `]`,
        // `H`, `W`, `m`, `'`, `y`), the pending state wins so the
        // second key completes the chord. Without this, any user
        // binding for a single letter (e.g. `n`, `p`, `g`, `1`) would
        // silently break the corresponding chord (`^a-n`, `]g`, `H1`,
        // `yp`, etc.) — the user reported `^a-n`/`^a-p` flashing the
        // pending indicator and then disappearing because their `n`/`p`
        // bindings preempted the chord resolution.
        //
        // `g` is the deliberate exception: bare `g` is also a vi motion
        // fragment that users may want to remap (the
        // `user_binding_resets_pending` test covers this), so chords
        // built on `g` (`gd`, `gf`, …) remain user-overridable.
        let chord_locked = !matches!(self.pending, PendingSeq::Normal | PendingSeq::G);
        if !chord_locked && let Some(action) = user.find(&ev) {
            self.reset();
            return ResolverOutcome::User(action.clone());
        }
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);

        // `^a ^a` (prefix prefix) — screen/tmux "last window": jump back
        // to the previously-active tab. Handled here, before the generic
        // ctrl block below, which would otherwise treat the second `^a`
        // as a fresh prefix and just re-arm `PendingSeq::W`. Plain `^a a`
        // (no ctrl on the second key) still falls through to focus-down
        // in the `W` chord block.
        if self.pending == PendingSeq::W && ctrl && matches!(ev.code, KeyCode::Char('a' | 'A')) {
            self.reset();
            return ResolverOutcome::Action(Action::PaneLastTab);
        }

        // Mid-sequence: Ctrl-A / Ctrl-W prefix waiting for a pane command.
        // Combines screen(1)-style (^a n=next, ^a p=prev, ^a c=new, ^a k=kill)
        // with vim-style (j/k focus, +/- resize).
        //
        // Runs BEFORE the generic Ctrl block below and matches on the key
        // *code* regardless of the Ctrl modifier. Holding Ctrl through the
        // second key (`^a ^n`, `^a ^p`, …) is the natural way to fire the
        // chord fast, and screen treats it the same as `^a n`. With the old
        // order, the Ctrl block ate `^a ^n` / `^a ^p` as an unknown control
        // code and reset the chord — the "rapid `^a-n` eats the command"
        // regression. (`^a ^a` is already intercepted just above as
        // PaneLastTab, so it never reaches the `'a' | 'A'` focus arm here.)
        if self.pending == PendingSeq::W {
            let out = match ev.code {
                // Focus switching — vim-style j/J, plus plain `^a a`/`^a A`.
                // Grouped to shut up clippy::match_same_arms.
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
                KeyCode::Char('u' | 'U') => ResolverOutcome::Action(Action::QuickSelectOpen),
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

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

        // Mid-sequence: `d` already seen — vim-style delete chord.
        // `dd` (no count) → remove current selection (picks-or-
        //   cursor, same shape as `R`).
        // `Ndd` (count set before the first `d`) → remove cursor +
        //   N-1 entries below, ignoring picks. The count is the
        //   user being explicit about scope.
        // Anything else: cancel the chord silently.
        if self.pending == PendingSeq::D {
            let out = match ev.code {
                KeyCode::Char('d') => {
                    let count = self.count.take().map(|n| n as usize);
                    ResolverOutcome::Action(Action::RemovePrompt(count))
                }
                _ => ResolverOutcome::Ignored,
            };
            self.reset();
            return out;
        }

        // Mid-sequence: `Z` already seen — vim-style quit chord.
        // `ZZ` → Quit (which already auto-saves the session).
        // Anything else: cancel.
        if self.pending == PendingSeq::Z {
            let out = match ev.code {
                KeyCode::Char('Z') => ResolverOutcome::Action(Action::Quit),
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
                KeyCode::Char('s') => ResolverOutcome::Action(Action::SortReverse),
                KeyCode::Char('U') => ResolverOutcome::Action(Action::ShowUserHost),
                KeyCode::Char('B') => ResolverOutcome::Action(Action::OpenTaskViewer),
                KeyCode::Char('p') => ResolverOutcome::Action(Action::ReopenLastBuffer),
                KeyCode::Char('y') => ResolverOutcome::Action(Action::OpenGraveyardView),
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

        // Mid-sequence: `[` or `]` waiting for a "next/prev <thing>"
        // sub-command. Currently just `g` for git changes.
        if matches!(
            self.pending,
            PendingSeq::PrevBracket | PendingSeq::NextBracket
        ) {
            let is_next = self.pending == PendingSeq::NextBracket;
            let out = match ev.code {
                KeyCode::Char('g') => ResolverOutcome::Action(if is_next {
                    Action::JumpNextGitChange
                } else {
                    Action::JumpPrevGitChange
                }),
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
                KeyCode::Char('f') => ResolverOutcome::Action(Action::YankPaths),
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
        // F9 opens the pane with `claude --resume` to continue the last
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
                    // Saturate rather than overflow: a pathologically long digit
                    // run (e.g. holding a key) would otherwise panic in debug and
                    // silently wrap in release. A count of u32::MAX is already far
                    // past any real listing, so saturation is harmless.
                    self.count = Some(
                        self.count
                            .unwrap_or(0)
                            .saturating_mul(10)
                            .saturating_add(digit),
                    );
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
            // `d` is now a chord-arming key (vim parity: `dd` /
            // `Ndd` for delete). Bare `d` was previously an alias
            // for `Enter` (EnterOrDisplay) — that role is now
            // Enter-only.
            KeyCode::Char('d') => {
                self.pending = PendingSeq::D;
                ResolverOutcome::Pending
            }
            // `Z` arms `ZZ` (vim quit). A single `Z` is a no-op
            // unless followed by another `Z`; anything else
            // cancels the chord.
            KeyCode::Char('Z') => {
                self.pending = PendingSeq::Z;
                ResolverOutcome::Pending
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

            // [g / ]g — jump cursor to prev/next git-changed entry.
            // Bracket pairs are reserved for "next/prev <thing>" jumps,
            // mirroring the [t/]t and [b/]b chords in the pager.
            KeyCode::Char('[') => {
                self.pending = PendingSeq::PrevBracket;
                ResolverOutcome::Pending
            }
            KeyCode::Char(']') => {
                self.pending = PendingSeq::NextBracket;
                ResolverOutcome::Pending
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
                ResolverOutcome::Action(Action::RemovePrompt(None))
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
            KeyCode::Char('S') => {
                self.reset();
                ResolverOutcome::Action(Action::SortCycle)
            }

            // Help overlay.
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.reset();
                ResolverOutcome::Action(Action::Help)
            }

            // Display / edit in top pane.
            KeyCode::Char('D') => {
                self.reset();
                ResolverOutcome::Action(Action::DisplayInPane)
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
mod tests;

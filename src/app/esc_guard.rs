//! The `Esc` guard for Claude Code's vim prompt (#925). With `/vim` on, `Esc`
//! in INSERT mode only leaves INSERT, but in NORMAL mode it interrupts the
//! running turn, and a vim user presses it by reflex. So a bare `Esc` (or
//! `^[`, the same byte) to a working Claude tab whose prompt isn't in INSERT
//! mode is dropped, with a flash naming the key that goes through: `Shift-Esc`
//! where the terminal reports Shift (it took the kitty keyboard flag), else
//! `^a Esc`, which works on every terminal. Both send one plain `Esc` byte,
//! which Claude Code reads as the interrupt and spyc's activity bookkeeping
//! counts as one.
//!
//! The mode is Claude Code's own report through its statusline
//! (`agent::claude_statusline`), never read off the screen: the `-- INSERT --`
//! indicator can be hidden, and its absence proves nothing. So the guard fails
//! open: no report, a mode spyc doesn't know, any state but `working`, any
//! other agent, or `[pane] claude_esc_guard = false` lets `Esc` through.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Effect, PaneInput, PaneTarget};
use crate::agent::claude_statusline::VimMode;
use crate::pane::AgentActivity;
use crate::state::sessions::AgentKind;

/// The plain `Esc` both ways through send.
const ESC: u8 = 0x1b;

/// What the guard reads of the focused tab.
#[derive(Clone, Copy, Debug)]
pub(super) struct EscSnapshot {
    /// `[pane] claude_esc_guard`.
    pub enabled: bool,
    /// The tab runs Claude Code.
    pub claude: bool,
    /// Its activity is `working`, the one state an `Esc` interrupts.
    pub working: bool,
    /// The vim mode its newest statusline reported.
    pub vim_mode: Option<VimMode>,
}

/// What becomes of a key the focused pane owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EscKey {
    /// Not the guard's to touch: it goes out as typed.
    Forward,
    /// `Shift-Esc`: one plain `Esc` byte.
    PlainEsc,
    /// A bare `Esc` that would interrupt the turn: dropped.
    Blocked(VimMode),
}

/// Decide a pane key's fate. **Pure** (the `route.rs` / `focus.rs` template).
pub(super) fn esc_key(key: KeyEvent, snap: EscSnapshot) -> EscKey {
    if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::SHIFT {
        return EscKey::PlainEsc;
    }
    let guarded = snap.enabled && snap.claude && snap.working && is_bare_esc(key);
    match snap.vim_mode {
        Some(mode) if guarded && mode != VimMode::Insert => EscKey::Blocked(mode),
        _ => EscKey::Forward,
    }
}

/// A key that sends a lone `Esc` byte: `Esc` itself, or `^[` on a terminal
/// that tells the two apart (on any other, `^[` arrives as `Esc`).
fn is_bare_esc(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => key.modifiers.is_empty(),
        KeyCode::Char('[') => key.modifiers == KeyModifiers::CONTROL,
        _ => false,
    }
}

/// The flash for a blocked `Esc`, naming the key that goes through.
pub(super) fn blocked_flash(mode: VimMode, shift_esc: bool) -> String {
    let through = if shift_esc { "Shift-Esc" } else { "^a Esc" };
    format!(
        "`Esc` blocked: claude is working in vim {} mode. `{through}` interrupts it.",
        mode.name()
    )
}

impl App {
    /// Whether the terminal reports `Shift-Esc` apart from `Esc`; set once,
    /// after `setup_terminal`.
    pub const fn set_shift_esc(&mut self, on: bool) {
        self.runtime.shift_esc = on;
    }

    /// The `SendToPane` for a key the focused pane owns, through the guard:
    /// none for a blocked `Esc`, whose flash says which key goes through.
    pub(super) fn send_pane_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let input = match esc_key(key, self.esc_snapshot()) {
            EscKey::Forward => PaneInput::Key(key),
            EscKey::PlainEsc => PaneInput::Bytes(vec![ESC]),
            EscKey::Blocked(mode) => {
                let flash = blocked_flash(mode, self.runtime.shift_esc);
                self.state.flash_info(flash);
                return Vec::new();
            }
        };
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input,
            on_ok: None,
            err_prefix: None,
        }]
    }

    /// `^a Esc`: a plain `Esc` to the active pane, past the guard.
    pub(super) fn send_esc_to_pane(&mut self) -> Vec<Effect> {
        if self.runtime.project().pane_tabs.is_none() {
            self.state.flash_error("no pane open (Ctrl-\\ to open one)");
            return Vec::new();
        }
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::Bytes(vec![ESC]),
            on_ok: None,
            err_prefix: Some("send failed"),
        }]
    }

    fn esc_snapshot(&self) -> EscSnapshot {
        let tabs = self.runtime.project().pane_tabs.as_ref();
        let info = tabs.map(crate::pane::PaneTabs::active_info);
        EscSnapshot {
            enabled: self.state.active_project().config.pane.claude_esc_guard,
            claude: info.is_some_and(|info| info.agent().kind() == AgentKind::Claude),
            working: info.is_some_and(|info| info.activity == AgentActivity::Working),
            vim_mode: info.and_then(|info| info.claude_statusline.as_ref()?.0.vim_mode),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A working Claude tab in vim NORMAL mode with the guard on: the state
    /// every case below varies one input of.
    const GUARDED: EscSnapshot = EscSnapshot {
        enabled: true,
        claude: true,
        working: true,
        vim_mode: Some(VimMode::Normal),
    };

    fn esc(modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, modifiers)
    }

    #[test]
    fn a_bare_esc_outside_insert_mode_is_blocked() {
        let ctrl_bracket = KeyEvent::new(KeyCode::Char('['), KeyModifiers::CONTROL);
        for mode in [VimMode::Normal, VimMode::Visual, VimMode::VisualLine] {
            let snap = EscSnapshot {
                vim_mode: Some(mode),
                ..GUARDED
            };
            for key in [esc(KeyModifiers::NONE), ctrl_bracket] {
                assert_eq!(esc_key(key, snap), EscKey::Blocked(mode), "{key:?}");
            }
        }
    }

    #[test]
    fn the_guard_fails_open() {
        let open = [
            EscSnapshot {
                vim_mode: Some(VimMode::Insert),
                ..GUARDED
            },
            EscSnapshot {
                vim_mode: None,
                ..GUARDED
            },
            EscSnapshot {
                working: false,
                ..GUARDED
            },
            EscSnapshot {
                claude: false,
                ..GUARDED
            },
            EscSnapshot {
                enabled: false,
                ..GUARDED
            },
        ];
        for snap in open {
            assert_eq!(
                esc_key(esc(KeyModifiers::NONE), snap),
                EscKey::Forward,
                "{snap:?}"
            );
        }
    }

    #[test]
    fn shift_esc_is_a_plain_esc_guarded_or_not() {
        for snap in [
            GUARDED,
            EscSnapshot {
                enabled: false,
                ..GUARDED
            },
        ] {
            assert_eq!(
                esc_key(esc(KeyModifiers::SHIFT), snap),
                EscKey::PlainEsc,
                "{snap:?}"
            );
        }
    }

    /// Only a bare `Esc` is the reflex the guard is for; any other key, or
    /// `Esc` or `^[` with another modifier, goes out as typed.
    #[test]
    fn other_keys_go_out_as_typed() {
        for key in [
            esc(KeyModifiers::ALT),
            esc(KeyModifiers::CONTROL),
            esc(KeyModifiers::SHIFT | KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE),
            KeyEvent::new(
                KeyCode::Char('['),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            assert_eq!(esc_key(key, GUARDED), EscKey::Forward, "{key:?}");
        }
    }

    #[test]
    fn the_flash_names_shift_esc_only_where_the_terminal_reports_shift() {
        assert_eq!(
            blocked_flash(VimMode::Normal, true),
            "`Esc` blocked: claude is working in vim NORMAL mode. `Shift-Esc` interrupts it."
        );
        assert_eq!(
            blocked_flash(VimMode::VisualLine, false),
            "`Esc` blocked: claude is working in vim VISUAL LINE mode. `^a Esc` interrupts it."
        );
    }
}

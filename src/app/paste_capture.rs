//! Capturing what the user pastes into an agent pane.
//!
//! An agent reads the system clipboard *itself* when its paste key arrives —
//! the terminal never carries image bytes — so the keystroke is spyc's only
//! signal, and the moment it passes is the only moment the same image is still
//! there to be read. This module decides whether a given keystroke is that
//! signal; the read itself is an `Effect` handled off-thread.
//!
//! The decision is deliberately narrow. Firing on the wrong key would mean
//! reading (and encoding) the clipboard on a keystroke that meant something
//! else — `^v` is readline's quoted-insert in a shell — so the capture is
//! gated on the focused tab actually running an agent that declares the key.

use crossterm::event::KeyEvent;

use crate::app::{App, Effect, image_ops};

/// The inputs `should_capture` needs, lifted off the live tab so the decision
/// is testable without a pty. `Copy` — same shape as `route.rs`'s snapshot.
#[derive(Clone, Copy, Debug)]
pub struct PasteSnapshot {
    /// The key the agent uses to paste an image, if it has one. `None` for a
    /// shell tab or an agent spyc has no verified key for.
    pub paste_key: Option<KeyEvent>,
    /// Whether the user has left the capture enabled (`[pane]
    /// preview_pasted_images`).
    pub enabled: bool,
}

/// Whether `key` is the focused agent's image-paste key, and spyc should
/// therefore read the clipboard alongside forwarding it.
///
/// Compares code *and* modifiers: `v` and `^v` are different keystrokes, and
/// matching on the code alone would fire the capture on every letter `v` the
/// user types into a prompt.
pub fn should_capture(snap: PasteSnapshot, key: KeyEvent) -> bool {
    if !snap.enabled {
        return false;
    }
    snap.paste_key
        .is_some_and(|k| k.code == key.code && k.modifiers == key.modifiers)
}

impl App {
    /// The capture effect for `key`, if it's the focused agent's paste key.
    /// Empty otherwise — the common case, and cheap: no clipboard access
    /// happens here, only on the worker the effect spawns.
    pub(crate) fn plan_clipboard_capture(&self, key: KeyEvent) -> Vec<Effect> {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return Vec::new();
        };
        let info = tabs.active_info();
        let snap = PasteSnapshot {
            paste_key: crate::agent::detect(&info.command).image_paste_key(),
            enabled: self.state.config.pane.preview_pasted_images,
        };
        if !should_capture(snap, key) {
            return Vec::new();
        }
        vec![Effect::CaptureClipboardImage(
            image_ops::ClipboardCaptureOp {
                tab_id: info.id.clone(),
            },
        )]
    }

    /// Drop the focused tab's uncommitted images — its prompt was submitted, so
    /// the agent's transcript is the record from here on.
    pub(crate) fn clear_pending_images_for_active_tab(&mut self) {
        let Some(id) = self
            .runtime
            .pane_tabs
            .as_ref()
            .map(|t| t.active_info().id.clone())
        else {
            return;
        };
        self.state.pane.pending_images.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn ctrl_v() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)
    }

    fn snap(paste_key: Option<KeyEvent>, enabled: bool) -> PasteSnapshot {
        PasteSnapshot { paste_key, enabled }
    }

    #[test]
    fn the_agents_paste_key_captures() {
        assert!(should_capture(snap(Some(ctrl_v()), true), ctrl_v()));
    }

    /// The bug this guards: matching the code alone would fire a clipboard read
    /// (and a full RGBA encode) on every literal `v` the user types.
    #[test]
    fn a_bare_v_is_not_the_paste_key() {
        let plain_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);
        assert!(!should_capture(snap(Some(ctrl_v()), true), plain_v));
    }

    /// A shell tab declares no paste key — `^v` there is readline's
    /// quoted-insert, and reading the clipboard would be pure waste.
    #[test]
    fn a_tab_with_no_paste_key_never_captures() {
        assert!(!should_capture(snap(None, true), ctrl_v()));
    }

    #[test]
    fn the_config_switch_turns_it_off() {
        assert!(!should_capture(snap(Some(ctrl_v()), false), ctrl_v()));
    }

    /// Sanity on the wiring the decision depends on: claude declares `^v`, and
    /// a plain shell declares nothing.
    #[test]
    fn claude_declares_ctrl_v_and_a_shell_declares_nothing() {
        assert_eq!(
            crate::agent::detect("claude").image_paste_key(),
            Some(ctrl_v())
        );
        assert!(crate::agent::detect("zsh").image_paste_key().is_none());
    }
}

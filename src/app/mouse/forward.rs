//! Forwarding a mouse event to a child that speaks the mouse, plus the focus
//! entry points and the child-facing report encoder. Extracted verbatim from
//! `mouse/mod.rs`.
//!
//! Press/release must pair to the *same* child, so `forward_to_child` records
//! whether the press was forwarded and `forward_release` reads that back.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use super::super::pager_handler::PagerSlot;
use super::super::{Effect, FrameLayout};
use super::route::Region;

impl super::super::App {
    /// Encode `ev` in the child's own protocol and send it to the active pane.
    ///
    /// Records whether the press was forwarded, so [`Self::forward_release`] can
    /// pair the matching release to the same child.
    pub(super) fn forward_to_child(&mut self, ev: MouseEvent, layout: &FrameLayout) -> Vec<Effect> {
        let Some(report) = mouse_report(ev, layout) else {
            return Vec::new();
        };
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return Vec::new();
        };
        let (mode, encoding) = tabs.active().mouse_protocol();
        let bytes = crate::pane::input::encode_mouse(report, mode, encoding);
        if bytes.is_empty() {
            return Vec::new();
        }
        // A press we forwarded owes the child a release. Wheel ticks don't (they
        // have no release half), so only a real button arms the obligation.
        if matches!(ev.kind, MouseEventKind::Down(_)) {
            self.view.mouse_press_forwarded = true;
        }
        // `Effect::SendToPane`, not a direct `send_bytes`: the executor is the only
        // thing that touches the OS.
        vec![Effect::SendToPane {
            target: super::super::effect::PaneTarget::Active,
            input: super::super::effect::PaneInput::Bytes(bytes),
            on_ok: None,
            // No flash on failure: a dead pty surfaces through the exit path, and
            // one per wheel tick would bury the real message under repeats.
            err_prefix: None,
        }]
    }

    /// Deliver a drag to the child that received the press, if any.
    ///
    /// Unlike a release this does NOT consume the pairing flag — a drag is the
    /// middle of the gesture and more will follow, so the flag has to survive
    /// until the release actually arrives.
    pub(super) fn forward_drag(
        &mut self,
        ev: MouseEvent,
        frame: ratatui::layout::Rect,
    ) -> Vec<Effect> {
        if !self.view.mouse_press_forwarded {
            return Vec::new();
        }
        let layout = self.frame_layout(frame);
        self.forward_to_child(ev, &layout)
    }

    /// Deliver a button release to the child that received the press.
    ///
    /// Keyed on the press having been forwarded rather than on where the pointer
    /// is now, which is what makes the pairing exact: a press that went to the list
    /// never produces a release for the child, and a press delivered to the child
    /// always gets its release even if the pointer left the pane before the button
    /// came up. An unpaired release is as bad as a missing one — the child would
    /// see a button it never saw pressed.
    pub(super) fn forward_release(
        &mut self,
        ev: MouseEvent,
        _button: MouseButton,
        frame: ratatui::layout::Rect,
    ) -> Vec<Effect> {
        if !std::mem::take(&mut self.view.mouse_press_forwarded) {
            return Vec::new();
        }
        let layout = self.frame_layout(frame);
        self.forward_to_child(ev, &layout)
    }
}

impl super::super::App {
    /// Give the keyboard to `region`, reusing the same entry points `^a j`/`^a k`
    /// and `^a a`/`^a b` use.
    ///
    /// `set_pane_focus` is doing real work here, not just being tidy: it declines
    /// while `^a z`-zoomed (the target region is collapsed off-screen) and, coming
    /// back from the pane, restores whichever split column the user left from.
    /// Assigning `state.focus` directly would lose both.
    /// Give the keyboard to the region that OWNS `slot`.
    ///
    /// Distinct from [`Self::focus_region`], which answers from layout geometry: a
    /// pager drawn over the pane's rect would otherwise focus the pty behind it.
    /// A `Modal` needs no move — `recompute_focus` resolves a full-frame Overlay to
    /// `Focus::Pager(Overlay)` on its own, and touching pane focus here is exactly
    /// what leaked a click through to the pane.
    pub(super) fn focus_pager_slot(&mut self, slot: PagerSlot) {
        match slot {
            PagerSlot::Modal => {}
            PagerSlot::Scrollback => self.set_pane_focus(true),
            PagerSlot::Top => {
                self.set_pane_focus(false);
                self.vsplit_focus(crate::app::state::Side::Left);
            }
            PagerSlot::Right => {
                self.set_pane_focus(false);
                self.vsplit_focus(crate::app::state::Side::Right);
            }
        }
    }

    pub(super) fn focus_region(&mut self, region: Option<Region>) {
        match region {
            Some(Region::Pane) => self.set_pane_focus(true),
            Some(Region::List) => {
                self.set_pane_focus(false);
                self.vsplit_focus(crate::app::state::Side::Left);
            }
            Some(Region::RightColumn) => {
                self.set_pane_focus(false);
                self.vsplit_focus(crate::app::state::Side::Right);
            }
            // Chrome and off-frame: nothing owns the keyboard there.
            Some(Region::Divider | Region::Prompt | Region::Status | Region::VDivider) | None => {}
        }
    }
}

/// Build the child-facing report for any forwardable mouse event, translating the
/// pointer into the pane's own coordinate space. `None` for kinds spyc doesn't
/// forward.
///
/// Takes the whole event rather than a direction flag, which is the shape error
/// this replaces: the previous `wheel_report(ev, up, layout)` hardcoded
/// `if up { 64 } else { 65 }`, so when buttons started routing here they were
/// handed `up = false` and every click went out as **xterm wheel-down (65)**.
/// Claude's own decoder reads 65 as a wheel tick and its button decoder rejects
/// anything with bit 64 set, so a left-click scrolled the agent and never clicked.
/// A function that maps the kind to its button can't express that mistake.
///
/// The coordinate translation is the other easy-to-skip half, and it produces a
/// *worse than broken* result: frame-absolute coordinates make the child think the
/// pointer is `pane.y` rows above where it is, which reads as the child's bug.
pub(super) fn mouse_report(
    ev: MouseEvent,
    layout: &super::FrameLayout,
) -> Option<crate::pane::input::MouseReport> {
    use crossterm::event::KeyModifiers as K;
    use crossterm::event::MouseButton as B;

    // xterm button encoding: 0/1/2 = left/middle/right, 64/65 = wheel up/down;
    // a drag is the held button with the motion bit set (added by `encode_mouse`).
    let (button, release, motion) = match ev.kind {
        MouseEventKind::ScrollUp => (64, false, false),
        MouseEventKind::ScrollDown => (65, false, false),
        MouseEventKind::Down(B::Left) => (0, false, false),
        MouseEventKind::Down(B::Middle) => (1, false, false),
        MouseEventKind::Down(B::Right) => (2, false, false),
        MouseEventKind::Up(B::Left) => (0, true, false),
        MouseEventKind::Up(B::Middle) => (1, true, false),
        MouseEventKind::Up(B::Right) => (2, true, false),
        MouseEventKind::Drag(B::Left) => (0, false, true),
        MouseEventKind::Drag(B::Middle) => (1, false, true),
        MouseEventKind::Drag(B::Right) => (2, false, true),
        // Not forwarded. `Moved` is buttonless motion, which spyc never asks for
        // (1002 reports only while a button is held) and `proc.rs` filters;
        // horizontal wheel has no spyc behaviour. Listed exhaustively so a new
        // `MouseEventKind` is a compile error here rather than a silent no-op.
        MouseEventKind::Moved | MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
            return None;
        }
    };

    let origin = layout
        .pane
        .unwrap_or(ratatui::layout::Rect::new(0, 0, 0, 0));
    Some(crate::pane::input::MouseReport {
        button,
        release,
        motion,
        col: ev.column.saturating_sub(origin.x),
        row: ev.row.saturating_sub(origin.y),
        shift: ev.modifiers.contains(K::SHIFT),
        alt: ev.modifiers.contains(K::ALT),
        ctrl: ev.modifiers.contains(K::CONTROL),
    })
}

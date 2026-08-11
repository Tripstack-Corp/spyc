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

#[cfg(test)]
mod tests {
    use super::super::super::{App, Effect, PaneInput, PaneTarget};
    use super::super::tests::make_pane_speak_mouse;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    fn at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// The bytes a `SendToPane` effect carries, or `None` for anything else.
    fn sent(fx: &[Effect]) -> Option<&[u8]> {
        match fx {
            [
                Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::Bytes(bytes),
                    ..
                },
            ] => Some(bytes),
            _ => None,
        }
    }

    /// An App with a mouse-aware pane and a point the layout says is inside it.
    fn app_with_a_mouse_pane(dir: &std::path::Path) -> (App, (u16, u16)) {
        let mut app = App::test_app(dir.to_path_buf());
        app.view.term_size = (120, 24);
        app.open_pane_tab("cat");
        make_pane_speak_mouse(&mut app);
        let layout = app.frame_layout(ratatui::layout::Rect::new(0, 0, 120, 24));
        let pane = layout.pane.expect("a pane is open");
        (app, (pane.x + 3, pane.y + 2))
    }

    /// **A press the child received owes it a release.** claude starts a
    /// selection on press and fires the actual click on RELEASE, so a press with
    /// no matching release leaves it drag-selecting forever and never clicking
    /// anything. The pairing is keyed on the press having been forwarded, not on
    /// where the pointer is now, so it holds even if the pointer left the pane.
    #[test]
    fn a_forwarded_press_is_followed_by_its_release() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let (mut app, (col, row)) = app_with_a_mouse_pane(tmp.path());
            let frame = ratatui::layout::Rect::new(0, 0, 120, 24);
            let layout = app.frame_layout(frame);

            let press = app.forward_to_child(
                at(MouseEventKind::Down(MouseButton::Left), col, row),
                &layout,
            );
            assert!(
                sent(&press).is_some(),
                "the press reaches the child: {press:?}"
            );
            assert!(
                app.view.mouse_press_forwarded,
                "and arms the obligation to deliver the release"
            );

            // Pointer has left the pane entirely — the release still goes to the
            // child that saw the press.
            let release = app.forward_release(
                at(MouseEventKind::Up(MouseButton::Left), col, row),
                MouseButton::Left,
                frame,
            );
            assert!(
                sent(&release).is_some(),
                "the release reaches the same child: {release:?}"
            );
            assert!(
                !app.view.mouse_press_forwarded,
                "and the obligation is discharged, not left standing"
            );

            // A second release is unpaired: the child never saw a press for it,
            // and an unpaired release is as bad as a missing one.
            let extra = app.forward_release(
                at(MouseEventKind::Up(MouseButton::Left), col, row),
                MouseButton::Left,
                frame,
            );
            assert!(
                extra.is_empty(),
                "an unpaired release is not forwarded: {extra:?}"
            );
        });
    }

    /// A press that never reached the child produces no release for it. Without
    /// this the child would see a button come up that it never saw go down.
    #[test]
    fn a_release_whose_press_went_elsewhere_is_not_forwarded() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let (mut app, (col, row)) = app_with_a_mouse_pane(tmp.path());
            let frame = ratatui::layout::Rect::new(0, 0, 120, 24);
            // Nothing armed the flag — the press landed on the file list.
            assert!(!app.view.mouse_press_forwarded);
            let fx = app.forward_release(
                at(MouseEventKind::Up(MouseButton::Left), col, row),
                MouseButton::Left,
                frame,
            );
            assert!(fx.is_empty(), "got {fx:?}");
        });
    }

    /// A drag is the middle of one gesture, so it rides the same pairing — but
    /// must NOT consume it: more drags and then the release still have to arrive.
    #[test]
    fn a_drag_rides_the_pairing_without_consuming_it() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let (mut app, (col, row)) = app_with_a_mouse_pane(tmp.path());
            let frame = ratatui::layout::Rect::new(0, 0, 120, 24);
            let drag = at(MouseEventKind::Drag(MouseButton::Left), col, row);

            assert!(
                app.forward_drag(drag, frame).is_empty(),
                "a drag whose press went elsewhere never reaches the child"
            );

            app.view.mouse_press_forwarded = true;
            assert!(
                sent(&app.forward_drag(drag, frame)).is_some(),
                "a claimed drag is delivered"
            );
            assert!(
                app.view.mouse_press_forwarded,
                "and the flag survives, or the release that follows would be dropped"
            );
        });
    }

    /// A wheel tick has no release half, so forwarding one must not arm an
    /// obligation the child will never be paid — the next real release would
    /// otherwise be delivered as if it paired with a press.
    #[test]
    fn a_forwarded_wheel_tick_arms_no_release() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let (mut app, (col, row)) = app_with_a_mouse_pane(tmp.path());
            let layout = app.frame_layout(ratatui::layout::Rect::new(0, 0, 120, 24));
            let fx = app.forward_to_child(at(MouseEventKind::ScrollDown, col, row), &layout);
            assert!(
                sent(&fx).is_some(),
                "the tick still reaches the child: {fx:?}"
            );
            assert!(
                !app.view.mouse_press_forwarded,
                "a wheel tick owes no release"
            );
        });
    }
}

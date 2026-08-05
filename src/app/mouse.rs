//! Pure mouse routing. Where does a mouse event go?
//!
//! The sibling of [`super::route`] (keys/paste) and [`super::focus`], following
//! the same template: a `Copy` snapshot, a pure decision fn, unit tests. The
//! reason it's a module rather than an inline `run.rs` branch is the one
//! documented in `route.rs`'s header — five shipped routing bugs came from
//! decisions living inline next to their side effects.
//!
//! **Hit-test the pointer, not the keyboard focus.** This is the whole point of a
//! mouse: the wheel scrolls what the cursor is over, even when the keyboard is
//! somewhere else. So [`region_at`] resolves the pointer against the same
//! [`FrameLayout`] the renderer used, and [`route_mouse`] takes that region —
//! never `state.focus`.
//!
//! This module decides; it does not act. Wiring the decision to an
//! `Event::Mouse` arm is a later change, which is why nothing here reads or
//! mutates `App`.

use ratatui::layout::Rect;

use super::modal::{Modal, ModalSnapshot, active_modal};
use super::{Effect, FrameLayout};
use crate::ui::pager::Mount;

/// Which frame region the pointer is over. Resolved from the pointer's
/// column/row against the live [`FrameLayout`], so it follows the cursor rather
/// than the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Region {
    /// The file list — the left column when a vertical split is open.
    List,
    /// The right column's content when a vertical split is open (`b` / the live
    /// preview).
    RightColumn,
    /// The bottom pty pane.
    Pane,
    /// The horizontal divider between list and pane (carries the tab bar).
    Divider,
    /// The prompt row (`:` line / shell prompt / flash).
    Prompt,
    /// The status bar.
    Status,
    /// The 1-column vertical separator between split columns.
    VDivider,
}

/// Where a mouse event is dispatched.
///
/// Deliberately smaller than the region set: several regions route to the same
/// sink, and a sink is "what to do", not "where the pointer was".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseSink {
    /// Drop the event. A modal owns the screen, the region isn't interactive, or
    /// the pane's child can't use it.
    Swallow,
    /// Move the file-list cursor / scroll the list.
    ///
    /// Not optional, and easy to mistake for one: wheel-over-list works today
    /// only because DEC 1007 has the *terminal* translate wheel into arrow keys.
    /// Enabling 1000 stops that translation, so without this sink turning capture
    /// on would trade a pane bug for a list bug.
    ListCursor,
    /// Scroll the pager under the pointer.
    Pager(Mount),
    /// Encode and forward to the pane's child (which requested mouse reporting).
    PaneForward,
    /// Give the keyboard to the pane AND forward the event to its child — the
    /// left-click-through contract. Both halves, in one variant, because a sink
    /// that only forwarded was how the focus half came to be silently missing
    /// while a test asserting the sink still passed.
    FocusAndForward,
    /// Give the keyboard to the region under the pointer.
    FocusRegion,
    /// Paste the system clipboard wherever a paste would land.
    Paste,
    /// Open the leader menu (right-click, from anywhere).
    LeaderMenu,
}

/// What the user did, reduced to the axis routing cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Gesture {
    /// A wheel tick. Direction is the caller's business — routing is the same
    /// either way.
    Wheel,
    /// Left button pressed.
    Left,
    /// Middle button pressed.
    Middle,
    /// Right button pressed.
    Right,
}

/// The App-state bits the mouse decision reads. `Copy` so tests build one
/// inline, and so the decision provably can't touch anything else.
#[derive(Debug, Clone, Copy)]
pub(super) struct MouseSnapshot {
    /// A transient modal that owns the screen, if any.
    pub modal: Option<Modal>,
    /// Where the pointer is — hit-tested via [`region_at`], NOT keyboard focus.
    pub region: Option<Region>,
    /// A pager mounted in the focused column, and where.
    pub pager_mount: Option<Mount>,
    /// The pane's child requested mouse reporting (`mouse_protocol_mode != None`).
    /// When false, forwarding would type escape bytes into a prompt that never
    /// asked for them — the bracketed-paste bug (#170) in a new costume.
    pub pane_wants_mouse: bool,
}

/// Route one mouse event to a sink. Pure and total.
///
/// Order matters and mirrors `route_input`'s: a modal wins over everything, then
/// the region decides. A pager mounted as an overlay covers the list, so it takes
/// events aimed at the list region.
pub(super) const fn route_mouse(snap: MouseSnapshot, gesture: Gesture) -> MouseSink {
    // A modal owns the whole screen; the pointer's region is irrelevant.
    if snap.modal.is_some() {
        return MouseSink::Swallow;
    }
    let Some(region) = snap.region else {
        return MouseSink::Swallow;
    };

    // Middle and right are spyc's everywhere, never forwarded — otherwise the
    // gesture would be unavailable in exactly the region where the pane has
    // focus, which is where you'd reach for it. Documented alongside the
    // selection caveat, since `:mouse off` is the way to get the child's own
    // right-click menu back.
    match gesture {
        Gesture::Middle => return MouseSink::Paste,
        Gesture::Right => return MouseSink::LeaderMenu,
        Gesture::Left => {
            // Left is click-THROUGH: focus the region, and (for a mouse-aware
            // child) let the event reach it too. The pane is live and visible, so
            // swallowing the first click just to focus would read as broken.
            return if matches!(region, Region::Pane) && snap.pane_wants_mouse {
                MouseSink::FocusAndForward
            } else {
                MouseSink::FocusRegion
            };
        }
        Gesture::Wheel => {}
    }

    match region {
        // An `Overlay`/`TopPane` pager is painted over the list, so a pointer in
        // the list region is really over the pager.
        Region::List | Region::RightColumn => match snap.pager_mount {
            Some(mount) => MouseSink::Pager(mount),
            None => MouseSink::ListCursor,
        },
        Region::Pane => {
            if snap.pane_wants_mouse {
                MouseSink::PaneForward
            } else {
                // No spyc-owned pane scrollback in v1 (see the plan's Deferred
                // section): silence beats typing `\e[<64;20;5M` at a shell.
                MouseSink::Swallow
            }
        }
        // Chrome. Nothing to scroll, and clicking it is a later addition (tab
        // bar hit-testing lives on the divider).
        Region::Divider | Region::Prompt | Region::Status | Region::VDivider => MouseSink::Swallow,
    }
}

/// True when `(col, row)` is inside `rect`.
const fn hit(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

/// Resolve a pointer position to a [`Region`], or `None` when it lands on no
/// region spyc owns.
///
/// Checked most-specific first: the 1-column `vdivider` and the right column sit
/// *inside* what `compute_layout` calls `list`, so testing `list` first would
/// swallow both. `status` is checked before `prompt` for the same reason
/// `status_position = "bottom"` is the off-by-one trap `FrameLayout.top_unit`
/// documents — with the status row last, the two are adjacent.
pub(super) const fn region_at(layout: &FrameLayout, col: u16, row: u16) -> Option<Region> {
    if let Some(pane) = layout.pane
        && hit(pane, col, row)
    {
        return Some(Region::Pane);
    }
    if let Some(divider) = layout.divider
        && hit(divider, col, row)
    {
        return Some(Region::Divider);
    }
    if let Some(vdivider) = layout.vdivider
        && hit(vdivider, col, row)
    {
        return Some(Region::VDivider);
    }
    if let Some(right) = layout.right
        && hit(right, col, row)
    {
        return Some(Region::RightColumn);
    }
    if hit(layout.status, col, row) {
        return Some(Region::Status);
    }
    if hit(layout.prompt, col, row) {
        return Some(Region::Prompt);
    }
    if hit(layout.list, col, row) {
        return Some(Region::List);
    }
    None
}

// ── wiring: the impure half ───────────────────────────────────────────────

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

impl super::App {
    /// Handle one mouse event: hit-test the pointer, route it, dispatch the sink.
    ///
    /// Only reachable while `[mouse] capture` is on — with it off the terminal
    /// sends DEC 1007 arrow keys instead and this never fires.
    pub(super) fn handle_mouse(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let (cols, rows) = self.view.term_size;
        let frame = ratatui::layout::Rect::new(0, 0, cols, rows);

        // A release is not a routing decision — it belongs to whoever received the
        // press, and forwarding it is an obligation rather than a choice. A child
        // that gets a press with no matching release believes the button is still
        // held: claude's own handler starts a selection on press and fires the
        // actual click on RELEASE, so a press-only click leaves it drag-selecting
        // forever and never clicks anything.
        if let MouseEventKind::Up(button) = ev.kind {
            return self.forward_release(ev, button, &frame);
        }

        let lines = self.state.config.mouse.scroll_lines.max(1);
        // `delta` is only meaningful for a wheel gesture; buttons ignore it.
        let (gesture, delta): (Gesture, i32) = match ev.kind {
            MouseEventKind::ScrollUp => (Gesture::Wheel, -i32::try_from(lines).unwrap_or(i32::MAX)),
            MouseEventKind::ScrollDown => {
                (Gesture::Wheel, i32::try_from(lines).unwrap_or(i32::MAX))
            }
            MouseEventKind::Down(MouseButton::Left) => (Gesture::Left, 0),
            MouseEventKind::Down(MouseButton::Middle) => (Gesture::Middle, 0),
            MouseEventKind::Down(MouseButton::Right) => (Gesture::Right, 0),
            // Drags, motion, horizontal wheel: no behaviour. spyc asks the terminal
            // only for 1000 (press/release), so `Moved`/`Drag` shouldn't arrive at
            // all — `proc.rs` filters them for the terminals that send them anyway.
            // Consequence, deliberate: click-drag selection INSIDE a child doesn't
            // work. That needs 1002 (motion only while a button is held), which
            // unlike 1003 wouldn't cost the idle-redraw invariant — a later change.
            // Matched explicitly so adding one is a visible decision here.
            MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Moved
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => return Vec::new(),
        };

        // The renderer's own layout — including the vsplit carve, `^a z` zoom and
        // `pane_hidden`. Reassembling it here is what made zoomed clicks land on
        // an off-screen list and the right column hit-test as the left.
        let layout = self.frame_layout(frame);
        let region = region_at(&layout, ev.column, ev.row);

        let snap = MouseSnapshot {
            // Same precedence the keyboard uses — one owner for "which modal
            // wins", so mouse and keys can't disagree about it.
            modal: active_modal(ModalSnapshot {
                has_find_picker: self.runtime.find_picker.is_some(),
                has_capture: self.runtime.pending_capture.is_some(),
                overlay_awaiting_dismiss: self.view.overlay_awaiting_dismiss,
                has_quick_select: self.view.quick_select.is_some(),
                has_harpoon: self.view.harpoon_menu.is_some(),
            }),
            region,
            pager_mount: self.focused_top_pager_mount(),
            pane_wants_mouse: self
                .runtime
                .pane_tabs
                .as_ref()
                .is_some_and(|t| t.active().wants_mouse()),
        };

        match route_mouse(snap, gesture) {
            MouseSink::Swallow => Vec::new(),
            MouseSink::ListCursor => {
                // Reuse the cursor Actions rather than poking the cursor: they
                // already handle clamping, the vsplit's focused column, and
                // `list_generation` invalidation.
                let action = if delta < 0 {
                    crate::keymap::Action::Up(lines)
                } else {
                    crate::keymap::Action::Down(lines)
                };
                // A cursor move can't fail, but `apply` is fallible for other
                // actions; surface rather than swallow if that ever changes.
                self.apply(&action).unwrap_or_else(|e| {
                    self.state.flash_error(format!("mouse: {e:#}"));
                    Vec::new()
                })
            }
            MouseSink::Pager(_) => {
                self.scroll_active_pager_by(delta);
                Vec::new()
            }
            MouseSink::FocusRegion => {
                self.focus_region(region);
                Vec::new()
            }
            MouseSink::Paste => vec![Effect::PasteFromClipboard],
            MouseSink::LeaderMenu => {
                self.state.resolver.enter_leader();
                // Show the which-key popup NOW rather than after
                // `chord_hint_delay_ms`: that debounce exists so a keyboard user
                // mid-chord isn't startled by a popup, and a deliberate
                // right-click has no such problem. Due-in-the-past makes the
                // existing `settle_chord_hint` build it on this same iteration.
                self.view.chord_hint_due = Some(std::time::Instant::now());
                Vec::new()
            }
            MouseSink::PaneForward => {
                let report = wheel_report(ev, delta < 0, &layout);
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    let (mode, encoding) = tabs.active().mouse_protocol();
                    let bytes = crate::pane::input::encode_mouse(report, mode, encoding);
                    if !bytes.is_empty() {
                        // Best-effort, like every other pane write: a dead pty
                        // surfaces through the exit path, and flashing per wheel
                        // tick would bury the real message under repeats.
                        let _ = tabs.active_mut().send_bytes(&bytes);
                    }
                }
                Vec::new()
            }
        }
    }
}

impl super::App {
    /// Give the keyboard to `region`, reusing the same entry points `^a j`/`^a k`
    /// and `^a a`/`^a b` use.
    ///
    /// `set_pane_focus` is doing real work here, not just being tidy: it declines
    /// while `^a z`-zoomed (the target region is collapsed off-screen) and, coming
    /// back from the pane, restores whichever split column the user left from.
    /// Assigning `state.focus` directly would lose both.
    fn focus_region(&mut self, region: Option<Region>) {
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

/// Build the child-facing report for a wheel event, translating the pointer into
/// the pane's own coordinate space.
///
/// The translation is the part that's easy to skip and produces a *worse than
/// broken* result: frame-absolute coordinates make the child think the pointer is
/// `pane.y` rows above where it is, which reads as the child's bug.
fn wheel_report(
    ev: MouseEvent,
    up: bool,
    layout: &super::FrameLayout,
) -> crate::pane::input::MouseReport {
    use crossterm::event::KeyModifiers as K;
    let origin = layout
        .pane
        .unwrap_or(ratatui::layout::Rect::new(0, 0, 0, 0));
    crate::pane::input::MouseReport {
        button: if up { 64 } else { 65 },
        // A wheel tick has no release half; emitting one makes click-counting
        // apps see phantom input.
        release: false,
        col: ev.column.saturating_sub(origin.x),
        row: ev.row.saturating_sub(origin.y),
        shift: ev.modifiers.contains(K::SHIFT),
        alt: ev.modifiers.contains(K::ALT),
        ctrl: ev.modifiers.contains(K::CONTROL),
    }
}

#[cfg(test)]
mod tests {
    use super::{Gesture, MouseSink, MouseSnapshot, Region, region_at, route_mouse};
    use crate::app::App;
    use crate::config::StatusPosition;
    use crate::ui::pager::Mount;
    use ratatui::layout::Rect;

    /// Snapshot with nothing going on: no modal, no pager, child doesn't want
    /// mouse. Tests override the one field they're about.
    const fn snap(region: Option<Region>) -> MouseSnapshot {
        MouseSnapshot {
            modal: None,
            region,
            pager_mount: None,
            pane_wants_mouse: false,
        }
    }

    #[test]
    fn every_modal_swallows_the_event() {
        use crate::app::modal::Modal;
        // Whatever the pointer is over, a modal owns the screen. Listed
        // explicitly rather than looped so a new `Modal` variant is a visible
        // decision here, not a silent default.
        for modal in [
            Modal::FindPicker,
            Modal::Capture,
            Modal::OverlayDismiss,
            Modal::QuickSelect,
            Modal::Harpoon,
        ] {
            for region in [Region::List, Region::Pane, Region::RightColumn] {
                let s = MouseSnapshot {
                    modal: Some(modal),
                    ..snap(Some(region))
                };
                assert_eq!(
                    route_mouse(s, Gesture::Wheel),
                    MouseSink::Swallow,
                    "{modal:?} over {region:?} must swallow"
                );
            }
        }
    }

    #[test]
    fn pointer_over_the_pane_routes_to_the_pane_even_when_the_list_has_the_keyboard() {
        // The ergonomics win, and the reason routing hit-tests the pointer: the
        // snapshot carries no focus field at all, so there is nothing here that
        // *could* make keyboard focus override the pointer.
        let s = MouseSnapshot {
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::PaneForward);
    }

    #[test]
    fn pane_child_without_mouse_mode_swallows_never_forwards() {
        // The #170 class: forwarding to a child that never enabled mouse mode
        // types `\e[<64;20;5M` into its prompt. Silence is the correct v1 answer.
        let s = snap(Some(Region::Pane));
        assert!(!s.pane_wants_mouse);
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Swallow);
    }

    #[test]
    fn list_routes_to_the_cursor_or_to_a_pager_covering_it() {
        assert_eq!(
            route_mouse(snap(Some(Region::List)), Gesture::Wheel),
            MouseSink::ListCursor
        );
        assert_eq!(
            route_mouse(snap(Some(Region::RightColumn)), Gesture::Wheel),
            MouseSink::ListCursor
        );
        // A mounted pager is painted over the list, so it takes those events.
        for mount in [Mount::Overlay, Mount::TopPane] {
            let s = MouseSnapshot {
                pager_mount: Some(mount),
                ..snap(Some(Region::List))
            };
            assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Pager(mount));
        }
    }

    #[test]
    fn chrome_and_off_frame_swallow() {
        for region in [
            Region::Divider,
            Region::Prompt,
            Region::Status,
            Region::VDivider,
        ] {
            assert_eq!(
                route_mouse(snap(Some(region)), Gesture::Wheel),
                MouseSink::Swallow,
                "{region:?}"
            );
        }
        assert_eq!(route_mouse(snap(None), Gesture::Wheel), MouseSink::Swallow);
    }

    /// Hit-testing against the real `compute_layout`, for both status positions.
    /// `status_position = "bottom"` is the case `FrameLayout.top_unit`'s doc
    /// calls out as the off-by-one trap, so it's not optional coverage.
    #[test]
    fn hit_test_resolves_each_region_for_both_status_positions() {
        let area = Rect::new(0, 0, 80, 24);
        for pos in [StatusPosition::Top, StatusPosition::Bottom] {
            let layout = App::compute_layout(area, true, 40, pos);

            let pane = layout.pane.expect("pane_open = true");
            assert_eq!(
                region_at(&layout, pane.x, pane.y),
                Some(Region::Pane),
                "{pos:?}: pane origin"
            );
            let divider = layout.divider.expect("pane_open = true");
            assert_eq!(
                region_at(&layout, divider.x, divider.y),
                Some(Region::Divider),
                "{pos:?}: divider"
            );
            assert_eq!(
                region_at(&layout, layout.status.x, layout.status.y),
                Some(Region::Status),
                "{pos:?}: status row — the position that moves"
            );
            assert_eq!(
                region_at(&layout, layout.prompt.x, layout.prompt.y),
                Some(Region::Prompt),
                "{pos:?}: prompt"
            );
            assert_eq!(
                region_at(&layout, layout.list.x, layout.list.y),
                Some(Region::List),
                "{pos:?}: list origin"
            );
        }
    }

    #[test]
    fn hit_test_with_no_pane_has_no_pane_or_divider_region() {
        let layout = App::compute_layout(Rect::new(0, 0, 80, 24), false, 40, StatusPosition::Top);
        assert!(layout.pane.is_none() && layout.divider.is_none());
        // Every row of a pane-less frame resolves to something other than Pane.
        for row in 0..24 {
            assert_ne!(region_at(&layout, 0, row), Some(Region::Pane), "row {row}");
        }
    }

    /// Middle and right are spyc's from EVERY region, including over a
    /// mouse-aware child. Forwarding them would make the gesture unavailable in
    /// exactly the region where the pane holds focus — which is where a user
    /// would reach for it.
    #[test]
    fn middle_and_right_are_never_forwarded_even_to_a_mouse_aware_child() {
        for region in [Region::List, Region::Pane, Region::RightColumn] {
            let s = MouseSnapshot {
                pane_wants_mouse: true,
                ..snap(Some(region))
            };
            assert_eq!(
                route_mouse(s, Gesture::Middle),
                MouseSink::Paste,
                "{region:?}: middle-click pastes"
            );
            assert_eq!(
                route_mouse(s, Gesture::Right),
                MouseSink::LeaderMenu,
                "{region:?}: right-click opens the leader menu"
            );
        }
    }

    /// Left is click-through: over a mouse-aware child the event reaches the
    /// child; everywhere else it moves the keyboard.
    #[test]
    fn left_click_focuses_but_forwards_into_a_mouse_aware_pane() {
        let aware = MouseSnapshot {
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(aware, Gesture::Left), MouseSink::PaneForward);

        // A child that never asked for mouse still gets focused, not forwarded —
        // the #170 gate applies to buttons exactly as it does to the wheel.
        let unaware = snap(Some(Region::Pane));
        assert_eq!(route_mouse(unaware, Gesture::Left), MouseSink::FocusRegion);

        for region in [Region::List, Region::RightColumn] {
            assert_eq!(
                route_mouse(snap(Some(region)), Gesture::Left),
                MouseSink::FocusRegion,
                "{region:?}"
            );
        }
    }

    /// A modal still wins over every button, not just the wheel.
    #[test]
    fn a_modal_swallows_buttons_too() {
        use crate::app::modal::Modal;
        let s = MouseSnapshot {
            modal: Some(Modal::FindPicker),
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        for gesture in [
            Gesture::Left,
            Gesture::Middle,
            Gesture::Right,
            Gesture::Wheel,
        ] {
            assert_eq!(
                route_mouse(s, gesture),
                MouseSink::Swallow,
                "{gesture:?} must not bypass a modal"
            );
        }
    }

    /// Clicking chrome or off-frame moves nothing — but middle/right still act,
    /// since they aren't about the region.
    #[test]
    fn chrome_focuses_nothing_but_still_pastes_and_opens_the_menu() {
        for region in [Region::Divider, Region::Status, Region::Prompt] {
            assert_eq!(
                route_mouse(snap(Some(region)), Gesture::Left),
                MouseSink::FocusRegion,
                "{region:?}: routed, then `focus_region` no-ops on chrome"
            );
            assert_eq!(
                route_mouse(snap(Some(region)), Gesture::Middle),
                MouseSink::Paste
            );
        }
        // Off-frame is the one case where even middle/right do nothing: there is
        // no region, so the early return fires before the gesture match.
        assert_eq!(route_mouse(snap(None), Gesture::Middle), MouseSink::Swallow);
    }

    /// Coordinate translation, for both `status_position` values — the trap
    /// `FrameLayout.top_unit`'s doc calls out, since the pane's row origin moves
    /// with the status row. Frame row `R` inside a pane starting at row `Y` must
    /// reach the child as pane row `R - Y`; skipping this makes clicks land
    /// `pane.y` rows off, which reads as the child's bug rather than ours.
    #[test]
    fn wheel_report_translates_into_the_panes_coordinate_space() {
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

        for pos in [StatusPosition::Top, StatusPosition::Bottom] {
            let layout = App::compute_layout(Rect::new(0, 0, 80, 24), true, 40, pos);
            let pane = layout.pane.expect("pane_open = true");
            assert!(
                pane.y > 0,
                "{pos:?}: pane must be offset for this to prove anything"
            );

            // Two rows into the pane, five columns in.
            let ev = MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: pane.x + 5,
                row: pane.y + 2,
                modifiers: KeyModifiers::NONE,
            };
            let report = super::wheel_report(ev, true, &layout);
            assert_eq!(report.row, 2, "{pos:?}: row must be pane-relative");
            assert_eq!(report.col, 5, "{pos:?}: col must be pane-relative");
            assert_eq!(report.button, 64, "wheel up");
            assert!(!report.release, "a wheel tick has no release half");

            // The pane's own origin is the child's 0,0.
            let at_origin = MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: pane.x,
                row: pane.y,
                modifiers: KeyModifiers::NONE,
            };
            let report = super::wheel_report(at_origin, false, &layout);
            assert_eq!((report.col, report.row), (0, 0), "{pos:?}: pane origin");
            assert_eq!(report.button, 65, "wheel down");
        }
    }

    #[test]
    fn hit_test_returns_none_outside_the_frame() {
        let layout = App::compute_layout(Rect::new(0, 0, 80, 24), true, 40, StatusPosition::Top);
        assert_eq!(region_at(&layout, 200, 5), None, "past the right edge");
        assert_eq!(region_at(&layout, 5, 200), None, "past the bottom");
    }
}

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
//! The pure half ([`route_mouse`], [`region_at`]) decides; the `impl App` half
//! below acts on that decision.

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
    /// A prompt is open (`Mode::Prompting`) — the `:` line, search, a rename, or
    /// any single-key confirm.
    ///
    /// Not covered by `modal`: no prompt is a [`Modal`], they are all
    /// `Mode::Prompting`, which is exactly why omitting this was dangerous rather
    /// than merely untidy. See [`route_mouse`]'s prompt arm.
    pub is_prompting: bool,
    /// A `^a v` scrollback pager owns the pane's rect, hiding the live child
    /// behind it.
    pub has_scroll_pager: bool,
    /// The active tab's child has exited — there is nothing to forward to.
    pub pane_closed: bool,
}

impl MouseSnapshot {
    /// Whether an event over the pane may be handed to the child.
    ///
    /// Three conditions, each guarding a distinct way forwarding misbehaves:
    /// the child must have asked for mouse reporting (else the bytes land in its
    /// prompt as literal text — the #170 class), it must still be alive, and it
    /// must not be hidden behind a `^a v` scrollback pager, which owns the pane's
    /// rect and would otherwise have the wheel poke a child the user can't see.
    const fn can_forward_to_child(self) -> bool {
        self.pane_wants_mouse && !self.pane_closed && !self.has_scroll_pager
    }

    /// Whether a pending chord set now would ever be seen by the resolver.
    ///
    /// A leader chord is only safe to arm if the NEXT key reaches
    /// `Resolver::feed`. Two states break that, and in both the chord latches
    /// instead: a prompt (`handle_prompt_key` never feeds the resolver) and a
    /// focused **`Mount::Overlay`** pager, which `route_input` resolves to
    /// `PagerKey` for every key *including meta chords* — unlike `Mount::TopPane`,
    /// where meta deliberately escapes, so the leader works there.
    ///
    /// A latch is not merely cosmetic: the popup stays on screen, and the first
    /// key that eventually reaches the resolver is consumed as a continuation —
    /// in the leader menu `p` is a chdir and `P` overwrites PROJECT_HOME.
    const fn resolver_will_see_the_next_key(self) -> bool {
        !self.is_prompting && !matches!(self.pager_mount, Some(Mount::Overlay))
    }
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

    // A prompt owns the keyboard, so the only gesture that can mean anything is a
    // paste — which `handle_paste` already routes into the prompt buffer.
    //
    // Right-click especially must not act here. `enter_leader()` sets a pending
    // chord, but while prompting the next key goes to `handle_prompt_key`, which
    // never feeds the resolver and never calls `clear_chord_hint` — so `pending`
    // latches, the which-key popup sits on screen for the whole prompt, and the
    // first key after the prompt closes is eaten as a leader continuation. In that
    // menu `p` is a chdir and `P` overwrites PROJECT_HOME, so a stray right-click
    // could move the user's project root. The same latch is reachable with no
    // prompt at all, via a focused full-frame pager, which is why this is a
    // snapshot field rather than a check on `Mode` at one call site.
    if snap.is_prompting {
        return match gesture {
            Gesture::Middle => MouseSink::Paste,
            Gesture::Left | Gesture::Right | Gesture::Wheel => MouseSink::Swallow,
        };
    }

    // Middle and right are spyc's everywhere, never forwarded — otherwise the
    // gesture would be unavailable in exactly the region where the pane has
    // focus, which is where you'd reach for it. Documented alongside the
    // selection caveat, since `:mouse off` is the way to get the child's own
    // right-click menu back.
    match gesture {
        Gesture::Middle => return MouseSink::Paste,
        Gesture::Right => {
            return if snap.resolver_will_see_the_next_key() {
                MouseSink::LeaderMenu
            } else {
                // Arming a chord nothing will consume latches it — see
                // `resolver_will_see_the_next_key`.
                MouseSink::Swallow
            };
        }
        Gesture::Left => {
            // Left is click-THROUGH: focus the region, and (for a mouse-aware
            // child) let the event reach it too. The pane is live and visible, so
            // swallowing the first click just to focus would read as broken.
            return if matches!(region, Region::Pane) && snap.can_forward_to_child() {
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
            if snap.can_forward_to_child() {
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
    /// Ignores everything unless spyc actually asked the terminal for mouse
    /// reporting.
    ///
    /// Not a redundant assertion: spyc receiving a mouse event does NOT imply it
    /// requested one. A foreground child can leave its own reporting enabled when
    /// it dies without resetting, and some terminals/multiplexers report
    /// regardless of what was asked — the case `proc.rs`'s motion filter already
    /// guards against. Without this gate those unsolicited events were fully acted
    /// upon with the feature switched off: middle-click pasted the clipboard and
    /// right-click opened the leader menu.
    pub(super) fn handle_mouse(&mut self, ev: MouseEvent) -> Vec<Effect> {
        if !crate::mouse_capture_is_on() {
            return Vec::new();
        }
        let (cols, rows) = self.view.term_size;
        let frame = ratatui::layout::Rect::new(0, 0, cols, rows);

        // A release is not a routing decision — it belongs to whoever received the
        // press, and forwarding it is an obligation rather than a choice. A child
        // that gets a press with no matching release believes the button is still
        // held: claude's own handler starts a selection on press and fires the
        // actual click on RELEASE, so a press-only click leaves it drag-selecting
        // forever and never clicks anything.
        if let MouseEventKind::Up(button) = ev.kind {
            return self.forward_release(ev, button, frame);
        }

        // A drag belongs to whoever received the press, for the same reason a
        // release does: it is the middle of one gesture, not a new decision. So it
        // rides the same `mouse_press_forwarded` pairing — which also means a drag
        // that began on the file list never leaks into the child, and a drag that
        // began in the child keeps reaching it after the pointer leaves the pane
        // (children track their own selection across the whole drag).
        //
        // spyc-side selection for a NON-mouse child is deliberately not here: this
        // change only stops throwing drags away, so a child that speaks mouse gets
        // its own selection back (claude's `onSelectionDrag` was dead solely
        // because these events were dropped). See `docs/drafts/mouse_selection_plan.md`.
        if matches!(ev.kind, MouseEventKind::Drag(_)) {
            return self.forward_drag(ev, frame);
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
            // Same sources `route_input`'s snapshot uses (`route.rs`), so the
            // mouse and the keyboard can't disagree about what owns the screen.
            is_prompting: matches!(self.state.mode, super::Mode::Prompting(_)),
            has_scroll_pager: self.view.scroll_pager.is_some(),
            pane_closed: self.state.pane.pane_snapshot.is_closed,
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
            // Left-click into a mouse-aware child: focus the pane AND deliver the
            // click. Both, or it reads as broken — see `MouseSink::FocusAndForward`.
            MouseSink::FocusAndForward => {
                self.focus_region(region);
                self.forward_to_child(ev, &layout)
            }
            MouseSink::PaneForward => self.forward_to_child(ev, &layout),
        }
    }

    /// Encode `ev` in the child's own protocol and send it to the active pane.
    ///
    /// Records whether the press was forwarded, so [`Self::forward_release`] can
    /// pair the matching release to the same child.
    fn forward_to_child(&mut self, ev: MouseEvent, layout: &FrameLayout) -> Vec<Effect> {
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
            target: super::effect::PaneTarget::Active,
            input: super::effect::PaneInput::Bytes(bytes),
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
    fn forward_drag(&mut self, ev: MouseEvent, frame: ratatui::layout::Rect) -> Vec<Effect> {
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
    fn forward_release(
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
fn mouse_report(
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
            is_prompting: false,
            has_scroll_pager: false,
            pane_closed: false,
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

    /// Left is click-through: over a mouse-aware child it focuses the pane **and**
    /// the event reaches the child; everywhere else it just moves the keyboard.
    ///
    /// The sink carries both halves for a reason. The previous `PaneForward` did
    /// only the forwarding, so the keyboard stayed in the file list after clicking
    /// into the agent — and this test still passed, because a sink's name says
    /// nothing about what the handler does with it.
    #[test]
    fn left_click_focuses_but_forwards_into_a_mouse_aware_pane() {
        let aware = MouseSnapshot {
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(
            route_mouse(aware, Gesture::Left),
            MouseSink::FocusAndForward,
            "must do both, not just forward"
        );

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

    /// **A right-click during a prompt must not latch a leader chord.**
    ///
    /// No prompt is a `Modal` — they are all `Mode::Prompting` — so without
    /// `is_prompting` on the snapshot this fell through to `LeaderMenu`.
    /// `enter_leader()` sets a pending chord, but while prompting the next key
    /// goes to `handle_prompt_key`, which never feeds the resolver and never
    /// calls `clear_chord_hint`. So `pending` latched: the which-key popup sat on
    /// screen for the whole prompt, and the first key after it closed was eaten as
    /// a leader continuation — where `p` is a chdir and `P` overwrites
    /// PROJECT_HOME. A stray right-click could move the user's project root.
    #[test]
    fn a_prompt_swallows_every_gesture_except_paste() {
        for region in [Region::List, Region::Pane, Region::RightColumn] {
            let prompting = MouseSnapshot {
                is_prompting: true,
                // Even over a mouse-aware child, and even with a pager mounted.
                pane_wants_mouse: true,
                ..snap(Some(region))
            };
            for gesture in [Gesture::Right, Gesture::Left, Gesture::Wheel] {
                assert_eq!(
                    route_mouse(prompting, gesture),
                    MouseSink::Swallow,
                    "{gesture:?} over {region:?} must not act while a prompt is open"
                );
            }
            // Paste is the one gesture that means something mid-prompt:
            // `handle_paste` routes it into the prompt buffer, newlines stripped.
            assert_eq!(
                route_mouse(prompting, Gesture::Middle),
                MouseSink::Paste,
                "{region:?}: middle-click still pastes into the prompt"
            );
        }
    }

    /// The leader latch is reachable with **no prompt at all**: a focused
    /// `Mount::Overlay` pager (`:grep` results, `?` help, a git view, `:graveyard`)
    /// resolves every key to `PagerKey` — meta chords included — so a chord armed
    /// by a right-click sits pending until the pager closes, then eats the next key.
    ///
    /// `Mount::TopPane` is deliberately NOT gated: `route_input` lets meta escape
    /// to the resolver there, so the leader menu works as intended.
    #[test]
    fn right_click_over_a_modal_pager_does_not_arm_a_chord_nothing_will_consume() {
        let overlay = MouseSnapshot {
            pager_mount: Some(Mount::Overlay),
            ..snap(Some(Region::List))
        };
        assert_eq!(
            route_mouse(overlay, Gesture::Right),
            MouseSink::Swallow,
            "an Overlay pager swallows every key, so the chord would latch"
        );

        let top = MouseSnapshot {
            pager_mount: Some(Mount::TopPane),
            ..snap(Some(Region::List))
        };
        assert_eq!(
            route_mouse(top, Gesture::Right),
            MouseSink::LeaderMenu,
            "meta escapes a TopPane pager, so the leader is safe to arm"
        );

        // With no pager and no prompt it arms normally.
        assert_eq!(
            route_mouse(snap(Some(Region::List)), Gesture::Right),
            MouseSink::LeaderMenu
        );
    }

    /// A `^a v` scrollback owns the pane's rect, so the live child is hidden
    /// behind it — forwarding there sends input to something the user cannot see.
    #[test]
    fn an_open_scrollback_stops_forwarding_to_the_hidden_child() {
        let s = MouseSnapshot {
            pane_wants_mouse: true,
            has_scroll_pager: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Swallow);
        // Left-click still moves focus — it just doesn't reach the child.
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::FocusRegion);
    }

    /// An exited child has nothing to receive the event. `pane_wants_mouse` may
    /// still read true if vt100 retains the mode after the child dies, so the
    /// closed check has to be its own condition rather than relying on that.
    #[test]
    fn a_closed_pane_is_never_forwarded_to() {
        let s = MouseSnapshot {
            pane_wants_mouse: true,
            pane_closed: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Swallow);
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::FocusRegion);
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
    fn mouse_report_translates_into_the_panes_coordinate_space() {
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
            let report = super::mouse_report(ev, &layout).expect("wheel is forwardable");
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
            let report = super::mouse_report(at_origin, &layout).expect("wheel is forwardable");
            assert_eq!((report.col, report.row), (0, 0), "{pos:?}: pane origin");
            assert_eq!(report.button, 65, "wheel down");
        }
    }

    /// **Every gesture must encode as its own xterm button.**
    ///
    /// The bug this pins: the encoder used to be wheel-shaped —
    /// `wheel_report(ev, up, ..)` with `button: if up { 64 } else { 65 }` — so when
    /// buttons started routing through it they arrived with `up = false` and every
    /// click went out as **wheel-down**. Claude's decoder reads 65 as a wheel tick
    /// and its button decoder rejects anything with bit 64 set, so a left-click
    /// scrolled the agent and never clicked. The old test only ever passed
    /// `ScrollUp`/`ScrollDown`, so it sailed through.
    #[test]
    fn every_button_encodes_as_itself_not_as_a_wheel_tick() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let layout = App::compute_layout(Rect::new(0, 0, 80, 24), true, 40, StatusPosition::Top);
        let pane = layout.pane.expect("pane_open = true");
        let at = |kind| MouseEvent {
            kind,
            column: pane.x + 3,
            row: pane.y + 1,
            modifiers: KeyModifiers::NONE,
        };

        // xterm: 0/1/2 = left/middle/right; 64/65 = wheel up/down. A drag is the
        // held button with the motion flag — `encode_mouse` turns that into bit 32.
        for (kind, button, release, motion) in [
            (MouseEventKind::Down(MouseButton::Left), 0, false, false),
            (MouseEventKind::Down(MouseButton::Middle), 1, false, false),
            (MouseEventKind::Down(MouseButton::Right), 2, false, false),
            (MouseEventKind::Up(MouseButton::Left), 0, true, false),
            (MouseEventKind::Up(MouseButton::Middle), 1, true, false),
            (MouseEventKind::Up(MouseButton::Right), 2, true, false),
            (MouseEventKind::Drag(MouseButton::Left), 0, false, true),
            (MouseEventKind::Drag(MouseButton::Middle), 1, false, true),
            (MouseEventKind::Drag(MouseButton::Right), 2, false, true),
            (MouseEventKind::ScrollUp, 64, false, false),
            (MouseEventKind::ScrollDown, 65, false, false),
        ] {
            let r = super::mouse_report(at(kind), &layout).expect("forwardable");
            assert_eq!(r.button, button, "{kind:?} must encode button {button}");
            assert_eq!(r.release, release, "{kind:?} release flag");
            assert_eq!(r.motion, motion, "{kind:?} motion flag");
            // The wheel bit must not leak onto a real button, and vice versa.
            assert_eq!(
                r.button & 64 != 0,
                matches!(kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown),
                "{kind:?}: bit 64 marks a wheel tick and nothing else"
            );
        }

        // Buttonless motion and horizontal wheel are not forwarded at all.
        // `Moved` in particular: spyc asks for 1002, which reports motion only
        // while a button is held, so a `Moved` is motion nobody requested.
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            assert!(
                super::mouse_report(at(kind), &layout).is_none(),
                "{kind:?} must not be forwarded"
            );
        }
    }

    #[test]
    fn hit_test_returns_none_outside_the_frame() {
        let layout = App::compute_layout(Rect::new(0, 0, 80, 24), true, 40, StatusPosition::Top);
        assert_eq!(region_at(&layout, 200, 5), None, "past the right edge");
        assert_eq!(region_at(&layout, 5, 200), None, "past the bottom");
    }
}

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

use super::modal::{ModalSnapshot, active_modal};
use super::{Effect, FrameLayout};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

mod forward;
mod route;
mod scroll;
mod selection;
pub(super) mod tab_hit;

// Re-exported so the impure half and its callers keep referring to
// `mouse::route_mouse` / `mouse::Region` exactly as before this split.
pub(super) use route::{
    Gesture, MouseSink, MouseSnapshot, PendingViewIntent, region_at, route_mouse,
};

/// Resolve a raw mouse event kind to the gesture + wheel delta every downstream
/// consumer (`ListCursor`, `Pager`, `PaneScrollKeys`) shares. `None` for a kind
/// with no behaviour (drags, motion, horizontal wheel — see the caller).
///
/// `invert_scroll` (`[mouse] invert_scroll`) flips the sign here, in this ONE
/// place, so the file list, the pager, and an agent pane's synthesized scroll
/// keys always agree with each other under either setting — none of them
/// re-check the config themselves.
fn gesture_and_delta(
    kind: MouseEventKind,
    lines: usize,
    invert_scroll: bool,
) -> Option<(Gesture, i32)> {
    let magnitude = i32::try_from(lines).unwrap_or(i32::MAX);
    let (gesture, delta) = match kind {
        MouseEventKind::ScrollUp => (Gesture::Wheel, -magnitude),
        MouseEventKind::ScrollDown => (Gesture::Wheel, magnitude),
        MouseEventKind::Down(MouseButton::Left) => (Gesture::Left, 0),
        MouseEventKind::Down(MouseButton::Middle) => (Gesture::Middle, 0),
        MouseEventKind::Down(MouseButton::Right) => (Gesture::Right, 0),
        // spyc asks the terminal only for 1000 (press/release), so `Moved`/`Drag`
        // shouldn't arrive at all — `proc.rs` filters them for the terminals that
        // send them anyway. Consequence, deliberate: click-drag selection INSIDE a
        // child doesn't work. That needs 1002 (motion only while a button is
        // held), which unlike 1003 wouldn't cost the idle-redraw invariant — a
        // later change. Matched explicitly so adding one is a visible decision.
        MouseEventKind::Up(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Moved
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => return None,
    };
    Some((gesture, if invert_scroll { -delta } else { delta }))
}

/// The surface an in-flight mouse drag belongs to. See
/// [`super::ViewState::mouse_selection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseDragTarget {
    /// A pager's charwise text selection, in this slot.
    Pager(crate::app::pager_handler::PagerSlot),
    /// A file-list row selection, in this column.
    List(super::state::Side),
    /// A spyc-side text selection over the pty pane's visible grid.
    Pane,
    /// A charwise selection over a single-line chrome surface.
    Chrome,
}

/// A file-list row selection.
///
/// `anchor`/`focus` rather than a sorted range so a backwards drag keeps its
/// direction, and so extending never has to re-derive which end the user started
/// from. Row *indices*, not paths: the copy resolves paths at release time from the
/// live listing, so a selection can't outlive a refresh with stale paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListSelection {
    pub side: super::state::Side,
    pub anchor: usize,
    pub focus: usize,
    /// Copy absolute paths instead of bare names — the modifier held at press.
    pub full_path: bool,
}

impl ListSelection {
    /// Inclusive `(low, high)` row indices.
    pub const fn range(&self) -> (usize, usize) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

/// A charwise selection over a single-line chrome surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeSelection {
    /// Screen row of the chrome line — its identity, since each is one row.
    pub y: u16,
    pub anchor_col: u16,
    pub focus_col: u16,
}

impl ChromeSelection {
    /// Inclusive `(low, high)` columns, relative to the row's own left edge.
    pub const fn range(self) -> (u16, u16) {
        if self.anchor_col <= self.focus_col {
            (self.anchor_col, self.focus_col)
        } else {
            (self.focus_col, self.anchor_col)
        }
    }
}

/// A sustained same-direction wheel-scroll gesture over an agent's own view. See
/// [`super::ViewState::pane_scroll_streak`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneScrollStreak {
    /// The tab this streak belongs to — switching tabs starts a fresh streak
    /// rather than inheriting the old one's elapsed time.
    pub tab_index: usize,
    /// -1 up, +1 down.
    pub dir: i8,
    pub started_at: std::time::Instant,
    pub last_at: std::time::Instant,
    /// Ticks in this streak, this one included. Gates opening an agent's own
    /// view — see `route::OPEN_AFTER_UP_TICKS`.
    pub ticks: u32,
}

/// One chrome line as drawn, recorded by the renderer for a later mouse copy.
#[derive(Debug, Clone)]
pub struct ChromeRow {
    pub y: u16,
    pub x: u16,
    pub line: ratatui::text::Line<'static>,
}

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
        // A gesture belongs to whoever received its press. When that was a pager,
        // spyc's own selection owns the drag and the release — checked BEFORE the
        // forwarding paths below, which would otherwise deliver mouse reports into
        // the agent while the user is selecting text somewhere else entirely.
        if let Some(target) = self.view.mouse_selection {
            match ev.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    return match target {
                        MouseDragTarget::Pager(_) => self.extend_pager_selection(ev),
                        MouseDragTarget::List(side) => self.extend_row_selection(ev, side),
                        MouseDragTarget::Pane => self.extend_pane_selection(ev),
                        MouseDragTarget::Chrome => self.extend_chrome_selection(ev),
                    };
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    return match target {
                        MouseDragTarget::Pager(_) => self.finish_pager_selection(ev),
                        MouseDragTarget::List(_) => self.finish_row_selection(ev),
                        MouseDragTarget::Pane => self.finish_pane_selection(ev),
                        MouseDragTarget::Chrome => self.finish_chrome_selection(ev),
                    };
                }
                // Any other button mid-drag abandons the selection rather than
                // interleaving two gestures.
                MouseEventKind::Down(_) | MouseEventKind::Up(_) => {
                    self.view.mouse_selection = None;
                }
                _ => {}
            }
        }

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
        let invert_scroll = self.state.config.mouse.invert_scroll;
        // `delta` is only meaningful for a wheel gesture; buttons ignore it.
        let Some((gesture, delta)) = gesture_and_delta(ev.kind, lines, invert_scroll) else {
            // Drags, motion, horizontal wheel: no behaviour. spyc asks the terminal
            // only for 1000 (press/release), so `Moved`/`Drag` shouldn't arrive at
            // all — `proc.rs` filters them for the terminals that send them anyway.
            // Consequence, deliberate: click-drag selection INSIDE a child doesn't
            // work. That needs 1002 (motion only while a button is held), which
            // unlike 1003 wouldn't cost the idle-redraw invariant — a later change.
            return Vec::new();
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
                has_image_gallery: self.view.image_gallery.is_some(),
                has_image_view: self.view.image_view.is_some(),
            }),
            region,
            pager_mount: self.focused_top_pager_mount(),
            covering_pager: self.pager_slot_at(ev.column, ev.row),
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
            pane_scroll_keys: self.active_pane_wheel_scroll().is_some(),
            // Only resolved over the divider: the width walk allocates, and a
            // wheel tick over the pane has no use for it.
            tab_under_pointer: if matches!(region, Some(route::Region::Divider)) {
                let widths = self
                    .runtime
                    .pane_tabs
                    .as_ref()
                    .map(|t| tab_hit::tab_widths(t, t.active().is_scrolling()))
                    .unwrap_or_default();
                tab_hit::tab_at_point(layout.divider, &widths, ev.column, ev.row)
            } else {
                None
            },
            // The renderer records every selectable chrome row each frame, so
            // this is a lookup rather than a second hit-test — one source for
            // "what got drawn" and "what a click can select".
            over_chrome_row: self.chrome_col_at(ev).is_some(),
        };

        match route_mouse(snap, gesture) {
            MouseSink::Swallow => Vec::new(),
            MouseSink::ListCursor(side) => {
                // NOT `Action::Up/Down`: those move `cur()` (the focused column) and
                // wrap at the ends via `rem_euclid`. The wheel must move the column
                // under the POINTER, and must stop at the ends — wrapping from the
                // bottom of the list back to the top is what made scrolling feel
                // like it was flying out of control.
                self.state.cursor_scroll_side(side, delta as isize);
                Vec::new()
            }
            MouseSink::Pager(slot) => {
                self.scroll_pager_in_slot(slot, delta);
                Vec::new()
            }
            MouseSink::FocusRegion => {
                self.focus_region(region);
                Vec::new()
            }
            MouseSink::FocusAndSelectRows(side) => {
                self.focus_region(region);
                self.begin_row_selection(ev, side)
            }
            MouseSink::FocusAndSelectPane => {
                self.focus_region(region);
                self.begin_pane_selection(ev)
            }
            MouseSink::SelectChrome => self.begin_chrome_selection(ev),
            MouseSink::PaneTab(index) => {
                // Reuse the keyboard's own tab switch rather than calling
                // `tabs.switch_to`: it also stashes/restores the per-tab
                // scrollback pager, pulls focus into the pane, and handles the
                // `^a z` fullscreen-list case. A hand-rolled switch here would
                // start out subtly different and drift further.
                let n = u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX);
                match self.apply(&crate::keymap::Action::PaneTabByIndex(n)) {
                    Ok(effects) => effects,
                    Err(e) => {
                        self.state.flash_error(format!("tab switch: {e:#}"));
                        Vec::new()
                    }
                }
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
            MouseSink::PaneScrollKeys => self.send_scroll_keys(delta),

            MouseSink::FocusAndSelect(slot) => {
                // The pager's OWN region, not the one the layout says is under the
                // pointer. An Overlay covering the pane made `region` be `Pane`, so
                // clicking inside the pager focused the pty behind it — the reported
                // "clicking in the pager also sends a click through to the underlying
                // pane".
                self.focus_pager_slot(slot);
                self.begin_pager_selection(ev, slot)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default (uninverted): a downward tick is positive, an upward tick is
    /// negative — "scroll down = toward the end", matching the pager and an
    /// agent pane's synthesized scroll keys, which read this exact sign with no
    /// flip of their own.
    #[test]
    fn uninverted_keeps_the_documented_sign() {
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollDown, 3, false),
            Some((Gesture::Wheel, 3))
        );
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollUp, 3, false),
            Some((Gesture::Wheel, -3))
        );
    }

    /// `invert_scroll = true` flips both directions — the escape hatch for a
    /// terminal/OS combination that reads backwards from spyc's default.
    #[test]
    fn invert_scroll_flips_both_directions() {
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollDown, 3, true),
            Some((Gesture::Wheel, -3))
        );
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollUp, 3, true),
            Some((Gesture::Wheel, 3))
        );
    }

    /// Every consumer (`ListCursor`, `Pager`, `PaneScrollKeys` in `handle_mouse`)
    /// reads the SAME delta this function returns — the toggle lives here, once,
    /// specifically so those three can never disagree with each other. This test
    /// is the guard against a future edit re-introducing a per-consumer flip.
    #[test]
    fn the_sign_is_decided_once_for_every_consumer() {
        for invert in [false, true] {
            let (_, down) = gesture_and_delta(MouseEventKind::ScrollDown, 1, invert).unwrap();
            let (_, up) = gesture_and_delta(MouseEventKind::ScrollUp, 1, invert).unwrap();
            assert_eq!(
                down, -up,
                "invert_scroll={invert}: up and down must be exact opposites"
            );
        }
    }

    /// Buttons carry no delta and are unaffected by the toggle.
    #[test]
    fn button_presses_carry_no_delta_either_way() {
        for invert in [false, true] {
            assert_eq!(
                gesture_and_delta(MouseEventKind::Down(MouseButton::Left), 5, invert),
                Some((Gesture::Left, 0))
            );
        }
    }

    /// Drags, motion, and horizontal wheel have no behaviour — matches the
    /// `handle_mouse` caller's early-return arms.
    #[test]
    fn no_behaviour_kinds_return_none() {
        for kind in [
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
            MouseEventKind::Up(MouseButton::Left),
        ] {
            assert_eq!(gesture_and_delta(kind, 1, false), None, "{kind:?}");
        }
    }

    /// `lines` (from `[mouse] scroll_lines`) scales the magnitude, not just a
    /// fixed step — the config's own documented purpose.
    #[test]
    fn lines_scales_the_magnitude() {
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollDown, 7, false),
            Some((Gesture::Wheel, 7))
        );
    }
}

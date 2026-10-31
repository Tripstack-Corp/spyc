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

pub(super) mod chord_hit;
mod forward;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod pane_copy_tests;
mod route;
mod scroll;
mod selection;
pub(super) mod tab_hit;

// Re-exported so the impure half and its callers keep referring to
// `mouse::route_mouse` / `mouse::Region` exactly as before this split.
pub(super) use route::{
    Gesture, MenuAt, MouseSink, MouseSnapshot, PendingViewIntent, region_at, route_mouse,
};

/// Apply `[mouse] invert_scroll` to a raw event, at the boundary, before anything
/// branches on it.
///
/// The knob means "this machine reports the wheel the opposite way round from
/// what I mean", which is a property of the *input* — so it is corrected once,
/// here, and everything downstream reads a wheel that means what it says. That
/// includes the report forwarded to a mouse-aware child, which a per-consumer
/// flip could not reach: inverting the file list and the pagers but not the pane
/// left the user who set the knob with a half-inverted spyc, the two halves of
/// one window scrolling opposite ways.
///
/// Buttons and drags pass through untouched — there is no direction to invert.
const fn with_wheel_direction_corrected(mut ev: MouseEvent, invert: bool) -> MouseEvent {
    if invert {
        ev.kind = match ev.kind {
            MouseEventKind::ScrollUp => MouseEventKind::ScrollDown,
            MouseEventKind::ScrollDown => MouseEventKind::ScrollUp,
            other => other,
        };
    }
    ev
}

/// Resolve a mouse event kind to the gesture + wheel delta every downstream
/// consumer (`ListCursor`, `Pager`, `PaneScrollKeys`) shares. `None` for a kind
/// with no behaviour (drags, motion, horizontal wheel — see the caller).
///
/// Direction-agnostic: the kind reaching here has already been through
/// [`with_wheel_direction_corrected`].
fn gesture_and_delta(kind: MouseEventKind, lines: usize) -> Option<(Gesture, i32)> {
    let magnitude = i32::try_from(lines).unwrap_or(i32::MAX);
    let (gesture, delta) = match kind {
        MouseEventKind::ScrollUp => (Gesture::Wheel, -magnitude),
        MouseEventKind::ScrollDown => (Gesture::Wheel, magnitude),
        MouseEventKind::Down(MouseButton::Left) => (Gesture::Left, 0),
        MouseEventKind::Down(MouseButton::Middle) => (Gesture::Middle, 0),
        MouseEventKind::Down(MouseButton::Right) => (Gesture::Right, 0),
        // `Up` and `Drag` never reach here — `handle_mouse` routes both to the
        // child that took the press, before this. `Moved` is dropped in `proc.rs`
        // (1002 reports motion only while a button is held, so a buttonless
        // `Moved` is motion spyc never asked for). Horizontal wheel has no
        // behaviour. All matched explicitly so adding one is a visible decision.
        MouseEventKind::Up(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Moved
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => return None,
    };
    Some((gesture, delta))
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
    /// Set by a double-click: the whole row is selected, and the pointer no
    /// longer moves either end. Carried on the selection rather than resolved
    /// into `anchor_col`/`focus_col` at press time so [`Self::range`] stays the
    /// one authority on which columns are lit, and the paint cannot disagree
    /// with what the release copies.
    pub whole_row: bool,
}

impl ChromeSelection {
    /// Inclusive `(low, high)` columns, relative to the row's own left edge.
    pub const fn range(self) -> (u16, u16) {
        if self.whole_row {
            return (0, u16::MAX);
        }
        if self.anchor_col <= self.focus_col {
            (self.anchor_col, self.focus_col)
        } else {
            (self.focus_col, self.anchor_col)
        }
    }
}

/// How close together two presses on one chrome row have to be to read as a
/// double-click. The interval every desktop toolkit settles near; spyc cannot
/// read the host's own setting through a pty, so it picks the same number.
pub const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

/// Is a press on chrome row `y` at `now` the second half of a double-click?
///
/// `prev` is the previous chrome press — its row and when it happened. Only the
/// ROW has to match, not the column: a double-click drifts a cell or two, and the
/// gesture selects the whole row anyway, so requiring the same column would fail
/// exactly the presses it is meant to catch.
///
/// Pure, and takes `now` rather than reading the clock, so the boundary is
/// testable — see this module's tests.
pub fn is_double_click(
    prev: Option<(u16, std::time::Instant)>,
    y: u16,
    now: std::time::Instant,
) -> bool {
    prev.is_some_and(|(prev_y, at)| prev_y == y && now.duration_since(at) <= DOUBLE_CLICK)
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

/// The pane and geometry a held grid belongs to; a tab or layout change invalidates it.
pub(super) struct PaneSelectionSnapshot {
    pub tab_id: String,
    pub term_size: (u16, u16),
    pub area: ratatui::layout::Rect,
    pub screen: crate::pane::snapshot::ScreenSnapshot,
}

/// One chrome line as drawn, recorded by the renderer for a later mouse copy.
///
/// Carries the drawn `width` because chrome does NOT always span the frame: a
/// full-height vsplit clamps the status and prompt rects to the left column
/// while the right column spans the same rows. Without it the hit-test has no
/// right edge, and a row that stops at column 60 answers for column 90 —
/// which is another region's.
#[derive(Debug, Clone)]
pub struct ChromeRow {
    pub y: u16,
    pub x: u16,
    pub width: u16,
    pub line: ratatui::text::Line<'static>,
}

impl super::App {
    /// Whether the active tab's child asked for mouse reporting of its own.
    fn active_pane_wants_mouse(&self) -> bool {
        self.runtime
            .project()
            .pane_tabs
            .as_ref()
            .is_some_and(|t| t.active().wants_mouse())
    }

    /// Each tab's on-screen width along the divider, for hit-testing a click
    /// against [`tab_hit::tab_at_point`]. Empty with no pane open.
    fn divider_tab_widths(&self, bar_width: u16) -> Vec<u16> {
        self.runtime
            .project()
            .pane_tabs
            .as_ref()
            .map(|t| {
                tab_hit::tab_layout(t, t.active().is_scrolling(), bar_width)
                    .into_iter()
                    .map(|c| c.width)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The pane tab whose label the pointer is on. Only resolved over the
    /// divider: the width walk allocates, and a wheel tick over the pane has no
    /// use for it. Not while the prompt row has taken the divider, since there
    /// are no tabs on it then.
    fn tab_under_pointer(
        &self,
        region: Option<route::Region>,
        layout: &super::FrameLayout,
        ev: MouseEvent,
    ) -> Option<usize> {
        if !matches!(region, Some(route::Region::Divider))
            || self.displaced_prompt_row(layout) == layout.divider
        {
            return None;
        }
        let bar_width = layout.divider.map_or(0, |d| d.width);
        let widths = self.divider_tab_widths(bar_width);
        tab_hit::tab_at_point(layout.divider, &widths, ev.column, ev.row)
    }

    /// Whether the pointer is on the `⚠N` of unread stderr, asked of the row the
    /// renderer recorded, so only a drawn notice answers.
    fn stderr_notice_under(&self, ev: MouseEvent) -> bool {
        let Some(notice) = crate::ui::status::stderr_notice(self.state.stderr.unread()) else {
            return false;
        };
        self.view
            .chrome_rows
            .borrow()
            .iter()
            .find(|r| r.y == ev.row && (r.x..r.x.saturating_add(r.width)).contains(&ev.column))
            .is_some_and(|r| {
                crate::ui::status::notice_at(&r.line, usize::from(ev.column - r.x), &notice)
            })
    }

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
        // Before anything branches on it — see
        // [`with_wheel_direction_corrected`]. Every path below, including the one
        // that hands a report to the child, reads the corrected event.
        let ev = with_wheel_direction_corrected(
            ev,
            self.state.active_project().config.mouse.invert_scroll,
        );
        let (cols, rows) = self.view.term_size;
        let frame = ratatui::layout::Rect::new(0, 0, cols, rows);

        if let Some(effects) = self.handle_selection_gesture(ev) {
            return effects;
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
        // spyc-owned drags were handled above. Only gestures whose press was
        // forwarded may reach the child here.
        if matches!(ev.kind, MouseEventKind::Drag(_)) {
            return self.forward_drag(ev, frame);
        }

        let lines = self.state.active_project().config.mouse.scroll_lines.max(1);
        // `delta` is only meaningful for a wheel gesture; buttons ignore it.
        let Some((gesture, delta)) = gesture_and_delta(ev.kind, lines) else {
            // Motion or a horizontal wheel: no behaviour. Releases and drags are
            // already gone — both returned above, to whoever took the press.
            return Vec::new();
        };

        // The renderer's own layout — including the vsplit carve, `^a z` zoom and
        // `pane_hidden`. Reassembling it here is what made zoomed clicks land on
        // an off-screen list and the right column hit-test as the left.
        let layout = self.frame_layout(frame);
        let region = region_at(&layout, ev.column, ev.row);

        let snap = self.mouse_snapshot(ev, gesture, &layout, frame, region);

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
            MouseSink::StderrLog => self.open_stderr_log(),
            MouseSink::PaneTab(index) => self.switch_to_clicked_tab(index),
            MouseSink::Paste => vec![Effect::PasteFromClipboard],
            MouseSink::ChordRow(index) => self.fire_chord_row(index),
            MouseSink::DismissChord => {
                self.state.resolver.cancel();
                self.clear_chord_hint();
                Vec::new()
            }
            MouseSink::ContextMenu(at) => self.open_menu_at(at, region),
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
            MouseSink::PaneHistory => self.wheel_into_pane_history(delta),

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

    /// What [`route_mouse`] decides from, for `ev` over `region` of `layout`.
    fn mouse_snapshot(
        &self,
        ev: MouseEvent,
        gesture: Gesture,
        layout: &FrameLayout,
        frame: ratatui::layout::Rect,
        region: Option<route::Region>,
    ) -> MouseSnapshot {
        let (over_chord_hint, chord_row) = self.chord_menu_at(layout, frame, ev);
        MouseSnapshot {
            // Same precedence the keyboard uses — one owner for "which modal
            // wins", so mouse and keys can't disagree about it.
            modal: active_modal(ModalSnapshot {
                has_find_picker: self.runtime.find_picker.is_some(),
                has_capture: self.runtime.pending_capture.is_some(),
                overlay_awaiting_dismiss: self.view.project().overlay_awaiting_dismiss,
                has_quick_select: self.view.quick_select.is_some(),
                has_harpoon: self.view.harpoon_menu.is_some(),
                has_image_gallery: self.view.image_gallery.is_some(),
                has_image_view: self.view.image_view.is_some(),
            }),
            region,
            pager_mount: self.focused_top_pager_mount(),
            covering_pager: self.pager_slot_at(ev.column, ev.row),
            pane_wants_mouse: self.active_pane_wants_mouse(),
            // Same sources `route_input`'s snapshot uses (`route.rs`), so the
            // mouse and the keyboard can't disagree about what owns the screen.
            force_pane_selection: matches!(gesture, Gesture::Left)
                && ev
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL),
            is_prompting: matches!(self.state.mode, super::Mode::Prompting(_)),
            has_scroll_pager: self.view.project().scroll_pager.is_some(),
            pane_closed: self.state.active_project().pane.pane_snapshot.is_closed,
            pane_scroll_keys: self.active_pane_wheel_scroll().is_some(),
            tab_under_pointer: self.tab_under_pointer(region, layout, ev),
            over_stderr_notice: self.stderr_notice_under(ev),
            // The renderer records every selectable chrome row each frame, so
            // this is a lookup rather than a second hit-test — one source for
            // "what got drawn" and "what a click can select".
            over_chrome_row: self.chrome_col_at(ev).is_some(),
            chord_armed: self.state.resolver.is_pending() || self.menu_is_open(),
            menu_open: self.menu_is_open(),
            list_row: self.list_row_under_right_click(gesture, region, ev),
            over_chord_hint,
            chord_row,
        }
    }

    /// The file row a right-click is on, in a column listing a directory. Only
    /// resolved for a right-click, the one gesture that asks.
    fn list_row_under_right_click(
        &self,
        gesture: Gesture,
        region: Option<route::Region>,
        ev: MouseEvent,
    ) -> Option<(super::state::Side, usize)> {
        let side = match (gesture, region) {
            (Gesture::Right, Some(route::Region::List)) => super::state::Side::Left,
            (Gesture::Right, Some(route::Region::RightColumn)) => super::state::Side::Right,
            _ => return None,
        };
        if self.state.get_col(side)?.view != super::View::Dir {
            return None;
        }
        Some((side, self.list_row_at(side, ev)?))
    }

    /// A click on the label of the 0-based tab `index`: the tab it names
    /// becomes the active one.
    ///
    /// Reuse the keyboard's own tab switch rather than calling
    /// `tabs.switch_to`: it also stashes/restores the per-tab scrollback pager,
    /// pulls focus into the pane, and handles the `^a z` fullscreen-list case.
    /// A hand-rolled switch here would start out subtly different and drift
    /// further.
    fn switch_to_clicked_tab(&mut self, index: usize) -> Vec<Effect> {
        let n = u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX);
        match self.apply(&crate::keymap::Action::PaneTabByIndex(n)) {
            Ok(effects) => effects,
            Err(e) => {
                self.state.flash_error(format!("tab switch: {e:#}"));
                Vec::new()
            }
        }
    }

    /// A right-click on an object: the keyboard goes where a left click would
    /// send it, so the keys the rows show are the ones that work next, and the
    /// object's menu opens.
    fn open_menu_at(&mut self, at: MenuAt, region: Option<route::Region>) -> Vec<Effect> {
        match at {
            MenuAt::Row(side, row) => {
                self.focus_region(region);
                self.state.col_mut(side).cursor.index = row;
                self.open_entry_menu()
            }
            MenuAt::PaneBody => {
                self.set_pane_focus(true);
                self.open_tab_menu()
            }
            MenuAt::Tab(index) => {
                let mut effects = self.switch_to_clicked_tab(index);
                effects.extend(self.open_tab_menu());
                effects
            }
        }
    }

    /// Whether `ev` is on the open which-key popup, laid out exactly as the
    /// renderer lays it out, and the entry under the pointer there, if any.
    fn chord_menu_at(
        &self,
        layout: &FrameLayout,
        frame: ratatui::layout::Rect,
        ev: MouseEvent,
    ) -> (bool, Option<usize>) {
        let Some(menu) = self.view.chord_hint.as_ref().and_then(|hint| {
            chord_hit::chord_hint_layout(
                &hint.rows,
                frame,
                layout.divider.map(|r| r.y),
                layout.vdivider.map(|r| r.x),
            )
        }) else {
            return (false, None);
        };
        let on = menu
            .rect
            .contains(ratatui::layout::Position::new(ev.column, ev.row));
        (on, chord_hit::entry_at(&menu, ev.column, ev.row))
    }

    /// A click on entry `index` of the open which-key popup: what the row's key
    /// would do, through the key's own path. A range row (`1-9`) can't say
    /// which member, so it names its keys and leaves the menu open for them.
    pub(super) fn fire_chord_row(&mut self, index: usize) -> Vec<Effect> {
        use crate::keymap::ChordEntry;
        let Some(entry) = self
            .view
            .chord_hint
            .as_ref()
            .and_then(|hint| hint.entries.get(index))
            .cloned()
        else {
            return Vec::new();
        };
        match entry {
            ChordEntry::Act(_, action) => {
                let action = self.state.resolver.take_menu_action(action);
                self.clear_chord_hint();
                self.update(super::update::UiMsg::Action(action))
                    .unwrap_or_else(|e| {
                        self.state.flash_error(format!("{e:#}"));
                        Vec::new()
                    })
            }
            ChordEntry::Sub(_, _, seq) => {
                self.state.resolver.enter(seq);
                // Drawn now, not after the keyboard's hesitation delay: the
                // user is already reading a menu.
                self.clear_chord_hint();
                self.view.chord_hint_due = Some(std::time::Instant::now());
                Vec::new()
            }
            ChordEntry::Range(keys, _) => {
                self.state
                    .flash_info(format!("{keys}: type the one you want"));
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    /// The default (uninverted): a downward tick is positive, an upward tick is
    /// negative — "scroll down = toward the end", matching the pager and an
    /// agent pane's synthesized scroll keys, which read this exact sign with no
    /// flip of their own.
    #[test]
    fn uninverted_keeps_the_documented_sign() {
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollDown, 3),
            Some((Gesture::Wheel, 3))
        );
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollUp, 3),
            Some((Gesture::Wheel, -3))
        );
    }

    /// `invert_scroll = true` flips both directions — the escape hatch for a
    /// terminal/OS combination that reads backwards from spyc's default. Applied
    /// to the EVENT, at the boundary, so every consumer inherits it including the
    /// report forwarded to a mouse-aware child.
    #[test]
    fn invert_scroll_flips_both_directions() {
        let at = |kind| MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert_eq!(
            with_wheel_direction_corrected(at(MouseEventKind::ScrollDown), true).kind,
            MouseEventKind::ScrollUp
        );
        assert_eq!(
            with_wheel_direction_corrected(at(MouseEventKind::ScrollUp), true).kind,
            MouseEventKind::ScrollDown
        );
        // …and reaches the shared delta through the ordinary path.
        assert_eq!(
            gesture_and_delta(
                with_wheel_direction_corrected(at(MouseEventKind::ScrollDown), true).kind,
                3
            ),
            Some((Gesture::Wheel, -3))
        );
    }

    /// Every consumer (`ListCursor`, `Pager`, `PaneScrollKeys`, and the report
    /// `PaneForward` hands the child) reads one already-corrected event — the
    /// toggle is applied at the boundary, once, specifically so they can never
    /// disagree with each other. This test is the guard against a future edit
    /// re-introducing a per-consumer flip.
    #[test]
    fn the_sign_is_decided_once_for_every_consumer() {
        let (_, down) = gesture_and_delta(MouseEventKind::ScrollDown, 1).unwrap();
        let (_, up) = gesture_and_delta(MouseEventKind::ScrollUp, 1).unwrap();
        assert_eq!(down, -up, "up and down must be exact opposites");
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollUp, 1),
            gesture_and_delta(MouseEventKind::ScrollUp, 1),
            "and the function must not consult the config itself"
        );
    }

    /// Buttons and drags carry no direction, so the toggle leaves them alone.
    #[test]
    fn button_presses_carry_no_delta_either_way() {
        let at = |kind| MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
        ] {
            assert_eq!(
                with_wheel_direction_corrected(at(kind), true).kind,
                kind,
                "{kind:?} has no direction to invert"
            );
        }
        assert_eq!(
            gesture_and_delta(MouseEventKind::Down(MouseButton::Left), 5),
            Some((Gesture::Left, 0))
        );
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
            assert_eq!(gesture_and_delta(kind, 1), None, "{kind:?}");
        }
    }

    /// `lines` (from `[mouse] scroll_lines`) scales the magnitude, not just a
    /// fixed step — the config's own documented purpose.
    #[test]
    fn lines_scales_the_magnitude() {
        assert_eq!(
            gesture_and_delta(MouseEventKind::ScrollDown, 7),
            Some((Gesture::Wheel, 7))
        );
    }

    // ── the dispatch entry itself ──
    //
    // `handle_mouse` reassembles the frame, hit-tests the pointer, and turns the
    // routed sink into effects and focus moves. The `MouseSink` half is pinned by
    // `route.rs`'s own tests; what follows drives the whole entry, because the
    // gate, the focus side effect and the wheel's sign live only here.

    use std::time::{Duration, Instant};

    /// A synthetic report at `(col, row)` with no modifiers.
    pub(super) fn at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    /// An App with a known frame size, plus a point the layout itself says is
    /// inside the file list — computed rather than guessed, so a layout change
    /// moves the click instead of silently aiming it somewhere else.
    fn app_and_a_point_in_the_list(dir: &std::path::Path) -> (App, (u16, u16)) {
        let mut app = App::test_app(dir.to_path_buf());
        app.view.term_size = (120, 24);
        app.state.active_project_mut().config.mouse.scroll_lines = 1;
        let layout = app.frame_layout(ratatui::layout::Rect::new(0, 0, 120, 24));
        let point = (layout.list.x + 2, layout.list.y + 1);
        (app, point)
    }

    /// A pane tab that speaks the mouse, the way a real child says so: the DEC
    /// mode is written to `cat`'s stdin and comes back through the pty, so the
    /// pane's own engine sees the request exactly as it would from claude.
    /// (The `\n` matters — the line discipline echoes a bare ESC as `^[`, so
    /// only cat's own re-emit carries the real escape.)
    ///
    /// `?1002` — button-event tracking — because that is what a child wanting
    /// drags asks for; under `?1000` a drag encodes to nothing, correctly.
    pub(super) fn make_pane_speak_mouse(app: &mut App) {
        let tabs = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .expect("a pane tab");
        tabs.active_mut()
            .send_bytes(b"\x1b[?1002h\n")
            .expect("write to the pty");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            tabs.active_mut().drain_output();
            if tabs.active().wants_mouse() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the pane never reported mouse mode");
    }

    /// An inline-agent pane carrying real captured history. `cat` is the child
    /// so it spawns in a test; the tab's command reads `claude`, which is all
    /// `agent::detect` consults. The echoed lines overflow the grid, so rows
    /// genuinely leave the screen and land in the emulator's scrollback.
    fn inline_agent_pane_with_history(app: &mut App, dir: &std::path::Path) {
        let wake = app.make_pane_wake();
        let pane =
            crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.project().context_path, wake)
                .expect("spawn cat");
        let entry = crate::pane::tabs::TabEntry::new(
            pane,
            crate::pane::tabs::TabInfo::new("claude", dir.to_path_buf()),
        );
        app.runtime.project_mut().pane_tabs = Some(crate::pane::tabs::PaneTabs::new(entry));
        let tabs = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .expect("a pane tab");
        let mut feed = String::new();
        for i in 0..80 {
            use std::fmt::Write as _;
            let _ = writeln!(feed, "history line {i}");
        }
        tabs.active_mut()
            .send_bytes(feed.as_bytes())
            .expect("write to the pty");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            tabs.active_mut().drain_output();
            if tabs
                .active_mut()
                .with_screen_mut(crate::ui::scrollback::scrollback_len)
                > 0
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the pane never captured any scrollback");
    }

    /// **A wheel tick over an inline agent's pane has to move the history spyc
    /// captured.** claude in its default TUI mode takes no alternate screen and
    /// requests no mouse modes (both verified in a pty), and spyc has no
    /// verified scroll key for it — so `route_mouse`'s `Region::Pane` ladder
    /// fell through to `Swallow`: nothing moved, and nothing said why. That
    /// reached us as "scrolling doesn't work with claude". Inside a pane spyc IS
    /// the terminal, so its own scrollback is the surface the wheel is asking
    /// for — and unlike codex there IS one, because inline claude sets no
    /// DECSTBM region and its completed output reaches the main buffer.
    #[test]
    fn a_sustained_wheel_up_over_an_inline_agent_reaches_spycs_history() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = tmp.path().join("work");
            std::fs::create_dir(&dir).unwrap();
            let mut app = App::test_app(dir.clone());
            app.view.term_size = (120, 24);
            inline_agent_pane_with_history(&mut app, &dir);

            // The two conditions that make this the swallowed case, asserted so
            // the test keeps naming its own premise: if claude ever starts
            // speaking mouse inline, this must fail loudly here rather than pass
            // for a reason it isn't testing.
            assert!(
                !app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .expect("a pane tab")
                    .active()
                    .wants_mouse(),
                "inline claude requests no mouse modes"
            );
            assert!(
                app.active_pane_wheel_scroll().is_none(),
                "and spyc has no verified scroll key to synthesize for it"
            );

            crate::set_mouse_capture_for_test(true);
            let layout = app.frame_layout(ratatui::layout::Rect::new(0, 0, 120, 24));
            let pane_rect = layout.pane.expect("a pane is open");
            let point = (pane_rect.x + 2, pane_rect.y + 1);
            for _ in 0..super::route::OPEN_AFTER_UP_TICKS {
                app.handle_mouse(at(MouseEventKind::ScrollUp, point.0, point.1));
            }
            crate::set_mouse_capture_for_test(false);

            assert!(
                app.runtime.pane_scroll_settle.is_some()
                    || app.view.project().scroll_pager.is_some(),
                "a sustained wheel-up over an inline agent must reach for the pane's \
                 captured history; instead the tick vanished"
            );
        });
    }

    /// The two ways the same gesture must leave the pane alone: a DOWN tick,
    /// because the live screen already IS the bottom (opening there is what put
    /// the agent-view path into an open/close flicker loop), and
    /// `[mouse] pane_scroll_view = "off"`, the documented way back to the old
    /// silence for anyone who wants their wheel to do nothing here.
    #[test]
    fn a_down_tick_or_pane_scroll_view_off_leaves_the_pane_alone() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = tmp.path().join("work");
            std::fs::create_dir(&dir).unwrap();
            let mut app = App::test_app(dir.clone());
            app.view.term_size = (120, 24);
            inline_agent_pane_with_history(&mut app, &dir);
            crate::set_mouse_capture_for_test(true);
            let layout = app.frame_layout(ratatui::layout::Rect::new(0, 0, 120, 24));
            let pane_rect = layout.pane.expect("a pane is open");
            let point = (pane_rect.x + 2, pane_rect.y + 1);
            let reached_for_history = |a: &App| {
                a.runtime.pane_scroll_settle.is_some() || a.view.project().scroll_pager.is_some()
            };

            for _ in 0..(super::route::OPEN_AFTER_UP_TICKS * 2) {
                app.handle_mouse(at(MouseEventKind::ScrollDown, point.0, point.1));
            }
            assert!(
                !reached_for_history(&app),
                "scrolling DOWN past the live screen must not open history"
            );

            app.view.pane_scroll_streak = None;
            app.state.active_project_mut().config.mouse.pane_scroll_view =
                crate::config::PaneScrollView::Off;
            for _ in 0..(super::route::OPEN_AFTER_UP_TICKS * 2) {
                app.handle_mouse(at(MouseEventKind::ScrollUp, point.0, point.1));
            }
            crate::set_mouse_capture_for_test(false);
            assert!(
                !reached_for_history(&app),
                "pane_scroll_view = \"off\" must keep the wheel silent over the pane"
            );
        });
    }

    /// **The capture gate.** A terminal or multiplexer can report the mouse when
    /// spyc never asked — a foreground child that died without resetting, for
    /// one. Acting on those with the feature off is how a stray middle click
    /// pasted the clipboard and a right click opened the leader menu.
    #[test]
    fn a_middle_click_pastes_only_while_capture_is_on() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let (mut app, (col, row)) = app_and_a_point_in_the_list(tmp.path());
            let click = at(MouseEventKind::Down(MouseButton::Middle), col, row);

            crate::set_mouse_capture_for_test(false);
            assert!(
                app.handle_mouse(click).is_empty(),
                "a report spyc never asked for must do nothing"
            );

            crate::set_mouse_capture_for_test(true);
            assert!(
                matches!(
                    app.handle_mouse(click).as_slice(),
                    [Effect::PasteFromClipboard]
                ),
                "with capture on the same click pastes"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// The wheel moves the column under the POINTER and **stops** at the ends.
    /// Not `Action::Up`/`Down`: those move the focused column and wrap through
    /// `rem_euclid`, which is what made scrolling feel like it was flying out of
    /// control.
    #[test]
    fn the_wheel_moves_the_pointed_list_and_never_wraps_past_the_end() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let (mut app, (col, row)) = app_and_a_point_in_the_list(tmp.path());
            app.seed_rows(&["a", "b", "c"]);
            crate::set_mouse_capture_for_test(true);

            let down = at(MouseEventKind::ScrollDown, col, row);
            let up = at(MouseEventKind::ScrollUp, col, row);
            assert!(
                app.handle_mouse(down).is_empty(),
                "the wheel emits no effect"
            );
            assert_eq!(
                app.state.active_project().left.cursor.index,
                1,
                "down moves toward the end"
            );

            for _ in 0..10 {
                app.handle_mouse(down);
            }
            assert_eq!(
                app.state.active_project().left.cursor.index,
                2,
                "the wheel stops on the last row rather than wrapping to the top"
            );
            for _ in 0..10 {
                app.handle_mouse(up);
            }
            assert_eq!(
                app.state.active_project().left.cursor.index,
                0,
                "and stops on the first row going back"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// **The pointer decides, not the keyboard.** The whole hit-test is built on
    /// this: with the keyboard in the right column, a wheel tick over the LEFT one
    /// scrolls the left. Reading focus instead would scroll whichever column the
    /// user last clicked in, which is the one thing a wheel is never about.
    #[test]
    fn the_wheel_scrolls_the_column_under_the_pointer_not_the_focused_one() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let right_dir = tmp.path().join("other");
            std::fs::create_dir_all(&right_dir).unwrap();
            let (mut app, _) = app_and_a_point_in_the_list(tmp.path());
            app.seed_rows(&["a", "b", "c"]);
            app.open_second_commander_at(&right_dir);
            assert_eq!(
                app.state.active_project().vsplit.map(|v| v.focus),
                Some(super::super::state::Side::Right),
                "the keyboard is in the right column"
            );
            crate::set_mouse_capture_for_test(true);

            let layout = app.frame_layout(ratatui::layout::Rect::new(0, 0, 120, 24));
            let left = (layout.list.x + 1, layout.list.y + 1);
            let before_right = app
                .state
                .active_project()
                .right
                .as_ref()
                .expect("split")
                .cursor
                .index;

            app.handle_mouse(at(MouseEventKind::ScrollDown, left.0, left.1));

            assert_eq!(
                app.state.active_project().left.cursor.index,
                1,
                "the pointed (left) column moved"
            );
            assert_eq!(
                app.state
                    .active_project()
                    .right
                    .as_ref()
                    .expect("split")
                    .cursor
                    .index,
                before_right,
                "the focused (right) column did not"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// Right-click opens the which-key popup **now**. The `chord_hint_delay_ms`
    /// debounce exists so a keyboard user mid-chord isn't startled by a popup; a
    /// deliberate right click has no such problem, so the due instant is set in
    /// the past for `settle_chord_hint` to build on this same iteration.
    #[test]
    fn a_right_click_enters_the_leader_and_shows_the_hint_without_the_delay() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let (mut app, (col, row)) = app_and_a_point_in_the_list(tmp.path());
            crate::set_mouse_capture_for_test(true);
            assert!(app.view.chord_hint_due.is_none());

            let fx = app.handle_mouse(at(MouseEventKind::Down(MouseButton::Right), col, row));

            assert!(fx.is_empty(), "the menu is state, not an effect");
            assert_eq!(
                app.state.resolver.pending_display().as_deref(),
                Some("leader-"),
                "the resolver is mid-chord on the leader"
            );
            let due = app.view.chord_hint_due.expect("a hint is due");
            assert!(
                due <= Instant::now(),
                "the popup must be due already, not after the keyboard debounce"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    // ── an open chord menu owns the pointer ──
    //
    // The which-key popup is painted over the layout, so a click on it must not
    // reach what lies beneath (B1), and a click anywhere else must close it
    // rather than act while the chord stays armed (B2): an armed leader takes
    // the next key, where `p` changes directory and `x` closes the project.

    pub(super) const FRAME: ratatui::layout::Rect = ratatui::layout::Rect::new(0, 0, 120, 24);

    /// An App over a directory of `n` files, so a cursor move is observable.
    pub(super) fn app_over_files(tmp: &std::path::Path, n: usize) -> App {
        app_over_files_in(&tmp.join("work"), n)
    }

    fn app_over_files_in(dir: &std::path::Path, n: usize) -> App {
        let dir = dir.to_path_buf();
        std::fs::create_dir(&dir).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(format!("file{i:02}.txt")), "x").unwrap();
        }
        let mut app = App::test_app(dir);
        app.view.term_size = (FRAME.width, FRAME.height);
        app.state.refresh_listing();
        app
    }

    /// One frame, as the terminal would draw it.
    pub(super) fn drawn(app: &mut App) -> ratatui::buffer::Buffer {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(
            FRAME.width,
            FRAME.height,
        ))
        .unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// The menu titled `title` in `buf`, found by its titled border, so the
    /// tests aim at what the user sees rather than at the geometry under test.
    pub(super) fn menu_in(buf: &ratatui::buffer::Buffer, title: &str) -> ratatui::layout::Rect {
        let sym = |x: u16, y: u16| buf[(x, y)].symbol().to_string();
        let heading = format!(" {title} ");
        for y in 0..FRAME.height {
            for x in 0..FRAME.width {
                let follows: String = (x + 1..FRAME.width.min(x + 1 + 16))
                    .map(|c| sym(c, y))
                    .collect();
                if sym(x, y) != "┌" || !follows.starts_with(&heading) {
                    continue;
                }
                let right = (x + 1..FRAME.width)
                    .find(|&c| sym(c, y) == "┐")
                    .expect("the menu's top-right corner");
                let bottom = (y + 1..FRAME.height)
                    .find(|&r| sym(x, r) == "└")
                    .expect("the menu's bottom-left corner");
                return ratatui::layout::Rect::new(x, y, right - x + 1, bottom - y + 1);
            }
        }
        panic!("no menu titled {title:?} is drawn");
    }

    /// The open menu's box as the frame draws it.
    pub(super) fn drawn_menu(app: &mut App, title: &str) -> ratatui::layout::Rect {
        menu_in(&drawn(app), title)
    }

    /// The cell where `text` is drawn inside the menu titled `title`: where a
    /// click on that row aims.
    pub(super) fn drawn_row(app: &mut App, title: &str, text: &str) -> (u16, u16) {
        let buf = drawn(app);
        let menu = menu_in(&buf, title);
        for y in menu.y + 1..menu.bottom() - 1 {
            let cells: Vec<String> = (menu.x..menu.right())
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            let mut at = 0;
            let line = cells.concat();
            let Some(byte) = line.find(text) else {
                continue;
            };
            for (i, cell) in cells.iter().enumerate() {
                if at == byte {
                    return (menu.x + u16::try_from(i).unwrap(), y);
                }
                at += cell.len();
            }
        }
        panic!("no row {text:?} in the menu titled {title:?}");
    }

    pub(super) fn press(app: &mut App, c: char) {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ))
        .unwrap();
    }

    /// Let the loop build the popup for whatever chord is armed, long after any
    /// hint delay.
    fn settle_menu(app: &mut App) {
        app.settle_chord_hint(
            Instant::now() + Duration::from_secs(60),
            &mut crate::app::RunCtx::for_test(),
        );
    }

    fn prompt_shown(app: &App) -> Option<String> {
        match &app.state.mode {
            crate::app::Mode::Prompting(p) => Some(p.prefix.clone()),
            crate::app::Mode::Normal => None,
        }
    }

    /// Right-click at `point`, then let the loop build the popup, as one
    /// iteration of the real loop does; returns the box as drawn.
    fn open_the_leader_by_right_click(
        app: &mut App,
        (col, row): (u16, u16),
    ) -> ratatui::layout::Rect {
        app.handle_mouse(at(MouseEventKind::Down(MouseButton::Right), col, row));
        app.handle_mouse(at(MouseEventKind::Up(MouseButton::Right), col, row));
        app.settle_chord_hint(Instant::now(), &mut crate::app::RunCtx::for_test());
        assert!(
            app.view.chord_hint.is_some(),
            "premise: the leader menu is open"
        );
        drawn_menu(app, "leader")
    }

    pub(super) fn click(app: &mut App, (col, row): (u16, u16)) -> Vec<Effect> {
        let mut fx = app.handle_mouse(at(MouseEventKind::Down(MouseButton::Left), col, row));
        fx.extend(app.handle_mouse(at(MouseEventKind::Up(MouseButton::Left), col, row)));
        fx
    }

    pub(super) fn centre(r: ratatui::layout::Rect) -> (u16, u16) {
        (r.x + r.width / 2, r.y + r.height / 2)
    }

    pub(super) fn inside(r: ratatui::layout::Rect, (col, row): (u16, u16)) -> bool {
        r.contains(ratatui::layout::Position::new(col, row))
    }

    /// The owner's report: right-click opens the leader, and a left click
    /// anywhere else must close it. The click does nothing else — every menu
    /// dismisses rather than acts on a click outside it.
    #[test]
    fn a_left_click_outside_the_open_leader_menu_closes_it_and_does_nothing_else() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = app_over_files(tmp.path(), 12);
            crate::set_mouse_capture_for_test(true);
            let layout = app.frame_layout(FRAME);
            let menu = open_the_leader_by_right_click(&mut app, (layout.list.x + 1, layout.list.y));
            let elsewhere = (layout.list.x + 1, layout.list.y + 3);
            assert!(
                !inside(menu, elsewhere),
                "premise: the click lands beside the menu"
            );
            let cursor = app.state.cur().cursor.index;

            let fx = click(&mut app, elsewhere);

            assert!(
                fx.is_empty(),
                "closing the menu is state, not an effect: {fx:?}"
            );
            assert!(
                !app.state.resolver.is_pending(),
                "the leader is disarmed, so the next key is an ordinary key"
            );
            assert!(app.view.chord_hint.is_none(), "the menu is closed");
            assert_eq!(
                app.state.cur().cursor.index,
                cursor,
                "the click only closed the menu; the cursor stays"
            );
            assert!(
                app.view.mouse_selection.is_none(),
                "and no row selection began"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// B2's repro from the clickable-menus draft: arm `g`, wait for its menu,
    /// click a row. The chord must not survive the click, or the next `g`
    /// completes `gg` and jumps to the top.
    #[test]
    fn a_click_while_a_keyboard_chord_is_armed_cancels_the_chord() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = app_over_files(tmp.path(), 12);
            crate::set_mouse_capture_for_test(true);
            app.state.cur_mut().cursor.index = 5;
            let g = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('g'),
                crossterm::event::KeyModifiers::NONE,
            );
            app.handle_key(g).unwrap();
            app.settle_chord_hint(
                Instant::now() + Duration::from_secs(60),
                &mut crate::app::RunCtx::for_test(),
            );
            let menu = drawn_menu(&mut app, "g");
            let layout = app.frame_layout(FRAME);
            let row = (layout.list.x + 1, layout.list.y + 2);
            assert!(
                !inside(menu, row),
                "premise: the click lands beside the menu"
            );

            click(&mut app, row);

            assert!(!app.state.resolver.is_pending(), "the click disarmed `g`");
            assert!(app.view.chord_hint.is_none(), "and closed its menu");
            assert_eq!(app.state.cur().cursor.index, 5, "and moved nothing");
            app.handle_key(g).unwrap();
            assert_eq!(
                app.state.resolver.pending_display().as_deref(),
                Some("g-"),
                "the next `g` arms a fresh chord instead of completing `gg`"
            );
            assert_eq!(
                app.state.cur().cursor.index,
                5,
                "so nothing jumped to the top"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// B1: the popup is painted over the list, so a click or a wheel tick on it
    /// must not select or scroll the rows beneath. The menu stays open; its rows
    /// are not clickable yet.
    #[test]
    fn a_click_on_the_open_menu_reaches_no_list_row_beneath_it() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = app_over_files(tmp.path(), 40);
            crate::set_mouse_capture_for_test(true);
            let layout = app.frame_layout(FRAME);
            let menu = open_the_leader_by_right_click(&mut app, (layout.list.x + 1, layout.list.y));
            let on_menu = centre(menu);
            assert_eq!(
                region_at(&layout, on_menu.0, on_menu.1),
                Some(route::Region::List),
                "premise: the list lies beneath the menu"
            );
            let cursor = app.state.cur().cursor.index;

            click(&mut app, on_menu);
            app.handle_mouse(at(MouseEventKind::ScrollDown, on_menu.0, on_menu.1));

            assert_eq!(
                app.state.cur().cursor.index,
                cursor,
                "neither the click nor the wheel reached the list"
            );
            assert!(app.view.mouse_selection.is_none(), "no row selection began");
            assert!(
                app.state.resolver.is_pending() && app.view.chord_hint.is_some(),
                "a click on the menu leaves it open"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// B1 where it costs most: the popup over a mouse-aware agent. A click on
    /// the menu must not be forwarded into the child under it.
    #[test]
    fn a_click_on_the_open_menu_is_never_forwarded_to_the_pane_beneath() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = tmp.path().join("work");
            std::fs::create_dir(&dir).unwrap();
            let mut app = App::test_app(dir);
            app.view.term_size = (FRAME.width, FRAME.height);
            app.open_pane_tab("cat");
            make_pane_speak_mouse(&mut app);
            crate::set_mouse_capture_for_test(true);
            let layout = app.frame_layout(FRAME);
            let pane = layout.pane.expect("a pane is open");
            let menu = open_the_leader_by_right_click(&mut app, (layout.list.x + 1, layout.list.y));
            let overlap = menu.intersection(pane);
            assert!(
                !overlap.is_empty(),
                "premise: the menu is drawn over the pane"
            );
            let (col, row) = centre(overlap);

            // The press alone: its release would discharge the forwarded flag.
            let fx = app.handle_mouse(at(MouseEventKind::Down(MouseButton::Left), col, row));

            assert!(
                !app.view.mouse_press_forwarded && fx.is_empty(),
                "the press must not reach the child beneath the menu: {fx:?}"
            );
            app.handle_mouse(at(MouseEventKind::Up(MouseButton::Left), col, row));
            assert!(
                app.state.resolver.is_pending() && app.view.chord_hint.is_some(),
                "a click on the menu leaves it open"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    // ── a menu row fires by click ──
    //
    // A row's click does what the row's key does, through the same path, so the
    // menu teaches the key it shows rather than offering a second behaviour.

    /// A click on an action row fires that action, as its key would, and the
    /// menu closes as the key would close it.
    #[test]
    fn a_click_on_a_menu_row_fires_its_action() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = app_over_files(tmp.path(), 12);
            crate::set_mouse_capture_for_test(true);
            app.state.cur_mut().cursor.index = 5;
            press(&mut app, 'g');
            settle_menu(&mut app);
            let row = drawn_row(&mut app, "g", "top of column");

            click(&mut app, row);

            assert_eq!(
                app.state.cur().cursor.index,
                0,
                "`gg` fired: the cursor is at the top"
            );
            assert!(
                !app.state.resolver.is_pending() && app.view.chord_hint.is_none(),
                "and the menu closed"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// A click on a submenu row walks into the submenu, as its key does, and the
    /// submenu is drawn at once: the user is already reading a menu.
    #[test]
    fn a_click_on_a_submenu_row_opens_the_submenu() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = app_over_files(tmp.path(), 12);
            crate::set_mouse_capture_for_test(true);
            let layout = app.frame_layout(FRAME);
            open_the_leader_by_right_click(&mut app, (layout.list.x + 1, layout.list.y));
            let row = drawn_row(&mut app, "leader", "worktree");

            click(&mut app, row);
            app.settle_chord_hint(Instant::now(), &mut crate::app::RunCtx::for_test());

            assert_eq!(
                app.state.resolver.pending_display().as_deref(),
                Some("W-"),
                "the click armed the worktree chord, as `Space w` does"
            );
            drawn_menu(&mut app, "W");
            crate::set_mouse_capture_for_test(false);
        });
    }

    /// A count typed before the chord reaches the action its row fires, as on the
    /// key path: `3`, `d`, then a click on `dd`'s row asks what `3dd` asks.
    #[test]
    fn a_count_typed_before_the_chord_reaches_the_row_a_click_fires() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut by_keys = app_over_files_in(&tmp.path().join("keys"), 12);
            for c in ['3', 'd', 'd'] {
                press(&mut by_keys, c);
            }
            let asked = prompt_shown(&by_keys);
            assert!(asked.is_some(), "premise: `3dd` asks before removing");

            let mut by_click = app_over_files_in(&tmp.path().join("click"), 12);
            crate::set_mouse_capture_for_test(true);
            press(&mut by_click, '3');
            press(&mut by_click, 'd');
            settle_menu(&mut by_click);
            let row = drawn_row(&mut by_click, "3d", "remove (confirm)");
            click(&mut by_click, row);
            crate::set_mouse_capture_for_test(false);

            assert_eq!(
                prompt_shown(&by_click),
                asked,
                "the click asks to remove what `3dd` asks to remove"
            );
        });
    }

    /// A range row stands for keys a click cannot choose between, so a click on
    /// it fires nothing and names the keys; the menu stays open for them.
    #[test]
    fn a_click_on_a_range_row_fires_nothing_and_names_its_keys() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = app_over_files(tmp.path(), 12);
            crate::set_mouse_capture_for_test(true);
            let layout = app.frame_layout(FRAME);
            open_the_leader_by_right_click(&mut app, (layout.list.x + 1, layout.list.y));
            let row = drawn_row(&mut app, "leader", "1-9");
            let project = app.state.active;

            click(&mut app, row);

            assert_eq!(app.state.active, project, "no project switch fired");
            assert!(
                app.state.resolver.is_pending() && app.view.chord_hint.is_some(),
                "the menu stays open for the number"
            );
            let flash = app
                .state
                .flash
                .as_ref()
                .map(|f| f.text.clone())
                .unwrap_or_default();
            assert!(
                flash.contains("1-9"),
                "the flash names the keys to press: {flash:?}"
            );
            crate::set_mouse_capture_for_test(false);
        });
    }
}

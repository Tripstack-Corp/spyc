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
use super::pager_handler::PagerSlot;
use super::{Effect, FrameLayout, MouseDragTarget};
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
    /// Move a file-list cursor. Carries the SIDE the pointer is over, because in a
    /// vsplit the wheel must scroll the column under the pointer without dragging
    /// keyboard focus to it — `cur()` would move the focused column instead.
    ///
    /// Not optional, and easy to mistake for one: wheel-over-list works today
    /// only because DEC 1007 has the *terminal* translate wheel into arrow keys.
    /// Enabling 1000 stops that translation, so without this sink turning capture
    /// on would trade a pane bug for a list bug.
    ListCursor(crate::app::state::Side),
    /// Scroll the pager under the pointer. Carries the SLOT rather than the mount:
    /// a mount can't tell `view.pager` from `view.right_pager`, and the wheel must
    /// scroll the pager the pointer is over, not the focused one.
    Pager(PagerSlot),
    /// Encode and forward to the pane's child (which requested mouse reporting).
    PaneForward,
    /// Send the child's own scroll keys, for an agent that doesn't speak mouse
    /// but does scroll on a keypress ([`crate::agent::AgentProfile::wheel_scroll`]).
    ///
    /// Distinct from [`Self::PaneForward`] because the wheel is being *translated*
    /// rather than delivered: only an agent with a hand-verified binding gets this,
    /// so the translation can't degenerate into the wheel-to-arrows history
    /// cycling that `[mouse] capture` exists to avoid.
    PaneScrollKeys,
    /// Give the keyboard to the pane AND forward the event to its child — the
    /// left-click-through contract. Both halves, in one variant, because a sink
    /// that only forwarded was how the focus half came to be silently missing
    /// while a test asserting the sink still passed.
    FocusAndForward,
    /// Give the keyboard to the region under the pointer.
    FocusRegion,
    /// Give the keyboard to the pager under the pointer AND anchor a text
    /// selection there.
    ///
    /// Both halves in one variant for the same reason [`Self::FocusAndForward`]
    /// carries both: #219 shipped a sink that did only the forwarding half while a
    /// test asserting the sink still passed. A press that anchored without focusing
    /// would leave the pager's own keys (`y`, Esc) going somewhere else.
    FocusAndSelect(PagerSlot),
    /// Give the keyboard to a file-list column AND anchor a row selection there.
    /// Both halves in one variant, for the reason [`Self::FocusAndForward`] documents.
    FocusAndSelectRows(crate::app::state::Side),
    /// Anchor a charwise selection on the single-line chrome surface under the
    /// pointer (status bar, pane divider/tab line).
    ///
    /// Replaces an earlier click-copies-the-whole-line: a bare click that silently
    /// took the entire line was surprising, and it couldn't copy just the one thing
    /// you wanted out of it — a session id, a branch name.
    SelectChrome,
    /// Give the keyboard to the pane AND anchor a spyc-side text selection over its
    /// visible grid — for a child that ignores mouse reports, so nothing else can
    /// do the selecting (codex's `^T` transcript, a plain shell).
    FocusAndSelectPane,
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
    /// A pager mounted in the focused column, and where. Still needed for the
    /// leader-chord latch check, which is about the *modal* Overlay specifically.
    pub pager_mount: Option<Mount>,
    /// The pager painted under the pointer, if any — from [`App::pager_slot_at`].
    ///
    /// Takes precedence over `region` for the wheel and for left-press, because a
    /// pager is drawn OVER the layout: an `Overlay` covers the whole frame and a
    /// `^a v` scrollback covers the pane's rect, yet `region_at` reports
    /// `Region::Pane` for both (it tests `layout.pane` first). Without this the
    /// wheel scrolled the agent behind a full-screen pager and a left click was
    /// forwarded into it.
    pub covering_pager: Option<PagerSlot>,
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
    /// The pane's agent has a verified scroll keybinding
    /// ([`crate::agent::AgentProfile::wheel_scroll`]). Only consulted when the
    /// child does NOT want mouse — forwarding a real mouse report always wins.
    pub pane_scroll_keys: bool,
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
    /// Whether spyc should do the selecting over the pane itself.
    ///
    /// The complement of [`Self::can_forward_to_child`] on the mouse axis: a child
    /// that speaks mouse draws its OWN selection (#224), and painting ours on top
    /// would double it up. What's left — codex, a plain shell — has no other way to
    /// be selected. Still requires a live, visible grid.
    const fn pane_is_selectable(self) -> bool {
        !self.pane_wants_mouse && !self.pane_closed && !self.has_scroll_pager
    }

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

    // A full-frame **modal pager** is the only interacting surface while it's open,
    // matching how `route_input` gives it every key including meta chords. Clicking
    // beside its box used to reach whatever the layout said was underneath —
    // selecting content in the pane "under" a pager the user is reading, which is
    // the POLA break reported against #228. Middle-click paste is kept (it targets
    // the pager's own search/jump prompt), and the wheel/left arms below still route
    // to it when the pointer is actually over its content.
    if matches!(snap.pager_mount, Some(Mount::Overlay)) && !matches!(gesture, Gesture::Middle) {
        return match (gesture, snap.covering_pager) {
            (Gesture::Wheel, Some(slot)) => MouseSink::Pager(slot),
            (Gesture::Left, Some(slot)) => MouseSink::FocusAndSelect(slot),
            // Over the modal's frame but not its content: it still owns the event.
            _ => MouseSink::Swallow,
        };
    }

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
            // An open pager owns the pointer wherever it is painted — checked
            // BEFORE the pane, which `region_at` would otherwise win for a pager
            // drawn over the pane's rect (forwarding the press into the agent and
            // making it start its own selection).
            if let Some(slot) = snap.covering_pager {
                return MouseSink::FocusAndSelect(slot);
            }
            // Left is click-THROUGH: focus the region, and (for a mouse-aware
            // child) let the event reach it too. The pane is live and visible, so
            // swallowing the first click just to focus would read as broken.
            if matches!(region, Region::Pane) && snap.can_forward_to_child() {
                return MouseSink::FocusAndForward;
            }
            // A live child that ignores mouse reports: nothing else can do the
            // selecting, so spyc does it over the visible grid. Gated on the child
            // being alive and not hidden behind a `^a v` pager, for the same reasons
            // forwarding is — selecting text off a dead or invisible grid is noise.
            if matches!(region, Region::Pane) && snap.pane_is_selectable() {
                return MouseSink::FocusAndSelectPane;
            }
            // A bare list (no pager over it): focus the column and start a row
            // selection, so drag-to-copy-filenames works where the names are.
            if matches!(region, Region::List) {
                return MouseSink::FocusAndSelectRows(crate::app::state::Side::Left);
            }
            if matches!(region, Region::RightColumn) {
                return MouseSink::FocusAndSelectRows(crate::app::state::Side::Right);
            }
            // The single-line chrome surfaces hold text and no keyboard focus, so a
            // press there can only mean "select this". The divider carries the tab
            // bar, where a custom session name is the thing worth copying.
            if matches!(region, Region::Status | Region::Divider) {
                return MouseSink::SelectChrome;
            }
            return MouseSink::FocusRegion;
        }
        Gesture::Wheel => {
            // Same precedence, same reason: scroll the pager you are looking at,
            // not the agent behind it.
            if let Some(slot) = snap.covering_pager {
                return MouseSink::Pager(slot);
            }
        }
    }

    match region {
        // A pager over the list was already claimed by `covering_pager` above. What
        // reaches here is the list itself. The region names which column, so the
        // wheel scrolls the one under the pointer.
        Region::List => MouseSink::ListCursor(crate::app::state::Side::Left),
        Region::RightColumn => MouseSink::ListCursor(crate::app::state::Side::Right),
        Region::Pane => {
            if snap.can_forward_to_child() {
                MouseSink::PaneForward
            } else if snap.pane_scroll_keys && !snap.pane_closed && !snap.has_scroll_pager {
                // The child ignores mouse reports but scrolls on a keypress
                // (agy). Same two guards forwarding uses: a dead child has
                // nothing to scroll, and a `^a v` pager owns the pane's rect, so
                // keys would scroll a child the user can't see.
                MouseSink::PaneScrollKeys
            } else {
                // No verified scroll key and nothing to forward (codex): silence
                // beats typing `\e[<64;20;5M` — or a history-recalling Up — into
                // a child that never asked for either.
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

/// Clamp `(col, row)` into `view`'s last-rendered content rect, so a pointer
/// beyond any edge — including past the top/bottom, which the caller scrolls
/// toward — still names the nearest character instead of naming none.
///
/// `None` only when the view has never rendered (`last_content_area` is
/// zero-sized) — there is no edge to clamp to.
fn clamp_to_area(view: &crate::ui::pager::PagerView, col: u16, row: u16) -> Option<(u16, u16)> {
    let area = view.last_content_area.get();
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let col = col.clamp(area.x, area.x + area.width - 1);
    let row = row.clamp(area.y, area.y + area.height - 1);
    Some((col, row))
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

    /// The active pane agent's verified scroll keybinding, if it has one.
    fn active_pane_wheel_scroll(&self) -> Option<crate::agent::WheelScroll> {
        let tabs = self.runtime.pane_tabs.as_ref()?;
        crate::agent::detect(&tabs.active_info().command).wheel_scroll()
    }

    /// Translate a wheel tick into the child's own scroll keys, one press per
    /// line, and send them as a single batch.
    ///
    /// One `SendToPane` rather than N: the executor writes to the pty once, so a
    /// three-line tick can't interleave with the child's own output mid-burst.
    fn send_scroll_keys(&self, delta: i32) -> Vec<Effect> {
        let Some(scroll) = self.active_pane_wheel_scroll() else {
            return Vec::new();
        };
        let (code, mods) = if delta < 0 { scroll.up } else { scroll.down };
        let key = crossterm::event::KeyEvent::new(code, mods);
        let per_press = crate::pane::input::encode_key(key);
        if per_press.is_empty() {
            return Vec::new();
        }
        // The PANE's own step, not `scroll_lines`. Those are different jobs: the list
        // and pagers want 1 line per wheel event, because a trackpad already emits
        // one event per notional line (owner-confirmed: "the file list speed is
        // great"). But here spyc is driving somebody ELSE's pager by synthesizing
        // arrows, and that pager moves one line per key with no safe way to ask it
        // for a page — so at 1 the wheel couldn't traverse a long `^T` history.
        // Only `delta`'s SIGN is used here (resolved into `code` above).
        let repeats = self.state.config.mouse.pane_scroll_lines.max(1);
        let mut bytes = Vec::with_capacity(per_press.len() * repeats);
        for _ in 0..repeats {
            bytes.extend_from_slice(&per_press);
        }
        vec![Effect::SendToPane {
            target: super::effect::PaneTarget::Active,
            input: super::effect::PaneInput::Bytes(bytes),
            on_ok: None,
            // Same reasoning as `forward_to_child`: no per-tick flash, or a dead
            // pty buries its real exit message under one repeat per wheel line.
            err_prefix: None,
        }]
    }

    /// Anchor a charwise selection where the pointer pressed.
    ///
    /// Claims the drag (`view.mouse_selection`) only if the press actually landed
    /// on a character. A press on the gutter or below the last line focuses the
    /// pager and nothing more, so it cannot strand a selection that never had an
    /// anchor — and, because claiming is what diverts drags away from the child,
    /// an unclaimed press leaves forwarding exactly as it was.
    fn begin_pager_selection(&mut self, ev: MouseEvent, slot: PagerSlot) -> Vec<Effect> {
        let Some(view) = self.pager_in_slot_mut(slot) else {
            return Vec::new();
        };
        let Some((line, col)) = view.hit_test(ev.column, ev.row) else {
            return Vec::new();
        };
        view.begin_char_selection(line, col);
        self.view.mouse_selection = Some(MouseDragTarget::Pager(slot));
        Vec::new()
    }

    /// Extend the in-progress selection to the pointer.
    ///
    /// A pointer past the content rect's top/bottom edge scrolls the pager toward
    /// it and extends to the new edge line — the pager keeps hold of the gesture
    /// (**"full priority" while it's open**) instead of freezing once the pointer
    /// leaves its rect, which read as the pager handing the drag off to whatever
    /// is on screen below it. `clamp_to_area` folds a horizontal miss (left/right
    /// of the box, or before the first render) into the same treatment: extend to
    /// the nearest edge rather than doing nothing.
    fn extend_pager_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(MouseDragTarget::Pager(slot)) = self.view.mouse_selection else {
            return Vec::new();
        };
        let Some(view) = self.pager_in_slot_mut(slot) else {
            return Vec::new();
        };
        let Some((col, row)) = clamp_to_area(view, ev.column, ev.row) else {
            return Vec::new(); // never rendered — nothing to scroll or clamp to
        };
        // Scroll toward the pointer BEFORE hit-testing the clamped edge row, so the
        // same on-screen row resolves to a new (further) line each time this fires
        // — which, since a physically-held drag keeps generating `Drag` events as
        // long as the pointer moves at all, gives a continuous scroll-and-extend
        // for as long as the user holds past the edge and wiggles the pointer.
        let area = view.last_content_area.get();
        if ev.row < area.y {
            view.scroll_by_within_content(-1, view.last_viewport_h.get());
        } else if ev.row >= area.y.saturating_add(area.height) {
            view.scroll_by_within_content(1, view.last_viewport_h.get());
        }
        if let Some((line, c)) = view.hit_test(col, row) {
            view.extend_char_selection(line, c);
        }
        Vec::new()
    }

    /// Finish the gesture: copy when it selected something, and treat a press that
    /// never moved as the plain click it was.
    ///
    /// The selection is left highlighted rather than cleared. Unlike the keyboard
    /// `y` key — vim convention: yank exits visual mode, unchanged here — a mouse
    /// release is the terminal-native "select, then it just sits there" contract,
    /// and it's what lets a follow-up `y` / `Y` / a fresh drag still find it.
    fn finish_pager_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(MouseDragTarget::Pager(slot)) = self.view.mouse_selection.take() else {
            return Vec::new();
        };
        let Some(view) = self.pager_in_slot_mut(slot) else {
            return Vec::new();
        };
        if let Some((col, row)) = clamp_to_area(view, ev.column, ev.row)
            && let Some((line, c)) = view.hit_test(col, row)
        {
            view.extend_char_selection(line, c);
        }
        if !view.char_selection_is_nonempty() {
            // A click, not a drag. Drop the zero-width selection so it doesn't
            // linger as a stray highlight, and leave the clipboard alone.
            view.visual = None;
            return Vec::new();
        }
        // Captured before `visual_yank_text` consumes it (exits visual mode — the
        // contract the `y` key relies on), so it can be reinstated below.
        let sel = view.visual;
        let Some((text, lines, _)) = view.visual_yank_text(false) else {
            return Vec::new();
        };
        view.visual = sel;
        if text.is_empty() {
            return Vec::new();
        }
        // `CopyToPagerClipboard`, not `CopyToClipboard`: it lands the confirmation
        // in the pager title where the user is already looking, which is the same
        // place the `y` key reports to. A status-bar flash would be behind the
        // full-frame mount.
        let ok_msg = format!("copied {lines} line{}", if lines == 1 { "" } else { "s" });
        vec![Effect::CopyToPagerClipboard { text, ok_msg }]
    }

    /// The chrome row under the pointer, and the pointer's column within it.
    ///
    /// Matched on the recorded row's `y`, which is its identity — each chrome
    /// surface is exactly one screen row. Reads what the renderer actually drew, so
    /// the column the user pressed maps to the character they saw.
    fn chrome_col_at(&self, ev: MouseEvent) -> Option<(u16, u16)> {
        let rows = self.view.chrome_rows.borrow();
        let row = rows.iter().find(|r| r.y == ev.row && ev.column >= r.x)?;
        Some((row.y, ev.column - row.x))
    }

    /// Anchor a chrome-line selection where the pointer pressed.
    fn begin_chrome_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some((y, col)) = self.chrome_col_at(ev) else {
            return Vec::new();
        };
        self.view.chrome_selection = Some(super::ChromeSelection {
            y,
            anchor_col: col,
            focus_col: col,
        });
        self.view.mouse_selection = Some(MouseDragTarget::Chrome);
        Vec::new()
    }

    /// Extend it. The pointer is clamped to the row it started on: these surfaces
    /// are one row each, so drifting vertically mid-drag should widen the selection
    /// rather than abandon it.
    fn extend_chrome_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(sel) = self.view.chrome_selection else {
            return Vec::new();
        };
        let col = {
            let rows = self.view.chrome_rows.borrow();
            let Some(row) = rows.iter().find(|r| r.y == sel.y) else {
                return Vec::new();
            };
            ev.column.saturating_sub(row.x)
        };
        if let Some(s) = self.view.chrome_selection.as_mut() {
            s.focus_col = col;
        }
        Vec::new()
    }

    /// Copy the selected columns, keeping the highlight up.
    ///
    /// A press that never moved is a click, not a selection — the highlight is
    /// dropped and the clipboard left alone, matching every other surface. That is
    /// also why this replaced click-copies-everything: a click now does nothing
    /// surprising.
    fn finish_chrome_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        self.view.mouse_selection = None;
        self.extend_chrome_selection(ev);
        let Some(sel) = self.view.chrome_selection else {
            return Vec::new();
        };
        if sel.anchor_col == sel.focus_col {
            self.view.chrome_selection = None;
            return Vec::new();
        }
        let (lo, hi) = sel.range();
        let text = {
            let rows = self.view.chrome_rows.borrow();
            let Some(row) = rows.iter().find(|r| r.y == sel.y) else {
                return Vec::new();
            };
            crate::ui::line_select::text_between_columns(
                &row.line,
                usize::from(lo),
                usize::from(hi),
            )
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return Vec::new();
        }
        vec![Effect::CopyToClipboard {
            text,
            ok: super::effect::ClipMsg::StatusLine,
        }]
    }

    /// Translate the pointer into the pane's own grid coordinates, clamped into it.
    ///
    /// Clamped rather than rejected outside the rect: dragging a little past the
    /// pane's edge is normal, and it should extend to the edge cell instead of
    /// stalling. `None` only when there is no pane rect at all.
    fn pane_cell_at(&self, ev: MouseEvent) -> Option<(u16, u16)> {
        let (cols, rows) = self.view.term_size;
        let pane = self.frame_layout(Rect::new(0, 0, cols, rows)).pane?;
        if pane.width == 0 || pane.height == 0 {
            return None;
        }
        let col = ev.column.clamp(pane.x, pane.x + pane.width - 1) - pane.x;
        let row = ev.row.clamp(pane.y, pane.y + pane.height - 1) - pane.y;
        Some((row, col))
    }

    /// Anchor a pane text selection where the pointer pressed.
    fn begin_pane_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(cell) = self.pane_cell_at(ev) else {
            return Vec::new();
        };
        self.view.pane_selection = Some((cell, cell));
        self.view.mouse_selection = Some(MouseDragTarget::Pane);
        Vec::new()
    }

    /// Extend it to the pointer.
    fn extend_pane_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        if let Some(cell) = self.pane_cell_at(ev)
            && let Some((_, focus)) = self.view.pane_selection.as_mut()
        {
            *focus = cell;
        }
        Vec::new()
    }

    /// Copy the selected grid text, keeping the highlight up.
    ///
    /// A press that never moved is a click, not a selection: the highlight is
    /// dropped and the clipboard left alone, matching the pager. Ordering happens
    /// here (`(row, col)` pairs, not per-axis) so a backwards drag copies the same
    /// text as the forward one.
    fn finish_pane_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        self.view.mouse_selection = None;
        if let Some(cell) = self.pane_cell_at(ev)
            && let Some((_, focus)) = self.view.pane_selection.as_mut()
        {
            *focus = cell;
        }
        let Some((a, b)) = self.view.pane_selection else {
            return Vec::new();
        };
        if a == b {
            self.view.pane_selection = None;
            return Vec::new();
        }
        let (start, end) = if a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1) {
            (a, b)
        } else {
            (b, a)
        };
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return Vec::new();
        };
        let text = tabs.active().selection_text(start, end);
        if text.trim().is_empty() {
            return Vec::new();
        }
        vec![Effect::CopyToClipboard {
            text,
            ok: super::effect::ClipMsg::PaneLines,
        }]
    }

    /// The rect a column's list is drawn into, and a `ListView` over its cached
    /// rows — the same pair the renderer builds, so a hit-test can't disagree with
    /// what's on screen.
    fn list_row_at(&self, side: crate::app::state::Side, ev: MouseEvent) -> Option<usize> {
        use crate::app::state::Side;
        let (cols, rows) = self.view.term_size;
        let layout = self.frame_layout(Rect::new(0, 0, cols, rows));
        let (area, cached, commander) = match side {
            Side::Left => (layout.list, &self.view.cached_rows, &self.state.left),
            Side::Right => (
                layout.right?,
                &self.view.right_cached_rows,
                self.state.right.as_ref()?,
            ),
        };
        crate::ui::list_view::ListView {
            rows: cached,
            cursor: commander.cursor.index,
            view_top: commander.cursor.view_top,
            empty_marker: false,
            focused: true,
            theme: &self.view.theme,
            selection: None,
        }
        .row_at(area, ev.column, ev.row)
    }

    /// Anchor a row selection where the pointer pressed, and move the cursor there.
    ///
    /// Moving the cursor is deliberate: clicking a file should put the cursor on it,
    /// which is what makes the click useful on its own rather than only as the start
    /// of a drag. Claims the drag only if the press landed on a row, so a press in
    /// the gutter between columns leaves forwarding and focus untouched.
    ///
    /// The modifier is read HERE rather than carried in the sink because it selects
    /// the copy's *content*, not its routing. Ctrl, not Shift — Shift is the
    /// terminal's own selection-bypass and is frequently consumed before spyc sees
    /// it.
    fn begin_row_selection(
        &mut self,
        ev: MouseEvent,
        side: crate::app::state::Side,
    ) -> Vec<Effect> {
        let Some(idx) = self.list_row_at(side, ev) else {
            return Vec::new();
        };
        let full_path = ev
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL);
        self.state.col_mut(side).cursor.index = idx;
        self.view.list_selection = Some(super::ListSelection {
            side,
            anchor: idx,
            focus: idx,
            full_path,
        });
        self.view.mouse_selection = Some(MouseDragTarget::List(side));
        Vec::new()
    }

    /// Extend the row selection to the pointer, dragging the cursor with it.
    ///
    /// A pointer off the rows holds the selection where it was, matching the pager:
    /// drifting a few pixels past the last row mid-drag is normal, and collapsing
    /// there would read as a bug.
    fn extend_row_selection(
        &mut self,
        ev: MouseEvent,
        side: crate::app::state::Side,
    ) -> Vec<Effect> {
        if let Some(idx) = self.list_row_at(side, ev) {
            if let Some(sel) = self.view.list_selection.as_mut() {
                sel.focus = idx;
            }
            self.state.col_mut(side).cursor.index = idx;
        }

        Vec::new()
    }

    /// Copy the selected rows' names (or absolute paths) and keep the highlight.
    ///
    /// Paths are resolved from the LIVE listing here rather than captured at press
    /// time, so a selection can't paste stale paths after a refresh. A single row is
    /// still a copy — unlike the pager, where a click that never moved is just a
    /// click, clicking one filename to copy it is the obvious gesture.
    fn finish_row_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        self.view.mouse_selection = None;
        let Some(sel) = self.view.list_selection else {
            return Vec::new();
        };
        if let Some(idx) = self.list_row_at(sel.side, ev)
            && let Some(s) = self.view.list_selection.as_mut()
        {
            s.focus = idx;
        }
        let sel = self.view.list_selection.expect("checked above");
        let (lo, hi) = sel.range();
        let commander = self.state.col(sel.side);
        let picked: Vec<String> = commander
            .rows
            .iter()
            .skip(lo)
            .take(hi.saturating_sub(lo) + 1)
            .map(|r| {
                if sel.full_path {
                    r.path.display().to_string()
                } else {
                    r.display.clone()
                }
            })
            .collect();
        if picked.is_empty() {
            return Vec::new();
        }
        let count = picked.len();
        vec![Effect::CopyToClipboard {
            text: picked.join("\n"),
            ok: super::effect::ClipMsg::ListNames {
                count,
                paths: sel.full_path,
            },
        }]
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
    /// Give the keyboard to the region that OWNS `slot`.
    ///
    /// Distinct from [`Self::focus_region`], which answers from layout geometry: a
    /// pager drawn over the pane's rect would otherwise focus the pty behind it.
    /// A `Modal` needs no move — `recompute_focus` resolves a full-frame Overlay to
    /// `Focus::Pager(Overlay)` on its own, and touching pane focus here is exactly
    /// what leaked a click through to the pane.
    fn focus_pager_slot(&mut self, slot: PagerSlot) {
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
            covering_pager: None,
            pane_wants_mouse: false,
            is_prompting: false,
            has_scroll_pager: false,
            pane_closed: false,
            pane_scroll_keys: false,
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
        // types `\e[<64;20;5M` into its prompt. Silence is the correct answer for
        // a child with no verified scroll key either (codex).
        let s = snap(Some(Region::Pane));
        assert!(!s.pane_wants_mouse && !s.pane_scroll_keys);
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Swallow);
    }

    /// An agent that ignores mouse reports but scrolls on a keypress (agy) gets
    /// its keys instead of silence — the regression `[mouse] capture` introduced
    /// by turning DEC 1007's wheel-to-arrows translation off.
    #[test]
    fn non_mouse_child_with_a_verified_scroll_key_gets_keys() {
        let s = MouseSnapshot {
            pane_scroll_keys: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::PaneScrollKeys);
    }

    /// Forwarding a real mouse report always beats synthesizing keys: a child that
    /// speaks mouse gets exact coordinates, which keys cannot express.
    #[test]
    fn forwarding_wins_over_scroll_keys_when_the_child_speaks_mouse() {
        let s = MouseSnapshot {
            pane_wants_mouse: true,
            pane_scroll_keys: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::PaneForward);
    }

    /// Same two guards forwarding uses. A dead child has nothing to scroll, and a
    /// `^a v` pager owns the pane's rect — scrolling the child behind it would move
    /// something the user cannot see.
    #[test]
    fn scroll_keys_are_suppressed_for_a_dead_or_covered_child() {
        for (closed, covered) in [(true, false), (false, true)] {
            let s = MouseSnapshot {
                pane_scroll_keys: true,
                pane_closed: closed,
                has_scroll_pager: covered,
                ..snap(Some(Region::Pane))
            };
            assert_eq!(
                route_mouse(s, Gesture::Wheel),
                MouseSink::Swallow,
                "closed={closed} covered={covered}"
            );
        }
    }

    /// A left press on a covered pager anchors a text selection in THAT pager's
    /// slot — whichever region the layout says is underneath.
    #[test]
    fn left_press_on_a_pager_starts_a_selection() {
        for (slot, region) in [
            (PagerSlot::Top, Region::List),
            (PagerSlot::Right, Region::RightColumn),
            (PagerSlot::Scrollback, Region::Pane),
        ] {
            let s = MouseSnapshot {
                covering_pager: Some(slot),
                ..snap(Some(region))
            };
            assert_eq!(
                route_mouse(s, Gesture::Left),
                MouseSink::FocusAndSelect(slot),
                "{slot:?} under a pointer over {region:?}"
            );
        }
    }

    /// With no pager over it, a left press on the list focuses the column AND
    /// anchors a row selection — that's how drag-to-copy-filenames starts. The
    /// side comes from the region, so `b` never selects in `a`.
    #[test]
    fn left_press_on_the_bare_list_selects_rows_in_that_column() {
        assert_eq!(
            route_mouse(snap(Some(Region::List)), Gesture::Left),
            MouseSink::FocusAndSelectRows(Side::Left)
        );
        assert_eq!(
            route_mouse(snap(Some(Region::RightColumn)), Gesture::Left),
            MouseSink::FocusAndSelectRows(Side::Right)
        );
    }

    /// Selection must not steal the click-through contract: a press over a
    /// mouse-aware child still focuses AND forwards, even with a pager mounted up
    /// top (the `D` + pane coexistence case).
    #[test]
    fn left_press_on_a_mouse_aware_pane_still_forwards_with_a_pager_mounted() {
        let s = MouseSnapshot {
            pane_wants_mouse: true,
            pager_mount: Some(Mount::TopPane),
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::FocusAndForward);
    }

    /// A prompt still wins over starting a selection — the arm that keeps a
    /// right-click from latching a chord also covers left.
    #[test]
    fn a_prompt_suppresses_selection() {
        let s = MouseSnapshot {
            is_prompting: true,
            pager_mount: Some(Mount::Overlay),
            ..snap(Some(Region::List))
        };
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::Swallow);
    }

    #[test]
    fn list_routes_to_the_cursor_or_to_a_pager_covering_it() {
        // The side comes from the REGION, so the wheel scrolls the column under the
        // pointer rather than the focused one.
        assert_eq!(
            route_mouse(snap(Some(Region::List)), Gesture::Wheel),
            MouseSink::ListCursor(Side::Left)
        );
        assert_eq!(
            route_mouse(snap(Some(Region::RightColumn)), Gesture::Wheel),
            MouseSink::ListCursor(Side::Right)
        );
        // A pager COVERING the pointer takes those events.
        for slot in [PagerSlot::Modal, PagerSlot::Top] {
            let s = MouseSnapshot {
                covering_pager: Some(slot),
                ..snap(Some(Region::List))
            };
            assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Pager(slot));
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

        // A child that never asked for mouse is never FORWARDED to — the #170 gate
        // applies to buttons exactly as it does to the wheel. It now gets a
        // spyc-side selection instead of a bare focus; see
        // `a_non_mouse_pane_selects_instead_of_merely_focusing`.
        let unaware = snap(Some(Region::Pane));
        assert_eq!(
            route_mouse(unaware, Gesture::Left),
            MouseSink::FocusAndSelectPane
        );

        // A list press focuses AND starts a row selection; see
        // `left_press_on_the_bare_list_selects_rows_in_that_column`.
        for (region, side) in [
            (Region::List, Side::Left),
            (Region::RightColumn, Side::Right),
        ] {
            assert_eq!(
                route_mouse(snap(Some(region)), Gesture::Left),
                MouseSink::FocusAndSelectRows(side),
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
    ///
    /// `Status` and `Divider` are excluded: they hold text and no keyboard focus, so
    /// a press there starts a selection — see
    /// `chrome_lines_start_a_selection_rather_than_taking_focus`.
    #[test]
    fn chrome_focuses_nothing_but_still_pastes_and_opens_the_menu() {
        // Only the prompt row is left: `Status` and `Divider` now start selections.
        let prompt = snap(Some(Region::Prompt));
        assert_eq!(
            route_mouse(prompt, Gesture::Left),
            MouseSink::FocusRegion,
            "routed, then `focus_region` no-ops on chrome"
        );
        assert_eq!(route_mouse(prompt, Gesture::Middle), MouseSink::Paste);
        assert_eq!(route_mouse(prompt, Gesture::Right), MouseSink::LeaderMenu);
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

    // ── pager selection: begin / extend / finish (App methods, not the pure
    // `route_mouse` decision) ──────────────────────────────────────────────

    use super::PagerSlot;
    use crate::app::Effect;
    use crate::app::state::Side;
    use crate::ui::pager::{PagerView, VisualKind};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    /// A pager mounted at a known, small content rect — as if it had just
    /// rendered — with enough lines to scroll. Lines are single chars at a
    /// known width so column math in the tests is easy to check by hand.
    fn mounted_pager() -> PagerView {
        let mut view = PagerView::new_plain("t", (0..10).map(|i| format!("line{i}")).collect());
        // Small on purpose: a 2-row viewport out of 10 lines guarantees both
        // "past the bottom" and "past the top" are reachable within a few
        // Drag events, without wrap complicating the row math.
        view.wrap = false;
        view.show_line_numbers = false;
        view.last_content_area.set(Rect::new(0, 0, 10, 2));
        view.last_viewport_h.set(2);
        view
    }

    fn app_with_pager(view: PagerView) -> App {
        let tmp = tempfile::tempdir().expect("tempdir");
        // `test_app` reads/writes under the state root; leak the tempdir for
        // the test's lifetime rather than threading a closure through every
        // call site below.
        let path = tmp.keep();
        crate::state::with_state_root(&path, || {
            let mut app = App::test_app(path.clone());
            app.view.pager = Some(view);
            app
        })
    }

    fn left_down(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left_drag(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left_up(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// The headline fix: a drag held past the BOTTOM edge scrolls the pager
    /// toward the pointer and keeps extending, rather than freezing once the
    /// pointer leaves the content rect — which is what made the pager seem to
    /// hand the gesture off to whatever is on screen below it.
    #[test]
    fn drag_past_the_bottom_edge_scrolls_and_keeps_extending() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(0, 0), PagerSlot::Modal);
        let before = app.view.pager.as_ref().unwrap().scroll;

        // Row 5 is three rows past the 2-row viewport — squarely "in the pane".
        app.extend_pager_selection(left_drag(0, 5));

        let view = app.view.pager.as_ref().expect("still mounted");
        assert!(
            view.scroll > before,
            "a drag past the bottom edge must scroll the pager, not freeze it"
        );
        let sel = view
            .visual
            .expect("selection survives a drag past the edge");
        assert!(
            sel.cursor > sel.anchor,
            "the selection must keep growing, not stick at the edge"
        );
    }

    /// The symmetric case: past the TOP edge scrolls up. The rect is offset
    /// from row 0 specifically so there is screen space above it to drag into.
    #[test]
    fn drag_past_the_top_edge_scrolls_up() {
        let mut view = mounted_pager();
        view.scroll = 5; // start mid-document so there's room to scroll up
        view.last_content_area.set(Rect::new(0, 3, 10, 2)); // rows 3-4; row 0 is "above"
        let mut app = app_with_pager(view);
        app.begin_pager_selection(left_down(0, 3), PagerSlot::Modal);

        app.extend_pager_selection(left_drag(0, 0));

        let view = app.view.pager.as_ref().expect("still mounted");
        assert!(view.scroll < 5, "a drag past the top edge must scroll up");
    }

    /// A pointer beside (not above/below) the rect clamps horizontally and
    /// keeps extending — it must not require the drag to stay inside the box
    /// pixel-perfectly to keep selecting.
    #[test]
    fn drag_beyond_the_right_edge_clamps_without_scrolling() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(0, 0), PagerSlot::Modal);
        let scroll_before = app.view.pager.as_ref().unwrap().scroll;

        app.extend_pager_selection(left_drag(50, 0)); // same row, way past the right edge

        let view = app.view.pager.as_ref().expect("still mounted");
        assert_eq!(
            view.scroll, scroll_before,
            "a horizontal miss must not scroll"
        );
        let sel = view.visual.expect("still selecting");
        // The rect's right edge (col 9) clamps first, and THEN `hit_test` clamps
        // again to the line's own length ("line0" is 5 chars, max index 4) — the
        // same end-of-line behavior `hit_test_clamps_past_end_of_line` pins.
        assert_eq!(
            sel.cursor_col, 4,
            "clamped into the rect, then to end-of-line"
        );
    }

    /// The other half of the report: releasing after a real drag must leave
    /// the selection highlighted (the terminal-native "it just sits there"
    /// contract), not clear it — while still copying and reporting the count.
    #[test]
    fn release_after_a_drag_copies_and_keeps_the_highlight() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(0, 0), PagerSlot::Modal);
        app.extend_pager_selection(left_drag(3, 1));

        let effects = app.finish_pager_selection(left_up(3, 1));

        assert!(
            matches!(effects.as_slice(), [Effect::CopyToPagerClipboard { .. }]),
            "a real drag must copy on release: {effects:?}"
        );
        let sel = app
            .view
            .pager
            .as_ref()
            .and_then(|v| v.visual)
            .expect("selection must survive the copy, unlike the keyboard `y` contract");
        assert_eq!(sel.kind, VisualKind::Char);
    }

    /// A press that never moves is a plain click: no copy, and no stray
    /// zero-width highlight left behind.
    #[test]
    fn release_without_motion_is_a_click_not_a_selection() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(2, 0), PagerSlot::Modal);

        let effects = app.finish_pager_selection(left_up(2, 0));

        assert!(effects.is_empty(), "a plain click must not copy anything");
        assert!(
            app.view.pager.as_ref().unwrap().visual.is_none(),
            "a plain click must not leave a stray highlight"
        );
    }
    // ── an open pager outranks the region painted beneath it ──────────────

    /// **The bug this pins.** An `Overlay` pager renders into `frame.area()` and a
    /// `^a v` scrollback renders into the pane's rect, but `region_at` tests
    /// `layout.pane` FIRST — so the pointer over either resolved to `Region::Pane`.
    /// The wheel then scrolled the agent *behind* a full-screen pager, and a left
    /// click was forwarded into it, which is what made claude start its own
    /// selection underneath the pager the user was reading.
    #[test]
    fn a_pager_covering_the_pane_outranks_the_pane() {
        for slot in [PagerSlot::Modal, PagerSlot::Scrollback] {
            let s = MouseSnapshot {
                covering_pager: Some(slot),
                // Everything that made the pane win before: pointer over the pane's
                // rect, and a live mouse-aware child eager to receive it.
                pane_wants_mouse: true,
                ..snap(Some(Region::Pane))
            };
            assert_eq!(
                route_mouse(s, Gesture::Wheel),
                MouseSink::Pager(slot),
                "{slot:?}: wheel must scroll the pager, not the agent behind it"
            );
            assert_eq!(
                route_mouse(s, Gesture::Left),
                MouseSink::FocusAndSelect(slot),
                "{slot:?}: click must select in the pager, not forward to the agent"
            );
        }
    }

    /// With no pager under the pointer the pane keeps click-through and the wheel —
    /// the coverage check must not have stolen the live-pane behaviour.
    #[test]
    fn an_uncovered_pane_still_forwards() {
        let s = MouseSnapshot {
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::PaneForward);
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::FocusAndForward);
    }

    /// **Deliberate reversal of what #228 shipped.** That version reasoned that
    /// beside a centered overlay the user still sees the list, so clicking there
    /// should reach it. In practice that read as a POLA break: it let a click
    /// select content in the pane *underneath* a pager the user was reading. An
    /// open modal pager is now the only interacting surface, matching how
    /// `route_input` hands it every key.
    ///
    /// Scoped to `Overlay` on purpose — see
    /// `a_non_modal_pager_leaves_the_rest_of_the_frame_alone`.
    #[test]
    fn a_modal_pager_swallows_input_beside_its_box() {
        let s = MouseSnapshot {
            pager_mount: Some(Mount::Overlay), // mounted...
            covering_pager: None,              // ...but not under THIS point
            ..snap(Some(Region::List))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Swallow);
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::Swallow);
    }

    /// A prompt still outranks a covering pager, and middle/right stay spyc's
    /// everywhere — the new precedence must sit below the existing ones.
    #[test]
    fn a_prompt_and_the_spyc_buttons_still_outrank_a_covering_pager() {
        let prompting = MouseSnapshot {
            is_prompting: true,
            covering_pager: Some(PagerSlot::Modal),
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(prompting, Gesture::Left), MouseSink::Swallow);

        let covered = MouseSnapshot {
            covering_pager: Some(PagerSlot::Modal),
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(covered, Gesture::Middle), MouseSink::Paste);
    }
    /// **A full-frame modal pager is the only interacting surface while it's open.**
    ///
    /// Clicking beside its box used to reach whatever the layout said was
    /// underneath — starting a selection in the pane "under" a pager the user is
    /// reading. Matches how `route_input` hands an Overlay every key.
    #[test]
    fn a_modal_pager_is_exclusive_even_where_it_does_not_cover() {
        let s = MouseSnapshot {
            pager_mount: Some(Mount::Overlay),
            covering_pager: None, // beside the box, over the pane's rect
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::Swallow);
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::Swallow);
        // Middle-click still pastes: it targets the pager's own search / jump prompt.
        assert_eq!(route_mouse(s, Gesture::Middle), MouseSink::Paste);
    }

    /// A NON-modal pager stays scoped to its own rect — it must not make the whole
    /// frame inert the way an Overlay does.
    #[test]
    fn a_non_modal_pager_leaves_the_rest_of_the_frame_alone() {
        let s = MouseSnapshot {
            pager_mount: Some(Mount::TopPane),
            covering_pager: None,
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Wheel), MouseSink::PaneForward);
    }
    // ── file list + status bar selection ─────────────────────────────────

    /// The single-line chrome surfaces hold text and no keyboard focus, so a press
    /// there can only mean "select this". Both of them: the divider carries the tab
    /// bar, where a custom session name is the thing worth copying.
    #[test]
    fn chrome_lines_start_a_selection_rather_than_taking_focus() {
        for region in [Region::Status, Region::Divider] {
            assert_eq!(
                route_mouse(snap(Some(region)), Gesture::Left),
                MouseSink::SelectChrome,
                "{region:?}"
            );
        }
    }

    /// A pager over the list still wins: `covering_pager` is checked before the
    /// row-selection arm, so clicking a pager never selects filenames behind it.
    #[test]
    fn a_covering_pager_outranks_row_selection() {
        let s = MouseSnapshot {
            covering_pager: Some(PagerSlot::Top),
            ..snap(Some(Region::List))
        };
        assert_eq!(
            route_mouse(s, Gesture::Left),
            MouseSink::FocusAndSelect(PagerSlot::Top)
        );
    }

    /// The `Row`s the renderer would have cached for `rows`. Built here rather than
    /// via `build_rows` (scoped to `app::render`) so the test doesn't widen
    /// production visibility; `row_at` only reads `len` and `display`.
    fn cached_for(rows: &[crate::app::RowData]) -> Vec<crate::ui::list_view::Row> {
        rows.iter()
            .map(|rd| crate::ui::list_view::Row {
                display: rd.display.clone(),
                kind: rd.kind,
                picked: false,
                taken: false,
                deleted: false,
                git_status: crate::ui::list_view::GitFileStatus::clean(),
                pending_delete: false,
            })
            .collect()
    }

    /// A list drag copies the dragged rows' NAMES, and with Ctrl held their absolute
    /// PATHS — the modifier is read at press time because it selects the copy's
    /// content, not its routing.
    ///
    /// Ctrl and not Shift: Shift is the terminal's own selection-bypass modifier and
    /// is frequently consumed before spyc ever sees the event.
    #[test]
    fn a_list_drag_copies_names_or_full_paths_with_ctrl() {
        use crate::app::state::Side;
        use crossterm::event::KeyModifiers;

        for (mods, want_paths) in [(KeyModifiers::NONE, false), (KeyModifiers::CONTROL, true)] {
            let tmp = tempfile::tempdir().expect("tempdir").keep();
            let effects = crate::state::with_state_root(&tmp, || {
                let mut app = App::test_app(tmp.clone());
                app.state.left.rows = ["alpha.rs", "beta.rs", "gamma.rs"]
                    .iter()
                    .map(|n| crate::app::RowData {
                        path: tmp.join(n),
                        display: (*n).to_string(),
                        kind: crate::fs::EntryKind::File,
                        deleted: false,
                    })
                    .collect();
                // Geometry the renderer would have settled.
                app.view.term_size = (80, 24);
                app.view.cached_rows = cached_for(&app.state.left.rows);

                let at = |kind, row, m| MouseEvent {
                    kind,
                    column: 0,
                    row,
                    modifiers: m,
                };
                app.begin_row_selection(
                    at(MouseEventKind::Down(MouseButton::Left), 1, mods),
                    Side::Left,
                );
                app.extend_row_selection(
                    at(MouseEventKind::Drag(MouseButton::Left), 3, mods),
                    Side::Left,
                );
                app.finish_row_selection(at(MouseEventKind::Up(MouseButton::Left), 3, mods))
            });

            let [Effect::CopyToClipboard { text, .. }] = effects.as_slice() else {
                panic!("expected one clipboard copy, got {effects:?}");
            };
            let lines: Vec<&str> = text.lines().collect();
            if want_paths {
                assert!(
                    lines.iter().all(|l| l.starts_with('/')),
                    "Ctrl held must copy absolute paths, got {lines:?}"
                );
                assert!(lines.iter().any(|l| l.ends_with("alpha.rs")));
            } else {
                assert!(
                    lines.iter().all(|l| !l.contains('/')),
                    "without Ctrl must copy bare names, got {lines:?}"
                );
            }
            assert!(lines.len() > 1, "a drag across rows selects a range");
        }
    }

    /// The highlight survives the copy, so it's visible and a follow-up yank can
    /// still find it — the same contract the pager's selection has.
    #[test]
    fn a_list_selection_persists_after_the_copy() {
        use crate::app::state::Side;
        use crossterm::event::KeyModifiers;
        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            app.state.left.rows = (0..4)
                .map(|i| crate::app::RowData {
                    path: tmp.join(format!("f{i}")),
                    display: format!("f{i}"),
                    kind: crate::fs::EntryKind::File,
                    deleted: false,
                })
                .collect();
            app.view.term_size = (80, 24);
            app.view.cached_rows = cached_for(&app.state.left.rows);
            let at = |kind, row| MouseEvent {
                kind,
                column: 0,
                row,
                modifiers: KeyModifiers::NONE,
            };
            // Screen row 1 is the list's first row — row 0 is the status bar, which
            // `row_at` correctly refuses.
            app.begin_row_selection(at(MouseEventKind::Down(MouseButton::Left), 1), Side::Left);
            app.extend_row_selection(at(MouseEventKind::Drag(MouseButton::Left), 3), Side::Left);
            let _ = app.finish_row_selection(at(MouseEventKind::Up(MouseButton::Left), 3));

            let sel = app
                .view
                .list_selection
                .expect("highlight must survive the copy");
            assert_eq!(sel.range(), (0, 2));
            assert!(
                app.view.mouse_selection.is_none(),
                "the drag itself must end, even though the selection stays"
            );
        });
    }
    // ── pane text selection (a child that ignores mouse) ──────────────────

    /// The complement of forwarding: a child that ignores mouse reports gets a
    /// spyc-side selection, because nothing else can select its text. This is what
    /// makes copy work inside codex's `^T` transcript overlay.
    #[test]
    fn a_non_mouse_pane_selects_instead_of_merely_focusing() {
        let s = snap(Some(Region::Pane));
        assert!(!s.pane_wants_mouse);
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::FocusAndSelectPane);
    }

    /// A child that speaks mouse draws its OWN selection (#224); painting ours on
    /// top would double it up, so forwarding still wins.
    #[test]
    fn a_mouse_aware_pane_still_forwards_rather_than_selecting() {
        let s = MouseSnapshot {
            pane_wants_mouse: true,
            ..snap(Some(Region::Pane))
        };
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::FocusAndForward);
    }

    /// No selecting text off a dead or hidden grid — the same two guards forwarding
    /// uses, for the same reasons.
    #[test]
    fn a_dead_or_covered_pane_is_not_selectable() {
        for (closed, covered) in [(true, false), (false, true)] {
            let s = MouseSnapshot {
                pane_closed: closed,
                has_scroll_pager: covered,
                ..snap(Some(Region::Pane))
            };
            assert_eq!(
                route_mouse(s, Gesture::Left),
                MouseSink::FocusRegion,
                "closed={closed} covered={covered}"
            );
        }
    }
    /// A mouse selection holds POSITIONS, not content, so the next keyboard action
    /// must retire it — otherwise navigating away leaves a highlight band sitting on
    /// whatever file now occupies those row indices, which is how this was reported.
    ///
    /// The pager's `visual` selection is exempt: it's modal by request, so `j`/`k`
    /// extend it and `Esc` cancels.
    #[test]
    fn a_key_retires_a_mouse_selection_but_not_the_pagers_visual_mode() {
        use crate::app::state::Side;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            app.view.list_selection = Some(crate::app::ListSelection {
                side: Side::Left,
                anchor: 1,
                focus: 3,
                full_path: false,
            });
            app.view.pane_selection = Some(((0, 0), (2, 4)));
            app.view.chrome_selection = Some(crate::app::ChromeSelection {
                y: 0,
                anchor_col: 2,
                focus_col: 8,
            });

            let mut pager = PagerView::new_plain("p", vec!["a".into(), "b".into()]);
            pager.visual = Some(crate::ui::pager::VisualSelection {
                anchor: 0,
                cursor: 1,
                anchor_col: 0,
                cursor_col: 0,
                kind: VisualKind::Char,
            });
            app.view.pager = Some(pager);

            let _ = app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

            assert!(
                app.view.list_selection.is_none(),
                "a keypress must retire the list selection"
            );
            assert!(
                app.view.pane_selection.is_none(),
                "a keypress must retire the pane selection"
            );
            assert!(
                app.view.chrome_selection.is_none(),
                "a keypress must retire the chrome selection"
            );
            assert!(
                app.view.pager.as_ref().and_then(|v| v.visual).is_some(),
                "the pager's visual mode is modal and must survive"
            );
        });
    }
    /// End-to-end: a drag across part of a chrome line copies exactly those columns
    /// — the point of the feature being substring selection rather than
    /// copy-the-whole-line (so you can take just a session id or a branch name).
    #[test]
    fn a_chrome_drag_copies_only_the_dragged_columns() {
        use crossterm::event::KeyModifiers;
        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            // Stand in for what the renderer records: one chrome row at y=0, x=0.
            app.view
                .chrome_rows
                .borrow_mut()
                .push(crate::app::ChromeRow {
                    y: 0,
                    x: 0,
                    line: ratatui::text::Line::from(vec![
                        ratatui::text::Span::raw("spyc"),
                        ratatui::text::Span::raw("|"),
                        ratatui::text::Span::raw("main*"),
                    ]),
                });
            let at = |kind, col| MouseEvent {
                kind,
                column: col,
                row: 0,
                modifiers: KeyModifiers::NONE,
            };
            // Drag columns 5..9 — "main*", crossing no segment seam of its own but
            // starting past two.
            app.begin_chrome_selection(at(MouseEventKind::Down(MouseButton::Left), 5));
            app.extend_chrome_selection(at(MouseEventKind::Drag(MouseButton::Left), 9));
            let effects = app.finish_chrome_selection(at(MouseEventKind::Up(MouseButton::Left), 9));

            let [Effect::CopyToClipboard { text, .. }] = effects.as_slice() else {
                panic!("expected one clipboard copy, got {effects:?}");
            };
            assert_eq!(text, "main*");
            assert!(
                app.view.chrome_selection.is_some(),
                "the highlight stays up after the copy, like the other surfaces"
            );
        });
    }

    /// A click that never moved is a click: no copy, no lingering highlight. This is
    /// what makes replacing click-copies-the-whole-line safe — a stray click on the
    /// status bar no longer silently overwrites the clipboard.
    #[test]
    fn a_chrome_click_without_motion_copies_nothing() {
        use crossterm::event::KeyModifiers;
        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            app.view
                .chrome_rows
                .borrow_mut()
                .push(crate::app::ChromeRow {
                    y: 0,
                    x: 0,
                    line: ratatui::text::Line::from(vec![ratatui::text::Span::raw("spyc|main*")]),
                });
            let at = |kind| MouseEvent {
                kind,
                column: 3,
                row: 0,
                modifiers: KeyModifiers::NONE,
            };
            app.begin_chrome_selection(at(MouseEventKind::Down(MouseButton::Left)));
            let effects = app.finish_chrome_selection(at(MouseEventKind::Up(MouseButton::Left)));
            assert!(effects.is_empty(), "a click must not copy: {effects:?}");
            assert!(
                app.view.chrome_selection.is_none(),
                "and leaves no highlight"
            );
        });
    }
}

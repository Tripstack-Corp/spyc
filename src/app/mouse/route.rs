//! Pure mouse routing decisions — the `Copy` snapshot, the pure fns, and the
//! types they speak in. Extracted verbatim from `mouse.rs` so the decision
//! layer can be read (and tested) without the impure `impl App` half.
//!
//! **Hit-test the pointer, not the keyboard focus.** This is the whole point of
//! a mouse: the wheel scrolls what the cursor is over, even when the keyboard
//! is somewhere else. So [`region_at`] resolves the pointer against the same
//! [`FrameLayout`] the renderer used, and [`route_mouse`] takes that region —
//! never `state.focus`.

use ratatui::layout::Rect;

use super::super::FrameLayout;
use super::super::modal::Modal;
use super::super::pager_handler::PagerSlot;
use super::PaneScrollStreak;
use crate::ui::pager::Mount;

/// Which frame region the pointer is over. Resolved from the pointer's
/// column/row against the live [`FrameLayout`], so it follows the cursor rather
/// than the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
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
pub enum MouseSink {
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
    /// Switch to the 0-based pane tab the pointer is over.
    ///
    /// Beats [`Self::SelectChrome`] on the divider: the tab labels are the one
    /// part of that row that means "go here" rather than "here is some text",
    /// and a click on a tab reads as activation in every tabbed UI. The rest of
    /// the divider — the cwd tail, the empty tail past the last tab — stays
    /// selectable, so copying the path still works.
    PaneTab(usize),
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
pub enum Gesture {
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
pub struct MouseSnapshot {
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
    /// The 0-based pane tab whose label the pointer is over, from
    /// [`super::tab_hit`]. `None` anywhere but the divider's tab bar — including
    /// the divider's cwd tail and the empty space past the last tab, which stay
    /// chrome-selectable.
    ///
    /// Resolved into the snapshot rather than inside [`route_mouse`] because the
    /// layout it needs is a `Vec` of spans, and the router is a `const fn` over
    /// a `Copy` snapshot.
    pub tab_under_pointer: Option<usize>,
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

/// How long spyc waits, after sending an agent's `transcript_toggle_key`, before
/// it's willing to send it again — see [`ViewState::pane_toggle_sent_at`].
/// Comfortably longer than one local pty round trip, short enough to recover
/// quickly if the scrape genuinely never confirms.
pub const TOGGLE_SETTLE: std::time::Duration = std::time::Duration::from_millis(400);

/// A gap between wheel ticks longer than this ends the current scroll streak — a
/// paused-then-resumed scroll is a new gesture, not a continuation of the old
/// speed. Comfortably longer than the inter-tick gap during a continuous flick.
pub const STREAK_GAP: std::time::Duration = std::time::Duration::from_millis(500);

/// How long a same-direction streak must run before it escalates to a page-sized
/// step. Owner-specified ("say past 1 second").
pub const ESCALATE_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// How many consecutive UP ticks (each within [`STREAK_GAP`] of the last) it takes
/// before a wheel gesture opens an agent's own scrollback view. Entering history is
/// a deliberate act, so it takes a deliberate gesture — one stray tick shouldn't
/// swap the pane out from under the user. Small enough that a real flick clears it
/// instantly; large enough that a jittery trackpad doesn't.
pub const OPEN_AFTER_UP_TICKS: u32 = 3;

/// Advance a wheel-scroll streak by one tick, and decide whether it has run long
/// enough to escalate. Pure (the `route.rs`/`focus.rs` template): takes `now` as a
/// parameter rather than reading the clock, so it's testable with synthetic
/// `Instant`s built via `checked_add`/`checked_sub` off a real `Instant::now()`
/// baseline (the same trick `codex_pin`'s tests use).
///
/// Restarts (rather than extending) the streak on a tab switch, a direction
/// change, or a gap longer than [`STREAK_GAP`] — each of those means "a new
/// gesture", not "the same one continuing", so none should inherit the old
/// streak's elapsed time or tick count.
pub fn scroll_streak_step(
    prev: Option<PaneScrollStreak>,
    tab_index: usize,
    dir: i8,
    now: std::time::Instant,
) -> (PaneScrollStreak, bool) {
    let restart = match prev {
        Some(s) => {
            s.tab_index != tab_index
                || s.dir != dir
                || now.duration_since(s.last_at).as_millis() > STREAK_GAP.as_millis()
        }
        None => true,
    };
    let (started_at, ticks) = if restart {
        (now, 1)
    } else {
        match prev {
            Some(s) => (s.started_at, s.ticks.saturating_add(1)),
            None => (now, 1), // unreachable: `prev.is_none()` implies `restart`
        }
    };
    let streak = PaneScrollStreak {
        tab_index,
        dir,
        started_at,
        last_at: now,
        ticks,
    };
    let escalate = now.duration_since(started_at).as_millis() >= ESCALATE_AFTER.as_millis();
    (streak, escalate)
}

/// What a wheel tick over an agent's own toggleable view should do — decided
/// PURELY from whether the view is open, the configured mode, and whether this
/// tick's streak has escalated. The impure half ([`App::send_agent_view_scroll_keys`])
/// is a thin wrapper: scrape for `is_open`, call this, translate the answer into
/// an effect + state mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentViewAction {
    /// Send the toggle key once, and start the settle guard.
    Toggle,
    /// Nothing this tick — `Off` mode while closed, or a toggle/close already
    /// sent and still settling.
    Nothing,
    /// Mount spyc's own `^a v` scrollback pager instead.
    UseSpycHistory,
    /// Send the agent's per-line scroll key, or its page key if `fast`.
    Scroll { fast: bool },
    /// Send the dedicated close key: already at the bottom, and still
    /// scrolling down has nowhere left to go — get out rather than no-op.
    Close,
}

/// Inputs to [`decide_agent_view_action`], bundled rather than four positional
/// `bool`s — each has a distinct meaning, and four bare bools at a call site is
/// exactly the shape that gets two of them silently transposed.
#[derive(Debug, Clone, Copy)]
pub struct AgentViewInputs {
    pub is_open: bool,
    /// `pane_toggle_sent_at`'s guard already resolved to a bool (elapsed vs.
    /// [`TOGGLE_SETTLE`]) — kept out of this function so it stays clock-free
    /// and trivially testable. Reused for BOTH directions of the open/close
    /// transition (see `send_agent_view_scroll_keys`'s doc): the same debounce
    /// that stops a fast-flick reopen also stops a re-close mid-settle.
    pub toggle_pending: bool,
    pub escalate: bool,
    pub at_bottom: bool,
    /// This tick's [`PaneScrollStreak::ticks`] — consecutive same-direction ticks,
    /// this one included. Only consulted while closed, as the "did the user mean
    /// it" half of the open gate.
    pub streak_ticks: u32,
}

/// Pure.
///
/// `dir`: -1 up, +1 down. Consulted twice: while CLOSED, only an up gesture may
/// open the view; while OPEN, it decides whether "at the bottom" should mean
/// "close" rather than "scroll" — scrolling UP at the bottom is just normal
/// scrolling.
pub const fn decide_agent_view_action(
    in_: AgentViewInputs,
    mode: crate::config::PaneScrollView,
    dir: i8,
) -> AgentViewAction {
    use crate::config::PaneScrollView as V;
    if !in_.is_open {
        // Only a sustained scroll UP opens history. Opening on a DOWN tick put
        // the pane in a flicker loop: the view opens at its bottom, so the next
        // down tick reads at-bottom and closes it, and the one after that
        // reopens — all while the user was only trying to scroll past the end of
        // the live buffer.
        if dir >= 0 || in_.streak_ticks < OPEN_AFTER_UP_TICKS {
            return AgentViewAction::Nothing;
        }
        return match mode {
            V::Off => AgentViewAction::Nothing,
            V::SpycHistory => AgentViewAction::UseSpycHistory,
            V::Native if in_.toggle_pending => AgentViewAction::Nothing,
            V::Native => AgentViewAction::Toggle,
        };
    }
    // Mode-independent: once open — however it got that way — "at the bottom,
    // still scrolling down" means the same thing regardless of how it opened.
    if dir > 0 && in_.at_bottom && !in_.toggle_pending {
        return AgentViewAction::Close;
    }
    AgentViewAction::Scroll { fast: in_.escalate }
}

/// Route one mouse event to a sink. Pure and total.
///
/// Order matters and mirrors `route_input`'s: a modal wins over everything, then
/// the region decides. A pager mounted as an overlay covers the list, so it takes
/// events aimed at the list region.
pub const fn route_mouse(snap: MouseSnapshot, gesture: Gesture) -> MouseSink {
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
            // A tab label is the one part of the divider that means "activate
            // me", so it outranks selecting the row's text.
            if let Some(index) = snap.tab_under_pointer {
                return MouseSink::PaneTab(index);
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
pub const fn region_at(layout: &FrameLayout, col: u16, row: u16) -> Option<Region> {
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

/// Clamp `(col, row)` into `view`'s last-rendered content rect, so a pointer
/// beyond any edge — including past the top/bottom, which the caller scrolls
/// toward — still names the nearest character instead of naming none.
///
/// `None` only when the view has never rendered (`last_content_area` is
/// zero-sized) — there is no edge to clamp to.
pub fn clamp_to_area(view: &crate::ui::pager::PagerView, col: u16, row: u16) -> Option<(u16, u16)> {
    let area = view.last_content_area.get();
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let col = col.clamp(area.x, area.x + area.width - 1);
    let row = row.clamp(area.y, area.y + area.height - 1);
    Some((col, row))
}

#[cfg(test)]
mod tests {
    use super::{
        AgentViewAction, AgentViewInputs, Gesture, MouseSink, MouseSnapshot, OPEN_AFTER_UP_TICKS,
        Region, decide_agent_view_action, region_at, route_mouse, scroll_streak_step,
    };
    use crate::app::App;
    use crate::app::pager_handler::PagerSlot;
    use crate::app::state::Side;
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
            tab_under_pointer: None,
        }
    }

    /// Left-clicking a tab label activates that tab instead of selecting text.
    #[test]
    fn a_left_press_on_a_tab_label_switches_to_it() {
        let mut s = snap(Some(Region::Divider));
        s.tab_under_pointer = Some(2);
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::PaneTab(2));
    }

    /// The rest of the divider stays selectable — the live cwd printed in its
    /// tail is the thing worth copying from that row.
    #[test]
    fn the_dividers_non_tab_columns_still_select() {
        let s = snap(Some(Region::Divider));
        assert_eq!(s.tab_under_pointer, None);
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::SelectChrome);
    }

    /// Tab activation is LEFT only. Middle stays paste and right stays the
    /// leader menu everywhere — a tab label is not an exception to that, and
    /// making it one would remove the only way to reach those over the divider.
    #[test]
    fn middle_and_right_are_unaffected_over_a_tab() {
        let mut s = snap(Some(Region::Divider));
        s.tab_under_pointer = Some(0);
        assert_eq!(route_mouse(s, Gesture::Middle), MouseSink::Paste);
        assert_eq!(route_mouse(s, Gesture::Right), MouseSink::LeaderMenu);
    }

    /// A modal or an open prompt still wins: tab activation must not become a
    /// hole in the guards that keep a click from reaching past an overlay or
    /// latching a chord while prompting.
    #[test]
    fn a_modal_or_prompt_still_beats_tab_activation() {
        use crate::app::modal::Modal;
        let mut s = snap(Some(Region::Divider));
        s.tab_under_pointer = Some(1);
        s.is_prompting = true;
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::Swallow);

        let mut s = snap(Some(Region::Divider));
        s.tab_under_pointer = Some(1);
        s.modal = Some(Modal::FindPicker);
        assert_eq!(route_mouse(s, Gesture::Left), MouseSink::Swallow);
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
            let report = crate::app::mouse::forward::mouse_report(ev, &layout)
                .expect("wheel is forwardable");
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
            let report = crate::app::mouse::forward::mouse_report(at_origin, &layout)
                .expect("wheel is forwardable");
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
            let r =
                crate::app::mouse::forward::mouse_report(at(kind), &layout).expect("forwardable");
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
                crate::app::mouse::forward::mouse_report(at(kind), &layout).is_none(),
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
    // ── scroll_streak_step: the sustained-scroll escalation timer ──────────

    fn later(base: std::time::Instant, ms: u64) -> std::time::Instant {
        base.checked_add(std::time::Duration::from_millis(ms))
            .expect("small offset from now")
    }

    /// The headline: a same-direction gesture escalates once it's run past
    /// `ESCALATE_AFTER` — this is the fix for "scrolling is too slow in ^t mode".
    #[test]
    fn a_sustained_same_direction_streak_escalates_after_the_threshold() {
        let t0 = std::time::Instant::now();
        let (s1, esc1) = scroll_streak_step(None, 0, 1, t0);
        assert!(!esc1, "the very first tick must not already be escalated");

        // Each successive gap stays under STREAK_GAP (500ms) so this is one
        // continuous streak, not a series of restarts.
        let (s2, esc2) = scroll_streak_step(Some(s1), 0, 1, later(t0, 400));
        assert!(!esc2, "under the threshold yet");

        let (s3, esc3) = scroll_streak_step(Some(s2), 0, 1, later(t0, 800));
        assert!(!esc3, "still under the threshold");

        let (_, esc4) = scroll_streak_step(Some(s3), 0, 1, later(t0, 1_100));
        assert!(esc4, "past ESCALATE_AFTER in the same direction");
    }

    /// A direction change is a new gesture — it must not inherit the old
    /// streak's elapsed time and escalate immediately.
    #[test]
    fn a_direction_change_restarts_the_streak() {
        let t0 = std::time::Instant::now();
        let (s1, _) = scroll_streak_step(None, 0, 1, t0);
        let (_, escalated_immediately) = scroll_streak_step(Some(s1), 0, -1, later(t0, 1_200));
        assert!(
            !escalated_immediately,
            "reversing direction must restart at the slow speed"
        );
    }

    /// A pause longer than STREAK_GAP is a new gesture too — resuming a scroll
    /// after glancing away shouldn't inherit the old speed either.
    #[test]
    fn a_long_pause_restarts_the_streak() {
        let t0 = std::time::Instant::now();
        let (s1, _) = scroll_streak_step(None, 0, 1, t0);
        // Long enough overall to be past ESCALATE_AFTER, but the GAP since the
        // last tick is what should matter here.
        let gap_tick_at = later(t0, 1_600);
        let (_, escalated) = scroll_streak_step(Some(s1), 0, 1, gap_tick_at);
        assert!(
            !escalated,
            "a stale streak with a big gap must restart, not escalate"
        );
    }

    /// Switching tabs must not hand a fast-scrolled codex tab's escalation to a
    /// DIFFERENT tab the user just switched to.
    #[test]
    fn switching_tabs_restarts_the_streak() {
        let t0 = std::time::Instant::now();
        let (s1, _) = scroll_streak_step(None, 0, 1, t0);
        let (_, escalated) = scroll_streak_step(Some(s1), 1, 1, later(t0, 1_200));
        assert!(
            !escalated,
            "a different tab index must not inherit the streak"
        );
    }

    /// Consecutive fast ticks within the gap keep extending the SAME streak
    /// (not restarting each time) — otherwise a real flick, whose ticks are only
    /// tens of ms apart, would never accumulate enough elapsed time to escalate.
    #[test]
    fn consecutive_fast_ticks_accumulate_toward_escalation() {
        let t0 = std::time::Instant::now();
        let mut streak = None;
        let mut escalated_at = None;
        for i in 0..40 {
            let t = later(t0, i * 30); // a tick every 30ms, like a real flick
            let (s, esc) = scroll_streak_step(streak, 0, 1, t);
            streak = Some(s);
            if esc && escalated_at.is_none() {
                escalated_at = Some(i);
            }
        }
        assert!(
            escalated_at.is_some(),
            "40 ticks at 30ms (1.2s) must cross ESCALATE_AFTER"
        );
    }

    /// The tick count that gates opening a view: it extends within one gesture
    /// and resets on any of the three restart conditions, so "three ticks" means
    /// three ticks of the SAME sustained scroll.
    #[test]
    fn the_tick_count_extends_within_a_gesture_and_resets_across_them() {
        let t0 = std::time::Instant::now();
        let (s1, _) = scroll_streak_step(None, 0, -1, t0);
        assert_eq!(s1.ticks, 1, "a fresh streak starts at one");

        let (s2, _) = scroll_streak_step(Some(s1), 0, -1, later(t0, 30));
        let (s3, _) = scroll_streak_step(Some(s2), 0, -1, later(t0, 60));
        assert_eq!(s3.ticks, 3, "consecutive same-direction ticks accumulate");

        let (reversed, _) = scroll_streak_step(Some(s3), 0, 1, later(t0, 90));
        assert_eq!(reversed.ticks, 1, "a direction change is a new gesture");

        let (after_gap, _) = scroll_streak_step(Some(s3), 0, -1, later(t0, 1_000));
        assert_eq!(after_gap.ticks, 1, "a gap past STREAK_GAP is a new gesture");

        let (other_tab, _) = scroll_streak_step(Some(s3), 1, -1, later(t0, 90));
        assert_eq!(other_tab.ticks, 1, "another tab is a new gesture");
    }

    // ── decide_agent_view_action: the codex ^T auto-open + escalation policy ──

    use crate::config::PaneScrollView;

    /// The DEFAULT behaviour, and the owner's stated preference: closed +
    /// `Native` + a sustained scroll UP opens it. Exactly one toggle send, never
    /// also a scroll this tick — see the doc on `send_agent_view_scroll_keys` for
    /// why not both.
    #[test]
    fn closed_and_native_opens_it_on_a_sustained_scroll_up() {
        assert_eq!(
            decide_agent_view_action(
                AgentViewInputs {
                    is_open: false,
                    toggle_pending: false,
                    escalate: false,
                    at_bottom: false,
                    streak_ticks: OPEN_AFTER_UP_TICKS
                },
                PaneScrollView::Native,
                -1,
            ),
            AgentViewAction::Toggle
        );
    }

    /// The headline: a DOWN gesture never opens the view, however long it runs.
    /// Opening on a down tick landed in the transcript at its bottom, which the
    /// next tick read as at-bottom and closed — an open/close flicker for a
    /// gesture that was only trying to scroll past the end of the live buffer.
    #[test]
    fn closed_and_scrolling_down_never_opens_it() {
        for mode in [PaneScrollView::Native, PaneScrollView::SpycHistory] {
            for ticks in [1, OPEN_AFTER_UP_TICKS, 50] {
                assert_eq!(
                    decide_agent_view_action(
                        AgentViewInputs {
                            is_open: false,
                            toggle_pending: false,
                            escalate: false,
                            at_bottom: false,
                            streak_ticks: ticks
                        },
                        mode,
                        1,
                    ),
                    AgentViewAction::Nothing,
                    "{mode:?} after {ticks} down ticks"
                );
            }
        }
    }

    /// The other half of the gate: a stray up tick or two — a trackpad jitter, a
    /// mis-flick — leaves the pane alone. Only a sustained gesture means it.
    #[test]
    fn closed_and_a_brief_scroll_up_does_not_open_it_yet() {
        for ticks in 1..OPEN_AFTER_UP_TICKS {
            assert_eq!(
                decide_agent_view_action(
                    AgentViewInputs {
                        is_open: false,
                        toggle_pending: false,
                        escalate: false,
                        at_bottom: false,
                        streak_ticks: ticks
                    },
                    PaneScrollView::Native,
                    -1,
                ),
                AgentViewAction::Nothing,
                "{ticks} up ticks"
            );
        }
    }

    /// A toggle already in flight (within TOGGLE_SETTLE) must NOT be re-sent —
    /// this is the guard against the open/close flicker a fast multi-tick flick
    /// would otherwise cause, since `^T` is a genuine toggle, not idempotent-open.
    #[test]
    fn closed_with_a_pending_toggle_does_nothing() {
        assert_eq!(
            decide_agent_view_action(
                AgentViewInputs {
                    is_open: false,
                    toggle_pending: true,
                    escalate: false,
                    at_bottom: false,
                    streak_ticks: OPEN_AFTER_UP_TICKS
                },
                PaneScrollView::Native,
                -1,
            ),
            AgentViewAction::Nothing
        );
    }

    /// `Off` never opens anything, pending or not — the second of the owner's
    /// three configurable choices ("have it do nothing").
    #[test]
    fn closed_and_off_does_nothing() {
        for pending in [false, true] {
            assert_eq!(
                decide_agent_view_action(
                    AgentViewInputs {
                        is_open: false,
                        toggle_pending: pending,
                        escalate: false,
                        at_bottom: false,
                        streak_ticks: OPEN_AFTER_UP_TICKS
                    },
                    PaneScrollView::Off,
                    -1,
                ),
                AgentViewAction::Nothing,
                "pending={pending}"
            );
        }
    }

    /// `SpycHistory` — the owner's stated personal preference — mounts spyc's OWN
    /// view instead of touching the agent's, regardless of any pending toggle
    /// (there is none to guard: this mode never sends one). Same up-gesture gate:
    /// mounting a pager over the pane is as disruptive as opening the agent's own.
    #[test]
    fn closed_and_spyc_history_uses_spycs_own_view() {
        for pending in [false, true] {
            assert_eq!(
                decide_agent_view_action(
                    AgentViewInputs {
                        is_open: false,
                        toggle_pending: pending,
                        escalate: false,
                        at_bottom: false,
                        streak_ticks: OPEN_AFTER_UP_TICKS
                    },
                    PaneScrollView::SpycHistory,
                    -1,
                ),
                AgentViewAction::UseSpycHistory,
                "pending={pending}"
            );
        }
    }

    /// Once open, the MODE stops mattering — all three converge on scrolling.
    /// Whatever opened it (auto or the user's own keyboard `^T`), the wheel must
    /// scroll it.
    #[test]
    fn open_always_scrolls_regardless_of_mode() {
        for mode in [
            PaneScrollView::Native,
            PaneScrollView::Off,
            PaneScrollView::SpycHistory,
        ] {
            assert_eq!(
                decide_agent_view_action(
                    AgentViewInputs {
                        is_open: true,
                        toggle_pending: false,
                        escalate: false,
                        at_bottom: false,
                        streak_ticks: 1
                    },
                    mode,
                    1,
                ),
                AgentViewAction::Scroll { fast: false },
                "{mode:?}"
            );
        }
    }

    /// The other headline: open + escalated sends the FAST (page) key — this is
    /// the fix for "scrolling is too slow in ^t mode".
    #[test]
    fn open_and_escalated_scrolls_fast() {
        assert_eq!(
            decide_agent_view_action(
                AgentViewInputs {
                    is_open: true,
                    toggle_pending: false,
                    escalate: true,
                    at_bottom: false,
                    streak_ticks: 1
                },
                PaneScrollView::Native,
                1,
            ),
            AgentViewAction::Scroll { fast: true }
        );
    }
    /// The headline: open, scrolling DOWN, already at the bottom -> close
    /// rather than a no-op scroll. Mode-independent, matching how `Scroll`
    /// already ignores mode once open.
    #[test]
    fn open_at_bottom_scrolling_down_closes() {
        for mode in [
            PaneScrollView::Native,
            PaneScrollView::Off,
            PaneScrollView::SpycHistory,
        ] {
            assert_eq!(
                decide_agent_view_action(
                    AgentViewInputs {
                        is_open: true,
                        toggle_pending: false,
                        escalate: false,
                        at_bottom: true,
                        streak_ticks: 1
                    },
                    mode,
                    1,
                ),
                AgentViewAction::Close,
                "{mode:?}"
            );
        }
    }

    /// Scrolling UP while at the bottom is just... scrolling up. `at_bottom`
    /// only means something for a DOWN gesture.
    #[test]
    fn at_bottom_scrolling_up_still_scrolls() {
        assert_eq!(
            decide_agent_view_action(
                AgentViewInputs {
                    is_open: true,
                    toggle_pending: false,
                    escalate: false,
                    at_bottom: true,
                    streak_ticks: 1
                },
                PaneScrollView::Native,
                -1,
            ),
            AgentViewAction::Scroll { fast: false }
        );
    }

    /// Not at the bottom: scrolling down is just... scrolling down, regardless
    /// of escalation.
    #[test]
    fn not_at_bottom_scrolling_down_still_scrolls() {
        for escalate in [false, true] {
            assert_eq!(
                decide_agent_view_action(
                    AgentViewInputs {
                        is_open: true,
                        toggle_pending: false,
                        escalate,
                        at_bottom: false,
                        streak_ticks: 1
                    },
                    PaneScrollView::Native,
                    1,
                ),
                AgentViewAction::Scroll { fast: escalate },
                "escalate={escalate}"
            );
        }
    }

    /// A close was just sent (still settling): a further down tick that still
    /// reads "open, at bottom" must NOT re-send close — this is the guard
    /// against sending `q` twice while codex's redraw is in flight, mirroring
    /// the open-side toggle-storm guard.
    #[test]
    fn a_pending_settle_suppresses_a_second_close() {
        assert_eq!(
            decide_agent_view_action(
                AgentViewInputs {
                    is_open: true,
                    toggle_pending: true,
                    escalate: false,
                    at_bottom: true,
                    streak_ticks: 1
                },
                PaneScrollView::Native,
                1,
            ),
            AgentViewAction::Scroll { fast: false },
            "must fall through to a harmless scroll key, not re-close"
        );
    }

    /// The regression, end to end over the two halves: a long DOWN flick against
    /// a closed transcript — the user trying to scroll past the bottom of the
    /// live buffer — leaves it closed for every tick. Before the gate the first
    /// tick opened it, the next closed it again, and the pane flashed.
    #[test]
    fn a_long_down_flick_never_opens_a_closed_view() {
        let t0 = std::time::Instant::now();
        let mut streak = None;
        for i in 0..20 {
            let (s, escalate) = scroll_streak_step(streak, 0, 1, later(t0, i * 30));
            streak = Some(s);
            assert_eq!(
                decide_agent_view_action(
                    AgentViewInputs {
                        is_open: false,
                        toggle_pending: false,
                        escalate,
                        at_bottom: false,
                        streak_ticks: s.ticks,
                    },
                    PaneScrollView::Native,
                    1,
                ),
                AgentViewAction::Nothing,
                "down tick {i}"
            );
        }
    }

    /// The complement: an UP flick opens it — but only once the gesture has run
    /// past the gate, not on its first tick.
    #[test]
    fn an_up_flick_opens_a_closed_view_once_past_the_gate() {
        let t0 = std::time::Instant::now();
        let mut streak = None;
        let mut opened_at = None;
        for i in 0..(u64::from(OPEN_AFTER_UP_TICKS) + 2) {
            let (s, escalate) = scroll_streak_step(streak, 0, -1, later(t0, i * 30));
            streak = Some(s);
            let action = decide_agent_view_action(
                AgentViewInputs {
                    is_open: false,
                    toggle_pending: false,
                    escalate,
                    at_bottom: false,
                    streak_ticks: s.ticks,
                },
                PaneScrollView::Native,
                -1,
            );
            if action == AgentViewAction::Toggle && opened_at.is_none() {
                opened_at = Some(s.ticks);
            }
        }
        assert_eq!(
            opened_at,
            Some(OPEN_AFTER_UP_TICKS),
            "opens on the gate's tick, no earlier and no later"
        );
    }

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
}

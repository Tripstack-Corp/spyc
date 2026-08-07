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

/// Advance a wheel-scroll streak by one tick, and decide whether it has run long
/// enough to escalate. Pure (the `route.rs`/`focus.rs` template): takes `now` as a
/// parameter rather than reading the clock, so it's testable with synthetic
/// `Instant`s built via `checked_add`/`checked_sub` off a real `Instant::now()`
/// baseline (the same trick `codex_pin`'s tests use).
///
/// Restarts (rather than extending) the streak on a tab switch, a direction
/// change, or a gap longer than [`STREAK_GAP`] — each of those means "a new
/// gesture", not "the same one continuing", so none should inherit the old
/// streak's elapsed time.
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
    let started_at = if restart {
        now
    } else {
        match prev {
            Some(s) => s.started_at,
            None => now, // unreachable: `prev.is_none()` implies `restart`
        }
    };
    let streak = PaneScrollStreak {
        tab_index,
        dir,
        started_at,
        last_at: now,
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
}

/// Pure.
///
/// `dir`: -1 up, +1 down. Only consulted once already open, to decide whether
/// "at the bottom" should mean "close" rather than "scroll" — scrolling UP at
/// the bottom is just normal scrolling.
pub const fn decide_agent_view_action(
    in_: AgentViewInputs,
    mode: crate::config::PaneScrollView,
    dir: i8,
) -> AgentViewAction {
    use crate::config::PaneScrollView as V;
    if !in_.is_open {
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

//! Pure key-event routing. Where does an incoming `KeyEvent` go?
//!
//! Before this module, every dispatch decision lived as an inline
//! guard inside `App::handle_key`. Five separate routing-shape bugs
//! shipped within a week (paste leak in `top_overlay` (#75); chord
//! swallowed in TopPane pager (#78); chord swallowed in LowerPane
//! pager (#80); exited-tab dropped on `^a` (#81); plus the original
//! V-key bug that motivated the `top_overlay` meta-escape in the
//! first place). The cleanup filed in v1.50.25 called for centralizing
//! these into one place — this is that place.
//!
//! The routing is a **pure function** of a small `RouteSnapshot` —
//! no `&App`, no side effects. That lets us unit-test every routing
//! decision without spinning up a TUI, and lets the test file double
//! as the regression matrix for the bugs we've already seen.
//!
//! `App::handle_key` builds a snapshot, calls `route_key`, and
//! dispatches by destination. The inline guards collapse into a
//! single `match`.

use crossterm::event::KeyEvent;

use crate::ui::pager::Mount;

use super::{App, Mode};

/// Which kind of input event is being routed. A key may be a meta-chord
/// (which escapes most content sinks to the resolver so `^a-j` works while
/// a pane/pager is focused); a **paste is always content** (never a
/// meta-chord), so it routes wherever a non-meta key would. The paste text
/// is passed separately to the dispatcher — the router only needs the
/// discriminant.
#[derive(Debug, Clone, Copy)]
pub(super) enum InputKind {
    Key(KeyEvent),
    Paste,
}

/// Where an incoming input event (key or paste) is dispatched. The
/// **modal** variants — a transient overlay swallowing all input — take
/// precedence over the **content** variants — the persistent region that
/// owns input. `route_input` returns this for both keys and paste, and
/// both `handle_key` and `handle_paste` dispatch on it via an *exhaustive*
/// match, so the two input kinds cannot drift: adding a variant is a build
/// error until both handle it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputSink {
    // ── modal sinks: eat ALL input, regardless of focus / meta-chord ──
    /// `F` fuzzy-finder picker is open (type-to-filter).
    FindPicker,
    /// A `!` capture child is running — forward input to its pty.
    Capture,
    /// A top-overlay subprocess has exited and is held awaiting any input
    /// to dismiss it.
    OverlayDismiss,
    /// Quick-select label overlay is open.
    QuickSelect,
    /// Harpoon menu is open.
    Harpoon,
    // ── content sinks: the focused region owns the input ──
    /// In-app prompt is active (`Mode::Prompting`); the key feeds
    /// the prompt's line editor.
    Prompt,
    /// File-list normal mode; the key drives the chord resolver
    /// (motions, marks, picks, `:`, etc.).
    Resolver,
    /// In-app pager has focus; the key drives scroll / search /
    /// pager-specific bindings.
    PagerKey,
    /// `V` (top-overlay editor) or `;` (top-overlay foreground
    /// command) pty owns the keystroke — encode and forward.
    OverlayPty,
    /// Bottom pty pane has keyboard focus; encode and forward to
    /// the subprocess (claude / codex / gemini / shell / …).
    BottomPane,
    /// Pane is in scrollback mode and a non-meta key drives the
    /// pane-scroll handler (j/k/G/etc.).
    PaneScroll,
    /// Exited pane tab — flash a hint, discard the key.
    PaneExitedFlash,
}

/// Pure snapshot of the App state bits the router needs. Built
/// from `&App` at call time. `Copy` so tests can construct one
/// directly without instantiating the whole App and can mutate
/// fields in update-syntax (`RouteSnapshot { foo: true, ..idle() }`).
#[derive(Debug, Clone, Copy)]
pub(super) struct RouteSnapshot {
    /// `F` fuzzy-finder picker is open (`runtime.find_picker`).
    pub has_find_picker: bool,
    /// A `!` capture child is running (`runtime.pending_capture`).
    pub has_capture: bool,
    /// A top-overlay subprocess has exited and awaits a dismiss keypress
    /// (`view.overlay_awaiting_dismiss`).
    pub overlay_awaiting_dismiss: bool,
    /// Quick-select label overlay is open (`view.quick_select`).
    pub has_quick_select: bool,
    /// Harpoon menu is open (`view.harpoon_menu`).
    pub has_harpoon: bool,
    /// `state.mode` is `Mode::Prompting(_)`.
    pub is_prompting: bool,
    /// `App::top_overlay.is_some()` — `V`/`;` overlay pty alive.
    pub has_top_overlay: bool,
    /// `view.pager`'s `Mount`, if any (top region: Overlay / TopPane).
    pub pager_mount: Option<Mount>,
    /// A bottom-region `^a v` scrollback (`view.scroll_pager`) is open.
    pub has_scroll_pager: bool,
    /// `App::pane_tabs.is_some()`.
    pub has_pane_tabs: bool,
    /// `state.pane_focused`.
    pub pane_focused: bool,
    /// Active pane is in scrollback (vt100 reverse-mode) mode.
    pub pane_scrolling: bool,
    /// Active pane's subprocess has exited.
    pub pane_closed: bool,
    /// Chord resolver is mid-sequence (`^a` seen, waiting on
    /// second key — likewise for `m{a-z}`, `'{a-z}`, etc.).
    pub resolver_pending: bool,
}

/// Decide where an input event goes. **Pure**, no mutation, no I/O.
///
/// A modal overlay (the finder, a running capture, a dismiss-awaiting
/// overlay, quick-select, harpoon) eats EVERY input kind regardless of
/// focus or meta-chord status, so those are checked first, in the same
/// precedence the historical `handle_key` pre-check ladder used. With no
/// modal active the content layer decides by focus + key-kind: a key may
/// be a meta-chord that escapes to the resolver, but a paste is always
/// content (`is_meta == false`) so it lands wherever a non-meta key would.
pub(super) const fn route_input(snap: RouteSnapshot, kind: InputKind) -> InputSink {
    // Modal layer — order mirrors the old pre-check ladder.
    if snap.has_find_picker {
        return InputSink::FindPicker;
    }
    if snap.has_capture {
        return InputSink::Capture;
    }
    if snap.overlay_awaiting_dismiss {
        return InputSink::OverlayDismiss;
    }
    if snap.has_quick_select {
        return InputSink::QuickSelect;
    }
    if snap.has_harpoon {
        return InputSink::Harpoon;
    }

    // Content layer.
    let is_meta = match kind {
        InputKind::Key(key) => super::is_spyc_meta_when_pane_focused(key, snap.resolver_pending),
        InputKind::Paste => false,
    };
    let bottom_owns = snap.has_pane_tabs && snap.pane_focused;

    // 1. Top-overlay pty (V editor, ; command). Meta chords and
    //    bottom-pane-focused keys fall through so the user can
    //    `^a-j` into claude while the editor stays visible above.
    if snap.has_top_overlay && !is_meta && !bottom_owns {
        return InputSink::OverlayPty;
    }

    // 2. Top-region pager (`view.pager`: Overlay or TopPane). Overlay is
    //    modal and always eats keys. A TopPane pager (`D`) coexists with the
    //    bottom: it yields non-meta keys to the bottom pane/scrollback when the
    //    bottom is focused (`bottom_typing`), and meta chords always escape to
    //    the resolver. `active_pager_mut!` resolves `PagerKey` to this pager
    //    (the top is focused, or it's a modal Overlay).
    if let Some(mount) = snap.pager_mount {
        let bottom_typing = matches!(mount, Mount::TopPane) && bottom_owns && !is_meta;
        let escape_meta = matches!(mount, Mount::TopPane) && is_meta;
        if !(bottom_typing || escape_meta) {
            return InputSink::PagerKey;
        }
        // else fall through to the scrollback / bottom-pane / resolver arms.
    }

    // 2b. Bottom-region scrollback (`view.scroll_pager`, `^a v`). Owns non-meta
    //     keys while the bottom pane is focused — coexisting with a top-region
    //     pager above (the symmetric half of `bottom_typing`). `active_pager_mut!`
    //     resolves `PagerKey` to the scrollback because the pane is focused.
    //     With the top focused instead, keys fall through to the file list so
    //     j/k navigate it while the scrollback stays visible (`^a-k` workflow);
    //     meta chords always escape.
    if snap.has_scroll_pager && bottom_owns && !is_meta {
        return InputSink::PagerKey;
    }

    // 3. Pane scrollback mode. Non-meta keys with the pane focused
    //    drive the scroll handler; meta keys escape to the resolver
    //    so pane commands (`^a-x`, focus switch) still work.
    if snap.has_pane_tabs && snap.pane_scrolling && snap.pane_focused && !is_meta {
        return InputSink::PaneScroll;
    }

    // 4. Exited pane tab. Non-meta keys flash a hint and are
    //    discarded; only meta chords (`^a-R`, `^a-x`, …) reach the
    //    resolver. Closes the v1.50.28 race where `^a` itself
    //    silently dropped the tab.
    if snap.has_pane_tabs && snap.pane_focused && snap.pane_closed && !is_meta {
        return InputSink::PaneExitedFlash;
    }

    // 5. Bottom pane forward — pty has focus, non-meta, and we're
    //    not in a prompt (typed prompt text doesn't leak into the
    //    pane).
    if bottom_owns && !is_meta && !snap.is_prompting {
        return InputSink::BottomPane;
    }

    // 6. Prompt — file-list area is the active region; a prompt is
    //    up so feed the line editor.
    if snap.is_prompting {
        return InputSink::Prompt;
    }

    // 7. Default: chord resolver / file-list navigation.
    InputSink::Resolver
}

impl App {
    /// Build the routing snapshot used by `route::route_input`.
    /// Pure read of the fields the router cares about.
    pub(super) fn route_snapshot(&self) -> RouteSnapshot {
        RouteSnapshot {
            has_find_picker: self.runtime.find_picker.is_some(),
            has_capture: self.runtime.pending_capture.is_some(),
            overlay_awaiting_dismiss: self.view.overlay_awaiting_dismiss,
            has_quick_select: self.view.quick_select.is_some(),
            has_harpoon: self.view.harpoon_menu.is_some(),
            is_prompting: matches!(self.state.mode, Mode::Prompting(_)),
            has_top_overlay: self.runtime.top_overlay.is_some(),
            pager_mount: self.view.pager.as_ref().map(|v| v.mount),
            has_scroll_pager: self.view.scroll_pager.is_some(),
            has_pane_tabs: self.runtime.pane_tabs.is_some(),
            pane_focused: self.state.pane_focused(),
            // MVU Phase 5: read from the Model snapshot (refreshed at
            // loop-top), not the live host — decouples routing from Runtime.
            pane_scrolling: self.state.pane.pane_snapshot.is_scrolling,
            pane_closed: self.state.pane.pane_snapshot.is_closed,
            resolver_pending: self.state.resolver.is_pending(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// Snapshot with every flag at its quiescent default.
    fn idle() -> RouteSnapshot {
        RouteSnapshot {
            has_find_picker: false,
            has_capture: false,
            overlay_awaiting_dismiss: false,
            has_quick_select: false,
            has_harpoon: false,
            is_prompting: false,
            has_top_overlay: false,
            pager_mount: None,
            has_scroll_pager: false,
            has_pane_tabs: false,
            pane_focused: false,
            pane_scrolling: false,
            pane_closed: false,
            resolver_pending: false,
        }
    }

    /// Test shim: the historical key-only router. Lets the regression
    /// matrix below stay verbatim while production routes both kinds
    /// through `route_input`.
    fn route_key(snap: RouteSnapshot, key: KeyEvent) -> InputSink {
        route_input(snap, InputKind::Key(key))
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    // ── happy paths ───────────────────────────────────────────────

    #[test]
    fn default_routes_to_resolver() {
        assert_eq!(route_key(idle(), key('j')), InputSink::Resolver);
    }

    #[test]
    fn prompting_routes_to_prompt() {
        let snap = RouteSnapshot {
            is_prompting: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('a')), InputSink::Prompt);
    }

    #[test]
    fn focused_pane_routes_to_bottom_pane() {
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('q')), InputSink::BottomPane);
    }

    #[test]
    fn focused_pane_meta_routes_to_resolver_for_chord() {
        // ^a is the chord prefix; even though the pane is focused,
        // it must reach the resolver to start the chord.
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    #[test]
    fn resolver_pending_continuation_treated_as_meta() {
        // After `^a`, `j` arrives — must reach the resolver to
        // complete the chord, not be forwarded to the pane.
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            resolver_pending: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::Resolver);
    }

    // ── pager: overlay mount eats everything ──────────────────────

    #[test]
    fn overlay_pager_eats_all_keys() {
        let snap = RouteSnapshot {
            pager_mount: Some(Mount::Overlay),
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::PagerKey);
        assert_eq!(route_key(snap, ctrl('a')), InputSink::PagerKey);
    }

    // ── regression: TopPane pager + meta chord (#78) ──────────────

    #[test]
    fn top_pane_pager_meta_escapes_to_resolver() {
        // D opens an in-app pager in the top slot with a bottom pane
        // visible. `^a` must reach the resolver so `^a-j` works.
        let snap = RouteSnapshot {
            pager_mount: Some(Mount::TopPane),
            has_pane_tabs: true,
            pane_focused: false, // pager has focus
            ..idle()
        };
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    #[test]
    fn top_pane_pager_non_meta_with_pager_focus_goes_to_pager() {
        let snap = RouteSnapshot {
            pager_mount: Some(Mount::TopPane),
            has_pane_tabs: true,
            pane_focused: false,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::PagerKey);
    }

    #[test]
    fn top_pane_pager_with_bottom_focus_routes_typing_to_pane() {
        // D pager up, but the user has focused claude with `^a-j`.
        // Non-meta keys should flow to claude, not the pager.
        let snap = RouteSnapshot {
            pager_mount: Some(Mount::TopPane),
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('q')), InputSink::BottomPane);
        // And `^a-k` still works to switch focus back.
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    // ── regression: LowerPane pager + meta chord (#80) ────────────

    #[test]
    fn lower_pane_pager_meta_escapes_to_resolver() {
        // ^a-v opens the pane-scrollback pager in the lower slot.
        // `^a-k` must reach the resolver so the user can focus the
        // file list.
        let snap = RouteSnapshot {
            has_scroll_pager: true,
            has_pane_tabs: true,
            pane_focused: true,   // pane is the underlying owner
            pane_scrolling: true, // entered scroll mode
            ..idle()
        };
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    #[test]
    fn lower_pane_pager_non_meta_with_pane_focus_goes_to_pager() {
        // LowerPane visually replaces the bottom pty; non-meta keys
        // (scroll, search, etc.) belong to the pager when the bottom
        // surface is focused.
        let snap = RouteSnapshot {
            has_scroll_pager: true,
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::PagerKey);
        assert_eq!(route_key(snap, key('/')), InputSink::PagerKey);
    }

    #[test]
    fn lower_pane_pager_non_meta_with_top_focus_flows_to_top() {
        // After `^a-k` from a `^a-v` scrollback pager, focus is on
        // the file list while the pager stays open. Non-meta keys
        // should now navigate the file list, not the scrollback —
        // symmetric to how a TopPane pager lets keys through to the
        // bottom pty when the bottom is focused.
        let snap = RouteSnapshot {
            has_scroll_pager: true,
            has_pane_tabs: true,
            pane_focused: false, // ^a-k flipped focus to top
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::Resolver);
        assert_eq!(route_key(snap, key('k')), InputSink::Resolver);
        // Meta keys still reach the resolver too.
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    /// Coexistence: a `D` TopPane pager up top AND a `^a v` scrollback below at
    /// the same time (the two-slot fix). Non-meta keys route to a pager either
    /// way — `active_pager_mut!` picks the focused region's pager — so neither
    /// evicts the other. Meta chords escape to the resolver.
    #[test]
    fn top_pager_and_scrollback_coexist_route_by_focus() {
        let bottom = RouteSnapshot {
            pager_mount: Some(Mount::TopPane),
            has_scroll_pager: true,
            has_pane_tabs: true,
            pane_focused: true, // bottom scrollback owns input
            ..idle()
        };
        assert_eq!(route_key(bottom, key('j')), InputSink::PagerKey);
        assert_eq!(route_key(bottom, ctrl('a')), InputSink::Resolver);

        let top = RouteSnapshot {
            pane_focused: false, // top `D` pager owns input
            ..bottom
        };
        assert_eq!(route_key(top, key('j')), InputSink::PagerKey);
        assert_eq!(route_key(top, ctrl('a')), InputSink::Resolver);
    }

    // ── regression: V/D top_overlay + paste / chord (#75 + V) ─────

    #[test]
    fn top_overlay_keeps_non_meta_when_bottom_not_focused() {
        // V editor is open; user types into it (j key, no chord).
        // Goes to the overlay pty, NOT the bottom pane.
        let snap = RouteSnapshot {
            has_top_overlay: true,
            has_pane_tabs: true,
            pane_focused: false,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::OverlayPty);
    }

    #[test]
    fn top_overlay_meta_chord_escapes_to_resolver() {
        // V editor is open; user wants to focus claude with `^a-j`.
        let snap = RouteSnapshot {
            has_top_overlay: true,
            has_pane_tabs: true,
            pane_focused: false,
            ..idle()
        };
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    #[test]
    fn top_overlay_with_bottom_focus_routes_typing_to_pane() {
        // V editor up, but user focused claude. Typing goes to claude.
        let snap = RouteSnapshot {
            has_top_overlay: true,
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('q')), InputSink::BottomPane);
    }

    // ── regression: exited tab + non-meta key (#81) ───────────────

    #[test]
    fn exited_pane_non_meta_flashes() {
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            pane_closed: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::PaneExitedFlash);
        assert_eq!(
            route_key(snap, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            InputSink::PaneExitedFlash,
        );
    }

    #[test]
    fn exited_pane_meta_chord_reaches_resolver() {
        // `^a-R` and `^a-x` must work on an exited tab.
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            pane_closed: true,
            ..idle()
        };
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    // ── pane scroll mode ──────────────────────────────────────────

    #[test]
    fn pane_scroll_eats_non_meta_keys() {
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            pane_scrolling: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::PaneScroll);
        assert_eq!(route_key(snap, key('G')), InputSink::PaneScroll);
    }

    #[test]
    fn pane_scroll_meta_chord_escapes() {
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            pane_scrolling: true,
            ..idle()
        };
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Resolver);
    }

    // ── prompts win over panes ────────────────────────────────────

    #[test]
    fn prompt_wins_over_focused_pane() {
        // User opened `:` while in the pane. Pane should NOT receive
        // keys — they go to the prompt.
        let snap = RouteSnapshot {
            is_prompting: true,
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('q')), InputSink::Prompt);
    }

    // A `Mount::Overlay` pager eats keys REGARDLESS of pane focus — both a
    // normal key and a meta chord go to the pager even with a focused pane
    // present. The existing `overlay_pager_eats_all_keys` uses bare `idle()`
    // (no pane, unfocused), leaving the coexistence case open; this pins it
    // ahead of the MVU `Focus::Pager(Overlay)` phase. Behavior is unchanged
    // (route_key is untouched) — this is a regression guard, not a new rule.
    #[test]
    fn overlay_pager_eats_all_keys_even_with_pane_focus() {
        let snap = RouteSnapshot {
            pager_mount: Some(Mount::Overlay),
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::PagerKey);
        assert_eq!(route_key(snap, ctrl('a')), InputSink::PagerKey);
    }

    // ── modal layer: eats every kind, regardless of focus / meta ──────
    //
    // A modal overlay swallows ALL input — including meta chords — and
    // takes precedence over the content layer. Each modal flag routes a
    // plain key, a `^a` meta chord, AND a paste to its sink, even with a
    // focused pane present.

    #[test]
    fn find_picker_eats_all_input() {
        let snap = RouteSnapshot {
            has_find_picker: true,
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::FindPicker);
        assert_eq!(route_key(snap, ctrl('a')), InputSink::FindPicker);
        assert_eq!(route_input(snap, InputKind::Paste), InputSink::FindPicker);
    }

    #[test]
    fn capture_eats_all_input() {
        let snap = RouteSnapshot {
            has_capture: true,
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::Capture);
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Capture);
        assert_eq!(route_input(snap, InputKind::Paste), InputSink::Capture);
    }

    #[test]
    fn overlay_dismiss_eats_all_input() {
        let snap = RouteSnapshot {
            overlay_awaiting_dismiss: true,
            has_top_overlay: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), InputSink::OverlayDismiss);
        assert_eq!(route_key(snap, ctrl('a')), InputSink::OverlayDismiss);
        assert_eq!(
            route_input(snap, InputKind::Paste),
            InputSink::OverlayDismiss
        );
    }

    #[test]
    fn quick_select_eats_all_input() {
        let snap = RouteSnapshot {
            has_quick_select: true,
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('a')), InputSink::QuickSelect);
        assert_eq!(route_key(snap, ctrl('a')), InputSink::QuickSelect);
        assert_eq!(route_input(snap, InputKind::Paste), InputSink::QuickSelect);
    }

    #[test]
    fn harpoon_eats_all_input() {
        let snap = RouteSnapshot {
            has_harpoon: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('1')), InputSink::Harpoon);
        assert_eq!(route_key(snap, ctrl('a')), InputSink::Harpoon);
        assert_eq!(route_input(snap, InputKind::Paste), InputSink::Harpoon);
    }

    /// Modal precedence: with several modal flags set at once, the order
    /// is find_picker > capture > dismiss > quick_select > harpoon —
    /// mirroring the historical `handle_key` pre-check ladder.
    #[test]
    fn modal_precedence_order() {
        let all = RouteSnapshot {
            has_find_picker: true,
            has_capture: true,
            overlay_awaiting_dismiss: true,
            has_quick_select: true,
            has_harpoon: true,
            ..idle()
        };
        assert_eq!(route_key(all, key('j')), InputSink::FindPicker);
        let no_finder = RouteSnapshot {
            has_find_picker: false,
            ..all
        };
        assert_eq!(route_key(no_finder, key('j')), InputSink::Capture);
        let no_capture = RouteSnapshot {
            has_capture: false,
            ..no_finder
        };
        assert_eq!(route_key(no_capture, key('j')), InputSink::OverlayDismiss);
        let no_dismiss = RouteSnapshot {
            overlay_awaiting_dismiss: false,
            ..no_capture
        };
        assert_eq!(route_key(no_dismiss, key('j')), InputSink::QuickSelect);
        let only_harpoon = RouteSnapshot {
            has_quick_select: false,
            ..no_dismiss
        };
        assert_eq!(route_key(only_harpoon, key('j')), InputSink::Harpoon);
    }

    /// The unifying invariant: a paste lands wherever a non-meta printable
    /// key would. Sweep a representative snapshot matrix and assert
    /// `route_input(snap, Paste) == route_input(snap, Key(non_meta))` for
    /// every combination — the executable form of "paste == content key", and
    /// the guard that keeps `handle_paste` and `handle_key` from drifting.
    #[test]
    fn paste_agrees_with_non_meta_key() {
        let mounts = [
            None,
            Some(Mount::Overlay),
            Some(Mount::TopPane),
            Some(Mount::LowerPane),
        ];
        let plain = key('x'); // non-meta printable
        // Sweep every combination of the 12 boolean snapshot bits crossed
        // with each pager mount. A flat bit-decode beats a 12-deep `for`
        // pyramid: bit `i` of `bits` drives the i-th field. `resolver_pending`
        // is held false on purpose: with a chord pending, EVERY key (incl.
        // `x`) is meta and escapes to the resolver, while a paste is never
        // meta — so "paste == non-meta key" only holds when no chord pends.
        let on = |bits: u32, i: u32| bits & (1 << i) != 0;
        for &pager_mount in &mounts {
            for bits in 0..(1u32 << 12) {
                let snap = RouteSnapshot {
                    has_find_picker: on(bits, 0),
                    has_capture: on(bits, 1),
                    overlay_awaiting_dismiss: on(bits, 2),
                    has_quick_select: on(bits, 3),
                    has_harpoon: on(bits, 4),
                    is_prompting: on(bits, 5),
                    has_top_overlay: on(bits, 6),
                    pager_mount,
                    has_scroll_pager: on(bits, 7),
                    has_pane_tabs: on(bits, 8),
                    pane_focused: on(bits, 9),
                    pane_scrolling: on(bits, 10),
                    pane_closed: on(bits, 11),
                    resolver_pending: false,
                };
                assert_eq!(
                    route_input(snap, InputKind::Paste),
                    route_input(snap, InputKind::Key(plain)),
                    "paste must agree with a non-meta key for {snap:?}"
                );
            }
        }
    }
}

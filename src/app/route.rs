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

/// Where an incoming key event should be dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyDestination {
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

/// Decide where a key goes. **Pure**, no mutation, no I/O.
pub(super) const fn route_key(snap: RouteSnapshot, key: KeyEvent) -> KeyDestination {
    let is_meta = super::is_spyc_meta_when_pane_focused(key, snap.resolver_pending);
    let bottom_owns = snap.has_pane_tabs && snap.pane_focused;

    // 1. Top-overlay pty (V editor, ; command). Meta chords and
    //    bottom-pane-focused keys fall through so the user can
    //    `^a-j` into claude while the editor stays visible above.
    if snap.has_top_overlay && !is_meta && !bottom_owns {
        return KeyDestination::OverlayPty;
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
            return KeyDestination::PagerKey;
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
        return KeyDestination::PagerKey;
    }

    // 3. Pane scrollback mode. Non-meta keys with the pane focused
    //    drive the scroll handler; meta keys escape to the resolver
    //    so pane commands (`^a-x`, focus switch) still work.
    if snap.has_pane_tabs && snap.pane_scrolling && snap.pane_focused && !is_meta {
        return KeyDestination::PaneScroll;
    }

    // 4. Exited pane tab. Non-meta keys flash a hint and are
    //    discarded; only meta chords (`^a-R`, `^a-x`, …) reach the
    //    resolver. Closes the v1.50.28 race where `^a` itself
    //    silently dropped the tab.
    if snap.has_pane_tabs && snap.pane_focused && snap.pane_closed && !is_meta {
        return KeyDestination::PaneExitedFlash;
    }

    // 5. Bottom pane forward — pty has focus, non-meta, and we're
    //    not in a prompt (typed prompt text doesn't leak into the
    //    pane).
    if bottom_owns && !is_meta && !snap.is_prompting {
        return KeyDestination::BottomPane;
    }

    // 6. Prompt — file-list area is the active region; a prompt is
    //    up so feed the line editor.
    if snap.is_prompting {
        return KeyDestination::Prompt;
    }

    // 7. Default: chord resolver / file-list navigation.
    KeyDestination::Resolver
}

impl App {
    /// Build the routing snapshot used by `route::route_key`.
    /// Pure read of the fields the router cares about.
    pub(super) fn route_snapshot(&self) -> RouteSnapshot {
        RouteSnapshot {
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

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    // ── happy paths ───────────────────────────────────────────────

    #[test]
    fn default_routes_to_resolver() {
        assert_eq!(route_key(idle(), key('j')), KeyDestination::Resolver);
    }

    #[test]
    fn prompting_routes_to_prompt() {
        let snap = RouteSnapshot {
            is_prompting: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('a')), KeyDestination::Prompt);
    }

    #[test]
    fn focused_pane_routes_to_bottom_pane() {
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            ..idle()
        };
        assert_eq!(route_key(snap, key('q')), KeyDestination::BottomPane);
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
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(snap, key('j')), KeyDestination::Resolver);
    }

    // ── pager: overlay mount eats everything ──────────────────────

    #[test]
    fn overlay_pager_eats_all_keys() {
        let snap = RouteSnapshot {
            pager_mount: Some(Mount::Overlay),
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), KeyDestination::PagerKey);
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::PagerKey);
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
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
    }

    #[test]
    fn top_pane_pager_non_meta_with_pager_focus_goes_to_pager() {
        let snap = RouteSnapshot {
            pager_mount: Some(Mount::TopPane),
            has_pane_tabs: true,
            pane_focused: false,
            ..idle()
        };
        assert_eq!(route_key(snap, key('j')), KeyDestination::PagerKey);
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
        assert_eq!(route_key(snap, key('q')), KeyDestination::BottomPane);
        // And `^a-k` still works to switch focus back.
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(snap, key('j')), KeyDestination::PagerKey);
        assert_eq!(route_key(snap, key('/')), KeyDestination::PagerKey);
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
        assert_eq!(route_key(snap, key('j')), KeyDestination::Resolver);
        assert_eq!(route_key(snap, key('k')), KeyDestination::Resolver);
        // Meta keys still reach the resolver too.
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(bottom, key('j')), KeyDestination::PagerKey);
        assert_eq!(route_key(bottom, ctrl('a')), KeyDestination::Resolver);

        let top = RouteSnapshot {
            pane_focused: false, // top `D` pager owns input
            ..bottom
        };
        assert_eq!(route_key(top, key('j')), KeyDestination::PagerKey);
        assert_eq!(route_key(top, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(snap, key('j')), KeyDestination::OverlayPty);
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
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(snap, key('q')), KeyDestination::BottomPane);
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
        assert_eq!(route_key(snap, key('j')), KeyDestination::PaneExitedFlash);
        assert_eq!(
            route_key(snap, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            KeyDestination::PaneExitedFlash,
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
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(snap, key('j')), KeyDestination::PaneScroll);
        assert_eq!(route_key(snap, key('G')), KeyDestination::PaneScroll);
    }

    #[test]
    fn pane_scroll_meta_chord_escapes() {
        let snap = RouteSnapshot {
            has_pane_tabs: true,
            pane_focused: true,
            pane_scrolling: true,
            ..idle()
        };
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::Resolver);
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
        assert_eq!(route_key(snap, key('q')), KeyDestination::Prompt);
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
        assert_eq!(route_key(snap, key('j')), KeyDestination::PagerKey);
        assert_eq!(route_key(snap, ctrl('a')), KeyDestination::PagerKey);
    }
}

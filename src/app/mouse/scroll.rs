//! Wheel scrolling for a pane whose child ignores mouse reports: translate a
//! wheel tick into the agent's own verified scroll keybinding, escalate a
//! sustained streak to page-wise keys, and drive codex's `^T` transcript
//! overlay. Extracted verbatim from `mouse/mod.rs`.
//!
//! The escalation exists because the agent, not spyc, is the rate limiter: a
//! long scroll sending one cursor-key per tick is slower than the user can
//! flick, so a streak past `ESCALATE_AFTER` switches to page keys.

use super::super::Effect;
use super::route::{
    AgentViewAction, AgentViewInputs, PendingViewIntent, TOGGLE_SETTLE, decide_agent_view_action,
    pending_view_confirmed, scroll_streak_step,
};

impl super::super::App {
    /// The active pane agent's verified scroll keybinding, if it has one.
    pub(super) fn active_pane_wheel_scroll(&self) -> Option<crate::agent::WheelScroll> {
        let tabs = self.runtime.pane_tabs.as_ref()?;
        crate::agent::detect(&tabs.active_info().command).wheel_scroll()
    }

    /// Translate a wheel tick into the child's own scroll keys.
    ///
    /// Agents with no dedicated, toggleable view (today: agy — its
    /// `transcript_open_marker` is `None`) keep the exact behaviour verified
    /// working: `wheel_scroll()`'s key, repeated `pane_scroll_lines` times, no
    /// screen-scraping at all. An agent that opts into
    /// `transcript_open_marker` (codex) gets the fuller machinery in
    /// `send_agent_view_scroll_keys` — gated auto-open, and escalation to a page
    /// key under a sustained gesture.
    pub(super) fn send_scroll_keys(&mut self, delta: i32) -> Vec<Effect> {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return Vec::new();
        };
        let profile = crate::agent::detect(&tabs.active_info().command);
        if let Some(marker) = profile.transcript_open_marker() {
            let dir: i8 = if delta < 0 { -1 } else { 1 };
            return self.send_agent_view_scroll_keys(profile, marker, dir);
        }
        let Some(scroll) = profile.wheel_scroll() else {
            return Vec::new();
        };
        let (code, mods) = if delta < 0 { scroll.up } else { scroll.down };
        let app_cursor = tabs.active().application_cursor();
        // The PANE's own step, not `scroll_lines`. Those are different jobs: the list
        // and pagers want 1 line per wheel event, because a trackpad already emits
        // one event per notional line (owner-confirmed: "the file list speed is
        // great"). But here spyc is driving somebody ELSE's pager by synthesizing
        // arrows, and that pager moves one line per key with no safe way to ask it
        // for a page — so at 1 the wheel couldn't traverse a long history.
        Self::repeat_key_effect(
            code,
            mods,
            self.state.config.mouse.pane_scroll_lines.max(1),
            app_cursor,
        )
    }

    /// The fuller wheel-to-keys machinery for an agent with its OWN toggleable
    /// scrollback view (today: codex's `^T`), gated on `marker` — see
    /// `AgentProfile::transcript_open_marker`.
    ///
    /// First decides whether the view is open by scraping the pane's CURRENT
    /// visible screen (`Pane::visible_lines` — the viewport, not scrollback:
    /// codex's own vt100 scrollback is confirmed empty, per #230's
    /// investigation, so scrollback has nothing to scrape). Cheap enough to run
    /// every tick — a plain substring search over one screen's worth of text —
    /// so no debounce is needed for the scrape itself; only the toggle-send
    /// needs one (see `pane_view_sent`'s doc).
    ///
    /// Opening it takes a sustained scroll UP (`OPEN_AFTER_UP_TICKS`); a
    /// downward gesture never opens it, only scrolls or closes one already open.
    pub(super) fn send_agent_view_scroll_keys(
        &mut self,
        profile: &'static dyn crate::agent::AgentProfile,
        marker: &str,
        dir: i8,
    ) -> Vec<Effect> {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return Vec::new();
        };
        let tab_index = tabs.active_index();
        // One scrape, reused for both checks below — codex's own vt100
        // scrollback is confirmed empty (#230), so this reads the viewport, not
        // scrollback, and there is no cheaper way to ask "is X still true" than
        // reading the same lines twice.
        let visible = tabs.active().visible_lines();
        let app_cursor = tabs.active().application_cursor();
        let is_open = visible.iter().any(|l| l.contains(marker));
        let at_bottom = is_open && profile.transcript_at_bottom(&visible);
        let pending = self.view.pane_view_sent;
        let toggle_pending = pending.is_some_and(|(sent, _)| sent.elapsed() < TOGGLE_SETTLE);

        let now = std::time::Instant::now();
        // Stepped on every tick, open or closed: while open its elapsed time
        // drives the page-key escalation, and while closed its tick count is what
        // separates a deliberate scroll-up-into-history from one stray tick.
        let (streak, escalate) =
            scroll_streak_step(self.view.pane_scroll_streak, tab_index, dir, now);

        let action = decide_agent_view_action(
            AgentViewInputs {
                is_open,
                toggle_pending,
                escalate,
                at_bottom,
                streak_ticks: streak.ticks,
            },
            self.state.config.mouse.pane_scroll_view,
            dir,
        );

        // State mutation lives here, once, keyed to the decision — not scattered
        // across the branches that produce it.
        self.view.pane_scroll_streak = Some(streak);
        // Retire the guard only when the screen shows what was actually asked for.
        // Testing `is_open` alone retires a pending CLOSE on the stale marker the
        // guard exists to ride out, which turns one close into one close key per
        // wheel tick — `q` each time, into codex's composer.
        if let Some((_, intent)) = pending
            && pending_view_confirmed(intent, is_open)
        {
            self.view.pane_view_sent = None;
        }

        match action {
            AgentViewAction::Nothing => Vec::new(),
            AgentViewAction::UseSpycHistory => {
                self.open_pane_scroll_pager();
                Vec::new()
            }
            AgentViewAction::Toggle => {
                let Some((code, mods)) = profile.transcript_toggle_key() else {
                    return Vec::new();
                };
                self.view.pane_view_sent = Some((now, PendingViewIntent::Open));
                Self::repeat_key_effect(code, mods, 1, app_cursor)
            }
            // Shares the debounce field with `Toggle`, but records the opposite
            // intent: for the next tick or two the scrape still reads
            // `is_open == true` (codex hasn't redrawn the composer yet), so the
            // guard must survive that stale read or a fast flick past the bottom
            // sends `q` again into what is by then the composer's text input.
            AgentViewAction::Close => {
                let Some((code, mods)) = profile.transcript_close_key() else {
                    return Vec::new();
                };
                self.view.pane_view_sent = Some((now, PendingViewIntent::Close));
                Self::repeat_key_effect(code, mods, 1, app_cursor)
            }
            AgentViewAction::Scroll { fast } => {
                if fast && let Some(f) = profile.fast_wheel_scroll() {
                    let (code, mods) = if dir < 0 { f.up } else { f.down };
                    return Self::repeat_key_effect(code, mods, 1, app_cursor);
                }
                let Some(scroll) = profile.wheel_scroll() else {
                    return Vec::new();
                };
                let (code, mods) = if dir < 0 { scroll.up } else { scroll.down };
                Self::repeat_key_effect(
                    code,
                    mods,
                    self.state.config.mouse.pane_scroll_lines.max(1),
                    app_cursor,
                )
            }
        }
    }

    /// Encode `key` and repeat it `n` times as ONE `SendToPane` batch — the
    /// executor writes to the pty once, so a multi-line tick can't interleave
    /// with the child's own output mid-burst.
    ///
    /// `app_cursor` is the target pane's DECCKM state: these are synthesized
    /// presses of the child's OWN scroll binding, so they need the same arrow
    /// form its typed keys get (`Pane::send_key`'s path) or the pager they drive
    /// ignores them.
    fn repeat_key_effect(
        code: crossterm::event::KeyCode,
        mods: crossterm::event::KeyModifiers,
        n: usize,
        app_cursor: bool,
    ) -> Vec<Effect> {
        let per_press =
            crate::pane::input::encode_key(crossterm::event::KeyEvent::new(code, mods), app_cursor);
        if per_press.is_empty() {
            return Vec::new();
        }
        let mut bytes = Vec::with_capacity(per_press.len() * n.max(1));
        for _ in 0..n.max(1) {
            bytes.extend_from_slice(&per_press);
        }
        vec![Effect::SendToPane {
            target: super::super::effect::PaneTarget::Active,
            input: super::super::effect::PaneInput::Bytes(bytes),
            on_ok: None,
            // No per-tick flash, and no early return on a dead pty: either would
            // bury the real exit message under one repeat per wheel line.
            err_prefix: None,
        }]
    }
}

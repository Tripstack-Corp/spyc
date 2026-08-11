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

#[cfg(test)]
mod tests {
    use super::super::super::{App, Effect, PaneInput};
    use std::time::{Duration, Instant};

    /// Every byte the effects would write to the pty.
    fn bytes(fx: &[Effect]) -> Vec<u8> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::SendToPane {
                    input: PaneInput::Bytes(b),
                    ..
                } => Some(b.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// A pane that looks to spyc like codex with its `^T` transcript open, at the
    /// bottom of it.
    ///
    /// The screen content is what `send_agent_view_scroll_keys` scrapes, so it is
    /// written through the pty and echoed back rather than injected — the marker
    /// and the `100%` footer arrive as codex's own would. `cat` is the child; only
    /// the command string decides which profile answers.
    fn codex_pane_showing_the_transcript(dir: &std::path::Path) -> App {
        let mut app = App::test_app(dir.to_path_buf());
        app.view.term_size = (120, 24);
        app.state.config.mouse.pane_scroll_view = crate::config::PaneScrollView::Native;
        app.open_pane_tab("cat");
        let tabs = app.runtime.pane_tabs.as_mut().expect("a pane tab");
        tabs.active_entry_mut().info.command = "codex".to_string();
        tabs.active_mut()
            .send_bytes("T R A N S C R I P T\n\u{2500}\u{2500} 100% \u{2500}\u{2500}\n".as_bytes())
            .expect("write to the pty");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            tabs.active_mut().drain_output();
            let visible = tabs.active().visible_lines();
            if visible.iter().any(|l| l.contains("T R A N S C R I P T"))
                && visible
                    .iter()
                    .rev()
                    .find(|l| l.contains('\u{2500}'))
                    .is_some_and(|l| l.contains(" 100% "))
            {
                return app;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the pane never showed the transcript marker and footer");
    }

    /// **One flick past the bottom closes the view once.** The scrape still reads
    /// "open" for a tick or two after the close key lands — codex hasn't redrawn
    /// the composer yet — so without the pending guard riding out that stale read
    /// every remaining tick of the flick sends another `q`, straight into the
    /// composer's text input as literal typing.
    ///
    /// Drives `send_agent_view_scroll_keys` itself. The route-level test of the
    /// same name re-implements this loop, so it stays green when the caller is
    /// wrong; this one doesn't.
    #[test]
    fn one_flick_past_the_bottom_sends_the_close_key_once_through_the_caller() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = codex_pane_showing_the_transcript(tmp.path());
            let profile = crate::agent::detect("codex");
            let marker = profile
                .transcript_open_marker()
                .expect("codex marks its transcript");

            // Five downward ticks, the shape of one trackpad flick.
            let closes = (0..5)
                .filter(|_| {
                    let fx = app.send_agent_view_scroll_keys(profile, marker, 1);
                    bytes(&fx).contains(&b'q')
                })
                .count();
            assert_eq!(
                closes, 1,
                "the close key went out {closes}× on one flick — each extra one is a `q` typed into codex's composer"
            );
        });
    }

    /// A downward tick with the view open but NOT at the bottom scrolls it, using
    /// codex's own binding — it must not close on the way down.
    #[test]
    fn a_tick_inside_the_transcript_scrolls_instead_of_closing() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.view.term_size = (120, 24);
            app.state.config.mouse.pane_scroll_view = crate::config::PaneScrollView::Native;
            app.open_pane_tab("cat");
            let tabs = app.runtime.pane_tabs.as_mut().expect("a pane tab");
            tabs.active_entry_mut().info.command = "codex".to_string();
            // Marker present, no `100%` footer: open, mid-history.
            tabs.active_mut()
                .send_bytes(
                    "T R A N S C R I P T\n\u{2500}\u{2500} 40% \u{2500}\u{2500}\n".as_bytes(),
                )
                .expect("write to the pty");
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                tabs.active_mut().drain_output();
                if tabs
                    .active()
                    .visible_lines()
                    .iter()
                    .any(|l| l.contains("T R A N S C R I P T"))
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }

            let profile = crate::agent::detect("codex");
            let marker = profile.transcript_open_marker().expect("marked");
            let out = bytes(&app.send_agent_view_scroll_keys(profile, marker, 1));
            assert!(!out.is_empty(), "a tick mid-transcript has to do something");
            assert!(
                !out.contains(&b'q'),
                "and it must not be the close key: {out:?}"
            );
        });
    }

    /// With no pane there is nothing to scroll — and nothing to panic on.
    #[test]
    fn a_tick_with_no_pane_does_nothing() {
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            assert!(app.send_scroll_keys(1).is_empty());
        });
    }
}

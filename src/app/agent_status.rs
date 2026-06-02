//! MVU Phase 6: the active pane's status-line agent short-id, resolved OFF the
//! render thread.
//!
//! Extracted from `app/mod.rs` (same descendant-module `impl App` pattern as
//! `streaming` / `commands` — reads App's private state directly). Keeps the
//! off-thread machinery + the `~/.claude/sessions/*.json` walk dispatch out of
//! the loop file. `resolve_short_id` is a per-file JSON walk that scales with
//! the user's accumulated sessions, so it must never run on the render thread
//! (it once showed ~65% of main-thread CPU). Pattern mirrors
//! `TabEntry::live_cwd` (#227): a landing slot + in-flight flag + a detached
//! worker that wakes the loop on completion; render reads the cache and never
//! blocks.

use super::{AGENT_STATUS_TTL, AgentKind, AgentStatusCache, App, Message};

impl App {
    pub(crate) fn active_agent_status(&self) -> Option<String> {
        // Extract everything needed from the active pane up front — a `'static`
        // profile ref (the registry lives forever) plus the spawn-time key —
        // then drop the `pane_tabs` borrow before touching the cache / slot.
        let (profile, kind, label, cwd, spawn) = {
            let active = self.pane_tabs.as_ref()?.active_info();
            let profile = crate::agent::detect(&active.command);
            let kind = profile.kind();
            if kind == AgentKind::Other {
                return None;
            }
            (
                profile,
                kind,
                profile.name(),
                active.cwd.clone(),
                active.spawn_epoch_secs,
            )
        };

        // The landed background result is drained + applied by
        // `apply_landed_agent_status` in the PRE-RECV SCAN, not here — the scan
        // runs every loop iteration, whereas this fn is skipped on the
        // overlay / top-pager render paths (render_inner early-returns before
        // the status bar). Draining only here would leave the slot full on
        // those paths and the scan's redraw nudge would busy-spin. So here we
        // only read the (scan-applied) cache and kick a refresh.

        // Is the cached short-id for the CURRENT active pane, and still fresh?
        let cache_matches = self
            .agent_status_cache
            .as_ref()
            .is_some_and(|c| c.kind == kind && c.cwd == cwd && c.spawn_epoch_secs == spawn);
        let fresh = cache_matches
            && self
                .agent_status_cache
                .as_ref()
                .is_some_and(|c| c.computed_at.elapsed() < AGENT_STATUS_TTL);

        // Kick a background refresh when stale / missing / for a different
        // pane, with none already in flight. The resolver scans every
        // `~/.claude/sessions/*.json` (sample once showed ~65% of main-thread
        // CPU here on a long-running user) — so it runs OFF this render thread;
        // the result lands in `agent_status_pending` and is applied above on a
        // later frame. Same off-thread pattern as `TabEntry::live_cwd` (#227).
        if !fresh
            && !self
                .agent_status_refreshing
                .load(std::sync::atomic::Ordering::Acquire)
        {
            self.agent_status_refreshing
                .store(true, std::sync::atomic::Ordering::Release);
            let pending = std::sync::Arc::clone(&self.agent_status_pending);
            let refreshing = std::sync::Arc::clone(&self.agent_status_refreshing);
            // Clone of the unified-channel sender so the worker can WAKE the
            // loop on completion (None before `run()` / in the test harness →
            // no wake, which is correct: those paths don't render in a loop).
            let wake = self.pane_wake_tx.clone();
            let thread_cwd = cwd;
            std::thread::spawn(move || {
                let short_id = profile.resolve_short_id(&thread_cwd, spawn);
                let status = Some(match short_id {
                    Some(id) => format!("{label}:{id}"),
                    None => label.to_string(),
                });
                *pending.lock().unwrap() = Some(AgentStatusCache {
                    computed_at: std::time::Instant::now(),
                    kind,
                    cwd: thread_cwd,
                    spawn_epoch_secs: spawn,
                    status,
                });
                refreshing.store(false, std::sync::atomic::Ordering::Release);
                // Wake AFTER the result + flag are stored, so the woken pre-recv
                // scan sees `agent_status_pending` populated and forces a redraw.
                if let Some(tx) = wake {
                    let _ = tx.send(Message::AgentStatusReady);
                }
            });
        }

        // Show this pane's cached short-id. Until the first refresh for a
        // freshly-focused pane lands, fall back to the bare agent label (no
        // short-id yet) — never block, never show another pane's id. A
        // same-pane 30 s refresh updates the cache in place, so the steady
        // state never flickers.
        if cache_matches {
            self.agent_status_cache
                .as_ref()
                .and_then(|c| c.status.clone())
        } else {
            Some(label.to_string())
        }
    }

    /// MVU Phase 6: drain a landed off-thread agent-status result into the
    /// cache. Called from the PRE-RECV SCAN every loop iteration, so the
    /// landing slot is ALWAYS emptied — regardless of which render path runs
    /// (the status bar, hence `active_agent_status`, is skipped while an
    /// overlay / top-pager is open; draining only there would leave the slot
    /// full and the scan's redraw nudge would busy-spin). Applies the result
    /// only if it's for the CURRENT active pane — a late result for a
    /// since-switched pane is discarded, never clobbering the active cache.
    /// Returns whether the cache changed (the caller sets `needs_draw`).
    pub(crate) fn apply_landed_agent_status(&mut self) -> bool {
        // Bind the `take()` first so the MutexGuard drops before the body.
        let landed = self.agent_status_pending.lock().unwrap().take();
        let Some(result) = landed else {
            return false;
        };
        let matches = self.pane_tabs.as_ref().is_some_and(|tabs| {
            let active = tabs.active_info();
            result.kind == crate::agent::detect(&active.command).kind()
                && result.cwd == active.cwd
                && result.spawn_epoch_secs == active.spawn_epoch_secs
        });
        if matches {
            self.agent_status_cache = Some(result);
        }
        matches
    }
}

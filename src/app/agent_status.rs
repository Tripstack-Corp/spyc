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
    /// PURE `&self` read for the draw pass: the active pane's cached agent
    /// short-id, or the bare agent label until the first refresh lands. Never
    /// spawns, never mutates — the staleness check + off-thread refresh kick
    /// live in `kick_agent_status_refresh`, called from the pre-recv scan (the
    /// `&mut` settle point) next to `apply_landed_agent_status`. Keeping this
    /// pure restores the "render mutates nothing" contract: a TestBackend
    /// snapshot render no longer silently spawns a worker that walks
    /// `~/.claude/sessions`.
    pub(crate) fn active_agent_status(&self) -> Option<String> {
        let (profile, kind, cwd, spawn) = self.active_agent_key()?;
        // Show this pane's cached short-id. Until the first refresh for a
        // freshly-focused pane lands, fall back to the bare agent label (no
        // short-id yet) — never block, never show another pane's id. A
        // same-pane 30 s refresh updates the cache in place, so the steady
        // state never flickers.
        if self.agent_cache_matches(kind, &cwd, spawn) {
            self.view
                .agent_status_cache
                .as_ref()
                .and_then(|c| c.status.clone())
        } else {
            Some(profile.name().to_string())
        }
    }

    /// The active pane's agent identity for status resolution: the `'static`
    /// profile ref (the registry lives forever) plus the spawn-time cache key.
    /// `None` when there's no active pane or it isn't a known agent. The
    /// `pane_tabs` borrow is dropped at return (cwd is cloned), so callers can
    /// freely touch the cache / runtime slots afterward.
    fn active_agent_key(
        &self,
    ) -> Option<(
        &'static dyn crate::agent::AgentProfile,
        AgentKind,
        std::path::PathBuf,
        u64,
    )> {
        let active = self.runtime.pane_tabs.as_ref()?.active_info();
        let profile = crate::agent::detect(&active.command);
        let kind = profile.kind();
        if kind == AgentKind::Other {
            return None;
        }
        Some((profile, kind, active.cwd.clone(), active.spawn_epoch_secs))
    }

    /// Is the cached short-id for the CURRENT active pane?
    fn agent_cache_matches(&self, kind: AgentKind, cwd: &std::path::Path, spawn: u64) -> bool {
        self.view
            .agent_status_cache
            .as_ref()
            .is_some_and(|c| c.kind == kind && c.cwd == cwd && c.spawn_epoch_secs == spawn)
    }

    /// MVU Phase 6 / render-purity: kick an off-thread agent-status refresh when
    /// the cache is stale / missing / for a different pane, with none already in
    /// flight. Called from the PRE-RECV SCAN (a `&mut` settle point), NOT from
    /// the `&self` draw pass — this is the half of the old `active_agent_status`
    /// that spawned a thread and stored a Runtime atomic, which the draw must
    /// not do. Co-located with `apply_landed_agent_status` (the scan also owns
    /// the landing-slot drain), so the kick fires regardless of which render
    /// path runs and the two halves no longer need to explain a split.
    ///
    /// The resolver scans every `~/.claude/sessions/*.json` (a sample once
    /// showed ~65% of main-thread CPU here on a long-running user), so it runs
    /// OFF-thread; the result lands in `agent_status_pending` and is applied by
    /// the scan on a later frame. Same off-thread pattern as `live_cwd` (#227).
    //
    // `&mut self` is deliberate even though the stores go through interior
    // mutability (Arc<AtomicBool>): it is the structural guarantee that the
    // `&self` draw pass CANNOT call this — the whole point of moving the spawn
    // off render. Hence the allow.
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub(crate) fn kick_agent_status_refresh(&mut self) {
        let Some((profile, kind, cwd, spawn)) = self.active_agent_key() else {
            return;
        };
        let fresh = self.agent_cache_matches(kind, &cwd, spawn)
            && self
                .view
                .agent_status_cache
                .as_ref()
                .is_some_and(|c| c.computed_at.elapsed() < AGENT_STATUS_TTL);
        if fresh
            || self
                .runtime
                .agent_status_refreshing
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        self.runtime
            .agent_status_refreshing
            .store(true, std::sync::atomic::Ordering::Release);
        let label = profile.name();
        let pending = std::sync::Arc::clone(&self.runtime.agent_status_pending);
        let refreshing = std::sync::Arc::clone(&self.runtime.agent_status_refreshing);
        // Clone of the unified-channel sender so the worker can WAKE the loop on
        // completion (None before `run()` / in the test harness → no wake, which
        // is correct: those paths don't render in a loop).
        let wake = self.runtime.pane_wake_tx.clone();
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
        let landed = self.runtime.agent_status_pending.lock().unwrap().take();
        let Some(result) = landed else {
            return false;
        };
        let matches = self.runtime.pane_tabs.as_ref().is_some_and(|tabs| {
            let active = tabs.active_info();
            result.kind == crate::agent::detect(&active.command).kind()
                && result.cwd == active.cwd
                && result.spawn_epoch_secs == active.spawn_epoch_secs
        });
        if matches {
            self.view.agent_status_cache = Some(result);
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentKind, AgentStatusCache, App};
    use std::path::{Path, PathBuf};

    fn cache(kind: AgentKind, cwd: &Path, spawn: u64) -> AgentStatusCache {
        AgentStatusCache {
            computed_at: std::time::Instant::now(),
            kind,
            cwd: cwd.to_path_buf(),
            spawn_epoch_secs: spawn,
            status: Some("claude:abcd".to_string()),
        }
    }

    #[test]
    fn agent_cache_matches_requires_kind_cwd_and_spawn() {
        let cwd = PathBuf::from("/tmp/proj");
        let mut app = App::test_app(cwd.clone());
        app.view.agent_status_cache = Some(cache(AgentKind::Claude, &cwd, 42));
        assert!(app.agent_cache_matches(AgentKind::Claude, &cwd, 42));
        // Any of the three key components differing → no match.
        assert!(!app.agent_cache_matches(AgentKind::Codex, &cwd, 42));
        assert!(!app.agent_cache_matches(AgentKind::Claude, Path::new("/tmp/other"), 42));
        assert!(!app.agent_cache_matches(AgentKind::Claude, &cwd, 99));
    }

    #[test]
    fn agent_cache_matches_false_when_cache_empty() {
        let app = App::test_app(PathBuf::from("/tmp/proj"));
        assert!(!app.agent_cache_matches(AgentKind::Claude, Path::new("/tmp/proj"), 0));
    }

    #[test]
    fn no_active_pane_yields_none_and_kick_is_a_noop() {
        // The pure draw read returns None with no agent pane, and the kick
        // (now the sole spawn site, off the render path) never flips the
        // in-flight flag or spawns when there's nothing to resolve.
        let mut app = App::test_app(PathBuf::from("/tmp/proj"));
        assert_eq!(app.active_agent_status(), None);
        app.kick_agent_status_refresh();
        assert!(
            !app.runtime
                .agent_status_refreshing
                .load(std::sync::atomic::Ordering::Acquire)
        );
    }
}

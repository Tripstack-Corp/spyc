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

use std::time::{Duration, Instant};

use super::{
    AGENT_STATUS_TTL, AgentKind, AgentStatusCache, App, Deadline, Effect, Message, RunCtx,
    VisualBell,
};
use crate::config::{DesktopVia, NotifyConfig};
use crate::pane::{AgentActivity, ReportedStatus};

/// How long after a tab's last pane output it still counts as `Working`. Long
/// enough to bridge the sub-second gaps between streamed token chunks; short
/// enough that "the agent stopped" reads as `Idle` promptly. A silent thinking
/// pause longer than this reads as `Idle` — true working-through-silence needs
/// the P1 semantic hook (`docs/AGENT_AWARENESS_PLAN.md`), not output timing.
const AGENT_ACTIVE_WINDOW: Duration = Duration::from_secs(2);

/// Cadence of the "spicy pulse" working animation (~4 Hz). Armed only while
/// ≥1 agent tab is `Working`, so an all-idle pane set still draws 0 fps.
const AGENT_ANIM_INTERVAL: Duration = Duration::from_millis(250);

/// Whether pane output at `last_output_at` supersedes a live report (the agent
/// resumed → drop the report, fall back to timing).
///
/// A `Blocked` report is **latched**: output never supersedes it. Blocked is the
/// "needs me" signal raised at a prompt the agent is waiting on (Claude's
/// `PermissionRequest` hook firing as a Yes/No prompt opens), and that prompt
/// keeps redrawing itself while it waits — those redraws are pane output, so any
/// output-based supersede (even behind a grace window) eventually bounces the
/// dot off red and back to the working pulse. Blocked is cleared only by its
/// TTL, a newer report, or the user actually answering the pane (the
/// `SendToPane` handler in `run_effects` drops it on the user's keystroke).
/// Every other status is superseded by any output after it.
fn report_superseded_by_output(r: ReportedStatus, last_output_at: Option<Instant>) -> bool {
    if r.status == AgentActivity::Blocked {
        return false;
    }
    last_output_at.is_some_and(|o| o > r.at)
}

/// Total lifetime of a P3-1 visual-bell border pulse before it clears.
const VISUAL_BELL_DURATION: Duration = Duration::from_millis(480);
/// Cadence of the pulse's gradient sweep (~16 Hz). Armed only while a flash is
/// live, so an idle pane never wakes for it.
const VISUAL_BELL_FRAME: Duration = Duration::from_millis(60);

/// What a status transition should trigger, after `[notify]` gating +
/// focused-tab suppression. The pure output of [`notification_for_transition`]:
/// `system` is `Some((summary, body))` when the OS notifier fires; `osc9` is
/// `Some(message)` when the terminal-escape channel fires; `bell`/`visual` when
/// those fire. All-off = nothing to do.
#[derive(Debug, Default, PartialEq, Eq)]
struct NotifyPlan {
    system: Option<(String, String)>,
    osc9: Option<String>,
    bell: bool,
    visual: bool,
}

impl NotifyPlan {
    /// No channel fired — the caller emits nothing.
    const fn is_silent(&self) -> bool {
        self.system.is_none() && self.osc9.is_none() && !self.bell && !self.visual
    }
}

/// Which desktop-notification mechanisms fire for `via`, given whether spyc is
/// running over SSH. Returns `(system, osc9)`. `Auto` routes to OSC-9 over SSH
/// (so the ping reaches the *client* terminal, not the remote box) and the OS
/// notifier locally — the "just works over SSH" default. Pure.
const fn desktop_delivery(via: DesktopVia, is_ssh: bool) -> (bool, bool) {
    match via {
        DesktopVia::Auto => {
            if is_ssh {
                (false, true)
            } else {
                (true, false)
            }
        }
        DesktopVia::System => (true, false),
        DesktopVia::Osc9 => (false, true),
        DesktopVia::Both => (true, true),
    }
}

/// Decide what a tab's `old → new` activity transition should trigger, given the
/// `[notify]` config, whether this tab is the one the user is actively watching
/// (its pane owns the keyboard), and whether spyc is over SSH. Pure + testable.
///
/// Only a *fresh* transition INTO `Blocked` ("needs me") or `Done` (a finished
/// turn) fires anything; a repeat or any other target is silent, as is a focused
/// tab when `suppress_focused_tab`. Beyond that, each channel decides *which*
/// edge it fires on: `Blocked` fires every enabled channel, but the routine
/// `Done` edge (once per turn) fires a channel only if it opts in via its
/// `*_done` flag — `desktop_done` for the ping, `bell_done` for the bell,
/// `visual_done` for the flash. The default splits by intrusiveness: the quiet
/// desktop ping notifies on `Done`, the interrupting bell + on-screen flash stay
/// `Blocked`-only so they don't fire per turn. The desktop mechanism(s) come from
/// [`desktop_delivery`]. Returns a silent plan when no channel fires.
fn notification_for_transition(
    old: AgentActivity,
    new: AgentActivity,
    cfg: &NotifyConfig,
    tab_focused: bool,
    is_ssh: bool,
    label: &str,
    tab_number: usize,
) -> NotifyPlan {
    // A fresh edge into a notify-worthy state; `is_done` distinguishes the
    // routine finished-turn edge (per-channel opt-in) from `Blocked` (fires all).
    let is_done = match new {
        AgentActivity::Blocked if old != AgentActivity::Blocked => false,
        AgentActivity::Done if old != AgentActivity::Done => true,
        _ => return NotifyPlan::default(),
    };
    if cfg.suppress_focused_tab && tab_focused {
        return NotifyPlan::default();
    }
    // On a `Done` edge each channel needs its own `*_done` opt-in; `Blocked`
    // (`!is_done`) fires every enabled channel.
    let want_desktop = cfg.desktop && (!is_done || cfg.desktop_done);
    let want_bell = cfg.bell && (!is_done || cfg.bell_done);
    let want_visual = cfg.visual && (!is_done || cfg.visual_done);

    let (want_system, want_osc9) = if want_desktop {
        desktop_delivery(cfg.desktop_via, is_ssh)
    } else {
        (false, false)
    };
    let (system, osc9) = if want_system || want_osc9 {
        let (summary, body) = notification_text(label, tab_number, new);
        // OSC-9 takes one line; fold the two parts into it.
        let osc9 = want_osc9.then(|| format!("{summary} — {body}"));
        let system = want_system.then_some((summary, body));
        (system, osc9)
    } else {
        (None, None)
    };
    NotifyPlan {
        system,
        osc9,
        bell: want_bell,
        visual: want_visual,
    }
}

/// Compose the `(summary, body)` for a transition notification: a glanceable
/// "which agent + what" summary plus the tab locator in the body. Only
/// `Blocked` / `Done` reach here (see [`notification_for_transition`]); the
/// fallback arm is a sane non-panicking default.
fn notification_text(label: &str, tab_number: usize, new: AgentActivity) -> (String, String) {
    match new {
        AgentActivity::Blocked => (
            format!("{label} needs you"),
            format!("tab {tab_number} is blocked"),
        ),
        AgentActivity::Done => (
            format!("{label} finished"),
            format!("tab {tab_number} — done"),
        ),
        _ => (label.to_string(), format!("tab {tab_number}")),
    }
}

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

    /// Pure derivation of a tab's [`AgentActivity`] from output timing — the
    /// testable core of [`Self::settle_agent_activity`]. A non-agent tab is
    /// `Unknown` (no dot); an agent tab is `Working` while its last output is
    /// within [`AGENT_ACTIVE_WINDOW`], else `Idle` (including before any output).
    fn activity_for(
        is_agent: bool,
        last_output_at: Option<Instant>,
        now: Instant,
    ) -> AgentActivity {
        if !is_agent {
            return AgentActivity::Unknown;
        }
        match last_output_at {
            Some(at) if now.saturating_duration_since(at) < AGENT_ACTIVE_WINDOW => {
                AgentActivity::Working
            }
            _ => AgentActivity::Idle,
        }
    }

    /// The authority resolution (P1, testable core): a live semantic
    /// [`ReportedStatus`] wins over the output-timing fallback. A report is
    /// *live* until it expires or the tab produces output **after** it (the
    /// agent resumed → timing takes back over). When no report is live, fall
    /// back to [`Self::activity_for`]. Non-agent tabs are always `Unknown`
    /// (a report targeting one is ignored — dots are agent-only).
    fn effective_activity(
        reported: Option<ReportedStatus>,
        is_agent: bool,
        last_output_at: Option<Instant>,
        now: Instant,
    ) -> AgentActivity {
        if !is_agent {
            return AgentActivity::Unknown;
        }
        if let Some(r) = reported {
            // `Blocked` is latched: no TTL expiry and output never supersedes it
            // (`report_superseded_by_output` returns false), so it holds until the
            // user answers the pane (Enter, in `run_effects`) or a newer report
            // lands. Every other status expires or yields to later output.
            let expired = r.status != AgentActivity::Blocked && now >= r.expiry;
            let superseded = report_superseded_by_output(r, last_output_at);
            if !expired && !superseded {
                return r.status;
            }
        }
        Self::activity_for(is_agent, last_output_at, now)
    }

    /// Agent-activity (P0): derive each agent tab's [`AgentActivity`] from the
    /// `last_output_at` that `drain_pane_output` stamps, advance the spicy
    /// pulse, and arm the two activity deadlines. Returns whether the frame
    /// changed (the caller marks a redraw).
    ///
    /// `&mut` settle point (pre-recv) — NOT the pure draw — because it reads the
    /// clock (`now`) and re-arms timers. A tab counts as `Working` while its
    /// last output is within [`AGENT_ACTIVE_WINDOW`]; `AgentIdle` is armed at the
    /// earliest pending flip so the loop wakes to drop it to `Idle`, and
    /// `AgentAnim` ticks the pulse while any tab is Working. When no tab is
    /// Working both are disarmed, so an all-idle pane set draws 0 fps (the idle
    /// invariant). Non-agent tabs are left `Unknown` (no dot).
    pub(crate) fn settle_agent_activity(
        &mut self,
        now: Instant,
        ctx: &mut RunCtx,
    ) -> (bool, Vec<Effect>) {
        // Snapshot the anim epoch + the notify inputs up front so the `pane_tabs`
        // mutable borrow below doesn't overlap the `self.view` / `self.state`
        // reads. `pane_focused` + the active tab index drive the P3-1
        // focused-tab suppression; the config is cloned (5 bools, cheap).
        let anim_epoch = self.view.started_at;
        let pane_focused = self.state.pane_focused();
        let is_ssh = self.view.is_ssh;
        let notify_cfg = self.state.config.notify.clone();
        // Only track `agent_status` transitions when a Lua handler wants the
        // event (the common case has none → no per-tab bookkeeping). Collect
        // during the borrow, fire after it drops (fire needs `&mut self`).
        let track_status = self.lua_wants_agent_status_event();
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            ctx.scheduler.disarm(Deadline::AgentIdle);
            ctx.scheduler.disarm(Deadline::AgentAnim);
            if track_status {
                // No tabs → prune every stale baseline.
                self.prune_agent_status_baselines(&std::collections::HashSet::new());
            }
            return (false, Vec::new());
        };
        let active_idx = tabs.active_index();

        let mut changed = false;
        let mut effects: Vec<Effect> = Vec::new();
        let mut start_visual = false;
        let mut earliest_flip: Option<Instant> = None;
        let mut any_working = false;
        let mut status_transitions: Vec<(String, AgentActivity)> = Vec::new();
        let mut live_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
            let is_agent = crate::agent::detect(&entry.info.command).kind() != AgentKind::Other;
            // Drop a report that's no longer authoritative (expired, or the tab
            // resumed output after it) so state + `:why-status` stay honest. A
            // `Blocked` report is latched — it never expires here and output
            // never supersedes it (see `effective_activity`); only the Enter-on-
            // pane clear in `run_effects` or a newer report drops it.
            if let Some(r) = entry.info.reported
                && ((r.status != AgentActivity::Blocked && now >= r.expiry)
                    || report_superseded_by_output(r, entry.info.last_output_at))
            {
                entry.info.reported = None;
            }
            let new = Self::effective_activity(
                entry.info.reported,
                is_agent,
                entry.info.last_output_at,
                now,
            );
            // Wake-arming: re-evaluate at the earliest of a live report's expiry
            // or a timing-Working tab's idle-flip, so the dot can't go stale. A
            // latched `Blocked` report never expires, so don't arm a wake for it
            // (nothing would flip; an Enter keystroke clears it via its own path).
            if let Some(r) = entry.info.reported {
                if r.status != AgentActivity::Blocked {
                    earliest_flip =
                        Some(earliest_flip.map_or(r.expiry, |m: Instant| m.min(r.expiry)));
                }
            } else if new == AgentActivity::Working
                && let Some(at) = entry.info.last_output_at
            {
                let flip = at + AGENT_ACTIVE_WINDOW;
                earliest_flip = Some(earliest_flip.map_or(flip, |m: Instant| m.min(flip)));
            }
            if new == AgentActivity::Working {
                any_working = true;
            }
            // P3-1 notification edge: compare `new` against `notified` (this
            // settle owns it), NOT `activity` — the MCP `report_status` handler
            // pre-sets `activity` for an instant dot, which would otherwise erase
            // the Idle→Blocked edge before we see it (→ no ping). Decide against
            // the old `notified`, then advance it.
            if new != entry.info.notified {
                let tab_focused = i == active_idx && pane_focused;
                let plan = notification_for_transition(
                    entry.info.notified,
                    new,
                    &notify_cfg,
                    tab_focused,
                    is_ssh,
                    &entry.info.label,
                    i + 1,
                );
                if !plan.is_silent() {
                    if plan.system.is_some() || plan.osc9.is_some() || plan.bell {
                        effects.push(Effect::Notify {
                            system: plan.system,
                            osc9: plan.osc9,
                            bell: plan.bell,
                        });
                    }
                    start_visual |= plan.visual;
                }
                entry.info.notified = new;
            }
            // Render field (the dot): the MCP handler may have already advanced it
            // for an instant update; keep it in sync here either way.
            if entry.info.activity != new {
                entry.info.activity = new;
                changed = true;
            }
            // Record this tab's effective status for the `agent_status` event
            // (dedup + firing happen after the borrow). Keyed by the stable
            // `SPYC_PANE_ID` so the event survives tab reorder.
            if track_status {
                live_ids.insert(entry.info.id.clone());
                status_transitions.push((entry.info.id.clone(), new));
            }
        }

        // Fire `agent_status` for tabs whose SEMANTIC status transitioned (the
        // per-tab baseline in `fire_agent_status_event` dedups a repeat, so an
        // output tick / anim frame that leaves the status the same fires
        // nothing), then drop baselines for closed tabs.
        if track_status {
            for (id, status) in status_transitions {
                self.fire_agent_status_event(&id, status);
            }
            self.prune_agent_status_baselines(&live_ids);
        }

        // Wake to flip Working→Idle at the earliest pending boundary.
        match earliest_flip {
            Some(t) => ctx.scheduler.arm(Deadline::AgentIdle, t),
            None => ctx.scheduler.disarm(Deadline::AgentIdle),
        }
        // Advance + keep ticking the pulse while anything works; freeze otherwise.
        if any_working {
            let frame = u64::try_from(
                now.saturating_duration_since(anim_epoch).as_millis()
                    / AGENT_ANIM_INTERVAL.as_millis(),
            )
            .unwrap_or(0);
            if frame != self.view.agent_anim_frame {
                self.view.agent_anim_frame = frame;
                changed = true;
            }
            ctx.scheduler
                .arm(Deadline::AgentAnim, now + AGENT_ANIM_INTERVAL);
        } else {
            ctx.scheduler.disarm(Deadline::AgentAnim);
        }
        // P3-1 visual bell: a Blocked/Done transition asked for a flash (and
        // `[notify].visual` is on) → start (or restart) the spice-heat border
        // pulse and arm its sweep tick. `settle_visual_bell` advances + clears it.
        if start_visual {
            self.view.visual_bell = Some(VisualBell {
                start: now,
                frame: 0,
            });
            ctx.scheduler
                .arm(Deadline::VisualBell, now + VISUAL_BELL_FRAME);
            changed = true;
        }
        (changed, effects)
    }

    /// Advance / decay the P3-1 visual-bell border pulse (a `&mut` settle point,
    /// PRE-recv — the pure draw can't read the clock). Clears the flash once
    /// [`VISUAL_BELL_DURATION`] elapses (disarming the tick, so an idle pane
    /// returns to 0 dps), else advances the sweep `frame` and re-arms. Returns
    /// whether the frame changed (the caller marks a redraw). A no-op — and a
    /// defensive disarm — when no flash is active.
    pub(crate) fn settle_visual_bell(&mut self, now: Instant, ctx: &mut RunCtx) -> bool {
        let Some(bell) = self.view.visual_bell else {
            ctx.scheduler.disarm(Deadline::VisualBell);
            return false;
        };
        if now.saturating_duration_since(bell.start) >= VISUAL_BELL_DURATION {
            self.view.visual_bell = None;
            ctx.scheduler.disarm(Deadline::VisualBell);
            return true;
        }
        let elapsed_ms = now.saturating_duration_since(bell.start).as_millis();
        let frame = u64::try_from(elapsed_ms / VISUAL_BELL_FRAME.as_millis()).unwrap_or(0);
        let changed = frame != bell.frame;
        if changed {
            self.view.visual_bell = Some(VisualBell {
                start: bell.start,
                frame,
            });
        }
        ctx.scheduler
            .arm(Deadline::VisualBell, now + VISUAL_BELL_FRAME);
        changed
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

    // Agent-activity (P0): the requirement behind the dot — Working only while
    // output is within the active window, Idle once quiet, Unknown (no dot) for
    // a non-agent tab. Tests the pure derivation directly (no RunCtx needed).
    #[test]
    fn activity_for_tracks_the_output_window() {
        use crate::pane::AgentActivity;
        use std::time::{Duration, Instant};
        // `base` is the output time; advance `now` past it by adding (Instant +
        // Duration is allowed; `Instant - Duration` trips the no-unchecked-time
        // lint and could underflow near process start).
        let base = Instant::now();
        // A non-agent tab never gets a dot, even with fresh output.
        assert_eq!(
            App::activity_for(false, Some(base), base),
            AgentActivity::Unknown
        );
        // An agent tab with no output yet is Idle (alive but quiet), not Unknown.
        assert_eq!(App::activity_for(true, None, base), AgentActivity::Idle);
        // Output at/within the window → Working.
        assert_eq!(
            App::activity_for(true, Some(base), base),
            AgentActivity::Working
        );
        assert_eq!(
            App::activity_for(true, Some(base), base + Duration::from_millis(500)),
            AgentActivity::Working
        );
        // Output older than the window → Idle (the "agent stopped" signal).
        assert_eq!(
            App::activity_for(true, Some(base), base + Duration::from_millis(2500)),
            AgentActivity::Idle
        );
    }

    // P1 authority: a live semantic report wins over output timing; an expired
    // or output-superseded report falls back to timing; a non-agent ignores it.
    #[test]
    fn effective_activity_honors_live_reports_then_falls_back() {
        use crate::pane::{AgentActivity, ReportedStatus};
        use std::time::{Duration, Instant};
        let base = Instant::now();
        let report = |status, at, expiry| Some(ReportedStatus { status, at, expiry });

        // A live `working` report wins even with NO output (silent thinking) —
        // the headline win over the timing-only P0 dot.
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Working, base, base + Duration::from_secs(60)),
                true,
                None,
                base,
            ),
            AgentActivity::Working
        );
        // A live `blocked` report → Blocked (the "needs me" signal).
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Blocked, base, base + Duration::from_secs(60)),
                true,
                Some(base),
                base + Duration::from_secs(1),
            ),
            AgentActivity::Blocked
        );
        // A non-blocked report past its TTL → falls back to timing (no recent
        // output → Idle). Blocked is exempt from TTL — see the latch test.
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Working, base, base + Duration::from_secs(1)),
                true,
                None,
                base + Duration::from_secs(2),
            ),
            AgentActivity::Idle
        );
        // Output AFTER the report (agent resumed) supersedes it → timing wins.
        let later = base + Duration::from_millis(500);
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Idle, base, base + Duration::from_secs(60)),
                true,
                Some(later),
                later,
            ),
            AgentActivity::Working
        );
        // A non-agent tab ignores any report (dots are agent-only).
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Working, base, base + Duration::from_secs(60)),
                false,
                None,
                base,
            ),
            AgentActivity::Unknown
        );
    }

    // A `blocked` report is LATCHED: neither output NOR its TTL supersedes it —
    // not the prompt's initial render, not the menu's later redraws while it
    // waits, and not the expiry clock (the bug where any of those bounced the
    // dot off red and back to the working pulse). It clears only via a newer
    // report or the user answering the pane with Enter (handled in
    // `run_effects`). A non-blocked report still hands back to timing on output.
    #[test]
    fn blocked_report_is_latched_against_output() {
        use crate::pane::{AgentActivity, ReportedStatus};
        use std::time::{Duration, Instant};
        let base = Instant::now();
        let expiry = base + Duration::from_secs(300);
        let blocked = Some(ReportedStatus {
            status: AgentActivity::Blocked,
            at: base,
            expiry,
        });

        // Output long after the report (the prompt still redrawing while it
        // waits) does NOT supersede — the dot stays Blocked.
        let later = base + Duration::from_secs(30);
        assert_eq!(
            App::effective_activity(blocked, true, Some(later), later),
            AgentActivity::Blocked,
            "blocked is latched — prompt redraws must not bounce it to working"
        );

        // Its TTL does NOT end it either — a blocked report well past `expiry`
        // still reads Blocked (latched until the user answers with Enter). This
        // is the "stuck until <ENTER>" behaviour: no clock revives the dot.
        let stale = Some(ReportedStatus {
            status: AgentActivity::Blocked,
            at: base,
            expiry: base + Duration::from_secs(1),
        });
        assert_eq!(
            App::effective_activity(stale, true, None, base + Duration::from_secs(600)),
            AgentActivity::Blocked,
            "blocked has no TTL — it stays until answered"
        );

        // Contrast: a non-blocked report (done) is still superseded by later
        // output (unchanged behavior).
        let done = Some(ReportedStatus {
            status: AgentActivity::Done,
            at: base,
            expiry,
        });
        assert_eq!(
            App::effective_activity(
                done,
                true,
                Some(base + Duration::from_millis(200)),
                base + Duration::from_millis(300),
            ),
            AgentActivity::Working,
            "done has no latch — output supersedes it"
        );
    }

    // P3-1: the pure transition → notification decision (the core of the ping).
    // `false` = not the focused tab; `false` = not over SSH (so `auto` → system).
    #[test]
    fn notify_fires_on_blocked_and_done_transitions() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Done, Idle, Working};
        let cfg = NotifyConfig::default(); // desktop(auto) + desktop_done + visual on; bell off.

        // Idle → Blocked on a background tab, local → system "needs me" ping.
        let p = super::notification_for_transition(Idle, Blocked, &cfg, false, false, "codex", 2);
        let (summary, body) = p.system.clone().expect("blocked pings the OS notifier");
        assert!(summary.contains("codex"), "summary names the agent");
        assert!(body.contains('2'), "body carries the tab locator");
        assert!(p.osc9.is_none(), "auto+local doesn't use OSC-9");
        assert!(!p.bell, "the terminal bell is off by default");
        assert!(
            p.visual,
            "the spice-heat visual flash fires (on by default)"
        );

        // Working → Done also pings (desktop_done on by default).
        let p = super::notification_for_transition(Working, Done, &cfg, false, false, "claude", 1);
        assert!(p.system.is_some());
    }

    // The OSC-9 fallback: `auto` routes to OSC-9 (not the OS notifier) over SSH,
    // so the ping reaches the client terminal — and the explicit modes obey.
    #[test]
    fn notify_desktop_delivery_routes_osc9_over_ssh() {
        use super::desktop_delivery;
        use crate::config::DesktopVia::{Auto, Both, Osc9, System};
        // (system, osc9)
        assert_eq!(
            desktop_delivery(Auto, false),
            (true, false),
            "auto local → system"
        );
        assert_eq!(
            desktop_delivery(Auto, true),
            (false, true),
            "auto SSH → OSC-9"
        );
        assert_eq!(
            desktop_delivery(System, true),
            (true, false),
            "system forced even on SSH"
        );
        assert_eq!(
            desktop_delivery(Osc9, false),
            (false, true),
            "osc9 forced even local"
        );
        assert_eq!(desktop_delivery(Both, false), (true, true));
    }

    #[test]
    fn notify_auto_over_ssh_emits_osc9_not_system() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Idle};
        let cfg = NotifyConfig::default(); // desktop_via = auto
        // is_ssh = true → the desktop ping goes out as OSC-9, not the OS notifier.
        let p = super::notification_for_transition(Idle, Blocked, &cfg, false, true, "codex", 2);
        assert!(p.system.is_none(), "no OS notifier over SSH under auto");
        let msg = p.osc9.expect("OSC-9 fires over SSH");
        assert!(
            msg.contains("codex") && msg.contains('2'),
            "OSC-9 carries agent + tab"
        );
    }

    #[test]
    fn focused_tab_notifies_by_default_but_knob_suppresses() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Idle};
        // By default a focused tab still fires — spyc's keyboard focus doesn't
        // mean the user is looking at the terminal (they're usually off in
        // another app while the agent works), so "needs me" must reach them.
        let cfg = NotifyConfig::default();
        assert!(
            !super::notification_for_transition(Idle, Blocked, &cfg, true, false, "codex", 2)
                .is_silent(),
            "focused tab notifies by default"
        );
        // The opt-in knob still silences the on-screen tab when set.
        let cfg = NotifyConfig {
            suppress_focused_tab: true,
            ..NotifyConfig::default()
        };
        assert!(
            super::notification_for_transition(Idle, Blocked, &cfg, true, false, "codex", 2)
                .is_silent(),
            "suppress_focused_tab = true stays silent for the watched tab"
        );
    }

    #[test]
    fn notify_done_gated_by_desktop_done() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Done, Idle, Working};
        let cfg = NotifyConfig {
            desktop_done: false,
            ..NotifyConfig::default()
        };
        // Done with desktop_done off → silent…
        assert!(
            super::notification_for_transition(Working, Done, &cfg, false, false, "codex", 3)
                .is_silent()
        );
        // …but Blocked still fires.
        assert!(
            super::notification_for_transition(Idle, Blocked, &cfg, false, false, "codex", 3)
                .system
                .is_some()
        );
    }

    #[test]
    fn notify_only_on_a_fresh_edge_not_a_repeat() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Done, Idle, Working};
        let cfg = NotifyConfig::default();
        // No edge (Blocked → Blocked, Done → Done) → silent: one ping per turn.
        assert!(
            super::notification_for_transition(Blocked, Blocked, &cfg, false, false, "x", 1)
                .is_silent()
        );
        assert!(
            super::notification_for_transition(Done, Done, &cfg, false, false, "x", 1).is_silent()
        );
        // Non-worthy transitions (resume / go quiet) never ping.
        assert!(
            super::notification_for_transition(Idle, Working, &cfg, false, false, "x", 1)
                .is_silent()
        );
        assert!(
            super::notification_for_transition(Working, Idle, &cfg, false, false, "x", 1)
                .is_silent()
        );
    }

    #[test]
    fn notify_bell_only_config_rings_without_a_desktop_popup() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Idle};
        let cfg = NotifyConfig {
            desktop: false,
            bell: true,
            ..NotifyConfig::default()
        };
        let p = super::notification_for_transition(Idle, Blocked, &cfg, false, false, "codex", 1);
        assert!(
            p.system.is_none() && p.osc9.is_none(),
            "no desktop notif when desktop is off"
        );
        assert!(p.bell, "the bell still rings");
        assert!(!p.is_silent());
    }

    // The intrusive channels default to Blocked-only: with bell + flash both on
    // (the noisy config a user hits), a per-turn `Done` must NOT ring or strobe —
    // only the quiet desktop ping fires on Done. `Blocked` still fires them all.
    #[test]
    fn notify_bell_and_flash_are_blocked_only_by_default() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Done, Idle, Working};
        let cfg = NotifyConfig {
            bell: true,
            visual: true,
            ..NotifyConfig::default()
        };

        // Blocked ("needs me") fires the interrupting channels.
        let p = super::notification_for_transition(Idle, Blocked, &cfg, false, false, "codex", 1);
        assert!(p.bell, "bell rings on Blocked");
        assert!(p.visual, "flash fires on Blocked");

        // Done (every finished turn) stays quiet on bell + flash; desktop still pings.
        let p = super::notification_for_transition(Working, Done, &cfg, false, false, "codex", 1);
        assert!(!p.bell, "no per-turn bell on Done");
        assert!(!p.visual, "no per-turn flash on Done");
        assert!(
            p.system.is_some(),
            "the quiet desktop ping still fires on Done"
        );
    }

    // `bell_done` / `visual_done` opt the intrusive channels back into `Done`.
    #[test]
    fn notify_bell_done_and_visual_done_opt_into_done() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Done, Working};
        let cfg = NotifyConfig {
            bell: true,
            bell_done: true,
            visual: true,
            visual_done: true,
            ..NotifyConfig::default()
        };
        let p = super::notification_for_transition(Working, Done, &cfg, false, false, "codex", 1);
        assert!(p.bell, "bell_done rings the bell on Done");
        assert!(p.visual, "visual_done flashes on Done");
    }
}

//! MVU Phase 6: the active pane's status-line agent short-id, resolved OFF the
//! render thread.
//!
//! Extracted from `app/mod.rs` (same descendant-module `impl App` pattern as
//! `streaming` / `commands` — reads App's private state directly). Keeps the
//! off-thread machinery + the `~/.claude/sessions/*.json` walk dispatch out of
//! the loop file. `resolve_short_id` is a per-file JSON walk that scales with
//! the user's accumulated sessions, so it must never run on the render thread
//! (it once showed ~65% of main-thread CPU). Pattern mirrors
//! `TabEntry::live_cwd` (#227): a landing + in-flight flag + a detached
//! worker that wakes the loop on completion; render reads the cache and never
//! blocks.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::{
    AGENT_STATUS_TTL, AgentKind, AgentStatusCache, App, Deadline, Effect, RunCtx, VisualBell, Wake,
};
use crate::config::{DesktopVia, NotifyConfig};
use crate::pane::{AgentActivity, ReportedStatus, TabInfo};

/// How long after a tab's last pane output it still counts as `Working`. Long
/// enough to bridge the sub-second gaps between streamed token chunks; short
/// enough that "the agent stopped" reads as `Idle` promptly. A silent thinking
/// pause longer than this reads as `Idle` — true working-through-silence needs
/// the P1 semantic hook (`docs/archive/AGENT_AWARENESS_PLAN.md`), not output timing.
const AGENT_ACTIVE_WINDOW: Duration = Duration::from_secs(2);

/// Screen-scan debounce. Codex's complete approval forms are checked this long
/// after the first pending repaint; other agents require this much silence.
const SCRAPE_QUIET_WINDOW: Duration = Duration::from_millis(250);

/// What [`App::scrape_step`] decided for one dirty tab: nothing to do, scan its
/// screen now, or re-check once its quiet window expires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrapeStep {
    /// The tab can never produce a scrape result (no detection rules), so drop
    /// its dirty flag and arm nothing.
    Skip,
    Scan,
    WaitUntil(Instant),
}

/// The active pane's agent identity for status resolution: profile, kind, cwd,
/// spawn-time cache key, and the tab's pinned session id (if any).
type ActiveAgentKey = (
    &'static dyn crate::agent::AgentProfile,
    AgentKind,
    std::path::PathBuf,
    u64,
    Option<String>,
);

/// Cadence of the "spicy pulse" working animation (10 Hz, a breath of 16
/// frames in `render::chrome`). Armed only while ≥1 agent tab is `Working`, so
/// an all-idle pane set still draws 0 fps.
const AGENT_ANIM_INTERVAL: Duration = Duration::from_millis(100);

/// Whether pane output at `last_output_at` supersedes a live report (the agent
/// resumed → drop the report, fall back to timing).
///
/// Semantic reports (`Blocked`, `Working`, `Waiting`, `Done`) survive pane
/// output across every hosted agent: prompt redraws, TUI footer flushes after a
/// `Stop` hook, and background-task status updates do not establish a new turn
/// or cancel an active wait. Only an explicit `Idle` report yields to fresh
/// output. `Blocked` is additionally latched against TTL expiry until settled
/// by a newer report or user input (`Enter`, `Esc`, `^c` in `run_effects`, or
/// matching tool completion for identified Codex questions), while visible
/// approval prompts in Codex and agy temporarily override non-blocked reports
/// via [`crate::agent::codex_approval::overrides_report`].
fn report_superseded_by_output(r: &ReportedStatus, last_output_at: Option<Instant>) -> bool {
    r.status == AgentActivity::Idle && last_output_at.is_some_and(|o| o > r.at)
}

/// A closed pane's reports, turn and scrape describe no agent any more.
fn forget_closed_agent(info: &mut crate::pane::tabs::TabInfo) {
    info.reported = None;
    info.held_waiting = None;
    info.turn_open = false;
    info.scrape_status = None;
    info.scrape_dirty = false;
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
    who: &Subject,
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
        let (summary, body) = notification_text(who, new);
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

/// Who a notification is about: the tab's label and its number in its
/// project, and the project, named only while several are open.
struct Subject<'a> {
    label: &'a str,
    tab_number: usize,
    project: Option<&'a str>,
}

/// Compose the `(summary, body)` for a transition notification: a glanceable
/// "which agent + what" summary plus the tab locator in the body, each naming
/// the project when [`Subject::project`] does. Only `Blocked` / `Done` reach
/// here (see [`notification_for_transition`]); the fallback arm is a sane
/// non-panicking default.
fn notification_text(who: &Subject, new: AgentActivity) -> (String, String) {
    let (label, tab) = (who.label, who.tab_number);
    let (agent, place) = who.project.map_or_else(
        || (label.to_owned(), format!("tab {tab}")),
        |project| {
            (
                format!("{label} in {project}"),
                format!("{project}: tab {tab}"),
            )
        },
    );
    match new {
        AgentActivity::Blocked => (format!("{agent} needs you"), format!("{place} is blocked")),
        AgentActivity::Done => (format!("{agent} finished"), format!("{place} — done")),
        _ => (agent, place),
    }
}

/// One project's part of the activity walk: its `[notify]`, how its
/// notifications name it, and what the user sees of it.
struct ProjectNotify {
    id: super::state::ProjectId,
    cfg: NotifyConfig,
    /// Its label, while several projects are open: a tab number alone doesn't
    /// say which project's tab.
    named: Option<String>,
    /// Its place in switcher order, which orders the bell's flash.
    order: usize,
    /// It is the project in view.
    in_view: bool,
    /// The tab the user sees in it ([`App::in_view_tab`]).
    tab_in_view: Option<usize>,
}

/// A bell rung from a project out of view, and the flash that says where.
struct BellNote {
    blocked: bool,
    order: usize,
    tab_number: usize,
    text: String,
}

impl BellNote {
    /// Blocked over done, then switcher order, then tab order: the walk's map
    /// order isn't the user's.
    fn outranks(&self, other: &Self) -> bool {
        (self.blocked, other.order, other.tab_number) > (other.blocked, self.order, self.tab_number)
    }
}

/// Choose the active pane's status-bar agent suffix. The tab's pinned session id
/// (its exact per-tab identity) wins over the `cached` proximity-resolved
/// short-id, which can't tell two agent tabs sharing a cwd apart — both would
/// show one id (#156). `cache_matches` gates the cached value to the current
/// pane; with no pin and no matching cache, the bare label shows until the first
/// resolve lands. Pure so the pin-preference is testable without pane fixtures.
fn choose_status_suffix(
    name: &str,
    pinned: Option<&str>,
    cached: Option<&str>,
    cache_matches: bool,
) -> Option<String> {
    if let Some(sid) = pinned {
        return Some(format!("{name}:{}", crate::state::sessions::short_id(sid)));
    }
    if cache_matches {
        cached.map(str::to_string)
    } else {
        Some(name.to_string())
    }
}

/// One settle's walk over every project's tabs: what every tab shares, and
/// what the walk gathers for [`App::settle_agent_activity`] to act on.
struct ActivityScan<'a> {
    now: Instant,
    is_ssh: bool,
    track_status: bool,
    codex_recovery: &'a mut HashMap<String, crate::agent::codex_recovery::CodexRecovery>,
    changed: bool,
    effects: Vec<Effect>,
    start_visual: bool,
    earliest_flip: Option<Instant>,
    any_working: bool,
    status_transitions: Vec<(String, AgentActivity)>,
    live_ids: HashSet<String>,
    bell_note: Option<BellNote>,
}

impl ActivityScan<'_> {
    /// When a tab's `Blocked` began, and a `Done` that lands while the tab is
    /// out of view, kept until it is in view. In view, the tab's unseen done
    /// and its `+` are seen.
    fn settle_attention(&mut self, info: &mut TabInfo, new: AgentActivity, in_view: bool) {
        if new == AgentActivity::Blocked {
            info.blocked_since.get_or_insert(self.now);
        } else {
            info.blocked_since = None;
        }
        if in_view {
            let seen = info.unseen_done.take().is_some();
            self.changed |= seen | std::mem::take(&mut info.has_activity);
        } else if new == AgentActivity::Done && info.notified != AgentActivity::Done {
            info.unseen_done.get_or_insert(self.now);
        }
    }

    /// Keep the most urgent bell rung from a project out of view.
    fn note_bell(&mut self, note: BellNote) {
        if self
            .bell_note
            .as_ref()
            .is_none_or(|best| note.outranks(best))
        {
            self.bell_note = Some(note);
        }
    }

    /// Settle one tab, `index` its place in `project` (0-based). `focused`:
    /// the user is looking at it, so a transition doesn't ping them.
    fn settle_tab(
        &mut self,
        entry: &mut crate::pane::TabEntry,
        index: usize,
        focused: bool,
        project: &ProjectNotify,
    ) {
        let kind = entry.info.agent().kind();
        let closed = entry.pane.is_closed();
        if closed {
            forget_closed_agent(&mut entry.info);
            self.codex_recovery.remove(&entry.info.id);
        }
        // Drop a report that's no longer authoritative (expired, or the tab
        // emitted output that supersedes it) so diagnostics stay honest. A
        // `Blocked` report is latched — it never expires here and output
        // never supersedes it (see `effective_activity`); only the Enter-on-
        // pane clear in `run_effects` or a newer report drops it.
        if let Some(r) = &entry.info.reported
            && ((r.status != AgentActivity::Blocked && self.now >= r.expiry)
                || report_superseded_by_output(r, entry.info.last_output_at))
        {
            entry.info.reported = None;
        }
        // Retain Codex's and agy's current visible wait even behind a live
        // report: a later working/done report need not repaint the unanswered
        // view. Other agents discard guesses behind live reports.
        if entry.info.reported.is_some()
            && !crate::agent::codex_approval::overrides_report(
                None,
                entry.info.scrape_status.map(|(status, _)| status),
                kind,
            )
        {
            entry.info.scrape_status = None;
        }
        let new = if closed {
            App::activity_for(kind != AgentKind::Other, None, self.now)
        } else {
            App::effective_activity(
                entry.info.reported.as_ref(),
                entry.info.scrape_status.map(|(s, _)| s),
                kind,
                entry.info.last_output_at,
                self.now,
            )
        };
        // Wake-arming: re-evaluate at the earliest of a live report's expiry
        // or a timing-Working tab's idle-flip, so the dot can't go stale. A
        // latched `Blocked` report never expires, so don't arm a wake for it
        // (nothing would flip; an Enter keystroke clears it via its own path).
        if let Some(r) = &entry.info.reported {
            if r.status != AgentActivity::Blocked {
                self.earliest_flip = Some(
                    self.earliest_flip
                        .map_or(r.expiry, |m: Instant| m.min(r.expiry)),
                );
            }
        } else if new == AgentActivity::Working
            && let Some(at) = entry.info.last_output_at
        {
            let flip = at + AGENT_ACTIVE_WINDOW;
            self.earliest_flip = Some(self.earliest_flip.map_or(flip, |m: Instant| m.min(flip)));
        }
        if new == AgentActivity::Working {
            self.any_working = true;
        }
        self.settle_attention(&mut entry.info, new, project.tab_in_view == Some(index));
        // P3-1 notification edge: compare `new` against `notified` (this
        // settle owns it), NOT `activity` — the MCP `report_status` handler
        // pre-sets `activity` for an instant dot, which would otherwise erase
        // the Idle→Blocked edge before we see it (→ no ping). Decide against
        // the old `notified`, then advance it.
        if new != entry.info.notified {
            let who = Subject {
                label: &entry.info.label,
                tab_number: index + 1,
                project: project.named.as_deref(),
            };
            let plan = notification_for_transition(
                entry.info.notified,
                new,
                &project.cfg,
                focused,
                self.is_ssh,
                &who,
            );
            if !plan.is_silent() {
                if plan.visual && !project.in_view && who.project.is_some() {
                    self.note_bell(BellNote {
                        blocked: new == AgentActivity::Blocked,
                        order: project.order,
                        tab_number: who.tab_number,
                        text: notification_text(&who, new).1,
                    });
                }
                if plan.system.is_some() || plan.osc9.is_some() || plan.bell {
                    self.effects.push(Effect::Notify {
                        system: plan.system,
                        osc9: plan.osc9,
                        bell: plan.bell,
                    });
                }
                self.start_visual |= plan.visual;
            }
            entry.info.notified = new;
        }
        // Render field (the dot): the MCP handler may have already advanced it
        // for an instant update; keep it in sync here either way.
        if entry.info.activity != new {
            entry.info.activity = new;
            self.changed = true;
        }
        // Record this tab's effective status for the `agent_status` event
        // (dedup + firing happen after the borrow). Keyed by the stable
        // `SPYC_PANE_ID` so the event survives tab reorder.
        if self.track_status {
            self.live_ids.insert(entry.info.id.clone());
            self.status_transitions.push((entry.info.id.clone(), new));
        }
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
        let (profile, kind, cwd, spawn, pinned) = self.active_agent_key()?;
        // Prefer the tab's pinned session id — its exact identity, a pure
        // short-id truncation with no session-index walk. The cached proximity
        // resolve is only the fallback for a pane with no pin yet (before its
        // first status hook fires); it can't tell two same-cwd tabs apart, which
        // is why both used to show one id. Until the first refresh for an
        // unpinned freshly-focused pane lands, the bare label shows — never
        // block, never another pane's id.
        let cache_matches = self.agent_cache_matches(kind, &cwd, spawn);
        let cached = self
            .view
            .agent_status_cache
            .as_ref()
            .and_then(|c| c.status.as_deref());
        choose_status_suffix(profile.name(), pinned.as_deref(), cached, cache_matches)
    }

    /// The active pane's agent identity for status resolution: the `'static`
    /// profile ref (the registry lives forever) plus the spawn-time cache key.
    /// `None` when there's no active pane or it isn't a known agent. The
    /// `pane_tabs` borrow is dropped at return (cwd is cloned), so callers can
    /// freely touch the cache / runtime slots afterward.
    fn active_agent_key(&self) -> Option<ActiveAgentKey> {
        let active = self.runtime.project().pane_tabs.as_ref()?.active_info();
        let profile = active.agent();
        let kind = profile.kind();
        if kind == AgentKind::Other {
            return None;
        }
        let pinned = active.pinned_session_id().map(str::to_string);
        Some((
            profile,
            kind,
            active.cwd.clone(),
            active.agent_epoch_secs(),
            pinned,
        ))
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
    pub(crate) fn kick_agent_status_refresh(&mut self) {
        let Some((profile, kind, cwd, spawn, pinned)) = self.active_agent_key() else {
            return;
        };
        // A pinned pane resolves its suffix purely in `active_agent_status` (an
        // exact per-tab short-id, no session-index walk) — don't spawn the
        // proximity resolver for it. (#156)
        if pinned.is_some() {
            return;
        }
        let fresh = self.agent_cache_matches(kind, &cwd, spawn)
            && self
                .view
                .agent_status_cache
                .as_ref()
                .is_some_and(|c| c.computed_at.elapsed() < AGENT_STATUS_TTL);
        if fresh || self.runtime.agent_status_refreshing {
            return;
        }
        self.runtime.agent_status_refreshing = true;
        let label = profile.name();
        let pending = self
            .runtime
            .agent_status_pending
            .sender(self.runtime.pane_wake_tx.clone(), Wake::AgentStatus);
        let thread_cwd = cwd;
        std::thread::spawn(move || {
            let short_id = profile.resolve_short_id(&thread_cwd, spawn);
            let status = Some(match short_id {
                Some(id) => format!("{label}:{id}"),
                None => label.to_string(),
            });
            let _ = pending.send(AgentStatusCache {
                computed_at: std::time::Instant::now(),
                kind,
                cwd: thread_cwd,
                spawn_epoch_secs: spawn,
                status,
            });
        });
    }

    /// MVU Phase 6: drain a landed off-thread agent-status result into the
    /// cache, ending the refresh in flight. Called from the PRE-RECV SCAN every
    /// loop iteration, so the landing is ALWAYS emptied — regardless of which
    /// render path runs (the status bar, hence `active_agent_status`, is skipped
    /// while an overlay / top-pager is open; draining only there would leave
    /// the landing full and the scan's redraw nudge would busy-spin). Applies the result
    /// only if it's for the CURRENT active pane — a late result for a
    /// since-switched pane is discarded, never clobbering the active cache.
    /// Returns whether the cache changed (the caller sets `needs_draw`).
    pub(crate) fn apply_landed_agent_status(&mut self) -> bool {
        let Some(result) = self.runtime.agent_status_pending.latest() else {
            return false;
        };
        self.runtime.agent_status_refreshing = false;
        let matches = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .is_some_and(|tabs| {
                let active = tabs.active_info();
                result.kind == active.agent().kind()
                    && result.cwd == active.cwd
                    && result.spawn_epoch_secs == active.agent_epoch_secs()
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

    /// Resolve semantic reports, verified approval dialogues and output timing.
    /// Visible approval prompts in Codex and agy temporarily override
    /// non-blocked reports without discarding them; semantic question/agent
    /// blocks retain precedence. Semantic reports (`Working`, `Waiting`,
    /// `Done`, `Blocked`) survive redraws and silent tool waits until expiry
    /// (or settlement for latched `Blocked`). `scrape` comes from the debounced
    /// visible-screen scan in `settle_scrape_quiet`.
    /// Non-agent tabs are always `Unknown` (a report targeting one is ignored —
    /// dots are agent-only).
    fn effective_activity(
        reported: Option<&ReportedStatus>,
        scrape: Option<AgentActivity>,
        kind: AgentKind,
        last_output_at: Option<Instant>,
        now: Instant,
    ) -> AgentActivity {
        if kind == AgentKind::Other {
            return AgentActivity::Unknown;
        }
        if crate::agent::codex_approval::overrides_report(reported, scrape, kind) {
            return AgentActivity::Blocked;
        }
        if let Some(r) = reported {
            // `Blocked` is latched: no TTL expiry and output never supersedes it
            // (`report_superseded_by_output` returns false), so it holds until the
            // user answers the pane (Enter, in `run_effects`) or a newer report
            // lands. Other semantic statuses expire at `expiry`; only `Idle`
            // yields to newer output.
            let expired = r.status != AgentActivity::Blocked && now >= r.expiry;
            let superseded = report_superseded_by_output(r, last_output_at);
            if !expired && !superseded {
                return r.status;
            }
        }
        // P1-2 scrape fallback: `settle_scrape_quiet` debounces the screen read.
        // Codex's complete forms have a bounded delay; other agents wait for
        // silence. Report precedence is resolved here, after that screen read.
        if let Some(s) = scrape {
            return s;
        }
        Self::activity_for(true, last_output_at, now)
    }

    /// P1-2 pure debounce step for one dirty tab: skip it, scan its screen now,
    /// or wait until its debounce expires. The caller chooses the first pending
    /// repaint for Codex and the last output for other agents. A missing start
    /// reads as due so the function is total.
    ///
    /// Deliberately blind to whether a report is live: a report that is about to
    /// be superseded must not cancel the scan. See [`Self::settle_scrape_quiet`].
    fn scrape_step(has_rules: bool, wait_from: Option<Instant>, now: Instant) -> ScrapeStep {
        if !has_rules {
            return ScrapeStep::Skip;
        }
        let Some(at) = wait_from else {
            return ScrapeStep::Scan;
        };
        let fire_at = at + SCRAPE_QUIET_WINDOW;
        if now >= fire_at {
            ScrapeStep::Scan
        } else {
            ScrapeStep::WaitUntil(fire_at)
        }
    }

    /// Scan dirty agent viewports, in every project, after their debounce.
    /// Codex's complete forms use the first pending repaint, so a waiting
    /// tool's animation cannot keep postponing detection. Other agents use the
    /// last output's quiet window.
    ///
    /// **A live report does not cancel the scan**. Skipping the scan behind one
    /// looks like a free optimization and is not: visible approval prompts in
    /// Codex and agy arrive while a `working` report is already live (from
    /// `UserPromptSubmit` / `PreInvocation`) and fire no hook of their own.
    /// Their `Blocked` scrape result temporarily overrides that non-blocked
    /// report via `codex_approval::overrides_report` without dropping it, and
    /// since `scrape_dirty` is only re-set by fresh output, skipping the scan
    /// behind a live report would permanently miss an approval prompt whose
    /// last output was drawing the prompt itself.
    ///
    /// `&mut` settle point, PRE-recv. Returns `true` if any tab's status changed.
    pub(crate) fn settle_scrape_quiet(
        &mut self,
        now: std::time::Instant,
        ctx: &mut RunCtx,
    ) -> bool {
        let mut changed = false;
        let mut earliest_fire: Option<std::time::Instant> = None;
        let entries = self
            .runtime
            .every_tab_set_mut()
            .flat_map(|(_, tabs)| tabs.tabs_mut().iter_mut());
        for entry in entries {
            if !entry.info.scrape_dirty {
                entry.info.scrape_pending_at = None;
                continue;
            }
            // SPYC-TRAP(scrape-scan-ignores-live-report): do not skip the scan
            // because `entry.info.reported.is_some()` — Codex's and agy's
            // visible approval prompts override a live non-blocked report.
            let profile = entry.info.agent();
            let rules = profile.detection_rules();
            let wait_from = if profile.kind() == AgentKind::Codex {
                Some(
                    *entry
                        .info
                        .scrape_pending_at
                        .get_or_insert(entry.info.last_output_at.unwrap_or(now)),
                )
            } else {
                entry.info.last_output_at
            };
            match Self::scrape_step(!rules.is_empty(), wait_from, now) {
                // No rules — these agents can never produce a scrape result, so they pay
                // neither the debounce nor the screen read.
                ScrapeStep::Skip => {
                    entry.info.scrape_dirty = false;
                    entry.info.scrape_pending_at = None;
                }
                ScrapeStep::WaitUntil(fire_at) => {
                    earliest_fire = Some(earliest_fire.map_or(fire_at, |m| m.min(fire_at)));
                }
                ScrapeStep::Scan => {
                    entry.info.scrape_dirty = false;
                    entry.info.scrape_pending_at = None;
                    let lines = entry.pane.visible_lines();
                    let scanned = crate::agent::detect_rules::scan(&lines, rules);
                    changed |= entry.info.scrape_status != scanned;
                    entry.info.scrape_status = scanned;
                }
            }
        }
        match earliest_fire {
            Some(t) => ctx.scheduler.arm(Deadline::ScrapeQuiet, t),
            None => ctx.scheduler.disarm(Deadline::ScrapeQuiet),
        }
        changed
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
        let anim_epoch = self.view.started_at;
        // Every project's tabs (#99): an agent in a project the user isn't in
        // keeps its dot and notifies, under its own project's `[notify]`, and
        // only the project in view's active tab can be the focused one. The
        // inputs are gathered up front so the walk's `pane_tabs` borrow doesn't
        // overlap the `self.view` / `self.state` reads; each config is cloned
        // (5 bools, cheap).
        let in_view = self.state.active;
        let pane_focused = self.state.pane_focused();
        let notify = self.project_notify();
        let open: HashSet<String> = self
            .runtime
            .every_tab_set()
            .flat_map(|(_, tabs)| tabs.tabs().iter().map(|tab| tab.info.id.clone()))
            .collect();
        self.state.codex_recovery.retain(|id, _| open.contains(id));
        // Only track `agent_status` transitions when a Lua handler wants the
        // event (the common case has none → no per-tab bookkeeping). Collect
        // during the walk, fire after it (fire needs `&mut self`).
        let track_status = self.lua_wants_agent_status_event();
        let mut scan = ActivityScan {
            now,
            is_ssh: self.view.is_ssh,
            track_status,
            codex_recovery: &mut self.state.codex_recovery,
            changed: false,
            effects: Vec::new(),
            start_visual: false,
            earliest_flip: None,
            any_working: false,
            status_transitions: Vec::new(),
            live_ids: HashSet::new(),
            bell_note: None,
        };
        for (id, tabs) in self.runtime.every_tab_set_mut() {
            let Some(project) = notify.iter().find(|project| project.id == id) else {
                continue;
            };
            let active_idx = tabs.active_index();
            for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
                let focused = id == in_view && i == active_idx && pane_focused;
                scan.settle_tab(entry, i, focused, project);
            }
        }
        let ActivityScan {
            mut changed,
            effects,
            start_visual,
            earliest_flip,
            any_working,
            status_transitions,
            live_ids,
            bell_note,
            ..
        } = scan;

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
            // The pulse says "look"; from another project, the flash says where,
            // unless an error is showing, which matters more.
            let error_showing = self
                .state
                .flash
                .as_ref()
                .is_some_and(|flash| matches!(flash.kind, super::FlashKind::Error));
            if let Some(note) = bell_note.filter(|_| !error_showing) {
                self.state.flash_info(note.text);
            }
        }
        (changed, effects)
    }

    /// Each open project's part of the activity walk, in switcher order.
    fn project_notify(&self) -> Vec<ProjectNotify> {
        let in_view = self.state.active;
        let tab_in_view = self.in_view_tab();
        let several = self.state.open_projects().nth(1).is_some();
        self.state
            .open_projects()
            .enumerate()
            .map(|(order, project)| ProjectNotify {
                id: project.id,
                cfg: project.config.notify.clone(),
                named: several.then(|| project.label()),
                order,
                in_view: project.id == in_view,
                tab_in_view: tab_in_view.filter(|_| project.id == in_view),
            })
            .collect()
    }

    /// The tab the user sees: the active tab of the project in view, while its
    /// pane is drawn. A hidden pane, a zoom onto the list or the right column,
    /// a modal pager or a full-screen image over the whole frame leaves none;
    /// the editor, a `D` pager, `^a v` and a split all keep the pane drawn.
    pub(super) fn in_view_tab(&self) -> Option<usize> {
        let pane = &self.state.active_project().pane;
        let drawn = !pane.pane_hidden
            && matches!(
                pane.zoom,
                super::state::ZoomTarget::None | super::state::ZoomTarget::BottomPane
            )
            && !self.modal_pager_open()
            && self.view.image_view.is_none();
        let tabs = self.runtime.project().pane_tabs.as_ref()?;
        drawn.then(|| tabs.active_index())
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
    /// A tab of an agent labelled `label`, its project unnamed (one open).
    fn who(label: &str, tab_number: usize) -> super::Subject<'_> {
        super::Subject {
            label,
            tab_number,
            project: None,
        }
    }

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

    /// Any landed result ends the refresh in flight, even one for a pane no
    /// longer active, or no refresh would ever start again.
    #[test]
    fn a_landed_result_ends_the_refresh_in_flight() {
        let cwd = PathBuf::from("/tmp/proj");
        let mut app = App::test_app(cwd.clone());
        app.runtime.agent_status_refreshing = true;
        app.runtime
            .agent_status_pending
            .land(cache(AgentKind::Claude, &cwd, 42));
        app.apply_landed_agent_status();
        assert!(!app.runtime.agent_status_refreshing);
    }

    #[test]
    fn no_active_pane_yields_none_and_kick_is_a_noop() {
        // The pure draw read returns None with no agent pane, and the kick
        // (now the sole spawn site, off the render path) never flips the
        // in-flight flag or spawns when there's nothing to resolve.
        let mut app = App::test_app(PathBuf::from("/tmp/proj"));
        assert_eq!(app.active_agent_status(), None);
        app.kick_agent_status_refresh();
        assert!(!app.runtime.agent_status_refreshing);
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
        let report = |status, at, expiry| Some(ReportedStatus::new(status, at, expiry));

        // A live `working` report wins even with NO output (silent thinking) —
        // the headline win over the timing-only P0 dot.
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Working, base, base + Duration::from_secs(60)).as_ref(),
                None,
                AgentKind::Agy,
                None,
                base,
            ),
            AgentActivity::Working
        );
        // A live `blocked` report → Blocked (the "needs me" signal).
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Blocked, base, base + Duration::from_secs(60)).as_ref(),
                None,
                AgentKind::Agy,
                Some(base),
                base + Duration::from_secs(1),
            ),
            AgentActivity::Blocked
        );
        // A non-blocked report past its TTL → falls back to timing (no recent
        // output → Idle). Blocked is exempt from TTL — see the latch test.
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Working, base, base + Duration::from_secs(1)).as_ref(),
                None,
                AgentKind::Agy,
                None,
                base + Duration::from_secs(2),
            ),
            AgentActivity::Idle
        );
        // Output AFTER the report (agent resumed) supersedes it → timing wins.
        let later = base + Duration::from_millis(500);
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Idle, base, base + Duration::from_secs(60)).as_ref(),
                None,
                AgentKind::Agy,
                Some(later),
                later,
            ),
            AgentActivity::Working
        );
        // A non-agent tab ignores any report (dots are agent-only).
        assert_eq!(
            App::effective_activity(
                report(AgentActivity::Working, base, base + Duration::from_secs(60)).as_ref(),
                None,
                AgentKind::Other,
                None,
                base,
            ),
            AgentActivity::Unknown
        );
    }

    // P1-2: the scrape fallback sits BETWEEN a live report and output timing —
    // consulted only when there's no report, and itself beaten by one.
    // The debounced `settle_scrape_quiet` guarantees a scrape result is only
    // written after silence, so effective_activity trusts it unconditionally.
    #[test]
    fn effective_activity_scrape_fallback_sits_between_report_and_timing() {
        use crate::pane::AgentActivity;
        use std::time::{Duration, Instant};
        let base = Instant::now();

        // No report, no output → timing alone would say Idle; scrape overrides.
        assert_eq!(
            App::effective_activity(
                None,
                Some(AgentActivity::Blocked),
                AgentKind::Agy,
                None,
                base
            ),
            AgentActivity::Blocked
        );
        // Scrape is trusted even with recent output (settle_scrape_quiet
        // handles the timing — effective_activity just reads the result).
        assert_eq!(
            App::effective_activity(
                None,
                Some(AgentActivity::Blocked),
                AgentKind::Agy,
                Some(base),
                base
            ),
            AgentActivity::Blocked
        );
        // A live report still wins over a non-override scrape guess, while a
        // visible `Blocked` approval prompt in Codex/agy temporarily overrides
        // a non-blocked report.
        let report = Some(crate::pane::ReportedStatus::new(
            AgentActivity::Working,
            base,
            base + Duration::from_secs(60),
        ));
        assert_eq!(
            App::effective_activity(
                report.as_ref(),
                Some(AgentActivity::Idle),
                AgentKind::Agy,
                None,
                base
            ),
            AgentActivity::Working
        );
        assert_eq!(
            App::effective_activity(
                report.as_ref(),
                Some(AgentActivity::Blocked),
                AgentKind::Agy,
                None,
                base
            ),
            AgentActivity::Blocked
        );
        // No scrape guess either → falls all the way through to timing.
        assert_eq!(
            App::effective_activity(None, None, AgentKind::Agy, None, base),
            AgentActivity::Idle
        );
        // Non-agent tab ignores scrape too (dots are agent-only).
        assert_eq!(
            App::effective_activity(
                None,
                Some(AgentActivity::Blocked),
                AgentKind::Other,
                None,
                base
            ),
            AgentActivity::Unknown
        );
    }

    // P1-2 debounce: a dirty tab is scanned only once its output has gone quiet
    // for SCRAPE_QUIET_WINDOW, so the scan reads a settled screen instead of a
    // half-drawn prompt. Below the window it reports when to re-check, which is
    // what `settle_scrape_quiet` arms `Deadline::ScrapeQuiet` on.
    #[test]
    fn scrape_step_waits_for_the_quiet_window_then_scans() {
        use super::{SCRAPE_QUIET_WINDOW, ScrapeStep};
        use std::time::{Duration, Instant};
        let base = Instant::now();

        // Output just landed → not due; wait until exactly one window later.
        assert_eq!(
            App::scrape_step(true, Some(base), base),
            ScrapeStep::WaitUntil(base + SCRAPE_QUIET_WINDOW)
        );
        // A hair short of the window still waits (no early scan).
        let almost = SCRAPE_QUIET_WINDOW
            .checked_sub(Duration::from_millis(1))
            .expect("the quiet window is longer than 1ms");
        assert_eq!(
            App::scrape_step(true, Some(base), base + almost),
            ScrapeStep::WaitUntil(base + SCRAPE_QUIET_WINDOW)
        );
        // Exactly at the boundary is due — the deadline we armed must fire, not
        // re-arm itself for the same instant and spin.
        assert_eq!(
            App::scrape_step(true, Some(base), base + SCRAPE_QUIET_WINDOW),
            ScrapeStep::Scan
        );
        assert_eq!(
            App::scrape_step(true, Some(base), base + Duration::from_secs(5)),
            ScrapeStep::Scan
        );
        // Total for the unreachable case (dirty implies an output stamp).
        assert_eq!(App::scrape_step(true, None, base), ScrapeStep::Scan);

        // No detection rules → nothing to scan, and nothing armed. This is the
        // whole cost story for claude/codex/zot, whose hooks report every state
        // they have.
        for (last, now) in [
            (Some(base), base),
            (Some(base), base + SCRAPE_QUIET_WINDOW),
            (None, base),
        ] {
            assert_eq!(App::scrape_step(false, last, now), ScrapeStep::Skip);
        }
    }

    /// The scan must not be cancelled by a live report. Visible approval
    /// prompts in Codex and agy arrive while a `working` report is live and
    /// fire no hook of their own; `codex_approval::overrides_report` lets the
    /// `Blocked` scrape result temporarily override that non-blocked report
    /// without discarding it. Because `scrape_dirty` is only re-set by fresh
    /// output, skipping the scan while a report is live would permanently miss
    /// an approval prompt whose last output was drawing the prompt itself.
    ///
    /// Encoded as a type-level fact rather than a behavioural assertion: the pure
    /// step takes no report argument at all, so it cannot regress. This test
    /// pins the two halves of the reasoning that make that safe.
    #[test]
    fn a_live_report_neither_cancels_the_scan_nor_loses_to_it() {
        use super::{SCRAPE_QUIET_WINDOW, ScrapeStep};
        use crate::pane::{AgentActivity, ReportedStatus};
        use std::time::{Duration, Instant};
        let base = Instant::now();

        // 1. A tab with rules is still scanned at the boundary — the step has no
        //    way to know a report is live, which is the point.
        assert_eq!(
            App::scrape_step(true, Some(base), base + SCRAPE_QUIET_WINDOW),
            ScrapeStep::Scan
        );

        // 2. Scanning behind a live report is harmless when no approval blocker
        //    matched: an unexpired report outranks a non-blocked scrape guess.
        let report = ReportedStatus::new(
            AgentActivity::Working,
            base + Duration::from_millis(10),
            base + Duration::from_secs(60),
        );
        assert_eq!(
            App::effective_activity(
                Some(&report),
                Some(AgentActivity::Idle),
                AgentKind::Agy,
                None,
                base + Duration::from_millis(20)
            ),
            AgentActivity::Working,
            "a live report must outrank a non-blocked scrape result"
        );

        // 3. When the scan finds a visible `Blocked` approval prompt, it
        //    temporarily overrides the live `Working` report — and once the
        //    prompt clears (`None`), `Working` resumes despite later output.
        let output_after = base + Duration::from_millis(50);
        assert_eq!(
            App::effective_activity(
                Some(&report),
                Some(AgentActivity::Blocked),
                AgentKind::Agy,
                Some(output_after),
                output_after + Duration::from_millis(1)
            ),
            AgentActivity::Blocked,
            "visible agy approval prompt must override a live working report"
        );
        assert_eq!(
            App::effective_activity(
                Some(&report),
                None,
                AgentKind::Agy,
                Some(output_after),
                output_after + Duration::from_secs(10)
            ),
            AgentActivity::Working,
            "retained working report must resume once the approval prompt clears"
        );
    }

    // A `blocked` report is LATCHED: neither output NOR its TTL supersedes it —
    // not the prompt's initial render, not the menu's later redraws while it
    // waits, and not the expiry clock (the bug where any of those bounced the
    // dot off red and back to the working pulse). It clears only via a newer
    // report or the user settling the prompt — Enter, Esc or `^c`, handled in
    // `run_effects`. A non-blocked report still hands back to timing on output.
    #[test]
    fn blocked_report_is_latched_against_output() {
        use crate::pane::{AgentActivity, ReportedStatus};
        use std::time::{Duration, Instant};
        let base = Instant::now();
        let expiry = base + Duration::from_secs(300);
        let blocked = Some(ReportedStatus::new(AgentActivity::Blocked, base, expiry));

        // Output long after the report (the prompt still redrawing while it
        // waits) does NOT supersede — the dot stays Blocked.
        let later = base + Duration::from_secs(30);
        assert_eq!(
            App::effective_activity(blocked.as_ref(), None, AgentKind::Agy, Some(later), later),
            AgentActivity::Blocked,
            "blocked is latched — prompt redraws must not bounce it to working"
        );

        // Its TTL does NOT end it either — a blocked report well past `expiry`
        // still reads Blocked (latched until the user answers with Enter). This
        // is the "stuck until <ENTER>" behaviour: no clock revives the dot.
        let stale = Some(ReportedStatus::new(
            AgentActivity::Blocked,
            base,
            base + Duration::from_secs(1),
        ));
        assert_eq!(
            App::effective_activity(
                stale.as_ref(),
                None,
                AgentKind::Agy,
                None,
                base + Duration::from_secs(600)
            ),
            AgentActivity::Blocked,
            "blocked has no TTL — it stays until answered"
        );

        // Contrast: an explicit `Idle` report still hands back to timing on
        // fresh output, while semantic states (`Done`, `Waiting`, `Working`)
        // survive redraw output until their TTL expires.
        let idle = Some(ReportedStatus::new(AgentActivity::Idle, base, expiry));
        assert_eq!(
            App::effective_activity(
                idle.as_ref(),
                None,
                AgentKind::Agy,
                Some(base + Duration::from_millis(200)),
                base + Duration::from_millis(300),
            ),
            AgentActivity::Working,
            "idle report yields when fresh output arrives"
        );
        for status in [
            AgentActivity::Done,
            AgentActivity::Waiting,
            AgentActivity::Working,
        ] {
            let reported = Some(ReportedStatus::new(status, base, expiry));
            assert_eq!(
                App::effective_activity(
                    reported.as_ref(),
                    None,
                    AgentKind::Agy,
                    Some(base + Duration::from_millis(200)),
                    base + Duration::from_secs(10),
                ),
                status,
                "{status:?} survives TUI redraw output until expiry"
            );
        }
    }

    // P3-1: the pure transition → notification decision (the core of the ping).
    // `false` = not the focused tab; `false` = not over SSH (so `auto` → system).
    #[test]
    fn notify_fires_on_blocked_and_done_transitions() {
        use crate::config::NotifyConfig;
        use crate::pane::AgentActivity::{Blocked, Done, Idle, Working};
        let cfg = NotifyConfig::default(); // desktop(auto) + desktop_done + visual on; bell off.

        // Idle → Blocked on a background tab, local → system "needs me" ping.
        let p =
            super::notification_for_transition(Idle, Blocked, &cfg, false, false, &who("codex", 2));
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
        let p = super::notification_for_transition(
            Working,
            Done,
            &cfg,
            false,
            false,
            &who("claude", 1),
        );
        assert!(p.system.is_some());
    }

    /// The bell's flash goes to the most urgent: blocked over done, then the
    /// earlier project in switcher order, then the lower tab.
    #[test]
    fn the_bell_note_for_the_most_urgent_wins() {
        let note = |blocked, order, tab_number| super::BellNote {
            blocked,
            order,
            tab_number,
            text: String::new(),
        };
        assert!(
            note(true, 3, 5).outranks(&note(false, 0, 1)),
            "blocked first"
        );
        assert!(
            note(false, 0, 9).outranks(&note(false, 2, 1)),
            "earlier project"
        );
        assert!(note(true, 1, 1).outranks(&note(true, 1, 2)), "lower tab");
        assert!(
            !note(true, 1, 2).outranks(&note(true, 1, 2)),
            "not over itself"
        );
    }

    /// With a project named, both halves say which project's tab it is; with
    /// none, they read as before.
    #[test]
    fn notification_text_names_the_project_when_given() {
        use crate::pane::AgentActivity::{Blocked, Done};
        let named = super::Subject {
            label: "claude",
            tab_number: 2,
            project: Some("web"),
        };
        assert_eq!(
            super::notification_text(&named, Blocked),
            (
                "claude in web needs you".into(),
                "web: tab 2 is blocked".into()
            )
        );
        assert_eq!(
            super::notification_text(&named, Done),
            ("claude in web finished".into(), "web: tab 2 — done".into())
        );
        assert_eq!(
            super::notification_text(&who("claude", 2), Done),
            ("claude finished".into(), "tab 2 — done".into())
        );
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
        let p =
            super::notification_for_transition(Idle, Blocked, &cfg, false, true, &who("codex", 2));
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
            !super::notification_for_transition(Idle, Blocked, &cfg, true, false, &who("codex", 2))
                .is_silent(),
            "focused tab notifies by default"
        );
        // The opt-in knob still silences the on-screen tab when set.
        let cfg = NotifyConfig {
            suppress_focused_tab: true,
            ..NotifyConfig::default()
        };
        assert!(
            super::notification_for_transition(Idle, Blocked, &cfg, true, false, &who("codex", 2))
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
            super::notification_for_transition(Working, Done, &cfg, false, false, &who("codex", 3))
                .is_silent()
        );
        // …but Blocked still fires.
        assert!(
            super::notification_for_transition(Idle, Blocked, &cfg, false, false, &who("codex", 3))
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
            super::notification_for_transition(Blocked, Blocked, &cfg, false, false, &who("x", 1))
                .is_silent()
        );
        assert!(
            super::notification_for_transition(Done, Done, &cfg, false, false, &who("x", 1))
                .is_silent()
        );
        // Non-worthy transitions (resume / go quiet) never ping.
        assert!(
            super::notification_for_transition(Idle, Working, &cfg, false, false, &who("x", 1))
                .is_silent()
        );
        assert!(
            super::notification_for_transition(Working, Idle, &cfg, false, false, &who("x", 1))
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
        let p =
            super::notification_for_transition(Idle, Blocked, &cfg, false, false, &who("codex", 1));
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
        let p =
            super::notification_for_transition(Idle, Blocked, &cfg, false, false, &who("codex", 1));
        assert!(p.bell, "bell rings on Blocked");
        assert!(p.visual, "flash fires on Blocked");

        // Done (every finished turn) stays quiet on bell + flash; desktop still pings.
        let p =
            super::notification_for_transition(Working, Done, &cfg, false, false, &who("codex", 1));
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
        let p =
            super::notification_for_transition(Working, Done, &cfg, false, false, &who("codex", 1));
        assert!(p.bell, "bell_done rings the bell on Done");
        assert!(p.visual, "visual_done flashes on Done");
    }

    #[test]
    fn pinned_session_id_wins_and_distinguishes_tabs() {
        // The bug: two tabs sharing a cwd both showed one proximity-resolved id.
        // Distinct pins must yield distinct suffixes, and the pin must override
        // whatever the (shared) cache holds.
        let a = super::choose_status_suffix(
            "claude",
            Some("aaaaaaaa-1111-2222"),
            Some("claude:zz"),
            true,
        );
        let b = super::choose_status_suffix(
            "claude",
            Some("bbbbbbbb-3333-4444"),
            Some("claude:zz"),
            true,
        );
        assert!(a.as_deref().is_some_and(|s| s.starts_with("claude:")));
        assert_ne!(a, b, "distinct pins → distinct suffixes");
        assert_ne!(
            a.as_deref(),
            Some("claude:zz"),
            "pin overrides the cached proximity id"
        );
    }

    #[test]
    fn unpinned_uses_cache_when_it_matches_else_bare_label() {
        assert_eq!(
            super::choose_status_suffix("claude", None, Some("claude:zz"), true),
            Some("claude:zz".to_string()),
        );
        // Cache is for another pane (no match) → bare label until this pane's
        // own resolve lands.
        assert_eq!(
            super::choose_status_suffix("claude", None, Some("claude:zz"), false),
            Some("claude".to_string()),
        );
        assert_eq!(
            super::choose_status_suffix("codex", None, None, false),
            Some("codex".to_string()),
        );
    }
}

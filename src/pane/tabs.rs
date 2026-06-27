//! Multi-tab management for the lower pane.
//!
//! `PaneTabs` wraps a `Vec<TabEntry>` and an active-tab index, keeping
//! all tab lifecycle logic out of `App`.

use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use super::Pane;

/// Three-phase scheduling for the `/resume <sid>` keystroke injection
/// that session restore uses to recover a Claude conversation. Each
/// variant carries the time the next write should fire so the App
/// event loop can drain pending sends each tick without per-tab
/// timers.
pub enum PendingResumeSend {
    /// Initial state right after spawn: wait for Claude's banner to
    /// finish rendering before typing anything. When the deadline
    /// passes we write `/resume <sid>` (no Enter) and transition to
    /// [`Self::Enter`].
    Text {
        sid: String,
        after: std::time::Instant,
    },
    /// Text has been written. After a small additional delay we
    /// write `\r` so the prompt actually submits. Splitting the
    /// write avoids the intermittent race where Claude's TUI was
    /// mid-render and dropped the trailing `\r` from a combined
    /// send.
    Enter {
        sid: String,
        after: std::time::Instant,
    },
    /// Enter has been written, but Claude's TUI intermittently drops
    /// a `\r` that lands during async startup work (MCP connects,
    /// version check, org-message fetch — each can remount the input
    /// component). No fixed delay wins that race, so we close the
    /// loop: at the deadline, scan the pane tail for the still-typed
    /// `/resume <sid>` and re-send `\r` while it's visible. The guard
    /// makes retries safe — once the prompt submits the sid leaves
    /// the screen, and a stray `\r` can't fire into the resumed
    /// session. Gives up after `retries_left` attempts (the user can
    /// recover by pressing Enter themselves).
    Verify {
        sid: String,
        after: std::time::Instant,
        retries_left: u8,
    },
}

/// True when the typed `/resume <sid>` is still sitting unsubmitted
/// in the pane's prompt — i.e. the sid is visible in the screen tail.
/// On submit Claude clears the input line, so the sid disappearing is
/// the "it worked" signal. Per-line `contains` doesn't survive a
/// hard wrap mid-sid, but the command is ~45 cols and real panes are
/// wider; a missed match just means no retry (today's behavior).
pub fn resume_still_unsubmitted(tail_lines: &[String], sid: &str) -> bool {
    tail_lines.iter().any(|l| l.contains(sid))
}

/// Activity state of a tab's process, shown as a colored dot per **agent** tab
/// in the divider (`App::settle_agent_activity`).
///
/// Two sources feed it (`docs/AGENT_AWARENESS_PLAN.md`): the coarse P0
/// *output-timing* signal (`Working` while output flows, else `Idle`), and the
/// P1 *semantic self-report* over the `report_status` MCP tool, which a
/// cooperative agent uses to assert `Working` (even through a silent thinking
/// pause), `Blocked` (waiting on the user — the founding "which agent needs me"
/// signal), or `Done`. A live report wins over timing (see
/// [`ReportedStatus`]); when none is active, timing drives Working/Idle.
/// `Unknown` is a non-agent tab or one with no signal yet — no dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AgentActivity {
    /// Doing something — output flowing, or a `working` self-report.
    Working,
    /// Quiet (timing), or an `idle` self-report.
    Idle,
    /// Waiting on the user (a `blocked` self-report) — "needs me". Semantic only.
    Blocked,
    /// Finished a turn (a `done` self-report). Semantic only.
    Done,
    /// Not an agent tab, or no signal yet. Renders no dot.
    #[default]
    Unknown,
}

/// A semantic activity self-report from a cooperative agent via the
/// `report_status` MCP tool — the P1 channel that supersedes output timing.
///
/// Authority model (`App::effective_activity`): a report wins over the timing
/// fallback **until** it expires (`expiry`, a backstop so a crashed agent's
/// stale `working`/`blocked` doesn't stick forever) **or** the tab produces
/// fresh output *after* the report (`at`) — new output means the agent resumed,
/// so timing takes back over. `at` also lets a newer report supersede an older.
#[derive(Clone, Copy, Debug)]
pub struct ReportedStatus {
    /// The reported state (`Working` / `Blocked` / `Idle` / `Done`).
    pub status: AgentActivity,
    /// When the report was received (monotonic) — beaten by newer output.
    pub at: std::time::Instant,
    /// Backstop expiry; after this the dot falls back to output timing.
    pub expiry: std::time::Instant,
}

/// Per-tab metadata displayed in the status line.
pub struct TabInfo {
    /// Full command string passed to `Pane::spawn`.
    pub command: String,
    /// Short display name — defaults to first word of command, user can rename.
    pub label: String,
    /// Working directory at spawn time.
    pub cwd: PathBuf,
    /// True when a background tab received output since last viewed.
    pub has_activity: bool,
    /// Monotonic instant of this tab's most recent pane output, stamped in
    /// `App::drain_pane_output`. `None` until the first output. Drives
    /// [`AgentActivity`] via `App::settle_agent_activity`.
    pub last_output_at: Option<std::time::Instant>,
    /// Cached activity state, recomputed OFF the draw path in
    /// `App::settle_agent_activity` (render is pure and can't read `now`).
    pub activity: AgentActivity,
    /// Latest semantic self-report from the agent (`report_status` MCP tool),
    /// or `None`. Overrides output timing per the [`ReportedStatus`] authority
    /// model; settle clears it once expired / superseded by fresh output.
    pub reported: Option<ReportedStatus>,
    /// Set when the tab was spawned by session restore as a `claude
    /// --resume`. On a non-zero exit shortly after spawn we treat the
    /// resume as failed and replace the tab with a fresh spawn of this
    /// fallback command.
    pub restore_fallback: Option<String>,
    /// Set on session restore when we want claude to resume a specific
    /// conversation: spawn a *fresh* `claude` (the `--resume` CLI flag
    /// trips a known regression that crashes at mount), then once
    /// claude has had time to finish its banner, type `/resume <sid>`
    /// followed by Enter — the slash-command path doesn't hit the bug.
    /// Three-phase: text and Enter go as separate writes (Claude's
    /// TUI mid-render would drop the trailing `\r` from a combined
    /// send), then a verify pass re-sends `\r` while the typed
    /// command is still visibly unsubmitted — async startup work can
    /// eat a lone Enter seconds after the banner looks settled.
    pub pending_resume_send: Option<PendingResumeSend>,
    /// When the tab's subprocess was launched. Bounds the
    /// restore-fallback window so a real user-driven exit much later
    /// doesn't trigger an automatic respawn.
    pub spawn_at: std::time::Instant,
    /// Wall-clock spawn time, in epoch seconds. Used at session-save
    /// time to disambiguate which `~/.claude/sessions/*.json` record
    /// belongs to *this* pane when multiple Claude tabs share a cwd
    /// — the matching session record's `startedAt` is closest to
    /// this value. `Instant::now()` (above) is monotonic and can't
    /// be compared against wall-clock data, so we record both.
    pub spawn_epoch_secs: u64,
    /// Codex session uuid pinned to this tab (Option B — `app::codex_pin`).
    /// Set at launch for a `codex resume <uuid>` pane, else filled by the
    /// spawn-time scan once codex writes its rollout. `^a v` resolves to this
    /// exact rollout when present (the strongest signal). `None` for non-codex
    /// tabs and codex tabs not yet pinned.
    pub codex_session_id: Option<String>,
}

impl TabInfo {
    pub fn new(command: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        let command = command.into();
        let label = command
            .split_whitespace()
            .next()
            .unwrap_or("???")
            .to_string();
        let cwd = cwd.into();
        Self {
            command,
            label,
            cwd,
            has_activity: false,
            last_output_at: None,
            activity: AgentActivity::Unknown,
            reported: None,
            restore_fallback: None,
            pending_resume_send: None,
            spawn_at: std::time::Instant::now(),
            spawn_epoch_secs: crate::sysinfo::epoch_secs(),
            codex_session_id: None,
        }
    }
}

/// A single tab: a `Pane` plus its metadata.
pub struct TabEntry {
    pub pane: Pane,
    pub info: TabInfo,
    /// Last-known live cwd of the child. A plain cache field: the pure
    /// `live_cwd` draw read clones it, and the `&mut refresh_live_cwd` kick
    /// (driven from `prepare_panes`, the pre-draw settle point) updates it
    /// from `live_cwd_pending` when a background refresh lands. `None` until
    /// the first refresh succeeds.
    live_cwd_cache: Option<PathBuf>,
    /// Slot a detached refresh thread writes a freshly-resolved cwd into.
    /// `cwd_for_pid` is a `lsof` fork-exec on macOS (~5–17 ms); running it
    /// inline in the render path stalled the whole event loop once per
    /// `LIVE_CWD_TTL` (a visible per-second typing hitch), so it now runs
    /// off-thread and the result is picked up on a later frame.
    live_cwd_pending: Arc<Mutex<Option<PathBuf>>>,
    /// True while a background cwd refresh is in flight, so we kick at most
    /// one at a time.
    live_cwd_refreshing: Arc<AtomicBool>,
    /// When we last kicked a refresh — the `LIVE_CWD_TTL` gate.
    live_cwd_kicked_at: Option<std::time::Instant>,
    /// Stashed `^a-v` scrollback pager. Holds the whole `PagerView`
    /// (scroll position, search state, visual selection, line buffer
    /// snapshot — everything) while the user is on another tab so a
    /// round-trip "scroll back, tab away, tab back" lands the pager
    /// exactly as the user left it. App-level `self.pager` carries
    /// at most one pager at a time; tab-switch swaps this slot in
    /// and out. `None` when the tab has no scrollback view stashed.
    pub stashed_scrollback_pager: Option<crate::ui::pager::PagerView>,
}

/// How long a resolved live-cwd is reused before kicking a background
/// re-poll. The lookup runs off-thread (see `live_cwd`), so this only
/// bounds how often a refresh thread is spawned, not any render cost.
const LIVE_CWD_TTL: std::time::Duration = std::time::Duration::from_secs(1);

/// Strip the ` [exited <N>]` suffix that [`PaneTabs::mark_exited`]
/// appends to dead-tab labels for display purposes. Returns the
/// label unchanged if no suffix is present.
///
/// Why this exists: the exit-status display is *runtime UI state*,
/// not persistent identity. Session save serializes
/// `TabEntry::info.label`, so without stripping, a tab that exited
/// at any point during the session ends up with a `[exited N]`
/// suffix glued onto its name in the JSON. On `spyc -r` the tab
/// respawns alive but the saved label is reapplied verbatim — the
/// user sees their freshly-running `htop` tagged "exited 0" until
/// they manually rename it (reported: "htop is actually still
/// running - the status is stale from resuming the session").
///
/// Callers apply this at both save and restore boundaries: save
/// strips so new sessions land clean; restore strips defensively
/// so older session files heal automatically the next time they
/// load.
pub fn strip_exit_suffix(label: &str) -> String {
    // mark_exited writes `format!("{} [exited {}]", label, code)`
    // where `code` is either a decimal integer or `"?"`. The
    // marker substring " [exited " is reserved (any user-set label
    // containing it is recovering display behavior they likely
    // didn't intend anyway), and the suffix is always at the *end*
    // of the label.
    if let Some((base, _)) = label.rsplit_once(" [exited ")
        && label.ends_with(']')
    {
        return base.to_string();
    }
    label.to_string()
}

impl TabEntry {
    pub fn new(pane: Pane, info: TabInfo) -> Self {
        Self {
            pane,
            info,
            live_cwd_cache: None,
            live_cwd_pending: Arc::new(Mutex::new(None)),
            live_cwd_refreshing: Arc::new(AtomicBool::new(false)),
            live_cwd_kicked_at: None,
            stashed_scrollback_pager: None,
        }
    }

    /// PURE `&self` read for the draw pass: the last resolved live cwd of the
    /// subprocess, or the spawn-time cwd until the first refresh lands. Never
    /// blocks, never spawns, never mutates — the landed-result pickup and the
    /// off-thread `cwd_for_pid` kick live in `refresh_live_cwd`, run from
    /// `prepare_panes` (the `&mut` pre-draw settle point). Keeping this pure is
    /// what lets `render_pane_status_line` honor the "render mutates nothing"
    /// contract (a TestBackend snapshot render no longer forks `lsof`).
    pub fn live_cwd(&self) -> std::path::PathBuf {
        self.live_cwd_cache
            .clone()
            .unwrap_or_else(|| self.info.cwd.clone())
    }

    /// `&mut` settle step (called from `prepare_panes`, NOT the draw): pick up
    /// any cwd a background refresh has landed, then kick a fresh lookup when
    /// the cached value is stale and none is in flight. `cwd_for_pid` is a
    /// `lsof` fork-exec on macOS (~5–17 ms) — it runs on a detached thread, so
    /// this never blocks the loop; the result is picked up here on a later
    /// frame. The render path used to do all this inline behind `&self`,
    /// stalling the event loop ~once per `LIVE_CWD_TTL`.
    pub fn refresh_live_cwd(&mut self) {
        // Pick up a result a background refresh has landed. Bind the `take()`
        // first so the `MutexGuard` drops before the body (no lock held).
        let landed = self.live_cwd_pending.lock().unwrap().take();
        if landed.is_some() {
            self.live_cwd_cache = landed;
        }
        let now = std::time::Instant::now();
        let stale = self
            .live_cwd_kicked_at
            .is_none_or(|at| now.duration_since(at) >= LIVE_CWD_TTL);
        if stale
            && !self.live_cwd_refreshing.load(Ordering::Acquire)
            && let Some(pid) = self.pane.process_id()
        {
            self.live_cwd_kicked_at = Some(now);
            self.live_cwd_refreshing.store(true, Ordering::Release);
            let pending = Arc::clone(&self.live_cwd_pending);
            let refreshing = Arc::clone(&self.live_cwd_refreshing);
            std::thread::spawn(move || {
                if let Some(cwd) = crate::proc_cwd::cwd_for_pid(pid) {
                    *pending.lock().unwrap() = Some(cwd);
                }
                refreshing.store(false, Ordering::Release);
            });
        }
    }
}

/// Container for multiple pane tabs.
pub struct PaneTabs {
    tabs: Vec<TabEntry>,
    active: usize,
    /// Index of the tab that was active *before* the current one, for
    /// screen/tmux-style "last window" jumps (`^a ^a`). `None` until
    /// the user has switched at least once, or when a removal would
    /// leave it dangling. Updated on every genuine change of `active`.
    last_active: Option<usize>,
}

impl PaneTabs {
    /// Create a new tab container with one initial tab.
    pub fn new(entry: TabEntry) -> Self {
        Self {
            tabs: vec![entry],
            active: 0,
            last_active: None,
        }
    }

    /// Move `active` to `idx`, remembering the prior tab as
    /// `last_active` when it's a real switch. Caller guarantees the
    /// container is non-empty and `idx` is in range. Clears the
    /// activity flag on the newly active tab.
    fn activate(&mut self, idx: usize) {
        if idx != self.active {
            self.last_active = Some(self.active);
        }
        self.active = idx;
        self.tabs[self.active].info.has_activity = false;
    }

    pub fn active(&self) -> &Pane {
        &self.tabs[self.active].pane
    }

    pub fn active_mut(&mut self) -> &mut Pane {
        &mut self.tabs[self.active].pane
    }

    /// `&mut` access to the active tab (not just its pane) — used by the
    /// pre-draw `prepare_panes` settle step to kick the active tab's live-cwd
    /// refresh off the render thread.
    pub fn active_tab_mut(&mut self) -> &mut TabEntry {
        &mut self.tabs[self.active]
    }

    pub fn active_info(&self) -> &TabInfo {
        &self.tabs[self.active].info
    }

    pub fn active_info_mut(&mut self) -> &mut TabInfo {
        &mut self.tabs[self.active].info
    }

    /// Direct mutable access to the active `TabEntry` — for callers
    /// that need to touch per-tab state outside the `Pane` and
    /// `TabInfo` projections above (e.g. the scrollback-resume scroll
    /// memory used by `^a-v` ↔ tab-switch).
    pub fn active_entry_mut(&mut self) -> &mut TabEntry {
        &mut self.tabs[self.active]
    }

    pub const fn len(&self) -> usize {
        self.tabs.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub const fn active_index(&self) -> usize {
        self.active
    }

    /// Switch to tab at `idx` (0-indexed). Clamped to valid range.
    /// Clears the activity flag on the newly active tab.
    pub fn switch_to(&mut self, idx: usize) {
        if !self.tabs.is_empty() {
            self.activate(idx.min(self.tabs.len() - 1));
        }
    }

    /// Jump back to the previously-active tab (`^a ^a`). Returns `true`
    /// if a jump happened, `false` when there's no valid prior tab
    /// (only one tab, never switched, or the prior tab was closed).
    /// The jump is itself a switch, so it swaps `active`/`last_active`
    /// — pressing it again toggles back.
    pub fn switch_to_last(&mut self) -> bool {
        if let Some(prev) = self.last_active
            && prev < self.tabs.len()
            && prev != self.active
        {
            self.activate(prev);
            return true;
        }
        false
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.activate((self.active + 1) % self.tabs.len());
        }
    }

    pub fn prev(&mut self) {
        if !self.tabs.is_empty() {
            let target = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
            self.activate(target);
        }
    }

    /// Add a new tab and switch to it. The tab we were on becomes
    /// `last_active`, so `^a ^a` right after opening a tab returns to
    /// where you were.
    pub fn push(&mut self, entry: TabEntry) {
        self.last_active = Some(self.active);
        self.tabs.push(entry);
        self.active = self.tabs.len() - 1;
    }

    /// Remove the active tab. Returns `true` if tabs remain, `false` if
    /// the last tab was removed (caller should tear down the pane area).
    pub fn close_active(&mut self) -> bool {
        self.remove_at(self.active)
    }

    /// Drain output from *every* tab so background tabs don't lose data.
    /// Sets `has_activity` on background tabs that received new output.
    pub fn drain_all(&mut self) {
        for (i, entry) in self.tabs.iter_mut().enumerate() {
            let had_bytes = entry.pane.drain_output();
            if had_bytes && i != self.active {
                entry.info.has_activity = true;
            }
        }
    }

    /// Fix up `last_active` after the tab at `removed` is dropped:
    /// invalidate it if it pointed at that tab, slide it left if it sat
    /// after it.
    const fn fixup_last_active_after_remove(&mut self, removed: usize) {
        self.last_active = match self.last_active {
            Some(x) if x == removed => None,
            Some(x) if x > removed => Some(x - 1),
            other => other,
        };
    }

    /// Mark exited tabs with their exit code. Returns `true` if any
    /// tab was newly marked (caller should trigger a redraw).
    /// The suffix this appends is recognized by
    /// [`strip_exit_suffix`] so callers serializing tab labels (e.g.
    /// session save) can drop the runtime-only annotation before
    /// writing to disk.
    pub fn mark_exited(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.tabs {
            if entry.pane.is_closed() && !entry.info.label.contains("[exited") {
                // Retry exit status harvest if drain_output missed it.
                entry.pane.try_harvest_exit_status();
                let code = entry
                    .pane
                    .exit_status()
                    .map_or_else(|| "?".to_string(), |s| s.exit_code().to_string());
                entry.info.label = format!("{} [exited {}]", entry.info.label, code);
                changed = true;
            }
        }
        changed
    }

    /// Replace the tab at `idx` in place. Active index and the order of
    /// remaining tabs are preserved. No-op if `idx` is out of range.
    pub fn replace_at(&mut self, idx: usize, entry: TabEntry) {
        if idx < self.tabs.len() {
            self.tabs[idx] = entry;
        }
    }

    /// Remove the tab at `idx`. Returns `true` if tabs remain, `false` if
    /// the container is now empty (caller should tear down the pane area).
    /// Active index follows the removed tab when the active tab itself is
    /// removed; otherwise it shifts to keep pointing at the same tab.
    ///
    /// Tears down the removed tab's child tree before dropping it —
    /// SIGTERM the process group, 250ms grace, then SIGKILL. Without
    /// this an `^a x` on a tab running `npm run dev` (or anything
    /// with subprocesses) would orphan the whole tree because
    /// `portable_pty::Child`'s default Drop is a no-op.
    ///
    /// The shutdown runs on a **detached thread** (`shutdown_detached`) so the
    /// input thread doesn't freeze 20-250 ms while a child winds down — the
    /// tab disappears from the UI immediately and the reap finishes in the
    /// background. `PtyHost::Drop`'s hard SIGKILL remains the backstop. (App
    /// *exit* keeps a synchronous shutdown — see `run_teardown` — to avoid
    /// orphaning children when the process is about to die.)
    pub fn remove_at(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return !self.tabs.is_empty();
        }
        let entry = self.tabs.remove(idx);
        entry
            .pane
            .shutdown_detached(std::time::Duration::from_millis(250));
        self.fixup_last_active_after_remove(idx);
        if self.tabs.is_empty() {
            return false;
        }
        if idx < self.active {
            self.active -= 1;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        true
    }

    /// Take the active `TabEntry` out of the container *without*
    /// shutting down its pty. Used by v1.5 Phase 6c demotion
    /// (`:pane-to-task`): the pty keeps running, the entry just
    /// stops being a tab and becomes a `BackgroundTask`. Returns
    /// `None` when there are no tabs.
    ///
    /// Because the removed index is always `self.active`, the only fixup
    /// needed is clamping when it was the last tab — `remove_at`'s
    /// `removed-idx < active` slide-left case can't occur here (idx == active),
    /// so it's omitted.
    pub fn take_active(&mut self) -> Option<TabEntry> {
        if self.tabs.is_empty() {
            return None;
        }
        let idx = self.active;
        let entry = self.tabs.remove(idx);
        self.fixup_last_active_after_remove(idx);
        if !self.tabs.is_empty() && self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        Some(entry)
    }

    /// Slice of all tab entries (for rendering the tab bar).
    pub fn tabs_mut(&mut self) -> &mut [TabEntry] {
        &mut self.tabs
    }

    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }
}

#[cfg(test)]
mod resume_verify_tests {
    use super::resume_still_unsubmitted;

    const SID: &str = "6b52fc7f-22f3-45e5-a3cd-32df70953197";

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn detects_unsubmitted_prompt() {
        let tail = lines(&[
            "  You've used 79% of your usage credits",
            "",
            &format!("> /resume {SID}"),
            "",
            "  -- INSERT --",
        ]);
        assert!(resume_still_unsubmitted(&tail, SID));
    }

    #[test]
    fn clear_prompt_means_submitted() {
        // After submit the input line clears and the transcript
        // renders — the sid is gone from the tail.
        let tail = lines(&["  Resuming conversation…", "", "> ", "  -- INSERT --"]);
        assert!(!resume_still_unsubmitted(&tail, SID));
    }

    #[test]
    fn autocomplete_popup_without_sid_does_not_match() {
        // The slash-command popup shows "/resume" but never the sid;
        // only the input line carries it.
        let tail = lines(&["> /res", "  /resume  Resume a conversation"]);
        assert!(!resume_still_unsubmitted(&tail, SID));
    }
}

#[cfg(test)]
mod tests {
    use super::{PaneTabs, TabEntry, TabInfo, strip_exit_suffix};
    use crate::pane::Pane;

    /// A long-lived dummy tab: `cat` blocks reading stdin so the pty
    /// stays open for the lifetime of the test.
    fn dummy_tab() -> TabEntry {
        let tmp = std::env::temp_dir();
        let ctx = tmp.join("spyc-tabs-test-context.json");
        let pane = Pane::spawn("cat", 24, 80, &tmp, &ctx, std::sync::Arc::new(|| {}))
            .expect("spawn dummy pane");
        TabEntry::new(pane, TabInfo::new("cat", &tmp))
    }

    fn tabs_with(n: usize) -> PaneTabs {
        let mut tabs = PaneTabs::new(dummy_tab());
        for _ in 1..n {
            tabs.push(dummy_tab());
        }
        tabs
    }

    #[test]
    fn last_tab_toggles_between_two() {
        let mut tabs = tabs_with(3);
        // push() left us on tab 2 with last_active = 1.
        assert_eq!(tabs.active_index(), 2);
        tabs.switch_to(0); // now on 0, last = 2
        assert_eq!(tabs.active_index(), 0);

        assert!(tabs.switch_to_last()); // -> 2
        assert_eq!(tabs.active_index(), 2);
        assert!(tabs.switch_to_last()); // -> 0 (toggle back)
        assert_eq!(tabs.active_index(), 0);
        assert!(tabs.switch_to_last()); // -> 2
        assert_eq!(tabs.active_index(), 2);
    }

    #[test]
    fn last_tab_noops_without_prior() {
        let mut tabs = PaneTabs::new(dummy_tab());
        // Single tab, never switched: nothing to jump back to.
        assert!(!tabs.switch_to_last());
        assert_eq!(tabs.active_index(), 0);
    }

    #[test]
    fn switching_to_same_tab_does_not_arm_last() {
        let mut tabs = tabs_with(2); // on tab 1, last = 0
        tabs.switch_to(1); // no-op switch to current
        // last_active should still be 0 (the genuine prior), so a jump
        // lands on 0 — switch_to(current) must not overwrite it with 1.
        assert!(tabs.switch_to_last());
        assert_eq!(tabs.active_index(), 0);
    }

    #[test]
    fn removing_last_active_tab_invalidates_jump() {
        let mut tabs = tabs_with(3);
        tabs.switch_to(0); // on 0, last = 2
        tabs.switch_to(1); // on 1, last = 0
        // Remove tab 0 (the last-active). active 1 slides to 0; the
        // jump target is gone, so switch_to_last is a no-op.
        tabs.remove_at(0);
        assert_eq!(tabs.active_index(), 0);
        assert!(!tabs.switch_to_last());
    }

    #[test]
    fn removing_lower_tab_reindexes_last_active() {
        let mut tabs = tabs_with(4); // on 3
        tabs.switch_to(2); // on 2, last = 3
        tabs.switch_to(0); // on 0, last = 2
        // Remove tab 1 (below both active=0? no, above active). It sits
        // below last_active(2), so last_active should slide to 1.
        tabs.remove_at(1);
        assert!(tabs.switch_to_last());
        assert_eq!(tabs.active_index(), 1); // formerly tab 2
    }

    #[test]
    fn strips_numeric_exit_code() {
        assert_eq!(strip_exit_suffix("claude [exited 0]"), "claude");
        assert_eq!(strip_exit_suffix("htop [exited 130]"), "htop");
    }

    #[test]
    fn strips_question_mark_exit() {
        // mark_exited writes "?" when exit_status() is None.
        assert_eq!(strip_exit_suffix("zsh [exited ?]"), "zsh");
    }

    #[test]
    fn passes_through_label_without_suffix() {
        assert_eq!(strip_exit_suffix("claude"), "claude");
        assert_eq!(strip_exit_suffix(""), "");
        assert_eq!(strip_exit_suffix("npm run dev"), "npm run dev");
    }

    #[test]
    fn only_strips_the_trailing_suffix() {
        // A label that happens to contain "[exited" in the middle
        // (weird but plausible if user named it) is unaffected.
        assert_eq!(
            strip_exit_suffix("note about [exited stuff] here"),
            "note about [exited stuff] here"
        );
    }

    #[test]
    fn handles_nested_suffix_idempotently() {
        // Double-call should be a no-op after the first strip.
        let once = strip_exit_suffix("claude [exited 0]");
        let twice = strip_exit_suffix(&once);
        assert_eq!(once, "claude");
        assert_eq!(twice, "claude");
    }

    #[test]
    fn requires_terminating_bracket() {
        // No closing `]` means it wasn't our suffix; leave alone.
        assert_eq!(strip_exit_suffix("claude [exited 0"), "claude [exited 0");
    }

    #[test]
    fn live_cwd_reads_cache_purely_and_refresh_picks_up_landed() {
        use std::path::PathBuf;
        use std::sync::atomic::Ordering;
        let mut entry = dummy_tab();
        let spawn_cwd = entry.info.cwd.clone();
        // The pure `&self` draw read returns the spawn-time cwd until the
        // first refresh lands — and never mutates / spawns.
        assert_eq!(entry.live_cwd(), spawn_cwd);

        // Seed a landed result and mark a refresh already in flight, so the
        // `&mut` settle step only PICKS UP the landed value (no
        // nondeterministic `lsof` kick), then the pure read reflects it.
        let landed = PathBuf::from("/tmp/spyc-live-cwd-test");
        *entry.live_cwd_pending.lock().unwrap() = Some(landed.clone());
        entry.live_cwd_refreshing.store(true, Ordering::Release);
        entry.refresh_live_cwd();
        assert_eq!(entry.live_cwd(), landed);
    }
}

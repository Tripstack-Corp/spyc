//! Non-input event sources feeding `App::run` (MVU Phase 3a,
//! `docs/MVU_PLAN.md`).
//!
//! The fs-watcher and git worker push onto the unified `Message` channel
//! (the watcher via a closure `EventHandler`, the git worker via a
//! forwarder thread spawned in `run()`). This module holds the run-loop
//! side of those sources:
//!
//! - [`coalesce_pending`] — drains a burst of `FsEvent`/`GitResult` into
//!   the pending buffers in one wakeup, surfacing only an `Input`.
//! - [`App::ingest_fs_event`] / [`App::ingest_git_result`] — the unchanged
//!   pre-recv drain bodies, extracted so the recv-arm buffering and the
//!   drain can never diverge. They read App's private state directly via
//!   the descendant-module rule (no field is made `pub`).
//! - [`take_reader_result`] — the shared reader-death exit decision used by
//!   both the Timeout (reader_done) and Disconnected arms.
//!
//! The fs-watch *topology* (which dirs to watch on chdir) lives off-thread in
//! the [`super::watch`] worker; this module only handles event *delivery*.

use std::sync::Mutex;

use anyhow::Result;
use crossterm::event::Event;

use crate::spyc_debug;

use super::{App, Deadline, Message, RunCtx, TaskStatus, state};

/// MVU Phase 3a: drain every immediately-available message into the
/// pending buffers, returning the FIRST `Input` encountered (if any).
/// `FsEvent`/`GitResult` are buffered (processed by the next iteration's
/// unchanged pre-recv drains); `Tick` is dropped (advisory). Stops at the
/// first `Input` so Input stays one-per-iteration and FIFO — any messages
/// after it remain queued for the next `recv`.
pub fn coalesce_pending(
    rx: &std::sync::mpsc::Receiver<Message>,
    fs_pending: &mut Vec<notify::Event>,
    git_pending: &mut Vec<state::GitWorkerResult>,
    mcp_pending: &mut Vec<crate::mcp_cmd::McpRequest>,
) -> Option<Event> {
    while let Ok(m) = rx.try_recv() {
        match m {
            Message::FsEvent(e) => fs_pending.push(e),
            Message::GitResult(r) => git_pending.push(r),
            // MVU Phase 3d: an MCP request carries its reply Sender — it MUST
            // be buffered (dropping it strands the client on its 5s timeout),
            // never join the no-op drop arm below.
            Message::Mcp(req) => mcp_pending.push(req),
            // Every wake collapses to nothing here: the outcome rides a
            // `runtime.*` slot (or the producer's own channel) and the pre-recv
            // drains pick it up on re-entry, so a burst of them becomes a single
            // re-scan. One arm for all seventeen kinds — see `Message::Wake`.
            // A `Tick` is dropped for the same reason: `recv_timeout` returning
            // Timeout IS the tick handler.
            Message::Wake(_) | Message::Tick(_) => {}
            Message::Input(ev) => return Some(ev),
        }
    }
    None
}

/// Shared tail of every [`coalesce_recv`] arm: drain the rest of the burst
/// into the pending Vecs, then surface a coalesced keystroke as `Input` or
/// synthesize `Timeout` so the loop re-enters its pre-recv drains.
fn coalesce_tail(
    rx: &std::sync::mpsc::Receiver<Message>,
    ctx: &mut RunCtx,
) -> Result<Message, std::sync::mpsc::RecvTimeoutError> {
    coalesce_pending(
        rx,
        &mut ctx.fs_pending,
        &mut ctx.git_pending,
        &mut ctx.mcp_pending,
    )
    .map_or(Err(std::sync::mpsc::RecvTimeoutError::Timeout), |ev| {
        Ok(Message::Input(ev))
    })
}

/// MVU Phase 3a: having received, *coalesce* — buffer every
/// immediately-available FsEvent/GitResult/Mcp into the pending Vecs
/// (drained at the top of the next iteration) and surface only an
/// Input (or a Tick/Timeout/Disconnected) to the dispatch match.
/// This collapses a burst into a single wakeup and bounds Input
/// latency to one iteration. Input is NEVER handled inside the
/// coalesce loop (an `Effect::ForegroundExec` parks the reader /
/// re-inits the TUI), only surfaced for the arm — and the coalesce stops at
/// the first one, so Input stays one-per-iteration and FIFO.
///
/// Lives next to [`coalesce_pending`] so the recv-arm buffering and the
/// drain can never diverge (H9): the arms here push the just-received
/// payload, then `coalesce_pending` drains the rest of the burst.
pub fn coalesce_recv(
    recvd: Result<Message, std::sync::mpsc::RecvTimeoutError>,
    rx: &std::sync::mpsc::Receiver<Message>,
    ctx: &mut RunCtx,
) -> Result<Message, std::sync::mpsc::RecvTimeoutError> {
    match recvd {
        Ok(Message::FsEvent(ev)) => {
            ctx.fs_pending.push(ev);
            coalesce_tail(rx, ctx)
        }
        Ok(Message::GitResult(r)) => {
            ctx.git_pending.push(r);
            coalesce_tail(rx, ctx)
        }
        // MVU Phase 3d: buffer the MCP request (carries its reply
        // Sender), collapse companions, synthesize Timeout so the
        // pre-recv MCP drain executes it + replies.
        Ok(Message::Mcp(req)) => {
            ctx.mcp_pending.push(req);
            coalesce_tail(rx, ctx)
        }
        // Any wake: nothing to buffer, so collapse companions and synthesize a
        // Timeout, which puts control back at the loop top where the pre-recv
        // drains re-scan every source. One arm covers all seventeen kinds, and
        // that is what makes this safe to extend — the previous shape listed
        // them individually under a catch-all, so a missed variant surfaced as
        // `effective` and panicked (#402: a yank killed spyc). `Wake::Pane` /
        // `Wake::Sink` label which source woke us; logged for traceability, never
        // used to target a drain, since the drains re-scan everything anyway.
        Ok(Message::Wake(w)) => {
            spyc_debug!("wake: {w:?}");
            coalesce_tail(rx, ctx)
        }
        surfaced @ (Ok(Message::Input(_) | Message::Tick(_)) | Err(_)) => surfaced,
    }
}

/// MVU Phase 3a: the run loop's reader-death exit decision, shared by the
/// Timeout arm (gated on `reader_done`) and the Disconnected arm. Drains a
/// recorded fatal reader error into an `Err` (preserving the prior
/// `event::read()?` contract); `Ok(())` means a clean stop. `.take()`s the
/// error so it isn't propagated twice.
pub fn take_reader_result(read_err: &Mutex<Option<std::io::Error>>) -> Result<()> {
    // Take into a local so the mutex guard drops before the branch
    // (clippy::significant_drop_in_scrutinee, nursery + -D warnings).
    let fatal = read_err.lock().unwrap().take();
    match fatal {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}

/// True when `path` is at the listing level — the dir itself (macOS FSEvents
/// coalesces intra-dir changes onto it) or a direct child. Such paths bypass
/// the gitignore-excludes drop so the cwd the user is viewing stays an
/// always-current class even for gitignored entries (e.g. spyc's own
/// `.spyc-context-*` files); only gitignored *subtree* churn the user isn't
/// looking at is dropped.
fn is_cwd_level(path: &std::path::Path, listing_dir: &std::path::Path) -> bool {
    path == listing_dir || path.parent() == Some(listing_dir)
}

impl App {
    /// Drop FSEvents under gitignored build/cache subtrees (`target/`,
    /// `node_modules/`, `.claude/`, …) before they drive a refresh: the
    /// recursive watch can't skip those subtrees (one FSEvents stream for the
    /// whole tree), and their churn — a cargo build, a fuzz run — would flood
    /// the loop and starve the git poll, leaving dirty markers stale. Fails open
    /// (no repo / no matcher → keep everything).
    ///
    /// Survivors, judged per active column that's in a repo: paths at the
    /// listing level (the cwd the user is viewing — a changed visible row, e.g.
    /// spyc's own `.spyc-context-*`) stay, and the vertical-split preview file
    /// stays even from a gitignored subtree (else its save never reaches the
    /// live reload). Each column's checker judges only paths under ITS OWN tree,
    /// so a path outside both trees (the other column, config files, gitdirs) is
    /// left untouched for another pass.
    ///
    /// Extracted from `ingest_fs_and_maybe_refresh` so the *composed* drop (the
    /// `retain_mut` + the cwd-level / preview exemptions, across two columns) is
    /// unit-testable without a `RunCtx`.
    pub(crate) fn drop_gitignored_fs_events(&self, fs_pending: &mut Vec<notify::Event>) {
        let preview_path = self
            .view
            .right_pager
            .as_ref()
            .and_then(|v| v.source_path.clone());
        let cols: Vec<(std::path::PathBuf, std::path::PathBuf)> = self
            .state
            .active_sides()
            .filter_map(|s| {
                let c = self.state.col(s);
                c.git_cache
                    .current_repo_root
                    .clone()
                    .map(|root| (root, c.listing.dir.clone()))
            })
            .collect();
        for (root, listing_dir) in cols {
            crate::git::excludes::with_checker(&root, |is_excluded| {
                fs_pending.retain_mut(|ev| {
                    ev.paths.retain(|p| {
                        // Not under this column's tree → leave it for another
                        // column's pass (or keep it). Under this tree: keep
                        // cwd-level paths and the previewed file unconditionally;
                        // drop deeper gitignored-subtree churn.
                        !p.starts_with(&listing_dir)
                            || is_cwd_level(p, &listing_dir)
                            || preview_path.as_deref() == Some(p.as_path())
                            || !is_excluded(p)
                    });
                    !ev.paths.is_empty()
                });
            });
        }
    }

    /// MVU Phase 3a: fold one buffered watcher event into the
    /// listing-refresh debounce state. Extracted verbatim from the old
    /// pre-recv `rx.try_recv()` drain so the recv-arm buffering and this
    /// drain can never diverge. Stamps against the caller's `now_pre` (the
    /// per-iteration clock), matching the old per-event read position;
    /// bumps `activity.live.watcher_events` once per event (not per path).
    pub fn ingest_fs_event(
        &mut self,
        ev: &notify::Event,
        now_pre: std::time::Instant,
        needs_reload: &mut bool,
        last_event_at: &mut Option<std::time::Instant>,
        first_event_after_refresh: &mut Option<std::time::Instant>,
    ) {
        self.view.activity.live.watcher_events =
            self.view.activity.live.watcher_events.saturating_add(1);
        for p in &ev.paths {
            let listing = self.is_listing_path(p);
            let config = self.is_config_path(p);
            spyc_debug!(
                "watcher event: {} (listing={listing}, config={config}, kind={:?})",
                p.display(),
                ev.kind
            );
            if config {
                *needs_reload = true;
            }
            if listing {
                // Anchor the max-defer window at the FIRST event of this
                // busy stretch (don't bump on subsequent ones, or continuous
                // activity starves the refresh).
                if first_event_after_refresh.is_none() {
                    *first_event_after_refresh = Some(now_pre);
                }
                *last_event_at = Some(now_pre);
            }
        }
    }

    /// MVU Phase 6 PR-C: the pre-recv filesystem step — drain the buffered
    /// `fs_pending` FsEvents (via `ingest_fs_event`), reload config on a config
    /// hit, then run the trailing-debounce/max-defer listing refresh
    /// (`should_fire_refresh`) and keep the advisory `RefreshQuiet` deadline
    /// armed at the predicate edge. Verbatim move of the old inline block; the
    /// debounce state (`last_event_at` / `first_event_after_refresh` /
    /// `last_refresh`) is threaded as `&mut` so it persists across iterations.
    /// Returns whether a redraw is needed (the caller uses `draw_reason = 3`).
    pub(crate) fn ingest_fs_and_maybe_refresh(
        &mut self,
        now_pre: std::time::Instant,
        ctx: &mut RunCtx,
    ) -> bool {
        let mut needs_draw = false;
        let mut needs_reload = false;
        // Drop FSEvents under gitignored build/cache subtrees before ingesting
        // (cwd-level paths + the open preview file survive) — see
        // `drop_gitignored_fs_events`.
        self.drop_gitignored_fs_events(&mut ctx.fs_pending);
        let mut git_event = false;
        let mut preview_event = false;
        for ev in std::mem::take(&mut ctx.fs_pending) {
            git_event |= ev.paths.iter().any(|p| self.is_gitdir_status_path(p));
            preview_event |= ev.paths.iter().any(|p| self.is_preview_path(p));
            // A working-tree edit (any path that is NOT a `.git/index`/`HEAD`
            // change) must force the next git poll to re-walk the column(s) it
            // lands in — the poll's mtime short-circuit can't see it and the
            // listing refresh only covers the focused column, so an UNFOCUSED
            // column's markers would otherwise go stale (`git restore` /
            // external edit). Gitignored churn is already dropped above.
            for p in &ev.paths {
                if !self.is_gitdir_status_path(p) {
                    self.state.flag_worktree_rewalk_for_path(p);
                }
            }
            self.ingest_fs_event(
                &ev,
                now_pre,
                &mut needs_reload,
                &mut ctx.last_event_at,
                &mut ctx.first_event_after_refresh,
            );
        }
        if needs_reload {
            self.reload_config();
            needs_draw = true;
        }
        // A discrete git operation (commit / stage / checkout / branch switch)
        // changed `.git/index` or `HEAD` — not the bursty working-tree churn the
        // trailing debounce exists to coalesce. Refresh git markers NOW so the
        // view tracks real git state near-instantly instead of waiting out the
        // debounce / max-defer window (the 1 Hz poll was the only sub-debounce
        // backstop, so markers lagged ~0.5–1 s after a commit/checkout).
        if git_event && self.state.refresh_git_state() {
            needs_draw = true;
        }
        // The previewed file changed on disk — kick an off-thread re-render of
        // the right split column (the rebuilt view lands via
        // `apply_preview_reloads`, so no `needs_draw` here; the wake drives it).
        if preview_event {
            self.kick_preview_reload();
        }
        // 500 ms trailing debounce on the watcher-driven listing refresh.
        let refresh_quiet = std::time::Duration::from_millis(500);
        // Fire when the watcher quiets down OR the max-defer cap bites (see
        // `should_fire_refresh`) — continuous fs activity can't starve the
        // trailing debounce forever.
        let max_refresh_defer = refresh_quiet * 2;
        // Arm RefreshQuiet at the exact instant should_fire_refresh can first
        // return true (advisory — the predicate below still decides firing).
        match (ctx.last_event_at, ctx.first_event_after_refresh) {
            (Some(at), Some(first)) => ctx.scheduler.arm(
                Deadline::RefreshQuiet,
                (ctx.last_refresh + refresh_quiet)
                    .max((at + refresh_quiet).min(first + max_refresh_defer)),
            ),
            _ => ctx.scheduler.disarm(Deadline::RefreshQuiet),
        }
        if super::util::should_fire_refresh(
            ctx.last_event_at,
            ctx.last_refresh,
            ctx.first_event_after_refresh,
            now_pre,
            refresh_quiet,
            max_refresh_defer,
        ) {
            ctx.last_event_at = None;
            ctx.first_event_after_refresh = None;
            // Off-thread: the 50k-entry walk + sort would otherwise block the
            // event loop on every debounced burst. The refreshed listing lands
            // a tick later via `FileOpDone` → `apply_file_outcomes` (which sets
            // `needs_draw` + `context_dirty` then), so neither is set here.
            self.spawn_listing_refresh();
            ctx.last_refresh = now_pre;
            ctx.scheduler.disarm(Deadline::RefreshQuiet);
        }
        needs_draw
    }

    /// MVU Phase 3d: whether the ~1 Hz `CaptureTick` deadline should be armed
    /// — i.e. a streaming view whose elapsed-timer title must keep advancing
    /// now that `MAX_IDLE_CAP` is gone. True for a live `!cmd` capture, or a
    /// `:task N` viewer whose *viewed* task is genuinely still running
    /// (Running AND its host hasn't hit EOF — a closed-but-not-yet-finalized
    /// task must NOT re-pin the tick, or idle CPU never settles). Disarmed the
    /// instant the capture finishes / the viewed task exits, so idle draws
    /// stay at 0.
    pub(crate) fn capture_tick_should_arm(&self) -> bool {
        if self.runtime.pending_capture.is_some() {
            return true;
        }
        self.view
            .pager
            .as_ref()
            .and_then(|v| v.task_id)
            .is_some_and(|id| {
                self.runtime.background_tasks.tasks.iter().any(|t| {
                    t.id == id && matches!(t.status, TaskStatus::Running) && !t.host.closed
                })
            })
    }

    /// MVU Phase 3a: apply one buffered git-worker result — the SOLE
    /// apply/count/take site (the recv arm + coalesce only buffer). Bumps
    /// `activity.live.git_results` per delivered result (before the generation
    /// gate), records the request roundtrip on the first result after a
    /// request, then applies it (generation-/repo-gated inside
    /// `apply_git_worker_result`). Returns `true` when the apply changed
    /// state — the caller then redraws and marks the context dirty.
    /// Extracted verbatim from the old pre-recv `git_result_rx.try_recv()`
    /// drain.
    pub fn ingest_git_result(&mut self, result: state::GitWorkerResult) -> bool {
        self.view.activity.live.git_results = self.view.activity.live.git_results.saturating_add(1);
        // Roundtrip duration: when the requesting column sent it (set by
        // `git_file_statuses_cached`) vs. now.
        if let Some(sent) = self
            .state
            .col_mut(result.side)
            .git_cache
            .last_git_request_at
            .take()
        {
            self.view.activity.git_last_ms =
                u32::try_from(sent.elapsed().as_millis()).unwrap_or(u32::MAX);
        }
        self.apply_git_worker_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::scheduler::Deadline;
    // `Wake` is only constructed in tests here — the production code matches
    // `Message::Wake(..)`, which needs nothing imported.
    use super::super::Wake;
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    fn fs_event(path: &Path) -> notify::Event {
        notify::Event::new(notify::EventKind::Any).add_path(path.to_path_buf())
    }

    /// The gitignore-excludes pre-filter keeps cwd-level paths (the dir itself
    /// or a direct child) so a gitignored file created/removed there still
    /// refreshes the listing; only deeper subtree paths fall to the exclude
    /// check.
    #[test]
    fn cwd_level_paths_bypass_the_excludes_drop() {
        let dir = Path::new("/repo");
        assert!(is_cwd_level(dir, dir), "the dir itself (macOS coalesce)");
        assert!(
            is_cwd_level(Path::new("/repo/.spyc-context-1.json"), dir),
            "direct child — kept even if gitignored"
        );
        assert!(
            !is_cwd_level(Path::new("/repo/target/debug/foo.o"), dir),
            "deep subtree path — subject to the exclude check"
        );
        assert!(
            !is_cwd_level(Path::new("/other/foo"), dir),
            "outside the listing dir"
        );
    }

    fn git_result(generation: u64) -> state::GitWorkerResult {
        state::GitWorkerResult {
            generation,
            repo_root: PathBuf::from("/no/such/repo"),
            side: state::Side::Left,
            entries: None,
            index_mtime: None,
            head_mtime: None,
        }
    }

    #[test]
    fn coalesce_returns_first_input_and_buffers_the_rest() {
        let (tx, rx) = mpsc::channel::<Message>();
        tx.send(Message::FsEvent(fs_event(Path::new("/a"))))
            .unwrap();
        tx.send(Message::GitResult(git_result(0))).unwrap();
        tx.send(Message::Input(Event::FocusGained)).unwrap();
        tx.send(Message::Input(Event::FocusLost)).unwrap();

        let mut fs_pending = Vec::new();
        let mut git_pending = Vec::new();
        let mut mcp_pending = Vec::new();
        let got = coalesce_pending(&rx, &mut fs_pending, &mut git_pending, &mut mcp_pending);

        // First Input is surfaced; the fs/git before it are buffered.
        assert_eq!(got, Some(Event::FocusGained));
        assert_eq!(fs_pending.len(), 1);
        assert_eq!(git_pending.len(), 1);
        // The SECOND Input stays queued (one-per-iteration, FIFO). `Message`
        // isn't `PartialEq` (it wraps notify::Event / GitWorkerResult), so
        // match rather than assert_eq.
        match rx.try_recv() {
            Ok(Message::Input(Event::FocusLost)) => {}
            _ => panic!("expected the second Input (FocusLost) still queued"),
        }
    }

    #[test]
    fn coalesce_buffers_everything_when_no_input() {
        let (tx, rx) = mpsc::channel::<Message>();
        tx.send(Message::FsEvent(fs_event(Path::new("/a"))))
            .unwrap();
        tx.send(Message::FsEvent(fs_event(Path::new("/b"))))
            .unwrap();
        tx.send(Message::GitResult(git_result(0))).unwrap();
        tx.send(Message::Tick(Deadline::GitPoll)).unwrap();
        // MVU Phase 3d: pager-stream/finder wakes collapse to nothing buffered
        // (the data rides their own channels; the loop re-drains on re-entry).
        tx.send(Message::Wake(Wake::PagerStream)).unwrap();
        tx.send(Message::Wake(Wake::PagerStream)).unwrap();
        tx.send(Message::Wake(Wake::Find)).unwrap();
        // MVU Phase 3d: an MCP request carries its reply Sender — it MUST be
        // buffered into mcp_pending, NEVER dropped (else the client strands).
        let (reply, _reply_rx) = mpsc::channel();
        tx.send(Message::Mcp(crate::mcp_cmd::McpRequest {
            command: crate::mcp_cmd::McpCommand::ClearPicks,
            reply,
        }))
        .unwrap();

        let mut fs_pending = Vec::new();
        let mut git_pending = Vec::new();
        let mut mcp_pending = Vec::new();
        let got = coalesce_pending(&rx, &mut fs_pending, &mut git_pending, &mut mcp_pending);

        assert_eq!(got, None);
        assert_eq!(fs_pending.len(), 2);
        assert_eq!(git_pending.len(), 1); // Tick + Grep/Find wakes dropped, not buffered
        assert_eq!(mcp_pending.len(), 1, "MCP request buffered, not dropped");
    }

    /// Regression net: EVERY payloadless done-wake must collapse to a
    /// synthesized Timeout in `coalesce_recv` (its outcome rides a `runtime.*`
    /// slot / the worker buffer, drained by the pre-recv scan).
    ///
    /// This list is no longer the guard — it *was*, and it failed twice while
    /// saying so. A variant wired into `coalesce_pending` + the
    /// `dispatch_effective` unreachable arm but missed in `coalesce_recv` used to
    /// fall through a `other => other` catch-all, surface as `effective`, and
    /// panic on the unreachable: FileOpDone/InventoryDone (#514), then LuaDone,
    /// then ClipboardPasteDone/ClipboardCopyDone — each time by a human not
    /// adding a line to a list, including to the very list this comment used to
    /// tell them to update.
    ///
    /// `coalesce_recv` now enumerates what it surfaces instead, so a new
    /// `Message` variant is a compile error in all three places that must agree.
    /// What's left for a test is the behaviour itself, which exhaustiveness can't
    /// express: that these particular variants collapse rather than surface.
    #[test]
    fn coalesce_recv_collapses_all_payloadless_done_wakes() {
        let wakes = [
            Message::Wake(Wake::PagerStream),
            Message::Wake(Wake::Find),
            Message::Wake(Wake::ReaderExited),
            Message::Wake(Wake::AgentStatus),
            Message::Wake(Wake::Graveyard),
            Message::Wake(Wake::Image),
            Message::Wake(Wake::Archive),
            Message::Wake(Wake::PreviewReload),
            Message::Wake(Wake::WorktreeJob),
            Message::Wake(Wake::CodexSession),
            Message::Wake(Wake::FileOp),
            Message::Wake(Wake::Inventory),
            Message::Wake(Wake::Lua),
            Message::Wake(Wake::ClipboardPaste),
            Message::Wake(Wake::ClipboardCopy),
        ];
        for done in wakes {
            let (_tx, rx) = mpsc::channel::<Message>();
            let mut ctx = RunCtx::for_test();
            let out = coalesce_recv(Ok(done), &rx, &mut ctx);
            assert!(
                matches!(out, Err(mpsc::RecvTimeoutError::Timeout)),
                "a payloadless done-wake must collapse to Timeout, not surface as `effective`"
            );
        }
    }

    #[test]
    fn take_reader_result_clean_when_empty() {
        let read_err = Mutex::new(None);
        assert!(take_reader_result(&read_err).is_ok());
    }

    #[test]
    fn take_reader_result_propagates_and_drains_fatal() {
        let read_err = Mutex::new(Some(std::io::Error::other("boom")));
        assert!(take_reader_result(&read_err).is_err());
        // Drained — a second call is a clean stop, never a re-propagation.
        assert!(take_reader_result(&read_err).is_ok());
    }

    #[test]
    fn ingest_fs_event_counts_per_event_and_stamps_once() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let cwd = tmp.path().to_path_buf();
            let mut app = App::test_app(cwd.clone());
            let base = std::time::Instant::now();
            let mut needs_reload = false;
            let mut last_event_at = None;
            let mut first = None;

            // A file directly in the listing dir is a listing path.
            let p = cwd.join("changed.txt");
            for _ in 0..3 {
                app.ingest_fs_event(
                    &fs_event(&p),
                    base,
                    &mut needs_reload,
                    &mut last_event_at,
                    &mut first,
                );
            }

            // Counted once per event (not per path), stamped at `now_pre`,
            // and the max-defer anchor fixed at the FIRST event.
            assert_eq!(app.view.activity.live.watcher_events, 3);
            assert_eq!(last_event_at, Some(base));
            assert_eq!(first, Some(base));
            assert!(!needs_reload); // not a config path
        });
    }

    #[test]
    fn capture_tick_off_when_idle() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let app = App::test_app(tmp.path().to_path_buf());
            // No capture, no task viewer → nothing streams, so CaptureTick
            // stays disarmed. With MAX_IDLE_CAP gone (3d), this is what keeps
            // a fully-idle loop blocked on recv() with 0 wakes/sec.
            assert!(!app.capture_tick_should_arm());
        });
    }

    #[test]
    fn capture_tick_off_for_viewer_of_non_running_task() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            // A `:task N` pager whose viewed task isn't running must NOT arm
            // the tick (no task id 1 exists → the strongest negative).
            let mut view = crate::ui::pager::PagerView::new_plain("task", vec![]);
            view.task_id = Some(1);
            app.view.pager = Some(view);
            assert!(!app.capture_tick_should_arm());
        });
    }

    #[test]
    fn ingest_git_result_counts_every_delivery_even_when_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            // Generation mismatch (state starts at 0) → dropped, but still
            // counted per delivery (activity is "results seen", not "applied").
            assert!(!app.ingest_git_result(git_result(99)));
            assert!(!app.ingest_git_result(git_result(99)));
            assert_eq!(app.view.activity.live.git_results, 2);
        });
    }

    #[test]
    fn ingest_git_result_takes_request_stamp_once() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.left.git_cache.last_git_request_at = Some(std::time::Instant::now());
            app.ingest_git_result(git_result(99));
            // Recorded + cleared on the first result; a second doesn't panic
            // or re-take.
            assert!(app.state.left.git_cache.last_git_request_at.is_none());
            app.ingest_git_result(git_result(99));
            assert!(app.state.left.git_cache.last_git_request_at.is_none());
        });
    }
}

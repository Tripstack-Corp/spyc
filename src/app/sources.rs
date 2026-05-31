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
//! - [`sync_listing_watch`] / `pick_recursive_mode` — fs-watch topology
//!   (which dirs to watch on chdir). Unchanged by Phase 3a; the delivery
//!   mechanism (closure vs Sender) is orthogonal to the watch topology.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use crossterm::event::Event;

use crate::spyc_debug;

use super::{App, Message, state};

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
) -> Option<Event> {
    while let Ok(m) = rx.try_recv() {
        match m {
            Message::FsEvent(e) => fs_pending.push(e),
            Message::GitResult(r) => git_pending.push(r),
            // MVU Phase 3b: pane wakes carry no payload — drop them here;
            // the loop re-enters the pre-recv pane scan regardless, so a
            // wake burst collapses to a single re-scan (the worker-side
            // 0→1 CAS is the primary firehose collapse; this is the second).
            Message::PaneOutput { .. }
            | Message::SinkOutput { .. }
            | Message::GrepOutput
            | Message::FindOutput
            | Message::Tick(_) => {}
            Message::Input(ev) => return Some(ev),
        }
    }
    None
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

impl App {
    /// MVU Phase 3a: fold one buffered watcher event into the
    /// listing-refresh debounce state. Extracted verbatim from the old
    /// pre-recv `rx.try_recv()` drain so the recv-arm buffering and this
    /// drain can never diverge. Stamps against the caller's `now_pre` (the
    /// per-iteration clock), matching the old per-event read position;
    /// bumps `activity_watcher_events` once per event (not per path).
    pub fn ingest_fs_event(
        &mut self,
        ev: &notify::Event,
        now_pre: std::time::Instant,
        needs_reload: &mut bool,
        last_event_at: &mut Option<std::time::Instant>,
        first_event_after_refresh: &mut Option<std::time::Instant>,
    ) {
        self.activity_watcher_events = self.activity_watcher_events.saturating_add(1);
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

    /// MVU Phase 3a: apply one buffered git-worker result — the SOLE
    /// apply/count/take site (the recv arm + coalesce only buffer). Bumps
    /// `activity_git_results` per delivered result (before the generation
    /// gate), records the request roundtrip on the first result after a
    /// request, then applies it (generation-/repo-gated inside
    /// `apply_git_worker_result`). Returns `true` when the apply changed
    /// state — the caller then redraws and marks the context dirty.
    /// Extracted verbatim from the old pre-recv `git_result_rx.try_recv()`
    /// drain.
    pub fn ingest_git_result(&mut self, result: state::GitWorkerResult) -> bool {
        self.activity_git_results = self.activity_git_results.saturating_add(1);
        // Roundtrip duration: when the request was sent (set by
        // `git_file_statuses_cached`) vs. now.
        if let Some(sent) = self.state.last_git_request_at.take() {
            self.activity_git_last_ms =
                u32::try_from(sent.elapsed().as_millis()).unwrap_or(u32::MAX);
        }
        self.apply_git_worker_result(result)
    }
}

/// Linux gates `Recursive` behind the `MAX_RECURSIVE_WATCH_DIRS` cap
/// to avoid blocking the main thread on `inotify_add_watch` walks
/// through `$HOME`-shaped trees. On other platforms (macOS FSEvents,
/// Windows ReadDirectoryChangesW), recursive watches are OS-level
/// and cheap, so `Recursive` is returned unconditionally.
#[cfg(target_os = "linux")]
fn pick_recursive_mode(new_dir: &Path) -> notify::RecursiveMode {
    use super::{MAX_RECURSIVE_WATCH_DIRS, count_subdirs_capped};
    use notify::RecursiveMode;
    if count_subdirs_capped(new_dir, MAX_RECURSIVE_WATCH_DIRS) > MAX_RECURSIVE_WATCH_DIRS {
        crate::spyc_debug!(
            "watcher: {} has > {} subdirs, using non-recursive watch (parent-row dirty refresh falls back to 1 Hz git poll)",
            new_dir.display(),
            MAX_RECURSIVE_WATCH_DIRS,
        );
        RecursiveMode::NonRecursive
    } else {
        RecursiveMode::Recursive
    }
}

#[cfg(not(target_os = "linux"))]
const fn pick_recursive_mode(_new_dir: &Path) -> notify::RecursiveMode {
    notify::RecursiveMode::Recursive
}

pub fn sync_listing_watch(
    fs_watcher: Option<&mut notify::RecommendedWatcher>,
    active: &mut Option<PathBuf>,
    active_git: &mut Option<PathBuf>,
    new_dir: &Path,
    gitdir: Option<&Path>,
) {
    use notify::{RecursiveMode, Watcher};
    let Some(w) = fs_watcher else {
        return;
    };
    if active.as_deref() != Some(new_dir) {
        if let Some(old) = active.as_ref() {
            let _ = w.unwatch(old);
        }
        // Recursive (when feasible): catches changes anywhere below
        // the listing dir so git status markers update on the parent
        // directory row when a file is added/modified in a
        // subdirectory (e.g. touching `docs/foo.md` while sitting at
        // the repo root). Events under `.git/` are filtered to
        // specific files (`index`, `HEAD`) by `is_listing_path` to
        // avoid `.git/objects` / pack / lockfile churn cascading into
        // needless `git status` calls.
        //
        // On Linux, `pick_recursive_mode` downgrades to non-recursive
        // when the subtree exceeds `MAX_RECURSIVE_WATCH_DIRS` —
        // otherwise `notify`'s synchronous per-subdir
        // `inotify_add_watch` walk blocks the main thread (the
        // `$HOME`-with-anaconda3 case). The 1 Hz git poll declared
        // at the top of `App::run` covers parent-row dirty-flag
        // refresh in that case with at most one second of lag.
        // macOS FSEvents is OS-level and unaffected.
        if w.watch(new_dir, pick_recursive_mode(new_dir)).is_ok() {
            *active = Some(new_dir.to_path_buf());
        } else {
            *active = None;
        }
    }
    // Watch the repo's *resolved* gitdir non-recursively. For a normal
    // repo that's `<root>/.git`; for a linked worktree it's
    // `<main>/.git/worktrees/<name>/` (resolved from the `.git` *file*),
    // which lives OUTSIDE the working tree — without watching it, a
    // worktree's index/HEAD changes (stage, commit, checkout, branch
    // switch) never fire the watcher and markers only refresh on the
    // slower periodic poll. We can't watch the `index` *file* directly:
    // git commits via atomic rename (write `index.lock`, rename to
    // `index`), which replaces the inode — a file-level watch follows
    // the *old* inode and goes deaf. A directory watch sees the rename
    // land. NonRecursive bounds the noise even with huge `.git/objects`
    // trees. `gitdir` is resolved + cached on chdir (`current_gitdir`).
    if active_git.as_deref() != gitdir {
        if let Some(old) = active_git.take() {
            let _ = w.unwatch(&old);
        }
        if let Some(gd) = gitdir
            && w.watch(gd, RecursiveMode::NonRecursive).is_ok()
        {
            *active_git = Some(gd.to_path_buf());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::scheduler::Deadline;
    use super::*;
    use std::sync::mpsc;

    fn fs_event(path: &Path) -> notify::Event {
        notify::Event::new(notify::EventKind::Any).add_path(path.to_path_buf())
    }

    fn git_result(generation: u64) -> state::GitWorkerResult {
        state::GitWorkerResult {
            generation,
            repo_root: PathBuf::from("/no/such/repo"),
            raw: None,
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
        let got = coalesce_pending(&rx, &mut fs_pending, &mut git_pending);

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
        // MVU Phase 3d: grep/finder wakes collapse to nothing buffered (the
        // data rides their own channels; the loop re-drains on re-entry).
        tx.send(Message::GrepOutput).unwrap();
        tx.send(Message::GrepOutput).unwrap();
        tx.send(Message::FindOutput).unwrap();

        let mut fs_pending = Vec::new();
        let mut git_pending = Vec::new();
        let got = coalesce_pending(&rx, &mut fs_pending, &mut git_pending);

        assert_eq!(got, None);
        assert_eq!(fs_pending.len(), 2);
        assert_eq!(git_pending.len(), 1); // Tick + Grep/Find wakes dropped, not buffered
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
            assert_eq!(app.activity_watcher_events, 3);
            assert_eq!(last_event_at, Some(base));
            assert_eq!(first, Some(base));
            assert!(!needs_reload); // not a config path
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
            assert_eq!(app.activity_git_results, 2);
        });
    }

    #[test]
    fn ingest_git_result_takes_request_stamp_once() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.last_git_request_at = Some(std::time::Instant::now());
            app.ingest_git_result(git_result(99));
            // Recorded + cleared on the first result; a second doesn't panic
            // or re-take.
            assert!(app.state.last_git_request_at.is_none());
            app.ingest_git_result(git_result(99));
            assert!(app.state.last_git_request_at.is_none());
        });
    }
}

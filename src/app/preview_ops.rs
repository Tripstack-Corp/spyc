//! Off-thread live reload of the vertical-split preview (`view.right_pager`).
//!
//! When the previewed file changes on disk (detected by `is_preview_path` in
//! the fs-event ingest), the file is re-read and re-rendered. Markdown render
//! and syntect highlight scale with file size, so that work must never run on
//! the event loop (render-purity / no-blocking-IO-on-the-loop). The pure
//! builder is `pager_handler::build_pager_view`; this module wraps it in a
//! detached worker.
//!
//! Shape mirrors `agent_status` (and the graveyard/mermaid ops): a landing slot
//! (`runtime.preview_results`), an in-flight flag (`runtime.preview_reloading`),
//! and a detached worker that wakes the loop with `Message::PreviewReloadDone`
//! on completion. The pre-recv scan drains the slot via
//! [`App::apply_preview_reloads`]. Bursts coalesce: while a reload is in flight,
//! further events set `view.preview_dirty` instead of spawning a second worker;
//! the drain re-kicks once if it's set, so the FINAL save is always the one that
//! sticks — without a thread per filesystem event.

use std::path::PathBuf;

use crate::ui::pager::PagerView;

use super::{App, Message, pager_handler};

/// A reload request handed to the worker. Carries everything the pure builder
/// needs so the worker touches no `App` state: the file, a clone of the theme,
/// the markdown open-mode, and the right column's wrap width (computed on the
/// main thread, where the tty size query belongs).
pub struct PreviewReloadReq {
    pub path: PathBuf,
    pub theme: crate::ui::theme::Theme,
    pub open_as_rendered: bool,
    pub wrap_width: u16,
}

/// Worker result, applied by [`App::apply_preview_reloads`]. Carries the source
/// path so a late result for a since-swapped/closed preview is discarded (the
/// same staleness guard `apply_landed_agent_status` uses for a switched pane).
pub enum PreviewOutcome {
    /// Rebuilt view (boxed — `PagerView` is large) for `path`.
    Reloaded { path: PathBuf, view: Box<PagerView> },
    /// Re-read/render of `path` failed (e.g. the file was deleted); short reason.
    Failed { path: PathBuf, error: String },
}

/// Run one reload on the detached worker — all IO + the markdown/syntect render
/// happen here, off the loop.
pub fn reload(req: PreviewReloadReq) -> PreviewOutcome {
    let PreviewReloadReq {
        path,
        theme,
        open_as_rendered,
        wrap_width,
    } = req;
    match pager_handler::build_pager_view(&path, &theme, open_as_rendered, Some(wrap_width)) {
        Ok(mut view) => {
            // The preview always rides the right column without a history entry;
            // stamp those here so the apply can install the view verbatim.
            view.mount = crate::ui::pager::Mount::RightPane;
            view.no_history = true;
            PreviewOutcome::Reloaded {
                path,
                view: Box::new(view),
            }
        }
        Err(error) => PreviewOutcome::Failed { path, error },
    }
}

impl App {
    /// Kick an off-thread reload of the right-split preview. Called from the
    /// fs-event ingest when the previewed file changed (a `&mut` settle point,
    /// never the `&self` draw pass — this spawns a thread). No-op when no
    /// preview is open. While a reload is in flight, mark `preview_dirty` and
    /// return — the in-flight worker's drain re-kicks, so a burst of saves
    /// collapses to at most one trailing re-render instead of a thread each.
    pub(crate) fn kick_preview_reload(&mut self) {
        let Some(path) = self
            .view
            .right_pager
            .as_ref()
            .and_then(|v| v.source_path.clone())
        else {
            return;
        };
        if self
            .runtime
            .preview_reloading
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.view.preview_dirty = true;
            return;
        }
        self.runtime
            .preview_reloading
            .store(true, std::sync::atomic::Ordering::Release);
        self.view.preview_dirty = false;
        let req = PreviewReloadReq {
            path,
            theme: self.view.theme.clone(),
            open_as_rendered: self.state.config.markdown.open_as_rendered,
            wrap_width: self.right_preview_body_width(),
        };
        let results = std::sync::Arc::clone(&self.runtime.preview_results);
        let reloading = std::sync::Arc::clone(&self.runtime.preview_reloading);
        // Clone of the unified-channel sender so the worker can WAKE the loop on
        // completion (None before `run()` / in the test harness → no wake, which
        // is correct: those paths don't render in a loop).
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let outcome = reload(req);
            *results.lock().unwrap() = Some(outcome);
            reloading.store(false, std::sync::atomic::Ordering::Release);
            // Wake AFTER the result + flag are stored, so the woken pre-recv
            // scan sees `preview_results` populated and forces a redraw.
            if let Some(tx) = wake {
                let _ = tx.send(Message::PreviewReloadDone);
            }
        });
    }

    /// Pre-recv drain: install a landed off-thread reload result, preserving the
    /// current scroll position (clamped to the new length, so a shrunk file
    /// doesn't blank the viewport). A late result for a since-swapped/closed
    /// preview is discarded; a failed reload (deleted file, etc.) flashes in the
    /// preview's footer and KEEPS the last-good render rather than blanking it.
    /// Re-kicks once if `preview_dirty` was set while the worker ran. Returns
    /// whether a redraw is needed.
    pub(crate) fn apply_preview_reloads(&mut self) -> bool {
        let Some(outcome) = self.runtime.preview_results.lock().unwrap().take() else {
            return false;
        };
        let current = self
            .view
            .right_pager
            .as_ref()
            .and_then(|v| v.source_path.clone());
        let mut redraw = false;
        match outcome {
            PreviewOutcome::Reloaded { path, view } => {
                if current.as_deref() == Some(path.as_path()) {
                    let mut view = *view;
                    // Preserve the reader's scroll across the reload (clamped to
                    // the new last line — the file may have shrunk).
                    if let Some(old) = self.view.right_pager.as_ref() {
                        let last = view.lines.len().saturating_sub(1);
                        view.scroll = old.scroll.min(u16::try_from(last).unwrap_or(u16::MAX));
                    }
                    self.view.right_pager = Some(view);
                    redraw = true;
                }
            }
            PreviewOutcome::Failed { path, error } => {
                if current.as_deref() == Some(path.as_path())
                    && let Some(v) = self.view.right_pager.as_mut()
                {
                    v.flash = Some(error);
                    redraw = true;
                }
            }
        }
        // A save landed while the worker ran — re-read now so the final content
        // is the one shown (the worker read a snapshot that may be stale).
        if self.view.preview_dirty {
            self.view.preview_dirty = false;
            self.kick_preview_reload();
        }
        redraw
    }
}

#[cfg(test)]
mod tests {
    use super::{App, PreviewOutcome};
    use crate::ui::pager::PagerView;
    use std::path::PathBuf;

    /// Seed `right_pager` with a preview of `path` scrolled to `scroll`.
    fn open_preview(app: &mut App, path: &str, lines: usize, scroll: u16) {
        let mut v = PagerView::new_plain(
            "doc.md".to_string(),
            (0..lines).map(|i| i.to_string()).collect(),
        );
        v.source_path = Some(PathBuf::from(path));
        v.scroll = scroll;
        app.view.right_pager = Some(v);
    }

    fn reloaded(path: &str, lines: usize) -> PreviewOutcome {
        let mut v = PagerView::new_plain(
            "doc.md".to_string(),
            (0..lines).map(|i| format!("new {i}")).collect(),
        );
        v.source_path = Some(PathBuf::from(path));
        PreviewOutcome::Reloaded {
            path: PathBuf::from(path),
            view: Box::new(v),
        }
    }

    /// Burst-collapse: while a reload is in flight, `kick_preview_reload` marks
    /// `preview_dirty` and returns instead of spawning a SECOND worker — the
    /// design that keeps a save-storm from spawning a thread per event. (The
    /// trailing re-kick from `apply_preview_reloads` spawns a real worker, so it
    /// isn't asserted here; this pins the no-second-worker half.)
    #[test]
    fn kick_coalesces_while_a_reload_is_in_flight() {
        use std::sync::atomic::Ordering;
        let mut app = App::test_app(PathBuf::from("/repo"));
        open_preview(&mut app, "/repo/doc.md", 10, 0);
        // Simulate an in-flight worker.
        app.runtime.preview_reloading.store(true, Ordering::Release);
        app.view.preview_dirty = false;

        app.kick_preview_reload();

        assert!(
            app.view.preview_dirty,
            "in-flight reload → mark dirty for one trailing re-render"
        );
        assert!(
            app.runtime.preview_reloading.load(Ordering::Acquire),
            "flag untouched — no second worker was kicked"
        );
    }

    /// A matching reload installs the fresh view and preserves the reader's
    /// scroll position (it fits within the new, longer content).
    #[test]
    fn apply_installs_and_preserves_scroll() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        open_preview(&mut app, "/repo/doc.md", 3, 5);
        *app.runtime.preview_results.lock().unwrap() = Some(reloaded("/repo/doc.md", 20));

        assert!(app.apply_preview_reloads(), "a real install redraws");
        let v = app.view.right_pager.as_ref().unwrap();
        assert_eq!(v.lines.len(), 20, "fresh content installed");
        assert_eq!(v.scroll, 5, "reader's scroll preserved across the reload");
    }

    /// Scroll is clamped to the new last line when the file shrank, so the
    /// viewport doesn't blank past the end.
    #[test]
    fn apply_clamps_scroll_when_file_shrank() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        open_preview(&mut app, "/repo/doc.md", 50, 40);
        *app.runtime.preview_results.lock().unwrap() = Some(reloaded("/repo/doc.md", 5));

        assert!(app.apply_preview_reloads());
        assert_eq!(
            app.view.right_pager.as_ref().unwrap().scroll,
            4,
            "clamped to the new last line (5 lines → index 4)"
        );
    }

    /// A late result for a since-swapped preview is discarded — the current
    /// preview is untouched and no redraw is claimed.
    #[test]
    fn apply_discards_stale_result_for_swapped_preview() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        open_preview(&mut app, "/repo/current.md", 3, 0);
        *app.runtime.preview_results.lock().unwrap() = Some(reloaded("/repo/old.md", 20));

        assert!(!app.apply_preview_reloads(), "stale result → no redraw");
        let v = app.view.right_pager.as_ref().unwrap();
        assert_eq!(
            v.source_path.as_deref(),
            Some(PathBuf::from("/repo/current.md").as_path())
        );
        assert_eq!(v.lines.len(), 3, "current preview left intact");
    }

    /// A failed reload (e.g. the file was deleted) flashes in the preview's
    /// footer but KEEPS the last-good render rather than blanking it.
    #[test]
    fn apply_failed_keeps_last_good_and_flashes() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        open_preview(&mut app, "/repo/doc.md", 7, 2);
        *app.runtime.preview_results.lock().unwrap() = Some(PreviewOutcome::Failed {
            path: PathBuf::from("/repo/doc.md"),
            error: "read: no such file".to_string(),
        });

        assert!(app.apply_preview_reloads());
        let v = app.view.right_pager.as_ref().unwrap();
        assert_eq!(v.lines.len(), 7, "last-good render kept");
        assert!(v.flash.is_some(), "error surfaced in the preview footer");
    }

    /// Nothing landed → no redraw, no panic.
    #[test]
    fn apply_noop_when_slot_empty() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        assert!(!app.apply_preview_reloads());
    }
}

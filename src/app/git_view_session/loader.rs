//! The loader a git view shows while it builds: once the build outruns the
//! cooking grace, its overlay opens on [`crate::ui::loader`]'s pepper and the
//! build's progress, redrawn each frame of the cooking clock, and the built
//! view later takes the same overlay. Closing the loader cancels the build:
//! the worker stops at its next file.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use super::{GitViewKind, PagerStreamMount, git_view_body_width};
use crate::app::App;
use crate::app::cooking::COOKING_GRACE;

/// How far a git view's build has got, shared between its worker and the
/// loader.
#[derive(Default)]
pub struct BuildProgress {
    /// Files to highlight: 0 while the changes are still being read.
    files: AtomicUsize,
    /// Files highlighted so far.
    highlighted: AtomicUsize,
    /// The loader was closed: the worker stops at its next file.
    cancelled: AtomicBool,
}

impl BuildProgress {
    /// The changes are read: `files` of them to highlight.
    pub fn begin_highlighting(&self, files: usize) {
        self.files.store(files, Ordering::Relaxed);
    }

    /// One more file highlighted.
    pub fn highlighted_one(&self) {
        self.highlighted.fetch_add(1, Ordering::Relaxed);
    }

    /// Stop the build at its next file.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Whether the loader was closed.
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Files highlighted, and files to highlight (0 while reading).
    fn counts(&self) -> (usize, usize) {
        (
            self.highlighted.load(Ordering::Relaxed),
            self.files.load(Ordering::Relaxed),
        )
    }
}

/// What the loader says a kind of build is doing.
#[derive(Clone, Copy)]
pub struct LoaderWords {
    heading: &'static str,
    /// Before there are files to count.
    reading: &'static str,
}

impl LoaderWords {
    pub const fn for_kind(kind: &GitViewKind) -> Self {
        match kind {
            GitViewKind::Diff { .. } | GitViewKind::SinceBase { .. } => Self {
                heading: "building the diff",
                reading: "reading the changes",
            },
            GitViewKind::Show { .. } => Self {
                heading: "building the commit",
                reading: "reading the commit",
            },
            GitViewKind::Blame { .. } => Self {
                heading: "building the blame",
                reading: "walking the history",
            },
        }
    }
}

/// What the loader last drew: it redraws only when one of these moves, not
/// on every wake the loop takes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Drawn {
    frame: u64,
    highlighted: usize,
    files: usize,
    tenths: u128,
}

impl App {
    /// Open the active project's building git view on its loader once the
    /// build outruns the cooking grace, and advance the loader each frame of
    /// the cooking clock, which the build's own guard keeps running. Whether
    /// the frame changed.
    pub(in crate::app) fn settle_git_view_loader(&mut self, now: Instant) -> bool {
        if self.cancel_closed_git_view_loader() {
            return false;
        }
        let Some(pending) = self.runtime.project().pending_git_view.as_ref() else {
            return false;
        };
        let elapsed = now.saturating_duration_since(pending.started);
        if elapsed < COOKING_GRACE {
            return false;
        }
        let (highlighted, files) = pending.progress.counts();
        let drawn = Drawn {
            frame: self.view.cooking_frame.unwrap_or(0),
            highlighted,
            files,
            tenths: elapsed.as_millis() / 100,
        };
        if pending.shown == Some(drawn) {
            return false;
        }
        let (id, words) = (pending.id, pending.words);
        if pending.shown.is_none() {
            let title = pending.title.clone();
            self.mount_stream_pager(
                PagerStreamMount::Overlay {
                    title,
                    line_count_hint: None,
                },
                id,
            );
        }
        let detail = if files == 0 {
            words.reading.to_string()
        } else {
            let noun = if files == 1 { "file" } else { "files" };
            format!("highlighting {highlighted} / {files} {noun}")
        };
        let loader = crate::ui::loader::Loader {
            frame: drawn.frame,
            heading: words.heading,
            detail: &detail,
            done: (files > 0).then(|| highlighted as f64 / files as f64),
            elapsed: &format!("{:.1}s", elapsed.as_secs_f64()),
        };
        let theme = self.view.theme.clone();
        if let Some(view) = self.view.project_mut().pager.as_mut() {
            let width = git_view_body_width(view.full_width);
            let height = match view.last_viewport_h.get() {
                0 => fallback_body_height(),
                h => usize::from(h),
            };
            view.lines = crate::ui::loader::render(&loader, width, height, &theme);
            view.show_line_numbers = false;
            view.wrap = false;
            view.streaming = true;
        }
        if let Some(pending) = self.runtime.project_mut().pending_git_view.as_mut() {
            pending.shown = Some(drawn);
        }
        true
    }

    /// Cancel a build whose loader is no longer the pager on screen: closed,
    /// or replaced by another. Whether it was.
    pub(super) fn cancel_closed_git_view_loader(&mut self) -> bool {
        let Some(pending) = self.runtime.project().pending_git_view.as_ref() else {
            return false;
        };
        let on_screen = self
            .view
            .project()
            .pager
            .as_ref()
            .is_some_and(|p| p.stream_id == Some(pending.id));
        if pending.shown.is_none() || on_screen {
            return false;
        }
        pending.progress.cancel();
        self.runtime.project_mut().pending_git_view = None;
        true
    }
}

/// The centred overlay's body height before the pager's first draw has
/// measured it: 92% of the terminal less the border and footer rows.
fn fallback_body_height() -> usize {
    let (_, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    (usize::from(rows) * 92 / 100).saturating_sub(3)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Sender, channel};

    use super::super::{GitViewContent, GitViewPayload, PendingGitView};
    use super::*;
    use crate::git::model::DiffModel;

    fn with_app(f: impl FnOnce(&mut App)) {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            f(&mut app);
        });
    }

    /// A build of `id` in flight since `started`, its result to come over the
    /// returned sender; and its progress.
    fn building(app: &mut App, id: u32) -> (Sender<GitViewPayload>, Instant) {
        let (tx, rx) = channel();
        let pending = PendingGitView::for_test(id, rx);
        let started = pending.started;
        app.runtime.project_mut().pending_git_view = Some(pending);
        (tx, started)
    }

    fn progress(app: &App) -> std::sync::Arc<BuildProgress> {
        let pending = app.runtime.project().pending_git_view.as_ref();
        std::sync::Arc::clone(&pending.expect("still building").progress)
    }

    fn pager_text(app: &App) -> String {
        let pager = app.view.project().pager.as_ref().expect("a pager");
        pager
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
    }

    fn built() -> GitViewPayload {
        let model = DiffModel {
            files: Vec::new(),
            truncated: false,
        };
        let hl = crate::ui::diff_render::highlight_diff(&model);
        GitViewPayload::Titled(
            "git diff main\u{2026}".into(),
            GitViewContent::Diff(model, hl),
        )
    }

    /// A build that finishes within the grace never shows a loader.
    #[test]
    fn a_quick_build_shows_no_loader() {
        with_app(|app| {
            let (_tx, started) = building(app, 4);
            assert!(!app.settle_git_view_loader(started + COOKING_GRACE / 2));
            assert!(app.view.project().pager.is_none());
        });
    }

    /// Past the grace the loader opens and follows the build, and the built
    /// view then takes its place in the same overlay, leaving no loader to go
    /// back to.
    #[test]
    fn a_slow_build_shows_its_loader_until_the_view_replaces_it() {
        with_app(|app| {
            let (tx, started) = building(app, 4);
            let due = started + COOKING_GRACE;
            assert!(app.settle_git_view_loader(due));
            let pager = app.view.project().pager.as_ref().expect("the loader");
            assert_eq!(pager.stream_id, Some(4));
            let text = pager_text(app);
            assert!(text.contains("building the commit"), "{text}");
            assert!(text.contains("reading the commit"), "{text}");
            assert!(text.contains("Esc cancels"), "{text}");
            assert!(!app.settle_git_view_loader(due), "nothing moved");

            let progress = progress(app);
            progress.begin_highlighting(12);
            progress.highlighted_one();
            progress.highlighted_one();
            progress.highlighted_one();
            assert!(app.settle_git_view_loader(due));
            let text = pager_text(app);
            assert!(text.contains("highlighting 3 / 12 files"), "{text}");

            tx.send(built()).unwrap();
            assert!(app.drain_pending_git_view());
            let pager = app.view.project().pager.as_ref().expect("the view");
            assert_eq!(pager.stream_id, Some(4));
            assert_eq!(pager.title, "git diff main\u{2026}");
            assert!(!pager.streaming);
            assert!(!pager_text(app).contains("Esc cancels"));
            assert!(
                app.view.project_mut().pager_history.pop_back().is_none(),
                "no loader to go back to"
            );
            assert!(app.runtime.project().pending_git_view.is_none());
        });
    }

    /// Closing the loader stops the build, and what it sends later shows
    /// nothing.
    #[test]
    fn closing_the_loader_cancels_the_build() {
        with_app(|app| {
            let (tx, started) = building(app, 4);
            assert!(app.settle_git_view_loader(started + COOKING_GRACE));
            let progress = progress(app);
            app.view.project_mut().pager = None;
            app.settle_git_view_loader(started + COOKING_GRACE * 2);
            assert!(progress.cancelled());
            assert!(app.runtime.project().pending_git_view.is_none());
            let _ = tx.send(built());
            assert!(!app.drain_pending_git_view());
            assert!(app.view.project().pager.is_none());
        });
    }

    /// An empty result closes the loader and says so.
    #[test]
    fn an_empty_build_closes_its_loader() {
        with_app(|app| {
            let (tx, started) = building(app, 4);
            assert!(app.settle_git_view_loader(started + COOKING_GRACE));
            tx.send(GitViewPayload::Empty).unwrap();
            assert!(app.drain_pending_git_view());
            assert!(app.view.project().pager.is_none());
            assert_eq!(app.flash_text(), Some("no changes"));
        });
    }
}

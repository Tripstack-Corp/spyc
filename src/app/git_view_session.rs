//! `GitViewStream`: the git-view (diff / show / blame) stream. PR 8b of the gix
//! migration built ONE bounded, owned model off-thread (via the gix builders in
//! [`crate::git::diff_model`] / [`crate::git::blame`]) and rendered it in-house
//! ([`crate::ui::diff_render`] / [`crate::ui::blame_render`]); this migrates that
//! onto the shared [`super::pager_stream`] abstraction.
//!
//! Unlike the streaming grep stream (batches, drop on completion) and the
//! one-shot transcript stream, `GitViewStream` RETAINS the built model
//! (`retain_after_finish` = true) so the unified⇄side-by-side `|` toggle
//! (`on_pager_command`) re-renders instantly without re-touching gix. The
//! `GitViewKind` / `GitViewPayload` / `GitViewModel` enums + the off-thread
//! `build_payload` + the pure layout helpers live here too; the open entry
//! point (`open_git_view`, called from `git_state`) mounts via
//! `spawn_pager_stream`.

use std::path::PathBuf;

use crate::git::model::{BlameModel, CommitMeta, DiffModel};
use crate::ui::diff_render::{self, DiffLayout};
use crate::ui::pager::{self, PagerView};
use crate::ui::{blame_render, theme::Theme};

use super::App;
use super::pager_stream::{DrainOutcome, PagerStream, PagerStreamCmd, PagerStreamMount, RenderCtx};

/// Which two sides a `git diff` view compares — the three diff keys (`gd` /
/// `gD` / `gu`) each pick one. Maps 1:1 to a `diff_model` builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffScope {
    /// `gd` / `git diff HEAD`: `HEAD` vs the working tree (staged + unstaged +
    /// untracked).
    HeadToWorktree,
    /// `gD` / `git diff --cached`: `HEAD` tree vs the index (staged only — what
    /// would commit).
    Cached,
    /// `gu` / `git diff`: the index vs the working tree (unstaged only — what
    /// changed since you staged).
    IndexToWorktree,
}

/// What the worker thread should build. Carries owned, `Send` inputs only.
pub enum GitViewKind {
    /// `git diff` over `paths`, at the given [`DiffScope`].
    Diff {
        /// Repository workdir root.
        repo_root: PathBuf,
        /// Repo-relative paths to restrict the diff to.
        paths: Vec<String>,
        /// Which two sides to compare.
        scope: DiffScope,
    },
    /// `git show <rev>`.
    Show {
        /// Repository workdir root.
        repo_root: PathBuf,
        /// The revision to show.
        rev: String,
    },
    /// `git blame <path>`.
    Blame {
        /// Repository workdir root.
        repo_root: PathBuf,
        /// Repo-relative path to blame.
        path: String,
    },
}

/// The built model the worker sends back over `rx`. All variants are `Send`
/// (owned `String`s / numbers). The big `Show` tuple is boxed to keep the
/// enum small (avoids a `large_enum_variant` lint).
pub enum GitViewPayload {
    /// A diff model ready to render.
    Diff(DiffModel),
    /// A `show` commit-meta + diff pair ready to render.
    Show(Box<(CommitMeta, DiffModel)>),
    /// A blame model ready to render.
    Blame(BlameModel),
    /// The diff/show produced no changes.
    Empty,
    /// The build failed (bad rev, not tracked, not a repo, …).
    Error(String),
}

/// The model retained on the main thread once received, so the `|` layout
/// toggle (and any re-render) can rebuild lines without re-touching gix.
pub enum GitViewModel {
    /// A retained diff model.
    Diff(DiffModel),
    /// A retained `show` commit-meta + diff pair.
    Show(Box<(CommitMeta, DiffModel)>),
    /// A retained blame model.
    Blame(BlameModel),
}

/// Below this per-column width the side-by-side layout is unreadable, so the
/// renderer falls back to unified for that render (without mutating the
/// session's stored layout). See [`split_too_narrow`].
const MIN_SPLIT_COL_W: usize = 24;

/// Decide whether a side-by-side render at total `width` would be too narrow
/// to be readable, given the column separator overhead. Pure + testable.
const fn split_too_narrow(width: usize) -> bool {
    width.saturating_sub(3) / 2 < MIN_SPLIT_COL_W
}

/// The effective render layout: fall back to unified when side-by-side would
/// be too narrow at this `width`. Does NOT mutate the stored layout.
const fn effective_layout(layout: DiffLayout, width: usize) -> DiffLayout {
    if matches!(layout, DiffLayout::SideBySide) && split_too_narrow(width) {
        DiffLayout::Unified
    } else {
        layout
    }
}

/// What rendering a model produced: the styled lines, whether the pager should
/// show its line-number gutter (off for diff/show — they carry their own; on
/// for blame), and whether the pager should wrap long lines (off for the
/// fixed-width side-by-side rows + blame gutter; on for unified, where long
/// source lines should wrap rather than clip).
struct Rendered {
    lines: Vec<ratatui::text::Line<'static>>,
    line_numbers: bool,
    wrap: bool,
}

/// Render a retained model at the given `width`/`layout`, reusing the model's
/// precomputed `hl` (syntax highlight) so a re-render — `|`, `f`, or a resize —
/// never re-runs syntect. The side-by-side rows are sized to exactly `width`,
/// so `width` MUST be the pager's true text-body width (see
/// [`git_view_body_width`]) or the rows wrap into stray tinted bars.
fn render_model(
    model: &GitViewModel,
    hl: Option<&diff_render::DiffHighlight>,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> Rendered {
    match model {
        GitViewModel::Diff(m) => {
            let eff = effective_layout(layout, width);
            let lines = match hl {
                Some(h) => diff_render::render_diff_highlighted(m, h, theme, eff, width),
                None => diff_render::render_diff(m, theme, eff, width),
            };
            Rendered {
                lines,
                line_numbers: false,
                wrap: matches!(eff, DiffLayout::Unified),
            }
        }
        GitViewModel::Show(b) => {
            let eff = effective_layout(layout, width);
            let lines = match hl {
                Some(h) => diff_render::render_show_highlighted(&b.0, &b.1, h, theme, eff, width),
                None => diff_render::render_show(&b.0, &b.1, theme, eff, width),
            };
            Rendered {
                lines,
                line_numbers: false,
                wrap: matches!(eff, DiffLayout::Unified),
            }
        }
        GitViewModel::Blame(m) => Rendered {
            lines: blame_render::render_blame(m, theme),
            line_numbers: true,
            wrap: false,
        },
    }
}

/// The pager's true text-body width for a git-view, matching the render path:
/// the full terminal width when `full_width`, else the centered overlay's body
/// (90% − borders, via [`pager::centered_body_width`]). Sizing the side-by-side
/// columns to anything wider makes every row wrap.
fn git_view_body_width(full_width: bool) -> usize {
    let (cols, _) = crossterm::terminal::size().unwrap_or((80, 24));
    let w = if full_width {
        cols
    } else {
        pager::centered_body_width(cols)
    };
    w as usize
}

/// In-flight git-view: the worker is building the model off-thread, but NO
/// pager is mounted yet. [`App::drain_pending_git_view`] mounts the overlay
/// only when a non-empty model arrives; an empty result just flashes "no
/// changes" (so `gd` over a clean path doesn't pop an overlay up and instantly
/// tear it back down). Held in `Runtime` until the worker reports.
pub struct PendingGitView {
    id: u32,
    rx: std::sync::mpsc::Receiver<GitViewPayload>,
    /// The pager title once mounted.
    title: String,
    /// Initial layout for diff/show (ignored for blame).
    layout: DiffLayout,
}

/// A [`PagerStream`] for a *mounted* git-view. The model is built off-thread and
/// the overlay is mounted (by `drain_pending_git_view`) only once it arrives, so
/// this stream always holds a ready model — `drain` is a no-op. The stream is
/// RETAINED (`retain_after_finish`) purely so the `|` layout toggle
/// (`on_pager_command`) can re-render from the held `model` without re-touching
/// gix.
pub struct GitViewStream {
    id: u32,
    /// Current layout for diff/show (ignored for blame).
    layout: DiffLayout,
    /// The built model, rendered at mount and re-rendered on `|`.
    model: GitViewModel,
    /// The model's syntax highlight, computed once at build time and reused for
    /// every re-render (`|`, `f`, resize) — see [`render_model`]. `None` for
    /// blame (which has its own renderer).
    highlight: Option<diff_render::DiffHighlight>,
    /// The pager title.
    title: String,
}

impl GitViewStream {
    /// Render the held model into `view` at the pager's true body width.
    /// Width must match the render path (depends on `full_width`), else the
    /// fixed-width side-by-side rows wrap into stray tinted bars.
    fn render_into(&self, view: &mut PagerView, ctx: &RenderCtx) {
        let width = git_view_body_width(ctx.full_width);
        let rendered = render_model(
            &self.model,
            self.highlight.as_ref(),
            &ctx.theme,
            self.layout,
            width,
        );
        view.lines = rendered.lines;
        view.show_line_numbers = rendered.line_numbers;
        view.wrap = rendered.wrap;
        view.streaming = false;
        view.title.clone_from(&self.title);
        // The `|` toggle swaps unified↔side-by-side, which changes the line
        // count. A `scroll` left deep into the old layout would point past
        // the new end and blank the viewport — clamp it to the new content.
        view.clamp_scroll_auto();
    }
}

impl PagerStream for GitViewStream {
    fn id(&self) -> u32 {
        self.id
    }

    fn retain_after_finish(&self) -> bool {
        true
    }

    fn drain(&mut self, _view: &mut PagerView, _ctx: &RenderCtx) -> DrainOutcome {
        // The model was rendered at mount time; the stream lives only to back
        // the `|` toggle. Nothing to drain.
        DrainOutcome::Idle
    }

    fn on_pager_command(
        &mut self,
        cmd: PagerStreamCmd,
        view: &mut PagerView,
        ctx: &RenderCtx,
    ) -> bool {
        match cmd {
            PagerStreamCmd::ToggleLayout => {
                // Only meaningful for diff/show (blame has no layout).
                if !matches!(self.model, GitViewModel::Diff(_) | GitViewModel::Show(_)) {
                    return false;
                }
                self.layout = match self.layout {
                    DiffLayout::Unified => DiffLayout::SideBySide,
                    DiffLayout::SideBySide => DiffLayout::Unified,
                };
                self.render_into(view, ctx);
                true
            }
            PagerStreamCmd::Rerender => {
                // `f` toggled the body width; re-bake the fixed-width rows at
                // the new width (`ctx.full_width` already reflects the toggle).
                self.render_into(view, ctx);
                true
            }
        }
    }
}

impl App {
    /// Spawn a git-view worker off-thread, but DON'T mount an overlay yet — the
    /// pager is mounted by [`Self::drain_pending_git_view`] only when a non-empty
    /// model arrives. An empty result (`gd` over a path with no changes) just
    /// flashes "no changes", so the overlay never pops up and tears itself back
    /// down. (Deferring keeps the build off the input thread — large repos don't
    /// freeze it — while giving git-like "no changes" feedback for clean paths.)
    pub fn open_git_view(&mut self, kind: GitViewKind, title: String) {
        let id = self.runtime.next_stream_id;
        self.runtime.next_stream_id = self.runtime.next_stream_id.wrapping_add(1);
        let (tx, rx) = std::sync::mpsc::channel::<GitViewPayload>();
        // Wake the loop on the worker's send and once more after it returns —
        // the final wake drives the drain that observes the rx (mirrors
        // `spawn_pager_stream`).
        let wake = self.make_pager_stream_wake();
        let final_wake = std::sync::Arc::clone(&wake);
        let tx = crate::fs::WakingSender::new(tx, wake);
        std::thread::spawn(move || {
            let _ = tx.send(build_payload(kind));
            final_wake();
        });
        self.runtime.pending_git_view = Some(PendingGitView {
            id,
            rx,
            title,
            // SideBySide is the product default for diff/show.
            layout: DiffLayout::SideBySide,
        });
    }

    /// Poll the in-flight git-view (if any). On a non-empty model, mount the
    /// overlay now and render it; on an empty result, flash "no changes" (no
    /// overlay); on error / dead worker, flash the error. Called every tick from
    /// the run loop alongside `drain_pager_stream`. Returns true when something
    /// changed so the caller can redraw.
    pub(crate) fn drain_pending_git_view(&mut self) -> bool {
        // Peek the worker's result without holding the borrow across the mutate.
        let result = {
            let Some(pending) = self.runtime.pending_git_view.as_ref() else {
                return false;
            };
            pending.rx.try_recv()
        };
        let payload = match result {
            Ok(GitViewPayload::Empty) => {
                self.state.flash_info("no changes");
                self.runtime.pending_git_view = None;
                return true;
            }
            Ok(GitViewPayload::Error(msg)) => {
                self.state.flash_error(msg);
                self.runtime.pending_git_view = None;
                return true;
            }
            Ok(payload) => payload,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.state.flash_error("git: worker failed");
                self.runtime.pending_git_view = None;
                return true;
            }
        };
        // Non-empty content → mount the overlay now and render it.
        let pending = self
            .runtime
            .pending_git_view
            .take()
            .expect("pending git-view present (checked above)");
        let model = match payload {
            GitViewPayload::Diff(m) => GitViewModel::Diff(m),
            GitViewPayload::Show(b) => GitViewModel::Show(b),
            GitViewPayload::Blame(m) => GitViewModel::Blame(m),
            GitViewPayload::Empty | GitViewPayload::Error(_) => {
                unreachable!("empty/error handled above")
            }
        };
        self.mount_stream_pager(
            PagerStreamMount::Overlay {
                title: pending.title.clone(),
                line_count_hint: None,
            },
            pending.id,
        );
        // Highlight once now (off the per-render path) so `|`, `f`, and resize
        // re-renders only re-lay-out. Blame has its own renderer — no highlight.
        let highlight = match &model {
            GitViewModel::Diff(m) => Some(diff_render::highlight_diff(m)),
            GitViewModel::Show(b) => Some(diff_render::highlight_diff(&b.1)),
            GitViewModel::Blame(_) => None,
        };
        let stream = GitViewStream {
            id: pending.id,
            layout: pending.layout,
            model,
            highlight,
            title: pending.title,
        };
        let full_width = self.view.pager.as_ref().is_some_and(|p| p.full_width);
        let ctx = RenderCtx {
            theme: self.view.theme.clone(),
            full_width,
        };
        if let Some(view) = self.view.pager.as_mut() {
            stream.render_into(view, &ctx);
        }
        self.runtime.pager_stream = Some(Box::new(stream));
        self.view.needs_full_repaint = true;
        true
    }
}

/// Build the worker payload off-thread. Pure of any `App`/OS-handle state — it
/// only touches the gix builders, which take owned inputs.
fn build_payload(kind: GitViewKind) -> GitViewPayload {
    use crate::git::{blame, diff_model};
    match kind {
        GitViewKind::Diff {
            repo_root,
            paths,
            scope,
        } => {
            let m = match scope {
                DiffScope::Cached => diff_model::diff_cached(&repo_root, &paths),
                DiffScope::IndexToWorktree => {
                    diff_model::diff_index_to_worktree(&repo_root, &paths)
                }
                DiffScope::HeadToWorktree => diff_model::diff_head_to_worktree(&repo_root, &paths),
            };
            match m {
                Some(model) if model.files.is_empty() => GitViewPayload::Empty,
                Some(model) => GitViewPayload::Diff(model),
                None => GitViewPayload::Error("git diff: not a git repository".into()),
            }
        }
        GitViewKind::Show { repo_root, rev } => match diff_model::show_model(&repo_root, &rev) {
            Some(pair) if pair.1.files.is_empty() => GitViewPayload::Empty,
            Some(pair) => GitViewPayload::Show(Box::new(pair)),
            None => GitViewPayload::Error(format!("git show: bad revision {rev}")),
        },
        GitViewKind::Blame { repo_root, path } => match blame::blame(&repo_root, &path) {
            Some(model) => GitViewPayload::Blame(model),
            None => GitViewPayload::Error("git blame: not tracked at HEAD".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_too_narrow_below_threshold() {
        // Per-column width = (width - 3) / 2; threshold is 24.
        // width 50 → (47)/2 = 23 < 24 → too narrow.
        assert!(split_too_narrow(50));
        // width 51 → (48)/2 = 24 → wide enough.
        assert!(!split_too_narrow(51));
        // Tiny terminals are always too narrow.
        assert!(split_too_narrow(0));
        assert!(split_too_narrow(10));
        // A roomy terminal is fine.
        assert!(!split_too_narrow(120));
    }

    fn with_app(f: impl FnOnce(&mut App)) {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            f(&mut app);
        });
    }

    /// `gd` over a path with no changes: the worker reports `Empty`, and that
    /// must just flash "no changes" — NEVER mount an overlay that pops up and
    /// instantly tears itself back down.
    #[test]
    fn pending_git_view_empty_flashes_without_mounting() {
        with_app(|app| {
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(GitViewPayload::Empty).unwrap();
            app.runtime.pending_git_view = Some(PendingGitView {
                id: 1,
                rx,
                title: "git diff HEAD".into(),
                layout: DiffLayout::SideBySide,
            });
            assert!(app.drain_pending_git_view());
            assert!(
                app.view.pager.is_none(),
                "an empty result must not mount an overlay"
            );
            assert!(app.runtime.pager_stream.is_none());
            assert!(app.runtime.pending_git_view.is_none());
            assert_eq!(app.flash_text(), Some("no changes"));
        });
    }

    /// A non-empty result mounts the overlay (tagged with the pending id) and
    /// installs the retained stream that backs the `|` toggle.
    #[test]
    fn pending_git_view_content_mounts_overlay() {
        with_app(|app| {
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(GitViewPayload::Diff(DiffModel {
                files: Vec::new(),
                truncated: false,
            }))
            .unwrap();
            app.runtime.pending_git_view = Some(PendingGitView {
                id: 7,
                rx,
                title: "git diff HEAD".into(),
                layout: DiffLayout::SideBySide,
            });
            assert!(app.drain_pending_git_view());
            let pager = app.view.pager.as_ref().expect("content mounts the overlay");
            assert_eq!(pager.stream_id, Some(7));
            assert!(
                app.runtime.pager_stream.is_some(),
                "retained stream installed for the `|` toggle"
            );
            assert!(app.runtime.pending_git_view.is_none());
        });
    }

    /// No in-flight git-view → a no-op (doesn't touch the pager).
    #[test]
    fn pending_git_view_noop_when_none() {
        with_app(|app| {
            assert!(!app.drain_pending_git_view());
        });
    }

    /// `f` (full-width toggle) on a git-view diff must re-bake the fixed-width
    /// side-by-side rows at the new (wider) body width — not leave them sized
    /// for the old, narrower centered overlay.
    #[test]
    fn full_width_toggle_rerenders_git_view_diff() {
        use crate::git::model::{
            DiffKind, DiffLine, DiffModel, FileDiff, FileStatus, Hunk, LineOrigin,
        };
        let content = DiffModel {
            files: vec![FileDiff {
                old_path: Some("f.txt".into()),
                new_path: Some("f.txt".into()),
                status: FileStatus::Modified,
                lang_hint: "txt".into(),
                kind: DiffKind::Text(vec![Hunk {
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![
                        DiffLine {
                            origin: LineOrigin::Remove,
                            text: "old".into(),
                        },
                        DiffLine {
                            origin: LineOrigin::Add,
                            text: "new".into(),
                        },
                    ],
                }]),
            }],
            truncated: false,
        };
        with_app(|app| {
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(GitViewPayload::Diff(content)).unwrap();
            app.runtime.pending_git_view = Some(PendingGitView {
                id: 9,
                rx,
                title: "git diff HEAD".into(),
                layout: DiffLayout::SideBySide,
            });
            assert!(app.drain_pending_git_view());
            let widest = |app: &App| {
                app.view
                    .pager
                    .as_ref()
                    .unwrap()
                    .lines
                    .iter()
                    .map(|l| {
                        l.spans
                            .iter()
                            .map(|s| crate::ui::display_width(s.content.as_ref()))
                            .sum::<usize>()
                    })
                    .max()
                    .unwrap_or(0)
            };
            let before = widest(app);
            // Exactly what the `f` key arm does: toggle full-width, re-render.
            app.view.pager.as_mut().unwrap().toggle_full_width();
            assert!(app.dispatch_pager_command(PagerStreamCmd::Rerender));
            let after = widest(app);
            assert!(
                after > before,
                "full-width re-render must widen the diff rows: {before} → {after}"
            );
        });
    }

    #[test]
    fn effective_layout_falls_back_when_narrow() {
        // Side-by-side downgrades to unified on a narrow viewport.
        assert_eq!(
            effective_layout(DiffLayout::SideBySide, 40),
            DiffLayout::Unified
        );
        // Side-by-side survives on a wide viewport.
        assert_eq!(
            effective_layout(DiffLayout::SideBySide, 120),
            DiffLayout::SideBySide
        );
        // Unified is never upgraded, regardless of width.
        assert_eq!(
            effective_layout(DiffLayout::Unified, 120),
            DiffLayout::Unified
        );
        assert_eq!(
            effective_layout(DiffLayout::Unified, 40),
            DiffLayout::Unified
        );
    }
}

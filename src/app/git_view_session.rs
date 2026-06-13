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

/// What the worker thread should build. Carries owned, `Send` inputs only.
pub enum GitViewKind {
    /// `git diff` (HEAD vs worktree, or `--cached`) over `paths`.
    Diff {
        /// Repository workdir root.
        repo_root: PathBuf,
        /// Repo-relative paths to restrict the diff to.
        paths: Vec<String>,
        /// `true` for the staged ("what would commit") view.
        cached: bool,
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

/// Render a retained model at the given `width`/`layout`. The side-by-side rows
/// are sized to exactly `width`, so `width` MUST be the pager's true text-body
/// width (see [`git_view_body_width`]) or the rows wrap into stray tinted bars.
fn render_model(model: &GitViewModel, theme: &Theme, layout: DiffLayout, width: usize) -> Rendered {
    match model {
        GitViewModel::Diff(m) => {
            let eff = effective_layout(layout, width);
            Rendered {
                lines: diff_render::render_diff(m, theme, eff, width),
                line_numbers: false,
                wrap: matches!(eff, DiffLayout::Unified),
            }
        }
        GitViewModel::Show(b) => {
            let eff = effective_layout(layout, width);
            Rendered {
                lines: diff_render::render_show(&b.0, &b.1, theme, eff, width),
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

/// A [`PagerStream`] for an active git-view. The worker builds one bounded model
/// off-thread and reports once; the first drain renders it into the pager and
/// the stream is RETAINED (`retain_after_finish`) so the `|` layout toggle
/// (`on_pager_command`) re-renders from the retained `model` without re-touching
/// gix. An empty/error/dead-worker result closes the pager (info/error flash).
pub struct GitViewStream {
    id: u32,
    rx: std::sync::mpsc::Receiver<GitViewPayload>,
    /// Current layout for diff/show (ignored for blame).
    layout: DiffLayout,
    /// The retained model, set once received. Backs the `|` toggle; doubling as
    /// the "already rendered" flag (drain no-ops once `Some`).
    model: Option<GitViewModel>,
    /// The final pager title (without the " — computing…" suffix).
    title: String,
}

impl GitViewStream {
    /// Render the retained model into `view` at the pager's true body width.
    /// Width must match the render path (depends on `full_width`), else the
    /// fixed-width side-by-side rows wrap into stray tinted bars.
    fn render_into(&self, view: &mut PagerView, ctx: &RenderCtx) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let width = git_view_body_width(ctx.full_width);
        let rendered = render_model(model, &ctx.theme, self.layout, width);
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

    fn drain(&mut self, view: &mut PagerView, ctx: &RenderCtx) -> DrainOutcome {
        // Already rendered — the stream lives only to back the `|` toggle.
        if self.model.is_some() {
            return DrainOutcome::Idle;
        }
        match self.rx.try_recv() {
            Ok(GitViewPayload::Empty) => DrainOutcome::CloseInfo("no changes".into()),
            Ok(GitViewPayload::Error(msg)) => DrainOutcome::CloseError(msg),
            Ok(GitViewPayload::Diff(m)) => {
                self.model = Some(GitViewModel::Diff(m));
                self.render_into(view, ctx);
                DrainOutcome::Finished
            }
            Ok(GitViewPayload::Show(b)) => {
                self.model = Some(GitViewModel::Show(b));
                self.render_into(view, ctx);
                DrainOutcome::Finished
            }
            Ok(GitViewPayload::Blame(m)) => {
                self.model = Some(GitViewModel::Blame(m));
                self.render_into(view, ctx);
                DrainOutcome::Finished
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => DrainOutcome::Idle,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                DrainOutcome::CloseError("git: worker failed".into())
            }
        }
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
                if !matches!(
                    self.model,
                    Some(GitViewModel::Diff(_) | GitViewModel::Show(_))
                ) {
                    return false;
                }
                self.layout = match self.layout {
                    DiffLayout::Unified => DiffLayout::SideBySide,
                    DiffLayout::SideBySide => DiffLayout::Unified,
                };
                self.render_into(view, ctx);
                true
            }
        }
    }
}

impl App {
    /// Spawn a git-view worker, mount a "computing…" overlay pager, and install
    /// the retained streaming session. A later tick (`drain_pager_stream` →
    /// `GitViewStream::drain`) renders the built model into that pager.
    pub fn open_git_view(&mut self, kind: GitViewKind, title: String) {
        self.spawn_pager_stream(
            PagerStreamMount::Overlay {
                title: format!("{title} — computing…"),
                line_count_hint: None,
            },
            move |tx| {
                let _ = tx.send(build_payload(kind));
            },
            move |id, rx| {
                Box::new(GitViewStream {
                    id,
                    rx,
                    // SideBySide is the product default for diff/show.
                    layout: DiffLayout::SideBySide,
                    model: None,
                    title,
                })
            },
        );
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
            cached,
        } => {
            let m = if cached {
                diff_model::diff_cached(&repo_root, &paths)
            } else {
                diff_model::diff_head_to_worktree(&repo_root, &paths)
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

//! State for an active git-view session (diff / show / blame). PR 8b of the
//! gix migration: instead of piping `git --color=always` bytes through the
//! pager (the old `git_state.rs` path), a worker thread builds ONE bounded,
//! owned model off-thread (via the gix builders in [`crate::git::diff_model`]
//! / [`crate::git::blame`]), sends it once, and the main thread renders it
//! into the matching pager via the in-house renderers
//! ([`crate::ui::diff_render`] / [`crate::ui::blame_render`]).
//!
//! Unlike [`super::grep_session`] (which streams batches of pre-built lines and
//! drops the session on completion), this RETAINS the built model on the main
//! thread so the unified⇄side-by-side `|` toggle and re-renders are instant
//! without re-touching gix. The session is dropped only when its backing pager
//! is closed/replaced (the id-gate in [`App::drain_git_view_session`]).
//!
//! Fields are `pub` because the struct is built via a literal and read back in
//! the drain + render paths. The open + drain + toggle `impl App` methods live
//! here too (mirroring `grep_session.rs`).

use std::path::PathBuf;

use crate::git::model::{BlameModel, CommitMeta, DiffModel};
use crate::ui::diff_render::{self, DiffLayout};
use crate::ui::pager;
use crate::ui::{blame_render, theme::Theme};

use super::App;

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

/// An active git-view session.
pub struct GitViewSession {
    /// Unique session id; pasted onto the pager view's `git_view_id` so a
    /// stale worker can't bleed into a fresh view.
    pub id: u32,
    /// Receiver for the one-shot built model from the worker.
    pub rx: std::sync::mpsc::Receiver<GitViewPayload>,
    /// Current layout for diff/show (ignored for blame).
    pub layout: DiffLayout,
    /// The retained model, set once received. Backs the `|` toggle.
    pub model: Option<GitViewModel>,
    /// True once the model has been received and rendered. The session
    /// stays alive (to back the toggle) but the drain no longer re-recvs.
    pub complete: bool,
    /// The final pager title (without the " — computing…" suffix).
    pub title: String,
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

/// Render a retained model into pager lines at the given `width`/`layout`.
/// Returns the lines and whether the pager should show line numbers (false
/// for diff/show — they carry their own gutters; true for blame).
fn render_model(
    model: &GitViewModel,
    theme: &Theme,
    layout: DiffLayout,
    width: usize,
) -> (Vec<ratatui::text::Line<'static>>, bool) {
    match model {
        GitViewModel::Diff(m) => (
            diff_render::render_diff(m, theme, effective_layout(layout, width), width),
            false,
        ),
        GitViewModel::Show(b) => (
            diff_render::render_show(&b.0, &b.1, theme, effective_layout(layout, width), width),
            false,
        ),
        GitViewModel::Blame(m) => (blame_render::render_blame(m, theme), true),
    }
}

/// Total viewport width for the renderer, derived from the terminal size minus
/// a small border margin (the pager's outer block).
fn render_width() -> usize {
    let (cols, _) = crossterm::terminal::size().unwrap_or((80, 24));
    (cols as usize).saturating_sub(2)
}

impl App {
    /// Spawn a git-view worker, install its session, and mount a "computing…"
    /// pager. A later tick (`drain_git_view_session`) renders the built model
    /// into that pager. Mirrors `open_grep_pager`, except the worker reports
    /// once and the session is retained to back the `|` toggle.
    pub fn open_git_view(&mut self, kind: GitViewKind, title: String) {
        let id = self.runtime.next_git_view_id;
        self.runtime.next_git_view_id = self.runtime.next_git_view_id.wrapping_add(1);
        let (tx, rx) = std::sync::mpsc::channel::<GitViewPayload>();
        // Wake the loop after the worker's single send and once more after it
        // returns (mirrors the grep final-wake) so the drain runs even if the
        // first wake raced a coalesce.
        let wake = self.make_git_view_wake();
        let final_wake = std::sync::Arc::clone(&wake);
        let tx = crate::fs::WakingSender::new(tx, wake);
        std::thread::spawn(move || {
            let payload = build_payload(kind);
            let _ = tx.send(payload);
            final_wake();
        });

        let mut view =
            pager::PagerView::new_plain(format!("{title} — computing…"), Vec::<String>::new());
        view.streaming = true;
        view.git_view_id = Some(id);
        view.saveable = true;
        // Push any previously-open pager onto the back stack so the user can
        // `:bprev` to it. Save its scroll first (mirrors `open_grep_pager`).
        self.remember_pager_position();
        if let Some(prev) = self.view.pager.take() {
            self.view.pager_history.push(prev);
        }
        self.set_pager(view);
        self.view.needs_full_repaint = true;
        self.runtime.git_view_session = Some(GitViewSession {
            id,
            rx,
            // SideBySide is the product default for diff/show.
            layout: DiffLayout::SideBySide,
            model: None,
            complete: false,
            title,
        });
    }

    /// Drain the git-view worker's one-shot result into the active pager.
    /// Called from the tick loop. Returns true when something changed (model
    /// rendered, error/empty flashed) so the caller can request a redraw.
    ///
    /// Unlike `drain_grep_session`, this does NOT drop the session on
    /// completion: the retained model backs the `|` layout toggle. The
    /// session is dropped only when its backing pager is gone (id-gate below)
    /// or on an empty/error/disconnect terminal result.
    pub fn drain_git_view_session(&mut self) -> bool {
        let Some(session) = self.runtime.git_view_session.as_mut() else {
            return false;
        };
        // Drop the session if the matching pager is gone (closed/replaced).
        let pager_matches = self
            .view
            .pager
            .as_ref()
            .is_some_and(|p| p.git_view_id == Some(session.id));
        if !pager_matches {
            self.runtime.git_view_session = None;
            return false;
        }
        // Already rendered — the session lives only to back the toggle.
        if session.complete {
            return false;
        }
        match session.rx.try_recv() {
            Ok(GitViewPayload::Empty) => {
                self.state.flash_info("no changes");
                self.close_git_view_pager();
                self.runtime.git_view_session = None;
                true
            }
            Ok(GitViewPayload::Error(msg)) => {
                self.state.flash_error(msg);
                self.close_git_view_pager();
                self.runtime.git_view_session = None;
                true
            }
            Ok(GitViewPayload::Diff(m)) => {
                session.model = Some(GitViewModel::Diff(m));
                session.complete = true;
                self.render_git_view_into_pager();
                true
            }
            Ok(GitViewPayload::Show(b)) => {
                session.model = Some(GitViewModel::Show(b));
                session.complete = true;
                self.render_git_view_into_pager();
                true
            }
            Ok(GitViewPayload::Blame(m)) => {
                session.model = Some(GitViewModel::Blame(m));
                session.complete = true;
                self.render_git_view_into_pager();
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.state.flash_error("git: worker failed");
                self.close_git_view_pager();
                self.runtime.git_view_session = None;
                true
            }
        }
    }

    /// Render the session's retained model into the live git-view pager,
    /// replacing its `lines` and setting `show_line_numbers` / title /
    /// `streaming`. No-op if there's no session/model or no matching pager.
    ///
    /// Borrow note: the render reads the session immutably (model + layout +
    /// title) to produce a local `Vec<Line>` and the derived flags FIRST, then
    /// drops that borrow before mutating `self.view.pager`. This avoids holding
    /// `&self.runtime.git_view_session` and `&mut self.view.pager` at once.
    fn render_git_view_into_pager(&mut self) {
        let width = render_width();
        let theme = &self.view.theme;
        let Some(session) = self.runtime.git_view_session.as_ref() else {
            return;
        };
        let Some(model) = session.model.as_ref() else {
            return;
        };
        let (lines, show_line_numbers) = render_model(model, theme, session.layout, width);
        let title = session.title.clone();
        // The immutable session borrow ends here; now mutate the pager.
        if let Some(view) = self.view.pager.as_mut() {
            view.lines = lines;
            view.show_line_numbers = show_line_numbers;
            view.streaming = false;
            view.title = title;
        }
    }

    /// `|` in the pager: flip the diff/show layout (unified⇄side-by-side) and
    /// re-render from the retained model. Returns true when it handled the key
    /// (an active diff/show git-view pager matched); false otherwise (blame or
    /// no git-view), so the key falls through as a no-op.
    pub fn toggle_git_view_layout(&mut self) -> bool {
        let Some(session) = self.runtime.git_view_session.as_mut() else {
            return false;
        };
        // Only meaningful for diff/show (blame has no layout).
        if !matches!(
            session.model,
            Some(GitViewModel::Diff(_) | GitViewModel::Show(_))
        ) {
            return false;
        }
        // The pager must still be the one this session backs.
        let matches = self
            .view
            .pager
            .as_ref()
            .is_some_and(|p| p.git_view_id == Some(session.id));
        if !matches {
            return false;
        }
        session.layout = match session.layout {
            DiffLayout::Unified => DiffLayout::SideBySide,
            DiffLayout::SideBySide => DiffLayout::Unified,
        };
        self.render_git_view_into_pager();
        true
    }

    /// Close the active git-view pager: restore the prior pager from history
    /// (so `gd`/`g show`/`gb` on a clean tree pops back to where the user
    /// was), or clear it if history is empty. Used by the empty/error/
    /// disconnect terminal paths.
    fn close_git_view_pager(&mut self) {
        let is_git_view = self
            .view
            .pager
            .as_ref()
            .is_some_and(|p| p.git_view_id.is_some());
        if !is_git_view {
            return;
        }
        self.view.pager = self.view.pager_history.pop_back();
        self.view.needs_full_repaint = true;
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

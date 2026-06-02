//! State for an active `:grep` session. The worker thread runs the
//! content searcher and pushes batches of matches through `rx`; the
//! main tick loop (`App::drain_grep_session`) drains them and appends
//! to the pager view whose `grep_id` matches `id`. When the matching
//! pager is closed or replaced (`bprev`/`bnext`/Esc/etc.), the session
//! is dropped and the worker exits on its next send.
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 1 + the impl-extraction
//! sweep). Fields are `pub` because the struct is built via a literal and
//! read back in the drain + title-render paths. The `:grep` open + drain
//! `impl App` methods live here too (`pub`, called from `commands` / the run
//! loop).

use std::path::PathBuf;

use crate::ui::pager;

use super::App;

pub struct GrepSession {
    /// Unique session id; pasted onto the pager view's `grep_id` so
    /// stale workers can't bleed into a fresh search.
    pub id: u32,
    /// Receiver for streaming match batches from the worker.
    pub rx: std::sync::mpsc::Receiver<Vec<crate::fs::grep::GrepMatch>>,
    /// Total matches forwarded so far. Drives the title's progress
    /// suffix and the cap-hit warning.
    pub count: usize,
    /// True once the worker disconnected (walk complete or cap hit).
    /// The pager flips `streaming` off and the title shows the final
    /// count instead of "scanning…".
    pub complete: bool,
    /// Cap-hit flag — set when `count` reaches `MAX_MATCHES` so the
    /// final title can warn the user that results were truncated.
    pub capped: bool,
    /// Pattern echoed in the title.
    pub pattern: String,
    /// Display root (project home or listing dir) for context in the
    /// title.
    pub root: PathBuf,
}

impl App {
    /// Spawn a `:grep` worker, install its session, and open a pager
    /// pre-populated with the title and an empty body. Subsequent
    /// ticks drain the rx and append rendered match lines until the
    /// worker disconnects or the pager is replaced.
    pub fn open_grep_pager(&mut self, pattern: &str) {
        let root = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.listing.dir.clone());
        // Validate the pattern up-front so we can flash an error
        // inline rather than open an empty pager that silently
        // produces zero results. The worker re-compiles the same
        // regex, but parse cost is trivial.
        if let Err(e) = grep_regex::RegexMatcherBuilder::new()
            .case_smart(true)
            .build(pattern)
        {
            self.state.flash_error(format!("grep: {e}"));
            return;
        }
        let id = self.runtime.next_grep_id;
        self.runtime.next_grep_id = self.runtime.next_grep_id.wrapping_add(1);
        let (tx, rx) = std::sync::mpsc::channel();
        let walk_root = root.clone();
        let pat = pattern.to_string();
        let pat_for_thread = pat.clone();
        // MVU Phase 3d: wake the loop on each batch (via WakingSender) and
        // once more after the worker returns — that final wake drives the
        // last drain_grep_session, which sees the rx disconnect and marks the
        // session complete (title loses "scanning…") with no poll floor.
        let wake = self.make_grep_wake();
        let final_wake = std::sync::Arc::clone(&wake);
        let tx = crate::fs::WakingSender::new(tx, wake);
        std::thread::spawn(move || {
            let _ = crate::fs::grep::search_streaming(&walk_root, &pat_for_thread, tx);
            final_wake();
        });
        let title = format!("grep — \"{pat}\" — scanning…");
        let mut view = pager::PagerView::new_plain(title, Vec::<String>::new());
        view.streaming = true;
        // Lock the gutter to the cap so it doesn't widen as results
        // stream in (otherwise visible text shifts right each time
        // the count crosses a power of 10: 9→10, 99→100, etc.).
        view.line_count_hint = Some(crate::fs::grep::MAX_MATCHES);
        view.grep_id = Some(id);
        view.saveable = true;
        // Push any previously-open pager onto the back stack so the
        // user can `:bprev` to it. Save its scroll first so the
        // position survives a crash before the user `:bprev`s back.
        self.remember_pager_position();
        if let Some(prev) = self.view.pager.take() {
            self.view.pager_history.push(prev);
        }
        self.set_pager(view);
        self.runtime.grep_session = Some(GrepSession {
            id,
            rx,
            count: 0,
            complete: false,
            capped: false,
            pattern: pat,
            root,
        });
        self.view.needs_full_repaint = true;
    }

    /// Drain any pending grep matches into the active pager. Called
    /// from the tick loop. Returns true when something changed
    /// (matches appended or worker completed) so the caller can
    /// request a redraw.
    pub fn drain_grep_session(&mut self) -> bool {
        let Some(session) = self.runtime.grep_session.as_mut() else {
            return false;
        };
        // Drop the session if the matching pager is gone. The user
        // closed/replaced it; the worker keeps running but will exit
        // on its next send when our rx is dropped.
        let pager_matches = self
            .view
            .pager
            .as_ref()
            .is_some_and(|p| p.grep_id == Some(session.id));
        if !pager_matches {
            self.runtime.grep_session = None;
            return false;
        }
        let mut got_any = false;
        loop {
            match session.rx.try_recv() {
                Ok(batch) => {
                    if let Some(view) = self.view.pager.as_mut() {
                        for m in &batch {
                            view.lines.push(ratatui::text::Line::from(m.render()));
                        }
                    }
                    session.count += batch.len();
                    if session.count >= crate::fs::grep::MAX_MATCHES {
                        session.capped = true;
                    }
                    got_any = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    session.complete = true;
                    got_any = true;
                    break;
                }
            }
        }
        if got_any {
            // Refresh title with current count + status.
            let suffix = if session.complete {
                if session.capped {
                    format!(" — {} matches (cap; refine pattern)", session.count)
                } else {
                    format!(" — {} matches", session.count)
                }
            } else {
                format!(" — {} matches — scanning…", session.count)
            };
            let root_label = crate::paths::display_tilde(&session.root);
            let new_title = format!("grep — \"{}\" — {root_label}{suffix}", session.pattern);
            if let Some(view) = self.view.pager.as_mut() {
                view.title = new_title;
                if session.complete {
                    view.streaming = false;
                }
            }
            if session.complete {
                self.runtime.grep_session = None;
            }
        }
        got_any
    }
}

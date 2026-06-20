//! `GrepStream`: the `:grep` content-search stream. A [`PagerStream`] that
//! appends streamed match batches to its overlay pager and refreshes the
//! title's progress suffix, completing when the worker disconnects. Migrated
//! onto the `pager_stream` abstraction (was the bespoke `GrepSession` +
//! `drain_grep_session`). The `:grep` open entry point (`open_grep_pager`,
//! called from `commands`) lives here too.

use std::path::PathBuf;

use ratatui::text::Line;

use super::App;
use super::pager_stream::{DrainOutcome, PagerStream, PagerStreamMount, RenderCtx};
use crate::ui::pager::PagerView;

/// A [`PagerStream`] for an active `:grep`: the worker streams match batches
/// over `rx`; each drain appends them to the pager and refreshes the progress
/// title, completing (no retain) when the worker disconnects (walk done / cap).
pub struct GrepStream {
    id: u32,
    rx: std::sync::mpsc::Receiver<Vec<crate::fs::grep::GrepMatch>>,
    /// Total matches forwarded so far — drives the title + the cap warning.
    count: usize,
    /// Set once `count` reaches `MAX_MATCHES` (truncation warning in the title).
    capped: bool,
    /// Pattern echoed in the title.
    pattern: String,
    /// Display root (project home or listing dir) for title context.
    root: PathBuf,
}

impl PagerStream for GrepStream {
    fn id(&self) -> u32 {
        self.id
    }

    fn drain(&mut self, view: &mut PagerView, _ctx: &RenderCtx) -> DrainOutcome {
        let mut got_any = false;
        let mut complete = false;
        loop {
            match self.rx.try_recv() {
                Ok(batch) => {
                    for m in &batch {
                        view.lines.push(Line::from(m.render()));
                    }
                    self.count += batch.len();
                    if self.count >= crate::fs::grep::MAX_MATCHES {
                        self.capped = true;
                    }
                    got_any = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    complete = true;
                    got_any = true;
                    break;
                }
            }
        }
        if !got_any {
            return DrainOutcome::Idle;
        }
        // Refresh the title with the running count + status.
        let suffix = if complete {
            if self.capped {
                format!(" — {} matches (cap; refine pattern)", self.count)
            } else {
                format!(" — {} matches", self.count)
            }
        } else {
            format!(" — {} matches — scanning…", self.count)
        };
        let root_label = crate::paths::display_tilde(&self.root);
        view.title = format!("grep — \"{}\" — {root_label}{suffix}", self.pattern);
        if complete {
            view.streaming = false;
            DrainOutcome::Finished
        } else {
            DrainOutcome::Changed
        }
    }
}

impl App {
    /// Spawn a `:grep` worker, mount an overlay pager pre-populated with the
    /// title + an empty body, and install the streaming session. Subsequent
    /// ticks (`drain_pager_stream` → `GrepStream::drain`) append rendered match
    /// lines until the worker disconnects or the pager is replaced.
    pub fn open_grep_pager(&mut self, pattern: &str) {
        let root = self.state.tool_root(self.state.focused_side());
        // Validate the pattern up-front so we can flash an error inline rather
        // than open an empty pager that silently produces zero results. The
        // worker re-compiles the same regex (trivial parse cost).
        if let Err(e) = grep_regex::RegexMatcherBuilder::new()
            .case_smart(true)
            .build(pattern)
        {
            self.state.flash_error(format!("grep: {e}"));
            return;
        }
        let pat = pattern.to_string();
        let walk_root = root.clone();
        let pat_for_thread = pat.clone();
        self.spawn_pager_stream(
            PagerStreamMount::Overlay {
                title: format!("grep — \"{pat}\" — scanning…"),
                // Lock the gutter to the cap so it doesn't widen as results
                // stream in (otherwise visible text shifts right each time the
                // count crosses a power of 10: 9→10, 99→100, …).
                line_count_hint: Some(crate::fs::grep::MAX_MATCHES),
            },
            move |tx| {
                let _ = crate::fs::grep::search_streaming(&walk_root, &pat_for_thread, tx);
            },
            move |id, rx| {
                Box::new(GrepStream {
                    id,
                    rx,
                    count: 0,
                    capped: false,
                    pattern: pat,
                    root,
                })
            },
        );
    }
}

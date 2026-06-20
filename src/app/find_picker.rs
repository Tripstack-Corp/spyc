//! `F` project-wide fuzzy filename picker. The walk runs in a worker
//! thread streaming batches of paths through `walk_rx`; the picker is
//! interactive immediately and the candidate list grows live as the
//! walker progresses. Re-rank runs on every keystroke and on every
//! fresh batch arrival (cheap: ~1us per candidate).
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 1 + the impl-extraction
//! sweep). Fields are `pub` (built via a struct literal). The `F` open /
//! render / key-handler `impl App` methods live here too (`pub`, called from
//! `actions` / `key_dispatch` / the run loop).

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::pager;

use super::App;

pub struct FindPicker {
    /// Repo-relative paths accumulated from the walk so far.
    /// Append-only during the walk; never modified by the user.
    pub candidates: Vec<PathBuf>,
    /// Absolute root the walk started from. Used to construct the
    /// final absolute path on Enter.
    pub root: PathBuf,
    /// User's current input.
    pub query: String,
    /// Current ranked subset (paths only; scores discarded after
    /// sort). Re-built on keystroke or new-batch arrival.
    pub filtered: Vec<PathBuf>,
    /// Index into `filtered`. 0 when query just changed; arrows
    /// move it within `[0, filtered.len())`.
    pub selected: usize,
    /// Cap on rendered results so a 100K-file repo doesn't blow up
    /// the pager Line vec on first paint.
    pub limit: usize,
    /// Receiver for streaming candidate batches from the walker
    /// thread. Set to `None` once the walk completes (channel
    /// disconnects when the worker drops its sender).
    pub walk_rx: Option<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
    /// True once the walker thread has finished. Drives the title
    /// suffix ("scanning..." vs final count).
    pub walk_complete: bool,
}

impl FindPicker {
    /// Re-rank `candidates` against the current `query` and store in
    /// `filtered`, keeping the cursor on the same path if it survives the
    /// re-rank. The walker streams candidates in batches and re-ranks on
    /// each; resetting `selected` to 0 every batch (the old behavior) yanked
    /// the cursor back to the top under the user, so a batch arriving just
    /// before Enter opened the wrong file. On a query change the previously
    /// selected path usually isn't in the new results, so it falls back to 0.
    pub fn refilter(&mut self) {
        let prev = self.filtered.get(self.selected).cloned();
        self.filtered = crate::fs::finder::rank(&self.candidates, &self.query, self.limit)
            .into_iter()
            .map(|(p, _score)| p)
            .collect();
        self.selected = prev
            .and_then(|p| self.filtered.iter().position(|q| *q == p))
            .unwrap_or(0);
    }

    /// Drain any batches that have arrived since the last tick.
    /// Returns true when new candidates were appended OR when the
    /// walk completed (caller should re-render either way: title
    /// changes from "scanning..." to a final count).
    pub fn drain_walk(&mut self) -> bool {
        let Some(rx) = self.walk_rx.as_ref() else {
            return false;
        };
        let mut got_any = false;
        loop {
            match rx.try_recv() {
                Ok(batch) => {
                    self.candidates.extend(batch);
                    got_any = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.walk_rx = None;
                    self.walk_complete = true;
                    got_any = true;
                    break;
                }
            }
        }
        got_any
    }
}

impl App {
    /// Open the F-finder. Spawns the walker on a worker thread so
    /// the picker is interactive immediately (typing filters the
    /// already-arrived candidates while the walker keeps streaming
    /// in the background). Closing the picker drops the receiver,
    /// which makes the walker exit on its next `tx.send`.
    pub fn open_find_picker(&mut self) {
        let root = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.cur().listing.dir.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let walk_root = root.clone();
        // MVU Phase 3d: wake the loop on each candidate batch (via
        // WakingSender) and once more after the walk returns — that final
        // wake drives the last drain_walk, which sees the rx disconnect and
        // flips `walk_complete` (title → final count) without the poll floor.
        let wake = self.make_find_wake();
        let final_wake = std::sync::Arc::clone(&wake);
        let tx = crate::fs::WakingSender::new(tx, wake);
        std::thread::spawn(move || {
            crate::fs::finder::walk_streaming(&walk_root, tx);
            final_wake();
        });
        let mut picker = FindPicker {
            candidates: Vec::new(),
            root,
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            limit: 200,
            walk_rx: Some(rx),
            walk_complete: false,
        };
        picker.refilter();
        self.runtime.find_picker = Some(picker);
        self.render_find_picker();
        self.view.needs_full_repaint = true;
    }

    /// Rebuild the pager view from current `find_picker` state.
    /// Called on open, after each keystroke that mutates the query
    /// or selection, and after each tick where the streaming walk
    /// produced new candidates (title shows progress).
    pub fn render_find_picker(&mut self) {
        let Some(picker) = self.runtime.find_picker.as_ref() else {
            return;
        };
        let total = picker.candidates.len();
        let shown = picker.filtered.len();
        let pos = if shown == 0 { 0 } else { picker.selected + 1 };
        let scan_suffix = if picker.walk_complete {
            String::new()
        } else {
            " — scanning…".to_string()
        };
        let title = format!(
            "find — \"{}\" — {pos}/{shown} of {total}{scan_suffix}",
            picker.query
        );
        let lines: Vec<String> = picker
            .filtered
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let mut view = pager::PagerView::new_plain(title, lines);
        view.show_line_numbers = false;
        view.no_history = true;
        // Picker rows must map 1:1 to source lines so the cursor +
        // selection math stays correct -- wrap would split a long
        // path across multiple visual rows and break that.
        view.wrap = false;
        view.picker_cursor = if shown == 0 {
            None
        } else {
            Some(picker.selected)
        };
        // While the walker is still streaming, suppress [EOF] /
        // tilde markers since the candidate list is still growing.
        view.streaming = !picker.walk_complete;
        self.set_pager(view);
    }

    /// Handle a key while the F-finder is open. The picker owns all input in
    /// this state — `route_input` routes here via `InputSink::FindPicker` and
    /// the caller returns unconditionally — so every key is swallowed and there
    /// is nothing to report back. Esc closes; Enter chdirs to the matched
    /// file's parent and places the cursor on it; Up/Down move selection;
    /// printable chars + Backspace edit the query and re-rank.
    pub fn handle_find_picker_key(&mut self, key: KeyEvent) {
        if self.runtime.find_picker.is_none() {
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.runtime.find_picker = None;
                self.clear_pager();
                self.view.needs_full_repaint = true;
            }
            KeyCode::Enter => {
                let target = self.runtime.find_picker.as_ref().and_then(|p| {
                    p.filtered
                        .get(p.selected)
                        .cloned()
                        .map(|rel| (p.root.clone(), rel))
                });
                self.runtime.find_picker = None;
                self.clear_pager();
                self.view.needs_full_repaint = true;
                if let Some((root, rel)) = target {
                    let abs = root.join(&rel);
                    if let Some(parent) = abs.parent() {
                        if let Err(e) = self.state.chdir(parent) {
                            self.state.flash_error(format!("chdir: {e}"));
                        } else if let Some(idx) =
                            self.state.cur().rows.iter().position(|r| r.path == abs)
                        {
                            self.state.cur_mut().cursor.index = idx;
                            let row_count = self.state.cur().rows.len();
                            self.state.cur_mut().cursor.clamp(row_count);
                        }
                    }
                }
            }
            KeyCode::Up => {
                if let Some(picker) = self.runtime.find_picker.as_mut()
                    && picker.selected > 0
                {
                    picker.selected -= 1;
                    self.render_find_picker();
                }
            }
            KeyCode::Down => {
                if let Some(picker) = self.runtime.find_picker.as_mut()
                    && picker.selected + 1 < picker.filtered.len()
                {
                    picker.selected += 1;
                    self.render_find_picker();
                }
            }
            KeyCode::Backspace => {
                if let Some(picker) = self.runtime.find_picker.as_mut()
                    && !picker.query.is_empty()
                {
                    picker.query.pop();
                    picker.refilter();
                    self.render_find_picker();
                }
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(picker) = self.runtime.find_picker.as_mut() {
                    picker.query.push(c);
                    picker.refilter();
                    self.render_find_picker();
                }
            }
            _ => {} // Swallow other keys while picker is open.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FindPicker;
    use std::path::PathBuf;

    fn picker(candidates: &[&str]) -> FindPicker {
        FindPicker {
            candidates: candidates.iter().map(PathBuf::from).collect(),
            root: PathBuf::from("/"),
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            limit: 200,
            walk_rx: None,
            walk_complete: false,
        }
    }

    #[test]
    fn refilter_preserves_selection_across_streaming_batches() {
        let mut p = picker(&["a.rs", "b.rs", "c.rs"]);
        p.refilter();
        // Park the cursor on the second result.
        p.selected = 1;
        let target = p.filtered[1].clone();
        // A new batch streams in (drain_walk appends, then refilters).
        p.candidates.push(PathBuf::from("d.rs"));
        p.refilter();
        // Cursor still on the same path — not yanked back to the top.
        assert_eq!(p.filtered[p.selected], target);
    }

    #[test]
    fn refilter_resets_to_top_when_selected_path_is_filtered_out() {
        let mut p = picker(&["alpha.rs", "beta.rs"]);
        p.refilter();
        p.selected = p
            .filtered
            .iter()
            .position(|x| x.ends_with("beta.rs"))
            .unwrap();
        // A query change that excludes the previously-selected path.
        p.query = "alpha".to_string();
        p.refilter();
        assert_eq!(p.selected, 0);
    }
}

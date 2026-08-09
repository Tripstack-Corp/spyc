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
//!
//! Key handling splits the `route.rs` / `focus.rs` way: the pure
//! [`decide_find_picker_key`] maps a `Copy` snapshot + key to a
//! [`FindPickerAction`], and `handle_find_picker_key` only applies it. Every
//! bound (list ends, empty query, the CONTROL/ALT chord guard) is decided in
//! the pure half, so the key matrix is table-testable without a live walk.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::pager;

use super::App;

/// The picker state [`decide_find_picker_key`] reads. `Copy` so tests construct
/// one inline.
#[derive(Debug, Clone, Copy)]
struct FindPickerSnapshot {
    /// Cursor index into the ranked results.
    selected: usize,
    /// How many results the current query matched.
    filtered_len: usize,
    /// Whether the query buffer has anything left to delete.
    query_is_empty: bool,
}

/// What a key does to the open `F` picker. Each variant is a complete
/// instruction — the bounds live in [`decide_find_picker_key`], so the applier
/// never re-guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FindPickerAction {
    /// Swallow it; the picker owns every key while open.
    Ignore,
    /// `Esc`, or `Enter` with nothing to accept — close without navigating.
    Close,
    /// `Enter` on a result — close, then land on the selected path.
    Accept,
    MoveUp,
    MoveDown,
    /// `Backspace` against a non-empty query.
    Backspace,
    /// A printable char — append to the query and re-rank.
    Insert(char),
}

/// Decide what a key does to the `F` picker. **Pure** (the `route.rs` /
/// `focus.rs` template), so every branch is unit-testable without a live walk.
///
/// Takes the whole [`KeyEvent`]: the printable-char arm has to read modifiers
/// so a `^n` / `⌥f` doesn't get typed into the query as an `n` / `f`.
const fn decide_find_picker_key(snap: FindPickerSnapshot, key: KeyEvent) -> FindPickerAction {
    match key.code {
        // `Enter` navigates only when the cursor is over a real result; against
        // an empty result set it merely dismisses, exactly like `Esc`.
        KeyCode::Enter if snap.selected < snap.filtered_len => FindPickerAction::Accept,
        KeyCode::Esc | KeyCode::Enter => FindPickerAction::Close,
        KeyCode::Up if snap.selected > 0 => FindPickerAction::MoveUp,
        KeyCode::Down if snap.selected + 1 < snap.filtered_len => FindPickerAction::MoveDown,
        KeyCode::Backspace if !snap.query_is_empty => FindPickerAction::Backspace,
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            FindPickerAction::Insert(c)
        }
        _ => FindPickerAction::Ignore,
    }
}

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
        let root = self.state.tool_root(self.state.focused_side());
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
        let Some(picker) = self.runtime.find_picker.as_ref() else {
            return;
        };
        let snap = FindPickerSnapshot {
            selected: picker.selected,
            filtered_len: picker.filtered.len(),
            query_is_empty: picker.query.is_empty(),
        };

        match decide_find_picker_key(snap, key) {
            FindPickerAction::Ignore => {}
            FindPickerAction::Close => self.close_find_picker(),
            FindPickerAction::Accept => {
                let target = self
                    .runtime
                    .find_picker
                    .as_ref()
                    .and_then(|p| p.filtered.get(p.selected).map(|rel| p.root.join(rel)));
                self.close_find_picker();
                if let Some(abs) = target {
                    self.land_on_found_path(&abs);
                }
            }
            FindPickerAction::MoveUp => {
                if let Some(picker) = self.runtime.find_picker.as_mut() {
                    picker.selected = picker.selected.saturating_sub(1);
                }
                self.render_find_picker();
            }
            FindPickerAction::MoveDown => {
                if let Some(picker) = self.runtime.find_picker.as_mut() {
                    picker.selected += 1;
                }
                self.render_find_picker();
            }
            FindPickerAction::Backspace => {
                if let Some(picker) = self.runtime.find_picker.as_mut() {
                    picker.query.pop();
                    picker.refilter();
                }
                self.render_find_picker();
            }
            FindPickerAction::Insert(c) => {
                if let Some(picker) = self.runtime.find_picker.as_mut() {
                    picker.query.push(c);
                    picker.refilter();
                }
                self.render_find_picker();
            }
        }
    }

    /// Tear down the picker together with the pager view it renders into.
    fn close_find_picker(&mut self) {
        self.runtime.find_picker = None;
        self.clear_pager();
        self.view.needs_full_repaint = true;
    }

    /// Cursor-land on `abs`: chdir to its parent and park the cursor on the
    /// file itself, leaving the verb to the user — the same semantics as a
    /// harpoon jump. A chdir failure flashes its whole cause chain.
    fn land_on_found_path(&mut self, abs: &Path) {
        let Some(parent) = abs.parent() else {
            return;
        };
        if let Err(e) = self.state.chdir(parent) {
            self.state.flash_error(format!("chdir: {e:#}"));
            return;
        }
        if let Some(idx) = self
            .state
            .cur()
            .rows
            .iter()
            .position(|r| r.path.as_path() == abs)
        {
            self.state.cur_mut().cursor.index = idx;
            let row_count = self.state.cur().rows.len();
            self.state.cur_mut().cursor.clamp(row_count);
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

#[cfg(test)]
mod picker_key_tests {
    use super::{FindPickerAction, FindPickerSnapshot, decide_find_picker_key};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// A picker showing `filtered_len` results with the cursor at `selected`
    /// and an empty query.
    const fn snap(selected: usize, filtered_len: usize) -> FindPickerSnapshot {
        FindPickerSnapshot {
            selected,
            filtered_len,
            query_is_empty: true,
        }
    }

    /// Same, but with something typed to delete.
    const fn typed(selected: usize, filtered_len: usize) -> FindPickerSnapshot {
        FindPickerSnapshot {
            selected,
            filtered_len,
            query_is_empty: false,
        }
    }

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn decide(s: FindPickerSnapshot, code: KeyCode) -> FindPickerAction {
        decide_find_picker_key(s, key(code, KeyModifiers::NONE))
    }

    #[test]
    fn esc_closes() {
        assert_eq!(decide(snap(0, 5), KeyCode::Esc), FindPickerAction::Close);
    }

    #[test]
    fn enter_accepts_the_selected_result() {
        assert_eq!(decide(snap(0, 5), KeyCode::Enter), FindPickerAction::Accept);
        assert_eq!(decide(snap(4, 5), KeyCode::Enter), FindPickerAction::Accept);
    }

    #[test]
    fn enter_on_no_results_closes_without_navigating() {
        // A query matching nothing still dismisses the picker — it just has no
        // path to land on.
        assert_eq!(decide(snap(0, 0), KeyCode::Enter), FindPickerAction::Close);
        // Defensive: a cursor somehow past the results is not a valid accept.
        assert_eq!(decide(snap(9, 5), KeyCode::Enter), FindPickerAction::Close);
    }

    #[test]
    fn up_and_down_move_within_the_results() {
        assert_eq!(decide(snap(1, 5), KeyCode::Up), FindPickerAction::MoveUp);
        assert_eq!(
            decide(snap(1, 5), KeyCode::Down),
            FindPickerAction::MoveDown
        );
    }

    #[test]
    fn up_and_down_are_inert_at_the_ends() {
        // Neither may run off the list: `Up` at the top would underflow
        // `selected`, `Down` at the bottom would index past `filtered`.
        assert_eq!(decide(snap(0, 5), KeyCode::Up), FindPickerAction::Ignore);
        assert_eq!(decide(snap(4, 5), KeyCode::Down), FindPickerAction::Ignore);
        assert_eq!(decide(snap(0, 0), KeyCode::Up), FindPickerAction::Ignore);
        assert_eq!(decide(snap(0, 0), KeyCode::Down), FindPickerAction::Ignore);
    }

    #[test]
    fn backspace_edits_only_a_non_empty_query() {
        assert_eq!(
            decide(typed(0, 5), KeyCode::Backspace),
            FindPickerAction::Backspace
        );
        assert_eq!(
            decide(snap(0, 5), KeyCode::Backspace),
            FindPickerAction::Ignore
        );
    }

    #[test]
    fn printable_chars_type_into_the_query() {
        assert_eq!(
            decide(snap(0, 5), KeyCode::Char('a')),
            FindPickerAction::Insert('a')
        );
        // Punctuation and digits are query text too — paths contain both.
        assert_eq!(
            decide(snap(0, 5), KeyCode::Char('.')),
            FindPickerAction::Insert('.')
        );
        assert_eq!(
            decide(snap(0, 5), KeyCode::Char('7')),
            FindPickerAction::Insert('7')
        );
    }

    #[test]
    fn shifted_letters_still_type() {
        // Uppercase must reach the query — the modifier guard rejects CONTROL
        // and ALT only, never SHIFT.
        assert_eq!(
            decide_find_picker_key(snap(0, 5), key(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            FindPickerAction::Insert('R')
        );
    }

    #[test]
    fn control_and_alt_chords_do_not_type_their_letter() {
        // The regression this guard exists for: `^c` must not append a literal
        // `c` to the query (and `⌥f` must not append an `f`).
        assert_eq!(
            decide_find_picker_key(snap(0, 5), key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            FindPickerAction::Ignore
        );
        assert_eq!(
            decide_find_picker_key(snap(0, 5), key(KeyCode::Char('f'), KeyModifiers::ALT)),
            FindPickerAction::Ignore
        );
    }

    #[test]
    fn unbound_keys_are_swallowed_not_leaked() {
        // The picker owns all input while open — nothing falls through to the
        // resolver behind it.
        for code in [
            KeyCode::Tab,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Delete,
            KeyCode::F(1),
        ] {
            assert_eq!(
                decide(snap(1, 5), code),
                FindPickerAction::Ignore,
                "{code:?} must be swallowed"
            );
        }
    }
}

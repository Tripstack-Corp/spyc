//! Single-line vi-mode editor for prompts.
//!
//! Starts in Insert mode (characters append at the cursor). Esc switches
//! to Normal mode where h/l/w/b/0/$/x/dd/i/a/I/A etc. work. Enter
//! submits from either mode. Up/Down cycle command history.
//!
//! This is intentionally a *subset* of vi — the 20-ish commands that
//! matter for a one-line shell prompt, not a full editor.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Insert,
    Normal,
}

/// Result of feeding a key to the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditResult {
    /// Keep editing — the key was consumed.
    Continue,
    /// User pressed Enter — submit the buffer.
    Submit,
    /// User cancelled (Backspace on empty in Insert, or Esc+Esc quickly,
    /// or ^C).
    Cancel,
    /// Request previous history entry.
    HistoryPrev,
    /// Request next history entry.
    HistoryNext,
}

/// Pending operator waiting for a motion (d, c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingOp {
    Delete, // d{motion}
    Change, // c{motion} — delete then enter Insert
}

pub struct LineEditor {
    pub buf: Vec<char>,
    pub cursor: usize,
    pub mode: Mode,
    pending_op: Option<PendingOp>,
}

impl LineEditor {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            cursor: 0,
            mode: Mode::Insert,
            pending_op: None,
        }
    }

    /// Replace the buffer contents (e.g. when loading a history entry).
    pub fn set_content(&mut self, s: &str) {
        self.buf = s.chars().collect();
        self.cursor = self.buf.len();
        self.mode = Mode::Insert;
    }

    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Feed a key. Returns what the prompt loop should do next.
    pub fn feed(&mut self, key: KeyEvent) -> EditResult {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Global keys (both modes).
        if matches!(key.code, KeyCode::Enter) {
            return EditResult::Submit;
        }
        if ctrl {
            match key.code {
                KeyCode::Char('c' | 'C') => return EditResult::Cancel,
                KeyCode::Char('p' | 'P') | KeyCode::Char('k' | 'K') => {
                    return EditResult::HistoryPrev;
                }
                KeyCode::Char('n' | 'N') | KeyCode::Char('j' | 'J') => {
                    return EditResult::HistoryNext;
                }
                _ => {}
            }
        }
        if matches!(key.code, KeyCode::Up) {
            return EditResult::HistoryPrev;
        }
        if matches!(key.code, KeyCode::Down) {
            return EditResult::HistoryNext;
        }

        match self.mode {
            Mode::Insert => self.feed_insert(key),
            Mode::Normal => self.feed_normal(key),
        }
    }

    // ---- Insert mode --------------------------------------------------------

    fn feed_insert(&mut self, key: KeyEvent) -> EditResult {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                // In vi, Esc moves cursor back one if not at start.
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.buf.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.buf.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.buf.len(),
            KeyCode::Char(c) if ctrl => {
                match c {
                    'u' | 'U' => {
                        // Clear from cursor to start.
                        self.buf.drain(..self.cursor);
                        self.cursor = 0;
                    }
                    'w' | 'W' => self.delete_word_back(),
                    'a' | 'A' => self.cursor = 0, // ^A = home
                    'e' | 'E' => self.cursor = self.buf.len(), // ^E = end
                    _ => {}
                }
            }
            KeyCode::Char(c) => {
                self.buf.insert(self.cursor, c);
                self.cursor += 1;
            }
            _ => {}
        }
        EditResult::Continue
    }

    // ---- Normal mode --------------------------------------------------------

    fn feed_normal(&mut self, key: KeyEvent) -> EditResult {
        // If an operator (d/c) is pending, the next key is a motion.
        if let Some(op) = self.pending_op.take() {
            match key.code {
                KeyCode::Char('w') => {
                    let end = self.next_word_start_delete();
                    self.delete_range(self.cursor, end);
                    if op == PendingOp::Change {
                        self.mode = Mode::Insert;
                    }
                }
                KeyCode::Char('b') => {
                    let start = self.prev_word_start();
                    self.delete_range(start, self.cursor);
                    self.cursor = start;
                    if op == PendingOp::Change {
                        self.mode = Mode::Insert;
                    }
                }
                KeyCode::Char('e') => {
                    let end = (self.word_end() + 1).min(self.buf.len());
                    self.delete_range(self.cursor, end);
                    if op == PendingOp::Change {
                        self.mode = Mode::Insert;
                    }
                }
                KeyCode::Char('$') => {
                    self.buf.truncate(self.cursor);
                    if op == PendingOp::Change {
                        self.mode = Mode::Insert;
                    } else if self.cursor > 0 {
                        self.cursor -= 1;
                    }
                }
                KeyCode::Char('0') => {
                    self.delete_range(0, self.cursor);
                    self.cursor = 0;
                    if op == PendingOp::Change {
                        self.mode = Mode::Insert;
                    }
                }
                KeyCode::Char('d') if op == PendingOp::Delete => {
                    // dd — delete entire line.
                    self.buf.clear();
                    self.cursor = 0;
                }
                KeyCode::Char('c') if op == PendingOp::Change => {
                    // cc — change entire line.
                    self.buf.clear();
                    self.cursor = 0;
                    self.mode = Mode::Insert;
                }
                KeyCode::Esc => {} // cancel pending op
                _ => {}            // unknown motion, discard
            }
            return EditResult::Continue;
        }

        match key.code {
            // Movement.
            KeyCode::Char('h') | KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if self.cursor + 1 < self.buf.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Char('0') | KeyCode::Home => self.cursor = 0,
            KeyCode::Char('$') | KeyCode::End => {
                if !self.buf.is_empty() {
                    self.cursor = self.buf.len() - 1;
                }
            }
            KeyCode::Char('^') => {
                self.cursor = self
                    .buf
                    .iter()
                    .position(|c| !c.is_whitespace())
                    .unwrap_or(0);
            }
            KeyCode::Char('w') => self.cursor = self.next_word_start(),
            KeyCode::Char('b') => self.cursor = self.prev_word_start(),
            KeyCode::Char('e') => self.cursor = self.word_end(),

            // Operators — wait for a motion key.
            KeyCode::Char('d') => self.pending_op = Some(PendingOp::Delete),
            KeyCode::Char('c') => self.pending_op = Some(PendingOp::Change),

            // Editing.
            KeyCode::Char('x') => {
                if self.cursor < self.buf.len() {
                    self.buf.remove(self.cursor);
                    if self.cursor >= self.buf.len() && self.cursor > 0 {
                        self.cursor -= 1;
                    }
                }
            }
            KeyCode::Char('D') => {
                self.buf.truncate(self.cursor);
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Char('C') => {
                self.buf.truncate(self.cursor);
                self.mode = Mode::Insert;
            }
            KeyCode::Char('S') => {
                self.buf.clear();
                self.cursor = 0;
                self.mode = Mode::Insert;
            }

            // Enter Insert mode.
            KeyCode::Char('i') => self.mode = Mode::Insert,
            KeyCode::Char('a') => {
                if self.cursor < self.buf.len() {
                    self.cursor += 1;
                }
                self.mode = Mode::Insert;
            }
            KeyCode::Char('I') => {
                self.cursor = 0;
                self.mode = Mode::Insert;
            }
            KeyCode::Char('A') => {
                self.cursor = self.buf.len();
                self.mode = Mode::Insert;
            }

            KeyCode::Esc => {
                return EditResult::Cancel;
            }
            _ => {}
        }
        EditResult::Continue
    }

    // ---- Helpers ------------------------------------------------------------

    /// Delete characters in `[start..end)` and clamp cursor.
    fn delete_range(&mut self, start: usize, end: usize) {
        let end = end.min(self.buf.len());
        if start < end {
            self.buf.drain(start..end);
        }
        if self.cursor >= self.buf.len() && !self.buf.is_empty() {
            self.cursor = self.buf.len() - 1;
        }
    }

    /// Like `next_word_start` but for `dw`: includes trailing whitespace
    /// after the word (vim's delete-word semantics).
    fn next_word_start_delete(&self) -> usize {
        let n = self.buf.len();
        let mut i = self.cursor;
        while i < n && !self.buf[i].is_whitespace() {
            i += 1;
        }
        while i < n && self.buf[i].is_whitespace() {
            i += 1;
        }
        i
    }

    fn delete_word_back(&mut self) {
        // Delete trailing whitespace, then the word.
        while self.cursor > 0 && self.buf[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
        while self.cursor > 0 && !self.buf[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
    }

    fn next_word_start(&self) -> usize {
        let n = self.buf.len();
        let mut i = self.cursor;
        // Skip current word.
        while i < n && !self.buf[i].is_whitespace() {
            i += 1;
        }
        // Skip whitespace.
        while i < n && self.buf[i].is_whitespace() {
            i += 1;
        }
        i.min(n.saturating_sub(1).max(0))
    }

    fn prev_word_start(&self) -> usize {
        let mut i = self.cursor;
        if i == 0 {
            return 0;
        }
        i -= 1;
        // Skip whitespace.
        while i > 0 && self.buf[i].is_whitespace() {
            i -= 1;
        }
        // Skip word.
        while i > 0 && !self.buf[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn word_end(&self) -> usize {
        let n = self.buf.len();
        let mut i = self.cursor;
        if i >= n {
            return n.saturating_sub(1);
        }
        i += 1;
        // Skip whitespace.
        while i < n && self.buf[i].is_whitespace() {
            i += 1;
        }
        // Skip to end of word.
        while i < n && !self.buf[i].is_whitespace() {
            i += 1;
        }
        if i > 0 {
            i - 1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn insert_and_submit() {
        let mut e = LineEditor::new();
        assert_eq!(e.feed(k(KeyCode::Char('l'))), EditResult::Continue);
        assert_eq!(e.feed(k(KeyCode::Char('s'))), EditResult::Continue);
        assert_eq!(e.text(), "ls");
        assert_eq!(e.feed(k(KeyCode::Enter)), EditResult::Submit);
    }

    #[test]
    fn esc_enters_normal() {
        let mut e = LineEditor::new();
        e.feed(k(KeyCode::Char('a')));
        e.feed(k(KeyCode::Char('b')));
        assert_eq!(e.mode, Mode::Insert);
        e.feed(k(KeyCode::Esc));
        assert_eq!(e.mode, Mode::Normal);
        // Cursor moved back one.
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn normal_h_l_movement() {
        let mut e = LineEditor::new();
        for c in "hello".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc)); // Normal, cursor at 4 (last char)
        e.feed(k(KeyCode::Char('h')));
        assert_eq!(e.cursor, 3);
        e.feed(k(KeyCode::Char('l')));
        assert_eq!(e.cursor, 4);
    }

    #[test]
    fn normal_0_dollar() {
        let mut e = LineEditor::new();
        for c in "hello".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc));
        e.feed(k(KeyCode::Char('0')));
        assert_eq!(e.cursor, 0);
        e.feed(k(KeyCode::Char('$')));
        assert_eq!(e.cursor, 4);
    }

    #[test]
    fn normal_x_deletes() {
        let mut e = LineEditor::new();
        for c in "abc".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc)); // cursor on 'c' (idx 2)
        e.feed(k(KeyCode::Char('0'))); // cursor on 'a'
        e.feed(k(KeyCode::Char('x'))); // delete 'a'
        assert_eq!(e.text(), "bc");
    }

    #[test]
    fn normal_i_resumes_insert() {
        let mut e = LineEditor::new();
        for c in "ab".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc));
        e.feed(k(KeyCode::Char('i')));
        assert_eq!(e.mode, Mode::Insert);
        e.feed(k(KeyCode::Char('X')));
        assert_eq!(e.text(), "aXb");
    }

    #[test]
    fn normal_a_appends() {
        let mut e = LineEditor::new();
        for c in "ab".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc)); // cursor on 'b' (1)
        e.feed(k(KeyCode::Char('0'))); // cursor on 'a'
        e.feed(k(KeyCode::Char('a'))); // Insert after 'a'
        assert_eq!(e.mode, Mode::Insert);
        e.feed(k(KeyCode::Char('X')));
        assert_eq!(e.text(), "aXb");
    }

    #[test]
    fn backspace_on_empty_is_noop() {
        let mut e = LineEditor::new();
        assert_eq!(e.feed(k(KeyCode::Backspace)), EditResult::Continue);
    }

    #[test]
    fn up_down_history() {
        let mut e = LineEditor::new();
        assert_eq!(e.feed(k(KeyCode::Up)), EditResult::HistoryPrev);
        assert_eq!(e.feed(k(KeyCode::Down)), EditResult::HistoryNext);
    }

    #[test]
    fn word_motion_w_b() {
        let mut e = LineEditor::new();
        for c in "hello world foo".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc));
        e.feed(k(KeyCode::Char('0'))); // start
        e.feed(k(KeyCode::Char('w'))); // → 'w' of "world"
        assert_eq!(e.cursor, 6);
        e.feed(k(KeyCode::Char('w'))); // → 'f' of "foo"
        assert_eq!(e.cursor, 12);
        e.feed(k(KeyCode::Char('b'))); // → 'w' of "world"
        assert_eq!(e.cursor, 6);
    }
}

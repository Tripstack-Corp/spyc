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
    /// User cancelled — `^C` from either mode, or `Esc` while in Normal
    /// mode. (Backspace-on-empty cancellation is handled by the caller,
    /// not here.)
    Cancel,
    /// Request previous history entry.
    HistoryPrev,
    /// Request next history entry.
    HistoryNext,
    /// User pressed Tab — request path/command completion.
    TabComplete,
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
    /// Switches to Insert mode with cursor at end.
    pub fn set_content(&mut self, s: &str) {
        self.buf = s.chars().collect();
        self.cursor = self.buf.len();
        self.mode = Mode::Insert;
    }

    /// Replace buffer contents, preserving the current mode.
    /// In Normal mode, cursor lands on the last character.
    pub fn set_content_keep_mode(&mut self, s: &str) {
        let was_normal = self.mode == Mode::Normal;
        self.buf = s.chars().collect();
        if was_normal && !self.buf.is_empty() {
            self.cursor = self.buf.len() - 1;
        } else {
            self.cursor = self.buf.len();
        }
    }

    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Splice a string into the buffer at the current cursor position,
    /// advancing the cursor past the inserted text. Used by the paste
    /// handler so an OS clipboard paste (bracketed paste / OSC 52
    /// equivalent) lands where the cursor is, not at the end of the
    /// line. Mode-agnostic; callers use it whether the user is in
    /// Insert or Normal mode.
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.buf.insert(self.cursor, c);
            self.cursor += 1;
        }
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
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('\t')) {
            return EditResult::TabComplete;
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
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.buf.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.buf.len() => {
                self.buf.remove(self.cursor);
            }
            KeyCode::Left if self.cursor > 0 => {
                self.cursor -= 1;
            }
            KeyCode::Right if self.cursor < self.buf.len() => {
                self.cursor += 1;
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
                    // cw stops at end of current word (vim convention).
                    // dw deletes through trailing whitespace to next word.
                    let end = if op == PendingOp::Change {
                        self.word_end_exclusive()
                    } else {
                        self.next_word_start_delete()
                    };
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
            KeyCode::Char('h') | KeyCode::Left if self.cursor > 0 => {
                self.cursor -= 1;
            }
            KeyCode::Char('l') | KeyCode::Right if self.cursor + 1 < self.buf.len() => {
                self.cursor += 1;
            }
            KeyCode::Char('0') | KeyCode::Home => self.cursor = 0,
            KeyCode::Char('$') | KeyCode::End if !self.buf.is_empty() => {
                self.cursor = self.buf.len() - 1;
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

            // History (like vim command-line normal mode).
            KeyCode::Char('k') => return EditResult::HistoryPrev,
            KeyCode::Char('j') => return EditResult::HistoryNext,

            // Operators — wait for a motion key.
            KeyCode::Char('d') => self.pending_op = Some(PendingOp::Delete),
            KeyCode::Char('c') => self.pending_op = Some(PendingOp::Change),

            // Editing.
            KeyCode::Char('x') if self.cursor < self.buf.len() => {
                self.buf.remove(self.cursor);
                if self.cursor >= self.buf.len() && self.cursor > 0 {
                    self.cursor -= 1;
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

    /// End of current word (exclusive) — for `cw`. Word boundary
    /// is a class transition: alphanumeric/underscore vs.
    /// punctuation vs. whitespace. So `foo-bar` cw at 0 only
    /// changes `foo` (stops at `-`), matching vim's default `iskeyword`.
    fn word_end_exclusive(&self) -> usize {
        let n = self.buf.len();
        if self.cursor >= n {
            return n;
        }
        let mut i = self.cursor;
        let cls = char_class(self.buf[i]);
        while i < n && char_class(self.buf[i]) == cls {
            i += 1;
        }
        i
    }

    /// Like `word_end_exclusive` but also skips trailing whitespace — for `dw`.
    fn next_word_start_delete(&self) -> usize {
        let mut i = self.word_end_exclusive();
        let n = self.buf.len();
        while i < n && char_class(self.buf[i]) == CharClass::Space {
            i += 1;
        }
        i
    }

    fn delete_word_back(&mut self) {
        // Delete trailing whitespace, then the previous-class chunk.
        while self.cursor > 0 && char_class(self.buf[self.cursor - 1]) == CharClass::Space {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
        if self.cursor == 0 {
            return;
        }
        let cls = char_class(self.buf[self.cursor - 1]);
        while self.cursor > 0 && char_class(self.buf[self.cursor - 1]) == cls {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
    }

    fn next_word_start(&self) -> usize {
        let n = self.buf.len();
        if self.cursor >= n {
            return n.saturating_sub(1);
        }
        let mut i = self.cursor;
        let cls = char_class(self.buf[i]);
        if cls == CharClass::Space {
            // Just skip whitespace; we land at the start of the
            // next word/punct chunk.
            while i < n && char_class(self.buf[i]) == CharClass::Space {
                i += 1;
            }
        } else {
            // Skip the rest of the current word/punct chunk.
            while i < n && char_class(self.buf[i]) == cls {
                i += 1;
            }
            // Skip any whitespace separating us from the next chunk.
            while i < n && char_class(self.buf[i]) == CharClass::Space {
                i += 1;
            }
        }
        i.min(n.saturating_sub(1))
    }

    fn prev_word_start(&self) -> usize {
        let mut i = self.cursor;
        if i == 0 {
            return 0;
        }
        i -= 1;
        // Skip preceding whitespace.
        while i > 0 && char_class(self.buf[i]) == CharClass::Space {
            i -= 1;
        }
        // Skip back over the current word/punct chunk.
        let cls = char_class(self.buf[i]);
        while i > 0 && char_class(self.buf[i - 1]) == cls {
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
        // If we're at the last char of a word/punct chunk (or on
        // whitespace), advance past it before searching for the
        // next end. This matches vim's `e` semantic of "next end
        // forward" rather than "end of current."
        let cur = char_class(self.buf[i]);
        let at_end_of_chunk =
            cur == CharClass::Space || i + 1 >= n || char_class(self.buf[i + 1]) != cur;
        if at_end_of_chunk {
            i += 1;
            while i < n && char_class(self.buf[i]) == CharClass::Space {
                i += 1;
            }
        }
        if i >= n {
            return n.saturating_sub(1);
        }
        let cls = char_class(self.buf[i]);
        while i + 1 < n && char_class(self.buf[i + 1]) == cls {
            i += 1;
        }
        i
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum CharClass {
    /// Word-character: alphanumeric or `_`. Vim's default `iskeyword`.
    Word,
    /// Whitespace (Unicode `is_whitespace`).
    Space,
    /// Everything else: punctuation, symbols.
    Punct,
}

fn char_class(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
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
    fn insert_str_splices_at_cursor_in_insert_mode() {
        // The reported bug: in Insert mode with the cursor mid-line,
        // a paste should land at the cursor, not at the end.
        // Build "ls -l", move cursor back two with Left (to between
        // 's' and ' '), then paste "ar" -- should be "lsar -l".
        let mut e = LineEditor::new();
        for c in "ls -l".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        // Cursor is at 5 (end). Walk left until just after 's'.
        e.feed(k(KeyCode::Left));
        e.feed(k(KeyCode::Left));
        e.feed(k(KeyCode::Left));
        // cursor is now at 2 (just before ' ').
        let cursor_before = e.cursor;
        assert_eq!(cursor_before, 2);
        e.insert_str("ar");
        assert_eq!(e.text(), "lsar -l");
        assert_eq!(e.cursor, cursor_before + 2);
    }

    #[test]
    fn insert_str_at_end_appends() {
        let mut e = LineEditor::new();
        for c in "ls".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        // Cursor is at end after typing in Insert mode.
        e.insert_str(" -lah");
        assert_eq!(e.text(), "ls -lah");
        assert_eq!(e.cursor, 7);
    }

    #[test]
    fn insert_str_at_start_prepends() {
        let mut e = LineEditor::new();
        for c in "lah".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.cursor = 0;
        e.insert_str("ls -");
        assert_eq!(e.text(), "ls -lah");
        assert_eq!(e.cursor, 4);
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

    #[test]
    fn w_treats_punctuation_as_word_boundary() {
        // vim's default `iskeyword`: word chars are alnum + `_`,
        // everything else (`-`, `/`, `.`, etc.) is its own word
        // class. So `foo-bar` has three "words": `foo`, `-`, `bar`.
        let mut e = LineEditor::new();
        for c in "foo-bar".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc));
        e.feed(k(KeyCode::Char('0'))); // cursor at 0 ('f')
        e.feed(k(KeyCode::Char('w')));
        assert_eq!(e.cursor, 3, "w from 'foo' should land on '-'");
        e.feed(k(KeyCode::Char('w')));
        assert_eq!(e.cursor, 4, "w from '-' should land on 'b' of bar");
    }

    #[test]
    fn dw_stops_at_punctuation() {
        // The headline bug: `dw` on `foo-bar` from position 0
        // should delete only `foo`, not the whole `foo-bar`.
        let mut e = LineEditor::new();
        for c in "foo-bar".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc));
        e.feed(k(KeyCode::Char('0')));
        // dw chord: 'd' enters pending op, 'w' is the motion.
        e.feed(k(KeyCode::Char('d')));
        e.feed(k(KeyCode::Char('w')));
        assert_eq!(e.text(), "-bar");
    }

    #[test]
    fn cw_stops_at_punctuation_and_enters_insert() {
        let mut e = LineEditor::new();
        for c in "foo-bar".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        e.feed(k(KeyCode::Esc));
        e.feed(k(KeyCode::Char('0')));
        e.feed(k(KeyCode::Char('c')));
        e.feed(k(KeyCode::Char('w')));
        assert_eq!(e.text(), "-bar");
        assert_eq!(e.mode, Mode::Insert);
    }

    #[test]
    fn ctrl_w_back_delete_stops_at_punctuation() {
        // Ctrl+W in Insert mode (delete word back) should also
        // respect punctuation boundaries: in `foo-bar` with cursor
        // at end, ^W deletes `bar` only, leaving `foo-`.
        let mut e = LineEditor::new();
        for c in "foo-bar".chars() {
            e.feed(k(KeyCode::Char(c)));
        }
        // cursor at 7 (past end of "bar")
        let ctrl_w = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        e.feed(ctrl_w);
        assert_eq!(e.text(), "foo-");
    }
}

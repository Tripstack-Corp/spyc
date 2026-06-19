//! Pager picker overlays — jump-history popup, worktree picker, the vi
//! history editor, and the session picker. Each returns `Some` when it
//! consumes the key, `None` to fall through. Split from `pager_handler`
//! verbatim; `impl App` child reading App privates.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Effect};

impl App {
    /// Jump-history popup: j/k navigate, Enter chdirs, x deletes, q/Esc closes.
    pub(super) fn handle_pager_jump_history(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let view = active_pager_mut!(self)?;
        // Jump-history popup: j/k navigate, Enter chdirs, x deletes,
        // q/Esc closes. Per-popup j/k handling because the pager
        // dispatch doesn't have a generic picker-move arm; each
        // popup type wires its own (matches how the session picker
        // and history editor do it).
        if self.view.pending_jump_history.is_some() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    view.picker_move(1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.picker_move(-1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Enter => {
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let snapshot = self
                        .view
                        .pending_jump_history
                        .take()
                        .expect("guarded by is_some check above");
                    self.clear_pager();
                    self.view.needs_full_repaint = true;
                    if let Some(path_str) = snapshot.get(cursor) {
                        let path = crate::paths::expand(path_str);
                        match self.state.chdir(&path) {
                            Ok(()) => {
                                // Push to top of history so MRU stays
                                // accurate even if user reaches via
                                // popup instead of typing.
                                self.state.jump_history.push(path_str);
                            }
                            Err(e) => self.state.flash_error(format!("cd: {e}")),
                        }
                    }
                    return Some(Vec::new());
                }
                KeyCode::Char('x') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // `x` deletes the entry at the cursor. Matches
                    // the inventory view's `x` for "remove this
                    // item." The `!?` shell-history popup uses ^D
                    // because it has a vi line-editor where `x` is
                    // taken; the jump popup has no editor so `x` is
                    // unambiguously "delete entry."
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let snapshot = self.view.pending_jump_history.as_mut().unwrap();
                    if let Some(path_str) = snapshot.get(cursor).cloned() {
                        // Remove from real history (find by content,
                        // since snapshot indices are reverse-ordered).
                        let entries = self.state.jump_history.entries();
                        if let Some(real_idx) = entries.iter().position(|e| e == &path_str) {
                            self.state.jump_history.remove(real_idx);
                        }
                        snapshot.remove(cursor);
                        if snapshot.is_empty() {
                            self.view.pending_jump_history = None;
                            self.clear_pager();
                            self.view.needs_full_repaint = true;
                            self.state.flash_info("jump history empty");
                            return Some(Vec::new());
                        }
                        // Rebuild the pager line list from the snapshot.
                        let lines: Vec<ratatui::text::Line<'static>> = snapshot
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                ratatui::text::Line::from(format!("  {:>3}  {}", i + 1, p))
                            })
                            .collect();
                        view.lines = lines;
                        if cursor >= view.lines.len() {
                            view.picker_cursor = Some(view.lines.len() - 1);
                        }
                        return Some(Vec::new());
                    }
                }
                _ => {}
            }
        }

        None
    }

    /// Worktree picker: 1-9 selects a worktree, chdirs, re-anchors PROJECT_HOME.
    pub(super) fn handle_pager_worktree_pick(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        // Worktree picker: 1-9 selects a worktree, chdirs, and
        // re-anchors PROJECT_HOME on the worktree root.
        if let Some(ref worktrees) = self.state.pending_worktrees
            && let KeyCode::Char(c @ '1'..='9') = key.code
        {
            let idx = (c as u8 - b'1') as usize;
            if let Some(path) = worktrees.get(idx).cloned() {
                self.clear_pager();
                self.state.pending_worktrees = None;
                self.view.needs_full_repaint = true;
                if let Err(e) = self.state.chdir(&path) {
                    self.state.flash_error(format!("chdir: {e}"));
                    return Some(Vec::new());
                }
                // Worktrees are independent project roots — point
                // PROJECT_HOME at the worktree so harpoon, MCP
                // context (and therefore search_paths /
                // search_content / claude's grep), status bar,
                // and `gh` (jump-home) all anchor on the worktree
                // instead of the parent repo. The original
                // behavior left PROJECT_HOME pointing at the main
                // repo, so a user driving an agent inside a
                // worktree got search results from the wrong
                // tree (reported by a daily-driver after a
                // confusing afternoon).
                //
                // `listing.dir` is the canonical worktree path
                // after `state.chdir` (which canonicalizes
                // internally). `reconcile_harpoon` saves the
                // outgoing list and loads a fresh one keyed on
                // the new project root.
                let new_home = self.state.left.listing.dir.clone();
                self.state.project_home = Some(new_home.clone());
                self.reconcile_harpoon();
                self.state.flash_info(format!(
                    "worktree: {} (PROJECT_HOME updated)",
                    crate::paths::display_tilde(&new_home),
                ));
                return Some(Vec::new());
            }
        }

        None
    }

    /// History editor: vi-edit the highlighted line, Enter runs, Ctrl+D deletes.
    pub(super) fn handle_pager_history_editor(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let view = active_pager_mut!(self)?;
        // History editor: vi-edit highlighted line, Enter runs, d/x deletes.
        if let Some(ref mut editor) = self.view.pending_history_pick {
            use crate::ui::line_edit::EditResult;
            let editor_is_normal = editor.mode == crate::ui::line_edit::Mode::Normal;

            // Ctrl+D deletes the highlighted entry from history (any mode).
            if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
                let cursor = view.picker_cursor.unwrap_or(0);
                let entries = self.state.history.entries();
                let hist_idx = entries.len().saturating_sub(1 + cursor);
                if hist_idx < entries.len() {
                    self.state.history.remove(hist_idx);
                    if self.state.history.entries().is_empty() {
                        self.clear_pager();
                        self.view.pending_history_pick = None;
                        self.view.needs_full_repaint = true;
                        self.state.flash_info("history is empty");
                        return Some(Vec::new());
                    }
                    let old_cursor = cursor;
                    self.show_history_popup();
                    if let Some(ref mut v) = self.view.pager {
                        let max = (v.line_count() as usize).saturating_sub(1);
                        v.picker_cursor = Some(old_cursor.min(max));
                        let new_cur = v.picker_cursor.unwrap_or(0);
                        let entries = self.state.history.entries();
                        let hist_idx = entries.len().saturating_sub(1 + new_cur);
                        if let Some(ref mut ed) = self.view.pending_history_pick {
                            if let Some(cmd) = entries.get(hist_idx) {
                                ed.set_content_keep_mode(cmd);
                            }
                            v.picker_edit_cursor = Some((Self::HIST_PREFIX_W + ed.cursor, ed.mode));
                            let text = format!("  {:>3}  {}", new_cur + 1, ed.text());
                            v.lines[new_cur] = ratatui::text::Line::from(text);
                        }
                    }
                }
                return Some(Vec::new());
            }

            // Inline sync: update editor from the current picker line.
            // Uses `view` and `editor` already borrowed in this scope.
            macro_rules! sync_editor {
                ($v:expr, $ed:expr, $hist:expr) => {{
                    let nc = $v.picker_cursor.unwrap_or(0);
                    let entries = $hist.entries();
                    let hi = entries.len().saturating_sub(1 + nc);
                    if let Some(cmd) = entries.get(hi) {
                        $ed.set_content_keep_mode(cmd);
                    }
                    let text = format!("  {:>3}  {}", nc + 1, $ed.text());
                    $v.lines[nc] = ratatui::text::Line::from(text);
                    $v.picker_edit_cursor = Some((Self::HIST_PREFIX_W + $ed.cursor, $ed.mode));
                }};
            }

            // In Normal mode, j/k/G/gg/n/N navigate, / searches, : jumps.
            if editor_is_normal {
                let handled = match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.view.history_pending_g = false;
                        view.picker_move(1, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.view.history_pending_g = false;
                        view.picker_move(-1, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('G') => {
                        self.view.history_pending_g = false;
                        let last = view.lines.len().saturating_sub(1);
                        let delta = last as isize - view.picker_cursor.unwrap_or(0) as isize;
                        view.picker_move(delta, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('g') => {
                        if self.view.history_pending_g {
                            self.view.history_pending_g = false;
                            let delta = -(view.picker_cursor.unwrap_or(0) as isize);
                            view.picker_move(delta, viewport);
                            sync_editor!(view, editor, self.state.history);
                        } else {
                            self.view.history_pending_g = true;
                        }
                        true
                    }
                    KeyCode::Char('/') => {
                        self.view.history_pending_g = false;
                        view.begin_search();
                        true
                    }
                    KeyCode::Char('n') => {
                        self.view.history_pending_g = false;
                        view.search_next(viewport);
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            sync_editor!(view, editor, self.state.history);
                        }
                        true
                    }
                    KeyCode::Char('N') => {
                        self.view.history_pending_g = false;
                        view.search_prev(viewport);
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            sync_editor!(view, editor, self.state.history);
                        }
                        true
                    }
                    KeyCode::Char(':') => {
                        self.view.history_pending_g = false;
                        view.jump_buf = Some(String::new());
                        true
                    }
                    // Disable pager keys that don't make sense here.
                    KeyCode::Char('l' | 'v') => true,
                    _ => {
                        self.view.history_pending_g = false;
                        false
                    }
                };
                if handled {
                    return Some(Vec::new());
                }
            }

            // Feed all other keys to the line editor.
            let result = editor.feed(key);
            // Sync the display line with the editor buffer.
            let pc = view.picker_cursor.unwrap_or(0);
            let text = format!("  {:>3}  {}", pc + 1, editor.text());
            view.lines[pc] = ratatui::text::Line::from(text);
            view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));

            match result {
                EditResult::Submit => {
                    let cmd = editor.text();
                    self.clear_pager();
                    self.view.pending_history_pick = None;
                    self.view.needs_full_repaint = true;
                    if cmd.trim().is_empty() {
                        return Some(Vec::new());
                    }
                    // Execute the (possibly edited) command directly.
                    self.state.last_captured_cmd = Some(cmd.clone());
                    self.state.history.push(cmd.trim());
                    match crate::shell::expand_percent(&cmd, &self.state.selection_paths()) {
                        Ok(expanded) => self.start_capture(&expanded, &cmd, &cmd),
                        Err(e) => self.state.flash_error(e.to_string()),
                    }
                }
                EditResult::Cancel => {
                    // Esc in Insert → Normal (handled by editor, returns Continue).
                    // Cancel only fires from Normal-mode Esc or Ctrl+C → close popup.
                    self.clear_pager();
                    self.view.pending_history_pick = None;
                    self.view.needs_full_repaint = true;
                }
                EditResult::HistoryPrev | EditResult::HistoryNext => {
                    // Up/Down in Insert mode → move between lines.
                    // HistoryPrev = Up key → move toward top of list (newer).
                    let delta: isize = if result == EditResult::HistoryPrev {
                        -1
                    } else {
                        1
                    };
                    view.picker_move(delta, viewport);
                    let new_cursor = view.picker_cursor.unwrap_or(0);
                    let entries = self.state.history.entries();
                    let hist_idx = entries.len().saturating_sub(1 + new_cursor);
                    if let Some(cmd) = entries.get(hist_idx) {
                        editor.set_content(cmd);
                    }
                    let text = format!("  {:>3}  {}", new_cursor + 1, editor.text());
                    view.lines[new_cursor] = ratatui::text::Line::from(text);
                    view.picker_edit_cursor =
                        Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
                }
                EditResult::TabComplete | EditResult::Continue => {}
            }
            return Some(Vec::new());
        }

        None
    }

    /// Session picker: j/k navigate, Enter/1-9 select, n new.
    pub(super) fn handle_pager_session_pick(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let view = active_pager_mut!(self)?;
        // Session picker: j/k navigate, Enter/1-9 select, n new.
        if self.state.pending_sessions.is_some() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    view.picker_move(1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.picker_move(-1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char(c @ '1'..='9') => {
                    // Direct selection — index into sessions (offset by 2 header lines).
                    let sessions = self.state.pending_sessions.take().unwrap();
                    let idx = (c as u8 - b'1') as usize;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        self.restore_session(&session);
                        return Some(Vec::new());
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Enter => {
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let sessions = self
                        .state
                        .pending_sessions
                        .take()
                        .expect("guarded by is_some check above");
                    if cursor < 2 {
                        // "New session" header.
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        self.state.flash_info("new session");
                        return Some(Vec::new());
                    }
                    let idx = cursor - 2;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        self.restore_session(&session);
                        return Some(Vec::new());
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Char('n' | 'N') => {
                    self.clear_pager();
                    self.state.pending_sessions = None;
                    self.view.needs_full_repaint = true;
                    self.state.flash_info("new session");
                    return Some(Vec::new());
                }
                _ => {}
            }
        }

        None
    }
}

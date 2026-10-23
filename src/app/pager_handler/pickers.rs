//! Pager picker overlays — jump-history popup, worktree picker, the vi
//! history editor, and the session picker. Each returns `Some` when it
//! consumes the key, `None` to fall through. Split from `pager_handler`
//! verbatim; `impl App` child reading App privates.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Effect};
use crate::ui::line_edit::{EditResult, Mode as LineMode};
use crate::ui::pager::PickerKind;

/// What a key does in the history editor before the line editor sees it.
/// Ctrl+D deletes in either mode; the rest of the vi motions only apply in
/// Normal mode, since while typing they are text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryKey {
    Delete,
    Move(isize),
    Bottom,
    Top,
    /// The first `g` of `gg`.
    ArmG,
    Search,
    SearchNext,
    SearchPrev,
    JumpPrompt,
    /// The line editor's key.
    Edit,
}

const fn history_key(key: KeyEvent, normal: bool, g_armed: bool) -> HistoryKey {
    if matches!(key.code, KeyCode::Char('d')) && key.modifiers.contains(KeyModifiers::CONTROL) {
        return HistoryKey::Delete;
    }
    if !normal {
        return HistoryKey::Edit;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => HistoryKey::Move(1),
        KeyCode::Char('k') | KeyCode::Up => HistoryKey::Move(-1),
        KeyCode::Char('G') => HistoryKey::Bottom,
        KeyCode::Char('g') if g_armed => HistoryKey::Top,
        KeyCode::Char('g') => HistoryKey::ArmG,
        KeyCode::Char('/') => HistoryKey::Search,
        KeyCode::Char('n') => HistoryKey::SearchNext,
        KeyCode::Char('N') => HistoryKey::SearchPrev,
        KeyCode::Char(':') => HistoryKey::JumpPrompt,
        _ => HistoryKey::Edit,
    }
}

impl App {
    /// Jump-history popup: j/k navigate, Enter chdirs, x deletes, q/Esc closes.
    pub(super) fn handle_pager_jump_history(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let in_view = self.picker_in_view(PickerKind::JumpHistory);
        let view = active_pager_mut!(self)?;
        // Jump-history popup: j/k navigate, Enter chdirs, x deletes,
        // q/Esc closes. Per-popup j/k handling because the pager
        // dispatch doesn't have a generic picker-move arm; each
        // popup type wires its own (matches how the session picker
        // and history editor do it).
        if in_view && self.view.pending_jump_history.is_some() {
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
                            Err(e) => self.state.flash_error(format!("cd: {e:#}")),
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
                    let snapshot = self
                        .view
                        .pending_jump_history
                        .as_mut()
                        .expect("guarded by is_some check above");
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

    /// The numbered pickers' keys (the worktree and project pickers): `j`/`k`
    /// (or arrows) move the highlighted row, and `Enter` picks it, `1`-`9` the
    /// row numbered, through `pick`. Other keys fall through, so `q`/`Esc`/
    /// scroll still work via the motion handler.
    fn numbered_picker_key(
        &mut self,
        key: KeyEvent,
        viewport: u16,
        pick: fn(&mut Self, usize),
    ) -> Option<Vec<Effect>> {
        let row = match key.code {
            KeyCode::Char('j' | 'k') | KeyCode::Down | KeyCode::Up => {
                let down = matches!(key.code, KeyCode::Char('j') | KeyCode::Down);
                if let Some(view) = active_pager_mut!(self) {
                    view.picker_move(if down { 1 } else { -1 }, viewport);
                }
                return Some(Vec::new());
            }
            KeyCode::Enter => active_pager_mut!(self)
                .and_then(|v| v.picker_cursor)
                .unwrap_or(0),
            KeyCode::Char(c @ '1'..='9') => (c as u8 - b'1') as usize,
            _ => return None,
        };
        pick(self, row);
        Some(Vec::new())
    }

    /// The project switcher (`Space j`): its keys are [`Self::numbered_picker_key`]'s,
    /// picking with `switch_to_pending_project`.
    pub(super) fn handle_pager_project_pick(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        self.state.pending_projects.as_ref()?;
        self.picker_in_view(PickerKind::Projects).then_some(())?;
        self.numbered_picker_key(key, viewport, Self::switch_to_pending_project)
    }

    /// Whether the pager in view is the picker `kind`: a picker's keys act
    /// only on its own pager, so one opened over it (the help, an older
    /// buffer, a worker's late pager) keeps its own keys.
    pub(crate) fn picker_in_view(&self, kind: PickerKind) -> bool {
        self.active_pager_ref()
            .is_some_and(|view| view.picker == Some(kind))
    }

    /// Forget every picker's rows, as closing the pager does, and every
    /// picker's pager kept in the buffer history: its rows are gone, and a
    /// picker opened since has rows of its own, which `]b` back to an old one
    /// would hand its keys.
    pub(crate) fn clear_pending_pickers(&mut self) {
        self.view.project_mut().pager_history.drop_pickers();
        self.state.pending_worktrees = None;
        self.state.pending_projects = None;
        self.state.pending_sessions = None;
        self.view.pending_history_pick = None;
        self.view.pending_jump_history = None;
    }

    /// Worktree picker: `j`/`k` (or arrows) move the highlighted row, `Enter`
    /// switches the **focused** column to it, and `1`-`9` quick-switch by number
    /// (focus `a` → switch `a`; focus `b` → switch `b`). `/` search syncs the
    /// cursor to its match (see `handle_pager_search_typing`). Other keys fall
    /// through (so `q`/`Esc`/scroll still work via the motion handler).
    pub(super) fn handle_pager_worktree_pick(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        self.state.pending_worktrees.as_ref()?;
        self.picker_in_view(PickerKind::Worktrees).then_some(())?;
        self.numbered_picker_key(key, viewport, Self::switch_to_pending_worktree)
    }

    /// Switch the focused column to `pending_worktrees[idx]` and close the
    /// picker. No-op (picker stays open) when `idx` is out of range.
    fn switch_to_pending_worktree(&mut self, idx: usize) {
        let Some(path) = self
            .state
            .pending_worktrees
            .as_ref()
            .and_then(|w| w.get(idx))
            .cloned()
        else {
            return;
        };
        self.clear_pager();
        self.state.pending_worktrees = None;
        self.view.needs_full_repaint = true;
        // Switch the FOCUSED column (`chdir` targets `cur()`). To put a worktree
        // in `b`, focus it (or open it with `^s n`) first. The per-column tools
        // follow this column's `current_repo_root`, so PROJECT_HOME is NOT
        // re-anchored — it stays the overall anchor (`g h`). (`g w` jumps to
        // this column's worktree root.)
        if let Err(e) = self.state.chdir(&path) {
            self.state.flash_error(format!("chdir: {e:#}"));
            return;
        }
        // Re-key the focused column's harpoon to the new worktree root.
        self.reconcile_harpoon();
        self.state
            .flash_info(format!("worktree: {}", crate::paths::display_tilde(&path)));
    }

    /// History editor: vi-edit the highlighted line, Enter runs, Ctrl+D deletes.
    pub(super) fn handle_pager_history_editor(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        active_pager_mut!(self)?;
        self.picker_in_view(PickerKind::History).then_some(())?;
        let normal = self.view.pending_history_pick.as_ref()?.mode == LineMode::Normal;
        let g_armed = self.view.history_pending_g;
        let action = history_key(key, normal, g_armed);
        self.view.history_pending_g = match action {
            HistoryKey::ArmG => true,
            HistoryKey::Delete => g_armed,
            HistoryKey::Edit if !normal => g_armed,
            _ => false,
        };
        match action {
            HistoryKey::Delete => self.delete_selected_history(),
            HistoryKey::Edit => self.feed_history_editor(key, viewport),
            _ => self.navigate_history(action, viewport),
        }
        Some(Vec::new())
    }

    /// Move the history editor's selection (or start a search or `:N` jump),
    /// loading whichever command it lands on.
    fn navigate_history(&mut self, action: HistoryKey, viewport: u16) {
        let (Some(view), Some(editor)) = (
            active_pager_mut!(self),
            self.view.pending_history_pick.as_mut(),
        ) else {
            return;
        };
        let moved = match action {
            HistoryKey::Move(delta) => {
                view.picker_move(delta, viewport);
                true
            }
            HistoryKey::Bottom => {
                let last = view.lines.len().saturating_sub(1);
                let delta = last as isize - view.picker_cursor.unwrap_or(0) as isize;
                view.picker_move(delta, viewport);
                true
            }
            HistoryKey::Top => {
                let delta = -(view.picker_cursor.unwrap_or(0) as isize);
                view.picker_move(delta, viewport);
                true
            }
            HistoryKey::SearchNext | HistoryKey::SearchPrev => {
                if action == HistoryKey::SearchNext {
                    view.search_next(viewport);
                } else {
                    view.search_prev(viewport);
                }
                view.current_match_line()
                    .map(|line| view.picker_cursor = Some(line))
                    .is_some()
            }
            HistoryKey::Search => {
                view.begin_search();
                false
            }
            HistoryKey::JumpPrompt => {
                view.jump_buf = Some(String::new());
                false
            }
            HistoryKey::ArmG | HistoryKey::Delete | HistoryKey::Edit => false,
        };
        if moved {
            Self::show_history_selection(view, editor, &self.state.history, true);
        }
    }

    /// Ctrl+D: delete the selected command from history. The next one takes
    /// its row; deleting the last closes the popup.
    fn delete_selected_history(&mut self) {
        let cursor = active_pager_mut!(self)
            .and_then(|v| v.picker_cursor)
            .unwrap_or(0);
        let entries = self.state.history.entries();
        let hist_idx = entries.len().saturating_sub(1 + cursor);
        if hist_idx >= entries.len() {
            return;
        }
        self.state.history.remove(hist_idx);
        if self.state.history.entries().is_empty() {
            self.clear_pager();
            self.view.pending_history_pick = None;
            self.view.needs_full_repaint = true;
            self.state.flash_info("history is empty");
            return;
        }
        self.show_history_popup();
        if let (Some(view), Some(editor)) = self.view.top_pager_and_history_pick_mut() {
            let max = view.line_count().saturating_sub(1);
            view.picker_cursor = Some(cursor.min(max));
            Self::show_history_selection(view, editor, &self.state.history, true);
        }
    }

    /// Any other key edits the selected command; Enter runs it, Esc in Normal
    /// mode closes, and Up/Down while typing move between commands.
    fn feed_history_editor(&mut self, key: KeyEvent, viewport: u16) {
        let (Some(view), Some(editor)) = (
            active_pager_mut!(self),
            self.view.pending_history_pick.as_mut(),
        ) else {
            return;
        };
        let result = editor.feed(key);
        Self::show_edited_history_row(view, editor);
        match result {
            EditResult::Submit => {
                let cmd = editor.text();
                self.run_history_command(cmd);
            }
            EditResult::Cancel => {
                // Esc in Insert → Normal (handled by editor, returns Continue).
                // Cancel only fires from Normal-mode Esc or Ctrl+C → close popup.
                self.clear_pager();
                self.view.pending_history_pick = None;
                self.view.needs_full_repaint = true;
            }
            EditResult::HistoryPrev | EditResult::HistoryNext => {
                // HistoryPrev = Up key → move toward top of list (newer).
                let delta = if result == EditResult::HistoryPrev {
                    -1
                } else {
                    1
                };
                view.picker_move(delta, viewport);
                Self::show_history_selection(view, editor, &self.state.history, false);
            }
            EditResult::TabComplete | EditResult::Continue => {}
        }
    }

    /// Close the popup and run `cmd`, the (possibly edited) command. An emptied
    /// line runs nothing.
    fn run_history_command(&mut self, cmd: String) {
        self.clear_pager();
        self.view.pending_history_pick = None;
        self.view.needs_full_repaint = true;
        if cmd.trim().is_empty() {
            return;
        }
        self.state.last_captured_cmd = Some(cmd.clone());
        self.state.history.push(cmd.trim());
        match self.state.expand_selection_percent(&cmd) {
            Ok(expanded) => self.start_capture(&expanded, &cmd, &cmd),
            Err(e) => self.state.flash_error(e.to_string()),
        }
    }

    /// Session picker: j/k navigate, Enter/1-9 select, n new.
    pub(super) fn handle_pager_session_pick(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let in_view = self.picker_in_view(PickerKind::Sessions);
        let view = active_pager_mut!(self)?;
        // Session picker: j/k navigate, Enter/1-9 select, n new.
        if in_view && self.state.pending_sessions.is_some() {
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
                    // Direct selection by the `[N]` shown on the row.
                    let sessions = self
                        .state
                        .pending_sessions
                        .take()
                        .expect("guarded by is_some check above");
                    let idx = (c as u8 - b'1') as usize;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        let fx = self.restore_session(&session);
                        return Some(fx);
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
                    if cursor < Self::SESSION_PICKER_HEADER_ROWS {
                        // "New session" header.
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        self.state.flash_info("new session");
                        return Some(Vec::new());
                    }
                    let idx = cursor - Self::SESSION_PICKER_HEADER_ROWS;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        let fx = self.restore_session(&session);
                        return Some(fx);
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

#[cfg(test)]
mod tests {
    use super::{HistoryKey, history_key};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// While typing, every key but Ctrl+D is text, so a command can contain
    /// `j`, `g`, `n`, `/` or `:`. In Normal mode those keys navigate.
    #[test]
    fn motions_apply_only_in_normal_mode() {
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        for normal in [true, false] {
            assert_eq!(history_key(ctrl_d, normal, false), HistoryKey::Delete);
        }
        assert_eq!(
            history_key(key('d'), true, false),
            HistoryKey::Edit,
            "plain `d` edits"
        );
        for c in ['j', 'k', 'G', 'g', '/', 'n', 'N', ':'] {
            assert_eq!(
                history_key(key(c), false, false),
                HistoryKey::Edit,
                "typing {c:?}"
            );
            assert_ne!(
                history_key(key(c), true, false),
                HistoryKey::Edit,
                "{c:?} in Normal mode"
            );
        }
        for c in ['h', 'l', 'v'] {
            assert_eq!(
                history_key(key(c), true, false),
                HistoryKey::Edit,
                "{c:?} is vi's"
            );
        }
        assert_eq!(history_key(key('g'), true, false), HistoryKey::ArmG);
        assert_eq!(history_key(key('g'), true, true), HistoryKey::Top);
    }
}

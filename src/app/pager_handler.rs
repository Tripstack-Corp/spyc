//! Pager key dispatch: `handle_pager_key`, the vi-style key router for
//! the in-app pager overlay (search results, git show/diff/blame, help,
//! task viewer, find/grep pickers, captured `!` output…).
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2). Like `render`,
//! this is an `impl App` method in a child module, so it reads App's
//! private state directly via the descendant-module rule — no field is
//! made `pub`. It's `pub` because the key-routing path in `app` calls
//! it. The many `self.*` helpers it delegates to (clear_pager,
//! restore_session, start_capture, task pause/resume…) stay in `app`.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::fs;
use crate::pane::Pane;
use crate::shell;
use crate::ui::pager;

use super::state::Focus;
use super::{App, Effect, EntryKind, PagerReturn, PagerView, TaskStatus, sh_c, state};

impl App {
    /// Route a key to the pager overlay. Also uses vi-like motion so the
    /// pager feels native to the rest of the UI.
    pub fn handle_pager_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(view) = &mut self.view.pager else {
            return Vec::new();
        };
        // Clear any one-shot flash message from the previous keypress.
        view.flash = None;

        // ^C inside the pager is contextual:
        //   - Task viewer + task running → SIGINT to the process group
        //     (mirrors a real ^C in the captured task's tty), flash
        //     the result *inside* the pager footer.
        //   - Task viewer + task finished → flash "process already
        //     stopped" inside the pager. Reported: extra ^C presses
        //     after a task exits (130 from the original ^C) leaked
        //     to the spyc-list status bar instead of telling the
        //     user the task was already gone.
        //   - Other pager views (file viewer, help, etc.) → flash
        //     "press Esc or q to close" inside the pager, since
        //     ^C-as-quit is muscle memory from `less` and the user
        //     expects feedback in the active screen.
        if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(id) = view.task_id {
                match self.interrupt_task(Some(id)) {
                    Ok(msg) | Err(msg) => {
                        if let Some(v) = self.view.pager.as_mut() {
                            v.flash = Some(msg);
                        }
                    }
                }
            } else if let Some(v) = self.view.pager.as_mut() {
                v.flash = Some("press Esc or q to close pager".into());
            }
            return Vec::new();
        }

        // Compute the pager's actual viewport from the terminal size.
        // Compute the pager's actual content viewport. Prefer the
        // renderer's cached `last_viewport_h` — it's the real
        // body-area row count from the most recent frame and is
        // correct for every mount (Overlay / TopPane / LowerPane).
        // Fall back to the centered-overlay heuristic only on the
        // very first key event (renderer hasn't run yet).
        //
        // Bug this fixes: `Mount::LowerPane` (`^a-v`) renders into
        // the lower-pane slot (~40 % of terminal height), but the
        // old heuristic always used `term_h * 92 / 100 - 2` —
        // viewport-too-tall, so `scroll_by`'s clamp via
        // `scroll_max(viewport)` returned a value smaller than the
        // real maximum. After the first `k` keypress, `scroll`
        // capped well above the snapshot's last lines (the HUD)
        // and the pager looked like it had truncated the bottom.
        let viewport = {
            let cached = view.last_viewport_h.get();
            if cached >= 2 {
                cached
            } else {
                let (_, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
                let pager_h = if view.full_width {
                    term_h
                } else {
                    (u32::from(term_h) * 92 / 100) as u16
                };
                pager_h.saturating_sub(2).max(2)
            }
        };

        // While typing a search query, most keys feed the buffer.
        if view.is_typing_search() {
            match key.code {
                KeyCode::Esc => view.cancel_search(),
                KeyCode::Enter => {
                    let committed = view.commit_search(viewport);
                    if !committed {
                        // Flash inside the pager itself, not on the
                        // file-list status bar -- the user is looking
                        // at the pager, the message belongs there.
                        view.flash = Some("no matches".into());
                    } else if let Some(ref mut editor) = self.view.pending_history_pick {
                        // Sync picker cursor to the first match.
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            let nc = line;
                            let entries = self.state.history.entries();
                            let hi = entries.len().saturating_sub(1 + nc);
                            if let Some(cmd) = entries.get(hi) {
                                editor.set_content_keep_mode(cmd);
                            }
                            let text = format!("  {:>3}  {}", nc + 1, editor.text());
                            view.lines[nc] = ratatui::text::Line::from(text);
                            view.picker_edit_cursor =
                                Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
                        }
                    }
                }
                KeyCode::Backspace => view.search_backspace(),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.search_push_char(c);
                }
                _ => {}
            }
            return Vec::new();
        }

        // Inline `:N` jump — accumulate digits, Enter commits, Esc cancels.
        if let Some(ref mut buf) = self.view.pager_jump_buf {
            match key.code {
                KeyCode::Char(c @ '0'..='9') => {
                    buf.push(c);
                    view.jump_buf = Some(buf.clone());
                }
                KeyCode::Backspace => {
                    if buf.pop().is_none() {
                        self.view.pager_jump_buf = None;
                        view.jump_buf = None;
                    } else {
                        view.jump_buf = Some(buf.clone());
                    }
                }
                KeyCode::Enter => {
                    if let Ok(n) = buf.parse::<usize>()
                        && n > 0
                    {
                        let target = n.saturating_sub(1);
                        if self.view.pending_history_pick.is_some() {
                            // History editor: jump to entry N.
                            let max = view.lines.len().saturating_sub(1);
                            let clamped = target.min(max);
                            view.picker_cursor = Some(clamped);
                            view.scroll =
                                u16::try_from(clamped.saturating_sub(2)).unwrap_or(u16::MAX);
                        } else {
                            // Regular pager: jump to line N.
                            view.scroll = u16::try_from(target).unwrap_or(u16::MAX);
                        }
                    }
                    view.jump_buf = None;
                    self.view.pager_jump_buf = None;
                    if self.view.pending_history_pick.is_some() {
                        self.sync_history_editor_to_cursor();
                    }
                }
                _ => {
                    // Esc or non-digit cancels.
                    self.view.pager_jump_buf = None;
                    view.jump_buf = None;
                }
            }
            return Vec::new();
        }

        // [b / ]b — pager buffer history navigation (two-key sequence).
        // [t / ]t — task viewer cycle (peek through backgrounded tasks).
        if let Some(bracket) = self.view.pager_pending_bracket.take() {
            if key.code == KeyCode::Char('b') {
                match bracket {
                    '[' => {
                        if let Some(current) = self.view.pager.take() {
                            match self.view.pager_history.go_back(current) {
                                Ok(prev) => {
                                    self.view.pager = Some(prev);
                                    self.view.needs_full_repaint = true;
                                    let back = self.view.pager_history.back_len();
                                    let fwd = self.view.pager_history.forward_len();
                                    self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                                }
                                Err(current) => {
                                    self.view.pager = Some(current);
                                    self.state.flash_info("no older buffers");
                                }
                            }
                        }
                    }
                    ']' => {
                        if let Some(current) = self.view.pager.take() {
                            match self.view.pager_history.go_forward(current) {
                                Ok(next) => {
                                    self.view.pager = Some(next);
                                    self.view.needs_full_repaint = true;
                                    let back = self.view.pager_history.back_len();
                                    let fwd = self.view.pager_history.forward_len();
                                    self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                                }
                                Err(current) => {
                                    self.view.pager = Some(current);
                                    self.state.flash_info("no newer buffers");
                                }
                            }
                        }
                    }
                    _ => {}
                }
                return Vec::new();
            }
            if key.code == KeyCode::Char('t') {
                let direction = if bracket == '[' { -1 } else { 1 };
                self.cycle_task_viewer(direction);
                return Vec::new();
            }
            // Unrecognized chord follow-up -- swallow it.
            return Vec::new();
        }

        // Jump-history popup: j/k navigate, Enter chdirs, x deletes,
        // q/Esc closes. Per-popup j/k handling because the pager
        // dispatch doesn't have a generic picker-move arm; each
        // popup type wires its own (matches how the session picker
        // and history editor do it).
        if self.view.pending_jump_history.is_some() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    view.picker_move(1, viewport);
                    return Vec::new();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.picker_move(-1, viewport);
                    return Vec::new();
                }
                KeyCode::Enter => {
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let snapshot = self.view.pending_jump_history.take().unwrap();
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
                    return Vec::new();
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
                            return Vec::new();
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
                        return Vec::new();
                    }
                }
                _ => {}
            }
        }

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
                    return Vec::new();
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
                let new_home = self.state.listing.dir.clone();
                self.state.project_home = Some(new_home.clone());
                self.reconcile_harpoon();
                self.state.flash_info(format!(
                    "worktree: {} (PROJECT_HOME updated)",
                    crate::paths::display_tilde(&new_home),
                ));
                return Vec::new();
            }
        }

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
                        return Vec::new();
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
                return Vec::new();
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
                        self.view.pager_jump_buf = Some(String::new());
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
                    return Vec::new();
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
                        return Vec::new();
                    }
                    // Execute the (possibly edited) command directly.
                    self.state.last_captured_cmd = Some(cmd.clone());
                    self.state.history.push(cmd.trim());
                    let expanded =
                        crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    self.start_capture(&expanded, &cmd, &cmd);
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
            return Vec::new();
        }

        // Session picker: j/k navigate, Enter/1-9 select, n new.
        if self.state.pending_sessions.is_some() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    view.picker_move(1, viewport);
                    return Vec::new();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.picker_move(-1, viewport);
                    return Vec::new();
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
                        return Vec::new();
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Enter => {
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let sessions = self.state.pending_sessions.take().unwrap();
                    if cursor < 2 {
                        // "New session" header.
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        self.state.flash_info("new session");
                        return Vec::new();
                    }
                    let idx = cursor - 2;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.clear_pager();
                        self.view.needs_full_repaint = true;
                        self.restore_session(&session);
                        return Vec::new();
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Char('n' | 'N') => {
                    self.clear_pager();
                    self.state.pending_sessions = None;
                    self.view.needs_full_repaint = true;
                    self.state.flash_info("new session");
                    return Vec::new();
                }
                _ => {}
            }
        }

        // Placement mode: pre-visual-block cursor positioning.
        // First `^v` enters this state; vi motions move the cursor
        // without defining a selection yet. Second `^v` commits to
        // visual block at the cursor; `V` commits to visual line at
        // the cursor's row; `Esc` cancels. We swallow keys that are
        // motion-related so they don't fall through to scroll.
        if view.is_placement() {
            match key.code {
                KeyCode::Esc => {
                    view.cancel_placement();
                    view.flash = Some("placement: cancelled".into());
                    return Vec::new();
                }
                KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.commit_placement_to_visual_block();
                    view.flash = Some("visual block".into());
                    return Vec::new();
                }
                KeyCode::Char('V') => {
                    view.commit_placement_to_visual_line();
                    view.flash = Some("visual line".into());
                    return Vec::new();
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    view.placement_move(0, -1, viewport);
                    return Vec::new();
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    view.placement_move(0, 1, viewport);
                    return Vec::new();
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    view.placement_move(1, 0, viewport);
                    return Vec::new();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.placement_move(-1, 0, viewport);
                    return Vec::new();
                }
                KeyCode::Char('0') | KeyCode::Home => {
                    view.placement_line_start();
                    return Vec::new();
                }
                KeyCode::Char('$') | KeyCode::End => {
                    view.placement_line_end();
                    return Vec::new();
                }
                KeyCode::Char('w') => {
                    view.placement_word_forward();
                    return Vec::new();
                }
                KeyCode::Char('b') => {
                    view.placement_word_backward();
                    return Vec::new();
                }
                KeyCode::Char('g') => {
                    // Single `g` jumps to top (no `gg` two-key required —
                    // simpler than reusing the pager's pending-g state
                    // machine, and placement is short-lived anyway).
                    view.placement_jump_to(0, viewport);
                    return Vec::new();
                }
                KeyCode::Char('G') => {
                    let last = view.lines.len().saturating_sub(1);
                    view.placement_jump_to(last, viewport);
                    return Vec::new();
                }
                _ => {
                    // Anything else: swallow. Keeps the user from
                    // accidentally scrolling or yanking while in
                    // placement.
                    return Vec::new();
                }
            }
        }

        // Visual mode: Line (`V`) or Block (`^v`). Intercept first
        // so motion keys (j/k/G/^d/^u/^f/^b/PageDn/PageUp/Space) move
        // the selection cursor instead of the scroll position, and
        // `y` yanks the selection. `Esc` / `V` (Line) / `^v` (Block)
        // cancel without yanking. In Block mode, `h`/`l` extend the
        // column cursor; `^v` while a Line selection is active
        // upgrades to Block (preserving anchor / cursor lines, vim
        // parity).
        if view.is_visual() {
            let half_page = i32::from(viewport) / 2;
            let page = i32::from(viewport);
            let in_block = view
                .visual
                .is_some_and(|v| v.kind == crate::ui::pager::VisualKind::Block);
            // Toggle / cancel keys: V cancels Line, ^v cancels Block,
            // and ^v from Line upgrades. Esc cancels either.
            if matches!(key.code, KeyCode::Esc) {
                view.cancel_visual();
                return Vec::new();
            }
            if matches!(key.code, KeyCode::Char('V'))
                && !key.modifiers.contains(KeyModifiers::CONTROL)
            {
                if in_block {
                    // V from block: drop down to Line (vim parity).
                    if let Some(sel) = view.visual.as_mut() {
                        sel.kind = crate::ui::pager::VisualKind::Line;
                    }
                } else {
                    view.cancel_visual();
                }
                return Vec::new();
            }
            if matches!(key.code, KeyCode::Char('v'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                if in_block {
                    view.cancel_visual();
                } else {
                    view.enter_visual_block();
                }
                return Vec::new();
            }
            match key.code {
                KeyCode::Char('y' | 'Y') => {
                    let include_title = self.state.config.yank.include_pager_title;
                    match view.yank_visual_to_clipboard(include_title) {
                        Ok(n) => {
                            let unit = if in_block { "row" } else { "line" };
                            view.flash = Some(format!(
                                "yanked {n} {unit}{} to clipboard",
                                if n == 1 { "" } else { "s" }
                            ));
                        }
                        Err(e) => view.flash = Some(format!("yank failed: {e}")),
                    }
                    return Vec::new();
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    view.visual_move(1, viewport);
                    return Vec::new();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.visual_move(-1, viewport);
                    return Vec::new();
                }
                KeyCode::Char('h') | KeyCode::Left if in_block => {
                    view.visual_col_move(-1);
                    return Vec::new();
                }
                KeyCode::Char('l') | KeyCode::Right if in_block => {
                    view.visual_col_move(1);
                    return Vec::new();
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(half_page as isize, viewport);
                    return Vec::new();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(-half_page as isize, viewport);
                    return Vec::new();
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(page as isize, viewport);
                    return Vec::new();
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(-page as isize, viewport);
                    return Vec::new();
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    view.visual_move(page as isize, viewport);
                    return Vec::new();
                }
                KeyCode::PageUp | KeyCode::Char('b') => {
                    view.visual_move(-page as isize, viewport);
                    return Vec::new();
                }
                KeyCode::Char('g') | KeyCode::Home => {
                    view.visual_jump_to(0, viewport);
                    return Vec::new();
                }
                KeyCode::Char('G') | KeyCode::End => {
                    let last = view.lines.len().saturating_sub(1);
                    view.visual_jump_to(last, viewport);
                    return Vec::new();
                }
                _ => {
                    // Unknown key while in visual mode — ignore so a
                    // stray `/` or `:` doesn't silently trigger a
                    // search/jump that the visual selection wasn't
                    // expecting. User must Esc out first.
                    return Vec::new();
                }
            }
        }

        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => {
                // v1.5 pane-scroll pager: snap the underlying pty
                // back to live and clear the divider's [SCROLL]
                // indicator. The pager is closed in the regular
                // path below.
                if self.view.pager.as_ref().is_some_and(|v| v.pane_scroll) {
                    self.close_pane_scroll_pager();
                    return Vec::new();
                }
                // Pager-help overlay: dismiss just the help, restore
                // whatever pager was active when `?` was pressed.
                // Restore from the dedicated `pager_help_stash` slot
                // so the original mount (Overlay / TopPane /
                // LowerPane) and `pane_scroll` flag come back intact
                // — going through `pager_history` here would lose
                // the v1.5 mount mounts (filtered by `no_history`)
                // and pop a stale file-viewer instead.
                if self
                    .view
                    .pager
                    .as_ref()
                    .is_some_and(|v| v.title == crate::ui::pager::PAGER_HELP_TITLE)
                {
                    self.view.pager = self.view.pager_help_stash.take();
                    self.view.pager_jump_buf = None;
                    self.view.pager_pending_bracket = None;
                    self.view.needs_full_repaint = true;
                    return Vec::new();
                }
                // Task viewer special close: if the viewed task has
                // exited (and the user has seen it), promote -- snapshot
                // its rendered view into buffer history and drop the
                // task from the bg list. Running tasks stay in bg.
                let promote_task: Option<u32> = self.view.pager.as_ref().and_then(|v| {
                    let id = v.task_id?;
                    let task = self
                        .runtime
                        .background_tasks
                        .tasks
                        .iter()
                        .find(|t| t.id == id)?;
                    if task.viewed_in_task_viewer && !matches!(task.status, TaskStatus::Running) {
                        Some(id)
                    } else {
                        None
                    }
                });
                if let Some(id) = promote_task {
                    if let Some(task) = self.runtime.background_tasks.take(id) {
                        let mut snapshot = Self::build_task_viewer_for(id, &task);
                        snapshot.task_id = None; // not a live viewer anymore
                        snapshot.no_history = false; // must be eligible for history
                        self.view.pager_history.push(snapshot);
                        // Reap the child handle if still around (already
                        // wait()'d when EOF arrived; this is just to drop
                        // the writer/rx). Implicit via task drop.
                        drop(task);
                    }
                    // Don't double-push the original viewer.
                    self.clear_pager();
                } else {
                    // Save eligible pagers to history before closing.
                    let is_picker = self.state.pending_worktrees.is_some()
                        || self.state.pending_sessions.is_some()
                        || self.view.pending_history_pick.is_some();
                    if !is_picker
                        && let Some(ref v) = self.view.pager
                        && v.picker_cursor.is_none()
                        && !v.streaming
                    {
                        // Persist scroll BEFORE the take —
                        // otherwise the take leaves self.view.pager
                        // None and the trailing clear_pager's
                        // save call is a no-op. Without this,
                        // file pagers closed via Esc/q never
                        // got their scroll position saved to
                        // disk (memory only, via history).
                        self.remember_pager_position();
                        if let Some(v) = self.view.pager.take() {
                            self.view.pager_history.push(v);
                        }
                    }
                    self.clear_pager();
                }
                self.state.pending_worktrees = None;
                self.state.pending_sessions = None;
                self.view.pending_history_pick = None;
                self.view.pending_jump_history = None;
                self.view.pager_jump_buf = None;
                self.view.pager_pending_bracket = None;
                self.view.needs_full_repaint = true;
            }
            KeyCode::Char('/') => view.begin_search(),
            KeyCode::Char('n') => view.search_next(viewport),
            KeyCode::Char('N') => view.search_prev(viewport),
            KeyCode::Char(':') => {
                self.view.pager_jump_buf = Some(String::new());
                view.jump_buf = Some(String::new());
            }
            KeyCode::Char('[' | ']') => {
                if let KeyCode::Char(c) = key.code {
                    self.view.pager_pending_bracket = Some(c);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => view.scroll_by(1, viewport),
            KeyCode::Char('k') | KeyCode::Up => view.scroll_by(-1, viewport),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(i32::from(viewport) / 2, viewport);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(-i32::from(viewport) / 2, viewport);
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(i32::from(viewport), viewport);
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(-i32::from(viewport), viewport);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => view.scroll_by(i32::from(viewport), viewport),
            KeyCode::PageUp | KeyCode::Char('b') => view.scroll_by(-i32::from(viewport), viewport),
            KeyCode::Char('g') | KeyCode::Home => view.scroll_to_top(),
            KeyCode::Char('G') | KeyCode::End => view.scroll_to_bottom(viewport),
            KeyCode::Char('l') => view.toggle_line_numbers(),
            KeyCode::Char('|') => {
                // git-view diff/show: flip unified ⇄ side-by-side and
                // re-render from the retained model. A no-op for blame /
                // non-git-view pagers (toggle returns false). Re-borrows
                // `self` (the `view` borrow above is not used in this arm).
                self.toggle_git_view_layout();
            }
            KeyCode::Char('w') => view.toggle_whitespace(),
            KeyCode::Char('W') => view.toggle_wrap(),
            KeyCode::Char('m') if !view.toggle_markdown() => {
                view.flash = Some("not a markdown file".into());
            }
            KeyCode::Char('f') => view.toggle_full_width(),
            KeyCode::Char('y') => {
                let include_title = self.state.config.yank.include_pager_title;
                match view.yank_to_clipboard(include_title) {
                    Ok(()) => view.flash = Some("yanked source to clipboard".into()),
                    Err(e) => view.flash = Some(format!("yank failed: {e}")),
                }
            }
            KeyCode::Char('Y') => {
                let include_title = self.state.config.yank.include_pager_title;
                match view.yank_visible_to_clipboard(include_title) {
                    Ok(()) => view.flash = Some("yanked visible to clipboard".into()),
                    Err(e) => view.flash = Some(format!("yank failed: {e}")),
                }
            }
            KeyCode::Char('V') => {
                // Enter visual line mode -- anchor at the top visible
                // line, then j/k/G/etc. extend the selection and `y`
                // yanks the inclusive range. The interceptor above
                // takes over all subsequent keys until Esc / V exit.
                view.enter_visual();
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // ^v — enter placement mode. The user moves a
                // cursor with vi motions (hjkl, w/b, 0/$, gg/G)
                // and presses ^v again to commit to a visual
                // block selection at that anchor — or `V` to
                // commit to Line visual at the cursor's row.
                // `Esc` cancels.
                //
                // The old "anchor at top visible line immediately"
                // behavior was awkward when the user wanted the
                // anchor anywhere other than the top of the
                // viewport; placement makes the anchor explicit.
                view.enter_placement();
                view.flash = Some(
                    "placement: hjkl/w/b/0/$/gG to move · ^v block · V line · Esc cancel".into(),
                );
            }
            KeyCode::Char('S') if view.task_id.is_some() => {
                // Task viewer: S (Stop) pauses the underlying task
                // via SIGSTOP to its process group. Mirrors the
                // :pause command for hand-on-keyboard control. The
                // pause/flash runs via run_effects (status-bar flash,
                // never the pager footer); no post-match code follows,
                // so returning the effect here is byte-identical.
                let id = view.task_id.unwrap();
                return self.pause_task(Some(id));
            }
            KeyCode::Char('C') if view.task_id.is_some() => {
                // Task viewer: C (Continue) resumes a paused task
                // via SIGCONT. Mirrors the :resume command.
                let id = view.task_id.unwrap();
                return self.resume_task(Some(id));
            }
            KeyCode::Char('p') => {
                // Hand the file off to $PAGER (default less) via full
                // TTY takeover. Same suspend_tui / resume_tui dance as
                // `v` for $EDITOR. Right tool for huge files past our
                // in-app cap, or for users who want less's specific
                // search / mark / pipe-out features.
                let Some(ref src) = view.source_path else {
                    view.flash = Some("no source file (try `s` to save first)".into());
                    return Vec::new();
                };
                let argv = shell::resolve_pager();
                let pager_cmd = argv.join(" ");
                let path_quoted = shell::shell_quote(&src.display().to_string());
                self.clear_pager();
                self.view.needs_full_repaint = true;
                return sh_c(&format!("{pager_cmd} {path_quoted}"), false);
            }
            KeyCode::Char('s') if view.saveable => match view.save_to_file() {
                Ok(path) => view.flash = Some(format!("saved: {}", path.display())),
                Err(e) => view.flash = Some(format!("save failed: {e}")),
            },
            KeyCode::Char('v') => {
                let argv = shell::resolve_editor();
                if argv.is_empty() {
                    view.flash = Some("no $VISUAL or $EDITOR set".to_string());
                    return Vec::new();
                }
                let editor_cmd = argv.join(" ");
                let scroll = view.scroll;
                // Preserve the v1.5 mount + pane_scroll across the
                // editor round-trip so a `v` from the lower-pane
                // scrollback pager doesn't return as a centered
                // overlay (reported as a regression).
                let mount = view.mount;
                let pane_scroll = view.pane_scroll;

                // Top-pane pager (D) with a real source path: route
                // the editor through the same top-overlay path as `V`
                // from the file list, so the bottom pane stays visible
                // for the editor session. Other mounts (overlay /
                // lower-pane) and the temp-file edit path keep the
                // full-screen Spawn flow.
                if matches!(mount, crate::ui::pager::Mount::TopPane)
                    && let Some(src) = view.source_path.clone()
                {
                    let cmd = format!(
                        "{editor_cmd} {}",
                        shell::shell_quote(&src.display().to_string())
                    );
                    let (rows, cols) = Self::top_overlay_size(
                        self.effective_pane_pct(),
                        self.runtime.pane_tabs.is_some(),
                    );
                    let cwd = self.state.listing.dir.clone();
                    self.clear_pager();
                    self.view.needs_full_repaint = true;
                    let wake = self.make_pane_wake();
                    match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
                        Ok(p) => {
                            self.runtime.top_overlay = Some(p);
                            self.state.focus = Focus::Overlay;
                        }
                        Err(e) => self.state.flash_error(format!("spawn: {e}")),
                    }
                    return Vec::new();
                }

                // Determine the file to edit and the return state.
                let (edit_path, pager_return) = if let Some(ref src) = view.source_path {
                    (
                        src.clone(),
                        PagerReturn::SourceFile {
                            path: src.clone(),
                            scroll,
                            mount,
                            pane_scroll,
                        },
                    )
                } else {
                    let title = view.title.clone();
                    match view.write_to_temp() {
                        Ok(tmp) => (
                            tmp.clone(),
                            PagerReturn::TempFile {
                                path: tmp,
                                title,
                                scroll,
                                mount,
                                pane_scroll,
                            },
                        ),
                        Err(e) => {
                            self.state.flash_error(format!("write temp: {e}"));
                            return Vec::new();
                        }
                    }
                };
                self.view.pending_pager_return = Some(pager_return);
                self.clear_pager();
                self.view.needs_full_repaint = true;
                return sh_c(
                    &format!(
                        "{editor_cmd} {}",
                        shell::shell_quote(&edit_path.display().to_string())
                    ),
                    false,
                );
            }
            KeyCode::Char('?') | KeyCode::F(1) => {
                // Stash the current pager so dismissing the help
                // (Esc/q) restores it verbatim — same content,
                // same mount. Going through `pager_history.push`
                // here was the v1.5 regression: it filters out
                // `no_history=true` views (which both
                // `Mount::LowerPane` `^a-v` and `Mount::TopPane`
                // `D` set, intentionally) — so the help would
                // dismiss to either nothing or a stale older
                // file-viewer pulled off the back stack.
                if let Some(current) = self.view.pager.take() {
                    self.view.pager_help_stash = Some(current);
                }
                self.view.pager = Some(crate::ui::pager::build_pager_help(&self.view.theme));
                self.view.needs_full_repaint = true;
            }
            _ => {}
        }
        Vec::new()
    }
}

/// Pager open/close/build hub: assign/clear the active pager (persisting
/// scroll position), build a `PagerView` from a file (text + markdown/JSON/
/// syntax + hex), and the `V`/`D` editor / pager-overlay spawns. Extracted
/// verbatim from `app/mod.rs` (the impl-extraction sweep). All `pub` — these
/// are called from many sites across `app` and its siblings (effect, actions,
/// session, tasks, navigate, git, …).
impl App {
    /// Persist the current pager's scroll position to disk if it's a
    /// file-backed view (`source_path` is set). Call before any
    /// assignment that drops or replaces `self.view.pager` so the user's
    /// reading position survives close + reopen. No-op for command
    /// output, help, picker UIs, etc. — those views intentionally
    /// don't carry a `source_path`.
    pub fn remember_pager_position(&mut self) {
        if let Some(view) = self.view.pager.as_ref()
            && let Some(path) = view.source_path.clone()
        {
            let scroll = view.scroll;
            self.view.pager_positions.record(&path, scroll);
        }
    }

    /// Close the active pager, persisting its scroll position first.
    /// Drop-in replacement for the raw `pager = None` assignment
    /// everywhere the user's reading position should survive close
    /// + reopen.
    pub fn clear_pager(&mut self) {
        self.remember_pager_position();
        self.view.pager = None;
    }

    /// Tear down a `^a-v` scrollback pager: snap the pty back to
    /// live, clear the pager, force a repaint, and flash the
    /// status change. Mirrors the Esc/q close path so chord-driven
    /// and focus-switch escapes land in the same final state. No-op
    /// when no pane_scroll pager is open — safe to call from
    /// `Action::PaneFocusUp` / `PaneFocusDown` unconditionally.
    pub fn close_pane_scroll_pager(&mut self) {
        if !self.view.pager.as_ref().is_some_and(|v| v.pane_scroll) {
            return;
        }
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_mut().exit_scroll_mode();
        }
        self.clear_pager();
        self.view.needs_full_repaint = true;
        self.state.flash_info("scroll: off");
    }

    /// Assign a new pager view, persisting the outgoing view's
    /// scroll position first. Drop-in replacement for the bare
    /// `pager = Some(view)` pattern at open / replace sites
    /// — covers the case where the user has one file open, opens
    /// another, then later wants to come back to the first one.
    pub fn set_pager(&mut self, view: PagerView) {
        self.remember_pager_position();
        self.view.pager = Some(view);
    }

    pub fn edit_in_pane(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
            self.state.flash_error("V: cannot edit a directory");
            return;
        }
        let argv = shell::resolve_editor();
        if argv.is_empty() {
            self.state.flash_error("no $VISUAL or $EDITOR set");
            return;
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => {
                self.runtime.top_overlay = Some(p);
                self.state.focus = state::Focus::Overlay;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
    }

    /// `D` — open the cursor file in spyc's in-app pager mounted in
    /// the top-pane slot, so the bottom pane (claude / zsh / etc.)
    /// stays visible. Mirror of `edit_in_pane` for the read path.
    /// Common workflow: `D` on a doc, `^a-j` into claude, work,
    /// `^a-k` back to scroll.
    ///
    /// v1.5 Phase 5 swapped the implementation from
    /// "spawn `\$PAGER` as a pty top overlay" to "use the in-app
    /// pager." The pager is more capable on every axis we care about
    /// (search, jump, syntax highlighting, range yank, markdown
    /// render, hex dump for binaries), and uses the existing
    /// `Mount::TopPane` rail laid in Phase 1.
    ///
    /// **Huge-file fallback:** files past `MAX_PAGER_BYTES` are
    /// still handed to `\$PAGER` as a top overlay because `less`
    /// streams from disk while the in-app pager loads the (already
    /// truncated) buffer into memory. Streaming wins for multi-GB
    /// logs.
    pub fn display_in_pane(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
            self.state.flash_error("D: cannot page a directory");
            return;
        }
        let file_size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if file_size > crate::fs::ops::MAX_PAGER_BYTES {
            // Huge file: $PAGER's stream-from-disk wins over our
            // in-memory pager. Fall back to the pre-v1.5 behavior
            // (spawn $PAGER as a top overlay).
            self.spawn_pager_overlay_for_path(&path);
            return;
        }
        let Some(mut view) = self.build_pager_view_for_file(&path) else {
            return;
        };
        view.mount = crate::ui::pager::Mount::TopPane;
        // Don't push to buffer history: this is a fresh open, not a
        // page the user navigated away from and might want to revisit
        // via `[b` / `]b`.
        view.no_history = true;
        self.set_pager(view);
        self.state.focus = state::Focus::Pager(pager::Mount::TopPane);
        self.view.needs_full_repaint = true;
    }

    /// Build a `PagerView` from a file on disk. Handles text (with
    /// markdown rendering / syntax highlighting / truncation banner
    /// for big files) and binary (hex dump). Flashes a read error
    /// and returns `None` on failure. The returned view has
    /// `mount = Overlay` (the default); callers override for
    /// `TopPane` / `LowerPane` mounts. Extracted from the old
    /// inline body of `ActivateIntent::Display` so both `Enter` /
    /// `d` (overlay) and `D` (top pane) share the same loading
    /// path.
    pub fn build_pager_view_for_file(&mut self, path: &Path) -> Option<PagerView> {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if shell::looks_like_text(path) {
            let file_size = std::fs::metadata(path).map_or(0, |m| m.len());
            // Big files used to OOM us: read_to_string + syntect every
            // token = file size × ~50 in pager state. Cap at
            // MAX_PAGER_BYTES; past that, load just MAX_PAGER_LINES of
            // plain text and tell the user how to hand off to $PAGER
            // for the full thing.
            let load_result = if file_size > crate::fs::ops::MAX_PAGER_BYTES {
                crate::fs::ops::read_truncated(path, crate::fs::ops::MAX_PAGER_LINES)
            } else {
                std::fs::read_to_string(path).map(|c| {
                    let n = c.lines().count();
                    (c, n, false)
                })
            };
            let (content, _line_count, truncated) = match load_result {
                Ok(t) => t,
                Err(e) => {
                    self.state.flash_error(format!("read: {e}"));
                    return None;
                }
            };
            let content = expand_tabs(&content);
            let is_md = crate::ui::markdown::is_markdown_path(path);
            // JSON pretty-print: try parse + canonical re-emit. On a
            // successful parse with output differing from the raw
            // bytes, `lines` holds the pretty version and `alt_lines`
            // holds the raw (`m` toggles). Re-uses the alt-view
            // machinery currently named for markdown (`alt_lines`,
            // `markdown_rendered`); a rename to a generic name is
            // queued for the folding work in v1.50.73.
            let json_pretty: Option<String> = if !truncated && crate::ui::json::is_json_path(path) {
                crate::ui::json::pretty_print(&content)
            } else {
                None
            };
            // Source-side lines: syntect-highlighted if available AND
            // we loaded the whole file (highlighting a partial file
            // would still mostly work but blows memory, and the
            // savings is the whole point of truncation).
            let source_lines: Vec<ratatui::text::Line<'static>> = if truncated {
                content
                    .lines()
                    .map(|l| ratatui::text::Line::from(l.to_string()))
                    .collect()
            } else {
                crate::ui::syntax::highlight_to_lines(&name, &content).unwrap_or_else(|| {
                    content
                        .lines()
                        .map(|l| ratatui::text::Line::from(l.to_string()))
                        .collect()
                })
            };
            let mut view = if let Some(pretty) = json_pretty {
                // Pretty differs from raw: build a styled view of the
                // pretty bytes, stash the (already-highlighted) raw
                // lines as alt for the `m` toggle.
                let pretty_lines: Vec<ratatui::text::Line<'static>> =
                    crate::ui::syntax::highlight_to_lines(&name, &pretty).unwrap_or_else(|| {
                        pretty
                            .lines()
                            .map(|l| ratatui::text::Line::from(l.to_string()))
                            .collect()
                    });
                let mut v = PagerView::new_styled(name.clone(), pretty_lines);
                if pretty != content {
                    v.alt_lines = Some(source_lines);
                    // `markdown_rendered = true` semantically means
                    // "the processed/alt-form is in `lines`". Same
                    // interpretation for JSON: pretty is "rendered".
                    v.markdown_rendered = true;
                }
                v
            } else if is_md && !truncated {
                // Pre-compute both views; `m` toggles. Yank/save
                // always hit the source via `source_text()`. Which
                // view shows first is configurable via
                // `[markdown] open_as_rendered`. Skipped for
                // truncated files since markdown rendering of half a
                // doc looks weird (broken refs, half-closed code
                // fences).
                // Hint the markdown renderer at the actual pager body
                // width so wide tables expand instead of wrapping into
                // the 80-col prose budget. Centered overlay pager
                // claims 90% of the terminal minus block borders;
                // matches the `pager_inner_area` math.
                //
                // Subtract the projected line-number gutter so a wide
                // table doesn't overflow the right edge of the
                // viewport. The gutter is `ilog10(lines) + 2` cells
                // wide (see `pager::render`); we don't yet know the
                // RENDERED line count (it can exceed the source's
                // because of soft-break-as-hard-break + table
                // expansion), so use 4× the source count as a
                // conservative estimate, which buys ~1 digit of
                // safety on the gutter.
                let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
                let body_w = crate::ui::pager::centered_body_width(term_w) as usize;
                let source_line_count = content.lines().count().max(1);
                let gutter_w = (source_line_count.saturating_mul(4)).max(1).ilog10() as usize + 2;
                let pager_w = body_w.saturating_sub(2 + gutter_w);
                let rendered =
                    crate::ui::markdown::render(&content, &self.view.theme, Some(pager_w));
                if self.state.config.markdown.open_as_rendered {
                    let mut v = PagerView::new_styled(name, rendered);
                    v.alt_lines = Some(source_lines);
                    v.markdown_rendered = true;
                    v
                } else {
                    // Source first: `lines` holds source, `alt_lines`
                    // holds the rendered view, `markdown_rendered`
                    // is false. `m` swap is symmetric.
                    let mut v = PagerView::new_styled(name, source_lines);
                    v.alt_lines = Some(rendered);
                    v.markdown_rendered = false;
                    v
                }
            } else {
                let display_name = if truncated {
                    format!(
                        "{name} \u{26a0} truncated · {} MB",
                        file_size / (1024 * 1024)
                    )
                } else {
                    name
                };
                let mut v = PagerView::new_styled(display_name, source_lines);
                if truncated {
                    // Append a banner row pointing at the escape
                    // hatch so the user knows the cap fired and what
                    // to do.
                    let warn_style = ratatui::style::Style::default()
                        .fg(self.view.theme.pick)
                        .add_modifier(ratatui::style::Modifier::BOLD);
                    v.lines.push(ratatui::text::Line::from(""));
                    v.lines
                        .push(ratatui::text::Line::from(ratatui::text::Span::styled(
                            format!(
                                "[truncated at {} lines · {} MB total · press p to open in $PAGER]",
                                crate::fs::ops::MAX_PAGER_LINES,
                                file_size / (1024 * 1024)
                            ),
                            warn_style,
                        )));
                    // Also flash an immediate hint — the banner is at
                    // the bottom and the user might not scroll there
                    // before wondering what happened to their file.
                    v.flash = Some(format!(
                        "truncated at {} lines · press p for full file in $PAGER",
                        crate::fs::ops::MAX_PAGER_LINES
                    ));
                }
                v
            };
            view.source_path = Some(path.to_path_buf());
            // Restore the scroll position from the previous visit (if
            // any). Clamp to `lines.len() - 1` so a saved row that's
            // now past the end (file shrank) lands at the new last
            // line rather than blanking the viewport.
            if let Some(saved) = self.view.pager_positions.get(path) {
                let last = view.lines.len().saturating_sub(1);
                view.scroll = saved.min(u16::try_from(last).unwrap_or(u16::MAX));
            }
            Some(view)
        } else {
            // Binary file: hex dump via pretty-hex.
            match fs::ops::hex_dump_lines(path, &self.view.theme) {
                Ok(lines) => {
                    let mut view = PagerView::new_plain(format!("{name} [hex]"), Vec::new());
                    view.lines = lines;
                    Some(view)
                }
                Err(e) => {
                    self.state.flash_error(format!("hex: {e}"));
                    None
                }
            }
        }
    }

    /// Pre-v1.5 `D` behavior: spawn `\$PAGER` as a top overlay pty.
    /// Now used only as the huge-file fallback path from
    /// `display_in_pane` — files past `MAX_PAGER_BYTES` benefit from
    /// `less`'s stream-from-disk over our in-memory pager.
    pub fn spawn_pager_overlay_for_path(&mut self, path: &Path) {
        let argv = shell::resolve_pager();
        if argv.is_empty() {
            self.state.flash_error("no $PAGER set");
            return;
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => {
                self.runtime.top_overlay = Some(p);
                self.state.focus = state::Focus::Overlay;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
    }
}

/// Expand tab characters to spaces at 8-column tab stops (the pager renders
/// with a fixed tab width). Sole caller is `build_pager_view_for_file`.
fn expand_tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = 8 - (col % 8);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else if ch == '\n' {
            out.push(ch);
            col = 0;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

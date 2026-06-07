//! Pager input *modes* that intercept keys before the scroll/motion
//! fall-through: contextual `^C`, `/` search typing, the `:N` jump buffer,
//! `[`/`]` chord follow-ups, and placement / visual selection. Each returns
//! `Some` when it consumes the key, `None` to fall through. Split from
//! `pager_handler` verbatim; `impl App` child reading App privates.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Effect};

impl App {
    /// Contextual `^C`: interrupt a viewed task, else flash an in-pager hint.
    pub(super) fn handle_pager_ctrl_c(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let view = self.view.pager.as_mut()?;
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
            return Some(Vec::new());
        }

        None
    }

    /// While typing a `/` search query, most keys feed the buffer.
    pub(super) fn handle_pager_search_typing(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let view = self.view.pager.as_mut()?;
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
            return Some(Vec::new());
        }

        None
    }

    /// Inline `:N` jump — accumulate digits, Enter commits, Esc cancels.
    pub(super) fn handle_pager_jump_buf(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let view = self.view.pager.as_mut()?;
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
            return Some(Vec::new());
        }

        None
    }

    /// `[b`/`]b` buffer-history nav and `[t`/`]t` task-viewer cycle (chord follow-up).
    pub(super) fn handle_pager_bracket(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
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
                return Some(Vec::new());
            }
            if key.code == KeyCode::Char('t') {
                let direction = if bracket == '[' { -1 } else { 1 };
                self.cycle_task_viewer(direction);
                return Some(Vec::new());
            }
            // Unrecognized chord follow-up -- swallow it.
            return Some(Vec::new());
        }

        None
    }

    /// Placement mode: pre-visual-block cursor positioning via vi motions.
    pub(super) fn handle_pager_placement(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let view = self.view.pager.as_mut()?;
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
                    return Some(Vec::new());
                }
                KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.commit_placement_to_visual_block();
                    view.flash = Some("visual block".into());
                    return Some(Vec::new());
                }
                KeyCode::Char('V') => {
                    view.commit_placement_to_visual_line();
                    view.flash = Some("visual line".into());
                    return Some(Vec::new());
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    view.placement_move(0, -1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    view.placement_move(0, 1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    view.placement_move(1, 0, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.placement_move(-1, 0, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('0') | KeyCode::Home => {
                    view.placement_line_start();
                    return Some(Vec::new());
                }
                KeyCode::Char('$') | KeyCode::End => {
                    view.placement_line_end();
                    return Some(Vec::new());
                }
                KeyCode::Char('w') => {
                    view.placement_word_forward();
                    return Some(Vec::new());
                }
                KeyCode::Char('b') => {
                    view.placement_word_backward();
                    return Some(Vec::new());
                }
                KeyCode::Char('g') => {
                    // Single `g` jumps to top (no `gg` two-key required —
                    // simpler than reusing the pager's pending-g state
                    // machine, and placement is short-lived anyway).
                    view.placement_jump_to(0, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('G') => {
                    let last = view.lines.len().saturating_sub(1);
                    view.placement_jump_to(last, viewport);
                    return Some(Vec::new());
                }
                _ => {
                    // Anything else: swallow. Keeps the user from
                    // accidentally scrolling or yanking while in
                    // placement.
                    return Some(Vec::new());
                }
            }
        }

        None
    }

    /// Visual mode (Line/Block): motion keys move the selection; `y` yanks.
    pub(super) fn handle_pager_visual(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let view = self.view.pager.as_mut()?;
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
                return Some(Vec::new());
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
                return Some(Vec::new());
            }
            if matches!(key.code, KeyCode::Char('v'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                if in_block {
                    view.cancel_visual();
                } else {
                    view.enter_visual_block();
                }
                return Some(Vec::new());
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
                    return Some(Vec::new());
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    view.visual_move(1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.visual_move(-1, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('h') | KeyCode::Left if in_block => {
                    view.visual_col_move(-1);
                    return Some(Vec::new());
                }
                KeyCode::Char('l') | KeyCode::Right if in_block => {
                    view.visual_col_move(1);
                    return Some(Vec::new());
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(half_page as isize, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(-half_page as isize, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(page as isize, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.visual_move(-page as isize, viewport);
                    return Some(Vec::new());
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    view.visual_move(page as isize, viewport);
                    return Some(Vec::new());
                }
                KeyCode::PageUp | KeyCode::Char('b') => {
                    view.visual_move(-page as isize, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('g') | KeyCode::Home => {
                    view.visual_jump_to(0, viewport);
                    return Some(Vec::new());
                }
                KeyCode::Char('G') | KeyCode::End => {
                    let last = view.lines.len().saturating_sub(1);
                    view.visual_jump_to(last, viewport);
                    return Some(Vec::new());
                }
                _ => {
                    // Unknown key while in visual mode — ignore so a
                    // stray `/` or `:` doesn't silently trigger a
                    // search/jump that the visual selection wasn't
                    // expecting. User must Esc out first.
                    return Some(Vec::new());
                }
            }
        }

        None
    }
}

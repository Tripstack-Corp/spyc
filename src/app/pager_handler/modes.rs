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
        let view = active_pager_mut!(self)?;
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
                        if let Some(v) = active_pager_mut!(self) {
                            v.flash = Some(msg);
                        }
                    }
                }
            } else if let Some(v) = active_pager_mut!(self) {
                v.flash = Some("press Esc or q to close pager".into());
            }
            return Some(Vec::new());
        }

        None
    }

    /// Route a paste into the active pager. While typing a `/` search the
    /// text extends the search buffer (newline-stripped, so a multi-line
    /// paste stays one query); otherwise a pager has no text input, so flash
    /// a hint inside it. Uses the `active_pager_mut!` macro so the borrow
    /// stays field-level (a `&mut self` accessor would borrow all of `*self`).
    pub fn handle_pager_paste(&mut self, text: &str) {
        let Some(view) = active_pager_mut!(self) else {
            return;
        };
        if view.is_typing_search() {
            for c in text.chars().filter(|c| *c != '\n' && *c != '\r') {
                view.search_push_char(c);
            }
        } else {
            view.flash = Some("paste ignored — press `/` to search".into());
        }
    }

    /// While typing a `/` search query, most keys feed the buffer.
    pub(super) fn handle_pager_search_typing(
        &mut self,
        key: KeyEvent,
        viewport: u16,
    ) -> Option<Vec<Effect>> {
        let view = active_pager_mut!(self)?;
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
                    } else if self.state.pending_worktrees.is_some() {
                        // Worktree picker: land the highlighted cursor on the
                        // search match so Enter then switches to that worktree.
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
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
    ///
    /// The buffer is the active `PagerView`'s own `jump_buf` (single source of
    /// truth, like `search`/`visual`); the footer indicator renders straight
    /// from it, so there is no separate copy to keep in sync. Jump mode is
    /// modal — this handler runs early (see `handle_pager_key`) and swallows
    /// every key while active, so no pager swap / focus change can interleave.
    pub(super) fn handle_pager_jump_buf(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        let view = active_pager_mut!(self)?;
        // Not in jump mode → fall through to the other pager handlers.
        view.jump_buf.as_ref()?;
        match key.code {
            KeyCode::Char(c @ '0'..='9') => {
                if let Some(buf) = view.jump_buf.as_mut() {
                    buf.push(c);
                }
            }
            KeyCode::Backspace => {
                // Pop a digit; backspacing past the first digit exits jump mode.
                if view.jump_buf.as_mut().is_some_and(|b| b.pop().is_none()) {
                    view.jump_buf = None;
                }
            }
            KeyCode::Enter => {
                let n = view
                    .jump_buf
                    .as_ref()
                    .and_then(|b| b.parse::<usize>().ok())
                    .filter(|&n| n > 0);
                view.jump_buf = None;
                if let Some(n) = n {
                    let target = n.saturating_sub(1);
                    if self.view.pending_history_pick.is_some() {
                        // History editor: jump to entry N.
                        let max = view.lines.len().saturating_sub(1);
                        let clamped = target.min(max);
                        view.picker_cursor = Some(clamped);
                        view.scroll = clamped.saturating_sub(2);
                    } else {
                        // Regular pager: jump to line N, clamped to the
                        // last line — a jump past EOF would otherwise leave
                        // `scroll` past the end and blank the viewport.
                        view.scroll = target;
                        view.clamp_scroll_auto();
                    }
                }
                if self.view.pending_history_pick.is_some() {
                    self.sync_history_editor_to_cursor();
                }
            }
            _ => {
                // Esc or non-digit cancels.
                view.jump_buf = None;
            }
        }
        Some(Vec::new())
    }

    /// `[b` (`forward = false`) / `]b` (`forward = true`): step the top pager's
    /// buffer history, swapping the live pager and flashing the new depth, or a
    /// "no older / no newer buffers" hint at the end. The back/forward arms were
    /// 15-line mirror copies — only the history call and the end-of-list wording
    /// differ.
    fn nav_pager_buffer(&mut self, forward: bool) {
        let Some(mut current) = self.view.pager.take() else {
            return;
        };
        // If navigating away from a mid-flight overlay stream, kill it now and
        // clear the streaming flag on the pager before pushing it to history.
        // Without this, `drain_pager_stream` kills the worker a tick later but
        // the pager is already in history with `streaming = true` — so navigating
        // back with `]b` shows a pager frozen at "scanning…" with no stream to
        // drain it.
        if current.streaming {
            if let Some(id) = current.stream_id
                && self
                    .runtime
                    .pager_stream
                    .as_ref()
                    .is_some_and(|s| s.id() == id)
            {
                self.runtime.pager_stream = None;
            }
            current.streaming = false;
            current.stream_id = None;
        }
        // A flash describes the keypress that set it, so it must not ride into
        // history and resurface when this view is restored.
        current.flash = None;
        let step = if forward {
            self.view.pager_history.go_forward(current)
        } else {
            self.view.pager_history.go_back(current)
        };
        let msg = match step {
            Ok(next) => {
                self.view.pager = Some(next);
                self.view.needs_full_repaint = true;
                let back = self.view.pager_history.back_len();
                let fwd = self.view.pager_history.forward_len();
                format!("buffer ←{back} →{fwd}")
            }
            Err(current) => {
                self.view.pager = Some(current);
                if forward {
                    "no newer buffers".to_string()
                } else {
                    "no older buffers".to_string()
                }
            }
        };
        // Flash in the pager, not the file-list status bar the pager covers.
        if let Some(view) = self.view.pager.as_mut() {
            view.flash = Some(msg);
        }
    }

    /// `[b`/`]b` buffer-history nav and `[t`/`]t` task-viewer cycle (chord follow-up).
    pub(super) fn handle_pager_bracket(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        // [b / ]b — pager buffer history navigation (two-key sequence).
        // [t / ]t — task viewer cycle (peek through backgrounded tasks).
        if let Some(bracket) = self.view.pager_pending_bracket.take() {
            // Buffer history + task cycle belong to the top/overlay pager.
            // A bottom scrollback (`view.scroll_pager`) is `no_history` and
            // task-less, so swallow the chord rather than navigate the top
            // pager's history from underneath the focused scrollback.
            if self.state.pane_focused() && self.view.scroll_pager.is_some() {
                return Some(Vec::new());
            }
            if key.code == KeyCode::Char('b') {
                match bracket {
                    '[' => self.nav_pager_buffer(false),
                    ']' => self.nav_pager_buffer(true),
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
        let view = active_pager_mut!(self)?;
        // Placement mode: cursor positioning before a visual commit.
        // A first `^v` (block intent) or `V` (line intent) enters this
        // state; vi motions move the cursor without defining a selection
        // yet. Second `^v` commits to visual block at the cursor; `V`
        // commits to visual line at the cursor's row; `Esc` cancels. We
        // swallow motion-related keys so they don't fall through to scroll.
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
        let view = active_pager_mut!(self)?;
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
                    if let Some((text, n, _)) = view.visual_yank_text(include_title) {
                        let unit = if in_block { "row" } else { "line" };
                        let ok_msg = format!(
                            "yanked {n} {unit}{} to clipboard",
                            if n == 1 { "" } else { "s" }
                        );
                        return Some(vec![Effect::CopyToPagerClipboard { text, ok_msg }]);
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

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::ui::pager::PagerView;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn ch(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    /// App with a 50-line pager and a pinned viewport so the jump's clamped
    /// scroll is deterministic regardless of the headless terminal size.
    fn jump_pager_app() -> App {
        let mut app = App::test_app(std::env::temp_dir());
        let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        let view = PagerView::new_plain("t", lines);
        view.last_viewport_h.set(20);
        app.view.pager = Some(view);
        app
    }

    fn pager(app: &App) -> &PagerView {
        app.view.pager.as_ref().expect("pager")
    }

    #[test]
    fn jump_buf_accumulates_on_the_pager_and_feeds_the_footer() {
        let mut app = jump_pager_app();
        app.handle_pager_key(ch(':'));
        app.handle_pager_key(ch('1'));
        app.handle_pager_key(ch('2'));
        // The buffer is the pager's own field — the single source of truth the
        // footer indicator (`status_text`) renders straight from, no mirror.
        assert_eq!(pager(&app).jump_buf.as_deref(), Some("12"));
    }

    #[test]
    fn jump_buf_enter_jumps_to_line_and_clears() {
        let mut app = jump_pager_app();
        for k in [ch(':'), ch('1'), ch('0'), code(KeyCode::Enter)] {
            app.handle_pager_key(k);
        }
        // `:10` → line 10 (0-indexed scroll 9); buffer cleared on commit.
        assert_eq!(pager(&app).scroll, 9);
        assert_eq!(pager(&app).jump_buf, None);
    }

    #[test]
    fn jump_buf_esc_cancels_without_jumping() {
        let mut app = jump_pager_app();
        for k in [ch(':'), ch('5')] {
            app.handle_pager_key(k);
        }
        let before = pager(&app).scroll;
        app.handle_pager_key(code(KeyCode::Esc));
        assert_eq!(pager(&app).jump_buf, None, "Esc exits jump mode");
        assert_eq!(pager(&app).scroll, before, "cancel does not jump");
    }

    #[test]
    fn jump_buf_backspace_past_first_digit_exits() {
        let mut app = jump_pager_app();
        for k in [ch(':'), ch('7'), code(KeyCode::Backspace)] {
            app.handle_pager_key(k);
        }
        assert_eq!(
            pager(&app).jump_buf.as_deref(),
            Some(""),
            "one digit removed → empty buffer, still in jump mode"
        );
        app.handle_pager_key(code(KeyCode::Backspace));
        assert_eq!(
            pager(&app).jump_buf,
            None,
            "backspacing past the first digit exits jump mode"
        );
    }

    /// `H` in the scrollback gives it a dedicated help in the bottom slot;
    /// pressing `H` again toggles to the full pager-keys help (separate but
    /// linked); `Esc` restores the stashed scrollback.
    #[test]
    fn scrollback_help_toggles_with_pager_help_and_esc_restores() {
        use crate::app::state::Focus;
        use crate::ui::pager::{Mount, PAGER_HELP_TITLE, SCROLLBACK_HELP_TITLE};

        let mut app = App::test_app(std::env::temp_dir());
        app.state.focus = Focus::Pane; // bottom scrollback owns input
        let mut sb = PagerView::new_plain(
            "transcript",
            (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>(),
        );
        sb.pane_scroll = true;
        sb.mount = Mount::LowerPane;
        app.view.scroll_pager = Some(sb);

        // First H → transcript help, real scrollback stashed.
        app.handle_pager_key(ch('H'));
        assert_eq!(
            app.view.scroll_pager.as_ref().map(|v| v.title.as_str()),
            Some(SCROLLBACK_HELP_TITLE)
        );
        assert!(app.view.scroll_pager_help_stash.is_some());

        // H again → toggle to the full pager-keys help (still stashed).
        app.handle_pager_key(ch('H'));
        assert_eq!(
            app.view.scroll_pager.as_ref().map(|v| v.title.as_str()),
            Some(PAGER_HELP_TITLE)
        );
        assert!(app.view.scroll_pager_help_stash.is_some());

        // H once more → back to transcript help (toggle is symmetric).
        app.handle_pager_key(ch('H'));
        assert_eq!(
            app.view.scroll_pager.as_ref().map(|v| v.title.as_str()),
            Some(SCROLLBACK_HELP_TITLE)
        );

        // Esc → restore the real scrollback (not snap-to-live, not close).
        app.handle_pager_key(code(KeyCode::Esc));
        assert_eq!(
            app.view.scroll_pager.as_ref().map(|v| v.title.as_str()),
            Some("transcript")
        );
        assert!(app.view.scroll_pager_help_stash.is_none());
    }

    /// App with a pager whose only `needle` sits on line 3, so a committed
    /// search that lands there proves the query really holds "needle" (an
    /// empty query — the leak case — matches line 0 instead).
    fn search_pager_app() -> App {
        let mut app = App::test_app(std::env::temp_dir());
        let lines = ["alpha", "beta", "gamma", "delta needle", "epsilon"]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>();
        let view = PagerView::new_plain("t", lines);
        view.last_viewport_h.set(5);
        app.view.pager = Some(view);
        app
    }

    /// #16: a paste while typing a `/` search extends the search buffer.
    /// Drives the production entry (`handle_paste` → `route_input` →
    /// `handle_pager_paste`), so the routing and the leaf are both covered.
    #[test]
    fn paste_while_typing_search_lands_in_the_query_not_the_pane() {
        let mut app = search_pager_app();
        app.handle_pager_key(ch('/'));
        assert!(pager(&app).is_typing_search());

        // Newlines are stripped, so a multi-line clipboard stays one query.
        let fx = app.handle_paste("nee\ndle".to_string());
        assert!(
            fx.is_empty(),
            "a paste into the search buffer emits no effects — nothing is \
             forwarded to a pty: {fx:?}"
        );
        assert!(
            pager(&app).is_typing_search(),
            "a paste extends the query, it does not commit it"
        );

        // Commit: only a query of "needle" can land on line 3.
        let view = app.view.pager.as_mut().expect("pager");
        assert!(view.commit_search(5), "pasted query must match");
        assert_eq!(view.current_match_line(), Some(3));
    }

    /// The other half of `handle_pager_paste`: a pager with no search open has
    /// no text sink, so the paste is swallowed with an in-pager hint — the
    /// point being it is not forwarded to the pty either.
    #[test]
    fn paste_without_a_search_flashes_in_the_pager_and_is_swallowed() {
        let mut app = search_pager_app();
        let fx = app.handle_paste("needle".to_string());
        assert!(fx.is_empty(), "no effects: {fx:?}");
        assert_eq!(
            pager(&app).flash.as_deref(),
            Some("paste ignored — press `/` to search")
        );
    }

    /// #166: the pager is drawn over the file list, so a status-bar flash renders
    /// half-occluded. Exhausting the buffer history must report in the pager.
    #[test]
    fn exhausted_buffer_history_flashes_in_the_pager_not_the_status_bar() {
        for (bracket, msg) in [('[', "no older buffers"), (']', "no newer buffers")] {
            let mut app = jump_pager_app();
            app.handle_pager_key(ch(bracket));
            app.handle_pager_key(ch('b'));
            assert_eq!(pager(&app).flash.as_deref(), Some(msg), "{bracket}b");
            assert!(
                app.state.flash.is_none(),
                "{bracket}b must not flash the occluded status bar: {:?}",
                app.state.flash
            );
        }
    }

    /// The success half: stepping to a real buffer reports its depth in the
    /// pager too, so the whole action speaks in one place.
    #[test]
    fn buffer_history_step_flashes_depth_in_the_pager() {
        let mut app = jump_pager_app();
        app.view
            .pager_history
            .push(PagerView::new_plain("older", vec!["a".to_string()]));
        app.handle_pager_key(ch('['));
        app.handle_pager_key(ch('b'));
        assert_eq!(pager(&app).title, "older", "stepped back a buffer");
        assert_eq!(pager(&app).flash.as_deref(), Some("buffer ←0 →1"));
        assert!(
            app.state.flash.is_none(),
            "depth belongs in the pager: {:?}",
            app.state.flash
        );
    }

    /// A pager carrying a flash must not resurrect it when restored from
    /// history — the message described a keypress that is long gone.
    #[test]
    fn buffer_history_does_not_resurrect_a_stale_flash() {
        let mut app = jump_pager_app();
        app.view
            .pager_history
            .push(PagerView::new_plain("older", vec!["a".to_string()]));
        for k in [ch('['), ch('b'), ch(']'), ch('b')] {
            app.handle_pager_key(k);
        }
        assert_eq!(pager(&app).title, "t", "back then forward returns us home");
        assert_eq!(pager(&app).flash.as_deref(), Some("buffer ←1 →0"));
    }

    /// A scrollback pager sits in its own slot, so the relocation has to follow
    /// `active_pager_slot()` — not `view.pager`, which is empty here.
    #[test]
    fn scrollback_key_message_lands_in_the_scrollback_not_the_status_bar() {
        use crate::app::state::Focus;
        use crate::ui::pager::Mount;

        let mut app = App::test_app(std::env::temp_dir());
        app.state.focus = Focus::Pane;
        let mut sb = PagerView::new_plain("transcript", vec!["line".to_string()]);
        sb.pane_scroll = true;
        sb.mount = Mount::LowerPane;
        app.view.scroll_pager = Some(sb);

        app.handle_pager_key(ch('t'));
        assert_eq!(
            app.view
                .scroll_pager
                .as_ref()
                .and_then(|v| v.flash.as_deref()),
            Some("tool calls: only in an agent transcript view")
        );
        assert!(
            app.state.flash.is_none(),
            "status bar is behind the scrollback: {:?}",
            app.state.flash
        );
    }

    /// `S` in the task viewer reaches the shared `pause_task`, which flashes the
    /// status bar for `:pause`. The pager keypress re-homes it without the shared
    /// handler needing to know who called it.
    #[test]
    fn task_viewer_pause_message_lands_in_the_pager() {
        let mut app = jump_pager_app();
        if let Some(v) = app.view.pager.as_mut() {
            v.task_id = Some(7);
        }
        app.handle_pager_key(ch('S'));
        assert_eq!(pager(&app).flash.as_deref(), Some("no task with id 7"));
        assert!(
            app.state.flash.is_none(),
            "occluded surface: {:?}",
            app.state.flash
        );
    }

    /// The deliberate exception, and why it needs no per-site opt-out: an action
    /// whose result is on the file list has already closed its pager, so there is
    /// nothing to relocate into and the message stays on the status bar.
    #[test]
    fn a_message_from_an_action_that_closes_the_pager_stays_on_the_status_bar() {
        use crate::app::state::Focus;
        use crate::ui::pager::Mount;

        let mut app = App::test_app(std::env::temp_dir());
        app.state.focus = Focus::Pane;
        let mut sb = PagerView::new_plain("transcript", vec!["line".to_string()]);
        sb.pane_scroll = true;
        sb.mount = Mount::LowerPane;
        app.view.scroll_pager = Some(sb);

        app.handle_pager_key(ch('q'));
        assert!(app.view.scroll_pager.is_none(), "q closed the scrollback");
        assert_eq!(
            app.state.flash.as_ref().map(|f| f.text.as_str()),
            Some("scroll: off"),
            "the pager is gone, so the list's status bar is the right surface"
        );
    }
}

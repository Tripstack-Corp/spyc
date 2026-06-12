//! Prompt-line key handling: the command/search prompt editor, the vi-style
//! prompt editor, and the history-bucket helpers. Split from key_dispatch.rs.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::History;

use crate::app::update::UiMsg;
use crate::app::{
    App, Effect, HistoryBucket, Mode, Prompt, PromptKind, history_bucket_for, is_path_prompt_kind,
};

impl App {
    pub(super) fn handle_prompt_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        // Single-key confirm prompts: `y` / `Y` proceeds, anything else cancels.
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::RemoveConfirm)
        ) {
            return self.handle_remove_confirm_key(key);
        }
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::GraveyardPurgeAllConfirm)
        ) {
            return self.handle_graveyard_purge_all_confirm(key);
        }
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::ClaudeCrashRecover { .. })
        ) {
            return self.handle_claude_crash_recover_key(key);
        }
        // Shell prompts (`!` / `;`) use the vi line editor + history.
        let has_editor = matches!(
            &self.state.mode,
            Mode::Prompting(p) if p.editor.is_some()
        );
        if has_editor {
            return self.handle_vi_prompt_key(key);
        }

        // --- Simple prompts (search, jump, pattern-pick, etc.) ---

        // ^C cancels too (vi muscle memory; same as Esc).
        let ctrl_c =
            matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL);
        // Esc cancels; Backspace on an empty buffer cancels too.
        let backspace_on_empty = matches!(key.code, KeyCode::Backspace)
            && matches!(&self.state.mode, Mode::Prompting(p) if p.buffer.is_empty());
        if matches!(key.code, KeyCode::Esc) || backspace_on_empty || ctrl_c {
            self.cancel_prompt();
            return Vec::new();
        }
        if matches!(key.code, KeyCode::Enter) {
            let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
                return Vec::new();
            };
            // Prompt submission is infallible (update wraps it in Ok).
            return self.update(UiMsg::Prompt(p)).unwrap_or_default();
        }

        // (J's history Up/Down used to live here; v1.33.0 promoted
        // J to a vi-line-editor prompt so handle_vi_prompt_key now
        // owns its history navigation alongside the other vi
        // prompts. Other simple prompts don't have history buckets.)

        // Tab completion.
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('\t')) {
            // Extract the buffer before taking &mut self.
            let buffer = if let Mode::Prompting(p) = &self.state.mode {
                p.buffer.clone()
            } else {
                return Vec::new();
            };
            let is_search = matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(p.kind, PromptKind::Search { .. })
            );
            if is_search {
                if !buffer.is_empty() {
                    self.state.temp_filter = Some(format!("{buffer}*"));
                    self.state.rebuild_rows();
                }
            } else if matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(
                    p.kind,
                    PromptKind::Jump
                        | PromptKind::CopyTo
                        | PromptKind::MoveTo
                        | PromptKind::MakeDir
                        | PromptKind::NewFile
                        | PromptKind::PaneNewTabCwd
                )
            ) {
                self.tab_complete_path();
            }
            return Vec::new();
        }
        self.view.tab_state = None;

        // Edit the buffer. Scoped borrow so we can run search afterwards.
        {
            let Mode::Prompting(prompt) = &mut self.state.mode else {
                return Vec::new();
            };
            match key.code {
                KeyCode::Backspace => {
                    prompt.buffer.pop();
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match c {
                            'u' | 'U' => prompt.buffer.clear(),
                            'w' | 'W' => {
                                while matches!(prompt.buffer.chars().last(), Some(c) if c.is_whitespace())
                                {
                                    prompt.buffer.pop();
                                }
                                while matches!(prompt.buffer.chars().last(), Some(c) if !c.is_whitespace())
                                {
                                    prompt.buffer.pop();
                                }
                            }
                            _ => {}
                        }
                    } else {
                        prompt.buffer.push(c);
                    }
                }
                _ => {}
            }
        }

        // For an active search, re-run the match incrementally against the
        // original cursor position so typing narrows towards a result but
        // backspace widens again.
        let search_info = if let Mode::Prompting(Prompt {
            kind: PromptKind::Search { saved_cursor },
            buffer,
            ..
        }) = &self.state.mode
        {
            Some((*saved_cursor, buffer.clone()))
        } else {
            None
        };
        if let Some((saved, query)) = search_info {
            if query.is_empty() {
                self.state.cursor.index = saved;
            } else if let Some(i) = self.state.find_match(&query, saved, false) {
                self.state.cursor.index = i;
            }
            self.state.cursor.clamp(self.state.rows.len());
        }

        Vec::new()
    }
    /// Return the appropriate history for the current prompt kind.
    /// Four buckets so they don't pollute each other:
    ///   - `pane_history` for new-pane-tab cmd / cwd prompts
    ///   - `jump_history` for the `J` jump-to-path prompt
    ///   - `command_history` for `:` (vim-style command line)
    ///   - `history` for shell-out prompts (`!`, `;`)
    ///
    /// Mixing `:` with `!` was the worst of these collisions: typing
    /// `!make sync-all` then later hitting `:` + Up surfaces
    /// `make sync-all` and submits it as a `:` command, which then
    /// errors with "unknown command".
    /// Resolve a [`HistoryBucket`] to the owning `History`. The split
    /// from [`history_bucket_for`] keeps the kind→bucket decision pure
    /// (and testable) while this side does the `&mut self` projection.
    const fn history_bucket_mut(&mut self, bucket: HistoryBucket) -> &mut History {
        match bucket {
            HistoryBucket::Shell => &mut self.state.history,
            HistoryBucket::PaneCmd => &mut self.state.pane_history,
            HistoryBucket::PaneCwd => &mut self.state.pane_cwd_history,
            HistoryBucket::Jump => &mut self.state.jump_history,
            HistoryBucket::Command => &mut self.state.command_history,
        }
    }

    const fn history_for_prompt(&mut self) -> &mut History {
        let kind = match &self.state.mode {
            Mode::Prompting(p) => Some(&p.kind),
            Mode::Normal => None,
        };
        self.history_bucket_mut(history_bucket_for(kind))
    }

    /// Handle keys for shell prompts that use the vi line editor.
    pub(super) fn handle_vi_prompt_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        use crate::ui::line_edit::EditResult;

        // Tab completion — intercept before feeding to the editor so we
        // don't depend on the editor's Tab handling (which varies by
        // terminal key delivery).
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('\t')) {
            let wants_path = matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(
                    p.kind,
                    PromptKind::Jump
                        | PromptKind::CopyTo
                        | PromptKind::MoveTo
                        | PromptKind::MakeDir
                        | PromptKind::NewFile
                        | PromptKind::PaneNewTabCwd
                        | PromptKind::ShellCmd
                        | PromptKind::ShellCmdCaptured
                        | PromptKind::Command
                )
            );
            if wants_path {
                self.tab_complete_path();
            }
            return Vec::new();
        }
        // Non-Tab clears double-Tab state.
        self.view.tab_state = None;

        // ^C in any prompt cancels and returns to normal mode --
        // vi muscle memory. Distinct from Esc only in keystroke,
        // identical in effect.
        if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.history_for_prompt().reset_nav();
            self.cancel_prompt();
            return Vec::new();
        }

        // `!?` and `J?` — when the buffer is empty and the user
        // types '?', immediately open the matching history popup (no
        // Enter needed). Mirrors spy's `J ?` muscle memory and
        // matches the long-standing `!?` shell-history affordance.
        // For `J`, the popup exists at `show_jump_history_popup` but
        // was previously only reachable via `J <Esc> <Space>` — two
        // prerequisites a spy user is unlikely to know.
        if key.code == KeyCode::Char('?')
            && let Mode::Prompting(Prompt {
                ref kind,
                ref buffer,
                ..
            }) = self.state.mode
            && buffer.is_empty()
        {
            match kind {
                PromptKind::ShellCmdCaptured => {
                    self.state.mode = Mode::Normal;
                    self.show_history_popup();
                    return Vec::new();
                }
                PromptKind::Jump => {
                    self.state.mode = Mode::Normal;
                    self.show_jump_history_popup();
                    return Vec::new();
                }
                _ => {}
            }
        }

        // `<Space>` or `?` in Normal mode opens the history popup. The
        // full sequence is `Esc Space` (or `Esc ?`): first Esc enters
        // Normal mode (the standard vi-line-editor behavior); Space/`?`
        // then asks for the bigger pager view. Reads more naturally
        // than double-Esc and doesn't fight Esc's "back out of
        // something" muscle memory.
        //
        // `?` is included so the `!?` affordance keeps working *after*
        // the user has started browsing history with `Esc k`: the
        // empty-buffer `?` block above only fires on a fresh prompt, so
        // once a command is recalled into the buffer it no longer
        // matches — but in Normal mode `?` is otherwise a no-op in the
        // line editor, so we route it here to the same viewer.
        //
        // Dispatched by prompt kind:
        //   PromptKind::Jump → show_jump_history_popup (j/k cd)
        //   anything else    → show_history_popup (shell !? popup)
        //
        // KNOWN LIMITATION: for `:` (command line) the !? popup
        // shows shell history, not command_history. Tracked in
        // ROADMAP for proper kind-routing.
        if matches!(key.code, KeyCode::Char(' ' | '?')) {
            let in_normal_mode = matches!(
                &self.state.mode,
                Mode::Prompting(p) if p.editor.as_ref().is_some_and(
                    |e| e.mode == crate::ui::line_edit::Mode::Normal
                )
            );
            if in_normal_mode {
                let is_jump = matches!(
                    &self.state.mode,
                    Mode::Prompting(p) if matches!(p.kind, PromptKind::Jump)
                );
                self.state.mode = Mode::Normal;
                if is_jump {
                    self.show_jump_history_popup();
                } else {
                    self.show_history_popup();
                }
                return Vec::new();
            }
        }

        // Feed key to the editor.
        let result = {
            let Mode::Prompting(prompt) = &mut self.state.mode else {
                return Vec::new();
            };
            let editor = prompt.editor.as_mut().expect("checked above");
            let r = editor.feed(key);
            // Sync the buffer for display (prompt.buffer drives rendering).
            prompt.buffer = editor.text();
            r
        };

        match result {
            EditResult::Submit => {
                let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal)
                else {
                    return Vec::new();
                };
                // Push to the appropriate history before dispatching.
                // Buckets stay isolated -- shell, pane command, pane
                // cwd, jump destinations, and `:` commands don't
                // cross-pollute each other's Up/Down browse. Same
                // `history_bucket_for` mapping the browse path uses.
                let hist = self.history_bucket_mut(history_bucket_for(Some(&p.kind)));
                if !p.buffer.trim().is_empty() {
                    hist.push(p.buffer.trim());
                }
                hist.reset_nav();
                // Prompt submission is infallible (update wraps it in Ok).
                return self.update(UiMsg::Prompt(p)).unwrap_or_default();
            }
            EditResult::Cancel => {
                self.history_for_prompt().reset_nav();
                self.cancel_prompt();
            }
            EditResult::HistoryPrev => {
                // Path prompts (copy/move/mkdir) have a vi editor but
                // share the shell-command history slot, which has
                // nothing useful to offer them — surfacing
                // `make sync-all` on Up in a `move to:` prompt was
                // surprising. Skip nav for those kinds; let other
                // shell-style prompts continue to cycle history.
                if is_path_prompt_kind(&self.state.mode) {
                    return Vec::new();
                }
                let current_text = {
                    let Mode::Prompting(p) = &self.state.mode else {
                        return Vec::new();
                    };
                    p.buffer.clone()
                };
                let hist = self.history_for_prompt();
                if let Some(entry) = hist.prev(&current_text) {
                    let entry = entry.to_string();
                    let Mode::Prompting(p) = &mut self.state.mode else {
                        return Vec::new();
                    };
                    if let Some(ed) = p.editor.as_mut() {
                        ed.set_content_keep_mode(&entry);
                    }
                    p.buffer = entry;
                }
            }
            EditResult::HistoryNext => {
                if is_path_prompt_kind(&self.state.mode) {
                    return Vec::new();
                }
                let hist = self.history_for_prompt();
                let replacement = match hist.next() {
                    Some(entry) => entry.to_string(),
                    None => hist.stashed().to_string(),
                };
                let Mode::Prompting(p) = &mut self.state.mode else {
                    return Vec::new();
                };
                if let Some(ed) = p.editor.as_mut() {
                    ed.set_content_keep_mode(&replacement);
                }
                p.buffer = replacement;
            }
            // Tab is intercepted before editor.feed() — this arm is
            // only reachable if the editor somehow returns it.
            EditResult::TabComplete | EditResult::Continue => {}
        }
        Vec::new()
    }
}

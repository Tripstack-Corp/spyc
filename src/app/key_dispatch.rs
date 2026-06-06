//! Keyboard input dispatch: the top-level `handle_key` router and the
//! mode-specific sub-handlers it delegates to — bound-action application
//! (`apply_user`), the prompt editors (`handle_prompt_key`,
//! `handle_vi_prompt_key`), the modal confirm handlers (remove,
//! graveyard purge-all, Claude crash-recover), and `undo_last_remove`.
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2), same child-module
//! `impl App` pattern as render / pager_handler / commands: methods read
//! App's private state via the descendant-module rule. Three are `pub` —
//! the run loop calls `handle_key`, the `apply` action path in actions.rs
//! calls `handle_remove_confirm_key` via a synthetic `y` key, and
//! `commands` calls `undo_last_remove`;
//! the rest stay private. The tab-completion helpers these delegate to
//! (`tab_complete_path` etc.) stay in `app` and resolve via the same rule.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::fs;
use crate::keymap::{Action, BoundAction, ResolverOutcome};
use crate::pane::{Pane, TabEntry, TabInfo};
use crate::shell;
use crate::spyc_debug;
use crate::state::History;

use super::route;
use super::update::UiMsg;
use super::{
    App, Effect, HistoryBucket, Mode, POST_CHORD_BOUNCE_WINDOW, PaneInput, PaneTarget, Prompt,
    PromptKind, View, history_bucket_for, is_path_prompt_kind, is_post_chord_bounce, sh_c,
    strip_ansi_escapes,
};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> Result<Vec<Effect>> {
        // Per-key dispatch trace, opt-in via `--key-trace` / SPYC_KEY_TRACE.
        // Captures the input as it arrives so a user reproducing an
        // "input doesn't work" issue can ship a log. We re-trace the
        // dispatch decision wherever a key gets routed.
        if crate::key_trace::is_enabled() {
            crate::key_trace::log(&format!(
                "RX kind={:?} code={:?} mods={:?} pane_focused={} pending={:?}",
                key.kind,
                key.code,
                key.modifiers,
                self.state.pane_focused(),
                self.state.resolver.pending_display(),
            ));
            // Stamp so any subsequent TX (pty write) logs its
            // latency against this event.
            crate::key_trace::note_rx_event();
        }

        // Swallow a Press/Repeat of the chord-completing key when it
        // arrives within ~60 ms of a focus-switch chord. Without this
        // guard, fast-typing `^a-j` (or holding the chord-completing
        // key even briefly) produces a stray byte to the now-focused
        // pane child — the j Press completes the chord, but a Repeat
        // or too-quick second Press follows with the new focus already
        // active, so it gets forwarded to the pane as raw input.
        // 60 ms covers system-key-repeat (~30-50 ms) and kitty-keyboard
        // Repeat events without affecting deliberate double-taps.
        if is_post_chord_bounce(
            self.view.focus_chord_completed,
            key,
            self.state.resolver.is_pending(),
        ) {
            crate::key_trace::log("  swallowed (post-chord bounce)");
            return Ok(Vec::new());
        }
        // Expire the stamp once its window has passed so it can't
        // suppress a deliberate same-key press later.
        if self
            .view
            .focus_chord_completed
            .is_some_and(|(at, _)| at.elapsed() >= POST_CHORD_BOUNCE_WINDOW)
        {
            self.view.focus_chord_completed = None;
        }

        // Any keypress clears a lingering flash message.
        self.state.flash = None;

        // F-finder is modal: while open, swallow all keys for picker
        // navigation (type-to-filter, Up/Down, Enter, Esc). Runs
        // before the capture / pager / file-list dispatch so the
        // picker can't be accidentally double-routed.
        if self.handle_find_picker_key(key) {
            return Ok(Vec::new());
        }

        // ^C is intentionally a no-op at the spyc-normal level (we
        // don't quit on Ctrl+C, that footgun's too easy with one
        // stray chord). Flash a hint so the user isn't left
        // wondering whether the key got captured -- common after
        // coming back from a `p` → `$PAGER` takeover where they
        // tried to ^C out of less and might have sent a second one
        // in confusion.
        //
        // Exclusions:
        //  - Capture mode forwards ^C to the child as 0x03 below.
        //  - Prompting mode treats ^C as cancel (vi muscle memory:
        //    `^C` in `:` should drop you back to normal mode, same
        //    as Esc) -- handled in `handle_vi_prompt_key`.
        //  - Pane focused: ^C must reach the child (zsh, etc.) so the
        //    user can interrupt a running command. Forwarding happens
        //    at the pane-focused dispatch below.
        //  - Pager open: ^C is contextual to the pager (interrupt a
        //    running task viewer, "process already stopped" hint
        //    when the task is done, "press Esc / q to close" for
        //    other pager views). Without this exclusion the user
        //    sees the spyc-list flash on the *background* status
        //    bar while looking at the pager — wrong screen for the
        //    notice.
        let pane_has_focus = self.runtime.pane_tabs.is_some() && self.state.pane_focused();
        if matches!(key.code, KeyCode::Char('c'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.runtime.pending_capture.is_none()
            && !matches!(self.state.mode, Mode::Prompting(_))
            && !pane_has_focus
            && self.view.pager.is_none()
        {
            self.state.flash_info(
                "^C is not a quit binding — use Q (or :q) to quit, Esc to cancel modes",
            );
            return Ok(Vec::new());
        }

        // While a `!` capture is running, forward typed keys to the
        // child via the master PTY writer so the user can answer
        // prompts (sudo password, ssh password, etc.). Ctrl+\ kills
        // the child outright; Ctrl+C is forwarded as 0x03 so the
        // child's tty driver can deliver SIGINT (matches a normal
        // terminal's behavior, and lets sudo cancel its prompt
        // cleanly).
        if let Some(capture) = &mut self.runtime.pending_capture {
            use std::io::Write as _;
            // Hard-kill escape: Ctrl+\ tears the child down even if
            // it has somehow detached from the controlling tty.
            if matches!(key.code, KeyCode::Char('\\'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                let _ = capture.host.child.kill();
                let _ = capture.host.child.wait();
                // ✗ — interrupted is a non-clean termination, same
                // glyph the bottom-status-bar uses for bg tasks that
                // exited non-zero.
                let title = format!("\u{2717} {} — interrupted", capture.title);
                if let Some(view) = self.view.pager.as_mut() {
                    view.title = title;
                    view.saveable = true;
                    view.streaming = false;
                }
                // MVU Phase 3c: clear the wake slot before dropping the
                // host, so the kill-driven EOF close-wake fires through a
                // None slot rather than spuriously waking the loop for a
                // capture that's already gone.
                capture.host.clear_wake_slot();
                self.runtime.pending_capture = None;
                return Ok(Vec::new());
            }
            // ^Z: send to background. Reader thread keeps draining; the
            // pager closes; user can resume with `:fg`.
            if matches!(key.code, KeyCode::Char('z'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.background_capture();
                return Ok(Vec::new());
            }
            let bytes = crate::pane::input::encode_key(key);
            if !bytes.is_empty() {
                let _ = capture.host.writer.write_all(&bytes);
                let _ = capture.host.writer.flush();
            }
            return Ok(Vec::new());
        }

        // Top overlay: once the subprocess exits, hold the screen until
        // any key so short-lived commands (`;ls`) don't flash and vanish.
        if self.view.overlay_awaiting_dismiss {
            self.runtime.top_overlay = None;
            self.view.overlay_awaiting_dismiss = false;
            self.view.needs_full_repaint = true;
            self.state.flash_info("command finished");
            return Ok(Vec::new());
        }

        // Quick Select picker eats all keys until dismissed.
        // Earlier than the harpoon menu so it'll never collide
        // with chord state.
        if self.view.quick_select.is_some() {
            return Ok(self.handle_quick_select_key(key));
        }

        // Harpoon menu eats all keys until dismissed (Esc/q).
        if self.view.harpoon_menu.is_some() {
            return Ok(self.handle_harpoon_menu_key(key));
        }

        // Key routing. The destination decision lives in
        // `route::route_key` — a pure function over a small state
        // snapshot. Five separate routing-shape bugs shipped within
        // a week (#75, #78, #80, #81, plus the original V-key bug)
        // because every routing site reinvented the (focus, mount,
        // key-type) decision. The router collapses those guards into
        // one place; each destination here is a thin dispatch arm.
        // See `src/app/route.rs` for the routing rules and the test
        // matrix encoding the five regression cases.
        let snap = self.route_snapshot();
        match route::route_key(snap, key) {
            route::KeyDestination::OverlayPty => {
                // Forward the keystroke to the overlay pty via the sole
                // executor (no flash — result was always ignored).
                return Ok(vec![Effect::SendToPane {
                    target: PaneTarget::Overlay,
                    input: PaneInput::Key(key),
                    on_ok: None,
                    err_prefix: None,
                }]);
            }
            route::KeyDestination::PagerKey => {
                return Ok(self.handle_pager_key(key));
            }
            route::KeyDestination::PaneScroll => {
                return Ok(self.handle_pane_scroll_key(key));
            }
            route::KeyDestination::PaneExitedFlash => {
                self.state
                    .flash_info("pane exited — `^a-R` to restart, `^a-x` to close");
                return Ok(Vec::new());
            }
            route::KeyDestination::BottomPane => {
                // Track what the user types so `yP` can yank the
                // last prompt.
                match key.code {
                    KeyCode::Enter => {
                        let trimmed = strip_ansi_escapes(&self.state.pane_prompt_buf);
                        if !trimmed.is_empty() {
                            self.state.last_pane_prompt = Some(trimmed);
                        }
                        self.state.pane_prompt_buf.clear();
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.pane_prompt_buf.clear();
                    }
                    KeyCode::Backspace => {
                        self.state.pane_prompt_buf.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.pane_prompt_buf.push(c);
                    }
                    _ => {}
                }
                // Forward the keystroke to the active pane via the sole
                // executor (no flash — result was always ignored). The
                // `pane_prompt_buf` tracking above stays as pure
                // transitions before the emit.
                return Ok(vec![Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::Key(key),
                    on_ok: None,
                    err_prefix: None,
                }]);
            }
            route::KeyDestination::Prompt => {
                return Ok(self.handle_prompt_key(key));
            }
            route::KeyDestination::Resolver => {
                // Fall through to the inventory/graveyard view
                // special cases and the resolver invocation below.
            }
        }
        // Inventory view: special key handling.
        if self.state.view == View::Inventory {
            match key.code {
                KeyCode::Esc => {
                    self.state.toggle_inventory_view();
                    return Ok(Vec::new());
                }
                KeyCode::Char('x' | 'd') => {
                    self.state.drop_cursor();
                    return Ok(Vec::new());
                }
                KeyCode::Char(' ' | 't') => {
                    self.state.inventory.toggle_pick(self.state.cursor.index);
                    self.state.list_generation = self.state.list_generation.wrapping_add(1);
                    let rpc = self.state.grid_dims.rows_per_col as usize;
                    self.state
                        .cursor_move_vertical(1, rpc, self.state.rows.len());
                    return Ok(Vec::new());
                }
                KeyCode::Char('p') => {
                    return Ok(self.put_inventory_to_cwd());
                }
                _ => {}
            }
        }
        // Graveyard view: special key handling. Same shape as
        // inventory; verbs are restore/purge instead of put/tag.
        // `dd` (vim-style two-key delete) is implemented via the
        // pager's `d` already being free here — second `d` confirms.
        if self.state.view == View::Graveyard {
            return Ok(self.handle_graveyard_view_key(key));
        }
        let outcome = self.state.resolver.feed(key, &self.state.user_keymap);
        if crate::key_trace::is_enabled() {
            crate::key_trace::log(&format!("  resolver -> {outcome:?}"));
        }
        match outcome {
            ResolverOutcome::Action(action) => {
                // Stamp focus-switch chord completions so the next
                // ~60 ms suppresses a same-key Repeat or bouncy second
                // Press from leaking into the now-focused pane.
                if matches!(action, Action::PaneFocusDown | Action::PaneFocusUp) {
                    self.view.focus_chord_completed = Some((std::time::Instant::now(), key.code));
                }
                return self.update(UiMsg::Action(action));
            }
            ResolverOutcome::User(bound) => return self.update(UiMsg::Bound(bound)),
            ResolverOutcome::Pending | ResolverOutcome::Ignored => {}
        }
        Ok(Vec::new())
    }

    /// MVU Phase 6 PR-C: route a bracketed-paste event. Into the active prompt
    /// buffer/editor when prompting; else to the `V`/`D` top-overlay subprocess
    /// (unless the bottom pane is explicitly focused); else to the active pane
    /// (focusing it + tracking the text for `yP`), wrapped in bracketed-paste
    /// markers; else flash a "nowhere to paste" hint. Verbatim move of the
    /// loop's `Event::Paste` arm.
    pub(crate) fn handle_paste(&mut self, text: String) -> Result<()> {
        if crate::key_trace::is_enabled() {
            crate::key_trace::log(&format!(
                "RX paste len={} pane_focused={} mode={:?}",
                text.len(),
                self.state.pane_focused(),
                std::mem::discriminant(&self.state.mode),
            ));
        }
        if let Mode::Prompting(ref mut p) = self.state.mode {
            // Paste into the active prompt buffer. Strip newlines (prompts
            // are single-line).
            let clean = text.replace(['\n', '\r'], " ");
            if let Some(ed) = p.editor.as_mut() {
                // Editor present (`!` / `;` / `:`): splice at the cursor so a
                // mid-line paste lands where the user is, then sync the
                // canonical buffer from the editor.
                ed.insert_str(&clean);
                p.buffer = ed.text();
            } else {
                // Simple prompt (search, mkdir, etc.) -- no cursor, append.
                p.buffer.push_str(&clean);
            }
        } else if let Some(overlay) = self
            .runtime
            .top_overlay
            .as_mut()
            .filter(|_| !(self.runtime.pane_tabs.is_some() && self.state.pane_focused()))
        {
            // `V`/`D` top-overlay is the foreground subprocess (editor or
            // pager). Route the paste to it rather than the bottom pane — the
            // bottom pane only wins when explicitly focused (`^a-j`); without
            // this guard, pasting into a `V`-launched $EDITOR sent the text to
            // claude *and* yanked focus there. Don't steal focus here.
            let mut buf = Vec::with_capacity(text.len() + 12);
            buf.extend_from_slice(b"\x1b[200~");
            buf.extend_from_slice(text.as_bytes());
            buf.extend_from_slice(b"\x1b[201~");
            overlay.send_bytes(&buf)?;
        } else if self.runtime.pane_tabs.is_some() {
            // Switch focus to the pane — the user clearly intends to interact
            // with it if they're pasting.
            if !self.state.pane_focused() {
                self.set_pane_focus(true);
            }
            // Track pasted text for yP (yank last prompt).
            self.state.pane_prompt_buf.push_str(&text);
            // Wrap in bracketed paste so the child app (e.g. claude) receives
            // the block as a single paste, not line-by-line.
            let pane = self.runtime.pane_tabs.as_mut().unwrap().active_mut();
            let mut buf = Vec::with_capacity(text.len() + 12);
            buf.extend_from_slice(b"\x1b[200~");
            buf.extend_from_slice(text.as_bytes());
            buf.extend_from_slice(b"\x1b[201~");
            pane.send_bytes(&buf)?;
        } else {
            // No prompt and no pane — nowhere sensible to send it. Some
            // terminals wrap rapid-fire keystrokes in bracketed paste, so
            // silently dropping could swallow real input; flash a hint.
            let n = text.chars().count();
            self.state.flash_info(format!(
                "paste ignored ({n} chars) — open `:` or `^\\` to paste"
            ));
        }
        Ok(())
    }

    /// MVU Phase 6 PR-C: handle a terminal resize — immediately resize all pty
    /// tabs + the top overlay so child shells re-render at the correct width,
    /// and rebuild the help overlay (its wrap points are baked at open time).
    /// Verbatim move of the loop's `Event::Resize` arm.
    pub(crate) fn handle_resize(&mut self, cols: u16, rows: u16) {
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let pane_pct = self.effective_pane_pct();
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            let layout = Self::compute_layout(
                area,
                true,
                pane_pct,
                self.state.config.layout.status_position,
            );
            if let Some(pane_rect) = layout.pane {
                for entry in tabs.tabs_mut() {
                    let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
                }
            }
        }
        if let Some(overlay) = self.runtime.top_overlay.as_mut() {
            let (r, c) = Self::top_overlay_size(pane_pct, self.runtime.pane_tabs.is_some());
            let _ = overlay.resize(r, c);
        }
        // Help content is baked at open time for the current width (wrap
        // points, column count). Rebuild so it matches the new dimensions.
        if self.help_is_open() {
            self.open_help();
        }
    }

    /// Dispatch a user-defined binding. Inline-data actions (unix command,
    /// preset pattern, preset path) run through the same machinery as the
    /// built-in prompts but skip the prompt UI.
    pub(super) fn apply_user(&mut self, bound: &BoundAction) -> Result<Vec<Effect>> {
        match bound {
            BoundAction::Plain(action) => return self.apply(action),
            BoundAction::UnixCmd(template) => {
                let cmd = shell::expand_percent(template, &self.state.selection_paths());
                return Ok(sh_c(&cmd, true));
            }
            BoundAction::PatternPick(pattern) => {
                if let Ok(pat) = glob::Pattern::new(pattern) {
                    for e in &self.state.listing.entries {
                        if pat.matches(&e.name) {
                            self.state.picks.insert(&e.path);
                        }
                    }
                    self.state.list_generation = self.state.list_generation.wrapping_add(1);
                }
            }
            BoundAction::Jump(path) => {
                let _ = self.state.jump_to(path);
            }
            BoundAction::Copy(dest) => {
                self.run_selection_to(dest, fs::ops::copy_selection_to, "copied");
            }
            BoundAction::Move(dest) => {
                self.run_selection_to(dest, fs::ops::move_selection_to, "moved");
            }
            BoundAction::ToggleMaskFixed(n) => {
                if *n == 1 {
                    self.state.masks.toggle_mask1();
                } else if *n == 2 {
                    self.state.masks.toggle_mask2();
                }
                self.state.rebuild_rows();
            }
        }
        self.state.cursor.clamp(self.state.rows.len());
        Ok(Vec::new())
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> Vec<Effect> {
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
            // Extract kind and buffer before taking &mut self.
            let (_kind, buffer) = if let Mode::Prompting(p) = &self.state.mode {
                (std::mem::discriminant(&p.kind), p.buffer.clone())
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

    /// Single-key confirmation for `R`. `y` / `Y` triggers the delete;
    /// anything else — including Enter, Esc, or any other letter — cancels.
    /// The prompt closes in every case.
    pub fn handle_remove_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        // Pull the targeted paths out of the preview slot. This is
        // the authoritative list (it was computed at prompt time so
        // `Ndd` ignores any cursor wiggle that might have happened);
        // selection_paths() would re-derive from current state and
        // could disagree.
        let preview = self.state.pending_delete_preview.take();
        if !confirmed {
            return Vec::new();
        }
        let paths: Vec<&Path> = preview.as_ref().map_or_else(
            || self.state.selection_paths(),
            |v| v.iter().map(PathBuf::as_path).collect(),
        );
        if paths.is_empty() {
            return Vec::new();
        }
        // Route through the graveyard: archive each path into
        // `<uuid>.tar.zst` first, then unlink the source. If the
        // archive step fails for any path we skip the unlink for
        // *that* path and surface a clear error — the user keeps
        // the file. Per-path failures don't stop the rest of the
        // batch; we report the count at the end.
        let mut archived = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for p in &paths {
            match crate::state::graveyard::Graveyard::write_entry(p) {
                Ok(_entry) => match fs::ops::remove_tree(p) {
                    Ok(()) => archived += 1,
                    Err(e) => {
                        failures.push(format!("{}: archived but unlink failed: {e}", p.display()));
                    }
                },
                Err(e) => {
                    // Archive failed — fall back to a hard delete
                    // would surprise the user (they expect undo);
                    // instead, leave the file alone and report.
                    failures.push(format!(
                        "{}: graveyard archive failed: {e} — file NOT removed",
                        p.display()
                    ));
                }
            }
        }
        if failures.is_empty() {
            self.state
                .flash_info(format!("removed {archived} item(s) (recoverable: gy)"));
        } else {
            // First failure goes in the flash; remainder in debug log.
            self.state.flash_error(failures[0].clone());
            for msg in &failures[1..] {
                spyc_debug!("R: {msg}");
            }
        }
        self.state.picks.clear();
        self.state.refresh_listing();
        Vec::new()
    }

    /// `:undo` — restore the most-recent graveyard entry to its
    /// original path. Best-effort recovery for the very common
    /// "I just deleted the wrong thing" case. If the original
    /// path is occupied (rare; user recreated it), tar's
    /// `set_overwrite(false)` errors and we surface that — the
    /// user can open `gy` and pick `p` to restore-to-cwd instead.
    pub fn undo_last_remove(&mut self) {
        let g = crate::state::graveyard::Graveyard::load();
        let Some(latest) = g.entries.into_iter().next() else {
            self.state.flash_info("undo: graveyard is empty");
            return;
        };
        let dest = latest.orig_path.parent().map_or_else(
            || std::path::PathBuf::from("/"),
            std::path::Path::to_path_buf,
        );
        match crate::state::graveyard::Graveyard::restore(&latest, &dest) {
            Ok(()) => {
                crate::state::graveyard::Graveyard::delete_entry(&latest);
                self.state.flash_info(format!(
                    "undo: restored {} → {}",
                    latest.filename,
                    dest.display()
                ));
                if matches!(self.state.view, View::Graveyard) {
                    self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                    self.state.cursor.clamp(self.state.graveyard.len());
                    self.state.rebuild_rows();
                }
                self.state.refresh_listing();
            }
            Err(e) => self
                .state
                .flash_error(format!("undo: {e} — try `gy` then `p` to restore to cwd")),
        }
    }

    /// Single-key confirmation for "purge ALL graveyard entries to
    /// system trash". Bound on `Z` from the graveyard view; routes
    /// to a separate prompt kind so the wording stays accurate.
    fn handle_graveyard_purge_all_confirm(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        if !confirmed {
            return Vec::new();
        }
        let mut trashed = 0usize;
        let mut errors = 0usize;
        for entry in self.state.graveyard.clone() {
            match crate::state::graveyard::Graveyard::cascade_entry_to_trash(&entry) {
                Ok(()) => trashed += 1,
                Err(_) => errors += 1,
            }
        }
        if errors > 0 {
            self.state
                .flash_error(format!("graveyard: trashed {trashed}, {errors} failed"));
        } else {
            self.state
                .flash_info(format!("graveyard: trashed {trashed} item(s)"));
        }
        self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
        self.state.cursor.clamp(self.state.graveyard.len());
        self.state.rebuild_rows();
        Vec::new()
    }

    /// Single-key confirmation for the auto-fired claude crash recovery
    /// prompt. `y` / `Y` / Enter kills the broken tab and replaces it with
    /// a fresh `claude` (the user can then `/resume` manually); anything
    /// else kills it and removes the tab so the dump is off-screen.
    fn handle_claude_crash_recover_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter);
        let prev_mode = std::mem::replace(&mut self.state.mode, Mode::Normal);
        let Mode::Prompting(Prompt {
            kind: PromptKind::ClaudeCrashRecover { tab_idx },
            ..
        }) = prev_mode
        else {
            return Vec::new();
        };

        // Snapshot cwd + fallback from the tab and best-effort kill the
        // child (bunfs claude is often still alive post-crash; an
        // already-closed pane errors here, ignored).
        let Some((cwd, fallback)) = self.runtime.pane_tabs.as_mut().and_then(|tabs| {
            let entry = tabs.tabs_mut().get_mut(tab_idx)?;
            entry.pane.try_kill();
            let fallback = entry
                .info
                .restore_fallback
                .clone()
                .unwrap_or_else(|| "claude".to_string());
            Some((entry.info.cwd.clone(), fallback))
        }) else {
            return Vec::new();
        };

        if !confirmed {
            if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                let still_have_tabs = tabs.remove_at(tab_idx);
                if !still_have_tabs {
                    self.runtime.pane_tabs = None;
                }
            }
            self.state.flash_info("claude crash dismissed; tab closed");
            self.view.needs_full_repaint = true;
            return Vec::new();
        }

        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        match Pane::spawn_with_env(
            &fallback,
            rows,
            cols,
            &cwd,
            &self.view.context_path,
            &[],
            wake,
        ) {
            Ok(p) => {
                let entry = TabEntry::new(p, TabInfo::new(&fallback, &cwd));
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.replace_at(tab_idx, entry);
                }
                self.state
                    .flash_info("started fresh claude — type /resume to recover");
            }
            Err(e) => self.state.flash_error(format!("claude spawn failed: {e}")),
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
    fn handle_vi_prompt_key(&mut self, key: KeyEvent) -> Vec<Effect> {
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

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

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::fs;
use crate::keymap::{Action, BoundAction, ResolverOutcome};
use crate::shell;

use super::route;
use super::update::UiMsg;
use super::{
    App, Effect, Mode, POST_CHORD_BOUNCE_WINDOW, PaneInput, PaneTarget, View, is_post_chord_bounce,
    sh_c, strip_ansi_escapes,
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
                        let trimmed = strip_ansi_escapes(&self.state.pane.pane_prompt_buf);
                        if !trimmed.is_empty() {
                            self.state.pane.last_pane_prompt = Some(trimmed);
                        }
                        self.state.pane.pane_prompt_buf.clear();
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.pane.pane_prompt_buf.clear();
                    }
                    KeyCode::Backspace => {
                        self.state.pane.pane_prompt_buf.pop();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.pane.pane_prompt_buf.push(c);
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
            self.state.pane.pane_prompt_buf.push_str(&text);
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
}

mod confirms;
mod prompts;

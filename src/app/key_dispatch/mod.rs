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

use crate::keymap::{Action, BoundAction, ResolverOutcome};
use crate::shell;

use super::route;
use super::update::UiMsg;
use super::{App, Effect, Mode, PaneInput, PaneTarget, View, sh_c, strip_ansi_escapes};

/// Wrap `text` in bracketed-paste markers so the receiving child app (claude,
/// an editor, …) sees it as one paste block rather than line-by-line.
///
/// Any bracketed-paste markers already inside `text` are stripped first: a
/// paste containing a literal `\x1b[201~` would otherwise close the block
/// early, and the child would interpret the tail as keystrokes/commands
/// (paste injection — the reason terminals sanitize paste content too).
fn bracketed_paste(text: &str) -> Vec<u8> {
    let cleaned = text.replace("\u{1b}[200~", "").replace("\u{1b}[201~", "");
    let mut buf = Vec::with_capacity(cleaned.len() + 12);
    buf.extend_from_slice(b"\x1b[200~");
    buf.extend_from_slice(cleaned.as_bytes());
    buf.extend_from_slice(b"\x1b[201~");
    buf
}

/// Encode a paste for a pane child. Wrap in bracketed-paste markers only when
/// the child has the mode on (`child_bracketed`) — otherwise send raw, the way
/// the `Capture` sink already does: a shell that never enabled DECSET 2004
/// (a remote `/bin/sh` over ssh, `csh`) takes the literal `\e[200~`/`\e[201~`
/// bytes as input and runs a multi-line paste line-by-line.
fn paste_bytes_for_pane(text: &str, child_bracketed: bool) -> Vec<u8> {
    if child_bracketed {
        bracketed_paste(text)
    } else {
        text.as_bytes().to_vec()
    }
}

/// What a key destined for the bottom pane should do, w.r.t. `^z` job control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneKeyAction {
    /// `^z` on a tab whose child has no job control of its own — toggle
    /// spyc-managed suspend/resume.
    Toggle,
    /// A non-`^z` key while the tab is suspended — swallow it (the child is
    /// stopped; forwarding would queue keystrokes that "type" on resume).
    EatSuspended,
    /// Forward to the pty as normal (the common case + a shell's own `^z`).
    Forward,
}

/// Decide a bottom-pane key's `^z` job-control fate. **Pure** (route.rs/focus.rs
/// template): `^z` toggles a spyc-managed suspend on every tab EXCEPT a shell
/// one, where it forwards so the shell backgrounds its own foreground job; a
/// suspended tab swallows every other key. Toggle covers both directions:
/// `toggle_pane_suspend` suspends when running and resumes when suspended.
///
/// The carve-out is the shell, not "everything but an agent": `toggle_pane_suspend`
/// signals the pty's foreground process group, which works for any child. A
/// full-screen TUI (`htop`, `rmatrix`, `vim`) runs with `ISIG` off, so the
/// forwarded `^z` reached it as a literal byte it discards — the key read as
/// dead on every non-agent tab.
const fn pane_suspend_key_action(key: KeyEvent, is_shell: bool, suspended: bool) -> PaneKeyAction {
    let is_ctrl_z =
        matches!(key.code, KeyCode::Char('z')) && key.modifiers.contains(KeyModifiers::CONTROL);
    if is_ctrl_z && !is_shell {
        PaneKeyAction::Toggle
    } else if suspended {
        PaneKeyAction::EatSuspended
    } else {
        PaneKeyAction::Forward
    }
}

/// What a key does to a running `!` capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureKeyAction {
    /// `^C` — interrupt the capture (SIGINT to its foreground process group).
    Interrupt,
    /// `^\` — hard-kill the capture (SIGKILL) and tear it down.
    Kill,
    /// `^Z` — send it to the background.
    Background,
    /// Anything else — forward the keystroke to the child as bytes, so the
    /// user can answer prompts (a sudo/ssh password, a `y`/`n`).
    Forward,
}

/// Classify a key for a live `!` capture. **Pure** (route.rs template): the
/// three control keys are spyc-managed, everything else forwards to the child.
const fn capture_key_action(key: KeyEvent) -> CaptureKeyAction {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => CaptureKeyAction::Interrupt,
        KeyCode::Char('\\') if ctrl => CaptureKeyAction::Kill,
        KeyCode::Char('z') if ctrl => CaptureKeyAction::Background,
        _ => CaptureKeyAction::Forward,
    }
}

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> Result<Vec<Effect>> {
        // A mouse text selection is a transient, pointer-made thing: the next
        // keyboard action retires it, the way a terminal's own selection clears when
        // you type. Both of these hold POSITIONS (list row indices, pane screen
        // cells) rather than content, so leaving one up after the content moves
        // highlights whatever now occupies those positions — a stale band on an
        // unrelated file after a chdir, which is how this was reported.
        //
        // The pager's `visual` selection is deliberately NOT cleared here: it is
        // modal by request (vim-consistent), so `j`/`k` extend it and `Esc` cancels.
        self.view.list_selection = None;
        self.view.pane_selection = None;
        self.view.chrome_selection = None;

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
        if super::util::is_post_chord_bounce(
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
            .is_some_and(|(at, _)| at.elapsed() >= super::util::POST_CHORD_BOUNCE_WINDOW)
        {
            self.view.focus_chord_completed = None;
        }

        // Any keypress clears a lingering flash message.
        self.state.flash = None;
        // Per-turn; the pager sink re-arms it (#166 flash relocation).
        self.view.input_went_to_pager = false;

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

        // Input routing. The destination decision lives in
        // `route::route_input` — a pure function over a small state
        // snapshot covering BOTH the modal overlays (finder, capture,
        // overlay-dismiss, quick-select, harpoon — which eat all input)
        // and the content destinations. Several routing-shape bugs
        // shipped within a week (#75, #78, #80, #81, plus the original
        // V-key bug) because every routing site reinvented the
        // (focus, mount, key-type) decision; the router collapses those
        // guards into one place and each sink here is a thin dispatch
        // arm. `handle_paste` matches the same `InputSink` exhaustively,
        // so keys and paste cannot drift. (The `^C` flash above stays
        // inline — it's a key-only side effect on the Resolver path, not
        // a sink; its guards already exclude every modal/content context,
        // and a paste never triggers it.) See `src/app/route.rs`.
        let snap = self.route_snapshot();
        match route::route_input(snap, route::InputKind::Key(key)) {
            // ── modal sinks: eat all input, checked before content ──
            route::InputSink::FindPicker => {
                // The finder always mounts its pager, so the `^C` flash
                // above (gated on `pager.is_none()`) never pre-empts it.
                self.handle_find_picker_key(key);
                return Ok(Vec::new());
            }
            route::InputSink::Capture => return Ok(self.handle_capture_key(key)),
            route::InputSink::OverlayDismiss => {
                self.dismiss_overlay();
                return Ok(Vec::new());
            }
            route::InputSink::QuickSelect => return Ok(self.handle_quick_select_key(key)),
            route::InputSink::Harpoon => return Ok(self.handle_harpoon_menu_key(key)),
            route::InputSink::ImageGallery => return Ok(self.handle_image_gallery_key(key)),
            route::InputSink::ImageView => return Ok(self.handle_image_view_key(key)),
            // ── content sinks ──
            route::InputSink::OverlayPty => {
                // Forward the keystroke to the overlay pty via the sole
                // executor (no flash — result was always ignored).
                return Ok(vec![Effect::SendToPane {
                    target: PaneTarget::Overlay,
                    input: PaneInput::Key(key),
                    on_ok: None,
                    err_prefix: None,
                }]);
            }
            route::InputSink::PagerKey => {
                return Ok(self.handle_pager_key(key));
            }
            route::InputSink::PaneScroll => {
                return Ok(self.handle_pane_scroll_key(key));
            }
            route::InputSink::PaneExitedFlash => {
                self.state
                    .flash_info("pane exited — `^a-R` to restart, `^a-x` to close");
                return Ok(Vec::new());
            }
            route::InputSink::BottomPane => {
                // `^z` job-control for the bottom pane, decided purely (tested):
                // `^z` toggles a spyc-managed suspend/resume on any non-shell tab
                // (so claude can't self-suspend and trip the macOS false-exit, and
                // so a raw-mode TUI that ignores the byte still suspends); while
                // suspended every other key is swallowed (the child is stopped —
                // forwarding would queue keystrokes that "type" on resume); a
                // shell tab forwards so its own job control keeps working. Meta
                // chords never reach here (they route to the resolver), so
                // `^a-x` still closes a suspended tab.
                let (is_shell, suspended) =
                    self.runtime.pane_tabs.as_ref().map_or((true, false), |t| {
                        let info = t.active_info();
                        (
                            crate::shell::command_is_shell(&info.command),
                            info.suspended,
                        )
                    });
                match pane_suspend_key_action(key, is_shell, suspended) {
                    PaneKeyAction::Toggle => return Ok(self.toggle_pane_suspend()),
                    PaneKeyAction::EatSuspended => {
                        self.state.flash_info("pane suspended — ^z to resume");
                        return Ok(Vec::new());
                    }
                    PaneKeyAction::Forward => {}
                }
                // The agent reads the system clipboard itself on its paste key
                // (the terminal can't carry image bytes), so this is spyc's one
                // chance to keep a copy of what it just took. Purely additive:
                // the key still forwards below, unmodified.
                let capture = self.plan_clipboard_capture(key);
                // Track what the user types so `yP` can yank the
                // last prompt.
                match key.code {
                    KeyCode::Enter => {
                        let trimmed = strip_ansi_escapes(&self.state.pane.pane_prompt_buf);
                        if !trimmed.is_empty() {
                            self.state.pane.last_pane_prompt = Some(trimmed);
                        }
                        self.state.pane.pane_prompt_buf.clear();
                        // Submitting hands the images to the agent, whose
                        // transcript becomes the record — so the uncommitted
                        // ring for this tab is done. Same signal the prompt
                        // buffer uses, deliberately: one notion of "sent".
                        self.clear_pending_images_for_active_tab();
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
                // transitions before the emit. Any clipboard capture rides
                // ALONGSIDE the forward, never instead of it — the agent must
                // still receive the paste it asked for.
                let mut effects = vec![Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::Key(key),
                    on_ok: None,
                    err_prefix: None,
                }];
                effects.extend(capture);
                return Ok(effects);
            }
            route::InputSink::Prompt => {
                return Ok(self.handle_prompt_key(key));
            }
            route::InputSink::Resolver => {
                // Fall through to the inventory/graveyard view
                // special cases and the resolver invocation below.
            }
        }
        // Inventory view: special key handling. The verb arms require a
        // *bare* key (no Ctrl/Alt) — matching `key.code` alone let `^d`/`^x`
        // (common half-page-scroll chords) hit the `Char('x'|'d')` arm and
        // destructively drop the cursor item. A modified key falls through to
        // the resolver instead.
        if self.state.cur().view == View::Inventory {
            let bare = !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
            match key.code {
                KeyCode::Esc => {
                    self.state.toggle_inventory_view();
                    return Ok(Vec::new());
                }
                KeyCode::Char('x' | 'd') if bare => {
                    return Ok(self.state.drop_cursor());
                }
                KeyCode::Char(' ' | 't') if bare => {
                    self.state
                        .inventory
                        .toggle_pick(self.state.cur().cursor.index);
                    self.state.cur_mut().list_generation =
                        self.state.cur().list_generation.wrapping_add(1);
                    let rpc = self.state.cur().grid_dims.rows_per_col as usize;
                    self.state
                        .cursor_move_vertical(1, rpc, self.state.cur().rows.len());
                    return Ok(Vec::new());
                }
                KeyCode::Char('p') if bare => {
                    return Ok(self.put_inventory_to_cwd());
                }
                _ => {}
            }
        }
        // Graveyard view: special key handling. Same shape as
        // inventory; verbs are restore/purge instead of put/tag.
        // `dd` (vim-style two-key delete) is implemented via the
        // pager's `d` already being free here — second `d` confirms.
        if self.state.cur().view == View::Graveyard {
            return Ok(self.handle_graveyard_view_key(key));
        }
        let outcome = self.state.resolver.feed(key, &self.state.user_keymap);
        if crate::key_trace::is_enabled() {
            crate::key_trace::log(&format!("  resolver -> {outcome:?}"));
        }
        match outcome {
            ResolverOutcome::Action(action) => {
                self.clear_chord_hint();
                // Stamp focus-switch chord completions so the next
                // ~60 ms suppresses a same-key Repeat or bouncy second
                // Press from leaking into the now-focused pane.
                if matches!(action, Action::PaneFocusDown | Action::PaneFocusUp) {
                    self.view.focus_chord_completed = Some((std::time::Instant::now(), key.code));
                }
                return self.update(UiMsg::Action(action));
            }
            ResolverOutcome::User(bound) => {
                self.clear_chord_hint();
                return self.update(UiMsg::Bound(bound));
            }
            ResolverOutcome::Pending => {
                // A chord prefix is armed — schedule the which-key hint popup
                // (unless disabled, or this is just a count prefix, which is
                // `Pending` but not a chord). `settle_chord_hint` shows it if
                // the chord is still pending when the delay elapses.
                let delay = self.state.config.layout.chord_hint_delay_ms;
                if delay > 0 && self.state.resolver.is_pending() {
                    self.view.chord_hint_due =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(delay));
                    self.view.chord_hint = None;
                } else {
                    self.clear_chord_hint();
                }
            }
            ResolverOutcome::Ignored => self.clear_chord_hint(),
        }
        Ok(Vec::new())
    }

    /// Tear down the which-key chord-hint popup and its pending timer. If a
    /// popup was actually on screen, request a full repaint so its overlay
    /// cells are cleared from underneath (the overlay-dismiss convention).
    fn clear_chord_hint(&mut self) {
        self.view.chord_hint_due = None;
        if self.view.chord_hint.take().is_some() {
            self.view.needs_full_repaint = true;
        }
    }

    /// Handle a key for a live `!` capture: `^C` → SIGINT, `^\` → SIGKILL +
    /// teardown, `^Z` → background, else forward the bytes so prompts get
    /// answered. Classification is the pure [`capture_key_action`]; reached via
    /// the `InputSink::Capture` arm, which guarantees `pending_capture` is set.
    fn handle_capture_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(capture) = self.runtime.pending_capture.as_mut() else {
            return Vec::new();
        };
        let forward = |key| {
            // A capture is a bare `PtyHost` with no vt100 emulator, so no DECCKM
            // state exists to honour — the CSI form is what a terminal that never
            // negotiated the mode sends.
            let bytes = crate::pane::input::encode_key(key, false);
            if bytes.is_empty() {
                Vec::new()
            } else {
                vec![Effect::SendToCapture { bytes }]
            }
        };
        match capture_key_action(key) {
            CaptureKeyAction::Interrupt => {
                #[cfg(unix)]
                {
                    capture
                        .host
                        .foreground_pgrp()
                        .or_else(|| capture.host.process_id())
                        .map(|pgrp| {
                            vec![Effect::SignalCapture {
                                pgrp,
                                sig: rustix::process::Signal::INT,
                            }]
                        })
                        .unwrap_or_default()
                }
                // No signal path off-unix; forward the 0x03 byte as before.
                #[cfg(not(unix))]
                {
                    forward(key)
                }
            }
            CaptureKeyAction::Kill => {
                // Also kill the command's own foreground group: job control
                // under `zsh -i -c` runs it apart from the wrapper shell that
                // Drop/`child.kill()` target, which would otherwise orphan it.
                #[cfg(unix)]
                let fg_pgrp = capture
                    .host
                    .foreground_pgrp()
                    .filter(|&fg| Some(fg) != capture.host.process_id());
                let _ = capture.host.child.kill();
                let _ = capture.host.child.wait();
                // ✗ — same non-clean-termination glyph the status bar uses.
                let title = format!("\u{2717} {} — interrupted", capture.title);
                if let Some(view) = self.view.pager.as_mut() {
                    view.title = title;
                    view.saveable = true;
                    view.streaming = false;
                }
                // Clear the wake slot before dropping the host so the EOF
                // close-wake fires through a None slot.
                capture.host.clear_wake_slot();
                self.runtime.pending_capture = None;
                #[cfg(unix)]
                if let Some(pgrp) = fg_pgrp {
                    return vec![Effect::SignalCapture {
                        pgrp,
                        sig: rustix::process::Signal::KILL,
                    }];
                }
                Vec::new()
            }
            CaptureKeyAction::Background => {
                self.background_capture();
                Vec::new()
            }
            CaptureKeyAction::Forward => forward(key),
        }
    }

    /// Tear down a top-overlay subprocess that has exited and is being
    /// held on screen awaiting any input (so short-lived commands like
    /// `;ls` don't flash and vanish). Reached via the
    /// `InputSink::OverlayDismiss` dispatch arm.
    fn dismiss_overlay(&mut self) {
        self.runtime.top_overlay = None;
        self.view.overlay_awaiting_dismiss = false;
        self.view.overlay_auto_dismiss = false;
        self.view.overlay_column = None;
        self.view.needs_full_repaint = true;
        // The overlay just closed — drop the stale `Overlay` focus this frame.
        self.recompute_focus();
        self.state.flash_info("command finished");
    }

    /// Route a bracketed-paste event through the SAME authority as keys
    /// (`route_input`), matching `InputSink` exhaustively so paste and keys
    /// cannot diverge: a paste lands wherever a non-meta key would. The leaf
    /// behaviour differs per kind (a paste is a block; a key is one stroke),
    /// but the routing is shared. Returns effects for the sole executor
    /// (`run_effects`); a paste is infallible, so no `Result` (unlike
    /// `handle_key`, which can fail inside `update`).
    pub(crate) fn handle_paste(&mut self, text: String) -> Vec<Effect> {
        if crate::key_trace::is_enabled() {
            crate::key_trace::log(&format!(
                "RX paste len={} pane_focused={} mode={:?}",
                text.len(),
                self.state.pane_focused(),
                std::mem::discriminant(&self.state.mode),
            ));
        }
        let snap = self.route_snapshot();
        match route::route_input(snap, route::InputKind::Paste) {
            // ── modal sinks ──
            route::InputSink::FindPicker => {
                // A paste while the F-finder is open is a filename to filter
                // by, not input for the bottom pane. Strip newlines: the query
                // is a single-line fuzzy filter over paths.
                if let Some(picker) = self.runtime.find_picker.as_mut() {
                    let clean = text.replace(['\n', '\r'], "");
                    if !clean.is_empty() {
                        picker.query.push_str(&clean);
                        picker.refilter();
                        self.render_find_picker();
                    }
                }
                Vec::new()
            }
            route::InputSink::Capture => {
                // Forward to the running `!` capture child RAW — no bracketed
                // markers. Captures are usually sudo / ssh / password prompts
                // that never enable bracketed-paste mode (DECSET 2004), so
                // wrapping would inject literal `\e[200~` escapes into the
                // answer. Matches the raw keystroke forwarding in
                // `handle_capture_key`. (Previously this leaked to the pane.)
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![Effect::SendToCapture {
                        bytes: text.into_bytes(),
                    }]
                }
            }
            route::InputSink::OverlayDismiss => {
                // Any input dismisses a held, exited overlay — a paste too.
                self.dismiss_overlay();
                Vec::new()
            }
            // No text sink — swallow (don't leak to the bottom pane). Quick-
            // select / harpoon / the image gallery are single-key menus;
            // pane-scrollback is effectively dead (scroll routes via PagerKey).
            route::InputSink::QuickSelect
            | route::InputSink::Harpoon
            | route::InputSink::ImageGallery
            | route::InputSink::ImageView
            | route::InputSink::PaneScroll => Vec::new(),
            // ── content sinks ──
            route::InputSink::Prompt => {
                // Splice into the active prompt buffer. Strip newlines (prompts
                // are single-line).
                if let Mode::Prompting(ref mut p) = self.state.mode {
                    let clean = text.replace(['\n', '\r'], " ");
                    if let Some(ed) = p.editor.as_mut() {
                        // Editor (`!` / `;` / `:`): splice at the cursor, then
                        // sync the canonical buffer from the editor.
                        ed.insert_str(&clean);
                        p.buffer = ed.text();
                    } else {
                        // Simple prompt (search, mkdir, …): no cursor, append.
                        p.buffer.push_str(&clean);
                    }
                }
                Vec::new()
            }
            route::InputSink::PagerKey => {
                // Into the pager's `/`-search buffer when typing one; else a
                // pager has no text input, so it flashes a hint. (Previously a
                // paste here leaked to the bottom pane.)
                self.handle_pager_paste(&text);
                Vec::new()
            }
            route::InputSink::OverlayPty => {
                // Route to the `V`/`;` top-overlay subprocess, bracketed so it
                // arrives as one block. Don't steal focus.
                vec![Effect::SendToPane {
                    target: PaneTarget::Overlay,
                    input: PaneInput::Bytes(bracketed_paste(&text)),
                    on_ok: None,
                    err_prefix: None,
                }]
            }
            route::InputSink::BottomPane => {
                // The user is interacting with the bottom pane — focus it (as a
                // keystroke there would) and track the text for `yP`. Wrap in
                // bracketed-paste markers only when the child actually enabled
                // the mode (claude/codex/a shell with it on); a plain shell over
                // ssh that never did would otherwise take the literal `\e[200~`
                // bytes as input. Mirrors the raw `Capture` forward above.
                if !self.state.pane_focused() {
                    self.set_pane_focus(true);
                }
                self.state.pane.pane_prompt_buf.push_str(&text);
                let child_bracketed = self
                    .runtime
                    .pane_tabs
                    .as_ref()
                    .is_some_and(|t| t.active().bracketed_paste_enabled());
                vec![Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::Bytes(paste_bytes_for_pane(&text, child_bracketed)),
                    on_ok: None,
                    err_prefix: None,
                }]
            }
            route::InputSink::PaneExitedFlash => {
                // An exited tab has no live pty — flash, never write bytes to a
                // dead pane (the former leak).
                self.state
                    .flash_info("pane exited — `^a-R` to restart, `^a-x` to close");
                Vec::new()
            }
            route::InputSink::Resolver => {
                // File-list normal mode — nowhere sensible to send a paste.
                // Some terminals wrap rapid-fire keystrokes in bracketed paste,
                // so flash rather than silently drop.
                let n = text.chars().count();
                self.state.flash_info(format!(
                    "paste ignored ({n} chars) — open `:` or `^\\` to paste"
                ));
                Vec::new()
            }
        }
    }

    /// MVU Phase 6 PR-C: handle a terminal resize — immediately resize all pty
    /// tabs + the top overlay so child shells re-render at the correct width,
    /// and rebuild the help overlay (its wrap points are baked at open time).
    /// Verbatim move of the loop's `Event::Resize` arm.
    pub(crate) fn handle_resize(&mut self, cols: u16, rows: u16) {
        self.view.term_size = (cols, rows);
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
        // Same for about: its markdown wrap points are baked at render time.
        if self.about_is_open() {
            self.open_about();
        }
        // The split preview's markdown was wrapped to the old column width;
        // re-render it off-thread at the new width (no-op when no preview is
        // open). The in-flight guard collapses a resize-drag's event burst to a
        // single trailing re-render.
        self.kick_preview_reload();
        // A git-view pager (diff/show) bakes its side-by-side columns and
        // wrapped long lines to a fixed body width — re-lay-out at the new
        // width so it reflows. No-op when no git-view pager is open; the syntax
        // highlight is cached on the stream, so this is layout-only (no syntect).
        self.dispatch_pager_command(crate::app::pager_stream::PagerStreamCmd::Rerender);
    }

    /// Dispatch a user-defined binding. Inline-data actions (unix command,
    /// preset pattern, preset path) run through the same machinery as the
    /// built-in prompts but skip the prompt UI.
    pub(super) fn apply_user(&mut self, bound: &BoundAction) -> Result<Vec<Effect>> {
        match bound {
            BoundAction::Plain(action) => return self.apply(action),
            BoundAction::UnixCmd(template) => {
                return match shell::expand_percent(template, &self.state.selection_paths()) {
                    Ok(cmd) => Ok(sh_c(&cmd, true)),
                    Err(e) => {
                        self.state.flash_error(e.to_string());
                        Ok(Vec::new())
                    }
                };
            }
            BoundAction::PatternPick(pattern) => {
                if let Ok(pat) = glob::Pattern::new(pattern) {
                    // Collect first: the entries read borrows `cur()`, which would
                    // clash with the `cur_mut()` insert in the loop body.
                    let matched: Vec<std::path::PathBuf> = self
                        .state
                        .cur()
                        .listing
                        .entries
                        .iter()
                        .filter(|e| pat.matches(&e.name))
                        .map(|e| e.path.clone())
                        .collect();
                    for path in &matched {
                        self.state.cur_mut().picks.insert(path);
                    }
                    self.state.cur_mut().list_generation =
                        self.state.cur().list_generation.wrapping_add(1);
                }
            }
            BoundAction::Jump(path) => {
                let _ = self.state.jump_to(path);
            }
            BoundAction::Command(cmd) => {
                // Dispatch exactly like typed `:` input (table commands +
                // `:!`/`:;` shell). Gated to $HOME config via `is_executing`.
                return Ok(self.dispatch_command(cmd));
            }
            BoundAction::Lua(name) => {
                // Run a $HOME-config Lua script off-thread (gated by
                // `is_executing`). Submits a job and returns; results land via
                // `Message::Wake(Wake::Lua)` → `handle_lua_done`.
                return Ok(self.apply_lua_binding(name));
            }
            BoundAction::ToggleMaskFixed(n) => {
                if *n == 1 {
                    self.state.cur_mut().masks.toggle_mask1();
                } else if *n == 2 {
                    self.state.cur_mut().masks.toggle_mask2();
                }
                self.state.rebuild_rows();
            }
        }
        let row_count = self.state.cur().rows.len();
        self.state.cur_mut().cursor.clamp(row_count);
        Ok(Vec::new())
    }
}

mod confirms;
mod prompts;

#[cfg(test)]
mod paste_tests {
    use super::{bracketed_paste, paste_bytes_for_pane};

    #[test]
    fn wraps_plain_text() {
        assert_eq!(bracketed_paste("hello"), b"\x1b[200~hello\x1b[201~");
    }

    /// A child with the mode on (claude/codex/shell that enabled DECSET 2004)
    /// gets the wrapped block; a child without it gets the bytes raw so the
    /// literal markers aren't injected as input.
    #[test]
    fn pane_paste_wraps_only_when_child_enabled_the_mode() {
        assert_eq!(
            paste_bytes_for_pane("ls -la", true),
            b"\x1b[200~ls -la\x1b[201~"
        );
        assert_eq!(paste_bytes_for_pane("ls -la", false), b"ls -la");
    }

    /// The regression: multi-line paste into a shell that never enabled the
    /// mode must not carry any `\e[200~`/`\e[201~` bytes (they'd land as the
    /// stray input seen when pasting into a remote `/bin/sh` over ssh).
    #[test]
    fn raw_pane_paste_has_no_bracket_markers() {
        let out = paste_bytes_for_pane("line one\nline two\n", false);
        assert_eq!(out, b"line one\nline two\n");
        let s = String::from_utf8(out).unwrap();
        assert!(!s.contains("\x1b[200~"));
        assert!(!s.contains("\x1b[201~"));
    }

    #[test]
    fn strips_embedded_end_marker_to_block_injection() {
        // A paste carrying its own end marker must not be able to close the
        // block early and have its tail run as keystrokes/commands.
        let out = bracketed_paste("safe\x1b[201~rm -rf /\x1b[200~more");
        assert_eq!(out, b"\x1b[200~saferm -rf /more\x1b[201~");
        // Exactly one opening and one closing marker remain.
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches("\x1b[200~").count(), 1);
        assert_eq!(s.matches("\x1b[201~").count(), 1);
    }
}

#[cfg(test)]
mod suspend_key_tests {
    use super::{PaneKeyAction, pane_suspend_key_action};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn ctrl_z() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)
    }
    fn plain(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn ctrl_z_toggles_on_a_non_shell_tab_running_or_suspended() {
        // Running: ^z suspends. Suspended: ^z resumes. Both Toggle.
        assert_eq!(
            pane_suspend_key_action(ctrl_z(), false, false),
            PaneKeyAction::Toggle
        );
        assert_eq!(
            pane_suspend_key_action(ctrl_z(), false, true),
            PaneKeyAction::Toggle
        );
    }

    #[test]
    fn shell_ctrl_z_forwards_for_its_own_job_control() {
        // The bug we must NOT reintroduce: a shell tab's ^z must reach the pty,
        // not get intercepted as a spyc suspend.
        assert_eq!(
            pane_suspend_key_action(ctrl_z(), true, false),
            PaneKeyAction::Forward
        );
    }

    #[test]
    fn suspended_tab_eats_non_ctrl_z_keys() {
        // While stopped, a stray key must not be forwarded (it would "type" on
        // resume); only ^z (handled above) wakes it.
        assert_eq!(
            pane_suspend_key_action(plain('j'), false, true),
            PaneKeyAction::EatSuspended
        );
    }

    #[test]
    fn running_tab_forwards_ordinary_keys() {
        assert_eq!(
            pane_suspend_key_action(plain('j'), false, false),
            PaneKeyAction::Forward
        );
        assert_eq!(
            pane_suspend_key_action(plain('j'), true, false),
            PaneKeyAction::Forward
        );
    }
}

#[cfg(test)]
mod capture_key_tests {
    use super::{CaptureKeyAction, capture_key_action};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn plain(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn control_keys_are_spyc_managed() {
        assert_eq!(capture_key_action(ctrl('c')), CaptureKeyAction::Interrupt);
        assert_eq!(capture_key_action(ctrl('\\')), CaptureKeyAction::Kill);
        assert_eq!(capture_key_action(ctrl('z')), CaptureKeyAction::Background);
    }

    #[test]
    fn everything_else_forwards_so_prompts_can_be_answered() {
        // A plain `c` is a password character, not an interrupt.
        assert_eq!(capture_key_action(plain('c')), CaptureKeyAction::Forward);
        assert_eq!(capture_key_action(plain('y')), CaptureKeyAction::Forward);
        assert_eq!(
            capture_key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            CaptureKeyAction::Forward
        );
    }
}

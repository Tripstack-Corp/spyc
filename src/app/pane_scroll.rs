//! Pane scrollback / transcript pager (`^a-v`): snapshot the active pane's
//! vt100 scrollback (or an agent's on-disk transcript) into a lower-pane
//! pager, the vi-style scroll-mode key handler, and the tab-switch stash/
//! restore pair that keeps a scrollback pager attached to its tab. Extracted
//! verbatim from `app/mod.rs` (the impl-extraction sweep). The open / stash /
//! restore / key-handler entry points are `pub` (called from `actions` /
//! `key_dispatch`); `mount_scroll_pager` is internal.

use super::{App, Effect, PaneTextKind, PaneTextSink, state};

impl App {
    /// Stash the active scrollback pager (if any) onto the
    /// currently-active tab's slot. Tab-switch handlers call this
    /// **before** flipping the active-tab pointer; the companion
    /// `restore_active_tab_scrollback_pager` runs **after** the flip
    /// to surface the destination tab's stashed pager if it has one.
    /// Together: scroll back on tab 1, `^a-n`, the pager visually
    /// disappears (replaced by tab 2's live pty); `^a-p` back to
    /// tab 1, the pager comes back at the same scroll / search /
    /// selection state.
    ///
    /// Only acts on scrollback pagers (`pane_scroll == true`).
    /// Content-bound pagers (Overlay file viewer, TopPane Markdown,
    /// etc.) are App-level and persist across tab switches.
    pub fn stash_scrollback_pager_to_active_tab(&mut self) {
        if !self.view.pager.as_ref().is_some_and(|v| v.pane_scroll) {
            return;
        }
        let view = self.view.pager.take();
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_entry_mut().stashed_scrollback_pager = view;
        }
    }

    /// Restore the active tab's stashed scrollback pager into
    /// `self.view.pager` if one is stashed AND no other pager is currently
    /// displayed. A non-scrollback pager (Overlay file viewer, etc.)
    /// up at the time of the tab switch is left alone; the stash
    /// surfaces on the next switch back where no overlay is in the
    /// way. See `stash_scrollback_pager_to_active_tab` for the
    /// outgoing half of the pair.
    pub fn restore_active_tab_scrollback_pager(&mut self) {
        if self.view.pager.is_some() {
            return;
        }
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && let Some(view) = tabs.active_entry_mut().stashed_scrollback_pager.take()
        {
            self.set_pager(view);
        }
    }

    pub fn open_pane_scroll_pager(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return;
        };
        let active_info = tabs.active_info();
        let label = active_info.label.clone();
        let command = active_info.command.clone();
        let cwd = active_info.cwd.clone();
        let spawn = active_info.spawn_epoch_secs;

        // Agent-aware scrollback. An agent's `AgentProfile` may carry a
        // `TranscriptSpec`: read its structured on-disk transcript — the
        // source of truth (codex/agy confine history to a scroll region
        // vt100 can't capture; claude's terminal output works too but
        // the transcript is cleaner) — and render the real conversation,
        // taking priority over the alt-screen guard + vt100 path below.
        // `config_key` gates the view (`None` = always-on, e.g. codex).
        // `miss_message` distinguishes "flash + stop" (codex — no usable
        // terminal capture) from "fall through to vt100" (claude/agy).
        let profile = crate::agent::detect(&command);
        if let Some(spec) = profile.transcript() {
            let enabled = match spec.config_key {
                None => spec.default_enabled,
                Some(key) => self
                    .state
                    .config
                    .pane
                    .transcript_enabled(key, spec.default_enabled),
            };
            if enabled {
                if let Some(path) = (spec.resolve)(cwd.as_path(), spawn) {
                    let lines = (spec.render)(path.as_path(), &self.view.theme);
                    if !lines.is_empty() {
                        self.mount_scroll_pager(format!(" {label} (transcript)"), lines);
                        return;
                    }
                }
                if let Some(msg) = spec.miss_message {
                    self.state.flash_info(msg);
                    return;
                }
            }
        }

        let tabs = self
            .runtime
            .pane_tabs
            .as_mut()
            .expect("pane_tabs presence checked above");
        let active = tabs.active_mut();
        if active.is_alternate_screen() {
            // Alt-screen apps (vim, less, htop, ...) do virtual
            // scrolling inside a fixed grid — old content lives in
            // app memory, not the terminal — so spyc has nothing to
            // show.
            self.state
                .flash_info("scroll: alt-screen app — use its own scrollback / history keys");
            return;
        }
        // Drain pending bytes before snapshotting. Bytes that hit
        // the OS pipe between the last render tick and this keypress
        // may still be in flight on the reader/parser threads; a few
        // short yields let them flush so the snapshot includes the
        // most-recent paint.
        for _ in 0..3 {
            active.drain_output();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        active.drain_output();
        // Empty scrollback ⇒ a fresh process, or an app that keeps
        // its own history (scroll region / virtual scroll). Flash a
        // hint; still open the pager so search/yank of the visible
        // screen works.
        let scrollback_rows = active.with_screen_mut(crate::ui::scrollback::scrollback_len);
        let lines = active.with_screen_mut(crate::ui::scrollback::lines_from_scrollback);
        if scrollback_rows == 0 {
            self.state
                .flash_info("no scrollback captured — this app keeps its own history");
        }
        self.mount_scroll_pager(format!(" {label} (history)"), lines);
    }

    /// Mount a lower-pane scroll/transcript pager from pre-built
    /// lines. Shared by the vt100-scrollback path and the codex
    /// on-disk transcript path. Enters the active pane's scroll mode
    /// (divider cues + key routing flip to the pager) and parks the
    /// view at the bottom on first render.
    fn mount_scroll_pager(&mut self, title: String, lines: Vec<ratatui::text::Line<'static>>) {
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_mut().enter_scroll_mode();
        }
        let mut view = crate::ui::pager::PagerView::new_styled(title, lines);
        view.mount = crate::ui::pager::Mount::LowerPane;
        view.pane_scroll = true;
        // Gutter off so existing content doesn't jump horizontally
        // when the pager opens. Toggle with `l`.
        view.show_line_numbers = false;
        view.no_history = true;
        // Wrap long lines (compiler errors, diffs, transcript turns)
        // — no horizontal scroll, so truncation would hide content.
        view.wrap = true;
        // Park at the bottom on first render via the deferred flag;
        // the LowerPane render branch knows the real viewport height
        // and scrolls there, avoiding a one-frame jump.
        view.pending_scroll_to_bottom.set(true);
        self.set_pager(view);
        self.state.focus = state::Focus::Pane;
        self.view.needs_full_repaint = true;
        self.state
            .flash_info("scroll: on (/, n/N, :N, V, y, Esc exit)");
    }

    pub fn handle_pane_scroll_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Effect> {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Handle pending `g` prefix: gg = scroll top, gf/gF = goto file.
        if self.view.scroll_pending_g {
            self.view.scroll_pending_g = false;
            return match key.code {
                KeyCode::Char('g') => {
                    self.runtime
                        .pane_tabs
                        .as_mut()
                        .unwrap()
                        .active_mut()
                        .scroll_to_top();
                    Vec::new()
                }
                // gf/gF while scrolling a pane — same path as the file-list
                // action: emit a `ReadPaneText`/`GotoFile` effect so the
                // pickable read + navigation run in `run_effects` (PR 5b).
                KeyCode::Char('f') => vec![Effect::ReadPaneText {
                    kind: PaneTextKind::Pickable(200),
                    then: PaneTextSink::GotoFile {
                        open_at_line: false,
                    },
                }],
                KeyCode::Char('F') => vec![Effect::ReadPaneText {
                    kind: PaneTextKind::Pickable(200),
                    then: PaneTextSink::GotoFile { open_at_line: true },
                }],
                _ => Vec::new(), // Unknown g-sequence, ignore
            };
        }

        let pane = self.runtime.pane_tabs.as_mut().unwrap().active_mut();
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => pane.scroll_up(1),
            KeyCode::Char('j') | KeyCode::Down => pane.scroll_down_or_exit(1),
            KeyCode::PageUp | KeyCode::Char('b') if ctrl => pane.scroll_up(20),
            KeyCode::Char('u') if ctrl => pane.scroll_up(10),
            KeyCode::PageDown | KeyCode::Char('f') if ctrl => pane.scroll_down_or_exit(20),
            KeyCode::Char('d') if ctrl => pane.scroll_down_or_exit(10),
            KeyCode::Char('g') => {
                self.view.scroll_pending_g = true;
            }
            KeyCode::Char('G') => pane.scroll_to_bottom(),
            KeyCode::Char('s') => match pane.save_to_file() {
                Ok(path) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.state.flash_info(format!("saved: {name}"));
                }
                Err(e) => self.state.flash_info(format!("save error: {e}")),
            },
            KeyCode::Esc | KeyCode::Char('q') => {
                pane.exit_scroll_mode();
                self.state.flash_info("scroll: off");
            }
            _ => {}
        }
        Vec::new()
    }
}

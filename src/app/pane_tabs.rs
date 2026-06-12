//! Bottom-pane / tab lifecycle: create / hide / close / restart tabs,
//! directional focus, resize + zoom, the pane/overlay sizing helpers,
//! the session-restore resume driver + crashed-restore detection, and
//! the `:dump-scrollback` diagnostic. The methods here are called from
//! sibling modules (`actions`, `key_dispatch`, `commands`, `tasks`, `render`,
//! `pager_handler`, `loop_steps`, `session`), so they're `pub` (or
//! `pub(super)` for the one that only a sibling needs).

use std::time::Duration;

use super::{
    App, Mode, Pane, PaneTabs, Prompt, PromptKind, RESTORE_RESUME_ENTER_DELAY,
    RESTORE_RESUME_VERIFY_DELAY, RESTORE_RESUME_VERIFY_RETRIES, RESTORE_RESUME_VERIFY_TAIL,
    StatusPosition, TabEntry, TabInfo, state,
};

impl App {
    /// `^a-\` / F10 — toggle the bottom pane. Three states:
    ///   1. No tabs yet (`pane_tabs.is_none()`): spawn the default
    ///      command. Same as before.
    ///   2. Tabs exist and visible: flip `pane_hidden = true`. The
    ///      child ptys keep running; render skips the pane area;
    ///      `pane_focused` parks at false so keystrokes go to the
    ///      file list. SIGWINCH is held off — the next show pass
    ///      restores the prior pane geometry.
    ///   3. Tabs exist and hidden: flip `pane_hidden = false`,
    ///      restore focus to the pane. Children pick up wherever
    ///      they left off.
    ///
    /// Why this is hide-don't-kill: previously toggle was destructive
    /// (`pane_tabs = None`, `Drop for PtyHost` → SIGKILL on the
    /// claude process group). Daily-drivers reported losing their
    /// in-flight conversation every time they wanted the whole
    /// screen for a few seconds. Explicit kill of a tab still goes
    /// through `^a-x` (`PaneCloseTab`); destroying the *whole*
    /// pane container is now a multi-step intentional act (close
    /// each tab via `^a-x`), not a one-keystroke side effect.
    pub fn toggle_pane(&mut self) {
        if self.runtime.pane_tabs.is_some() {
            self.state.pane.pane_hidden = !self.state.pane.pane_hidden;
            self.view.needs_full_repaint = true;
            if self.state.pane.pane_hidden {
                // Park focus on the list while hidden. Keystrokes
                // can't drive an off-screen pane sensibly. Zoom is
                // mutually exclusive with hidden — clear it so a
                // re-show doesn't try to render zoomed onto a
                // newly-resized area.
                self.state.focus = state::Focus::FileList;
                self.state.pane.pane_zoomed = false;
                self.state.pane.pane_focus_before_zoom = None;
                self.state.flash_info("pane hidden — F10/^a-\\ to show");
            } else {
                // Re-show: focus the pane so the next keystroke
                // lands in the child. Matches the "I'm opening this
                // because I want to interact with it" intent.
                self.state.focus = state::Focus::Pane;
                self.state.flash_info("pane shown");
            }
            return;
        }
        // No pane container exists — `^a-\` is a pure hide/show
        // toggle, not a create binding. Previously this silently
        // spawned the default command ($SPYC_PANE_CMD or `claude`),
        // which surprised users expecting a no-op (reported by
        // Justin: "I see ^a-c defaults to claude, but POLA"). Point
        // the user at the explicit creation binding instead.
        self.state.flash_info("no pane — ^a-c to create one");
    }

    /// Spawn a new pane tab. If no tabs exist, creates the container.
    /// Default cwd is the current listing dir — i.e. "open here",
    /// matching the user's mental model of where they're browsing.
    /// Reported by Justin: F9 (`ResumePane`) used to spawn at
    /// `PROJECT_HOME` (typically the dir spyc was launched from)
    /// instead of the dir he had since navigated into. `^a-c`
    /// already pre-fills its cwd prompt with `listing.dir`; this
    /// brings the bare-spawn path in line.
    ///
    /// Users who want a specific anchor should use `^a-c` and edit
    /// the prompt, or invoke `:project` to move PROJECT_HOME.
    pub fn open_pane_tab(&mut self, cmd: &str) {
        let cwd = self.state.listing.dir.clone();
        self.open_pane_tab_in(cmd, &cwd);
    }

    /// Spawn a pane tab running `cmd` in `cwd`. Returns `true` iff the pty
    /// actually spawned and a tab was pushed — callers that arm follow-up
    /// state on "the tab we just added" (restart flash, restore `/resume`
    /// injection) MUST gate on this, or they act on the wrong tab when the
    /// spawn fails. On failure this flashes the error and returns `false`.
    pub fn open_pane_tab_in(&mut self, cmd: &str, cwd: &std::path::Path) -> bool {
        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        match Pane::spawn_with_env(cmd, rows, cols, cwd, &self.view.context_path, &[], wake) {
            Ok(p) => {
                self.state.focus = state::Focus::Pane;
                self.state
                    .flash_info(format!("pane: {cmd} (^W k for list)"));
                let entry = TabEntry::new(p, TabInfo::new(cmd, cwd));
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.push(entry);
                } else {
                    self.runtime.pane_tabs = Some(PaneTabs::new(entry));
                }
                true
            }
            Err(e) => {
                self.state.flash_error(format!("pane spawn failed: {e}"));
                false
            }
        }
    }

    /// For tabs that have a `pending_resume_send` armed (set by
    /// `restore_session`), drive the two-phase keystroke injection
    /// that recovers a Claude conversation. We avoid the `--resume`
    /// CLI flag because it trips a known regression that crashes at
    /// mount; the slash-command path goes through `tM_` and works
    /// fine.
    ///
    /// Three phases:
    /// - `Text` (after banner-settle): write `/resume <sid>` with no
    ///   trailing Enter and transition to `Enter`.
    /// - `Enter` (after a small additional delay): write `\r` and
    ///   transition to `Verify`.
    /// - `Verify` (closed-loop): if `/resume <sid>` is still sitting
    ///   unsubmitted in the pane tail, re-send `\r` and re-arm, up to
    ///   `RESTORE_RESUME_VERIFY_RETRIES` times.
    ///
    /// Splitting the text/Enter writes narrows the race where
    /// Claude's TUI was still mid-render when the original combined
    /// `/resume <sid>\r` arrived, but no fixed delay closes it —
    /// async startup work (MCP connects, version check, org-message
    /// fetch) can remount the input component and eat a lone `\r`
    /// seconds after spawn. The `Verify` phase makes the submit
    /// observed rather than hoped-for: the sid vanishing from the
    /// screen tail is the success signal, and while it's visible a
    /// retry `\r` is harmless. (The v1.70 "bells" design subsumes
    /// this; delete the phase when that lands.)
    pub fn send_pending_resumes(&mut self, now: std::time::Instant) {
        use crate::pane::tabs::{PendingResumeSend, resume_still_unsubmitted};
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            return;
        };
        for entry in tabs.tabs_mut() {
            match entry.info.pending_resume_send.take() {
                Some(PendingResumeSend::Text { sid, after }) if now >= after => {
                    let _ = entry.pane.send_bytes(format!("/resume {sid}").as_bytes());
                    entry.info.pending_resume_send = Some(PendingResumeSend::Enter {
                        sid,
                        after: now + RESTORE_RESUME_ENTER_DELAY,
                    });
                }
                Some(PendingResumeSend::Enter { sid, after }) if now >= after => {
                    let _ = entry.pane.send_bytes(b"\r");
                    entry.info.pending_resume_send = Some(PendingResumeSend::Verify {
                        sid,
                        after: now + RESTORE_RESUME_VERIFY_DELAY,
                        retries_left: RESTORE_RESUME_VERIFY_RETRIES,
                    });
                }
                Some(PendingResumeSend::Verify {
                    sid,
                    after,
                    retries_left,
                }) if now >= after => {
                    let tail = entry.pane.recent_lines(RESTORE_RESUME_VERIFY_TAIL);
                    if resume_still_unsubmitted(&tail, &sid) && retries_left > 0 {
                        let _ = entry.pane.send_bytes(b"\r");
                        entry.info.pending_resume_send = Some(PendingResumeSend::Verify {
                            sid,
                            after: now + RESTORE_RESUME_VERIFY_DELAY,
                            retries_left: retries_left - 1,
                        });
                    }
                    // Submitted (sid gone from the tail) or retries
                    // exhausted: cleared by take(). On exhaustion the
                    // user recovers by pressing Enter themselves.
                }
                other => {
                    // Not yet due, or empty — put back what we took.
                    entry.info.pending_resume_send = other;
                }
            }
        }
    }

    /// Locate a `claude --resume` tab from session restore that looks
    /// broken (non-zero exit, or alive-but-printed-a-crash-dump within
    /// the 30s window). Disarms the marker on tabs whose window has
    /// passed without trouble, so a real user-driven exit later isn't
    /// mistaken for a restore failure. Returns the index of the first
    /// crashed tab found, if any.
    pub fn find_crashed_restore_tab(&mut self, now: std::time::Instant) -> Option<usize> {
        let tabs = self.runtime.pane_tabs.as_mut()?;
        let window = Duration::from_secs(30);
        let dump_grace = Duration::from_secs(3);
        for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
            if entry.info.restore_fallback.is_none() {
                continue;
            }
            let age = now.duration_since(entry.info.spawn_at);
            if age > window {
                entry.info.restore_fallback = None;
                continue;
            }
            let bad_exit = entry.pane.is_closed()
                && entry.pane.exit_status().is_some_and(|s| s.exit_code() != 0);
            // Always re-scan once dump_grace has elapsed: claude often
            // prints the entire crash dump in <1s then sits quiescent,
            // and `output_dirty` gets cleared on every render — gating
            // on it would silently swallow the prompt.
            let dump_signature = !entry.pane.is_closed()
                && age >= dump_grace
                && pane_has_crash_marker(&entry.pane.recent_lines(200));
            if bad_exit || dump_signature {
                return Some(i);
            }
        }
        None
    }

    pub fn start_new_tab_prompt(&mut self) {
        // Precedence: $SPYC_PANE_CMD > [pane] default_command in
        // .spycrc.toml > "claude" fallback. Env var wins so a user
        // can override on the fly per shell without editing config.
        let default_cmd = crate::envset::var("SPYC_PANE_CMD")
            .or_else(|| self.state.config.pane.default_command.clone())
            .unwrap_or_else(|| "claude".to_string());
        let mut p = Prompt::shell(PromptKind::PaneNewTabCmd, "pane command: ");
        p.buffer.clone_from(&default_cmd);
        if let Some(ed) = p.editor.as_mut() {
            ed.set_content(&default_cmd);
        }
        self.state.mode = Mode::Prompting(p);
    }

    /// ^W x — close the active pane tab.
    pub fn close_active_tab(&mut self) {
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && !tabs.close_active()
        {
            // Last tab removed.
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
            self.view.needs_full_repaint = true;
            self.state.flash_info("pane: last tab closed");
        }
    }

    /// ^a R — restart the active tab's command. Closes the tab and spawns
    /// a fresh one with the same command and working directory.
    pub fn restart_active_tab(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return;
        };
        let cmd = tabs.active_info().command.clone();
        let cwd = tabs.active_info().cwd.clone();
        // Close the old tab first.
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && !tabs.close_active()
        {
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
        }
        // Spawn a replacement with the same command and cwd. Only claim
        // "restarted" if it actually spawned — otherwise leave
        // open_pane_tab_in's "pane spawn failed" flash in place rather than
        // clobbering it with a false success (the old tab is already gone).
        if self.open_pane_tab_in(&cmd, &cwd) {
            self.state.flash_info(format!("pane: restarted {cmd}"));
        }
    }

    /// ^W j / ^W k — set keyboard focus directionally (no wrap).
    pub fn set_pane_focus(&mut self, want_pane: bool) {
        if self.runtime.pane_tabs.is_none() {
            return;
        }
        if self.state.pane_focused() == want_pane {
            return; // already there — no-op
        }
        // The focus decision is a pure fn of these two bits (see
        // `super::focus::decide_focus`); the branch-order contract — every
        // non-Pane arm collapses to `pane_focused() == false` — is pinned by
        // tests there.
        self.state.focus = super::focus::decide_focus(
            super::focus::FocusSnapshot {
                has_top_overlay: self.runtime.top_overlay.is_some(),
                pager_mount: self.view.pager.as_ref().map(|v| v.mount),
            },
            want_pane,
        );
        if self.state.pane_focused() {
            let label = self
                .runtime
                .pane_tabs
                .as_ref()
                .map_or("pane", |t| t.active_info().label.as_str());
            self.state.flash_info(format!("focus: {label}"));
        } else {
            // When a `;cmd` overlay is showing the spyc-list slot, the
            // "non-pane" side is the overlay subprocess, not the file
            // list. Label accordingly so the user can read what just
            // got focus instead of guessing.
            let label = if self.runtime.top_overlay.is_some() {
                "overlay"
            } else {
                "spyc"
            };
            self.state.flash_info(format!("focus: {label}"));
        }
    }

    /// Re-derive `state.focus` from the live surfaces, preserving the current
    /// pane-vs-not intent.
    ///
    /// The pane-intent (is the bottom region focused?) is the stable bit set by
    /// `set_pane_focus` / pane opens / scrollback; the *non-pane* variant
    /// (`Overlay` / `Pager` / `FileList`) is simply whichever surface is
    /// front-most right now, derived by the same pure [`super::focus::decide_focus`]
    /// `set_pane_focus` uses. Called at the loop top (and on surface close) so
    /// `state.focus` always reflects reality with **no per-open-site
    /// bookkeeping** — most pager opens (`:grep`, git-view, help, …) never touch
    /// focus, and closes left a stale `Overlay`/`Pager` behind. Behavior-
    /// preserving while routing/render still read only `pane_focused()`: this
    /// only refines the non-`Pane` discriminant, which `pane_focused()` ignores.
    pub(super) fn recompute_focus(&mut self) {
        let want_pane = matches!(self.state.focus, state::Focus::Pane);
        self.state.focus = super::focus::decide_focus(
            super::focus::FocusSnapshot {
                has_top_overlay: self.runtime.top_overlay.is_some(),
                pager_mount: self.view.pager.as_ref().map(|v| v.mount),
            },
            want_pane,
        );
    }

    /// `:dump-scrollback` diagnostic. Runs the same drain +
    /// snapshot path as `^a-v`, then writes the captured lines as
    /// plain text to `/tmp/spyc-scrollback.txt`. Tail the file to
    /// confirm whether content visible on the live pane (HUD
    /// overlays, etc.) is actually reaching our vt100 emulator at
    /// snapshot time.
    pub fn dump_scrollback_snapshot(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            self.state.flash_error("dump-scrollback: no pane open");
            return;
        };
        let active = tabs.active_mut();
        for _ in 0..3 {
            active.drain_output();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        active.drain_output();
        let lines = active.with_screen_mut(crate::ui::scrollback::lines_from_scrollback);
        let mut out = String::new();
        for line in &lines {
            for span in &line.spans {
                out.push_str(&span.content);
            }
            out.push('\n');
        }
        // Owner-only (0600) in the state dir, not the old world-readable,
        // symlink-followable `/tmp/spyc-scrollback.txt` (pane scrollback can
        // contain anything the agent printed). Truncate so each dump is a
        // fresh snapshot.
        let name = "spyc-scrollback.txt";
        let display = crate::state::state_file_path(name)
            .map_or_else(|| name.to_string(), |p| p.display().to_string());
        match crate::state::open_state_file_truncate(name) {
            Some(mut f) => match std::io::Write::write_all(&mut f, out.as_bytes()) {
                Ok(()) => self
                    .state
                    .flash_info(format!("wrote {} lines to {display}", lines.len())),
                Err(e) => self
                    .state
                    .flash_error(format!("dump-scrollback: write failed: {e}")),
            },
            None => self
                .state
                .flash_error("dump-scrollback: no state dir for the output file".to_string()),
        }
    }

    /// ^W + / ^W - — change the bottom pane's share of the middle rect
    /// in 5% steps, clamped to [10%, 90%].
    pub fn resize_pane(&mut self, delta_pct: i32) {
        if self.runtime.pane_tabs.is_none() {
            return;
        }
        if self.state.pane.pane_zoomed {
            self.state.flash_info("pane is zoomed (^a z to exit)");
            return;
        }
        let current = i32::from(self.state.pane.pane_height_pct);
        let new = (current + delta_pct).clamp(10, 90);
        self.state.pane.pane_height_pct = new as u16;
    }

    /// The pane percentage to use for layout/sizing computations.
    /// Returns 100 when zoomed (list collapses to 0 rows) so that the
    /// stored `pane_height_pct` — the user's preferred split — stays
    /// untouched and is restored on un-zoom.
    pub const fn effective_pane_pct(&self) -> u16 {
        if self.state.pane.pane_zoomed {
            100
        } else {
            self.state.pane.pane_height_pct
        }
    }

    /// ^a z / ^w z — toggle "zoom" on the bottom pane. When zoomed,
    /// the file list collapses to 0 rows and the pane fills the
    /// middle region (status + prompt rows still render). Focus is
    /// forced into the pane on zoom-on; the prior focus is restored
    /// on zoom-off. No-op (with a flash) when the pane is closed.
    pub fn toggle_pane_zoom(&mut self) {
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_info("no pane open");
            return;
        }
        if self.state.pane.pane_zoomed {
            self.state.pane.pane_zoomed = false;
            if let Some(prev) = self.state.pane.pane_focus_before_zoom.take() {
                self.state.focus = if prev {
                    state::Focus::Pane
                } else {
                    state::Focus::FileList
                };
            }
            self.state.flash_info("zoom: off");
        } else {
            self.state.pane.pane_focus_before_zoom = Some(self.state.pane_focused());
            self.state.pane.pane_zoomed = true;
            self.state.focus = state::Focus::Pane;
            self.state.flash_info("zoom: on (^a z to exit)");
        }
        // Resize all pty children to the new pane rect so their
        // child shells re-render at the right dimensions; otherwise
        // Claude's UI is the wrong size until the next terminal resize.
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(
            area,
            true,
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        if let (Some(pane_rect), Some(tabs)) = (layout.pane, self.runtime.pane_tabs.as_mut()) {
            for entry in tabs.tabs_mut() {
                let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
            }
        }
        self.view.needs_full_repaint = true;
    }

    /// Compute the (rows, cols) the bottom pane will occupy.
    pub fn pane_spawn_size(height_pct: u16, status_position: StatusPosition) -> (u16, u16) {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(area, true, height_pct, status_position);
        match layout.pane {
            Some(r) => (r.height.max(1), r.width.max(1)),
            None => (rows.saturating_sub(3).max(1), cols.max(1)),
        }
    }

    /// Compute the (rows, cols) available for the top-overlay pty. This
    /// is the top area: everything above the divider (or the whole
    /// screen minus the prompt row if no bottom pane).
    pub fn top_overlay_size(pane_height_pct: u16, has_bottom_pane: bool) -> (u16, u16) {
        let (cols, total_rows) = crossterm::terminal::size().unwrap_or((80, 24));
        if !has_bottom_pane {
            // Full screen minus prompt row.
            return (total_rows.saturating_sub(1).max(1), cols.max(1));
        }
        // With bottom pane: top region = total - divider(1) - bottom pane.
        let usable = total_rows.saturating_sub(1); // minus divider
        let bottom = (u32::from(usable) * u32::from(pane_height_pct) / 100) as u16;
        let top = usable.saturating_sub(bottom);
        (top.max(1), cols.max(1))
    }
}

/// True when scrollback contains a known Claude/bun crash signature.
/// These markers don't appear in healthy Claude startup output.
fn pane_has_crash_marker(lines: &[String]) -> bool {
    const MARKERS: &[&str] = &[
        // bun's single-file runtime path; appears in unhandled-exception dumps.
        "/$bunfs/root/",
        // e.g. `g9H is not a function` on the resume path regression.
        "is not a function",
        // sandbox helper failed and `failIfUnavailable` is set.
        "Error: sandbox required but unavailable",
    ];
    lines
        .iter()
        .any(|line| MARKERS.iter().any(|m| line.contains(m)))
}

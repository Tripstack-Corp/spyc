//! The pager's scroll / vi-motion fall-through (`handle_pager_motion`):
//! the terminal handler reached when no input-mode or picker consumed the
//! key — scrolling, search, toggles, yank, close, and the $EDITOR/$PAGER
//! handoff. Always consumes (returns `Vec<Effect>`). Split verbatim.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::pane::Pane;
use crate::shell;

use crate::app::{App, Effect, PagerReturn, TaskStatus, sh_c};

impl App {
    /// Fall-through: scroll / vi-motion / toggles / close / editor handoff.
    pub(super) fn handle_pager_motion(&mut self, key: KeyEvent, viewport: u16) -> Vec<Effect> {
        let Some(view) = active_pager_mut!(self) else {
            return Vec::new();
        };
        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => {
                // A focused right-split preview: q/Esc closes the whole split
                // (drop the preview + clear the shape), not just the pager.
                if self.right_column_focused() {
                    self.close_vsplit();
                    return Vec::new();
                }
                // v1.5 pane-scroll pager: snap the underlying pty
                // back to live and clear the divider's [SCROLL]
                // indicator. The pager is closed in the regular
                // path below. (`active_pager_ref` so a focused bottom
                // scrollback in `view.scroll_pager` is what's checked,
                // not the top pager.)
                if self.active_pager_ref().is_some_and(|v| v.pane_scroll) {
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
                    self.view.pager_pending_bracket = None;
                    self.view.needs_full_repaint = true;
                    return Vec::new();
                }
                // Task viewer special close: if the viewed task has
                // exited (and the user has seen it), promote -- snapshot
                // its rendered view into buffer history and drop the
                // task from the bg list. Running tasks stay in bg.
                // Reads the ACTIVE pager (the focused column's), not a hardcoded
                // `view.pager` — else closing `b`'s pager would promote `a`'s task.
                let promote_task: Option<u32> = self.active_pager_ref().and_then(|v| {
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
                    // Save eligible pagers to history before closing. Operates
                    // on the ACTIVE pager (the focused column's slot), so closing
                    // `b`'s pager doesn't evict `a`'s — a hardcoded
                    // `view.pager.take()` here closed both columns at once.
                    let is_picker = self.state.pending_worktrees.is_some()
                        || self.state.pending_sessions.is_some()
                        || self.view.pending_history_pick.is_some();
                    let eligible = !is_picker
                        && self
                            .active_pager_ref()
                            .is_some_and(|v| v.picker_cursor.is_none() && !v.streaming);
                    if eligible {
                        // Persist scroll BEFORE the take — otherwise the take
                        // leaves the slot None and `remember_pager_position`'s
                        // save is a no-op (file pagers closed via Esc/q would
                        // never get their scroll saved to disk).
                        let slot = self.active_pager_slot();
                        self.remember_pager_position();
                        if let Some(v) = self.take_active_pager() {
                            self.view.pager_history.push(v);
                        }
                        // The rest of `clear_pager`'s teardown (the slot is
                        // already empty after the take). Only the left/single
                        // slot uses `overlay_column`; closing `b`'s pager must
                        // NOT unpin a still-open pager in `a`.
                        if !matches!(slot, crate::app::pager_handler::PagerSlot::Right) {
                            self.view.overlay_column = None;
                        }
                        self.recompute_focus();
                    } else {
                        self.clear_pager();
                    }
                }
                self.state.pending_worktrees = None;
                self.state.pending_sessions = None;
                self.view.pending_history_pick = None;
                self.view.pending_jump_history = None;
                self.view.pager_pending_bracket = None;
                self.view.needs_full_repaint = true;
            }
            KeyCode::Char('/') => view.begin_search(),
            KeyCode::Char('n') => view.search_next(viewport),
            KeyCode::Char('N') => view.search_prev(viewport),
            KeyCode::Char(':') => {
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
                // non-git-view pagers (returns false). Re-borrows `self` (the
                // `view` borrow above is not used in this arm).
                self.dispatch_pager_command(crate::app::pager_stream::PagerStreamCmd::ToggleLayout);
            }
            KeyCode::Char('w') => view.toggle_whitespace(),
            KeyCode::Char('W') => view.toggle_wrap(),
            KeyCode::Char('m') if !view.toggle_markdown() => {
                view.flash = Some("not a markdown file".into());
            }
            KeyCode::Char('f') => {
                view.toggle_full_width();
                // git-view diffs bake fixed-width side-by-side rows at the
                // body width; the toggle changes that width, so re-render the
                // retained model. No-op for plain-text pagers (no stream;
                // wrapping handles width at render). Re-borrows `self` after
                // the `view` use above ends.
                self.dispatch_pager_command(crate::app::pager_stream::PagerStreamCmd::Rerender);
            }
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
                    let cwd = self.state.left.listing.dir.clone();
                    self.clear_pager();
                    self.view.needs_full_repaint = true;
                    let wake = self.make_pane_wake();
                    match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
                        // Routes to the focused column's overlay slot (`b` gets
                        // its own), auto-dismiss on exit, focus the editor.
                        Ok(p) => self.install_overlay_pty(p),
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
                // Help is a top/overlay-pager concern (stash → restore the
                // `view.pager` slot). A focused bottom scrollback
                // (`view.scroll_pager`) has no `?` binding, so don't open help
                // over the top pager from underneath it.
                if !(self.state.pane_focused() && self.view.scroll_pager.is_some()) {
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
            }
            KeyCode::Char('r') if view.pane_scroll => {
                // Reload a transcript scrollback: re-resolve + re-read + render
                // off-thread (a full-screen agent keeps appending, so the
                // snapshot goes stale). `open_pane_scroll_pager` re-spawns into
                // `view.scroll_pager`. (`view`'s borrow ends at the guard.)
                self.open_pane_scroll_pager();
            }
            // Mermaid diagram on screen: `o` opens it full-res in the OS viewer
            // (Preview.app / xdg-open); `i` renders it as a full-screen image
            // overlay inside spyc (graphics terminals). Both render off-thread
            // via `Effect::RenderMermaid`; `apply_mermaid_outcomes` installs the
            // result + flashes status. `.map(...)` ends the immutable borrow
            // before we mutate `flash`. See docs/MERMAID_PAGER_PLAN.md.
            KeyCode::Char('o' | 'i') => {
                use crate::app::mermaid_ops::{MermaidMode, MermaidRenderOp};
                match view.visible_mermaid_block().map(|b| b.source.clone()) {
                    Some(source) => {
                        view.flash = Some("rendering mermaid diagram\u{2026}".to_string());
                        let mode = if key.code == KeyCode::Char('i') {
                            let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
                            // Reserve the bottom row for the dismiss hint, so the
                            // protocol is sized to the *actual* draw area — else
                            // ratatui-image's Image refuses to render (size > area).
                            MermaidMode::View {
                                cols,
                                rows: rows.saturating_sub(1),
                                // Dark by default — reads best in a terminal.
                                dark: true,
                            }
                        } else {
                            MermaidMode::Open
                        };
                        return vec![Effect::RenderMermaid(MermaidRenderOp { source, mode })];
                    }
                    None => {
                        view.flash = Some(
                            "no mermaid diagram in view (toggle rendered with `m`)".to_string(),
                        );
                    }
                }
            }
            _ => {}
        }
        Vec::new()
    }
}

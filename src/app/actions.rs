//! Action dispatch: `apply` / `apply_inner` — the controller that turns
//! a resolved `Action` into state mutations plus effects for the
//! run loop, with post-action project-state reconciliation
//! (`reconcile_harpoon` / `sync_harpoon_filter_set`).
//!
//! `apply` runs `apply_inner` then reconciles the harpoon list;
//! pure-domain actions are handled first by `AppState::apply`
//! (`src/app/state.rs`), and the arms here are the ones that need
//! terminal / pane / pager / session access.
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2 tidy-up), same
//! child-module `impl App` pattern: reads App's private state via the
//! descendant-module rule. `apply` (called from `commands` / `key_dispatch`),
//! `reconcile_harpoon`, and `sync_harpoon_filter_set` (called from `app`
//! and siblings) are `pub`; `apply_inner` stays private.

use anyhow::Result;

use crate::fs;
use crate::keymap::Action;
use crate::pane::PaneTabs;
use crate::spyc_debug;
use crate::state::Harpoon;
use crate::ui::pager::PagerView;

use super::state;
use super::{
    ActivateIntent, App, ClipMsg, Effect, Mode, PaneTextKind, PaneTextSink, Prompt, PromptKind,
    View,
};

impl App {
    /// Wrapper around the action dispatcher that reconciles
    /// project-scoped state (currently just the harpoon list) after
    /// each action. Cheap: a no-op when `state.project_home` matches
    /// the loaded harpoon's project field.
    pub fn apply(&mut self, action: &Action) -> Result<Vec<Effect>> {
        let result = self.apply_inner(action);
        self.reconcile_harpoon();
        result
    }

    /// Save the current harpoon (if any) and load a fresh one when
    /// `state.project_home` has shifted. Also flips `harpoon` on/off
    /// when `PROJECT_HOME` is set/unset.
    pub fn reconcile_harpoon(&mut self) {
        let want = self.state.project_home.as_deref();
        let have = self.state.harpoon.as_ref().map(|h| h.project.as_path());
        if want == have {
            return;
        }
        // Save the outgoing list before we drop it.
        if let Some(h) = self.state.harpoon.as_ref()
            && let Err(e) = h.save()
        {
            spyc_debug!("harpoon save on PROJECT_HOME swap failed: {e}");
        }
        self.state.harpoon = want.map(Harpoon::load);
        // Close the menu if it's open — its cursor referenced the old
        // list and would point at stale rows.
        self.harpoon_menu = None;
        self.sync_harpoon_filter_set();
        // If `=h` was active, the now-stale set may render an empty
        // list silently; rebuild rows so the user sees the new state.
        if matches!(self.state.temp_filter.as_deref(), Some("h")) {
            self.state.rebuild_rows();
        }
    }

    /// Refresh `state.harpoon_filter_set` from the active harpoon.
    /// Call after any list mutation (append/remove/swap/delete) so
    /// `=h` reflects the new state on the next `rebuild_rows`.
    pub fn sync_harpoon_filter_set(&mut self) {
        self.state.harpoon_filter_set = self
            .state
            .harpoon
            .as_ref()
            .map(|h| h.ancestor_set().clone())
            .unwrap_or_default();
    }

    fn apply_inner(&mut self, action: &Action) -> Result<Vec<Effect>> {
        spyc_debug!(
            "apply {:?}: cursor={} vt={} grid={}x{} pp={} len={}",
            action,
            self.state.cursor.index,
            self.state.cursor.view_top,
            self.state.grid_dims.cols,
            self.state.grid_dims.rows_per_col,
            self.state.grid_dims.items_per_page(),
            self.state.rows.len(),
        );

        // In dir view, `p` (Drop) means "put inventory to cwd".
        if *action == Action::Drop && self.state.view == View::Dir {
            return Ok(self.put_inventory_to_cwd());
        }

        // yp — yank visible pane output to system clipboard. MVU Phase 5:
        // emit a `ReadPaneText` effect so the live-pane read + guards run in
        // `run_effects` (handler stays pure-Model, no Runtime read).
        if *action == Action::YankPrompt {
            return Ok(vec![Effect::ReadPaneText {
                kind: PaneTextKind::Visible,
                then: PaneTextSink::Clipboard {
                    ok: ClipMsg::PaneLines,
                },
            }]);
        }
        // yP — yank last typed pane prompt to system clipboard.
        if *action == Action::YankLastPrompt {
            return Ok(self.yank_last_prompt_to_clipboard());
        }
        // ya — yank full pane scrollback to system clipboard. MVU Phase 5:
        // emits `ReadPaneText` (see `yp`).
        if *action == Action::YankScrollback {
            return Ok(vec![Effect::ReadPaneText {
                kind: PaneTextKind::Scrollback(10_000),
                then: PaneTextSink::Clipboard {
                    ok: ClipMsg::Scrollback,
                },
            }]);
        }
        // yf — yank cursor file's absolute path (or all picks,
        // newline-separated) to system clipboard.
        if *action == Action::YankPaths {
            return Ok(self.yank_paths_to_clipboard());
        }

        // Try pure-domain dispatch first.
        match self.state.apply(action) {
            state::ApplyResult::Handled => {
                // Yolo mode: `[delete] confirm = false` opts out of
                // the y/N prompt. The pure-domain dispatch set up
                // the prompt and `pending_delete_preview` as
                // normal; synthesize the `y` keystroke here so the
                // deletion fires in the same tick — no warning
                // highlight ever paints.
                if !self.state.config.delete.confirm
                    && matches!(
                        self.state.mode,
                        Mode::Prompting(ref p) if matches!(p.kind, PromptKind::RemoveConfirm)
                    )
                {
                    let synthetic = crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char('y'),
                        crossterm::event::KeyModifiers::NONE,
                    );
                    return Ok(self.handle_remove_confirm_key(synthetic));
                }
                return Ok(Vec::new());
            }
            state::ApplyResult::Post(post) => return Ok(post),
            state::ApplyResult::OpenPager(req) => {
                let view = match req.lines {
                    state::PagerLines::Plain(lines) => {
                        let mut v = PagerView::new_plain(req.title, lines);
                        v.columns = req.columns;
                        if req.fit_to_content {
                            v.fit_to_content = true;
                            // Line-number gutter is noise for short summaries.
                            v.show_line_numbers = false;
                        }
                        v
                    }
                };
                self.set_pager(view);
                return Ok(Vec::new());
            }
            state::ApplyResult::NotHandled => {}
        }

        // Terminal-touching arms that must stay in App. Most arms mutate
        // and produce no effects; the few that do (pane send/pipe) assign
        // into `effects`, which is returned after the post-match cursor
        // clamp (so the clamp still runs for those arms, as before).
        let mut effects: Vec<Effect> = Vec::new();
        match action {
            Action::EnterOrDisplay => {
                let post = self.activate(ActivateIntent::Display);
                self.state.cursor.clamp(self.state.rows.len());
                return Ok(post);
            }
            Action::EnterOrEdit => {
                let post = self.activate(ActivateIntent::Edit);
                self.state.cursor.clamp(self.state.rows.len());
                return Ok(post);
            }
            Action::EditInPane => {
                self.edit_in_pane();
                return Ok(Vec::new());
            }
            Action::DisplayInPane => {
                self.display_in_pane();
                return Ok(Vec::new());
            }

            Action::ChmodAdd(mode_char) => {
                let paths = self.state.selection_paths();
                if paths.is_empty() {
                    return Ok(Vec::new());
                }
                let bits: u32 = match mode_char {
                    'w' => 0o200,
                    'x' => 0o111,
                    _ => return Ok(Vec::new()),
                };
                let count = paths.len();
                self.run_and_flash(
                    fs::ops::chmod_add_bits(&paths, bits),
                    format!("chmod +{mode_char} on {count} item(s)"),
                );
                self.state.refresh_listing();
            }

            Action::Help => self.open_help(),

            Action::SortCycle => {
                self.state.sort_order = self.state.sort_order.cycle_next();
                self.state
                    .listing
                    .sort(self.state.sort_order, self.state.sort_reversed);
                self.state.rebuild_rows();
                let suffix = if self.state.sort_reversed {
                    " (reversed)"
                } else {
                    ""
                };
                self.state
                    .flash_info(format!("sort: {}{}", self.state.sort_order, suffix));
            }

            Action::SortReverse => {
                self.state.sort_reversed = !self.state.sort_reversed;
                self.state
                    .listing
                    .sort(self.state.sort_order, self.state.sort_reversed);
                self.state.rebuild_rows();
                let suffix = if self.state.sort_reversed {
                    " (reversed)"
                } else {
                    ""
                };
                self.state
                    .flash_info(format!("sort: {}{}", self.state.sort_order, suffix));
            }

            Action::OpenTaskViewer => self.open_task_viewer(None),

            Action::ReopenLastBuffer => {
                if let Some(prev) = self.pager_history.pop_back() {
                    self.pager = Some(prev);
                    self.needs_full_repaint = true;
                    self.state
                        .flash_info(format!("buffer ←{}", self.pager_history.back_len()));
                } else {
                    self.state.flash_info("no buffers in history");
                }
            }

            Action::FindFile => self.open_find_picker(),

            Action::ReloadConfig => self.reload_config(),

            Action::TogglePane
            | Action::ResumePane
            | Action::PaneFocusDown
            | Action::PaneFocusUp
            | Action::PaneSendSelection
            | Action::PaneGrow
            | Action::PaneShrink
            | Action::TogglePaneZoom
            | Action::PaneScrollEnter
            | Action::PaneScrollSave
            | Action::PaneNewTab
            | Action::PaneCloseTab
            | Action::PaneTabByIndex(_)
            | Action::PaneNextTab
            | Action::PanePrevTab
            | Action::PaneLastTab
            | Action::PaneRenameTab
            | Action::PaneRestartTab
            | Action::HarpoonJump(_)
            | Action::HarpoonAppend
            | Action::HarpoonRemove
            | Action::HarpoonOpenMenu
            | Action::PanePipeContent
            | Action::PanePipeInventory
                if matches!(
                    self.state.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::PaneNewTabCmd
                            | PromptKind::PaneNewTabCwd
                            | PromptKind::PaneRenameTab,
                        ..
                    })
                ) =>
            {
                self.cancel_prompt();
                return self.apply(action);
            }

            Action::TogglePane => self.toggle_pane(),
            Action::ResumePane => self.open_pane_tab("claude --resume"),
            Action::PaneFocusDown => self.set_pane_focus(true),
            Action::PaneFocusUp => self.set_pane_focus(false),
            Action::PaneSendSelection => effects = self.send_selection_to_pane(),
            Action::PaneGrow => self.resize_pane(5),
            Action::PaneShrink => self.resize_pane(-5),
            Action::TogglePaneZoom => self.toggle_pane_zoom(),
            Action::PaneScrollEnter => {
                self.open_pane_scroll_pager();
            }
            Action::PaneScrollSave => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    match tabs.active_mut().save_to_file() {
                        Ok(path) => {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            self.state.flash_info(format!("saved: {name}"));
                        }
                        Err(e) => self.state.flash_info(format!("save error: {e}")),
                    }
                }
            }

            Action::PaneNewTab => self.start_new_tab_prompt(),
            Action::PaneCloseTab => self.close_active_tab(),
            Action::PaneTabByIndex(n) => {
                self.stash_scrollback_pager_to_active_tab();
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.switch_to((*n as usize).saturating_sub(1));
                    // Switching tabs implies "I want to interact
                    // with this other tab" — pull focus into the
                    // pane so the next keystroke lands in the
                    // child, not the file list. Matches the
                    // behavior of `^a c` (new tab) which already
                    // does this in `open_pane_tab_in`.
                    self.state.focus = state::Focus::Pane;
                }
                self.restore_active_tab_scrollback_pager();
                self.needs_full_repaint = true;
            }
            Action::PaneNextTab => {
                self.stash_scrollback_pager_to_active_tab();
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.next();
                    self.state.focus = state::Focus::Pane;
                }
                self.restore_active_tab_scrollback_pager();
                self.needs_full_repaint = true;
            }
            Action::PanePrevTab => {
                self.stash_scrollback_pager_to_active_tab();
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.prev();
                    self.state.focus = state::Focus::Pane;
                }
                self.restore_active_tab_scrollback_pager();
                self.needs_full_repaint = true;
            }
            Action::PaneLastTab => {
                self.stash_scrollback_pager_to_active_tab();
                let jumped = self
                    .pane_tabs
                    .as_mut()
                    .is_some_and(PaneTabs::switch_to_last);
                if jumped {
                    // Same rationale as PaneTabByIndex: a tab switch
                    // means "interact with that tab", so pull focus
                    // into the pane.
                    self.state.focus = state::Focus::Pane;
                    self.needs_full_repaint = true;
                } else {
                    self.state.flash_info("no previous tab");
                }
                // Runs in both cases: on a jump it surfaces the target
                // tab's stash; on a no-op it round-trips the pager we
                // just stashed back onto the (unchanged) active tab.
                self.restore_active_tab_scrollback_pager();
            }
            Action::PaneRenameTab => {
                if let Some(tabs) = self.pane_tabs.as_ref() {
                    let current = tabs.active_info().label.clone();
                    let mut p = Prompt::shell(PromptKind::PaneRenameTab, "tab name: ");
                    p.buffer.clone_from(&current);
                    if let Some(ed) = p.editor.as_mut() {
                        ed.set_content(&current);
                    }
                    self.state.mode = Mode::Prompting(p);
                }
            }

            Action::PaneRestartTab => self.restart_active_tab(),

            Action::PanePipeContent => effects = self.pipe_content_to_pane(false),
            Action::PanePipeInventory => effects = self.pipe_content_to_pane(true),

            Action::QuickSelectOpen => self.open_quick_select(),
            Action::OpenGraveyardView => {
                self.state.open_graveyard_view();
                // Discoverability hint on entry only — open_graveyard_view
                // toggles, so check the post-call view to distinguish
                // enter vs exit. Justin reported being unable to figure
                // out the restore chord from inside the view; the flash
                // surfaces the two main ones plus `?` for the rest.
                if matches!(self.state.view, View::Graveyard) {
                    self.state
                        .flash_info("graveyard: p restore here · P original · dd/x purge · ? help");
                }
            }
            Action::HarpoonJump(n) => self.harpoon_jump(*n),
            Action::HarpoonAppend => self.harpoon_append(),
            Action::HarpoonRemove => self.harpoon_remove(),
            Action::HarpoonOpenMenu => self.harpoon_open_menu(),

            Action::WorktreeList => self.worktree_list(),

            Action::GitDiff | Action::GitDiffCached => {
                let cached = matches!(action, Action::GitDiffCached);
                if self.state.git.info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_diff(cached);
                }
            }
            Action::GitBlame => {
                if self.state.git.info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_blame();
                }
            }

            Action::ShowMemory => self.show_session_info(),
            Action::ColorToggle => {
                self.theme = self.theme.toggled();
                self.state.flash_info(if self.theme.mono {
                    "colors off"
                } else {
                    "colors on"
                });
            }

            Action::ToggleActivity => {
                self.show_activity = !self.show_activity;
                self.state.flash_info(if self.show_activity {
                    "activity monitor on"
                } else {
                    "activity monitor off"
                });
            }

            Action::Redraw => {
                self.needs_full_repaint = true;
            }
            Action::Quit => self.request_quit(),

            Action::GotoFile | Action::GotoFileLine => {
                // Emit a `ReadPaneText`/`GotoFile` effect (PR 5b): the executor
                // reads the pickable lines + the pane's cwd and navigates, so
                // the live-pane read lives in `run_effects`.
                effects = vec![Effect::ReadPaneText {
                    kind: PaneTextKind::Pickable(200),
                    then: PaneTextSink::GotoFile {
                        open_at_line: matches!(action, Action::GotoFileLine),
                    },
                }];
            }

            // All other actions were already handled by `self.state.apply()`.
            _ => {}
        }
        self.state.cursor.clamp(self.state.rows.len());
        Ok(effects)
    }
}

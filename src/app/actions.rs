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

use super::git_view_session::DiffScope;
use super::state;
use super::{
    ActivateIntent, App, ClipMsg, Effect, Mode, PaneTextKind, PaneTextSink, Prompt, PromptKind,
    View,
};

impl App {
    /// Wrapper around the action dispatcher that reconciles each column's
    /// per-worktree harpoon list after every action. Cheap: a no-op for a
    /// column whose [`AppState::harpoon_root`] matches its loaded list.
    pub fn apply(&mut self, action: &Action) -> Result<Vec<Effect>> {
        let result = self.apply_inner(action);
        self.reconcile_harpoon();
        result
    }

    /// Reconcile every active column's harpoon list: save + reload a column's
    /// list when its [`AppState::harpoon_root`] has shifted (chdir into a
    /// different worktree, or `PROJECT_HOME` set/unset). Per-column so `b` in a
    /// separate worktree keeps its own bookmarks. Cheap: a no-op for each
    /// column whose root is unchanged (the common case every frame). A
    /// focus switch needs no reload — each column's list is already loaded for
    /// its own worktree, so `cur().harpoon` just resolves to the right one.
    pub fn reconcile_harpoon(&mut self) {
        // Explicit Left + conditional Right (not `active_sides()`) so the
        // immutable iterator borrow doesn't collide with the `&mut self` loop
        // body — same shape as `refresh_git_state`.
        self.reconcile_harpoon_for(state::Side::Left);
        if self.state.right.is_some() {
            self.reconcile_harpoon_for(state::Side::Right);
        }
    }

    fn reconcile_harpoon_for(&mut self, side: state::Side) {
        let want = self.state.harpoon_root(side);
        let have = self
            .state
            .col(side)
            .harpoon
            .as_ref()
            .map(|h| h.project.clone());
        if want == have {
            return;
        }
        // Save the outgoing list before we drop it.
        if let Some(h) = self.state.col(side).harpoon.as_ref()
            && let Err(e) = h.save()
        {
            spyc_debug!("harpoon save on root swap failed: {e}");
        }
        self.state.col_mut(side).harpoon = want.as_deref().map(Harpoon::load);
        self.sync_harpoon_filter_set_for(side);
        if side == self.state.focused_side() {
            // The menu's cursor referenced the old list — close it.
            self.view.harpoon_menu = None;
            // If `=h` was active, the now-stale set may render an empty list
            // silently; rebuild rows so the user sees the new state. Only the
            // focused column's rows are rebuilt by `rebuild_rows`.
            if matches!(self.state.cur().temp_filter.as_deref(), Some("h")) {
                self.state.rebuild_rows();
            }
        }
    }

    /// Refresh the focused column's `harpoon_filter_set` from its harpoon.
    /// Call after any list mutation (append/remove/swap/delete) so `=h`
    /// reflects the new state on the next `rebuild_rows`.
    pub fn sync_harpoon_filter_set(&mut self) {
        self.sync_harpoon_filter_set_for(self.state.focused_side());
    }

    fn sync_harpoon_filter_set_for(&mut self, side: state::Side) {
        let set = self
            .state
            .col(side)
            .harpoon
            .as_ref()
            .map(|h| h.ancestor_set().clone())
            .unwrap_or_default();
        self.state.col_mut(side).harpoon_filter_set = set;
    }

    fn apply_inner(&mut self, action: &Action) -> Result<Vec<Effect>> {
        spyc_debug!(
            "apply {:?}: cursor={} vt={} grid={}x{} pp={} len={}",
            action,
            self.state.cur().cursor.index,
            self.state.cur().cursor.view_top,
            self.state.cur().grid_dims.cols,
            self.state.cur().grid_dims.rows_per_col,
            self.state.cur().grid_dims.items_per_page(),
            self.state.cur().rows.len(),
        );

        // In dir view, `p` (Drop) means "put inventory to cwd".
        if *action == Action::Drop && self.state.cur().view == View::Dir {
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

        // Try pure-domain dispatch first, normalized to the unified `Update`
        // (MVU Stage 3C — `Handled` and `Post` collapse into `Handled(fx)`).
        match state::Update::from(self.state.apply(action)) {
            state::Update::Handled(post) => {
                // Yolo mode: `[delete] confirm = false` opts out of
                // the y/N prompt. The pure-domain dispatch set up
                // the prompt and `pending_delete_preview` as
                // normal; synthesize the `y` keystroke here so the
                // deletion fires in the same tick — no warning
                // highlight ever paints. The check is gated on
                // `mode == Prompting(RemoveConfirm)`, which only the
                // delete action (old `Handled`, empty `post`) sets —
                // so merging old `Handled`+`Post` here is safe: a
                // `Post` result never has that mode and falls to
                // `Ok(post)` unchanged.
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
                return Ok(post);
            }
            state::Update::OpenPager(req) => {
                self.open_pager_request(req);
                return Ok(Vec::new());
            }
            state::Update::Quit => {
                // `ApplyResult` has no `Quit` variant, so this is
                // unreachable today; handle defensively (no panic on a
                // daily-driver path) in case a future Action maps to it.
                self.request_quit();
                return Ok(Vec::new());
            }
            state::Update::Defer => {}
        }

        // Terminal-touching arms that must stay in App. Most arms mutate
        // and produce no effects; the few that do (pane send/pipe) assign
        // into `effects`, which is returned after the post-match cursor
        // clamp (so the clamp still runs for those arms, as before).
        let mut effects: Vec<Effect> = Vec::new();
        match action {
            Action::EnterOrDisplay => {
                let post = self.activate(ActivateIntent::Display);
                let n = self.state.cur().rows.len();
                self.state.cur_mut().cursor.clamp(n);
                return Ok(post);
            }
            Action::EnterOrEdit => {
                let post = self.activate(ActivateIntent::Edit);
                let n = self.state.cur().rows.len();
                self.state.cur_mut().cursor.clamp(n);
                return Ok(post);
            }
            Action::EditInPane => {
                self.edit_in_pane();
                return Ok(Vec::new());
            }
            Action::DisplayInPane => {
                return Ok(self.display_in_pane());
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
                let next = self.state.cur().sort_order.cycle_next();
                self.state.cur_mut().sort_order = next;
                self.state.apply_sort();
                let suffix = if self.state.cur().sort_reversed {
                    " (reversed)"
                } else {
                    ""
                };
                self.state
                    .flash_info(format!("sort: {}{}", self.state.cur().sort_order, suffix));
            }

            Action::OpenTaskViewer => self.open_task_viewer(None),

            Action::ReopenLastBuffer => {
                if let Some(prev) = self.view.pager_history.pop_back() {
                    self.view.pager = Some(prev);
                    self.view.needs_full_repaint = true;
                    self.state
                        .flash_info(format!("buffer ←{}", self.view.pager_history.back_len()));
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
            | Action::PaneSendPrefix
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
            | Action::VsplitCycle
            | Action::VsplitFocusLeft
            | Action::VsplitFocusRight
            | Action::ToggleDim
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
            Action::PaneSendPrefix => effects = self.send_prefix_to_pane(),
            // Context-sensitive: resize the split the focused pane belongs to
            // — the vertical split's width when a column is focused, else the
            // bottom pane's height.
            Action::PaneGrow => self.resize_focused_split(5),
            Action::PaneShrink => self.resize_focused_split(-5),
            Action::VsplitCycle => self.cycle_vsplit(),
            Action::VsplitFocusLeft => self.vsplit_focus(state::Side::Left),
            Action::VsplitFocusRight => self.vsplit_focus(state::Side::Right),
            Action::OpenSecondCommander => self.open_second_commander(),
            Action::CloseSecondCommander => self.close_second_commander(),
            // `^d`: close the second commander if one is open, else quit. The
            // no-split quit path (`request_quit`) keeps its own two-tap "press
            // again to quit" confirm — so quitting is still `^d^d` with a
            // warning, while closing the split is a single `^d`.
            Action::QuitOrCloseCommander => {
                if self.state.right.is_some() {
                    self.close_second_commander();
                } else {
                    self.request_quit();
                }
            }
            Action::ToggleDim => {
                self.view.dim_inactive = !self.view.dim_inactive;
                self.view.needs_full_repaint = true;
                self.state.flash_info(if self.view.dim_inactive {
                    "dim: on"
                } else {
                    "dim: off"
                });
            }
            Action::TogglePaneZoom => self.toggle_pane_zoom(),
            Action::PaneScrollEnter => {
                self.open_pane_scroll_pager();
            }
            Action::PaneScrollSave => {
                let result = self
                    .runtime
                    .pane_tabs
                    .as_mut()
                    .map(|tabs| tabs.active_mut().save_to_file());
                if let Some(result) = result {
                    self.state.flash_saved_file(result);
                }
            }

            Action::PaneNewTab => self.start_new_tab_prompt(),
            Action::PaneCloseTab => self.close_active_tab(),
            Action::PaneTabByIndex(n) => {
                // Switching tabs implies "I want to interact with this other
                // tab" — pull focus into the pane. From a fullscreen list
                // (`TopList` zoom), it instead fullscreens the chosen tab (see
                // `fullscreen_tab_if_list_zoomed`), navigating between
                // fullscreen views. Matches `^a c` (new tab).
                self.stash_scrollback_pager_to_active_tab();
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.switch_to((*n as usize).saturating_sub(1));
                    self.state.focus = state::Focus::Pane;
                }
                self.fullscreen_tab_if_list_zoomed();
                self.restore_active_tab_scrollback_pager();
                self.view.needs_full_repaint = true;
            }
            Action::PaneNextTab => {
                self.stash_scrollback_pager_to_active_tab();
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.next();
                    self.state.focus = state::Focus::Pane;
                }
                self.fullscreen_tab_if_list_zoomed();
                self.restore_active_tab_scrollback_pager();
                self.view.needs_full_repaint = true;
            }
            Action::PanePrevTab => {
                self.stash_scrollback_pager_to_active_tab();
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.prev();
                    self.state.focus = state::Focus::Pane;
                }
                self.fullscreen_tab_if_list_zoomed();
                self.restore_active_tab_scrollback_pager();
                self.view.needs_full_repaint = true;
            }
            Action::PaneLastTab => {
                self.stash_scrollback_pager_to_active_tab();
                let jumped = self
                    .runtime
                    .pane_tabs
                    .as_mut()
                    .is_some_and(PaneTabs::switch_to_last);
                if jumped {
                    // Same rationale as PaneTabByIndex: a tab switch
                    // means "interact with that tab", so pull focus
                    // into the pane.
                    self.state.focus = state::Focus::Pane;
                    self.fullscreen_tab_if_list_zoomed();
                    self.view.needs_full_repaint = true;
                } else {
                    self.state.flash_info("no previous tab");
                }
                // Runs in both cases: on a jump it surfaces the target
                // tab's stash; on a no-op it round-trips the pager we
                // just stashed back onto the (unchanged) active tab.
                self.restore_active_tab_scrollback_pager();
            }
            Action::PaneRenameTab => {
                if let Some(tabs) = self.runtime.pane_tabs.as_ref() {
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
                if matches!(self.state.cur().view, View::Graveyard) {
                    self.state
                        .flash_info("graveyard: p restore here · P original · dd/x purge · ? help");
                }
            }
            Action::HarpoonJump(n) => self.harpoon_jump(*n),
            Action::HarpoonAppend => self.harpoon_append(),
            Action::HarpoonRemove => self.harpoon_remove(),
            Action::HarpoonOpenMenu => self.harpoon_open_menu(),

            Action::WorktreeList => self.worktree_list(),

            Action::GitDiff | Action::GitDiffCached | Action::GitDiffUnstaged => {
                let scope = match action {
                    Action::GitDiffCached => DiffScope::Cached,
                    Action::GitDiffUnstaged => DiffScope::IndexToWorktree,
                    _ => DiffScope::HeadToWorktree,
                };
                if self.state.cur().git.info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_diff(scope);
                }
            }
            Action::GitBlame => {
                if self.state.cur().git.info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_blame();
                }
            }
            Action::GitRestore => {
                effects = self.git_restore_cursor();
            }

            Action::ShowMemory => self.show_session_info(),
            Action::ColorToggle => {
                self.view.theme = self.view.theme.toggled();
                self.state.flash_info(if self.view.theme.mono {
                    "colors off"
                } else {
                    "colors on"
                });
            }

            Action::ToggleActivity => {
                self.view.show_activity = !self.view.show_activity;
                self.state.flash_info(if self.view.show_activity {
                    "activity monitor on"
                } else {
                    "activity monitor off"
                });
            }

            Action::Redraw => {
                self.view.needs_full_repaint = true;
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
        let row_count = self.state.cur().rows.len();
        self.state.cur_mut().cursor.clamp(row_count);
        Ok(effects)
    }
}

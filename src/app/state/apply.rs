//! `AppState::apply` — the pure `Action` dispatcher (the MVU update core).
//! Split from `state` verbatim.

use std::path::{Path, PathBuf};

use crate::fs;
use crate::keymap::Action;

use crate::app::{Effect, Mode, PostAction, Prompt, PromptKind, View};

use super::{AppState, ApplyResult, PagerLines, PagerRequest};

use super::count_files_in_dir;

impl AppState {
    /// Handle the pure-domain arms of `Action` dispatch.
    ///
    /// Returns `ApplyResult::Handled` when the action was fully processed
    /// (cursor is clamped before returning), `ApplyResult::OpenPager` when
    /// the caller should open a pager, `ApplyResult::Post` for a `PostAction`,
    /// or `ApplyResult::NotHandled` when the caller must handle the action
    /// (terminal-touching: pager, pane, theme, redraw, etc.).
    pub fn apply(&mut self, action: &Action) -> ApplyResult {
        let len = self.rows.len();
        let rows_per_col = self.grid_dims.rows_per_col as usize;
        let per_page = self.grid_dims.items_per_page();

        match action {
            // -- Cursor motion --
            Action::Up(n) => {
                if !self.cursor_move_vertical(-(*n as isize), rows_per_col, len) {
                    self.flash_info("~");
                }
            }
            Action::Down(n) => {
                if !self.cursor_move_vertical(*n as isize, rows_per_col, len) {
                    self.flash_info("~");
                }
            }
            Action::Left(n) => {
                if !self.cursor_move_columns(-(*n as isize), rows_per_col, len) {
                    self.flash_info("~");
                }
            }
            Action::Right(n) => {
                if !self.cursor_move_columns(*n as isize, rows_per_col, len) {
                    self.flash_info("~");
                }
            }
            Action::PageUp => self.cursor_move_global(-(per_page as isize), len),
            Action::PageDown => self.cursor_move_global(per_page as isize, len),
            Action::GotoFirst => self.goto_col_top(rows_per_col),
            Action::GotoLast => self.goto_col_bottom(rows_per_col, len),

            // ]g / [g — cursor to next/prev git-changed entry. Wraps
            // when there's no match in the desired direction so the
            // user can keep pressing the chord without thinking about
            // direction. No-op flash when the listing has no changes.
            Action::JumpNextGitChange => {
                if !self.jump_to_git_change(true) {
                    self.flash_info("no git changes in this directory");
                }
            }
            Action::JumpPrevGitChange => {
                if !self.jump_to_git_change(false) {
                    self.flash_info("no git changes in this directory");
                }
            }

            // -- Navigation --
            Action::Climb => return ApplyResult::Post(self.climb()),
            Action::Home => {
                if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                    return ApplyResult::Post(vec![Effect::ChangeDir {
                        path: home,
                        focus: None,
                        on_ok: None,
                        err_prefix: "chdir",
                    }]);
                }
            }

            // -- Picks --
            Action::TogglePick => self.toggle_pick_cursor(),
            Action::PickPatternPrompt => {
                if self.view == View::Dir {
                    self.mode =
                        Mode::Prompting(Prompt::simple(PromptKind::PatternPick, "pick pattern: "));
                }
            }
            Action::PickToggleAll => self.toggle_all_picks(),

            // -- Inventory --
            Action::Take => match self.take() {
                Some(msg) if msg.starts_with("yanked") => self.flash_info(msg),
                Some(err) => self.flash_error(err),
                None => {}
            },
            Action::Untake => {
                if self.view != View::Dir {
                    return ApplyResult::Handled;
                }
                if let Some(row) = self.rows.get(self.cursor.index) {
                    let path = row.path.clone();
                    if self.inventory.contains(&path) {
                        // Find and remove by original path.
                        let id = self
                            .inventory
                            .items()
                            .find(|i| i.orig_path == path)
                            .map(|i| i.id.clone());
                        if let Some(id) = id {
                            self.inventory.remove_by_id(&id);
                            self.flash_info("removed from inventory");
                        }
                    } else {
                        self.flash_error("not in inventory");
                    }
                }
                self.rebuild_rows();
            }
            Action::Drop => {
                // In dir view, p = put (handled by App, not here).
                // This arm only fires from inventory view fallthrough.
                self.drop_cursor();
            }
            Action::ToggleInventoryView => self.toggle_inventory_view(),
            Action::EmptyInventory => {
                self.inventory.clear();
                self.rebuild_rows();
            }

            // -- Masks & filtering --
            Action::ToggleMask(n) => {
                if *n == 1 {
                    self.masks.toggle_mask1();
                } else if *n == 2 {
                    self.masks.toggle_mask2();
                }
                self.rebuild_rows();
            }
            Action::LimitPrompt => {
                let prefix = if self.temp_filter.is_some() {
                    "limit (active)="
                } else {
                    "limit="
                };
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::Limit, prefix));
            }
            Action::CommandPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::Command, ":"));
            }

            // -- Shell prompts --
            Action::ShellCapturedPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmdCaptured, "!"));
            }
            Action::ShellForegroundPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmd, ";"));
            }
            Action::StartShell => {
                let sh = crate::envset::var("SHELL").unwrap_or_else(|| "/bin/sh".into());
                return ApplyResult::Post(
                    PostAction::Spawn {
                        program: sh,
                        args: vec![],
                        pause_after: false,
                    }
                    .into(),
                );
            }

            // -- Search --
            Action::SearchPrompt => {
                self.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::Search {
                        saved_cursor: self.cursor.index,
                    },
                    "/",
                ));
            }
            Action::SearchNext => {
                if let Some(term) = self.last_search.clone() {
                    let n = self.rows.len();
                    if n > 0 {
                        let start = (self.cursor.index + 1) % n;
                        if let Some(i) = self.find_match(&term, start, false) {
                            self.cursor.index = i;
                        }
                    }
                }
            }
            Action::SearchPrev => {
                if let Some(term) = self.last_search.clone() {
                    let n = self.rows.len();
                    if n > 0 {
                        let start = if self.cursor.index == 0 {
                            n - 1
                        } else {
                            self.cursor.index - 1
                        };
                        if let Some(i) = self.find_match(&term, start, true) {
                            self.cursor.index = i;
                        }
                    }
                }
            }

            // -- Navigation prompts --
            Action::JumpPrompt => {
                // Vi line editor so the user can pull up a history
                // entry (j/k in Normal, Up/Down anywhere) and tweak
                // it before submitting -- e.g. recall ~/src/spyc
                // and append `/src` before pressing Enter.
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::Jump, "jump to: "));
            }

            // -- File operation prompts --
            Action::CopyPrompt => {
                if !self.selection_paths().is_empty() {
                    // `shell` constructor gives the prompt a vi line
                    // editor so the user can navigate / edit the
                    // destination path with familiar bindings (w b
                    // 0 $ cw etc.). Up/Down history nav is skipped
                    // for path prompts in `handle_vi_prompt_key`
                    // so the shell command history doesn't surface.
                    self.mode = Mode::Prompting(Prompt::shell(PromptKind::CopyTo, "copy to: "));
                }
            }
            Action::MovePrompt => {
                if !self.selection_paths().is_empty() {
                    self.mode = Mode::Prompting(Prompt::shell(PromptKind::MoveTo, "move to: "));
                }
            }
            Action::MakeDirPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::MakeDir, "mkdir: "));
            }
            Action::NewFilePrompt => {
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::NewFile, "new file: "));
            }
            Action::RemovePrompt(count) => {
                // `count.is_some()` = explicit `Ndd` from the user.
                // None = bare `R` or bare `dd` → picks-or-cursor
                // semantics (existing `R` behavior).
                let paths: Vec<PathBuf> = if let Some(n) = count {
                    // Cursor + (n-1) entries below, clamped at end
                    // of list. No wrap. Ignores picks — the count
                    // is the user being explicit.
                    let start = self.cursor.index;
                    self.rows
                        .iter()
                        .skip(start)
                        .take(*n)
                        .map(|r| r.path.clone())
                        .collect()
                } else {
                    self.selection_paths()
                        .into_iter()
                        .map(Path::to_path_buf)
                        .collect()
                };
                if paths.is_empty() {
                    return ApplyResult::Handled;
                }
                // Borrow the slice for the rest of the function.
                let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
                // Pre-walk to count files inside any selected dirs so
                // the user sees the actual blast radius of `R`. Cheap
                // (interactive flow, sub-second on any sane subtree)
                // and load-bearing for safety: today's prompt just
                // says "N file(s)?" which a user can reflexively `y`
                // their way through, even if N includes a directory
                // tree that would recursively delete thousands.
                let mut file_count = 0u64;
                let mut dir_count = 0u64;
                let mut dir_files = 0u64;
                for p in &paths {
                    match std::fs::symlink_metadata(p) {
                        Ok(md) if md.is_dir() => {
                            dir_count += 1;
                            dir_files += count_files_in_dir(p);
                        }
                        _ => file_count += 1,
                    }
                }
                let prompt = if dir_count == 0 {
                    format!("remove {file_count} file(s)? (y/N): ")
                } else if file_count == 0 && dir_count == 1 {
                    format!("remove DIR (recursive, {dir_files} file(s))? (y/N): ")
                } else if file_count == 0 {
                    format!("remove {dir_count} dir(s) (recursive, {dir_files} file(s))? (y/N): ")
                } else {
                    format!(
                        "remove {file_count} file(s) + {dir_count} dir(s) (recursive, {dir_files} file(s))? (y/N): "
                    )
                };
                // Capture the targeted paths so the list view can
                // highlight them in the warning color while the
                // confirm prompt is up. Cleared on confirm/cancel
                // in `handle_remove_confirm_key`.
                self.pending_delete_preview =
                    Some(paths.iter().map(|p| (*p).to_path_buf()).collect());
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::RemoveConfirm, prompt));
            }

            // -- Long listing (pager) --
            Action::LongList => {
                let owned: Vec<PathBuf>;
                let paths: Vec<&Path> = if self.selection_paths().is_empty() {
                    owned = self
                        .listing
                        .entries
                        .iter()
                        .map(|e| e.path.clone())
                        .collect();
                    owned.iter().map(PathBuf::as_path).collect()
                } else {
                    self.selection_paths()
                };
                let lines = fs::long_listing::format_long_listing(&paths);
                let title = format!("long listing — {}", self.listing.dir.display());
                self.cursor.clamp(self.rows.len());
                return ApplyResult::OpenPager(PagerRequest {
                    title,
                    lines: PagerLines::Plain(lines),
                    columns: 1,
                    fit_to_content: true,
                });
            }

            // -- File type --
            Action::FileType => {
                let paths = self.selection_paths();
                if paths.is_empty() {
                    self.cursor.clamp(self.rows.len());
                    return ApplyResult::Post(Vec::new());
                }
                if paths.len() == 1 {
                    let label = fs::ops::file_type_label(paths[0]);
                    let name = paths[0].file_name().map_or_else(
                        || paths[0].display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    self.flash_info(format!("{name}: {label}"));
                } else {
                    let lines: Vec<String> = paths
                        .iter()
                        .map(|p| {
                            let name = p.file_name().map_or_else(
                                || p.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            format!("{name}: {}", fs::ops::file_type_label(p))
                        })
                        .collect();
                    self.cursor.clamp(self.rows.len());
                    return ApplyResult::OpenPager(PagerRequest {
                        title: "file types".to_string(),
                        lines: PagerLines::Plain(lines),
                        columns: 1,
                        fit_to_content: false,
                    });
                }
            }

            // -- Marks --
            Action::SetMark(letter) => self.set_mark(*letter),
            Action::JumpMark(letter) => return ApplyResult::Post(self.jump_to_mark(*letter)),
            Action::JumpStartDir => {
                return ApplyResult::Post(vec![Effect::ChangeDir {
                    path: self.start_dir.clone(),
                    focus: None,
                    on_ok: None,
                    err_prefix: "jump to start failed",
                }]);
            }
            Action::JumpProjectHome => match self.project_home.clone() {
                Some(dir) => {
                    return ApplyResult::Post(vec![Effect::ChangeDir {
                        path: dir,
                        focus: None,
                        on_ok: None,
                        err_prefix: "jump to project home failed",
                    }]);
                }
                None => self.flash_error("PROJECT_HOME not set (gP to set, :project)"),
            },
            Action::SetProjectHomeHere => {
                let dir = self.listing.dir.clone();
                self.flash_info(format!("PROJECT_HOME: {}", dir.display()));
                self.project_home = Some(dir);
            }
            Action::SetStartDirHere => {
                let dir = self.listing.dir.clone();
                self.flash_info(format!("start dir: {}", dir.display()));
                self.start_dir = dir;
            }
            Action::ShowUserHost => self.flash_info(self.user_host.clone()),
            Action::JumpPrevDir => match self.prev_dir.clone() {
                Some(prev) => {
                    return ApplyResult::Post(vec![Effect::ChangeDir {
                        path: prev,
                        focus: None,
                        on_ok: None,
                        err_prefix: "jump back failed",
                    }]);
                }
                None => self.flash_error("no previous directory"),
            },

            // -- Info --
            Action::Date => self.flash_info(crate::sysinfo::format_now()),
            Action::Version => {
                self.flash_info(format!(
                    "\u{1f336}\u{fe0f} spyc {}",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            Action::SetEnvPrompt => {
                self.mode =
                    Mode::Prompting(Prompt::simple(PromptKind::SetEnv, "setenv NAME=VALUE: "));
            }

            // -- Worktree prompts (pure state: just set mode) --
            Action::WorktreeNew => {
                if self.git.info.is_none() {
                    self.flash_error("not in a git repository");
                } else {
                    let p = Prompt::shell(PromptKind::WorktreeNewBranch, "worktree branch: ");
                    self.mode = Mode::Prompting(p);
                }
            }
            Action::WorktreeDelete => {
                if self.git.info.is_none() {
                    self.flash_error("not in a git repository");
                } else {
                    let dir = self.listing.dir.display().to_string();
                    self.mode = Mode::Prompting(Prompt::simple(
                        PromptKind::WorktreeDeleteConfirm,
                        format!("remove worktree {dir}? (y/N): "),
                    ));
                }
            }

            // -- No-op --
            Action::Noop => {}

            // -- Reserved keys (flash a hint instead of doing something
            //    unintended; the actual feature is on the roadmap) --
            Action::MacroRecordReserved => {
                self.flash_info("q reserved for future macro recording — Q or :q to quit");
            }

            // -- Everything else stays in App --
            _ => return ApplyResult::NotHandled,
        }

        self.cursor.clamp(self.rows.len());
        ApplyResult::Handled
    }

    // --- Command / prompt dispatch (pure-domain arms) ---
}

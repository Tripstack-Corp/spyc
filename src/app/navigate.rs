//! Path-reference navigation and Enter/Edit activation. `activate`
//! (the `Enter` / `e` action handler) descends dirs / displays / edits
//! the cursored file; `goto_file_navigate` (`gf` / `gF`) resolves a path
//! reference scraped from pane output and chdirs + focuses it; and
//! `jump_to_pane_path` does the same from a pre-extracted path string
//! (Quick Select's Path-open). Extracted verbatim from `app/mod.rs`
//! (the impl-extraction sweep). `jump_to_pane_path` / `goto_file_navigate`
//! are `pub` (called from `quick_select` / `effect`); `activate` is
//! `pub(super)` (called from `actions`, and capped to match the
//! app-private `ActivateIntent` it takes).

use std::path::{Path, PathBuf};

use crate::shell;
use crate::spyc_debug;
use crate::ui::pager;

use super::{ActivateIntent, App, Effect, EntryKind, PostAction, View, state};

impl App {
    /// Navigate spyc to a path matched in the pane (uppercase intent
    /// for a Path match). Mirrors `goto_file_navigate`'s post-resolve
    /// flow but starts from a pre-extracted path string rather than
    /// running pathref again.
    pub fn jump_to_pane_path(&mut self, raw: &str) {
        let path = std::path::PathBuf::from(raw);
        let resolved = if path.is_absolute() {
            path
        } else {
            // Resolve against the active pane tab's cwd first, falling
            // back to spyc's listing dir — same precedence `gf` uses.
            let tab_cwd = self
                .runtime
                .pane_tabs
                .as_ref()
                .map(|t| t.active_info().cwd.clone());
            let candidate = tab_cwd.as_ref().map(|c| c.join(&path));
            match candidate {
                Some(p) if p.exists() => p,
                _ => self.state.listing.dir.join(&path),
            }
        };
        if !resolved.exists() {
            self.state
                .flash_error(format!("path not found: {}", resolved.display()));
            return;
        }
        let (chdir_to, focus) = if resolved.is_dir() {
            (resolved, None)
        } else if let Some(parent) = resolved.parent() {
            (parent.to_path_buf(), Some(resolved.clone()))
        } else {
            self.state.flash_error("path has no parent dir");
            return;
        };
        if let Err(e) = self.state.chdir(&chdir_to) {
            self.state.flash_error(format!("chdir: {e}"));
            return;
        }
        if let Some(p) = focus {
            self.state.focus_on_path(&p);
        }
        self.state.focus = state::Focus::FileList;
        self.state.rebuild_rows();
        self.view.needs_full_repaint = true;
    }

    // ---- Path references (M13) ------------------------------------------------

    /// `gf` / `gF` — scan the active pane's visible output for a file path
    /// reference, navigate the file list there, and optionally open the
    /// pager at the referenced line.
    /// Resolve a path reference from already-read pane `lines` and navigate to
    /// it (chdir + focus); `open_at_line` (gF) also opens the file in the pager
    /// at the referenced line. The pickable read + the pane's cwd are supplied
    /// by the `ReadPaneText` / `GotoFile` executor (PR 5b) so the live-pane read
    /// lives in `run_effects` — this half stays pure of the Runtime handle.
    pub fn goto_file_navigate(
        &mut self,
        lines: Vec<String>,
        pane_cwd: PathBuf,
        open_at_line: bool,
    ) {
        // Also try resolving against the spyc cwd (project root), not just
        // the pane tab's cwd — Claude often prints paths relative to the
        // project root regardless of the shell's cwd.
        let spyc_cwd = self.state.listing.dir.clone();

        // Debug: dump visible lines to the debug log so we can see what
        // the vt100 screen actually contains.
        spyc_debug!(
            "gf: {} lines from pane, pane_cwd={}, spyc_cwd={}",
            lines.len(),
            pane_cwd.display(),
            spyc_cwd.display()
        );
        for (i, line) in lines.iter().enumerate() {
            if !line.trim().is_empty() {
                spyc_debug!("gf line[{i}]: {:?}", line);
            }
        }

        let pathref = crate::pane::pathref::extract_path_ref(&lines, &pane_cwd).or_else(|| {
            (pane_cwd != spyc_cwd)
                .then(|| crate::pane::pathref::extract_path_ref(&lines, &spyc_cwd))
                .flatten()
        });

        let Some(pathref) = pathref else {
            self.state
                .flash_error("no path reference found in pane output");
            return;
        };

        spyc_debug!(
            "gf: found path={}, line={:?}",
            pathref.path.display(),
            pathref.line
        );

        let path = pathref.path;
        let line = pathref.line;

        // Exit scroll mode and switch focus to the file list so the user
        // sees the navigation result.
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && tabs.active().is_scrolling()
        {
            tabs.active_mut().exit_scroll_mode();
        }
        self.state.focus = state::Focus::FileList;
        self.view.needs_full_repaint = true;

        // Navigate: if it's a directory, chdir there; if a file, chdir to
        // its parent and focus on it.
        if path.is_dir() {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("gf: {e}"));
            }
            return;
        }

        if let Some(parent) = path.parent() {
            if parent != self.state.listing.dir
                && let Err(e) = self.state.chdir(parent)
            {
                self.state.flash_error(format!("gf: {e}"));
                return;
            }
            self.state.focus_on_path(&path);
        }

        // gF: also open the file in the pager at the referenced line.
        if open_at_line {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );

            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let lines_vec: Vec<String> = text.lines().map(String::from).collect();
                    let mut view = pager::PagerView::new_plain(&name, lines_vec);
                    view.source_path = Some(path);
                    // Jump to the referenced line (0-indexed scroll).
                    if let Some(ln) = line {
                        view.scroll = u16::try_from(ln.saturating_sub(1)).unwrap_or(u16::MAX);
                    }
                    self.set_pager(view);
                }
                Err(e) => {
                    self.state
                        .flash_error(format!("gF: cannot read {name}: {e}"));
                }
            }
        } else if let Some(ln) = line {
            self.state.flash_info(format!(
                "{}:{}",
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                ),
                ln
            ));
        }
    }

    pub(super) fn activate(&mut self, intent: ActivateIntent) -> Vec<Effect> {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return Vec::new();
        };
        let path = row.path.clone();
        let kind = row.kind;

        // Inventory view: enter drills down to the containing directory and
        // focuses on the item, then continues with the intent on that item.
        if self.state.view == View::Inventory {
            let target_dir = if kind == EntryKind::Dir {
                path.clone()
            } else {
                path.parent()
                    .map_or_else(|| path.clone(), Path::to_path_buf)
            };
            if let Err(e) = self.state.chdir(&target_dir) {
                self.state.flash_error(format!("chdir: {e}"));
                return Vec::new();
            }
            self.state.view = View::Dir;
            self.state.focus_on_path(&path);
            self.state.rebuild_rows();
            if kind == EntryKind::Dir {
                return Vec::new();
            }
        }

        // Symlinks are classified by lstat (`DirEntry::metadata`),
        // so a symlink-to-dir comes through as `Symlink`, not `Dir`.
        // Resolve through to the target for navigation so Enter does
        // the obvious thing on `node_modules/foo -> .pnpm/...`. We
        // *don't* generalize this to every op — `R`, picks, etc.
        // intentionally operate on the link itself.
        let descend = kind == EntryKind::Dir
            || (kind == EntryKind::Symlink && crate::fs::target_is_dir(&path));

        if descend {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("chdir: {e}"));
            }
            return Vec::new();
        }

        // File: dispatch based on intent.
        match intent {
            ActivateIntent::Display => {
                if let Some(view) = self.build_pager_view_for_file(&path, None) {
                    self.set_pager(view);
                }
                Vec::new()
            }
            ActivateIntent::Edit => {
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    return Vec::new();
                }
                let program = argv.remove(0);
                argv.push(path.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
                .into()
            }
        }
    }
}

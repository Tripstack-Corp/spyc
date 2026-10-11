//! `AppState` selection: picks, marks, inventory take/drop, and the
//! inventory/graveyard view toggles. Split from `state` verbatim.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::state::{Cursor, Mark};

use crate::app::View;

use super::AppState;

impl AppState {
    pub fn selection_paths(&self) -> Vec<&Path> {
        if self.cur().view == View::Dir && !self.cur().picks.is_empty() {
            self.cur().picks.iter().map(PathBuf::as_path).collect()
        } else if let Some(row) = self.cur().rows.get(self.cur().cursor.index) {
            vec![row.path.as_path()]
        } else {
            Vec::new()
        }
    }

    /// [`Self::selection_paths`], each marked with whether the column lists it
    /// as a directory. Kinds come from `lstat`, so a symlink to a directory is
    /// not one. A filter can hide a pick from `rows` but not from the listing,
    /// so both are consulted.
    pub fn selection_pane_paths(&self) -> Vec<crate::shell::PanePath> {
        use crate::fs::EntryKind;
        let col = self.cur();
        let dirs: HashSet<&Path> = col
            .rows
            .iter()
            .map(|r| (r.path.as_path(), r.kind))
            .chain(
                col.listing
                    .entries
                    .iter()
                    .map(|e| (e.path.as_path(), e.kind)),
            )
            .filter(|&(_, kind)| kind == EntryKind::Dir)
            .map(|(p, _)| p)
            .collect();
        self.selection_paths()
            .into_iter()
            .map(|p| crate::shell::PanePath {
                path: p.to_path_buf(),
                is_dir: dirs.contains(p),
            })
            .collect()
    }

    /// Expand `%` in a shell command against the selection, each path relative
    /// to the column's directory, which is where the command runs. Inside an
    /// archive mount the command runs outside it, so the paths stay absolute:
    /// relative, they would name files beside the archive.
    pub fn expand_selection_percent(
        &self,
        template: &str,
    ) -> Result<String, crate::shell::expand::NonUtf8Path> {
        let dir = self.cur().listing.dir.as_path();
        let cwd = self.mounts.resolve(dir).is_none().then_some(dir);
        crate::shell::expand_percent(template, &self.selection_paths(), cwd)
    }

    pub fn set_mark(&mut self, letter: char) {
        let focus = self
            .cur()
            .rows
            .get(self.cur().cursor.index)
            .map(|r| r.path.clone());
        self.marks.set(
            letter,
            Mark {
                dir: self.cur().listing.dir.clone(),
                focus,
            },
        );
        match self.marks.save() {
            Ok(()) => self.flash_info(format!("mark '{letter}' set")),
            Err(e) => self.flash_error(format!("mark saved in-memory only: {e:#}")),
        }
    }

    pub fn toggle_pick_cursor(&mut self) {
        if self.cur().view != View::Dir {
            return;
        }
        let idx = self.cur().cursor.index;
        // Lift the path out before the `cur_mut()` toggle so the `rows` read
        // borrow ends first.
        if let Some(path) = self.cur().rows.get(idx).map(|r| r.path.clone()) {
            self.cur_mut().picks.toggle(&path);
            self.cur_mut().list_generation = self.cur().list_generation.wrapping_add(1);
        }
    }

    pub fn toggle_all_picks(&mut self) {
        if self.cur().view != View::Dir {
            return;
        }
        let any_unpicked = self
            .cur()
            .rows
            .iter()
            .any(|r| !self.cur().picks.contains(&r.path));
        if any_unpicked {
            let paths: Vec<std::path::PathBuf> =
                self.cur().rows.iter().map(|r| r.path.clone()).collect();
            for path in &paths {
                self.cur_mut().picks.insert(path);
            }
        } else {
            self.cur_mut().picks.clear();
        }
        self.cur_mut().list_generation = self.cur().list_generation.wrapping_add(1);
    }

    /// Yank files into the inventory cache. Takes picks if any, else
    /// cursor item. Only regular files are accepted.
    pub fn take(&self) -> Vec<crate::app::Effect> {
        if self.cur().view != View::Dir {
            return Vec::new();
        }
        let to_take: Vec<PathBuf> = if !self.cur().picks.is_empty() {
            self.cur().picks.iter().cloned().collect()
        } else if let Some(row) = self.cur().rows.get(self.cur().cursor.index) {
            vec![row.path.clone()]
        } else {
            vec![]
        };
        if to_take.is_empty() {
            return Vec::new();
        }
        vec![crate::app::Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Yank {
                sources: to_take
                    .into_iter()
                    .map(crate::app::inventory_ops::YankSource::plain)
                    .collect(),
            },
        )]
    }

    /// Remove the cursor item from inventory (move to graveyard).
    pub fn drop_cursor(&self) -> Vec<crate::app::Effect> {
        if let Some(id) = self
            .inventory
            .items()
            .nth(self.cur().cursor.index)
            .map(|i| i.id.clone())
        {
            vec![crate::app::Effect::Inventory(
                crate::app::inventory_ops::InventoryOp::Remove {
                    id,
                    show_flash: false,
                },
            )]
        } else {
            Vec::new()
        }
    }

    pub fn toggle_inventory_view(&mut self) {
        self.cur_mut().view = match self.cur().view {
            View::Dir | View::Graveyard => View::Inventory,
            View::Inventory => View::Dir,
        };
        // Leaving graveyard view drops the snapshot so a stale set
        // of entries can't bleed into a later open.
        self.graveyard.clear();
        self.cur_mut().cursor = Cursor::new();
        self.rebuild_rows();
    }

    /// Open the graveyard view: load a fresh snapshot from disk
    /// and switch the visible list. Toggle on second call.
    pub fn open_graveyard_view(&mut self) {
        if matches!(self.cur().view, View::Graveyard) {
            self.graveyard.clear();
            self.cur_mut().view = View::Dir;
        } else {
            self.graveyard = crate::state::graveyard::Graveyard::load().entries;
            self.cur_mut().view = View::Graveyard;
        }
        self.cur_mut().cursor = Cursor::new();
        self.rebuild_rows();
    }
}

//! `AppState` selection: picks, marks, inventory take/drop, and the
//! inventory/graveyard view toggles. Split from `state` verbatim.

use std::path::{Path, PathBuf};

use crate::state::{Cursor, Mark};

use crate::app::View;

use super::AppState;

impl AppState {
    pub fn selection_paths(&self) -> Vec<&Path> {
        if self.view == View::Dir && !self.picks.is_empty() {
            self.picks.iter().map(PathBuf::as_path).collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.as_path()]
        } else {
            Vec::new()
        }
    }

    pub fn set_mark(&mut self, letter: char) {
        let focus = self.rows.get(self.cursor.index).map(|r| r.path.clone());
        self.marks.set(
            letter,
            Mark {
                dir: self.listing.dir.clone(),
                focus,
            },
        );
        match self.marks.save() {
            Ok(()) => self.flash_info(format!("mark '{letter}' set")),
            Err(e) => self.flash_error(format!("mark saved in-memory only: {e}")),
        }
    }

    pub fn toggle_pick_cursor(&mut self) {
        if self.view != View::Dir {
            return;
        }
        if let Some(row) = self.rows.get(self.cursor.index) {
            self.picks.toggle(&row.path);
            self.list_generation = self.list_generation.wrapping_add(1);
        }
    }

    pub fn toggle_all_picks(&mut self) {
        if self.view != View::Dir {
            return;
        }
        let any_unpicked = self.rows.iter().any(|r| !self.picks.contains(&r.path));
        if any_unpicked {
            for r in &self.rows {
                self.picks.insert(&r.path);
            }
        } else {
            self.picks.clear();
        }
        self.list_generation = self.list_generation.wrapping_add(1);
    }

    /// Yank files into the inventory cache. Takes picks if any, else
    /// cursor item. Only regular files are accepted.
    pub fn take(&mut self) -> Option<String> {
        if self.view != View::Dir {
            return None;
        }
        let to_take: Vec<PathBuf> = if !self.picks.is_empty() {
            self.picks.iter().cloned().collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.clone()]
        } else {
            vec![]
        };
        let total = to_take.len();
        let (count, err) = self.inventory.yank_many(&to_take);
        self.rebuild_rows();
        let skipped = total - count;
        if count > 0 {
            let msg = if skipped > 0 {
                format!("yanked {count} file(s), skipped {skipped} (dirs/special)")
            } else {
                format!("yanked {count} file(s) to inventory")
            };
            return Some(msg);
        }
        err
    }

    /// Remove the cursor item from inventory (move to graveyard).
    pub fn drop_cursor(&mut self) {
        self.inventory.remove_at(self.cursor.index);
        self.rebuild_rows();
        self.cursor.clamp(self.rows.len());
    }

    pub fn toggle_inventory_view(&mut self) {
        self.view = match self.view {
            View::Dir | View::Graveyard => View::Inventory,
            View::Inventory => View::Dir,
        };
        // Leaving graveyard view drops the snapshot so a stale set
        // of entries can't bleed into a later open.
        self.graveyard.clear();
        self.cursor = Cursor::new();
        self.rebuild_rows();
    }

    /// Open the graveyard view: load a fresh snapshot from disk
    /// and switch the visible list. Toggle on second call.
    pub fn open_graveyard_view(&mut self) {
        if matches!(self.view, View::Graveyard) {
            self.graveyard.clear();
            self.view = View::Dir;
        } else {
            self.graveyard = crate::state::graveyard::Graveyard::load().entries;
            self.view = View::Graveyard;
        }
        self.cursor = Cursor::new();
        self.rebuild_rows();
    }
}

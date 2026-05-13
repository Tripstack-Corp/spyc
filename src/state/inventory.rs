//! File-backed inventory cache.
//!
//! Yank (`y`) copies file content into `~/.local/state/spyc/inventory/`.
//! Each item is stored as a UUID pair: `<uuid>.json` (metadata) and
//! `<uuid>.dat` (file content). Put (`p`) copies from the cache to the
//! destination. Removed items go to a graveyard for undo safety.
//!
//! Only regular files are accepted — no directories or special files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Metadata for a single cached inventory item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedItem {
    /// Unique identifier.
    pub id: String,
    /// Original absolute path when yanked.
    pub orig_path: PathBuf,
    /// Original filename (for display and put).
    pub filename: String,
    /// Epoch seconds when yanked.
    pub timestamp: u64,
    /// File size in bytes.
    pub size: u64,
}

/// File-backed inventory. Items live in the cache directory as
/// `<id>.json` + `<id>.dat` pairs.
#[derive(Debug, Clone)]
pub struct Inventory {
    /// Items keyed by ID, ordered by original path for stable display.
    items: BTreeMap<String, CachedItem>,
    /// Picks within inventory view (set of IDs).
    pub picks: std::collections::HashSet<String>,
}

fn inventory_dir() -> Option<PathBuf> {
    state_base().map(|b| b.join("inventory"))
}

fn state_base() -> Option<PathBuf> {
    crate::state::state_root()
}

fn now_secs() -> u64 {
    crate::sysinfo::epoch_secs()
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            items: BTreeMap::new(),
            picks: std::collections::HashSet::new(),
        }
    }

    /// Load inventory from the cache directory.
    pub fn load() -> Self {
        let Some(dir) = inventory_dir() else {
            return Self::new();
        };
        Self {
            items: load_items(&dir),
            picks: std::collections::HashSet::new(),
        }
    }

    /// Yank a file into the inventory cache. Returns an error message
    /// if the file can't be yanked (not a regular file, too large, etc.).
    pub fn yank(&mut self, path: &Path) -> Result<(), String> {
        let Some(dir) = inventory_dir() else {
            return Err("no state directory".into());
        };
        // Only regular files.
        let meta =
            std::fs::metadata(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{}: not a regular file", path.display()));
        }
        // Skip if already in inventory (by original path).
        if self.contains(path) {
            return Ok(());
        }
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // UUIDv7 — time-ordered, no collision risk on rapid yanks.
        let id = uuid::Uuid::now_v7().simple().to_string();
        let item = CachedItem {
            id: id.clone(),
            orig_path: path.to_path_buf(),
            filename,
            timestamp: now_secs(),
            size: meta.len(),
        };
        // Write content + metadata.
        let _ = std::fs::create_dir_all(&dir);
        let dat_path = dir.join(format!("{id}.dat"));
        let json_path = dir.join(format!("{id}.json"));
        std::fs::copy(path, &dat_path).map_err(|e| format!("copy failed: {e}"))?;
        let json = serde_json::to_string_pretty(&item).map_err(|e| format!("json: {e}"))?;
        std::fs::write(&json_path, json).map_err(|e| format!("write meta: {e}"))?;
        self.items.insert(id, item);
        Ok(())
    }

    /// Yank multiple files. Returns count of successfully yanked files
    /// and the first error (if any).
    pub fn yank_many(&mut self, paths: &[PathBuf]) -> (usize, Option<String>) {
        let mut count = 0;
        let mut first_err = None;
        for p in paths {
            match self.yank(p) {
                Ok(()) => count += 1,
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        (count, first_err)
    }

    /// Put a cached item to a destination directory. Copies the cached
    /// content with the original filename. Returns the destination path.
    pub fn put_item(&self, id: &str, dest_dir: &Path) -> Result<PathBuf, String> {
        let item = self
            .items
            .get(id)
            .ok_or_else(|| "item not found".to_string())?;
        let Some(dir) = inventory_dir() else {
            return Err("no state directory".into());
        };
        let src = dir.join(format!("{id}.dat"));
        let dst = dest_dir.join(&item.filename);
        if dst.exists() {
            return Err(format!("{} already exists", dst.display()));
        }
        std::fs::copy(&src, &dst).map_err(|e| format!("put failed: {e}"))?;
        Ok(dst)
    }

    /// Put items to dest_dir: picked items if any, else all.
    /// Returns (put_count, removed_ids, first_error).
    pub fn put_to(&mut self, dest_dir: &Path) -> (usize, Vec<String>, Option<String>) {
        let ids: Vec<String> = if self.picks.is_empty() {
            self.items.keys().cloned().collect()
        } else {
            self.picks.iter().cloned().collect()
        };
        let mut count = 0;
        let mut removed = Vec::new();
        let mut first_err = None;
        for id in &ids {
            match self.put_item(id, dest_dir) {
                Ok(_) => {
                    count += 1;
                    removed.push(id.clone());
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        // Remove put items from inventory (move to graveyard).
        for id in &removed {
            self.move_to_graveyard(id);
        }
        self.picks.clear();
        (count, removed, first_err)
    }

    /// Read the cached content of an item (for piping to pane).
    pub fn read_content(&self, id: &str) -> Option<Vec<u8>> {
        let dir = inventory_dir()?;
        let path = dir.join(format!("{id}.dat"));
        std::fs::read(&path).ok()
    }

    /// Remove an item by ID, moving it to the graveyard.
    pub fn remove_by_id(&mut self, id: &str) {
        self.move_to_graveyard(id);
        self.picks.remove(id);
    }

    /// Remove the item at cursor index.
    pub fn remove_at(&mut self, index: usize) -> Option<CachedItem> {
        let id = self.items.keys().nth(index)?.clone();
        let item = self.items.get(&id).cloned();
        self.remove_by_id(&id);
        item
    }

    /// Move an item from inventory to graveyard. Reads the
    /// inventory `.dat` blob, archives it through the graveyard's
    /// uniform tar.zst pipeline so the schema stays consistent
    /// with `R`-driven entries (single restore code path), and
    /// then removes the inventory pair on success.
    ///
    /// Failures here are logged via the debug log (no return value
    /// — the inventory mutation has already happened, and the
    /// callers don't have a place to surface a flash). The .dat
    /// file is left in place if archiving fails so manual recovery
    /// is still possible.
    fn move_to_graveyard(&mut self, id: &str) {
        let Some(item) = self.items.remove(id) else {
            return;
        };
        let Some(inv_dir) = inventory_dir() else {
            return;
        };
        let dat = inv_dir.join(format!("{id}.dat"));
        let json = inv_dir.join(format!("{id}.json"));
        let res = crate::state::graveyard::Graveyard::write_entry_as(
            &dat,
            &item.filename,
            item.orig_path.clone(),
        );
        match res {
            Ok(_entry) => {
                // Archive succeeded; drop the inventory pair.
                let _ = std::fs::remove_file(&dat);
                let _ = std::fs::remove_file(&json);
            }
            Err(e) => {
                crate::spyc_debug!(
                    "inventory→graveyard archive failed for {id} ({}): {e}",
                    item.filename
                );
            }
        }
    }

    /// Clear all inventory items (move everything to graveyard).
    pub fn clear(&mut self) {
        let ids: Vec<String> = self.items.keys().cloned().collect();
        for id in ids {
            self.move_to_graveyard(&id);
        }
        self.picks.clear();
    }

    /// Toggle pick on item at index.
    pub fn toggle_pick(&mut self, index: usize) {
        if let Some(id) = self.items.keys().nth(index).cloned() {
            if !self.picks.remove(&id) {
                self.picks.insert(id);
            }
        }
    }

    /// Check if item at index is picked.
    #[allow(dead_code)]
    pub fn is_picked(&self, index: usize) -> bool {
        self.items
            .keys()
            .nth(index)
            .is_some_and(|id| self.picks.contains(id))
    }

    // ── Query API (compatible with old Inventory) ────────────────

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Check if a path is in the inventory (by original path).
    pub fn contains(&self, path: &Path) -> bool {
        self.items.values().any(|item| item.orig_path == path)
    }

    /// Iterate original paths (for display, MCP context, etc.).
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.items.values().map(|item| &item.orig_path)
    }

    /// Iterate items in order.
    pub fn items(&self) -> impl Iterator<Item = &CachedItem> {
        self.items.values()
    }

    /// Get item by index.
    #[allow(dead_code)]
    pub fn get_at(&self, index: usize) -> Option<&CachedItem> {
        self.items.values().nth(index)
    }

    /// Get IDs of picked items (or all if no picks).
    pub fn selected_ids(&self) -> Vec<String> {
        if self.picks.is_empty() {
            self.items.keys().cloned().collect()
        } else {
            self.picks.iter().cloned().collect()
        }
    }
}

/// Load all items from a cache directory.
fn load_items(dir: &Path) -> BTreeMap<String, CachedItem> {
    let mut items = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return items;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(item) = serde_json::from_str::<CachedItem>(&text) else {
            continue;
        };
        // Verify the .dat file still exists.
        let dat = path.with_extension("dat");
        if dat.exists() {
            items.insert(item.id.clone(), item);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    // All sub-cases share one tempdir/Inventory to keep state contiguous.
    // Per-thread `with_state_root` isolates this test from siblings without
    // touching process-global env vars.
    #[test]
    fn cached_inventory_operations() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut inv = Inventory::new();

            // --- yank and contains ---
            let file = make_test_file(tmp.path(), "test.txt", "hello");
            inv.yank(&file).unwrap();
            assert!(inv.contains(&file));
            assert_eq!(inv.len(), 1);
            inv.clear();

            // --- yank rejects directory ---
            let subdir = tmp.path().join("subdir");
            std::fs::create_dir(&subdir).unwrap();
            let err = inv.yank(&subdir).unwrap_err();
            assert!(err.contains("not a regular file"));

            // --- yank deduplicates ---
            let dedup = make_test_file(tmp.path(), "dedup.txt", "dup");
            inv.yank(&dedup).unwrap();
            inv.yank(&dedup).unwrap(); // no error, just skips
            assert_eq!(inv.len(), 1);
            inv.clear();

            // --- put copies to dest ---
            let file = make_test_file(tmp.path(), "src.txt", "content");
            inv.yank(&file).unwrap();
            let dest = tmp.path().join("dest");
            std::fs::create_dir(&dest).unwrap();
            let (count, removed, err) = inv.put_to(&dest);
            assert_eq!(count, 1);
            assert!(err.is_none());
            assert!(!removed.is_empty());
            assert!(dest.join("src.txt").exists());
            assert_eq!(
                std::fs::read_to_string(dest.join("src.txt")).unwrap(),
                "content"
            );
            assert!(inv.is_empty()); // removed after put

            // --- put refuses overwrite ---
            let clash = make_test_file(tmp.path(), "clash.txt", "original");
            inv.yank(&clash).unwrap();
            let dest2 = tmp.path().join("dest2");
            std::fs::create_dir(&dest2).unwrap();
            std::fs::write(dest2.join("clash.txt"), "existing").unwrap();
            let (count, _, err) = inv.put_to(&dest2);
            assert_eq!(count, 0);
            assert!(err.unwrap().contains("already exists"));
            assert_eq!(inv.len(), 1); // stays
            inv.clear(); // clean up

            // --- remove moves to graveyard ---
            let rm_file = make_test_file(tmp.path(), "rm.txt", "bye");
            inv.yank(&rm_file).unwrap();
            assert_eq!(inv.len(), 1);
            inv.remove_at(0);
            assert!(inv.is_empty());
            let gy = tmp.path().join("graveyard");
            let gy_files: Vec<_> = std::fs::read_dir(&gy)
                .unwrap()
                .filter_map(Result::ok)
                .collect();
            assert!(gy_files.len() >= 2); // .json + .dat (may have more from clear above)

            // --- clear moves all to graveyard ---
            let f1 = make_test_file(tmp.path(), "c1.txt", "c1");
            let f2 = make_test_file(tmp.path(), "c2.txt", "c2");
            inv.yank(&f1).unwrap();
            inv.yank(&f2).unwrap();
            inv.clear();
            assert!(inv.is_empty());

            // --- toggle pick ---
            let p1 = make_test_file(tmp.path(), "p1.txt", "p1");
            let p2 = make_test_file(tmp.path(), "p2.txt", "p2");
            inv.yank(&p1).unwrap();
            inv.yank(&p2).unwrap();
            inv.toggle_pick(0);
            assert!(inv.is_picked(0));
            assert!(!inv.is_picked(1));
            inv.toggle_pick(0);
            assert!(!inv.is_picked(0));

            // --- partial put ---
            inv.toggle_pick(0); // pick only first
            let out = tmp.path().join("out");
            std::fs::create_dir(&out).unwrap();
            let (count, _, _) = inv.put_to(&out);
            assert_eq!(count, 1);
            assert_eq!(inv.len(), 1); // only unpicked remains
            inv.clear();

            // --- load round trip ---
            let persist = make_test_file(tmp.path(), "persist.txt", "data");
            inv.yank(&persist).unwrap();
            assert_eq!(inv.len(), 1);
            let dir = tmp.path().join("inventory");
            let loaded = super::load_items(&dir);
            assert_eq!(loaded.len(), 1);
            assert!(loaded.values().any(|i| i.orig_path == persist));
            inv.clear();

            // --- paths iterates orig paths ---
            let x = make_test_file(tmp.path(), "x.txt", "x");
            inv.yank(&x).unwrap();
            let paths: Vec<_> = inv.paths().collect();
            assert_eq!(paths, vec![&x]);
        });
    }
}

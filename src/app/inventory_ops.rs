use std::path::PathBuf;

use super::App;
use crate::state::inventory::Inventory;

/// Tier 5: Inventory operations that involve filesystem IO.
/// Note: These operations currently run on detached worker threads without
/// serialization. This means rapid concurrent operations (like double-yanking)
/// can race during the read-modify-write cache update. This is an accepted
/// race condition for now, as inventory ops are usually human-driven and sparse.
#[derive(Debug)]
pub enum InventoryOp {
    Yank { paths: Vec<PathBuf> },
    Remove { id: String, show_flash: bool },
    Clear,
    Put { dest_dir: PathBuf, ids: Vec<String> },
}

#[derive(Debug)]
pub enum InventoryOutcome {
    Yanked {
        count: usize,
        skipped: usize,
        first_err: Option<String>,
    },
    Removed {
        found: bool,
        show_flash: bool,
    },
    Cleared,
    Put {
        count: usize,
        err: Option<String>,
        dest_dir: PathBuf,
    },
}

pub fn run_inventory_op(op: InventoryOp) -> InventoryOutcome {
    let mut inv = Inventory::load();
    match op {
        InventoryOp::Yank { paths } => {
            let total = paths.len();
            let (count, first_err) = inv.yank_many(&paths);
            InventoryOutcome::Yanked {
                count,
                skipped: total - count,
                first_err,
            }
        }
        InventoryOp::Remove { id, show_flash } => {
            let found = inv.items().any(|i| i.id == id);
            if found {
                inv.remove_by_id(&id);
            }
            InventoryOutcome::Removed { found, show_flash }
        }
        InventoryOp::Clear => {
            inv.clear();
            InventoryOutcome::Cleared
        }
        InventoryOp::Put { dest_dir, ids } => {
            let mut count = 0;
            let mut removed = Vec::new();
            let mut first_err = None;
            for id in &ids {
                match inv.put_item(id, &dest_dir) {
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
            for id in &removed {
                inv.remove_by_id(id);
            }
            InventoryOutcome::Put {
                count,
                err: first_err,
                dest_dir,
            }
        }
    }
}

impl App {
    pub(crate) fn apply_inventory_outcomes(&mut self) -> bool {
        let landed: Vec<InventoryOutcome> =
            std::mem::take(&mut *self.runtime.inventory_results.lock().unwrap());
        if landed.is_empty() {
            return false;
        }
        for outcome in landed {
            self.apply_one_inventory_outcome(outcome);
        }
        true
    }

    fn apply_one_inventory_outcome(&mut self, outcome: InventoryOutcome) {
        let is_put = matches!(outcome, InventoryOutcome::Put { .. });
        match outcome {
            InventoryOutcome::Yanked {
                count,
                skipped,
                first_err,
            } => {
                if count > 0 {
                    if skipped > 0 {
                        self.state.flash_info(format!(
                            "yanked {count} file(s), skipped {skipped} (dirs/special)"
                        ));
                    } else {
                        self.state
                            .flash_info(format!("yanked {count} file(s) to inventory"));
                    }
                } else if let Some(e) = first_err {
                    self.state.flash_error(e);
                }
            }
            InventoryOutcome::Removed { found, show_flash } => {
                if show_flash {
                    if found {
                        self.state.flash_info("removed from inventory");
                    } else {
                        self.state.flash_error("not in inventory");
                    }
                }
            }
            InventoryOutcome::Cleared => {}
            InventoryOutcome::Put {
                count,
                err,
                dest_dir,
            } => {
                if count > 0 {
                    self.state
                        .flash_info(format!("put {count} file(s) to {}", dest_dir.display()));
                }
                if let Some(e) = err {
                    self.state.flash_error(e);
                }
            }
        }
        self.reload_inventory_rows(is_put);
    }

    fn reload_inventory_rows(&mut self, is_put: bool) {
        let picks = std::mem::take(&mut self.state.inventory.picks);
        self.state.inventory = Inventory::load();
        self.state.inventory.picks = picks;
        self.state.rebuild_rows();
        let row_count = self.state.cur().rows.len();
        self.state.cur_mut().cursor.clamp(row_count);
        if is_put {
            self.state.refresh_listing(); // Only Put updates the directory listing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, InventoryOp, InventoryOutcome, run_inventory_op};
    use crate::state::inventory::Inventory;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn write_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn yank_op_caches_files_and_counts_skips() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let a = write_file(work.path(), "a.txt", "alpha");
            let subdir = work.path().join("sub");
            std::fs::create_dir(&subdir).unwrap();
            let out = run_inventory_op(InventoryOp::Yank {
                paths: vec![a, subdir],
            });
            let InventoryOutcome::Yanked {
                count,
                skipped,
                first_err,
            } = out
            else {
                panic!("expected Yanked");
            };
            assert_eq!(count, 1, "the regular file is yanked");
            assert_eq!(skipped, 1, "the directory is skipped");
            assert!(first_err.is_some(), "the directory rejection is reported");
            assert_eq!(Inventory::load().len(), 1);
        });
    }

    #[test]
    fn remove_op_moves_item_out_of_inventory() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let a = write_file(work.path(), "a.txt", "alpha");
            let mut inv = Inventory::load();
            inv.yank(&a).unwrap();
            let id = inv.items().next().unwrap().id.clone();
            let out = run_inventory_op(InventoryOp::Remove {
                id,
                show_flash: true,
            });
            assert!(matches!(
                out,
                InventoryOutcome::Removed {
                    found: true,
                    show_flash: true
                }
            ));
            assert_eq!(Inventory::load().len(), 0);
        });
    }

    #[test]
    fn remove_op_reports_missing_id() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let out = run_inventory_op(InventoryOp::Remove {
                id: "deadbeef".into(),
                show_flash: false,
            });
            assert!(matches!(
                out,
                InventoryOutcome::Removed { found: false, .. }
            ));
        });
    }

    #[test]
    fn clear_op_empties_inventory() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let mut inv = Inventory::load();
            inv.yank(&write_file(work.path(), "a.txt", "a")).unwrap();
            inv.yank(&write_file(work.path(), "b.txt", "b")).unwrap();
            assert_eq!(Inventory::load().len(), 2);
            let out = run_inventory_op(InventoryOp::Clear);
            assert!(matches!(out, InventoryOutcome::Cleared));
            assert_eq!(Inventory::load().len(), 0);
        });
    }

    #[test]
    fn put_op_copies_to_dest_and_drops_item() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let a = write_file(work.path(), "a.txt", "payload");
            let mut inv = Inventory::load();
            inv.yank(&a).unwrap();
            let id = inv.items().next().unwrap().id.clone();
            let dest = work.path().join("dest");
            std::fs::create_dir(&dest).unwrap();
            let out = run_inventory_op(InventoryOp::Put {
                dest_dir: dest.clone(),
                ids: vec![id],
            });
            let InventoryOutcome::Put { count, err, .. } = out else {
                panic!("expected Put");
            };
            assert_eq!(count, 1);
            assert!(err.is_none(), "{err:?}");
            assert_eq!(
                std::fs::read_to_string(dest.join("a.txt")).unwrap(),
                "payload"
            );
            assert_eq!(
                Inventory::load().len(),
                0,
                "put removes the item from inventory"
            );
        });
    }

    #[test]
    fn apply_inventory_outcome_drains_slot_and_flashes() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let mut app = App::test_app(root.path().to_path_buf());
            assert!(
                !app.apply_inventory_outcomes(),
                "empty slot applies nothing"
            );
            app.runtime
                .inventory_results
                .lock()
                .unwrap()
                .push(InventoryOutcome::Yanked {
                    count: 2,
                    skipped: 0,
                    first_err: None,
                });
            assert!(app.apply_inventory_outcomes());
            assert!(app.flash_text().unwrap().contains("yanked 2"));
        });
    }
}

//! Tier 5: the `R`-delete archive, `:undo` restore, and graveyard `Z`
//! purge-all run their tar+zstd / trash IO OFF the input thread.
//!
//! `handle_remove_confirm_key` / `undo_last_remove` /
//! `handle_graveyard_purge_all_confirm` used to call `Graveyard::write_entry` +
//! `remove_tree` / `restore` / `cascade_entry_to_trash` INLINE in the
//! key-dispatch path — seconds-to-minutes of blocking CPU+disk IO on the single
//! event-loop thread (deleting a `target/` or `node_modules/` froze the whole
//! app with no redraw, no input, no flash). They now emit `Effect::Graveyard`;
//! `run_effects` runs the op on a detached worker (the same one-shot off-thread
//! pattern as the agent-status / live-cwd resolvers) and wakes the loop with
//! `Message::GraveyardDone`. `apply_graveyard_outcomes` drains the landed
//! outcomes in the pre-recv scan and does the flash + listing / graveyard
//! refresh — re-deriving the current view, since it may have changed since the
//! op was kicked.

use std::path::PathBuf;

use super::{App, View};
use crate::state::graveyard::{Entry, Graveyard};

/// A graveyard mutation to run off-thread. The cheap parts (picking the latest
/// entry to restore, cloning the in-memory entry list) happen on the main
/// thread at kick time; only the tar+zstd / trash IO — proportional to the tree
/// size — is deferred here.
#[derive(Debug)]
pub enum GraveyardOp {
    /// `R` delete: archive each path into `<uuid>.tar.zst`, then unlink it. A
    /// per-path archive failure leaves that file in place (the user expects
    /// undo); per-path failures don't stop the batch.
    Archive { paths: Vec<PathBuf> },
    /// `:undo`: un-tar `entry` back to `dest`, then drop the entry on success.
    /// `Entry` is boxed so it isn't the enum's large variant.
    Restore { entry: Box<Entry>, dest: PathBuf },
    /// graveyard `Z`: cascade every entry to the system trash.
    PurgeAll { entries: Vec<Entry> },
}

/// The result of a [`GraveyardOp`], applied by [`App::apply_graveyard_outcomes`].
#[derive(Debug)]
pub enum GraveyardOutcome {
    Archived {
        archived: usize,
        failures: Vec<String>,
    },
    Restored {
        filename: String,
        dest: PathBuf,
        result: Result<(), String>,
    },
    Purged {
        trashed: usize,
        errors: usize,
    },
}

/// Run a graveyard op to completion (BLOCKING tar / trash IO). Called only on
/// the `Effect::Graveyard` worker thread — never the event loop.
pub fn run_graveyard_op(op: GraveyardOp) -> GraveyardOutcome {
    match op {
        GraveyardOp::Archive { paths } => {
            let mut archived = 0usize;
            let mut failures = Vec::new();
            for p in &paths {
                match Graveyard::write_entry(p) {
                    Ok(_entry) => match crate::fs::ops::remove_tree(p) {
                        Ok(()) => archived += 1,
                        Err(e) => failures
                            .push(format!("{}: archived but unlink failed: {e}", p.display())),
                    },
                    Err(e) => failures.push(format!(
                        "{}: graveyard archive failed: {e} — file NOT removed",
                        p.display()
                    )),
                }
            }
            GraveyardOutcome::Archived { archived, failures }
        }
        GraveyardOp::Restore { entry, dest } => {
            let result = Graveyard::restore(&entry, &dest).map_err(|e| e.to_string());
            if result.is_ok() {
                Graveyard::delete_entry(&entry);
            }
            GraveyardOutcome::Restored {
                filename: entry.filename.clone(),
                dest,
                result,
            }
        }
        GraveyardOp::PurgeAll { entries } => {
            let mut trashed = 0usize;
            let mut errors = 0usize;
            for entry in &entries {
                match Graveyard::cascade_entry_to_trash(entry) {
                    Ok(()) => trashed += 1,
                    Err(_) => errors += 1,
                }
            }
            GraveyardOutcome::Purged { trashed, errors }
        }
    }
}

impl App {
    /// Drain + apply every landed graveyard outcome (flash + listing / graveyard
    /// refresh). Called every pre-recv scan, so the slot is ALWAYS emptied
    /// regardless of which wake survived coalescing. Returns whether anything
    /// was applied (the caller marks the frame dirty). View-dependent refreshes
    /// re-derive `self.state.view` HERE — the view may have changed since the op
    /// was kicked.
    pub(crate) fn apply_graveyard_outcomes(&mut self) -> bool {
        let landed: Vec<GraveyardOutcome> =
            std::mem::take(&mut *self.runtime.graveyard_results.lock().unwrap());
        if landed.is_empty() {
            return false;
        }
        for outcome in landed {
            self.apply_one_graveyard_outcome(outcome);
        }
        true
    }

    fn apply_one_graveyard_outcome(&mut self, outcome: GraveyardOutcome) {
        match outcome {
            GraveyardOutcome::Archived { archived, failures } => {
                if failures.is_empty() {
                    self.state
                        .flash_info(format!("removed {archived} item(s) (recoverable: gy)"));
                } else {
                    // First failure in the flash; the rest in the debug log.
                    self.state.flash_error(failures[0].clone());
                    for msg in &failures[1..] {
                        crate::spyc_debug!("R: {msg}");
                    }
                }
                self.state.refresh_listing();
            }
            GraveyardOutcome::Restored {
                filename,
                dest,
                result,
            } => match result {
                Ok(()) => {
                    self.state
                        .flash_info(format!("undo: restored {filename} → {}", dest.display()));
                    if matches!(self.state.view, View::Graveyard) {
                        self.reload_graveyard_rows();
                    }
                    self.state.refresh_listing();
                }
                Err(e) => self
                    .state
                    .flash_error(format!("undo: {e} — try `gy` then `p` to restore to cwd")),
            },
            GraveyardOutcome::Purged { trashed, errors } => {
                if errors > 0 {
                    self.state
                        .flash_error(format!("graveyard: trashed {trashed}, {errors} failed"));
                } else {
                    self.state
                        .flash_info(format!("graveyard: trashed {trashed} item(s)"));
                }
                self.reload_graveyard_rows();
            }
        }
    }

    /// Reload the in-memory graveyard list from disk and re-clamp/rebuild the
    /// rows so the open graveyard view reflects the mutation.
    fn reload_graveyard_rows(&mut self) {
        self.state.graveyard = Graveyard::load().entries;
        self.state.cursor.clamp(self.state.graveyard.len());
        self.state.rebuild_rows();
    }
}

#[cfg(test)]
mod tests {
    use super::{App, GraveyardOp, GraveyardOutcome, run_graveyard_op};
    use crate::state::graveyard::Graveyard;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn archive_then_restore_roundtrips_in_the_worker_body() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let work = tempdir().unwrap();
            let src = work.path().join("doomed.txt");
            writeln!(std::fs::File::create(&src).unwrap(), "delete me").unwrap();

            // Archive: the file is tarred into the graveyard then unlinked.
            let out = run_graveyard_op(GraveyardOp::Archive {
                paths: vec![src.clone()],
            });
            let GraveyardOutcome::Archived { archived, failures } = out else {
                panic!("expected Archived");
            };
            assert_eq!(archived, 1);
            assert!(failures.is_empty(), "{failures:?}");
            assert!(!src.exists(), "source should be unlinked after archive");

            // Restore the just-archived entry into a fresh dir.
            let entry = Graveyard::load()
                .entries
                .into_iter()
                .next()
                .expect("one entry");
            let dest = tempdir().unwrap();
            let out = run_graveyard_op(GraveyardOp::Restore {
                entry: Box::new(entry),
                dest: dest.path().to_path_buf(),
            });
            let GraveyardOutcome::Restored {
                result, filename, ..
            } = out
            else {
                panic!("expected Restored");
            };
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(filename, "doomed.txt");
            assert!(dest.path().join("doomed.txt").exists());
            // The entry is dropped from the graveyard on a successful restore.
            assert!(Graveyard::load().entries.is_empty());
        });
    }

    #[test]
    fn archive_op_reports_per_path_failure_for_missing_source() {
        let root = tempdir().unwrap();
        crate::state::with_state_root(root.path(), || {
            let missing = root.path().join("nope.txt");
            let out = run_graveyard_op(GraveyardOp::Archive {
                paths: vec![missing],
            });
            let GraveyardOutcome::Archived { archived, failures } = out else {
                panic!("expected Archived");
            };
            assert_eq!(archived, 0);
            assert_eq!(failures.len(), 1);
            assert!(failures[0].contains("NOT removed"), "{}", failures[0]);
        });
    }

    #[test]
    fn apply_drains_the_slot_and_flashes_the_count() {
        let mut app = App::test_app(std::env::temp_dir());
        // Empty slot → nothing applied.
        assert!(!app.apply_graveyard_outcomes());
        // A landed outcome is drained, flashed, and the slot emptied.
        app.runtime
            .graveyard_results
            .lock()
            .unwrap()
            .push(GraveyardOutcome::Archived {
                archived: 3,
                failures: Vec::new(),
            });
        assert!(app.apply_graveyard_outcomes());
        assert!(app.flash_text().unwrap().contains("removed 3"));
        assert!(!app.apply_graveyard_outcomes());
    }
}

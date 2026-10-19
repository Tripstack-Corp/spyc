//! A write in flight owns its archive. Each write re-reads the archive once it
//! lands, emptying the staging tree first, and clears the journal, so while
//! one runs a second write, a delete, rename or file put into that archive, a
//! discard, an unmount, and a mount of an archive inside it are refused rather
//! than lost. An edit saved to an extracted copy can't be refused, since an
//! editor or an agent saves it outside spyc's ops, so one saved after the
//! write was planned is held aside from the re-read and put back, pending.

use std::path::{Path, PathBuf};

use super::{current_staged_stats, display_name, sibling_of};
use crate::app::App;
use crate::app::archive_ops::ArchiveOutcome;
use crate::app::archive_route::PendingChange;
use crate::archive::index::staging_rel_for;
use crate::archive::journal::StagedStats;

/// Edits saved to an archive's extracted copies while it was written, held
/// aside from its re-read (`Runtime::archive_held`): the tree they are in,
/// and each one's inner path and path under it.
#[derive(Debug)]
pub(in crate::app) struct Held {
    root: PathBuf,
    copies: Vec<(String, PathBuf)>,
}

impl App {
    /// Record that a write of `archive`, planned from the staged copies as
    /// `planned` stats them, starts.
    pub(super) fn start_write(&mut self, archive: &Path, planned: StagedStats) {
        self.runtime
            .archive_writes
            .insert(archive.to_path_buf(), planned);
    }

    /// The write `outcome` ends, if it ends one, is over. One that landed
    /// holds aside the edits saved since it was planned.
    pub(super) fn finish_write(&mut self, outcome: &ArchiveOutcome) {
        match outcome {
            ArchiveOutcome::Written { archive, .. } => {
                if let Some(planned) = self.runtime.archive_writes.remove(archive) {
                    self.hold_late_edits(archive, &planned);
                }
            }
            ArchiveOutcome::WriteFailed { archive, .. } => {
                self.runtime.archive_writes.remove(archive);
            }
            _ => {}
        }
    }

    /// Move aside each extracted copy of `archive` edited since its write was
    /// planned from `planned`: the write didn't carry it, and the re-read
    /// empties the staging tree. A copy spyc extracted itself in the meantime
    /// is not an edit.
    fn hold_late_edits(&mut self, archive: &Path, planned: &StagedStats) {
        let Some(mount) = self.state.mounts.get(archive) else {
            return;
        };
        let now = current_staged_stats(mount);
        let late: Vec<(String, PathBuf)> = mount
            .index
            .entries
            .iter()
            .filter(|e| {
                now.get(&e.inner).is_some_and(|stat| {
                    planned.get(&e.inner) != Some(stat) && mount.staged.get(&e.inner) != Some(stat)
                })
            })
            .map(|e| (e.inner.clone(), e.staging_rel()))
            .collect();
        let staging = mount.staging_root.clone();
        let Some(root) = sibling_of(&staging, "held").filter(|_| !late.is_empty()) else {
            return;
        };
        let copies = late
            .into_iter()
            .filter(|(_, rel)| move_file(&staging.join(rel), &root.join(rel)))
            .collect();
        self.runtime
            .archive_held
            .insert(archive.to_path_buf(), Held { root, copies });
    }

    /// Put back the edits held aside for `archive` now that it has been read
    /// again, each pending as a change to it, and say so. One the write did
    /// carry, a streamed archive's untouched member being repacked from its
    /// staged copy, matches what the re-read extracted and is dropped.
    pub(super) fn restore_late_edits(&mut self, archive: &Path) {
        let Some(held) = self.runtime.archive_held.remove(archive) else {
            return;
        };
        let Some(mount) = self.state.mounts.get_mut(archive) else {
            self.runtime
                .archive_held
                .insert(archive.to_path_buf(), held);
            return;
        };
        let (mut pending, mut stranded) = (Vec::new(), Vec::new());
        for (inner, rel) in held.copies {
            let entry = mount.index.entries.iter().find(|e| e.inner == inner);
            let member = entry.is_some();
            let to = entry.map_or_else(
                || mount.staging_root.join(staging_rel_for(&inner, 0)),
                |e| mount.staging_path(e),
            );
            let from = held.root.join(rel);
            if same_bytes(&from, &to) {
                continue;
            }
            if !move_file(&from, &to) {
                stranded.push(from);
                continue;
            }
            if member {
                mount.journal.replace(&inner);
            } else {
                mount.journal.add(&inner);
            }
            pending.push(inner);
        }
        if let Some(copy) = stranded.first() {
            self.state.flash_error(format!(
                "{} changed while it was written, and couldn't be put back: it is at {}",
                display_name(archive),
                copy.display()
            ));
            return;
        }
        let _ = std::fs::remove_dir_all(&held.root);
        if !pending.is_empty() {
            self.state.flash_info(format!(
                "{} changed while {} was written — pending again",
                pending.join(", "),
                display_name(archive)
            ));
        }
    }

    /// Whether `archive` is being written, saying so when it is.
    pub(super) fn refuse_while_written(&mut self, archive: &Path) -> bool {
        if !self.runtime.archive_writes.contains_key(archive) {
            return false;
        }
        self.say_being_written(archive);
        true
    }

    /// Whether an archive `at` lives inside is being written, saying so when
    /// one is: its re-read empties the tree the copy `at` reads from is in.
    pub(super) fn refuse_inside_written(&mut self, at: &Path) -> bool {
        let Some(outer) = self
            .runtime
            .archive_writes
            .keys()
            .find(|w| at != w.as_path() && at.starts_with(w))
            .cloned()
        else {
            return false;
        };
        self.say_being_written(&outer);
        true
    }

    fn say_being_written(&mut self, archive: &Path) {
        self.state.flash_error(format!(
            "archive: {} is being written — try again once it finishes",
            display_name(archive)
        ));
    }

    /// Whether any of `changes` is to an archive being written, saying so when
    /// one is.
    pub(super) fn refuse_changes_while_written(&mut self, changes: &[PendingChange]) -> bool {
        changes
            .iter()
            .any(|change| self.refuse_while_written(change.archive()))
    }
}

/// Move the file `from` to `to`, making `to`'s directory; whether it moved.
fn move_file(from: &Path, to: &Path) -> bool {
    to.parent()
        .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
        && std::fs::rename(from, to).is_ok()
}

/// Whether the files `a` and `b` both exist and hold the same bytes.
fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.len() == y.len() => {
            matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
        }
        _ => false,
    }
}

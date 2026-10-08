//! Staged copies and the edits made to them: watching what spyc hands an
//! editor, mapping a staged file back to its member, and recording a change as
//! pending, so the badge, `:archive info` and the write all agree.

use std::path::{Path, PathBuf};

use super::{AUTO_SCAN_MAX, changed_among};
use crate::app::App;
use crate::app::file_ops::PagerDest;

impl App {
    /// Start watching a staged copy for a change spyc won't make itself.
    pub(in crate::app) fn watch_for_edits(&mut self, real: &Path) {
        let Some((archive, inner)) = self.staged_owner(real) else {
            return;
        };
        if let Some(mount) = self.state.mounts.get_mut(&archive)
            && !mount.editing.contains(&inner)
        {
            mount.editing.push(inner);
        }
    }

    /// The mount and journal path a staged file stands for.
    fn staged_owner(&self, real: &Path) -> Option<(PathBuf, String)> {
        let at = self.mount_path_for_staged(real)?;
        let (mount, _) = self.state.mounts.member_of(&at)?;
        let entry = mount.entry_at(&at)?;
        Some((mount.archive().to_path_buf(), entry.inner.clone()))
    }

    /// The mount path a staged file stands in for — the inverse of
    /// `ArchiveMount::staging_path`, used to rewrite a held-back effect.
    pub(super) fn mount_path_for_staged(&self, real: &Path) -> Option<PathBuf> {
        let mount = self
            .state
            .mounts
            .iter()
            .find(|m| real.starts_with(&m.staging_root))?;
        let rel = real.strip_prefix(&mount.staging_root).ok()?;
        // A case-colliding member stages under the reserved directory
        // (`<reserved>/case-N/<inner>`), so strip both of those components before
        // reading the path back as a member. No member name can start with the
        // reserved component — `index::normalize` refuses it — so a match here is
        // spyc's own escape and never the archive's.
        let rel = rel
            .strip_prefix(crate::archive::index::STAGING_RESERVED)
            .ok()
            .and_then(|after| {
                let rank = after.components().next()?;
                after.strip_prefix(rank).ok()
            })
            .unwrap_or(rel);
        Some(mount.archive().join(rel))
    }

    /// Open an already-extracted member. The pager's own planner takes it from
    /// here — by now it's an ordinary local file.
    pub(super) fn open_staged_in_pager(&mut self, real: &Path, dest: PagerDest) {
        if let Some(op) = self.plan_pager_open(real, None, dest) {
            self.spawn_file_op(op);
        }
    }

    /// Remember what a freshly staged file looked like, so an edit spyc didn't
    /// make is visible later as a size/mtime that no longer matches.
    pub(super) fn record_staged(&mut self, real: &Path) {
        let Ok(md) = std::fs::metadata(real) else {
            return;
        };
        // Keyed by **journal path**, which is what every reader of this map uses.
        // The staging-relative path is not the same string: a case-colliding member
        // stages under the reserved directory, so recording its location gave one
        // member two names and its edits were never noticed.
        let Some((archive, inner)) = self.staged_owner(real) else {
            return;
        };
        let Some(mount) = self.state.mounts.get_mut(&archive) else {
            return;
        };
        mount.staged.insert(
            inner,
            crate::archive::journal::StagedStat {
                size: md.len(),
                mtime: md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                is_dir: md.is_dir(),
            },
        );
    }

    /// Stat every pending addition into its mount's `staged` map.
    ///
    /// An addition's bytes arrive by a *copy* the effect layer runs, so neither
    /// filler of that map ever sees one: `current_staged_stats` walks the index,
    /// which an addition is by definition not in, and `record_staged` runs off a
    /// materialize outcome, which a copy doesn't produce. The listing reads the
    /// map, so every put member listed as `size = 0, mtime = epoch` — an empty
    /// file, in the one listing with no on-disk row to compare it against.
    ///
    /// Re-stat rather than insert-once: a put member can be edited afterwards, and
    /// there is no reason for its row to keep describing the file it arrived as.
    /// Additions are user puts, so this is a handful of stats, not a walk.
    pub(in crate::app) fn record_staged_additions(&mut self) {
        if self.state.mounts.is_empty() {
            return;
        }
        let archives: Vec<PathBuf> = self
            .state
            .mounts
            .iter()
            .map(|m| m.archive().to_path_buf())
            .collect();
        for archive in archives {
            let Some(mount) = self.state.mounts.get_mut(&archive) else {
                continue;
            };
            let additions: Vec<String> =
                mount.journal.additions().map(ToString::to_string).collect();
            for inner in additions {
                // An addition stages at its journal path directly — it has no
                // index entry to carry a case rank.
                let Ok(md) = std::fs::metadata(mount.staging_root.join(&inner)) else {
                    continue;
                };
                mount.staged.insert(
                    inner,
                    crate::archive::journal::StagedStat {
                        size: md.len(),
                        mtime: md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                        is_dir: md.is_dir(),
                    },
                );
            }
        }
    }

    /// Notice a staged member that changed, and record it as a pending change.
    ///
    /// Runs on every loop wake. An edit is otherwise invisible until something
    /// asks the disk, and the *draw* pass can't: a staged copy that supersedes its
    /// archived bytes has to be in the Model for the badge, `:archive info` and
    /// the repack to agree about it.
    ///
    /// Deliberately **not** limited to members spyc handed to an editor. There are
    /// several routes to the same staged file — the pager's `v` edits its own
    /// `source_path`, an agent in the pane can write it, so can `!vim` — and a fix
    /// that only watched the ones spyc knew about left the badge blank for all of
    /// them. What bounds the cost instead is the size of the staged set: it holds
    /// only members that have actually been extracted, which for a zip is whatever
    /// the user has read. Past [`AUTO_SCAN_MAX`] — a streamed tarball, where
    /// mounting extracts everything — statting it on every keypress would be
    /// visible jank, so those fall back to [`Self::scan_archive_edits`] at the
    /// moments something reports or writes, plus the `editing` set, which stays
    /// cheap however large the archive is.
    pub(in crate::app) fn settle_archive_edits(&mut self) -> bool {
        if self.state.mounts.is_empty() {
            return false;
        }
        let found: Vec<(PathBuf, Vec<String>)> = self
            .state
            .mounts
            .iter()
            .map(|m| {
                let mut candidates: Vec<String> = m.editing.clone();
                if m.staged.len() <= AUTO_SCAN_MAX {
                    for inner in m.staged.keys() {
                        if !candidates.contains(inner) {
                            candidates.push(inner.clone());
                        }
                    }
                }
                (m.archive().to_path_buf(), changed_among(m, &candidates))
            })
            .filter(|(_, changed)| !changed.is_empty())
            .collect();
        self.record_replacements(found)
    }

    /// The same over every staged member — for the moments something is about to
    /// *report* on an archive or write it, where an agent's edit to a member the
    /// user never opened has to be caught too. Costs a stat per staged member, so
    /// it runs on those moments rather than on a timer.
    pub(in crate::app) fn scan_archive_edits(&mut self) -> bool {
        if self.state.mounts.is_empty() {
            return false;
        }
        let found: Vec<(PathBuf, Vec<String>)> = self
            .state
            .mounts
            .iter()
            .map(|m| {
                let all: Vec<String> = m.staged.keys().cloned().collect();
                (m.archive().to_path_buf(), changed_among(m, &all))
            })
            .filter(|(_, changed)| !changed.is_empty())
            .collect();
        self.record_replacements(found)
    }

    /// Enter the detected changes into each journal, and stop watching them.
    fn record_replacements(&mut self, found: Vec<(PathBuf, Vec<String>)>) -> bool {
        let any = !found.is_empty();
        for (archive, changed) in found {
            let Some(mount) = self.state.mounts.get_mut(&archive) else {
                continue;
            };
            for inner in changed {
                mount.journal.replace(&inner);
                mount.editing.retain(|w| *w != inner);
            }
        }
        if any {
            // The rows are cached against `list_generation`, and an edit doesn't
            // change which rows exist — only what one of them should now say. The
            // rebuild is what makes the marker appear rather than waiting for the
            // next unrelated listing change.
            self.state.rebuild_rows();
        }
        any
    }
}

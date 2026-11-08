//! Staged copies and the edits made to them: watching what spyc hands an
//! editor, mapping a staged file back to its member, recording a change as
//! pending, and checking a mount for changes, so the badge, the entry menu,
//! `:archive info` and the write all agree.

use std::path::{Path, PathBuf};

use crate::app::App;
use crate::app::file_ops::PagerDest;
use crate::archive::journal::StagedStats;
use crate::archive::mount::ArchiveMount;

impl App {
    /// Whether a mount has anything a write would change — the journal, or a
    /// staged file that no longer matches what spyc wrote.
    pub(in crate::app) fn mount_is_dirty(&self, archive: &Path) -> bool {
        let Some(mount) = self.state.mounts.get(archive) else {
            return false;
        };
        mount.is_dirty() || differs(&mount.staged, &current_staged_stats(mount))
    }

    /// Archives carrying changes nobody has written back. Read at quit time, so
    /// the warning names them rather than counting silently.
    pub(in crate::app) fn dirty_mounts(&self) -> Vec<PathBuf> {
        self.state
            .mounts
            .iter()
            .map(|m| m.archive().to_path_buf())
            .filter(|a| self.mount_is_dirty(a))
            .collect()
    }

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

    /// Why the staged file `real` can't go to an editor: it is a member of a
    /// read-only mount, which could never take the edit back. `None` for any
    /// other file.
    pub(in crate::app) fn staged_edit_refusal(&self, real: &Path) -> Option<String> {
        let at = self.mount_path_for_staged(real)?;
        let (mount, _) = self.state.mounts.resolve(&at)?;
        crate::app::state::archive::read_only_refusal(&mount.capability)
    }

    /// The mount and journal path a staged file stands for.
    fn staged_owner(&self, real: &Path) -> Option<(PathBuf, String)> {
        let at = self.mount_path_for_staged(real)?;
        let (mount, _) = self.state.mounts.member_of(&at)?;
        let entry = mount.entry_at(&at)?;
        Some((mount.archive().to_path_buf(), entry.inner.clone()))
    }

    /// Where an op that wrote to `dest` wrote, as the user named it: the place
    /// in a mount a staging path stands for, else `dest`. A copy or put into a
    /// mount is pointed at the staging tree, and its message names the place.
    pub(in crate::app) fn shown_dest(&self, dest: &Path) -> PathBuf {
        self.mount_path_for_staged(dest)
            .unwrap_or_else(|| dest.to_path_buf())
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
        // The staging root stands for the archive itself, named without the
        // trailing `/` an empty join adds.
        if rel.as_os_str().is_empty() {
            return Some(mount.archive().to_path_buf());
        }
        Some(mount.archive().join(rel))
    }

    /// Open an already-extracted member. The pager's own planner takes it from
    /// here — by now it's an ordinary local file, so `D`'s top pane gets what
    /// `D` on a file gets.
    pub(super) fn open_staged_in_pager(&mut self, real: &Path, dest: PagerDest) {
        let op = match dest {
            PagerDest::TopPane => self.plan_display_in_pane(real),
            PagerDest::Overlay { .. } => self.plan_pager_open(real, None, dest),
        };
        if let Some(op) = op {
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

/// Stat each member spyc extracted, keyed the same way the recorded stats are.
/// The difference between the two is the only signal that an edit spyc didn't
/// perform has happened. Only recorded members are read, so the check costs a
/// stat per extracted member however large the archive.
pub(super) fn current_staged_stats(mount: &ArchiveMount) -> StagedStats {
    stats_of(
        &mount.staging_root,
        mount
            .staged
            .keys()
            .filter_map(|inner| mount.index.get(inner)),
    )
}

/// Stat every member of `index` with a file in the staging tree `staging_root`,
/// whoever put it there: what a new mount records as spyc's own, and what a
/// write's re-read would otherwise wipe. Both run on the archive worker, as a
/// stat per member is a walk of the whole archive. A tree with nothing in it,
/// as a seekable mount's is when it is read, has no member to stat, and one
/// look says so.
pub(in crate::app) fn on_disk_stats(
    index: &crate::archive::index::ArchiveIndex,
    staging_root: &Path,
) -> StagedStats {
    if holds_nothing(staging_root) {
        return StagedStats::new();
    }
    stats_of(staging_root, index.entries.iter())
}

/// Whether `dir` is missing or empty. A read that fails otherwise says
/// neither, so the walk still runs.
fn holds_nothing(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

fn stats_of<'a>(
    staging_root: &Path,
    entries: impl Iterator<Item = &'a crate::archive::IndexEntry>,
) -> StagedStats {
    let mut now = StagedStats::new();
    for entry in entries {
        let path = crate::archive::listing::staging_path(staging_root, entry);
        let Ok(md) = stat_staged(&path) else {
            continue;
        };
        // Directories are skipped: a staging directory's mtime moves whenever
        // *any* member inside it is written, so counting it would report a change
        // to a member nobody touched.
        if md.is_dir() {
            continue;
        }
        now.insert(
            entry.inner.clone(),
            crate::archive::journal::StagedStat {
                size: md.len(),
                mtime: md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                is_dir: md.is_dir(),
            },
        );
    }
    now
}

/// Stat a staged copy. A test counts them on its own thread: a check for
/// changes costs one per member it looks at.
fn stat_staged(path: &Path) -> std::io::Result<std::fs::Metadata> {
    #[cfg(test)]
    STAGED_STATS.with(|n| n.set(n.get() + 1));
    std::fs::metadata(path)
}

#[cfg(test)]
thread_local! {
    static STAGED_STATS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many staged copies this thread has statted since it last asked.
#[cfg(test)]
pub(in crate::app) fn staged_stats_taken() -> usize {
    STAGED_STATS.with(|n| n.replace(0))
}

/// How many staged members spyc will stat on every loop wake to notice an edit.
///
/// A handful of reads costs nothing; a streamed tarball's whole member list would
/// cost tens of milliseconds per keypress.
const AUTO_SCAN_MAX: usize = 256;

/// Which of `candidates` has a staged copy that no longer matches what spyc
/// recorded — skipping any already recorded as replaced.
fn changed_among(mount: &ArchiveMount, candidates: &[String]) -> Vec<String> {
    candidates
        .iter()
        .filter(|inner| !mount.journal.is_replaced(inner))
        .filter(|inner| {
            let Some(entry) = mount.index.get(inner) else {
                return false;
            };
            let Ok(md) = stat_staged(&mount.staging_path(entry)) else {
                return false;
            };
            if md.is_dir() {
                return false;
            }
            let now = crate::archive::journal::StagedStat {
                size: md.len(),
                mtime: md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                is_dir: false,
            };
            mount.staged.get(inner.as_str()) != Some(&now)
        })
        .cloned()
        .collect()
}

/// Whether anything staged differs from what spyc recorded when it wrote it.
pub(super) fn differs(
    recorded: &crate::archive::journal::StagedStats,
    now: &crate::archive::journal::StagedStats,
) -> bool {
    now.iter()
        .any(|(inner, current)| recorded.get(inner) != Some(current))
}

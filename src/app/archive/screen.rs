//! The App half of the archive screen. `run_effects` passes every outgoing
//! effect through [`App::screen_archive_effect`], which mounts what the effect
//! needs and then acts on `archive_route::route_archive_effect`'s verdict: pass
//! it through, extract first, refuse, unmount, or record a pending change.

use std::path::{Path, PathBuf};

use super::{archive_ancestor_of, display_name, staging_target};
use crate::app::archive_ops::{ArchiveOp, MaterializeThen};
use crate::app::{App, Effect};

impl App {
    /// Run an outgoing effect past the archive screen.
    ///
    /// `Some` means execute it; `None` means it was held back for extraction (or
    /// refused), and the drain will bring it round again once the bytes exist.
    /// Two questions, in order: does this effect want an archive mounted before it
    /// can run at all, and does it name something inside one that already is?
    pub(in crate::app) fn screen_archive_effect(&mut self, effect: Effect) -> Option<Effect> {
        self.mount_and_retry(effect)
    }

    /// Hold back a `ChangeDir` that names a place inside an archive nobody has
    /// mounted, and mount it first.
    ///
    /// Every way of landing on a path goes through this effect — a mark, a harpoon
    /// slot, a restored session, `navigate_to`, `J` — so doing it here means none
    /// of them has to know that a path can name a place inside a file. The mount
    /// carries the original effect and re-issues it, which is how the cursor
    /// target and the message survive the round trip.
    pub(super) fn mount_and_retry(&mut self, effect: Effect) -> Option<Effect> {
        let Effect::ChangeDir { path, .. } = &effect else {
            return self.screen_mount_paths(effect);
        };
        // An already-mounted path is served from the index by `chdir_into_mount`,
        // and a real directory is a real directory.
        if !self.state.config.archive.enable || self.state.mounts.contains(path) || path.is_dir() {
            return self.screen_mount_paths(effect);
        }
        let Some((archive, _inner)) = archive_ancestor_of(path) else {
            return self.screen_mount_paths(effect);
        };
        self.request_mount_then(&archive, false, Some(Box::new(effect)))
            .into_iter()
            .next()
    }

    /// The member/container screen proper: [`route_archive_effect`]'s verdict.
    fn screen_mount_paths(&mut self, effect: Effect) -> Option<Effect> {
        use crate::app::archive_route::{ArchiveSink, route_archive_effect};
        let staged_check = |p: &Path| {
            self.state
                .mounts
                .member_of(p)
                .and_then(|(mount, _)| mount.bytes_at(p))
                .is_some_and(|bytes| bytes.exists())
        };
        let inventory_names = |ids: &[String]| -> Vec<String> {
            self.state
                .inventory
                .items()
                .filter(|item| ids.contains(&item.id))
                .map(|item| item.filename.clone())
                .collect()
        };
        // The one fs question the pure router can't answer for itself: a copy-in
        // source that is a directory can't round-trip through the journal.
        let is_dir = |p: &Path| p.is_dir();
        match route_archive_effect(
            effect,
            &self.state.mounts,
            &staged_check,
            &inventory_names,
            &is_dir,
        ) {
            ArchiveSink::PassThrough(effect) => Some(effect),
            ArchiveSink::Refuse(why) => {
                self.state.flash_error(why);
                None
            }
            // Replaced, not spawned: the op goes back into the same effect list
            // the executor is already walking, so there is one path to the worker
            // and the screen stays a rewriter.
            ArchiveSink::Materialize { members, then } => {
                self.extraction_effect(&members, MaterializeThen::Retry(then))
            }
            // The archive file itself is going away. Its mount has to go with it,
            // but not its unwritten changes: those would vanish with nothing to
            // put them back into, so say so instead.
            ArchiveSink::UnmountFirst { archives, effect } => {
                if let Some(dirty) = archives.iter().find(|a| self.mount_is_dirty(a)) {
                    self.state.flash_error(format!(
                        "{}: unwritten changes — :archive write or :archive discard first",
                        display_name(dirty)
                    ));
                    return None;
                }
                for archive in &archives {
                    if let Some(staging) = self.state.mounts.remove(archive) {
                        // Cleaning it is an effect, and the one effect this
                        // returns is the user's — teardown collects it.
                        self.state.mounts.defer_cleanup(staging);
                    }
                }
                Some(*effect)
            }
            ArchiveSink::Record(changes) => {
                self.record_pending(changes);
                None
            }
            // Bringing a file in needs its bytes somewhere before the repack can
            // read them, so the rewritten op runs *and* the change is recorded.
            ArchiveSink::RewriteAndRecord { effect, changes } => {
                if let Some(dir) = staging_target(&effect)
                    && let Err(e) = std::fs::create_dir_all(&dir)
                {
                    self.state
                        .flash_error(format!("archive: staging {}: {e:#}", dir.display()));
                    return None;
                }
                self.record_pending(changes);
                Some(*effect)
            }
        }
    }

    /// The extraction op for a batch of members, to run in place of the effect
    /// that wanted them.
    fn extraction_effect(&mut self, members: &[PathBuf], then: MaterializeThen) -> Option<Effect> {
        let (mount, _) = members.first().and_then(|p| self.state.mounts.resolve(p))?;
        let archive = mount.source().to_path_buf();
        let staging_root = mount.staging_root.clone();
        let entries: Vec<(PathBuf, crate::archive::IndexEntry)> = members
            .iter()
            .filter_map(|p| mount.entry_at(p).map(|e| (p.clone(), e.clone())))
            .filter(|(_, e)| e.readable)
            .collect();
        if entries.is_empty() {
            self.state
                .flash_error("archive: nothing readable in the selection");
            return None;
        }
        self.state
            .flash_progress(format!("extracting {} member(s)…", entries.len()));
        Some(Effect::Archive(ArchiveOp::MaterializeMany {
            archive,
            entries,
            staging_root,
            then,
        }))
    }

    /// Fold recorded changes into the mounts' journals.
    fn record_pending(&mut self, changes: Vec<crate::app::archive_route::PendingChange>) {
        use crate::app::archive_route::PendingChange;
        let mut deleted = 0usize;
        let mut renamed = 0usize;
        let mut added = 0usize;
        for change in changes {
            match change {
                PendingChange::Delete { archive, inner } => {
                    // Deleting a file the user brought in un-adds it: the archive
                    // never held it, so its staged bytes are the only copy and
                    // leaving them behind would collide with a second put.
                    let staged = self
                        .state
                        .mounts
                        .get(&archive)
                        .filter(|mount| mount.journal.is_addition(&inner))
                        .map(|mount| mount.staging_root.join(&inner));
                    if let Some(mount) = self.state.mounts.get_mut(&archive) {
                        match &staged {
                            Some(_) => {
                                mount.journal.forget_addition(&inner);
                            }
                            None => mount.journal.delete(inner),
                        }
                        deleted += 1;
                    }
                    if let Some(staged) = staged {
                        let _ = std::fs::remove_file(&staged);
                    }
                }
                PendingChange::Rename { archive, from, to } => {
                    if let Some(mount) = self.state.mounts.get_mut(&archive) {
                        mount.journal.rename(from, to);
                        renamed += 1;
                    }
                }
                PendingChange::Add { archive, inner } => {
                    if let Some(mount) = self.state.mounts.get_mut(&archive) {
                        mount.journal.add(inner);
                        added += 1;
                    }
                }
            }
        }
        if deleted + renamed + added == 0 {
            return;
        }
        self.state.refresh_listing();
        let what = [(deleted, "removed"), (renamed, "renamed"), (added, "added")]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, verb)| format!("{n} {verb}"))
            .collect::<Vec<_>>()
            .join(", ");
        // Un-adding the last pending addition leaves nothing to write back, so
        // pointing at `:archive write` would name a no-op.
        let touched: Vec<PathBuf> = self
            .state
            .mounts
            .iter()
            .map(|m| m.archive().to_path_buf())
            .collect();
        let still_dirty = touched.iter().any(|a| self.mount_is_dirty(a));
        self.state.flash_info(if still_dirty {
            format!("{what} — :archive write to apply")
        } else {
            what
        });
    }
}

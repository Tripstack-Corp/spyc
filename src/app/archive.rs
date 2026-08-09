//! App-layer glue for archive mounts: kicking a mount, applying what the worker
//! sends back, the staging-tree lifecycle, and the `:archive` command.
//!
//! The Model holds the mounts (`state.mounts`) and the pure `archive` crate does
//! the deciding; this is the impure half that talks to the worker and the disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use crate::archive::mount::ArchiveMount;
use crate::archive::{Journal, journal::StagedStats};

use super::archive_ops::{ArchiveOp, ArchiveOutcome, MaterializeThen, mount_notes};
use super::file_ops::PagerDest;
use super::{App, Effect, Mode, Prompt, PromptKind};

impl App {
    /// Kick a mount for `path`. `confirmed` is set on the second pass, after the
    /// user answered a size prompt.
    pub(super) fn request_mount(&mut self, path: &Path, confirmed: bool) -> Vec<Effect> {
        let Some(staging_root) = staging_root_for(path) else {
            self.state
                .flash_error("archive: no state directory to stage into");
            return Vec::new();
        };
        // A streamed mount can run for seconds; a fresh flag per mount means an
        // `Esc` for one can't cancel the next.
        self.runtime.archive_cancel =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.state
            .flash_info(format!("reading {}…", display_name(path)));
        vec![Effect::Archive(ArchiveOp::Mount {
            path: path.to_path_buf(),
            staging_root,
            limits: self.state.config.archive.limits(),
            max_entries: self.state.config.archive.max_entries,
            confirmed,
            cancel: std::sync::Arc::clone(&self.runtime.archive_cancel),
        })]
    }

    /// Ask the in-flight streamed mount to stop. No-op when nothing is running —
    /// the flag is only read by a live extraction pass.
    pub(super) fn cancel_archive_mount(&self) {
        self.runtime.archive_cancel.store(true, Ordering::Relaxed);
    }

    /// Get a member's bytes onto disk, then do `then`.
    ///
    /// Already staged — from a streamed mount, or a previous read — means acting
    /// immediately; otherwise the extraction rides a worker and `then` happens
    /// when it lands. Either way the caller doesn't have to know which.
    pub(super) fn open_member(&mut self, path: &Path, then: MaterializeThen) -> Vec<Effect> {
        let Some((mount, _)) = self.state.mounts.resolve(path) else {
            return Vec::new();
        };
        let Some(entry) = mount.entry_at(path).cloned() else {
            self.state.flash_error("archive: no such member");
            return Vec::new();
        };
        if entry.kind == crate::archive::index::ArchiveEntryKind::Dir {
            return Vec::new();
        }
        let staged = mount.staging_path(&entry);
        if staged.exists() {
            return self.do_after_materialize(then, &staged);
        }
        if !entry.readable {
            self.state.flash_error(format!(
                "{}: encrypted or unsupported compression",
                entry.inner
            ));
            return Vec::new();
        }
        vec![Effect::Archive(ArchiveOp::Materialize {
            archive: mount.archive().to_path_buf(),
            entry: Box::new(entry),
            staging_root: mount.staging_root.clone(),
            then,
        })]
    }

    /// Drain landed archive outcomes. Returns whether to redraw, plus any
    /// follow-on effects (a pager open after a materialize).
    pub(crate) fn apply_archive_outcomes(&mut self) -> (bool, Vec<Effect>) {
        let landed: Vec<ArchiveOutcome> =
            std::mem::take(&mut *self.runtime.archive_results.lock().unwrap());
        if landed.is_empty() {
            return (false, Vec::new());
        }
        let mut effects = Vec::new();
        for outcome in landed {
            effects.extend(self.apply_one_archive_outcome(outcome));
        }
        (true, effects)
    }

    fn apply_one_archive_outcome(&mut self, outcome: ArchiveOutcome) -> Vec<Effect> {
        match outcome {
            ArchiveOutcome::Mounted {
                index,
                capability,
                warnings,
                staging_root,
            } => {
                let archive = index.archive.clone();
                let notes = mount_notes(&capability, &warnings);
                let protected = self.protected_archives();
                let evicted = self.state.mounts.insert(
                    ArchiveMount {
                        index: *index,
                        journal: Journal::default(),
                        staged: StagedStats::new(),
                        capability,
                        warnings,
                        staging_root,
                        last_used: 0,
                    },
                    &protected,
                );
                // Enter it, then say what was odd about it — the flash outlives
                // the chdir, so the note is what the user is left reading.
                self.enter_mount(&archive);
                if let Some(note) = notes.first() {
                    let extra = notes.len().saturating_sub(1);
                    self.state.flash_info(if extra > 0 {
                        format!("{note} (+{extra} more — :archive info)")
                    } else {
                        note.clone()
                    });
                }
                self.view.needs_full_repaint = true;
                clean_effects(evicted)
            }
            ArchiveOutcome::NeedsConfirm { path, question } => {
                self.state.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::ArchiveMountConfirm { path },
                    format!("{question} [Y/n] "),
                ));
                Vec::new()
            }
            // The name said archive, the bytes said otherwise: open it as the
            // file it is, which is what `Enter` would have done anyway.
            ArchiveOutcome::NotAnArchive { path, then } => self.do_after_materialize(then, &path),
            ArchiveOutcome::Failed { path, error } => {
                self.state
                    .flash_error(format!("{}: {error}", display_name(&path)));
                Vec::new()
            }
            ArchiveOutcome::Materialized { real, then } => {
                self.record_staged(&real);
                self.do_after_materialize(then, &real)
            }
            ArchiveOutcome::MaterializedMany { staged, then } => {
                for (_, real) in &staged {
                    self.record_staged(real);
                }
                match then {
                    MaterializeThen::OpenPager(dest) => {
                        if let Some((_, real)) = staged.first() {
                            self.open_staged_in_pager(real, dest);
                        }
                        Vec::new()
                    }
                    MaterializeThen::Retry(effect) => {
                        let map: std::collections::HashMap<PathBuf, PathBuf> =
                            staged.into_iter().collect();
                        vec![super::archive_route::rewrite_paths(*effect, &map)]
                    }
                }
            }
            ArchiveOutcome::MaterializeFailed { error } => {
                self.state.flash_error(error);
                Vec::new()
            }
            ArchiveOutcome::Written { archive, report } => {
                // The journal is only cleared once the archive on disk actually
                // holds the changes — a failed write leaves them pending.
                if let Some(mount) = self.state.mounts.get_mut(&archive) {
                    mount.journal.clear();
                    mount.staged.clear();
                }
                let snapshot = report
                    .snapshot
                    .as_ref()
                    .map_or_else(String::new, |_| " (original in the graveyard)".to_string());
                self.state.flash_info(format!(
                    "wrote {} — {} member(s), {}{snapshot}",
                    display_name(&archive),
                    report.members,
                    crate::fs::ops::format_size(report.bytes),
                ));
                // Re-mount so the index describes what is now on disk rather than
                // what used to be.
                self.request_mount(&archive, true)
            }
            ArchiveOutcome::WriteFailed { archive, error } => {
                self.state.flash_error(format!(
                    "{} NOT written: {error} — changes are still pending",
                    display_name(&archive)
                ));
                Vec::new()
            }
            ArchiveOutcome::Cleaned => Vec::new(),
        }
    }

    /// Carry out what was waiting on a member's bytes.
    fn do_after_materialize(&mut self, then: MaterializeThen, real: &Path) -> Vec<Effect> {
        match then {
            MaterializeThen::OpenPager(dest) => {
                self.open_staged_in_pager(real, dest);
                Vec::new()
            }
            // A single-member retry: the effect's own paths are rewritten from
            // this one mapping.
            MaterializeThen::Retry(effect) => {
                let mut staged = std::collections::HashMap::new();
                if let Some(original) = self.mount_path_for_staged(real) {
                    staged.insert(original, real.to_path_buf());
                }
                vec![super::archive_route::rewrite_paths(*effect, &staged)]
            }
        }
    }

    /// The mount path a staged file stands in for — the inverse of
    /// `ArchiveMount::staging_path`, used to rewrite a held-back effect.
    fn mount_path_for_staged(&self, real: &Path) -> Option<PathBuf> {
        let mount = self
            .state
            .mounts
            .iter()
            .find(|m| real.starts_with(&m.staging_root))?;
        let rel = real.strip_prefix(&mount.staging_root).ok()?;
        // A case-colliding member stages under its own prefix, so strip that
        // before reading the path back as a member.
        let rel = rel
            .components()
            .next()
            .and_then(|first| {
                first
                    .as_os_str()
                    .to_str()
                    .filter(|s| s.starts_with(".spyc-case-"))
                    .and_then(|_| rel.strip_prefix(first).ok())
            })
            .unwrap_or(rel);
        Some(mount.archive().join(rel))
    }

    /// Open an already-extracted member. The pager's own planner takes it from
    /// here — by now it's an ordinary local file.
    fn open_staged_in_pager(&mut self, real: &Path, dest: PagerDest) {
        if let Some(op) = self.plan_pager_open(real, None, dest) {
            self.spawn_file_op(op);
        }
    }

    /// Remember what a freshly staged file looked like, so an edit spyc didn't
    /// make is visible later as a size/mtime that no longer matches.
    fn record_staged(&mut self, real: &Path) {
        let Ok(md) = std::fs::metadata(real) else {
            return;
        };
        let Some(archive) = self
            .state
            .mounts
            .iter()
            .find(|m| real.starts_with(&m.staging_root))
            .map(|m| m.archive().to_path_buf())
        else {
            return;
        };
        let Some(mount) = self.state.mounts.get_mut(&archive) else {
            return;
        };
        let Ok(rel) = real.strip_prefix(&mount.staging_root) else {
            return;
        };
        let key = rel
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        mount.staged.insert(
            key,
            crate::archive::journal::StagedStat {
                size: md.len(),
                mtime: md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                is_dir: md.is_dir(),
            },
        );
    }

    /// Move the focused column to a mount's root.
    fn enter_mount(&mut self, archive: &Path) {
        self.state.mounts.touch(archive);
        if let Err(e) = self.state.chdir(archive) {
            self.state.flash_error(format!("archive: {e:#}"));
        }
    }

    /// Archives that must not be evicted: one a column is standing in, or one
    /// carrying changes the user hasn't written back.
    fn protected_archives(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for dir in self.state.column_dirs() {
            if let Some((mount, _)) = self.state.mounts.resolve(&dir) {
                out.push(mount.archive().to_path_buf());
            }
        }
        out.extend(
            self.state
                .mounts
                .iter()
                .filter(|m| m.is_dirty())
                .map(|m| m.archive().to_path_buf()),
        );
        out
    }

    /// Drop a mount and its staged bytes. Refuses while a column is inside it —
    /// unmounting the ground you're standing on would strand the column.
    pub(super) fn unmount_archive(&mut self, archive: &Path) -> Vec<Effect> {
        if self.state.column_dirs().iter().any(|d| {
            self.state
                .mounts
                .resolve(d)
                .is_some_and(|(m, _)| m.archive() == archive)
        }) {
            self.state.flash_error("archive: climb out of it first (h)");
            return Vec::new();
        }
        if let Some(staging) = self.state.mounts.remove(archive) {
            self.state
                .flash_info(format!("unmounted {}", display_name(archive)));
            clean_effects(vec![staging])
        } else {
            self.state.flash_error("archive: not mounted");
            Vec::new()
        }
    }

    /// `:archive [info|list|unmount]`.
    pub(crate) fn cmd_archive(&mut self, arg: &str) -> Vec<Effect> {
        match arg.trim() {
            "" | "info" => {
                self.archive_info();
                Vec::new()
            }
            "list" => {
                self.archive_list();
                Vec::new()
            }
            "write" => {
                if let Some((mount, _)) = self.state.mounts.resolve(&self.state.cur().listing.dir) {
                    let archive = mount.archive().to_path_buf();
                    self.write_effect(&archive).into_iter().collect()
                } else {
                    self.state.flash_error("archive: not inside an archive");
                    Vec::new()
                }
            }
            "discard" => {
                if let Some((mount, _)) = self.state.mounts.resolve(&self.state.cur().listing.dir) {
                    let archive = mount.archive().to_path_buf();
                    if let Some(m) = self.state.mounts.get_mut(&archive) {
                        m.journal.clear();
                    }
                    // Re-mount to throw away any edited staged copies along with
                    // the journal, so "discard" means all of it.
                    self.state.flash_info("archive: pending changes discarded");
                    self.request_mount(&archive, true)
                } else {
                    self.state.flash_error("archive: not inside an archive");
                    Vec::new()
                }
            }
            "cancel" => {
                self.cancel_archive_mount();
                self.state.flash_info("archive: cancel requested");
                Vec::new()
            }
            "unmount" => {
                let dir = self.state.cur().listing.dir.clone();
                if let Some((mount, _)) = self.state.mounts.resolve(&dir) {
                    let archive = mount.archive().to_path_buf();
                    // Unwritten changes would go with the mount, so ask before
                    // dropping them rather than after.
                    if self.mount_is_dirty(&archive)
                        && self.state.config.archive.write_back == crate::config::WriteBack::Ask
                    {
                        let counts = self
                            .state
                            .mounts
                            .get(&archive)
                            .map_or_else(String::new, |m| m.journal.counts().badge());
                        self.state.mode = Mode::Prompting(Prompt::simple(
                            PromptKind::ArchiveWriteConfirm {
                                archive: archive.clone(),
                            },
                            format!("write {counts} back to {}? [Y/n] ", display_name(&archive)),
                        ));
                        return Vec::new();
                    }
                    // Step out *now*, not via a deferred `ChangeDir`: the unmount
                    // below refuses while a column is inside, and a queued effect
                    // hasn't moved it yet.
                    if let Some(parent) = archive.parent().map(Path::to_path_buf) {
                        self.state
                            .change_dir(&parent, Some(&archive), None, "chdir");
                    }
                    self.unmount_archive(&archive)
                } else {
                    self.state.flash_error("archive: not inside an archive");
                    Vec::new()
                }
            }
            other => {
                self.state.flash_error(format!(
                    "archive: unknown subcommand `{other}` \
                     (info | list | write | discard | unmount | cancel)"
                ));
                Vec::new()
            }
        }
    }

    /// Everything spyc knows about the mount the cursor is in.
    fn archive_info(&mut self) {
        let dir = self.state.cur().listing.dir.clone();
        let Some((mount, inner)) = self.state.mounts.resolve(&dir) else {
            self.state.flash_error("archive: not inside an archive");
            return;
        };
        let mut lines = vec![
            format!("archive: {}", mount.archive().display()),
            format!("format:  {}", mount.format().label()),
            format!(
                "members: {}{}",
                mount.index.entries.len(),
                if mount.index.truncated {
                    " (capped)"
                } else {
                    ""
                }
            ),
            format!(
                "size:    {} uncompressed, {} on disk",
                crate::fs::ops::format_size(mount.index.total_uncompressed),
                crate::fs::ops::format_size(mount.index.compressed_size),
            ),
            format!(
                "write:   {}",
                mount
                    .capability
                    .reason()
                    .map_or_else(|| "yes".to_string(), |why| format!("no — {why}")),
            ),
            format!("staging: {}", mount.staging_root.display()),
            format!("here:    /{inner}"),
        ];
        if mount.is_dirty() {
            lines.push(format!("pending: {}", mount.journal.counts().badge()));
        }
        if !mount.warnings.is_empty() {
            lines.push(String::new());
            lines.push("notes:".to_string());
            lines.extend(mount.warnings.iter().map(|w| format!("  {w}")));
        }
        self.open_archive_dump("archive info", lines);
    }

    /// Every mounted archive, for when several are open at once.
    fn archive_list(&mut self) {
        if self.state.mounts.is_empty() {
            self.state.flash_info("archive: nothing mounted");
            return;
        }
        let lines: Vec<String> = self
            .state
            .mounts
            .iter()
            .map(|m| {
                let badge = if m.is_dirty() {
                    format!(" [{}]", m.journal.counts().badge())
                } else {
                    String::new()
                };
                let ro = if m.capability.is_writable() {
                    ""
                } else {
                    " (ro)"
                };
                format!(
                    "{} — {} members, {}{}{}",
                    m.archive().display(),
                    m.index.entries.len(),
                    m.format().label(),
                    ro,
                    badge
                )
            })
            .collect();
        self.open_archive_dump("mounted archives", lines);
    }

    /// Open a text dump in the pager — the same shape as `:activity dump` and
    /// `:agent list`.
    fn open_archive_dump(&mut self, title: &'static str, lines: Vec<String>) {
        let mut view = crate::ui::pager::PagerView::new_plain(title, lines);
        view.saveable = true;
        self.set_pager(view);
    }

    /// Run an outgoing effect past the archive screen.
    ///
    /// `Some` means execute it; `None` means it was held back for extraction (or
    /// refused), and the drain will bring it round again once the bytes exist.
    pub(super) fn screen_archive_effect(&mut self, effect: Effect) -> Option<Effect> {
        use super::archive_route::{ArchiveSink, route_archive_effect};
        let staged_check = |p: &Path| {
            self.state
                .mounts
                .resolve(p)
                .and_then(|(mount, _)| mount.entry_at(p).map(|e| mount.is_materialized(e)))
                .unwrap_or(false)
        };
        match route_archive_effect(effect, &self.state.mounts, &staged_check) {
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
            ArchiveSink::Record(changes) => {
                self.record_pending(changes);
                None
            }
        }
    }

    /// The extraction op for a batch of members, to run in place of the effect
    /// that wanted them.
    fn extraction_effect(&mut self, members: &[PathBuf], then: MaterializeThen) -> Option<Effect> {
        let (mount, _) = members.first().and_then(|p| self.state.mounts.resolve(p))?;
        let archive = mount.archive().to_path_buf();
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
            .flash_info(format!("extracting {} member(s)…", entries.len()));
        Some(Effect::Archive(ArchiveOp::MaterializeMany {
            archive,
            entries,
            staging_root,
            then,
        }))
    }

    /// Answer to "write these changes back?" — `yes` writes and then unmounts,
    /// `no` unmounts and leaves the changes pending in nothing (the mount is
    /// gone), which is why the prompt defaults to yes.
    pub(super) fn finish_archive_write_confirm(
        &mut self,
        archive: &Path,
        yes: bool,
    ) -> Vec<Effect> {
        if yes {
            // Unmount after the write lands, not before: the drain re-mounts the
            // archive so the index matches what is now on disk.
            return self.write_effect(archive).into_iter().collect();
        }
        if let Some(parent) = archive.parent().map(Path::to_path_buf) {
            self.state.change_dir(&parent, Some(archive), None, "chdir");
        }
        self.state
            .flash_info("archive: unmounted, changes discarded");
        self.unmount_archive(archive)
    }

    /// Fold recorded changes into the mounts' journals.
    fn record_pending(&mut self, changes: Vec<super::archive_route::PendingChange>) {
        use super::archive_route::PendingChange;
        let mut deleted = 0usize;
        for change in changes {
            match change {
                PendingChange::Delete { archive, inner } => {
                    if let Some(mount) = self.state.mounts.get_mut(&archive) {
                        mount.journal.delete(inner);
                        deleted += 1;
                    }
                }
            }
        }
        if deleted > 0 {
            self.state.refresh_listing();
            self.state.flash_info(format!(
                "{deleted} member(s) marked for removal — :archive write to apply"
            ));
        }
    }

    /// The write-back op for a mount, or `None` when there is nothing to write.
    ///
    /// The plan is built here, on the main thread, from the journal plus a fresh
    /// stat of the staged files — the comparison against what spyc recorded at
    /// materialize time is what notices an edit made by an editor or an agent.
    fn write_effect(&mut self, archive: &Path) -> Option<Effect> {
        const MB: u64 = 1024 * 1024;
        let snapshot_limit = self.state.config.archive.snapshot_max_mb.saturating_mul(MB);
        // Everything the op needs is gathered before the first flash, so the
        // immutable read of the mount ends before the mutable borrow begins.
        let prepared = {
            let mount = self.state.mounts.get(archive)?;
            let now = current_staged_stats(mount);
            if !mount.is_dirty() && !differs(&mount.staged, &now) {
                None
            } else if let Some(why) = mount.capability.reason() {
                Some(Err(format!("archive is read-only: {why}")))
            } else {
                Some(Ok(ArchiveOp::Write {
                    steps: crate::archive::plan_repack(
                        &mount.index,
                        &mount.journal,
                        &mount.staged,
                        &now,
                    ),
                    index: Box::new(mount.index.clone()),
                    staging_root: mount.staging_root.clone(),
                    opts: crate::archive::write::RepackOptions {
                        snapshot_original: mount.index.compressed_size <= snapshot_limit,
                        free_space_margin: 8 * MB,
                    },
                }))
            }
        };
        match prepared {
            None => {
                self.state.flash_info("archive: nothing to write");
                None
            }
            Some(Err(why)) => {
                self.state.flash_error(why);
                None
            }
            Some(Ok(op)) => {
                self.state
                    .flash_info(format!("writing {}…", display_name(archive)));
                Some(Effect::Archive(op))
            }
        }
    }

    /// Whether a mount has anything a write would change — the journal, or a
    /// staged file that no longer matches what spyc wrote.
    pub(super) fn mount_is_dirty(&self, archive: &Path) -> bool {
        let Some(mount) = self.state.mounts.get(archive) else {
            return false;
        };
        mount.is_dirty() || differs(&mount.staged, &current_staged_stats(mount))
    }

    /// Archives carrying changes nobody has written back. Read at quit time, so
    /// the warning names them rather than counting silently.
    pub(super) fn dirty_mounts(&self) -> Vec<PathBuf> {
        self.state
            .mounts
            .iter()
            .map(|m| m.archive().to_path_buf())
            .filter(|a| self.mount_is_dirty(a))
            .collect()
    }

    /// Best-effort removal of every staging tree this process created. Called at
    /// teardown; the startup sweep is the backstop for a process that died.
    pub(super) fn clean_all_staging(&mut self) {
        for root in self.state.mounts.drain_all() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

/// Remove staging trees left by spyc processes that are gone.
///
/// Staging directories are named `<pid>-<hash>`, so a dir whose pid no longer
/// exists belongs to a crashed or killed run and nothing will ever come back for
/// it. Runs at startup, off the hot path, and ignores every error — a stale
/// directory is untidy, never fatal.
pub fn sweep_orphan_staging() {
    let Some(root) = archives_root() else { return };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let me = std::process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some((pid, _)) = name.split_once('-') else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        if pid != me && !crate::sysinfo::pid_alive(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn clean_effects(roots: Vec<PathBuf>) -> Vec<Effect> {
    if roots.is_empty() {
        Vec::new()
    } else {
        vec![Effect::Archive(ArchiveOp::Clean {
            staging_roots: roots,
        })]
    }
}

/// Where all staging trees live.
fn archives_root() -> Option<PathBuf> {
    crate::state::state_root().map(|r| r.join("archives"))
}

/// Staging directory for one archive: `<pid>-<hash of its path>`.
///
/// The pid scopes it to this process, so two spyc instances browsing the same
/// archive don't share a tree and the orphan sweep can tell whose is whose. The
/// hash keeps the name short and free of separators.
fn staging_root_for(archive: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    archive.hash(&mut hasher);
    Some(archives_root()?.join(format!("{}-{:016x}", std::process::id(), hasher.finish())))
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Stat every staged file a mount knows about, keyed the same way the recorded
/// stats are. The difference between the two is the only signal that an edit spyc
/// didn't perform has happened.
fn current_staged_stats(mount: &ArchiveMount) -> crate::archive::journal::StagedStats {
    let mut now = crate::archive::journal::StagedStats::new();
    for entry in &mount.index.entries {
        let path = mount.staging_path(entry);
        let Ok(md) = std::fs::metadata(&path) else {
            continue;
        };
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

/// Whether anything staged differs from what spyc recorded when it wrote it.
fn differs(
    recorded: &crate::archive::journal::StagedStats,
    now: &crate::archive::journal::StagedStats,
) -> bool {
    now.iter()
        .any(|(inner, current)| recorded.get(inner) != Some(current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_staging_root_is_scoped_to_this_process_and_the_archive() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let a = staging_root_for(Path::new("/src/one.zip")).unwrap();
            let b = staging_root_for(Path::new("/src/two.zip")).unwrap();
            assert_ne!(a, b, "different archives never share a tree");
            assert_eq!(
                a,
                staging_root_for(Path::new("/src/one.zip")).unwrap(),
                "the same archive resolves to the same tree"
            );
            let name = a.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                name.starts_with(&format!("{}-", std::process::id())),
                "the pid scopes it: {name}"
            );
            assert!(a.starts_with(tmp.path().join("archives")));
        });
    }

    /// The sweep is what makes a `SIGKILL`ed spyc's staging bytes recoverable
    /// disk space, so it must reap a dead pid's tree and never touch a live one.
    #[test]
    fn the_sweep_reaps_dead_processes_and_spares_the_living() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let root = archives_root().unwrap();
            let mine = root.join(format!("{}-abc", std::process::id()));
            // pid 1 is always alive; a pid this high is not in use on any
            // reasonable system (Linux caps at 2^22 by default).
            let alive = root.join("1-def");
            let dead = root.join("4194304-999");
            let junk = root.join("not-a-pid");
            for d in [&mine, &alive, &dead, &junk] {
                std::fs::create_dir_all(d).unwrap();
            }

            sweep_orphan_staging();

            assert!(mine.exists(), "our own tree is in use");
            assert!(
                alive.exists(),
                "another running spyc's tree is not ours to delete"
            );
            assert!(!dead.exists(), "a dead process's tree is reaped");
            assert!(junk.exists(), "an unrecognized name is left alone");
        });
    }

    #[test]
    fn the_sweep_is_a_no_op_with_no_archives_dir() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), sweep_orphan_staging);
    }

    #[test]
    fn display_name_falls_back_to_the_whole_path() {
        assert_eq!(display_name(Path::new("/a/b/pkg.zip")), "pkg.zip");
        assert_eq!(display_name(Path::new("/")), "/");
    }
}

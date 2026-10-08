//! `:archive` and the verbs behind it (info, list, write, discard, unmount,
//! cancel), plus the answer to the write-before-unmount prompt.

use std::path::{Path, PathBuf};

use super::{clean_effects, current_staged_stats, differs, display_name};
use crate::app::archive_ops::ArchiveOp;
use crate::app::{App, Effect, Mode, Prompt, PromptKind};

impl App {
    /// Drop a mount and its staged bytes. Refuses while a column is inside it —
    /// unmounting the ground you're standing on would strand the column.
    pub(in crate::app) fn unmount_archive(&mut self, archive: &Path) -> Vec<Effect> {
        if self.state.column_dirs().iter().any(|d| {
            self.state
                .mounts
                .resolve(d)
                .is_some_and(|(m, _)| m.archive() == archive)
        }) {
            self.state.flash_error("archive: climb out of it first (h)");
            return Vec::new();
        }
        // Dropping this one would delete the staged copy another mount is reading
        // from, leaving that one pointing at nothing.
        let staging = self
            .state
            .mounts
            .get(archive)
            .map(|m| m.staging_root.clone());
        let child = staging.and_then(|staging| {
            self.state
                .mounts
                .iter()
                .find(|m| m.depth > 0 && m.archive() != archive && m.source().starts_with(&staging))
                .map(|m| m.archive().to_path_buf())
        });
        if let Some(child) = child {
            self.state.flash_error(format!(
                "archive: unmount {} first — it lives inside this one",
                display_name(&child)
            ));
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

    /// Throw away an archive's pending changes: the journal *and* the staged copies
    /// that superseded its bytes.
    ///
    /// Clearing the journal alone left an edited copy on disk, and the next scan
    /// re-recorded it as pending moments later — so discarded changes came straight
    /// back. Re-mounting would drop them too, but a live mount is re-*entered*
    /// rather than re-read, and issuing a staging clean beside a fresh mount races
    /// it: each archive op rides its own worker thread and both name the same
    /// staging path. Removing exactly what was recorded needs neither.
    fn discard_pending(&mut self, archive: &Path) {
        let mut drop_files: Vec<(String, PathBuf)> = Vec::new();
        if let Some(mount) = self.state.mounts.get(archive) {
            for change in mount.journal.changes() {
                match change {
                    // A member the user brought in has no index entry; its bytes sit
                    // under its own inner path.
                    crate::archive::Change::Added { inner } => {
                        drop_files.push((inner.clone(), mount.staging_root.join(inner)));
                    }
                    // A replaced one is at its entry's staging path, which differs
                    // from the inner path for a case-colliding member.
                    crate::archive::Change::Replaced { inner } => {
                        if let Some(entry) = mount.index.get(inner) {
                            drop_files.push((inner.clone(), mount.staging_path(entry)));
                        }
                    }
                    // A delete or rename never wrote anything: they're index edits.
                    crate::archive::Change::Deleted { .. }
                    | crate::archive::Change::Renamed { .. } => {}
                }
            }
        }
        for (_, path) in &drop_files {
            let _ = std::fs::remove_file(path);
        }
        if let Some(mount) = self.state.mounts.get_mut(archive) {
            for (inner, _) in &drop_files {
                mount.staged.remove(inner);
            }
            mount.journal.clear();
            mount.editing.clear();
        }
        self.state.flash_info("archive: pending changes discarded");
        // The rows come from the index plus the journal, so they have to be rebuilt
        // for the deletes and renames to come back.
        self.state.refresh_listing();
    }

    /// Which archive `:archive write` / `discard` means.
    ///
    /// Inside a mount it's that one. From outside it used to be nothing at all,
    /// which left the container's `~` marker as a dead end: you could see an
    /// archive held unwritten changes and had to climb back in to act on it. So:
    /// the archive under the cursor if it's a dirty mount — that's the row telling
    /// you — else the only dirty mount when there's exactly one. More than one is
    /// named rather than guessed at.
    fn archive_to_act_on(&self, verb: &str) -> Result<PathBuf, String> {
        if let Some((mount, _)) = self.state.mounts.resolve(&self.state.cur().listing.dir) {
            return Ok(mount.archive().to_path_buf());
        }
        let cursor = self
            .state
            .cur()
            .rows
            .get(self.state.cur().cursor.index)
            .map(|r| r.path.clone());
        if let Some(path) = cursor
            && self.state.mounts.get(&path).is_some()
            && self.mount_is_dirty(&path)
        {
            return Ok(path);
        }
        let dirty = self.dirty_mounts();
        match dirty.len() {
            0 => Err(format!("archive: nothing to {verb}")),
            1 => Ok(dirty[0].clone()),
            _ => Err(format!(
                "archive: {} archives have changes — put the cursor on the one to {verb}, \
                 or go into it",
                dirty.len()
            )),
        }
    }

    /// `:archive [info|list|unmount]`.
    pub(crate) fn cmd_archive(&mut self, arg: &str) -> Vec<Effect> {
        // Anything about to be reported — or written — should account for edits
        // made outside spyc's own ops, including to members the user never opened.
        self.scan_archive_edits();
        match arg.trim() {
            "" | "info" => {
                self.archive_info();
                Vec::new()
            }
            "list" => {
                self.archive_list();
                Vec::new()
            }
            "write" => match self.archive_to_act_on("write") {
                Ok(archive) => self.write_effect(&archive).into_iter().collect(),
                Err(why) => {
                    self.state.flash_error(why);
                    Vec::new()
                }
            },
            "discard" => match self.archive_to_act_on("discard") {
                Ok(archive) => {
                    self.discard_pending(&archive);
                    Vec::new()
                }
                Err(why) => {
                    self.state.flash_error(why);
                    Vec::new()
                }
            },
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
            // Out here the useful answer is what *is* mounted — an archive holding
            // unwritten changes is otherwise invisible from outside it.
            if self.state.mounts.is_empty() {
                self.state.flash_error("archive: not inside an archive");
            } else {
                self.archive_list();
            }
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

    /// Answer to "write these changes back?" — `yes` writes and then unmounts,
    /// `no` unmounts and leaves the changes pending in nothing (the mount is
    /// gone), which is why the prompt defaults to yes.
    pub(in crate::app) fn finish_archive_write_confirm(
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
                        verify_budget: self
                            .state
                            .config
                            .archive
                            .extract_budget_mb
                            .saturating_mul(MB),
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
                    .flash_progress(format!("writing {}…", display_name(archive)));
                Some(Effect::Archive(op))
            }
        }
    }
}

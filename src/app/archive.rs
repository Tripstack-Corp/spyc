//! App-layer glue for archive mounts: kicking a mount, applying what the worker
//! sends back, the staging-tree lifecycle, and the `:archive` command.
//!
//! The Model holds the mounts (`state.mounts`) and the pure `archive` crate does
//! the deciding; this is the impure half that talks to the worker and the disk.
//! Mounting and the worker's outcomes live here; the `:archive` verbs are in
//! `command`, the effect screen's App half in `screen`, and staged-edit
//! tracking in `edits`.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use crate::archive::mount::ArchiveMount;
use crate::archive::{Journal, journal::StagedStats};

use super::archive_ops::{ArchiveOp, ArchiveOutcome, MaterializeThen, mount_notes};
use super::state::ProjectId;
use super::{App, Effect, Mode, Prompt, PromptKind};

mod command;
mod edits;
mod reads;
mod screen;
mod writes;

pub(super) use writes::Held;

pub(super) use reads::Joined;

impl App {
    /// Kick a mount for `path`. `confirmed` is set on the second pass, after the
    /// user answered a size prompt.
    pub(super) fn request_mount(&mut self, path: &Path, confirmed: bool) -> Vec<Effect> {
        self.request_mount_then(path, confirmed, None)
    }

    /// Mount `path`, then run `then` — the held-back effect that wanted a place
    /// inside it. See [`Self::mount_and_retry`].
    pub(super) fn request_mount_then(
        &mut self,
        path: &Path,
        confirmed: bool,
        then: Option<Box<Effect>>,
    ) -> Vec<Effect> {
        // Already mounted: step back in rather than reading it again. A re-mount
        // installs a fresh journal, so it would silently discard changes the user
        // hasn't written back — and for a compressed tar it would re-stream the
        // whole archive to learn what it already knows.
        //
        // `get`, not `contains`: a nested archive's path is *inside* the mount
        // above it, so "is this path in a mount" is true before it is mounted at
        // all — and answering it would land the column on a file member's path.
        if self.state.mounts.get(path).is_some() {
            // Already mounted, so whatever was waiting on it can just run.
            if let Some(effect) = then {
                return vec![*effect];
            }
            self.enter_mount(path);
            return Vec::new();
        }
        if self.is_reading(path) {
            self.join_read(path, then);
            return Vec::new();
        }
        let Some(staging_root) = staging_root_for(path) else {
            self.state
                .flash_error("archive: no state directory to stage into");
            return Vec::new();
        };
        let cancel = self.mount_cancel();
        self.start_read(path);
        self.state
            .flash_progress(format!("reading {}…", display_name(path)));
        vec![Effect::Archive(ArchiveOp::Mount {
            path: path.to_path_buf(),
            staging_root,
            limits: self.state.active_project().config.archive.limits(),
            max_entries: self.state.active_project().config.archive.max_entries,
            confirmed,
            cancel,
            then,
            address: None,
            depth: 0,
            reset_staging: false,
        })]
    }

    /// Mount an archive that lives *inside* another one: `at` is its member path
    /// (where the column browses it), `source` the extracted copy the bytes come
    /// from.
    ///
    /// Each level costs a full copy of that container in the level above's
    /// staging — the bytes have to be a real file before anything can read them —
    /// so `[archive] max_depth` bounds how far this can go.
    pub(super) fn request_nested_mount(
        &mut self,
        at: &Path,
        source: &Path,
        then: Option<Box<Effect>>,
    ) -> Vec<Effect> {
        if self.state.mounts.get(at).is_some() {
            if let Some(effect) = then {
                return vec![*effect];
            }
            self.enter_mount(at);
            return Vec::new();
        }
        if self.refuse_inside_written(at) {
            return Vec::new();
        }
        if self.is_reading(source) {
            self.join_read(source, then);
            return Vec::new();
        }
        let depth = self
            .state
            .mounts
            .resolve(at)
            .map_or(0, |(parent, _)| parent.depth + 1);
        let max = self.state.active_project().config.archive.max_depth;
        if depth > max {
            self.state.flash_error(format!(
                "archive: {} is {depth} archives deep — [archive] max_depth is {max}",
                display_name(at)
            ));
            return Vec::new();
        }
        // Staging is keyed on the address, so re-entering a nested archive finds
        // the same tree rather than a second copy of it.
        let Some(staging_root) = staging_root_for(at) else {
            self.state
                .flash_error("archive: no state directory to stage into");
            return Vec::new();
        };
        let cancel = self.mount_cancel();
        self.start_read(source);
        self.state
            .flash_progress(format!("reading {}…", display_name(at)));
        vec![Effect::Archive(ArchiveOp::Mount {
            path: source.to_path_buf(),
            staging_root,
            limits: self.state.active_project().config.archive.limits(),
            max_entries: self.state.active_project().config.archive.max_entries,
            confirmed: false,
            cancel,
            then,
            address: Some(at.to_path_buf()),
            depth,
            reset_staging: false,
        })]
    }

    /// Re-read an archive spyc has just rewritten.
    ///
    /// A repack renumbers a zip's entries and moves a tar's offsets, so every
    /// locator in the old index points at the wrong bytes and the mount has to be
    /// built again from the file on disk. The stale staging tree goes with it —
    /// **in the same op**, because each archive op runs on its own thread and a
    /// separate `Clean` would race the extraction refilling that directory.
    ///
    /// `nested_source` is the staged copy for an archive that lives inside
    /// another one; `None` for a file on disk.
    pub(super) fn request_remount(
        &mut self,
        archive: &Path,
        nested_source: Option<PathBuf>,
    ) -> Vec<Effect> {
        let Some(staging_root) = staging_root_for(archive) else {
            self.state
                .flash_error("archive: no state directory to stage into");
            return Vec::new();
        };
        let depth = if nested_source.is_some() {
            self.state
                .mounts
                .resolve(archive)
                .map_or(1, |(parent, _)| parent.depth + 1)
        } else {
            0
        };
        let cancel = self.mount_cancel();
        let read = nested_source.unwrap_or_else(|| archive.to_path_buf());
        self.start_read(&read);
        vec![Effect::Archive(ArchiveOp::Mount {
            path: read,
            staging_root,
            limits: self.state.active_project().config.archive.limits(),
            max_entries: self.state.active_project().config.archive.max_entries,
            // Already read once at this size — re-reading it is not a new
            // decision for the user to make.
            confirmed: true,
            cancel,
            then: None,
            address: Some(archive.to_path_buf()),
            depth,
            reset_staging: true,
        })]
    }

    /// A fresh cancel flag for one mount, so a cancel for one can't stop the
    /// next. A streamed mount can run for seconds, and `:archive cancel`
    /// stops the last one the user started; one started in the background,
    /// for a project out of view or closed, keeps its flag to itself.
    fn mount_cancel(&mut self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        if !self.state.in_background {
            self.runtime.archive_cancel = std::sync::Arc::clone(&cancel);
        }
        cancel
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
        let Some((mount, _)) = self.state.mounts.member_of(path) else {
            return Vec::new();
        };
        let entry = match mount.member_at(path) {
            Some(crate::archive::mount::MemberRef::Archived(entry)) => entry.clone(),
            // A file the user brought in: the staging tree is the only copy, so
            // there is nothing to extract and nothing to fall through to.
            Some(crate::archive::mount::MemberRef::Added { rel, .. }) => {
                let staged = mount.staging_root.join(rel);
                if staged.is_dir() {
                    return Vec::new();
                }
                if !staged.exists() {
                    self.state
                        .flash_error("archive: the added file's staged copy is gone");
                    return Vec::new();
                }
                return self.do_after_materialize(then, &staged);
            }
            None => {
                self.state.flash_error("archive: no such member");
                return Vec::new();
            }
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
            // Where the bytes are, not where the mount is addressed — a nested
            // archive is read out of its staged copy.
            archive: mount.source().to_path_buf(),
            entry: Box::new(entry),
            staging_root: mount.staging_root.clone(),
            then,
        })]
    }

    /// Drain landed archive outcomes. Returns whether to redraw, plus any
    /// follow-on effects (a pager open after a materialize).
    pub(crate) fn apply_archive_outcomes(&mut self) -> (bool, Vec<Effect>) {
        let landed: Vec<(ProjectId, ArchiveOutcome)> = std::mem::take(
            &mut *self
                .runtime
                .archive_results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if landed.is_empty() {
            return (false, Vec::new());
        }
        let mut effects = Vec::new();
        for (origin, outcome) in landed {
            self.finish_write(&outcome);
            let joined = self.finish_read(&outcome);
            let mounted = match &outcome {
                ArchiveOutcome::Mounted { index, .. } => Some(index.archive.clone()),
                _ => None,
            };
            if origin == self.state.active {
                effects.extend(self.apply_one_archive_outcome(outcome));
            } else {
                self.apply_archive_outcome_elsewhere(origin, outcome);
            }
            if let Some(joined) = joined {
                effects.extend(self.serve_joined(joined, mounted.as_deref()));
            }
        }
        (true, effects)
    }

    /// An outcome for a project the user isn't in. It lands there
    /// ([`App::land_in`]), and an archive op it starts (a re-read after a
    /// write, a nested mount, a staging clean) is spawned there too, so its
    /// outcome comes back to that project. A size question or an editor would
    /// act on the view in front of the user, so each is dropped and said so
    /// (the arms check `in_background`), as is an effect held back for the
    /// mount.
    fn apply_archive_outcome_elsewhere(&mut self, origin: ProjectId, outcome: ArchiveOutcome) {
        // An in-flight message of this project's announced the op that is
        // over; one of the user's announces work still running.
        self.state.clear_progress_flash_of(origin);
        if !self.state.has_project(origin) {
            self.land_closed(origin, |app| {
                app.apply_archive_outcome_orphaned(origin, outcome);
            });
            return;
        }
        self.land_in(origin, |app| {
            let mut held_back = false;
            for effect in app.apply_one_archive_outcome(outcome) {
                match effect {
                    Effect::Archive(op) => app.spawn_archive_op(op),
                    _ => held_back = true,
                }
            }
            if held_back {
                app.state
                    .flash_error("what was waiting on the archive was dropped; run it again there");
            }
        });
    }

    /// [`Self::spawn_archive_op_from`] the active project: the one whose
    /// handler asked (`run_effects` runs in the user's project), or the one
    /// an outcome landed in (`apply_archive_outcome_elsewhere`).
    pub(super) fn spawn_archive_op(&self, op: ArchiveOp) {
        self.spawn_archive_op_from(self.state.active, op);
    }

    /// Spawn the detached archive worker: run `op`, push its outcome onto
    /// `runtime.archive_results` tagged with `origin`, and wake the loop. The
    /// single spawn site. `origin` may have closed: a re-read a closed
    /// project's write starts lands as its own.
    fn spawn_archive_op_from(&self, origin: ProjectId, op: ArchiveOp) {
        let results = std::sync::Arc::clone(&self.runtime.archive_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let outcome = super::archive_ops::run_archive_op(op);
            results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((origin, outcome));
            if let Some(tx) = wake {
                let _ = tx.send(super::Message::Wake(super::Wake::Archive));
            }
        });
    }

    fn apply_one_archive_outcome(&mut self, outcome: ArchiveOutcome) -> Vec<Effect> {
        // An outcome means the op it was announcing is over, so the in-flight
        // message goes now — before the arms below get their say. Done here rather
        // than per-arm because the one that leaked was the arm with nothing to
        // report: entering an archive that had no warnings left "reading …" on
        // screen, reading as though more were coming. Only a `Progress` flash is
        // cleared, so a warning or error already showing survives.
        self.state.clear_progress_flash();
        match outcome {
            ArchiveOutcome::Mounted {
                index,
                capability,
                warnings,
                staging_root,
                then,
                depth,
            } => self.apply_mounted(*index, capability, warnings, staging_root, then, depth),
            // A question is modal: asked over another project, it would be
            // answered for the wrong place.
            ArchiveOutcome::NeedsConfirm { path, .. } if self.state.in_background => {
                self.state.flash_error(format!(
                    "{} asks before it opens; open it again there",
                    display_name(&path)
                ));
                Vec::new()
            }
            ArchiveOutcome::NeedsConfirm {
                path,
                question,
                then,
            } => {
                // The prompt is a round trip through the keyboard, and a
                // `PromptKind` is plain data — so what was waiting on the mount
                // waits here until the answer comes back.
                self.runtime.archive_mount_then = then.map(|effect| (path.clone(), effect));
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
                    MaterializeThen::Retry(effect) => {
                        let map: std::collections::HashMap<PathBuf, PathBuf> =
                            staged.into_iter().collect();
                        vec![super::archive_route::rewrite_paths(*effect, &map)]
                    }
                    // Opening and editing act on one member, so a batch acts on
                    // the first — the paths that produce these carry exactly one.
                    then => match staged.first() {
                        Some((_, real)) => {
                            let real = real.clone();
                            self.do_after_materialize(then, &real)
                        }
                        None => Vec::new(),
                    },
                }
            }
            ArchiveOutcome::MaterializeFailed { error } => {
                self.state.flash_error(error);
                Vec::new()
            }
            ArchiveOutcome::Written { archive, report } => self.apply_written(archive, report),
            ArchiveOutcome::WriteFailed { archive, error } => {
                self.flash_write_failed(&archive, &error);
                Vec::new()
            }
            ArchiveOutcome::Cleaned => Vec::new(),
        }
    }

    /// An outcome for a project that has closed. The mount table is the
    /// process's, so what an op did to it lands as it would anywhere: a mount
    /// is recorded, an extracted member is recorded as spyc's own, and a write
    /// clears the journal and has the archive read again from clean staging,
    /// that read tagged with the closed project so it lands here too. What
    /// would act on a project (entering a mount, opening a pager or an editor,
    /// asking a question) has none to act on and is dropped; a failed write
    /// still says so, as does work held back for a member. It runs in
    /// [`App::land_closed`], so what it says is named for the closed project.
    fn apply_archive_outcome_orphaned(&mut self, origin: ProjectId, outcome: ArchiveOutcome) {
        let effects = match outcome {
            ArchiveOutcome::Mounted {
                index,
                capability,
                warnings,
                staging_root,
                depth,
                then,
            } => {
                if then.is_some() {
                    self.flash_held_back_dropped();
                }
                let archive = index.archive.clone();
                let effects = self.record_mount(*index, capability, warnings, staging_root, depth);
                self.restore_late_edits(&archive);
                effects
            }
            ArchiveOutcome::Materialized { real, then } => {
                self.record_staged(&real);
                if holds_back_work(&then) {
                    self.flash_held_back_dropped();
                }
                Vec::new()
            }
            ArchiveOutcome::MaterializedMany { staged, then } => {
                for (_, real) in &staged {
                    self.record_staged(real);
                }
                if holds_back_work(&then) {
                    self.flash_held_back_dropped();
                }
                Vec::new()
            }
            ArchiveOutcome::Written { archive, report } => self.apply_written(archive, report),
            ArchiveOutcome::WriteFailed { archive, error } => {
                self.flash_write_failed(&archive, &error);
                Vec::new()
            }
            _ => Vec::new(),
        };
        for effect in effects {
            if let Effect::Archive(op) = effect {
                self.spawn_archive_op_from(origin, op);
            }
        }
    }

    /// Say that what a closed project's archive op held back, the yank or
    /// copy-out a member was extracted for, never ran.
    fn flash_held_back_dropped(&mut self) {
        self.state
            .flash_error("what was waiting on the archive was dropped; the project closed");
    }

    fn flash_written(&mut self, archive: &Path, report: &crate::archive::write::RepackReport) {
        let snapshot = report
            .snapshot
            .as_ref()
            .map_or_else(String::new, |_| " (original in the graveyard)".to_string());
        self.state.flash_info(format!(
            "wrote {} — {} member(s), {}{snapshot}",
            display_name(archive),
            report.members,
            crate::fs::ops::format_size(report.bytes),
        ));
    }

    fn flash_write_failed(&mut self, archive: &Path, error: &str) {
        self.state.flash_error(format!(
            "{} NOT written: {error} — changes are still pending",
            display_name(archive)
        ));
    }

    /// A mount landed: record it in the (global) mount table, then enter it,
    /// or hand back the effect that was waiting for it.
    fn apply_mounted(
        &mut self,
        index: crate::archive::index::ArchiveIndex,
        capability: crate::archive::scan::Capability,
        warnings: Vec<String>,
        staging_root: PathBuf,
        then: Option<Box<Effect>>,
        depth: usize,
    ) -> Vec<Effect> {
        let archive = index.archive.clone();
        let notes = mount_notes(&capability, &warnings);
        let mut effects = self.record_mount(index, capability, warnings, staging_root, depth);
        // Enter it, then say what was odd about it — the flash outlives
        // the chdir, so the note is what the user is left reading. A
        // held-back effect names where to land instead, and re-running it
        // is what gets the cursor and the message it came with.
        match then {
            Some(effect) => {
                self.state.mounts.touch(&archive);
                effects.push(*effect);
            }
            None => self.enter_mount(&archive),
        }
        if let Some(note) = notes.first() {
            let extra = notes.len().saturating_sub(1);
            self.state.flash_info(if extra > 0 {
                format!("{note} (+{extra} more — :archive info)")
            } else {
                note.clone()
            });
        }
        self.restore_late_edits(&archive);
        self.view.needs_full_repaint = true;
        effects
    }

    /// Record a landed mount in the (global) mount table, handing back the
    /// clean-up of whatever it evicted.
    fn record_mount(
        &mut self,
        index: crate::archive::index::ArchiveIndex,
        capability: crate::archive::scan::Capability,
        warnings: Vec<String>,
        staging_root: PathBuf,
        depth: usize,
    ) -> Vec<Effect> {
        let protected = self.protected_archives();
        let mut mount = ArchiveMount {
            index,
            journal: Journal::default(),
            staged: StagedStats::new(),
            capability,
            warnings,
            staging_root,
            last_used: 0,
            depth,
            editing: Vec::new(),
        };
        // Whatever is on disk at mount time is what spyc just put there —
        // a streamed mount wrote every member during the pass. Recording
        // it is what stops the first dirty check from reading the whole
        // archive as edited behind our back.
        mount.staged = current_staged_stats(&mount);
        clean_effects(self.state.mounts.insert(mount, &protected))
    }

    /// A write-back landed: the journal is clean, and the archive is re-read.
    fn apply_written(
        &mut self,
        archive: PathBuf,
        report: crate::archive::write::RepackReport,
    ) -> Vec<Effect> {
        // The journal is only cleared once the archive on disk actually
        // holds the changes — a failed write leaves them pending.
        if let Some(mount) = self.state.mounts.get_mut(&archive) {
            mount.journal.clear();
            mount.staged.clear();
        }
        self.flash_written(&archive, &report);
        // Re-read it: a repack renumbers a zip's entries and moves a tar's
        // offsets, so every locator in the old index now points at the
        // wrong bytes. The mount has to be *dropped* for that to happen —
        // a live one is re-entered rather than re-indexed, which is right
        // everywhere except here. A nested archive is read from its staged
        // copy, which the write just replaced in place.
        let nested = self
            .state
            .mounts
            .get(&archive)
            .and_then(|m| (m.depth > 0).then(|| m.source().to_path_buf()));
        self.state.mounts.remove(&archive);
        let effects = self.request_remount(&archive, nested);
        // Writing an archive that lives inside another one changes a file
        // in that one's staging, so the outer now has a pending change of
        // its own. Say so rather than leaving it to the quit warning.
        let outer = self
            .state
            .mounts
            .member_of(&archive)
            .map(|(outer, _)| display_name(outer.archive()));
        if let Some(outer) = outer {
            self.state.flash_info(format!(
                "wrote {} into {outer} — :archive write there too",
                display_name(&archive)
            ));
        }
        effects
    }

    /// Carry out what was waiting on a member's bytes.
    fn do_after_materialize(&mut self, then: MaterializeThen, real: &Path) -> Vec<Effect> {
        match then {
            MaterializeThen::OpenPager(dest) => {
                self.open_staged_in_pager(real, dest);
                Vec::new()
            }
            // A container inside a container: the bytes are on disk now, so mount
            // them — addressed where the user sees them, read from where they are.
            MaterializeThen::Mount { at, then } => self.request_nested_mount(&at, real, then),
            // An editor takes the terminal, or an overlay in a project the user
            // isn't looking at, where it would wait unseen.
            MaterializeThen::Edit { .. } if self.state.in_background => {
                self.state
                    .flash_error("edit not started; open it again there");
                Vec::new()
            }
            MaterializeThen::Edit { in_pane } => {
                // Watch it from here: whatever the editor writes to this copy is
                // the only record that the member changed.
                self.watch_for_edits(real);
                let argv = crate::shell::resolve_editor();
                if argv.is_empty() {
                    self.state.flash_error("no $VISUAL or $EDITOR set");
                    return Vec::new();
                }
                self.state
                    .flash_info("editing the extracted copy — :archive write to put it back");
                if in_pane {
                    let cmd = format!(
                        "{} {}",
                        argv.join(" "),
                        crate::shell::shell_quote(&real.display().to_string())
                    );
                    self.spawn_top_overlay(&cmd);
                    return Vec::new();
                }
                let mut argv = argv;
                let program = argv.remove(0);
                argv.push(real.display().to_string());
                super::spawn_foreground(program, argv, false)
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
        for (_, dir) in self.state.every_column_dir() {
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
        // A nested mount reads from a staged copy inside the mount above it, so
        // evicting the parent would take the child's bytes with it.
        for child in self.state.mounts.iter() {
            if child.depth > 0
                && let Some((parent, _)) = self.state.mounts.resolve(child.archive())
            {
                out.push(parent.archive().to_path_buf());
            }
        }
        out
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

/// [`super::archive_route::archive_ancestor`] against the real filesystem.
///
/// The name test runs before the `is_file` stat and costs no syscall, so an
/// ordinary path pays nothing here — only one with an archive-shaped component in
/// it goes on to ask the disk.
pub(super) fn archive_ancestor_of(path: &Path) -> Option<(PathBuf, String)> {
    super::archive_route::archive_ancestor(path, &|p: &Path| {
        p.file_name()
            .is_some_and(|n| crate::archive::looks_mountable(&n.to_string_lossy()))
            && p.is_file()
    })
}

/// The clean-up of the staging trees `roots`. A clean-up runs on its own
/// thread, and a read of the same archive starting before it runs reads into
/// a tree at the same path, so each tree is first moved aside ([`set_aside`])
/// and the clean-up deletes only what it was handed.
fn clean_effects(roots: Vec<PathBuf>) -> Vec<Effect> {
    let roots: Vec<PathBuf> = roots.into_iter().filter_map(set_aside).collect();
    if roots.is_empty() {
        Vec::new()
    } else {
        vec![Effect::Archive(ArchiveOp::Clean {
            staging_roots: roots,
        })]
    }
}

/// Move the staging tree `root` out of its path, to a sibling the orphan sweep
/// knows by the same pid prefix, and return where it went; `None` when there
/// is no tree there to clean. A tree that can't be moved is cleaned in place.
fn set_aside(root: PathBuf) -> Option<PathBuf> {
    let aside = sibling_of(&root, "gone")?;
    match std::fs::rename(&root, &aside) {
        Ok(()) => Some(aside),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => Some(root),
    }
}

/// A path beside the staging tree `root`, unique to this process, for what is
/// moved out of it (`what`): its name keeps `root`'s, so the orphan sweep
/// knows it by the same pid prefix.
fn sibling_of(root: &Path, what: &str) -> Option<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut name = root.file_name()?.to_os_string();
    name.push(format!(".{what}-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
    Some(root.with_file_name(name))
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

/// Whether `then` carries an effect held back for the member, rather than
/// acting on the extracted copy in a project's view.
const fn holds_back_work(then: &MaterializeThen) -> bool {
    matches!(
        then,
        MaterializeThen::Retry(_) | MaterializeThen::Mount { then: Some(_), .. }
    )
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The directory a rewritten write-in op will put files in, so it can be created
/// first — the inventory and copy workers write into a directory, they don't make
/// one.
fn staging_target(effect: &Effect) -> Option<PathBuf> {
    match effect {
        Effect::Inventory(super::inventory_ops::InventoryOp::Put { dest_dir, .. }) => {
            Some(dest_dir.clone())
        }
        Effect::FileOp(super::file_ops::FileOp::Copy { dest, .. }) => Some(dest.clone()),
        _ => None,
    }
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
            let Ok(md) = std::fs::metadata(mount.staging_path(entry)) else {
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

    /// Every member's staged path maps back to the member it stands in for —
    /// including a case-ranked one, whose bytes live under the reserved directory
    /// rather than at its own name.
    ///
    /// This is the inverse of `ArchiveMount::staging_path`, and the two are
    /// derived independently: the forward direction asks the entry, the reverse
    /// direction strips a prefix. A round trip is the only thing that keeps them
    /// agreeing, and getting it wrong hands one member another's journal path —
    /// which is how edits stop being noticed.
    #[test]
    fn a_staged_path_maps_back_to_its_own_member_at_every_rank() {
        use crate::archive::ArchiveFormat;
        use crate::archive::index::{Draft, IndexBuilder, Locator};
        use crate::archive::scan::Capability;

        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut b = IndexBuilder::new(100);
            for (i, name) in ["a/README", "a/readme"].iter().enumerate() {
                b.push(name, Draft::file(1, Locator::Zip { index: i }));
            }
            let (index, _) = b.finish(PathBuf::from("/src/pkg.zip"), ArchiveFormat::Zip, 10);

            let mut app = App::test_app(tmp.path().to_path_buf());
            let staging_root = tmp.path().join("staging");
            app.state.mounts.insert(
                ArchiveMount {
                    index: index.clone(),
                    journal: Journal::default(),
                    staged: StagedStats::new(),
                    capability: Capability::ReadWrite,
                    warnings: Vec::new(),
                    staging_root: staging_root.clone(),
                    last_used: 0,
                    depth: 0,
                    editing: Vec::new(),
                },
                &[],
            );

            let mut ranks = Vec::new();
            for entry in &index.entries {
                ranks.push(entry.case_rank);
                let staged = staging_root.join(entry.staging_rel());
                assert_eq!(
                    app.mount_path_for_staged(&staged),
                    Some(PathBuf::from("/src/pkg.zip").join(&entry.inner)),
                    "{} staged at {}",
                    entry.inner,
                    entry.staging_rel().display()
                );
            }
            assert!(
                ranks.contains(&0) && ranks.iter().any(|r| *r > 0),
                "the fixture must cover both an unranked and a ranked member: {ranks:?}"
            );
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

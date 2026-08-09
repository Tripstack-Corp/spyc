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
            match then {
                MaterializeThen::OpenPager(dest) => self.open_staged_in_pager(&staged, dest),
            }
            return Vec::new();
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
            ArchiveOutcome::NotAnArchive { path, then } => {
                match then {
                    MaterializeThen::OpenPager(dest) => self.open_staged_in_pager(&path, dest),
                }
                Vec::new()
            }
            ArchiveOutcome::Failed { path, error } => {
                self.state
                    .flash_error(format!("{}: {error}", display_name(&path)));
                Vec::new()
            }
            ArchiveOutcome::Materialized { real, then } => {
                self.record_staged(&real);
                match then {
                    MaterializeThen::OpenPager(dest) => self.open_staged_in_pager(&real, dest),
                }
                Vec::new()
            }
            ArchiveOutcome::MaterializeFailed { error } => {
                self.state.flash_error(error);
                Vec::new()
            }
            ArchiveOutcome::Cleaned => Vec::new(),
        }
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
            "cancel" => {
                self.cancel_archive_mount();
                self.state.flash_info("archive: cancel requested");
                Vec::new()
            }
            "unmount" => {
                let dir = self.state.cur().listing.dir.clone();
                if let Some((mount, _)) = self.state.mounts.resolve(&dir) {
                    let archive = mount.archive().to_path_buf();
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
                    "archive: unknown subcommand `{other}` (info | list | unmount | cancel)"
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

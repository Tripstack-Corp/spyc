//! Archive IO on a worker thread: mounting (indexing, or streaming a compressed
//! tar into staging) and materializing one member.
//!
//! Indexing a `.tar.gz` means decompressing it end to end, so none of this may
//! touch the event loop. Same shape as `graveyard_ops`: an [`ArchiveOp`] rides
//! `Effect::Archive` out to a detached thread, the [`ArchiveOutcome`] lands in a
//! `Runtime` slot, and `Message::ArchiveDone` wakes the loop to drain it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::archive::budget::{BudgetLimits, MountDecision, MountFacts, decide_mount};
use crate::archive::index::{ArchiveIndex, IndexEntry};
use crate::archive::journal::RepackStep;
use crate::archive::scan::{Capability, assess, warnings};
use crate::archive::write::RepackOptions;
use crate::archive::{detect_at, read};

use super::file_ops::PagerDest;

/// What to do with a member once its bytes are on disk.
#[derive(Debug)]
pub enum MaterializeThen {
    /// Open it in the pager at `dest` — `Enter` / `D` on a member.
    OpenPager(PagerDest),
    /// Re-run the effect that was held back, against the extracted copies. The
    /// op it carries never learns its inputs came out of an archive.
    Retry(Box<super::Effect>),
    /// Open the extracted copy in `$EDITOR`. Whatever the editor saves is picked
    /// up by the next write-back, which compares each staged file against what
    /// spyc wrote when it extracted it.
    Edit { in_pane: bool },
}

#[derive(Debug)]
pub enum ArchiveOp {
    /// Identify, index, and (for a compressed tar) extract an archive.
    ///
    /// `confirmed` is the second pass after the user answered a size prompt: the
    /// decision runs again rather than being carried across the round trip, so
    /// the worker stays stateless and a changed archive can't slip past it.
    Mount {
        path: PathBuf,
        staging_root: PathBuf,
        limits: BudgetLimits,
        max_entries: usize,
        confirmed: bool,
        cancel: Arc<AtomicBool>,
        /// An effect that was waiting for this archive to be mounted — a
        /// `ChangeDir` naming a path inside it, held back because the mount
        /// didn't exist yet. Re-issued once it does, so the original keeps its
        /// cursor target and its message. `None` for an ordinary `Enter`.
        then: Option<Box<super::Effect>>,
    },
    /// Extract one member so something can read it.
    Materialize {
        archive: PathBuf,
        entry: Box<IndexEntry>,
        staging_root: PathBuf,
        then: MaterializeThen,
    },
    /// Extract several members — one held-back op's whole selection.
    MaterializeMany {
        archive: PathBuf,
        /// `(mount path, member)` pairs, so the reply can say which extracted
        /// copy stands in for which row.
        entries: Vec<(PathBuf, IndexEntry)>,
        staging_root: PathBuf,
        then: MaterializeThen,
    },
    /// Write pending changes back over the archive.
    ///
    /// Carries a clone of the index because the worker needs it to resolve which
    /// stored member each carried-across step refers to. A 200k-member clone is
    /// not cheap, but a write is rare and user-initiated, and re-indexing on the
    /// worker would be wrong for a streamed mount whose members only exist in
    /// staging.
    Write {
        index: Box<ArchiveIndex>,
        steps: Vec<RepackStep>,
        staging_root: PathBuf,
        opts: RepackOptions,
    },
    /// Delete staging trees — an unmount, an eviction, or the quit sweep.
    Clean { staging_roots: Vec<PathBuf> },
}

#[derive(Debug)]
pub enum ArchiveOutcome {
    Mounted {
        /// Boxed: an index with 200k members is much larger than every other
        /// variant, and this enum is moved through a channel.
        index: Box<ArchiveIndex>,
        capability: Capability,
        warnings: Vec<String>,
        staging_root: PathBuf,
        /// The held-back effect from [`ArchiveOp::Mount`], to run now that the
        /// mount exists.
        then: Option<Box<super::Effect>>,
    },
    /// Big enough to ask about first. Answering `y` re-issues the op.
    NeedsConfirm {
        path: PathBuf,
        question: String,
        then: Option<Box<super::Effect>>,
    },
    /// The magic bytes say this isn't a container after all — the name filter
    /// admitted it, so fall back to opening it as a file.
    NotAnArchive {
        path: PathBuf,
        then: MaterializeThen,
    },
    Failed {
        path: PathBuf,
        error: String,
    },
    Materialized {
        real: PathBuf,
        then: MaterializeThen,
    },
    /// Several members landed. `staged` maps each mount path to the extracted
    /// copy standing in for it.
    MaterializedMany {
        staged: Vec<(PathBuf, PathBuf)>,
        then: MaterializeThen,
    },
    MaterializeFailed {
        error: String,
    },
    /// The archive was replaced. `archive` identifies the mount whose journal
    /// should now be considered clean.
    Written {
        archive: PathBuf,
        report: crate::archive::write::RepackReport,
    },
    WriteFailed {
        archive: PathBuf,
        error: String,
    },
    Cleaned,
}

/// Run an archive op to completion (BLOCKING decompression / disk IO). Only ever
/// called on the `Effect::Archive` worker thread.
pub fn run_archive_op(op: ArchiveOp) -> ArchiveOutcome {
    match op {
        ArchiveOp::Mount {
            path,
            staging_root,
            limits,
            max_entries,
            confirmed,
            cancel,
            then,
        } => carry_then(
            mount(
                &path,
                &staging_root,
                &limits,
                max_entries,
                confirmed,
                &cancel,
            ),
            then,
        ),
        ArchiveOp::Materialize {
            archive,
            entry,
            staging_root,
            then,
        } => match read::materialize(&archive, &entry, &staging_root) {
            Ok(real) => ArchiveOutcome::Materialized { real, then },
            Err(e) => ArchiveOutcome::MaterializeFailed {
                error: format!("{}: {e:#}", entry.inner),
            },
        },
        ArchiveOp::MaterializeMany {
            archive,
            entries,
            staging_root,
            then,
        } => {
            let mut staged = Vec::with_capacity(entries.len());
            for (mount_path, entry) in &entries {
                match read::materialize(&archive, entry, &staging_root) {
                    Ok(real) => staged.push((mount_path.clone(), real)),
                    // One unreadable member must not sink the whole selection —
                    // the retry runs with what came out, and the op reports on
                    // what it was actually given.
                    Err(e) => {
                        crate::spyc_debug!("archive: materialize {}: {e:#}", entry.inner);
                    }
                }
            }
            if staged.is_empty() {
                ArchiveOutcome::MaterializeFailed {
                    error: "archive: nothing could be extracted".to_string(),
                }
            } else {
                ArchiveOutcome::MaterializedMany { staged, then }
            }
        }
        ArchiveOp::Write {
            index,
            steps,
            staging_root,
            opts,
        } => {
            let archive = index.archive.clone();
            match crate::archive::write::repack(&index, &steps, &staging_root, &opts) {
                Ok(report) => ArchiveOutcome::Written { archive, report },
                Err(e) => ArchiveOutcome::WriteFailed {
                    archive,
                    error: format!("{e:#}"),
                },
            }
        }
        ArchiveOp::Clean { staging_roots } => {
            for root in staging_roots {
                let _ = std::fs::remove_dir_all(&root);
            }
            ArchiveOutcome::Cleaned
        }
    }
}

/// Attach the held-back effect to a mount outcome.
///
/// Done here rather than inside [`mount`] so the decision to carry something
/// lives at the op boundary and `mount` keeps its one job. Only the two outcomes
/// that lead somewhere carry it: a mount that failed, or turned out not to be an
/// archive, has nowhere for a `ChangeDir` to land — and re-issuing it would find
/// no mount, hold it back again, and mount in a circle.
fn carry_then(outcome: ArchiveOutcome, then: Option<Box<super::Effect>>) -> ArchiveOutcome {
    match outcome {
        ArchiveOutcome::Mounted {
            index,
            capability,
            warnings,
            staging_root,
            ..
        } => ArchiveOutcome::Mounted {
            index,
            capability,
            warnings,
            staging_root,
            then,
        },
        ArchiveOutcome::NeedsConfirm { path, question, .. } => ArchiveOutcome::NeedsConfirm {
            path,
            question,
            then,
        },
        other => other,
    }
}

fn mount(
    path: &Path,
    staging_root: &Path,
    limits: &BudgetLimits,
    max_entries: usize,
    confirmed: bool,
    cancel: &AtomicBool,
) -> ArchiveOutcome {
    let Some(format) = detect_at(path) else {
        return ArchiveOutcome::NotAnArchive {
            path: path.to_path_buf(),
            then: MaterializeThen::OpenPager(PagerDest::Overlay { scroll: None }),
        };
    };

    let indexed = if format.is_seekable() {
        // Nothing has been extracted, so the decision runs on exact sizes read
        // from the container's own directory.
        match read::index_seekable(path, format, max_entries) {
            Ok(indexed) => {
                if let Some(stop) = gate(path, &indexed, limits, confirmed, true) {
                    return stop;
                }
                indexed
            }
            Err(e) => return failed(path, &e),
        }
    } else {
        // A compressed tar can only be measured by decompressing it, so the
        // pre-flight runs on the archive's own size as a floor and the real
        // ceiling is enforced inside the pass.
        match preflight_streamed(path, staging_root, limits, confirmed) {
            Some(stop) => return stop,
            None => match read::stream_mount(
                path,
                format,
                staging_root,
                limits.extract_budget,
                max_entries,
                cancel,
            ) {
                Ok(indexed) => {
                    if let Some(stop) = gate(path, &indexed, limits, true, false) {
                        return stop;
                    }
                    indexed
                }
                Err(e) => return failed(path, &e),
            },
        }
    };

    let capability = assess(&indexed.facts, format);
    let warnings = warnings(&indexed.facts, &indexed.index);
    ArchiveOutcome::Mounted {
        index: Box::new(indexed.index),
        capability,
        warnings,
        staging_root: staging_root.to_path_buf(),
        // `carry_then` attaches the held-back effect at the op boundary.
        then: None,
    }
}

/// Apply the budget decision to a finished index. `ask` is false once the bytes
/// are already extracted — there is nothing left to decline.
fn gate(
    path: &Path,
    indexed: &read::Indexed,
    limits: &BudgetLimits,
    confirmed: bool,
    ask: bool,
) -> Option<ArchiveOutcome> {
    let facts = MountFacts {
        total_uncompressed: indexed.index.total_uncompressed,
        size_is_exact: true,
        compressed_size: indexed.index.compressed_size,
        entries: indexed.index.entries.len(),
        skipped: indexed.facts.skipped(),
        free_space: None,
        needs_extraction: false,
    };
    match decide_mount(&facts, limits) {
        MountDecision::Refuse(why) => Some(ArchiveOutcome::Failed {
            path: path.to_path_buf(),
            error: why,
        }),
        MountDecision::Confirm(question) if ask && !confirmed => {
            Some(ArchiveOutcome::NeedsConfirm {
                then: None,
                path: path.to_path_buf(),
                question,
            })
        }
        // Either there was nothing to ask, or the user already said yes.
        MountDecision::Proceed | MountDecision::Confirm(_) => None,
    }
}

/// The size check that runs *before* a streamed mount decompresses anything.
fn preflight_streamed(
    path: &Path,
    staging_root: &Path,
    limits: &BudgetLimits,
    confirmed: bool,
) -> Option<ArchiveOutcome> {
    let compressed = std::fs::metadata(path).map_or(0, |m| m.len());
    // Free space on the filesystem that will hold the staging tree. Its own
    // directory may not exist yet, so ask about the nearest ancestor that does.
    let free = staging_root
        .ancestors()
        .find_map(|a| a.exists().then(|| read::available_space(a)).flatten());
    let facts = MountFacts::preflight_streamed(compressed, free);
    match decide_mount(&facts, limits) {
        MountDecision::Refuse(why) => Some(ArchiveOutcome::Failed {
            path: path.to_path_buf(),
            error: why,
        }),
        MountDecision::Confirm(question) if !confirmed => Some(ArchiveOutcome::NeedsConfirm {
            then: None,
            path: path.to_path_buf(),
            question,
        }),
        // Either there was nothing to ask, or the user already said yes.
        MountDecision::Proceed | MountDecision::Confirm(_) => None,
    }
}

fn failed(path: &Path, e: &anyhow::Error) -> ArchiveOutcome {
    ArchiveOutcome::Failed {
        path: path.to_path_buf(),
        // `{e:#}` — the alternate form carries the whole source chain, so a
        // refusal names its cause rather than just the outermost context.
        error: format!("{e:#}"),
    }
}

/// Notes worth flashing when a mount opens: the capability demotion first (it
/// changes what the user can do), then the oddities.
pub fn mount_notes(capability: &Capability, warnings: &[String]) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(why) = capability.reason() {
        notes.push(format!("read-only: {why}"));
    }
    notes.extend(warnings.iter().cloned());
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::atomic::AtomicBool;

    fn zip_at(path: &Path, members: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        for (name, data) in members {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    fn mount_op(
        path: &Path,
        staging: &Path,
        limits: BudgetLimits,
        confirmed: bool,
    ) -> ArchiveOutcome {
        run_archive_op(ArchiveOp::Mount {
            path: path.to_path_buf(),
            staging_root: staging.to_path_buf(),
            limits,
            max_entries: 1000,
            confirmed,
            cancel: Arc::new(AtomicBool::new(false)),
            then: None,
        })
    }

    #[test]
    fn mounting_a_zip_indexes_it_without_creating_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        let staging = tmp.path().join("staging");
        zip_at(&archive, &[("a.txt", b"alpha"), ("d/b.txt", b"beta")]);

        let out = mount_op(&archive, &staging, BudgetLimits::default(), false);
        let ArchiveOutcome::Mounted {
            index, capability, ..
        } = out
        else {
            panic!("expected a mount, got {out:?}");
        };
        assert_eq!(index.entries.len(), 3, "two members plus the implied dir");
        assert!(capability.is_writable());
        assert!(!staging.exists(), "a seekable mount extracts nothing");
    }

    /// The name filter admits anything that *looks* like a container, so the
    /// worker has to hand back a plain file rather than fail.
    #[test]
    fn a_file_that_only_looks_like_an_archive_comes_back_as_a_plain_file() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("notes.zip");
        std::fs::write(&fake, b"this is just text\n").unwrap();

        let out = mount_op(&fake, &tmp.path().join("s"), BudgetLimits::default(), false);
        assert!(
            matches!(out, ArchiveOutcome::NotAnArchive { .. }),
            "got {out:?}"
        );
    }

    #[test]
    fn a_corrupt_archive_fails_with_its_cause() {
        let tmp = tempfile::tempdir().unwrap();
        let broken = tmp.path().join("broken.zip");
        // A valid zip magic followed by garbage: detected as a zip, unreadable
        // as one.
        std::fs::write(&broken, b"PK\x03\x04garbage-not-a-real-central-directory").unwrap();

        let out = mount_op(
            &broken,
            &tmp.path().join("s"),
            BudgetLimits::default(),
            false,
        );
        let ArchiveOutcome::Failed { error, .. } = out else {
            panic!("expected a failure, got {out:?}");
        };
        assert!(error.contains("broken.zip"), "names the archive: {error}");
        assert!(
            error.len() > "reading zip".len(),
            "carries a cause: {error}"
        );
    }

    #[test]
    fn an_empty_archive_is_refused_rather_than_mounted_blank() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty.zip");
        zip_at(&empty, &[]);

        let out = mount_op(
            &empty,
            &tmp.path().join("s"),
            BudgetLimits::default(),
            false,
        );
        let ArchiveOutcome::Failed { error, .. } = out else {
            panic!("expected a refusal, got {out:?}");
        };
        assert!(error.contains("empty"), "{error}");
    }

    #[test]
    fn an_oversized_mount_asks_before_it_extracts_and_proceeds_once_confirmed() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("big.zip");
        let staging = tmp.path().join("staging");
        zip_at(&archive, &[("a.txt", &vec![b'x'; 4096])]);
        // Thresholds low enough that this fixture reads as a bomb: 4 KB of one
        // repeated byte compresses to ~130 bytes, a ratio near 30.
        let limits = BudgetLimits {
            warn_over: 1024,
            extract_budget: 1024 * 1024,
            ratio_limit: 10,
            ..BudgetLimits::default()
        };

        // A seekable archive is only asked about when its expansion looks like a
        // bomb — nothing is extracted by mounting one, so size alone is no reason
        // to interrupt.
        let out = mount_op(&archive, &staging, limits, false);
        let ArchiveOutcome::NeedsConfirm { question, .. } = out else {
            panic!("expected a confirm, got {out:?}");
        };
        assert!(question.contains("mount anyway?"), "{question}");

        // Answering yes re-issues the same op with `confirmed`.
        let out = mount_op(&archive, &staging, limits, true);
        assert!(matches!(out, ArchiveOutcome::Mounted { .. }), "got {out:?}");
    }

    #[test]
    fn materializing_a_member_returns_its_real_path() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        let staging = tmp.path().join("staging");
        zip_at(&archive, &[("d/b.txt", b"beta")]);
        let ArchiveOutcome::Mounted { index, .. } =
            mount_op(&archive, &staging, BudgetLimits::default(), false)
        else {
            panic!("mount failed");
        };
        let entry = index.get("d/b.txt").unwrap().clone();

        let out = run_archive_op(ArchiveOp::Materialize {
            archive,
            entry: Box::new(entry),
            staging_root: staging,
            then: MaterializeThen::OpenPager(PagerDest::Overlay { scroll: None }),
        });
        let ArchiveOutcome::Materialized { real, .. } = out else {
            panic!("expected bytes, got {out:?}");
        };
        assert_eq!(std::fs::read(&real).unwrap(), b"beta");
    }

    #[test]
    fn a_failed_materialize_names_the_member() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("gone.zip");
        let mut entry = crate::archive::index::IndexEntry {
            inner: "missing.txt".to_string(),
            kind: crate::archive::index::ArchiveEntryKind::File,
            size: 1,
            mtime: None,
            mode: None,
            uid: None,
            gid: None,
            link_target: None,
            locator: crate::archive::Locator::Zip { index: 0 },
            case_rank: 0,
            readable: true,
        };
        entry.readable = true;

        let out = run_archive_op(ArchiveOp::Materialize {
            archive,
            entry: Box::new(entry),
            staging_root: tmp.path().join("staging"),
            then: MaterializeThen::OpenPager(PagerDest::Overlay { scroll: None }),
        });
        let ArchiveOutcome::MaterializeFailed { error } = out else {
            panic!("expected a failure, got {out:?}");
        };
        assert!(error.contains("missing.txt"), "{error}");
    }

    #[test]
    fn cleaning_removes_the_staging_trees() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(a.join("deep")).unwrap();
        std::fs::write(a.join("deep/f.txt"), b"x").unwrap();
        std::fs::create_dir_all(&b).unwrap();

        run_archive_op(ArchiveOp::Clean {
            staging_roots: vec![a.clone(), b.clone(), tmp.path().join("never-existed")],
        });
        assert!(!a.exists());
        assert!(!b.exists());
    }

    #[test]
    fn mount_notes_lead_with_the_capability_demotion() {
        let notes = mount_notes(
            &Capability::ReadOnly("2 duplicate member name(s)".to_string()),
            &["1 member(s) differ only by case".to_string()],
        );
        assert_eq!(notes.len(), 2);
        assert!(notes[0].starts_with("read-only:"), "{notes:?}");
    }

    #[test]
    fn an_ordinary_mount_has_nothing_to_say() {
        assert!(mount_notes(&Capability::ReadWrite, &[]).is_empty());
    }
}

//! Writing an archive back.
//!
//! This is the one place in the feature that can destroy something the user
//! can't recreate, so it is built to fail safe at every step: the new archive is
//! written to a temp file **beside** the original, verified by reading it back,
//! and only then renamed over it. A failure anywhere leaves the original
//! byte-identical, because nothing touches it until the rename.
//!
//! Untouched members are carried across without being decompressed — a zip raw
//! copy, a tar re-emit — so a repack costs I/O rather than CPU, and members spyc
//! can't even read (an encrypted zip entry) survive it verbatim.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::index::{ArchiveEntryKind, ArchiveIndex, Locator};
use super::journal::{RepackStep, StepSource};
use super::{ArchiveFormat, read};

/// Knobs for one write-back.
#[derive(Debug, Clone, Copy)]
pub struct RepackOptions {
    /// Copy the original into the graveyard first, so `:undo` can bring it back.
    /// Off for a large archive, where the copy costs as much again as the repack.
    pub snapshot_original: bool,
    /// Refuse when the filesystem holding the archive has less than this much
    /// room beyond the archive's own size.
    pub free_space_margin: u64,
}

/// What a completed write-back did.
#[derive(Debug, Clone)]
pub struct RepackReport {
    pub members: usize,
    /// Size of the archive that was written.
    pub bytes: u64,
    /// Graveyard label the original was snapshotted under, when it was.
    pub snapshot: Option<String>,
}

/// Write `steps` over the archive's [`ArchiveIndex::source`] file.
///
/// The order of operations is the safety property: precheck, write a temp,
/// verify the temp, snapshot the original, rename. The snapshot comes *after*
/// verification so a doomed repack doesn't fill the graveyard, and *before* the
/// rename so there is never a moment where neither copy is intact.
pub fn repack(
    index: &ArchiveIndex,
    steps: &[RepackStep],
    staging_root: &Path,
    opts: &RepackOptions,
) -> Result<RepackReport> {
    // The file the bytes live in — the staged copy for a nested archive, whose
    // address names a member of the archive above it.
    let archive = index.source();
    let parent = archive
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    if let Some(free) = read::available_space(parent)
        && free < index.compressed_size.saturating_add(opts.free_space_margin)
    {
        bail!(
            "{} free where the archive lives — not enough room to write it safely",
            crate::fs::ops::format_size(free)
        );
    }

    // A streamed archive's untouched members are carried across *from staging*,
    // which is a cache: something outside spyc can remove a file from it, and then
    // the archive could never be written again. The container still holds those
    // bytes, so refill what's gone before writing — never overwriting a staged copy
    // that's there, since an edited one is the change being written.
    if !index.format.is_seekable() {
        let missing = steps.iter().any(|step| match &step.source {
            StepSource::Archived { inner } => index.get(inner).is_some_and(|entry| {
                matches!(entry.locator, Locator::Staged)
                    && !staging_root.join(entry.staging_rel()).exists()
            }),
            StepSource::Staging { .. } => false,
        });
        if missing {
            read::restage_missing(
                archive,
                index,
                staging_root,
                index.entries.len().saturating_add(1024),
            )
            .context("refilling the staging tree from the archive")?;
        }
    }

    // The temp file lives in the archive's own directory so the final rename
    // stays on one filesystem, where it is atomic.
    let tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("staging a new {}", archive.display()))?;
    match index.format {
        ArchiveFormat::Zip => write_zip(index, steps, staging_root, tmp.path())?,
        _ => write_tar(index, steps, staging_root, tmp.path())?,
    }

    verify(tmp.path(), index.format, steps)
        .with_context(|| format!("verifying the new {}", archive.display()))?;

    let snapshot = if opts.snapshot_original {
        crate::state::graveyard::Graveyard::write_entry(archive)
            .map(|entry| entry.filename)
            .map_err(|e| anyhow::anyhow!("snapshotting the original failed: {e}"))
            .map(Some)?
    } else {
        None
    };

    let bytes = std::fs::metadata(tmp.path()).map_or(0, |m| m.len());
    // `persist` is the rename. Until this line the original is untouched.
    tmp.persist(archive)
        .map_err(|e| e.error)
        .with_context(|| format!("replacing {}", archive.display()))?;

    Ok(RepackReport {
        members: steps.len(),
        bytes,
        snapshot,
    })
}

fn write_zip(
    index: &ArchiveIndex,
    steps: &[RepackStep],
    staging_root: &Path,
    dest: &Path,
) -> Result<()> {
    let mut src = zip::ZipArchive::new(BufReader::new(
        File::open(index.source())
            .with_context(|| format!("opening {}", index.source().display()))?,
    ))?;
    let out_file = File::create(dest).with_context(|| format!("writing {}", dest.display()))?;
    let mut out = zip::ZipWriter::new(BufWriter::new(out_file));

    for step in steps {
        match &step.source {
            StepSource::Archived { inner } => {
                let entry = index
                    .get(inner)
                    .with_context(|| format!("{inner} vanished from the index"))?;
                let Locator::Zip { index: pos } = entry.locator else {
                    bail!("{inner} is not a zip member");
                };
                let opts = zip::write::SimpleFileOptions::default()
                    .unix_permissions(entry.mode.unwrap_or(0o644));
                if entry.kind == ArchiveEntryKind::Dir {
                    // A zip directory is a name with a trailing separator, which
                    // `add_directory` appends and a raw copy would not.
                    out.add_directory(step.out.as_str(), opts)?;
                } else {
                    // The raw copy: compressed bytes and metadata carried across
                    // without a decompress, so even an unreadable member survives.
                    let member = src.by_index_raw(pos)?;
                    out.raw_copy_file_rename(member, step.out.as_str())?;
                }
            }
            StepSource::Staging { rel } => {
                write_from_staging_zip(&mut out, &staging_root.join(rel), &step.out)?;
            }
        }
    }
    out.finish()?.flush()?;
    Ok(())
}

fn write_from_staging_zip<W: Write + Seek>(
    out: &mut zip::ZipWriter<W>,
    staged: &Path,
    name: &str,
) -> Result<()> {
    let md = std::fs::symlink_metadata(staged)
        .with_context(|| format!("reading {}", staged.display()))?;
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(mode_of(&md));
    if md.is_dir() {
        out.add_directory(name, opts)?;
        return Ok(());
    }
    if md.file_type().is_symlink() {
        let target = std::fs::read_link(staged)?;
        out.add_symlink(name, target.to_string_lossy().as_ref(), opts)?;
        return Ok(());
    }
    out.start_file(name, opts)?;
    let mut f = File::open(staged).with_context(|| format!("reading {}", staged.display()))?;
    std::io::copy(&mut f, out).with_context(|| format!("copying {}", staged.display()))?;
    Ok(())
}

fn write_tar(
    index: &ArchiveIndex,
    steps: &[RepackStep],
    staging_root: &Path,
    dest: &Path,
) -> Result<()> {
    let out_file = File::create(dest).with_context(|| format!("writing {}", dest.display()))?;
    let sink: Box<dyn Write> = match index.format {
        ArchiveFormat::Tar => Box::new(BufWriter::new(out_file)),
        ArchiveFormat::TarGz => Box::new(flate2::write::GzEncoder::new(
            BufWriter::new(out_file),
            flate2::Compression::default(),
        )),
        ArchiveFormat::TarZst => {
            Box::new(zstd::stream::write::Encoder::new(BufWriter::new(out_file), 0)?.auto_finish())
        }
        ArchiveFormat::Zip => bail!("a zip is not written as a tar"),
    };
    let mut builder = tar::Builder::new(sink);
    // Full headers: mode, uid/gid and mtime are carried across rather than
    // regenerated, which is the difference between a faithful repack and one that
    // resets every timestamp.
    builder.mode(tar::HeaderMode::Complete);

    for step in steps {
        match &step.source {
            StepSource::Archived { inner } => {
                let entry = index
                    .get(inner)
                    .with_context(|| format!("{inner} vanished from the index"))?;
                // Decided by *kind* before any read: only a file has bytes. A
                // streamed mount's members are all `Locator::Staged`, directory
                // entries included, and reading one gave `Is a directory` — so a
                // `.tar.gz` holding explicit directory entries (which is any
                // tarball of a tree) could never be written back. A symlink was
                // read for nothing, and a dangling one failed the same way.
                match entry.kind {
                    ArchiveEntryKind::Dir => {
                        let mut header = header_for(entry, 0);
                        builder.append_data(&mut header, &step.out, std::io::empty())?;
                    }
                    ArchiveEntryKind::Symlink => {
                        let target = entry.link_target.clone().unwrap_or_default();
                        let mut header = header_for(entry, 0);
                        builder.append_link(&mut header, &step.out, Path::new(&target))?;
                    }
                    ArchiveEntryKind::File => {
                        let bytes = match entry.locator {
                            // A plain tar is seekable, so untouched bytes come
                            // straight from it.
                            Locator::TarData { offset } => {
                                read_at(index.source(), offset, entry.size)?
                            }
                            // A streamed mount extracted everything, so its
                            // "archived" bytes are the staged copy.
                            Locator::Staged => {
                                std::fs::read(staging_root.join(entry.staging_rel()))
                                    .with_context(|| format!("reading staged {inner}"))?
                            }
                            _ => bail!("{inner} has no readable tar source"),
                        };
                        let mut header = header_for(entry, bytes.len() as u64);
                        builder.append_data(&mut header, &step.out, bytes.as_slice())?;
                    }
                }
            }
            StepSource::Staging { rel } => {
                let staged = staging_root.join(rel);
                let md = std::fs::symlink_metadata(&staged)
                    .with_context(|| format!("reading {}", staged.display()))?;
                if md.file_type().is_symlink() {
                    let target = std::fs::read_link(&staged)?;
                    let mut header = tar::Header::new_gnu();
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_mode(mode_of(&md));
                    header.set_size(0);
                    builder.append_link(&mut header, &step.out, &target)?;
                } else if md.is_dir() {
                    let mut header = tar::Header::new_gnu();
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_mode(mode_of(&md));
                    header.set_size(0);
                    builder.append_data(&mut header, &step.out, std::io::empty())?;
                } else {
                    let mut f = File::open(&staged)
                        .with_context(|| format!("reading {}", staged.display()))?;
                    builder.append_file(&step.out, &mut f)?;
                }
            }
        }
    }
    builder.into_inner()?.flush()?;
    Ok(())
}

/// Rebuild a tar header from what the index captured. The fields a header can
/// express are exactly the ones `scan::assess` guarantees are all an archive uses
/// before it lets a write happen.
fn header_for(entry: &super::IndexEntry, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(match entry.kind {
        ArchiveEntryKind::Dir => tar::EntryType::Directory,
        ArchiveEntryKind::Symlink => tar::EntryType::Symlink,
        ArchiveEntryKind::File => tar::EntryType::Regular,
    });
    header.set_size(size);
    header.set_mode(entry.mode.unwrap_or(0o644));
    header.set_uid(entry.uid.unwrap_or(0));
    header.set_gid(entry.gid.unwrap_or(0));
    if let Some(mtime) = entry.mtime
        && let Ok(since) = mtime.duration_since(std::time::UNIX_EPOCH)
    {
        header.set_mtime(since.as_secs());
    }
    header
}

fn read_at(archive: &Path, offset: u64, size: u64) -> Result<Vec<u8>> {
    let mut f = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    f.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    f.take(size).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(unix)]
fn mode_of(md: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    md.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn mode_of(_md: &std::fs::Metadata) -> u32 {
    0o644
}

/// Read the archive we just wrote and check it holds exactly what was planned.
///
/// This is what makes the write trustworthy rather than hopeful: a truncated
/// stream, a member that failed to compress, or a name the writer mangled all
/// show up here — while the original is still in place and the temp file is still
/// just a temp file.
fn verify(written: &Path, format: ArchiveFormat, steps: &[RepackStep]) -> Result<()> {
    let indexed = if format.is_seekable() {
        read::index_seekable(written, format, steps.len().saturating_add(1024))?
    } else {
        // A compressed tar can only be read by streaming it, and verification has
        // to read it anyway; extract to a throwaway directory that is dropped with
        // the guard.
        let scratch = tempfile::tempdir()?;
        read::stream_mount(
            written,
            format,
            scratch.path(),
            u64::MAX,
            steps.len().saturating_add(1024),
            &std::sync::atomic::AtomicBool::new(false),
        )?
    };

    let mut expected: Vec<&str> = steps.iter().map(|s| s.out.as_str()).collect();
    expected.sort_unstable();
    let mut actual: Vec<&str> = indexed
        .index
        .entries
        .iter()
        .filter(|e| e.locator != Locator::Implied)
        .map(|e| e.inner.as_str())
        .collect();
    actual.sort_unstable();

    if expected != actual {
        let missing: Vec<&&str> = expected.iter().filter(|n| !actual.contains(n)).collect();
        let extra: Vec<&&str> = actual.iter().filter(|n| !expected.contains(n)).collect();
        bail!(
            "the written archive doesn't match the plan — {} missing {missing:?}, {} unexpected {extra:?}",
            missing.len(),
            extra.len(),
        );
    }
    Ok(())
}

/// Absolute staging path for one plan step, for the callers that need to check a
/// step's source before running a repack.
pub fn staged_path(staging_root: &Path, step: &RepackStep) -> Option<PathBuf> {
    match &step.source {
        StepSource::Staging { rel } => Some(staging_root.join(rel)),
        StepSource::Archived { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::journal::{Journal, StagedStats, plan_repack};

    fn opts() -> RepackOptions {
        RepackOptions {
            snapshot_original: false,
            free_space_margin: 0,
        }
    }

    fn zip_at(path: &Path, members: &[(&str, &[u8], u32)]) {
        let file = File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        for (name, data, mode) in members {
            let o = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
            if name.ends_with('/') {
                w.add_directory(*name, o).unwrap();
            } else {
                w.start_file(*name, o).unwrap();
                w.write_all(data).unwrap();
            }
        }
        w.finish().unwrap();
    }

    fn tar_at(path: &Path, members: &[(&str, &[u8], u32)], format: ArchiveFormat) {
        let file = File::create(path).unwrap();
        let sink: Box<dyn Write> = match format {
            ArchiveFormat::Tar => Box::new(file),
            ArchiveFormat::TarGz => Box::new(flate2::write::GzEncoder::new(
                file,
                flate2::Compression::default(),
            )),
            ArchiveFormat::TarZst => Box::new(
                zstd::stream::write::Encoder::new(file, 0)
                    .unwrap()
                    .auto_finish(),
            ),
            ArchiveFormat::Zip => unreachable!(),
        };
        let mut b = tar::Builder::new(sink);
        for (name, data, mode) in members {
            let mut h = tar::Header::new_gnu();
            // A trailing `/` makes it an explicit directory entry, which is what
            // `tar czf` on a tree writes — and what nothing here used to cover.
            let dir = name.ends_with('/');
            h.set_size(if dir { 0 } else { data.len() as u64 });
            h.set_mode(*mode);
            h.set_mtime(1_700_000_000);
            h.set_entry_type(if dir {
                tar::EntryType::Directory
            } else {
                tar::EntryType::Regular
            });
            b.append_data(&mut h, name, *data).unwrap();
        }
        b.into_inner().unwrap();
    }

    /// Read an archive back as `(name, bytes)` pairs, whatever its format.
    fn contents(archive: &Path) -> Vec<(String, Vec<u8>)> {
        let format = crate::archive::detect_at(archive).expect("still an archive");
        let scratch = tempfile::tempdir().unwrap();
        let indexed = if format.is_seekable() {
            read::index_seekable(archive, format, 1000).unwrap()
        } else {
            read::stream_mount(
                archive,
                format,
                scratch.path(),
                u64::MAX,
                1000,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap()
        };
        let mut out = Vec::new();
        for entry in &indexed.index.entries {
            if entry.locator == Locator::Implied || entry.kind == ArchiveEntryKind::Dir {
                continue;
            }
            let bytes = read::materialize(archive, entry, scratch.path())
                .ok()
                .and_then(|p| std::fs::read(p).ok())
                .unwrap_or_default();
            out.push((entry.inner.clone(), bytes));
        }
        out.sort();
        out
    }

    // --- zip ---

    /// The base case that everything else rests on: a repack with no changes
    /// produces an archive holding exactly what the original did.
    #[test]
    fn repacking_an_unchanged_zip_preserves_every_member() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(
            &archive,
            &[
                ("a.txt", b"alpha", 0o644),
                ("d/b.txt", b"beta", 0o644),
                ("run.sh", b"#!/bin/sh\n", 0o755),
            ],
        );
        let before = contents(&archive);
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let steps = plan_repack(
            &indexed.index,
            &Journal::default(),
            &StagedStats::new(),
            &StagedStats::new(),
        );

        let report = repack(&indexed.index, &steps, &tmp.path().join("staging"), &opts()).unwrap();

        assert_eq!(report.members, steps.len());
        assert_eq!(contents(&archive), before);
    }

    #[cfg(unix)]
    #[test]
    fn a_repack_preserves_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(&archive, &[("run.sh", b"#!/bin/sh\n", 0o755)]);
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let steps = plan_repack(
            &indexed.index,
            &Journal::default(),
            &StagedStats::new(),
            &StagedStats::new(),
        );
        repack(&indexed.index, &steps, &tmp.path().join("staging"), &opts()).unwrap();

        let staging = tmp.path().join("check");
        let after = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let entry = after.index.get("run.sh").unwrap();
        let real = read::materialize(&archive, entry, &staging).unwrap();
        let mode = std::fs::metadata(real).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "mode {mode:o} lost the executable bit");
    }

    #[test]
    fn deleting_a_member_removes_it_and_leaves_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(
            &archive,
            &[("keep.txt", b"keep", 0o644), ("drop.txt", b"drop", 0o644)],
        );
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let mut journal = Journal::default();
        journal.delete("drop.txt");
        let steps = plan_repack(
            &indexed.index,
            &journal,
            &StagedStats::new(),
            &StagedStats::new(),
        );

        repack(&indexed.index, &steps, &tmp.path().join("staging"), &opts()).unwrap();

        assert_eq!(
            contents(&archive),
            [("keep.txt".to_string(), b"keep".to_vec())]
        );
    }

    #[test]
    fn a_renamed_member_keeps_its_bytes_under_the_new_name() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(&archive, &[("src/main.rs", b"fn main() {}", 0o644)]);
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let mut journal = Journal::default();
        journal.rename("src", "source");
        let steps = plan_repack(
            &indexed.index,
            &journal,
            &StagedStats::new(),
            &StagedStats::new(),
        );

        repack(&indexed.index, &steps, &tmp.path().join("staging"), &opts()).unwrap();

        assert_eq!(
            contents(&archive),
            [("source/main.rs".to_string(), b"fn main() {}".to_vec())]
        );
    }

    #[test]
    fn an_edited_member_is_written_back_from_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        let staging = tmp.path().join("staging");
        zip_at(&archive, &[("a.txt", b"original", 0o644)]);
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

        // Extract, then edit the staged copy the way an editor would.
        let entry = indexed.index.get("a.txt").unwrap();
        let real = read::materialize(&archive, entry, &staging).unwrap();
        std::fs::write(&real, b"edited by the user").unwrap();

        let mut journal = Journal::default();
        journal.replace("a.txt");
        let steps = plan_repack(
            &indexed.index,
            &journal,
            &StagedStats::new(),
            &StagedStats::new(),
        );
        repack(&indexed.index, &steps, &staging, &opts()).unwrap();

        assert_eq!(
            contents(&archive),
            [("a.txt".to_string(), b"edited by the user".to_vec())]
        );
    }

    #[test]
    fn an_added_file_lands_in_the_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        let staging = tmp.path().join("staging");
        zip_at(&archive, &[("a.txt", b"alpha", 0o644)]);
        std::fs::create_dir_all(staging.join("docs")).unwrap();
        std::fs::write(staging.join("docs/notes.md"), b"# notes").unwrap();

        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let mut journal = Journal::default();
        journal.add("docs/notes.md");
        let steps = plan_repack(
            &indexed.index,
            &journal,
            &StagedStats::new(),
            &StagedStats::new(),
        );

        repack(&indexed.index, &steps, &staging, &opts()).unwrap();

        assert_eq!(
            contents(&archive),
            [
                ("a.txt".to_string(), b"alpha".to_vec()),
                ("docs/notes.md".to_string(), b"# notes".to_vec()),
            ]
        );
    }

    // --- tar ---

    #[test]
    fn a_tar_round_trips_through_a_repack() {
        for format in [
            ArchiveFormat::Tar,
            ArchiveFormat::TarGz,
            ArchiveFormat::TarZst,
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let name = match format {
                ArchiveFormat::Tar => "pkg.tar",
                ArchiveFormat::TarGz => "pkg.tar.gz",
                _ => "pkg.tar.zst",
            };
            let archive = tmp.path().join(name);
            let staging = tmp.path().join("staging");
            tar_at(
                &archive,
                &[("a.txt", b"alpha", 0o644), ("d/b.txt", b"beta", 0o600)],
                format,
            );

            // A compressed tar mounts by extracting, which is also what gives its
            // members their `Staged` locators.
            let indexed = if format.is_seekable() {
                read::index_seekable(&archive, format, 1000).unwrap()
            } else {
                read::stream_mount(
                    &archive,
                    format,
                    &staging,
                    u64::MAX,
                    1000,
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap()
            };
            let mut journal = Journal::default();
            journal.delete("a.txt");
            let steps = plan_repack(
                &indexed.index,
                &journal,
                &StagedStats::new(),
                &StagedStats::new(),
            );

            repack(&indexed.index, &steps, &staging, &opts()).unwrap();

            assert_eq!(
                contents(&archive),
                [("d/b.txt".to_string(), b"beta".to_vec())],
                "{name}"
            );
        }
    }

    /// A streamed mount gives *every* member a `Staged` locator — directory
    /// entries included — and an untouched member was read as bytes to carry it
    /// across. On a directory that is `Is a directory (os error 21)`, so a
    /// `.tar.gz` of a real tree (which always has explicit directory entries)
    /// could never be written back at all.
    #[test]
    fn a_streamed_tar_with_directory_entries_writes_back() {
        for format in [ArchiveFormat::TarGz, ArchiveFormat::TarZst] {
            let tmp = tempfile::tempdir().unwrap();
            // Named for its format: `detect_at` cross-checks magic against the
            // name, so a zstd body called `.tar.gz` reads as neither.
            let archive = tmp.path().join(match format {
                ArchiveFormat::TarZst => "pkg.tar.zst",
                _ => "pkg.tar.gz",
            });
            let staging = tmp.path().join("staging");
            tar_at(
                &archive,
                &[
                    ("pkg/", b"", 0o755),
                    ("pkg/a.txt", b"alpha", 0o644),
                    ("pkg/sub/", b"", 0o755),
                    ("pkg/sub/b.txt", b"beta", 0o644),
                ],
                format,
            );
            let indexed = read::stream_mount(
                &archive,
                format,
                &staging,
                u64::MAX,
                1000,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
            let mut journal = Journal::default();
            journal.delete("pkg/a.txt");
            let steps = plan_repack(
                &indexed.index,
                &journal,
                &StagedStats::new(),
                &StagedStats::new(),
            );

            repack(&indexed.index, &steps, &staging, &opts()).expect("{format:?} must write");

            // The surviving file is intact and the directories came across as
            // directories rather than failing the write.
            let after = contents(&archive);
            assert!(
                after.contains(&("pkg/sub/b.txt".to_string(), b"beta".to_vec())),
                "{format:?}: {after:?}"
            );
            assert!(
                !after.iter().any(|(n, _)| n == "pkg/a.txt"),
                "{format:?}: the delete applied"
            );
            // Streamed, not `index_seekable`: pointed at a compressed tar that
            // reader finds no headers and hands back an *empty* index rather than
            // an error, which would make this assertion vacuous.
            let reindexed = read::stream_mount(
                &archive,
                format,
                &tmp.path().join("check"),
                u64::MAX,
                1000,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
            let dir = reindexed
                .index
                .get("pkg/sub")
                .expect("the directory is in the rewritten archive");
            assert_eq!(
                dir.kind,
                crate::archive::index::ArchiveEntryKind::Dir,
                "{format:?}: and still a directory"
            );
        }
    }

    /// Staging is a cache: something outside spyc can remove a member spyc
    /// extracted. Observed on a real archive — a daemon reaped ~100 macOS
    /// AppleDouble (`._*`) files out of `~/.local/state`, and the write then
    /// refused with `No such file or directory`, permanently.
    #[test]
    fn a_repack_refills_staging_that_lost_a_member() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.tar.gz");
        let staging = tmp.path().join("staging");
        tar_at(
            &archive,
            &[
                ("keep.txt", b"kept", 0o644),
                ("gone.txt", b"recovered", 0o644),
                ("drop.txt", b"dropped", 0o644),
            ],
            ArchiveFormat::TarGz,
        );
        let indexed = read::stream_mount(
            &archive,
            ArchiveFormat::TarGz,
            &staging,
            u64::MAX,
            1000,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();

        // Something else removes a staged member spyc isn't changing.
        std::fs::remove_file(staging.join("gone.txt")).unwrap();

        let mut journal = Journal::default();
        journal.delete("drop.txt");
        let steps = plan_repack(
            &indexed.index,
            &journal,
            &StagedStats::new(),
            &StagedStats::new(),
        );
        repack(&indexed.index, &steps, &staging, &opts()).expect("writes rather than refusing");

        let after = contents(&archive);
        assert!(
            after.contains(&("gone.txt".to_string(), b"recovered".to_vec())),
            "the lost member came back out of the archive: {after:?}"
        );
        assert!(
            after.contains(&("keep.txt".to_string(), b"kept".to_vec())),
            "{after:?}"
        );
        assert!(
            !after.iter().any(|(n, _)| n == "drop.txt"),
            "and the delete still applied"
        );
    }

    /// The refill must not undo the change being written: an edited staged copy is
    /// the pending change, so a member that IS there is left alone.
    #[test]
    fn refilling_staging_never_overwrites_an_edited_member() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.tar.gz");
        let staging = tmp.path().join("staging");
        tar_at(
            &archive,
            &[
                ("edited.txt", b"from the archive", 0o644),
                ("gone.txt", b"recovered", 0o644),
            ],
            ArchiveFormat::TarGz,
        );
        let indexed = read::stream_mount(
            &archive,
            ArchiveFormat::TarGz,
            &staging,
            u64::MAX,
            1000,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();

        // One member edited in staging, another lost from it — so the refill runs
        // in the same repack that carries the edit.
        std::fs::write(staging.join("edited.txt"), b"edited by hand").unwrap();
        std::fs::remove_file(staging.join("gone.txt")).unwrap();

        let mut journal = Journal::default();
        journal.replace("edited.txt");
        let steps = plan_repack(
            &indexed.index,
            &journal,
            &StagedStats::new(),
            &StagedStats::new(),
        );
        repack(&indexed.index, &steps, &staging, &opts()).unwrap();

        let after = contents(&archive);
        assert!(
            after.contains(&("edited.txt".to_string(), b"edited by hand".to_vec())),
            "the edit survived the refill: {after:?}"
        );
        assert!(
            after.contains(&("gone.txt".to_string(), b"recovered".to_vec())),
            "and the lost member still came back: {after:?}"
        );
    }

    #[test]
    fn a_tar_repack_keeps_modes_and_mtimes() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.tar");
        tar_at(
            &archive,
            &[("run.sh", b"#!/bin/sh\n", 0o755)],
            ArchiveFormat::Tar,
        );
        let indexed = read::index_seekable(&archive, ArchiveFormat::Tar, 1000).unwrap();
        let steps = plan_repack(
            &indexed.index,
            &Journal::default(),
            &StagedStats::new(),
            &StagedStats::new(),
        );

        repack(&indexed.index, &steps, &tmp.path().join("staging"), &opts()).unwrap();

        let after = read::index_seekable(&archive, ArchiveFormat::Tar, 1000).unwrap();
        let entry = after.index.get("run.sh").unwrap();
        assert_eq!(entry.mode, Some(0o755), "mode carried across");
        assert_eq!(
            entry.mtime,
            Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)),
            "mtime carried across rather than reset to now"
        );
    }

    // --- failing safe ---

    /// The property the whole design exists for: if anything goes wrong, the
    /// original is exactly as it was. Here a plan references a staged file that
    /// isn't there, so the write fails partway.
    #[test]
    fn a_failed_repack_leaves_the_original_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(&archive, &[("a.txt", b"alpha", 0o644)]);
        let before = std::fs::read(&archive).unwrap();

        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let mut journal = Journal::default();
        journal.add("ghost.md"); // never staged
        let steps = plan_repack(
            &indexed.index,
            &journal,
            &StagedStats::new(),
            &StagedStats::new(),
        );

        let err = repack(&indexed.index, &steps, &tmp.path().join("staging"), &opts()).unwrap_err();
        assert!(format!("{err:#}").contains("ghost.md"), "{err:#}");
        assert_eq!(
            std::fs::read(&archive).unwrap(),
            before,
            "the original is byte-identical after a failed write"
        );
    }

    /// A plan naming a member the index doesn't have fails before anything is
    /// replaced. Caught by `write_zip`'s index lookup, not by `verify` — which is
    /// what `a_write_only_verify_can_catch_leaves_the_original_alone` covers.
    #[test]
    fn a_plan_referencing_an_unknown_member_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(&archive, &[("a.txt", b"alpha", 0o644)]);
        let before = std::fs::read(&archive).unwrap();
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

        let steps = vec![RepackStep {
            out: "a.txt".to_string(),
            source: StepSource::Archived {
                inner: "not-in-the-index.txt".to_string(),
            },
        }];
        assert!(repack(&indexed.index, &steps, tmp.path(), &opts()).is_err());
        assert_eq!(std::fs::read(&archive).unwrap(), before);
    }

    /// **The verify step earns its keep.** A plan whose stored name the archive
    /// reads back differently — here `./a.txt`, which the zip stores verbatim and
    /// [`crate::archive::index::normalize`] resolves to `a.txt` — is written
    /// without complaint by every layer *except* verify. So this is the one shape
    /// only verify can refuse, and it must: the alternative is an archive whose
    /// member the user can no longer address.
    ///
    /// Asserts the refusal came from verify by name, because the neighbouring
    /// unknown-member test is caught earlier and reads as if it covered this.
    #[test]
    fn a_write_only_verify_can_catch_leaves_the_original_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(&archive, &[("a.txt", b"alpha", 0o644)]);
        let before = std::fs::read(&archive).unwrap();
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

        let steps = vec![RepackStep {
            out: "./a.txt".to_string(),
            source: StepSource::Archived {
                inner: "a.txt".to_string(),
            },
        }];
        let err = repack(&indexed.index, &steps, tmp.path(), &opts()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("doesn't match the plan"),
            "the refusal must come from verify, not an earlier guard: {msg}"
        );
        assert_eq!(
            std::fs::read(&archive).unwrap(),
            before,
            "a repack that fails verification never touches the original"
        );
    }

    /// **Verify runs before the rename, and before the snapshot.** Both of those
    /// orderings are the safety property `repack`'s docstring names, and neither
    /// is visible in a passing write: only a write that verify *rejects* can tell
    /// the difference. A doomed repack must leave the original in place AND leave
    /// the graveyard empty — filling it with snapshots of writes that never
    /// happened is how the undo affordance stops being one.
    #[test]
    fn a_rejected_write_neither_replaces_the_original_nor_snapshots_it() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let archive = tmp.path().join("pkg.zip");
            zip_at(&archive, &[("a.txt", b"alpha", 0o644)]);
            let before = std::fs::read(&archive).unwrap();
            let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

            let steps = vec![RepackStep {
                out: "./a.txt".to_string(),
                source: StepSource::Archived {
                    inner: "a.txt".to_string(),
                },
            }];
            assert!(
                repack(
                    &indexed.index,
                    &steps,
                    tmp.path(),
                    &RepackOptions {
                        snapshot_original: true,
                        free_space_margin: 0,
                    },
                )
                .is_err()
            );
            assert_eq!(
                std::fs::read(&archive).unwrap(),
                before,
                "verify has to run while the original is still the original"
            );
            assert!(
                crate::state::graveyard::Graveyard::load()
                    .entries
                    .iter()
                    .all(|e| e.filename != "pkg.zip"),
                "a write that never happened must not be snapshotted"
            );
        });
    }

    /// No temp files left behind, whether the write succeeded or failed — the
    /// archive's own directory is what the user is looking at.
    #[test]
    fn a_repack_leaves_no_temp_files_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_at(&archive, &[("a.txt", b"alpha", 0o644)]);
        let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
        let steps = plan_repack(
            &indexed.index,
            &Journal::default(),
            &StagedStats::new(),
            &StagedStats::new(),
        );
        repack(&indexed.index, &steps, &tmp.path().join("staging"), &opts()).unwrap();

        let leftovers: Vec<String> = std::fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "pkg.zip")
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
    }

    /// The snapshot is the undo affordance: the original goes to the graveyard
    /// before it is replaced, so a regretted write is recoverable.
    #[test]
    fn a_snapshot_puts_the_original_in_the_graveyard() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let archive = tmp.path().join("pkg.zip");
            zip_at(&archive, &[("a.txt", b"alpha", 0o644)]);
            let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
            let mut journal = Journal::default();
            journal.delete("a.txt");
            let steps = plan_repack(
                &indexed.index,
                &journal,
                &StagedStats::new(),
                &StagedStats::new(),
            );

            let report = repack(
                &indexed.index,
                &steps,
                &tmp.path().join("staging"),
                &RepackOptions {
                    snapshot_original: true,
                    free_space_margin: 0,
                },
            )
            .unwrap();

            assert_eq!(report.snapshot.as_deref(), Some("pkg.zip"));
            let graveyard = crate::state::graveyard::Graveyard::load();
            assert!(
                graveyard.entries.iter().any(|e| e.filename == "pkg.zip"),
                "the original is recoverable"
            );
        });
    }
}

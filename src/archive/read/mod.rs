//! The only half of `archive` that touches the filesystem: building an index,
//! streaming a compressed tar into staging, and materializing one member.
//!
//! Everything here runs on a worker thread — indexing a large tar means
//! decompressing it, which must never happen on the event loop.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};

use super::ArchiveFormat;
use super::index::{
    ArchiveEntryKind, ArchiveIndex, Draft, IndexBuilder, IndexEntry, Locator, normalize,
};
use super::scan::IndexFacts;

/// An indexed archive, plus what was odd about it.
#[derive(Debug)]
pub struct Indexed {
    pub index: ArchiveIndex,
    pub facts: IndexFacts,
}

/// Free bytes on the filesystem holding `path`, when the OS will say.
pub fn available_space(path: &Path) -> Option<u64> {
    let stat = rustix::fs::statvfs(path).ok()?;
    stat.f_bavail.checked_mul(stat.f_frsize)
}

/// Index a seekable container — a zip's central directory, or a tar's headers —
/// without extracting anything.
pub fn index_seekable(archive: &Path, format: ArchiveFormat, cap: usize) -> Result<Indexed> {
    match format {
        ArchiveFormat::Zip => index_zip(archive, cap),
        ArchiveFormat::Tar => index_tar(archive, cap),
        // A compressed tar has no index to read without decompressing it, which
        // is `stream_mount`'s job.
        _ => bail!("{} is not seekable", format.label()),
    }
}

fn index_zip(archive: &Path, cap: usize) -> Result<Indexed> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let compressed_size = file.metadata().map_or(0, |m| m.len());
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .with_context(|| format!("reading zip {}", archive.display()))?;
    let mut builder = IndexBuilder::new(cap);
    for i in 0..zip.len() {
        // `by_index_raw` reads the member's central-directory record and does
        // not decompress — the reason indexing a multi-gigabyte zip is instant.
        let Ok(member) = zip.by_index_raw(i) else {
            builder.facts.unsupported_method += 1;
            continue;
        };
        let kind = if member.is_dir() {
            ArchiveEntryKind::Dir
        } else if member.is_symlink() {
            ArchiveEntryKind::Symlink
        } else {
            ArchiveEntryKind::File
        };
        let readable = matches!(
            member.compression(),
            zip::CompressionMethod::Stored
                | zip::CompressionMethod::Deflated
                | zip::CompressionMethod::Zstd
        );
        if member.encrypted() {
            builder.facts.encrypted += 1;
        } else if !readable {
            builder.facts.unsupported_method += 1;
        }
        let draft = Draft {
            kind,
            size: member.size(),
            mtime: member.last_modified().and_then(zip_mtime),
            mode: member.unix_mode(),
            uid: None,
            gid: None,
            // A zip stores a symlink's target as the member's *content*, so it
            // costs a decompress to learn — deferred to materialize time.
            link_target: None,
            locator: Locator::Zip { index: i },
            readable: readable && !member.encrypted(),
        };
        let name = member.name().to_string();
        drop(member);
        if !builder.push(&name, draft) {
            break;
        }
    }
    let (index, facts) = builder.finish(archive.to_path_buf(), ArchiveFormat::Zip, compressed_size);
    Ok(Indexed { index, facts })
}

fn index_tar(archive: &Path, cap: usize) -> Result<Indexed> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let compressed_size = file.metadata().map_or(0, |m| m.len());
    let mut builder = IndexBuilder::new(cap);
    let mut tar = tar::Archive::new(BufReader::new(file));
    let entries = tar
        .entries()
        .with_context(|| format!("reading tar {}", archive.display()))?;
    for entry in entries {
        let Ok(entry) = entry else { break };
        let offset = entry.raw_file_position();
        let (name, draft) = tar_draft(&entry, Locator::TarData { offset }, &mut builder.facts);
        let Some(draft) = draft else { continue };
        if !builder.push(&name, draft) {
            break;
        }
    }
    let (index, facts) = builder.finish(archive.to_path_buf(), ArchiveFormat::Tar, compressed_size);
    Ok(Indexed { index, facts })
}

/// Index **and** extract a compressed tar in one pass.
///
/// A gzip or zstd stream can't be seeked, so reaching the last header means
/// decompressing everything before it — at which point the bytes are in hand and
/// throwing them away would mean paying for them again on the first read. The
/// budget is therefore enforced *during* the walk rather than up front, and the
/// caller's `cancel` flag is honored between members so a runaway archive can be
/// abandoned from the UI.
pub fn stream_mount(
    archive: &Path,
    format: ArchiveFormat,
    staging_root: &Path,
    budget: u64,
    cap: usize,
    cancel: &AtomicBool,
) -> Result<Indexed> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let compressed_size = file.metadata().map_or(0, |m| m.len());
    let reader: Box<dyn Read> = match format {
        ArchiveFormat::TarGz => Box::new(flate2::read::GzDecoder::new(BufReader::new(file))),
        ArchiveFormat::TarZst => Box::new(zstd::stream::read::Decoder::new(BufReader::new(file))?),
        _ => bail!("{} is not a streamed format", format.label()),
    };
    std::fs::create_dir_all(staging_root)
        .with_context(|| format!("creating staging dir {}", staging_root.display()))?;

    let mut builder = IndexBuilder::new(cap);
    let mut written: u64 = 0;
    let mut tar = tar::Archive::new(reader);
    let entries = tar
        .entries()
        .with_context(|| format!("reading {}", archive.display()))?;
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let mut entry = entry.with_context(|| format!("reading {}", archive.display()))?;
        let (name, draft) = tar_draft(&entry, Locator::Staged, &mut builder.facts);
        let Some(draft) = draft else { continue };
        if let Ok(clean) = normalize(&name) {
            written = written.saturating_add(draft.size);
            if written > budget {
                bail!(
                    "over the {} extract budget — raise [archive] extract_budget_mb to mount it",
                    crate::fs::ops::format_size(budget)
                );
            }
            write_member(
                &mut entry,
                &draft,
                &clean.inner,
                staging_root,
                &mut builder.facts,
            )?;
        }
        if !builder.push(&name, draft) {
            break;
        }
    }
    let (index, facts) = builder.finish(archive.to_path_buf(), format, compressed_size);
    Ok(Indexed { index, facts })
}

/// Re-extract only the members whose staged copies have gone missing.
///
/// Staging is a cache in a user-writable directory, and things outside spyc remove
/// files from it — a backup or indexing daemon reaping macOS AppleDouble (`._*`)
/// entries is one observed case. For a streamed archive those staged bytes are the
/// only copy *outside* the container, so losing one used to make the archive
/// unwritable for good. The container still has them, so refill instead.
///
/// An existing file is never overwritten: a staged copy the user has *edited* is
/// the whole point of the pending change, and refilling must not undo it. Returns
/// how many members were restored.
pub fn restage_missing(
    archive: &Path,
    format: ArchiveFormat,
    staging_root: &Path,
    cap: usize,
) -> Result<usize> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let reader: Box<dyn Read> = match format {
        ArchiveFormat::TarGz => Box::new(flate2::read::GzDecoder::new(BufReader::new(file))),
        ArchiveFormat::TarZst => Box::new(zstd::stream::read::Decoder::new(BufReader::new(file))?),
        _ => bail!("{} is not a streamed format", format.label()),
    };
    let mut facts = super::scan::IndexFacts::default();
    let mut restored = 0;
    let mut seen = 0;
    let mut tar = tar::Archive::new(reader);
    for entry in tar
        .entries()
        .with_context(|| format!("reading {}", archive.display()))?
    {
        let mut entry = entry.with_context(|| format!("reading {}", archive.display()))?;
        seen += 1;
        if seen > cap {
            break;
        }
        let (name, draft) = tar_draft(&entry, Locator::Staged, &mut facts);
        let Some(draft) = draft else { continue };
        let Ok(clean) = normalize(&name) else {
            continue;
        };
        // `staging_rel` is what a reader looks under, so a case-colliding member is
        // restored where its own reader expects it.
        if staging_root.join(&clean.inner).exists() {
            continue;
        }
        write_member(&mut entry, &draft, &clean.inner, staging_root, &mut facts)?;
        restored += 1;
    }
    Ok(restored)
}

/// Put one streamed member on disk under `staging_root`.
fn write_member<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    draft: &Draft,
    inner: &str,
    staging_root: &Path,
    facts: &mut IndexFacts,
) -> Result<()> {
    let dest = staging_root.join(inner);
    match draft.kind {
        ArchiveEntryKind::Dir => {
            std::fs::create_dir_all(&dest)
                .with_context(|| format!("creating {}", dest.display()))?;
        }
        ArchiveEntryKind::Symlink => {
            let target = draft.link_target.as_deref().unwrap_or_default();
            if link_stays_inside(parent_of(inner), target) {
                create_parent(&dest)?;
                let _ = std::fs::remove_file(&dest);
                symlink(target, &dest)?;
            } else {
                // A link out of the mount is the tar-slip shape: list it, but
                // don't create something that points at the real filesystem.
                facts.escaping_links += 1;
            }
        }
        ArchiveEntryKind::File => {
            create_parent(&dest)?;
            let mut out =
                File::create(&dest).with_context(|| format!("writing {}", dest.display()))?;
            std::io::copy(entry, &mut out)
                .with_context(|| format!("writing {}", dest.display()))?;
            out.flush()?;
            apply_mode(&dest, draft.mode);
        }
    }
    Ok(())
}

/// Extract one member's bytes into the staging tree and return where they
/// landed. Idempotent: an already-materialized member is left alone.
pub fn materialize(archive: &Path, entry: &IndexEntry, staging_root: &Path) -> Result<PathBuf> {
    let dest = staging_root.join(entry.staging_rel());
    if dest.exists() {
        return Ok(dest);
    }
    if entry.kind == ArchiveEntryKind::Dir {
        std::fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;
        return Ok(dest);
    }
    if !entry.readable {
        bail!("{}: encrypted or unsupported compression", entry.inner);
    }
    create_parent(&dest)?;
    let bytes = match entry.locator {
        Locator::Zip { index: pos } => read_zip_member(archive, pos)?,
        Locator::TarData { offset } => read_tar_member(archive, offset, entry.size)?,
        // Already on disk (a streamed mount, or the user's own file) — if it
        // isn't there, it was removed behind our back.
        Locator::Staged | Locator::Implied => {
            bail!("{}: staged bytes are missing", entry.inner)
        }
    };
    if entry.kind == ArchiveEntryKind::Symlink {
        let target = entry
            .link_target
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        if !link_stays_inside(entry.parent(), &target) {
            bail!("{}: symlink points outside the archive", entry.inner);
        }
        symlink(&target, &dest)?;
        return Ok(dest);
    }
    // Write through a temp file in the destination directory: a failed or
    // interrupted extraction must not leave a short file that later reads as
    // materialized.
    let dir = dest.parent().unwrap_or(staging_root);
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("staging {}", dest.display()))?;
    tmp.write_all(&bytes)
        .with_context(|| format!("staging {}", dest.display()))?;
    tmp.persist(&dest)
        .map_err(|e| e.error)
        .with_context(|| format!("staging {}", dest.display()))?;
    apply_mode(&dest, entry.mode);
    Ok(dest)
}

/// A member's bytes, in memory, without staging them.
///
/// [`materialize`] is the path for anything that hands a member to another
/// process — it needs a real file. A reader that only wants the content takes
/// this instead: writing into the staging tree is how the *mount* records what it
/// extracted, so a read from outside the event loop that staged its own copy
/// would make the archive read as changed.
pub fn member_bytes(archive: &Path, entry: &IndexEntry) -> Result<Vec<u8>> {
    if !entry.readable {
        bail!("{}: encrypted or unsupported compression", entry.inner);
    }
    match entry.locator {
        Locator::Zip { index: pos } => read_zip_member(archive, pos),
        Locator::TarData { offset } => read_tar_member(archive, offset, entry.size),
        // Only a streamed mount produces these, and its bytes are already on
        // disk — there is nothing in the container to re-read them from.
        Locator::Staged | Locator::Implied => {
            bail!("{}: staged bytes are missing", entry.inner)
        }
    }
}

fn read_zip_member(archive: &Path, pos: usize) -> Result<Vec<u8>> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))?;
    let mut member = zip.by_index(pos)?;
    let mut bytes = Vec::with_capacity(usize::try_from(member.size()).unwrap_or(0));
    member.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_tar_member(archive: &Path, offset: u64, size: u64) -> Result<Vec<u8>> {
    let mut file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    file.take(size).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Read one tar header into a [`Draft`]. `None` for members whose shape spyc
/// doesn't model (hardlinks, devices, fifos) — they're counted and skipped.
fn tar_draft<R: Read>(
    entry: &tar::Entry<'_, R>,
    locator: Locator,
    facts: &mut IndexFacts,
) -> (String, Option<Draft>) {
    let header = entry.header();
    let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
    let entry_type = header.entry_type();
    let kind = if entry_type.is_dir() {
        ArchiveEntryKind::Dir
    } else if entry_type.is_symlink() {
        ArchiveEntryKind::Symlink
    } else if entry_type.is_file() {
        ArchiveEntryKind::File
    } else {
        if entry_type.is_hard_link() {
            facts.hardlinks += 1;
        } else {
            facts.specials += 1;
        }
        return (name, None);
    };
    let draft = Draft {
        kind,
        size: header.size().unwrap_or(0),
        mtime: header
            .mtime()
            .ok()
            .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
        mode: header.mode().ok(),
        uid: header.uid().ok(),
        gid: header.gid().ok(),
        link_target: entry
            .link_name_bytes()
            .map(|b| String::from_utf8_lossy(&b).into_owned()),
        locator,
        readable: true,
    };
    (name, Some(draft))
}

/// Whether a symlink stored at `link_dir` pointing at `target` stays within the
/// mount.
///
/// This is the tar-slip check: a member is free to name `../../../etc/passwd` as
/// a link target, and creating that link would hand a later read or write a path
/// on the real filesystem. Absolute targets are out by definition; relative ones
/// are walked to see whether they ever climb above the mount root.
fn link_stays_inside(link_dir: &str, target: &str) -> bool {
    if target.is_empty() || target.starts_with('/') {
        return false;
    }
    let mut depth = if link_dir.is_empty() {
        0i64
    } else {
        link_dir.split('/').count() as i64
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => depth += 1,
        }
    }
    true
}

fn parent_of(inner: &str) -> &str {
    inner.rsplit_once('/').map_or("", |(p, _)| p)
}

fn create_parent(dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(target: &str, dest: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, dest)
        .with_context(|| format!("linking {}", dest.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn symlink(_target: &str, _dest: &Path) -> Result<()> {
    bail!("symlinks are not supported on this platform")
}

/// Restore an archived mode, ignoring failure — a member that lands without its
/// executable bit is a cosmetic loss, not a reason to fail the mount.
#[cfg(unix)]
fn apply_mode(dest: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    let Some(mode) = mode else { return };
    // Mask to the permission bits and force owner-write: a read-only member has
    // to stay replaceable, or a repack couldn't overwrite its staged copy.
    let perms = std::fs::Permissions::from_mode((mode & 0o777) | 0o200);
    let _ = std::fs::set_permissions(dest, perms);
}

#[cfg(not(unix))]
fn apply_mode(_dest: &Path, _mode: Option<u32>) {}

/// A zip's DOS timestamp carries no timezone; every tool reads it as local wall
/// time. We take it as UTC so the same archive lists identically everywhere,
/// which matters more here than agreeing with the machine that wrote it.
fn zip_mtime(dt: zip::DateTime) -> Option<SystemTime> {
    let date = jiff::civil::Date::new(
        i16::try_from(dt.year()).ok()?,
        i8::try_from(dt.month()).ok()?,
        i8::try_from(dt.day()).ok()?,
    )
    .ok()?;
    let civil = date.at(
        i8::try_from(dt.hour()).ok()?,
        i8::try_from(dt.minute()).ok()?,
        i8::try_from(dt.second()).ok()?,
        0,
    );
    let secs = civil
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()?
        .timestamp()
        .as_second();
    u64::try_from(secs)
        .ok()
        .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s))
}

#[cfg(test)]
mod tests;

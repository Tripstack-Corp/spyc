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
    ArchiveEntryKind, ArchiveIndex, Draft, IndexBuilder, IndexEntry, Locator, Pushed, normalize,
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
        if builder.push(&name, draft) == Pushed::Full {
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
        if builder.push(&name, draft) == Pushed::Full {
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
        // Index BEFORE extracting, and take the destination from the index. A
        // member's staging path depends on its case rank, which is the builder's
        // to assign — deriving it here instead is how the streamed writer came
        // to put bytes where no reader looks.
        let staged = match builder.push(&name, draft.clone()) {
            Pushed::Staged(rel) => rel,
            // A refused name has no entry, so nothing would ever read what we
            // wrote for it.
            Pushed::Skipped => continue,
            Pushed::Full => break,
        };
        // Budgeted on bytes that actually arrive, not on what the header
        // claims. A tar header's size field is attacker input: declaring 0
        // and then streaming gigabytes used to walk straight past this gate,
        // because the gate added up the declarations. `write_member` reports
        // what it really wrote and stops at the remaining allowance, so a
        // member can overshoot by at most one byte before this fires.
        let allowance = budget.saturating_sub(written);
        let wrote = write_member(
            &mut entry,
            &draft,
            &staged,
            staging_root,
            allowance,
            &mut builder.facts,
        )?;
        written = written.saturating_add(wrote);
        if written > budget {
            bail!(
                "over the {} extract budget — raise [archive] extract_budget_mb to mount it",
                crate::fs::ops::format_size(budget)
            );
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
    index: &super::ArchiveIndex,
    staging_root: &Path,
    cap: usize,
) -> Result<usize> {
    let format = index.format;
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
        // Asked of the index rather than derived from the name: `staging_rel` is
        // what a reader looks under, and only the entry knows its case rank. A
        // member the index doesn't hold has no reader to satisfy.
        let Some(entry_in_index) = index.get(&clean.inner) else {
            continue;
        };
        let staged = entry_in_index.staging_rel();
        if staging_root.join(&staged).exists() {
            continue;
        }
        // Refilling a member that was already admitted at mount time — the
        // budget was spent then, so there is no allowance left to re-charge it
        // against.
        write_member(
            &mut entry,
            &draft,
            &staged,
            staging_root,
            u64::MAX,
            &mut facts,
        )?;
        restored += 1;
    }
    Ok(restored)
}

/// Put one streamed member on disk under `staging_root`.
///
/// A member whose destination can't be reached without traversing a symlink is
/// counted and skipped rather than written — see [`contained_dest`]. Skipping is
/// not an error: one hostile member must not abandon the mount, and the count is
/// what makes the refusal visible in the mount's warnings.
/// Returns how many bytes actually reached disk, which is what the caller's
/// extract budget is measured in — a header's declared size is a claim, not a
/// measurement. Reads at most `allowance + 1` bytes so the caller can tell "fit"
/// from "overshot" without the overshoot itself being unbounded.
fn write_member<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    draft: &Draft,
    staging_rel: &Path,
    staging_root: &Path,
    allowance: u64,
    facts: &mut IndexFacts,
) -> Result<u64> {
    let Ok(dest) = contained_dest(staging_root, staging_rel) else {
        facts.link_traversals += 1;
        return Ok(0);
    };
    match draft.kind {
        ArchiveEntryKind::Dir => {
            std::fs::create_dir_all(&dest)
                .with_context(|| format!("creating {}", dest.display()))?;
        }
        ArchiveEntryKind::Symlink => {
            let target = draft.link_target.as_deref().unwrap_or_default();
            create_parent(&dest)?;
            if link_target_contained(staging_root, &dest, target) {
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
            // One byte past the allowance: enough for the caller to see the
            // budget was exceeded, without copying the rest of a bomb to find out.
            let wrote = std::io::copy(&mut entry.take(allowance.saturating_add(1)), &mut out)
                .with_context(|| format!("writing {}", dest.display()))?;
            out.flush()?;
            apply_mode(&dest, draft.mode);
            return Ok(wrote);
        }
    }
    Ok(0)
}

/// Extract one member's bytes into the staging tree and return where they
/// landed. Idempotent: an already-materialized member is left alone.
pub fn materialize(archive: &Path, entry: &IndexEntry, staging_root: &Path) -> Result<PathBuf> {
    // Same containment rule as the streamed path: a destination reachable only
    // through a symlink is refused. Here it *is* an error rather than a silent
    // skip — materialize is called for one member the user asked for, so the
    // refusal has somewhere to be reported.
    let dest = contained_dest(staging_root, entry.staging_rel())?;
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
        if !link_target_contained(staging_root, &dest, &target) {
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

/// How much of a declared size we are willing to reserve before seeing a byte.
///
/// A container's size field is attacker input. Handing it to `with_capacity` is
/// how a 2 KB tar asks for 64 GB — and an allocation that large *aborts* the
/// process rather than unwinding, so the panic hook never runs and the terminal
/// is left in raw mode. Reserve a modest amount and let `read_to_end` grow on
/// bytes that actually arrived: the declared size may bound a read, never an
/// allocation.
const RESERVE_CAP: u64 = 1 << 20;

fn reserve_for(declared: u64) -> usize {
    usize::try_from(declared.min(RESERVE_CAP)).unwrap_or(0)
}

fn read_zip_member(archive: &Path, pos: usize) -> Result<Vec<u8>> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))?;
    let mut member = zip.by_index(pos)?;
    let mut bytes = Vec::with_capacity(reserve_for(member.size()));
    member.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_tar_member(archive: &Path, offset: u64, size: u64) -> Result<Vec<u8>> {
    let mut file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity(reserve_for(size));
    // `take(size)` still bounds the read by the declaration, which is safe in the
    // other direction: a header claiming more than the file holds simply stops at
    // EOF, and one claiming less can't over-read.
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

/// Where member `inner` may be written under `staging_root`, or an error naming
/// why it may not be.
///
/// Containment is decided against the **filesystem**, one component at a time,
/// not by counting `..` in the name. Counting cannot work: it reasons about where
/// a member's name *says* it goes, while an earlier symlink member decides where
/// the write *lands*. Two links that each look contained on their own —
/// `d/link1 -> ..`, then `d/link1/link2 -> ..` — compose into one that points
/// above the root, and a later file member written through it leaves the mount
/// entirely. That composition is invisible to any per-name check, which is why
/// this walks instead.
///
/// The rule is therefore: **extraction never traverses a symlink.** Any existing
/// component of the destination that is a link makes the member unreachable and
/// it is refused. That is stronger than "the resolved path is inside the root",
/// and deliberately so — it is a property of each step rather than of the final
/// answer, so there is no ordering of members that can arrange an escape. It also
/// costs nothing real: an archive that needs to write *through* a symlinked
/// directory is pathological, and its members are still listed and readable from
/// the container.
///
/// The alternative — per-component `openat`/`O_NOFOLLOW` — is airtight against a
/// concurrent attacker racing the walk, but needs directory descriptors threaded
/// through every write site and a new dependency for the `*at` family. Staging
/// lives in spyc's own private directory, so the race needs local write access
/// there; refusing links closes the archive-controlled hazard with std alone.
fn contained_dest(staging_root: &Path, inner: impl AsRef<Path>) -> Result<PathBuf> {
    let inner = inner.as_ref();
    let shown = inner.display();
    // The root has to exist before it can be canonicalized, and for a seekable
    // container nothing has created it yet — the old code brought it into being
    // as a side effect of `create_parent`. Creating it here keeps that, and it is
    // the one directory in the walk we know is ours rather than the archive's.
    std::fs::create_dir_all(staging_root)
        .with_context(|| format!("creating staging dir {}", staging_root.display()))?;
    let root = std::fs::canonicalize(staging_root)
        .with_context(|| format!("resolving staging root {}", staging_root.display()))?;

    // Only plain names may participate. `normalize` already rejects `..` and a
    // leading `/` on the way in; re-checking here keeps this function correct on
    // its own terms rather than on a caller's.
    let mut names = Vec::new();
    for part in inner.components() {
        match part {
            std::path::Component::Normal(n) => names.push(n),
            std::path::Component::CurDir => {}
            _ => bail!("{shown}: unsafe path component"),
        }
    }

    // Walked in the CALLER's spelling of the root, not the canonical one, because
    // the returned path is a key: the app layer looks a staged member up by the
    // path it built from its own `staging_root`. Handing back a canonicalized
    // path silently stops matching wherever a path component is a symlink —
    // `/var` on macOS is one — so containment is decided canonically below while
    // the path itself stays in the caller's namespace.
    let mut cur = staging_root.to_path_buf();
    let mut parts = names.into_iter().peekable();
    while let Some(part) = parts.next() {
        let next = cur.join(part);
        match std::fs::symlink_metadata(&next) {
            Ok(md) if md.file_type().is_symlink() => {
                bail!("{shown}: path traverses a symlink at {}", next.display());
            }
            Ok(md) if md.is_dir() => cur = next,
            Ok(_) => {
                // A file where a directory is needed. Legal only as the last
                // component — that's the member itself being replaced.
                if parts.peek().is_some() {
                    bail!("{shown}: {} is not a directory", next.display());
                }
                cur = next;
            }
            Err(_) => {
                // Nothing exists from here down, so no remaining component can
                // be a link. Join the rest and stop walking.
                cur = next;
                cur.extend(parts);
                break;
            }
        }
    }

    // Belt and braces against the walk above ever being loosened: resolve the
    // deepest part of the result that exists and confirm it is still inside.
    // Every component was proven not to be a symlink, so this must agree — it is
    // here to fail loudly if that stops being true.
    let mut probe = cur.as_path();
    loop {
        match crate::paths::canonical_contains(&root, probe) {
            Some(true) => break,
            Some(false) => bail!("{shown}: resolves outside the staging root"),
            None => match probe.parent() {
                Some(p) => probe = p,
                None => bail!("{shown}: cannot resolve a containing directory"),
            },
        }
    }
    Ok(cur)
}

/// Whether the symlink about to be created at `dest` points somewhere inside the
/// mount.
///
/// Resolved against `dest`'s **canonical** parent — the directory the link will
/// really sit in — and then folded, so the answer is about where the link points
/// on disk rather than about how its target is spelled. The old spelling-based
/// version accepted `..` from a link whose parent had itself been relocated by an
/// earlier link.
///
/// `dest`'s parent exists by the time this is called ([`create_parent`] runs
/// first), so a `None` from [`crate::paths::canonical_contains`] means the root
/// itself is unresolvable — refuse, rather than guess.
fn link_target_contained(staging_root: &Path, dest: &Path, target: &str) -> bool {
    if target.is_empty() || Path::new(target).is_absolute() {
        return false;
    }
    let Some(parent) = dest.parent() else {
        return false;
    };
    let Ok(parent) = std::fs::canonicalize(parent) else {
        return false;
    };
    let mut resolved = parent;
    for part in Path::new(target).components() {
        match part {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    // The link's target need not exist (a dangling link is legal in an archive),
    // so ask about the deepest ancestor that does — that is the directory the
    // link would actually reach through.
    let mut probe = resolved.as_path();
    loop {
        if let Some(contained) = crate::paths::canonical_contains(staging_root, probe) {
            return contained;
        }
        match probe.parent() {
            Some(p) => probe = p,
            None => return false,
        }
    }
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

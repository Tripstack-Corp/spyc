//! Archive browsing — the format core behind mounting a zip/tarball as a
//! browsable tree (`docs/drafts/ARCHIVE_BROWSING_PLAN.md`).
//!
//! Split so every decision is testable without a live mount: `index` (the entry
//! table + name normalization), `scan` (what we may safely do with it),
//! `budget` (whether to mount at all), `journal` (pending in-mount changes),
//! `listing` (index + journal → an `fs::Listing`), and `read` — the only half
//! that touches the filesystem.

pub mod budget;
pub mod index;
pub mod journal;
pub mod listing;
pub mod mount;
pub mod read;
pub mod scan;
pub mod write;

pub use index::{ArchiveIndex, IndexEntry, Locator};
pub use journal::{Change, Journal, MemberChange, RepackStep, StepSource, plan_repack};
pub use mount::{ArchiveMount, Mounts};
pub use scan::{Capability, IndexFacts};

use std::path::Path;

/// A container spyc can mount. One variant per *container*, not per extension —
/// `.jar` / `.whl` / `.epub` / `.docx` are all [`Self::Zip`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarZst,
}

impl ArchiveFormat {
    /// Whether one entry's bytes can be reached without walking the whole
    /// container.
    ///
    /// This picks the mount strategy: seekable formats are indexed only (a zip's
    /// central directory costs nothing to read) and materialize per entry, while
    /// the rest get one streaming pass that extracts as it goes — a second pass
    /// per file would re-decompress the entire stream.
    pub const fn is_seekable(self) -> bool {
        matches!(self, Self::Zip | Self::Tar)
    }

    /// Whether entries are tar members (as opposed to zip members).
    pub const fn is_tar(self) -> bool {
        matches!(self, Self::Tar | Self::TarGz | Self::TarZst)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::TarZst => "tar.zst",
        }
    }
}

/// Suffixes that make a file worth sniffing on `Enter`.
///
/// Detection proper needs the file's magic bytes, and reading those for every
/// `Enter` on every file would put a syscall in front of the most-pressed key in
/// spyc. The name is the cheap pre-filter: it decides whether to *ask*, never
/// what the answer is.
const MOUNTABLE_SUFFIXES: &[&str] = &[
    ".zip", ".jar", ".war", ".apk", ".xpi", ".ipa", ".whl", ".epub", ".docx", ".xlsx", ".pptx",
    ".tar", ".tgz", ".tar.gz", ".tzst", ".tar.zst",
];

/// Whether a file name is worth sniffing for a container. Pure and cheap — the
/// inline check on the `Enter` path.
pub fn looks_mountable(file_name: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    MOUNTABLE_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// Head bytes [`detect`] needs: tar's `ustar` magic sits at offset 257.
pub const SNIFF_BYTES: usize = 512;

/// Identify `file_name`'s container from its magic bytes, using the name only
/// where the bytes can't decide.
///
/// Magic alone can't distinguish `foo.tar.gz` from a single gzipped `foo.gz` —
/// both are just a gzip stream — so a compressed stream is a mount candidate
/// only when the name claims a tar inside. A `.tar` with a pre-POSIX header no
/// magic matcher claims still mounts on its extension; the indexer is what
/// rejects it if it isn't really a tar.
pub fn detect(file_name: &str, head: &[u8]) -> Option<ArchiveFormat> {
    let name = file_name.to_ascii_lowercase();
    let ends = |suffixes: &[&str]| suffixes.iter().any(|s| name.ends_with(s));
    match infer::get(head).map(|t| t.mime_type()) {
        // Name-independent: the zip container is unambiguous, and every
        // zip-derived format (.jar/.whl/.epub/.docx/.apk) reads the same way.
        Some("application/zip") => Some(ArchiveFormat::Zip),
        Some("application/gzip") => ends(&[".tar.gz", ".tgz"]).then_some(ArchiveFormat::TarGz),
        Some("application/zstd") => ends(&[".tar.zst", ".tzst"]).then_some(ArchiveFormat::TarZst),
        Some("application/x-tar") => Some(ArchiveFormat::Tar),
        _ => ends(&[".tar"]).then_some(ArchiveFormat::Tar),
    }
}

/// [`detect`] against a path on disk, reading at most [`SNIFF_BYTES`]. `None`
/// for anything unreadable, so a caller can treat it as "not an archive".
pub fn detect_at(path: &Path) -> Option<ArchiveFormat> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let head = read_head(path, SNIFF_BYTES).ok()?;
    detect(&name, &head)
}

fn read_head(path: &Path, cap: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut buf = vec![0u8; cap];
    let mut f = std::fs::File::open(path)?;
    let mut filled = 0;
    // A short file is not an error — fill what we can and truncate. `read` is
    // also free to return less than asked without being at EOF.
    while filled < cap {
        match f.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZIP: &[u8] = b"PK\x03\x04\x14\x00\x00\x00\x08\x00";
    const GZIP: &[u8] = b"\x1f\x8b\x08\x00\x00\x00\x00\x00";
    const ZSTD: &[u8] = b"\x28\xb5\x2f\xfd\x00\x00\x00\x00";

    fn tar_head() -> Vec<u8> {
        // A ustar header: name at 0, magic at 257.
        let mut h = vec![0u8; 512];
        h[..8].copy_from_slice(b"file.txt");
        h[257..262].copy_from_slice(b"ustar");
        h
    }

    #[test]
    fn zip_is_detected_by_magic_whatever_it_is_called() {
        for name in ["a.zip", "lib.jar", "pkg.whl", "book.epub", "doc.docx", "x"] {
            assert_eq!(detect(name, ZIP), Some(ArchiveFormat::Zip), "{name}");
        }
    }

    #[test]
    fn tar_is_detected_by_magic() {
        assert_eq!(detect("a.tar", &tar_head()), Some(ArchiveFormat::Tar));
    }

    /// A pre-POSIX tar has no magic to match, so the extension carries it.
    #[test]
    fn legacy_tar_without_magic_falls_back_to_the_extension() {
        assert_eq!(detect("old.tar", &[0u8; 512]), Some(ArchiveFormat::Tar));
    }

    /// The distinction magic cannot make: a gzipped *tar* is a mount, a single
    /// gzipped file is not.
    #[test]
    fn a_compressed_stream_mounts_only_when_the_name_claims_a_tar() {
        assert_eq!(detect("src.tar.gz", GZIP), Some(ArchiveFormat::TarGz));
        assert_eq!(detect("src.tgz", GZIP), Some(ArchiveFormat::TarGz));
        assert_eq!(detect("notes.txt.gz", GZIP), None);
        assert_eq!(detect("dump.sql.gz", GZIP), None);

        assert_eq!(detect("src.tar.zst", ZSTD), Some(ArchiveFormat::TarZst));
        assert_eq!(detect("src.tzst", ZSTD), Some(ArchiveFormat::TarZst));
        assert_eq!(detect("notes.txt.zst", ZSTD), None);
    }

    #[test]
    fn detection_is_case_insensitive() {
        assert_eq!(detect("SRC.TAR.GZ", GZIP), Some(ArchiveFormat::TarGz));
    }

    #[test]
    fn plain_files_are_not_archives() {
        assert_eq!(detect("main.rs", b"fn main() {}"), None);
        assert_eq!(detect("empty", &[]), None);
    }

    #[test]
    fn the_name_filter_admits_containers_and_rejects_ordinary_files() {
        for name in [
            "a.zip",
            "lib.jar",
            "src.tar.gz",
            "src.tgz",
            "p.tar.zst",
            "x.TAR",
        ] {
            assert!(looks_mountable(name), "{name}");
        }
        // The filter only decides whether to sniff, so a false positive costs a
        // 512-byte read; a false negative would make the archive unmountable.
        for name in ["main.rs", "notes.txt.gz", "archive", "zip", "a.zipper"] {
            assert!(!looks_mountable(name), "{name}");
        }
    }

    #[test]
    fn only_zip_and_plain_tar_are_seekable() {
        assert!(ArchiveFormat::Zip.is_seekable());
        assert!(ArchiveFormat::Tar.is_seekable());
        assert!(!ArchiveFormat::TarGz.is_seekable());
        assert!(!ArchiveFormat::TarZst.is_seekable());
    }

    #[test]
    fn detect_at_reads_a_short_file_without_erroring() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("tiny.txt");
        std::fs::write(&p, b"hi").unwrap();
        assert_eq!(detect_at(&p), None);

        let z = tmp.path().join("a.zip");
        std::fs::write(&z, ZIP).unwrap();
        assert_eq!(detect_at(&z), Some(ArchiveFormat::Zip));
    }

    #[test]
    fn detect_at_on_a_missing_path_is_none() {
        assert_eq!(detect_at(Path::new("/nope/missing.zip")), None);
    }
}

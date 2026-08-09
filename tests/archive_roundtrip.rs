//! End-to-end pass over the archive core, from outside the crate: build a real
//! archive, index it, render the listing a mount would show, and pull one member
//! back out.
//!
//! The unit tests cover each half in isolation; this asserts they compose — an
//! index built by `read` produces the rows `listing` claims, and those rows point
//! at bytes `materialize` can actually deliver.

use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use spyc::archive::journal::{Journal, StagedStats};
use spyc::archive::{ArchiveFormat, listing, read};

fn build_zip(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    for (name, body) in [
        ("README.md", "# pkg\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("src/util/mod.rs", "pub fn helper() {}\n"),
    ] {
        w.start_file(name, opts).unwrap();
        w.write_all(body.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

fn build_tar_zst(path: &Path) {
    let enc = zstd::stream::write::Encoder::new(std::fs::File::create(path).unwrap(), 0)
        .unwrap()
        .auto_finish();
    let mut b = tar::Builder::new(enc);
    for (name, body) in [("a.txt", "alpha\n"), ("nested/b.txt", "beta\n")] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_700_000_000);
        h.set_entry_type(tar::EntryType::Regular);
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap();
}

/// The listing type itself lives in the crate's private `fs` module, so the row
/// names are read off the (public) fields rather than naming it.
macro_rules! row_names {
    ($listing:expr) => {
        $listing
            .entries
            .iter()
            .map(|e| e.display_name())
            .collect::<Vec<String>>()
    };
}

/// A zip mounts without extracting anything, lists its tree, and hands back one
/// member's bytes on request — the read path PR 2 and PR 3 are built on.
#[test]
fn a_zip_indexes_lists_and_materializes_one_member() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    let staging = tmp.path().join("staging");
    build_zip(&archive);

    assert_eq!(
        spyc::archive::detect_at(&archive),
        Some(ArchiveFormat::Zip),
        "the container is identified by its magic bytes"
    );

    let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 10_000).unwrap();
    let (journal, staged) = (Journal::default(), StagedStats::new());

    let root = listing::listing_for(&indexed.index, &journal, &staged, "");
    assert_eq!(row_names!(root), ["src/", "README.md"]);
    assert_eq!(
        root.dir, archive,
        "the mount root is the archive's own path"
    );

    let src = listing::listing_for(&indexed.index, &journal, &staged, "src");
    assert_eq!(row_names!(src), ["util/", "main.rs"]);

    // Nothing has touched the disk yet.
    assert!(!staging.exists());

    let entry = indexed.index.get("src/util/mod.rs").unwrap();
    let dest = read::materialize(&archive, entry, &staging).unwrap();
    assert_eq!(
        std::fs::read_to_string(&dest).unwrap(),
        "pub fn helper() {}\n"
    );

    // Only the requested member was extracted.
    assert!(!staging.join("src/main.rs").exists());
    assert!(!staging.join("README.md").exists());
}

/// A compressed tar can't be read piecemeal, so mounting it extracts in one
/// streaming pass — after which every member is local and the listing matches.
#[test]
fn a_compressed_tar_streams_once_and_then_lists_from_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.tar.zst");
    let staging = tmp.path().join("staging");
    build_tar_zst(&archive);

    let format = spyc::archive::detect_at(&archive).unwrap();
    assert_eq!(format, ArchiveFormat::TarZst);
    assert!(!format.is_seekable());

    let indexed = read::stream_mount(
        &archive,
        format,
        &staging,
        u64::MAX,
        10_000,
        &AtomicBool::new(false),
    )
    .unwrap();

    let (journal, staged) = (Journal::default(), StagedStats::new());
    let root = listing::listing_for(&indexed.index, &journal, &staged, "");
    assert_eq!(row_names!(root), ["nested/", "a.txt"]);

    assert_eq!(
        std::fs::read_to_string(staging.join("nested/b.txt")).unwrap(),
        "beta\n",
        "the streaming pass leaves every member on disk"
    );
    assert_eq!(indexed.index.total_uncompressed, 6 + 5);
}

/// Pending changes are reflected in what the user sees without touching the
/// archive — the property the whole deferred-write design rests on.
#[test]
fn journal_changes_shape_the_listing_without_touching_the_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    build_zip(&archive);
    let before = std::fs::read(&archive).unwrap();

    let indexed = read::index_seekable(&archive, ArchiveFormat::Zip, 10_000).unwrap();
    let mut journal = Journal::default();
    journal.delete("README.md");
    journal.rename("src", "source");
    let staged = StagedStats::new();

    let root = listing::listing_for(&indexed.index, &journal, &staged, "");
    assert_eq!(row_names!(root), ["source/"], "deleted gone, renamed moved");

    let renamed = listing::listing_for(&indexed.index, &journal, &staged, "source");
    assert_eq!(row_names!(renamed), ["util/", "main.rs"]);

    assert_eq!(
        std::fs::read(&archive).unwrap(),
        before,
        "the archive file itself is untouched until a repack"
    );
}

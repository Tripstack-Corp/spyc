//! Fixture-driven tests for the IO half.
//!
//! Archives are built in a tempdir with the `zip`/`tar` writers rather than
//! committed as binary blobs, so what one contains is readable in the test that
//! uses it.

use super::*;
use crate::archive::index::ArchiveEntryKind;

// --- fixtures ---

fn zip_at(path: &Path, members: &[(&str, &[u8], u32)]) {
    let file = File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    for (name, data, mode) in members {
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
        if name.ends_with('/') {
            w.add_directory(*name, opts).unwrap();
        } else {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
    }
    w.finish().unwrap();
}

fn tar_into<W: Write>(w: W, members: &[(&str, &[u8], u32)]) -> W {
    let mut b = tar::Builder::new(w);
    for (name, data, mode) in members {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(*mode);
        h.set_mtime(1_000_000);
        h.set_entry_type(if name.ends_with('/') {
            tar::EntryType::Directory
        } else {
            tar::EntryType::Regular
        });
        b.append_data(&mut h, name, *data).unwrap();
    }
    b.into_inner().unwrap()
}

/// Build a tar whose member names bypass tar-rs's own validation.
///
/// `Builder::append_data` refuses a `..` path outright — which is exactly
/// why the *reader* has to defend itself: archives in the wild are written
/// by tools with no such scruples. Writing the name into the raw header and
/// recomputing the checksum is how one of those archives looks.
fn hostile_tar_gz_at(path: &Path, members: &[(&str, &[u8])]) {
    let enc =
        flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::default());
    let mut b = tar::Builder::new(enc);
    for (name, data) in members {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_000_000);
        h.set_entry_type(tar::EntryType::Regular);
        let raw = h.as_mut_bytes();
        raw[..name.len()].copy_from_slice(name.as_bytes());
        h.set_cksum();
        b.append(&h, *data).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
}

fn tar_at(path: &Path, members: &[(&str, &[u8], u32)]) {
    tar_into(File::create(path).unwrap(), members);
}

fn tar_gz_at(path: &Path, members: &[(&str, &[u8], u32)]) {
    let enc =
        flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::default());
    tar_into(enc, members).finish().unwrap();
}

fn tar_zst_at(path: &Path, members: &[(&str, &[u8], u32)]) {
    let enc = zstd::stream::write::Encoder::new(File::create(path).unwrap(), 0)
        .unwrap()
        .auto_finish();
    tar_into(enc, members);
}

const SAMPLE: &[(&str, &[u8], u32)] = &[
    ("README.md", b"# hello", 0o644),
    ("src/main.rs", b"fn main() {}", 0o644),
    ("src/run.sh", b"#!/bin/sh\n", 0o755),
];

fn inners(indexed: &Indexed) -> Vec<&str> {
    indexed
        .index
        .entries
        .iter()
        .map(|e| e.inner.as_str())
        .collect()
}

// --- zip ---

#[test]
fn a_zip_indexes_its_members_and_the_directories_they_imply() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    zip_at(&archive, SAMPLE);

    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
    assert_eq!(
        inners(&indexed),
        ["README.md", "src", "src/main.rs", "src/run.sh"]
    );
    assert_eq!(indexed.index.get("README.md").unwrap().size, 7);
    assert!(indexed.index.get("src").unwrap().kind == ArchiveEntryKind::Dir);
    assert_eq!(indexed.index.total_uncompressed, 7 + 12 + 10);
    assert!(indexed.index.get("src/main.rs").unwrap().mtime.is_some());
}

/// Indexing must not read any member data — that's what makes entering a
/// large zip free. Nothing lands in staging until something asks for it.
#[test]
fn indexing_a_zip_writes_nothing_to_staging() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    let staging = tmp.path().join("staging");
    zip_at(&archive, SAMPLE);

    index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
    assert!(!staging.exists(), "no staging directory is even created");
}

#[test]
fn materializing_a_zip_member_yields_the_original_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    let staging = tmp.path().join("staging");
    zip_at(&archive, SAMPLE);
    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

    let entry = indexed.index.get("src/main.rs").unwrap();
    let dest = materialize(&archive, entry, &staging).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"fn main() {}");
    assert_eq!(dest, staging.join("src/main.rs"));

    // Only what was asked for — its sibling is still unextracted.
    assert!(!staging.join("src/run.sh").exists());
}

#[test]
fn materializing_twice_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    let staging = tmp.path().join("staging");
    zip_at(&archive, SAMPLE);
    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
    let entry = indexed.index.get("README.md").unwrap();

    let first = materialize(&archive, entry, &staging).unwrap();
    std::fs::write(&first, b"edited by the user").unwrap();
    let second = materialize(&archive, entry, &staging).unwrap();
    assert_eq!(
        std::fs::read(&second).unwrap(),
        b"edited by the user",
        "a second materialize must not clobber a staged edit"
    );
}

#[cfg(unix)]
#[test]
fn a_materialized_member_keeps_its_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    let staging = tmp.path().join("staging");
    zip_at(&archive, SAMPLE);
    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

    let entry = indexed.index.get("src/run.sh").unwrap();
    let dest = materialize(&archive, entry, &staging).unwrap();
    let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
    assert!(mode & 0o111 != 0, "mode {mode:o} lost the executable bit");
}

// --- plain tar ---

#[test]
fn a_plain_tar_indexes_by_offset_and_materializes_by_seeking() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.tar");
    let staging = tmp.path().join("staging");
    tar_at(&archive, SAMPLE);

    let indexed = index_seekable(&archive, ArchiveFormat::Tar, 1000).unwrap();
    assert_eq!(
        inners(&indexed),
        ["README.md", "src", "src/main.rs", "src/run.sh"]
    );
    let entry = indexed.index.get("src/main.rs").unwrap();
    assert!(matches!(entry.locator, Locator::TarData { .. }));

    let dest = materialize(&archive, entry, &staging).unwrap();
    assert_eq!(std::fs::read(dest).unwrap(), b"fn main() {}");
}

#[test]
fn tar_headers_carry_mode_and_mtime_into_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.tar");
    tar_at(&archive, SAMPLE);
    let indexed = index_seekable(&archive, ArchiveFormat::Tar, 1000).unwrap();
    let run = indexed.index.get("src/run.sh").unwrap();
    assert_eq!(run.mode, Some(0o755));
    assert_eq!(
        run.mtime,
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000))
    );
}

// --- streamed (compressed) tar ---

#[test]
fn a_compressed_tar_is_indexed_and_extracted_in_one_pass() {
    for (name, build) in [
        ("pkg.tar.gz", tar_gz_at as fn(&Path, &[(&str, &[u8], u32)])),
        ("pkg.tar.zst", tar_zst_at),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join(name);
        let staging = tmp.path().join("staging");
        build(&archive, SAMPLE);
        let format = crate::archive::detect_at(&archive).unwrap();

        let indexed = stream_mount(
            &archive,
            format,
            &staging,
            u64::MAX,
            1000,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(
            inners(&indexed),
            ["README.md", "src", "src/main.rs", "src/run.sh"],
            "{name}"
        );
        // The pass leaves every member on disk, so nothing re-decompresses.
        assert_eq!(
            std::fs::read(staging.join("src/main.rs")).unwrap(),
            b"fn main() {}",
            "{name}"
        );
        assert_eq!(
            indexed.index.get("README.md").unwrap().locator,
            Locator::Staged,
            "{name}"
        );
    }
}

#[test]
fn a_streamed_mount_stops_at_the_extract_budget_and_names_the_knob() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("big.tar.gz");
    let staging = tmp.path().join("staging");
    let big = vec![b'x'; 4096];
    tar_gz_at(&archive, &[("a.bin", &big, 0o644), ("b.bin", &big, 0o644)]);

    let err = stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        2048,
        1000,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("extract_budget_mb"), "{msg}");
}

#[test]
fn a_streamed_mount_stops_when_cancelled() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.tar.gz");
    let staging = tmp.path().join("staging");
    tar_gz_at(&archive, SAMPLE);

    let err = stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        u64::MAX,
        1000,
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("cancelled"));
}

#[test]
fn a_streamed_mount_refuses_a_format_it_cannot_stream() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    zip_at(&archive, SAMPLE);
    assert!(
        stream_mount(
            &archive,
            ArchiveFormat::Zip,
            &tmp.path().join("s"),
            u64::MAX,
            10,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(index_seekable(&archive, ArchiveFormat::TarGz, 10).is_err());
}

// --- safety ---

/// The attack this whole layer exists to stop: a member named `../escape`
/// must never be written outside the staging root.
#[test]
fn a_traversal_member_is_skipped_not_written_outside_staging() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("evil.tar.gz");
    let staging = tmp.path().join("staging");
    hostile_tar_gz_at(
        &archive,
        &[("../escaped.txt", b"pwned"), ("safe.txt", b"fine")],
    );

    let indexed = stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        u64::MAX,
        1000,
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(
        inners(&indexed),
        ["safe.txt"],
        "the escaping member is not indexed"
    );
    assert_eq!(indexed.facts.traversal_names, 1);
    assert!(
        !tmp.path().join("escaped.txt").exists(),
        "nothing may be written above the staging root"
    );
    assert!(staging.join("safe.txt").exists());
}

#[cfg(unix)]
#[test]
fn a_symlink_escaping_the_mount_is_listed_but_never_created() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("links.tar.gz");
    let staging = tmp.path().join("staging");
    {
        let enc = flate2::write::GzEncoder::new(
            File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        let mut b = tar::Builder::new(enc);
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        h.set_mode(0o777);
        b.append_link(&mut h, "escape", "../../../etc/passwd")
            .unwrap();
        let mut h2 = tar::Header::new_gnu();
        h2.set_entry_type(tar::EntryType::Symlink);
        h2.set_size(0);
        h2.set_mode(0o777);
        b.append_link(&mut h2, "inside", "safe.txt").unwrap();
        b.into_inner().unwrap().finish().unwrap();
    }

    let indexed = stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        u64::MAX,
        1000,
        &AtomicBool::new(false),
    )
    .unwrap();

    assert!(inners(&indexed).contains(&"escape"), "it still lists");
    assert!(
        !staging.join("escape").exists(),
        "but no link to the real filesystem is created"
    );
    assert_eq!(indexed.facts.escaping_links, 1);
    assert!(
        staging.join("inside").is_symlink(),
        "an inside link is fine"
    );
}

#[test]
fn link_targets_are_judged_by_where_they_land() {
    // Inside the mount, at any depth.
    assert!(link_stays_inside("", "sibling.txt"));
    assert!(link_stays_inside("a/b", "../c.txt"));
    assert!(link_stays_inside("a/b", "../../top.txt"));
    assert!(link_stays_inside("a", "./b/c"));
    // Out of it.
    assert!(!link_stays_inside("", "../escape"));
    assert!(!link_stays_inside("a", "../../escape"));
    assert!(!link_stays_inside("a/b", "../../../escape"));
    assert!(!link_stays_inside("a", "/etc/passwd"));
    assert!(!link_stays_inside("a", ""));
}

#[test]
fn the_entry_cap_stops_a_walk_early() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("many.zip");
    let members: Vec<(String, Vec<u8>)> = (0..10)
        .map(|i| (format!("f{i:02}.txt"), b"x".to_vec()))
        .collect();
    let refs: Vec<(&str, &[u8], u32)> = members
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice(), 0o644))
        .collect();
    zip_at(&archive, &refs);

    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 4).unwrap();
    assert!(indexed.index.truncated);
    assert_eq!(indexed.index.entries.len(), 4);
}

#[test]
fn available_space_reports_something_for_a_real_directory() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(available_space(tmp.path()).is_some_and(|free| free > 0));
    assert_eq!(available_space(Path::new("/nope/missing")), None);
}

/// A zip stores its timestamp in the DOS format, whose seconds field counts
/// *two-second* units — so an odd second can't survive the round trip, and a
/// listing showing one second less than the writer intended is the format,
/// not a conversion bug.
#[test]
fn a_dos_timestamp_converts_to_a_real_instant() {
    let dt = zip::DateTime::from_date_and_time(2026, 8, 8, 12, 30, 14).unwrap();
    let secs = zip_mtime(dt)
        .expect("a valid DOS date converts")
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(secs, 1_786_192_214, "2026-08-08T12:30:14Z");

    let odd = zip::DateTime::from_date_and_time(2026, 8, 8, 12, 30, 15).unwrap();
    assert_eq!(
        zip_mtime(odd),
        zip_mtime(dt),
        "the DOS format has two-second resolution"
    );
}

#[test]
fn an_impossible_date_does_not_panic() {
    // A corrupt DOS field can name day 0 or month 0; that must read as
    // "no timestamp", not a crash on the indexing worker.
    assert!(zip::DateTime::from_date_and_time(2026, 0, 0, 0, 0, 0).is_err());
}

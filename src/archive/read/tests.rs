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

/// spyc must be able to read what it just extracted.
///
/// A `0o000` member is legal and not rare — build systems produce them. Staging
/// it at the archive's mode makes the pager, `y` and copy-out all fail EACCES on
/// a member the container holds perfectly well.
#[cfg(unix)]
#[test]
fn a_member_with_no_permissions_still_reads_back() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    let staging = tmp.path().join("staging");
    zip_at(&archive, &[("locked.txt", b"secret", 0o000)]);
    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

    let entry = indexed.index.get("locked.txt").unwrap();
    assert_eq!(
        entry.mode.map(|m| m & 0o777),
        Some(0o000),
        "the index keeps the archive's mode; without that this proves nothing"
    );

    let dest = materialize(&archive, entry, &staging).unwrap();
    let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"secret",
        "staged at mode {mode:o}, which spyc cannot read"
    );
}

/// The staging tree is spyc's own cache, and `c` copy-out is `std::fs::copy`,
/// which carries the source's permission bits. Staging a `0o777` member verbatim
/// therefore lands a world-writable file in the user's tree — an archive
/// deciding the permissions of a file outside it.
#[cfg(unix)]
#[test]
fn a_world_writable_member_does_not_stage_world_writable() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("pkg.zip");
    let staging = tmp.path().join("staging");
    zip_at(&archive, &[("wide.sh", b"#!/bin/sh\n", 0o777)]);
    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

    let entry = indexed.index.get("wide.sh").unwrap();
    let dest = materialize(&archive, entry, &staging).unwrap();
    let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode & 0o077,
        0,
        "staged {mode:o}: group/other bits survived"
    );
    assert!(
        mode & 0o100 != 0,
        "staged {mode:o}: lost the executable bit"
    );
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

/// The same cases the old spelling-based check covered, now asked of the real
/// filesystem — where the link would sit, not how its target reads.
#[cfg(unix)]
#[test]
fn link_targets_are_judged_by_where_they_land() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(staging.join("a/b")).unwrap();

    let at = |rel: &str| staging.join(rel);
    // Inside the mount, at any depth.
    assert!(link_target_contained(&staging, &at("l"), "sibling.txt"));
    assert!(link_target_contained(&staging, &at("a/b/l"), "../c.txt"));
    assert!(link_target_contained(
        &staging,
        &at("a/b/l"),
        "../../top.txt"
    ));
    assert!(link_target_contained(&staging, &at("a/l"), "./b/c"));
    // Out of it.
    assert!(!link_target_contained(&staging, &at("l"), "../escape"));
    assert!(!link_target_contained(&staging, &at("a/l"), "../../escape"));
    assert!(!link_target_contained(
        &staging,
        &at("a/b/l"),
        "../../../escape"
    ));
    assert!(!link_target_contained(&staging, &at("a/l"), "/etc/passwd"));
    assert!(!link_target_contained(&staging, &at("a/l"), ""));
}

/// What the depth counter could not express: an identical target is contained or
/// not depending on where its link physically sits, and a planted link moves it.
#[cfg(unix)]
#[test]
fn the_same_target_is_judged_differently_once_a_link_relocates_its_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(staging.join("d")).unwrap();

    // `..` from staging/d/ is staging — contained.
    assert!(link_target_contained(&staging, &staging.join("d/l"), ".."));
    // The identical target from a link that landed at the staging root is not.
    assert!(!link_target_contained(&staging, &staging.join("l"), ".."));
}

/// `contained_dest` is the rule the whole fix rests on: any existing component
/// that is a symlink makes the member unreachable.
#[cfg(unix)]
#[test]
fn a_destination_behind_a_symlink_component_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(staging.join("real/deep")).unwrap();
    std::os::unix::fs::symlink("..", staging.join("real/up")).unwrap();

    // A plain path through real directories resolves, and stays put.
    let ok = contained_dest(&staging, "real/deep/file.txt").unwrap();
    assert_eq!(ok, staging.join("real/deep/file.txt"));
    // Nonexistent intermediate dirs are fine — they cannot be links.
    assert!(contained_dest(&staging, "brand/new/path.txt").is_ok());
    // Through the link, refused, even though the link points back inside.
    let err = contained_dest(&staging, "real/up/file.txt").unwrap_err();
    assert!(format!("{err:#}").contains("symlink"), "got: {err:#}");
    // And a `..` component is refused on its own terms.
    assert!(contained_dest(&staging, "../out.txt").is_err());
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

/// **A member with a corrupt DOS timestamp still lists, with no timestamp.**
///
/// The requirement is about the archive, not about one function: month 0 / day 0
/// is not a date, so the row must show no mtime rather than a crash or an
/// invented one.
///
/// The old version of this test asserted
/// `zip::DateTime::from_date_and_time(2026, 0, 0, …).is_err()` — the `zip`
/// crate's own constructor, never touching spyc. Driven end to end instead,
/// which is the only way the shape occurs for real: every checked constructor
/// refuses an impossible date, so one can arrive only by being *parsed* out of
/// an archive.
///
/// Worth knowing where the guard actually is: `zip` validates on parse
/// (`try_from_msdos(..).ok()`), so `last_modified()` answers `None` and
/// [`zip_mtime`] is never reached with an invalid date at all. Its own
/// `.ok()?` chain is belt-and-braces against that changing, and this test does
/// not — cannot — exercise it; reaching it needs `DateTime::from_msdos_unchecked`,
/// which is `unsafe`. What this pins is the user-visible half.
#[test]
fn an_impossible_date_reads_as_no_timestamp() {
    let tmp = tempfile::tempdir().unwrap();
    let intact = tmp.path().join("intact.zip");
    zip_at(&intact, &[("a.txt", b"hi", 0o644)]);
    // The premise: an untouched archive of the same shape DOES carry a
    // timestamp. Without this the assertion below could pass on a build that
    // never reads mtimes at all.
    assert!(
        index_seekable(&intact, ArchiveFormat::Zip, 1000)
            .unwrap()
            .index
            .get("a.txt")
            .and_then(|e| e.mtime)
            .is_some(),
        "the fixture must have a timestamp to lose"
    );

    let archive = tmp.path().join("corrupt-date.zip");
    zip_at(&archive, &[("a.txt", b"hi", 0o644)]);
    zero_dos_timestamps(&archive);

    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
    let entry = indexed.index.get("a.txt").expect("the member still lists");
    assert_eq!(
        entry.mtime, None,
        "month 0 / day 0 is not a date — drop the timestamp, don't invent one"
    );
}

/// Zero the DOS mod-time/mod-date fields in every local header and central
/// directory record, giving month 0 and day 0 — what a truncated or corrupt
/// archive carries, and what no `zip` constructor will build.
fn zero_dos_timestamps(path: &Path) {
    let mut bytes = std::fs::read(path).expect("read the zip");
    // Local file header: PK, then version/flags/method (6 bytes),
    // then modtime+moddate. Central directory: PK, with two extra
    // version fields ahead of the same pair.
    for i in 0..bytes.len().saturating_sub(4) {
        let off = match &bytes[i..i + 4] {
            [0x50, 0x4b, 0x03, 0x04] => Some(i + 10),
            [0x50, 0x4b, 0x01, 0x02] => Some(i + 12),
            _ => None,
        };
        if let Some(o) = off
            && o + 4 <= bytes.len()
        {
            bytes[o..o + 4].fill(0);
        }
    }
    std::fs::write(path, bytes).expect("write the patched zip");
}

// --- containment: a link may not decide where a later member lands ---

/// Build a tar.gz from a script of members, using the raw writers so symlinks
/// and ordering are under the test's control.
///
/// Ordering is the whole subject here — the hazard is member N+1 landing
/// somewhere member N arranged — so these are appended exactly as given.
#[cfg(unix)]
fn linked_tar_gz_at(path: &Path, members: &[Member<'_>]) {
    let enc =
        flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::default());
    let mut b = tar::Builder::new(enc);
    for m in members {
        let mut h = tar::Header::new_gnu();
        h.set_mode(0o755);
        h.set_mtime(1_000_000);
        match m {
            Member::Dir(name) => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_size(0);
                b.append_data(&mut h, name, &[][..]).unwrap();
            }
            Member::Link { name, target } => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                b.append_link(&mut h, name, target).unwrap();
            }
            Member::File { name, data } => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_size(data.len() as u64);
                b.append_data(&mut h, name, *data).unwrap();
            }
        }
    }
    b.into_inner().unwrap().finish().unwrap();
}

#[cfg(unix)]
enum Member<'a> {
    Dir(&'a str),
    Link { name: &'a str, target: &'a str },
    File { name: &'a str, data: &'a [u8] },
}

#[cfg(unix)]
fn mount_linked(archive: &Path, staging: &Path) -> Indexed {
    stream_mount(
        archive,
        ArchiveFormat::TarGz,
        staging,
        u64::MAX,
        1000,
        &AtomicBool::new(false),
    )
    .unwrap()
}

/// The 2.1 blocker, as reported: two links that each pass a per-name depth check
/// compose into one pointing above the root, and a third member is written
/// through it.
///
/// `d/link1 -> ..` sits at depth 1 and climbs to 0 — contained. `d/link1/link2
/// -> ..` reads as depth 2 climbing to 1 — also contained — but it is *created*
/// at `staging/link2`, because link1 already redirected its parent, so its `..`
/// leaves the mount. Against the lexical check this test writes ESCAPED.txt
/// outside the staging root.
#[cfg(unix)]
#[test]
fn a_symlink_chain_cannot_walk_a_later_member_out_of_the_mount() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("chain.tar.gz");
    let staging = tmp.path().join("deep/staging");
    linked_tar_gz_at(
        &archive,
        &[
            Member::Dir("d/"),
            Member::Link {
                name: "d/link1",
                target: "..",
            },
            Member::Link {
                name: "d/link1/link2",
                target: "..",
            },
            Member::File {
                name: "d/link1/link2/ESCAPED.txt",
                data: b"pwned",
            },
        ],
    );

    let indexed = mount_linked(&archive, &staging);

    // The escape target, one level above staging, and every other level up.
    assert!(
        !tmp.path().join("deep/ESCAPED.txt").exists(),
        "a member escaped one level above the staging root"
    );
    assert!(
        !tmp.path().join("ESCAPED.txt").exists(),
        "a member escaped to the sandbox root"
    );
    assert!(
        !staging.join("ESCAPED.txt").exists(),
        "the second link must not have been created at staging root either"
    );
    // And the mount says so rather than reporting a clean archive.
    assert!(
        indexed.facts.link_traversals > 0,
        "a refused member must be counted, got facts {:?}",
        indexed.facts
    );
    assert!(
        !crate::archive::scan::assess(&indexed.facts, ArchiveFormat::TarGz).is_writable(),
        "an archive we declined to fully extract must not offer write-back"
    );
    assert!(
        crate::archive::scan::warnings(&indexed.facts, &indexed.index)
            .iter()
            .any(|w| w.contains("symlink")),
        "the refusal must surface in the mount warnings"
    );
}

/// A `..` sitting *behind a link component* is not the same as no `..` at all.
///
/// `d/link1 -> ..` is contained and correctly created: from `staging/d` it lands
/// on the staging root. But a second link spelled `d/link1/../ESCAPED` cancels to
/// `staging/d/ESCAPED` on paper, while `open()` follows `link1` first and the
/// `..` then climbs out of the staging root. Against the folding version this
/// reads `secret` back through a link that lives inside the mount — and the fuzz
/// target could not catch it either, because `assert_contained` folded the same
/// way and so agreed with the bug it exists to find.
///
/// Both member orderings are pinned below. They fail for *different* reasons,
/// which is the point: with `link1` first the composed path resolves and lands
/// outside; with `link1` last the target is dangling at creation time, so the
/// `..` behind the not-yet-existing component is what has to be refused. A fix
/// that only resolves the filesystem passes the first and ships the second.
#[cfg(unix)]
#[test]
fn a_target_that_climbs_through_a_link_cannot_leave_the_mount() {
    for (order, members) in [
        (
            "link first",
            vec![
                Member::Dir("d"),
                Member::Link {
                    name: "d/link1",
                    target: "..",
                },
                Member::Link {
                    name: "L",
                    target: "d/link1/../ESCAPED",
                },
            ],
        ),
        (
            "link last",
            vec![
                Member::Dir("d"),
                Member::Link {
                    name: "L",
                    target: "d/link1/../ESCAPED",
                },
                Member::Link {
                    name: "d/link1",
                    target: "..",
                },
            ],
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("climb.tar.gz");
        let staging = tmp.path().join("deep/staging");
        // The prize: outside the mount, one level above the staging root.
        std::fs::create_dir_all(tmp.path().join("deep")).unwrap();
        std::fs::write(tmp.path().join("deep/ESCAPED"), b"secret").unwrap();

        linked_tar_gz_at(&archive, &members);
        let _ = mount_linked(&archive, &staging);

        let planted = staging.join("L");
        assert!(
            std::fs::read_to_string(&planted).is_err(),
            "{order}: a file outside the mount is readable through the staging tree"
        );
        assert!(
            std::fs::symlink_metadata(&planted).is_err(),
            "{order}: a link resolving outside the mount must never be created"
        );
        // The contained half is untouched — landing ON the staging root is legal,
        // so this is a ceiling on composition, not a ban on `..`.
        assert!(
            std::fs::symlink_metadata(staging.join("d/link1")).is_ok(),
            "{order}: a link that lands on the staging root is still created"
        );
    }
}

/// A dangling target that only ever descends stays legal.
///
/// The ordering rule above refuses a `..` behind a component that doesn't exist
/// yet. It must not also refuse the ordinary case it sits next to: a relative
/// link whose `..` applies to a directory that *does* exist, pointing at a member
/// the archive simply hasn't extracted yet. Tarballs are full of these
/// (`bin/tool -> ../lib/tool`), and refusing them would trade a security fix for
/// a broken mount.
#[cfg(unix)]
#[test]
fn a_link_pointing_at_a_not_yet_extracted_sibling_is_still_created() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("order.tar.gz");
    let staging = tmp.path().join("deep/staging");
    linked_tar_gz_at(
        &archive,
        &[
            Member::Dir("bin"),
            // `..` resolves against `staging/bin`, which exists; `lib/tool` does
            // not exist yet and only descends from the staging root.
            Member::Link {
                name: "bin/tool",
                target: "../lib/tool",
            },
            Member::Dir("lib"),
            Member::File {
                name: "lib/tool",
                data: b"#!/bin/sh\n",
            },
        ],
    );
    let _ = mount_linked(&archive, &staging);

    assert!(
        std::fs::symlink_metadata(staging.join("bin/tool")).is_ok(),
        "a forward reference inside the mount must still be linked"
    );
    assert_eq!(
        std::fs::read_to_string(staging.join("bin/tool")).unwrap(),
        "#!/bin/sh\n",
        "and it resolves once its target arrives"
    );
}

/// The same shape one hop longer, so a fix that only special-cases two links
/// still fails.
#[cfg(unix)]
#[test]
fn a_deep_symlink_chain_is_refused_at_every_hop() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("deepchain.tar.gz");
    let staging = tmp.path().join("a/b/c/staging");
    linked_tar_gz_at(
        &archive,
        &[
            Member::Dir("x/"),
            Member::Dir("x/y/"),
            Member::Link {
                name: "x/y/l1",
                target: "..",
            },
            Member::Link {
                name: "x/y/l1/l2",
                target: "..",
            },
            Member::Link {
                name: "x/y/l1/l2/l3",
                target: "..",
            },
            Member::File {
                name: "x/y/l1/l2/l3/OUT.txt",
                data: b"pwned",
            },
        ],
    );

    let indexed = mount_linked(&archive, &staging);

    for dir in ["a/b/c", "a/b", "a", ""] {
        let candidate = tmp.path().join(dir).join("OUT.txt");
        assert!(!candidate.exists(), "escaped to {}", candidate.display());
    }
    assert!(indexed.facts.link_traversals > 0);
}

/// A single link out, followed by a member written through it — the classic
/// two-step, with the link itself already caught by the old check. Kept so the
/// *pairing* is asserted, not just the link's rejection.
#[cfg(unix)]
#[test]
fn a_member_written_through_an_escaping_link_lands_nowhere() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("twostep.tar.gz");
    let staging = tmp.path().join("staging");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    linked_tar_gz_at(
        &archive,
        &[
            Member::Link {
                name: "hop",
                target: "../outside",
            },
            Member::File {
                name: "hop/THROUGH.txt",
                data: b"pwned",
            },
        ],
    );

    mount_linked(&archive, &staging);

    assert!(
        !outside.join("THROUGH.txt").exists(),
        "a member was written through a refused link"
    );
    // `hop` does end up existing — as a real directory, created for the member
    // that named it as a parent. That is the containment working: the member
    // landed inside the mount instead of following the link out of it.
    let hop = std::fs::symlink_metadata(staging.join("hop")).unwrap();
    assert!(
        !hop.file_type().is_symlink(),
        "the link must not be created"
    );
    assert!(
        staging.join("hop/THROUGH.txt").is_file(),
        "and stays inside"
    );
}

/// An absolute link target is out by definition — it needs no `..` at all.
#[cfg(unix)]
#[test]
fn an_absolute_link_target_is_never_created() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("abs.tar.gz");
    let staging = tmp.path().join("staging");
    linked_tar_gz_at(
        &archive,
        &[
            Member::Link {
                name: "abs",
                target: "/etc/passwd",
            },
            Member::File {
                name: "abs/nope.txt",
                data: b"x",
            },
        ],
    );

    let indexed = mount_linked(&archive, &staging);

    assert!(
        !std::fs::symlink_metadata(staging.join("abs"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "no link to an absolute path may be created"
    );
    assert!(!Path::new("/etc/passwd/nope.txt").exists());
    assert_eq!(indexed.facts.escaping_links, 1);
}

/// A target that climbs out and comes back to a *different* tree nets an escape
/// even though its component count balances — the arithmetic a depth counter
/// does is not the question.
#[cfg(unix)]
#[test]
fn a_mixed_target_that_nets_an_escape_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("mixed.tar.gz");
    let staging = tmp.path().join("staging");
    let sibling = tmp.path().join("sibling");
    std::fs::create_dir_all(&sibling).unwrap();
    linked_tar_gz_at(
        &archive,
        &[
            Member::Dir("keep/"),
            // From staging/keep/: up two (out of staging), down into sibling.
            Member::Link {
                name: "keep/sneak",
                target: "../../sibling",
            },
            Member::File {
                name: "keep/sneak/LANDED.txt",
                data: b"pwned",
            },
        ],
    );

    mount_linked(&archive, &staging);

    assert!(
        !sibling.join("LANDED.txt").exists(),
        "a balanced-looking target still escaped"
    );
}

/// The other half of the contract: a link that genuinely stays inside is still
/// created, and still resolves. Containment must not be bought by refusing
/// everything.
#[cfg(unix)]
#[test]
fn a_link_that_stays_inside_is_still_created_and_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("ok.tar.gz");
    let staging = tmp.path().join("staging");
    linked_tar_gz_at(
        &archive,
        &[
            Member::File {
                name: "real.txt",
                data: b"hello",
            },
            Member::Dir("sub/"),
            Member::Link {
                name: "sub/back",
                target: "../real.txt",
            },
        ],
    );

    let indexed = mount_linked(&archive, &staging);

    let link = staging.join("sub/back");
    assert!(
        std::fs::symlink_metadata(&link).unwrap().is_symlink(),
        "a contained link must still be created"
    );
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "hello");
    assert_eq!(indexed.facts.escaping_links, 0);
    assert_eq!(indexed.facts.link_traversals, 0);
    assert!(crate::archive::scan::assess(&indexed.facts, ArchiveFormat::TarGz).is_writable());
}

/// The seekable path reaches extraction through `materialize`, not
/// `write_member`, so it needs its own proof — the fuzz target found the escape
/// on a plain `.tar` as well as a `.tar.gz`.
#[cfg(unix)]
#[test]
fn materialize_refuses_a_member_behind_a_planted_link() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(staging.join("d")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    // Stand in for the link an earlier member would have created.
    std::os::unix::fs::symlink("../../outside", staging.join("d/hop")).unwrap();

    let archive = tmp.path().join("seek.tar");
    tar_into(
        File::create(&archive).unwrap(),
        &[("d/hop/x.txt", b"pwned", 0o644)],
    );
    let indexed = index_seekable(&archive, ArchiveFormat::Tar, 1000).unwrap();
    let entry = indexed
        .index
        .entries
        .iter()
        .find(|e| e.inner == "d/hop/x.txt")
        .expect("member indexed");

    let err = materialize(&archive, entry, &staging)
        .expect_err("materializing through a symlink must be refused");
    assert!(
        format!("{err:#}").contains("symlink"),
        "the error must name the cause, got: {err:#}"
    );
    assert!(
        !outside.join("x.txt").exists(),
        "bytes escaped the staging root"
    );
}

// --- declared sizes are claims, not measurements ---

/// A tar header's size field is attacker input. Reserving it aborts the process
/// on a big enough lie — an abort, not an unwind, so the panic hook never runs
/// and the terminal is left in raw mode.
///
/// Built by hand: `tar::Builder` writes an honest header, and the whole point is
/// a dishonest one.
fn tar_header_with_declared_size(name: &str, declared: &[u8; 12], body: &[u8]) -> Vec<u8> {
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[100..108].copy_from_slice(b"0000644\0");
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    h[124..136].copy_from_slice(declared);
    h[136..148].copy_from_slice(b"00000000000\0");
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
    let chk = format!("{:06o}\0 ", sum & 0o777_777);
    h[148..156].copy_from_slice(chk.as_bytes());

    let mut out = Vec::from(h);
    out.extend_from_slice(body);
    out.resize(out.len().div_ceil(512) * 512, 0);
    out
}

fn tar_with_declared_size(path: &Path, name: &str, declared: &[u8; 12], body: &[u8]) {
    let mut out = tar_header_with_declared_size(name, declared, body);
    out.extend_from_slice(&[0u8; 1024]);
    std::fs::write(path, out).unwrap();
}

/// HIGH-2's PoC, in the direction that stayed live: a member **understating**
/// its size.
///
/// The finding named three consequences of trusting a zip's declared size. Two
/// were fixed — the allocation (`reserve_for`) and the decompression-bomb gate
/// (`size_is_exact`). The third, the MCP 100 KB read cap, checked the declaration
/// and nothing checked the bytes: a central directory claiming `size: 1` for a
/// 300 KB member sailed past the cap and `member_bytes` returned all 300 KB.
/// Measured, before this fix, at exactly that.
///
/// A tar can't do this (its stored bytes end where the next header begins, so a
/// lie under-reads), which is part of why the zip half was easy to miss.
#[test]
fn an_understated_zip_size_cannot_smuggle_bytes_past_a_read_cap() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("liar.zip");
    let body = vec![b'A'; 300_000];
    {
        let mut w = zip::ZipWriter::new(File::create(&archive).unwrap());
        w.start_file("big.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(&body).unwrap();
        w.finish().unwrap();
    }
    // Patch the central directory's uncompressed-size field to 1. `ZipWriter`
    // writes an honest header; the whole point is a dishonest one.
    let mut raw = std::fs::read(&archive).unwrap();
    let cd = raw
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .expect("central directory header");
    raw[cd + 24..cd + 28].copy_from_slice(&1u32.to_le_bytes());
    std::fs::write(&archive, &raw).unwrap();

    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();
    let entry = indexed.index.get("big.txt").expect("indexed");
    assert_eq!(
        entry.size, 1,
        "the index must carry the LIE; without that this test proves nothing"
    );

    // A 100 KB ceiling, the same one `mcp::readers` applies.
    let cap = 100 * 1024;
    let err = member_bytes_within(&archive, entry, cap)
        .expect_err("300 KB must not come back under a 100 KB cap");
    assert!(
        format!("{err:#}").contains("read limit"),
        "must say why: {err:#}"
    );

    // The uncapped path is unchanged — this is a ceiling, not a new refusal.
    let all = member_bytes(&archive, entry).expect("uncapped read still works");
    assert_eq!(all.len(), 300_000, "and still returns the real bytes");
}

/// The octal maximum (~64 GB) behind an empty body. Before the fix this reached
/// `Vec::with_capacity(68719476735)`.
///
/// **What this pins is the READ, not the allocation.** `take(size).read_to_end`
/// bounds the bytes either way, so these assertions hold with `reserve_for`
/// removed — verified. The allocation half cannot be reproduced end-to-end here:
/// a tar's octal size field tops out at ~64 GB, and a 64 GB reservation does not
/// abort on a 64-bit host that overcommits. `reserve_for` is unit-tested below,
/// and `every_declared_size_allocation_is_capped` is what ties the two together
/// by requiring the call sites to use it — which is how `write.rs` kept a raw
/// `with_capacity` through HIGH-1's fix.
#[test]
fn a_declared_size_far_beyond_the_file_does_not_drive_an_allocation() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("liar.tar");
    let staging = tmp.path().join("staging");
    tar_with_declared_size(&archive, "big.bin", b"777777777777", b"");

    let indexed = index_seekable(&archive, ArchiveFormat::Tar, 1000).unwrap();
    let entry = indexed.index.get("big.bin").expect("indexed");
    assert_eq!(entry.size, 0o777_777_777_777, "the claim is carried as-is");

    // The read completes, bounded by what the file actually holds, instead of
    // reserving what was claimed. (A lying header still yields whatever follows
    // it — here the tar's own end-of-archive padding — which is garbage in and
    // garbage out; the property under test is that it costs the file's size, not
    // the claim's.)
    let dest = materialize(&archive, entry, &staging).unwrap();
    let got = std::fs::read(&dest).unwrap().len() as u64;
    let on_disk = std::fs::metadata(&archive).unwrap().len();
    assert!(
        got <= on_disk,
        "read {got} bytes from a {on_disk}-byte archive"
    );
    assert!(got < 0o777_777_777_777, "the claim was not honoured");
}

#[test]
fn a_reservation_is_capped_however_large_the_claim() {
    assert_eq!(reserve_for(0), 0);
    assert_eq!(reserve_for(64), 64);
    assert_eq!(reserve_for(u64::MAX), RESERVE_CAP as usize);
    assert_eq!(reserve_for(RESERVE_CAP * 4096), RESERVE_CAP as usize);
}

/// Every declared-size allocation in `src/archive/` goes through
/// [`reserve_for`].
///
/// HIGH-1 named three sites. Two were fixed and `reserve_for` was unit-tested,
/// and `write.rs`'s `read_at` kept `Vec::with_capacity(usize::try_from(size))`
/// for the whole campaign — reachable, because a plain `.tar` is seekable, so
/// mounting one extracts nothing and the extract budget never inspects it: a
/// header lying about its size mounts fine and lands there on the first
/// `:archive write`.
///
/// Nothing caught it. The end-to-end test pins the *read* (which `take` bounds
/// anyway) and the unit test pins the *function* — neither connects the function
/// to its call sites, and the allocation itself can't be provoked from a tar
/// header on a 64-bit host (octal sizes cap at ~64 GB, which overcommit absorbs).
///
/// So the property has to be checked structurally — across **every** shape that
/// sizes an allocation from a number, not just `with_capacity`. Checking one
/// spelling while claiming to check the class is how the `write.rs` site
/// survived a whole campaign: `vec![0u8; declared]`, `.reserve(declared)` and
/// `.reserve_exact(declared)` allocate exactly the same way and were invisible.
#[test]
fn every_declared_size_allocation_is_capped() {
    // Split so this guard's own source can't match the needle.
    let with_capacity = ["with_", "capacity("].concat();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/archive");
    let mut offenders = Vec::new();

    /// The one production allocation under `src/archive/` sized by a spyc
    /// constant rather than by something an archive said. `read_head` fills a
    /// `SNIFF_BYTES` buffer to look at magic numbers.
    const CONSTANT_SIZED: &[&str] = &["vec![0u8; cap]"];

    /// Does this line allocate a caller-supplied count?
    fn allocates_a_count(line: &str, with_capacity: &str) -> bool {
        if line.contains(with_capacity)
            || line.contains(".reserve(")
            || line.contains(".reserve_exact(")
        {
            return true;
        }
        // `vec![elem; count]` allocates `count`; a plain `vec![a, b]` list does
        // not, and the trailing `;` of the statement must not be mistaken for
        // the repeat separator — so look only *inside* the brackets.
        if let Some((_, rest)) = line.split_once("vec![")
            && let Some((inside, _)) = rest.split_once(']')
        {
            return inside.contains(';');
        }
        false
    }

    fn scan(
        dir: &std::path::Path,
        with_capacity: &str,
        offenders: &mut Vec<String>,
        allocates: fn(&str, &str) -> bool,
        allowed: &[&str],
    ) {
        for entry in std::fs::read_dir(dir).expect("read archive dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                scan(&path, with_capacity, offenders, allocates, allowed);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            // A standalone `tests.rs` is test code end to end, and
            // `production_half` can only strip an *inline* `#[cfg(test)]` block
            // — handed this file it returns the whole thing, so a fixture
            // building `vec![b'A'; 300_000]` reads as a production allocation.
            if path.file_name().is_some_and(|n| n == "tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read .rs");
            let production = crate::guard_support::production_half(&text);
            for (i, line) in production.lines().enumerate() {
                // Comments about the hazard are not the hazard.
                if line.trim_start().starts_with("//") || !allocates(line, with_capacity) {
                    continue;
                }
                if line.contains("reserve_for(") || allowed.iter().any(|a| line.contains(a)) {
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                offenders.push(format!("{name}:{}", i + 1));
            }
        }
    }
    scan(
        &root,
        &with_capacity,
        &mut offenders,
        allocates_a_count,
        CONSTANT_SIZED,
    );
    offenders.sort();

    assert!(
        offenders.is_empty(),
        "these size an allocation from a count before seeing a byte — route the \
         ones fed by a container's declared size through `read::reserve_for`, \
         which caps it. A big enough lie *aborts* the process rather than \
         unwinding, so the panic hook never restores the terminal. (A genuinely \
         constant size goes in this guard's CONSTANT_SIZED list, with the \
         reason.) Offenders: {offenders:?}"
    );
}

/// The bomb gate has to measure what arrives. A header declaring 0 while the
/// stream carries megabytes used to walk straight past it, because the gate
/// added up declarations.
#[test]
fn the_extract_budget_counts_bytes_that_arrive_not_bytes_declared() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("bomb.tar.gz");
    let staging = tmp.path().join("staging");

    // A tar whose header says 0 bytes but whose data blocks hold 64 KB.
    let mut raw = Vec::new();
    {
        let mut h = [0u8; 512];
        let name = "lies.bin";
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"0000644\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        h[124..136].copy_from_slice(b"00000000000\0"); // declared: 0
        h[136..148].copy_from_slice(b"00000000000\0");
        h[156] = b'0';
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
        let chk = format!("{:06o}\0 ", sum & 0o777_777);
        h[148..156].copy_from_slice(chk.as_bytes());
        raw.extend_from_slice(&h);
    }
    let enc = flate2::write::GzEncoder::new(
        File::create(&archive).unwrap(),
        flate2::Compression::default(),
    );
    {
        let mut enc = enc;
        enc.write_all(&raw).unwrap();
        enc.finish().unwrap();
    }

    // Budget of 1 KB. Whatever the header claims, at most a hair over that may
    // reach disk before the mount is refused.
    let outcome = stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        1024,
        1000,
        &AtomicBool::new(false),
    );
    if outcome.is_ok() {
        let staged: u64 = std::fs::read_dir(&staging).map_or(0, |rd| {
            rd.flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        });
        assert!(
            staged <= 1025,
            "{staged} bytes reached disk against a 1024-byte budget"
        );
    }
}

/// The honest path must keep working: a member whose declared size is true is
/// still extracted whole, and a mount inside its budget still mounts.
#[test]
fn an_honest_archive_still_extracts_completely() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("ok.tar.gz");
    let staging = tmp.path().join("staging");
    let body = vec![b'x'; 4096];
    tar_gz_at(&archive, &[("big.txt", &body, 0o644)]);

    stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        1 << 20,
        1000,
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(
        std::fs::read(staging.join("big.txt")).unwrap().len(),
        4096,
        "an in-budget member must arrive intact"
    );
}

/// Two members differing only by case must each read back their OWN bytes.
///
/// `staging_rel()` exists so they don't collide on a case-insensitive volume —
/// every default macOS volume. Every reader used it; the streaming writer used
/// the raw member path instead, so on macOS the second member's bytes landed on
/// top of the first's and the reader for rank 1 looked under a prefix nothing had
/// ever written. Reading `a/README` returned the *other* member's content, and
/// `a/readme` was unreadable and — because the repack reads the same path —
/// unwritable.
///
/// Asserted through the index rather than against fixed paths, so it tests the
/// contract ("a member's bytes are where its own entry says") rather than the
/// current spelling of the escape prefix.
#[test]
fn a_streamed_case_collision_stages_each_member_where_its_own_entry_says() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("case.tar.gz");
    let staging = tmp.path().join("staging");
    tar_gz_at(
        &archive,
        &[("a/README", b"UPPER", 0o644), ("a/readme", b"lower", 0o644)],
    );

    let indexed = stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        1 << 20,
        1000,
        &AtomicBool::new(false),
    )
    .unwrap();

    // The premise: the two really are ranked as a collision. Without this the
    // test could pass on a hypothetical index that never ranked them.
    assert_eq!(
        indexed.facts.case_collisions, 1,
        "the two names differ only by case, so exactly one is ranked above 0"
    );

    for (inner, want) in [("a/README", &b"UPPER"[..]), ("a/readme", &b"lower"[..])] {
        let entry = indexed
            .index
            .get(inner)
            .unwrap_or_else(|| panic!("{inner} is in the index"));
        let at = staging.join(entry.staging_rel());
        let got = std::fs::read(&at)
            .unwrap_or_else(|e| panic!("{inner} staged at {}: {e}", at.display()));
        assert_eq!(
            got, want,
            "{inner} must read its own bytes, not the other member's"
        );
    }
}

/// A refill puts a member back where its own entry says too — the same contract,
/// on the path that repairs staging when something outside spyc empties it.
///
/// This is the half a comment used to claim and the code didn't: `restage_missing`
/// walked the container deriving paths from member names, so a case-ranked member
/// was checked for (and rewritten) at a path no reader consults. For a streamed
/// archive the staged bytes are the only copy outside the container, so the
/// archive became permanently unwritable.
#[test]
fn a_refill_puts_a_case_ranked_member_back_where_its_reader_looks() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("case.tar.gz");
    let staging = tmp.path().join("staging");
    tar_gz_at(
        &archive,
        &[("a/README", b"UPPER", 0o644), ("a/readme", b"lower", 0o644)],
    );
    let indexed = stream_mount(
        &archive,
        ArchiveFormat::TarGz,
        &staging,
        1 << 20,
        1000,
        &AtomicBool::new(false),
    )
    .unwrap();

    let ranked = indexed
        .index
        .entries
        .iter()
        .find(|e| e.case_rank > 0)
        .expect("one of the pair is ranked");
    let at = staging.join(ranked.staging_rel());
    let want = std::fs::read(&at).expect("it was staged");
    std::fs::remove_file(&at).expect("something outside spyc reaps it");

    let restored = restage_missing(&archive, &indexed.index, &staging, 1000).unwrap();

    assert_eq!(restored, 1, "the one missing member is refilled");
    assert_eq!(
        std::fs::read(&at).expect("back where its reader looks"),
        want,
        "and with its own bytes"
    );
}

/// A member can't reach the namespace spyc escapes case collisions into.
///
/// Ranked copies stage under a reserved prefix. Nothing stopped an archive from
/// containing a member *named* that prefix: its own (rank 0) staging path was
/// then the same path the ranked member's reader consults, `materialize` returns
/// early on `dest.exists()`, and whichever landed first served both. Contrived to
/// build, but it is content substitution under the archive's control.
///
/// The demonstrated fixture: `a/README`, `a/readme`, and a decoy sitting where
/// `a/readme` escapes to.
#[test]
fn a_member_cannot_squat_on_the_case_escape_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("decoy.zip");
    let staging = tmp.path().join("staging");
    // Aimed by asking where a rank-1 member escapes to, rather than by spelling
    // the prefix out — so this keeps aiming at the escape namespace if the
    // namespace is ever renamed, instead of quietly passing.
    let decoy = crate::archive::index::staging_rel_for("a/readme", 1)
        .to_str()
        .expect("the escape path is utf-8")
        .to_string();
    zip_at(
        &archive,
        &[
            (&decoy, b"DECOY", 0o644),
            ("a/README", b"RANK0", 0o644),
            ("a/readme", b"RANK1", 0o644),
        ],
    );

    let indexed = index_seekable(&archive, ArchiveFormat::Zip, 1000).unwrap();

    // Every indexed member must own its staging path — no two entries may
    // resolve to the same one, whatever the archive named them.
    let mut seen = std::collections::HashMap::new();
    for entry in &indexed.index.entries {
        if let Some(other) = seen.insert(entry.staging_rel(), entry.inner.clone()) {
            panic!(
                "{} and {} both stage at {}",
                other,
                entry.inner,
                entry.staging_rel().display()
            );
        }
    }

    // And the member that would have been shadowed reads its own bytes.
    let ranked = indexed
        .index
        .get("a/readme")
        .expect("the ranked member is indexed");
    let at = materialize(&archive, ranked, &staging).unwrap();
    assert_eq!(
        std::fs::read(&at).unwrap(),
        b"RANK1",
        "a/readme must not be served the decoy's bytes"
    );
}

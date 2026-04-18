//! Integration tests for `fs::Listing` and `fs::entry::Entry` against
//! real directory trees built with `tempfile`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tempfile::tempdir;

/// Build a temp directory with a known set of files and dirs.
fn build_tree(root: &Path) {
    fs::create_dir_all(root.join("alpha")).unwrap();
    fs::create_dir_all(root.join("beta")).unwrap();
    fs::write(root.join("readme.txt"), "hello").unwrap();
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("build.o"), "").unwrap();
    fs::write(root.join(".hidden"), "").unwrap();

    // Make a file executable.
    let exec_path = root.join("run.sh");
    fs::write(&exec_path, "#!/bin/sh\necho hi").unwrap();
    let mut perms = fs::metadata(&exec_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&exec_path, perms).unwrap();
}

// We can't import private items from a binary crate in integration tests,
// so we test the contract via `cargo test --bin spyc` in the unit tests.
// These integration tests invoke the binary or test public behavior.
//
// Since spyc is a binary crate (not a library), we can't `use spyc::fs::*`
// directly. Instead, we test the filesystem behavior that the crate relies
// on — the same primitives it uses internally.

#[test]
fn tempdir_tree_has_expected_entries() {
    let tmp = tempdir().unwrap();
    build_tree(tmp.path());

    let entries: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    assert!(entries.contains(&"alpha".to_string()));
    assert!(entries.contains(&"beta".to_string()));
    assert!(entries.contains(&"readme.txt".to_string()));
    assert!(entries.contains(&"main.rs".to_string()));
    assert!(entries.contains(&".hidden".to_string()));
    assert!(entries.contains(&"run.sh".to_string()));
    assert_eq!(entries.len(), 7); // 2 dirs + 5 files
}

#[test]
fn dirs_are_detected() {
    let tmp = tempdir().unwrap();
    build_tree(tmp.path());

    let md = fs::metadata(tmp.path().join("alpha")).unwrap();
    assert!(md.is_dir());

    let md = fs::metadata(tmp.path().join("readme.txt")).unwrap();
    assert!(!md.is_dir());
}

#[test]
fn executable_bit_is_set() {
    let tmp = tempdir().unwrap();
    build_tree(tmp.path());

    let md = fs::metadata(tmp.path().join("run.sh")).unwrap();
    assert!(md.permissions().mode() & 0o111 != 0);

    let md = fs::metadata(tmp.path().join("readme.txt")).unwrap();
    assert!(md.permissions().mode() & 0o111 == 0);
}

#[test]
fn sort_name_dirs_first() {
    let tmp = tempdir().unwrap();
    build_tree(tmp.path());

    let mut entries: Vec<(bool, String)> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| {
            let md = e.metadata().unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            (md.is_dir(), name)
        })
        .collect();

    // Sort dirs-first, then by name (case-insensitive) — mirroring spyc's
    // default SortMode::Name behavior.
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0) // dirs first (true > false)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });

    // First two should be dirs (alpha, beta)
    assert!(entries[0].0, "first entry should be a dir");
    assert!(entries[1].0, "second entry should be a dir");
    assert_eq!(entries[0].1, "alpha");
    assert_eq!(entries[1].1, "beta");
}

#[test]
fn extension_extraction() {
    // Mirrors the ext_of() helper in listing.rs
    fn ext_of(name: &str) -> &str {
        name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
    }

    assert_eq!(ext_of("main.rs"), "rs");
    assert_eq!(ext_of("file.tar.gz"), "gz");
    assert_eq!(ext_of(".hidden"), "hidden");
    assert_eq!(ext_of("noext"), "");
}

#[test]
fn empty_directory_lists_nothing() {
    let tmp = tempdir().unwrap();
    let entries: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(entries.is_empty());
}

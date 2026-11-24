//! The one file an upgrade runs (decision L).
//!
//! A jar validates, holds and `exec`s one file, in its store or a copy of its
//! own, and never looks the named path up again, so a file replaced at that
//! path after validation never runs.
//!
//! A store is a directory of entries, `<store>/<entry>/spyc-jar`, which spyc
//! writes and names (by the SHA-256 it computed). A jar's store is the
//! directory two levels above the file it runs from, and only when that
//! directory holds [`MARKER`] and is the user's, `0700` and not a link: a jar
//! run from a scratch path (tests, `cargo run`) would otherwise take an
//! arbitrary directory for one. A jar with no store keeps its copies in
//! [`COPIES`] beside its socket. Either way a copy is written whole, under a
//! name of its own, and nothing is ever written to an entry twice. The jar
//! hashes nothing.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

/// The file whose presence makes a directory a store.
pub const MARKER: &str = ".spyc-jar-store";
/// An entry's binary: `<store>/<entry>/spyc-jar`.
pub const ENTRY_FILE: &str = "spyc-jar";
/// Where a jar with no store keeps its copies, beside its socket.
pub const COPIES: &str = "jar-bin";

/// `dir` is a directory, not a link, owned by this user and closed to
/// everyone else.
pub fn private_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = dir
        .symlink_metadata()
        .with_context(|| format!("look at {}", dir.display()))?;
    let uid = rustix::process::getuid().as_raw();
    if !meta.file_type().is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        bail!(
            "{} must be a directory of uid {uid}'s, mode 0700 and not a link",
            dir.display()
        );
    }
    Ok(())
}

/// The store `image` belongs to, when it runs from one.
pub fn store_of(image: &Path) -> Option<PathBuf> {
    if image.file_name()? != ENTRY_FILE {
        return None;
    }
    let store = image.parent()?.parent()?;
    let marked = store
        .join(MARKER)
        .symlink_metadata()
        .is_ok_and(|m| m.is_file());
    (marked && private_dir(store).is_ok()).then(|| store.to_path_buf())
}

/// `build`, as `describe` and `upgraded` give it: the name of the entry, or
/// of the jar's own copy, that `image` is; empty for a jar run from anywhere
/// else.
pub fn build_of(image: &Path, copies: &Path) -> String {
    let in_copies = image.file_name().is_some_and(|f| f == ENTRY_FILE)
        && image.parent().and_then(Path::parent) == Some(copies);
    if store_of(image).is_none() && !in_copies {
        return String::new();
    }
    image
        .parent()
        .and_then(Path::file_name)
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The one file an upgrade to `named` runs: `named` itself when it's an
/// entry of `store`, or a copy written whole into `store`, or into `copies`
/// for a jar with none. `named` must be absolute.
pub fn one_file(named: &Path, store: Option<&Path>, copies: &Path) -> anyhow::Result<PathBuf> {
    if !named.is_absolute() {
        bail!("{} isn't an absolute path", named.display());
    }
    if let Some(store) = store
        && store_of(named).is_some_and(|s| same_dir(&s, store))
    {
        return Ok(named.to_path_buf());
    }
    let dir = if let Some(s) = store {
        s.to_path_buf()
    } else {
        use std::os::unix::fs::DirBuilderExt;
        match std::fs::DirBuilder::new().mode(0o700).create(copies) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).with_context(|| format!("create {}", copies.display())),
        }
        private_dir(copies)?;
        copies.to_path_buf()
    };
    write_whole(named, &dir)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Copy `src` into a new entry of `dir`, named after it and made unique:
/// written to a temporary name, synced, made `0555`, then renamed, so no
/// partial binary ever sits under an entry's name.
fn write_whole(src: &Path, dir: &Path) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    let stem = src
        .file_name()
        .map_or_else(|| "jar".into(), |n| n.to_string_lossy().into_owned());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let entry = dir.join(format!("{stem}.{:x}.{nanos:x}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&entry)
        .with_context(|| format!("create {}", entry.display()))?;
    let tmp = entry.join(".spyc-jar.partial");
    let copied = (|| -> anyhow::Result<()> {
        let mut from =
            std::fs::File::open(src).with_context(|| format!("open {}", src.display()))?;
        let mut to = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        std::io::copy(&mut from, &mut to).with_context(|| format!("copy {}", src.display()))?;
        to.flush()?;
        to.sync_all()?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o555))?;
        let file = entry.join(ENTRY_FILE);
        std::fs::rename(&tmp, &file)?;
        if let Ok(d) = std::fs::File::open(&entry) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_dir(&entry);
        return Err(e);
    }
    Ok(entry.join(ENTRY_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A directory of the test's own, removed when it's dropped.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("jar-store-{name}-{nanos}"));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn private(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn binary(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
    }

    #[test]
    fn a_store_is_marked_private_and_two_levels_up() {
        let tmp = Tmp::new("marked");
        let store = tmp.path().join("store");
        private(&store);
        let image = store.join("abc123").join(ENTRY_FILE);
        binary(&image);
        assert_eq!(store_of(&image), None, "no marker, no store");
        std::fs::write(store.join(MARKER), b"").unwrap();
        assert_eq!(store_of(&image), Some(store.clone()));
        assert_eq!(build_of(&image, Path::new("/nowhere")), "abc123");
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(store_of(&image), None, "open to others");
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
        let other = image.with_file_name("not-spyc-jar");
        binary(&other);
        assert_eq!(store_of(&other), None, "not an entry's binary");
        let linked = tmp.path().join("linked");
        std::os::unix::fs::symlink(&store, &linked).unwrap();
        assert_eq!(
            store_of(&linked.join("abc123").join(ENTRY_FILE)),
            None,
            "a link"
        );
    }

    #[test]
    fn an_entry_is_used_as_it_is_and_anything_else_is_copied_whole() {
        let tmp = Tmp::new("copies");
        let store = tmp.path().join("store");
        private(&store);
        std::fs::write(store.join(MARKER), b"").unwrap();
        let entry = store.join("abc123").join(ENTRY_FILE);
        binary(&entry);
        let copies = tmp.path().join("copies");
        assert_eq!(one_file(&entry, Some(&store), &copies).unwrap(), entry);

        let loose = tmp.path().join("jar-next");
        std::fs::write(&loose, b"the new binary").unwrap();
        let into_store = one_file(&loose, Some(&store), &copies).unwrap();
        assert_eq!(store_of(&into_store).as_deref(), Some(store.as_path()));
        assert_eq!(std::fs::read(&into_store).unwrap(), b"the new binary");
        let mode = std::fs::metadata(&into_store).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o555);
        assert!(build_of(&into_store, &copies).starts_with("jar-next."));
        std::fs::write(&loose, b"swapped").unwrap();
        assert_eq!(
            std::fs::read(&into_store).unwrap(),
            b"the new binary",
            "a copy, not a link"
        );

        let without = one_file(&loose, None, &copies).unwrap();
        assert_eq!(without.parent().unwrap().parent().unwrap(), copies);
        assert!(build_of(&without, &copies).starts_with("jar-next."));
        let again = one_file(&loose, None, &copies).unwrap();
        assert_ne!(again, without, "every copy under a name of its own");

        assert!(one_file(Path::new("relative/spyc-jar"), Some(&store), &copies).is_err());
        assert!(one_file(&tmp.path().join("missing"), None, &copies).is_err());
        let left: Vec<_> = std::fs::read_dir(&copies).unwrap().collect();
        assert_eq!(
            left.len(),
            2,
            "a failed copy leaves nothing behind: {left:?}"
        );
    }
}

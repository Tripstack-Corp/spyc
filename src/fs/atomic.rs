//! Atomic file writes.
//!
//! Write the new contents to a temporary file in the *same directory* as the
//! destination, then rename it over the target. On a POSIX filesystem the
//! rename is atomic, so a concurrent reader (a claude/codex process reading
//! `.mcp.json` / `.codex/config.toml`, the next spyc launch loading marks)
//! sees either the old file or the complete new one — never a half-written
//! one. A crash mid-write leaves the original intact and discards the temp
//! file, instead of truncating the real file to nothing.

use std::io::Write;
use std::path::Path;

/// Atomically replace `path` with `contents`.
///
/// The temp file is created in `path`'s parent directory so the final rename
/// stays on one filesystem (a cross-device rename is not atomic and would
/// fail). The caller must ensure that parent directory exists first. The temp
/// file inherits `NamedTempFile`'s owner-only (0600) permissions — appropriate
/// for the per-user config/state files this is used for.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    // `persist` does the rename; on failure it hands back the io::Error.
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Atomically replace `dst` with a copy of `src`, permission bits included.
///
/// The rename is also what lets a read-only `dst` be replaced: writing into it
/// would have to open it for writing, which its own mode refuses.
pub fn copy_atomic(src: &Path, dst: &Path) -> std::io::Result<()> {
    let dir = dst.parent().unwrap_or_else(|| Path::new("."));
    let tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::fs::copy(src, tmp.path())?;
    tmp.persist(dst).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{copy_atomic, write_atomic};

    #[test]
    fn a_copy_replaces_a_read_only_destination_and_keeps_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("src"), dir.path().join("dst"));
        std::fs::write(&src, "first").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o444)).unwrap();
        copy_atomic(&src, &dst).unwrap();

        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&src, "second").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o444)).unwrap();
        copy_atomic(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "second");
        let mode = std::fs::metadata(&dst).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o444);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn writes_and_replaces_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg.json");
        write_atomic(&path, b"first").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        // Overwrite: the destination ends up with exactly the new bytes.
        write_atomic(&path, b"second longer contents").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second longer contents");
    }

    #[test]
    fn leaves_no_temp_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marks.toml");
        write_atomic(&path, b"x").unwrap();
        // The directory holds only the destination — the temp file was renamed.
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("marks.toml")]);
    }
}

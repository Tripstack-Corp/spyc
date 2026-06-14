//! File-system operations, pure Rust: both the *mutating* ops
//! (`copy_tree`/`move_tree`/`remove_tree`/`*_selection_to`/`chmod_add_bits`)
//! and the read-side *viewer* helpers the file-display path needs
//! (`read_truncated`, `format_size`, `file_type_label`, `read_hex_window`).
//! The *styling* of the hex window into pager lines lives in the UI layer
//! ([`crate::ui::hex`]) — `fs` reads bytes, `ui` paints them.
//!
//! The mutating ops replace the `sh -c "cp -r ..."`-style shell-outs we used
//! originally. Benefits:
//!
//! - Portable: no dependency on the host's `cp`/`mv`/`rm`/`chmod` binaries.
//! - Faster for small ops (no subprocess).
//! - Errors come back as typed `io::Error`s we can surface in the UI.
//!
//! Non-goals:
//!
//! - Bit-for-bit parity with BSD `cp` flags. We mirror the *common* shape
//!   (recursive, preserving symlinks, mv semantics) — if someone needs
//!   obscure flags they can `!cp -X %` through the shell.
//! - Preserving every attribute. `std::fs::copy` copies permissions on
//!   Unix, which is enough for almost everything; xattrs / ACLs / xdev
//!   reflinks are out of scope.

use std::fs;
use std::io;
use std::io::BufRead as _;
use std::io::Read as _;
use std::path::Path;
use std::path::PathBuf;

/// Soft cap above which the in-app pager loads only the first
/// `MAX_PAGER_LINES` lines of a file instead of the whole content.
/// Files past this cap will also skip syntect highlighting (it's
/// the dominant memory amplifier — every token allocates a styled
/// span). The user can press `p` in the pager to hand the file
/// off to `$PAGER` (less, by default) which mmap's huge files
/// efficiently.
pub const MAX_PAGER_BYTES: u64 = 5 * 1024 * 1024;

/// Hard cap on the line count we load when a file is past
/// `MAX_PAGER_BYTES`. 5000 is enough to glance at a header / spot-
/// check / find a pattern; full traversal of huge files belongs
/// in `$PAGER`.
pub const MAX_PAGER_LINES: usize = 5000;

/// Read at most the first `MAX_PAGER_LINES` lines of `path`.
/// Returns `(content, total_lines_read, truncated)`. `truncated`
/// is true if we hit the line cap before EOF; callers use it to
/// decide whether to skip syntect and emit a banner row.
pub fn read_truncated(path: &Path, max_lines: usize) -> io::Result<(String, usize, bool)> {
    let f = fs::File::open(path)?;
    let mut reader = io::BufReader::new(f);
    let mut buf = String::new();
    let mut lines_read = 0usize;
    let mut line = String::new();
    while lines_read < max_lines {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok((buf, lines_read, false));
        }
        buf.push_str(&line);
        lines_read += 1;
    }
    // We hit the cap. Try to read one more byte to know whether
    // there's actually more content (truncated == true) or the
    // file ended exactly at the cap.
    let mut probe = [0u8; 1];
    let truncated = matches!(reader.read(&mut probe), Ok(n) if n > 0);
    Ok((buf, lines_read, truncated))
}

/// Remove a path, whether it is a regular file, a symlink, or a directory
/// tree. Symlinks are never followed — we remove the link, not its target.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Recursively copy `src` to `dst`. Symlinks are re-created as symlinks
/// pointing at the same target; they are **not** followed.
///
/// Refuses, like `cp(1)`, when `dst` is the same file as `src` (else
/// `fs::copy` would open it with `O_TRUNC` and silently zero it) or when
/// `dst` lies inside the `src` tree (else the recursion re-discovers the
/// directories it just created and descends until `ENAMETOOLONG`).
pub fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    reject_self_or_nested(src, dst, "copy")?;
    copy_tree_recursive(src, dst)
}

fn copy_tree_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let target = fs::read_link(src)?;
            std::os::unix::fs::symlink(target, dst)?;
        }
        #[cfg(not(unix))]
        {
            // Non-Unix: copy the link target as a file. We only support
            // Linux and macOS, so this branch should be unreachable.
            fs::copy(src, dst)?;
        }
        return Ok(());
    }
    if md.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_tree_recursive(&child_src, &child_dst)?;
        }
        return Ok(());
    }
    fs::copy(src, dst)?;
    Ok(())
}

/// Move `src` to `dst`. Tries a cheap `rename` first and falls back to
/// `copy_tree` + `remove_tree` only when the rename fails with EXDEV
/// (cross-filesystem). All other rename errors propagate — we never mask
/// a permissions or destination-exists error with a sneaky copy.
pub fn move_tree(src: &Path, dst: &Path) -> io::Result<()> {
    // Refuse same-file / dir-into-itself up front, matching `mv(1)`.
    // (A bare `rename(src, src)` is a silent no-op and an EXDEV move into
    // the source's own subtree would fall through to `copy_tree`.)
    reject_self_or_nested(src, dst, "move")?;
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_tree(src, dst)?;
            remove_tree(src)
        }
        Err(e) => Err(e),
    }
}

/// Refuse a copy/move whose destination is the same file as the source,
/// or lies inside the source directory tree — the two cases `cp(1)` itself
/// rejects ("X and Y are the same file" / "cannot copy a directory into
/// itself"). Both are silent data-loss in `std::fs`: `fs::copy(src, src)`
/// opens the destination with `O_TRUNC` before reading and zeroes the file,
/// and a dir-into-its-own-subtree copy keeps re-discovering the directories
/// it just created until the path exceeds `PATH_MAX`.
fn reject_self_or_nested(src: &Path, dst: &Path, verb: &str) -> io::Result<()> {
    if same_file(src, dst) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cannot {verb}: '{}' and '{}' are the same file",
                src.display(),
                dst.display()
            ),
        ));
    }
    if let (Ok(csrc), Some(cdst)) = (fs::canonicalize(src), canonical_target(dst))
        && cdst != csrc
        && cdst.starts_with(&csrc)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cannot {verb}: '{}' is inside '{}'",
                dst.display(),
                src.display()
            ),
        ));
    }
    Ok(())
}

/// True when `a` and `b` resolve to the same on-disk file. Uses a `dev`+`ino`
/// comparison on Unix (robust across `.`, trailing slashes, hard links, and
/// symlinks); both paths must exist, so a not-yet-created destination is never
/// "the same file".
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Best-effort absolute, symlink-resolved form of a path that may not exist
/// yet: canonicalize the deepest existing ancestor and re-append the trailing
/// components that don't exist. Returns `None` only when no ancestor resolves.
fn canonical_target(path: &Path) -> Option<PathBuf> {
    if let Ok(c) = fs::canonicalize(path) {
        return Some(c);
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path;
    loop {
        let parent = cur.parent()?;
        tail.push(cur.file_name()?.to_owned());
        if let Ok(mut out) = fs::canonicalize(parent) {
            for comp in tail.iter().rev() {
                out.push(comp);
            }
            return Some(out);
        }
        cur = parent;
    }
}

/// Copy every entry in `sources` into `dest`, mirroring `cp` semantics:
///
/// - If `dest` is an existing directory, each source is copied to
///   `dest/<source-name>`.
/// - If `dest` does not exist and exactly one source is given, it is
///   copied directly to `dest` (i.e. `cp A B` when `B` does not exist
///   makes `B` a copy of `A`).
/// - Otherwise — multi-source into a non-existing or non-directory dest —
///   returns `InvalidInput`.
pub fn copy_selection_to(sources: &[&Path], dest: &Path) -> io::Result<()> {
    dispatch_selection(sources, dest, copy_tree, "copy")
}

/// Move semantics, same dispatch as `copy_selection_to` but using
/// `move_tree` underneath.
pub fn move_selection_to(sources: &[&Path], dest: &Path) -> io::Result<()> {
    dispatch_selection(sources, dest, move_tree, "move")
}

fn dispatch_selection(
    sources: &[&Path],
    dest: &Path,
    one: fn(&Path, &Path) -> io::Result<()>,
    verb: &str,
) -> io::Result<()> {
    if sources.is_empty() {
        return Ok(());
    }
    let dest_exists = dest.exists();
    if dest_exists && dest.is_dir() {
        for src in sources {
            let Some(name) = src.file_name() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("cannot {verb}: source has no file name"),
                ));
            };
            one(src, &dest.join(name))?;
        }
        return Ok(());
    }
    if !dest_exists && sources.len() == 1 {
        return one(sources[0], dest);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "cannot {verb}: destination must be an existing directory when moving multiple sources"
        ),
    ))
}

/// OR the given bits into every path's permission mode. Pure-Rust
/// equivalent of `chmod +w` / `chmod +x`.
#[cfg(unix)]
pub fn chmod_add_bits(paths: &[&Path], bits: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for path in paths {
        let md = fs::metadata(path)?;
        let current = md.permissions().mode();
        let mut perms = md.permissions();
        perms.set_mode(current | bits);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn chmod_add_bits(_paths: &[&Path], _bits: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "chmod is only supported on unix",
    ))
}

/// Human-readable size: `512B`, `4.0K`, `1.2M`, `3.4G`, etc. Shared by the
/// long-listing table (`fs::long_listing`) and the file-type label below.
pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T", "P"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else if value >= 10.0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

// ---- File type detection (`f`) -------------------------------------------

/// Return a short human label describing the file's kind: directory, symlink
/// (to where), ELF/Mach-O executable, common media / archive magic, or the
/// text/binary fallback. Pure-Rust replacement for the `file(1)` one-liner.
pub fn file_type_label(path: &Path) -> String {
    let md = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return format!("error: {e}"),
    };
    let ft = md.file_type();
    if ft.is_dir() {
        return "directory".to_string();
    }
    if ft.is_symlink() {
        let tgt = fs::read_link(path)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        return if tgt.is_empty() {
            "symbolic link".to_string()
        } else {
            format!("symbolic link -> {tgt}")
        };
    }
    if !ft.is_file() {
        return "special file".to_string();
    }

    // Peek at up to 512 bytes of the head for magic-byte sniffing.
    let head = read_head(path, 512).unwrap_or_default();
    if let Some(label) = magic_label(&head) {
        return format!("{label}, {}", format_size(md.len()));
    }
    // Fall back: binary (contains NUL) vs text (ASCII/UTF-8-friendly).
    let is_text = !head.contains(&0);
    let kind = if is_text { "text file" } else { "binary data" };
    format!("{kind}, {}", format_size(md.len()))
}

fn read_head(path: &Path, cap: usize) -> io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; cap];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

/// Detect a handful of common file magics. Intentionally narrow — this
/// isn't libmagic; we only claim the signatures we are confident about.
fn magic_label(head: &[u8]) -> Option<&'static str> {
    let cases: &[(&[u8], &str)] = &[
        (b"\x7fELF", "ELF executable"),
        (b"\xfe\xed\xfa\xce", "Mach-O 32-bit executable"),
        (b"\xce\xfa\xed\xfe", "Mach-O 32-bit executable (reverse)"),
        (b"\xfe\xed\xfa\xcf", "Mach-O 64-bit executable"),
        (b"\xcf\xfa\xed\xfe", "Mach-O 64-bit executable (reverse)"),
        (b"\xca\xfe\xba\xbe", "Mach-O universal binary"),
        (b"\x89PNG\r\n\x1a\n", "PNG image"),
        (b"GIF87a", "GIF image"),
        (b"GIF89a", "GIF image"),
        (b"\xff\xd8\xff", "JPEG image"),
        (b"%PDF-", "PDF document"),
        (b"PK\x03\x04", "ZIP archive"),
        (b"PK\x05\x06", "ZIP archive (empty)"),
        (b"\x1f\x8b", "gzip compressed"),
        (b"BZh", "bzip2 compressed"),
        (b"\xfd7zXZ\x00", "xz compressed"),
        (b"7z\xbc\xaf\x27\x1c", "7-Zip archive"),
        (b"#!", "script (shebang)"),
        (b"<?xml", "XML document"),
        (b"{\n", "JSON document"),
        (b"[\n", "JSON array"),
        (b"<!DOCTYPE html", "HTML document"),
        (b"<html", "HTML document"),
    ];
    for (sig, label) in cases {
        if head.starts_with(sig) {
            return Some(label);
        }
    }
    None
}

// ---- Hex dump (`d` / Enter on binary files) --------------------------------

/// Max bytes we'll read for the hex view. 64 KiB is plenty to inspect
/// headers, magic bytes, and initial structure without loading a giant
/// binary into memory.
pub const HEX_CAP: usize = 64 * 1024;

/// Read up to `cap` bytes from `path` for the hex view. The second tuple
/// element is whether the file has *more* content beyond `cap` (so the caller
/// can flag the dump as truncated). Uses `read_to_end` over a `take(cap)` so a
/// short `read()` doesn't under-fill the buffer, then probes one extra byte: a
/// file that ends exactly at `cap` is complete, not truncated.
///
/// The byte read lives here in the `fs` layer; styling the bytes into pager
/// lines is [`crate::ui::hex::hex_dump_lines`].
pub fn read_hex_window(path: &Path, cap: usize) -> io::Result<(Vec<u8>, bool)> {
    use std::io::Read as _;
    let mut f = fs::File::open(path)?;
    let mut buf = Vec::new();
    (&mut f).take(cap as u64).read_to_end(&mut buf)?;
    let mut probe = [0u8; 1];
    let truncated = matches!(f.read(&mut probe), Ok(n) if n > 0);
    Ok((buf, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn read_hex_window_not_truncated_when_file_ends_exactly_at_cap() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("exact.bin");
        File::create(&path)
            .unwrap()
            .write_all(&[0xABu8; 8])
            .unwrap();
        let (buf, truncated) = read_hex_window(&path, 8).unwrap();
        assert_eq!(buf.len(), 8);
        assert!(!truncated, "a file ending exactly at cap is complete");
    }

    #[test]
    fn read_hex_window_truncated_when_file_exceeds_cap() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("over.bin");
        File::create(&path)
            .unwrap()
            .write_all(&[0xABu8; 9])
            .unwrap();
        let (buf, truncated) = read_hex_window(&path, 8).unwrap();
        assert_eq!(buf.len(), 8);
        assert!(truncated);
    }

    #[test]
    fn read_truncated_returns_full_content_when_under_cap() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("small.txt");
        File::create(&path)
            .unwrap()
            .write_all(b"line1\nline2\nline3\n")
            .unwrap();
        let (content, lines, truncated) = read_truncated(&path, 100).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
        assert_eq!(lines, 3);
        assert!(!truncated);
    }

    #[test]
    fn read_truncated_caps_at_max_lines_and_flags_remainder() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("big.txt");
        let mut f = File::create(&path).unwrap();
        for i in 0..50 {
            writeln!(f, "line {i}").unwrap();
        }
        let (content, lines, truncated) = read_truncated(&path, 10).unwrap();
        assert_eq!(lines, 10);
        assert!(
            truncated,
            "expected truncated flag for 50-line file capped at 10"
        );
        assert_eq!(content.lines().count(), 10);
        assert!(content.starts_with("line 0\n"));
        assert!(content.ends_with("line 9\n"));
    }

    #[test]
    fn read_truncated_handles_exactly_max_lines() {
        // File ends exactly at the cap — should NOT be flagged truncated.
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("exact.txt");
        let mut f = File::create(&path).unwrap();
        for i in 0..5 {
            writeln!(f, "{i}").unwrap();
        }
        let (_, lines, truncated) = read_truncated(&path, 5).unwrap();
        assert_eq!(lines, 5);
        assert!(!truncated, "file ending at the cap should not be flagged");
    }

    #[test]
    fn copy_tree_file() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        let dst = tmp.path().join("b.txt");
        File::create(&src).unwrap().write_all(b"hello").unwrap();
        copy_tree(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"hello");
        assert!(src.exists(), "source should remain after copy");
    }

    #[test]
    fn copy_tree_directory() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        File::create(src.join("x.txt"))
            .unwrap()
            .write_all(b"X")
            .unwrap();
        fs::create_dir(src.join("inner")).unwrap();
        File::create(src.join("inner/y.txt"))
            .unwrap()
            .write_all(b"Y")
            .unwrap();

        let dst = tmp.path().join("dst");
        copy_tree(&src, &dst).unwrap();
        assert_eq!(fs::read(dst.join("x.txt")).unwrap(), b"X");
        assert_eq!(fs::read(dst.join("inner/y.txt")).unwrap(), b"Y");
    }

    #[cfg(unix)]
    #[test]
    fn copy_tree_preserves_symlinks() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().join("target.txt");
        File::create(&target).unwrap().write_all(b"t").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let dst_link = tmp.path().join("link_copy");
        copy_tree(&link, &dst_link).unwrap();
        let md = fs::symlink_metadata(&dst_link).unwrap();
        assert!(md.file_type().is_symlink());
    }

    #[test]
    fn move_tree_renames_on_same_fs() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src.txt");
        let dst = tmp.path().join("dst.txt");
        File::create(&src).unwrap().write_all(b"m").unwrap();
        move_tree(&src, &dst).unwrap();
        assert!(!src.exists());
        assert!(dst.exists());
    }

    #[test]
    fn remove_tree_file() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("doomed.txt");
        File::create(&p).unwrap();
        remove_tree(&p).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn remove_tree_directory() {
        let tmp = tempdir().unwrap();
        let d = tmp.path().join("doomed");
        fs::create_dir(&d).unwrap();
        File::create(d.join("x")).unwrap();
        remove_tree(&d).unwrap();
        assert!(!d.exists());
    }

    #[test]
    fn copy_selection_multi_into_dir() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        File::create(&a).unwrap().write_all(b"a").unwrap();
        File::create(&b).unwrap().write_all(b"b").unwrap();
        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();
        copy_selection_to(&[&a, &b], &dest).unwrap();
        assert_eq!(fs::read(dest.join("a")).unwrap(), b"a");
        assert_eq!(fs::read(dest.join("b")).unwrap(), b"b");
    }

    #[test]
    fn copy_selection_multi_requires_existing_dir() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        File::create(&a).unwrap();
        File::create(&b).unwrap();
        let dest = tmp.path().join("notyet"); // does not exist
        let err = copy_selection_to(&[&a, &b], &dest).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn copy_tree_refuses_same_file_without_truncating() {
        // `fs::copy(src, src)` would open the file with O_TRUNC and zero it.
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("data.txt");
        File::create(&p).unwrap().write_all(b"important").unwrap();
        let err = copy_tree(&p, &p).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read(&p).unwrap(),
            b"important",
            "source must be untouched after a refused same-file copy"
        );
    }

    #[test]
    fn copy_selection_into_own_dir_refuses_and_preserves_content() {
        // Pick a file and give its own directory as the destination: the
        // computed target (dir/<name>) is the same inode as the source.
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("note.txt");
        File::create(&f).unwrap().write_all(b"keep me").unwrap();
        let err = copy_selection_to(&[&f], tmp.path()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read(&f).unwrap(), b"keep me");
    }

    #[test]
    fn copy_tree_refuses_dest_inside_source() {
        // Copying a dir into its own subtree would recurse until ENAMETOOLONG.
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("proj");
        fs::create_dir(&src).unwrap();
        File::create(src.join("file"))
            .unwrap()
            .write_all(b"x")
            .unwrap();
        let backup = src.join("backup");
        fs::create_dir(&backup).unwrap();
        let dst = backup.join("proj"); // proj/backup/proj — inside src
        let err = copy_tree(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(
            !dst.exists(),
            "no nested copy should have been created before the refusal"
        );
    }

    #[test]
    fn move_tree_refuses_same_file() {
        // `mv a a` is an error in cp/mv land; a bare rename(src, src) is a
        // silent no-op, so guard it for a clear message.
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("m.txt");
        File::create(&p).unwrap().write_all(b"m").unwrap();
        let err = move_tree(&p, &p).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(p.exists(), "source must survive a refused same-file move");
    }

    #[test]
    fn copy_tree_into_sibling_dir_still_works() {
        // Regression guard: the self/nested check must not reject a legitimate
        // copy whose destination merely shares a parent with the source.
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        File::create(src.join("a"))
            .unwrap()
            .write_all(b"A")
            .unwrap();
        let dst = tmp.path().join("dst");
        copy_tree(&src, &dst).unwrap();
        assert_eq!(fs::read(dst.join("a")).unwrap(), b"A");
        assert!(src.join("a").exists());
    }

    #[test]
    fn format_size_picks_appropriate_unit() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1024), "1.0K");
        assert_eq!(format_size(1536), "1.5K");
        assert_eq!(format_size(10 * 1024), "10K");
        assert_eq!(format_size(1024 * 1024), "1.0M");
        assert_eq!(format_size(12 * 1024 * 1024), "12M");
    }

    #[test]
    fn file_type_label_detects_png_magic() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("x.png");
        File::create(&p)
            .unwrap()
            .write_all(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0])
            .unwrap();
        let label = file_type_label(&p);
        assert!(label.starts_with("PNG image"), "got: {label}");
    }

    #[test]
    fn file_type_label_text_fallback() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("notes.txt");
        File::create(&p)
            .unwrap()
            .write_all(b"just a short note without any NUL bytes")
            .unwrap();
        let label = file_type_label(&p);
        assert!(label.starts_with("text file"), "got: {label}");
    }

    #[test]
    fn file_type_label_binary_fallback() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("blob.bin");
        File::create(&p)
            .unwrap()
            .write_all(&[1, 2, 3, 0, 4, 5])
            .unwrap();
        let label = file_type_label(&p);
        assert!(label.starts_with("binary data"), "got: {label}");
    }

    #[cfg(unix)]
    #[test]
    fn chmod_add_execute_bit() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("script");
        File::create(&p).unwrap();
        let before = fs::metadata(&p).unwrap().permissions().mode();
        chmod_add_bits(&[&p], 0o111).unwrap();
        let after = fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(after & 0o111, 0o111);
        assert_eq!(after & !0o111, before & !0o111);
    }
}

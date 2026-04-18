//! Mutating file-system operations, pure Rust.
//!
//! These replace the `sh -c "cp -r ..."`-style shell-outs we used
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

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::io::Read as _;
use std::path::Path;

/// Value of the POSIX `EXDEV` errno ("cross-device link"). Same on Linux
/// and macOS; hardcoded so we can detect cross-filesystem renames without
/// pulling in `libc` just for this one constant.
const EXDEV: i32 = 18;

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

pub fn remove_all(paths: &[&Path]) -> io::Result<()> {
    for p in paths {
        remove_tree(p)?;
    }
    Ok(())
}

/// Recursively copy `src` to `dst`. Symlinks are re-created as symlinks
/// pointing at the same target; they are **not** followed.
pub fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
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
            copy_tree(&child_src, &child_dst)?;
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
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(EXDEV) => {
            copy_tree(src, dst)?;
            remove_tree(src)
        }
        Err(e) => Err(e),
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

// ---- Long listing (`L`) ---------------------------------------------------

/// Produce `ls -l`-style lines for each path. One line per path; columns
/// are mode, size, name. Unreadable paths render as `?? <path>: <error>`
/// so one bad entry doesn't kill the whole listing.
pub fn format_long_listing(paths: &[&Path]) -> Vec<String> {
    paths.iter().map(|p| format_long_line(p)).collect()
}

fn format_long_line(path: &Path) -> String {
    let md = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return format!("?? {}: {e}", path.display()),
    };
    let mode = format_mode(&md);
    let size = format_size(md.len());
    let name = display_name(path, &md);
    let mut out = String::with_capacity(40 + name.len());
    let _ = write!(out, "{mode:<10}  {size:>8}  {name}");
    if md.file_type().is_symlink() {
        if let Ok(target) = fs::read_link(path) {
            let _ = write!(out, " -> {}", target.display());
        }
    }
    out
}

fn display_name(path: &Path, md: &fs::Metadata) -> String {
    let base = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    if md.is_dir() {
        format!("{base}/")
    } else if md.file_type().is_symlink() {
        base
    } else if is_exec(md) {
        format!("{base}*")
    } else {
        base
    }
}

#[cfg(unix)]
fn is_exec(md: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    md.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_exec(_md: &fs::Metadata) -> bool {
    false
}

/// Format as `drwxr-xr-x` style. On non-Unix we only have kind info so we
/// render the first character from the file type and `?` for the rest.
fn format_mode(md: &fs::Metadata) -> String {
    let mut out = String::with_capacity(10);
    out.push(kind_char(md));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = md.permissions().mode();
        for (shift, _) in [(6, 0), (3, 1), (0, 2)] {
            let bits = (mode >> shift) & 0b111;
            out.push(if bits & 0b100 != 0 { 'r' } else { '-' });
            out.push(if bits & 0b010 != 0 { 'w' } else { '-' });
            out.push(if bits & 0b001 != 0 { 'x' } else { '-' });
        }
    }
    #[cfg(not(unix))]
    for _ in 0..9 {
        out.push('?');
    }
    out
}

fn kind_char(md: &fs::Metadata) -> char {
    let ft = md.file_type();
    if ft.is_dir() {
        'd'
    } else if ft.is_symlink() {
        'l'
    } else {
        '-'
    }
}

/// Human-readable size: `512B`, `4.0K`, `1.2M`, `3.4G`, etc.
fn format_size(bytes: u64) -> String {
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
const HEX_CAP: usize = 64 * 1024;

/// Read up to `HEX_CAP` bytes from `path` and format as a hex dump.
/// Returns styled lines ready for the pager: each line split into
/// offset (dim), hex bytes (default), and ASCII sidebar (warm).
pub fn hex_dump_lines(
    path: &Path,
    theme: &crate::ui::theme::Theme,
) -> io::Result<Vec<ratatui::text::Line<'static>>> {
    use pretty_hex::{HexConfig, config_hex};
    use ratatui::{
        style::{Modifier, Style},
        text::{Line, Span},
    };

    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; HEX_CAP];
    let n = f.read(&mut buf)?;
    buf.truncate(n);

    let hex_str = config_hex(
        &buf,
        HexConfig {
            title: false,
            width: 16,
            group: 0,
            ..HexConfig::default()
        },
    );

    let offset_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);
    let hex_style = Style::default().fg(theme.file);
    let ascii_style = Style::default().fg(theme.pick).add_modifier(Modifier::DIM);
    let sep_style = Style::default().fg(theme.status_suffix);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for text_line in hex_str.lines() {
        // pretty-hex lines look like:
        //   00000000: 7f 45 4c 46 02 01 01 00 00 00 00 00 00 00 00 00  .ELF............
        // Split into offset (before ':'), hex (middle), ascii (after '  ').
        if let Some(colon) = text_line.find(':') {
            let offset_part = &text_line[..colon];
            let rest = &text_line[colon + 1..];
            // The ASCII sidebar is the last segment after "  " (double space).
            let (hex_part, ascii_part) = if let Some(sep) = rest.rfind("  ") {
                (&rest[..sep], rest[sep + 2..].trim())
            } else {
                (rest, "")
            };
            lines.push(Line::from(vec![
                Span::styled(offset_part.to_string(), offset_style),
                Span::styled(":", sep_style),
                Span::styled(hex_part.to_string(), hex_style),
                Span::styled("  ", sep_style),
                Span::styled(ascii_part.to_string(), ascii_style),
            ]));
        } else {
            // Fallback: unstyled.
            lines.push(Line::from(text_line.to_string()));
        }
    }

    if n >= HEX_CAP {
        lines.push(Line::from(Span::styled(
            format!("... truncated at {HEX_CAP} bytes ({n} read)"),
            offset_style,
        )));
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

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

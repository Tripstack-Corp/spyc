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

use std::fs;
use std::io;
use std::io::BufRead as _;
use std::io::Read as _;
use std::path::Path;

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

/// Per-row data for the long-listing table. One field per column.
struct LongRow {
    inode: String,
    mode: String,
    oct: String,
    links: String,
    owner: String,
    group: String,
    size: String,
    bytes: String,
    blocks: String,
    mtime: String,
    atime: String,
    ctime: String,
    birth: String,
    name: String,
}

const LONG_HEADERS: [&str; 14] = [
    "INODE", "MODE", "OCT", "LINKS", "OWNER", "GROUP", "SIZE", "BYTES", "BLOCKS", "MTIME", "ATIME",
    "CTIME", "BIRTH", "NAME",
];

/// Per-column alignment. `true` = right-align (numeric), `false` = left.
const LONG_RIGHT: [bool; 14] = [
    true, false, false, true, false, false, true, true, true, false, false, false, false, false,
];

impl LongRow {
    fn cells(&self) -> [&str; 14] {
        [
            &self.inode,
            &self.mode,
            &self.oct,
            &self.links,
            &self.owner,
            &self.group,
            &self.size,
            &self.bytes,
            &self.blocks,
            &self.mtime,
            &self.atime,
            &self.ctime,
            &self.birth,
            &self.name,
        ]
    }
}

/// Produce a tabular `ls -l`-on-steroids listing: one header row plus one
/// data row per path. Columns: inode, mode (symbolic), octal mode, links,
/// owner and group (resolved via `getpwuid_r`/`getgrgid_r`), size (human),
/// bytes, 512B blocks, mtime, atime, ctime, birth, name. Symlinks render
/// as `name -> target` in the NAME column. Column widths are computed once
/// across all rows so everything aligns. Unreadable paths render as
/// `?? <path>: <error>` lines after the table.
pub fn format_long_listing(paths: &[&Path]) -> Vec<String> {
    let mut rows: Vec<LongRow> = Vec::with_capacity(paths.len());
    let mut errors: Vec<String> = Vec::new();
    for path in paths {
        match fs::symlink_metadata(path) {
            Ok(md) => rows.push(make_long_row(path, &md)),
            Err(e) => errors.push(format!("?? {}: {e}", path.display())),
        }
    }

    if rows.is_empty() {
        return errors;
    }

    let widths = compute_column_widths(&rows);
    let mut out = Vec::with_capacity(rows.len() + errors.len() + 1);
    out.push(format_long_header(&widths));
    for row in &rows {
        out.push(format_long_row(row, &widths));
    }
    out.extend(errors);
    out
}

fn compute_column_widths(rows: &[LongRow]) -> [usize; 14] {
    use unicode_width::UnicodeWidthStr;
    let mut widths = [0usize; 14];
    for (i, h) in LONG_HEADERS.iter().enumerate() {
        widths[i] = h.width();
    }
    for row in rows {
        for (i, cell) in row.cells().iter().enumerate() {
            widths[i] = widths[i].max(cell.width());
        }
    }
    widths
}

fn format_long_header(widths: &[usize; 14]) -> String {
    let mut s = String::new();
    for (i, h) in LONG_HEADERS.iter().enumerate() {
        if i > 0 {
            s.push_str("  ");
        }
        write_cell(&mut s, h, widths[i], LONG_RIGHT[i]);
    }
    // Trim trailing whitespace from the last (left-aligned) column
    // so we don't render an oddly long header line.
    s.truncate(s.trim_end().len());
    s
}

fn format_long_row(row: &LongRow, widths: &[usize; 14]) -> String {
    let mut s = String::new();
    for (i, cell) in row.cells().iter().enumerate() {
        if i > 0 {
            s.push_str("  ");
        }
        write_cell(&mut s, cell, widths[i], LONG_RIGHT[i]);
    }
    s.truncate(s.trim_end().len());
    s
}

fn write_cell(s: &mut String, val: &str, width: usize, right: bool) {
    use unicode_width::UnicodeWidthStr;
    let pad = width.saturating_sub(val.width());
    if right {
        for _ in 0..pad {
            s.push(' ');
        }
        s.push_str(val);
    } else {
        s.push_str(val);
        for _ in 0..pad {
            s.push(' ');
        }
    }
}

#[cfg(unix)]
fn make_long_row(path: &Path, md: &fs::Metadata) -> LongRow {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let inode = md.ino().to_string();
    let mode = format_mode(md);
    let oct = format!("{:04o}", md.permissions().mode() & 0o7777);
    let links = md.nlink().to_string();
    let uid = md.uid();
    let owner = lookup_user_name(uid).unwrap_or_else(|| uid.to_string());
    let gid = md.gid();
    let group = lookup_group_name(gid).unwrap_or_else(|| gid.to_string());
    let size = format_size(md.len());
    let bytes = md.len().to_string();
    let blocks = md.blocks().to_string();
    let mtime = md
        .modified()
        .ok()
        .map_or_else(|| "-".to_string(), format_local_time);
    let atime = md
        .accessed()
        .ok()
        .map_or_else(|| "-".to_string(), format_local_time);
    let ctime = format_local_time_from_unix(md.ctime(), md.ctime_nsec());
    let birth = md
        .created()
        .ok()
        .map_or_else(|| "-".to_string(), format_local_time);
    let name = name_with_target(path, md);
    LongRow {
        inode,
        mode,
        oct,
        links,
        owner,
        group,
        size,
        bytes,
        blocks,
        mtime,
        atime,
        ctime,
        birth,
        name,
    }
}

#[cfg(not(unix))]
fn make_long_row(path: &Path, md: &fs::Metadata) -> LongRow {
    let mode = format_mode(md);
    let size = format_size(md.len());
    let bytes = md.len().to_string();
    let mtime = md
        .modified()
        .ok()
        .map_or_else(|| "-".to_string(), format_local_time);
    let name = name_with_target(path, md);
    LongRow {
        inode: "-".to_string(),
        mode,
        oct: "-".to_string(),
        links: "-".to_string(),
        owner: "-".to_string(),
        group: "-".to_string(),
        size,
        bytes,
        blocks: "-".to_string(),
        mtime,
        atime: "-".to_string(),
        ctime: "-".to_string(),
        birth: "-".to_string(),
        name,
    }
}

fn name_with_target(path: &Path, md: &fs::Metadata) -> String {
    let base = display_name(path, md);
    if md.file_type().is_symlink() {
        if let Ok(target) = fs::read_link(path) {
            return format!("{base} -> {}", target.display());
        }
    }
    base
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
fn lookup_user_name(uid: u32) -> Option<String> {
    uzers::get_user_by_uid(uid).map(|u| u.name().to_string_lossy().into_owned())
}

#[cfg(unix)]
fn lookup_group_name(gid: u32) -> Option<String> {
    uzers::get_group_by_gid(gid).map(|g| g.name().to_string_lossy().into_owned())
}

fn format_local_time(t: std::time::SystemTime) -> String {
    let secs = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => return "—".to_string(),
    };
    format_local_time_from_unix(secs, 0)
}

fn format_local_time_from_unix(secs: i64, _nsec: i64) -> String {
    let Ok(ts) = jiff::Timestamp::from_second(secs) else {
        return "—".to_string();
    };
    ts.to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
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
    fn format_size_picks_appropriate_unit() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1024), "1.0K");
        assert_eq!(format_size(1536), "1.5K");
        assert_eq!(format_size(10 * 1024), "10K");
        assert_eq!(format_size(1024 * 1024), "1.0M");
        assert_eq!(format_size(12 * 1024 * 1024), "12M");
    }

    #[cfg(unix)]
    #[test]
    fn long_listing_emits_table_with_header_and_one_row_per_file() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("hello.txt");
        let b = tmp.path().join("world.md");
        File::create(&a).unwrap().write_all(b"hi").unwrap();
        File::create(&b).unwrap().write_all(b"yo!").unwrap();

        let lines = format_long_listing(&[&a, &b]);
        // 1 header + 2 data rows.
        assert_eq!(lines.len(), 3, "got: {lines:?}");

        // Header has all expected column names.
        let header = &lines[0];
        for col in [
            "INODE", "MODE", "OCT", "LINKS", "OWNER", "GROUP", "SIZE", "BYTES", "BLOCKS", "MTIME",
            "ATIME", "CTIME", "BIRTH", "NAME",
        ] {
            assert!(header.contains(col), "header missing {col}: {header}");
        }

        // Data rows include the filenames and concrete bytes/mode.
        assert!(lines[1].contains("hello.txt"), "row 1: {}", lines[1]);
        assert!(lines[2].contains("world.md"), "row 2: {}", lines[2]);
        assert!(lines[1].contains("-rw"), "no mode in row 1: {}", lines[1]);
        // 2-byte file shows up in BYTES column literally as "2".
        assert!(lines[1].split_whitespace().any(|s| s == "2"));
        // 3-byte file shows up in BYTES column literally as "3".
        assert!(lines[2].split_whitespace().any(|s| s == "3"));
    }

    #[cfg(unix)]
    #[test]
    fn long_listing_columns_align_across_rows() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("longer_name.txt");
        File::create(&a).unwrap();
        File::create(&b).unwrap();

        let lines = format_long_listing(&[&a, &b]);
        // The MODE column is at the same byte offset on every row, since
        // the INODE column to its left is right-aligned to a fixed width.
        let mode_col_offset = lines[0].find("MODE").unwrap();
        // Both data rows should have a mode glyph (`-` or `d`) at that offset.
        for row in &lines[1..] {
            let ch = row.as_bytes().get(mode_col_offset).copied().unwrap_or(b' ');
            assert!(
                matches!(ch, b'-' | b'd' | b'l' | b'b' | b'c' | b'p' | b's'),
                "mode column misaligned in row at offset {mode_col_offset}: {row:?}",
            );
        }
    }

    #[test]
    fn long_listing_unreadable_path_appends_error_line() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let lines = format_long_listing(&[&missing]);
        assert!(!lines.is_empty());
        // Only errors -> no header line; the error itself is the first line.
        assert!(
            lines[0].starts_with("?? "),
            "expected error line, got {:?}",
            lines[0]
        );
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

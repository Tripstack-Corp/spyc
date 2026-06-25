//! The `L` long-listing formatter — an `ls -l`-on-steroids table.
//!
//! Produces one header row plus one data row per path: inode, symbolic + octal
//! mode, links, owner/group (resolved via `uzers`), human + raw size, blocks,
//! and m/a/c/birth times. Column widths are computed once across all rows so
//! everything aligns. Extracted verbatim from `fs/ops.rs` (800-LoC campaign).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
/// owner and group (resolved via `uzers`), size (human),
/// bytes, 512B blocks, mtime, atime, ctime, birth, name. Symlinks render
/// as `name -> target` in the NAME column. Column widths are computed once
/// across all rows so everything aligns. Unreadable paths render as
/// `?? <path>: <error>` lines after the table.
pub fn format_long_listing(paths: &[&Path]) -> Vec<String> {
    let mut rows: Vec<LongRow> = Vec::with_capacity(paths.len());
    let mut errors: Vec<String> = Vec::new();
    // Memoize owner/group name resolution per uid/gid: `uzers::get_user_by_uid`
    // / `get_group_by_gid` are NSS lookups, which on an LDAP/AD-backed machine
    // hit the network. A directory's files almost all share a handful of
    // uids/gids, so caching collapses `L` from one round-trip *per row* (which
    // could stall seconds-to-minutes on a big listing) to one *per distinct id*.
    let mut uid_cache: HashMap<u32, String> = HashMap::new();
    let mut gid_cache: HashMap<u32, String> = HashMap::new();
    for path in paths {
        match fs::symlink_metadata(path) {
            Ok(md) => rows.push(make_long_row(path, &md, &mut uid_cache, &mut gid_cache)),
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
fn make_long_row(
    path: &Path,
    md: &fs::Metadata,
    uid_cache: &mut HashMap<u32, String>,
    gid_cache: &mut HashMap<u32, String>,
) -> LongRow {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let inode = md.ino().to_string();
    let mode = format_mode(md);
    let oct = format!("{:04o}", md.permissions().mode() & 0o7777);
    let links = md.nlink().to_string();
    let uid = md.uid();
    let owner = uid_cache
        .entry(uid)
        .or_insert_with(|| lookup_user_name(uid).unwrap_or_else(|| uid.to_string()))
        .clone();
    let gid = md.gid();
    let group = gid_cache
        .entry(gid)
        .or_insert_with(|| lookup_group_name(gid).unwrap_or_else(|| gid.to_string()))
        .clone();
    let size = crate::fs::ops::format_size(md.len());
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
fn make_long_row(
    path: &Path,
    md: &fs::Metadata,
    _uid_cache: &mut HashMap<u32, String>,
    _gid_cache: &mut HashMap<u32, String>,
) -> LongRow {
    let mode = format_mode(md);
    let size = crate::fs::ops::format_size(md.len());
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
    if md.file_type().is_symlink()
        && let Ok(target) = fs::read_link(path)
    {
        return format!("{base} -> {}", target.display());
    }
    base
}

fn display_name(path: &Path, md: &fs::Metadata) -> String {
    let base = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    // Same `ls -F` decoration the directory listing uses (dir `/`, exec `*`),
    // via the shared classifier rather than a second hand-rolled copy.
    format!(
        "{base}{}",
        crate::fs::entry::kind_suffix(crate::fs::entry::classify(md))
    )
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
        return 'd';
    }
    if ft.is_symlink() {
        return 'l';
    }
    // Special files render with their `ls -l` glyph rather than collapsing to
    // a plain `-` (which mislabels FIFOs/sockets/devices as regular files).
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        if ft.is_fifo() {
            return 'p';
        }
        if ft.is_socket() {
            return 's';
        }
        if ft.is_char_device() {
            return 'c';
        }
        if ft.is_block_device() {
            return 'b';
        }
    }
    '-'
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn kind_char_detects_socket_not_regular_file() {
        // A unix-domain socket is a non-regular file; kind_char must report
        // `s`, not collapse it to `-` like a plain file.
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let md = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(kind_char(&md), 's');
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

    #[cfg(unix)]
    #[test]
    fn long_listing_memoizes_owner_group_per_id() {
        // Two files sharing the current uid/gid: the second row is a cache hit,
        // and must resolve to the same owner/group name the direct NSS lookup
        // gives. Guards the per-id memoization added for the LDAP-stall fix.
        use std::os::unix::fs::MetadataExt;
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        File::create(&a).unwrap();
        File::create(&b).unwrap();

        let lines = format_long_listing(&[&a, &b]);
        assert_eq!(lines.len(), 3, "header + 2 rows: {lines:?}");

        let md = std::fs::metadata(&a).unwrap();
        let owner = lookup_user_name(md.uid()).unwrap_or_else(|| md.uid().to_string());
        let group = lookup_group_name(md.gid()).unwrap_or_else(|| md.gid().to_string());
        for row in &lines[1..] {
            assert!(row.contains(&owner), "row missing owner {owner}: {row}");
            assert!(row.contains(&group), "row missing group {group}: {row}");
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
}

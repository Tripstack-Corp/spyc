use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::entry::{Entry, EntryKind};

/// Sort order for the file listing. Dirs-first grouping is always applied;
/// this controls the secondary sort within each group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    Name,
    Size,
    Mtime,
    Ext,
}

impl SortMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "name" => Some(Self::Name),
            "size" => Some(Self::Size),
            "mtime" | "time" | "date" => Some(Self::Mtime),
            "ext" | "extension" | "type" => Some(Self::Ext),
            _ => None,
        }
    }

    /// Cycle to the next sort mode. Order matches the docs / status
    /// bar: Name → Size → Mtime → Ext → Name. Bound to `S` in the
    /// resolver.
    pub const fn cycle_next(self) -> Self {
        match self {
            Self::Name => Self::Size,
            Self::Size => Self::Mtime,
            Self::Mtime => Self::Ext,
            Self::Ext => Self::Name,
        }
    }
}

impl fmt::Display for SortMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name => write!(f, "name"),
            Self::Size => write!(f, "size"),
            Self::Mtime => write!(f, "mtime"),
            Self::Ext => write!(f, "ext"),
        }
    }
}

/// Dirs first, then executables, files, symlinks, other.
const fn kind_rank(k: EntryKind) -> u8 {
    match k {
        EntryKind::Dir => 0,
        EntryKind::Executable => 1,
        EntryKind::File => 2,
        EntryKind::Symlink => 3,
        EntryKind::Other => 4,
    }
}

fn ext_of(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, ext)| ext)
}

/// A snapshot of one directory's entries.
#[derive(Debug, Clone)]
pub struct Listing {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    /// True when the on-disk directory had more than `MAX_ENTRIES`
    /// items and we stopped early. Caller surfaces this as a flash
    /// so the user knows the listing isn't the full picture.
    pub truncated: bool,
}

/// Hard cap on entries Listing::read will materialize. A user
/// reported entering a tmp directory with so many entries that spyc
/// hung and they had to kill the terminal — every entry costs a
/// `stat()` syscall plus a sort comparison, so 1M entries can spend
/// minutes blocking the event loop on a slow filesystem. Most real
/// directories the user wants to navigate are well under this cap;
/// when we hit it, `truncated` is set so the caller can surface a
/// flash and the user can `R` / `:!find` / climb out instead of
/// waiting for the read to finish.
pub const MAX_ENTRIES: usize = 50_000;

impl Listing {
    /// An empty listing for a given directory (used when the dir isn't readable).
    pub const fn empty(dir: PathBuf) -> Self {
        Self {
            dir,
            entries: Vec::new(),
            truncated: false,
        }
    }

    pub fn read<P: AsRef<Path>>(dir: P) -> Result<Self> {
        Self::read_capped(dir, MAX_ENTRIES)
    }

    /// Same as [`read`] but with a caller-supplied cap. Public for
    /// tests; production code goes through `read` (with `MAX_ENTRIES`).
    pub fn read_capped<P: AsRef<Path>>(dir: P, cap: usize) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let mut entries: Vec<Entry> = Vec::new();
        let mut truncated = false;
        let rd = std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {}", dir.display()))?;
        for item in rd {
            if entries.len() >= cap {
                truncated = true;
                break;
            }
            let Ok(item) = item else { continue };
            if let Ok(e) = Entry::from_dir_entry(&item) {
                entries.push(e);
            }
        }
        let mut listing = Self {
            dir,
            entries,
            truncated,
        };
        listing.sort(SortMode::Name, false);
        Ok(listing)
    }

    /// Re-sort entries in-place. `reversed` inverts the per-mode
    /// natural direction (Name/Ext ascending, Size/Mtime descending
    /// = largest/newest first) — dirs-first grouping is always
    /// preserved.
    pub fn sort(&mut self, mode: SortMode, reversed: bool) {
        // Decorate each entry with its precomputed sort key, sort, then
        // strip the keys. The naive comparator called `to_ascii_lowercase()`
        // (a fresh allocation) on every comparison — 2-4 Strings per call —
        // so a 50k-entry directory cost ~1.5M short-lived allocations per
        // sort, on the event loop. Precomputing keys makes it O(n) instead.
        let mut decorated: Vec<(SortKey, Entry)> = std::mem::take(&mut self.entries)
            .into_iter()
            .map(|e| (SortKey::new(&e, mode), e))
            .collect();
        decorated.sort_by(|(ka, a), (kb, b)| {
            kind_rank(a.kind).cmp(&kind_rank(b.kind)).then_with(|| {
                let primary = ka.cmp_primary(kb);
                if reversed { primary.reverse() } else { primary }
            })
        });
        self.entries = decorated.into_iter().map(|(_, e)| e).collect();
    }
}

/// A precomputed per-entry sort key. Built once per entry (so the
/// `to_ascii_lowercase()` allocations happen O(n) times, not on every
/// comparison). Within a single `sort` call `mode` is fixed, so every key
/// is the same variant — the mixed arms in `cmp_primary` are unreachable.
enum SortKey {
    Name(String), // lowercased name
    Size(u64),
    Mtime(std::time::SystemTime),
    Ext(String, String), // (lowercased ext, lowercased name)
}

impl SortKey {
    fn new(e: &Entry, mode: SortMode) -> Self {
        match mode {
            SortMode::Name => Self::Name(e.name.to_ascii_lowercase()),
            SortMode::Size => Self::Size(e.size),
            SortMode::Mtime => Self::Mtime(e.mtime),
            SortMode::Ext => Self::Ext(
                ext_of(&e.name).to_ascii_lowercase(),
                e.name.to_ascii_lowercase(),
            ),
        }
    }

    /// The per-mode "natural direction" comparison (before `reversed` is
    /// applied): Name/Ext ascending, Size/Mtime descending.
    fn cmp_primary(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Name(a), Self::Name(b)) => a.cmp(b),
            (Self::Size(a), Self::Size(b)) => b.cmp(a), // largest first
            (Self::Mtime(a), Self::Mtime(b)) => b.cmp(a), // newest first
            (Self::Ext(ea, na), Self::Ext(eb, nb)) => ea.cmp(eb).then_with(|| na.cmp(nb)),
            _ => Ordering::Equal, // mixed variants never occur within one sort
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn read_capped_truncates_when_over_cap() {
        let tmp = tempdir().unwrap();
        // 8 files; cap at 5 so we hit the truncation branch without
        // burning real test time on 50k stat() calls.
        for i in 0..8 {
            File::create(tmp.path().join(format!("f{i:02}"))).unwrap();
        }
        let listing = Listing::read_capped(tmp.path(), 5).unwrap();
        assert_eq!(listing.entries.len(), 5);
        assert!(listing.truncated);
    }

    #[test]
    fn read_capped_does_not_truncate_under_cap() {
        let tmp = tempdir().unwrap();
        for i in 0..3 {
            File::create(tmp.path().join(format!("f{i:02}"))).unwrap();
        }
        let listing = Listing::read_capped(tmp.path(), 100).unwrap();
        assert_eq!(listing.entries.len(), 3);
        assert!(!listing.truncated);
    }

    #[test]
    fn empty_listing_is_not_truncated() {
        let l = Listing::empty(std::path::PathBuf::from("/tmp"));
        assert!(!l.truncated);
    }

    fn mk(name: &str, kind: EntryKind, size: u64, mtime_secs: u64) -> Entry {
        Entry {
            path: PathBuf::from(name),
            name: name.to_string(),
            kind,
            size,
            mtime: std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime_secs),
        }
    }

    fn names(l: &Listing) -> Vec<&str> {
        l.entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn fixture() -> Listing {
        // Intentionally unsorted, mixed kinds + mixed case.
        Listing {
            dir: PathBuf::from("/tmp"),
            entries: vec![
                mk("Banana.txt", EntryKind::File, 30, 100),
                mk("apple.rs", EntryKind::File, 10, 300),
                mk("zeta", EntryKind::Dir, 0, 200),
                mk("Alpha", EntryKind::Dir, 0, 400),
                mk("cherry.rs", EntryKind::File, 20, 500),
            ],
            truncated: false,
        }
    }

    #[test]
    fn sort_name_dirs_first_case_insensitive() {
        let mut l = fixture();
        l.sort(SortMode::Name, false);
        // Dirs first (Alpha, zeta), then files ascending case-insensitively.
        assert_eq!(
            names(&l),
            ["Alpha", "zeta", "apple.rs", "Banana.txt", "cherry.rs"]
        );
    }

    #[test]
    fn sort_name_reversed_inverts_within_group_not_grouping() {
        let mut l = fixture();
        l.sort(SortMode::Name, true);
        // Still dirs-first, but each group's secondary order is reversed.
        assert_eq!(
            names(&l),
            ["zeta", "Alpha", "cherry.rs", "Banana.txt", "apple.rs"]
        );
    }

    #[test]
    fn sort_size_largest_first_within_group() {
        let mut l = fixture();
        l.sort(SortMode::Size, false);
        // Dirs first (size 0, tie broken by sort stability), then files largest-first.
        assert_eq!(
            names(&l),
            ["zeta", "Alpha", "Banana.txt", "cherry.rs", "apple.rs"]
        );
    }

    #[test]
    fn sort_mtime_newest_first_within_group() {
        let mut l = fixture();
        l.sort(SortMode::Mtime, false);
        // Dirs first by mtime desc (Alpha=400 > zeta=200), then files newest-first.
        assert_eq!(
            names(&l),
            ["Alpha", "zeta", "cherry.rs", "apple.rs", "Banana.txt"]
        );
    }

    #[test]
    fn sort_ext_then_name() {
        let mut l = fixture();
        l.sort(SortMode::Ext, false);
        // Dirs first (no ext). Files: ".rs" before ".txt"; within .rs by name.
        assert_eq!(
            names(&l),
            ["Alpha", "zeta", "apple.rs", "cherry.rs", "Banana.txt"]
        );
    }
}

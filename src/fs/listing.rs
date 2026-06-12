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
        self.entries.sort_by(|a, b| {
            kind_rank(a.kind).cmp(&kind_rank(b.kind)).then_with(|| {
                let primary = match mode {
                    SortMode::Name => a
                        .name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase()),
                    SortMode::Size => b.size.cmp(&a.size), // largest first
                    SortMode::Mtime => b.mtime.cmp(&a.mtime), // newest first
                    SortMode::Ext => {
                        let ea = ext_of(&a.name).to_ascii_lowercase();
                        let eb = ext_of(&b.name).to_ascii_lowercase();
                        ea.cmp(&eb).then_with(|| {
                            a.name
                                .to_ascii_lowercase()
                                .cmp(&b.name.to_ascii_lowercase())
                        })
                    }
                };
                if reversed { primary.reverse() } else { primary }
            })
        });
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
}

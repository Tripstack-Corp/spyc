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
}

impl Listing {
    /// An empty listing for a given directory (used when the dir isn't readable).
    pub const fn empty(dir: PathBuf) -> Self {
        Self {
            dir,
            entries: Vec::new(),
        }
    }

    pub fn read<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let mut entries: Vec<Entry> = Vec::new();
        let rd = std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {}", dir.display()))?;
        for item in rd {
            let Ok(item) = item else { continue };
            if let Ok(e) = Entry::from_dir_entry(&item) {
                entries.push(e);
            }
        }
        let mut listing = Self { dir, entries };
        listing.sort(SortMode::Name);
        Ok(listing)
    }

    /// Re-sort entries in-place.
    pub fn sort(&mut self, mode: SortMode) {
        self.entries.sort_by(|a, b| {
            kind_rank(a.kind)
                .cmp(&kind_rank(b.kind))
                .then_with(|| match mode {
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
                })
        });
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

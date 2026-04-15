use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::entry::{Entry, EntryKind};

/// A snapshot of one directory's entries.
#[derive(Debug, Clone)]
pub struct Listing {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
}

impl Listing {
    pub fn read<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let mut entries: Vec<Entry> = Vec::new();
        let rd = std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {}", dir.display()))?;
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };
            if let Ok(e) = Entry::from_dir_entry(item) {
                entries.push(e);
            }
        }
        // spy-ish: dotfiles mixed in, dirs sort with files, case-insensitive by name.
        entries.sort_by(|a, b| {
            // Directories first, then executables, then files, then links/other.
            fn rank(k: EntryKind) -> u8 {
                match k {
                    EntryKind::Dir => 0,
                    EntryKind::Executable => 1,
                    EntryKind::File => 2,
                    EntryKind::Symlink => 3,
                    EntryKind::Other => 4,
                }
            }
            rank(a.kind)
                .cmp(&rank(b.kind))
                .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
        });
        Ok(Self { dir, entries })
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

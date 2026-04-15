use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Per-directory multi-select. Path-keyed so it is resilient to the listing
/// being re-read (new/removed entries do not desynchronize the selection).
#[derive(Debug, Default, Clone)]
pub struct Picks {
    set: HashSet<PathBuf>,
}

impl Picks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.set.clear();
    }

    pub fn toggle(&mut self, path: &Path) {
        if !self.set.remove(path) {
            self.set.insert(path.to_path_buf());
        }
    }

    pub fn insert(&mut self, path: &Path) {
        self.set.insert(path.to_path_buf());
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, path: &Path) {
        self.set.remove(path);
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.set.contains(path)
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.set.iter()
    }
}

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The cross-directory "taken files" collection. Uses a BTreeSet so the
/// inventory view has a deterministic order.
#[derive(Debug, Default, Clone)]
pub struct Inventory {
    set: BTreeSet<PathBuf>,
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn add(&mut self, path: PathBuf) {
        self.set.insert(path);
    }

    pub fn extend<I: IntoIterator<Item = PathBuf>>(&mut self, iter: I) {
        for p in iter {
            self.set.insert(p);
        }
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        self.set.remove(path)
    }

    pub fn clear(&mut self) {
        self.set.clear();
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.set.contains(path)
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.set.iter()
    }
}

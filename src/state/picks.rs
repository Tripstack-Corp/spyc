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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn new_is_empty() {
        let p = Picks::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn toggle_adds_and_removes() {
        let mut p = Picks::new();
        let path = Path::new("/tmp/a.txt");
        p.toggle(path);
        assert!(p.contains(path));
        assert_eq!(p.len(), 1);
        p.toggle(path);
        assert!(!p.contains(path));
        assert!(p.is_empty());
    }

    #[test]
    fn insert_is_idempotent() {
        let mut p = Picks::new();
        let path = Path::new("/tmp/b.txt");
        p.insert(path);
        p.insert(path);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn remove_nonexistent_is_noop() {
        let mut p = Picks::new();
        p.remove(Path::new("/nope"));
        assert!(p.is_empty());
    }

    #[test]
    fn clear_empties_all() {
        let mut p = Picks::new();
        p.insert(Path::new("/a"));
        p.insert(Path::new("/b"));
        p.insert(Path::new("/c"));
        assert_eq!(p.len(), 3);
        p.clear();
        assert!(p.is_empty());
    }

    #[test]
    fn contains_is_path_exact() {
        let mut p = Picks::new();
        p.insert(Path::new("/tmp/foo"));
        assert!(p.contains(Path::new("/tmp/foo")));
        assert!(!p.contains(Path::new("/tmp/bar")));
        assert!(!p.contains(Path::new("/tmp/foo.txt")));
    }
}

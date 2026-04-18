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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn new_is_empty() {
        let inv = Inventory::new();
        assert!(inv.is_empty());
        assert_eq!(inv.len(), 0);
    }

    #[test]
    fn add_and_contains() {
        let mut inv = Inventory::new();
        inv.add(PathBuf::from("/tmp/a"));
        assert!(inv.contains(Path::new("/tmp/a")));
        assert_eq!(inv.len(), 1);
    }

    #[test]
    fn extend_adds_multiple() {
        let mut inv = Inventory::new();
        inv.extend(vec![PathBuf::from("/b"), PathBuf::from("/a")]);
        assert_eq!(inv.len(), 2);
    }

    #[test]
    fn paths_are_sorted() {
        let mut inv = Inventory::new();
        inv.extend(vec![
            PathBuf::from("/z"),
            PathBuf::from("/a"),
            PathBuf::from("/m"),
        ]);
        let paths: Vec<_> = inv.paths().collect();
        assert_eq!(
            paths,
            vec![
                &PathBuf::from("/a"),
                &PathBuf::from("/m"),
                &PathBuf::from("/z"),
            ]
        );
    }

    #[test]
    fn remove_returns_whether_present() {
        let mut inv = Inventory::new();
        inv.add(PathBuf::from("/x"));
        assert!(inv.remove(Path::new("/x")));
        assert!(!inv.remove(Path::new("/x")));
    }

    #[test]
    fn clear_empties() {
        let mut inv = Inventory::new();
        inv.extend(vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        inv.clear();
        assert!(inv.is_empty());
    }

    #[test]
    fn duplicates_are_ignored() {
        let mut inv = Inventory::new();
        inv.add(PathBuf::from("/same"));
        inv.add(PathBuf::from("/same"));
        assert_eq!(inv.len(), 1);
    }
}

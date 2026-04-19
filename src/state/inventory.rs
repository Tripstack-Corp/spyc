use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const INVENTORY_FILE: &str = "inventory";

fn state_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        Some(PathBuf::from(xdg).join("spyc"))
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/spyc"))
    }
}

/// The cross-directory "taken files" collection. Uses a BTreeSet so the
/// inventory view has a deterministic order. Persisted to disk on every
/// change so it survives restarts.
#[derive(Debug, Default, Clone)]
pub struct Inventory {
    set: BTreeSet<PathBuf>,
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load inventory from disk. Returns an empty inventory if the file
    /// doesn't exist or can't be read.
    pub fn load() -> Self {
        let Some(path) = state_dir().map(|d| d.join(INVENTORY_FILE)) else {
            return Self::new();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::new();
        };
        let set: BTreeSet<PathBuf> = text
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        Self { set }
    }

    /// Save inventory to disk (best-effort).
    fn save(&self) {
        let Some(dir) = state_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(INVENTORY_FILE);
        let text: String = self
            .set
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(&path, if text.is_empty() { text } else { text + "\n" });
    }

    #[allow(dead_code)]
    pub fn add(&mut self, path: PathBuf) {
        self.set.insert(path);
        self.save();
    }

    pub fn extend<I: IntoIterator<Item = PathBuf>>(&mut self, iter: I) {
        for p in iter {
            self.set.insert(p);
        }
        self.save();
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let removed = self.set.remove(path);
        if removed {
            self.save();
        }
        removed
    }

    pub fn clear(&mut self) {
        self.set.clear();
        self.save();
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

//! Frecency-based directory ranking for the J (jump) prompt.
//!
//! Tracks visited directories with a frequency x recency score (inspired by
//! zoxide). Persisted to `$XDG_STATE_HOME/spyc/frecency.json` (or
//! `$HOME/.local/state/spyc/frecency.json`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    count: u32,
    last_visit: u64, // epoch seconds
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Frecency {
    entries: HashMap<PathBuf, Entry>,
}

impl Frecency {
    /// Score a directory based on visit count and recency.
    /// Uses zoxide-style tiered decay: recent visits score higher.
    fn score(&self, path: &Path, now: u64) -> f64 {
        let Some(e) = self.entries.get(path) else {
            return 0.0;
        };
        let age_hrs = now.saturating_sub(e.last_visit) as f64 / 3600.0;
        let recency = if age_hrs < 1.0 {
            4.0
        } else if age_hrs < 24.0 {
            2.0
        } else if age_hrs < 168.0 {
            1.0 // 1 week
        } else {
            0.5
        };
        f64::from(e.count) * recency
    }

    /// Record a visit to `dir`. Bumps count and timestamp, prunes if
    /// over the entry cap, and persists to disk.
    pub fn record(&mut self, dir: &Path) {
        let now = epoch_secs();
        let e = self.entries.entry(dir.to_path_buf()).or_insert(Entry {
            count: 0,
            last_visit: 0,
        });
        e.count += 1;
        e.last_visit = now;

        if self.entries.len() > MAX_ENTRIES {
            self.prune(now);
        }
        let _ = self.save();
    }

    /// Remove lowest-scored entries to bring the count back to `MAX_ENTRIES`.
    fn prune(&mut self, now: u64) {
        let mut scored: Vec<_> = self
            .entries
            .keys()
            .map(|p| (p.clone(), self.score(p, now)))
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let to_remove = self.entries.len().saturating_sub(MAX_ENTRIES);
        for (p, _) in scored.into_iter().take(to_remove) {
            self.entries.remove(&p);
        }
    }

    /// Search for directories whose path contains `fragment` (case-insensitive),
    /// ranked by frecency score (highest first).
    pub fn search(&self, fragment: &str) -> Vec<PathBuf> {
        let now = epoch_secs();
        let frag_lower = fragment.to_lowercase();
        let mut hits: Vec<(PathBuf, f64)> = self
            .entries
            .keys()
            .filter(|p| p.to_string_lossy().to_lowercase().contains(&frag_lower))
            .map(|p| (p.clone(), self.score(p, now)))
            .collect();
        hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        hits.into_iter().map(|(p, _)| p).collect()
    }

    /// Number of tracked directories.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    // ── Persistence ─────────────────────────────────────────────

    fn disk_path() -> Option<PathBuf> {
        state_dir().map(|d| d.join("frecency.json"))
    }

    /// Best-effort load. Missing or malformed files silently yield empty.
    pub fn load() -> Self {
        let Some(path) = Self::disk_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Serialize and write. Creates the parent directory if needed.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::disk_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string(self).unwrap_or_default();
        std::fs::write(&path, text)
    }
}

fn state_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join("spyc"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/spyc"))
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frecency() -> Frecency {
        Frecency::default()
    }

    #[test]
    fn record_and_score() {
        let mut f = make_frecency();
        // Inject directly to avoid disk writes in tests.
        let now = epoch_secs();
        f.entries.insert(
            PathBuf::from("/tmp/a"),
            Entry {
                count: 5,
                last_visit: now,
            },
        );
        let s = f.score(Path::new("/tmp/a"), now);
        // Visited just now → recency 4.0, count 5 → score 20.0
        assert!((s - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn record_bumps_count() {
        let mut f = make_frecency();
        let now = epoch_secs();
        f.entries.insert(
            PathBuf::from("/tmp/b"),
            Entry {
                count: 1,
                last_visit: now,
            },
        );
        // Simulate a second visit.
        let e = f.entries.get_mut(Path::new("/tmp/b")).unwrap();
        e.count += 1;
        assert_eq!(e.count, 2);
    }

    #[test]
    fn prune_over_cap() {
        let mut f = make_frecency();
        let now = epoch_secs();
        // Insert MAX_ENTRIES + 10 entries.
        for i in 0..MAX_ENTRIES + 10 {
            f.entries.insert(
                PathBuf::from(format!("/tmp/dir{i}")),
                Entry {
                    count: 1,
                    last_visit: now,
                },
            );
        }
        assert!(f.entries.len() > MAX_ENTRIES);
        f.prune(now);
        assert_eq!(f.entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn search_fragment() {
        let mut f = make_frecency();
        let now = epoch_secs();
        f.entries.insert(
            PathBuf::from("/home/user/src/spyc"),
            Entry {
                count: 10,
                last_visit: now,
            },
        );
        f.entries.insert(
            PathBuf::from("/home/user/src/spy-tools"),
            Entry {
                count: 2,
                last_visit: now,
            },
        );
        f.entries.insert(
            PathBuf::from("/tmp/unrelated"),
            Entry {
                count: 50,
                last_visit: now,
            },
        );
        let results = f.search("spy");
        assert_eq!(results.len(), 2);
        // Higher count → higher score → first result.
        assert_eq!(results[0], PathBuf::from("/home/user/src/spyc"));
    }

    #[test]
    fn search_case_insensitive() {
        let mut f = make_frecency();
        let now = epoch_secs();
        f.entries.insert(
            PathBuf::from("/home/user/Documents"),
            Entry {
                count: 3,
                last_visit: now,
            },
        );
        let results = f.search("doc");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], PathBuf::from("/home/user/Documents"));
    }

    #[test]
    fn roundtrip_json() {
        let mut f = make_frecency();
        let now = epoch_secs();
        f.entries.insert(
            PathBuf::from("/tmp/roundtrip"),
            Entry {
                count: 7,
                last_visit: now,
            },
        );
        let json = serde_json::to_string(&f).unwrap();
        let loaded: Frecency = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        let e = loaded.entries.get(Path::new("/tmp/roundtrip")).unwrap();
        assert_eq!(e.count, 7);
    }
}

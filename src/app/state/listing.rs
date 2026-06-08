//! `AppState` directory listing + navigation: row rebuild, temp filter,
//! refresh, and chdir/change_dir/climb. Split from `state` verbatim.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::fs::Listing;
use crate::state::Cursor;

use crate::app::{Effect, Matcher, RowData, View, row_from_entry};

use super::AppState;

use super::format_age;

impl AppState {
    pub fn focus_on_path(&mut self, path: &Path) {
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.cursor.index = i;
        }
    }

    pub fn rebuild_rows(&mut self) {
        self.list_generation = self.list_generation.wrapping_add(1);
        self.rows = match self.view {
            View::Dir => {
                let base: Vec<RowData> = self
                    .listing
                    .entries
                    .iter()
                    .filter(|e| !self.masks.hides(&e.name))
                    .map(row_from_entry)
                    .collect();
                self.apply_temp_filter(base)
            }
            View::Inventory => self
                .inventory
                .items()
                .map(|item| RowData {
                    path: item.orig_path.clone(),
                    display: format!(
                        "{}  ← {}",
                        item.filename,
                        item.orig_path.parent().unwrap_or(Path::new("/")).display()
                    ),
                    kind: crate::fs::EntryKind::File,
                })
                .collect(),
            View::Graveyard => self
                .graveyard
                .iter()
                .map(|e| {
                    let glyph = match e.kind {
                        crate::state::graveyard::EntryKind::File => "[f]",
                        crate::state::graveyard::EntryKind::Dir => "[d]",
                        crate::state::graveyard::EntryKind::Symlink => "[l]",
                    };
                    let parent = e
                        .orig_path
                        .parent()
                        .map_or_else(|| "/".to_string(), |p| p.display().to_string());
                    let count_tag = if matches!(e.kind, crate::state::graveyard::EntryKind::Dir)
                        && e.file_count > 0
                    {
                        format!(" ({} files)", e.file_count)
                    } else {
                        String::new()
                    };
                    let age = format_age(e.timestamp);
                    let kind = match e.kind {
                        crate::state::graveyard::EntryKind::Dir => crate::fs::EntryKind::Dir,
                        _ => crate::fs::EntryKind::File,
                    };
                    RowData {
                        path: e.orig_path.clone(),
                        display: format!("{glyph} {}{count_tag} ({age})  ← {parent}", e.filename),
                        kind,
                    }
                })
                .collect(),
        };
        self.cursor.clamp(self.rows.len());
    }

    pub fn apply_temp_filter(&self, rows: Vec<RowData>) -> Vec<RowData> {
        let Some(ref pattern) = self.temp_filter else {
            return rows;
        };
        if pattern == "!" {
            rows.into_iter()
                .filter(|r| self.picks.contains(&r.path))
                .collect()
        } else if pattern == "h" {
            // Harpoon filter — keep entries whose absolute path is
            // in the project's harpoon set (slot paths plus all
            // their ancestor directories). Empty set → empty list.
            rows.into_iter()
                .filter(|r| self.harpoon_filter_set.contains(&r.path))
                .collect()
        } else if pattern == "git" {
            // Show only entries that appear in `git status` with a
            // non-Clean state. `git_files` keys files by basename and
            // also marks parent directories that contain changes
            // (basename + trailing `/`), so directories show up too —
            // useful for navigating into a subtree with edits.
            rows.into_iter()
                .filter(|r| {
                    self.git
                        .files
                        .get(&r.display)
                        .copied()
                        .is_some_and(|s| !s.is_clean())
                })
                .collect()
        } else {
            let matcher = Matcher::build(pattern);
            rows.into_iter()
                .filter(|r| matcher.matches(&r.display))
                .collect()
        }
    }

    pub fn refresh_listing(&mut self) {
        match Listing::read(&self.listing.dir) {
            Ok(new) => {
                self.listing = new;
                // Refresh the top-bar branch/dirty string too — without
                // this the bar stays on `main` after edits and only
                // updates when the user changes directories. Event-
                // driven refresh would normally invalidate the raw
                // cache (file mtimes moved but `.git/index` may not
                // have — and we need fresh content for ` M`
                // markers).
                //
                // But: on huge trees, `git status` is 200-500 ms per
                // spawn, and an active filesystem (claude writing
                // findings, build outputs, even some IDE auto-saves)
                // can trip `refresh_listing` every 3 s — burning the
                // worker thread nonstop. Throttle the invalidation
                // (huge: 10 s, small: 1 s). The 1 Hz / 10 Hz safety
                // poll in `refresh_git_state` still catches
                // `.git/index` changes immediately; the only
                // trade-off is a small lag in working-tree ` M`
                // markers for edits within the throttle window.
                let throttle = if self.git_cache.is_huge_tree {
                    std::time::Duration::from_secs(10)
                } else {
                    std::time::Duration::from_secs(1)
                };
                let should_invalidate = self
                    .git_cache
                    .last_git_invalidation
                    .is_none_or(|t| t.elapsed() >= throttle);
                if should_invalidate {
                    self.git_cache.git_status_cache = None;
                    self.git_cache.last_git_invalidation = Some(std::time::Instant::now());
                }
                let dir = self.listing.dir.clone();
                let new_git_files = self.git_file_statuses_cached(&dir);
                let new_git_info = self.compute_git_info_fast();
                let mut new_keys: Vec<&str> = new_git_files.keys().map(String::as_str).collect();
                new_keys.sort_unstable();
                crate::spyc_debug!(
                    "refresh_listing: dir={} git_info: {:?} → {:?}, git_files: {} → {} (new={:?})",
                    self.listing.dir.display(),
                    self.git.info,
                    new_git_info,
                    self.git.files.len(),
                    new_git_files.len(),
                    new_keys,
                );
                self.git.set(new_git_info, new_git_files);
                self.rebuild_rows();
            }
            Err(e) => {
                crate::spyc_debug!(
                    "refresh_listing: Listing::read({}) failed: {e}",
                    self.listing.dir.display(),
                );
            }
        }
    }

    pub fn chdir(&mut self, path: &Path) -> Result<()> {
        let canonical = std::fs::canonicalize(path)?;
        let new_listing = Listing::read(&canonical)?;
        if self.listing.dir != canonical {
            self.prev_dir = Some(self.listing.dir.clone());
        }
        let _ = std::env::set_current_dir(&canonical);
        // If the directory had more than `MAX_ENTRIES`, the read
        // stopped early. Surface that to the user with a flash so a
        // partial listing isn't mistaken for the whole picture — the
        // alternative was the pre-fix behavior of hanging the event
        // loop for many seconds on a 1M-entry tmp dir.
        if new_listing.truncated {
            self.flash_info(format!(
                "listing capped at {} entries — directory has more",
                crate::fs::listing::MAX_ENTRIES
            ));
        }
        self.listing = new_listing;
        self.listing.sort(self.sort_order, self.sort_reversed);
        // Maybe re-evaluate "is this a huge tree?" — only runs the
        // bounded-DFS walk when the chdir crosses into a different
        // project (different repo root, or out of any repo). Drilling
        // around inside the same project inherits the cached value.
        // Must happen *before* the git calls below so they see the
        // right `huge` value on the first run after chdir.
        self.update_huge_tree(&canonical);
        // Refill the raw-status cache (if needed) before computing
        // branch/dirty — `compute_git_info_fast` reads `dirty` off
        // the cached raw output, so it must be current.
        let files = self.git_file_statuses_cached(&canonical);
        let info = self.compute_git_info_fast();
        self.git.set(info, files);
        // Cache key from the cached repo root — no subprocess. The
        // chdir implicitly switched repos if the new tree has a
        // different `.git/`, so seed the cache here rather than wait
        // for the next 1 Hz poll to detect the mismatch.
        self.git_cache.git_poll_cache = self.compute_git_mtime_key_fast();
        self.picks.clear();
        self.temp_filter = None;
        self.cursor = Cursor::new();
        self.view = View::Dir;
        self.rebuild_rows();
        self.frecency.record(&canonical);
        Ok(())
    }

    /// Execute an [`Effect::ChangeDir`]: `chdir`, then on success focus
    /// `focus` (by path) and flash `on_ok`; on failure flash
    /// `"{err_prefix}: {e}"`. The single implementation shared by the
    /// `run_effects` executor (the pure-Model `apply()` Action arms route
    /// their chdirs through it via the deferred effect) — kept on `AppState`
    /// so its behavior is unit-testable without a `Tui`. Impure App-layer
    /// callers that need bespoke post-chdir work (harpoon / finder /
    /// inventory) stay on `chdir` directly.
    pub fn change_dir(
        &mut self,
        path: &Path,
        focus: Option<&Path>,
        on_ok: Option<&str>,
        err_prefix: &str,
    ) {
        match self.chdir(path) {
            Ok(()) => {
                if let Some(f) = focus {
                    self.focus_on_path(f);
                }
                if let Some(msg) = on_ok {
                    self.flash_info(msg.to_string());
                }
            }
            Err(e) => self.flash_error(format!("{err_prefix}: {e}")),
        }
    }

    /// `..` / `h` — climb to the parent directory (or leave the inventory
    /// view). MVU Phase 5: the parent chdir is a deferred [`Effect::ChangeDir`]
    /// (returned for the `apply()` arm to emit) so this stays a pure-Model
    /// transition. Focus is by the just-left directory's path — the parent's
    /// row for that child has `r.path == old_dir`, so it lands on the same row
    /// the former by-display-name match did. The inventory-exit branch does no
    /// IO and clamps the cursor itself (it previously relied on `apply()`'s
    /// trailing clamp, which the effect early-return now skips).
    pub fn climb(&mut self) -> Vec<Effect> {
        if self.view == View::Inventory {
            self.view = View::Dir;
            self.rebuild_rows();
            self.cursor.clamp(self.rows.len());
            return Vec::new();
        }
        if let Some(parent) = self.listing.dir.parent().map(Path::to_path_buf) {
            let old_dir = self.listing.dir.clone();
            return vec![Effect::ChangeDir {
                path: parent,
                focus: Some(old_dir),
                on_ok: None,
                err_prefix: "chdir",
            }];
        }
        Vec::new()
    }

    // --- Action dispatch (pure-domain arms) ---

    /// Resolve a `:project`/`:startdir` argument to an absolute directory.
    /// Accepts `.` (current listing dir), `~`-expanded paths, absolute paths,
    /// or relative paths (resolved against the listing dir). Rejects files
    /// and non-existent paths with a descriptive error.
    pub fn resolve_dir_arg(&self, arg: &str) -> std::result::Result<PathBuf, String> {
        let target = if arg == "." {
            self.listing.dir.clone()
        } else {
            crate::paths::expand(arg)
        };
        let abs = if target.is_absolute() {
            target
        } else {
            self.listing.dir.join(&target)
        };
        let canon = std::fs::canonicalize(&abs).map_err(|e| e.to_string())?;
        if !canon.is_dir() {
            return Err(format!("not a directory: {}", abs.display()));
        }
        Ok(canon)
    }
}

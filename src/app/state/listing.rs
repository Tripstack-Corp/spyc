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
        if let Some(i) = self.cur().rows.iter().position(|r| r.path == path) {
            self.cur_mut().cursor.index = i;
        }
    }

    pub fn rebuild_rows(&mut self) {
        self.cur_mut().list_generation = self.cur().list_generation.wrapping_add(1);
        self.cur_mut().rows = match self.cur().view {
            View::Dir => {
                let base: Vec<RowData> = self
                    .cur()
                    .listing
                    .entries
                    .iter()
                    .filter(|e| !self.cur().masks.hides(&e.name))
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
        let row_count = self.cur().rows.len();
        self.cur_mut().cursor.clamp(row_count);
    }

    /// Re-sort the listing with the current `sort_order` / `sort_reversed` and
    /// rebuild the visible rows. Shared by the `:sort`, `:sort reverse`, and
    /// `:set sort=` command arms, which only differ in how they mutate the
    /// sort state and what they flash.
    pub fn apply_sort(&mut self) {
        let (order, reversed) = (self.cur().sort_order, self.cur().sort_reversed);
        self.cur_mut().listing.sort(order, reversed);
        self.rebuild_rows();
    }

    pub fn apply_temp_filter(&self, rows: Vec<RowData>) -> Vec<RowData> {
        let Some(ref pattern) = self.cur().temp_filter else {
            return rows;
        };
        if pattern == "!" {
            rows.into_iter()
                .filter(|r| self.cur().picks.contains(&r.path))
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
                    self.cur()
                        .git
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
        match Listing::read(&self.cur().listing.dir) {
            Ok(new) => {
                self.cur_mut().listing = new;
                // Event-driven refresh touches the FOCUSED column's git (the
                // 1 Hz poll sweeps both columns; dual fs-watch is deferred).
                let side = self.focused_side();
                // Refresh the top-bar branch/dirty string too — without
                // this the bar stays on `main` after edits and only
                // updates when the user changes directories. Event-
                // driven refresh would normally invalidate the raw
                // cache (file mtimes moved but `.git/index` may not
                // have — and we need fresh content for ` M`
                // markers).
                //
                // But: an active filesystem (claude writing findings, build
                // outputs, IDE auto-saves) can trip `refresh_listing`
                // repeatedly. Throttle the raw-cache invalidation to 1 s so a
                // burst doesn't re-walk `git status` on every event. The 1 Hz
                // safety poll in `refresh_git_state` still catches `.git/index`
                // changes immediately; the only trade-off is up to ~1 s lag in
                // working-tree ` M` markers for edits within the window.
                let throttle = std::time::Duration::from_secs(1);
                let should_invalidate = self
                    .col(side)
                    .git_cache
                    .last_git_invalidation
                    .is_none_or(|t| t.elapsed() >= throttle);
                if should_invalidate {
                    self.col_mut(side).git_cache.git_status_cache = None;
                    self.col_mut(side).git_cache.last_git_invalidation =
                        Some(std::time::Instant::now());
                    // This walk reflects the current worktree, so any earlier
                    // deferred re-walk is now satisfied.
                    self.col_mut(side).git_cache.pending_worktree_rewalk = false;
                } else {
                    // Throttled this round — defer the re-walk so the working-tree
                    // change can't stay stale. The 1 Hz git poll's mtime
                    // short-circuit won't catch it (an unstaged edit moves no
                    // `.git/index`/`HEAD` mtime), so flag it for a forced re-walk
                    // on the next poll instead of dropping it.
                    self.col_mut(side).git_cache.pending_worktree_rewalk = true;
                }
                let dir = self.col(side).listing.dir.clone();
                let new_git_files = self.git_file_statuses_cached(side, &dir);
                let new_git_info = self.compute_git_info_fast(side);
                let mut new_keys: Vec<&str> = new_git_files.keys().map(String::as_str).collect();
                new_keys.sort_unstable();
                crate::spyc_debug!(
                    "refresh_listing: dir={} git_info: {:?} → {:?}, git_files: {} → {} (new={:?})",
                    self.col(side).listing.dir.display(),
                    self.col(side).git.info,
                    new_git_info,
                    self.col(side).git.files.len(),
                    new_git_files.len(),
                    new_keys,
                );
                self.col_mut(side).git.set(new_git_info, new_git_files);
                self.rebuild_rows();
            }
            Err(e) => {
                crate::spyc_debug!(
                    "refresh_listing: Listing::read({}) failed: {e}",
                    self.cur().listing.dir.display(),
                );
            }
        }
    }

    pub fn chdir(&mut self, path: &Path) -> Result<()> {
        let canonical = std::fs::canonicalize(path)?;
        let new_listing = Listing::read(&canonical)?;
        if self.cur().listing.dir != canonical {
            self.prev_dir = Some(self.cur().listing.dir.clone());
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
        self.cur_mut().listing = new_listing;
        let (order, reversed) = (self.cur().sort_order, self.cur().sort_reversed);
        self.cur_mut().listing.sort(order, reversed);
        // chdir is the FOCUSED column's navigation — refresh that column's git.
        let side = self.focused_side();
        // Resolve + cache the repo root for the new dir *before* the git
        // calls below so they see the right root on the first run after chdir.
        self.update_repo_root(side, &canonical);
        // Refill the raw-status cache (if needed) before computing
        // branch/dirty — `compute_git_info_fast` reads `dirty` off
        // the cached raw output, so it must be current.
        let files = self.git_file_statuses_cached(side, &canonical);
        let info = self.compute_git_info_fast(side);
        self.col_mut(side).git.set(info, files);
        // Cache key from the cached repo root — no subprocess. The
        // chdir implicitly switched repos if the new tree has a
        // different `.git/`, so seed the cache here rather than wait
        // for the next 1 Hz poll to detect the mismatch.
        let key = self.compute_git_mtime_key_fast(side);
        self.col_mut(side).git_cache.git_poll_cache = key;
        self.cur_mut().picks.clear();
        self.cur_mut().temp_filter = None;
        self.cur_mut().cursor = Cursor::new();
        self.cur_mut().view = View::Dir;
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
        if self.cur().view == View::Inventory {
            self.cur_mut().view = View::Dir;
            self.rebuild_rows();
            let row_count = self.cur().rows.len();
            self.cur_mut().cursor.clamp(row_count);
            return Vec::new();
        }
        if let Some(parent) = self.cur().listing.dir.parent().map(Path::to_path_buf) {
            let old_dir = self.cur().listing.dir.clone();
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
            self.cur().listing.dir.clone()
        } else {
            crate::paths::expand(arg)
        };
        let abs = if target.is_absolute() {
            target
        } else {
            self.cur().listing.dir.join(&target)
        };
        let canon = std::fs::canonicalize(&abs).map_err(|e| e.to_string())?;
        if !canon.is_dir() {
            return Err(format!("not a directory: {}", abs.display()));
        }
        Ok(canon)
    }
}

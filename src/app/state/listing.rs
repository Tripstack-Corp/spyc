//! `AppState` directory listing + navigation: row rebuild, temp filter,
//! refresh, and chdir/change_dir/climb. Split from `state` verbatim.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::fs::{Entry, Listing};
use crate::state::Cursor;

use crate::app::{Effect, Matcher, RowData, View, row_from_entry};

use super::{AppState, Side};

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
            View::Dir => self.build_dir_rows(),
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
                    deleted: false,
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
                        deleted: false,
                    }
                })
                .collect(),
        };
        let row_count = self.cur().rows.len();
        self.cur_mut().cursor.clamp(row_count);
    }

    /// Build the `View::Dir` rows: the on-disk entries (mask-filtered, in the
    /// listing's sort order) plus synthesized "ghost" rows for git-deleted
    /// files that are gone from disk, so a deletion stays visible (the view
    /// then renders ghosts struck-through). Ghosts are interleaved via the
    /// listing's own sort so a deleted file sorts into place — e.g. the source
    /// of an unstaged rename sits right next to its new name under name order.
    fn build_dir_rows(&self) -> Vec<RowData> {
        use std::collections::HashSet;

        let masks = &self.cur().masks;
        let live: Vec<&Entry> = self
            .cur()
            .listing
            .entries
            .iter()
            .filter(|e| !masks.hides(&e.name))
            .collect();
        let live_names: HashSet<&str> = live.iter().map(|e| e.name.as_str()).collect();

        // `git.files` keys in-dir files by bare basename and dir aggregates by
        // `name/`. A git-deleted file that's no longer on disk (and isn't
        // mask-hidden) becomes a ghost; one still on disk (e.g. `git rm
        // --cached`) keeps its real row.
        let ghost_names: Vec<&String> = self
            .cur()
            .git
            .files
            .iter()
            .filter(|(name, st)| {
                !name.ends_with('/')
                    && st.is_deleted()
                    && !live_names.contains(name.as_str())
                    && !masks.hides(name)
            })
            .map(|(name, _)| name)
            .collect();

        if ghost_names.is_empty() {
            // Fast path: no deletions to surface — identical to the old behavior.
            return self.apply_temp_filter(live.iter().copied().map(row_from_entry).collect());
        }

        // Combine live + ghost placeholders and re-sort with the listing's OWN
        // comparator (a throwaway `Listing` reuses `Listing::sort` verbatim, so
        // the order — and #525's allocation-free sort — can't drift). Ghosts
        // are then identified by name to set the struck-through flag.
        let dir = self.cur().listing.dir.clone();
        let mut combined: Vec<Entry> = live.iter().map(|&e| e.clone()).collect();
        for name in &ghost_names {
            combined.push(Entry::deleted_placeholder(&dir, name));
        }
        let ghost_set: HashSet<&str> = ghost_names.iter().map(|n| n.as_str()).collect();
        let mut tmp = Listing {
            dir,
            entries: combined,
            truncated: false,
        };
        tmp.sort(self.cur().sort_order, self.cur().sort_reversed);
        let rows: Vec<RowData> = tmp
            .entries
            .iter()
            .map(|e| {
                let mut rd = row_from_entry(e);
                rd.deleted = ghost_set.contains(e.name.as_str());
                rd
            })
            .collect();
        self.apply_temp_filter(rows)
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
                .filter(|r| self.cur().harpoon_filter_set.contains(&r.path))
                .collect()
        } else if pattern == "git" {
            // Show only entries that appear in `git status` with a
            // non-Clean state. `git_files` keys files by basename and
            // also marks parent directories that contain changes
            // (basename + trailing `/`), so directories show up too —
            // useful for navigating into a subtree with edits. `git_key`
            // strips the executable `*` decoration so exec files match.
            rows.into_iter()
                .filter(|r| {
                    self.cur()
                        .git
                        .files
                        .get(r.git_key())
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
        // Self-heal first: if a column's cwd was deleted out from under us (an
        // external `git worktree remove` / `rm -rf` / another agent's teardown —
        // anything spyc didn't route through its own `remove_worktree`), snap it
        // back to PROJECT_HOME instead of leaving it stranded on a dead path with
        // stale rows. Checks both columns (cheap `is_dir` stats); no-op when none
        // is orphaned. Runs here because every fs-event / poll refresh lands in
        // this method, so an external deletion is healed on the next refresh.
        self.reset_orphaned_columns_to_home();
        match Listing::read(&self.cur().listing.dir) {
            Ok(new) => self.apply_refreshed_listing(new),
            Err(e) => {
                crate::spyc_debug!(
                    "refresh_listing: Listing::read({}) failed: {e}",
                    self.cur().listing.dir.display(),
                );
            }
        }
    }

    /// Install an already-read listing into the focused column and bring its
    /// git markers + rows up to date — the back half of [`refresh_listing`],
    /// shared with the off-thread watcher refresh (`App::spawn_listing_refresh`,
    /// which does the heavy `Listing::read` on a worker and then calls this with
    /// the focused column's freshly-read `Listing`). The caller guarantees `new`
    /// is for the focused column's current dir (the sync path reads it inline;
    /// the async path staleness-checks `list_generation` before calling).
    pub fn apply_refreshed_listing(&mut self, new: Listing) {
        self.cur_mut().listing = new;
        // Event-driven refresh touches the FOCUSED column's git. With
        // dual fs-watch (PR D) both columns' trees + gitdirs fire
        // events, so the focused column refreshes here on its own edits;
        // `.git` index/HEAD events refresh BOTH columns via the
        // git-event path (`refresh_git_state`). A non-focused column's
        // working-tree edits still wait for the 1 Hz poll.
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
            self.col_mut(side).git_cache.last_git_invalidation = Some(std::time::Instant::now());
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

    pub fn chdir(&mut self, path: &Path) -> Result<()> {
        self.chdir_side(self.focused_side(), path)
    }

    /// `chdir`, but targeting a SPECIFIC column rather than always the focused
    /// one. The process cwd, `prev_dir`, and frecency are *focused-column*
    /// concerns, so they move only when `side` IS the focused column — resetting
    /// a background column (e.g. `b` after its worktree is removed) must not yank
    /// the process cwd or back-nav history out from under the focused one.
    /// `chdir` is the `side == focused_side()` case, behavior-for-behavior.
    pub fn chdir_side(&mut self, side: Side, path: &Path) -> Result<()> {
        let canonical = std::fs::canonicalize(path)?;
        let new_listing = Listing::read(&canonical)?;
        let focused = self.focused_side() == side;
        if focused && self.col(side).listing.dir != canonical {
            self.prev_dir = Some(self.col(side).listing.dir.clone());
        }
        if focused {
            let _ = std::env::set_current_dir(&canonical);
        }
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
        self.col_mut(side).listing = new_listing;
        let (order, reversed) = (self.col(side).sort_order, self.col(side).sort_reversed);
        self.col_mut(side).listing.sort(order, reversed);
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
        self.col_mut(side).picks.clear();
        self.col_mut(side).temp_filter = None;
        self.col_mut(side).cursor = Cursor::new();
        self.col_mut(side).view = View::Dir;
        self.rebuild_rows();
        if focused {
            self.frecency.record(&canonical);
        }
        Ok(())
    }

    /// Snap any column (A or B) whose cwd no longer exists back to a real
    /// directory with a flash, rather than stranding it on a deleted path.
    /// Cause-agnostic: covers a worktree torn down by spyc's own
    /// `remove_worktree`/`clean_worktree`, **and** one removed out from under us
    /// externally (a raw `git worktree remove`, `rm -rf`, or another agent) —
    /// the latter is caught because [`Self::refresh_listing`] calls this on every
    /// refresh. Target is PROJECT_HOME when it's still a directory; if PROJECT_HOME
    /// is unset or itself gone, falls back to the orphaned dir's nearest existing
    /// ancestor (worst case `/`) so the heal is **never** a no-op — a column is
    /// always moved to somewhere real. No-op only when no column is orphaned.
    /// (The `remove_worktree`/`clean_worktree` MCP flow no longer refuses a
    /// worktree a column is in — it relies on this.)
    pub fn reset_orphaned_columns_to_home(&mut self) {
        let home = self.project_home.clone().filter(|h| h.is_dir());
        let mut sides = vec![Side::Left];
        if self.right.is_some() {
            sides.push(Side::Right);
        }
        for side in sides {
            let orphaned = self.col(side).listing.dir.clone();
            if orphaned.is_dir() {
                continue; // still a valid directory — leave it
            }
            // Prefer PROJECT_HOME; else the nearest existing ancestor of the
            // dead path (so even a missing PROJECT_HOME can't leave the column
            // stranded). `to_home` only when we actually used PROJECT_HOME, so
            // the flash doesn't claim "project home" for an ancestor landing.
            let (target, to_home) = match &home {
                Some(h) => (h.clone(), true),
                None => (nearest_existing_ancestor(&orphaned), false),
            };
            if self.chdir_side(side, &target).is_ok() {
                self.flash_info(if to_home {
                    "directory not found, returning to project home".to_string()
                } else {
                    format!(
                        "directory not found, moved to {}",
                        crate::paths::display_tilde(&target)
                    )
                });
            }
        }
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

/// The nearest existing directory at or above `path` — walk up its ancestors
/// and return the first that is a directory. Column dirs are canonicalized
/// (absolute), so the walk terminates at `/` (always a directory); the
/// `unwrap_or` is a belt-and-suspenders for a pathological relative input.
/// Used as the orphaned-column heal target when PROJECT_HOME is also gone, so
/// a column is never left on a dead path.
fn nearest_existing_ancestor(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.is_dir())
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
}

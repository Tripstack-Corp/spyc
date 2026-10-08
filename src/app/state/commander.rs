//! Per-column browser Model and its constructor.

use super::{GitCache, GitState, RowData, View};
use crate::config::Config;
use crate::fs::Listing;
use crate::state::{Cursor, IgnoreMasks, Picks};
use crate::ui::list_view::GridDims;
use std::path::{Path, PathBuf};

/// One file-commander's worth of pure Model state — the per-browser fields a
/// vertical-split column owns: the directory it shows, the cursor in it, its
/// picks/masks/filter/sort, and its display rows + grid geometry.
///
/// Extracted in PR A of the vsplit Stage 2 plan as a behaviour-preserving move.
/// `left` is always present; `right` is `None` until a second column is opened
/// (Stage 2 PR C, the feature). The pure-Model **update** path
/// (`AppState::apply` / `dispatch_*` / cursor / selection / listing) reaches the
/// **focused** column through [`super::AppState::cur`] / [`super::AppState::cur_mut`] — while
/// `right` is `None` that always resolves to `left`, so the accessor is
/// behaviour-preserving. Render addresses `left` / `right` explicitly (it draws
/// both columns). (`git`/`git_cache` are per-column fields below — moved off
/// `AppState` for dual git, so `b` in a different repo renders its own markers.)
pub struct Commander {
    /// The directory this browser is showing + its entries.
    pub listing: Listing,
    /// Multi-select set (keyed by path) scoped to this browser's listing.
    pub picks: Picks,
    /// File-ignore toggles (`.`/`~`), applied during `rebuild_rows`.
    pub masks: IgnoreMasks,
    /// The `:limit` / `=` filter pattern narrowing this browser's rows.
    pub temp_filter: Option<String>,
    pub sort_order: crate::fs::listing::SortMode,
    /// When true, invert the per-mode natural direction (Name/Ext
    /// ascending → descending, Size/Mtime descending → ascending).
    /// Toggled by `gs` and `:sort reverse`. Dirs-first grouping is
    /// always preserved regardless.
    pub sort_reversed: bool,
    /// Which content this browser shows: `Dir` / `Inventory` / `Graveyard`.
    pub view: View,
    /// Cursor index + viewport scroll (`view_top`) within this browser.
    pub cursor: Cursor,
    /// The rendered display rows (derived from `listing` + filter + sort).
    pub rows: Vec<RowData>,
    /// The geometry slice of this browser's last rendered grid (cols ×
    /// rows-per-col), written by render and read by cursor/page-math.
    pub grid_dims: GridDims,
    /// Monotonic counter bumped whenever this browser's display row list
    /// changes. Used by App to skip redundant `build_rows()` calls.
    pub list_generation: u64,
    /// Per-column git display pair (branch + per-file markers) for THIS
    /// browser's repo/worktree. Per-column so `b` in a different worktree
    /// shows its own markers, not `a`'s. (Moved off `AppState` for dual git.)
    pub git: GitState,
    /// Per-column git cache + worker plumbing (repo root/gitdir, status cache,
    /// the per-column generation gate, the worker outbox). Per-column so two
    /// columns in different repos can't collide on a single generation.
    pub git_cache: GitCache,
    /// This column's harpoon list — pinned per-**worktree** file pointers
    /// (`None` outside a repo with no `PROJECT_HOME`). Per-column so `b` in a
    /// separate worktree gets its own bookmarks: harpoon stores absolute paths,
    /// so a shared list would jump `b` into `a`'s copy (wrong worktree/branch).
    /// Keyed by [`super::AppState::harpoon_root`]; `App::reconcile_harpoon` swaps it
    /// when that root shifts.
    pub harpoon: Option<crate::state::Harpoon>,
    /// Snapshot of [`Self::harpoon`]'s ancestor-set (slot paths plus every
    /// parent directory of every slot). Refreshed whenever the list mutates so
    /// `apply_temp_filter` (`=h`) stays pure-domain. Empty when `harpoon` is
    /// `None`.
    pub harpoon_filter_set: std::collections::HashSet<PathBuf>,
    /// Bare basenames the user just removed (`R`) that aren't git-untracked,
    /// held as optimistic struck-through ghosts until the authoritative
    /// off-thread `git status` lands. Without this the row vanishes on the sync
    /// post-unlink `refresh_listing` (git markers are async, so no `is_deleted()`
    /// ghost yet) and only reappears as a ghost when the worker result arrives —
    /// a visible list "bounce". `build_dir_rows` unions these into its ghost set;
    /// `apply_git_worker_result` clears them once git is authoritative (a tracked
    /// deletion is then a real ghost, an untracked/ignored one simply gone).
    /// Dir-scoped — cleared on chdir.
    pub pending_ghosts: std::collections::HashSet<String>,
}

impl Commander {
    /// Build a fresh commander rooted at `dir`: read + sort the listing and
    /// seed masks from `config`, with empty picks/filter and the cursor at the
    /// top. `rows` is left empty — the caller runs `rebuild_rows()` once this
    /// is the focused commander (it builds rows through `cur()`). Used to open
    /// the second (right) column; mirrors the `left` init in `bootstrap`.
    pub fn for_dir(dir: &Path, config: &Config) -> anyhow::Result<Self> {
        let sort_order = crate::fs::listing::SortMode::Name;
        let mut listing = Listing::read(dir)?;
        listing.sort(sort_order, false);
        let mut masks = IgnoreMasks::default();
        masks.apply_config(&config.ignore_masks);
        Ok(Self {
            listing,
            picks: Picks::new(),
            masks,
            temp_filter: None,
            sort_order,
            sort_reversed: false,
            view: View::Dir,
            cursor: Cursor::new(),
            rows: Vec::new(),
            grid_dims: GridDims {
                cols: 1,
                rows_per_col: 1,
            },
            list_generation: 0,
            git: GitState::default(),
            git_cache: GitCache::default(),
            // Populated by `App::reconcile_harpoon` once this column's repo
            // root / PROJECT_HOME is known (a fresh commander has no root yet).
            harpoon: None,
            harpoon_filter_set: std::collections::HashSet::new(),
            pending_ghosts: std::collections::HashSet::new(),
        })
    }
}

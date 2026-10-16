//! `Project`: a home directory and everything the user is doing under it —
//! the file columns, the split, the pane layout and where the keyboard was.
//! #99 step 1 (`docs/drafts/PROJECTS_PLAN.md` §0): introduced
//! behaviour-identical, holding exactly what `AppState` held flat before —
//! there is still exactly one project, created at launch exactly as today.
//! `AppState::active_project()` / `active_project_mut()` are the only way
//! anything reaches it; the existing column handles (`cur()`, `col(side)`,
//! …) delegate through them, unchanged from every caller's perspective.

use std::path::PathBuf;

use crate::config::Config;
use crate::keymap::UserKeymap;

use super::{Commander, Focus, PaneLayout, VSplit};

/// Identity of a project — a uuid, never an index (an index drifts when a
/// project closes) or a path (a path changes identity on a re-home). `Copy`:
/// cheap to carry wherever "which project" needs naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectId(uuid::Uuid);

impl ProjectId {
    /// A fresh id. Named `new`, not `default`, so a `ProjectId` is never
    /// conjured implicitly — every one names a real, just-created project.
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

/// A home directory and everything the user is doing under it: the file
/// columns, the split's shape, the pane layout, and where the keyboard was.
/// See `docs/drafts/PROJECTS_PLAN.md` §0 and §1a.
pub struct Project {
    pub id: ProjectId,
    /// Today's `project_home`. Stays `Option`, not required: `:project
    /// clear` (`state/dispatch.rs`) sets it to `None` today and must keep
    /// working unchanged — making a project's home mandatory belongs to a
    /// later step (once a project's home is its identity), not this one.
    pub home: Option<PathBuf>,
    /// The left (primary) file-commander's per-browser Model state. Always
    /// present — the single-column default *is* the left commander.
    pub left: Commander,
    /// The right (secondary) file-commander, when a second column is open.
    pub right: Option<Commander>,
    /// The vertical (left/right) split, when open. `None` is the
    /// single-column default.
    pub vsplit: Option<VSplit>,
    /// Which surface owns the keyboard, in this project.
    pub focus: Focus,
    /// This project's bottom-pane layout + prompt state.
    pub pane: PaneLayout,
    pub start_dir: PathBuf,
    pub prev_dir: Option<PathBuf>,
    pub config: Config,
    pub user_keymap: UserKeymap,
}

impl Project {
    /// Build a fresh project around an already-constructed `left` commander
    /// (its `Listing`, masks, git state, harpoon, … are set up by the
    /// caller, exactly as `App::new` builds today's single commander) and
    /// the config/keymap it resolved to get there. `right`/`vsplit`/
    /// `prev_dir` start empty and `focus` starts on the file list, matching
    /// what `App::new`'s single `AppState` literal set today.
    pub fn new(
        home: Option<PathBuf>,
        left: Commander,
        start_dir: PathBuf,
        config: Config,
        user_keymap: UserKeymap,
        pane: PaneLayout,
    ) -> Self {
        Self {
            id: ProjectId::new(),
            home,
            left,
            right: None,
            vsplit: None,
            focus: Focus::default(),
            pane,
            start_dir,
            prev_dir: None,
            config,
            user_keymap,
        }
    }
}

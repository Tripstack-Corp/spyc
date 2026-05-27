//! Transient and persistent UI state (cursor, picks, inventory, masks, marks).

use std::cell::RefCell;
use std::path::PathBuf;

pub mod codex_transcript;
pub mod cursor;
pub mod frecency;
pub mod graveyard;
pub mod harpoon;
pub mod health;
#[allow(dead_code, clippy::question_mark)]
pub mod history;
pub mod ignore;
pub mod inventory;
pub mod marks;
pub mod pager_positions;
pub mod picks;
pub mod session_names;
pub mod sessions;

pub use cursor::Cursor;
pub use frecency::Frecency;
pub use harpoon::Harpoon;
pub use history::History;
pub use ignore::IgnoreMasks;
pub use inventory::Inventory;
pub use marks::{Mark, Marks};
pub use picks::Picks;

thread_local! {
    static STATE_ROOT_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Resolve the spyc state-root directory (the equivalent of
/// `$XDG_STATE_HOME/spyc`). Every persistent state module appends its
/// own subdirectory (`harpoon`, `sessions`, `graveyard`, …) under
/// this root.
///
/// Resolution order:
/// 1. Per-thread test override (see `with_state_root`).
/// 2. `$XDG_STATE_HOME/spyc`.
/// 3. `$HOME/.local/state/spyc`.
/// 4. `None` on exotic systems with neither.
///
/// The thread-local override lets parallel tests isolate from each
/// other without mutating process-global env vars — every previous
/// test pattern (`unsafe { set_var("XDG_STATE_HOME", …) }`) collapses
/// into a scoped `with_state_root` call.
pub fn state_root() -> Option<PathBuf> {
    if let Some(p) = STATE_ROOT_OVERRIDE.with(|c| c.borrow().clone()) {
        return Some(p);
    }
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join("spyc"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/spyc"))
}

/// Test-only: run `body` with `state_root()` pinned to `root`. The
/// override is unwound when `body` returns *or panics* (RAII guard).
#[cfg(test)]
pub fn with_state_root<R>(root: &std::path::Path, body: impl FnOnce() -> R) -> R {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            STATE_ROOT_OVERRIDE.with(|c| *c.borrow_mut() = None);
        }
    }
    STATE_ROOT_OVERRIDE.with(|c| *c.borrow_mut() = Some(root.to_path_buf()));
    let _g = Guard;
    body()
}

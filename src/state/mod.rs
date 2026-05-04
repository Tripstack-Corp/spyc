//! Transient and persistent UI state (cursor, picks, inventory, masks, marks).

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

/// Test-only mutex guarding tests that mutate process-global env vars
/// (`XDG_STATE_HOME` for state modules; `SHELL` for the shell module).
/// `std::env::set_var` is process-global, so when these tests ran in
/// parallel they raced — one test's `set_var` would replace another
/// test's value mid-call, surfacing as `NotFound` deep inside a
/// graveyard restore or as a wrong shell path in `user_shell_*`
/// assertions. Each affected test holds this lock for its full body
/// so the env stays stable. Poisoning is recovered (a panicking test
/// still releases the lock). One shared lock keeps the helper trivial;
/// the cost of serializing these ~15 tests is negligible.
#[cfg(test)]
pub fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, PoisonError};
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

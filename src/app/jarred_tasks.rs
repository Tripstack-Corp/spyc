//! The `!` captures and background tasks whose programs run in jars, as a
//! session saves them and a restore takes them back (S2c).

use crate::state::sessions::SavedTask;

use super::App;

impl App {
    /// The captures and tasks a save records.
    pub(super) fn saved_tasks(&self) -> Vec<SavedTask> {
        Vec::new()
    }

    /// Take back each saved task's jar, as a background task.
    pub(in crate::app) fn restore_tasks(&mut self, _tasks: &[SavedTask]) {}
}

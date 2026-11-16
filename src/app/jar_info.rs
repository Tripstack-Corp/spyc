//! What each tab's jar knows about the tab (R18).

use super::App;

impl App {
    /// Tell each tab's jar what it holds.
    pub(crate) fn settle_jar_info(&mut self) -> bool {
        false
    }
}

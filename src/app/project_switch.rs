//! Switching the project the user sees and drives (#99,
//! `docs/drafts/PROJECTS_PLAN.md` §5).

use super::App;
use super::state::ProjectId;

impl App {
    /// Make `id`, an open project, the one the user sees and drives.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no key binding drives the switch; tests do")
    )]
    pub(super) const fn switch_project(&mut self, id: ProjectId) {
        self.set_active(id);
    }
}

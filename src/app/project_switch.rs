//! Switching the project the user sees and drives (#99,
//! `docs/drafts/PROJECTS_PLAN.md` §5).

use super::App;
use super::file_ops::FileOp;
use super::state::ProjectId;
use crate::ui::theme::Theme;

impl App {
    /// Make `id`, an open project, the one the user sees and drives, and bring
    /// it up to date, since neither the watcher nor the git poll looked at it
    /// while it was out of view:
    /// - every column is re-read on a worker and re-walked, and the split
    ///   preview reloaded;
    /// - the colours in force are its own, and the user's colours-off toggle
    ///   stays as it was;
    /// - a scrollback snapshot armed in the project left behind is abandoned,
    ///   as a tab switch abandons one.
    ///
    /// The project left behind carries on: its context file is written now if
    /// it is behind, since its agents read it while the loop writes only the
    /// active project's; its pager streams are its own (`ProjectRuntime`); and
    /// its tabs are drained and settled with every other project's.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no key binding drives the switch; tests do")
    )]
    pub(super) fn switch_project(&mut self, id: ProjectId) {
        if id == self.state.active {
            return;
        }
        assert!(
            self.state.has_project(id),
            "switch_project is handed an open project"
        );
        if std::mem::take(&mut self.view.project_mut().context_dirty) {
            self.write_context();
        }
        self.runtime.pane_scroll_settle = None;
        self.set_active(id);
        let reads = self.state.relist_columns();
        if !reads.is_empty() {
            self.spawn_file_op(FileOp::RelistColumns { reads });
        }
        self.state.refresh_git_state();
        let mono = self.view.theme.mono;
        self.view.theme =
            Theme::default().with_overrides(&self.state.active_project().config.colors);
        self.view.theme.mono = mono;
        self.kick_preview_reload();
        self.view.project_mut().context_dirty = true;
        self.view.needs_full_repaint = true;
    }
}

//! The projects closed lately, which the switcher (`Space j`) lists after the
//! open ones and reopens (#99, `docs/drafts/PROJECTS_PLAN.md` §3): what a
//! close keeps, newest first and one per anchor, and the rows the switcher
//! numbers.

use std::path::Path;

use super::{AppState, ProjectId};
use crate::state::sessions::ClosedProject;

/// How many closed projects a spyc keeps, and the switcher lists.
pub const RECENTLY_CLOSED: usize = 5;

/// One of the switcher's rows, in the order they are numbered: an open
/// project to go to, or a closed one to reopen.
#[derive(Debug, Clone)]
pub enum SwitcherRow {
    Open(ProjectId),
    Closed(Box<ClosedProject>),
}

impl AppState {
    /// Keep `closed` as the newest closed project, in place of any other at
    /// its anchor, and no more than [`RECENTLY_CLOSED`].
    pub fn remember_closed(&mut self, closed: ClosedProject) {
        self.forget_closed(closed.project.anchor());
        self.closed_projects.insert(0, closed);
        self.closed_projects.truncate(RECENTLY_CLOSED);
    }

    /// Forget the closed project anchored at `dir`, which is open again.
    pub fn forget_closed(&mut self, dir: &Path) {
        self.closed_projects
            .retain(|closed| closed.project.anchor() != dir);
    }

    /// Keep `adopted`, the closed projects of a session this spyc takes on,
    /// beside its own: newest first, one per anchor, no more than
    /// [`RECENTLY_CLOSED`].
    pub fn adopt_closed(&mut self, adopted: &[ClosedProject]) {
        let mut closed = std::mem::take(&mut self.closed_projects);
        closed.extend_from_slice(adopted);
        self.closed_projects = newest_per_anchor(closed);
        self.closed_projects.truncate(RECENTLY_CLOSED);
    }
}

/// `closed`, newest first, keeping only the newest at each anchor. Of two
/// that closed at once, the first given stays.
pub fn newest_per_anchor(mut closed: Vec<ClosedProject>) -> Vec<ClosedProject> {
    closed.sort_by_key(|c| std::cmp::Reverse(c.closed_secs));
    let mut seen = std::collections::HashSet::new();
    closed.retain(|c| seen.insert(c.project.anchor().to_path_buf()));
    closed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn closed(dir: &str, closed_secs: u64) -> ClosedProject {
        serde_json::from_value(serde_json::json!({
            "closed_secs": closed_secs,
            "project": {
                "id": ProjectId::new(), "home": dir, "label": dir, "start_dir": dir,
                "pane_height_pct": 30
            }
        }))
        .expect("a closed project")
    }

    fn anchors(state: &AppState) -> Vec<(PathBuf, u64)> {
        state
            .closed_projects
            .iter()
            .map(|c| (c.project.anchor().to_path_buf(), c.closed_secs))
            .collect()
    }

    /// A close puts the project first, in place of an older close of the same
    /// one, and the oldest goes once there are more than five.
    #[test]
    fn a_close_is_kept_first_once_and_the_oldest_goes() {
        let mut state = AppState::test_default(PathBuf::from("/w"));
        for (i, dir) in ["/a", "/b", "/c", "/d", "/e"].into_iter().enumerate() {
            state.remember_closed(closed(dir, i as u64));
        }
        state.remember_closed(closed("/b", 10));
        let first = anchors(&state);
        assert_eq!(first[0], (PathBuf::from("/b"), 10));
        assert_eq!(first.len(), 5);
        assert_eq!(
            first.iter().filter(|(d, _)| d == Path::new("/b")).count(),
            1
        );
        state.remember_closed(closed("/f", 11));
        let after: Vec<PathBuf> = anchors(&state).into_iter().map(|(d, _)| d).collect();
        assert_eq!(after, ["/f", "/b", "/e", "/d", "/c"].map(PathBuf::from));
    }

    /// An adopted session's closed projects join this spyc's own by when they
    /// closed, the newest of each anchor kept.
    #[test]
    fn adopted_closed_projects_merge_newest_first() {
        let mut state = AppState::test_default(PathBuf::from("/w"));
        state.remember_closed(closed("/a", 5));
        state.adopt_closed(&[closed("/a", 9), closed("/b", 7), closed("/c", 1)]);
        assert_eq!(
            anchors(&state),
            [("/a", 9), ("/b", 7), ("/c", 1)].map(|(d, s)| (PathBuf::from(d), s))
        );
    }

    /// `Space n` onto a closed project's anchor forgets it.
    #[test]
    fn forgetting_drops_only_that_anchor() {
        let mut state = AppState::test_default(PathBuf::from("/w"));
        state.remember_closed(closed("/a", 1));
        state.remember_closed(closed("/b", 2));
        state.forget_closed(Path::new("/a"));
        assert_eq!(anchors(&state), [(PathBuf::from("/b"), 2)]);
    }
}

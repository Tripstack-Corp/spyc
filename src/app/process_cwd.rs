//! Process-cwd reconcile: the process cwd follows the focused column.
//!
//! `AppState::chdir_side` moves it on a navigation, but only for the column
//! focused at that moment. Anything that changes *which* column is focused, or
//! installs a column without navigating — an agent's `open_worktree` into `b`,
//! a restore, a focus switch — used to leave the process behind, and a worktree
//! removed from under a stale cwd broke every in-process git call after it
//! (#495). One loop-bottom compare makes all of those correct, the
//! `settle_mouse_mode` shape: a read here, the `chdir` in the executor.

use std::path::{Path, PathBuf};

use super::{App, Effect};

/// The `chdir` that brings `actual` to `desired`, if one is owed. `desired` is
/// `None` when the focused column has no directory to be in (a virtual path
/// inside an archive mount), so the process stays put. `actual` is `None` when
/// the cwd can't be read, typically because it was deleted, which always owes
/// a move.
pub(super) fn owed_chdir(desired: Option<&Path>, actual: Option<&Path>) -> Option<PathBuf> {
    let desired = desired?;
    (actual != Some(desired)).then(|| desired.to_path_buf())
}

impl App {
    /// Where the process cwd belongs: the focused column's directory, unless
    /// that is a virtual path inside a mounted archive.
    pub(super) fn desired_process_cwd(&self) -> Option<&Path> {
        let dir = self.state.cur().listing.dir.as_path();
        self.state.mounts.resolve(dir).is_none().then_some(dir)
    }

    /// Reconcile the process cwd against the focused column. One `getcwd` on an
    /// iteration that was happening anyway, and nothing emitted when they
    /// agree, so idle stays at 0 dps.
    pub(super) fn settle_process_cwd(&self) -> Vec<Effect> {
        let actual = std::env::current_dir().ok();
        owed_chdir(self.desired_process_cwd(), actual.as_deref())
            .map(|dir| vec![Effect::SetProcessCwd { dir }])
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::owed_chdir;
    use crate::app::App;
    use std::path::Path;

    #[test]
    fn a_chdir_is_owed_only_on_divergence() {
        let a = Path::new("/w/a");
        let b = Path::new("/w/b");
        assert_eq!(owed_chdir(Some(a), Some(a)), None, "agreement");
        assert_eq!(owed_chdir(Some(b), Some(a)).as_deref(), Some(b));
        // A cwd that can't be read (deleted under the process) always moves.
        assert_eq!(owed_chdir(Some(a), None).as_deref(), Some(a));
        // A mount has no directory to be in: stay wherever the process is.
        assert_eq!(owed_chdir(None, Some(a)), None);
        assert_eq!(owed_chdir(None, None), None);
    }

    /// The reconcile only works if the loop runs it: pin the call site, not just
    /// the helper.
    #[test]
    fn the_loop_settles_the_process_cwd() {
        let run = crate::guard_support::production_half(include_str!("run.rs"));
        assert!(
            run.contains("self.settle_process_cwd()"),
            "App::run no longer reconciles the process cwd (#495)"
        );
    }

    /// #495: an agent's `open_worktree` makes `b` the focused column without
    /// navigating, so nothing moved the process. It must now want `b`'s dir,
    /// and after a second open, the second one's.
    #[test]
    fn opening_a_worktree_in_b_moves_where_the_process_belongs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let (a, w1, w2) = (root.join("a"), root.join("w1"), root.join("w2"));
        for d in [&a, &w1, &w2] {
            std::fs::create_dir_all(d).unwrap();
        }
        crate::state::with_state_root(&root, || {
            let mut app = App::test_app(a.clone());
            app.state.left.listing.dir.clone_from(&a);
            assert_eq!(app.desired_process_cwd(), Some(a.as_path()));
            app.open_second_commander_at_background(&w1);
            assert_eq!(app.desired_process_cwd(), Some(w1.as_path()));
            app.open_second_commander_at_background(&w2);
            assert_eq!(app.desired_process_cwd(), Some(w2.as_path()));
            // And the settle owes the move from wherever the process was left.
            assert_eq!(
                owed_chdir(app.desired_process_cwd(), Some(&w1)).as_deref(),
                Some(w2.as_path())
            );
        });
    }
}

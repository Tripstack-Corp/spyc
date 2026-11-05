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
        let run = crate::guard_support::production_half(include_str!("pre_recv.rs"));
        assert!(
            run.contains("self.settle_process_cwd()"),
            "App::run no longer reconciles the process cwd (#495)"
        );
    }

    /// The process cwd follows the focused column before a Lua event fires, as a
    /// handler runs on the Lua thread and reads the cwd as it finds it, and
    /// again after the last step that can navigate (a `spyc -c` command, an MCP
    /// one), so it is settled whenever the loop waits.
    #[test]
    #[ignore = "red: the loop settles the process cwd once, after the Lua events and before the MCP commands"]
    fn the_loop_settles_the_process_cwd_before_lua_and_after_the_last_navigation() {
        let src = crate::guard_support::production_half(include_str!("pre_recv.rs"));
        let start = src
            .find("fn settle_before_recv")
            .expect("pre_recv.rs lost `settle_before_recv`");
        let body = &src[start..];
        let at = |needle: &str| {
            body.find(needle)
                .unwrap_or_else(|| panic!("`settle_before_recv` lost `{needle}`"))
        };
        let settles: Vec<usize> = body
            .match_indices("self.settle_process_cwd()")
            .map(|(i, _)| i)
            .collect();
        let lua = at("self.settle_lua_events()");
        let last_navigation =
            at("self.settle_startup_commands()").max(at("self.drain_mcp_pending("));
        assert!(
            settles.iter().any(|&s| s < lua),
            "a Lua event fires before the process cwd follows the focused column"
        );
        assert!(
            settles.iter().any(|&s| s > last_navigation),
            "the loop can wait with an MCP or `spyc -c` navigation still owed"
        );
    }

    /// A child the executor hands the terminal to (`$`, `$EDITOR`, a `unix`
    /// binding) runs in the focused column's directory, which a navigation
    /// earlier in the same tick may have changed without moving the process.
    #[test]
    #[ignore = "red: a foreground child inherits the process cwd"]
    fn a_foreground_child_is_given_the_focused_columns_directory() {
        let src = crate::guard_support::production_half(include_str!("effect/executor.rs"));
        let call = src
            .find("fg.run(")
            .map(|at| &src[at..at + src[at..].find(");").expect("`fg.run(` is closed")])
            .expect("the executor no longer runs a foreground child");
        assert!(
            call.contains("self.desired_process_cwd()"),
            "`{call}` doesn't name the focused column's directory"
        );
    }

    /// #980: the process cwd moves only through `Effect::SetProcessCwd`. The
    /// Model and the pure layers never call `set_current_dir`, which moves it
    /// for every thread at once.
    #[test]
    #[ignore = "red: `AppState::chdir_side` calls `set_current_dir`"]
    fn the_pure_layers_never_move_the_process_cwd() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files: Vec<std::path::PathBuf> = ["app/route.rs", "app/modal.rs", "app/focus.rs"]
            .iter()
            .map(|f| src.join(f))
            .collect();
        let mut dirs: Vec<std::path::PathBuf> = ["app/state", "app/render", "ui"]
            .iter()
            .map(|d| src.join(d))
            .collect();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    files.push(path);
                }
            }
        }
        let test_only = |path: &Path| {
            path.components().any(|c| c.as_os_str() == "tests")
                || path
                    .file_stem()
                    .is_some_and(|s| s == "tests" || s.to_string_lossy().ends_with("_tests"))
        };
        let mut scanned = Vec::new();
        for path in files.iter().filter(|p| !test_only(p)) {
            let text =
                std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let production = crate::guard_support::production_half(&text);
            for line in production.lines() {
                let code = line.split("//").next().unwrap_or_default();
                assert!(
                    !code.contains("set_current_dir"),
                    "{}: `{}` — the process cwd is the loop's: return the move as \
                     `Effect::SetProcessCwd` (see `settle_process_cwd`)",
                    path.display(),
                    line.trim()
                );
            }
            scanned.push((path.clone(), production));
        }
        // A scan that found nothing to read passes for the wrong reason.
        assert!(scanned.len() >= 20, "scanned only {} files", scanned.len());
        assert!(
            scanned
                .iter()
                .any(|(path, text)| path.ends_with("app/state/listing.rs")
                    && text.contains("fn chdir_side")),
            "the Model's navigation is no longer where this guard reads"
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
            app.state
                .active_project_mut()
                .left
                .listing
                .dir
                .clone_from(&a);
            assert_eq!(app.desired_process_cwd(), Some(a.as_path()));
            app.open_second_commander_at_background(&w1)
                .expect("b opens");
            assert_eq!(app.desired_process_cwd(), Some(w1.as_path()));
            app.open_second_commander_at_background(&w2)
                .expect("b opens");
            assert_eq!(app.desired_process_cwd(), Some(w2.as_path()));
            // And the settle owes the move from wherever the process was left.
            assert_eq!(
                owed_chdir(app.desired_process_cwd(), Some(&w1)).as_deref(),
                Some(w2.as_path())
            );
        });
    }
}

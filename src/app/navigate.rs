//! Path-reference navigation and Enter/Edit activation. `activate`
//! (the `Enter` / `e` action handler) descends dirs / displays / edits
//! the cursored file; `goto_file_navigate` (`gf` / `gF`) resolves a path
//! reference scraped from pane output and chdirs + focuses it; and
//! `jump_to_pane_path` does the same from a pre-extracted path string
//! (Quick Select's Path-open). Extracted verbatim from `app/mod.rs`
//! (the impl-extraction sweep). `jump_to_pane_path` / `goto_file_navigate`
//! are `pub` (called from `quick_select` / `effect`); `activate` is
//! `pub(super)` (called from `actions`, and capped to match the
//! app-private `ActivateIntent` it takes).

use std::path::{Path, PathBuf};

use crate::shell;
use crate::spyc_debug;

use super::file_ops::PagerDest;
use super::{ActivateIntent, App, Effect, EntryKind, PostAction, View, state};

/// The directories a path printed in the pane resolves against, best first:
/// the pane's cwd, then its worktree root, since a tool run from a
/// subdirectory (`cargo` in `src/`) prints paths relative to the root; then
/// the focused column's directory and its worktree root, then PROJECT_HOME.
/// Finding the pane's root `stat`s up the tree, so `J` builds this on its
/// worker.
pub(super) fn path_ref_bases(
    pane_cwd: &Path,
    column_dir: &Path,
    column_root: Option<&Path>,
    project_home: Option<&Path>,
) -> Vec<PathBuf> {
    let pane_root = super::state::find_repo_root(pane_cwd);
    let mut bases: Vec<PathBuf> = Vec::new();
    for base in [
        Some(pane_cwd),
        pane_root.as_deref(),
        Some(column_dir),
        column_root,
        project_home,
    ]
    .into_iter()
    .flatten()
    {
        if !bases.iter().any(|b| b == base) {
            bases.push(base.to_path_buf());
        }
    }
    bases
}

/// The path reference `gf` and `J` act on: the newest one in `lines` that
/// exists under any of `bases` ([`path_ref_bases`]).
pub(super) fn find_path_ref(
    lines: &[String],
    bases: &[PathBuf],
) -> Option<crate::pane::pathref::PathRef> {
    crate::pane::pathref::extract_path_ref_under(lines, bases)
}

impl App {
    /// [`path_ref_bases`] for a pane whose cwd is `pane_cwd`, against the
    /// focused column.
    fn path_ref_bases_for(&self, pane_cwd: &Path) -> Vec<PathBuf> {
        let col = self.state.cur();
        path_ref_bases(
            pane_cwd,
            &col.listing.dir,
            col.git_cache.current_repo_root.as_deref(),
            self.state.project_home.as_deref(),
        )
    }

    /// `raw` as the pane printed it, resolved against the bases `gf` uses;
    /// with none holding it, under the listing dir, so the miss names a path.
    fn resolve_pane_path(&self, raw: &str) -> PathBuf {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return path;
        }
        let column_dir = self.state.cur().listing.dir.clone();
        let tab_cwd = self
            .runtime
            .pane_tabs
            .as_ref()
            .map_or_else(|| column_dir.clone(), |t| t.active_info().cwd.clone());
        self.path_ref_bases_for(&tab_cwd)
            .into_iter()
            .map(|base| base.join(&path))
            .find(|p| p.exists())
            .unwrap_or_else(|| column_dir.join(&path))
    }

    /// Navigate spyc to a path matched in the pane (uppercase intent
    /// for a Path match). Mirrors `goto_file_navigate`'s post-resolve
    /// flow but starts from a pre-extracted path string rather than
    /// running pathref again.
    pub fn jump_to_pane_path(&mut self, raw: &str) {
        let resolved = self.resolve_pane_path(raw);
        if !resolved.exists() {
            self.state
                .flash_error(format!("path not found: {}", resolved.display()));
            return;
        }
        let (chdir_to, focus) = if resolved.is_dir() {
            (resolved, None)
        } else if let Some(parent) = resolved.parent() {
            (parent.to_path_buf(), Some(resolved.clone()))
        } else {
            self.state.flash_error("path has no parent dir");
            return;
        };
        if let Err(e) = self.state.chdir(&chdir_to) {
            self.state.flash_error(format!("chdir: {e:#}"));
            return;
        }
        if let Some(p) = focus {
            self.state.focus_on_path(&p);
        }
        self.state.focus = state::Focus::FileList;
        self.state.rebuild_rows();
        self.view.needs_full_repaint = true;
    }

    // ---- Path references (M13) ------------------------------------------------

    /// `gf` / `gF` — scan the active pane's visible output for a file path
    /// reference, navigate the file list there, and optionally open the
    /// pager at the referenced line.
    /// Resolve a path reference from already-read pane `lines` and navigate to
    /// it (chdir + focus); `open_at_line` (gF) also opens the file in the pager
    /// at the referenced line. The pickable read + the pane's cwd are supplied
    /// by the `ReadPaneText` / `GotoFile` executor (PR 5b) so the live-pane read
    /// lives in `run_effects` — this half stays pure of the Runtime handle.
    pub fn goto_file_navigate(
        &mut self,
        lines: Vec<String>,
        pane_cwd: PathBuf,
        open_at_line: bool,
    ) {
        let bases = self.path_ref_bases_for(&pane_cwd);

        // Debug: dump visible lines to the debug log so we can see what
        // the vt100 screen actually contains.
        spyc_debug!("gf: {} lines from pane, bases={bases:?}", lines.len());
        for (i, line) in lines.iter().enumerate() {
            if !line.trim().is_empty() {
                spyc_debug!("gf line[{i}]: {:?}", line);
            }
        }

        let Some(pathref) = find_path_ref(&lines, &bases) else {
            self.state
                .flash_error("no path reference found in pane output");
            return;
        };

        spyc_debug!(
            "gf: found path={}, line={:?}",
            pathref.path.display(),
            pathref.line
        );

        let path = pathref.path;
        let line = pathref.line;

        // Exit scroll mode and switch focus to the file list so the user
        // sees the navigation result.
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && tabs.active().is_scrolling()
        {
            tabs.active_mut().exit_scroll_mode();
        }
        self.state.focus = state::Focus::FileList;
        self.view.needs_full_repaint = true;

        // Navigate: if it's a directory, chdir there; if a file, chdir to
        // its parent and focus on it.
        if path.is_dir() {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("gf: {e:#}"));
            }
            return;
        }

        if let Some(parent) = path.parent() {
            if parent != self.state.cur().listing.dir
                && let Err(e) = self.state.chdir(parent)
            {
                self.state.flash_error(format!("gf: {e:#}"));
                return;
            }
            self.state.focus_on_path(&path);
        }

        // gF: also open the file in the pager at the referenced line.
        if open_at_line {
            // Route through `plan_pager_open`: a regular file builds + installs
            // inline here (bounded read, syntax/markdown render, lands at the
            // referenced line); a non-regular file (a hostile `/dev/zero` /
            // `/dev/input/*` reference scraped from pane output) reads on the
            // file-op worker so it can't block this thread. `gF` already runs
            // inside `run_effects`, so we spawn that worker directly rather than
            // returning the effect.
            let dest = PagerDest::Overlay {
                scroll: line.map(|ln| ln.saturating_sub(1)),
            };
            if let Some(op) = self.plan_pager_open(&path, None, dest) {
                self.spawn_file_op(op);
            }
        } else if let Some(ln) = line {
            self.state.flash_info(format!(
                "{}:{}",
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                ),
                ln
            ));
        }
    }

    pub(super) fn activate(&mut self, intent: ActivateIntent) -> Vec<Effect> {
        let Some(row) = self.state.cur().rows.get(self.state.cur().cursor.index) else {
            return Vec::new();
        };
        let path = row.path.clone();
        let kind = row.kind;
        let deleted = row.deleted;

        // A git-deleted "ghost" row points at a file that's gone from disk —
        // there's nothing to open or descend into. Surface the deletion instead
        // (and `gd` still shows what was removed).
        if deleted {
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            self.state
                .flash_info(format!("{name}: deleted — `gd` to view, `gr` to restore"));
            return Vec::new();
        }

        // Inventory view: enter drills down to the containing directory and
        // focuses on the item, then continues with the intent on that item.
        if self.state.cur().view == View::Inventory {
            let target_dir = if kind == EntryKind::Dir {
                path.clone()
            } else {
                path.parent()
                    .map_or_else(|| path.clone(), Path::to_path_buf)
            };
            if let Err(e) = self.state.chdir(&target_dir) {
                self.state.flash_error(format!("chdir: {e:#}"));
                return Vec::new();
            }
            self.state.cur_mut().view = View::Dir;
            self.state.focus_on_path(&path);
            self.state.rebuild_rows();
            if kind == EntryKind::Dir {
                return Vec::new();
            }
        }

        // Symlinks are classified by lstat (`DirEntry::metadata`),
        // so a symlink-to-dir comes through as `Symlink`, not `Dir`.
        // Resolve through to the target for navigation so Enter does
        // the obvious thing on `node_modules/foo -> .pnpm/...`. We
        // *don't* generalize this to every op — `R`, picks, etc.
        // intentionally operate on the link itself.
        let descend = kind == EntryKind::Dir
            || (kind == EntryKind::Symlink && crate::fs::target_is_dir(&path));

        if descend {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("chdir: {e:#}"));
            }
            return Vec::new();
        }

        // A member of a mounted archive: its bytes aren't on disk yet, so read
        // it through the mount rather than opening a path that doesn't exist.
        // A *mounted archive* is not a member of itself — it falls through to the
        // container branch below, so leaving one and coming back re-enters it.
        if self.state.mounts.holds_member(&path) {
            // A member that is itself a container: walk into it, the same as
            // `Enter` on an archive out on disk. Its bytes have to become a real
            // file first, so the mount rides the extraction.
            if matches!(intent, ActivateIntent::Display)
                && self.state.config.archive.enable
                && path
                    .file_name()
                    .is_some_and(|n| crate::archive::looks_mountable(&n.to_string_lossy()))
            {
                return self.open_member(
                    &path,
                    crate::app::archive_ops::MaterializeThen::Mount {
                        at: path.clone(),
                        then: None,
                    },
                );
            }
            return match intent {
                ActivateIntent::Display => self.open_member(
                    &path,
                    crate::app::archive_ops::MaterializeThen::OpenPager(PagerDest::Overlay {
                        scroll: None,
                    }),
                ),
                // Edit the extracted copy; the next write-back notices the
                // change and puts it back.
                ActivateIntent::Edit => self.open_member(
                    &path,
                    crate::app::archive_ops::MaterializeThen::Edit { in_pane: false },
                ),
            };
        }

        // A container: walk into it instead of paging its bytes. The name is only
        // a pre-filter — the worker sniffs the magic and hands back a plain file
        // when the extension lied, so `Enter` still opens `notes.zip` if that's
        // what it really is.
        if self.state.config.archive.enable
            && path
                .file_name()
                .is_some_and(|n| crate::archive::looks_mountable(&n.to_string_lossy()))
        {
            return self.request_mount(&path, false);
        }

        // File: dispatch based on intent.
        match intent {
            ActivateIntent::Display => {
                // An image goes to the full-screen overlay instead of the pager
                // — a hex dump of a PNG answers nothing the picture doesn't.
                if let Some(effect) = self.plan_image_open(&path) {
                    return vec![effect];
                }
                // Regular file → built + installed inline; a special file (a
                // device under the cursor in `/dev`) → read off-thread via the
                // returned `OpenSpecialFile` effect, so a blocking read can't
                // freeze the input thread.
                self.plan_pager_open(&path, None, PagerDest::Overlay { scroll: None })
                    .map(Effect::FileOp)
                    .into_iter()
                    .collect()
            }
            ActivateIntent::Edit => {
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    return Vec::new();
                }
                let program = argv.remove(0);
                argv.push(path.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
                .into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// gF on a real file routes through `build_pager_view_for_file` (the
    /// bounded, syntax/markdown-aware open path) instead of the old unbounded
    /// `read_to_string`, and lands the pager at the referenced line.
    #[test]
    fn gf_opens_regular_file_at_referenced_line() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = tmp.path().to_path_buf();
            std::fs::write(dir.join("foo.txt"), "l1\nl2\nl3\nl4\n").unwrap();
            let mut app = App::test_app(dir.clone());

            // A pane line referencing the file at line 3 (compiler/grep shape).
            app.goto_file_navigate(vec!["foo.txt:3".to_string()], dir.clone(), true);

            let pager = app.view.pager.as_ref().expect("gF opened a pager");
            assert_eq!(
                pager.source_path.as_deref(),
                Some(dir.join("foo.txt").as_path()),
                "pager is backed by the referenced file"
            );
            assert_eq!(pager.scroll, 2, "scroll jumps to line 3 (0-indexed)");
        });
    }

    /// An app whose column lists `dir`, read straight in: `test_app` starts
    /// empty, and `chdir` would `set_current_dir`.
    fn app_listing(dir: &Path) -> App {
        let mut app = App::test_app(dir.to_path_buf());
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(dir).unwrap();
        app.state.rebuild_rows();
        app
    }

    /// The file the focused column's cursor is on.
    fn cursor_file(app: &App) -> PathBuf {
        let col = app.state.cur();
        col.rows
            .get(col.cursor.index)
            .expect("a cursor row")
            .path
            .clone()
    }

    /// A repo at `<tmp>/repo` with `src/main.rs` and `src/app/x.rs`, plus a
    /// decoy `src/app/a.rs` that sorts first, so the cursor lands on `x.rs`
    /// only if `gf` put it there.
    /// Canonical, as a pane's cwd is: the listing holds canonical paths, and
    /// macOS's temp dir sits behind the `/var` symlink.
    ///
    /// The tests below open their column in the target's own directory: `gf`
    /// then lands without a `chdir`, which `set_current_dir`s, and a process
    /// cwd left in a dropped temp dir breaks every gix test running beside it.
    fn repo(tmp: &Path) -> PathBuf {
        let root = tmp.canonicalize().unwrap().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src/app")).unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("src/app/x.rs"), "").unwrap();
        std::fs::write(root.join("src/app/a.rs"), "").unwrap();
        root
    }

    /// #6: a tool run from a subdirectory (`cargo` in `src/`) prints paths
    /// relative to the worktree root, which neither the pane's cwd nor the
    /// column's directory resolves.
    #[test]
    fn gf_resolves_a_path_relative_to_the_panes_worktree_root() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let root = repo(tmp.path());
            let mut app = app_listing(&root.join("src/app"));
            let lines = vec!["error[E0308]: --> src/app/x.rs:3:5".to_string()];
            app.goto_file_navigate(lines, root.join("src"), false);
            assert_eq!(cursor_file(&app), root.join("src/app/x.rs"));
        });
    }

    /// With nothing nearer resolving a path, PROJECT_HOME does.
    #[test]
    fn gf_falls_back_to_project_home() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let base = tmp.path().canonicalize().unwrap();
            let home = base.join("project");
            std::fs::create_dir_all(home.join("docs")).unwrap();
            std::fs::write(home.join("docs/a.md"), "").unwrap();
            std::fs::write(home.join("docs/0.md"), "").unwrap();
            let elsewhere = base.join("elsewhere");
            std::fs::create_dir(&elsewhere).unwrap();
            let mut app = app_listing(&home.join("docs"));
            app.state.project_home = Some(home.clone());
            app.goto_file_navigate(vec!["see docs/a.md".to_string()], elsewhere, false);
            assert_eq!(cursor_file(&app), home.join("docs/a.md"));
        });
    }

    /// The newest line wins whichever base resolves it: an older path under the
    /// pane's cwd doesn't beat a newer one only the worktree root resolves.
    #[test]
    fn gf_takes_the_newest_path_whichever_base_resolves_it() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let root = repo(tmp.path());
            let mut app = app_listing(&root.join("src/app"));
            let lines = vec!["main.rs".to_string(), "src/app/x.rs:3".to_string()];
            app.goto_file_navigate(lines, root.join("src"), false);
            assert_eq!(cursor_file(&app), root.join("src/app/x.rs"));
        });
    }

    /// `^a u`'s uppercase open resolves a picked path against the same bases.
    #[test]
    fn a_quick_select_open_resolves_against_the_worktree_root() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let root = repo(tmp.path());
            let app = app_listing(&root.join("src/app"));
            assert_eq!(
                app.resolve_pane_path("src/app/x.rs"),
                root.join("src/app/x.rs")
            );
        });
    }

    /// gF refuses a non-regular target (here: a path that doesn't resolve to a
    /// regular file) rather than reading it — the guard against a hostile
    /// /dev/zero / FIFO in pane output. Uses a directory reference, which is
    /// handled as a chdir, never an open: no pager is created.
    #[test]
    fn gf_directory_reference_does_not_open_a_pager() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            // Cargo's own cwd for the test binary: the `chdir` this makes
            // `set_current_dir`s, and into a temp dir it would leave every
            // gix test after it in a deleted directory.
            let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let dir = tmp.path().to_path_buf();
            let mut app = App::test_app(dir.clone());

            app.goto_file_navigate(vec![target.display().to_string()], dir, true);

            assert!(
                app.view.pager.is_none(),
                "a directory reference chdirs, never opens a file pager"
            );
        });
    }

    /// gF (and Enter / `D`, via the shared `plan_pager_open`) must never read a
    /// non-regular file on the input thread — a char device / FIFO can block or
    /// stream forever (the reported Enter-on-/dev/stderr lockup, and the exotic
    /// blocking-read device behind it). The open is *diverted off-thread*: the
    /// planner returns an `OpenSpecialFile` op (run on the file-op worker)
    /// instead of building inline, and nothing is paged synchronously. The
    /// refusal/sample of the file itself is covered by the `file_ops` worker +
    /// drain tests. A unix socket is a portable non-regular stand-in.
    #[cfg(unix)]
    #[test]
    fn gf_diverts_non_regular_file_off_the_input_thread() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let dir = tmp.path().to_path_buf();
            let sock = dir.join("s.sock");
            let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
            let mut app = App::test_app(dir);

            let op = app.plan_pager_open(&sock, None, PagerDest::Overlay { scroll: None });
            assert!(
                matches!(
                    op,
                    Some(crate::app::file_ops::FileOp::OpenSpecialFile { .. })
                ),
                "a socket is read off-thread, never inline on the input thread"
            );
            assert!(
                app.view.pager.is_none(),
                "nothing is paged synchronously for a non-regular file"
            );
        });
    }
}

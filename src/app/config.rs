//! Config + filesystem-watch path classification: reload `.spycrc.toml`,
//! enumerate the watched config paths, and decide whether a `notify` event
//! path is a config file or a listing-dir change. Extracted verbatim from
//! `app/mod.rs` (the impl-extraction sweep), same child-module `impl App`
//! pattern. All four are `pub` — `reload_config`/`is_config_path`/
//! `is_listing_path` are called from `actions`/`sources`, and
//! `candidate_config_paths` from `run_setup` in the parent module.

use std::path::{Path, PathBuf};

use super::state::ProjectId;
use super::{App, Config, FlashKind, IgnoreMasks, Theme, UserKeymap};

impl App {
    /// Reload every open project's config from its own rc and rebuild its
    /// keymap, the project in view last, so its theme is the one in force;
    /// then re-run `init.lua` once. A project whose rc fails keeps its old
    /// config. A problem in another project's rc is the flash, named for it,
    /// over the "reloaded" note of the one in view; one in the project in view
    /// wins over both.
    pub fn reload_config(&mut self) {
        let files = crate::config::project_file::scan();
        let active = self.state.active;
        let others: Vec<ProjectId> = self
            .state
            .open_projects()
            .map(|p| p.id)
            .filter(|id| *id != active)
            .collect();
        let mut problem = None;
        for id in others {
            if let Some(flash) = self.reload_elsewhere(id, &files, true)
                && problem.is_none()
            {
                problem = Some(flash);
            }
        }
        self.reload_project_config(&files, true);
        let users_problem = self
            .state
            .flash
            .as_ref()
            .is_some_and(|f| matches!(f.kind, FlashKind::Error));
        if let Some(problem) = problem.filter(|_| !users_problem) {
            self.state.flash = Some(problem);
        }
        // The rebuilt keymaps dropped the synthetic `@map:` entries, so re-run
        // `init.lua` to re-register them (silent when there's no `init.lua` —
        // a configless `^R` shouldn't flash about Lua).
        self.reload_init_lua(false);
    }

    /// Reload project `id`, out of view, as [`Self::reload_project_config`]
    /// does, and return its problem as an error flash named for it. The flash
    /// the user was reading stays theirs.
    fn reload_elsewhere(
        &mut self,
        id: ProjectId,
        files: &[crate::config::project_file::Entry],
        announce: bool,
    ) -> Option<super::FlashMessage> {
        let flash = self.in_project(id, |app| {
            app.reload_project_config(files, announce);
            app.state.flash.take()
        });
        let mut flash = flash.filter(|f| matches!(f.kind, FlashKind::Error))?;
        let label = self.state.label_of(id).unwrap_or_default();
        flash.text = format!("{label}: {}", flash.text);
        Some(flash)
    }

    /// Reload the active project's config, from `files` the user's own file
    /// for it. The project rc is read from its **start dir**, not the
    /// directory the user has browsed into (`listing.dir`): otherwise `^R`
    /// while sitting in a hostile directory would load that directory's
    /// `.spycrc.toml`, re-opening the very keypress-RCE vector that
    /// `load_default`'s project-trust gating closes. (Executing project
    /// bindings are dropped regardless; pinning the dir keeps even the
    /// cosmetic settings anchored to the project.) `announce` says "reloaded
    /// N"; a problem is said either way.
    fn reload_project_config(
        &mut self,
        files: &[crate::config::project_file::Entry],
        announce: bool,
    ) {
        let (start_dir, anchor) = self.state.active_project().config_inputs();
        self.state.active_project_mut().config_key = Some((start_dir.clone(), anchor.clone()));
        match Config::load_for_project(&start_dir, &anchor, files) {
            Ok(new_config) if announce => self.adopt_config(new_config),
            Ok(new_config) => {
                let warnings = self.install_config(new_config, true);
                let mut keymap = std::mem::take(&mut self.state.active_project_mut().user_keymap);
                self.install_lua_maps(&mut keymap);
                self.state.active_project_mut().user_keymap = keymap;
                if !warnings.is_empty() {
                    self.state
                        .flash_error(format!("config: {}", warnings.join("; ")));
                }
            }
            Err(e) => self.state.flash_error(format!("config error: {e:#}")),
        }
    }

    /// Read again, quietly, the config of each project whose start directory
    /// or anchor moved since its config was read (`Project::config_key`):
    /// `Space P`, `:project`, `gS` and `-r` re-homing a project bring in the
    /// files that name its new place, unless the same files would be read
    /// (`Space P` with the start dir unchanged and no other file naming the
    /// new home), when only the key moves. A problem flashes, named for a
    /// project out of view. Whether anything was read, for a redraw.
    pub(super) fn settle_project_configs(&mut self) -> bool {
        let moved: Vec<ProjectId> = self
            .state
            .open_projects()
            .filter(|p| {
                p.config_key
                    .as_ref()
                    .is_some_and(|k| *k != p.config_inputs())
            })
            .map(|p| p.id)
            .collect();
        if moved.is_empty() {
            return false;
        }
        let files = crate::config::project_file::scan();
        let mut read = false;
        for id in moved {
            let Some(project) = self.state.project(id) else {
                continue;
            };
            let ((start_dir, anchor), key) = (project.config_inputs(), project.config_key.clone());
            // The same files would be read: only the key moves.
            let file = crate::config::project_file::find(&files, &anchor).0;
            if key
                .as_ref()
                .is_some_and(|(read_at, _)| *read_at == start_dir)
                && file.map(|f| f.path) == project.config.project.as_ref().map(|f| f.path.clone())
            {
                if let Some(project) = self.state.open_projects_mut().find(|p| p.id == id) {
                    project.config_key = Some((start_dir, anchor));
                }
                continue;
            }
            read = true;
            if id == self.state.active {
                self.reload_project_config(&files, false);
            } else if let Some(flash) = self.reload_elsewhere(id, &files, false) {
                self.state.flash = Some(flash);
            }
        }
        read
    }

    /// Put a freshly loaded config in force, saying how many files it read,
    /// or what was wrong in them.
    pub(super) fn adopt_config(&mut self, new_config: Config) {
        let count = new_config.sources.len();
        let warnings = self.install_config(new_config, false);
        // Non-fatal problems (bad scan-pattern regex, etc.) win over
        // the success note so a typo is visible the moment the rc is
        // saved, not only under --debug.
        if warnings.is_empty() {
            self.state
                .flash_info(format!("reloaded {count} config file(s)"));
        } else {
            self.state
                .flash_error(format!("config: {}", warnings.join("; ")));
        }
    }

    /// Put `new_config` in force: keymap, theme, masks, and what `:agent hud`
    /// chose unless the file changed `agent_hud` itself. Returns its warnings.
    /// A `moved` project's read, which no edit asked for, keeps the user's
    /// `:agent hud` choice whatever the files say, and the columns' mask
    /// toggles while the masks the files set are the same.
    fn install_config(&mut self, new_config: Config, moved: bool) -> Vec<String> {
        self.state.active_project_mut().user_keymap =
            UserKeymap::from_bindings(new_config.bindings.clone());
        // The theme is the project in view's; another project's colours take
        // effect when the user switches to it (`switch_project`).
        if !self.state.in_background {
            let mono = self.view.theme.mono;
            self.view.theme = Theme::default().with_overrides(&new_config.colors);
            self.view.theme.mono = mono;
        }
        // Reset to built-in mask defaults first, then apply config
        // overrides — so removing `[[ignore_masks]]` entries from
        // the rc file reverts the group to defaults on reload.
        let mut masks = IgnoreMasks::default();
        masks.apply_config(&new_config.ignore_masks);
        let warnings = new_config.warnings.clone();
        let before = &self.state.active_project().config;
        let (before_hud, same_masks) = (
            before.pane.agent_hud,
            before.ignore_masks == new_config.ignore_masks,
        );
        if !moved {
            self.state
                .active_project_mut()
                .pane
                .agent_hud
                .config_reloaded(before_hud, new_config.pane.agent_hud);
        }
        self.state.active_project_mut().config = new_config;
        if !(moved && same_masks) {
            let sides: Vec<_> = self.state.active_sides().collect();
            for side in sides {
                self.state.col_mut(side).masks = masks.clone();
                self.state.rebuild_rows_for(side);
            }
        }
        warnings
    }

    /// Candidate config paths — used by the file watcher. We watch the
    /// directories holding these even when the files don't exist yet so
    /// that `touch ~/.spycrc.toml` picks up immediately.
    pub fn candidate_config_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(home) = crate::config::home_dir() {
            out.push(home.join(".spycrc.toml"));
        }
        // Anchor on each open project's start dir, not a browsed
        // `listing.dir`, so the project rcs we watch (and reload on change)
        // match what `reload_config` actually loads — and browsing into a
        // hostile directory never causes us to watch/react to its
        // `.spycrc.toml`.
        out.extend(
            self.state
                .open_projects()
                .map(|p| p.start_dir.join(".spycrc.toml")),
        );
        // The Lua entry point: a watch event on it routes through
        // `reload_config` (which re-runs init.lua), so editing init.lua takes
        // effect without `^R` / `:lua reload`. Watching its parent
        // (`~/.config/spyc/`) also covers edits to `lua/<name>.lua` scripts
        // that a `map KEY lua <name>` binding re-reads on each run anyway.
        if let Some(init) = crate::state::config_root().map(|r| r.join("init.lua")) {
            out.push(init);
        }
        // Each project's own file, or the one `:project config` would make:
        // a new file may name any project, so the directory is watched.
        if let Some(dir) = crate::config::project_file::dir() {
            out.extend(
                self.state
                    .open_projects()
                    .map(|p| dir.join(format!("{}.toml", p.label()))),
            );
        }
        out
    }

    /// The parent dirs of [`Self::candidate_config_paths`], each once: what
    /// the watch worker watches, since an editor that replaces a file on save
    /// removes the inode a file watch would follow. One that doesn't exist yet
    /// is watched once it does: the worker retries it on every resend.
    pub(super) fn config_parents(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for parent in self
            .candidate_config_paths()
            .iter()
            .filter_map(|p| p.parent())
        {
            if !out.iter().any(|o| o == parent) {
                out.push(parent.to_path_buf());
            }
        }
        out
    }

    pub fn is_config_path(&self, path: &Path) -> bool {
        self.candidate_config_paths().iter().any(|c| c == path)
            || self
                .state
                .open_projects()
                .any(|p| p.config.sources.iter().any(|c| c == path))
            || is_project_file(path)
    }

    /// True iff `path` is the listing directory or anything beneath it
    /// that we care about for refresh purposes. `notify` events sometimes
    /// include just the directory and sometimes the affected child;
    /// recursive listing watches (since v1.21.7) also send events for
    /// arbitrary depths, so we accept the whole subtree -- with
    /// `.git/` carved out for tighter filtering since rebase/gc/pack
    /// activity inside there would otherwise spam refresh.
    pub fn is_listing_path(&self, path: &Path) -> bool {
        // Ignore OUR OWN context-file writes -- the ~150ms snapshot churn lands
        // in the listing dir and would otherwise spin refresh_listing →
        // git-status → redraw. Only *our* pid's files (every project's, and
        // their write-temps) are filtered: other instances' context files
        // appearing / vanishing (e.g. another spyc's startup orphan sweep) are
        // real listing changes the user must see, so they pass through (the
        // refresh debounce bounds any residual churn from a second active
        // instance).
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(crate::context::context_file_pid)
            == Some(std::process::id())
        {
            return false;
        }
        // Accept if the path is a listing path for EITHER active column — both
        // columns' trees are watched (dual fs-watch), so `b`'s edits refresh
        // its markers too. `current_gitdir` is per-column.
        self.state.active_sides().any(|side| {
            let c = self.state.col(side);
            listing_path_in(path, &c.listing.dir, c.git_cache.current_gitdir.as_deref())
        })
    }

    /// True iff `path` is the file currently shown in the vertical-split
    /// preview (`view.right_pager.source_path`). Drives the live reload: a
    /// watcher event on this exact path re-renders the right column. Exact
    /// match — the preview tracks one file, and the watched parent dir delivers
    /// events for all its siblings, so anything looser would over-trigger.
    pub fn is_preview_path(&self, path: &Path) -> bool {
        self.view
            .project()
            .right_pager
            .as_ref()
            .and_then(|v| v.source_path.as_deref())
            .is_some_and(|src| src == path)
    }

    /// True for a watched-gitdir change that signals a discrete git
    /// operation — `index` (stage/commit/checkout) or `HEAD` (branch
    /// switch / commit), or the gitdir itself (macOS FSEvents sometimes
    /// coalesces intra-dir changes onto the dir path). Unlike a
    /// working-tree edit, these aren't bursty churn the trailing debounce
    /// exists to coalesce — they're one-shot events whose markers should
    /// refresh immediately. Subset of `is_listing_path`'s gitdir arm; kept
    /// separate so the caller can give git-state events a no-debounce path.
    pub fn is_gitdir_status_path(&self, path: &Path) -> bool {
        // Either column's gitdir (per-column; both are watched).
        self.state.active_sides().any(|side| {
            gitdir_status_in(
                path,
                self.state.col(side).git_cache.current_gitdir.as_deref(),
            )
        })
    }
}

/// Per-column `is_listing_path` test: is `path` a listing-relevant event for a
/// column rooted at `dir` with resolved `gitdir`? Accepts anything at/below the
/// listing dir (recursive watch) — the 500 ms debounce + git-status index-cache
/// bound noisy subtrees — but inside the gitdir only the discrete signals
/// (`index`/`HEAD`, or the gitdir itself for FSEvents coalescing), rejecting
/// `.git/objects` / pack / lockfile / refs churn.
fn listing_path_in(path: &Path, dir: &Path, gitdir: Option<&Path>) -> bool {
    if let Some(git_dir) = gitdir {
        if path == git_dir {
            return true;
        }
        if path.starts_with(git_dir) {
            if path.parent() == Some(git_dir)
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
            {
                return matches!(name, "index" | "HEAD");
            }
            return false;
        }
    }
    path.starts_with(dir)
}

/// Per-column `is_gitdir_status_path` test: a discrete git-state signal in this
/// column's `gitdir` — `index`/`HEAD`, or the gitdir itself (FSEvents coalesces
/// intra-dir changes onto the dir path).
fn gitdir_status_in(path: &Path, gitdir: Option<&Path>) -> bool {
    let Some(git_dir) = gitdir else {
        return false;
    };
    if path == git_dir {
        return true;
    }
    path.parent() == Some(git_dir)
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| matches!(name, "index" | "HEAD"))
}

impl App {
    /// `:project config`: open the user's own config file for the project
    /// anchored at `anchor` in the editor, in the top overlay. With none,
    /// `file`, one is made first (`project_file::create`), read so the project
    /// goes by it, and its directory watched. Refused while an editor is open
    /// in the project, whose unsaved work a second one would end. A failure
    /// flashes with its cause.
    pub(super) fn edit_project_config(
        &mut self,
        anchor: &Path,
        label: &str,
        file: Option<PathBuf>,
    ) {
        let project = self.runtime.project();
        let editing = [&project.top_overlay, &project.top_overlay_right]
            .into_iter()
            .flatten()
            .any(|pane| !pane.is_closed() && pane.exit_status().is_none());
        if editing {
            self.state
                .flash_info("project config: an editor is open here; close it first");
            return;
        }
        let file = file.filter(|path| path.is_file());
        let Some(path) = file.or_else(|| self.make_project_config(anchor, label)) else {
            return;
        };
        self.edit_path_in_pane(&path);
    }

    /// Make the user's file for the project anchored at `anchor`, named
    /// `label`, read it so the project goes by it, and have its directory
    /// watched. `None`, flashed with its cause, when it can't be made.
    fn make_project_config(&mut self, anchor: &Path, label: &str) -> Option<PathBuf> {
        let Some(dir) = crate::config::project_file::dir() else {
            self.state
                .flash_error("project config: neither $XDG_CONFIG_HOME nor $HOME is set");
            return None;
        };
        // One made since the project's config was read is the project's.
        let found =
            crate::config::project_file::find(&crate::config::project_file::scan_dir(&dir), anchor);
        let made = found.0.map_or_else(
            || crate::config::project_file::create(&dir, anchor, label),
            |file| Ok(file.path),
        );
        match made {
            Ok(path) => {
                self.view.config_watch_gen += 1;
                self.reload_config();
                let erred = self
                    .state
                    .flash
                    .as_ref()
                    .is_some_and(|f| matches!(f.kind, FlashKind::Error));
                if !erred {
                    self.state.flash_info(format!(
                        "project config: {}",
                        crate::paths::display_tilde(&path)
                    ));
                }
                Some(path)
            }
            Err(e) => {
                self.state.flash_error(format!("project config: {e:#}"));
                None
            }
        }
    }
}

/// Whether `path` is a `*.toml` directly in the project files' directory,
/// through a symlink to either: any one may name an open project.
fn is_project_file(path: &Path) -> bool {
    let Some(dir) = crate::config::project_file::dir() else {
        return false;
    };
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    path.extension().is_some_and(|x| x == "toml")
        && path
            .parent()
            .is_some_and(|p| p == dir || canonical(p) == canonical(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_gitdir_status_path` accepts exactly the discrete git-state
    /// signals (`index`/`HEAD`, or the gitdir itself) and rejects gitdir
    /// housekeeping + working-tree paths — so the caller can give those a
    /// no-debounce refresh without firing on every `.git/objects` write.
    #[test]
    fn gitdir_status_path_matches_index_head_and_dir_only() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        app.state.active_project_mut().left.git_cache.current_gitdir =
            Some(PathBuf::from("/repo/.git"));

        // The discrete-operation signals.
        assert!(app.is_gitdir_status_path(Path::new("/repo/.git/index")));
        assert!(app.is_gitdir_status_path(Path::new("/repo/.git/HEAD")));
        // macOS FSEvents sometimes coalesces onto the dir path itself.
        assert!(app.is_gitdir_status_path(Path::new("/repo/.git")));

        // Housekeeping under the gitdir — must NOT trigger an immediate refresh.
        assert!(!app.is_gitdir_status_path(Path::new("/repo/.git/objects/ab/cd")));
        assert!(!app.is_gitdir_status_path(Path::new("/repo/.git/refs/heads/main")));
        assert!(!app.is_gitdir_status_path(Path::new("/repo/.git/index.lock")));
        // Working-tree paths go through the normal debounced listing refresh.
        assert!(!app.is_gitdir_status_path(Path::new("/repo/src/main.rs")));
    }

    /// Outside a repo (no cached gitdir) nothing is a git-status path.
    #[test]
    fn gitdir_status_path_false_without_gitdir() {
        let app = App::test_app(PathBuf::from("/repo"));
        assert!(
            app.state
                .active_project()
                .left
                .git_cache
                .current_gitdir
                .is_none()
        );
        assert!(!app.is_gitdir_status_path(Path::new("/repo/.git/index")));
    }

    /// Only OUR OWN context file (and its write-temp) is filtered from the
    /// listing refresh — another instance's context file appearing/vanishing
    /// in the cwd is a real change the user must see, and an ordinary file is
    /// always relevant. (The cwd is an always-current class.)
    #[test]
    fn listing_path_filters_only_our_own_context_file() {
        let app = App::test_app(PathBuf::from("/repo"));
        let our = std::process::id();
        let ours_json = format!("/repo/.spyc-context-{our}.json");
        let ours_tmp = format!("/repo/.spyc-context-{our}.tmp");
        let other = format!("/repo/.spyc-context-{}.json", our.wrapping_add(1));

        assert!(
            !app.is_listing_path(Path::new(&ours_json)),
            "our own write churn → suppressed"
        );
        assert!(
            !app.is_listing_path(Path::new(&ours_tmp)),
            "our write-temp → suppressed"
        );
        assert!(
            app.is_listing_path(Path::new(&other)),
            "another instance's file → visible"
        );
        assert!(
            app.is_listing_path(Path::new("/repo/notes.txt")),
            "ordinary cwd file → relevant"
        );
        // #99: each project has its own file, and its write-temp.
        let project = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5a6b";
        for ours in ["json", "tmp"].map(|ext| format!("/repo/.spyc-context-{our}-{project}.{ext}"))
        {
            assert!(
                !app.is_listing_path(Path::new(&ours)),
                "{ours} → suppressed"
            );
        }
        let other = format!("/repo/.spyc-context-{}-{project}.json", our.wrapping_add(1));
        assert!(app.is_listing_path(Path::new(&other)), "{other} → visible");
    }

    /// `is_listing_path`'s gitdir carve-out: ACCEPT `.git/index` / `.git/HEAD`
    /// (and the gitdir itself, for FSEvents coalescing) plus the working tree,
    /// but REJECT `.git/objects` / `refs` / lockfile churn. That rejection is
    /// the whole reason the predicate filters `.git/` (rebase/gc/pack spam →
    /// refresh spin), yet it was only tested via the SEPARATE
    /// `is_gitdir_status_path` — and `is_listing_path` ALSO accepts the working
    /// tree, so the two diverge and need independent coverage.
    #[test]
    fn listing_path_accepts_index_head_rejects_object_churn() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        app.state.active_project_mut().left.git_cache.current_gitdir =
            Some(PathBuf::from("/repo/.git"));

        // Accepted: the two refresh-worthy gitdir files + the gitdir itself.
        assert!(app.is_listing_path(Path::new("/repo/.git/index")));
        assert!(app.is_listing_path(Path::new("/repo/.git/HEAD")));
        assert!(app.is_listing_path(Path::new("/repo/.git")));
        // Accepted: ordinary working-tree files (the arm gitdir_status lacks).
        assert!(app.is_listing_path(Path::new("/repo/src/main.rs")));
        // Rejected: object/pack/ref/lockfile churn under .git — else every
        // rebase/gc/pack write would spin refresh_listing → git-status → redraw.
        assert!(!app.is_listing_path(Path::new("/repo/.git/objects/ab/cd")));
        assert!(!app.is_listing_path(Path::new("/repo/.git/refs/heads/main")));
        assert!(!app.is_listing_path(Path::new("/repo/.git/index.lock")));
    }

    /// `is_preview_path` matches exactly the open vertical-split preview's
    /// source file — nothing when no preview is open, and not a sibling.
    #[test]
    fn preview_path_matches_only_the_open_preview_source() {
        let mut app = App::test_app(PathBuf::from("/repo"));
        // No split open → nothing is a preview path.
        assert!(!app.is_preview_path(Path::new("/repo/doc.md")));

        let mut view = crate::ui::pager::PagerView::new_plain("doc.md".to_string(), Vec::new());
        view.source_path = Some(PathBuf::from("/repo/doc.md"));
        app.view.project_mut().right_pager = Some(view);
        assert!(app.is_preview_path(Path::new("/repo/doc.md")));
        assert!(
            !app.is_preview_path(Path::new("/repo/other.md")),
            "a sibling in the same (watched) dir is not the preview"
        );
    }
}

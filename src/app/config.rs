//! Config + filesystem-watch path classification: reload `.spycrc.toml`,
//! enumerate the watched config paths, and decide whether a `notify` event
//! path is a config file or a listing-dir change. Extracted verbatim from
//! `app/mod.rs` (the impl-extraction sweep), same child-module `impl App`
//! pattern. All four are `pub` — `reload_config`/`is_config_path`/
//! `is_listing_path` are called from `actions`/`sources`, and
//! `candidate_config_paths` from `run_setup` in the parent module.

use std::path::{Path, PathBuf};

use super::{App, Config, IgnoreMasks, Theme, UserKeymap};

impl App {
    /// Reload `.spycrc.toml` and rebuild the user keymap. Leaves the old
    /// config in place on failure and flashes the error.
    pub fn reload_config(&mut self) {
        // Reload the project rc from the **startup** cwd, not the directory
        // the user has browsed into (`listing.dir`). Otherwise `^R` while
        // sitting in a hostile directory would load that directory's
        // `.spycrc.toml` — re-opening the very keypress-RCE vector that
        // load_default's project-trust gating closes. (Executing project
        // bindings are dropped regardless; pinning the dir keeps even the
        // cosmetic settings load anchored to where spyc was launched.)
        match Config::load_default(&self.state.start_dir) {
            Ok(new_config) => {
                self.state.user_keymap = UserKeymap::from_bindings(new_config.bindings.clone());
                self.view.theme = Theme::default().with_overrides(&new_config.colors);
                // Reset to built-in mask defaults first, then apply config
                // overrides — so removing `[[ignore_masks]]` entries from
                // the rc file reverts the group to defaults on reload.
                self.state.left.masks = IgnoreMasks::default();
                self.state.left.masks.apply_config(&new_config.ignore_masks);
                let count = new_config.sources.len();
                let warnings = new_config.warnings.clone();
                self.state.config = new_config;
                self.state.rebuild_rows();
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
            Err(e) => self.state.flash_error(format!("config error: {e}")),
        }
    }

    /// Candidate config paths — used by the file watcher. We watch the
    /// directories holding these even when the files don't exist yet so
    /// that `touch ~/.spycrc.toml` picks up immediately.
    pub fn candidate_config_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join(".spycrc.toml"));
        }
        // Anchor on the startup cwd, not the browsed `listing.dir`, so the
        // project rc we watch (and reload on change) matches what
        // `reload_config` actually loads — and browsing into a hostile
        // directory never causes us to watch/react to its `.spycrc.toml`.
        out.push(self.state.start_dir.join(".spycrc.toml"));
        out
    }

    pub fn is_config_path(&self, path: &Path) -> bool {
        self.candidate_config_paths().iter().any(|c| c == path)
            || self.state.config.sources.iter().any(|c| c == path)
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
        // git-status → redraw. Only *our* pid's file (and its write-temp
        // sibling) is filtered: other instances' context files appearing /
        // vanishing (e.g. another spyc's startup orphan sweep) are real listing
        // changes the user must see, so they pass through (the refresh debounce
        // bounds any residual churn from a second active instance).
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let pid = std::process::id();
            if name == format!(".spyc-context-{pid}.json")
                || name == format!(".spyc-context-{pid}.tmp")
            {
                return false;
            }
        }
        let dir = self.state.left.listing.dir.as_path();

        // `.git/` filtering against the repo's *resolved* gitdir
        // (cached on chdir). For a normal repo that's `<root>/.git`;
        // for a linked worktree it's `<main>/.git/worktrees/<name>/`
        // (the `.git` here is a *file*, and the real index/HEAD live
        // outside the working tree). macOS FSEvents sometimes coalesces
        // intra-directory changes into a single event whose path *is*
        // the gitdir itself, so accept that as "something happened in
        // there, refresh." Direct children: only `index` (staging/
        // status) or `HEAD` (branch switch) -- everything else (objects,
        // packs, lockfiles, gc activity, refs/, logs/) is rejected so
        // background git housekeeping doesn't cascade.
        if let Some(git_dir) = self.state.left.git_cache.current_gitdir.as_deref() {
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

        // Anywhere at or below the listing dir (recursive watch) --
        // accept. The 500ms trailing debounce + git-status's index-
        // cache mean even noisy subtrees don't produce unbounded
        // refresh/status work.
        path.starts_with(dir)
    }

    /// True iff `path` is the file currently shown in the vertical-split
    /// preview (`view.right_pager.source_path`). Drives the live reload: a
    /// watcher event on this exact path re-renders the right column. Exact
    /// match — the preview tracks one file, and the watched parent dir delivers
    /// events for all its siblings, so anything looser would over-trigger.
    pub fn is_preview_path(&self, path: &Path) -> bool {
        self.view
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
        let Some(git_dir) = self.state.left.git_cache.current_gitdir.as_deref() else {
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
        app.state.left.git_cache.current_gitdir = Some(PathBuf::from("/repo/.git"));

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
        assert!(app.state.left.git_cache.current_gitdir.is_none());
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
        app.view.right_pager = Some(view);
        assert!(app.is_preview_path(Path::new("/repo/doc.md")));
        assert!(
            !app.is_preview_path(Path::new("/repo/other.md")),
            "a sibling in the same (watched) dir is not the preview"
        );
    }
}

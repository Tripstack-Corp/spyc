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
                self.state.masks = IgnoreMasks::default();
                self.state.masks.apply_config(&new_config.ignore_masks);
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
        // Ignore our own context file writes -- they land in the
        // listing directory and would otherwise trigger a self-
        // perpetuating refresh_listing → git-status → redraw cycle.
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.starts_with(".spyc-context-")
        {
            return false;
        }
        let dir = self.state.listing.dir.as_path();

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
        if let Some(git_dir) = self.state.git_cache.current_gitdir.as_deref() {
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
}

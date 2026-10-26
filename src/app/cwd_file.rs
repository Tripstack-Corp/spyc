//! `--cwd-file` (#63): a quit writes the directory the file list shows, for a
//! shell wrapper to `cd` into once spyc has exited.
//!
//! The write runs where [`App::request_quit`] confirms a quit, beside the
//! session save, so every normal quit writes it and nothing else does. The
//! file holds the path alone, with no newline, and is replaced by a rename, so
//! a wrapper never reads half a path.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use super::App;
use super::state::Mounts;

impl App {
    /// Where a confirmed quit writes the browsed directory: `--cwd-file`, else
    /// `$SPYC_CWD_FILE`, as `spyc::run` resolved it.
    pub fn set_cwd_file(&mut self, path: Option<PathBuf>) {
        self.runtime.cwd_file = path;
    }

    /// Write the focused column's directory, in the project in view, to the
    /// cwd file if there is one. A failure joins the exit summary; the quit
    /// goes ahead.
    pub(super) fn write_cwd_file(&mut self) {
        let Some(target) = self.runtime.cwd_file.as_deref() else {
            return;
        };
        let dir = shell_dir(&self.state.cur().listing.dir, &self.state.mounts);
        let Err(e) = crate::fs::atomic::write_atomic(target, dir.as_os_str().as_bytes()) else {
            return;
        };
        let note = format!(
            "cwd NOT written to {} ({e:#})",
            crate::paths::display_tilde(target)
        );
        self.exit_summary = Some(match self.exit_summary.take() {
            Some(summary) => format!("{summary} · {note}"),
            None => note,
        });
    }
}

/// The directory a shell can `cd` into for a column showing `dir`. A path
/// inside a mounted archive names no directory, so it climbs to the one
/// holding the outermost archive.
fn shell_dir<'a>(dir: &'a Path, mounts: &Mounts) -> &'a Path {
    let mut dir = dir;
    while mounts.contains(dir)
        && let Some(parent) = dir.parent()
    {
        dir = parent;
    }
    dir
}

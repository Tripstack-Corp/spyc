//! The tasks a directory declares for a runner, read without running anything.
//! `make` is the one runner: [`make`] is the pure half, from a Makefile's text
//! to its targets, and [`discover`] the file half, which finds the Makefile and
//! reads it.
#![cfg_attr(not(test), expect(dead_code, reason = "no caller outside the tests"))]

pub mod make;

use std::path::{Path, PathBuf};

use anyhow::Result;

pub use make::MakeTarget;

/// A Makefile and the targets it declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taskfile {
    /// The file read: the one given, or the one make picks in the directory
    /// given.
    pub path: PathBuf,
    /// Whether plain `make` in the file's directory reads this file, so a run
    /// needs no `-f`.
    pub is_default: bool,
    pub targets: Vec<MakeTarget>,
}

/// Read the Makefile `dir_or_file` names: the file itself, or in a directory
/// the one make picks (`GNUmakefile`, `makefile`, `Makefile`, in that order).
/// An `include` is followed when its path is literal, exists and resolves
/// inside `root`; one that doesn't is left out, silently.
#[allow(clippy::unnecessary_wraps, reason = "a stub")]
pub fn discover(dir_or_file: &Path, _root: &Path) -> Result<Taskfile> {
    Ok(Taskfile {
        path: dir_or_file.to_path_buf(),
        is_default: false,
        targets: Vec::new(),
    })
}

#[cfg(test)]
mod tests;

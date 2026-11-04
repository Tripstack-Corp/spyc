//! The tasks a directory declares for a runner, read without running anything.
//! `make` is the one runner: [`make`] is the pure half, from a Makefile's text
//! to its targets, and [`discover`] the file half, which finds the Makefile and
//! reads it and the files it includes. Nothing here starts a process.
//!
//! An `include` is followed only when its path is literal, exists, and resolves
//! inside the root the caller names, the containment the MCP tools' roots use:
//! a Makefile is someone else's text, and `include ../../.ssh/config` must not
//! become a read. A relative path is taken from the top Makefile's directory,
//! where make runs. A file is read once, so an include cycle ends.

pub mod make;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub use make::MakeTarget;

/// The names make looks for in a directory, in the order it looks.
const MAKEFILE_NAMES: [&str; 3] = ["GNUmakefile", "makefile", "Makefile"];

/// A file larger than this is not read: no Makefile a person writes is near it.
const MAX_BYTES: u64 = 4 << 20;

/// How many files one discovery reads, the Makefile and its includes.
const MAX_FILES: usize = 64;

/// Whether `path` is named as make names a Makefile.
pub fn is_makefile(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| MAKEFILE_NAMES.contains(&n))
}

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
pub fn discover(dir_or_file: &Path, root: &Path) -> Result<Taskfile> {
    let (path, is_default) = locate(dir_or_file)?;
    let top = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let text = read(&path)?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut read_so_far: Vec<PathBuf> = path.canonicalize().into_iter().collect();
    let mut found = make::Assembly::default();
    make::walk(make::scan(&text), &mut found, &mut |name| {
        follow(name, &top, &root, &mut read_so_far)
    });
    Ok(Taskfile {
        path,
        is_default,
        targets: found.finish(),
    })
}

/// The Makefile `dir_or_file` names, and whether make would pick it unbidden.
fn locate(dir_or_file: &Path) -> Result<(PathBuf, bool)> {
    if dir_or_file.is_dir() {
        let Some(name) = picked(dir_or_file)? else {
            bail!("no Makefile in {}", dir_or_file.display());
        };
        return Ok((dir_or_file.join(name), true));
    }
    let dir = dir_or_file.parent().unwrap_or(Path::new("."));
    let name = dir_or_file.file_name().and_then(|n| n.to_str());
    let is_default = name.is_some() && picked(dir)? == name;
    Ok((dir_or_file.to_path_buf(), is_default))
}

/// The name make reads in `dir`, matched exactly: on a case-insensitive
/// filesystem `makefile` would open `Makefile`, and the title would misname it.
fn picked(dir: &Path) -> Result<Option<&'static str>> {
    let present: Vec<std::ffi::OsString> = std::fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    Ok(MAKEFILE_NAMES
        .into_iter()
        .find(|name| present.iter().any(|p| p == name) && dir.join(name).is_file()))
}

/// A Makefile's text, lossily decoded: a stray byte costs a label, not the menu.
/// Only a regular file, a link followed: opening a FIFO or a device to read
/// can wait for ever, holding the worker that asked.
fn read(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if !meta.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    let len = meta.len();
    if len > MAX_BYTES {
        bail!("{} is too large to read ({len} bytes)", path.display());
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Read the file an `include` names as `name`, or decline: a name holding a
/// reference or a wildcard, one that doesn't exist, one outside `root`, one
/// already read, or one past the file cap.
fn follow(
    name: &str,
    top: &Path,
    root: &Path,
    read_so_far: &mut Vec<PathBuf>,
) -> Option<make::Scan> {
    let literal = !name.is_empty()
        && !name.starts_with('~')
        && !name.contains(['$', '*', '?', '[', '\\', '%'])
        && !name.chars().any(char::is_control);
    if !literal || read_so_far.len() >= MAX_FILES {
        return None;
    }
    let real = top.join(name).canonicalize().ok()?;
    if !real.starts_with(root) || read_so_far.contains(&real) || !real.is_file() {
        return None;
    }
    read_so_far.push(real.clone());
    read(&real).ok().map(|text| make::scan(&text))
}

#[cfg(test)]
mod tests;

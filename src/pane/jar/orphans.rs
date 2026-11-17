//! Jars no spyc owns (S2c): live jars whose owner went without a successor,
//! whose token this spyc keeps, each as it describes itself (R31) and by
//! what it holds (R18). A spyc's start offers to restore or discard them
//! (`app::orphan_jars`); a discard ends one and its program.

use std::path::{Path, PathBuf};

use super::Place;

/// A jar no spyc owns, as it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub socket: PathBuf,
    /// `pane`, `task` or `capture`; empty for a jar never told.
    pub is: String,
    pub label: String,
    /// What its tab or task ran; empty for a jar never told.
    pub command: String,
    pub agent: String,
    pub project: String,
    pub project_home: String,
    pub pane_id: String,
    /// Where its program started.
    pub cwd: String,
}

/// The jars in `place` (this user's when `None`) that no spyc owns.
pub fn find(_place: Option<Place>) -> Vec<Orphan> {
    Vec::new()
}

/// End the jar at `socket` in `place` (this user's when `None`), and its
/// program.
pub fn discard(_place: Option<Place>, _socket: &Path) -> anyhow::Result<()> {
    anyhow::bail!("not yet")
}

//! One Claude tab's Claude Code home: the `CLAUDE_CONFIG_DIR` its claude runs
//! with, else `~/.claude`, which holds its session records and transcripts.
//! The status hook runs in claude's own environment, a wrapper's included, so
//! its report wins; until one arrives, the pane's launch record names the
//! home the shell set; failing both, spyc's own applies. Only an absolute path
//! names a place.

use std::path::{Path, PathBuf};

use crate::pane::pty_host::LaunchEnvironment;

/// A `CLAUDE_CONFIG_DIR` as a home: absolute, else none.
pub fn from_config_dir(value: &std::ffi::OsStr) -> Option<PathBuf> {
    let path = Path::new(value);
    path.is_absolute().then(|| path.to_path_buf())
}

/// The home a launch record names. One that leaves `CLAUDE_CONFIG_DIR` unset
/// names none, and the tab keeps spyc's own.
pub fn from_launch(env: &LaunchEnvironment) -> Option<PathBuf> {
    from_config_dir(env.claude_config_dir.as_deref()?)
}

/// What decides a tab's home, captured on the loop without touching the
/// filesystem and resolved where reading the launch record is allowed: a
/// worker, or an action that reads session files anyway.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lookup {
    /// The status hook's report, or the home a restored tab was saved with.
    pub reported: Option<PathBuf>,
    /// The pane's launch record, read on [`Self::resolve`].
    pub launch_record: Option<PathBuf>,
    /// spyc's own.
    pub default: Option<PathBuf>,
}

impl Lookup {
    /// The home the tab reads.
    pub fn resolve(&self) -> Option<PathBuf> {
        self.tab_own().or_else(|| self.default.clone())
    }

    /// The tab's own home, the one a save records: never spyc's, which a
    /// restore reads anyway.
    pub fn tab_own(&self) -> Option<PathBuf> {
        self.reported.clone().or_else(|| {
            let record = LaunchEnvironment::read(self.launch_record.as_deref()?)?;
            from_launch(&record)
        })
    }
}

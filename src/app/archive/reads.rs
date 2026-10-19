//! One read of an archive at a time. A request for an archive whose read is
//! running joins that read, and is served once it lands. A compressed tar is
//! extracted as it is read, into a staging tree every read of it shares, so a
//! second read landing after the first would rewrite every member, an edit
//! the user saved in between included.

use std::path::{Path, PathBuf};

use super::display_name;
use crate::app::archive_ops::ArchiveOutcome;
use crate::app::state::ProjectId;
use crate::app::{App, Effect};

/// A request that joined a read in flight (`Runtime::archive_reads`): the
/// project it came from, and the effect it held back for the mount, `None`
/// to enter it.
#[derive(Debug)]
pub(in crate::app) struct Joined {
    project: ProjectId,
    then: Option<Box<Effect>>,
}

impl App {
    /// Whether a read of `read`, the path an archive's bytes come from, is in
    /// flight.
    pub(super) fn is_reading(&self, read: &Path) -> bool {
        self.runtime.archive_reads.contains_key(read)
    }

    /// Record that a read of `read` starts.
    pub(super) fn start_read(&mut self, read: &Path) {
        self.runtime
            .archive_reads
            .entry(read.to_path_buf())
            .or_default();
    }

    /// Join the read of `read` in flight from the active project, saying the
    /// archive is being read.
    pub(super) fn join_read(&mut self, read: &Path, then: Option<Box<Effect>>) {
        let project = self.state.active;
        if let Some(joined) = self.runtime.archive_reads.get_mut(read) {
            joined.push(Joined { project, then });
        }
        self.state
            .flash_progress(format!("reading {}…", display_name(read)));
    }

    /// The requests that joined the read `outcome` ends, if it ends one.
    pub(super) fn finish_read(&mut self, outcome: &ArchiveOutcome) -> Option<Vec<Joined>> {
        let read: &PathBuf = match outcome {
            ArchiveOutcome::Mounted { index, .. } => {
                index.source.as_ref().unwrap_or(&index.archive)
            }
            ArchiveOutcome::NeedsConfirm { path, .. }
            | ArchiveOutcome::NotAnArchive { path, .. }
            | ArchiveOutcome::Failed { path, .. } => path,
            _ => return None,
        };
        self.runtime.archive_reads.remove(read.as_path())
    }

    /// Serve what joined a read that has landed, each in its own project:
    /// `mounted` is the archive it mounted, `None` when it ended otherwise.
    /// A mount is entered, and work held back for it runs in the project in
    /// view; in another, that work would act on the view in front of the
    /// user, so it is dropped and said so, as a landing there drops it. A read
    /// that ended without a mount says why through the request that started it
    /// (an error, a question, or the file opened as itself), and its joiners
    /// are let go.
    pub(super) fn serve_joined(
        &mut self,
        joined: Vec<Joined>,
        mounted: Option<&Path>,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        for Joined { project, then } in joined {
            self.state.clear_progress_flash_of(project);
            let Some(archive) = mounted else {
                continue;
            };
            match then {
                None => {
                    self.land_in(project, |app| app.enter_mount(archive));
                }
                Some(effect) if project == self.state.active => effects.push(*effect),
                Some(_) => {
                    self.land_in(project, |app| {
                        app.state.flash_error(
                            "what was waiting on the archive was dropped; run it again there",
                        );
                    });
                }
            }
        }
        effects
    }
}

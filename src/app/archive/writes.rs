//! A write in flight owns its archive. Each write re-reads the archive once it
//! lands, emptying the staging tree first, and clears the journal, so while
//! one runs a second write, a delete, rename or file put into that archive, a
//! discard and an unmount are refused rather than lost. An edit saved to an
//! extracted copy is not among them: an editor or an agent saves it, outside
//! spyc's ops.

use std::path::Path;

use super::display_name;
use crate::app::App;
use crate::app::archive_ops::ArchiveOutcome;
use crate::app::archive_route::PendingChange;

impl App {
    /// Record that a write of `archive` starts.
    pub(super) fn start_write(&mut self, archive: &Path) {
        self.runtime.archive_writes.insert(archive.to_path_buf());
    }

    /// The write `outcome` ends, if it ends one, is over.
    pub(super) fn finish_write(&mut self, outcome: &ArchiveOutcome) {
        if let ArchiveOutcome::Written { archive, .. }
        | ArchiveOutcome::WriteFailed { archive, .. } = outcome
        {
            self.runtime.archive_writes.remove(archive);
        }
    }

    /// Whether `archive` is being written, saying so when it is.
    pub(super) fn refuse_while_written(&mut self, archive: &Path) -> bool {
        if !self.runtime.archive_writes.contains(archive) {
            return false;
        }
        self.state.flash_error(format!(
            "archive: {} is being written — try again once it has landed",
            display_name(archive)
        ));
        true
    }

    /// Whether any of `changes` is to an archive being written, saying so when
    /// one is.
    pub(super) fn refuse_changes_while_written(&mut self, changes: &[PendingChange]) -> bool {
        changes
            .iter()
            .any(|change| self.refuse_while_written(change.archive()))
    }
}

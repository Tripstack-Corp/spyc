//! The flash: the one-line message the status row shows until something
//! replaces it.

use super::{AppState, ProjectId};
use crate::app::{FlashKind, FlashMessage};

impl AppState {
    pub fn flash_info<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Info,
        });
    }

    pub fn flash_error<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Error,
        });
    }

    /// Announce work that is starting, to be replaced or cleared when it lands.
    ///
    /// A flash has no lifetime of its own — it shows until something else
    /// replaces it — so an in-flight message left behind by a completion that had
    /// nothing to say goes on claiming the work is still running.
    pub fn flash_progress<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Progress(self.active),
        });
    }

    /// Drop the active project's in-flight message now that its operation has
    /// finished ([`Self::clear_progress_flash_of`]).
    pub fn clear_progress_flash(&mut self) {
        self.clear_progress_flash_of(self.active);
    }

    /// Drop the in-flight message project `id` raised, now that its operation
    /// has finished.
    ///
    /// Only ever clears a [`FlashKind::Progress`] of `id`'s, so a real message
    /// that landed in the meantime — a warning about the archive, an error —
    /// survives, as does another project's announcement of work still running.
    pub fn clear_progress_flash_of(&mut self, id: ProjectId) {
        if self
            .flash
            .as_ref()
            .is_some_and(|f| matches!(f.kind, FlashKind::Progress(owner) if owner == id))
        {
            self.flash = None;
        }
    }

    /// Flash the outcome of a "save pane contents to file" action: the
    /// saved file's basename on success, the error text on failure. Shared
    /// by the two pane-scrollback save handlers (the `PaneScrollSave` action
    /// and the `s` key in pane-scroll mode), which were byte-for-byte
    /// identical. Errors stay `flash_info` (not `flash_error`) to preserve
    /// the pre-existing presentation.
    pub fn flash_saved_file(&mut self, result: std::io::Result<std::path::PathBuf>) {
        match result {
            Ok(path) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                self.flash_info(format!("saved: {name}"));
            }
            Err(e) => self.flash_info(format!("save error: {e:#}")),
        }
    }
}

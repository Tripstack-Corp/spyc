//! The `^a g` image gallery: the images the focused agent tab actually
//! received, read back out of its own transcript.
//!
//! This is the answer to "what *was* `[Image #3]`?" — the agent prints an
//! opaque token and then the picture is gone from the conversation, but its
//! transcript kept the bytes next to the prompt that carried them.
//!
//! Opening is two off-thread hops, never one: `Effect::IndexTranscriptImages`
//! streams the (multi-MB, mostly-base64) transcript into a metadata index, and
//! only when the user picks a row does `Effect::OpenTranscriptImage` re-read
//! that one record and decode it. Holding every image from the index pass would
//! cost tens of megabytes to show one picture.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Effect, View, image_ops};

impl App {
    /// `^a g` / `:images` — index the focused agent tab's transcript images and
    /// open the gallery when they land. A second press closes it (toggle,
    /// matching the graveyard viewer).
    pub(crate) fn open_image_gallery(&mut self) -> Vec<Effect> {
        if matches!(self.state.cur().view, View::Images) {
            self.state.close_images_view();
            return Vec::new();
        }
        // Bind the gallery to this tab: its uncommitted images are read
        // straight from the ring by id, never copied (they're megabytes).
        self.state.gallery_tab_id = self
            .runtime
            .pane_tabs
            .as_ref()
            .map(|t| t.active_info().id.clone());
        let has_pending = self.active_tab_pending_count() > 0;
        let Some(path) = self.transcript_image_path(!has_pending) else {
            // No readable transcript, but clipboard-captured images exist —
            // show those alone rather than refusing. This is exactly the
            // pre-submit case the capture was built for.
            if has_pending {
                self.state.open_images_view(None, Vec::new());
                self.state
                    .flash_info("unsent images \u{00b7} Enter to view \u{00b7} q to close");
            }
            return Vec::new();
        };
        self.state
            .flash_info("reading images from transcript\u{2026}");
        vec![Effect::IndexTranscriptImages(
            image_ops::TranscriptIndexOp { path },
        )]
    }

    /// How many uncommitted (pasted, not yet sent) images the focused tab has.
    fn active_tab_pending_count(&self) -> usize {
        self.state
            .gallery_tab_id
            .as_ref()
            .and_then(|id| self.state.pane.pending_images.get(id))
            .map_or(0, Vec::len)
    }

    /// Resolve the transcript file for the focused pane tab. `report` gates the
    /// flashes: a miss is only worth telling the user about when it leaves them
    /// with nothing at all — with unsent images to show, the gallery opens and
    /// an error would be noise. Each miss keeps its own message, since "no
    /// tab", "this agent can't do it", and "nothing found on disk" are three
    /// different problems.
    fn transcript_image_path(&mut self, report: bool) -> Option<std::path::PathBuf> {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            if report {
                self.state
                    .flash_error("no pane tab — open an agent with ^a c first");
            }
            return None;
        };
        let info = tabs.active_info();
        let profile = crate::agent::detect(&info.command);
        let Some(spec) = profile.transcript_images() else {
            if report {
                self.state.flash_error(format!(
                    "{}: spyc can't read images from this agent's transcript",
                    profile.name()
                ));
            }
            return None;
        };
        // The session uuid pinned to this pane — the resolver's strongest
        // signal, and the only reliable one when two agent tabs share a cwd.
        let pinned = info.pinned_session_id().map(str::to_string);
        let query = crate::agent::TranscriptQuery {
            cwd: &info.cwd,
            spawn_epoch_secs: info.spawn_epoch_secs,
            command: &info.command,
            session_id: pinned.as_deref(),
        };
        let path = (spec.resolve)(query);
        if path.is_none() && report {
            self.state.flash_error(format!(
                "{}: no transcript found for this session",
                profile.name()
            ));
        }
        path
    }

    /// Keys inside `View::Images`. Movement falls through to the normal list
    /// bindings (the gallery is a list view); this handles only what's specific
    /// to it, so `j`/`k`/`G`/`/` keep working unchanged.
    ///
    /// `Enter` / `i` show the image full-screen, `Esc` / `q` close the gallery.
    /// Returns `None` when the key isn't ours, so the caller keeps routing.
    pub(crate) fn handle_images_view_key(&mut self, key: KeyEvent) -> Option<Vec<Effect>> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state.close_images_view();
                Some(Vec::new())
            }
            KeyCode::Enter | KeyCode::Char('i') => Some(self.open_cursor_image()),
            _ => None,
        }
    }

    /// Show the image under the cursor full-screen (the worker re-reads and
    /// decodes it; `apply_image_outcomes` installs the overlay).
    fn open_cursor_image(&mut self) -> Vec<Effect> {
        use crate::app::state::listing::GalleryRow;
        let (cols, rows) = self.view.term_size;
        // Leave the footer row to the overlay's verb hints.
        let rows = rows.saturating_sub(1);
        match self.state.gallery_row_at_cursor() {
            // An uncommitted image is already in memory — the worker only has
            // to build its protocol, with nothing to fetch first.
            Some(GalleryRow::Pending(n)) => {
                let Some(p) = self.state.pending_gallery_images().get(n) else {
                    self.state.flash_error("no image under cursor");
                    return Vec::new();
                };
                vec![Effect::ShowImageBytes(image_ops::ImageBytesOp {
                    bytes: p.encoded.clone(),
                    origin: image_ops::ImageOrigin::Pending { seq: p.seq },
                    cols,
                    rows,
                })]
            }
            Some(GalleryRow::Received(n)) => {
                let Some(entry) = self.state.transcript_images.get(n).cloned() else {
                    self.state.flash_error("no image under cursor");
                    return Vec::new();
                };
                let Some(path) = self.state.transcript_images_path.clone() else {
                    self.state
                        .flash_error("gallery has no transcript to read from");
                    return Vec::new();
                };
                vec![Effect::OpenTranscriptImage(
                    image_ops::TranscriptImageOpenOp {
                        path,
                        entry,
                        cols,
                        rows,
                    },
                )]
            }
            None => {
                self.state.flash_error("no image under cursor");
                Vec::new()
            }
        }
    }
}

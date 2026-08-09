//! The `^a g` image gallery: what the focused agent tab has received, plus
//! anything pasted into it that hasn't been sent yet.
//!
//! This is the answer to "what *was* `[Image #3]`?" — the agent prints an
//! opaque token and the picture is gone from the conversation, but its
//! transcript kept the bytes next to the prompt that carried them — and to
//! "what did I just attach?", which is answered from spyc's own clipboard
//! capture, since nothing has been written to a transcript yet.
//!
//! It renders as a **popup over the frame**, not as a view the file list
//! switches into. Checking an attachment is a glance mid-task; taking the whole
//! frame away to answer it loses the user's place for no benefit.
//!
//! Reading is two off-thread hops, never one: `Effect::IndexTranscriptImages`
//! streams the (multi-MB, mostly-base64) transcript into a metadata index, and
//! only when the user picks a row does the bytes for that one image get read
//! and decoded.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Effect, ImageGallery, image_ops};

/// Which image a gallery cursor position refers to.
///
/// The popup concatenates two lists — unsent images first, then received ones —
/// so the cursor index means different things in different halves. Resolving
/// that in exactly one place is what keeps "row 3" and "the image opened by row
/// 3" from drifting apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryRow {
    /// Nth unsent (pasted, not yet submitted) image.
    Pending(usize),
    /// Nth image from the agent's transcript.
    Received(usize),
}

impl GalleryRow {
    /// Resolve a cursor index against a gallery of `pending` + `received` rows.
    /// `None` past the end.
    pub const fn of(index: usize, pending: usize, received: usize) -> Option<Self> {
        if index < pending {
            Some(Self::Pending(index))
        } else if index - pending < received {
            Some(Self::Received(index - pending))
        } else {
            None
        }
    }
}

impl App {
    /// `^a g` / `:images` — open the gallery for the focused agent tab. A second
    /// press closes it.
    ///
    /// Unsent images are already in hand, so the popup opens on them
    /// immediately; the transcript index arrives from the worker and fills in
    /// the received half when it lands.
    pub(crate) fn open_image_gallery(&mut self) -> Vec<Effect> {
        if self.view.image_gallery.is_some() {
            self.close_image_gallery();
            return Vec::new();
        }
        let Some(tab_id) = self
            .runtime
            .pane_tabs
            .as_ref()
            .map(|t| t.active_info().id.clone())
        else {
            self.state
                .flash_error("no pane tab — open an agent with ^a c first");
            return Vec::new();
        };
        let has_pending = self
            .state
            .pane
            .pending_images
            .get(&tab_id)
            .is_some_and(|v| !v.is_empty());
        // Report a transcript miss only when it leaves the user with nothing:
        // with unsent images to show, the popup opens and an error would be noise.
        let path = self.transcript_image_path(!has_pending);
        if path.is_none() && !has_pending {
            return Vec::new();
        }
        self.view.image_gallery = Some(ImageGallery {
            tab_id,
            received: Vec::new(),
            transcript_path: path.clone(),
            cursor: 0,
            loading: path.is_some(),
        });
        self.view.needs_full_repaint = true;
        path.map_or_else(Vec::new, |path| {
            vec![Effect::IndexTranscriptImages(
                image_ops::TranscriptIndexOp { path },
            )]
        })
    }

    pub(crate) fn close_image_gallery(&mut self) {
        self.view.image_gallery = None;
        // The popup covered live cells (a pane, the list); they have to be
        // repainted, not diffed against what the popup left behind.
        self.view.needs_full_repaint = true;
    }

    /// Install a finished transcript index into the open popup.
    ///
    /// Dropped if the user already closed the gallery, and pinned to the tab it
    /// was requested for — an index that landed after a tab switch would
    /// otherwise show one agent's images under another's name.
    pub(crate) fn apply_gallery_index(
        &mut self,
        path: &std::path::Path,
        images: Vec<crate::state::transcript_images::TranscriptImage>,
    ) {
        let Some(g) = self.view.image_gallery.as_mut() else {
            return;
        };
        if g.transcript_path.as_deref() != Some(path) {
            return;
        }
        g.received = images;
        g.loading = false;
        // Land on the newest received image — the one most likely being checked
        // — unless unsent ones are showing, which are newer still.
        let pending = self
            .state
            .pane
            .pending_images
            .get(&g.tab_id)
            .map_or(0, Vec::len);
        if pending == 0 {
            g.cursor = g.received.len().saturating_sub(1);
        }
        self.view.needs_full_repaint = true;
    }

    /// How many rows the open gallery has, and how they split.
    pub(crate) fn gallery_counts(&self) -> (usize, usize) {
        self.view.image_gallery.as_ref().map_or((0, 0), |g| {
            let pending = self
                .state
                .pane
                .pending_images
                .get(&g.tab_id)
                .map_or(0, Vec::len);
            (pending, g.received.len())
        })
    }

    /// Keys inside the gallery popup. It's modal (routed via `Modal::ImageGallery`),
    /// so every key is answered here.
    pub(crate) fn handle_image_gallery_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let (pending, received) = self.gallery_counts();
        let total = pending + received;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_image_gallery(),
            KeyCode::Char('j') | KeyCode::Down => self.move_gallery_cursor(1, total),
            KeyCode::Char('k') | KeyCode::Up => self.move_gallery_cursor(-1, total),
            KeyCode::Char('g') | KeyCode::Home => self.set_gallery_cursor(0),
            KeyCode::Char('G') | KeyCode::End => {
                self.set_gallery_cursor(total.saturating_sub(1));
            }
            // 1-9 jump straight to a row, matching the harpoon menu's shape.
            KeyCode::Char(c @ '1'..='9') => {
                let n = c as usize - '1' as usize;
                if n < total {
                    self.set_gallery_cursor(n);
                    return self.open_cursor_image();
                }
            }
            KeyCode::Enter | KeyCode::Char('i') => return self.open_cursor_image(),
            _ => {}
        }
        Vec::new()
    }

    fn move_gallery_cursor(&mut self, delta: isize, total: usize) {
        let Some(g) = self.view.image_gallery.as_mut() else {
            return;
        };
        let last = total.saturating_sub(1);
        g.cursor = g.cursor.saturating_add_signed(delta).min(last);
    }

    const fn set_gallery_cursor(&mut self, index: usize) {
        if let Some(g) = self.view.image_gallery.as_mut() {
            g.cursor = index;
        }
    }

    /// Show the image under the cursor full-screen. Closes the popup first —
    /// the overlay is itself full-screen, so leaving the list underneath would
    /// only be something to repaint around.
    fn open_cursor_image(&mut self) -> Vec<Effect> {
        let (pending_n, received_n) = self.gallery_counts();
        let Some(g) = self.view.image_gallery.as_ref() else {
            return Vec::new();
        };
        let (cols, rows) = self.view.term_size;
        // Leave the footer row to the overlay's verb hints.
        let rows = rows.saturating_sub(1);
        let effects = match GalleryRow::of(g.cursor, pending_n, received_n) {
            // An unsent image is already in memory — the worker only has to
            // build its protocol, with nothing to fetch first.
            Some(GalleryRow::Pending(n)) => self
                .state
                .pane
                .pending_images
                .get(&g.tab_id)
                .and_then(|ring| ring.get(n))
                .map(|p| {
                    vec![Effect::ShowImageBytes(image_ops::ImageBytesOp {
                        bytes: p.encoded.clone(),
                        origin: image_ops::ImageOrigin::Pending { seq: p.seq },
                        cols,
                        rows,
                    })]
                }),
            Some(GalleryRow::Received(n)) => match (g.received.get(n), g.transcript_path.clone()) {
                (Some(entry), Some(path)) => Some(vec![Effect::OpenTranscriptImage(
                    image_ops::TranscriptImageOpenOp {
                        path,
                        entry: entry.clone(),
                        cols,
                        rows,
                    },
                )]),
                _ => None,
            },
            None => None,
        };
        if let Some(fx) = effects {
            self.close_image_gallery();
            fx
        } else {
            self.state.flash_error("no image under cursor");
            Vec::new()
        }
    }

    /// Resolve the transcript file for the focused pane tab. `report` gates the
    /// flashes — each miss keeps its own message, since "this agent can't do
    /// it" and "nothing found on disk" are different problems.
    fn transcript_image_path(&mut self, report: bool) -> Option<std::path::PathBuf> {
        let tabs = self.runtime.pane_tabs.as_ref()?;
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
}

#[cfg(test)]
mod tests {
    use super::GalleryRow;

    /// The whole point of the type: the cursor index means different things in
    /// the two halves, and "row 3" must open the image row 3 renders.
    #[test]
    fn the_cursor_resolves_across_both_sections() {
        // 2 unsent, then 3 received.
        assert_eq!(GalleryRow::of(0, 2, 3), Some(GalleryRow::Pending(0)));
        assert_eq!(GalleryRow::of(1, 2, 3), Some(GalleryRow::Pending(1)));
        assert_eq!(GalleryRow::of(2, 2, 3), Some(GalleryRow::Received(0)));
        assert_eq!(GalleryRow::of(4, 2, 3), Some(GalleryRow::Received(2)));
        assert_eq!(GalleryRow::of(5, 2, 3), None, "past the end");
    }

    /// With nothing pasted the gallery is transcript-only, and index 0 must be
    /// the first received image rather than a phantom unsent one.
    #[test]
    fn with_no_unsent_images_every_row_is_received() {
        assert_eq!(GalleryRow::of(0, 0, 2), Some(GalleryRow::Received(0)));
        assert_eq!(GalleryRow::of(1, 0, 2), Some(GalleryRow::Received(1)));
        assert_eq!(GalleryRow::of(2, 0, 2), None);
    }

    /// The pre-submit case the clipboard capture exists for: unsent images with
    /// no transcript behind them at all.
    #[test]
    fn unsent_images_alone_are_addressable() {
        assert_eq!(GalleryRow::of(0, 1, 0), Some(GalleryRow::Pending(0)));
        assert_eq!(GalleryRow::of(1, 1, 0), None);
    }

    #[test]
    fn an_empty_gallery_has_no_rows() {
        assert_eq!(GalleryRow::of(0, 0, 0), None);
    }
}

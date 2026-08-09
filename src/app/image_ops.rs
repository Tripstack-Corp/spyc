//! The image-display path: encoded bytes → a `ratatui_image::Protocol` sized to
//! the terminal → the full-screen [`ImageView`](crate::app::ImageView) overlay.
//!
//! One channel serves every producer of a displayable image. A mermaid diagram
//! ([`mermaid_ops`](crate::app::mermaid_ops)) rasterizes its SVG first and an
//! image file is read off disk here, but both land as an [`ImageOutcome`] on
//! `runtime.image_results` and wake the loop with `Message::ImageDone`;
//! [`App::apply_image_outcomes`] installs whichever arrives. What the overlay's
//! verbs do with the result is driven by [`ImageOrigin`], not by which producer
//! ran.
//!
//! Decode and protocol encode are both far too heavy for the loop (a 4K raster
//! is ~33 MB of RGBA), so every path here runs on a detached worker — the
//! `graveyard_ops` shape: Effect → worker → Runtime slot → payloadless Message
//! → pre-recv drain.

use std::path::PathBuf;

use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

/// The largest file spyc will try to decode as an image.
///
/// Decoding allocates width × height × 4 bytes regardless of how well the file
/// compressed, so the guard is on the *encoded* size as a cheap proxy: a 64 MB
/// PNG is either a pathological image or not an image at all, and either way
/// the answer is a flash, not an OOM.
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// A decoded image ready for the overlay: the blittable protocol, the extension
/// matching the bytes it came from, and the natural pixel size.
pub type BuiltImage = (Protocol, &'static str, (u32, u32));

/// Where the image on screen came from — the discriminator the overlay verbs
/// read. Every verb means something for every origin (there are no dead keys):
/// `Y` copies a diagram's source or a file's path, `o` re-renders a diagram or
/// hands the file to the OS viewer, `c` (theme) is the one diagram-only verb
/// because only a diagram can be re-rendered in another palette.
#[derive(Debug, Clone)]
pub enum ImageOrigin {
    /// A ```mermaid block rendered by `mermaid_ops`; carries its source so the
    /// diagram can be re-rendered (`c`) or yanked (`Y`).
    Mermaid { source: String },
    /// An image file opened from the list.
    File { path: PathBuf },
    /// An image the agent received, read back out of its transcript. Carries
    /// what the gallery row showed so the overlay can still identify it once
    /// the list is gone.
    Transcript {
        /// spyc's own sequence number, e.g. `3`.
        seq: usize,
        /// The agent's own `[Image #N]` label, when the prompt carried one.
        agent_label: Option<String>,
        /// The prompt this image arrived with — what `Y` yanks.
        prompt: String,
    },
}

impl ImageOrigin {
    /// The mermaid source, when this is a diagram — the gate for the verbs that
    /// only a re-renderable diagram supports.
    pub fn mermaid_source(&self) -> Option<&str> {
        match self {
            Self::Mermaid { source } => Some(source),
            Self::File { .. } | Self::Transcript { .. } => None,
        }
    }

    /// Short label for the overlay footer.
    pub fn label(&self) -> String {
        match self {
            Self::Mermaid { .. } => "mermaid diagram".to_string(),
            Self::File { path } => path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            ),
            // The agent's own label is what the user saw in its output, so lead
            // with it and keep spyc's sequence as the disambiguator — the agent's
            // counter restarts, so it can't stand alone.
            Self::Transcript {
                seq, agent_label, ..
            } => match agent_label {
                Some(l) => format!("image {seq} (agent {l})"),
                None => format!("image {seq}"),
            },
        }
    }

    /// What `Y` copies for this origin, with a word for the flash.
    pub fn yankable(&self) -> (String, &'static str) {
        match self {
            Self::Mermaid { source } => (source.clone(), "mermaid source"),
            Self::File { path } => (path.display().to_string(), "path"),
            Self::Transcript { prompt, .. } => (prompt.clone(), "prompt"),
        }
    }
}

/// Open an image file full-screen: read + decode + build a `Protocol` for a
/// `cols`×`rows` cell box, all on the worker.
#[derive(Debug)]
pub struct ImageOpenOp {
    pub path: PathBuf,
    pub cols: u16,
    pub rows: u16,
}

/// Index an agent transcript's images (the `^a g` gallery). A multi-MB file of
/// base64 — never on the loop.
#[derive(Debug)]
pub struct TranscriptIndexOp {
    pub path: PathBuf,
}

/// Open one already-indexed transcript image: re-read its record, decode the
/// base64, build the protocol.
#[derive(Debug)]
pub struct TranscriptImageOpenOp {
    pub path: PathBuf,
    pub entry: crate::state::transcript_images::TranscriptImage,
    pub cols: u16,
    pub rows: u16,
}

/// Worker result, applied by [`App::apply_image_outcomes`].
pub enum ImageOutcome {
    /// Ready to blit: the protocol plus the *encoded* bytes (kept for `s`/`y`/`b`
    /// without a re-render) and the natural pixel dimensions for the footer.
    Viewed {
        protocol: Box<Protocol>,
        encoded: Vec<u8>,
        /// Extension for `s`, from the decoded format — NOT from the source
        /// filename, which may lie or be absent.
        ext: &'static str,
        dims: (u32, u32),
        origin: ImageOrigin,
        /// Whether this render used the dark theme — mermaid's `c` toggle reads it.
        dark: bool,
    },
    /// A transcript's image index, ready for the gallery. Empty is a legitimate
    /// result (no images in this conversation), so the drain says so rather
    /// than opening an empty list.
    Indexed {
        /// Kept so the gallery can re-read an image's bytes on open — the index
        /// deliberately doesn't hold them.
        path: PathBuf,
        images: Vec<crate::state::transcript_images::TranscriptImage>,
    },
    /// Handed to the OS viewer (mermaid's `o`); nothing to install.
    Opened,
    /// Complete, user-facing failure reason — flashed verbatim, so the producer
    /// (not the drain) owns the wording.
    Failed(String),
}

/// Read `op.path` and build its protocol. Runs on the detached worker.
pub fn open_image_file(op: ImageOpenOp, picker: Option<Picker>) -> ImageOutcome {
    let ImageOpenOp { path, cols, rows } = op;
    let Some(picker) = picker else {
        return ImageOutcome::Failed(no_protocol_reason());
    };
    match std::fs::metadata(&path) {
        Ok(md) if md.len() > MAX_IMAGE_BYTES => {
            return ImageOutcome::Failed(format!(
                "{}: too large to decode ({} MB)",
                path.display(),
                md.len() / (1024 * 1024)
            ));
        }
        Ok(_) => {}
        Err(e) => return ImageOutcome::Failed(format!("{}: {e}", path.display())),
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return ImageOutcome::Failed(format!("{}: {e}", path.display())),
    };
    match build_view(&bytes, &picker, cols, rows) {
        Ok((protocol, ext, dims)) => ImageOutcome::Viewed {
            protocol: Box::new(protocol),
            encoded: bytes,
            ext,
            dims,
            origin: ImageOrigin::File { path },
            dark: false,
        },
        Err(e) => ImageOutcome::Failed(format!("{}: {e}", path.display())),
    }
}

/// Index a transcript's images. Runs on the detached worker.
pub fn index_transcript(op: TranscriptIndexOp) -> ImageOutcome {
    let images = crate::state::transcript_images::index(&op.path);
    ImageOutcome::Indexed {
        path: op.path,
        images,
    }
}

/// Re-read one indexed transcript image and build its protocol. Runs on the
/// detached worker.
pub fn open_transcript_image(op: TranscriptImageOpenOp, picker: Option<Picker>) -> ImageOutcome {
    let TranscriptImageOpenOp {
        path,
        entry,
        cols,
        rows,
    } = op;
    let Some(picker) = picker else {
        return ImageOutcome::Failed(no_protocol_reason());
    };
    let bytes = match crate::state::transcript_images::load(&path, &entry) {
        Ok(b) => b,
        Err(e) => return ImageOutcome::Failed(format!("image {}: {e}", entry.seq)),
    };
    match build_view(&bytes, &picker, cols, rows) {
        Ok((protocol, ext, dims)) => ImageOutcome::Viewed {
            protocol: Box::new(protocol),
            encoded: bytes,
            ext,
            dims,
            origin: ImageOrigin::Transcript {
                seq: entry.seq,
                agent_label: entry.agent_label,
                prompt: entry.prompt_excerpt,
            },
            dark: false,
        },
        Err(e) => ImageOutcome::Failed(format!("image {}: {e}", entry.seq)),
    }
}

/// Encoded bytes → a `Protocol` fitted to a `cols`×`rows` cell box, plus the
/// format's extension and the natural pixel size.
///
/// `Resize::Fit` only ever *downscales*, so a small raster keeps its natural
/// size rather than being blown up to fill the screen — right for a photo
/// (upscaling only adds blur), and why mermaid rasterizes its vector source at
/// the box size beforehand instead of relying on this.
pub fn build_view(
    bytes: &[u8],
    picker: &Picker,
    cols: u16,
    rows: u16,
) -> Result<BuiltImage, String> {
    let format =
        image::guess_format(bytes).map_err(|_| "not a recognized image format".to_string())?;
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode: {e}"))?;
    let dims = (
        image::GenericImageView::width(&img),
        image::GenericImageView::height(&img),
    );
    let protocol = raster_to_protocol(img, picker, cols, rows)?;
    Ok((protocol, ext_for(format), dims))
}

/// The shared last mile: a decoded raster → a `Protocol` for a `cols`×`rows`
/// cell box. Every producer funnels through here so sizing can't drift between
/// the mermaid path and the file path.
pub fn raster_to_protocol(
    img: image::DynamicImage,
    picker: &Picker,
    cols: u16,
    rows: u16,
) -> Result<Protocol, String> {
    picker
        .new_protocol(
            img,
            ratatui::layout::Size::new(cols, rows),
            ratatui_image::Resize::Fit(None),
        )
        .map_err(|e| format!("protocol: {e}"))
}

/// Why nothing can be shown inline, with the way out. Shared so the mermaid and
/// file paths give the same answer to the same terminal limitation.
pub fn no_protocol_reason() -> String {
    "terminal has no image protocol (use `o` to open externally)".to_string()
}

/// Extension for the `s` (save) verb, from the *decoded* format.
const fn ext_for(format: image::ImageFormat) -> &'static str {
    match format {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        _ => "png",
    }
}

impl super::App {
    /// Pre-recv drain: install finished image renders and flash failures.
    /// Returns whether a redraw is needed. Mirrors `apply_graveyard_outcomes`.
    pub(crate) fn apply_image_outcomes(&mut self) -> bool {
        let outcomes: Vec<ImageOutcome> = {
            let mut slot = self
                .runtime
                .image_results
                .lock()
                .expect("image_results mutex poisoned only by a panicking worker");
            if slot.is_empty() {
                return false;
            }
            std::mem::take(&mut *slot)
        };
        let mut redraw = false;
        for outcome in outcomes {
            redraw = true;
            match outcome {
                ImageOutcome::Viewed {
                    protocol,
                    encoded,
                    ext,
                    dims,
                    origin,
                    dark,
                } => {
                    // Install the full-screen overlay; the draw blits it.
                    self.view.image_view = Some(super::ImageView {
                        protocol: *protocol,
                        encoded,
                        ext,
                        dims,
                        origin,
                        dark,
                        flash: None,
                    });
                }
                ImageOutcome::Indexed { path, images } => {
                    // An empty index is a real answer, not a view worth
                    // opening: an empty gallery reads as "spyc lost them".
                    if images.is_empty() {
                        self.state
                            .flash_info("no images in this agent's conversation yet");
                    } else {
                        let count = images.len();
                        self.state.open_images_view(path, images);
                        self.state.flash_info(format!(
                            "{count} image{} \u{00b7} Enter to view \u{00b7} q to close",
                            if count == 1 { "" } else { "s" }
                        ));
                    }
                }
                ImageOutcome::Opened => self.flash_image_status("opened in external viewer"),
                ImageOutcome::Failed(reason) => self.flash_image_status(&reason),
            }
        }
        redraw
    }

    /// Report an image-path message wherever the user is actually looking: the
    /// pager's status line when one is open (a diagram is always opened *from* a
    /// pager), the main status bar otherwise (an image file opened from the
    /// list has no pager — the message would be swallowed).
    fn flash_image_status(&mut self, msg: &str) {
        if let Some(pager) = self.view.pager.as_mut() {
            pager.flash = Some(msg.to_string());
        } else {
            self.state.flash_error(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×3 PNG, encoded at test time so the fixture can't drift from what the
    /// decoder expects.
    fn png_2x3() -> Vec<u8> {
        let img = image::DynamicImage::new_rgba8(2, 3);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png)
            .expect("in-memory PNG encode");
        buf.into_inner()
    }

    #[test]
    fn origin_gates_the_mermaid_only_verbs() {
        let m = ImageOrigin::Mermaid {
            source: "graph TD; A-->B".to_string(),
        };
        assert_eq!(m.mermaid_source(), Some("graph TD; A-->B"));
        assert_eq!(m.label(), "mermaid diagram");

        let f = ImageOrigin::File {
            path: PathBuf::from("/tmp/shot.png"),
        };
        assert!(f.mermaid_source().is_none(), "a file has no mermaid source");
        assert_eq!(f.label(), "shot.png", "footer names the file, not the path");
    }

    /// The label must not collapse to an empty string for a path with no file
    /// name — an unnamed footer reads as a rendering bug.
    #[test]
    fn file_label_falls_back_to_the_whole_path() {
        let f = ImageOrigin::File {
            path: PathBuf::from("/"),
        };
        assert!(!f.label().is_empty());
    }

    /// The extension comes from the decoded format, so `s` can't write a `.png`
    /// holding JPEG bytes just because the source file was misnamed.
    #[test]
    fn extension_tracks_the_decoded_format() {
        assert_eq!(ext_for(image::ImageFormat::Jpeg), "jpg");
        assert_eq!(ext_for(image::ImageFormat::Gif), "gif");
        assert_eq!(ext_for(image::ImageFormat::WebP), "webp");
        assert_eq!(ext_for(image::ImageFormat::Png), "png");
    }

    /// Non-image bytes must be refused before `load_from_memory` gets a chance
    /// to interpret them.
    #[test]
    fn non_image_bytes_are_refused() {
        let picker = Picker::halfblocks();
        // `Protocol` isn't `Debug`, so no `expect_err` — match the error out.
        let Err(err) = build_view(b"this is not an image", &picker, 40, 20) else {
            panic!("plain text must not decode as an image");
        };
        assert!(err.contains("not a recognized image format"), "got: {err}");
    }

    #[test]
    fn decodes_and_reports_natural_dimensions() {
        let picker = Picker::halfblocks();
        let Ok((_, ext, dims)) = build_view(&png_2x3(), &picker, 40, 20) else {
            panic!("a valid PNG must decode");
        };
        assert_eq!(ext, "png");
        assert_eq!(dims, (2, 3), "dims are the image's own, not the cell box");
    }

    /// A file over the cap is refused from its stat, without being read — the
    /// guard exists to avoid the allocation, so reading first would defeat it.
    /// The fixture is a *sparse* file (`set_len` on a fresh file reserves no
    /// blocks), so this costs no disk.
    #[test]
    fn oversized_file_is_refused_before_the_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("huge.png");
        std::fs::File::create(&path)
            .and_then(|f| f.set_len(MAX_IMAGE_BYTES + 1))
            .expect("sparse fixture");
        let out = open_image_file(
            ImageOpenOp {
                path,
                cols: 40,
                rows: 20,
            },
            Some(Picker::halfblocks()),
        );
        match out {
            ImageOutcome::Failed(reason) => {
                assert!(reason.contains("too large"), "got: {reason}");
            }
            _ => panic!("expected an oversized refusal"),
        }
    }

    /// Without a graphics protocol the refusal must still name the way out
    /// (`o` opens externally) rather than dead-ending.
    #[test]
    fn no_picker_refuses_with_the_alternative() {
        let out = open_image_file(
            ImageOpenOp {
                path: PathBuf::from("/any.png"),
                cols: 40,
                rows: 20,
            },
            None,
        );
        match out {
            ImageOutcome::Failed(reason) => {
                assert!(reason.contains("no image protocol"), "got: {reason}");
                assert!(reason.contains('o'), "names the external-open way out");
            }
            _ => panic!("expected a refusal without a picker"),
        }
    }

    /// A missing file reports the path — a bare "No such file or directory"
    /// naming nothing is the failure mode AGENTS.md calls out.
    #[test]
    fn missing_file_names_the_path() {
        let picker = Picker::halfblocks();
        let out = open_image_file(
            ImageOpenOp {
                path: PathBuf::from("/nonexistent/spyc-probe.png"),
                cols: 40,
                rows: 20,
            },
            Some(picker),
        );
        match out {
            ImageOutcome::Failed(reason) => {
                assert!(reason.contains("spyc-probe.png"), "got: {reason}");
            }
            _ => panic!("expected a failure for a missing file"),
        }
    }
}

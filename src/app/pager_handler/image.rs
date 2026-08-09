//! Full-screen image-overlay verbs: `handle_image_view_key` plus its save /
//! yank / theme-toggle / base64 action helpers. The overlay is modal —
//! `handle_pager_key` routes here before any pager handler when
//! `view.image_view` is `Some`. Same `impl App` child-module pattern as
//! `modes` / `motion` / `pickers`: reads App's private state via the
//! descendant-module rule (no field made `pub`); `pub(super)` for the one
//! entry the root calls.
//!
//! Every verb reads [`ImageOrigin`] to decide what it acts on, so the same key
//! does the analogous thing for a diagram and for a file (`Y` yanks a diagram's
//! source or a file's path, `o` re-renders or hands the file to the OS viewer).
//! `c` is the one origin-specific verb: only a diagram can be re-rendered in
//! another palette.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::image_ops::ImageOrigin;
use crate::app::{App, Effect};

impl App {
    /// Verbs for the full-screen image overlay (modal — other keys are
    /// swallowed so nothing scrolls underneath): `s` save, `y` copy the image,
    /// `Y` copy the source/path, `c` toggle light/dark, `b` flip to a base64
    /// text buffer, `o` open externally, q/Esc/i dismiss.
    pub(super) fn handle_image_view_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q' | 'i') => {
                self.view.image_view = None;
                self.view.needs_full_repaint = true;
            }
            KeyCode::Char('s') => self.save_image_view(),
            KeyCode::Char('Y') => self.yank_image_source(),
            KeyCode::Char('y') => self.yank_image_to_clipboard(),
            KeyCode::Char('c') => return self.toggle_image_theme(),
            KeyCode::Char('b') => self.image_to_base64_pager(),
            KeyCode::Char('o') => return self.open_image_externally(),
            _ => {}
        }
        Vec::new()
    }

    /// `s` in the image overlay: write the image to the cwd, reporting the path
    /// (or error) in the overlay footer. A small bounded write, kept inline in
    /// this overlay handler (the image overlay has no effect path of its own;
    /// the pager's text save routes through `Effect::SavePagerOutput`). A file
    /// origin is already on disk, so it reports where instead of writing a
    /// pointless duplicate.
    fn save_image_view(&mut self) {
        let Some(iv) = self.view.image_view.as_mut() else {
            return;
        };
        if let ImageOrigin::File { path } = &iv.origin {
            iv.flash = Some(format!("already on disk: {}", path.display()));
            return;
        }
        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let ext = iv.ext;
        let result = std::env::current_dir().and_then(|d| {
            let p = d.join(format!("spyc_mermaid_{stamp}.{ext}"));
            std::fs::write(&p, &iv.encoded).map(|()| p)
        });
        iv.flash = Some(match result {
            Ok(p) => format!("saved: {}", p.display()),
            Err(e) => format!("save failed: {e:#}"),
        });
        // Footer-only change: do NOT force a full repaint. The input arm already
        // marks a (diff) draw, which repaints the changed footer cells while
        // leaving the image cells untouched — so the inline image is not
        // re-emitted. A full repaint clears the screen and re-blits the image,
        // a visible flash on every verb keypress.
    }

    /// `Y` in the image overlay: copy what's *behind* the image — a diagram's
    /// mermaid source, or a file's path.
    fn yank_image_source(&mut self) {
        let Some(iv) = self.view.image_view.as_mut() else {
            return;
        };
        let (text, label) = iv.origin.yankable();
        iv.flash = Some(match crate::clipboard::copy(&text) {
            Ok(()) => format!("{label} copied to clipboard"),
            Err(e) => format!("copy failed: {e:#}"),
        });
        // Footer-only — no full repaint (see `save_image_view`: avoids flashing
        // the inline image).
    }

    /// `y` in the image overlay: copy the image itself to the system clipboard
    /// (image data, via `arboard`).
    fn yank_image_to_clipboard(&mut self) {
        let Some(iv) = self.view.image_view.as_mut() else {
            return;
        };
        iv.flash = Some(match crate::clipboard::copy_image(&iv.encoded) {
            Ok(()) => "image copied to clipboard".to_string(),
            Err(e) => format!("copy failed: {e:#}"),
        });
        // Footer-only — no full repaint (see `save_image_view`: avoids flashing
        // the inline image).
    }

    /// `c` in the image overlay: toggle light/dark and re-render off-thread.
    /// Mermaid-only — a raster has no source to re-render from. Returns the
    /// render Effect; `apply_image_outcomes` swaps in the new protocol when it
    /// lands.
    fn toggle_image_theme(&mut self) -> Vec<Effect> {
        let Some(iv) = self.view.image_view.as_mut() else {
            return Vec::new();
        };
        let Some(source) = iv.origin.mermaid_source().map(str::to_string) else {
            iv.flash = Some("theme toggle is mermaid-only".to_string());
            self.view.needs_full_repaint = true;
            return Vec::new();
        };
        let dark = !iv.dark;
        iv.flash = Some(format!(
            "rendering {} theme\u{2026}",
            if dark { "dark" } else { "light" }
        ));
        self.view.needs_full_repaint = true;
        let (cols, rows) = self.view.term_size;
        vec![Effect::RenderMermaid(
            crate::app::mermaid_ops::MermaidRenderOp {
                source,
                mode: crate::app::mermaid_ops::MermaidMode::View {
                    cols,
                    rows: rows.saturating_sub(1),
                    dark,
                },
            },
        )]
    }

    /// `o` in the image overlay: hand the image to the OS viewer. A diagram is
    /// re-rendered at natural size by the worker first (the full-res read path);
    /// a file is already on disk, so it goes straight to the viewer — the same
    /// inline `that_detached` spawn `quick_select` uses for a URL (it forks and
    /// returns, so there is nothing to move off-thread).
    fn open_image_externally(&mut self) -> Vec<Effect> {
        let Some(iv) = self.view.image_view.as_mut() else {
            return Vec::new();
        };
        match &iv.origin {
            ImageOrigin::Mermaid { source } => {
                vec![Effect::RenderMermaid(
                    crate::app::mermaid_ops::MermaidRenderOp {
                        source: source.clone(),
                        mode: crate::app::mermaid_ops::MermaidMode::Open,
                    },
                )]
            }
            ImageOrigin::File { path } => {
                iv.flash = Some(match open::that_detached(path) {
                    Ok(()) => "opened in external viewer".to_string(),
                    Err(e) => format!("open failed: {e:#}"),
                });
                Vec::new()
            }
            // A transcript image has no file of its own — spill it to a temp
            // file first. Bounded write on the main thread, like `s` above.
            ImageOrigin::Transcript { seq, .. } => {
                let path = std::env::temp_dir().join(format!("spyc-agent-image-{seq}.{}", iv.ext));
                iv.flash = Some(
                    match std::fs::write(&path, &iv.encoded).and_then(|()| {
                        open::that_detached(&path).map(|()| path.display().to_string())
                    }) {
                        Ok(p) => format!("opened {p}"),
                        Err(e) => format!("open failed: {e:#}"),
                    },
                );
                Vec::new()
            }
        }
    }

    /// `b` in the image overlay: flip to a text pager holding the image's
    /// base64 (yank it there with the pager's own `y`). The image view is
    /// dismissed and any pager beneath pushed to history, so `q` from the
    /// base64 buffer returns where the image came from.
    fn image_to_base64_pager(&mut self) {
        use base64::Engine;
        let Some(iv) = self.view.image_view.take() else {
            return;
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&iv.encoded);
        let lines: Vec<String> = b64
            .as_bytes()
            .chunks(120)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        if let Some(prev) = self.view.pager.take() {
            self.view.pager_history.push(prev);
        }
        let title = format!("{} \u{2014} base64", iv.origin.label());
        self.view.pager = Some(crate::ui::pager::PagerView::new_plain(&title, lines));
        self.view.needs_full_repaint = true;
    }
}

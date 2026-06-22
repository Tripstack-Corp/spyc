//! Full-screen image-overlay (mermaid diagram) verbs, extracted verbatim from
//! the `pager_handler` root: `handle_image_view_key` plus its save / yank /
//! theme-toggle / base64 action helpers. The overlay is modal —
//! `handle_pager_key` routes here before any pager handler when
//! `view.image_view` is `Some`. Same `impl App` child-module pattern as
//! `modes` / `motion` / `pickers`: reads App's private state via the
//! descendant-module rule (no field made `pub`); `pub(super)` for the one
//! entry the root calls.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Effect};

impl App {
    /// Verbs for the full-screen image overlay (modal — other keys are
    /// swallowed so nothing scrolls underneath): `s` save the PNG, `y` copy the
    /// image, `Y` copy the mermaid source, `c` toggle light/dark, `b` flip to a
    /// base64 text buffer, `o` open externally, q/Esc/i dismiss.
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
            KeyCode::Char('o') => {
                // Open the current diagram externally — re-render via the worker
                // (mermaid views always carry their source).
                if let Some(source) = self
                    .view
                    .image_view
                    .as_ref()
                    .and_then(|iv| iv.source.clone())
                {
                    return vec![Effect::RenderMermaid(
                        crate::app::mermaid_ops::MermaidRenderOp {
                            source,
                            mode: crate::app::mermaid_ops::MermaidMode::Open,
                        },
                    )];
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// `s` in the image overlay: write the rendered PNG to the cwd, reporting
    /// the path (or error) in the overlay footer. Small write, done inline like
    /// the pager's text `save_to_file`.
    fn save_image_view(&mut self) {
        let Some(iv) = self.view.image_view.as_mut() else {
            return;
        };
        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let result = std::env::current_dir().and_then(|d| {
            let p = d.join(format!("spyc_mermaid_{stamp}.png"));
            std::fs::write(&p, &iv.png).map(|()| p)
        });
        iv.flash = Some(match result {
            Ok(p) => format!("saved: {}", p.display()),
            Err(e) => format!("save failed: {e}"),
        });
        // Footer-only change: do NOT force a full repaint. The input arm already
        // marks a (diff) draw, which repaints the changed footer cells while
        // leaving the image cells untouched — so the inline image is not
        // re-emitted. A full repaint clears the screen and re-blits the image,
        // a visible flash on every verb keypress.
    }

    /// `Y` in the image overlay: copy the mermaid source to the clipboard
    /// (mermaid diagrams only — no-op with a footer note otherwise).
    fn yank_image_source(&mut self) {
        let Some(iv) = self.view.image_view.as_mut() else {
            return;
        };
        iv.flash = Some(match iv.source.clone() {
            Some(src) => match crate::clipboard::copy(&src) {
                Ok(()) => "mermaid source copied to clipboard".to_string(),
                Err(e) => format!("copy failed: {e}"),
            },
            None => "no source to copy (not a mermaid diagram)".to_string(),
        });
        // Footer-only — no full repaint (see `save_image_view`: avoids flashing
        // the inline image).
    }

    /// `y` in the image overlay: copy the rendered PNG to the system clipboard
    /// (image data, via `arboard`).
    fn yank_image_to_clipboard(&mut self) {
        let Some(iv) = self.view.image_view.as_mut() else {
            return;
        };
        iv.flash = Some(match crate::clipboard::copy_image(&iv.png) {
            Ok(()) => "image copied to clipboard".to_string(),
            Err(e) => format!("copy failed: {e}"),
        });
        // Footer-only — no full repaint (see `save_image_view`: avoids flashing
        // the inline image).
    }

    /// `c` in the image overlay: toggle light/dark and re-render off-thread
    /// (mermaid-only — we re-run the source through the worker with the other
    /// theme). Returns the render Effect; `apply_mermaid_outcomes` swaps in the
    /// new protocol when it lands.
    fn toggle_image_theme(&mut self) -> Vec<Effect> {
        let Some(iv) = self.view.image_view.as_mut() else {
            return Vec::new();
        };
        let Some(source) = iv.source.clone() else {
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
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
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

    /// `b` in the image overlay: flip to a text pager holding the PNG's base64
    /// (yank it there with the pager's own `y`). The image view is dismissed and
    /// the markdown pager pushed to history, so `q` from the base64 buffer
    /// returns to the diagram's doc.
    fn image_to_base64_pager(&mut self) {
        use base64::Engine;
        let Some(iv) = self.view.image_view.take() else {
            return;
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&iv.png);
        let lines: Vec<String> = b64
            .as_bytes()
            .chunks(120)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        if let Some(prev) = self.view.pager.take() {
            self.view.pager_history.push(prev);
        }
        self.view.pager = Some(crate::ui::pager::PagerView::new_plain(
            "mermaid diagram \u{2014} PNG base64",
            lines,
        ));
        self.view.needs_full_repaint = true;
    }
}

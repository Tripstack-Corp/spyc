//! Off-thread mermaid rendering (`Effect::RenderMermaid`), two modes:
//!
//! - **Open**: render to a PNG, write a temp file, hand it to the OS viewer via
//!   `open::that_detached` (Preview.app — the full-res read path; works on any
//!   local terminal).
//! - **View**: render to a `ratatui_image::Protocol` sized to the terminal and
//!   show it as a full-screen overlay *inside* spyc (graphics terminals only;
//!   the `Picker` is supplied by `run_effects`, `None` ⇒ no image protocol).
//!
//! Both are far too heavy (parse → layout → SVG → resvg raster → font load) for
//! the loop, so they run on a detached worker like the graveyard ops. The
//! worker pushes a [`MermaidOutcome`] onto `runtime.mermaid_results` and wakes
//! the loop with `Message::MermaidDone`; `App::apply_mermaid_outcomes` (pre-recv
//! scan) opens/installs the result and flashes status. All pure-Rust
//! (mermaid-rs-renderer → resvg), no Node/Chromium.
//! See `docs/MERMAID_PAGER_PLAN.md`.

use std::path::PathBuf;

use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

/// What to do with a rendered diagram.
#[derive(Debug)]
pub enum MermaidMode {
    /// Render → temp PNG → open in the OS viewer (the `o` key).
    Open,
    /// Render → `Protocol` sized to `cols`×`rows` cells for a full-screen
    /// in-spyc overlay (the `i` key). The `Picker` is injected by `run_effects`.
    /// `dark` selects the dark theme (the `c` toggle).
    View { cols: u16, rows: u16, dark: bool },
}

/// A render request handed to the worker via `Effect::RenderMermaid`.
#[derive(Debug)]
pub struct MermaidRenderOp {
    /// Raw ` ```mermaid ` block source.
    pub source: String,
    pub mode: MermaidMode,
}

/// Worker result, applied by `apply_mermaid_outcomes`.
pub enum MermaidOutcome {
    /// Open path: rendered + handed to the OS viewer.
    Opened,
    /// Open path failed; short reason for the status line.
    Failed(String),
    /// View path: protocol ready for the full-screen overlay, plus the PNG
    /// bytes (for `s`/`y`/`b`) and the mermaid source (for `Y`).
    Viewed {
        protocol: Box<Protocol>,
        png: Vec<u8>,
        source: String,
        /// Which theme this was rendered with — tracked so `c` can toggle it.
        dark: bool,
    },
    /// View path failed (incl. "no image protocol"); short reason.
    ViewFailed(String),
}

/// Render `op.source` per its mode. Runs on the detached worker — all IO and
/// the (blocking) protocol encode are off the loop. `picker` is `Some` only
/// when the terminal supports a graphics protocol (needed by `View`).
pub fn render_mermaid_op(op: MermaidRenderOp, picker: Option<Picker>) -> MermaidOutcome {
    let MermaidRenderOp { source, mode } = op;
    match mode {
        MermaidMode::Open => match render_to_png(&source, None, false) {
            Ok(bytes) => open_png(&source, &bytes),
            Err(e) => MermaidOutcome::Failed(e),
        },
        MermaidMode::View { cols, rows, dark } => {
            let Some(picker) = picker else {
                return MermaidOutcome::ViewFailed(
                    "terminal has no image protocol (use `o` to open externally)".to_string(),
                );
            };
            match render_to_protocol(&source, &picker, cols, rows, dark) {
                Ok((protocol, png)) => MermaidOutcome::Viewed {
                    protocol: Box::new(protocol),
                    png,
                    source,
                    dark,
                },
                Err(e) => MermaidOutcome::ViewFailed(e),
            }
        }
    }
}

/// Persist the PNG to a stable temp path (one file per diagram source) and open
/// it in the OS viewer.
fn open_png(source: &str, bytes: &[u8]) -> MermaidOutcome {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    let path: PathBuf = std::env::temp_dir().join(format!("spyc-mermaid-{:016x}.png", h.finish()));
    if let Err(e) = std::fs::write(&path, bytes) {
        return MermaidOutcome::Failed(format!("write temp file: {e}"));
    }
    match open::that_detached(&path) {
        Ok(()) => MermaidOutcome::Opened,
        Err(e) => MermaidOutcome::Failed(format!("open: {e}")),
    }
}

/// mermaid source → a `Protocol` filling `cols`×`rows` cells for the overlay.
/// We render the *vector* SVG at ~the cell area's pixel size (so it fills the
/// screen crisply) — `Resize::Fit` only downscales, so a small natural raster
/// would otherwise sit tiny in a corner.
fn render_to_protocol(
    source: &str,
    picker: &Picker,
    cols: u16,
    rows: u16,
    dark: bool,
) -> Result<(Protocol, Vec<u8>), String> {
    let font = picker.font_size();
    let box_px = (
        u32::from(cols) * u32::from(font.width.max(1)),
        u32::from(rows) * u32::from(font.height.max(1)),
    );
    let bytes = render_to_png(source, Some(box_px), dark)?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("decode: {e}"))?;
    let protocol = picker
        .new_protocol(
            img,
            ratatui::layout::Size::new(cols, rows),
            ratatui_image::Resize::Fit(None),
        )
        .map_err(|e| format!("protocol: {e}"))?;
    // Return the PNG too — the overlay keeps it for `s`/`y`/`b` without re-render.
    Ok((protocol, bytes))
}

/// mermaid source → PNG bytes, pure-Rust: mermaid-rs-renderer for the SVG, resvg
/// (system fonts + white background) for the raster. `fit_px` (when `Some`)
/// renders the vector scaled to fill that pixel box, preserving aspect — so a
/// small diagram fills a large terminal crisply; `None` keeps natural size (the
/// external-open path, where the viewer does its own scaling). `Err` carries a
/// short reason for the status line.
/// A dark `Theme` for the `c` toggle. The renderer ships only light themes
/// (`modern` / `mermaid_default`), so we override `modern()`'s key colours with
/// a terminal-friendly dark palette (Catppuccin Mocha-ish).
fn dark_theme() -> mermaid_rs_renderer::Theme {
    let mut t = mermaid_rs_renderer::Theme::modern();
    t.background = "#1e1e2e".to_string();
    t.primary_color = "#313244".to_string(); // node fill
    t.primary_text_color = "#cdd6f4".to_string();
    t.primary_border_color = "#89b4fa".to_string();
    t.line_color = "#9399b2".to_string(); // edges
    t.text_color = "#cdd6f4".to_string();
    t.edge_label_background = "#1e1e2e".to_string();
    t.cluster_background = "#181825".to_string(); // subgraph fill
    t.cluster_border = "#45475a".to_string();
    t.secondary_color = "#45475a".to_string();
    t.tertiary_color = "#585b70".to_string();
    t
}

fn render_to_png(source: &str, fit_px: Option<(u32, u32)>, dark: bool) -> Result<Vec<u8>, String> {
    use resvg::tiny_skia::{Color, Pixmap, Transform};
    // Slightly roomier than the crate's 50/50 default — eases the label/box
    // overlaps seen on dense flowcharts.
    let mut opts = mermaid_rs_renderer::RenderOptions::default()
        .with_node_spacing(60.0)
        .with_rank_spacing(70.0);
    if dark {
        opts.theme = dark_theme();
    }
    let svg = mermaid_rs_renderer::render_with_options(source, opts).map_err(|e| e.to_string())?;
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(&svg, &opt).map_err(|e| format!("svg parse: {e}"))?;
    let nat = tree.size();
    let (pw, ph, transform) = match fit_px {
        Some((bw, bh)) if nat.width() > 0.0 && nat.height() > 0.0 => {
            let scale = (bw as f32 / nat.width())
                .min(bh as f32 / nat.height())
                .max(0.01);
            (
                ((nat.width() * scale).ceil() as u32).max(1),
                ((nat.height() * scale).ceil() as u32).max(1),
                Transform::from_scale(scale, scale),
            )
        }
        _ => {
            let s = nat.to_int_size();
            (s.width(), s.height(), Transform::identity())
        }
    };
    let mut pixmap = Pixmap::new(pw, ph).ok_or_else(|| "diagram has zero size".to_string())?;
    // Fill the background to match the theme, so transparent SVG regions don't
    // show the wrong colour (white behind a dark diagram, or black behind light).
    pixmap.fill(if dark {
        Color::from_rgba8(30, 30, 46, 255) // #1e1e2e, matches dark_theme().background
    } else {
        Color::WHITE
    });
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| format!("png encode: {e}"))
}

impl super::App {
    /// Pre-recv drain: open/install finished `Effect::RenderMermaid` outcomes
    /// and flash status. Returns whether a redraw is needed. Mirrors
    /// `apply_graveyard_outcomes`.
    pub(crate) fn apply_mermaid_outcomes(&mut self) -> bool {
        let outcomes: Vec<MermaidOutcome> = {
            let mut slot = self.runtime.mermaid_results.lock().unwrap();
            if slot.is_empty() {
                return false;
            }
            std::mem::take(&mut *slot)
        };
        let mut redraw = false;
        for outcome in outcomes {
            redraw = true;
            match outcome {
                MermaidOutcome::Viewed {
                    protocol,
                    png,
                    source,
                    dark,
                } => {
                    // Install the full-screen overlay; the draw blits it.
                    self.view.image_view = Some(super::ImageView {
                        protocol: *protocol,
                        png,
                        source: Some(source),
                        dark,
                        flash: None,
                    });
                }
                MermaidOutcome::Opened => self.flash_pager("opened diagram in external viewer"),
                MermaidOutcome::Failed(reason) | MermaidOutcome::ViewFailed(reason) => {
                    self.flash_pager(&format!("mermaid render failed: {reason}"));
                }
            }
        }
        redraw
    }

    /// Set the pager status-line flash if a pager is open (no-op otherwise).
    fn flash_pager(&mut self, msg: &str) {
        if let Some(pager) = self.view.pager.as_mut() {
            pager.flash = Some(msg.to_string());
        }
    }
}

//! Off-thread mermaid render + external open (`Effect::RenderMermaid`).
//!
//! Rendering a diagram (parse → layout → SVG → resvg raster → font load) is far
//! too heavy for the input/render thread, so it runs on a detached worker like
//! the graveyard ops: `render_mermaid_op` renders the block to a PNG, writes it
//! to a temp file, and hands it to the OS viewer via `open::that_detached`
//! (Preview.app on macOS — full zoom/scroll, the way you actually read a dense
//! diagram). The worker pushes a [`MermaidOutcome`] onto
//! `runtime.mermaid_results` and wakes the loop with `Message::MermaidDone`;
//! `App::apply_mermaid_outcomes` (pre-recv scan) surfaces the result in the
//! pager status line. See `docs/MERMAID_PAGER_PLAN.md` (Phase 3).
//!
//! All pure-Rust (mermaid-rs-renderer → resvg), no Node/Chromium.

use std::path::PathBuf;

/// A render request handed to the worker via `Effect::RenderMermaid`.
#[derive(Debug)]
pub struct MermaidRenderOp {
    /// Raw ` ```mermaid ` block source.
    pub source: String,
}

/// Worker result, surfaced in the pager status line by `apply_mermaid_outcomes`.
pub enum MermaidOutcome {
    /// Rendered and handed to the OS viewer.
    Opened,
    /// Render/open failed; carries a short reason for the status line
    /// (syntax error, unsupported shape, raster failure, …).
    Failed(String),
}

/// Render `op.source` to a PNG, persist it to a temp file, and open it in the
/// OS image viewer. Runs on the detached worker — all IO is off the loop.
pub fn render_mermaid_op(op: MermaidRenderOp) -> MermaidOutcome {
    let bytes = match render_to_png(&op.source) {
        Ok(b) => b,
        Err(e) => return MermaidOutcome::Failed(e),
    };
    let path = temp_png_path(&op.source);
    if let Err(e) = std::fs::write(&path, &bytes) {
        return MermaidOutcome::Failed(format!("write temp file: {e}"));
    }
    match open::that_detached(&path) {
        Ok(()) => MermaidOutcome::Opened,
        Err(e) => MermaidOutcome::Failed(format!("open: {e}")),
    }
}

/// Stable temp path per diagram source, so re-opening the same block reuses
/// (overwrites) one file instead of littering `$TMPDIR`.
fn temp_png_path(source: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    std::env::temp_dir().join(format!("spyc-mermaid-{:016x}.png", h.finish()))
}

/// mermaid source → PNG bytes, pure-Rust: mermaid-rs-renderer for the SVG,
/// resvg (with system fonts + a white background) for the raster. `Err` carries
/// a short reason for the status line.
fn render_to_png(source: &str) -> Result<Vec<u8>, String> {
    let svg = mermaid_rs_renderer::render(source).map_err(|e| e.to_string())?;
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(&svg, &opt).map_err(|e| format!("svg parse: {e}"))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| "diagram has zero size".to_string())?;
    // Mermaid is dark-on-light; fill white so a transparent SVG background
    // doesn't render as black in the viewer.
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| format!("png encode: {e}"))
}

impl super::App {
    /// Pre-recv drain: surface finished `Effect::RenderMermaid` outcomes in the
    /// pager status line (success or the failure reason). Returns whether a
    /// redraw is needed. Mirrors `apply_graveyard_outcomes`.
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
            let note = match outcome {
                MermaidOutcome::Opened => "opened diagram in external viewer".to_string(),
                MermaidOutcome::Failed(reason) => format!("mermaid render failed: {reason}"),
            };
            // Surface in the pager's own status line if it's still open.
            if let Some(pager) = self.view.pager.as_mut() {
                pager.flash = Some(note);
                redraw = true;
            }
        }
        redraw
    }
}

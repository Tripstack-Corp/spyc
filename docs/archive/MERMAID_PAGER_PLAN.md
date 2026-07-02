# Mermaid diagram rendering in the markdown pager — implementation plan

**Status:** COMPLETE — shipped v1.58.11 (#446–#448). Archived as historical record.
**Goal:** when viewing a markdown file (e.g. `share/DEPLOY.md`) in spyc's pager,
render fenced ` ```mermaid ` blocks as actual diagrams (terminal graphics)
instead of dumping the raw source, with a graceful fallback to source on
terminals that lack a graphics protocol.

This plan is grounded in the existing architecture (MVU, effects-as-data,
off-thread workers, the `PagerStream` seam) and in the post-#444 rule:
**no terminal-query round-trips on the live input/render path.**

---

## 1. The shape of the problem

Two independent sub-problems with very different maturity:

- **Mermaid → image** (immature, pre-1.0): no pure-Rust engine is at
  mermaid.js parity. We accept `mermaid-rs-renderer` (pure Rust, no
  Node/Chromium, ~1.4k★, 23 diagram types, emits SVG **and** PNG via a
  transitive `resvg`) as the front-runner, knowing fidelity is "improving but
  not yet mermaid-cli-perfect." This matches spyc's lightweight / no-Chromium
  ethos. `mmdc` (Chromium) is explicitly rejected.
- **Image → terminal** (solid): `ratatui-image` (v11.x, tracks **ratatui
  0.30.1** = spyc's version, ratatui-org maintained) handles
  Kitty/iTerm2/Sixel + unicode-halfblock fallback. `image` for decode.

### The genuinely hard part — images in a scrollable text pager

The pager renders `PagerView.lines: Vec<Line<'static>>` through a ratatui
`Paragraph` (`src/ui/pager/render.rs:140` `render_single_column`). Terminal
graphics protocols paint **pixel rectangles via escape sequences that bypass
the cell grid** and do not clip to a `Rect` the way text does:

- **Kitty**: cell-precise placement, supports cropping → smooth partial scroll
  is possible.
- **iTerm2**: inline placement, limited clipping.
- **Sixel**: no real clipping — a partially-scrolled image is garbage.
- **Halfblocks**: it *is* cells, so it clips perfectly but is low-fidelity.

So a tall diagram that's half-scrolled-off can't be rendered uniformly across
protocols. The MVP sidesteps this (draw the image only when its full row-range
is within the viewport; show a placeholder otherwise); Phase 2 leans on Kitty's
clipping for smooth scroll.

### Inline is a *preview*; "open" is how you read it

The DEPLOY.md spike rendered at **2147×1445 px**. Scaled to fit ~70 terminal
cells wide, that's a legible *overview* but the label text is too small to read
— inherent to terminal-cell resolution, not a rendering bug. So the design is
explicitly two-tier:

- **Inline**: an at-a-glance preview/thumbnail (readable for small diagrams; an
  overview map for dense ones), only on graphics-capable terminals.
- **"Open" escape hatch (first-class, not an afterthought)**: a keybinding that
  writes the rendered PNG to a temp file and hands it to the OS viewer via
  `open::that_detached(path)` — the **existing** spyc pattern
  (`src/app/quick_select.rs:162`), detached so it never tears the TUI. On macOS
  that's Preview.app (full zoom/scroll/pan); Linux-local → `xdg-open` → default
  viewer. This is the same PNG bytes the inline path already produced — one
  render, two sinks — and it works **even on terminals with no graphics
  protocol** (you don't need inline support to render-to-temp-and-open). For a
  dense diagram, this is the primary way to actually read it.
  - **Caveat — needs a local display**: over SSH (the fika-vm) `open`/`xdg-open`
    targets the *remote* machine, which has no GUI → it should no-op with a flash
    ("no display — open locally"). So "open" is a local-machine convenience;
    over SSH the inline preview (if the local terminal + passthrough support
    Kitty/iTerm2) is the only in-place option.

---

## 2. Architecture (fits the MVU invariants)

```
markdown parse (eager, cheap)                 off-thread (Effect + worker)            pure &self draw
─────────────────────────────                 ─────────────────────────────          ───────────────
detect ```mermaid blocks                       mermaid_rs_renderer::render_*  ──┐      stateless Image
→ AppState: Vec<MermaidBlock>     ── Effect ──> → PNG bytes                      │      widget over the
  { source, line_range, id }       Render…      → image::load → DynamicImage     │      block's Rect,
  + placeholder lines in the                    → picker.new_protocol(...)       │      ONLY if fully
  rendered Vec<Line>                            (encode = BLOCKING, off-thread) ─┘      visible
                                                → Runtime slot + wake loop
                                                → pre-recv drain moves ready
                                                  Protocol into ViewState cache
```

**State placement (the three-disjoint-types rule):**

- **Model (`AppState`)**: `Vec<MermaidBlock> { id, source, placeholder_line_range }`
  — pure domain, no handles. Drives "which blocks exist + where."
- **Runtime**: the render worker's outbox (`Arc<Mutex<Vec<MermaidResult>>>`),
  same shape as `graveyard_results`.
- **ViewState**: the ready-to-blit `ratatui_image::protocol::Protocol`s keyed
  by block `id` (a render cache / ephemeral — never in the Model), plus the
  one-time `Picker` (font size + detected `ProtocolType`).

**Effect:** add `Effect::RenderMermaid { id, source, cell_size }`. `run_effects`
spawns a detached worker (graveyard template, `effect.rs:584`), pushes
`MermaidResult { id, protocol | error }` to the Runtime slot, wakes the loop
with a payloadless `Message::MermaidReady` (mirrors `GraveyardDone`). The
pre-recv drain moves it into the ViewState cache and marks a redraw.

**Protocol detection (the #444 rule):** build the `Picker` **once at startup**
in `setup_terminal()` (lib.rs), *before* the input reader thread spawns and
while we still own stdin — same seam as `supports_keyboard_enhancement()`.
Wrap it like that call: on error/timeout (SSH!) fall back to "no graphics →
source mode." **Never** call `Picker::from_query_stdio()` on the live path.
Store the resolved `Picker` (font cell size + `ProtocolType`) in Runtime/ViewState.

**Draw stays pure `&self`:** the stateless `Image` widget renders a
*pre-encoded* `Protocol` with no blocking work. All encoding/resize happens in
the worker (`picker.new_protocol(dyn_img, size, Resize::Fit)` is blocking → off
the draw thread). We do **not** use `StatefulImage` (it encodes at render time).

---

## 3. Crate additions

`Cargo.toml` (validated — builds against ratatui 0.30, cargo-deny green, **zero C deps**):

```toml
ratatui-image = { version = "11", default-features = false, features = ["crossterm"] }
mermaid-rs-renderer = { version = "0.2", default-features = false, features = ["png"] }
image = { version = "0.25", default-features = false, features = ["png"] }
```

- **`ratatui-image` defaults pull `chafa-dyn` → a C library via `pkg-config`.**
  `default-features = false` + `crossterm` drops it. Sixel still works (pure-Rust
  `icy_sixel`); Kitty/iTerm2/halfblocks are built in. We lose only chafa's fancier
  symbol fallback — irrelevant, since our no-graphics fallback is source text.
- `mermaid-rs-renderer` minus its `cli` default; `png` keeps the resvg PNG path.
- Added tree (all pure-Rust, MIT/Apache/MPL, deny-clean): resvg/usvg/tiny-skia/
  fontdb/rustybuzz/ttf-parser/kurbo (SVG+font render), icy_sixel, image+png.
- **Font loading**: resvg renders the mermaid labels as `<text>` → the render
  worker must `fontdb.load_system_fonts()` once (tens of ms) and reuse the
  `fontdb`/`Picker`; an empty fontdb renders boxes with no text. (The fika-vm has
  system fonts.)

---

## 4. Phased implementation (one PR per phase, gated + dogfooded)

### Phase 0 — deps + startup protocol detection (no behavior yet)
- Add crates; `make check` + `make lint-linux` green; cargo-deny clean.
- In `setup_terminal()`: build `Picker` once (font size + `ProtocolType`),
  store on `App`/Runtime. Graceful fallback on query failure (SSH-safe).
- No user-visible change yet. Verifies the dep tree + detection in isolation.

### Phase 1 — detect mermaid blocks (still text-only output)
- In `src/ui/markdown/renderer.rs` `end_code_block()` (~594): when
  `lang == "mermaid"`, instead of plain-dim text, (a) record a `MermaidBlock`
  (source + the line range it occupies) and (b) emit a **placeholder** block of
  lines (a bordered "▣ mermaid diagram — rendering…" box sized to a sensible
  default) into the `Vec<Line>`.
- Thread the collected `Vec<MermaidBlock>` out of `markdown::render` into the
  `PagerView` / `AppState` (extend the return type or a side-channel struct).
- Pure, unit-testable: assert block detection + line-range math on sample
  markdown (incl. multiple blocks, a block at EOF, CRLF). No rendering yet.

### Phase 2 — off-thread render worker (`Effect::RenderMermaid`)
- Add the `Effect` variant + `MermaidResult`/`Message::MermaidReady` +
  Runtime outbox slot + pre-recv drain (graveyard template end-to-end).
- Worker (render once at high res, **keep both outputs**):
  `mermaid_rs_renderer::render(src)` → SVG → resvg(+`load_system_fonts`) → **PNG
  bytes**, then `image::load_from_memory` → `picker.new_protocol(img,
  target_size, Resize::Fit)` → **`Protocol`**. The PNG bytes feed the "open"
  sink (Phase 3); the downscaled `Protocol` feeds the inline sink (Phase 4) —
  one render, two consumers. On error, push the error (→ source fallback +
  status note, Phase 5).
- On pager open (markdown), emit one `RenderMermaid` per detected block.
- Cache by `id` in ViewState; the placeholder updates when ready.

### Phase 3 — "open" externally (simple, universal, the read-it path)
- Action (e.g. `o` while the pager is open and the cursor is on a mermaid
  block): write the block's cached PNG bytes to a temp file
  (`$TMPDIR/spyc-mermaid-<id>.png`) and `open::that_detached(path)` — the
  existing pattern (`quick_select.rs:162`), detached so the TUI isn't torn.
  macOS → Preview.app (zoom/scroll/pan); Linux-local → `xdg-open`.
- Works on **any** terminal locally (no graphics protocol needed). If the PNG
  isn't rendered yet, kick the render and open on completion (or flash "still
  rendering").
- **SSH/no-display**: detect (no `$DISPLAY`/`$WAYLAND_DISPLAY` on Linux, or a
  failed `open`) → flash "no display — open locally"; don't pretend it worked.
- Smallest end-to-end slice that delivers real value and sidesteps the entire
  inline scroll-clip problem.

### Phase 4 — draw the image inline (the visible-only MVP)
- In `pager/render.rs`, after the `Paragraph` body paints: for each block whose
  **full** visual row-range ∈ `[scroll, scroll + viewport_h)` AND whose
  `Protocol` is ready AND graphics are supported, compute its `Rect` and
  `frame.render_widget(Image::new(protocol), rect)` (layers on top of the
  placeholder cells). This is the at-a-glance *preview*; "open" (Phase 3) is how
  you actually read a dense one.
- Partially-scrolled / not-ready / no-graphics → leave the placeholder text
  (Phase 1) showing. Robust on every protocol.
- Scroll math: block visual-row-start derived from its source line range +
  wrap expansion (the pager already computes wrapped offsets for the gutter).

### Phase 5 — fallbacks + failure status
- **Render failure (syntax error, unsupported shapes, panic) → fall back to the
  rendered markdown/source for that block AND surface *why* in the pager status
  line.** The pager already has a status row: `PagerView::status_text()`
  (`src/ui/pager/scroll_search.rs:381`) feeds the reserved bottom row
  (`render.rs:96/131`), today used for search / visual-range / block-dims. Add a
  transient status variant (e.g. a `status_note: Option<String>` the `status_text()`
  fold prefers when set, with the search/visual states still winning while
  active) carrying `⚠ mermaid: <short error> — showing source`. So the failure is
  never silent: source is preserved *and* the reason is visible. (This is the
  first non-search consumer of the status row beyond visual/block mode — keep the
  precedence explicit and unit-test it.)
- No graphics protocol at all: show the **raw mermaid source** (today's
  behavior) inline, with the "open" hook (Phase 3) still available locally;
  optionally a one-time status note "terminal has no image protocol — showing
  source (press `o` to open)."

### Phase 6 — caching, resize, tests, docs
- Invalidate + re-render on terminal resize (cell size / target px change) and
  on file reload; key the cache on `(block_id, content_width)`.
- Tests: pure layers only (block detection, range math, fallback selection,
  effect emission, `open`-no-display gating). `TestBackend` snapshots can't
  capture graphics — the image path is **manual-verify** on Kitty/iTerm2 + the
  fika-vm (Sixel/SSH).
- Docs in the same commits: AGENTS.md module map (markdown/pager/new worker),
  ARCHITECTURE.md thread list (the render worker), and a note in the pager docs.

---

## 5. Risks & open questions

1. **Fidelity**: `mermaid-rs-renderer` is pre-1.0; complex diagrams may look
   off. The source-fallback (Phase 5) bounds the downside, and the "open"
   full-res path (Phase 3) means even an imperfect-but-correct render is useful.
   Spike on the real DEPLOY.md flowchart was faithful. Re-evaluate `mmdc` only if
   fidelity proves unacceptable (and accept the Chromium cost then).
2. **Scroll-clip across protocols**: inline MVP renders only when fully visible.
   Smooth partial-scroll is Kitty-only realistically — but "open" (Phase 3)
   makes this low-stakes, since reading happens in the OS viewer.
3. **Dependency weight**: ✅ resolved in Phase 0 — pure-Rust tree (resvg/usvg/
   tiny-skia/fontdb/rustybuzz/icy_sixel/image/mermaid-rs-renderer), cargo-deny
   green, **zero C deps** (dropped `chafa-dyn`). Builds against ratatui 0.30.
4. **Pager reflow**: the pager wraps text to width; the image block reserves a
   fixed cell height. Width changes must recompute both the placeholder size and
   the target render size.
5. **Multi-column pager mode**: images only make sense in single-column; disable
   image rendering (keep source) in multi-column.
6. **Lower/Top-pane mounts**: decide whether images render in pane mounts or only
   the centered overlay (start: overlay only).

## 6. Effort

~6 PRs (one per phase). Phase 0 (deps) ✅ done; Phase 1 (detect) is small and
pure; Phase 2 (worker) + Phase 3 ("open") are the first user-visible value and
relatively self-contained; Phase 4 (inline blit) is the trickiest; Phase 5–6
are fallbacks/polish. Each behavior PR builds a release and is dogfooded on a
graphics-capable terminal (Kitty/iTerm2/WezTerm) **and** the fika-vm before
merge, per the standing rule. Note Phase 3 ("open") delivers a usable feature
without any of Phase 4's inline-graphics complexity — a natural early ship.

## 7. Future embedded-image possibilities (the infra unlocks these)

Phases 0–3 build a reusable seam — "off-thread produce a `Protocol`, cache it in
ViewState, blit it into a pager Rect when visible" plus startup protocol
detection. Once that exists, cheap follow-ons:

- **Image-file preview**: open a `.png`/`.jpg`/`.gif`/`.webp` in the pager and
  show the actual image (decode via `image`, same `Picker`/`Protocol` path) —
  arguably higher daily value than mermaid, and ~all the work is already done.
- **Other fenced diagram languages**: ` ```dot ` / Graphviz via `layout-rs`
  (pure-Rust DOT→SVG), ` ```plantuml ` (if a renderer exists / shell-out), all
  funneling through the same SVG→`resvg`→`Protocol` pipe.
- **Inline markdown images**: `![alt](./local.png)` in a viewed `.md` rendered
  in place (same block-placeholder + overlay-blit machinery as mermaid).
- **Sparkline/chart blocks**: a ` ```chart ` fence → plotted PNG.
- **Thumbnail column / preview pane**: image preview in a side pane as the
  cursor moves over image files (reuses detection + the LowerPane mount).

These are explicitly out of scope for the initial feature but motivate keeping
the Phase 0–3 seam generic (a `Protocol` cache keyed by an opaque id + source,
not mermaid-specific) so they're additive, not rewrites.

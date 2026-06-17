# Mermaid diagram rendering in the markdown pager — implementation plan

**Status:** proposed (not started)
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

`Cargo.toml`:

```toml
ratatui-image = "11"                 # tracks ratatui 0.30.1; Kitty/iTerm2/Sixel/halfblock
mermaid-rs-renderer = { version = "0.2", default-features = false, features = ["png"] }
# `image` is already transitively present via ratatui-image; add explicit dep if needed.
```

- Disable `mermaid-rs-renderer`'s `cli` default feature (we only want the lib +
  `png`). `resvg` comes in transitively.
- Run `make` (cargo-deny gate) — verify license/advisory acceptance for the new
  tree (resvg, image codecs). This is a **dependency-weight** decision; flag the
  added tree size in the PR per the "lightweight" rule.

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
- Worker: `mermaid_rs_renderer::render_with_options(src, …)` → PNG →
  `image::load_from_memory` → `picker.new_protocol(img, target_size, Resize::Fit)`
  → push `Protocol`. On render error, push the error (→ fall back to source for
  that block).
- On pager open (markdown), emit one `RenderMermaid` per detected block.
- Cache by `id` in ViewState; the placeholder updates to the image when ready.

### Phase 3 — draw the image inline (the visible-only MVP)
- In `pager/render.rs`, after the `Paragraph` body paints: for each block whose
  **full** visual row-range ∈ `[scroll, scroll + viewport_h)` AND whose
  `Protocol` is ready AND graphics are supported, compute its `Rect` and
  `frame.render_widget(Image::new(protocol), rect)` (layers on top of the
  placeholder cells).
- Partially-scrolled / not-ready / no-graphics → leave the placeholder text
  (Phase 1) showing. Robust on every protocol.
- Scroll math: block visual-row-start derived from its source line range +
  wrap expansion (the pager already computes wrapped offsets for the gutter).

### Phase 4 — fallbacks + ergonomics
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
  behavior); optionally a one-time status note "terminal has no image protocol —
  showing source."
- Add an action (e.g. `m` while the pager is open on a mermaid block, or a
  toggle) to open the diagram in a **dedicated full-screen image pager** —
  sidesteps the scroll-clip problem for "I just want to see it big," and works
  acceptably even on Sixel.

### Phase 5 — caching, resize, tests, docs
- Invalidate + re-render on terminal resize (cell size / target px change) and
  on file reload; key the cache on `(block_id, content_width)`.
- Tests: pure layers only (block detection, range math, fallback selection,
  effect emission). `TestBackend` snapshots can't capture graphics — the image
  path is **manual-verify** on Kitty/iTerm2 + the fika-vm (Sixel/SSH).
- Docs in the same commits: AGENTS.md module map (markdown/pager/new worker),
  ARCHITECTURE.md thread list (the render worker), and a note in the pager docs.

---

## 5. Risks & open questions

1. **Fidelity**: `mermaid-rs-renderer` is pre-1.0; complex diagrams may look
   off. Phase 4's source-fallback bounds the downside. Re-evaluate `mmdc` only
   if fidelity proves unacceptable (and accept the Chromium cost then).
2. **Scroll-clip across protocols**: MVP renders only when fully visible. Smooth
   partial-scroll (Phase 2+) is Kitty-only realistically.
3. **Dependency weight**: resvg + image codecs + ratatui-image is a non-trivial
   tree. Measure and justify in the Phase 0 PR.
4. **Pager reflow**: the pager wraps text to width; the image block reserves a
   fixed cell height. Width changes must recompute both the placeholder size and
   the target render size.
5. **Multi-column pager mode**: images only make sense in single-column; disable
   image rendering (keep source) in multi-column.
6. **Lower/Top-pane mounts**: decide whether images render in pane mounts or only
   the centered overlay (start: overlay only).

## 6. Effort

~5 PRs (one per phase). Phase 0–1 are small and low-risk; Phase 2–3 are the
meat (worker + draw integration); Phase 4–5 are polish. Each behavior PR builds
a release and is dogfooded on a graphics-capable terminal (Kitty/iTerm2/WezTerm)
**and** the fika-vm before merge, per the standing rule.

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

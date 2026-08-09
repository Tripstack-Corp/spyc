# Preview images pasted to the agent — plan (#119)

**Status:** planned — scope decided, no code written.
**Measured against:** `b2b391f`.

**Decisions (2026-08-08):** all five PRs are in scope, including the image-file
preview. The gallery gets a real key, `^a g`. Claude-only for the transcript
half; codex/agy wait until their on-disk shape is confirmed.

## The complaint

Claude Code renders a pasted image as the opaque token `[Image #3]`. You can't
see what you attached before you send, and you can't go back and check what
`[Image #3]` *was* three prompts later. spyc sits one layer above the agent,
owns the terminal, and already has a graphics pipeline — it is the right place
to answer both questions.

## What already exists (the leverage)

- `ratatui-image` 11 + a startup-detected `Picker` (`lib.rs::detect_image_picker`,
  incl. the iTerm2 OSC-1337 override — SPYC-TRAP `iterm-osc1337`). Non-graphics
  terminals fall back to **halfblocks**, which is useless for a mermaid diagram
  but genuinely readable for a screenshot.
- `ViewState::image_view: Option<ImageView>` — a modal full-screen overlay with
  working verbs (`s` save, `y` copy image, `Y` copy source, `c` theme, `b`
  base64, `o` open externally, `q`/`Esc`/`i` dismiss) in
  `app/pager_handler/image.rs`, painted by `render/inner.rs`.
- The off-thread render pattern: `Effect::RenderMermaid` → detached worker →
  `runtime.mermaid_results` → payloadless `Message::MermaidDone` → pre-recv
  drain. Exactly the shape a decode+`Protocol` build needs.
- `clipboard::copy_image` already pulls in `arboard` **with image data** — the
  read direction (`get_image`) is the same crate, unused so far.
- Per-agent transcript resolution (`AgentProfile::transcript()` →
  `claude_transcript::resolve_active_jsonl`), session-id-pinned per tab.
- An observe-then-forward hook in the pane key path already exists:
  `InputSink::BottomPane` tracks `pane_prompt_buf` for `yP` before forwarding.

Gap worth naming: spyc has **no image-file preview at all** today — `ImageView`
is reachable only from a mermaid block. That's an adjacent freebie, below.

## Two sources of truth, and why both are needed

### 1. The clipboard, at paste time (pre-submit)

The terminal never carries image bytes — bracketed paste is text. Claude Code
reads the **system clipboard itself** when you press `Ctrl+V`. spyc sees only
the keystroke, which is enough: spyc can read the same clipboard, at the same
moment, with `arboard::Clipboard::get_image()`.

This is the only source that exists **while you're still composing** — the
moment the question "what was #2 again?" actually gets asked. It is also
agent-agnostic (works for codex/agy/anything).

### 2. The agent transcript (post-submit) — verified

`~/.claude/projects/<slug>/<session>.jsonl` carries the image inline, next to
the placeholder that names it:

```
type:"user"  message.content = [
  {type:"text",  text:"columns should be space aligned nicely: [Image #3]"},
  {type:"image", source:{type:"base64", media_type:"image/jpeg", data:"…"}}
]
```

Confirmed on a real transcript: the text block and the image block share a
record, and the record carries a `timestamp`. So the transcript is the
authoritative record of what the agent actually received, and it survives spyc
restarts and `claude --resume`.

**Correction (found while building PR 2):** an earlier reading of this file
called `[Image #N]` session-monotonic and therefore a stable key. It is not. A
full scan of the same transcript shows the counter **restarts** mid-file (`#1`
appears again at line 7388, after `#12` at 6408 — a clear or a resume), one
record can carry **two** images (`#6` and `#7` at line 2233, paired positionally
with two image blocks), and some images carry **no label at all** (line 7966).
So **spyc owns the numbering** — a 1-based sequence over the indexed images —
and `[Image #N]` is shown alongside as the agent's own cross-reference, not used
as a key.

Gotchas found while reading it:
- `type:"attachment"` records **duplicate** the same image under
  `attachment.prompt[]` — sometimes *before* the `user` record, sometimes as the
  only copy (there is no `user` twin for `#3`), and sometimes twice over
  (`#12` appears as two attachments). So neither "prefer user" nor "dedupe by
  label" works: dedupe on **content identity** (length + head/tail digest).
- media types are `image/png` **and** `image/jpeg`. spyc's `image` crate is
  currently `default-features = false, features = ["png"]` — jpeg (and webp/gif,
  which the API also accepts) must be added.
- images are 100 KB–670 KB of base64 *each*; a working transcript is multi-MB.
  The reader must stream and **index**, never slurp, and decode only the one
  image the user opens.

Codex: no `input_image` / `data:image/…;base64` records found in any local
rollout, so its on-disk shape is unverified — probe at implementation time,
ship claude first, add codex behind the same profile hook if it's there.

## Staged plan

Five PRs, one root cause each, in dependency order. PRs 3–5 are the issue;
PRs 1–2 are the foundation and pay for themselves.

**Sequencing.** PRs 1 → 2 → 3 → 4 are a straight line and depend on nothing
unproven. PR 5 is gated on the `arboard`-off-the-main-thread question (risk 1),
so **spike that first** — it's ~20 minutes and it's the only finding that could
change the design. If it fails, PRs 1–4 still ship the whole post-submit half
and only the pre-submit preview is lost; the campaign can't stall on it.

### PR 1 — `image_ops`: arbitrary bytes → the existing overlay

- New `src/app/image_ops.rs`: `ImageRenderOp { bytes, cols, rows, label }` →
  detached worker → decode → `Protocol` sized to the cell box → new
  `runtime.image_results` slot + payloadless `Message::ImageDone`.
- Extract mermaid's "raster → `Protocol` fitted to `cols`×`rows`" half into the
  shared builder; `mermaid_ops` calls it. Behavior-preserving, snapshots untouched.
- `ImageView` gains `origin: ImageOrigin { Mermaid{source} | Bytes{label} }` so
  `Y`/`c` (mermaid-only verbs) gate correctly instead of silently no-op'ing, and
  the footer can show `label · 1024×768 · png · 412 KB`.
- `image` crate: `+ jpeg, webp, gif`.
- Tests: fit-box math (pure), origin-gated verbs, decode of tiny png/jpeg fixtures.

### PR 2 — image-file preview (the adjacent freebie)

`i` (or `o` → Preview) on a `.png`/`.jpg`/`.webp`/`.gif` in the list opens it in
the overlay via PR 1's path. Closes a gap `ImageView`'s own doc-comment already
anticipates ("Generalizes to image-file preview"), and exercises PR 1 end-to-end
before any agent plumbing exists.

### PR 3 — transcript image index (the authoritative record)

- `src/state/transcript_images.rs`: stream the jsonl once, emit
  `Vec<TranscriptImage { label:"#3", media_type, byte_len, dims, timestamp,
  prompt_excerpt, offset }>`. Dims come from the header bytes only (decode a
  prefix), not a full decode. Dedupe the `attachment` twin.
- `AgentProfile::transcript_images() -> Option<…>`, `None` by default, **claude
  only** — same shape as `transcript()`. codex/agy return `None` until their
  on-disk shape is confirmed; the gallery degrades to the clipboard-captured
  (PR 5) images alone on those tabs, which is a coherent state, not a hole.
- Off-thread (multi-MB file), via PR 1's worker slot or the `PagerStream` seam.

### PR 4 — the gallery (`^a g` + `:images`)

One list, two sections: **pending** (this prompt, not yet sent) above
**sent** (from the transcript), newest first, each row
`#3  png 1024×768  15:48  "columns should be space aligned nicely:"`.
`Enter`/`i` opens the overlay (PR 1 verbs come free), `n`/`p` step between
images without leaving it, `q` back.

`^a g` because this is **PANE tier** (`Action::tier()` guard requires it on the
`^a` prefix, not the leader), and `g` is one of the few free continuations —
`i`/`I`, `v`/`V`, `u`/`U`, `s`/`S`, `P` are all taken. `:images` is the
registered `COMMAND_TABLE` entry; the key is the convenience alias. The chord
hint for `^a` picks the row up for free, which is the discovery path.

### PR 5 — live capture at paste time (the pre-submit half)

- `AgentProfile::image_paste_key() -> Option<KeyEvent>`, default `None`,
  `Ctrl+V` for claude — per-agent, because it's the agent's binding, not spyc's.
- In `InputSink::BottomPane`, alongside the existing `pane_prompt_buf` tracking:
  on a match **while an agent tab is focused**, forward the key unchanged *and*
  emit `Effect::CaptureClipboardImage`. Never consume it. Never fire on a shell
  tab — `Ctrl+V` there is readline's quoted-insert and the clipboard read would
  be pure waste.
- Worker does the `arboard` read; "no image on the clipboard" is a silent
  no-op, not a flash.
- Storage: a per-tab capped ring in `state.pane` holding the **encoded** bytes
  (not RGBA — a 1080p screenshot is 8 MB decoded), cap ~8 images / ~64 MB,
  cleared on `Enter` (the images have moved to the transcript by then). Memory
  only: nothing touches disk unless the user presses `s`. Worth saying out loud
  in the docs — these are the user's screenshots.
- Feedback is **non-modal**: a transient divider/status note
  `📎 img 2 · 1024×768 · png`. A popup on every paste would be intolerable.
  `[pane] preview_pasted_images = true|false` to disable the capture entirely.

## Risks and traps

1. ~~**`arboard` off the main thread on macOS.**~~ **RESOLVED** — spiked on
   macOS 25.6: a clipboard image round-trips exactly (dims and pixels) when read
   from a spawned thread, in 0.66 ms at 64×32 and 3.25 ms at 3840×2160. No
   main-thread requirement, no hang.

   The spike did surface a design consequence: `get_image` returns **RGBA, not
   the original encoded bytes** — 33 MB for a 4K screenshot. So the capture
   worker must encode to PNG before storing (the encode, not the read, is the
   cost — pay it once, off-thread) and the ring must cap on *encoded* bytes. A
   full-res encode is kept rather than a downscale so the overlay's `s` verb
   saves what the agent actually received.
2. **Sync-update swallows images** — `run.rs:315` already disables DEC 2026 sync
   output while `image_view` is `Some`. Any new image surface inherits that
   condition; forget it and the image never paints.
3. **Full repaint re-blits the image** (visible flash). The mermaid campaign's
   footer lesson applies verbatim: overlay footer/status updates must mark a
   diff draw, never `needs_full_repaint`.
4. **Non-graphics terminals.** `picker` may be halfblocks or `None`. Degrade,
   never dead-end: the gallery still lists metadata, `o` opens externally via
   the temp-file + `open::that_detached` path, `s` saves. For photographs
   halfblocks is an acceptable preview — say so rather than refusing.
5. **Over SSH** the clipboard read hits the *server's* clipboard (the same
   asymmetry `clipboard.rs`'s OSC-52 comment documents). But Claude's own
   `Ctrl+V` read has the identical problem, so parity holds: if the paste worked
   for the agent, the read works for spyc. The transcript path is unaffected.
6. **Transcript size** — index lazily, decode one image on demand. A naive
   `read_to_string` on a 40 MB jsonl full of base64 would stall the worker and
   balloon RSS.
7. **New `image` features** grow the dependency tree → re-check `audit.yml` /
   cargo-deny licensing before merging PR 1.

## Docs each PR must carry (same commit, not a follow-up)

- PR 1: `AGENTS.md` module index (`image_ops.rs` — guard-checked by
  `every_app_module_is_in_the_agents_index`).
- PR 2: `FEATURES.md`, `docs/KEYBINDINGS.md`, `src/ui/help.rs`.
- PR 3: `AGENTS.md` module index (`transcript_images.rs`), `docs/HARNESS.md`
  (the per-agent quirk: claude stores images inline, codex unconfirmed).
- PR 4: `FEATURES.md`, `docs/KEYBINDINGS.md`, `src/ui/help.rs`, `README.md`.
- PR 5: `CONFIGURATION.md` (`[pane] preview_pasted_images`),
  `docs/AGENT_ORCHESTRATION.md` if the profile hook lands near the status wiring.

No version bumps (`main` is the `-CURRENT` stream); each commit's
`type(scope): subject` *is* its CHANGELOG line.

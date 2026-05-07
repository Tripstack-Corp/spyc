# spyc v1.5 — pager / task-viewer unification

Working doc for the v1.5 release. Pager and task viewer become
*renderers* you can mount into any of three slots — overlay,
top pane, lower pane — and the lower pane's `^a-v` scroll mode
becomes a real pager (search, jump, range yank) instead of a
flat byte buffer. Tasks gain bidirectional flow with panes:
yank a pane into a background task, push a backgrounded task
into a new pane.

This is a 1.5 bump because it shifts the model from "modal
overlays / discrete pane types" to "one renderer with three
mount points and shared state with the pty subsystem." Visible
to users; invisible breakage potential is small if we phase it.

## Why this is a 1.5

- The pager already covers everything `$PAGER` covers (search,
  syntax, line numbers, range yank, ANSI, markdown, jump-to-line).
  Continuing to spawn `less` for `D` and `p` is leaving capability
  on the floor.
- Pane scroll mode is the current weak point in the agent
  workflow. "Read what claude printed 200 lines ago" today means
  scrolling without search, without jump, without yank-by-range.
  The same pager that handles `! cmd` capture should handle pane
  history.
- Task → pane → background-task migration is the long-tail piece
  the BUGS.md "task runner should be equivalent to a lower pane"
  entry has been pointing at.

## Goal — what "v1.5 done" looks like

- A `PagerView` knows it can be `Overlay | TopPane | LowerPane`
  and `App::render` mounts it accordingly. The widget already
  takes a `Rect`; the `Mount` enum just decides which one.
- `^a-v` (pane scroll) opens a pager mounted in the lower pane,
  fed by the pty's scrollback (cell-grid → styled lines). All the
  pager features work: `/` search, `:N` jump, `V` visual, `y`
  yanks, `[t/]t` cycles task viewers, etc.
- `D` opens the cursor file in the in-app pager (mounted top, not
  centered overlay). Falls through to overlay-`$PAGER` for files
  past `MAX_PAGER_BYTES` so streaming-from-disk stays available.
- Visual mode gains **block (columnar) selection** in addition
  to line. `^v` enters block mode; `y` yanks the rectangle.
- A backgrounded task (`! cmd` then `^z`) can be promoted to a
  new pane tab. A pane tab can be demoted to a background task
  (running pty stays alive, we stop displaying it).

## Non-goals for v1.5

- Forwarding the child's *cursor shape* (nvim's `\x1b[5 q`) to the
  host terminal. Worth doing — but it's an independent piece,
  separately landable.
- Mouse capture inside panes. (Tracked separately in BUGS.md
  BIGGER section.)
- Real Model-View-Update refactor of `app/mod.rs` — that's
  `REFACTOR_PLAN.md`'s territory.

## Phases

Each phase ends with a green `make check` and a shippable PR.
Phases compose; the user gets value at every step even if 1.5
slips.

### Phase 1 — `Mount` enum on `PagerView`

**What:** Replace the current implicit "pager is always centered
overlay" with an explicit `Mount` field:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mount {
    /// Centered overlay (current behavior).
    Overlay,
    /// Mounted into the top-pane slot, replacing the file list.
    /// The bottom pane still renders below.
    TopPane,
    /// Mounted into the bottom-pane slot, replacing the pty.
    /// Used by lower-pane scrollback.
    LowerPane,
}
```

`App::render` reads `view.mount` and passes the right `Rect`
into the existing `pager::render`. The widget is unchanged; we
only change *where* it draws.

**Scope:**
- Add `Mount` to `PagerView` (default `Overlay` so all existing
  call sites are correct without edits).
- Wire one new mount path in `App::render`. Don't migrate any
  callers yet — Phase 1 just lays the rail.

**Success:** all 566 tests still pass; nothing visible changes;
new `Mount::TopPane` rendering is exercised via a one-shot
manual test (e.g. a `:tppager` debug command that mounts the
help view top-pane briefly).

**Risk:** very low. Pure plumbing.

### Phase 2 — Pty scrollback adapter

**What:** A new module `src/ui/scrollback.rs` that converts
`vt100::Screen`'s scrollback (cell grid) into
`Vec<Line<'static>>` with styles preserved (one ratatui span per
contiguous run of identical-style cells, trim trailing blanks).

**Scope:**
- New module. Pure function `pub fn lines_from_scrollback(screen:
  &vt100::Screen) -> Vec<Line<'static>>`.
- Unit tests: empty scrollback → empty vec; styled cell run →
  single span with that style; trailing whitespace trimmed; CJK
  wide chars counted correctly.
- No integration with `App` yet — just the adapter.

**Success:** golden-style snapshot tests of small scrollback
buffers. ~80 lines of new code, fully tested.

**Risk:** low. vt100 0.16's `Screen` API is well-defined; this
is straight transcription.

### Phase 3 — `^a-v` becomes pager-mounted-LowerPane

**What:** Replace the current pane scroll mode with a
`PagerView::new_styled(...)` built from the scrollback adapter,
mounted `LowerPane`. `Esc` exits the pager and resumes the live
pty view; the pager's `task_id` is repurposed to a new
`pane_scroll_id` so the dispatch path knows it's a pane scroll
(not a task viewer).

**Scope:**
- New PagerView builder that takes the source pane's scrollback
  and freezes it (snapshot — live updates aren't relevant in
  history view).
- `Action::PaneScrollEnter` (already exists) now constructs and
  mounts the pager instead of toggling vt100's scroll mode.
- Default `show_line_numbers = false` for this pager (avoid the
  visual jump from line-number gutter appearing). User can
  toggle with `l` like in any pager.
- Exit: `Esc` / `q` drops the pager and returns focus to the
  live pane.

**Success:** `^a-v` opens the pager filled with the pane's
history; `/foo` finds matches; `V` enters visual; `y` yanks the
range; `Esc` returns to the live pane. All existing pager keys
work (or are deliberately disabled — e.g. `[t/]t` task cycle is
nonsensical here, becomes a no-op).

**Risk:** medium. Needs to coexist with the existing pane
scroll path; we're effectively retiring it. Dropping the old
path immediately is cleaner than carrying both.

### Phase 4 — Block (columnar) visual mode

**What:** `^v` from normal pager mode enters `VisualBlock`
(rectangle selection). `j/k` extend rows, `h/l` extend columns,
`y` yanks the rectangle (newline-joined; trailing whitespace
preserved within columns).

**Scope:**
- `VisualSelection` gains a `kind: Line | Block` discriminator.
  Anchor and cursor stay (line, col); block mode reads both
  axes, line mode only reads line.
- Renderer paints the selection rect (current code paints whole
  lines; block paints `[lo_col..=hi_col]` of `[lo_line..=hi_line]`).
- `yank_visual_to_clipboard` branches on `kind`.
- Tests: block range derivation, yank text construction,
  edge cases (empty rows, wide chars).

**Success:** can `^v` then drag a 5×3 rectangle in a CSV file,
yank it, paste it elsewhere as a 5×3 rectangle.

**Risk:** medium. Wide-char (CJK) handling for column selection
is fiddly. Visual rendering during wrap is also tricky — we may
need to disable wrap inside block mode (vim does the same).

### Phase 5 — `D` uses in-app pager (overlay fallback for huge files)

**What:** `Action::DisplayInPane` opens the cursor file as a
`PagerView` mounted `TopPane` instead of spawning `$PAGER` in a
top overlay. Falls through to the existing
overlay-`$PAGER` path when:
- file size > `MAX_PAGER_BYTES` (streaming from disk wins for
  multi-GB files), OR
- `--in-pager` mode is explicitly disabled (escape hatch).

**Scope:**
- New `display_in_pane` body that loads the file via the same
  `crate::fs::ops::read_truncated` path the centered pager
  already uses.
- Mounted `TopPane` so the bottom pane stays visible — same
  workflow benefit as today's `D`, but with our richer pager.
- `^a-j` / `^a-k` flips focus between the in-app pager and the
  bottom pane (already wired for top-overlay; reuse the same
  routing — pager in `TopPane` mount counts as "top focused").

**Success:** `D` on a markdown file → rendered in top pane,
syntax highlighted, claude visible below, `^a-j` to type, `^a-k`
to scroll. `D` on a 500MB log → still spawns less in overlay.

**Risk:** low — depends on Phase 1 (Mount enum) and the
existing pager infrastructure.

### Phase 6 — Task ↔ pane migration

**What:** Two new actions:
- `:task-to-pane [N]` — promote a backgrounded `!` task to a
  new pane tab. Reads from the task's pty (already alive),
  registers it in `pane_tabs`, drops it from `background_tasks`.
- `:pane-to-task` — demote the active pane tab to a background
  task. Inverse of above. Pane keeps running; just stops
  displaying.

**Scope:**
- A shared `PtyHost` trait or struct that both `Pane` and
  `BackgroundTask` wrap. Today they each hold their own
  `child` / `writer` / `output_rx`; unifying lets us move the
  same handles between containers.
- Promotion / demotion functions on `App`. Mostly housekeeping:
  re-tag the task's title as a tab label, swap the buffer into
  vt100 parser state (or fresh-start the parser if we don't
  preserve scrollback through migration — TBD).
- Tests: round-trip a task to a pane and back, confirm the
  child PID is unchanged and output keeps flowing.

**Success:** start `! npm run dev` → ^z to background → `:task-to-pane`
→ now it's a pane tab next to claude. From there, `:pane-to-task`
puts it back in the background list.

**Risk:** highest of the phases. Touches both pty subsystems
and the surrounding state machine. Save for last.

## Sequencing

Phases 1–3 are the headline win — pager-as-scrollback. They're
interlocking:
1. Mount enum (rail-laying)
2. Scrollback adapter (data)
3. `^a-v` rewrite (consumer)

Ship those three together as the first 1.5-track release
(probably v1.42.0 → first 1.5 alpha). 4 (block yank) and 5
(`D` upgrade) are independent of each other and either can
follow. Phase 6 (task ↔ pane) waits for 1–5 because the cleaner
pty abstraction makes it tractable.

## Open questions

- **Live updates in scrollback view?** Phase 3 freezes a
  snapshot. If the user wants the scrollback to keep growing
  while they're scrolled back (so `]t` to next match keeps
  working as new output arrives), we need either a refresh
  hook or a "snapshot at entry, append during view." Probably
  v1.5 fix-ups, not core 1.5.
- **`gd` / `gf` from pane scrollback?** Today `gf` parses a
  filename out of the pane's last 200 lines. With the pager
  mounted lower, `gf` should mean "open the file under the
  pager cursor." Need to decide whether to keep both bindings
  or unify. Probably unify — pager `gf` works on the highlighted
  match, which is the same conceptual thing.
- **Where does the pager footer go in `LowerPane` mount?** The
  status / search bar currently lives at the pager's bottom
  edge. In `LowerPane` mount that's the same row the pane
  status line normally occupies. Might end up sharing it (a
  modal indicator: "[scroll] /search 3/17") rather than
  stacking.
- **Cursor-shape forwarding** (nvim's beam-on-insert) is
  out-of-scope for 1.5 but the v1.41.26 fix surfaced it. Track
  separately so v1.5 isn't blocked on it.

## Tracking

Open this doc and `ROADMAP.md` at the start of each session.
Phases get checked off here; user-visible changes go to
`CHANGELOG.md` per the existing pattern. When all six phases
ship, bump `Cargo.toml` to `1.5.0` and tag.

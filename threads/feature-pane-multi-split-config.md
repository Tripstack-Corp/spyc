# feature-pane-multi-split-config — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: feature-pane-multi-split-config
Created: 2026-05-15T08:01:56.260725+00:00

---
Entry: Claude Code (caleb) 2026-05-15T08:01:56.260725+00:00
Role: planner
Type: Plan
Title: Plan: configurable pane count + layout (splits, not just tabs)

Spec: planner-architecture

## Why this thread exists

Today spyc has exactly one bottom-pane region. Multiple commands live there as **tabs**, not splits — `pane_tabs: Option<PaneTabs>` (`src/app/mod.rs:528`) is a `Vec<TabEntry>` with one active index. Caleb has asked twice now whether pane count and layout could be configurable; this thread plans that out.

For background: the toggle-context thread (`feature-pane-toggle-preserve-context`) and the same-day findings entry (`caleb-initial-thoughts-and-findings` #4) both note that `pane.default_command` is the only `[pane]` knob today and that startup is single-tab only.

## Current shape

- Layout computation: `App::compute_layout` (`src/app/mod.rs:1892`) takes a single `pane_pct: u16` and a `status_position`. Pane area is a single horizontal slice at the top or bottom.
- One vt100 parser per `TabEntry`, only the active tab's grid is rendered (`PaneWidget`, `src/pane/widget.rs`).
- Focus is binary: `state.pane_focused: bool` (list vs. pane). Zoom is binary too (`pane_zoomed`).
- Resize: child ptys get one `resize(rows, cols)` per tab from `compute_layout`'s `pane` rect.
- Tab management already covers most of what users want from "multiple panes": ^a c new, ^W ] / ^W [ cycle, ^W 1..9 jump, ^W x close, ^W r rename, ^W R restart, ^W z zoom.

So "more panes" really means: **split the pane region into N rectangles, each holding its own tab stack**, with independent focus and sizing.

## Goal

- Start spyc with K bottom-pane splits, each carrying its own startup command (and optionally tab list).
- Resize splits at runtime, persist sizes per session.
- One focus indicator across { list, split-1, split-2, …, split-N }, cyclable.
- All existing per-tab affordances still work inside each split.
- No regression to the single-split default.

## Option space

### Option 1 — Horizontal-only splits (poor man's tmux)

Keep the bottom region a single horizontal band; subdivide it left/right into N columns. Each column has its own `PaneTabs`.

Shape:
```toml
[pane]
splits = ["claude", "bash"]            # two cols, equal width
# splits = ["claude", "bash", "codex"] # three cols
split_pcts = [60, 40]                  # optional widths
```

Tradeoffs:
- ✅ Smallest blast radius. `compute_layout` returns `Vec<Rect>` for the pane region instead of `Option<Rect>`. Focus expands from bool to `enum Focus { List, Pane(usize) }`.
- ✅ All tab machinery transplants intact; one `PaneTabs` per column.
- ⚠️ No way to express row/column hybrids ("claude on top, two shells below it"). For most use cases that's fine.
- ⚠️ Resize bindings need a per-split selector (`^W <` / `^W >` shrink/grow horizontal — distinct from `^W +/-` vertical).

### Option 2 — Tree layout (full tmux/wezterm parity)

Express splits as a tree:

```toml
[[pane.splits]]
type = "vertical"           # split top/bottom
size_pct = 50
[[pane.splits.children]]
command = "claude"
[[pane.splits.children]]
type = "horizontal"         # left/right inside this half
[[pane.splits.children.children]]
command = "bash"
[[pane.splits.children.children]]
command = "lazygit"
```

Or a more compact DSL string: `"v(claude, h(bash, lazygit))"`.

Tradeoffs:
- ✅ Maximally flexible. Matches what power users coming from tmux expect.
- ✅ Resize semantics are local to each split node — standard "drag the divider" model.
- ⚠️ Layout state goes from `pane_height_pct: u16` to a full tree. Persistence schema for `Session` (`src/state/sessions.rs:72`) gains `pane_tree: PaneNode`.
- ⚠️ Render path becomes recursive: walk the tree, allocate Rects per node. New tests for every leaf-vs-branch case.
- ⚠️ Focus needs an ordering — usually "depth-first leaves" for tab-style cycling, plus directional movement (`^W h/j/k/l`). Doable, but a chunk of work.
- ⚠️ Config-DSL discoverability hurts. TOML is awkward for trees; the inline string is easier to write but needs a parser.

### Option 3 — Grid layout (named rows × cols)

Force a regular grid, parameterized by row and column counts and a list of commands by index:

```toml
[pane]
rows = 2
cols = 2
commands = ["claude", "bash", "logs", "lazygit"]
# row_pcts = [60, 40]
# col_pcts = [50, 50]
```

Tradeoffs:
- ✅ Trivially serializable; trivial render. Resize is two slider vectors.
- ⚠️ Less flexible than trees (no "one tall left + two short right"). Probably fine for ≥80% of asks.
- ⚠️ Reads natural for "I want a 2×2 dashboard"; awkward for "I want one big pane plus a small status pane".

### Recommendation

**Option 1 (horizontal-only) as v1, Option 2 (tree) deferred behind a feature flag.**

The 80% use case is "claude on the left, bash on the right" or "claude + bash + codex". A single `pane.splits = [...]` array gets there with a fraction of the code in the recursive-tree version. If users ask for nested splits later, the tree path is an extension — the array form maps to a degenerate tree (all leaves at depth 1).

## Proposed config surface (Option 1)

```toml
[pane]
default_command = "claude"   # existing — used when splits[] is empty
splits = []                  # new — list of per-column startup commands
split_pcts = []              # new — optional widths summing to ≤100; remainder distributes evenly
height_pct = 30              # new — initial bottom-pane height (today hardcoded to 30)
```

Validation:
- `splits.len() ≤ N_MAX` (suggest 6, matching ^W 1..6 reach).
- `split_pcts.len() == splits.len()` or empty.
- `split_pcts.iter().sum() ≤ 100`.
- `height_pct ∈ 10..=90`.

## Internal data-model changes (Option 1)

```rust
// src/app/mod.rs
struct App {
    // Was: pane_tabs: Option<PaneTabs>
    pane_splits: Vec<PaneTabs>,           // empty == "no pane"
    active_split: usize,                  // index into pane_splits
    split_pcts: Vec<u16>,                 // parallel to pane_splits; <100 each
    ...
}

// src/app/state.rs
enum Focus { List, Pane(usize) }          // replaces bool pane_focused
```

`Session` (`src/state/sessions.rs:72`) gains:
```rust
pub splits: Vec<Vec<SavedTab>>,           // outer = split, inner = tab list
pub split_pcts: Vec<u16>,
pub active_split: usize,
```

Backwards-compat: `tabs: Vec<SavedTab>` becomes the single-split shorthand. Loader migrates `tabs` → `splits[0]` on read; saver writes both for one release.

## Key-binding additions

- `^W H` / `^W L` — focus left / right split (or `^W h/l` if we want true tmux parity, but `h` already does something elsewhere; double-check `src/keymap/resolver.rs`).
- `^W <` / `^W >` — shrink / grow the active split horizontally.
- `^W n` (existing) — new tab in *active split*. Unchanged.
- `^a |` or `^W |` — split the active split horizontally (new column). Optional first-version cut.
- `^a -` (existing — pane shrink vertical) stays as-is; affects the whole bottom-region height.

## Implementation order

1. Refactor `pane_tabs: Option<PaneTabs>` → `pane_splits: Vec<PaneTabs>` + `active_split`, **with a one-element vec equivalent to today**. Land this with no user-visible change.
2. Plumb `Focus` enum through resolver / render / input.
3. Extend `compute_layout` to slice the pane rect into N columns by `split_pcts`. Resize all child ptys per column.
4. Read `pane.splits` from config; if non-empty, spawn one tab per split on startup.
5. Add `^W H/L` focus, `^W </>` resize.
6. Extend `Session` save/load to round-trip splits.
7. Doc + spycrc template (`src/config/default.spycrc.toml`).

Items 1 and 2 are the load-bearing refactor; the rest are additive.

## File pointers

- Layout: `src/app/mod.rs:1892` (`compute_layout`, `top_overlay_size`, `pane_spawn_size`).
- Pane state: `src/app/mod.rs:528` (`pane_tabs`), `src/app/state.rs:161-170` (`pending_new_tab_cmd`, `pane_height_pct`).
- Render: `src/pane/widget.rs:67` and the call sites at `src/app/mod.rs:2463/2521/2752`.
- Tab cycle / focus: `src/keymap/action.rs:104-120` (the `Pane*` block), `src/keymap/resolver.rs:200`.
- Config schema: `src/config/mod.rs:34/37/71/91-105` (`LayoutConfig`, `PaneConfig`), default template at `src/config/default.spycrc.toml`.
- Session shape: `src/state/sessions.rs:72` (`Session`), save at `src/app/mod.rs:7629`, restore at `src/app/mod.rs:7913`.

## Open questions

- Do we want splits to live only at the bottom, or also as a top split (e.g. claude in a right column next to the file list)? Today the list is always on top; a right-column option is a larger refactor.
- How does `toggle_pane` interact with multi-split? Probably hide-all (the `feature-pane-toggle-preserve-context` Option A "hide-don't-kill" path generalizes cleanly: hide the entire pane row regardless of split count).
- Should the spycrc `default_command` lose meaning when `splits[]` is set, or fall through? (Recommend: if `splits` empty, use `default_command`. If `splits` non-empty, ignore `default_command`. Document both.)
- Per-split `cwd`? Reasonable to add as `[[pane.split]] command = "...", cwd = "..."` table form once we leave Option 1's compact array.

Companion thread: `feature-pane-state-general-recovery` covers the persistence question for the non-claude case.

tags: #pane #layout #splits #configuration #planning

<!-- Entry-ID: 01KRNAFZ8414WWQYC6PV9C58AR -->

---
Entry: Claude Code (caleb) 2026-05-15T08:10:41.941616+00:00
Role: planner
Type: Plan
Title: Re-scope: configure startup tabs only, no splits

Spec: planner-architecture

## Re-scope (supersedes entry #0)

Caleb clarified after reading the prior entry: he meant configuring the **existing interactive tab system** — multiple tabs in the single bottom-pane region — not new layout primitives. No splits, no tree, no grid. Just: let me declare in `~/.spycrc.toml` that on startup the bottom pane should open with N tabs, the same way `^a c` does interactively.

Discarding the Option-1/2/3 split-layout planning from entry #0. The plan below is the much smaller correct shape.

## Goal

A spycrc knob that opens K tabs in the bottom pane at startup, instead of just one. Each tab gets a command and optional cwd, mirroring what `^a c` (`Action::PaneNewTab`) creates interactively (`src/app/mod.rs:4646-4676`).

## Proposed config surface

Compact array form for the common case:

```toml
[pane]
default_command = "claude"   # existing — used as the single-tab fallback
# tabs = ["claude", "bash", "codex"]   # NEW — opens 3 tabs at startup
```

Table form for cases that need per-tab cwd / label:

```toml
[[pane.tab]]
command = "claude"

[[pane.tab]]
command = "bash"
cwd = "~/scratch"
label = "scratch"           # display name; falls back to command if omitted
```

The two forms coexist (toml-arrays-of-tables and plain arrays are distinguishable). Validation:
- `tabs.len() ≤ 9` (matches `^W 1..9` jump reach — `Action::PaneTabByIndex(u8)`, `src/keymap/action.rs:114`).
- Empty `tabs` / no `[[pane.tab]]` → today's behavior (single tab from `default_command`).
- If both `tabs` and `[[pane.tab]]` are present: hard error at config load.

## Internal changes

`PaneConfig` (`src/config/mod.rs:91`) gains:

```rust
pub struct PaneConfig {
    pub default_command: Option<String>,
    pub tabs: Vec<PaneTabConfig>,        // NEW — empty == single-tab fallback
}

pub struct PaneTabConfig {
    pub command: String,
    pub cwd: Option<PathBuf>,            // resolves `~`, relative-to-cwd
    pub label: Option<String>,
}
```

Wire-up at startup: in `App::new` (around `src/app/mod.rs:875` where `show_session_picker` is conditionally called), after the session-picker branch, **iff** no session restore happened **and** `pane.tabs` is non-empty, loop and call `open_pane_tab_in(cmd, cwd)` per tab (`src/app/mod.rs:4657`). Set `active_tab` to the first tab. The existing single-tab fallback path stays for the empty case.

Order of precedence:
1. `spyc -r` session restore (if user picks a saved session) — wins, brings back the session's exact tab list.
2. New `pane.tabs` config — opens declared tabs.
3. Auto-open a single tab from `pane.default_command` (or `$SPYC_PANE_CMD`, or `"claude"`) — today's behavior. Note: today's auto-open is actually triggered by `TogglePane`, not at startup; need to confirm whether spyc opens a pane unprompted on launch or waits for `F10`. If it waits, the new `pane.tabs` should *also* defer to first toggle, only auto-opening on first user request. Better: introduce a single "open initial tabs" step that runs in either case.

Let me check that startup behavior in a follow-up.

## Things explicitly NOT in scope

- No splits, no tree, no grid.
- No focus-enum refactor — `pane_focused: bool` stays.
- No `compute_layout` change — it stays returning one pane rect.
- No new key bindings — `^W ]`, `^W [`, `^W 1..9`, `^W x`, etc. already handle multi-tab navigation.
- No persistence schema change — `Session.tabs: Vec<SavedTab>` already round-trips multi-tab layouts. The new config knob is just an alternative *seed*.

## File pointers

- Config schema: `src/config/mod.rs:91` (`PaneConfig`), default template at `src/config/default.spycrc.toml:24-25`. The on-disk shape struct nearby (`src/config/mod.rs:99-105`) gains the new field.
- Tab open: `src/app/mod.rs:4646` (`open_pane_tab`) and `:4657` (`open_pane_tab_in`).
- Startup wiring: `src/app/mod.rs:875` (resume branch), and wherever the first-toggle fallback runs (`toggle_pane` at `:4632`).
- Tab cap (9): `Action::PaneTabByIndex(u8)` at `src/keymap/action.rs:114`; tests likely in `src/keymap/resolver.rs` near `:930`.

## Companion threads

- `feature-pane-toggle-preserve-context` — hide-unhide context loss (different problem).
- `feature-pane-state-general-recovery` — what gets persisted/restored for each tab kind (different problem).

This thread's topic slug (`feature-pane-multi-split-config`) is now mildly misleading since we're not doing splits. Leaving the slug as-is to keep the history; future entries clarify the scope.

tags: #pane #tabs #configuration #planning

<!-- Entry-ID: 01KRNB010DF3JVVYEJ834VADP5 -->

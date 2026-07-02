# spyc pane startup tabs (and a side door to multi-split)

**Status:** plan, not yet implemented. Sourced from external-contributor
analysis (Caleb Howard, 2026-05-15).

**Target release:** opportunistic — small enough to land in any future
release. No urgent driver.

## Thesis

Today spyc has exactly one bottom-pane region. Multiple commands can
live there as **tabs** (one active at a time), navigated via `^a c` /
`^W ]` / `^W [` / `^W 1..9`. But startup is single-tab only — driven
by `[pane] default_command` (or `$SPYC_PANE_CMD`, or `"claude"`).

A common ask: declare in `.spycrc.toml` that *on startup* the bottom
pane should open with N tabs already in place — the same way `^a c`
creates them interactively, just driven by config. No splits, no tree,
no grid. Just "open these K tabs for me when I launch."

The session-restore path (`spyc -r`) already round-trips multi-tab
layouts via `Session.tabs`. This is the analogue for "I want this
layout *every time* I open this project."

## Goal

A spycrc knob that opens K tabs in the bottom pane at startup, instead
of just one. Each tab gets a command and optional cwd, mirroring what
`^a c` (`Action::PaneNewTab`) creates interactively
(`src/app/mod.rs:4646-4676`).

## Proposed config surface

Compact array form for the common case:

```toml
[pane]
default_command = "claude"            # existing — single-tab fallback
# tabs = ["claude", "bash", "codex"]  # NEW — opens 3 tabs at startup
```

Table form for cases that need per-tab cwd / label:

```toml
[[pane.tab]]
command = "claude"

[[pane.tab]]
command = "bash"
cwd = "~/scratch"
label = "scratch"   # display name; falls back to command if omitted
```

The two forms coexist (TOML arrays-of-tables and plain arrays are
distinguishable). Validation:

- `tabs.len() ≤ 9` — matches `^W 1..9` jump reach
  (`Action::PaneTabByIndex(u8)`, `src/keymap/action.rs:114`).
- Empty `tabs` *and* no `[[pane.tab]]` → today's behavior (single tab
  from `default_command`).
- If both `tabs` and `[[pane.tab]]` are present: hard error at config
  load (don't try to merge — surfaces the ambiguity).

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

Startup wiring lands near `App::new` (`src/app/mod.rs:875`), after the
session-picker branch. Order of precedence:

1. **`spyc -r` session restore** (if user picks a saved session) —
   wins; brings back the session's exact tab list.
2. **`pane.tabs` config** — opens declared tabs.
3. **Single-tab fallback** — `pane.default_command` (or
   `$SPYC_PANE_CMD`, or `"claude"`). Today's behavior.

One open question to confirm at implementation time: today's
single-tab open is triggered by `TogglePane` (the first `F10` /
`^a-\`), not at startup. If we keep that "wait for user request"
behavior, `pane.tabs` should also defer to first-toggle (only spawn
when the user asks for the pane). Probably cleaner: introduce a single
"open initial tabs" step that runs in either path.

## Things explicitly NOT in scope

- **No splits**, no tree, no grid. (See next section for why.)
- **No focus-enum refactor** — `pane_focused: bool` stays.
- **No `compute_layout` change** — it stays returning one pane rect.
- **No new key bindings** — `^W ]`, `^W [`, `^W 1..9`, `^W x` already
  handle multi-tab navigation.
- **No persistence schema change** — `Session.tabs: Vec<SavedTab>`
  already round-trips multi-tab. The new config knob is just an
  alternative *seed*.

## File pointers

- Config schema: `src/config/mod.rs:91` (`PaneConfig`), default
  template at `src/config/default.spycrc.toml:24-25`. The on-disk
  shape struct nearby (`src/config/mod.rs:99-105`) gains the new
  field.
- Tab open: `src/app/mod.rs:4646` (`open_pane_tab`) and `:4657`
  (`open_pane_tab_in`).
- Startup wiring: `src/app/mod.rs:875` (resume branch), and wherever
  the first-toggle fallback runs (`toggle_pane` at `:4632`).
- Tab cap (9): `Action::PaneTabByIndex(u8)` at
  `src/keymap/action.rs:114`.

## Phases

The whole thing fits in one phase, ~half a day:

1. Extend `PaneConfig` + on-disk shape struct.
2. Extend `default.spycrc.toml` template + `parses_pane_tabs` tests.
3. Wire the startup loop, deferring to existing `open_pane_tab_in`.
4. Document the precedence in `default.spycrc.toml` comments.

---

## The bigger ambition (deferred): true multi-split

Caleb's first pass at this thread proposed full pane *splits* — not
just multi-tab. He walked it back to the scope above after
clarification. Documenting the bigger plan here in case we ever want
to escalate.

### Three options for true splits

**Option 1 — Horizontal-only splits.** Subdivide the bottom region
into N columns; each column has its own `PaneTabs`.

```toml
[pane]
splits = ["claude", "bash"]
split_pcts = [60, 40]
```

- ✅ Smallest blast radius for a real splits implementation —
  `compute_layout` returns `Vec<Rect>` instead of `Option<Rect>`;
  focus expands from `bool` to `enum Focus { List, Pane(usize) }`.
- ⚠️ No row/column hybrids ("claude on top, two shells below").
- ⚠️ Needs new resize bindings (`^W <` / `^W >` horizontal).

**Option 2 — Tree layout (tmux/wezterm parity).** Splits as a
recursive tree. Full flexibility. Compact DSL: `"v(claude, h(bash,
lazygit))"`.

- ✅ Maximally flexible.
- ⚠️ `Session` persistence gains a full tree. Render path becomes
  recursive. Focus needs directional movement (`^W h/j/k/l`),
  conflicting with vi motions. TOML is awkward for trees; the inline
  DSL needs a parser.

**Option 3 — Grid layout (regular rows × cols).** Force `rows × cols`
parameterized; commands by index.

- ✅ Trivial render. Resize is two slider vectors.
- ⚠️ Awkward for "one big + one small."

### Why deferred

The 80% use case the original ask captures is "claude on the left,
bash on the right" — already adequately served by the multi-tab
startup config above (you switch tabs with `^a n` / `^W ]`, both
ptys stay alive in the background). Real splits add significant
implementation cost (focus enum threading, layout refactor, session
schema migration, new resize bindings) for a relatively small bump
in expressiveness over multi-tab.

If a real splits ask shows up (someone needs *simultaneous* visual
state from two ptys — `htop` next to a live log tail, say), Option 1
(horizontal-only) is the natural starting point and slots cleanly on
top of the multi-tab config: degenerate splits with one entry each
collapse to the current single-region behavior.

### Companion concerns

- `toggle_pane` interaction: the hide-don't-kill path
  ([`PANE_RECOVERY_PLAN.md`](PANE_RECOVERY_PLAN.md) Tier 1 and the
  open `feature-pane-toggle-preserve-context` work) generalizes
  cleanly — hide the entire pane row regardless of split count.
- Per-split persistence: `Session.splits: Vec<Vec<SavedTab>>` if we
  go there. Backwards-compat by treating today's `Session.tabs` as
  `splits[0]`.

## Open questions

1. **Does today's "wait for first toggle to spawn the pane" behavior
   need to change?** If `pane.tabs` is set, do we auto-spawn at
   startup, or still wait for the user's first `F10`? Defaulting to
   the latter keeps the spec coherent.
2. **`pane.default_command` semantics when `pane.tabs` is set.** Two
   reasonable choices: (a) fall through to `default_command` for the
   first-tab default when both are configured; (b) ignore
   `default_command` entirely when `tabs` is non-empty. Recommend
   (b): one declarative source-of-truth.
3. **Per-tab env overrides?** Probably no — the use cases we know about
   don't need it. Add later if asked.

## Provenance

Plan sourced from `feature-pane-multi-split-config` thread on
Caleb's `watercooler/threads` branch (entries 2026-05-15T08:01:56Z
and 2026-05-15T08:10:41Z). Re-scope captured verbatim with light
edits for our voice; the "bigger ambition" section is the original
splits proposal, demoted to a deferred-future section. File pointers
verified against `main` at the time of writing.

# Mouse text selection — implementation plan

> **Shipped (v2.1.0) — archived as historical record.** All four drag-select
> clusters landed: forward-the-drag first (#224), then the pager (#227), the
> file list plus the status line (#233), a pane whose child ignores the mouse
> (#234), and the prompt row and activity HUD (#281). Follow-ups fixed what
> hand-driving found — the echo-then-confirm message (#342), copying part of a
> flash (#372), a chrome row's right edge (#375). OSC 52 (open decision
> 5) exists in `src/clipboard.rs`, so the SSH problem Tier 5 raises is
> answered. Click-to-select a list row remains rejected, not deferred.
>
> The plan was not amended as the implementation moved; the pre-2.1 review
> (`docs/archive/review-2.1/B-app-interaction.md`) records where the two
> diverged. The living reference is AGENTS.md → `src/app/mouse/` and
> ARCHITECTURE.md → "Mouse routing".

## Goal

Drag to select text, release to have it on the clipboard — on the agent pane,
the pager, and the file list. The missing half of `[mouse] capture`: capture
takes the terminal's own click-drag selection away, and until spyc gives an
equivalent back, "select and copy" means either remembering a bypass modifier or
reaching for a screenshot.

That is not hypothetical. The trigger for this plan was the owner screenshotting
`:activity` output to send it, because that was easier than copying the text —
with a `y` yank sitting in the pager the whole time. When the discoverable path
loses to a screenshot, the feature is missing regardless of what exists.

## Why this is cheaper than it looks

Three things already exist that would otherwise be most of the work.

**`vt100::Screen::contents_between(start_row, start_col, end_row, end_col)` is
public and purpose-built.** Its own doc: *"This is useful for things like
determining the contents of a clipboard selection."* Two details make it the
right primitive rather than merely a convenient one:

- it walks `grid().visible_rows()`, so it follows the pane's **current scroll
  position** — the same rows the widget drew, no coordinate translation;
- it honours `row.wrapped()`, emitting a newline only on a **hard** line end. A
  hand-rolled cell walk gets soft-wrapped lines wrong (a spurious `\n` mid-line),
  which is the single most annoying bug in a terminal selection.

**The pane widget already applies a per-cell modifier.**
`PaneWidget::render` (`src/pane/widget.rs`) walks `screen.cell(row, col)` and
adds `Modifier::DIM` to every cell when unfocused. A selection highlight is one
more condition inside that existing loop, not a new render path.

**The routing skeleton is done.** `route_mouse` / `MouseSnapshot` / `region_at` /
`Gesture` landed in #212–#221, `mouse_report` encodes any event kind, and
`clipboard::copy` exists. `^a u` quick-select is the established precedent for
"point at text, yank it".

## What must be built

### Tier 0 — ask for drag events (DEC 1002, *not* 1003)

spyc requests `?1000h?1006h` today, and `src/app/proc.rs` drops
`Moved`/`Drag` in the reader. No drag ever reaches the loop.

Adding **1002** (button-event tracking) reports motion **only while a button is
held**. That distinction is the whole reason this is affordable: the redraw storm
the campaign has been avoiding is `?1003h` (any-event motion), which fires on
idle pointer movement. With 1002, no button held means no events, so the
0-dps-at-idle invariant is untouched — and during a drag the per-motion redraw is
exactly what we want, since the highlight has to track the pointer.

`EnableWheelReporting` becomes `?1000h?1002h?1006h`. Note
`DisableWheelReporting` **already** clears 1002/1003 (#218), so teardown needs no
change — and `mouse_mode_seq`'s existing test asserting no `1003h` stays valid
and still meaningful.

The reader filter must stop dropping `Drag`. It should keep dropping `Moved`
(no button held — with 1002 that shouldn't arrive at all, so a `Moved` means the
terminal ignored what we asked for, which is precisely the case the filter was
written for).

### Tier 1 — press is ambiguous until it moves

At press time a click and a drag are indistinguishable. So:

```
press          → record anchor, mark pending. Do NOT start a selection.
first motion   → promote to a drag; selection begins at the anchor
release, moved → finish the selection, copy
release, still → it was a click; existing click behaviour applies
```

The load-bearing constraint: **left-click is already click-through** (#219 —
focus the pane *and* forward to a mouse-aware child). A press over such a child
has already been delivered. spyc cannot retroactively claim the drag, which
forces Tier 2's split rather than making it a preference.

### Tier 2 — who owns a drag

| Pointer over | Owner | Why |
|---|---|---|
| a **mouse-aware** child (claude, vim, htop) | **the child** | Its own selection is better than ours and it already got the press. claude's `onSelectionDrag` / `onHoverAt` exist and are currently dead only because we drop motion — forwarding brings them alive for free. |
| a **non-mouse** child (plain shell) | **spyc** | Nothing else can. This is the case the deferred "spyc-owned pane scrollback" left as a no-op. |
| the **pager** | **spyc** | No child involved. |
| the **file list** | **spyc** | No child involved. |

So drag ownership follows the same `can_forward_to_child` predicate the wheel and
buttons already use. One rule, four surfaces.

### Tier 3 — the selection model

A `Selection { surface, anchor: (row, col), focus: (row, col), mode }` in
`ViewState` (render ephemeral, cleared freely, never persisted). Coordinates are
**surface-relative**, so the pane's are pane-relative — the same translation
`mouse_report` already does.

`mode` is linewise-by-default with a modifier for block/rectangular. **Not
Shift** — Shift is the terminal's own selection-bypass modifier and is frequently
consumed before spyc sees it (the note already in `native_scroll_plan.md`). Alt
is the conventional block-select modifier and is available.

### Tier 4 — rendering the highlight

- **Pane**: one condition in `PaneWidget`'s existing cell loop.
- **Pager**: the pager renders `Vec<Line>`; a highlight means restyling spans
  within the selected range. More work than the pane because a `Line` is spans,
  not cells, and the selection cuts across span boundaries.
- **List**: rows are already styled per-row; a selection here is arguably just
  the existing pick mechanism and may not be worth a second concept.

Use a theme colour, not a hardcoded `REVERSED` — `REVERSED` collides with the
block cursor the widget already draws, and with a diff's own background wash.

### Tier 5 — copy, and the SSH problem this exposes

On release, extract and copy. Pane text comes from `contents_between`; pager text
from the existing `source_yank_text` / `visible_yank_text` machinery, range-scoped.

Trailing whitespace must be trimmed per line: a terminal grid is space-padded to
its full width, so an untrimmed selection pastes a rectangle of spaces.

> [!IMPORTANT]
> **`src/clipboard.rs` copies to the WRONG MACHINE over SSH, today.** It shells
> out to `pbcopy` / `xclip` / `wl-copy`, which are the *server's* clipboard. Yank
> something over SSH and it lands on the remote host, not the laptop the user is
> typing on.
>
> This is not new and not caused by selection — it already affects `y` in the
> pager and `^a u` quick-select. But selection makes it the primary path, so it
> stops being ignorable.
>
> The fix is **OSC 52**, which asks the *terminal* to set the clipboard, so it
> lands client-side. spyc already has the precedent for exactly this shape:
> `[notify] desktop_via = "auto"` routes OSC-9 to the client terminal over SSH
> versus the OS notifier locally (`desktop_delivery(via, is_ssh)`, `view.is_ssh`).
> Clipboard writes want the same `auto` treatment.
>
> Caveat to verify, not assume: OSC 52 write support varies (kitty, WezTerm,
> iTerm2, Ghostty, Alacritty yes; tmux needs `set -g set-clipboard on`; some
> terminals gate it behind a setting for good security reasons). So it's
> `auto` with a fallback, not a replacement. `term_title.rs` already sanitizes
> OSC payloads and has a test using a hostile `\x1b]52;c;…` string — the escaping
> hazard is understood in-tree.

## Owner spec (2026-08-05) — three surfaces, and "the content, not the chrome"

Scope is settled, and it is wider than the earlier "pane + pager, skip the list"
recommendation. All three spyc-owned surfaces are in:

1. **Pager** — must work identically whether it's full-screen (`Mount::Overlay`)
   or the pop-up (`Mount::TopPane` / `LowerPane`), and regardless of whether
   line numbers, whitespace/line-break markers, or markdown rendering are on.
   The copy is **the content and nothing else** — no gutter digits, no `·`/`→`/`$`
   markers, no border or title.
2. **File list** — select file names. A modifier gives the **full path**; without
   it, just the **name**.
3. **Status bar** — select and copy the whole top/status line text.

### Resolved: how "the content and nothing else" is satisfied

Two facts in the existing pager make this nearly free, and they're the reason this
does NOT need a screen-buffer scrape:

- **The decoration is render-time only.** `show_line_numbers` builds the gutter in
  `render.rs` (`gutter_w`, ~line 149) and `expand_tabs` / `apply_whitespace_markers`
  add the markers there too (~186–188). None of it is in `view.lines`. So
  extraction that reads `view.lines` is decoration-free *by construction* — there
  is no filtering step to get wrong, and turning line numbers on cannot change
  what a copy produces.
- **The markdown source is retained.** `alt_lines` holds the other side of the
  rendered↔source pair and `m` toggles which is live (`markdown_rendered`).
  `source_text()` already prefers `alt_lines` for exactly this reason ("POLA for
  paste into chat").

So: **a selection always yields the text currently displayed, taken from
`view.lines`.** In a markdown view, `m` is what chooses rendered prose vs raw
markdown, and the selection follows it. That satisfies "the underlying actual
markdown *or* the text being displayed" without inventing a rendered→source line
map, which is impossible to do faithfully anyway — the two sides have different
line counts, so a *ranged* selection has no well-defined image in the other. Whole-view
source copy stays `y` (`source_yank_text`), which is unchanged.

> [!IMPORTANT]
> Do not implement pager selection by reading the rendered `Buffer`. It is the
> obvious shortcut and it silently re-introduces every decoration the spec
> excludes, plus wrap artifacts — and it would make the copy depend on whether
> the gutter happened to be on.

### Charwise selection has to be added

`VisualKind` today is `Line | Block` only. A mouse drag is neither: it starts
mid-line and ends mid-line, taking everything between (vim's `v`, and what every
terminal does). That is a third kind, `Char`.

Adding it is contained — `Line`/`Block` already have arms to extend in the three
places that matter: `VisualSelection::range`, the highlight arms in
`render.rs` (~350–410), and `visual_yank_text`. It also lands keyboard `v` for
free, which the pager currently lacks.

### Wrap is the trap

With `wrap = true` (the scrollback default) one source line occupies N visual
rows, so a pointer row is not a line index. `layout::visual_rows` already owns
that math for scrolling and must be the single source of truth for the
pointer→(line, col) hit-test too. A second, independent mapping here is how the
highlight and the copied text come to disagree about what was selected.

### Per-surface extraction

| Surface | Selection unit | Copy yields |
|---|---|---|
| Pager | charwise (`VisualKind::Char`), from `view.lines` | displayed text, decoration-free; trailing per-line whitespace trimmed |
| File list | rows | file **names**, or **absolute paths** with the modifier held |
| Status bar | whole line | the status text as displayed |

For the file-list modifier: **not Shift** (the terminal's own
selection-bypass, frequently consumed before spyc sees it) — that constraint
already applies to block-select below. Use Ctrl or Alt, and state it in
`FEATURES.md` + `docs/KEYBINDINGS.md`.

The list is *not* the pick mechanism. Picks persist and drive operations; a
selection is transient and only feeds the clipboard. Keeping them separate avoids
a drag silently changing what the next file operation acts on.

## Open decisions

These need answers before implementation, and two of them change the shape.

### 1. Selection stability under live output — the real design question

A selection anchored to visible-grid coordinates is **wrong the instant the child
emits output**, because the grid scrolls under it. Three options:

| | Behaviour | Cost |
|---|---|---|
| **(a) clear on output** | Any pane output drops the selection | Trivial. But an agent emitting output while you drag makes selection unusable in exactly the pane that matters most. |
| **(b) freeze on drag** | Drag start enters the pane's scroll mode; the viewport stops following the tail | What tmux and WezTerm do. Stable by construction. spyc already has pane scroll mode and `^a v`. |
| **(c) absolute anchoring** | Anchor to scrollback-absolute rows, translate per frame | Correct under scrolling AND live output, but needs a coordinate space `contents_between` doesn't speak. |

**Recommendation: (b).** It matches what users already expect from tmux, it makes
the whole class of bug impossible rather than handled, and the machinery exists.
Cost: a drag implicitly freezes the pane, which must be visible (the divider
already indicates scroll mode) and must auto-exit on release-with-no-selection.

### 2. ~~Which surfaces in v1~~ RESOLVED

Superseded by the owner spec above: pager + file list + status bar, all three.
The pane is covered for a mouse-aware child by #224 (its own selection) and needs
spyc-side selection only for a non-mouse child, which is now the *last* piece
rather than the first.

### 3. ~~Auto-copy on release, or an explicit copy step?~~ RESOLVED

**Auto-copy on release, with a config toggle to disable it.** Owner decision:
auto-copy is the preferred behaviour — it matches the emulator behaviour being
replaced and the X11 primary-selection convention — but someone who dislikes
having their clipboard overwritten by a stray drag must be able to turn it off.

```toml
[mouse]
# Copy the selection to the clipboard as soon as the drag ends. Off means the
# selection is made and highlighted, and an explicit yank key copies it.
selection_auto_copy = true
```

Default `true`. With it `false` the drag still selects and highlights; copying
becomes an explicit key, which the pager's existing `y` already models.

The flash names the copied line/byte count either way, so an auto-copy is never
silent — that's what keeps the default honest rather than surprising.

### 4. Does this replace Shift-drag?

No — the terminal's bypass keeps working regardless, and it stays the documented
escape hatch. Worth stating explicitly so the docs don't imply otherwise.

### 5. Is OSC 52 in this plan or its own?

It's a **pre-existing bug** with a wider blast radius than selection (`y`, `^a u`).
Arguably it should land first, independently, so selection inherits a correct
clipboard rather than shipping alongside a fix for something it didn't break.

**Recommendation: separate PR, landed before Tier 5.**

## PR split

1. ~~**1002 + drag plumbing**~~ — **SHIPPED (#224).** Brought claude's own
   in-pane selection alive, which is what "copy in a claude session works great"
   refers to.
2. **`VisualKind::Char`** — charwise selection in the pager, keyboard-first
   (`v`), with the `range` / highlight / yank arms. No mouse yet, so it is
   reviewable against the existing `V`/`^v` behaviour it extends.
3. **Pager mouse selection** — the press→motion→release state machine (Tier 1),
   the `visual_rows`-based pointer→(line, col) hit-test, and copy-on-release.
   Delivers the `:activity` / `:grep` / diff case that motivated all of this.
4. **File list + status bar** — row selection with the name/full-path modifier,
   and whole-line status copy. Smaller, and independent of 2–3.
5. **OSC 52 clipboard routing** — `auto`: client-side over SSH, OS clipboard
   locally. Fixes a live bug for every SSH user on the *existing* yank paths
   (`y`, `^a u`), so it is independently valuable. Should land before selection
   makes copy the primary path, but does not block 2–4 locally.
6. **Pane selection for a non-mouse child** — freeze-on-drag (decision 1) and
   `contents_between` extraction. Last, because #224 already covers the
   mouse-aware children and this only serves plain shells.

## Verification

- **Automated**: `mouse_mode_seq` asserts `1002h` present and `1003h` absent
  (the storm guard must survive the change); `contents_between` range extraction
  round-trips a known grid including a soft-wrapped line and a CJK cell; trailing
  whitespace is trimmed; the press→motion→release state machine is a pure
  decision with a table test, including release-without-motion still being a click.
- **Manual**: idle with capture on and the pointer moving with **no button held**
  → the `A` overlay must show **0 dps** (this is the whole 1002-vs-1003 bet, and
  it is invisible in any test); drag inside claude selects in claude; drag in a
  plain `zsh` pane selects in spyc; drag across a soft-wrapped line pastes one
  logical line with no interior newline; over SSH, a yank lands on the **laptop**
  clipboard; `Shift`-drag still hands selection to the terminal.

# Clickable menus — discoverability that survives the terminal

**Status:** proposal. Not yet scoped into a release.
**Measured against:** `49bb40d` (`main`, `2.2.0-CURRENT`).
**Origin:** a herdr demo, where the reaction worth chasing was *the user loved
not having to learn all of the keyboard shortcuts*.

## Thesis

spyc already draws the menu. It already opens it on a delay when you hesitate.
It already labels every row with the key that fires it. The one thing it does
not do is let you click a row.

So this is not "spyc grows context menus". It is three changes: fix a
click-through bug, make the popup that is **already on screen** clickable, and
add one computed menu for the row under the cursor.

The first instinct — a right-click context menu — is the wrong foundation, for
a reason that is measurable rather than aesthetic. That finding is §1.

---

## 1. The finding that shapes the design

**Right-click is not portable.** Measured on two terminals, same spyc build:

| Terminal | Right-click over spyc | What spyc sees |
|---|---|---|
| iTerm2 | iTerm2's own context menu opens (New Window / Split Pane / Copy / Paste / Edit Session / …) | **nothing** |
| ghostty | passes through | `Gesture::Right` → `MouseSink::LeaderMenu`, leader popup opens |

The consequence is sharper than "one terminal differs". `src/app/mouse/route.rs:470`
has mapped `Gesture::Right` to `MouseSink::LeaderMenu` since the #226–#234 mouse
campaign, and on iTerm2 **it has never once fired**. A shipped binding that was
silently dead in a major terminal, unnoticed for a full release cycle, is the
proof that right-click cannot carry a feature whose entire purpose is
discoverability. Telling the user to reconfigure Preferences → Pointer is not an
answer for the person who did not want to learn anything in the first place.

A second reason, independent and weaker but pointing the same way: right-click
is itself a learned guess. Someone who does not already expect terminal
applications to have context menus will never try it. A hidden gesture cannot be
the cold-discovery path by construction — which is the exact goal being chased.

**Left-click is the portable gesture.** No terminal steals it under mouse
reporting, and spyc already depends on it for row selection, tab activation and
pane forwarding. `[mouse] capture` is default-on (`src/config/mod.rs:359`), so
this reaches every user without configuration.

**What this kills, deliberately:** a `KeyChord::Mouse` resolver variant, a
`Ctrl`/`Alt` modifier convention for menus, and `^a`-then-right-click. All three
were live proposals before the measurement; none survives it.

---

## 2. Two live bugs this closes

Both are ordinary defects, reachable today, and both are fixed by the single
rule PR 1 installs. They are worth stating separately because they justify PR 1
on their own, independent of the feature.

### B1 — the popup is clicked through

`MouseSnapshot` (`src/app/mouse/route.rs:143`) carries no chord-hint field. So a
left-click on the visible popup routes by `region_at` to whatever the layout
says is underneath it: list rows (starting a drag-select), a chrome row, or the
pane — where the press is **forwarded into the agent**.

This is the same defect class as #228, which `covering_pager` fixed for the
modal pager. The comment at `route.rs:422` states the principle already learned:
*"Clicking beside its box used to reach whatever the layout said was underneath
— selecting content in the pane 'under' a pager the user is reading."* The
chord-hint popup is painted the same way and honours none of it.

### B2 — a click while a chord is armed latches it

`clear_chord_hint` (`src/app/key_dispatch/mod.rs:435`) has callers only inside
`key_dispatch`. Nothing in `src/app/mouse/` clears it.

Repro: arm `g`, wait out `chord_hint_delay_ms`, click a list row. The cursor
moves, the popup stays on screen, `resolver.pending` is still `PendingSeq::G`,
and the **next key** is consumed as a `g` continuation.

This is the same latch `resolver_will_see_the_next_key` (`route.rs:234`) exists
to prevent for prompts, and that guard's own doc names why it matters: in the
leader menu `p` is a chdir and `P` overwrites `PROJECT_HOME`.

---

## 3. What already exists

| Piece | Where | State |
|---|---|---|
| The popup widget | `render_chord_hint`, `src/app/render/overlays.rs:608` | Column-flowing, wrapping, divider-dodging. Complete. |
| Open-on-hesitate | `settle_chord_hint`, `src/app/loop_steps.rs:208` | The cold-discovery path, already shipping. |
| Open-now-on-gesture | `MouseSink::LeaderMenu`, `src/app/mouse/mod.rs:390` | Works; unreachable on iTerm2 (§1). |
| The vocabulary | `Action::describe()` / `tier()`, `src/keymap/action.rs:306` / `:247` | Every action already has a label and a guard-enforced tier. |
| Menu contents | `Resolver::continuations()`, `src/keymap/resolver/mod.rs:152` | `Vec<ChordEntry>`, static per `PendingSeq`. |
| Overlay-owns-the-pointer | `covering_pager`, `route.rs:422` | The precedent PR 1 follows. |
| Geometry-is-one-source | `tab_widths`, `src/app/mouse/tab_hit.rs` | The precedent PR 2 must follow. |

---

## 4. PR 1 — the popup owns the pointer

**Shape:** `fix(mouse): the chord-hint popup owns the pointer while it is open`

Add one field to `MouseSnapshot`, mirroring `covering_pager`:

```rust
/// Pointer is inside the open which-key popup's box.
pub over_chord_hint: bool,
```

Routing, checked **before** `region_at` for the same reason `covering_pager` is:

- `Left` / `Right` / `Wheel` inside the box → `MouseSink::Swallow` (PR 2 replaces
  the `Left` arm).
- `Wheel` is included deliberately: scrolling the list behind a popup you are
  reading is B1 in a different costume.
- Any click **outside** the box, while a chord is armed → cancel the chord
  (clear the resolver's pending sequence and the hint), and do not otherwise act.

That last rule is the fix for B2 and it needs a decision recorded: a click
outside an open menu **dismisses** rather than acts. This is what every menu the
user has ever used does, and acting-and-dismissing on one click would make a
stray click during an armed chord do two things at once.

**Tests:** pure `route_mouse` cases for each gesture inside the box; a case
proving a click outside clears `pending`; a regression naming B2's repro.

---

## 5. PR 2 — its rows are clickable

**Shape:** `feat(mouse): click a which-key row to fire it`

Two things block it today.

### (a) `ChordHint` throws the action away

`settle_chord_hint` (`loop_steps.rs:220`) flattens `ChordEntry` into
`(&'static str, &'static str)` — `Act(keys, action)` becomes
`(keys, action.describe())`, and the `Action` is gone. `ChordHint.rows` must
keep the entry.

`Sub` is the harder half: `ChordEntry::Sub(keys, label)`
(`resolver/mod.rs:71`) does not carry the sub-sequence it opens. **Feeding
`keys` back to the resolver does not work** — `keys` is a *display* string that
may list alternatives or a range; the `^a` menu alone contains `"a h"`, `"n ]"`,
`"1–9"` and `"\ C"`. So `Sub` gains its target:

```rust
Sub(&'static str, &'static str, PendingSeq),
```

### (b) The geometry is computed inside the renderer

`render_chord_hint` derives `key_w`, `label_w`, `col_w`, `body_h`, `n_cols`, the
column packing, `rows_tall`, the box `width`/`height`, the centring and
`place_clear_of_line` — all inline, all unreachable from a hit-test.

Re-deriving that for the hit-test is the `tab_widths` trap verbatim, and
`tab_hit.rs` already records what it costs: *"any drift makes a click land on
the neighbouring tab, and only for the tabs after the one that drifted — which
reads as 'clicks are off by one sometimes' rather than as a width bug."* With a
wrapping, column-flowing menu the drift would be worse and less legible.

So: extract a pure `chord_hint_layout(...) -> ChordHintLayout` holding the box
`Rect` plus one `Rect` per entry, and have **both** the renderer and the
hit-test consume it. This is also what supplies PR 1's `over_chord_hint`.

### Dispatch

- `Act(_, action)` → dispatch that `Action` through the normal path, then clear
  the resolver and the hint.
- `Sub(_, _, seq)` → set `resolver.pending = seq` and rebuild the hint, so the
  popup walks into the submenu exactly as the keystroke would.
- **Numeric prefix:** a count armed before the chord (`3j`) is consumed by
  `take_count()` on the key path. The click path must take it the same way, or
  `3` then a click silently drops the count.

**The row keeps its key visible.** `d  delete to graveyard`. The menu teaches
the keymap rather than replacing it — which is the answer to "are we becoming a
mouse application". It is which-key extended to the pointer.

**Tests:** a hit-test case per column in a multi-column menu (the packing is
where drift hides); a case proving the renderer and the hit-test consume one
layout — pin the call site, per `every_declared_size_allocation_is_capped`'s
shape; a count-preservation case.

---

## 6. PR 3 — one contextual menu

**Shape:** `feat(keymap): a context menu for the row under the cursor`

The first **computed** menu: `continuations()` is static per `PendingSeq`, and
this one is a function of what the cursor is on.

```rust
fn menu_for(ctx: MenuContext) -> Vec<ChordEntry>
```

built in the `route.rs` / `focus.rs` template — a `Copy` snapshot, a pure
function, unit tests. `MenuContext` carries the entry kind (directory, file,
symlink, archive container, archive member, worktree root), whether picks are
non-empty, and the git state of the row.

**Applicability must be real.** spyc already knows what it will refuse:
`src/archive/scan.rs:76`'s `Capability`, `merged` on `list_worktrees`, empty picks.
The menu must source its entries from those same predicates. Offering "extract"
on a plain file, or "write archive" on a mount with nothing journalled, is worse
than no menu at all — it converts a discoverability feature into a source of
flashed errors.

**No second registry.** The menu projects from `Action`; a hand-written tree
rots within a release. Guard: every `Tier::Frame` action appears in at least one
context menu or in an explicit exempt list, checked at build time the way
`leader_and_pane_namespaces_respect_tiers` checks tiers today.

**Tier and binding.** `Action::ContextMenu` is `Tier::Frame` — it acts on the
commander, and tagging it `Meta` purely to buy a leader slot would be a dodge of
a taxonomy that is guard-enforced precisely so it stays truthful. That means a
**Frame** binding (letter / `g` / `[`/`]`), not `Space`.

> **Open decision for the owner — the default key.** The Frame namespace is
> dense and a `:`-only command is useless to the audience this feature exists
> for. This is the one item in this document that is a preference rather than a
> derivation, so it is not pre-empted here.

**Right-click stays wired as an accelerator** where the terminal delivers it, so
ghostty / kitty / wezterm users get the herdr feel. Nothing depends on it, and
on iTerm2 its absence costs nothing because the key works.

---

## 7. Non-goals

- **A menu bar.** A permanent row costs vertical space a terminal does not have.
  Agreed with the owner up front.
- **Hover highlight.** It needs `?1003h`. `src/app/proc.rs:104` drops button-free
  `Moved` deliberately — *"belt to 1002's braces"* against the redraw storm that
  the native-mouse campaign was built to avoid. Reopening that is a
  separate decision with its own evidence bar, not a detail of this feature.
- **`KeyChord::Mouse`, mouse modifiers, `^a`-then-right-click.** Killed by §1.
  The abandoned `feat/mouse-bindings` branch heads this way and should stay
  abandoned; the audit's note on it already said *redo rather than resume*.
- **Press-drag-release menu tracking.** Works under 1002 and is the original
  Mac/X11 gesture, but it is a second interaction model for the same widget.
  Revisit only if click-to-open proves awkward in real use.

---

## 8. Risks

- **PR 2 is where a regression would hide.** The popup is currently inert, so
  every click it starts accepting is new surface. The mitigation is the shared
  layout: one geometry, two consumers, pinned by a test.
- **PR 3 can flash errors into a feature meant to reassure.** Applicability
  (§6) is not polish; it is the difference between a menu and a trap.
- **`ChordEntry::Sub` gaining a field touches every static menu.** Mechanical,
  but it is a wide diff and belongs in PR 2 rather than smuggled into PR 1.
- **Right-click's current behaviour changes for ghostty users.** Today it opens
  the leader from anywhere; after PR 3 it should open the *contextual* menu over
  a row. That is the intent, but it is a behaviour change for the one terminal
  cohort that has the feature today, and the CHANGELOG line owes them the note.

---

## 9. Exit criteria

1. Clicking the visible which-key popup never reaches what is behind it, on any
   surface (list, chrome, pane). B1 closed.
2. A click while a chord is armed cannot leave `resolver.pending` set. B2
   closed, with a regression test naming the repro.
3. `Space`, `^a`, `g`, `H` — each opens its popup, and every row in each fires
   on a left-click, including rows in the second and third columns of a
   multi-column menu.
4. Hesitating on any prefix and then clicking a row completes a full task
   without the user having typed a key other than the prefix.
5. The contextual menu offers nothing spyc would refuse, verified against a
   mounted archive and a claimed worktree.
6. All of the above on iTerm2, where right-click never arrives.
7. `make check` green; docs updated in the same commit — `docs/KEYBINDINGS.md`,
   `src/ui/help.rs`, `FEATURES.md`, `DESIGN.md` (a UI-language change), and
   `AGENTS.md` if a module is added.

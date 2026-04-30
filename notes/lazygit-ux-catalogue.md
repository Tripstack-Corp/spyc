# lazygit UX patterns — borrow / adapt / skip

Branch: `worktree-investigate+lazygit-support`. Inputs:
- lazygit upstream at `lazygit-upstream/` (commit `69635f3`).
- spyc surface vocabulary in `DESIGN.md`. Recommendations below use
  only those names: status bar, prompt, list, divider, pane, pager,
  top overlay, inventory view, session picker, flash, confirm.
- Action vocabulary in `src/keymap/action.rs`.

A note on tension before the catalogue: lazygit is **mouse-first
with keyboard parity** (clicks on panel headers, diff lines, footer
labels; `MouseEvents: true` by default). spyc's DESIGN.md is explicit
that "Keys are the API; mouse is a courtesy." That difference shows
up most strongly in surfaces 4 (popups) and 6 (row verbs) below: the
*affordance* lazygit shows on screen often only earns its keep
because clicking it is a primary input. Where I recommend adapting,
I'm recommending the keyboard half, not the click target.

---

## 1. Numbered panels & direct-jump

**lazygit:** Side windows are rendered with a `[N]` prefix in the
panel title (`[1]-Status`, `[2]-Files`, …, `[5]-Stash`).
`gui.Views.Status.TitlePrefix = jumpLabels[0]` etc. at
`pkg/gui/views.go:212-238`. Pressing `1`..`5` calls
`JumpToSideWindowController.goToSideWindow` at
`pkg/gui/controllers/jump_to_side_window_controller.go:50`. If the
target window is already focused and `SwitchTabsWithPanelJumpKeys` is
set, the same key cycles its inner tabs. Configurable via
`Keybinding.Universal.JumpToBlock` (`pkg/config/user_config.go:925`).

**spyc today:** No "jump to surface N" idiom on the keyboard. The
list and pane are switched via `^W k` / `^W j` (Action::PaneFocusUp /
PaneFocusDown), and pane *tabs* are switched via `^W 1..9`
(Action::PaneTabByIndex) — but that's pane-tab-scoped, not
window-scoped, and requires the `^W` chord.
The divider already shows `[1*] claude ─[2+] bash` for pane tabs
(DESIGN.md "Divider"), so the *visual idiom* is in place; only the
direct-number-jump is missing for spyc-major surfaces.

**Recommendation:** **skip as a global numeric jump**. spyc has
exactly two top-level surfaces (list, pane) where lazygit has five,
so `1` and `2` would be wasted on a binding that `^W j`/`^W k`
already covers cleanly. The `[N+]`/`[N●]` task-divider glyphs
(DESIGN.md) already use single digits in titles for *task* numbers;
hijacking `1`..`9` globally would collide. Better keyboard parity
already exists.

**Effort sketch:** N/A.

---

## 2. Context-sensitive footer ("options map")

**lazygit:** A bottom row reads `Stage: <space> | Commit: c | Edit: e
| Stash: s | Discard: d | Reset: D | Keybindings: ? | Cancel: <esc>`
and changes whenever focus moves to a different panel. Built by
`OptionsMapMgr.renderContextOptionsMap` at
`pkg/gui/options_map.go:37-105`: take current-context bindings, add
non-overlapping global bindings, filter to those tagged
`DisplayOnScreen && !IsDisabled`, prepend mode-specific extras
(cherry-picking, bisect, rebase, patch building). Format with ` | `
separator and ellipses on overflow (`options_map.go:108-137`).

**spyc today:** No equivalent. The status bar
(`src/ui/status.rs:39-`) renders fixed powerline segments; the help
overlay (`src/ui/help.rs`) is a full-screen pager dump. The user has
to either remember the binding or open `?` and read the table. Pages
of pager-mode keys (`?`, `n`, `N`, `s`, `:N`) and prompt-mode keys
are particularly easy to forget in-the-moment.

**Recommendation:** **adapt — into the prompt row, not the status
bar.** DESIGN.md is explicit: "Don't introduce a third status row.
If you need more, the answer is the `I` info overlay or a flash."
The flash row IS already a transient one-row affordance at the
prompt, so a "context cheat-sheet" line that paints when no other
prompt is active is a natural extension — not a new surface. Scope
it tight: only show it inside a non-default mode (pager open, find
finder open, picker active) where the keys ARE different from list
mode. In list mode where the user already knows the keys, leave the
prompt row empty as today.

**Effort sketch:** No new `Action`. Add a `context_hints()` accessor
to each overlay (pager, finder, history editor) returning a
`Vec<(key, label)>`; have `app::draw_prompt_row` paint that
sequence in `Style::DIM` (mono-friendly via DIM modifier per
DESIGN.md "Mono mode") when the prompt is otherwise idle. Truncate
with `…` on narrow terminals exactly as lazygit does.

---

## 3. Command log + "Random tip"

**lazygit:** The bottom-right "Extras" panel (`Views.Extras`) is a
log of every git command the app ran, plus the action label that
triggered it. `LogAction` writes the yellow header
(`command_log_panel.go:25-34`); `LogCommand` writes the indented
command (`:36-52`). On startup, `printCommandLogHeader` once prints
"Random tip: …" picked from a hand-curated list keyed off current
keybindings (`:54-188`). It is **not** rotating mid-session — only
re-rolled per process start.

**spyc today:** Two adjacent ideas, neither identical.
(a) `!?` history editor — vi-editable popup of past `!`-captured
commands (CLAUDE.md "What it does"). User-typed, not app-run.
(b) Backgrounded-task viewer — peek at a `^Z`'d task's output
buffer, with `[N+]`/`[N●]`/`[N✓]`/`[N✗]` glyphs in the divider
(DESIGN.md "Icons & glyphs"). Closer in spirit, but per-task, not a
unified log.

**Recommendation:** **skip the command log.** Spyc doesn't *run* git
or other commands behind the user's back the way lazygit does;
almost every shell-out is user-initiated through `!`/`;`/`:!` and is
already preserved in the history surface. The lazygit command log
exists because *the app generated the side effect*; spyc never owns
that role. Adding a synthetic "log of stuff spyc just did" would
duplicate flash messages and `!?` history without earning new
information.

The "Random tip" is a different question. **Adapt as a one-shot
flash on first launch of the session, not a panel.** spyc already
rolls a fresh spice-pair `SESSION_NAME` per session; emitting one
DIM tip via `flash_info` on the first idle frame after launch (only
if `Gui.ShowRandomTip`-style config is on, off by default) fits
the "transient one-row message" idiom in DESIGN.md without standing
up a new surface. This is the kind of polish that earns its keep
only after the first three suspects from the gap-analysis are fixed.

**Effort sketch:** No new Action. A `Vec<&'static str>` of tips
keyed off current bindings (e.g. via `Action::describe`), one
`flash_info` call from `App::startup`, gated on
`config.tips.show_on_launch` (default false).

---

## 4. Popups / pickers (Menu, Confirm, Alert, Prompt, Toast)

**lazygit:** Five popup affordances, all in
`pkg/gui/popup/popup_handler.go`: `Menu` (`:58`), `Confirm` (`:109`),
`Alert` (`:105` — a Confirm with no actions), `Prompt` (`:132` —
typed input), `Toast` (`:62` — non-blocking flash). Menu is the most
distinctive: a filterable list with sections (Local / Global /
Navigation), columns, disabled-reason hints, see
`controllers/options_menu_action.go:13-58`.

**spyc today:** Mapping straight onto DESIGN.md:
- `Confirm` → spyc `confirm` (typed-letter inline at prompt). ✓
- `Alert` → spyc `flash` (one-line) or `pager` (longer). ✓
- `Prompt` → spyc `prompt` (prefix character). ✓
- `Toast` → spyc `flash`. ✓
- `Menu` → **no equivalent.** Closest seed: the pager already has a
  `picker_cursor` field (`src/ui/pager.rs:84-90`) used by the find
  finder, task viewer, and history editor. So the *machinery* exists
  inside the pager surface — it just isn't generalized for "pick one
  Action from this list" use cases.

**Recommendation:** **adapt — extend the pager into a generalized
pick-from-list mode.** Per DESIGN.md "One shape per job": a new
modal type would violate "Pager is the primary; new overlay-shaped
features should generally render *into* the pager." A
`PagerView::picker_items: Vec<(Label, Action)>` with Enter-to-fire
gives spyc lazygit's Menu without adding a fifth overlay. This pays
for itself anywhere the user currently has to remember a `:cmd`:
`:project` listing, `W l` worktree pick, even an "open file in…"
chooser.

The other four popup types do **not** need adapting; spyc already
has each one under a different name, and the names are already
intuitive in DESIGN.md vocabulary.

**Effort sketch:** Extend `PagerView` with an action-bound picker
variant, add a `Action::OpenMenu(MenuKind)` variant (or pass items
directly), wire Enter to dispatch the bound action. Existing pager
search (`/`), `:N` jump, and `q` come along for free. Migrate `W l`
and the `gB` task viewer to this generalized form as proof.

---

## 5. Sub-menu drill-down — scoped help

**lazygit:** `?` opens a Menu (not a static page) split into three
sections: **Local** (current panel), **Global**, **Navigation**.
Built by `OptionsMenuAction.Call` and `getBindings` at
`controllers/options_menu_action.go:13-79`. The user can type-filter
within the menu (`AllowFilteringKeybindings: true`).

**spyc today:** `?` opens a single-overlay help dump
(`src/ui/help.rs`, 439 lines of static keybinding tables).
Comprehensive but undifferentiated — context-sensitive keys (pager
vs list vs prompt) all live in the same scroll buffer. The keybinding
list reads in mono (DESIGN.md compliance) but the user has to scan.

**Recommendation:** **adapt — scope `?` to current overlay first,
then `?` again for global.** This is the natural reuse of the same
generalized picker recommended in §4: open the help with a section
header for the active surface (Pager / Pane / List / Prompt) at the
top, the rest below; pager-search (`/`) inside it gives free
filterable behavior matching lazygit.

**Effort sketch:** No new Action. Have `Action::Help` consult the
current overlay/focus and feed the help renderer a "primary
section" hint; existing help data stays where it is. If the §4
generalized picker lands, this becomes ~50 lines.

---

## 6. Single-key action vocabulary on rows

**lazygit:** `<space>`, `c`, `d`, `D`, `e`, `s` apply to the focused
row in the focused panel. The same keys do *different* things in
different panels (e.g. `e` on a file = stage edit; `e` on a commit =
interactive rebase here). The footer (§2) is what makes this
discoverable.

**spyc today:** Spyc *already does row-level verbs* — they're just
named in DESIGN.md as "operations resolve against the cursor row or
the picked set." Examples from `keymap/action.rs`:
`Action::TogglePick` (`t`), `Action::Take` (`yy`),
`Action::RemovePrompt` (`R`), `Action::CopyPrompt` (`c`),
`Action::EnterOrEdit` (`e`), `Action::FileType` (`f`). The verbs
are *globally consistent*: `c` always means "copy", not "copy here
but commit there".

**Recommendation:** **skip — and keep skipping.** lazygit's
panel-scoped key reuse is a direct consequence of having five
panels; spyc has effectively one (the list), so the same letter
shouldn't be taught two meanings. Globally-consistent verbs are a
spyc strength, and DESIGN.md's `g <x>` / `^a <x>` / `^W <x>` chord
families exist *specifically* to avoid panel-scoped overload. The
contextual *hint* (§2) gives spyc the discoverability win without
the cognitive cost.

**Effort sketch:** N/A.

---

## 7. Two-letter chord jumps

**lazygit:** `co` (checkout), `cf` (copy filename), `cl` (copy
commit hash to clipboard), `gp` (pull), `gP` (push). Two-letter
mnemonics, no prefix family.

**spyc today:** `g <x>` is the read-only / go-to family and is
densely populated (`gh`, `gP`, `gS`, `gU`, `gV`, `gd`, `gD`, `gb`,
`gf`, `gF`, `gp`, `gB`); `^W <x>` is the pane family; `W <x>` is
worktree; `m <x>` / `' <x>` are marks. lazygit's `gp`/`gP`
collide directly with spyc's `gp` (Action::ReopenLastBuffer) and
`gP` (Action::SetProjectHomeHere).

**Recommendation:** **skip — different conventions, both fine.**
spyc's chord-family discipline (DESIGN.md "Vi where it fits,
screen/tmux where it doesn't") is the right call for a file
commander; lazygit's flat 2-letter mnemonic space works for it
because it owns the whole screen. Worth noting only as a *stay-aware*
point: if spyc ever grows its own git-write actions (push, pull),
they should land under a deliberate prefix like `g G p` / `g G P`
or `:push` / `:pull`, not naked `gp`/`gP`.

**Effort sketch:** N/A — preserve existing chords.

---

## Top 3 to consider first

1. **Generalized pager picker (§4 Menu adaptation).** Single
   highest-leverage change because §5 (scoped help) and several
   future features (`:project` chooser, `W l` worktree picker as a
   first-class surface) all build on it. Pays for itself the day
   it lands.

2. **Context-sensitive prompt-row hint (§2 footer adaptation).**
   Cheap, scoped, and directly addresses the longstanding
   "I-know-it-exists-but-forgot-the-key" failure mode that the help
   overlay only solves with a context switch. Particularly valuable
   inside the pager (where `?`/`n`/`N`/`s`/`:N` are easily forgotten).

3. **Scoped `?` help (§5).** Becomes nearly free once §4 lands; even
   without §4, restructuring the existing help table to lead with
   the *active surface*'s keys is a doc edit, not a feature. Tackle
   last so it inherits the picker affordance instead of standing one
   up.

Everything else (numeric jump, command log, row-verb panel reuse,
flat 2-letter chords) is a deliberate *skip* — the patterns earn
their keep in lazygit because of structural choices spyc has
already made differently, and adopting them would dilute spyc's
own design language rather than strengthen it.

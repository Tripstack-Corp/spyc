# Yazi competitive review

Last reviewed: 2026-05-28
Yazi reference point: github.com/sxyazi/yazi @ ~37k stars; PR #4005
(drag-and-drop) merged the same day as this review.

## Why this doc exists

Yazi is the closest neighbour in the TUI file-commander space and is
already cited by `LAUNCH_PREP.md` as the gold-standard "reputable,
install-and-rely-on TUI tool" we benchmark launch hygiene against.
`ROADMAP.md` carries four Yazi-inspired entries (bulk rename, cwd
export, visual-mode range pick, structured event stream) but there is
no single place that lays out what Yazi actually does, where we
overlap, and where we deliberately don't.

This doc is that place. It is a snapshot — Yazi ships fast, so the
contents will go stale. Re-read before any "should we copy X?"
conversation.

## What spyc is, in one line

A two-pane file commander whose distinguishing feature is a local MCP
socket the bottom-pane agent (Claude Code / Codex / Gemini) calls into.
The file commander is the noun the agent operates on. Yazi is not in
this game.

## Yazi's recent move: PR #4005 — drag and drop

Merged 2026-05-28 (today). Implements end-to-end drag-and-drop via
**kitty's OSC 72 DnD protocol**:

- Drag files from Yazi out to other apps; drop files from external
  apps into Yazi.
- Currently requires kitty 0.47.0+ — kitty is the only terminal that
  implements OSC 72 today.
- Exposes Lua-facing `rt.tty` queue/flush APIs and DnD event bindings;
  ships a preset `dnd.lua` plugin and a tip component for the UI
  affordance.
- ~50 files touched across `yazi-tty`, `yazi-term`, `yazi-shim`,
  `yazi-scheduler`, `yazi-plugin`, `yazi-binding`, `yazi-actor`.

### Implication for spyc

`ROADMAP.md` already lists "Drag and drop — files from the desktop
into spyc via OSC 52 or path paste." That entry predates the OSC 72
spec landing in kitty and is now stale in two ways:

1. **OSC 52 is clipboard, not DnD.** It was a placeholder mechanism;
   OSC 72 is the actual protocol.
2. **The "path paste" fallback is the only thing we'd actually ship
   short-term.** Until iTerm2, WezTerm, Ghostty, and the others adopt
   OSC 72, native DnD is a kitty-only feature. spyc users on macOS
   are predominantly iTerm2/Ghostty/Terminal.app, so the kitty-gated
   payoff is small.

Recommendation: update the ROADMAP entry to reference OSC 72 and
Yazi's PR #4005, and explicitly defer the implementation until at
least one more terminal ships OSC 72. The path-paste fallback can be
done independently and is cheap.

## Feature-by-feature standing

Columns: **Yazi** = ships it; **spyc** = ships it / on roadmap / out of
scope. Roadmap pointers cite the file only — `ROADMAP.md` gets edited
daily and line numbers go stale fast.

### Core capabilities

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Async I/O, multithreaded scheduler     | Yes     | Background loading roadmapped (`ROADMAP.md`) |
| Image preview (kitty/iTerm2/sixel)     | Yes     | Out of scope today; not roadmapped     |
| Code preview / syntax highlighting     | Yes     | Yes (tree-sitter, shipped v1.50.61) |
| Markdown rendered preview              | No (?)  | Yes (shipped v1.26.0)              |
| Multi-tab                              | Yes     | Yes                                    |
| Trash bin                              | Yes (single tier) | Yes — two-tier: in-app **graveyard** (compressed, undo-able from spyc) cascading to system trash (`src/state/graveyard.rs`) |
| Archive extraction                     | Yes     | Not implemented                        |
| Bulk rename via `$EDITOR`              | Yes     | Roadmap (`ROADMAP.md`)             |
| Visual-mode range select               | Yes     | Roadmap (`ROADMAP.md`)             |
| Cwd export on quit                     | Yes (`y` wrapper) | Roadmap, Foundations queue (`ROADMAP.md`) |
| Drag and drop (OSC 72)                 | Yes (PR #4005) | Roadmap, stale (`ROADMAP.md`) — see above |
| `fzf` / `fd` / `ripgrep` / `zoxide` integration | Yes | `F` finder, `:grep`, frecency `J` (homegrown) |
| Mouse support                          | Yes     | Limited; explicit non-goal beyond current (`ROADMAP.md`) |

### Extensibility

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Lua plugin system                      | Yes (concurrent, BETA) | **Non-goal** (`ROADMAP.md`) |
| Theme system / "Flavors"               | Yes (BETA) | Nerd-font / mono toggle; no theme DSL |
| Custom previewers / fetchers           | Yes (Lua) | Not exposed                           |
| Keymap customization                   | Yes (`keymap.toml`) | Yes (`.spycrc.toml`)         |
| Package manager for plugins/themes     | Yes     | N/A (no plugin system)                 |

### Automation / external integration

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Event publish (`ya pub`)               | Yes     | **Explicit non-goal** (`ROADMAP.md`) — wider attack surface, not on thesis |
| Event subscribe (`--local-events`, `--remote-events`) | Yes | Roadmap as subscriber socket on existing MCP UDS (`ROADMAP.md`) |
| Cross-instance state distribution      | Yes ("Data Distribution Service") | Per-PID MCP socket; instances coexist but don't share state |
| MCP server (agent-callable)            | **No**  | **Yes — core thesis** (`README.md`) |
| Virtual filesystem for remote files    | Yes     | Out of scope                           |

### Distribution / hygiene

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Pre-built binaries on tagged release   | Yes     | Roadmap for 2.0 (`LAUNCH_PREP.md`)  |
| Homebrew tap                           | Yes     | Roadmap for 2.0 (`LAUNCH_PREP.md`)  |
| Signed artifacts                       | Partial | Minisign roadmap (`ROADMAP.md`)    |
| Docs site                              | Yes (yazi-rs.github.io) | Single-file `*.md` — deferred (`LAUNCH_PREP.md`) |
| Migration page from peer tools         | Yes (per-tool keymap tables) | Roadmap (`LAUNCH_PREP.md`) |
| GitHub presence                        | Yes (37k stars) | Bitbucket today; GitHub move is 2.0 blocker (`LAUNCH_PREP.md`) |

## Where Yazi clearly leads

Aspects where Yazi is materially ahead and we should not pretend
otherwise:

1. **Image preview.** Yazi treats it as a headline feature with
   multiple protocols. spyc has none. This is a real gap for users
   coming from Yazi; it should at least be acknowledged in the
   eventual migration page.
2. **Plugin ecosystem.** Yazi's Lua plugins and package manager mean
   third parties ship their own previewers, fetchers, themes. spyc's
   non-goal stance is a deliberate trade — say so plainly in the
   migration page so the value of *not* having a plugin system
   (single-binary stability, no plugin-API churn) is the framing.
3. **Mass.** 37k stars vs. our pre-launch numbers. Star count is not a
   feature, but it changes the "is this safe to install?" calculus
   for new users. The 2.0 launch hygiene pass is the response to
   this and is on track.
4. **Drag and drop (as of today).** kitty-only and Lua-gated, but
   they're now first in this corner of the design space.

## Where spyc clearly leads

These are the columns where we're not just different, we're better
positioned for the audience we care about:

1. **MCP bridge.** Yazi's automation story is `ya pub` / event streams
   into custom shell scripts. spyc speaks MCP — the protocol that
   Claude Code, Codex, and the rest of the coding-agent ecosystem
   already speaks. No glue code on the user's side.
2. **Two-pane agent pairing as a first-class layout.** Yazi has
   plugins that approximate this; we ship it as the default UX.
3. **Picks / inventory as a structured selection model.** Yazi has
   visual mode and cross-directory selection; spyc has picks
   *plus* an inventory the agent can read via `get_spyc_context` —
   selection becomes a data structure the agent acts on, not just a
   set of highlighted rows.
4. **Session save/restore for both Claude and Codex.** Resume a
   spyc and the agent panes resume too. Out of Yazi's scope by
   design.
5. **Git-aware listings + frecency `J`.** Both present in spyc as
   built-ins; Yazi gets them via plugins / external tools.
6. **Two-tier delete (graveyard → system trash).** Yazi has a flat
   trash bin. spyc stages deletes in a compressed in-app graveyard
   (tar.zst, undo-able from inside spyc with `p`/`P`) that cascades
   FIFO to the system trash when a cap is hit. Means "I just deleted
   the wrong thing" is one keystroke away from recovery without a
   context switch to Finder / `gio trash list`. See
   `src/state/graveyard.rs` for the design rationale.

## Where we deliberately differ

Worth being explicit so the next "Yazi has this, should we?"
conversation has a fast answer.

- **Plugin system.** Non-goal. `ROADMAP.md` is the canonical
  statement: "A decade of maintenance debt for a feature 3% of users
  will touch."
- **Mouse beyond current.** Non-goal (`ROADMAP.md`). Keyboard-first
  by thesis.
- **Event publishing (`ya pub` equivalent).** Non-goal per
  `ROADMAP.md–338` — the consumer ecosystem we care about is
  keyboard/agent-flavoured, not a generic automation bus, and the
  attack surface of accepting arbitrary publishes from anywhere on
  the box is not worth it for our shape.
- **Virtual filesystem for remote files.** Out of scope; ssh +
  local mount is the supported workflow.
- **Localization.** English-only by stated non-goal
  (`ROADMAP.md`).

## Recommendations

Concrete actions falling out of this review:

1. **Update `ROADMAP.md` (drag and drop).** Replace the OSC 52
   reference with OSC 72; cite Yazi PR #4005; note that the native
   path is kitty-only today and defer until ≥1 more terminal adopts
   OSC 72; keep the path-paste fallback as an independent, cheap win.
2. **Add an "image preview" row to the migration page**
   (`LAUNCH_PREP.md`). Honest framing: "Yazi has it; spyc doesn't;
   if you live in image-heavy directories, Yazi may suit you better."
   Trying to hide the gap will burn trust faster than naming it.
3. **In the same migration page, lead the differentiator paragraph
   with MCP/Claude pairing**, then picks-as-data-structure, then
   session save/restore. These are the three things Yazi cannot match
   without changing what it is.
4. **No change to the plugin-system non-goal.** Yazi's plugin system
   is its main extensibility vector; ours is the MCP surface. Don't
   re-litigate.
5. **Re-run this review** when Yazi cuts its next significant release
   or when a second terminal adopts OSC 72 (whichever comes first).

## Notes for future passes

- The Yazi docs site (yazi-rs.github.io) is the canonical feature
  list. The README's feature bullets are a subset.
- `ya pub` and the events flags are the closest analog to our
  MCP socket. The shapes are different enough that "we could just
  do what Yazi does" is the wrong question — the right question is
  whether the MCP socket should grow a publish verb. Today's answer
  is no; revisit if a non-agent consumer (tmux status segment,
  Neovim plugin) actually shows up asking for it.
- This doc supersedes the implicit "we know about Yazi" status of
  the four Yazi-inspired ROADMAP entries. Those entries stay as the
  per-feature design notes; this doc is the synoptic view.

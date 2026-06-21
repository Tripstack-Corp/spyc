# spyc Design

This document defines the **UI design language**: component names,
visual conventions, interaction patterns, and the philosophy that
new features should fit. The goal is that extensions land in a
recognizable, consistent shape rather than as one-off widgets.

For *engine* decisions (concurrency, persistence, MCP), see
[`ARCHITECTURE.md`](ARCHITECTURE.md). For per-module navigation,
see [`AGENTS.md`](AGENTS.md).

## Theme: tokyo-night with a pepper

The palette descends from tokyo-night, recolored slightly to suit a
file-commander mood. Defined in `src/ui/theme.rs`. Every named role
ships a default; `.spycrc.toml`'s `[colors]` table overrides them.

| Role | Default | Used for |
|---|---|---|
| `dir` | tokyo blue | directory rows |
| `exec` | soft green | executable / git-added marker |
| `symlink` | cyan | symlinks / git-renamed marker |
| `file` | off-white | regular files |
| `cursor_bg` | warm terracotta | cursor row when list focused |
| `cursor_bg_dim` | muted indigo | cursor row when pane focused |
| `pick` | amber | picked rows / `~` modified marker / pepper accents |
| `take` | lavender (alt) | inventory rows (taken into the cache) |
| `status_user` | lavender | session-name segment background |
| `status_path` | pale lavender | path segment foreground |
| `status_suffix` | indigo grey | suffix segment (`[picks:0 …]`) |
| `prompt_prefix` | amber | the leading `:`, `!`, `;`, `/`, etc. |

**Mono mode** (`C` toggle) drops to default terminal colors with
modifiers (BOLD, DIM, REVERSED) carrying the structural information.
Anything new must read in mono — no role-only-by-color.

## Component vocabulary

These are the names. Use them in code and when writing.

### Layout components

- **Status bar** — single row, powerline segments. Top by default,
  bottom via `[layout] status_position = "bottom"`. Segments, in
  order: pepper logo `🌶️` · `PROJECT_HOME` (basename) · session name
  (`SAFFRON_CUMIN` style) · path · git branch + dirty flag · suffix
  (`[picks:0 inv:0 m1:on m2:on hidden:14]`). Segments collapse left
  to right when the terminal is narrow.
- **List** — the file area. Vi-navigated. Each row: cursor bar,
  pick check, take check, name, git marker, size/age (depending on
  mode). The "central widget" — most operations resolve against the
  cursor row or the picked set.
- **Prompt** — one row, anchored opposite the status bar. A
  prefix character indicates mode: `:` command line, `!` captured
  shell, `;` interactive shell, `$` interactive shell handoff, `/`
  search, `=` limit filter, `J` jump, `T` glob pick, `c`/`M` copy/
  move target, `+` mkdir, `O` new file, etc.
- **Divider** — pane separator. Rule with tab indicators (`─[1*] claude
  ─[2+] bash`) and the active tab's *live* cwd (`── ↪ /tmp` if
  drifted from spawn). `[SCROLL]` tag right-aligned when in pane
  scrollback mode.
- **Pane** — bottom split. PTY-hosted subprocess (default `claude`).
  Tabs within. `^a` is the prefix (`screen`-style); `^w` is an alias.

### Overlays (mutually exclusive)

- **Pager** — full-screen content viewer. Used for: file content,
  `!` captured output, `gd`/`gD`/`gb` git output, `I` info, help
  (`?`), pane-output dump. Shares a search bar (`/`), `:N` jump,
  hex toggle, line-number toggle, save (`s`), `q` to dismiss.
- **Top overlay** — interactive `;` command. Embedded `Pane` over
  the top portion of the screen. Owns all keys until the
  subprocess exits, then a single key dismisses.
- **Inventory view** — alternate listing of the per-user file
  cache + graveyard. Toggled with `i`.
- **Session picker** — startup `-r` view, j/k navigation, Enter to
  restore, `n` for fresh.

Only one overlay is visible at a time. Pager is the primary; new
overlay-shaped features should generally render *into* the pager
rather than create a new modal type.

### Transient surfaces

- **Flash** — single-line message at the prompt row, cleared on the
  next key. Two flavors: `flash_info` (neutral) and `flash_error`
  (red). The default surface for "something happened" feedback.
  **Use this** for non-blocking notifications; reach for an overlay
  only when the user needs to read more than one line or interact.
- **Confirm** — typed-letter inline confirmation embedded in the
  prompt (`y`/`n`). Used by destructive operations (`R` remove,
  worktree delete) and interrupt-on-quit. Do not introduce a
  separate dialog box — extend confirm.
- **Activity HUD** — tiny right-anchored debug overlay (`A`
  toggle). Reports dps, bytes/sec, poll period. Engineering tool;
  features should not depend on it.

## Icons & glyphs

Used sparingly — every glyph below has a specific meaning.

| Glyph | Meaning |
|---|---|
| `🌶️` | spyc logo (status bar, exit summary) |
| `~` | git-modified file marker |
| `+` | git-added marker / pane-tab activity badge |
| `?` | git-untracked marker |
| `-` | git-deleted marker |
| `>` | git-renamed marker |
| `↪` | live pane cwd has drifted from spawn cwd |
| `⏳` | `!` captured command running |
| `…` | middle-truncation in a path |
| `*` | active pane tab |
| `[SCROLL]` | pane is in scrollback mode |
| `[exited N]` | pane subprocess exited with code N |
| `[N+]` | bg task #N running with new output (teal, divider) |
| `[N●]` | bg task #N running, quiescent (blue, divider) |
| `[N✓]` | bg task #N exited cleanly (green, divider) |
| `[N✗]` | bg task #N non-zero exit / killed / crashed (red, divider) |

Powerline arrows (``) separate status segments and require a
Nerd Font; mono mode switches to plain spaces and a single rule.

Tilde-collapse (`/Users/derek/src/spyc` → `~/src/spyc`) is applied
at every user-facing path display via `paths::display_tilde`. Match
is anchored at directory boundaries. **MCP context output is
exempt** — consumers expect absolute paths.

## Interaction philosophy

### Keys are the API

Mouse is a courtesy (DEC 1007 alternate scroll for wheel, native
text selection works). Every action has a keybinding; every
keybinding maps to an `Action` enum variant. New features add a
variant, a default binding in the resolver, and a help-table row.

### Vi where it fits, screen/tmux where it doesn't

- Movement, marks, prompt editing → vi.
- Pane prefix, tab switching → screen (`^a` family).
- Worktree management → `W` family (uppercase = "weighty"
  operation that touches git state).

When inventing a new chord, prefer:
- Single-letter for common single-file ops (`R`, `f`, `+`).
- `g <x>` for "go-to" or read-only git commands (`gh`, `gP`,
  `gd`, `gb`, `gf`).
- `^a <x>` for pane-tab scope.
- `:cmd` for one-shot ops that don't deserve a binding.

Capital is a **stronger** variant of the lowercase, where it makes
sense (`y`/`Y`, `c`/`C` is *not* this — `C` predates and is mono
toggle; document exceptions).

### One shape per job

- Need a long output? **Pager.**
- Need a yes/no? **Confirm in the prompt row.**
- Need short feedback? **Flash.**
- Need to type a value? **Prompt with prefix character.**
- Need to interactively run something? **Top overlay.**

If a feature feels like it needs a *new* surface, the answer is
usually that an existing surface should be extended.

### No spinners on the hot path (yet)

Idle dps target is 0. Anything that polls — file watcher, capture
streams, MCP socket — runs in a thread and pushes events; the
loop only redraws when something changed. When background loading
lands (see ROADMAP), the spinner is a single-row prompt-area
indicator with a >50ms threshold so the common case never sees it.

## Status segments: grammar

The suffix `[picks:0 inv:0 m1:on m2:on hidden:14]` is a
key-value sequence in fixed order. New segments added there must:

- Be one short word, no spaces (`m1`, `inv`, `hidden`).
- Show only when non-default or relevant (the `limit:`, `hidden:`
  segments only appear when active).
- Stay inside one row even at narrow terminals.

Don't introduce a third status row. If you need more, the answer is
the `I` info overlay or a flash.

## Naming glossary

| Term | What it is |
|---|---|
| **picks** | per-directory multi-select (lost on chdir) |
| **inventory** | persistent cross-directory file cache (`y` yanks into it) |
| **graveyard** | files removed via `R` or graveyard'd from inventory; recoverable until pruned |
| **marks** | vi-style `m{a-z}` named cursor positions, persistent |
| **session** | workspace snapshot at quit (cwd, tabs, focus, `PROJECT_HOME`, etc.) |
| **session name** | spice-pair display name (`SAFFRON_CUMIN`); editable via `:name` |
| **`PROJECT_HOME`** | sticky per-session project root; `gh` jumps; new pane tabs default cwd here |
| **`start_dir`** | backtick-jump target; not project-scoped |
| **limit filter** | temporary glob applied to the listing (`=*.rs`); `=` clears |
| **mask** | persistent ignore-glob group (`a` toggles mask 1, `o` mask 2) |
| **pane** | the bottom-split PTY child |
| **tab** | one PTY child within the pane |
| **overlay** | a modal surface drawn over the list/pane area |
| **flash** | one-line transient message at the prompt row |
| **prompt** | single-row text input; prefix character indicates mode |
| **resolver** | the FSM that turns keys (with chord state) into `Action`s |

## When extending

A checklist for adding a feature with UI surface:

1. Does it fit an existing surface (flash / confirm / pager /
   prompt / overlay)? If yes, extend that. Reach for a new surface
   only when those genuinely don't fit.
2. Pick a name from the glossary. If the concept is new, name it
   in plain English first; if the name sticks, add it to the
   glossary in the same commit.
3. Pick a key from a chord family that already has a theme. Avoid
   inventing a new prefix.
4. Wire an `Action` enum variant in `keymap/action.rs`, default
   binding in the resolver, handler in the matching `src/app/`
   child module (or `command_table.rs` for a `:`-command), help
   row in `ui/help.rs`.
5. Confirm it reads in mono. If color carries meaning, also use a
   modifier (BOLD / DIM / REVERSED).
6. Update CHANGELOG, BACKLOG_DRAFT_NOTES.md (if it closes one), and any user-facing
   doc. ARCHITECTURE only if the *engine* changed; DESIGN only if
   the *language* changed.

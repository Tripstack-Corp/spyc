# caleb-initial-thoughts-and-findings — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: caleb-initial-thoughts-and-findings
Created: 2026-05-15T05:38:39.223205+00:00

---
Entry: Claude Code (caleb) 2026-05-15T05:38:39.223205+00:00
Role: scribe
Type: Note
Title: First entry — spy → spyc keymap & action-vocabulary comparison matrix

Spec: docs

tags: #observations #keymap #spy-parity

# Frame for the thread

This thread collects my (Caleb's) initial impressions of spyc, working from the perspective of a long-time spy user. I've used spy daily for decades — `e` to descend, `t` to pick, `yy`/`p` to move things, `R` to delete, `:!cmd %` to do things to selections — and the thing I'm most curious about is how that muscle memory transfers to spyc.

First entry below is the boring-but-foundational reference: a side-by-side comparison of spy's keymap against spyc's, and a translation of spy's action vocabulary against spyc's `Action` enum. Subsequent entries will record the things I notice using the tool day-to-day.

# 1. Keymap matrix — spy keys, walked

Spyc's vocabulary verified against `src/keymap/action.rs:6-180`.

## Direct parity (same key, same/equivalent action)

These are the muscle-memory bindings that "just work" — sit down at spyc and they do what spy taught me to expect.

| Key | spy action | spyc action |
|---|---|---|
| `^B` `<PageUp>` | pageup | PageUp |
| `^F` `<PageDown>` | pagedown | PageDown |
| `^D` | quit | Quit (also `Q`, `:q`) |
| `^L` | redraw | Redraw |
| `^R` | loadrc | ReloadConfig |
| `^T` | unpick (toggle all) | PickToggleAll |
| `^X` | chmod +x | ChmodAdd('x') |
| `<Space>` | right | Right |
| `!` | unix_cmd | ShellCapturedPrompt (now streams to pager) |
| `$` | startshell | StartShell |
| `/` | search | SearchPrompt |
| `:` | spy_cmd | CommandPrompt |
| `?` `F1` | keys / help | Help |
| `~` `<Home>` | home | Home |
| `<Up>` `<Down>` `<Left>` `<Right>` | up/down/left/right | Up/Down/Left/Right |
| `J` | jump | JumpPrompt |
| `L` | longlist | LongList |
| `R` | remove | RemovePrompt |
| `T` | patternpick | PickPatternPrompt |
| `Q` | quit | Quit |
| `c` | copy | CopyPrompt |
| `d` | display | EnterOrDisplay |
| `e` `v` | enter | EnterOrEdit |
| `f` | file | FileType |
| `h` `j` `k` `l` | left/down/up/right | Left/Down/Up/Right |
| `i` | inventory | ToggleInventoryView |
| `n` | next (search) | SearchNext |
| `p` | drop (inventory put) | Drop |
| `s` | setenv | SetEnvPrompt |
| `t` | pick | TogglePick |
| `u` | climb | Climb |
| `z` | empty (inventory) | EmptyInventory |
| `I` | showmemory | ShowMemory (richer: pid, rss, counts) |

That's ~30 bindings carried over straight. The basic file-commander loop (move, descend, pick, copy/move/remove, take/put, search, shell-out, command-line) is fully spy-compatible.

## Repurposed (same key, different action — muscle-memory hazards)

| Key | spy did | spyc does | Where the spy action moved |
|---|---|---|---|
| `<Enter>` | down (cursor) | EnterOrDisplay | spyc treats Enter like `d` — opens dir / pager. The "Enter = down" pattern is gone (use `j` or `<Down>`). |
| `^I` (Tab) | right | unbound at top level | (no top-level binding I can see; presumably free for overlays) |
| `^J` | nextfile (cursor down) | (newline in pane only) | Cursor down is `j` / `<Down>`. `Ctrl+J` is reserved for pane multi-line input. |
| `^K` | previous (cursor up) | unbound at top level | Cursor up is `k` / `<Up>`. |
| `^P` | unix (process check) | unbound | Use `!ps -ef` or similar. |
| `^W` | unix (chmod +w) | **pane chord prefix** (alias of `^a`) | chmod +w via `!chmod +w %`; `^W` now leads pane chords. |
| `-` | up | Climb | `-` climbs to parent now (matches `u`). Use `k` / `<Up>` for cursor. |
| `D` | date | DisplayInPane (open `$PAGER` in top pane, bottom stays visible) | Date moved to `:date`. |
| `H` | home | **harpoon chord prefix** (`Ha` `Hx` `H1`–`H9` `Hh`) | Home moved to `~` and `<Home>`. |
| `M` | (mail) | MovePrompt | spy's `m` lowercase = move; spyc moved that to `M` and freed `m{a-z}` for vi marks. |
| `m` | move | SetMark prefix (vi-style: `ma`, `mb`, ...) | Move moved up to `M`. |
| `N` | makedirs | SearchPrev | makedirs moved to `+`. |
| `V` | version | EditInPane (open `$EDITOR` in top pane, bottom stays visible) | Version moved to `gV` / `:version`. |
| `y` | take | **chord prefix** (`yy` = take, `yp` `yP` `ya` `yf` = various yanks) | Take is `yy` now. |
| `1`–`9` | unix-bound (hview / houdini / sysinfo / processes) | motion-count prefix (`5j`); harpoon-slot inside `H` chord; tab-index inside `^a` chord | Houdini-specific binds gone; digits are motion / chord context now. |

The two big "wait, that's different" moments for a spy user: `H` is harpoon, not home; `D` opens a pager rather than printing the date. Easy enough to retrain; worth knowing on day one.

## Renamed (same action, different key)

| spy action | spy key | spyc action | spyc key |
|---|---|---|---|
| date | `D` | Date | `:date` |
| version | `V` | Version | `gV` / `:version` |
| home | `H` | Home | `~` / `<Home>` |
| move | `m` | MovePrompt | `M` |
| makedirs | `N` | MakeDirPrompt | `+` |
| help | `#` `<F2>` | Help | `?` / `F1` |

## Dropped from spy

| Spy key + binding | Why probably gone |
|---|---|
| `1`/`2`/`3`/`4` → hview/houdini/sysinfo/procs | Houdini/SideFX-specific |
| `^P` → process check | Use shell-out (`!ps`, `!htop`) |
| `^W` → chmod +w | Reused as pane prefix; chmod is `!chmod +w %` |
| `M` → mail | Use shell-out (`!mutt`, `!neomutt`, etc.) |
| `^I` (Tab) → right | Probably free for future overlay use |
| `^J` / `^K` → next/prev cursor | Redundant with j/k and arrow keys |
| `<F2>` → help | Consolidated to `?` / `F1` |

# 2. Action vocabulary translation

Spy's "spy tools" list against spyc's `Action` enum. Every spy action that's still semantically meaningful has a spyc counterpart; some are renamed.

| spy action | spyc Action | Notes |
|---|---|---|
| pageup | `PageUp` | — |
| pagedown | `PageDown` | — |
| date | `Date` | Now `:date` only |
| jump | `JumpPrompt` | Same UX; expanded path syntax |
| makedirs | `MakeDirPrompt` | Now `+` |
| longlist | `LongList` | — |
| patternpick | `PickPatternPrompt` | — |
| quit | `Quit` | Adds `Q` and `:q` |
| remove | `RemovePrompt` | Selection now goes to **graveyard**, recoverable via `gy`/`:undo` |
| move | `MovePrompt` | Now `M` |
| copy | `CopyPrompt` | — |
| enter | `EnterOrEdit` | — |
| display | `EnterOrDisplay` | — |
| climb | `Climb` | — |
| file | `FileType` | — |
| left/right/up/down | `Left/Right/Up/Down` | Adds count prefix (`5j`) |
| pick | `TogglePick` | — |
| help/keys | `Help` | Single command, two key bindings |
| home | `Home` | — |
| unpick | `PickToggleAll` | — |
| redraw | `Redraw` | — |
| unix_cmd | `ShellCapturedPrompt` | Now PTY-backed, streams into pager, supports `!!`/`!?`/`^Z`-bg |
| spy_cmd | `CommandPrompt` | Vim-style `:` prompt |
| loadrc | `ReloadConfig` | Auto-reload also live |
| ignoretoggle | `ToggleMask(N)` | Two masks: `a` (dotfiles), `o` (build artifacts) |
| startshell | `StartShell` | — |
| search | `SearchPrompt` | Substring by default; glob if pattern contains `* ? [` |
| next | `SearchNext` | Adds `SearchPrev` (`N`) |
| take | `Take` | Now requires `yy` (y is chord prefix) |
| drop | `Drop` | Same key (`p`) |
| inventory | `ToggleInventoryView` | — |
| empty | `EmptyInventory` | Items go to graveyard on the way out |
| showmemory | `ShowMemory` | Richer info pager |
| colortoggle | `ColorToggle` | Now `C` |
| nextfile | (collapsed into `Down`) | `^J` repurposed |
| previous | (collapsed into `Up`) | `^K` repurposed |
| command | `CommandPrompt` | spy bound it to `%`; spyc uses `:`, and `%` is selection-substitution |
| version | `Version` | Now `gV` / `:version` |
| setenv | `SetEnvPrompt` | — |

**Net translation: every meaningful spy action is preserved.** A handful got merged into adjacent actions (cursor up/down via vim keys instead of `^J`/`^K`), a handful moved keys, but nothing in the spy vocabulary is functionally lost.

# 3. New in spyc (the additive layer)

These have no spy ancestor — they're spyc-era additions. Grouped roughly:

**Vi-style cursor & marks**
- `gg` / `G` first/last entry
- count prefix (`5j` `10k`)
- vi marks: `m{a-z}` set, `'{a-z}` jump, `''` jump-back, `` ` `` start-dir

**Project-aware navigation**
- `gh` / `gP` / `gS` / `gU` — project home, set-project-home, start-dir, user@host
- Harpoon: `Ha` / `Hx` / `H1`–`H9` / `Hh`
- `J` jump-to-path with frecency
- `F` project-wide fuzzy filename finder

**Yank family**
- `yy` (take), `yf` (path to clipboard), `yp` (pane output), `yP` (last prompt), `ya` (full scrollback)

**Filtering**
- `=` glob filter, `=git` / `=g` git-changed, `=h` harpoon, `=!` picks, `=` empty clears

**File ops added**
- `+` makedir, `O` new-file-in-editor, `M` move (was `m`)

**Git integration**
- `gd` diff HEAD, `gD` diff cached, `gb` blame
- `]g` / `[g` cursor to next/prev git-changed entry
- `Wl` / `Wn` / `Wd` worktree list/new/delete
- Two-character left-gutter git status markers per file

**Recoverability**
- Graveyard: `R` puts to graveyard (not direct delete); `gy` / `:undo` / `:graveyard` to recover

**Top-pane / bottom-pane split**
- `^\` / `F10` / `^a \` toggle pane
- Bottom pane defaults to `claude` (also codex, gemini)
- `^a j` / `^a k` focus
- `^a c` new tab, `^a 1`–`9` switch, `^a K`/`^a x` close, `^a r` rename, `^a R` restart
- `^a +` / `^a -` resize, `^a z` zoom
- `^a v` scroll-mode, `^a u` quick select, `^a s` send-selection-paths, `^a P` pipe-content, `^a i` pipe-inventory
- `V` open editor in top pane, `D` open pager in top pane (bottom stays visible)

**Background tasks**
- `^Z` (in capture pager) backgrounds; `:fg` / `:fg N` resume; `gB` task viewer
- `:pause` / `:resume` SIGSTOP/SIGCONT
- `:task-to-pane` / `:pane-to-task` promote/demote between bg-task and pane-tab
- `:bprev` / `:bnext` / `gp` / `[b` / `]b` pager buffer history

**MCP bridge** (the spyc thesis surface)
- spyc starts a project-scoped MCP server on a Unix socket; bottom-pane Claude/codex/gemini queries it via tool calls
- Tools: `get_spyc_context`, `get_file_content`, `navigate_to`, `set_filter`, `pick_files`, `clear_picks`, `search_paths`, `search_content`, `search_picks`, `search_inventory`
- Reverse direction: `gf` / `gF` jump from pane output paths into the file list

# 4. UX similarity — initial assessment

**Strong continuity.** A spy user's basic loop transfers cleanly: `hjkl` to move, `e`/`v` or `d`/`<Enter>` to descend, `t` to pick, `yy`/`p` for inventory, `R` to remove (now soft via graveyard, which I welcome), `c`/`M` to copy/move, `:` for commands, `!` for shell-out, `?` for help. The first hour will be muscle-memory-comfortable for anyone who's used spy.

**Three day-one re-trains.** The bindings that will trip me up on the way in:
1. `<Enter>` is now an action key (open dir/pager), not cursor down. Must use `j` / `<Down>`.
2. `H` is the harpoon chord prefix, not home. Home moved to `~` / `<Home>`.
3. `D` opens the in-pane pager, not the date. Date moved to `:date` (which I'd never have looked for if I hadn't consulted help).

**Cumulative additions outweigh subtractions.** The action vocabulary is a strict superset of spy's once you account for renames. The new surfaces (pane, MCP bridge, harpoon, project-home, git integration, graveyard, fuzzy finder, marks, counts) make spyc feel like spy + a decade of vim/tmux/ide ideas glued in carefully. Worth noting that the bone shape — the keyboard-driven multi-column file commander that lets me think in selections and shell-out — is preserved without compromise.

**Removed-because-Houdini.** The spy bindings I won't miss: `1`/`2`/`3`/`4` for hview/houdini/sysinfo/procs, `M` for mail. Those were SideFX-environment-specific and have no place in a general tool. Their slots got better uses (motion counts, move-prompt).

# 5. To investigate next (entries to come)

- `=git` workflow vs `]g`/`[g` jumps — does the new git-aware navigation actually beat `:!git status` muscle memory in practice?
- Harpoon: how does the slot semantics feel against marks? Both exist; both jump-and-position. Where does each win?
- Bottom-pane MCP integration: how often do I find myself naturally describing context to Claude vs. expecting it to query?
- The pager surface: how does it stack against `:!less`-style external paging? The `[b`/`]b` buffer history is unfamiliar; want to test it on a real workflow.
- Visual-line mode (`V` in the pager) for range yank — new pattern; need to use it.
- Quick Select (`^a u`) — I've never had this affordance; want to see whether it earns its key.
- Background task lifecycle (`^Z` / `:fg` / `:task-to-pane`) — the most ambitious surface I haven't tried.

Provenance:
- spyc action vocabulary cited from `src/keymap/action.rs:6-180` (the `Action` enum and `describe` impl).
- spy keymap and action list provided by Caleb (long-time user, transcribed from working spy install).
- spyc keymap from the in-app `?` help overlay (194 lines); cross-validated against the Action enum.
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KRN29MHY5FYWG59QTK0MQ3M1 -->

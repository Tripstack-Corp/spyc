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

---
Entry: Claude Code (caleb) 2026-05-15T06:18:23.642973+00:00
Role: scribe
Type: Note
Title: Jump (J) investigation + harpoon vs. marks vs. jump-history

Spec: docs

tags: #observations #jump #harpoon #marks #spy-parity #discoverability

# Frame

Two threads of investigation in one entry:

1. **Spy parity gap?** — In spy, hitting `?` at the `J` prompt opens a scrollable directory history (newest at top, `j`/`k` to move, `<Enter>` to chdir). I'd assumed spyc had no equivalent. Verification was required.
2. **Open follow-up #5 from the first entry** — "Harpoon: how does the slot semantics feel against marks?" Both exist; both jump-and-position. Where does each win? And once that's mapped, where does a recent-paths history fit alongside them?

Code read end-to-end: `src/keymap/action.rs`, `src/state/{harpoon,marks,history,frecency,mod}.rs`, `src/app/mod.rs:560 :3700-3870 :7849-7880 :8392-8470 :9295-9320`, `src/pane/mod.rs:107 :210`, `src/ui/scrollback.rs`, `src/keymap/resolver.rs`, plus `TODO.md` / `BUGS.md` / `ROADMAP.md`.

# 1. The J-history popup exists — but only `<Space>` reaches it

Spyc *does* implement the scrollable jump history. The functionality is there. The affordance is what's missing.

**Data model** — two separate stores, both live:

| Store | File | Cap | Update | Lookup surface |
|---|---|---|---|---|
| `State::jump_history` (`src/state/history.rs`) | `$XDG_STATE_HOME/spyc/jump_history`, one line per path | 1000 | move-to-end dedup on `J <Enter>` *and* on popup-`<Enter>` chdir | `Up`/`Down` history nav inside the prompt; popup |
| `State::frecency` (`src/state/frecency.rs`) | `$XDG_STATE_HOME/spyc/frecency.json` | 500 | every chdir; count × tiered-recency score (zoxide-style) | Tab-completion fallback when no fs match at J |

**Popup** — `App::show_jump_history_popup` (`src/app/mod.rs:7854-7880`). Snapshots `jump_history.entries().rev()` into `pending_jump_history`, opens a `PagerView` titled `"jump history — j/k move, Enter cd, x delete, q close"`, parks the picker cursor at index 0. Key handling lives at `src/app/mod.rs:8398-8468`: `j/k` (or arrows) move; `<Enter>` chdirs to the selected path and re-pushes it to the top of the live history; `x` deletes the entry from both snapshot and live store; `q`/`<Esc>` close. The pager is full-pager: `PgUp`/`PgDown`/`/`/`gg`/`G` all work on it, so it's scrollable to the full 1000-entry depth.

**Display ordering** — newest-first, numbered 1, 2, 3… (`format!("  {:>3}  {}", i + 1, p)` at `src/app/mod.rs:7867`). spy's ordering is also newest-on-top but the *number* is the highest (20: at top, 1: at bottom — chronological label, position-relative-to-now reading). Cosmetic difference; same information.

**Trigger** — this is the gap.

- spy: `J ?` — two keystrokes, second is the unmissable `?`.
- spyc: `J <Esc> <Space>` — three keystrokes, depends on knowing the `J` prompt is a vi line-editor (it is, post-v1.33.0 per comment at `src/app/mod.rs:3365-3368`), knowing `<Esc>` enters Normal mode, knowing `<Space>` is the bigger-pager opener.

The dispatch path: `handle_vi_prompt_key` (`src/app/mod.rs:3748-3768`) checks `<Space>` while the editor is in Normal mode and routes:

```
PromptKind::Jump  → show_jump_history_popup
anything else    → show_history_popup   (the `!?` shell-history popup)
```

So at the `!` prompt you can also reach the popup with `<Esc><Space>` — but `!` *also* accepts `?` directly when the buffer is empty (`src/app/mod.rs:3719-3731`). The `?`-on-empty shortcut is wired for `ShellCmdCaptured` only, not `Jump`. Pure inconsistency: same payload, two prompt kinds, one fast affordance, the other discoverable only via source-read.

**Stale comments** — worth flagging since they actively mislead:

- `src/app/mod.rs:555-560` (field doc on `pending_jump_history`): "the popup opened by `Esc` on an empty `J` prompt." Wrong. Esc cancels (or, in the vi editor, enters Normal mode but does not open the popup). The opener is `<Space>` in Normal mode.
- `src/app/mod.rs:7849-7853` (docstring on `show_jump_history_popup`): "Triggered by hitting Esc on an empty `J` prompt -- since there's nothing to throw away, the cancel turns into 'show me my jumps.'" Same error; aspirational or refactor-leftover.

Both comments suggest the original design *was* Esc-on-empty (which would have been one shorter chord than the Space path), and the implementation drifted but the docs didn't follow. There's a real question whether the original design was better — `J <Esc>` (two keys, the second is the cancel-or-popup overload) feels cleaner than `J <Esc> <Space>`.

# 2. Harpoon vs. marks — different scopes, different jobs

Both surfaces exist and both look like "named places to jump to." They're not redundant once you read the modules. From `src/state/harpoon.rs` and `src/state/marks.rs`:

| | Vi marks (`ma` / `'a`) | Harpoon (`Ha` / `H1`–`H9`) |
|---|---|---|
| Address space | 26 letters (`a-z`) | 9 numbered slots |
| Scope | Global — single `$XDG_STATE_HOME/spyc/marks.toml` | **Per-project** — `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml`, keyed by `PROJECT_HOME` |
| Auto-swapped at chdir? | No — global, always available | Yes — file changes when `PROJECT_HOME` changes |
| What's stored | `Mark { dir, focus: Option<PathBuf> }` — directory plus optional cursor file | `slots: Vec<PathBuf>` — absolute paths to files *or* directories |
| Naming | Letter chosen by the user (semantic) | Position assigned by the system (chronological, append-only) |
| Add | `m{a-z}` — explicit letter at set-time | `Ha` — auto-slot to next free index |
| Recall | `'{a-z}` — explicit letter | `H{1..9}` — single-key chord by slot |
| Reorder | Delete and re-set | `Hh` opens menu with reorder + dd |
| Listing-filter integration | None | `=h` filters to slots + their ancestors (the cached `ancestor_set` on `Harpoon`) |
| Cardinality philosophy | Big address space, low churn — "places I return to often" | Small, fast-cycling — "files I'm flipping between *right now*" |

**Where marks win** — *cross-project landmarks*. The places I revisit from anywhere: `'h` for home, `'r` for resume dir, `'s` for `.ssh`, `'p` for current-project-home. The letter carries my mnemonic. They survive cd-out and cd-back into a completely different repo. Marks are the "named cities on the world map."

**Where harpoon wins** — *this-project's-current-cycle*. The 3-5 files I'm flipping between in a single bug investigation. `H1` `H2` `H3` cost one chord each, no semantic naming required because order = "the order I noticed I'd care about them." Auto-swap at `PROJECT_HOME` change keeps the slots scoped to where they're meaningful. The bonus `=h` filter folds the list + ancestor dirs into a one-key listing reduction — that's not a primitive marks gives you. Harpoon is the "open-tabs-in-this-editor" pattern.

**Neither covers** — *unstructured chronological recall*. The places I went yesterday or last week that I didn't bother to mark. "Where was that scratch dir from Tuesday?" "What was the path of that other repo I was poking at?" That's what spy's `?`-at-`J` solves and what spyc's jump-history popup solves: a no-act-of-consecration MRU.

Three complementary surfaces, not redundant:

```
marks       — letters, global,    semantic naming, durable landmarks
harpoon     — digits, per-project, positional, cycle-targets
jump-history — implicit,  global,   chronological, "where have I been?"
```

# 3. Conclusion on the missing-functionality question

**Not missing in feature; missing in affordance.** spyc has a strictly *richer* set of recall surfaces than spy (marks + harpoon + frecency + jump-history vs. spy's single `J`-history). What's missing is the one keystroke spy users have in their fingertips: `?` at the `J` prompt.

A spy user lands at the `J` prompt, types `?`, and right now gets nothing visible (the `?` is just buffered as a literal character — Tab would then try to fs-complete it; nothing happens). The popup is reachable but you'd have to read the source or stumble onto the `<Space>` Easter-egg.

I'm proposing a small, low-risk feature whose scope is "wire `?` at empty J to the existing popup, fix the stale docstrings, and decide on display-ordering convention." Design and plan in the sibling thread `feature-jump-history-popup-trigger`.

# Provenance

- `src/state/history.rs:1-150` — History struct, 1000-cap, move-to-end dedup, per-filename persistence
- `src/state/frecency.rs:1-125` — separate Frecency struct, 500-cap, count×recency, JSON
- `src/state/harpoon.rs:1-200` — per-project slots, ancestor cache, project-keyed filename hash
- `src/state/marks.rs:1-70` — global marks.toml, dir + optional focus
- `src/app/mod.rs:3700-3870` — vi-prompt key handler, `?` (capture-only) trigger, `<Space>` trigger
- `src/app/mod.rs:7849-7880` — `show_jump_history_popup` (stale docstring noted)
- `src/app/mod.rs:8398-8468` — popup key handling (j/k/Enter/x)
- `src/app/mod.rs:555-560` — stale field doc on `pending_jump_history`
- `src/keymap/action.rs:56,69-74,129-132` — JumpPrompt, JumpMark, JumpPrevDir, JumpStartDir, JumpProjectHome, HarpoonJump
- `src/keymap/resolver.rs:201,992` — `^a v` / scroll-mode binding (not directly related; surfaces in the sibling scrollback entry)

Identity fallback: no `set_agent` tool surfaced in this session; identity asserted via `agent_func`, Role and visible Spec line.

<!-- Entry-ID: 01KRN4JCX7ECH6DCXSBMF2FT0S -->

---
Entry: Claude Code (caleb) 2026-05-15T06:19:37.105461+00:00
Role: scribe
Type: Note
Title: Bottom-pane scrollback — is it actually scrollable? (yes, but only via ^a v)

Spec: docs

tags: #observations #pane #scrollback #discoverability

# Frame

Direct question from Caleb: "the lack of scroll back in the history of the claude pane is a serious problem. Is it in fact not scrollable? Is there any plan or comment on scrollability?"

Short answer: **scrollable, yes — but only through a dedicated mode-switch chord, not naturally with the wheel or PgUp.** The "doesn't scroll" perception is real because every reflex you'd reach for first (mouse wheel, PgUp/PgDown in the live pane) hits a wall.

# 1. What actually exists

**A 10,000-line scrollback buffer per pane** — `src/pane/mod.rs:107` and `:210` both construct the vt100 emulator as `vt100::Parser::new(rows, cols, 10_000)`. That's not configurable, but 10k lines is a lot of Claude turns (a verbose Claude turn is maybe 80-120 lines including tool calls, so the buffer holds ~80-100 of them).

**A dedicated scroll-mode pager** — `^a v` (`Action::PaneScrollEnter`, bound in `src/keymap/resolver.rs:201` and `:992`) is the user-facing entry point. The action handler is `open_pane_scroll_pager` at `src/app/mod.rs:6520-6580`. What it does:

1. Drains pending pty bytes through the vt100 parser (with a brief 3×10ms sleep loop to catch in-flight HUD redraws — comment at :6534-6554 explains why; reported regression was "pager doesn't include all on-screen text, e.g. Claude HUD plugin").
2. Snapshots the full vt100 buffer (scrollback + live screen) into styled lines via `crate::ui::scrollback::lines_from_scrollback` (`src/ui/scrollback.rs:50-96`), preserving colors and trimming right-edge cell padding.
3. Mounts the resulting `PagerView` at `Mount::LowerPane` with `pane_scroll = true`, line-numbers off (toggle with `l`), wrap on (long log/diff lines fold instead of truncating — explicit rationale in the comment at :6572), parked at the bottom on first render.
4. Sets `pane.is_scrolling()` so the status surfaces — divider re-color, `[SCROLL]` tag in status row (per `src/ui/help.rs:297`), active tab label uppercased — all signal you've changed modes.

Inside scroll mode you have the full pager vocabulary:
- `j`/`k`/`<Down>`/`<Up>`/`PgUp`/`PgDn`/`gg`/`G` — movement
- `/` — search, `n`/`N` — next/prev match
- `v` — visual-line mode, `y` — yank range to clipboard
- `gf` / `gF` — jump to a path reference (with line) extracted from the scrollback into the file list (Phase 1 of bidirectional path refs, `TODO.md:228-232` marks this done)
- `s` (`Action::PaneScrollSave`) — save the snapshot to a file
- `q`/`<Esc>` — exit back to live pane

**Two ancillary affordances** that bypass scroll-mode:
- `y a` — `YankScrollback` (`src/keymap/action.rs:36`) — yank the entire pane scrollback to the clipboard in one chord, no mode-switch.
- `:dump-scrollback` — writes the snapshot to a file (handler at `src/app/mod.rs:6485-6520`).

# 2. Why the live pane *feels* unscrollable

Three reasons, none of which are "the data isn't there":

**(a) No mouse capture at all.** `BUGS.md:60-71` is explicit: "spyc never calls `EnableMouseCapture` on the host terminal; `src/pane/input.rs` has no encoder for `Event::Mouse(_)`. Apps that default to `MouseEvents: true` (lazygit, htop, broot in mouse mode) look half-broken — clicks on panel headers / commit list / footer keybindings, **scroll-wheel on diffs**, all silently no-op." Wheel events go to your terminal emulator, not spyc, not the pty child.

What partially saves wheel-in-alt-screen-apps: `src/main.rs:227-252` enables DEC private mode 1007 ("alternate-screen scroll"), which tells the *outer* terminal to translate wheel events into `<Up>`/`<Down>` arrows while the child is in alt-screen. That's why the wheel sometimes seems to do something inside Claude — Claude is interpreting arrow keys, not seeing scroll events. It's not scrolling the *pane's* scrollback; it's scrolling whatever the alt-screen app does with arrow keys.

**(b) PgUp/PgDn in the live pane go to the child, not spyc.** spyc forwards key events to the focused pty. The pane's vt100 buffer isn't a UI element you can move a cursor in — it's an emulator state. To "scroll back" you have to switch to spyc's interpretation of that buffer, which is what `^a v` does.

**(c) The mode-switch chord is two keypresses (`^a v`) and lives behind a meta-key prefix.** No equivalent of a regular terminal's "press PgUp and you see history." From cold start: nothing in the live pane gives an immediate visual hint that there's an entire chord-prefix subsystem at `^a`. The `?` help overlay covers it, but a new user who's never typed `?` will miss it.

# 3. Existing plans / comments on scrollability

Several entries in the project docs already touch this; none are a full fix.

- `BUGS.md:27` — **already filed** as a UX issue: *"it's very confusing still to remember you're in scroll mode in the bottom half — we need a stronger top line/bottom line marker for this."* Tracks the post-entry confusion; doesn't address discoverability of the entry point itself.
- `BUGS.md:60-71` — mouse forwarding gap. Has a sketched two-layer fix ("enable mouse capture on the host terminal *and* add the `Event::Mouse` arm in `pane::input::encode_key` to encode SGR mouse reports"). Marked with the caveat "Worth designing carefully because spyc itself doesn't want mouse events outside the pane — the right shape is 'forward to pane only when pane is focused.'" Not started.
- `BUGS.md:99-105` — separate scrollback corruption issue: Claude's progress bars / spinners can leave rendering artifacts in scrollback that `^L` doesn't clean. "Solution t.b.d."
- `ROADMAP.md:158` — "Alt-screen scroll-mode hint + `[pane] default_command` config" — proposes a hint surface for scroll-mode entry.
- `TODO.md:237-240` — "Session forking — `^W f`" (medium effort, gated behind two MCP items): *"Duplicate pane tab with scrollback replayed. Old roadmap item, more valuable after the two items above land."* Tangential — session-save-with-history, not pane scrollability per se.
- `TODO.md:326` / `ROADMAP.md:518-521` — "Page scroll overlap in pager" (keep 2-3 lines of previous page visible). Refinement to the pager that scroll-mode uses; would improve `^a v` reading.
- `TODO.md:327` / `ROADMAP.md:520` — "Auto-scroll reading mode." Refinement.

So: the team has noticed the *post-entry* UX (the [SCROLL] indicator isn't loud enough) and the *pre-entry* gap (mouse forwarding entirely missing) — but the *thing in between* — a smooth, discoverable scroll affordance from the live pane — isn't on the open list.

# 4. Minimal-effort fixes (not a feature plan, just observations)

Stuff that could be cheap wins, if Caleb wants them:

1. **Mouse-wheel → scroll-mode hand-off.** Capture mouse on the host terminal, forward most events to the child, but special-case wheel-while-focused: open `^a v` automatically and use the wheel delta to seed the initial scroll position. Bridges the muscle-memory gap without giving up the keyboard-driven scroll-mode story. Would need the two-layer mouse fix from `BUGS.md:60-71` first.
2. **`PgUp` in live pane → auto-enter scroll mode.** Cheaper. Doesn't need mouse capture. If the pane is focused and the user presses `PgUp`/`PgDn`, intercept those before they go to the child and invoke `open_pane_scroll_pager`, then immediately apply a page move. Risk: alt-screen apps that legitimately use PgUp lose it — would need a guard like `!is_alternate_screen()` (the scroll-mode entry already checks this at `src/app/mod.rs:6529-6533` and bails for alt-screen panes, so the same gate applies).
3. **Status-bar hint.** When the user lands on a pane the first time per session, flash `^a v scrolls history` for 2-3s. Pure discoverability, no behavior change.

Worth flagging that option 2 would clash with Claude itself — Claude responds to PgUp / Ctrl-PgUp for its own message-history navigation. Probably needs a modifier guard (Shift-PgUp? Alt-PgUp?) to avoid stealing chord-space.

# Summary

- The Claude pane **is scrollable**, via `^a v` → in-app pager with j/k, search, visual yank, save.
- 10,000 lines of vt100 scrollback per pane.
- Two chord-bypass affordances: `ya` yanks all scrollback to clipboard; `:dump-scrollback` writes to file.
- The "not scrollable" perception is correct *for naive scroll inputs* (mouse wheel, PgUp): spyc doesn't capture mouse, and PgUp goes to the child.
- Already-filed concerns: scroll-mode-indicator weakness (`BUGS.md:27`), mouse forwarding entirely missing (`BUGS.md:60-71`).
- No filed plan to bridge live-pane → scroll-mode with a single-key affordance. Three possible cheap wins enumerated above (PgUp-auto-enter being the smallest).

Provenance:
- `src/pane/mod.rs:107,210` — 10,000-line scrollback construction
- `src/app/mod.rs:6520-6580` — `open_pane_scroll_pager` (the `^a v` payload)
- `src/app/mod.rs:6485-6520` — `:dump-scrollback`
- `src/keymap/resolver.rs:201,992` — `^a v` binding
- `src/keymap/action.rs:36,110-111,210,242-243` — `YankScrollback`, `PaneScrollEnter`, `PaneScrollSave`
- `src/ui/scrollback.rs:1-100` — scrollback adapter (vt100 → styled lines)
- `src/main.rs:227-252` — DEC 1007 alt-screen-scroll translation
- `BUGS.md:27` (scroll-mode marker), `:60-71` (mouse forwarding), `:99-105` (Claude scrollback corruption)
- `ROADMAP.md:158` (alt-screen scroll-mode hint), `:518-521` (pager refinements)
- `TODO.md:237-240` (session forking), `:326-327` (pager refinements)

<!-- Entry-ID: 01KRN4MMPT5BCPEH62BC01ZHPA -->

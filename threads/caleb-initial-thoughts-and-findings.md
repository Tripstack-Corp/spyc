# caleb-initial-thoughts-and-findings

Topic: caleb-initial-thoughts-and-findings
Status: OPEN
Ball: Claude Code (caleb)

---

Entry: Claude Code (caleb) (scribe) [Note] - First entry — spy → spyc keymap & action-vocabulary comparison matrix @ 2026-05-15T05:38:39.223205+00:00

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

---

Entry: Claude Code (caleb) (scribe) [Note] - Jump (J) investigation + harpoon vs. marks vs. jump-history @ 2026-05-15T06:18:23.642973+00:00

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


---

Entry: Claude Code (caleb) (scribe) [Note] - Bottom-pane scrollback — is it actually scrollable? (yes, but only via ^a v) @ 2026-05-15T06:19:37.105461+00:00

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


---

Entry: Claude Code (caleb) (scribe) [Note] - Configuration reference — every user-facing knob in spyc @ 2026-05-15T07:21:46.732112+00:00

Spec: docs

tags: #reference #configuration #spycrc #keymap #mcp #env

# Frame

Inventory pass: everything a user can configure in spyc, with source pointers. Verified against the tree at `repo/spyc` by reading `src/config/{mod,dsl,default.spycrc.toml}`, `src/main.rs`, `src/mcp.rs`, `src/state/*`, `src/pane/mod.rs`, `src/keymap/{action,resolver}.rs`, plus a `grep` pass over `std::env::var` / `env!` / `option_env!` across the whole crate. Where I cite a file but not a precise line, the surface was big enough that a single line wouldn't capture it.

# 1. Config file discovery

**Two TOML files, merged** (`src/config/mod.rs:1-5,241-243`):

1. `$HOME/.spycrc.toml` — per-user defaults
2. `<cwd>/.spycrc.toml` — per-project overrides (win over user)

Both optional; missing files silently fall back to built-in defaults. Project file is resolved against `cwd` at startup (not PROJECT_HOME — confirmed by reading `Config::load_default`).

**Live reload** — both files are watched via `notify` and reload on save without restart. `^R` / `Action::ReloadConfig` triggers a manual reload. (Watcher wired in `src/app/mod.rs` around the per-tick `tick_config_watcher` path.)

**Bootstrapping** — `spyc --print-config` dumps a fully-commented `default.spycrc.toml` to stdout (`src/main.rs:71-73, 101-104`). The template is the very file at `src/config/default.spycrc.toml`, baked in via `include_str!` at `src/config/mod.rs:17`.

# 2. `.spycrc.toml` sections — what each one controls

The authoritative inventory is `src/config/default.spycrc.toml` (94 lines, fully commented). Sections, all read directly from that file:

| Section | Key(s) | Default | What it does |
|---|---|---|---|
| `[layout]` | `status_position` | `"top"` | `"top"` (stock) or `"bottom"` (vim/tmux-style, prompt sits one row above status). |
| `[pane]` | `default_command` | `"claude"` | Pre-fill for `^a c` (new pane tab) prompt. `$SPYC_PANE_CMD` env wins over this (precedence noted at `src/config/mod.rs:93` and ui/help.rs:219). |
| `[yank]` | `include_pager_title` | `true` | Prepend a short header (pager title / filename / command) to yanked text. Set `false` to yank just content. |
| `[markdown]` | `open_as_rendered` | `true` | Default view for `.md`/`.markdown` files in the pager. `m` toggles in either direction; per-side scroll memory preserved. |
| `[colors]` | 13 keys (see below) | built-in palette | Hex (`"#aabbcc"`) or named colors. Anything unset falls back. |
| `[[ignore_masks]]` | array-of-tables: `group`, `enabled`, `patterns` | (built-in dotfile + build-artifact set) | **Defining any mask REPLACES the built-ins wholesale** (per the comment at `default.spycrc.toml:65`). Group 1 toggled by `a`, group 2 by `o`. Globs match filename only. |
| `keymap` | array of DSL strings | (empty — built-in bindings only) | Per-line `map <KEY> <ACTION>` (see §3). |

**Color keys** (`src/config/default.spycrc.toml:46-62`):
- File-type: `dir`, `exec`, `symlink`, `file`, `other`
- Cursor: `cursor_bg`, `cursor_fg`
- Selection: `pick`, `take`
- Status bar: `status_user`, `status_path`, `status_suffix`
- Prompt: `prompt_prefix`

Merge logic at `src/config/mod.rs:265-285` (and beyond): per-color `merge_color` overlay — project overrides user, user overrides default, all per-key. Unset values inherit; you don't have to specify the whole palette.

# 3. Keymap DSL

Parser at `src/config/dsl.rs`. Two layers:

**Key (left-hand side)** — `parse_key` (lines 76-94) and `parse_named` (lines 97-125):
- Single printable char: `f`, `;`, `H`
- Control: `^P`, `^W` (lowercased internally, stored as `Ctrl(char)`)
- Named: `<Enter>`/`<Return>`/`<CR>`, `<Space>`/`<Sp>`, `<Tab>`, `<Backspace>`/`<Bs>`, `<Esc>`/`<Escape>`, `<Up>`/`<Down>`/`<Left>`/`<Right>`, `<Home>`/`<End>`, `<PageUp>`/`<Pgup>`, `<PageDown>`/`<Pgdn>`, function keys (per the agent's read of `parse_named`)

(Chord prefixes like `g x` for `gx` work via the resolver, not the DSL — DSL binds the leaf keystroke.)

**Action (right-hand side)** — `parse_action` (`src/config/dsl.rs:127-213`). Three argument shapes:

| Shape | Example | Notes |
|---|---|---|
| Plain (no args) | `map f file` | One of the bareword actions below. |
| `=ARG` suffix | `map H patternpick =*.hpp` | Used by `ignoretoggle =N`, `patternpick =GLOB`, `jump =PATH`. |
| Trailing free text | `map ^P unix ps -ef` | Only `unix` — rest of line is the template; `%` expands to current selection at runtime. |

**Bareword plain actions accepted by the DSL** (`src/config/dsl.rs:133-210`): `quit`, `redraw`, `help`/`keys`, `up`/`previous`, `down`/`nextfile`, `left`, `right`, `pageup`, `pagedown`, `home`, `climb`, `enter`/`edit`, `display`, `pick`, `unpick`, `take`, `drop`, `inventory`, `empty`, `search`, `next`, `startshell`, `unix_cmd`, `foreground_cmd`, `longlist`, `file`, `copy`, `move`, `remove`, `makedirs`, `panescroll`, `panesave`, `togglepane`.

That is the **complete set of bindable actions via the DSL** — narrower than the full `Action` enum in `src/keymap/action.rs`. Things like `HarpoonAppend`, `SetMark(_)`, `JumpMark(_)`, `PaneTabByIndex(_)`, `Yank*`, `Goto{File,FileLine}`, `WorktreeList`, `GitDiff*` etc. exist as actions but currently have **no DSL spelling** — they're reachable only via the built-in bindings. Worth flagging if user keymap customization grows.

**Special** — `unmap` is parsed but currently a no-op (`src/config/dsl.rs:53`: `"unmap" => Ok(None), // TODO: represent unbind.`). So you can override built-in bindings by re-mapping the key to a different action, but you can't *remove* a built-in binding cleanly.

**Comments / blank lines** — `#`-prefixed and empty lines are skipped (per `parse` body and the `default.spycrc.toml` keymap example).

# 4. Command-line flags

`Cli` struct in `src/main.rs:42-73` (clap-derived):

| Flag | Short | Effect |
|---|---|---|
| `--resume` | `-r` | Open the pane immediately with `claude --resume` at startup. |
| `--debug` | `-d` | Write debug log to `/tmp/spyc-debug-<ts>.log` (equivalent to `SPYC_DEBUG=1`). |
| `--key-trace` | — | Trace every key event + dispatch decision to `/tmp/spyc-key-trace-<ts>.log` (equivalent to `SPYC_KEY_TRACE=1`). Used to diagnose input-lag reports. |
| `--mcp` | — | Run as stdio MCP server (JSON-RPC on stdin/stdout). Spyc-as-spawned-by-Claude. |
| `--verbose` | — | With `--version`, print extended build info (git sha, build time, rustc version, TERM, COLORTERM, OS). |
| `--print-config` | — | Dump fully-commented default `.spycrc.toml` template to stdout and exit. Standard bootstrap: `spyc --print-config > ~/.spycrc.toml`. |
| `--version` / `--help` | `-V` / `-h` | Standard clap. |

# 5. Environment variables — read at runtime

Verified by `grep std::env::var src/` over the whole crate.

**Spyc-specific**:

| Var | Where read | Purpose |
|---|---|---|
| `SPYC_PANE_CMD` | `src/app/mod.rs:4642, 6390` | Default pane command — **wins over `[pane] default_command`** in `.spycrc.toml`. |
| `SPYC_CONTEXT` | `src/context.rs:36` (constant), consumed by `src/mcp.rs:111` and set on every pane spawn (`src/pane/mod.rs:90`) | Path to the JSON context-snapshot file written by spyc for child processes (pane MCP server etc.). |
| `SPYC_MCP_SOCK` | `src/mcp.rs:132` | Unix socket path the stdio MCP server connects to. Usually set in the project's `.mcp.json` so Claude finds the right spyc. |
| `SPYC_DEBUG` | `src/debug_log.rs:32` | Same as `--debug`. Off if unset/empty/`0`/`false`. |
| `SPYC_KEY_TRACE` | `src/key_trace.rs:28` | Same as `--key-trace`. Off if unset/empty/`0`/`false`. |
| `SPYC_PTY_DEBUG` | `src/pane/mod.rs:105` | Enable PTY-layer kernel debug dump for spawned panes. Presence-only check (any value). |

**Standard / POSIX**:

| Var | Where read | Purpose |
|---|---|---|
| `HOME` | `src/paths.rs:17,41,…`, `src/state/sessions.rs:176`, `src/config/mod.rs:354`, `src/mcp.rs:49` | `~` expansion, config/state directory resolution. |
| `XDG_STATE_HOME` | `src/state/mod.rs:52` | If set, becomes `…/spyc` for state files; else falls back to `$HOME/.local/state/spyc`. |
| `EDITOR` / `VISUAL` | `src/shell/mod.rs:18-21` | `$VISUAL` first, then `$EDITOR`, then `vi`. Used by `v` and `EditInPane`. |
| `PAGER` | `src/shell/mod.rs:32` | Used by `D` and `DisplayInPane`. Falls back to `less`. |
| `SHELL` | `src/shell/mod.rs:62`, `src/app/state.rs:863` | Used by `$` (start shell), `!`-shell-out spawns. Falls back to `/bin/sh`. |
| `TERM` | `src/main.rs:115` (display only); pane spawns hard-coded to `xterm-256color` | Read for `--verbose`; **not** read for behavior. |
| `COLORTERM` | `src/main.rs:118` (display only); pane spawns force-set to `truecolor` (`src/pane/mod.rs:91`) | Read for `--verbose`; **not** read for behavior. |
| `TMUX` | `src/term_title.rs:17` | Presence-only — affects terminal-title push/pop behavior when nested in tmux. |

**Compile-time** (`env!()` in `src/main.rs:111-114`, `src/mcp.rs:29`): `CARGO_PKG_VERSION`, `SPYC_GIT_SHA`, `SPYC_BUILD_TIME`, `SPYC_RUSTC_VERSION`. Set by `build.rs`. Not user-settable at runtime.

# 6. Pane / bottom-pane configuration

Knobs the user controls:

| Surface | Mechanism | Source |
|---|---|---|
| Default new-tab command | `[pane] default_command` in `.spycrc.toml` OR `$SPYC_PANE_CMD` (env wins) | `src/app/mod.rs:4642, 6390`; `default.spycrc.toml:20-25` |
| Pane height % | `^W +` / `^W -` (resize), `^W z` (zoom) — runtime only | `src/keymap/action.rs` `PaneGrow`/`PaneShrink`/`TogglePaneZoom` |
| Pane focus | `^W j` / `^W k` (also `^a j` / `^a k`) | `src/keymap/action.rs` `PaneFocus{Down,Up}` |
| New tab / close / rename / restart | `^a c`, `^a K`/`^a x`, `^a r`, `^a R` | `Action::PaneNewTab`/`PaneCloseTab`/`PaneRenameTab`/`PaneRestartTab` |
| Switch tabs | `^a 1`..`^a 9`, `^a n`/`^a p` | `Action::PaneTabByIndex(n)`, `PaneNextTab`/`PanePrevTab` |
| Scroll mode | `^a v` (enter), `q`/`<Esc>` (exit), `s` (save while in scroll mode) | `Action::PaneScrollEnter`/`PaneScrollSave` |
| Send selection / pipe content / pipe inventory | `^a s`, `^a P`, `^a i` | `Action::PaneSendSelection`/`PanePipeContent`/`PanePipeInventory` |

**Hard-coded** (not user-configurable):
- Scrollback depth: **10,000 lines** (`src/pane/mod.rs:107, 210` — `vt100::Parser::new(rows, cols, 10_000)`).
- Pane `TERM`: `xterm-256color` (`src/pane/mod.rs`).
- Pane `COLORTERM`: `truecolor` (`src/pane/mod.rs:91`).
- Pane height % is persisted *into the session* but isn't an `.spycrc.toml` key — set by gesture, restored on session load.

# 7. Masks / filters

**Default behavior** (no user masks defined):
- Group 1 (toggled by `a`): dotfiles (`.*`)
- Group 2 (toggled by `o`): build artifacts (`*.o`, `target/`, `node_modules/`, `*.pyc` — verbatim from `default.spycrc.toml:74-77`)

**User override** — `[[ignore_masks]]` array. Each entry:
- `group: u8` — `1` or `2` (which toggle key)
- `enabled: bool` — start hidden at launch
- `patterns: [String]` — filename globs

**Important semantic** (highlighted in the default config at line 65): "Defining any mask here REPLACES the built-ins wholesale." Not additive. If you want a custom group 1 *and* still hide dotfiles, you have to repeat the dotfile pattern.

Mask source state lives in `src/state/ignore.rs` (module exists; not re-read here in detail).

# 8. Colors / theme

Configurable per §2 ([colors] table). No themes per se — just a flat 13-key palette overlay.

**Not user-configurable**:
- Line numbers in the pager (toggled at runtime with `l`).
- Gutter style (two-character git-status markers — hard-coded format).
- Status bar layout (sections, separators, what's displayed — hard-coded).
- The pre-renderable `?` help overlay style.

**Runtime palette flip** — `C` (`Action::ColorToggle`) flips between primary and inverted (light/dark-aware) palettes. Configurable colors apply to whichever variant is active.

# 9. MCP server — what spyc exposes

Two entry points (`src/mcp.rs`):

1. **In-process socket server** — spyc itself listens on a Unix socket at `$XDG_STATE_HOME/spyc/mcp-<pid>.sock` while running.
2. **Stdio MCP server** — `spyc --mcp` (`src/main.rs:106-109`) spawns a thin proxy that reads JSON-RPC from stdin and bridges to the socket. This is what Claude Code etc. actually launch.

**Discovery** — the stdio bridge finds the right socket via `$SPYC_MCP_SOCK` (`src/mcp.rs:132`) or by walking project markers (`.spyc-context-<pid>.json`) in the project tree.

**Resource exposed** (`src/mcp.rs:29, 921-936`):
- URI: `spyc://context`
- Content: JSON snapshot of spyc's live state (cwd, cursor file, picks, inventory, filter, git branch, project_home, session_name). Source-of-truth is the file at `$SPYC_CONTEXT`.

**Tools exposed** (`src/mcp.rs:970-1120` — verified by direct read of the `"name":` keys):

| Tool | Writes? | Behavior |
|---|---|---|
| `get_spyc_context` | no | Return the same JSON the `spyc://context` resource serves. |
| `navigate_to` | **yes** | Jump file list to a path. Dir → chdir; file → parent + cursor. |
| `set_filter` | **yes** | Set or clear the glob filter (`null` clears). |
| `pick_files` | **yes** | Select files matching glob patterns (additive). |
| `clear_picks` | **yes** | Clear all picks. |
| `get_file_content` | no | Read file (size + text-only guarded). |
| `search_paths` | no | Fuzzy filename search over PROJECT_HOME / cwd (gitignore-aware). |
| `search_content` | no | Regex search across project content (ripgrep matcher, smart-case). |
| `search_picks` | no | Regex search confined to current picks. |
| `search_inventory` | no | Regex search confined to the persistent inventory. |

**User-extensible?** No. Tools are hard-coded in `src/mcp.rs`. Read-only tools are handled inline; writable tools route through a `McpCommand` mpsc channel back to the main event loop (`src/mcp_cmd.rs`). To add a tool you'd need to ship a code change.

**Lifecycle** — at startup spyc checks whether another spyc on this machine already owns MCP for this project; if so it prompts to take over (`src/main.rs:159-…` `prompt_mcp_takeover_if_needed`). Enterprise MCP setups skip both takeover and `.mcp.json` writing (`mcp::enterprise_defines_spyc()`).

# 10. State files — every persistent surface spyc writes

State root: `$XDG_STATE_HOME/spyc` or `$HOME/.local/state/spyc` (`src/state/mod.rs:52-56`).

| File | Module | Format | Cap / lifecycle |
|---|---|---|---|
| `sessions/<id>.json` | `src/state/sessions.rs` | JSON | Max 20 retained, deduped by `(cwd, tab_commands)`, oldest pruned. Restored on startup via picker. |
| `harpoon/<basename>.<hash>.toml` | `src/state/harpoon.rs` | TOML | Per project (keyed by `PROJECT_HOME` hash), 9 slots max, auto-saved on mutation. |
| `marks.toml` | `src/state/marks.rs` | TOML | Global, letter-keyed (a-z), each entry stores dir + optional focus file. |
| `history` | `src/state/history.rs` | plain text, one cmd/line | Shell `!`/`;` history, 1000 max, move-to-end dedup. |
| `pane_history` | `src/state/history.rs` (via `load_file`) | plain text | New-pane-tab command history. Same shape as `history`. |
| `jump_history` | `src/state/history.rs` (via `load_file`) | plain text | J-prompt destinations. 1000 max, move-to-end dedup. (Per the J-investigation entry earlier in this thread.) |
| `command_history` | `src/state/history.rs` (via `load_file`) | plain text | `:` (vim-style) command history. Same shape. |
| `frecency.json` | `src/state/frecency.rs` | JSON | 500 max, count × tiered-recency score. Recorded on every chdir; surfaced via Tab fallback at J. |
| `graveyard/<uuid>.json` + `<uuid>.tar.zst` | `src/state/graveyard.rs` | JSON + zstd-tar | Soft-delete buffer. Cascades to system trash when total exceeds ~500 MB. |
| `mcp-<pid>.sock` | `src/mcp.rs` | Unix socket | Live only while spyc is running. |

# 11. Session persistence

`Session` struct in `src/state/sessions.rs` captures, on save:

- `cwd`, `start_dir`, `project_home`
- Pane layout: `pane_height_pct`, `pane_focused`, `active_tab`, `tabs: Vec<SavedTab>`
- Each `SavedTab`: command, label, cwd, agent kind (Claude/Codex/Gemini/Other), agent_session_id (resume token), spawn epoch
- A spice-pair session name (e.g. `SAFFRON_PAPRIKA`) for human picker labels
- Timestamps: `saved_at` (ISO), `epoch_secs`

Restore is conversation-aware: Codex resumes via `codex resume <uuid>`; Claude resumes by spawning fresh then typing `/resume <sid>` (because the `--resume` flag has a current crash on non-empty initial messages — explicit comment at `src/app/mod.rs` around the restore path).

# 12. Project context / PROJECT_HOME

- **Auto-detected** at startup if `cwd/.git` exists → `project_home = cwd`. Otherwise unset.
- **Manual** via `gP` (`Action::SetProjectHomeHere`) or `:project [.|<path>|clear]`.
- **Jump to it** via `gh` (`Action::JumpProjectHome`).
- **Persisted** in the session (`Session.project_home`).
- **Distinct from** `start_dir` (target of backtick `` ` ``), which is settable separately via `gS` (`Action::SetStartDirHere`). Two separate one-slot pointers.

PROJECT_HOME also scopes:
- Harpoon file selection (one harpoon list per project).
- MCP context disambiguation when multiple spyc instances run on overlapping trees.

# 13. Auto-approval

Mentioned in `docs/AUTO_APPROVAL_PLAN.md` (planning doc, not yet a runtime surface). When it lands, expect `.spycrc.toml` to grow a section for it — not present today.

# 14. Diagnostic / observability surfaces

| What | Toggle | Output |
|---|---|---|
| Debug log | `-d` / `SPYC_DEBUG=1` | `/tmp/spyc-debug-<ts>.log` |
| Key trace | `--key-trace` / `SPYC_KEY_TRACE=1` | `/tmp/spyc-key-trace-<ts>.log` |
| PTY debug | `SPYC_PTY_DEBUG=anything` | Pane debug dump for spawned subprocess |
| In-app memory stats | `I` (`Action::ShowMemory`) | Pager with pid, rss, file/picks/inventory counts |
| In-app health view | runtime — invoked from app/state | `src/state/health.rs` |

No telemetry. No network reporting. Everything stays local.

# 15. What is *not* user-configurable (compile-time)

For the curious:

- Scrollback per pane: 10,000 lines (`src/pane/mod.rs:107,210`)
- Harpoon slots: 9 (`src/state/harpoon.rs:40` per agent investigation)
- History caps: 1000 entries (`src/state/history.rs:9`)
- Frecency cap: 500 entries (`src/state/frecency.rs:12`)
- Sessions cap: 20 (`src/state/sessions.rs:11`)
- Pane `TERM` / `COLORTERM` (hard-coded in `pane/mod.rs`)
- Resolver chord shapes — `^a`, `^w`, `g`, `]`, `[`, `y`, `H`, `m`, `'`, `=` prefixes are baked into `src/keymap/resolver.rs`. Custom chord prefixes would need code changes.
- The set of action *names* the DSL accepts (§3) — narrower than the full `Action` enum.

# 16. The shortest "bootstrap your config" path

```
spyc --print-config > ~/.spycrc.toml
# edit
```

Edits take effect the moment you save (watcher), or you can press `^R` to force a reload.

# Provenance

- `src/config/default.spycrc.toml` (full read, 94 lines)
- `src/config/mod.rs` — discovery (`load_default`, `:241-243`), `DEFAULT_TEMPLATE` (`:17`), per-color merge (`:265-285`)
- `src/config/dsl.rs` — full parser, key (`parse_key`, `parse_named`), action (`parse_action`, `:127-213`)
- `src/main.rs` — `Cli` struct (`:42-73`), startup (`:99-148`)
- `src/mcp.rs` — server name/version (`:29`), context resource (`:921-936`), tool definitions (`:970-1120`), socket discovery (`:111,132`)
- `src/state/{mod,sessions,harpoon,marks,history,frecency,graveyard,ignore}.rs` — state files and caps
- `src/pane/mod.rs` — pane spawn env (`:74-92`), scrollback (`:107,210`)
- `src/app/mod.rs` — `SPYC_PANE_CMD` precedence (`:4642, 6390`)
- `src/context.rs` — `CONTEXT_ENV_VAR` constant (`:36`)
- `src/debug_log.rs:32`, `src/key_trace.rs:28`, `src/shell/mod.rs:18-62`, `src/term_title.rs:17`, `src/ui/help.rs:219`

Identity fallback: no `set_agent` tool surfaced this session; identity asserted via `agent_func`, Role, and visible Spec line.


---

Entry: Claude Code (caleb) (implementer) [Note] - Pane-toggle context loss + pane/session config reference @ 2026-05-15T07:51:06.718747+00:00

Spec: observer

Caleb asked four things:
1. When the lower pane is hidden via `F10` / `^a-\` and unhidden, is the claude session lost? Is there a config option / mechanism to retain it?
2. Are lower-pane size and count configurable?
3. Can different startup commands be set per pane?
4. Is current session state persisted across restart?

## 1) Pane-toggle kills the claude process. Yes, context is lost.

`F10` and `^a \` both resolve to `Action::TogglePane` (`src/keymap/resolver.rs:144`, `src/keymap/resolver.rs:294`, `src/app/mod.rs:9595`). The handler is `App::toggle_pane` at `src/app/mod.rs:4632`:

```rust
fn toggle_pane(&mut self) {
    if self.pane_tabs.is_some() {
        self.pane_tabs = None;          // <-- drops every tab
        self.state.pane_focused = false;
        self.state.pane_zoomed = false;
        ...
        self.state.flash_info("pane closed");
        return;
    }
    let cmd = std::env::var("SPYC_PANE_CMD").unwrap_or_else(|_| "claude".to_string());
    self.open_pane_tab(&cmd);           // <-- fresh spawn on re-toggle
}
```

`pane_tabs = None` drops every `TabEntry`, which drops each `Pane`, which drops its `PtyHost`. `Drop for PtyHost` at `src/pane/pty_host.rs:297` sends **SIGKILL** to the child's process group via `kill_process_group(pid, Signal::KILL)` and reaps:

```rust
impl Drop for PtyHost {
    fn drop(&mut self) {
        if self.closed { return; }
        if let Some(pid) = self.child.process_id() {
            #[cfg(unix)]
            kill_group(pid, rustix::process::Signal::KILL);
        } ...
    }
}
```

Consequences for the toggle path:
- Claude is killed mid-flight. It never prints its `Resume this session with: claude --resume <token>` exit banner, so even the scrollback-based capture in `extract_claude_resume_token` (`src/state/sessions.rs:504`) cannot recover the sid.
- `toggle_pane` does **not** call `save_session` before nulling `pane_tabs`, so no `agent_session_id` is harvested.
- On re-toggle, `open_pane_tab(&cmd)` spawns a bare `claude` (or `$SPYC_PANE_CMD`) — no resume.

**There is no config option that changes this.** The toggle is unconditionally destructive. `pane.default_command` (`src/config/mod.rs:96`) only controls *which* command is spawned, not whether the prior tab is retained.

The closest existing path is `F11` / `Action::ResumePane` (`src/keymap/action.rs:103`, `src/app/mod.rs:9304`), which calls `open_pane_tab("claude --resume")`. That uses claude's CLI `--resume` flag (the codebase has comments noting it is regression-prone with non-empty initialMessages — `src/app/mod.rs:1436`, `src/state/sessions.rs:15`). It surfaces claude's built-in picker for the cwd, not the specific sid that was running before the toggle.

## 2) Lower-pane size: runtime-tweakable, NOT in spycrc

- Stored as `state.pane_height_pct: u16` (`src/app/state.rs:166`), default `30`.
- Runtime: `^a +` / `^a -` → `Action::PaneGrow` / `PaneShrink` → `resize_pane(±5)` (`src/app/mod.rs:9308-9309`, `src/keymap/action.rs:107-108`).
- Zoom: `^a z` / `^w z` → `Action::TogglePaneZoom` → fullscreens (effective_pane_pct = 100). Doesn't kill — just resizes the existing pty (`src/app/mod.rs:6812`, line 6842 calls `entry.pane.resize(...)`).
- Persisted: `Session.pane_height_pct` saved/restored (`src/state/sessions.rs:79`, `src/app/mod.rs:7705`, `src/app/mod.rs:7934`).
- **Not exposed** in `~/.spycrc.toml` — `[pane]` only has `default_command`. To pin a startup size you'd have to add it.

## 3) Pane count + per-pane startup commands

- **Only one bottom-pane region.** No tmux-style horizontal/vertical splits. Stored as `pane_tabs: Option<PaneTabs>` (`src/app/mod.rs:528`).
- **Multiple tabs** in that one region: `^a c` / `^W n` → `Action::PaneNewTab` (`src/keymap/action.rs:112`), `^W ]` / `^W [` switch, `^W 1..9` jump by index, `^W x` close, `^W r` rename, `^W R` restart. Each `TabEntry` carries its own command, cwd, label.
- **At startup**, only one tab is opened — using `pane.default_command` from spycrc (or `$SPYC_PANE_CMD`, or `"claude"` fallback). The codebase doesn't read a `[[pane.tabs]]` array. The only way to start with multiple tabs is to launch with `spyc -r` and pick a saved session that had multiple tabs.

So per-pane startup commands per *tab* are not directly configurable — they come from saved sessions, not config.

## 4) Cross-restart session persistence: yes, fairly complete

State root is `$XDG_STATE_HOME/spyc/` (or `~/.local/state/spyc/`) — `src/state/mod.rs:48`.

Per-instance session snapshot (`src/state/sessions.rs`):
- File: `$XDG_STATE_HOME/spyc/sessions/<epoch_ms>.json`, cap 20, deduped by `cwd|tab_cmds`.
- Written on quit by `App::save_session` (`src/app/mod.rs:7629` → `src/app/mod.rs:9423`).
- Restored by `App::restore_session` (`src/app/mod.rs:7913`) when the user picks one from `show_session_picker`, triggered by the `-r`/`--resume` CLI flag at `src/app/mod.rs:875`.
- Fields: `cwd`, `tabs[]` (each: `command`, `label`, `cwd`, `agent_kind`, `agent_session_id`, `agent_session_name`), `active_tab`, `pane_height_pct`, `pane_focused`, `name` (spice-pair), `project_home`.
- Claude tabs: sid is captured either by scanning recent scrollback for `claude --resume <token>` or by reading the most-recent matching `~/.claude/projects/<encoded-cwd>/*.jsonl`. On restore, the tab is spawned fresh and `entry.info.pending_resume_send = Some((sid, now))` queues `/resume <sid>\r` to be typed once the pty has settled (`src/app/mod.rs:7982-7990`, `src/app/mod.rs:5768`). The CLI `--resume` flag is *not* used for Claude precisely because of the regression.
- Codex tabs: `codex resume <UUID>` baked into the spawn command directly.
- Gemini tabs: best-effort `--resume <index>` after a `--list-sessions` lookup.

Other persisted state (each module owns its own file under the state root):
- `history` files — pane/jump/command/shell histories (`src/state/history.rs:3`).
- `frecency.json` — chdir frecency (`src/state/frecency.rs:4`).
- `harpoon/<basename>.<hash>.toml` — per-project harpoon list (`src/state/harpoon.rs:20`).
- `marks` — global marks (`src/state/marks.rs:72`).
- `graveyard/` — deletion history with pairs (`src/state/graveyard.rs:24`).
- `inventory` (`src/state/inventory.rs:45`).
- `health` (`src/state/health.rs:175`).

These load lazily at use-site, so a fresh `spyc` launch (no `-r`) still recovers history/frecency/harpoon/marks/graveyard — only the *workspace layout* (which tabs were open, their cmds/cwds/sids) is gated behind the session picker.

## TL;DR for caleb

| Question | Answer |
|---|---|
| Is claude context lost on pane toggle? | Yes — SIGKILL via `Drop for PtyHost`. |
| Config to retain it? | None today. |
| Mechanism that exists? | Cross-restart resume via `save_session` / `restore_session` (sid → `/resume`), but not wired into the toggle path. |
| Lower-pane size configurable? | Runtime (`^a +/-`), per-session, but no spycrc knob. |
| Number of panes configurable? | Multi-tab in one region (no splits); startup is single-tab only via `pane.default_command`. |
| Different startup commands per pane? | Not via config; only via saved sessions. |
| Session state persisted across restart? | Yes — sessions JSON + history/frecency/harpoon/marks/graveyard files under `$XDG_STATE_HOME/spyc`. Workspace layout requires `spyc -r` to surface the picker. |

Filed sibling plan thread `feature-pane-toggle-preserve-context` with options for fixing #1.

tags: #observations #pane #toggle #pty #sessions #configuration #spy-parity


---

Entry: Claude Code (caleb) (scribe) [Note] - Corrected: spyc state by scope — keystrokes × persistence × MCP write-access (one-pager) @ 2026-05-18T07:09:36.831730+00:00

Spec: docs

**Supersedes the prior entry** (`01KRWWJM94E6PZBSNVF0T9TF3Y`). Every row in this version was re-verified against `src/keymap/{action,resolver}.rs`, `src/state/*.rs`, `src/app/state.rs`, `src/state/ignore.rs`, and `src/ui/help.rs`.

**What was wrong before:**

- Mask toggles row claimed `i` toggled dotfiles. `i` actually toggles **inventory view**. The dotfile mask toggle is `a`. (resolver.rs:413 → `ToggleInventoryView`; resolver.rs:423 → `ToggleMask(1)`; help.rs:102 confirms `a` = "toggle mask 1 (dotfiles by default)".)
- Graveyard row had `R` as the restore chord. `R` is unbound in the top-pane; restore is `:undo` (most-recent), or — inside the `gy` graveyard view — `p` (to cwd) / `P` (to original path). (help.rs:88-97.)
- `J` was described as "frecency-ranked"; frecency is actually a **fallback completion source** when filesystem completion misses (app/mod.rs:4143). The prompt itself is a path-entry with command-history recall.
- Yank-prefix sub-actions were under-specified: `yp` is yank-pane-output, `yP` is yank-last-prompt, `ya` is yank-pane-scrollback (help.rs:69-71).

## Table

| Feature | Keys / commands | Scope | Persistence | MCP-managed? |
|---|---|---|---|---|
| **Picks** (multi-select) | `t` toggle · `T` glob · `^T` all/clear · `=!` filter to picks | **Per-directory**, in-memory | None — lost on quit | ✓ `pick_files`, `clear_picks`, `search_picks` (R/W) |
| **Limit filter** | `=<glob>` · `=!` picks · `=h` harpoon · `=g` / `=git` git-changed · `=` clears | **Per-session** (in-memory `temp_filter: Option<String>`) | None | ✓ `set_filter` (R/W) |
| **Ignore masks** | `a` toggle mask 1 (dotfiles) · `o` toggle mask 2 (build artifacts) | **Per-session** | Defaults in config; toggles in-memory | ✗ |
| **Project Home** | `gP` set to cwd · `gh` jump to it · `:project [.\|<path>\|clear]` | **Per-session**, auto-detected from `.git` at startup | Saved with session (`spyc -r`); else session-only | Exposed read-only via `get_spyc_context` |
| **Cursor position / view-top** | `j` `k` `gg` `G` `^d` `^u` etc. | **Per-listing**, in-memory | Never persisted across chdir or restart | Cursor file exposed by `get_spyc_context` |
| **Inventory** (file-ops staging) | `yy` take · `Y` untake (remove from inventory) · `p` put · `yf` yank path · `i` toggle inventory view · `z` clear (to graveyard) | **Per-user / global** | `$XDG_STATE_HOME/spyc/inventory/<id>.{json,dat}` | ✓ `search_inventory` (read only) |
| **Yank-to-clipboard family** | `yf` cursor/picks path · `yp` pane output · `yP` last typed prompt · `ya` full scrollback | (operations, not state) | n/a | ✗ |
| **Marks** (named bookmarks) | `m{a-z}` set · `'{a-z}` jump · `''` last dir · `` ` `` start dir | **Per-user / global** (single 26-slot namespace, *not* project-scoped) | `$XDG_STATE_HOME/spyc/marks.toml` | ✗ |
| **Harpoon** (pinned working set) | `Ha` append · `H1`..`H9` jump · `Hx` remove · `Hh` menu · `=h` filter | **Per-project** (keyed by hash of `PROJECT_HOME`) | `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml` | ✗ |
| **Sessions** | `spyc -r` / `--resume` CLI (tabs, cwd, pane, project_home etc.) | **Per-user / global** | `$XDG_STATE_HOME/spyc/sessions/*.json` | ✗ |
| **Frecency** (J fallback completion) | `J` path prompt (`~` / `$VAR` expanded; history via `↑`/`↓`; frecency completes when filesystem completion misses) | **Per-user / global** (cross-project directory score) | `$XDG_STATE_HOME/spyc/frecency.json` | ✗ |
| **Graveyard** (undo for deletes) | `gy` open view · `:undo` restore most-recent · in view: `p` restore to cwd, `P` restore to original, `dd`/`x` purge, `Z` purge all | **Per-user / global** | `$XDG_STATE_HOME/spyc/graveyard/` (archives) | ✗ |
| **Worktrees** | `W l` list worktrees (pick to chdir) | **Per-project**, lives in git, not spyc | git metadata | ✗ |
| **Git status overlay** | passive · `=g` / `=git` filter · `]g` / `[g` next/prev changed | **Per-directory**, refreshed live | None — recomputed from `git status` | ✗ |
| **Command-line / shell history** | `:` `!` `;` prompts; `↑` / `↓` to browse | **Per-user / global** | History file under state dir, deduped | ✗ |
| **Per-user config** | `~/.spycrc.toml` — keybindings, theme, masks, layout | **Per-user / global** | TOML file, live-reload on change | ✗ |
| **Per-project config** | `<project>/.spycrc.toml` (overrides user config) | **Per-project** | TOML file in project root | ✗ |
| **MCP socket** | implicit — Claude integration | **Per-session** (PID-scoped) | `.spyc-context-<pid>.json` in project root, atomic writes | All `mcp__spyc__*` tools read/write this surface |
| **cwd** | every chdir | **Per-session** | Not persisted (but recoverable via session restore) | ✓ `navigate_to` (R/W) |

`gh` = jump to PROJECT_HOME. `Hh` = open harpoon overlay menu. Different chords.

## Scope notes worth knowing

- **Single global state root:** `$XDG_STATE_HOME/spyc` (or `~/.local/state/spyc`) holds everything persistent. All disk-backed features live as subdirs/files under it. No project-local state dir; project-scoping is done by keying *into* global files (harpoon hashes the project path; marks don't scope at all).
- **Marks vs harpoon, the key asymmetry:** marks are a single 26-slot global namespace — `ma` set in project A and project B *collide* (last write wins). Harpoon is the project-scoped equivalent: each project has its own independent 9-slot list. Choose accordingly — marks for "the doc I always want", harpoon for "the four files in *this* project I'm cycling between".
- **Picks vs inventory:** both are multi-select-like, but picks are *per-directory, ephemeral, references to live paths* and inventory is *global, persistent, cached file copies*. `t` adds a pick; `yy` (take) lifts the cursor or all picks *into* inventory.
- **MCP write surface is narrow:** Claude can only mutate picks, the limit filter, and cwd. Marks, harpoon, sessions, project_home, mask toggles, and the config files are **user-only** today — the agent can read them (via `get_spyc_context`) but not set them. That asymmetry is intentional per AGENTS.md / SECURITY.md.
- **Per-session vs per-listing:** `temp_filter` is per-session (survives chdir — it's a `String` on `AppState`, not tied to a dir). Picks reset implicitly because they're keyed by absolute path and the listing changes. Cursor position resets at every chdir.
- **Auto-detection at startup:** `project_home` is auto-set to the nearest `.git` ancestor of cwd; `XDG_STATE_HOME` defaults to `~/.local/state` if unset; config files are sought at both `~/.spycrc.toml` and `<project>/.spycrc.toml` with project winning.
- **`spyc -r` is the only "save my state" gesture** — there is no `:w` or autosave. Everything not in a state file (picks, filter, mask toggles, cursor position) is forfeit on quit unless captured by a session.

## Pointers (source of truth)

- `src/state/mod.rs` — `state_root()` resolution (`$XDG_STATE_HOME/spyc` → `~/.local/state/spyc`).
- `src/state/{picks,marks,harpoon,inventory,sessions,frecency,graveyard,history}.rs` — one module per persistent feature.
- `src/state/ignore.rs:27-40` — `IgnoreMasks { mask1, mask2 }` with mask1 = `.*` (dotfiles), mask2 = build files.
- `src/app/state.rs:570-605` — `apply_temp_filter` (the actual filter modes: `"!"`, `"h"`, `"git"`, glob).
- `src/app/state.rs:1438-1454` — limit-filter prompt acceptance of `g`/`git`/`h`/`!`.
- `src/keymap/action.rs` — full action catalog with one-line chord notes.
- `src/keymap/resolver.rs:413-429` — `i` → `ToggleInventoryView`, `a` → `ToggleMask(1)`, `o` → `ToggleMask(2)`.
- `src/ui/help.rs:88-97` — graveyard chords (`:undo`, in-view `p` / `P` / `dd` / `Z`).

---

Entry: Claude Code (caleb) (scribe) [Note] - Addendum to scope one-pager: ^R reload, graveyard archive format, history file paths @ 2026-05-18T07:13:05.350126+00:00

Spec: docs

Refines (does not supersede) `01KRWYP9304BGYAR7BAN65ZEPJ`. The corrected table is materially right; this entry adds one missing chord and tightens two persistence descriptions after a second validation pass against source.

## 1. Missing chord — `^R` (ReloadConfig)

Manual config reload, complementary to the file-watcher live-reload already noted in the **Per-user config** row.

- Key: `^R` (Ctrl-R).
- Action: `Action::ReloadConfig` — re-reads `~/.spycrc.toml` + `<project>/.spycrc.toml` in the same hierarchy as startup.
- Source: `src/keymap/action.rs:99` (`ReloadConfig, // ^R — re-read ~/.spycrc.toml + project config`); `src/keymap/resolver.rs:139` (`KeyCode::Char('r' | 'R') => ResolverOutcome::Action(Action::ReloadConfig)`, inside the `if ctrl` block at resolver.rs:123).

Suggested row patch (slot into the config rows):

| **Manual config reload** | `^R` | (operation, not state) | n/a — re-reads both `.spycrc.toml` files in hierarchy | ✗ |

## 2. Tighter wording — Graveyard persistence

Previous: `$XDG_STATE_HOME/spyc/graveyard/` (archives).

Tighter: `$XDG_STATE_HOME/spyc/graveyard/<uuid>.tar.zst` (zstd-compressed tarballs; one tarball per removed entry, with `<uuid>.json` sidecar metadata).

- Source: `src/state/graveyard.rs:19,26,70-93` — explicit "tar.zst" archive shape, with `<uuid>.tar.zst` blob plus per-entry metadata struct tracking `paths_count` and compressed `size`.

## 3. Tighter wording — Command/shell history persistence

Previous: "History file under state dir, deduped."

Tighter: `$XDG_STATE_HOME/spyc/history` (plain text, one command per line) for `:` `!` `;` prompt history; **separately** `$XDG_STATE_HOME/spyc/pane_history` for pane prompt history. Dedup is **move-to-end on push** (the new entry wins, prior duplicates are removed) — not a "consecutive only" dedup.

- Source: `src/state/history.rs:1-4` (docstring); `:26` (default filename "history"); `:20` (separate `pane_history` file); `:61-66` ("Remove earlier duplicate (move-to-end dedup)") plus the `push_deduplicates_moves_to_end` test at `:201`.

## What was right and didn't change

Spot-checked the remaining rows to be sure:

- **Picks** `t` / `T` / `^T` — resolver.rs:391 (bare `t`), 397 (bare `T`), 129 (`^T` inside ctrl block). ✓
- **Sessions** filename pattern — `<epoch-ms>.json` per session (sessions.rs:5,100). ✓
- **MCP context** — `.spyc-context-<pid>.json` written to project root via atomic temp-rename (context.rs:42,51). ✓
- **Config hierarchy** — `~/.spycrc.toml` then `<cwd>/.spycrc.toml` with project winning, plus an explicit watcher tracking the loaded paths (config/mod.rs:1-5, 54, 242-243). ✓
- **Cursor position** — no persistence code in `src/state/cursor.rs` or `src/app/state.rs`; the "never persisted across chdir or restart" claim holds. ✓

The table in entry `01KRWYP9304BGYAR7BAN65ZEPJ` remains the canonical one-pager; treat this entry as three small fixes layered on top.

---

Entry: Claude Code (caleb) (scribe) [Note] - Top status bar — every segment decoded @ 2026-05-18T22:35:55.574249+00:00

Spec: docs

Decoding the top-of-pane status line, e.g.

` 🌶️   spyc  FENUGREEK_SAFFRON  ~/Downloads   claude:2c16d025  [picks:0 inv:0 m1:on m2:on hidden:1] `

| Segment | Meaning | Source |
|---|---|---|
| 🌶️ | spyc logo (pepper emoji), no background | `src/ui/status.rs:79` |
| `spyc` | `project_home` — basename of `PROJECT_HOME` (hidden if unset) | `src/ui/status.rs:13` |
| `FENUGREEK_SAFFRON` | `session_name` — random spice-pair handle | `src/ui/status.rs:15` |
| `~/Downloads` | active pane's cwd (truncated middle if narrow) | `src/ui/status.rs:16` |
| `claude:2c16d025` | `agent_info` — bottom-pane agent label + short session id | `src/ui/status.rs:21-26` |
| `[ … ]` | `suffix` — live state counters | `src/ui/status.rs:17-18` |

**Suffix internals** (built in `src/app/mod.rs:2937-2946`):

- `picks:N` — currently picked files (`state.picks.len()`)
- `inv:N` — items stashed in the inventory (`state.inventory.len()`)
- `m1:on|off` / `m2:on|off` — mask 1 / mask 2 toggles
- `hidden:N` — entries hidden by current mask/filter (`total − shown`)

Optional fragments that appear conditionally:
- ` limit:<expr>` — active temporary filter; renders as `limit:picks` when filter is `!`
- ` bg:N●` / ` bg:N●M✓` — running / done background-task counts. Only when no bottom-pane tabs are open (with tabs, this info renders in the divider above the pane instead).

**Color band semantics** (powerline mode, `src/ui/status.rs:54-73`): warm brown for project, lavender for session, indigo for path, dark-blue for git, distinct accent for agent, darker for suffix. Mono mode falls back to a plain `project session: <path>  <suffix>` layout (`src/ui/status.rs:172-214`).

tags: #reference #status-bar #ui #discoverability

---

Entry: Claude Code (caleb) (scribe) [Note] - session_name vs agent_info — two different identities in one status line @ 2026-05-18T22:36:28.651192+00:00

Spec: docs

The status bar shows two distinct identifiers that are easy to conflate. They're for completely different things.

### `session_name` — handle for the **spyc workspace session**
- A spice pair assigned at startup, e.g. `FENUGREEK_SAFFRON` (`src/state/session_names.rs:8-53`). Format: two distinct uppercase spice words separated by `_`; ~900 possible pairings, collision-safe for the per-user 20-session cap.
- User-renameable via the `:name <NEW>` command (`src/ui/help.rs:152-154`; normalization at `src/state/session_names.rs:60+`).
- Identifies one entry in spyc's saved-session list at `$XDG_STATE_HOME/spyc/sessions/<epoch-ms>.json`. The snapshot holds tab layout, per-tab cwd, `project_home`, `pane_height_pct`, `pane_focused`, `active_tab`, and per-tab agent resume tokens (`src/state/sessions.rs:71-89`).
- Also fed into the host terminal's window title — `🌶️: spyc · FENUGREEK_SAFFRON` (`src/term_title.rs:58-69`), so you can disambiguate windows in your dock / tab list.
- **TL;DR**: a memorable label so you can pick the right session row in the picker on next launch.

### `agent_info` — live label for the **active bottom-pane agent**
- Shown only when the focused bottom-pane tab is running a known coding agent (claude / codex / gemini); hidden for `bash`, `vim`, `make`, anything else (`src/app/mod.rs:2883-2905`, `AgentKind::Other` returns `None`).
- Format: `claude:<8-hex>` / `gemini:<8-hex>` / bare `codex` (the codex short-id resolution is a follow-up — its rollout filenames encode the UUID but spyc doesn't parse them yet).
- The hex is the short form of the agent's *own* resume id — claude's UUID-or-thread-name, gemini's UUID. That id is the resume token spyc uses to reattach to the same conversation across restarts.
- Drives per-agent resume mechanics on session restore — Claude `/resume <token>` over stdin (CLI flag is regression-prone), Codex `codex resume <UUID>` directly, Gemini `--resume <index>` after a `--list-sessions` lookup (`src/state/sessions.rs:13-26`, `src/app/mod.rs:8064-8092`).
- **TL;DR**: tells you which agent conversation the **active** pane tab is attached to right now, and is the token used to reattach to that same conversation after a restart.

So `session_name` names the **whole spyc window state**; `agent_info` names the **conversation inside one pane tab**. A single spyc session can host several agent tabs, each with its own `agent_info`.

tags: #reference #status-bar #session #agent-info #discoverability

---

Entry: Claude Code (caleb) (scribe) [Note] - Restarting spyc with a saved session, and reattaching the claude pane @ 2026-05-18T22:37:09.340941+00:00

Spec: docs

This supersedes the "session state not persisted across restarts" remark in entry [4] — it **is** persisted (and reattached, including the claude conversation), just only via the interactive picker. Documenting the actual workflow here.

### 1) Launching spyc and choosing a saved session

- Run `spyc -r` (or `spyc --resume`). The `-r` flag's only effect is to fire `show_session_picker()` once the app is fully constructed (`src/main.rs:46-48`, `src/app/mod.rs:877-879`).
- The picker lists every saved session with: spice-name (`(unnamed)` if blank), relative age, cwd, tab labels, and a per-tab agent summary — `claude:<name> (<short-id>)` / `codex:<short-id>` / `gemini:<short-id>` (`src/app/mod.rs:7865-7918`).
- Keys inside the picker: `j/k` navigate, `Enter` restore, `n` for new, `q` close (`src/app/mod.rs:7920-7927`).
- **There is no `spyc --session FENUGREEK_SAFFRON` flag.** The spice name is purely a visible label so you can recognize the right row. Restore is always interactive.

### 2) Does session restore also reattach the claude pane to its conversation?

Yes — automatically. `restore_session` (`src/app/mod.rs:8027-8125`) walks the saved tabs and dispatches per agent kind using each tab's stored resume token:

- **Claude**: spawn a **fresh** `claude` (the `--resume` CLI flag has a known regression that crashes at mount with non-empty `initialMessages`), then once the pty has settled, `pending_resume_send` types `/resume <sid>\r` into the pane's stdin to attach to the saved conversation (`src/app/mod.rs:8065`, `8096-8104`; comments at `src/state/sessions.rs:13-26` and `src/app/mod.rs:1438`, `5818-5825`). A watchdog (`src/app/mod.rs:311`, `5842`) auto-falls-back to a fresh `claude` if the restored tab looks broken.
- **Codex**: spawned directly as `codex resume <UUID>` (its `--resume` flag works); falls back to `codex resume --last` with no saved id (`src/app/mod.rs:8064-8075`).
- **Gemini**: spyc synchronously runs `gemini --list-sessions`, maps the saved UUID → its required positional index, then spawns `gemini --resume <N>`. Falls back to bare `gemini` if the binary isn't on `PATH` or the UUID isn't in the listing (`src/app/mod.rs:8076-8092`, `gemini_resume_index_for` at `:6152`).
- **Other** commands (`bash`, `vim`, …): re-runs the saved command line; no conversation concept (`src/app/mod.rs:8093`).

Single Enter in the picker → workspace **and** each agent pane back on its prior conversation.

### 3) Restarting / reattaching claude from inside a running spyc

Several paths, depending on intent (`src/ui/help.rs:189-214`):

- **F9** — opens a new pane tab running `claude --resume`, which surfaces claude's *own* picker for any past conversation it knows about for this cwd (`src/ui/help.rs:193`, `src/app/mod.rs:9447` → `open_pane_tab("claude --resume")`).
- **^a R** — restart the active tab's command (re-runs whatever launched it; for a plain `claude` tab that's a fresh claude in the same cwd).
- **^a c** — new pane tab, prompts for command + cwd. Type `claude --resume <token>` to attach to a specific saved conversation directly. The `<token>` is the `(<short-id>)` shown next to `claude:<name>` in spyc's session picker.
- **Inside the running claude itself** — type `/resume <token>`. This is exactly what spyc types into the pane on session restore. `/resume` alone opens claude's built-in picker.
- **^a K / ^a x** then **^\\** (or **F10**) — close the tab, re-open a fresh bottom pane with the default command.

### Worth noting

- Save is **on quit**, not continuous — `src/state/sessions.rs:95-104` (`save_session`) writes one JSON file per session, pruned to `MAX_SESSIONS = 20`.
- The `agent_session_id` field was renamed from `claude_session_id` in v1.41.6; the `#[serde(alias = "claude_session_id")]` keeps older saves loadable (`src/state/sessions.rs:42-56`).
- Sessions that point at a directory that no longer exists short-circuit with a `session dir gone:` flash (`src/app/mod.rs:8036-8040`) — restore aborts safely instead of chdir'ing into a void.

tags: #reference #session-restore #claude #codex #gemini #discoverability

---

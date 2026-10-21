# UX 2.0 — the object under the pointer (2.4 design)

**TL;DR for reviewers.** Every object spyc draws — a file row, a directory, a
pane, a tab label, a split divider, a status segment, the blank space below the
list — gets a menu of the actions that apply to it. Each row shows the key that
would have done the same thing, and after a menu fires an action the prompt row
says which key would have fired it. A Makefile becomes an object whose menu is
its targets, and a target runs in the live `!` pager, which becomes an object
too: right-click it to background the task. Right-click opens the menu — plain
where the terminal passes it, with ⇧ or ⌘ held on iTerm2 — and `.` opens it
everywhere. Right-click on blank space still opens the leader.

**Dependency.** This plan executes after `PROJECTS_PLAN.md` has landed
(owner decision, 2026-10-11), and ships in 2.4, after 2.3 ships on Projects
alone (owner decision, 2026-10-21). Project menus are in scope (§3f); the `file:line`
anchors below are against pre-Projects `main` and are re-verified in slice 0.

What approving this doc agrees to:

- **One hit-test, one object model (§1).** A `Target` enum names what is
  under the pointer or the cursor. The renderer and the hit-test share each
  surface's geometry, as `tab_hit.rs` already does for the tab bar.
- **Two ways in, one popup (§2).** `.` (decided) and the `Menu` key open the
  menu for the object under the cursor; right-click,
  with whatever modifier the terminal needs to pass it through, opens it for
  the object under the pointer. Right-click on blank space opens the leader,
  as it does today. A click outside an open menu dismisses it.
- **Menus project from `Action` (§3).** `menu_for(Target) -> Vec<ChordEntry>`
  is a pure function over a `Copy` snapshot. No hand-written tree; a guard
  pins that every `Tier::Frame` and `Tier::Pane` action is in some menu or on
  an exempt list. Entries come from the predicates spyc already refuses on.
- **Makefile targets (§4).** A new `taskfile` module parses `Makefile` with
  `makefile-lossless` and never evaluates it. A target runs as a `!` capture
  in the live pager; `Space m` and `:make` reach the same menu by keyboard.
- **The pager is an object (§3h).** The live `!` pager and the task viewer get
  menus: background, interrupt, kill, promote to a pane tab, pause, resume.
  Their keys are raw matches today, so a small `PagerAction` enum is lifted
  first so the menu projects from it like every other.
- **Teaching (§5).** Every row keeps its key. A menu-fired action flashes
  `^a R  restart pane tab` in the prompt row until the user has typed that key
  three times (`[ui] menu_hints = "fade"`, the default); counts persist in the
  state directory.
- **Clickable which-key (§6).** `CLICKABLE_MENUS_PLAN.md` becomes slices 1–2 of
  this plan and is archived once this doc is approved.

**Rollout (§7).** Ten slices, one PR each, after Projects. Slice 0 is
measurement and anchor re-verification.

**Owner decisions so far** (2026-10-11): the key is `.`; targets run in the
live pager, backgroundable from its menu; fade after three key presses;
`Space m` on the leader; Projects is a dependency and project menus are in.

**Needs your call** ([Open questions](#open-questions-for-review)): how far
the `PagerAction` lift goes; whether a pane's menu offers a tab move between
projects.

---

**Status: draft for review.** Measured against `d62909e` (`main` after #602).
Origin: onboarding a new user, who could not tell which shortcuts existed and
asked that if menus were added they carry the shortcut so the keys could be
learned at their own pace. The second input is the finding in
`docs/drafts/CLICKABLE_MENUS_PLAN.md` §1, which this plan builds on rather than
repeats. Facts about today's code carry a `file:line`; the preferences this
plan needed are recorded above as owner decisions.

---

## 0. The constraint this plan is shaped around

The request was "right-click on any object brings up its actions". The
foundation has to be wider than that, for a reason already measured:
`CLICKABLE_MENUS_PLAN.md` §1 found that iTerm2 opens its own context menu on
right-click and spyc sees nothing, so the `Gesture::Right → MouseSink::LeaderMenu`
arm (`src/app/mouse/route.rs:470`) has never fired there. Windows Terminal,
which hosts the WSL path spyc supports, pastes on right-click by default. A
feature whose purpose is cold discovery cannot depend on a gesture that two of
the host terminals eat before the application sees it, and the fix of "go to
Preferences → Pointer" asks the new user to learn something to avoid learning
something.

So right-click stays the pointer's route, and the terminals that eat it have
a modifier that passes it through: on iTerm2, ⇧-right-click and ⌘-right-click
both reach the application (owner's observation, 2026-10-11). spyc already
accepts them — `gesture_and_delta` matches the button and ignores the
modifier byte (`src/app/mouse/mod.rs:70`), so a modified right-click is
`Gesture::Right` today. What is missing is the sentence that tells an iTerm2
user to hold the key, and that is a documentation and first-menu problem, not a
routing one (§2). The keyboard route exists because `DESIGN.md` → "Keys are the
API" says nothing may be reachable by pointer alone, and because the new user's
other hand is on the keyboard anyway.

A long left press was considered as a modifier-free pointer gesture and
rejected by the owner: a timeout on every press slows the user it is meant to
help. It is listed under non-goals so it is not re-proposed.

Slice 0 records, per terminal, which right-click reaches spyc, before the
documentation claims anything (§7).

---

## 1. The object model

Today the pointer resolves to a `Region` (`src/app/mouse/route.rs:23`): list,
right column, pane, divider, prompt, status, vdivider. That is where the
pointer is, not what it is on. A menu needs the latter.

```
enum Target {
    Entry { side: Side, kind: EntryKind, git: GitMark, picked: bool,
            executable: bool, taskfile: Option<Taskfile> },
    Blank { side: Side },          // list rows past the last entry
    PaneBody,                      // the active tab's child
    PaneTab(usize),                // a label in the divider's tab bar
    DividerSpace,                  // the divider past the last tab
    VDivider,
    StatusSegment(Segment),        // project, session, git, agent (§3f)
    Prompt,
}
```

`Target` is `Copy` and resolved into the snapshot the way `tab_under_pointer`
is (`route.rs:143`): the router stays a `const fn` over a `Copy` value, and the
resolution that needs layout vectors happens before it. The keyboard route
resolves the same enum from the focused column's cursor, or the active tab, so
`menu_for` (§3) never knows which input opened it.

**Geometry is one source.** Each surface that becomes hit-testable exports a
pure layout the renderer also consumes, following `tab_layout` / `tab_at`
(`src/app/mouse/tab_hit.rs:53`, `:182`): the which-key box
(`CLICKABLE_MENUS_PLAN.md` §5b, `render_chord_hint`,
`src/app/render/overlays.rs:466` computes it inline today), and the status bar's
segment spans (`src/ui/status.rs`). The list already maps a row to an entry for
drag-select. The penalty for re-deriving is the one `tab_hit.rs` records: a
click lands on the neighbour, only sometimes, and reads as "off by one".

New module: `src/app/mouse/target.rs`. `route.rs` is 2,076 lines with its
tests and adding the hit-test there is the wrong direction.

---

## 2. Opening and closing a menu

| input                                     | on an object                                                   | on blank space    |
| ----------------------------------------- | -------------------------------------------------------------- | ----------------- |
| `.` (list focus), `Menu` key (any)        | menu for the cursor's entry, or the active tab from pane focus | —                 |
| right-click, any modifier                 | menu for the object under the pointer                          | leader (as today) |

**The key.** `Action::ContextMenu` is `Tier::Frame` from the list and acts on
the active tab from the pane; that is two actions, `ContextMenu` (Frame) and
`PaneContextMenu` (Pane), or one `Tier::Meta` action, which `Help` and `Quit`
share, bound on both prefixes. Meta is the honest tier: the menu is
informational until a row is chosen, and the row's own action carries its own
tier. `.` is free in Normal mode (every lowercase letter is taken;
`,` `.` `@` `#` `%` `&` `*` `<` `>` `{` `}` `\` `^` `"` and `E` are not) and
reads as "this thing" in the shell. `Menu` (`KeyCode::Menu` in crossterm 0.29)
is the OS convention and costs nothing to add. `.` is decided (owner,
2026-10-11).

**Right-click** keeps its routing and changes its sink: `MouseSink::LeaderMenu`
becomes `MouseSink::ContextMenu` with the target, and the leader is what a
`Blank` target's menu is. It is never forwarded to a pane child
(`route.rs:466`), so the pane body's menu is reachable without leaving the
agent. Modifiers are ignored on `Right`, as today, so the same gesture is plain
on ghostty, kitty and wezterm (where ⇧ is the terminal's bypass and would hand
the click back to it) and ⇧- or ⌘-modified on iTerm2. Opening the menu by
right-click focuses the object's region first, as a left click would, so the
keys the rows show are the ones that work next (§5).

**Telling the user.** The only cold-discovery route on iTerm2 is the key, so
the key has to be visible before any menu is: the status bar's suffix shows
`. menu` until the first menu has opened in this install (one flag beside the
fade counts, §5), and the help overlay's first section names the modifier
rule per terminal. `CONFIGURATION.md` already carries a per-terminal table for
the selection bypass (`CONFIGURATION.md:440`); the right-click row joins it.

**While a menu is open.** It is the which-key popup with computed rows, drawn
by `render_chord_hint` and owning the pointer by `CLICKABLE_MENUS_PLAN.md` §4's
rule: a click on a row fires it, a click outside dismisses and does nothing
else, the wheel is swallowed. The row's key fires it, as in every chord menu.
Computed menus also take `↑`/`↓`/`Enter`/`Esc`, because arrows are never chord
continuations and a learner who does not know `R` can still pick "restart".
Static chord menus (`Space`, `^a`, `g`, `H`) stay key-and-click: `^a j`/`^a k`
are continuations there, and a second navigation model inside a menu that
already has one is the ambiguity `Esc`-to-cancel was meant to avoid. A numeric
count armed before the key is consumed by the menu's dispatch the way
`take_count()` consumes it on the key path.

---

## 3. What each object offers

`menu_for(Target, MenuSnapshot) -> Vec<ChordEntry>` lives in
`src/keymap/menu.rs`, pure, unit-tested per target. `ChordEntry::Sub` gains the
`PendingSeq` it opens, per `CLICKABLE_MENUS_PLAN.md` §5a; that change is slice
2's, not this section's. The `keys` string on each entry is the binding the
resolver accepts from the object's focus, so the popup teaches the real key.
Rows with no default key (the `:` commands) show the command.

**Applicability is real.** An entry appears only when the action would not be
refused. The predicates exist: `EntryKind` and `is_executable`
(`src/fs/entry.rs:6`, `:108`), the archive `Capability` (`src/archive/scan.rs:76`),
`merged` on `list_worktrees`, non-empty picks, a tab's `is_shell` and
`suspended` (`pane_suspend_key_action`, `src/app/key_dispatch/mod.rs:78`),
`needs_live_child_confirm` (`src/app/pane_tabs.rs:38`). A menu that offers
"extract" on a plain file converts a reassurance into a flashed error, which is
worse than no menu.

**No second registry.** Guard `every_frame_and_pane_action_has_a_menu`: each
`Tier::Frame` and `Tier::Pane` `Action` variant appears in at least one
`menu_for` output across a fixed set of targets, or on an exempt list with a
reason (cursor motion, `Noop`, `MacroRecordReserved`). The same shape as
`leader_and_pane_namespaces_respect_tiers`. The which-key completeness guard
(`src/keymap/resolver/tests/prefixes.rs:228`) already covers the static menus.

### 3a. File, directory, symlink

Ordered by how often a new user wants them; the first row is what `Enter`
does, so the menu's top line restates the default and names its key.

| row                          | key        | action                         | shown when                      |
| ---------------------------- | ---------- | ------------------------------ | ------------------------------- |
| open / enter                 | `Enter`    | `EnterOrDisplay`               | always                          |
| edit (full screen)           | `e`        | `EnterOrEdit`                  | file                            |
| edit in top pane             | `V`        | `EditInPane`                   | file                            |
| view in top pane             | `D`        | `DisplayInPane`                | file                            |
| run (captured)               | `:exec`    | `ExecuteEntry` (new, §3g)      | executable file                 |
| make target …                | `Space m`  | `Sub(PendingSeq::Make)`        | `taskfile.is_some()` (§4)       |
| pick / unpick                | `t`        | `TogglePick`                   | always                          |
| take to inventory            | `yy`       | `Take`                         | always                          |
| yank path                    | `yf`       | `YankPaths`                    | always                          |
| copy to … / move to …        | `c` / `M`  | `CopyPrompt` / `MovePrompt`    | always                          |
| delete to graveyard          | `d`        | `RemovePrompt`                 | always                          |
| diff / blame / restore       | `gd` `gb` `gr` | `GitDiff` …                | git mark says so                |
| chmod +x                     | `:chmod`   | `ChmodAdd('x')`                | file, not executable            |
| add to harpoon               | `Ha`       | `HarpoonAppend`                | always                          |
| send path to pane            | `^a s`     | `PaneSendSelection`            | a pane tab is open              |
| pipe contents to pane        | `^a p`     | `PanePipeContent`              | file, a pane tab is open        |
| mount / extract              | `Enter` / `:archive` | per `Capability`     | archive                         |
| worktree: list / new / delete| `Wl` `Wn` `Wd` | `Worktree*`                | directory is a repo root        |
| new file / new directory     | `N` / `+`  | `NewFilePrompt` / `MakeDirPrompt` | directory                    |

With picks non-empty, the copy/move/delete/take rows say "N picked" and act on
the picks, as the keys do. The row's label is `Action::describe()`
(`src/keymap/action.rs:321`); nothing is re-worded for the menu.

### 3b. Makefile

A `Makefile` row's menu is §3a with the make submenu first, and the submenu's
rows are the targets (§4). The same submenu appears on a directory containing
a `Makefile`, after "enter".

### 3c. Pane body and tab label

Both resolve to the same tab's menu; the label version names the tab in its
title.

| row                      | key      | action                              | shown when                     |
| ------------------------ | -------- | ----------------------------------- | ------------------------------ |
| restart                  | `^a R`   | `PaneRestartTab`                    | always                         |
| suspend / resume         | `^z`     | `PaneToggleSuspend` (new, §3g)      | not a shell tab                |
| fork conversation        | `^a F`   | `PaneForkTab`                       | agent tab with a session       |
| rename                   | `^a r`   | `PaneRenameTab`                     | always                         |
| close                    | `^a x`   | `PaneCloseTab`                      | always (confirm if live)       |
| zoom / restore           | `^a z`   | `TogglePaneZoom`                    | always                         |
| scrollback               | `^a v`   | `PaneScrollEnter`                   | always                         |
| quick select             | `^a u`   | `QuickSelectOpen`                   | always                         |
| yank prompt / scrollback | `yp` `ya`| `YankPrompt` / `YankScrollback`     | always                         |
| new tab                  | `^a c`   | `PaneNewTab`                        | always                         |
| to background task       | `:pane-to-task` | `PaneToTask` (new, §3g)      | shell or `!` tab               |
| focus list               | `^a k`   | `PaneFocusUp`                       | pane focused                   |

"Move tab to project" is open question 2.

### 3d. Divider and vdivider

Divider space past the last tab: new tab, next/previous tab, zoom, grow /
shrink. VDivider: toggle split (`^a |`), flip height (`^a f`), open / close
second commander (`^s n` / `^s x`), dim (`^a d`).

### 3e. Blank space

The leader, unchanged. With a `.spycrc.toml` that binds leader keys, the menu
shows them, as the popup does today.

### 3f. Status segments and projects

The bar's `projects` segment (`docs/archive/PROJECTS_PLAN.md` §5) is the one place every
open project is drawn, so it is where project menus live. `status.rs` exports
its segment spans, and within the `projects` segment, one span per label, the
way `tab_hit.rs` does for tabs.

| object                     | left-click         | menu rows                                                                 |
| -------------------------- | ------------------ | ------------------------------------------------------------------------- |
| a project label            | switch to it (`Space N`) | switch, close (`Space x`), re-home (`Space P`), jump home (`Space p`), rename (`:project name`) |
| the segment's count (`2/4`) | project switcher (`Space j`) | switcher, new project (`Space n`), next/previous (`Space ]` `[`), go to the agent that needs you (`Space !`) |
| git segment                | —                  | worktree list / new / delete, `gd`                                        |
| agent segment              | focus the pane     | the pane menu for that tab (§3c)                                          |
| session segment            | —                  | `:name`, `Space s`, `spyc -r` picker                                      |

With one project open the `projects` segment is today's `project` segment and
its menu is the first row's rows minus switch and close. Rows project from
the Projects actions as they land, so this section has no action of its own
to add; it is the hit-test and the menu table.

### 3g. New actions

Four `Action` variants, each a one-line `describe()` and a help row, because a
menu may only project what the keymap can name:

- `ExecuteEntry` — run the cursor file as `./name` through `start_capture`
  (`src/app/tasks.rs:133`), the `!` path, so it lands in the live pager like a
  make target (§4); `:exec` is its command, no default key (policy: rare
  operations stay off keys). Interactive executables keep the `;` prompt.
- `PaneToTask` — `demote_pane_to_task` (`src/app/tasks.rs:466`) is reached by
  `:pane-to-task` only. Lifting it to an action lets the pane menu offer it;
  the command stays.
- `PaneToggleSuspend` — `toggle_pane_suspend` (`src/app/pane_tabs.rs:440`) is
  reached today by a special case in `pane_suspend_key_action`, not by an
  `Action`. Lifting it is the fix that lets a menu offer it and lets a user
  rebind it.
- `ContextMenu` — §2.

`RunMakeTarget` is §4's, and `PagerAction` is §3h's.

### 3h. Live pager and task viewer

A `!` capture streams into the modal pager, `^Z` moves it to
`background_tasks` and closes the pager (`background_capture`,
`src/app/tasks.rs:198`), `:fg` brings it back, `g B` opens the task viewer in
peek mode, and `:task-to-pane` promotes a task to a pane tab. The machinery is
complete; the pager is the one surface whose keys are a raw `KeyCode` match
(`src/app/pager_handler/modes.rs`) rather than an enum, so a menu has nothing
to project from.

**`PagerAction`.** A small enum in `pager_handler`, with `describe()` and the
key each row shows, that the raw match dispatches through for the rows below.
Motion keys (`j`, `/`, `n`, `G`) stay in the match; the enum is for what a
menu offers, and its completeness guard is "every `PagerAction` appears in the
pager menu". The lift is the same shape as `PaneToggleSuspend`'s and is the
first commit of slice 6.

| row                     | key     | shown when                                        |
| ----------------------- | ------- | ------------------------------------------------- |
| background              | `^Z`    | a live `!` capture                                |
| interrupt               | `^C`    | the capture is running                            |
| hard kill               | `^\`    | the capture is running                            |
| foreground              | `:fg N` | task viewer, task backgrounded                    |
| pause / continue        | `S` / `C` | task viewer, running / paused                   |
| to pane tab             | `:task-to-pane N` | task viewer                             |
| next / previous task    | `]t` `[t` | task viewer                                     |
| save to file            | `s`     | any pager with a buffer                           |
| reopen last buffer      | `gp`    | any pager                                         |
| close                   | `q`     | always                                            |

**Routing.** Today a modal pager swallows everything but the wheel and a left
press on its content (`route.rs:422`). `Gesture::Right` over a covering pager
becomes `MouseSink::ContextMenu(Target::Pager(slot))`, with the capture's
state in the snapshot so "background" is offered only while there is a task
to background. The `scrollback` (`^a v`) and `D` pagers get the same menu
minus the task rows.

---

## 4. Makefile smarts

New module `src/taskfile/` (`mod.rs`, `make.rs`), capped by the 800-line rule.

**Parse, never evaluate.** The alternative to a parser is `make -pq`, which
prints the expanded rule database and is what shell completion uses. It is
rejected for the same reason a repo's `.spycrc.toml` loads as `Trust::Project`
(`src/config/load.rs:42`): a Makefile is someone else's text, and `make -p`
evaluates `$(shell …)` while reading it. spyc's own `Makefile` runs
`cargo metadata` in one (`Makefile:26`), harmless; a hostile one would not be.
Discovery must have no side effects because it runs on a cursor move.

**The crate.** `makefile-lossless` 0.4.x (jelmer; Apache-2.0; one dependency,
`rowan`; ~43k lines; releases monthly or faster, 0.4.8 on 2026-10-10). It
parses to a CST and exposes what discovery needs without the lossless half:
`Makefile::from_file_relaxed` (errors returned, tree still produced),
`rules()`, `targets()`, `phony_targets()`, `includes()`, `default_goal_candidates`,
`doc_comments()` (whole-line `#` comments directly above a rule, stopping at a
blank line), `parse_with_variant` for GNU vs BSD. The lossless edit API is
weight spyc does not use; the `jiff` precedent ("it's only 50 LoC" is not a
reason to hand-roll a solved problem) still applies, and a Makefile grammar
with conditionals, `define`, line continuations and `::` rules is not 50 LoC.
`deny.toml` has no objection to Apache-2.0 or rowan. Pin a minor, follow
releases; the API has been stable across 0.4.

One gap worth naming: spyc's own targets are documented with the `make help`
idiom, a trailing `## text` on the rule line (`Makefile:35`), and
`doc_comments()` reads the lines above, not the trailing comment. Labels are
sourced in order: a `##` comment on the target's own line, found through
`comment_ranges()` on the rule's line; the first `doc_comments()` line; none.

**Which targets.** Explicit targets only: no `%` patterns, no unexpanded
`$(…)` in the name, no leading `.` (`.PHONY`, `.DEFAULT_GOAL`, `.SUFFIXES`),
no archive members. `.PHONY` targets first in source order, then the rest;
the default goal is marked. Target-specific variable rules and static patterns
are skipped. `include`d files are followed only when the path is literal,
exists, and sits inside the worktree, the containment rule MCP roots already
use; `-include` of a missing file is silent. One Makefile per directory:
`GNUmakefile`, `makefile`, `Makefile` in make's own precedence.

**Cache.** Keyed by `(path, mtime, len)`, invalidated by the fs watcher like
the listing. Parsing a 300-line Makefile is well under a frame; the cache is
for the cursor resting on a directory, not for speed.

**Running.** A chosen target is a `!` capture, `make -C <dir> <target>`,
through `start_capture` with the pager titled `make <target>` (owner decision:
the live pager, not a pane tab). Everything a long run needs is already there:
`^Z` or the pager's menu backgrounds it, the task viewer peeks at it, `:fg`
returns to it, and `:task-to-pane` promotes it when the user wants a tab after
all. Re-running is `!?` history or the menu again. The run is the user's
choice, like typing the command, so no consent prompt beyond the menu row;
the pager's title shows what ran.

**Keyboard route.** `Space m` opens the make submenu for the focused column's
directory (the cursor's Makefile when on one): `Tier::Global`, workspace
operation, same tier argument `docs/archive/PROJECTS_PLAN.md` §4 makes. `:make [target]`
runs one directly. Both land in the leader's hint list for the completeness
guard. `m` is free on the leader and is the mnemonic.

**Follow-up, not 2.4.** `Justfile` — spyc ships one. `just --dump --dump-format json`
is the parse, if it evaluates nothing; whether top-level backtick assignments
run under `--dump` has to be checked before it is used. `package.json`
scripts and `Cargo.toml` are read-only JSON/TOML and cheap once the
`Taskfile` trait has two implementations. Each wants its own issue.

---

## 5. Teaching the keys

The user's ask was explicit: the menus must carry the shortcut so the keys can
be learned over time. Three mechanisms, one of them new.

1. **The row keeps its key.** `d  delete to graveyard`. This is which-key's
   format and it does not change. The key shown is the one the resolver
   accepts from the object's focus; the click has already moved focus there
   (§2), so it is accurate when read.
2. **The after-action flash (new).** When an action is fired from a menu row
   by click, arrow, or `Enter`, the prompt row flashes
   `^a R  restart pane tab` for the flash's normal duration. Fired by its own
   key, nothing is shown; the user already knows. A row that opened a submenu
   flashes nothing; the submenu's rows carry their keys.
3. **Fade (new).** The flash stops for an action once the user has fired it by
   key `menu_hints_fade_after` times (default 3). Counts are per action,
   keyed by `Action::canonical_name()` (`src/keymap/action.rs:459`, the
   `.spycrc.toml` binding names, so a renamed key keeps its count), stored in
   `<state>/learned.json` through `fs::write_atomic` like `frecency.json`.
   Global, not per project: muscle memory is the user's, the argument
   `docs/archive/PROJECTS_PLAN.md` §1a makes for the histories. Only key-fired dispatches
   count; a menu-fired one never does, or clicking three times would silence
   the hint it was meant to earn. `:hints reset` clears the file; `:hints`
   lists what has faded. The which-key popup is untouched by fade: it is the
   reference, not the lesson.
4. **The help overlay stays the reference.** `?` opens it from any menu, and
   each menu's title row says so once: `?  all keys`.

Config, in `[ui]`:

```toml
menu_hints = "fade"          # "fade" (default), "always", or "off"
menu_hints_fade_after = 3    # key presses of an action before its hint stops
```

`"always"` is `"fade"` with no threshold; `"off"` writes nothing. The file is
the one new piece of persisted state in this plan, and it holds counts and
nothing else.

---

## 6. What this absorbs from `CLICKABLE_MENUS_PLAN.md`

Its §2 bugs (B1: the popup is clicked through; B2: a click while a chord is
armed latches it), its §4 PR 1 (the popup owns the pointer), its §5 PR 2 (rows
are clickable; `Sub` gains its `PendingSeq`; `chord_hint_layout` extracted)
become slices 1 and 2 here unchanged. Its §6 PR 3, one computed menu for the
cursor row, is §3a. Its non-goals (menu bar, hover highlight, `KeyChord::Mouse`,
mouse modifiers) stand. Its open decision, the key, is this doc's open question
1. On approval the draft moves to `docs/archive/` with a one-line pointer here,
and #478 is retitled to this plan.

One of its findings is narrowed: iTerm2 eats an *unmodified* right-click,
and ⇧ or ⌘ passes it through. The draft's conclusion that right-click cannot
be the only route stands; its implication that iTerm2 users have no pointer
route does not. Slice 0 re-measures with modifiers on the current terminals.

---

## 7. Sequencing

This plan starts once `PROJECTS_PLAN.md` steps 1–8 have merged. The reasons
are practical: §3f needs the `projects` segment and its actions to exist, the
leader table this plan adds `m` to is the one Projects rewrites, and two
tracks editing `status.rs` and the leader in parallel is a merge nobody
needs. Nothing in slices 1–5 depends on Projects, but nothing in them is
urgent enough to pay for the conflict.

Each slice is one PR with its own tests and docs; `make check` green on each.

0. **Measure and re-anchor.** On iTerm2, Terminal.app, ghostty, kitty,
   wezterm, Windows Terminal (WSL): which of plain, ⇧-, ⌥-, ⌃- and
   ⌘-right-click reach spyc as `Gesture::Right`. A table in this doc's §0,
   with versions, which becomes the `CONFIGURATION.md` row. A terminal where
   none arrives (Windows Terminal's right-click paste is the candidate) gets
   the key as its route and the doc says so. Then every `file:line` in this
   doc is re-verified against post-Projects `main`, since `route.rs`,
   `status.rs` and the leader table will have moved. Half a day.
1. **The popup owns the pointer.** `CLICKABLE_MENUS_PLAN.md` PR 1. Closes B1
   and B2, with a regression test naming B2's repro.
2. **Rows are clickable.** PR 2: `ChordHint.rows` keeps the `ChordEntry`,
   `Sub` carries its `PendingSeq`, `chord_hint_layout` is shared by renderer
   and hit-test, count preserved, arrows/Enter on computed menus only.
3. **`Target` and `menu_for`.** `mouse/target.rs`, `keymap/menu.rs`, the
   new actions (§3g), the entry menu (§3a) and the pane menu (§3c), the guard,
   the `.`/`Menu` binding, right-click re-sunk to `ContextMenu`. The
   after-action flash and `menu_hints`.
4. **Fade.** `state/learned.rs`, the counter on key-fired dispatch, the
   `"fade"`/`"always"`/`"off"` modes, `:hints`, `:hints reset`, the
   first-menu status suffix. Tests: a menu-fired action never counts; the
   file survives an unknown action name (an older spyc reading a newer file).
5. **Divider, vdivider and blank** menus (§3d, §3e).
6. **The pager as an object** (§3h): `PagerAction`, the raw match dispatching
   through it, `Target::Pager`, right-click over a covering pager,
   `PaneToTask`. Tests: "background" is absent once the task is backgrounded;
   the scrollback pager never offers task rows.
7. **`taskfile`**: the crate, discovery, labels, cache, the `Makefile` entry
   menu, `Space m`, `:make`, running through `start_capture`. Fuzz target for
   discovery before the parser is wired, per the project's rule: a Makefile
   is attacker-supplied text.
8. **Status segments and projects** (§3f): segment spans from `status.rs`,
   per-label spans in the `projects` segment, left-click to switch, the
   project menus.
9. **Docs sweep**: `docs/KEYBINDINGS.md`, `src/ui/help.rs`, `FEATURES.md`,
   `CONFIGURATION.md` (`[ui]`), `DESIGN.md` (the mouse paragraph
   under "Keys are the API", and a "Menus" entry in the component
   vocabulary), `AGENTS.md` for the two new modules, `CHANGELOG` note for
   ghostty users whose right-click over a row now opens the row's menu rather
   than the leader.

Slices 1–5 are the discoverability core and are what a new user notices;
6–8 are the objects that make it complete. If the release runs long, 8 is
cut before 7 and 7 before 6: a pager you cannot background from the mouse is
a worse gap than a status bar you cannot click.

---

## 8. Non-goals

- **A menu bar.** As before: vertical space.
- **Hover highlight.** Needs `?1003h`; `src/app/proc.rs:108` drops
  button-free `Moved` on purpose. Separate decision, separate evidence.
- **A long left press.** Rejected by the owner (2026-10-11): a timeout on
  every press slows the user it was meant to help. Not to be re-proposed as a
  modifier-free route; the modifier rule in §2 is the answer on iTerm2.
- **Double-click to open.** Implementable from timestamps, but it is a third
  gesture on the row and "open" already has the top row of the menu and
  `Enter`.
- **Menus inside the pager, the help overlay, or the image viewer.** Each is a
  modal with its own keys; a menu on a menu is not discoverability.
- **Reordering, pinning, or user-defined menu rows.** The menu projects
  `Action`; a `.spycrc.toml` binding changes the key shown, not the row set.
- **Running targets from a Makefile the user has not opened.** Discovery is
  for the directory the column is in and the file under the cursor, not a
  project-wide scan.

---

## 9. Risks

- **Slice 2 is where a regression hides**, as the earlier draft said: the
  popup is inert today, and every click it accepts is new surface. One
  geometry, two consumers, pinned by a test.
- **The iTerm2 modifier rule is folklore until slice 0 writes it down.** If
  ⇧ turns out to be iTerm2's own bypass on some versions, ⌘ is the fallback
  and the table says which.
- **Applicability drift.** A predicate the menu reads but a dispatch no
  longer checks, or the reverse, puts a refused row in a menu. The guard
  covers coverage, not applicability; applicability is tested per target
  with the refusing states (mounted archive, claimed worktree, shell tab).
- **A Makefile that parses to nothing** (every target behind `$(…)` or `%`)
  yields an empty submenu. The row then says `make: no explicit targets`
  rather than hiding, so the user learns why.
- **The leader got dense in 2.3.** Nine Projects keys plus `m`. The popup
  wraps; the guard checks completeness; nothing else to do unless it reads
  badly in use.
- **The `PagerAction` lift is the widest diff in the plan.** `modes.rs` is a
  long match with mode-dependent arms (live capture, task viewer, file view).
  Keeping the enum to the rows a menu offers, and leaving motion in the match,
  bounds it; open question 1 is whether that line is right.
- **`PaneToggleSuspend` lifting** touches a key path with a known subtlety
  (`^z` on a shell tab forwards; on an agent tab toggles). The existing tests
  at `key_dispatch/mod.rs:820` pin it; the lift must keep them.

---

## 10. Exit criteria

1. A user who has not read `KEYBINDINGS.md` can, by pointer alone on iTerm2:
   restart a stuck agent, open a file in the editor, run `make check` from
   spyc's own `Makefile`, background it while it runs, bring it back, switch
   project, and close a tab — and sees the key for each.
2. Every row in every menu fires by click, by its key, and (computed menus)
   by arrows and `Enter`; a click outside dismisses; nothing reaches what is
   behind the popup. B1 and B2 closed with regression tests.
3. `every_frame_and_pane_action_has_a_menu` passes with an exempt list the
   reviewer agrees is complete, and every `PagerAction` is in the pager menu.
4. No menu offers an action spyc would refuse, checked against a mounted
   archive, a claimed worktree, a shell tab, a suspended tab and a plain file.
5. Discovery on a Makefile executes nothing: a `$(shell touch /tmp/x)` in a
   test fixture leaves no file.
6. The slice 0 table is in this doc and in `CONFIGURATION.md`, and an iTerm2
   user can find the modifier rule from the help overlay without reading
   either.
7. With `menu_hints = "fade"`, the flash for `PaneRestartTab` stops after
   three `^a R` presses and never after any number of menu clicks.
8. `make check` green; docs updated in the slice that changes behaviour.

---

## Open questions for review

1. **How far `PagerAction` goes.** Written as "the rows a menu offers, motion
   stays in the match". The alternative is lifting every pager key, which
   makes the pager rebindable from `.spycrc.toml` for the first time — a
   feature, but not this plan's, and a diff that would dominate slice 6.
2. **A tab's project.** Whether the pane menu offers "move tab to project …",
   a `Sub` over the open projects. It needs the tab's pty and its MCP binding
   to change project, which `PROJECTS_PLAN.md` does not design. Left out
   unless Projects turns out to make it cheap.

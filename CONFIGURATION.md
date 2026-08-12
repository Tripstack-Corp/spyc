# Configuring spyc

Everything spyc reads at runtime, with examples. Two surfaces:

- **`.spycrc.toml`** — TOML: layout, notifications, colors, ignore masks, and the
  keymap DSL.
- **`~/.config/spyc/`** — Lua: `map KEY lua <name>` scripts and an optional
  `init.lua` for keybinds, `:` commands, and event hooks.

The fastest start: `spyc --print-config` prints a fully-commented template with
every option at its default — pipe it to a file and uncomment what you want:

```sh
spyc --print-config > ~/.spycrc.toml
```

## Where config lives

| File | Scope |
|------|-------|
| `~/.spycrc.toml` | per-user defaults |
| `<project>/.spycrc.toml` | per-project overrides — **win over** the user file |
| `~/.config/spyc/lua/<name>.lua` | scripts run by `map KEY lua <name>` |
| `~/.config/spyc/init.lua` | registers keybinds / `:` commands / event hooks |

Both `.spycrc.toml` files are **watched** — edits take effect **without a
restart** (`:lua reload` or `^R` re-runs `init.lua`). Project settings layer on
top of user settings per-field, so a bare `[notify]` in a project file doesn't
clobber your user defaults.

**Security:** the *executing* keymap verbs (`unix`, `command`, `lua`, `jump`)
only take effect from **`~/.spycrc.toml`** — a project-local `.spycrc.toml` in an
untrusted clone can't bind a key to run code. Lua scripts load only from
`~/.config/spyc/`.

---

## `.spycrc.toml` at a glance

```toml
[layout]
status_position = "top"        # or "bottom" (vim/tmux convention; prompt sits above)
chord_hint_delay_ms = 300      # ms holding a chord (g, ^a, H) before the which-key popup; 0 disables
color_depth = "auto"           # "auto" (truecolor if $COLORTERM says so, else 256), "truecolor", or "256"
vsplit_mode = "full_height"    # shape `^s |` opens a vertical split in; or "top_only" (pane stays full-width below)

[pane]
default_command = "claude"     # pre-filled into the `^a c` new-tab prompt
new_tab_cwd = "worktree_root"  # focused column's worktree root (gw's target); or "project_home" (PROJECT_HOME), "browse_dir" (the focused column's dir)
claude_transcript_scrollback = false  # `^a v` reads Claude's JSONL transcript instead of terminal scrollback
                                      # (only decides which comes up FIRST — `T` swaps in the view, and the
                                      #  transcript is used regardless when there's no terminal capture)
codex_mcp = true               # register spyc's MCP server for codex panes
preview_pasted_images = true   # keep a copy of images you paste into an agent pane, for `^a g`

[yank]
include_pager_title = true     # prepend a source header to pager yanks (y/Y)

[pager]
tab_width = 4                  # columns a tab expands to (min 1)

[markdown]
open_as_rendered = true        # open .md in rendered view (m toggles source)

[diff]
intraline = "char"             # intra-line highlight granularity: "char" | "word"

[delete]
confirm = true                 # y/N prompt before R / dd; false = yolo (still recoverable in the graveyard)
```

All optional; anything unset uses the built-in default. See `spyc --print-config`
for the annotated version of each.

### Color depth — `[layout] color_depth`

spyc's theme is 24-bit truecolor. Terminals that can't parse `\x1b[38;2;r;g;bm`
drop it wholesale — you get no color and no highlight. The worst offender is
macOS's **bundled GNU screen 4.00.03** (frozen at pre-GPLv3, from 2006), which
also mangles the powerline/emoji glyphs.

| value | behavior |
|-------|----------|
| `auto` (default) | 256 inside GNU screen; else truecolor when `$COLORTERM` advertises `truecolor`/`24bit`, else 256 |
| `truecolor` | always emit 24-bit RGB |
| `256` | quantize every color to the nearest xterm-256 index |

When not truecolor, the finished frame is remapped once before it's written, so
**all** colors degrade — theme, syntax highlighting, diffs, and ANSI passthrough.
`--color auto|truecolor|256` overrides the config for a single run.

**GNU screen:** `auto` drops to 256 whenever it's running inside screen (`$STY`
set), *even if `$COLORTERM=truecolor`* — screen inherits that claim from the outer
terminal but doesn't render 24-bit SGR (macOS's bundled 4.00.03 can't at all; 5.x
needs `truecolor on`, off by default), so trusting it leaves you colorless. If
you've turned `truecolor on` in a modern screen, force it back with
`color_depth = "truecolor"`. tmux is unaffected — it renders RGB and keeps
truecolor.

In any non-truecolor mode spyc also swaps the 🌶️ header glyph for a spice-red
block — old screen mangles the 2-cell emoji, so the block keeps the header
looking intentional rather than broken.

### Vertical-split height — `[layout] vsplit_mode`

The shape `^s |` (alias `^a |`) opens a vertical split in.

| value | behavior |
|-------|----------|
| `full_height` (default) | the divider runs the whole frame height; the right column is a full-height reading surface and the pty pane is confined under the left column |
| `top_only` | only the file-list region splits; the pane stays full-width below both columns |

Aliases: `full` for the first, `half` / `half_height` / `top` for the second.
`^s f` flips an *open* split either way, so this only sets where it starts. A
second commander (`^s n`) ignores the setting and always opens `top_only` — two
peer browsers normally want one full-width pane beneath them — and `^s f` is how
you make that one full-height too.

---

## `[clipboard]`

```toml
[clipboard]
via = "auto"   # auto | system | osc52 | both
```

Where a yank lands. `auto` uses an **OSC-52 terminal escape when spyc is over SSH**
and the local helper (`pbcopy` / `wl-copy` / `xclip`) otherwise.

This matters more than it sounds: the local helpers set the clipboard of the machine
spyc *runs on*. Over SSH that's the server, so a yank silently succeeds somewhere you
can never paste from. OSC 52 travels back up the same connection the UI is drawn on,
so it reaches the terminal you're typing at.

Locally it stays helper-first deliberately — OSC 52 is write-only with no reply, so
spyc can't confirm the terminal honored it, and some terminals disable it on purpose
(a remote host writing your clipboard is a real risk). Inside tmux it needs
`set -g set-clipboard on`; spyc DCS-wraps the escape so tmux forwards it to the outer
terminal. A selection too large for the escape (~75 KB of base64) is **refused with a
message** rather than risking a terminal truncating it silently — it does not fall
back to the helper, because under `auto` over SSH the helper isn't enabled and writing
the server's clipboard wouldn't help you anyway. Only `via = "both"` has a second
mechanism to reach.

```toml
[clipboard]
command = "wl-copy"
```

`command` overrides everything above: when set, spyc runs this exact command
verbatim — whitespace-split into argv, same "no shell features" contract as
`$EDITOR`/`$PAGER` resolution, so wrap it in a script if you need pipes or
redirection — and pipes the yanked text to its stdin. `via`/OSC-52 are skipped
entirely, not layered underneath. `$SPYC_CLIPBOARD` is the env-var form and wins
over the config key if both are set, matching how other spyc envs layer over
static config.

## Notifications — `[notify]`

The "which agent needs me" signal. When an agent pane changes status, spyc fires
on the **transition itself** (0 delay, not a timer). Three channels:

- **Desktop** — an OS notification / OSC-9 escape naming the tab.
- **Bell** — the terminal bell (BEL).
- **Visual** — spyc's spice-heat gradient **border pulse** (the branded flash).

### The model: Blocked vs Done, per channel

- `Blocked` ("needs me") fires **every enabled channel**.
- The routine `Done` (a finished turn, once per turn) fires a channel **only if
  that channel opts in** via its `*_done` flag.

By default the **intrusive** channels (bell + flash) stay **Blocked-only**, while
the **quiet** desktop ping also fires on `Done`. That keeps a per-turn ring/strobe
from being annoying while you still get a "finished" ping.

```toml
[notify]
desktop = true            # notify on Blocked (and Done, if desktop_done) — on by default
desktop_via = "auto"      # "auto" | "system" | "osc9" | "both"  (see below)
desktop_done = true       # also ping desktop on Done; false = Blocked-only
bell = false              # ring the terminal bell (BEL)
bell_done = false         # ring on Done too (default: Blocked-only)
visual = true             # the spice-heat border-pulse flash — on by default
visual_done = false       # flash on Done too (default: Blocked-only)
suppress_focused_tab = false  # stay quiet about the tab you're already watching
```

**`desktop_via`** — how the desktop ping is delivered:

| value | behavior |
|-------|----------|
| `auto` (default) | **OSC-9 escape over SSH** (pops on your *client* terminal) + the OS notifier locally |
| `system` | OS notifier only (`notify-rust` — the machine spyc *runs* on) |
| `osc9` | terminal escape only (needs iTerm2 / kitty / WezTerm) |
| `both` | fire both |

`auto` is the "just works over SSH" default: over SSH the ping reaches your
laptop terminal, not the remote box.

**`suppress_focused_tab`** is **off** by default on purpose — spyc having focus
isn't the same as your eyes being on the terminal; you're usually in another app
while the agent works. Set it `true` if you want the tab you're actively watching
to stay silent.

### Recipes

```toml
# Quietest useful setup: one desktop ping only when an agent is *blocked*.
[notify]
desktop_done = false
visual = false

# Maximal: ring + flash on every transition, both delivery mechanisms.
[notify]
desktop_via = "both"
bell = true
bell_done = true
visual_done = true

# Terminal-only (no OS notifications), flash on Blocked:
[notify]
desktop = false
# visual stays true → Blocked-only flash

# Silence everything:
[notify]
desktop = false
visual = false
```

Run **`:notify test`** to fire every channel on demand — verify your setup
without waiting for a real agent transition.

---

## Diff — `[diff]`

`intraline` sets how finely a modified line pair is marked up on top of the
row wash, in `gd` / `gD` / a commit opened from `gl`.

| value | marks | reads as |
| --- | --- | --- |
| `"char"` *(default)* | just the characters that differ — `value` → `values` brightens the `s` | precise; tells you exactly what moved |
| `"word"` | the whole changed token, like `git --word-diff` — brightens all of `values` | calmer on dense code; hides which characters moved |

Both granularities mark **every** changed region on the line, so an unchanged
token sitting between two changed ones is never swept into the highlight.

Pick `"word"` if per-character marking on tightly-packed code reads as
confetti to you; `"char"` is the default because saying precisely which
characters changed is what an intra-line highlight is for.

A very long line (roughly 4000+ characters, or 500+ words) gets no intra-line
highlight at all — on a minified or generated line it would be unreadable
noise, and the row wash still shows the line changed.

---

## Archives — `[archive]`

`Enter` on a zip / tarball browses it as a directory (see FEATURES.md →
Archives). A mount is an *index*, so entering one costs no disk; these knobs
bound the one case that does — a compressed tar, which has to be extracted as it
streams because it can't be listed any other way.

```toml
[archive]
enable = true              # false → Enter pages the archive as bytes
extract_budget_mb = 512    # hard ceiling for a .tar.gz / .tar.zst mount
warn_over_mb = 128         # confirm before extracting more than this
max_entries = 200000       # cap on indexed members
max_depth = 2              # how deep archives may nest (0 = no nesting)
write_back = "ask"         # "ask" | "never" — offer to write pending changes
snapshot_max_mb = 64       # graveyard-snapshot the original below this size
```

`max_depth` is about disk, not correctness: a container inside a container has to
be copied out whole before anything can read it, so every level costs its own
size in staging. Set it to 0 to refuse nested archives entirely.

A write-back never puts the original at risk: the new archive is written to a
temp file beside it, verified by reading it back, and only then renamed into
place. `snapshot_max_mb` is about *regret*, not corruption — below that size the
original is also copied into the graveyard so `:undo` can restore it.

Staging lives under `$XDG_STATE_HOME/spyc/archives/<pid>-<hash>/` and is removed
when the archive is unmounted or spyc exits; a tree left behind by a killed
process is reaped at the next launch.

## Mouse — `[mouse]`

```toml
[mouse]
capture = true       # real mouse reporting (wheel + buttons). Default ON.
scroll_lines = 1     # lines per wheel tick, for surfaces spyc scrolls itself
pane_scroll_lines = 3 # lines per tick for a pane spyc drives with synthesized keys
pane_scroll_view = "native" # native | off | spyc_history
invert_scroll = false # true flips the wheel direction everywhere
```

`pane_scroll_view` decides what a wheel gesture does when it hits an agent pane
whose own scrollback view spyc can drive but that view isn't open yet — today,
codex's `^T` transcript. `native` (default) opens it and scrolls; `off` leaves it
closed and only scrolls if it happens to already be open; `spyc_history` opens
spyc's own `^a v` scrollback pager instead. Doesn't affect an already-open view
(always scrolled) or an agent with no such view (agy scrolls its live content
directly via Shift+Arrow, with nothing to open).

Only a sustained scroll **up** — three consecutive ticks, reset by a pause, a
reversal or a tab switch — opens a closed view. A downward gesture never does:
it's aimed at the live buffer, and opening a transcript there lands at its
bottom, which the next tick reads as "at the bottom" and closes again.

A sustained same-direction wheel gesture — past ~1 second — escalates from the
per-line step to a page-sized one, for an agent with a verified fast key (codex's
`^T` documents `pgup/pgdn to page` in its own footer). `pane_scroll_lines` is the
line-scroll floor under that escalation, and the only knob agy uses (it has no
page-sized equivalent).

`capture` asks the terminal for real mouse reporting so spyc can scroll whatever
is under the pointer, instead of DEC 1007's trick of translating the wheel into
arrow keys (which a focused pane receives as history navigation).

`invert_scroll` flips the wheel direction. Left at its default (`false`), a
downward tick moves the file-list cursor to a later row and the pager further
into the content — the "scroll down = toward the end" mapping shared by browsers
and `less`. Set it to `true` if that reads backwards for you.

It's named for the mechanism rather than for a convention on purpose: whether
"down" *should* move the content or the cursor depends on your OS trackpad
setting and your terminal, so a `natural_scroll`-style flag would be ambiguous
in precisely the situation you'd reach for it. The flip is applied once, to the
event itself, before anything branches on it — so the file list, the pager, an
agent pane's synthesized scroll keys **and the wheel report forwarded to a
mouse-aware child** (claude, vim, htop) all move together. Half-inverting spyc
would be worse than either setting: the pane scrolling one way and the column
beside it the other.

> [!WARNING]
> **Capture takes native click-drag text selection away from your terminal.**
> That is inherent to mouse reporting, not a spyc choice. Three ways to get it
> back:
>
> | | |
> |---|---|
> | **Bypass modifier** (per-drag) | Hold **Shift** — Ghostty, WezTerm, kitty, Alacritty, most others. **Option** or **Fn** on iTerm2. |
> | **`:mouse off`** (per-session) | Immediate, no restart, no file edit. `:mouse on` to re-enable, `:mouse auto` to follow the config again. Survives a config reload — it is scoped to the session, not written to a file. |
> | **`capture = false`** (permanent) | Turn it off entirely — the pre-#226 behavior. |
>
> spyc's mouse-free yank paths: **`^a u`** quick-select (URLs / paths / SHAs) and
> **`y`** in the pager.

### What the buttons do

| Button | Action |
|---|---|
| **Left** | Focus the region under the pointer. Over a mouse-aware child (claude, vim, htop) the click also reaches it — the pane is live, so swallowing the first click just to focus would read as broken. **Dragging** is forwarded too, so the child's own text selection works. |
| **Middle** | Paste the system clipboard wherever a paste would land (pane, `:` line, shell prompt). |
| **Right** | Open the leader menu, from any region, with no chord-hint delay. |

Middle and right are **always spyc's** — never forwarded, even over a mouse-aware
child, since otherwise the gesture would be unavailable in exactly the region where
the pane holds focus. If you need a child's own right-click menu, `:mouse off`.

### Selecting text

Capture takes the terminal's own click-drag selection away. Where that leaves you:

| | |
|---|---|
| **Inside a mouse-aware child** (claude, vim, htop) | Drag normally — the drag is forwarded, so the child's own selection works. |
| **Anywhere else** (plain shell pane, pager, file list) | Hold **Shift** (Option/Fn on iTerm2) to hand the drag to your terminal, or `:mouse off`. |
| **Copying a diagnostic** | `:activity dump`, `:grep`, `:why-git` etc. open a pager — **`y`** yanks the source to the clipboard, **`Y`** the visible text. No mouse needed. |

spyc-owned selection for the surfaces it draws itself is planned, not built.

Clicking a *row* in the file list is deliberately not a thing: clicking a region is
coarse and hard to get wrong, while aiming at a one-cell-tall line invites
near-misses that silently move the cursor. `j`/`k`, `F`, and frecency jumps are all
faster and more precise.

`scroll_lines` defaults to **1** because trackpads and most modern terminals
already emit one wheel event per notional line — a multiplier there makes the list
fly. Raise it for a detented wheel that sends one event per physical click.

`scroll_lines` applies to the surfaces spyc scrolls itself. A pane forwarding to a
mouse-aware child is unaffected — the child receives one event per tick and picks
its own step. Clamped to at least 1.

## Colors — `[colors]`

Hex (`"#aabbcc"`) or named (`"red"`). Anything unset falls back to the built-in
palette. Match these to spyc's brand palette (see `docs/BRAND.md`) if you want
screenshots and the running tool to agree.

```toml
[colors]
dir           = "#82aaff"
exec          = "#c3e88d"
symlink       = "#89ddff"
file          = "#cccccc"
cursor_bg     = "#ff9e64"   # the cursor row
pick          = "#ffcb6b"   # multi-select
take          = "#c792ea"   # inventory
status_user   = "#bb9af7"
status_path   = "#7dcfff"
popup_border  = "#bb9af7"   # which-key / harpoon pop-up outline
# Diff / show / blame renderer:
diff_add_fg   = "#9ece6a"
diff_del_fg   = "#f7768e"
diff_add_bg   = "#122619"   # row wash behind +/- lines
diff_del_bg   = "#2a161b"
```

(`spyc --print-config` lists the full token set.)

---

## Ignore masks — `[[ignore_masks]]`

Two toggleable groups: group 1 (`a` key) and group 2 (`o` key). Patterns are
globs matched against the filename. **Defining any mask here replaces the
built-ins wholesale** — so redefine both groups if you customize.

```toml
[[ignore_masks]]
group = 1
enabled = true
patterns = [".*"]                       # dotfiles

[[ignore_masks]]
group = 2
enabled = true
patterns = ["*.o", "target/", "node_modules/", "*.pyc"]
```

---

## Quick Select patterns — `[[scan.patterns]]`

`^a u` (Quick Select) labels URLs / paths / SHAs / IPv4 in the visible pane;
lowercase yanks a match, uppercase opens it. Add your own patterns — they're
**appended** to the built-ins:

```toml
[[scan.patterns]]
name  = "ticket"                # label for the match kind
regex = "TICKET-[0-9]+"         # matched against the visible pane text

[[scan.patterns]]
name  = "ticket-link"
regex = "TICKET-[0-9]+"
url   = "https://tracker.example.com/browse/{}"   # uppercase-open target; {} = the matched text
```

A pattern with an un-compilable regex is skipped with a warning, not a crash.

---

## Keymap — the `keymap` DSL

One string per binding in a `keymap = [ ... ]` array. Forms:

| Form | Does |
|------|------|
| `map <KEY> unix <command...>` | run a shell command (`%` = current selection) |
| `map <KEY> command <:cmd...>` | run a `:` command (e.g. `graveyard`, `activity`) |
| `map <KEY> lua <name>` | run `~/.config/spyc/lua/<name>.lua` |
| `map <KEY> patternpick <glob>` | multi-select files matching a glob |
| `map <KEY> jump <path>` | jump the file list to a directory |

`<KEY>` is a single char (`f`), a Ctrl-combo (`^P`), or a named key (`<F2>`). The
DSL binds single keys — for multi-key chords, use `init.lua`'s `spyc.map`.

Several low-frequency features ship as `:` commands **without** a default key so
the keymap stays uncluttered — bind the ones you use:

```toml
keymap = [
  "map f unix file %",             # `file` on the cursor/selection
  "map ^P unix ps aux",
  "map H patternpick *.hpp",
  "map A command activity",        # toggle the activity monitor
  "map ^Y command graveyard",      # recover soft-deleted files
  "map z lua mymacro",             # ~/.config/spyc/lua/mymacro.lua
]
```

> Reminder: `unix` / `command` / `lua` / `jump` only bind from `~/.spycrc.toml`,
> not a project file.

---

## Lua scripting — `~/.config/spyc/`

For logic a DSL line can't express, embed Lua (mlua, vendored Lua 5.4). Two entry
points:

### 1. Per-key scripts

`map z lua mymacro` runs `~/.config/spyc/lua/mymacro.lua` on the keypress:

```lua
-- ~/.config/spyc/lua/mymacro.lua
local ctx = spyc.context()          -- { cwd, cursor_file, git_branch, picks, project_home, session_name, version, ... }
spyc.notify("on " .. ctx.cursor_file .. " @ " .. (ctx.git_branch or "no-branch"))
spyc.action("git_blame")            -- invoke any built-in action by its snake_case name
```

### 2. `init.lua` — keybinds, `:` commands, and event hooks

`~/.config/spyc/init.lua` runs once at startup (re-run with `:lua reload` / `^R`)
and registers callbacks that fire later:

```lua
-- ~/.config/spyc/init.lua

-- bind a key (multi-key chords work here, unlike the DSL)
spyc.map("g z", function()
  spyc.navigate(spyc.context().project_home)
end)

-- register a runtime ":" command
spyc.command("recent", function()
  for _, p in ipairs(spyc.git_log{ limit = 5 }) do
    spyc.notify(p.subject)
  end
end)

-- event hooks (low-frequency; fire on a transition, not every tick)
spyc.on("dir_changed", function(ev) spyc.warn("now in " .. ev.cwd) end)
spyc.on("agent_status", function(ev)   -- ev.pane, ev.state ∈ working|blocked|done|idle
  if ev.state == "blocked" then spyc.notify("pane " .. ev.pane .. " needs you") end
end)
```

Events: `startup`, `dir_changed` (`{cwd}`), `project_changed` (`{project_home}`),
`agent_status` (`{pane, state}`).

### The `spyc.*` API

- **Context:** `spyc.context()`, `spyc.cwd()`, `spyc.cursor()`.
- **Live reads** (synchronous, gitignore-aware, scoped to the search root):
  `spyc.worktrees()`, `spyc.git_status()`, `spyc.git_log{ limit = N }`,
  `spyc.read(path)`, `spyc.search_paths(query)`,
  `spyc.search_content(regex)` → rows of `{ file, line, text }`.
- **Drive the view:** `spyc.navigate(path)`, `spyc.pick(...)`,
  `spyc.clear_picks()`, `spyc.filter(...)`, `spyc.report_status(state)`.
- **Invoke behaviors:** `spyc.action("<snake_case_name>")` (any built-in action,
  e.g. `git_blame`), `spyc.cmd("<:command>")`.
- **Talk to the user:** `spyc.notify(msg)`, `spyc.warn(msg)`.

### Safety

Scripts run on a **dedicated worker thread** with an instruction-budget kill
switch and a hard 30-second ceiling; a script running longer than ~1s raises an
interactive `keep waiting? [y/N]` prompt (the loop stays responsive because the
interpreter is off the main thread). Disable Lua entirely with `:lua off` or the
`--no-lua` launch flag. Full charter: `docs/archive/LUA_SCRIPTING_PLAN.md`.

---

## See also

- **`spyc --print-config`** — the annotated template (source of truth for every
  TOML option).
- **`README.md`** — install + a quick tour.
- **`FEATURES.md`** — the full feature reference.
- **`docs/BRAND.md`** — the palette, for theming to match the brand.

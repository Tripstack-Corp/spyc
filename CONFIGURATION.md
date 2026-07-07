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

[pane]
default_command = "claude"     # pre-filled into the `^a c` new-tab prompt
new_tab_cwd = "worktree_root"  # focused column's worktree root (gw's target); or "project_home" (PROJECT_HOME), "browse_dir" (the focused column's dir)
claude_transcript_scrollback = false  # `^a v` reads Claude's JSONL transcript instead of terminal scrollback
codex_mcp = true               # register spyc's MCP server for codex panes

[yank]
include_pager_title = true     # prepend a source header to pager yanks (y/Y)

[pager]
tab_width = 4                  # columns a tab expands to (min 1)

[markdown]
open_as_rendered = true        # open .md in rendered view (m toggles source)

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

---

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

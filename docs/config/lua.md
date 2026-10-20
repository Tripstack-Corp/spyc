# Lua scripting — `~/.config/spyc/`

Part of [Configuring spyc](../../CONFIGURATION.md).

For logic a DSL line can't express, embed Lua (mlua, vendored Lua 5.4). Two entry
points:

## 1. Per-key scripts

`map z lua mymacro` runs `~/.config/spyc/lua/mymacro.lua` on the keypress:

```lua
-- ~/.config/spyc/lua/mymacro.lua
local ctx = spyc.context()          -- { cwd, cursor_file, git_branch, picks, project_home, session_name, version, ... }
spyc.notify("on " .. ctx.cursor_file .. " @ " .. (ctx.git_branch or "no-branch"))
spyc.action("git_blame")            -- invoke any built-in action by its snake_case name
```

## 2. `init.lua` — keybinds, `:` commands, and event hooks

`~/.config/spyc/init.lua` runs once at startup (re-run with `:lua reload` / `^R`)
and registers callbacks that fire later:

```lua
-- ~/.config/spyc/init.lua

-- bind a key: one key, as the DSL takes it (`z`, `^x`, `<F5>`)
spyc.map("<F5>", function()
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
spyc.on("agent_status", function(ev)   -- ev.pane, ev.state ∈ working|blocked|done|waiting|idle
  if ev.state == "blocked" then spyc.notify("pane " .. ev.pane .. " needs you") end
end)
```

Events: `startup`, `dir_changed` (`{cwd}`), `project_changed` (`{project_home}`),
`agent_status` (`{pane, state}`). Each fires on a transition, and a change a handler
makes itself (a `dir_changed` handler that navigates) doesn't fire it again, so hooks
can't loop; high-frequency events (cursor moves, pane output) aren't exposed.
Tab-completion lists the `:` commands `init.lua` registered, and `:lua status`
reports how many maps and commands it registered.

## The `spyc.*` API

- **Context:** `spyc.context()`, `spyc.cwd()`, `spyc.cursor()`.
- **Live reads** (synchronous, gitignore-aware, scoped to the search root):
  `spyc.worktrees()`, `spyc.git_status()`, `spyc.git_log{ limit = N }`,
  `spyc.read(path)`, `spyc.search_paths(query)`,
  `spyc.search_content(regex)` → rows of `{ file, line, text }`. They run on the
  Lua worker through spyc's MCP readers, with no round trip to the main loop. A
  failure (a bad path, not a repo, an invalid regex) raises a Lua error; nothing
  found is an empty table.
- **Drive the view:** `spyc.navigate(path)`, `spyc.pick(...)`,
  `spyc.clear_picks()`, `spyc.filter(...)`, `spyc.report_status(state)`.
- **Invoke behaviours:** `spyc.action("<snake_case_name>")` (any built-in action,
  e.g. `git_blame`), with an optional count (`spyc.action("down", 3)`), and
  `spyc.cmd("<:command>")`. `set_mark` and `jump_mark` need a mark letter, so
  reach them through `spyc.cmd`.
- **Talk to the user:** `spyc.notify(msg)`, `spyc.warn(msg)`.

## Safety

Scripts run on a **dedicated worker thread** with an instruction-budget kill
switch and a hard 30-second ceiling; a script running longer than ~1s raises an
interactive `keep waiting? [y/N]` prompt (the loop stays responsive because the
interpreter is off the main thread). The prompt waits while an editor or a `D`
pager has the keyboard, so a key typed there can't answer it. Lua is `$HOME`-only:
a project config can never run it, and `init.lua` loads only from
`~/.config/spyc/`. Disable Lua entirely with `:lua off` or the `--no-lua` launch
flag. Full charter: `docs/archive/LUA_SCRIPTING_PLAN.md`.

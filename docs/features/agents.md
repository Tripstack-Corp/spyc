# Agents and MCP

Part of spyc's [feature reference](../../FEATURES.md). How a hosted agent works with spyc: the tools it reaches, the skill that teaches it, and what spyc shows about it.

## MCP server (Claude + Codex integration)

spyc runs a background MCP server for the agents it hosts. Saying yes to an agent's first-launch
`[Y/n]` adds a `spyc` entry to that agent's own MCP configuration, once, so it
discovers spyc in every project — no `--mcp-config` flag needed:

- **Claude Code**: its user-scope MCP servers, through
  `claude mcp add-json -s user` (or your organization's `managed-mcp.json`).
- **codex**: `[mcp_servers.spyc]` in `$CODEX_HOME/config.toml`.
- **agy**: `~/.gemini/config/mcp_config.json`.

An agent always reaches the spyc that launched it, and so does one you start
by typing its name into a shell tab. That tab then follows the agent: it shows
the agent's activity dot while it runs, drops its status and session when it
exits, reads as a shell while it is stopped with `^Z`, and is saved as the shell
it was launched as. An agent that outlived its spyc (a Claude Code background
session across `spyc -r`) reaches the restarted one instead. How the entry finds
the right spyc: [ARCHITECTURE.md → "MCP server"](../../ARCHITECTURE.md#mcp-server).

**`spyc --install-agents`** does the same from the command line, without
waiting for a launch: claude's `settings.json` (in `$CLAUDE_CONFIG_DIR`, else
`~/.claude`) and its user-scope MCP servers; codex's `hooks.json` and
`config.toml`; agy's `~/.gemini/config/hooks.json` and `mcp_config.json`.
`--install-agents=agy` picks agents; `spyc --remove-agents` takes spyc's part
back out and leaves the rest of each file, and is what to run before
uninstalling spyc. spyc writes nothing into projects, and takes out the copies
older versions left there, unless another running spyc still uses them or git
tracks them. An organization's `managed-mcp.json` keeps control of claude's MCP
servers, and spyc installs only the hooks there. Files are edited in place:
a symlinked `settings.json` stays a link, a file that won't parse is left
alone with the reason printed, and an MCP server named `spyc` that runs
something else is never replaced. codex's `config.toml` keeps its comments and
layout. codex asks you to approve new hooks once, in `/hooks`, and from then on
they apply in every project, trusted or not.

spyc starts codex panes with **`--no-daemon`**, so codex runs the session
itself rather than on its shared background server, whose hooks and tools
can't tell which spyc or tab a session belongs to.
**`[pane] codex_daemon = true`** lets codex panes join it anyway, at the cost of
their activity dots and MCP attribution. A codex typed into a shell tab joins
it unless started with `codex --no-daemon`; its tab shows codex with an
output-timing dot, and `:why-status` says how to get hook status. A session on
the shared server never lights, or answers for, another tab. See
[`docs/HARNESS.md`](../HARNESS.md) → "codex's shared daemon".

**`[pane] codex_mcp = false`** (`.spycrc.toml`) stops spyc from registering
its MCP server for codex — an escape hatch for a codex `/review` bug
([openai/codex#25856](https://github.com/openai/codex/issues/25856)) where
codex mis-resolves the MCP tool-call approval elicitation and hangs on the
first spyc tool call. Status hooks still install (activity dots keep working);
codex just loses spyc's MCP tools. Claude is unaffected. Default on.

Every spyc on the machine shares that one entry, and each one's agents
reach it, with nothing to take over. An agent started outside spyc, in
another terminal, gets no spyc tools: only the agents spyc launches do.
Enterprise `managed-settings.json` policies
(`deniedMcpServers`/`allowedMcpServers`) are respected for the
claude side; codex has no equivalent enterprise hook.

Claude can query and control the workspace through these tools:

**Read tools:**
- **`get_spyc_context`** -- returns cwd, cursor file, picks, inventory,
  active filter, git branch, `project_home`, `session_name`, plus the
  running spyc's `pid` and `version` (`<x.y.z> (<git-sha>)`). The version
  string lets a client spot a stale server (a tool it expects is
  missing → compare the git SHA to the repo HEAD → restart spyc). Called
  from an agent pane it also returns `pane`, the caller's own tab (id,
  index, label, live cwd, worktree root, branch), so an agent in worktree
  X learns X while the user browses Y, and `project`, the project that tab
  belongs to (id, label, home, and whether it is the one the user is in).
  Every tool answers for and acts on the caller's project
- **`get_file_content`** -- reads a file's text content (up to 100KB)

**Write tools (Claude can mutate the TUI):**
- **`report_status(status, [pane], [ttl_ms], [note])`** -- self-report activity
  for your pane's dot: `working` / `blocked` (the "needs me" hot-red dot) /
  `done` / `waiting` (turn over, background work will wake you; `note` says on
  what) / `idle`. Overrides spyc's output-timing guess and targets the caller's
  own tab by default. A report lapses after a while unless renewed, except
  `blocked`, which stays until the user answers or a newer report replaces it.
  How reports, hooks and interrupts combine:
  [`docs/AGENT_ORCHESTRATION.md`](../AGENT_ORCHESTRATION.md).
- **`register_scope(paths, intent, [pr], [note])`** / **`list_scopes`** /
  **`release_scope(id)`** / **`wait_for_scope_clear(paths, [timeout_ms])`** --
  merge-coordination registry. Declare the files/globs you're touching and
  whether you're `editing` or `merging`; other agents `list_scopes` to see
  overlaps, or **`wait_for_scope_clear`** to *block* until no other `merging`
  claim overlaps theirs (bounded by a timeout — default 5m, cap 10m), so
  concurrent agents serialize instead of colliding. In-memory, session-persisted
  (survives `-r`), auto-released when your tab closes. Advisory — spyc never
  blocks a merge itself. Inspect in-TUI with `:agent list` / `:agent registry`.
- **`navigate_to`** -- change directory or focus cursor on a file
- **`set_filter`** -- set or clear the file listing filter (glob)
- **`pick_files`** -- pick files matching glob patterns (additive)
- **`clear_picks`** -- clear all picks
- **`create_worktree(branch)`** -- create a git worktree off the
  agent's project's repo (existing branch, else a new one off the
  default branch) in a sibling `<repo>.worktrees/<branch>/` dir;
  returns `{branch, path}`. The repo is `PROJECT_HOME`'s (a project with
  no home: its start directory's), else the calling agent's own pane's
  for a project outside any repo — never the directory the user happens
  to be browsing, which could be anything on their system.
  `list_worktrees` lists the same repo, and a relative path given to the
  other worktree tools resolves against `PROJECT_HOME` (a project with no
  home: its start directory).
  Lets a skill spin up a worktree to work in a second column while the
  first stays on its branch.
- **`remove_worktree(path)`** -- tear down a worktree by the path
  `create_worktree` returned, safe by default. It archives untracked and
  uncommitted content into the graveyard (recoverable, under
  `<worktree>-<timestamp>`), removes the worktree, and deletes the branch
  only if it's merged into the integration base. A worktree claimed with
  `claim_worktree` is refused; a column sitting inside is moved to
  PROJECT_HOME rather than refused.
  - **The removal can't strand a half-worktree.** A process still writing
    into the tree (a build into `target/`) can only leave orphaned bytes, and
    the reply names them.
  - **A worktree an earlier removal left half-done is finished, not
    refused**: calling `remove_worktree` on it again completes the job.
- **`clean_worktree(path)`** -- an alias of `remove_worktree`, kept for the
  tool name.
- **`open_worktree(path)`** -- open the second spyc column (column `b`)
  at the worktree (re-targets `b` if already open), so the agent can
  work in it while the main column stays put. After this,
  `navigate_to` / search / `pick_files` act on `b`. An agent-opened
  column `b` does **not** steal keyboard focus: the user keeps typing to
  the pane where the conversation is (only `^s n`, the user's own open,
  moves the keyboard into `b`). Nor does it destroy a **preview** you were
  reading there: the right column hosts one thing, so the preview is set
  aside with its split shape and `^s x` puts it straight back (re-read from
  disk, in case it changed meanwhile). Your own `^s n` over a preview is an
  explicit replace — nothing is owed back.

A column is never left stranded in a **deleted worktree**: if its
directory is removed out from under it by *any* means -- spyc's own
`remove_worktree`, an external `git worktree remove`, `rm -rf`, or
another agent -- the next listing refresh snaps that column back to
PROJECT_HOME (or, if PROJECT_HOME is gone too, the nearest existing
ancestor of the dead path) with a `directory not found, …` flash, so
a pane is never stranded.

**Search tools (gitignore-aware, scoped to the focused commander's worktree root — its repo root, else PROJECT_HOME, else cwd):**
- **`search_paths(query, [limit])`** -- fuzzy filename search,
  gitignore-aware and ranked like the `F` picker.
  Returns repo-relative paths, fzf-style ranked.
- **`search_content(pattern, [limit])`** -- regex content search,
  with the same matcher as `:grep`. Returns
  `{path, line, col, text}` objects.
- **`search_picks(pattern, [limit])`** -- *spyc-shaped*: content
  search restricted to the user's currently-picked files. Picks
  are TUI multi-select state Claude can't see otherwise, so this
  is the only way to grep the user's intended subset.
- **`search_inventory(pattern, [limit])`** -- *spyc-shaped*:
  content search over the persistent inventory cache (yanked
  files surviving across sessions). Lets Claude grep accumulated
  "interesting files" without re-establishing context.

**Git tools (read in-process, scoped like search):**
- **`git_status`** -- working-tree status as structured JSON,
  `{path, staged, unstaged, untracked}` per changed path.
- **`git_log([limit])`** -- recent commits newest-first,
  `{short_id, author, time, subject}` per commit.
- **`git_diff([cached], [unstaged], [paths])`** -- unified-diff text
  at one of three scopes: the working tree (staged + unstaged +
  untracked) vs HEAD (default), `cached:true` for staged-vs-HEAD, or
  `unstaged:true` for the index vs the working tree (plain `git diff` —
  only what changed since the last `git add`, the read for a
  stage-then-keep-editing checkpoint); `paths` restricts to specific
  files/subtrees. The read for reviewing your own changes before
  committing.

**Working in another worktree (`root`):** every read/search/git tool
above (`get_file_content`, `search_paths`, `search_content`,
`git_status`, `git_log`, `git_diff`) takes an optional `root` (an
absolute path) to target a *different* worktree than the user's
focused column — pass the path returned by `create_worktree` /
`list_worktrees`. Without it the tools follow the focused column (so
an agent editing in a sibling worktree should pass `root`).

Flash messages (`[mcp] navigated to src/`) tell the user when an
agent changes the workspace. The `gf`/`gF` keys complete the loop:
jump from Claude's output back to the file list.

## Installable agent skill

```sh
spyc --install-skill
#  → ~/.claude/skills/spyc/          (Claude Code)
#  → ~/.codex/skills/spyc/           (codex; honors $CODEX_HOME)
#  → ~/.gemini/config/skills/spyc/   (agy / Antigravity)
```

Writes an embedded usage guide into each agent's **personal** skills directory,
so it applies in every project without touching any repo.

All three agents read the same skill format, so the one copy spyc carries is
installed to each unchanged; only the directory differs.

Contents:

| File | Covers |
|---|---|
| `SKILL.md` | grounding via `get_spyc_context`, the shell-vs-spyc tool table, status reporting, scope coordination, the gotchas |
| `references/worktrees.md` | full worktree lifecycle, claims, the scope registry, the squash-merge kept-branch case |
| `references/search.md` | which of the four search corpora to use, and `root` scoping |
| `references/git.md` | the three `git_diff` scopes, and when to shell out anyway |

The skill is the depth under spyc's MCP server. The instructions the server
gives every agent are kept short, since an agent may repeat them alongside each
tool: which tools to prefer, how to target your own worktree, claims, safe
cleanup and status reporting. Each tool's description has its arguments; the
skill has the full workflows, and an agent loads it when it needs them.

**Staying current.** spyc offers a single `[Y/n]` update on startup when its
embedded copy differs from what's installed — one prompt covering every host,
naming the ones that are behind, and refreshing only those (accepting never
silently adopts a host you never installed to). If hosts disagree about whether
edits are at stake, the cautious warning wins. It only ever offers for an
**already-installed** skill — install it explicitly once, and you're never
nagged about a feature you didn't opt into. A decline is remembered against the
skill's content, so spyc asks again when the skill itself changes, not on every
version bump.

**Your edits are safe.** A `.spyc-skill.json` manifest records what spyc wrote,
which is what lets it tell "spyc's copy moved on" from "you edited this." An
edited skill is reported as such and never overwritten without saying so.

Manage it in-app with `:skill`:

| Arg | Does |
|---|---|
| `:skill` / `:skill status` | report installed version and whether it's stale or edited |
| `:skill update` | install/overwrite now (also clears a remembered decline) |
| `:skill remove` | delete `~/.claude/skills/spyc/` |
| `:skill ask` | forget a decline, so the next launch offers again |

## Native agent HUD

`:agent hud compact` enables a two-row strip below the active Codex, Claude
Code, or Antigravity pane; `:agent hud full` adds a tools row and a mode row.
Every agent shares one layout of aligned, coloured columns, led by the tab's
own activity dot, its session and model, with a `waiting` agent's note at the
end of the row. The tools row is a summary: every call, every failure, what is
running and the three most called, each marked for its own failures. A Claude
Code tab also gets a `todo` cell once `TodoWrite` or `TaskCreate`/`TaskUpdate`
has reported: completed over total, with how many are in progress. Usage scopes
stay separate, and unavailable context occupancy and limits stay explicit. A
Claude Code tab's context is a token count, followed by Claude Code's own
percentage and the plan's five-hour and seven-day usage once spyc's statusline
wrapper forwards them, each with when it resets: local clock time (`↻3:40pm`,
`↻Thu 9am`), or the time left with `[pane] agent_hud_resets = "countdown"`. `:agent hud off` stops collection; `:agent hud dump` (or
`info`), or a double-click on the strip, opens saveable source and freshness
diagnostics with every tool's tally. A drag across the strip copies its text.
The strip always leaves the agent at least six rows, and costs nothing while
the agent is idle. See [configuration and source semantics](../config/agent-hud.md#agent-hud--pane-agent_hud)
for the home-only preference and size fallbacks.

## Agent image gallery — what you actually pasted

An agent renders a pasted image as an opaque token (`[Image #3]`), so you
can't see what you attached and can't check it later. **`^a g`** (or
`:images`) lists the images the focused agent tab actually *received*, read
back out of its own transcript — so it survives spyc restarts and
`claude --resume` and Codex resume/fork:

```
  1. #1    jpeg    485 KB  yesterday   it would be helpful to show the creation date…
  2. #2    png      77 KB  4h ago      all numbers should be presented as "human friendly"
  3. #3    jpeg    493 KB  2h ago      columns in tables should be space aligned nicely:
```

It's a popup over whatever you were doing, not a view the file list
switches into — checking an attachment is a glance, not a destination.
`j`/`k` move, `1`-`9` jump, `Enter` (or `i`) shows one full-screen with
the usual image verbs (`s` save, `y` copy, `Y` copy the prompt it arrived
with, `b` base64, `o` external viewer), and `q` closes.

Images you've pasted but **not sent yet** lead the list, marked 📎:

```
  📎 #1    png     412 KB  just now    1920×1080, not sent yet
  1. #1    jpeg    485 KB  yesterday   it would be helpful to show the creation…
```

That half is what answers "what did I just attach?" *before* you hit
Enter — the moment the question actually gets asked. It works because an
agent reads the system clipboard itself when you press its paste key (`^v`
for Claude and Codex, also `Ctrl+Alt+V` for Codex on terminals that forward it).
The terminal can't carry image bytes, so spyc reads the same clipboard at that
moment and keeps a copy. Purely additive: the paste
still reaches the agent untouched.

These live in memory only — nothing is written to disk unless you save it
— and they're dropped when you submit the prompt, at which point the
agent's transcript becomes the record. Only agents with a known paste key
are watched, never a shell tab. Turn it off with `[pane]
preview_pasted_images = false`.

The **leading number is spyc's**, not the agent's. The agent's `[Image #N]`
is shown beside it for cross-reference but can't be the handle: its counter
restarts when a conversation is cleared or resumed, so the same label can
name two different pictures. Images the agent received without a label
appear too.

Claude and Codex are supported; other agents say their transcript format isn't.
A forked Codex session lists its parent's images up to the fork. An image sent
twice appears twice, and one attachment recorded more than one way appears once.
A Codex image sent inline still opens after its temporary file is gone; one that
can't be read reports why when you open it.

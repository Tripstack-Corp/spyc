---
name: spyc
description: Work inside spyc, the vi-keyboard terminal file/worktree manager, using its MCP tools instead of shell equivalents. Use for git worktree lifecycle (create/open/list/remove, never `git worktree`), gitignore-aware filename and content search (instead of `find`/`rg`/`grep`), in-process git status/log/diff (instead of shelling to git), reading and driving what the user is looking at, reporting agent status to the pane tab, and coordinating file scopes with other agents before editing or merging.
---

# spyc

You are running as an agent inside spyc's pty pane. spyc is a terminal file and
worktree manager; it exposes its own state and operations over MCP, so you can
see what the user sees and act on the real repository without shelling out.

Prefer spyc's tools over shell equivalents **mid-task**, not only when answering
questions about the user's view. They are in-process, gitignore-aware, and return
structured data, so they are both faster and less error-prone than parsing
command output.

## Ground yourself first

Call `get_spyc_context` before doing anything that depends on location. It
returns the user's cwd, cursor file, picked files, active filter, git branch,
`project_home`, session name, and the running spyc's pid + version.

This is what lets you skip asking "which file?" — the cursor and picks usually
*are* the answer. Relative paths you pass to other tools resolve against spyc's
cwd, so grounding first also makes those calls correct.

## Use these instead of the shell

| Instead of | Use | Why |
|---|---|---|
| `find`, `fd` | `search_paths` | fuzzy, gitignore-aware, structured |
| `rg`, `grep` | `search_content` | gitignore-aware regex, structured matches |
| `git status/log/diff` | `git_status`, `git_log`, `git_diff` | in-process (gix), no subprocess |
| `git worktree …` | `list_worktrees`, `create_worktree`, `open_worktree`, `remove_worktree`, `clean_worktree` | safe-by-default, graveyard-backed |
| `cat`, `head` | `get_file_content` | resolves against spyc's cwd |

Details: `references/search.md`, `references/git.md`, `references/worktrees.md`.

## Report your status

Call `report_status` as your turn changes. It drives the activity dot on your
pane tab, which is how the user sees at a glance which agent needs them:

- `working` — you started a non-trivial task
- `blocked` — **you stopped to ask a question or for permission.** This is the
  one that earns attention; it lights the tab hot-red.
- `done` — you finished a turn
- `idle` — nothing pending

Cheap and idempotent, so call it freely. It overrides spyc's output-timing guess,
which keeps your dot honest through long silent thinking. `blocked` is *latched*:
it stays until the user presses Enter into your pane or you send a newer report,
so it cannot be washed away by output.

## Working in another worktree

Every read tool takes an optional `root` (an absolute path) to target a worktree
other than the user's focused column. When you are working in a worktree you
created, pass its path as `root` — otherwise the tool reads the *user's* column
and you will silently get answers about the wrong tree.

If you need something spyc's tools don't cover in that worktree, shell out with
explicit absolute paths rather than relying on cwd.

## Coordinate before you edit or merge

When more than one agent is active, declare your scope first:

1. `register_scope(paths, intent="editing"|"merging", pr?, note?)` → returns
   `{claim_id, conflicting_merges}`
2. `list_scopes` to see who else is touching what, and who is mid-merge
3. `wait_for_scope_clear(paths, timeout_ms?)` **blocks** until no other owner's
   `merging` claim overlaps yours
4. `release_scope(id)` when done (auto-released when your tab closes)

Advisory, not enforced — but it is how you avoid two agents rewriting the same
file. Full protocol in `references/worktrees.md`.

## Gotchas that actually bite

- **No shell cwd continuity.** Each `Bash` call is a fresh subprocess inheriting
  your *original* launch directory. `cd /foo` does not persist. Use the compound
  form (`cd /foo && cmd`) or absolute paths (`make -C /path`). If a `make` or
  `cargo` command fails unexpectedly, run `pwd && ls` before retrying — this is
  the single most common source of confused loops.
- **`root` is per-call.** Setting it on one tool does not scope the next.
- **The user's view is shared state.** `navigate_to`, `pick_files`, `set_filter`
  move what they are looking at. Useful for showing your work; disruptive if you
  do it without saying so.
- **Picks are a real selection.** If `get_spyc_context` shows picks, the user
  probably means those files. Prefer them over guessing from a description.

## If the tools are not there

If `get_spyc_context` is unavailable, say so plainly:

> I don't see the spyc MCP tools — are we running inside spyc?

Then fall back to shell equivalents. Also worth checking: if a tool you expect is
missing but others work, compare the `version` from `get_spyc_context` against
the repo HEAD — a long-running spyc can be older than the checkout, and the fix
is for the user to restart it.

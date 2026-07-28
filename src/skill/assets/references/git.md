# Git

spyc's git is **100% in-process** via gix — no subprocess. Use these rather than
shelling out: the results are structured, and they can't be confused by a
different cwd than you expect.

```
git_status(root?)
git_log(limit?, root?)
git_diff(cached?, unstaged?, paths?, root?)
```

## The three diff scopes

This is the part worth getting right:

| Call | Compares | Answers |
|---|---|---|
| `git_diff()` | working tree vs HEAD | "everything I've changed" |
| `git_diff(cached: true)` | staged vs HEAD | "what a commit would contain" |
| `git_diff(unstaged: true)` | index vs working tree | "what changed since my last `git add`" |

`unstaged: true` is the one to reach for after a staged checkpoint — it shows only
what moved since then, instead of re-reading the whole delta.

Narrow with `paths` when you care about specific files; it's cheaper and the
output is easier to reason about than filtering a full diff yourself.

## When to shell out anyway

spyc covers status, log, and diff. Everything else is still git's job:

- committing, branching, rebasing, tagging
- `git push` / `gh pr` operations
- anything the tools above simply don't expose

Two rules when you do:

1. **Use explicit absolute paths or `-C <repo>`.** There is no cwd continuity
   between calls, so a bare `git commit` may run in the wrong tree.
2. **Never `git worktree`** — see `worktrees.md`. That one is not a gap in
   coverage; it's deliberately spyc's.

## Reading before writing

Before proposing or applying an edit, `git_status` cheaply tells you whether the
tree is already dirty. That changes what "revert this" means, and it's how you
avoid mixing your change into someone else's uncommitted work.

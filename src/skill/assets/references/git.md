# Git

spyc's git is **100% in-process** via gix — no subprocess. Use these rather than
shelling out: the results are structured, and they can't be confused by a
different cwd than you expect.

```
git_status(root?)
git_log(limit?, root?)
git_diff(cached?, unstaged?, base?, paths?, root?)
```

## The diff scopes

This is the part worth getting right:

| Call | Compares | Answers |
|---|---|---|
| `git_diff()` | working tree vs HEAD | "everything I've changed" |
| `git_diff(cached: true)` | staged vs HEAD | "what a commit would contain" |
| `git_diff(unstaged: true)` | index vs working tree | "what changed since my last `git add`" |
| `git_diff(base: "main")` | where the branch left `main` vs working tree | "what has this branch changed?" |

`unstaged: true` is the one to reach for after a staged checkpoint — it shows only
what moved since then, instead of re-reading the whole delta.

`base` is the one for a branch as a whole: its commits since it forked plus its
uncommitted and untracked work, and none of what landed on `main` after the fork.
Any rev works (`base: "origin/main"`, `base: "HEAD~3"`); an empty string means the
repo's default branch. A local branch with an `origin/` counterpart counts from
whichever fork is later, so a branch rebased onto `origin/main` doesn't claim the
commits local `main` hasn't pulled. It can't be combined with `cached` or
`unstaged`.

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

# Worktrees, claims, and scope coordination

**Never run `git worktree`.** spyc owns worktree lifecycle so it can archive your
uncommitted work to its graveyard before removing anything, refuse to delete a
tree another session claimed, and keep its own columns consistent. Shelling out
bypasses all three.

## Creating one

```
create_worktree(branch, base?, open?)  →  {branch, path}
```

- Existing branch is reused; otherwise it is created off `base` (default: the
  repo's default branch).
- Lands in a sibling `<repo>.worktrees/<branch>/`, always anchored on the **main**
  repo even when called from inside a linked worktree.
- `open: true` also opens it in column b and focuses it, so you can work there
  immediately. Without it, the tree exists but the user's view doesn't move.
- Errors if the branch is already checked out elsewhere.

**Then pass that returned `path` as `root`** to every read tool while you work
there. This is the step people forget; without it you keep reading the user's
column.

## The PR flow this exists for

Setting up isolated work:

1. `create_worktree(branch, open: true)` — never `git checkout -b` in the main
   checkout
2. edit, and run the project's gate before committing
3. commit, push, open the PR
4. after it merges: `remove_worktree(path)`

Some repos hard-enforce this with a hook that blocks edits to the main checkout.
If an edit is rejected for that reason, the fix is a worktree, not a workaround.

## Listing and safety signals

```
list_worktrees()
```

Per tree: branch, dirty counts, which is current, ahead/behind the base,
**`merged`** (the safe-to-remove signal), and whether it is `locked` (claimed by
another session). Read `merged` before proposing a teardown.

## Removing one

```
remove_worktree(path)      # safe by default
clean_worktree(path)       # same, explicit about archiving
```

Safe-by-default means, in order: archive untracked + uncommitted changes to
spyc's graveyard (recoverable), remove the worktree, then delete the branch
**only if it is merged**. An unmerged branch's ref is kept deliberately — it is
the backup of your commits.

Refuses a worktree claimed by another session; release it first. A spyc column
sitting inside the tree is reset to `project_home` rather than refusing.

### The squash-merge gotcha

With squash merges, the branch's own commit is **not** an ancestor of the squashed
commit on main. So `remove_worktree`'s ancestry check reports
`kept branch '<name>' (1 commit not in base)` even though the content shipped.
That is conservative, not wrong.

To confirm it really landed before deleting the ref:

```
git diff --stat <branch> main     # empty output == content identical
git branch -D <branch>
```

Check the diff first. Don't force-delete on the assumption that a merged PR means
a merged ref.

## Claims (leases between sessions)

```
claim_worktree(path, reason)
release_worktree(path)
```

A claim is advisory but honored by `remove_worktree`, which refuses a claimed
tree. Claim one when you'll be working in it across several turns so a parallel
agent doesn't tear it out from under you.

## Scope registry (merge coordination)

Separate from worktree claims: this is about *files*, and it is the mechanism for
not colliding with other agents.

```
register_scope(paths, intent="editing"|"merging", pr?, note?)
  → {claim_id, conflicting_merges}
list_scopes()
wait_for_scope_clear(paths, timeout_ms?)   # blocks
release_scope(claim_id)
```

- Declare **before** touching or merging files. The returned
  `conflicting_merges` tells you immediately whether someone is mid-merge on
  paths you care about.
- `intent: "merging"` is the stronger claim — `wait_for_scope_clear` blocks only
  on other owners' `merging` claims, not on `editing` ones.
- `wait_for_scope_clear` parks in spyc's event loop (no busy-wait). Default
  timeout 5 minutes, capped at 10.
- Claims are in-memory but session-persisted, so they survive `spyc -r`.
- Auto-released when your tab closes; still call `release_scope` when you finish,
  so a long-lived tab doesn't hold a stale claim.

Inspect the registry in-TUI with `:agent list` / `:agent registry`.

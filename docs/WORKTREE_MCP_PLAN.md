# spyc worktree MCP — encapsulate the housekeeping, default to the graveyard

**Status:** Committed + greenlit for implementation (2026-06-21); a product-launch priority. Owner decisions are baked into §11. This charter turns a
dogfooding observation into a plan: when Claude cleaned up six worktrees in
this repo it called spyc's MCP exactly *once* (`get_spyc_context`) and ran the
entire inspect-and-decide loop on raw `git`. The reason is structural — spyc's
MCP covers *orient* and *act*, but not *inspect/decide*, which is the phase that
actually dominates (and endangers) worktree cleanup. spyc already computes all
the needed git intelligence in-process via `gix`; it just isn't exposed. This
plan exposes it and makes safe removal the easy default by routing at-risk
content to the **graveyard** instead of refusing (or leaving the user to back
up to `/tmp` by hand).

**Goal:** an agent (or the user) can run a complete worktree-cleanup loop —
*enumerate the board → judge what's safe → remove safely → verify* — entirely
through spyc tools, with spyc encapsulating the git housekeeping and the
graveyard as a built-in undo. No shelling out, no hand-rolled `format-patch`
backups, no `git worktree remove --force` that can silently drop work.

**Non-goals:** a general-purpose git CLI over MCP; multi-split worktrees; the
"repo bootstrap / tutor" feature (a separate backlog item — noted as a
follow-on in §9, not built here).

---

## 1. The gap, in terms of the agent loop

A maintenance task has four phases. Today's MCP surface only covers two:

| Phase | What's needed | MCP today | Fallback used |
|-------|---------------|-----------|---------------|
| **Orient** | "what worktrees exist, where am I" | `get_spyc_context` (current branch/cwd only — no inventory) | `git worktree list --porcelain` |
| **Inspect / decide** | per-tree dirty status; is the branch merged / ahead-behind; what would I lose | **nothing** | `git status`, `git branch --merged`, `git log main..b`, `git diff`, `merge-base` |
| **Act** | remove the tree + branch, preserve at-risk content | `create/open/clean/remove_worktree` (no branch delete; `remove` *refuses* dirty rather than preserving) | `git worktree remove --force` + `git branch -D` + `format-patch` to `/tmp` |
| **Verify** | confirm final state | `get_spyc_context` (partial) | `git worktree list` |

The inspect/decide phase has **zero** coverage and is the most dangerous one —
it's where "is it safe to delete?" gets answered. With no tool, the agent is
forced into shell, and once in shell it stays there for the act phase too. The
irony: spyc is a `gix`-powered, git-aware file manager that already computes
per-file status (the gutter), diffs (`|`), and worktree discovery for its own
UI. None of that intelligence is reachable over MCP.

---

## 2. What already exists (reuse, don't reinvent)

The research that backs this plan found most building blocks already present
and, conveniently, **pure** — the git facade is "paths in, owned `Send` data
out, no `App` dependency" (`src/git/mod.rs:8`), which means these can run *off
the main thread* (see §5).

**Ready to wire up as-is:**

- `git::worktree::list(dir) -> Vec<Worktree { path, head, branch }>`
  (`src/git/worktree.rs:64`) — main-first ordering, in-process gix. *Gap: no
  status/ahead-behind/merged fields on `Worktree`.*
- `git::status::repo_status(root)` / `repo_status_stable(root)`
  (`src/git/status.rs:212` / `:352`) — per-file staged/unstaged/untracked,
  racy-snapshot-guarded; can be pointed at any worktree path.
- `git::diff_model::diff_head_to_worktree(root, paths)` (`:53`),
  `diff_cached(root, paths)` (`:120`), `show_model(root, rev)` (`:175`)
  (`src/git/diff_model/mod.rs`) — structured `DiffModel` (`src/git/model.rs`),
  100% owned data, already serializable. `CommitMeta` (`model.rs:132`) too.
- `git::blame::blame(root, path)` (`src/git/blame.rs:21`).
- `git::discovery::head_branch(root)` (`:37`), `gitdir(root)` (`:27`).
- **Graveyard:** `state::graveyard::write_entry(src)` /
  `write_entry_as(src, label, orig)` / `restore` / `load` / `cascade_*`
  (`src/state/graveyard.rs`) — tar.zst + JSON metadata, global store under
  `$XDG_STATE_HOME/spyc/graveyard/`, 500 MB FIFO cascade. Off-thread lane
  already exists: `Effect::Graveyard(GraveyardOp)` → worker →
  `Message::GraveyardDone` (`src/app/graveyard_ops.rs`, `src/app/effect.rs`).
- `worktree_clean::clean_worktree(path) -> CleanReport`
  (`src/app/worktree_clean.rs:36`) — already archives **untracked** files to
  the graveyard under a `<name>-<timestamp>` label, then removes. This is the
  seed of the safe-by-default behavior; we generalize it.

**MCP plumbing (how a tool is added), for reference:**

- Schema: `handle_tools_list` JSON array, `src/mcp/protocol.rs:131–339`.
- Dispatch: `handle_tools_call`, `src/mcp/protocol.rs:342–537` — read-only
  tools run **on the socket thread**; writable tools send an `McpCommand` and
  block ≤5s on a reply.
- Command enum: `McpCommand`, `src/mcp_cmd.rs`.
- Handler: `execute_mcp_command`, `src/app/mcp.rs:231–470` (runs on the **main
  thread**, mutates `AppState` directly, replies synchronously — *no Effect*).
  Worktree-arg safety: `resolve_worktree_arg`, `src/app/mcp.rs:203`.
- Server-side guidance: `SERVER_INSTRUCTIONS`, `src/mcp/mod.rs:36`.

**Genuinely new work** (gix primitives are available — `revision` feature is
on — but unwrapped):

- `git::branch_status` — merged-into-base + ahead/behind (needs `merge_base` /
  `is_ancestor`). None of this exists today; confirmed absent.
- `git::log(root, rev, limit) -> Vec<CommitMeta>` — walk via existing
  `find_commit` / `parent_ids`.
- `git::branch::delete(root, name, force)` — ref deletion (none exists; gix can
  delete refs).
- `graveyard::write_blob(bytes, label)` — archive arbitrary bytes (a `.patch`),
  not just filesystem paths (today's API is path-centric). Small wrapper over
  `write_entry_as` via a temp file.

---

## 3. Proposed tool surface

Phased so the **core safety win ships first**. Each tool notes read-only (R) vs
writable (W) and its execution lane (§5).

### Phase 1 — inventory + safe removal (the heart of the ask)

**`list_worktrees`** (R, socket thread) — the missing orient/inspect entry
point. Returns the whole board, one entry per worktree:

```json
{ "path", "branch" | "detached_head": "<sha7>",
  "is_current": bool,            // the focused column's cwd is in this tree
  "is_open_b": bool,             // column b is open here
  "dirty": { "staged": N, "unstaged": N, "untracked": N },
  "merged_into_base": bool,      // base = repo default branch (§7)
  "ahead": N, "behind": N,       // vs base
  "head": "<sha7>", "subject": "<last commit subject>" }
```

Replaces, in one call, the `git worktree list` + per-tree `git status` +
`git branch --merged/--no-merged` + `git rev-list --count` dance. This single
tool is the highest-leverage item in the plan.

**`remove_worktree`** (W; heavy work off-thread, see §5) — *enhanced* to be
**safe-by-default** rather than refuse-on-dirty. Default behavior:

1. Enumerate at-risk content (`repo_status(path)` for untracked + uncommitted
   tracked changes; `branch_status` for unmerged commits).
2. Archive to the graveyard under a `<name>-<timestamp>` label:
   - untracked files (existing `clean_worktree` path), and
   - a unified-diff `.patch` of uncommitted tracked changes
     (`diff_head_to_worktree` → serialize → `write_blob`).
3. Remove the working tree (`git::worktree::remove`).
4. Branch: `delete_branch: "auto" | "always" | "never"` (default `auto` =
   delete **only if merged into base**; keep otherwise). Keeping an unmerged
   branch's ref is what preserves its commits — see §6.

Returns a report: `{ removed, branch_deleted, graveyard_label,
archived: { untracked_files: N, diff_patch: bool } }`. Escape hatches:
`archive: false` (skip graveyard) and `force: true` (remove even if a column is
open — still guarded by default).

**Decision (owner, §10):** fold `clean_worktree` into this safe-default
`remove_worktree`, keeping `clean_worktree` as a thin documented alias for
back-compat (it's referenced in `SERVER_INSTRUCTIONS` + AGENTS.md). Recommended:
yes — "remove" *is* the safe path; a separate "clean" verb just splits the
mental model.

**`create_worktree` — base off PROJECT_HOME by default (POLA fix).** Today
`create_worktree` branches the new worktree off the *focused column's current
HEAD* (`worktree::add` uses the column's `listing.dir`, `src/app/mcp.rs:331`) —
the surprising behavior we hit live: with column A switched to a feature branch,
"new worktree for branch X" silently based X on that feature branch instead of
`main`. **Decision (owner): default the base to PROJECT_HOME's default branch**
(the §7 resolver, anchored on `project_home`, not the focused column), with the
explicit `base` override (Phase 3) for the rarer "stack on my current branch"
case. PROJECT_HOME must resolve to the *main repo* root, not a linked worktree.

### Phase 2 — inspection read tools (general decide-phase coverage)

All R, all socket-thread, all thin wrappers over the pure `git::` facade:

- **`branch_status`** `(branch?, base?)` → `{ merged, ahead, behind,
  merge_base }`. The "is it safe to delete this branch" answer. *New facade
  work.*
- **`git_status`** `(path?)` → the `repo_status` result for a worktree.
- **`git_diff`** `(path?, rev?, staged?)` → serialized `DiffModel`
  (worktree / staged / a specific commit). Pure reuse of the diff-model layer.
- **`git_log`** `(rev?, limit?)` → `Vec<CommitMeta>`. *New facade work
  (small).*

### Phase 3 — convenience (optional)

- `create_worktree` enhancements: a `base` ref and an `open: true` to open
  column b in one call. Possibly an arbitrary-repo target (today it anchors on
  the focused column's repo, `src/app/mcp.rs:331`).

---

## 4. Graveyard-as-default safety model

This is the user's core ask: *"by default just leverage the graveyard to make
it safe and easy to clean up (vs. using /tmp)."*

**What's at risk on removal, and how each is preserved:**

| At-risk content | Preservation | Recoverable? |
|-----------------|--------------|--------------|
| Untracked files | graveyard archive (existing `clean_worktree`) | fully (`restore`) |
| Uncommitted tracked edits | `diff_head_to_worktree` → `.patch` → `write_blob` | as a patch to re-apply |
| Unmerged commits | **keep the branch ref** (don't delete) | fully — commits live on the branch |
| Merged commits | already in base; branch deletion is safe | n/a |

**Key insight that simplifies everything:** the branch ref *is* the natural
backup for commits. So the plan does **not** try to `format-patch` unmerged
commits into the graveyard as the primary safety net — it simply **keeps the
branch** when commits are unmerged (`delete_branch: auto`). The graveyard
handles what a branch ref *can't* hold: untracked files and uncommitted edits.
This is both safer (full commit recoverability via the ref + reflog) and far
less code than bundling commits.

**The one graveyard extension needed:** `write_blob(bytes, label)` so a
generated `.patch` can be archived without a real filesystem path. Implemented
as: temp file → `write_entry_as(tmp, label, synthetic_orig)` → unlink temp.
Everything else (tar.zst, metadata, restore, 500 MB cascade, the graveyard
viewer `gy`) works unchanged. Archived patches show up in the existing
graveyard UI and restore like any other entry.

**Stretch (deferred):** a proper `git bundle` of unmerged commits for the
`delete_branch: always` case, giving full history recovery even after the ref
is gone. Listed in §10; not needed for the safe default.

---

## 5. The central design decision: keep the main loop unblocked

This is the load-bearing engineering call. Today, **writable** MCP commands run
on the **main thread** inside the pre-recv drain (`execute_mcp_command`,
`src/app/mcp.rs`), and the socket thread blocks ≤5s. Running a multi-worktree
status walk (`list_worktrees`) or a status + diff + tar.zst archive
(`remove_worktree`) on the main thread would **block render/input** — exactly
the blocking-IO-on-the-hot-path anti-pattern this project has repeatedly fought
(and a 12-worktree archive could blow the 5s timeout outright).

**The architecture already gives us the way out:** the `git::` and
`state::graveyard::` layers are *pure* (paths in, owned data out, no `App`).
So the slow work doesn't need the main thread at all.

**Recommended model — two execution lanes:**

1. **Read tools (`list_worktrees`, `branch_status`, `git_status`, `git_diff`,
   `git_log`)** run entirely **on the socket thread**, like `search_content`
   does today. They call the pure `git::` facade directly + read cwd/column
   state from the context file (`read_cwd_from_context`, `src/mcp/readers.rs`).
   Zero main-thread involvement → the loop never stalls. (Column-b / current
   flags are best-effort from whatever the context snapshot exposes — the
   session-two-cwd work put a second cwd there.)

2. **Mutating tools (`remove_worktree`, `create_worktree`)** split into:
   - a **pure off-main phase** — the status walk, `.patch` generation,
     graveyard archive, `git::worktree::remove`, `git::branch::delete` — run on
     the socket thread (all path-based, no `App`); then
   - a **tiny main-thread reconcile** — refresh the listing if the removed tree
     was on screen, flash a message, `write_context()` — dispatched as a
     lightweight `McpCommand::ReconcileAfterWorktreeChange { .. }`.

   The socket thread does the slow work (blocking only itself), hands a quick
   reconcile to main, and replies to the client. Because the archive runs on
   the socket thread, **bump that tool's reply timeout** (e.g. 30s) — it's a
   user-initiated cleanup, not an interactive nav.

**Rejected alternative:** routing the archive through the existing async
`Effect::Graveyard` worker. It's already off-thread, but it replies via
`Message::GraveyardDone` (fire-and-forget) — incompatible with MCP's synchronous
request/reply, and it would report "removed" before the archive finished
(breaking read-after-write). Keep the archive synchronous on the socket thread;
revisit the worker only if archives of huge untracked trees become a problem.

This lane split is the bit most worth getting right in review — it's what keeps
the feature from regressing the very responsiveness the project guards.

### 5.1 Async / Tasks — staged adoption (decided 2026-06-24)

Live evidence from a dogfooding cleanup: `remove_worktree` on a worktree with a
large cargo `target/` tripped the 5s reply timeout ("spyc did not respond within
5 seconds") **even though the off-main job completed fine** — only the
socket-thread reply correlation gave up. The fix is an *async shape*, and MCP now
has a first-class one.

**What the protocol offers.** The 2025-11-25 spec adds an **experimental**
[Tasks utility](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks):
a task-augmented `tools/call` returns a `CreateTaskResult` with a `taskId`
immediately, work continues in the background, and the requestor polls
`tasks/get` → fetches via `tasks/result` (blocks to terminal) → can enumerate
history via `tasks/list` and abort via `tasks/cancel`. It's purely additive and
**server-opt-in per tool** (`tools/list` advertises `execution.taskSupport:
optional|required|forbidden`), so we could expose Tasks for *only* the heavy
worktree mutations and leave the other ~16 tools synchronous. `"optional"` keeps
old clients working; `"required"` would break them (`-32601`).

**Two facts that gate us:** (1) it's flagged experimental — "design and behavior
may evolve"; (2) the *benefit is client-gated* — Tasks does nothing until the
client (effectively just Claude Code for us) declares the `tasks` capability and
actually augments the call + polls. spyc is a single-user, single-client, local
socket server, so the *standardization* payoff is muted — the win is the async
*shape*, which we can deliver at the app level without the wire format.

**Decision — do NOT adopt the `rmcp` SDK.** It's the mature official SDK, but
it's **tokio-first with no sync API**, and spyc is deliberately std-threads + one
blocking `recv` (0 wakes at idle, no busy-poll — a stated MVU invariant). It also
expects per-request task spawning, which fights our read-on-socket-thread /
delegate-writes-to-the-loop model, needs custom-transport glue for our stdio↔unix
-socket bridge anyway, and drags a large dep tree into a small-deps project. Our
hand-rolled MCP (~2.5 KLoC on `serde_json`, zero MCP deps) uses a *tiny, stable*
slice of the protocol; tracking it by hand is cheap. (One narrow hedge worth a
quick check if we ever want spec-exact wire structs: whether `rmcp` publishes its
protocol **types** as a runtime-agnostic crate we could pull *without* tokio.)

**Plan — three stages, nothing wasted:**

- **Stage 0 (interim, done in the PR that adds this section):** give the heavy
  worktree mutations (`create/remove/clean_worktree`) a 60s reply timeout instead
  of the interactive 5s (`src/mcp/protocol.rs`). Kills the false "did not respond"
  for the common big-`target/` case. No protocol change.
- **Stage 1 (with the safe-`remove_worktree` work, §3/§10):** build the async
  shape at the *app* level — a heavy mutation returns a `task_id`, a read-only
  `task_status` reader on the socket thread reports progress, and a small bounded
  in-memory registry doubles as the "tasks performed" history. Deliberately
  **mirror the spec's vocabulary** (`taskId`, `working/completed/failed`,
  `tasks/list`-shaped listing) so Stage 2 is a rename, not a rewrite. Rides the
  existing off-main `worktree_ops.rs` lane.
- **Stage 2 (only when Claude Code advertises `tasks`):** bump the advertised
  `protocolVersion` to `2025-11-25`, declare `tasks.requests.tools.call`, mark the
  heavy tools `execution.taskSupport: "optional"`, and wrap the Stage-1 registry
  in the real `tasks/get|result|list|cancel` methods. Track the client; don't lead
  it.

---

## 6. Branch deletion semantics

No branch-deletion code exists today (`git::worktree::remove` deliberately
leaves the ref, `src/git/worktree.rs:319`). Add `git::branch::delete(root,
name, force)` over gix ref deletion. `remove_worktree`'s `delete_branch`:

- `"auto"` (default): delete iff `branch_status.merged == true`. Unmerged →
  keep (commits stay alive on the ref).
- `"always"`: delete regardless (with the stretch `git bundle` archive if built;
  otherwise rely on reflog + a warning in the response).
- `"never"`: never delete; just remove the working tree.

Detached-HEAD worktrees (like the `spyc-recover` one in the motivating session)
have no branch → `delete_branch` is a no-op; the report says so.

---

## 7. Base-branch detection

`branch_status` / `merged_into_base` need a base. Resolve in order:
`origin/HEAD` symref → `main` → `master` → first of `git branch`. Accept an
explicit `base` override on the tools. For `create_worktree` the base resolves
from **PROJECT_HOME** (the main repo root), not the focused column (§3 Phase 1).
Minor work; no new dependency.

**POLA, part two — the worktree *path* (fixed):** the base-branch fix (#511)
left a sibling bug. `git::worktree::add` built the `<repo>.worktrees/<branch>`
path from the workdir `gix::discover` returned for the asking dir — so creating
a worktree while the asking column sat *inside* an existing linked worktree
nested the new one under `<linked>.worktrees/<branch>`. Fixed by anchoring the
path on the **main** worktree root (resolved from the shared `common_dir`, the
same way `list()` does), independent of where the asking column is. One change
in `add`; covers both the MCP and the `W n` TUI path.

---

## 8. Testing

Follows the testing-campaign lessons (pure layers carry the load; unit tests
stay **chdir-free** — `set_current_dir` races the parallel runner):

- **Pure facade fns** (`branch_status`, `log`, `branch::delete`) — unit tests
  against temp repos built with the real `git` binary, asserted via gix (the
  established `src/git/` test pattern).
- **`graveyard::write_blob`** — round-trip test (archive bytes → `load` →
  `restore` → compare).
- **Safe-remove integration** — temp repo + worktree with untracked + uncommitted
  + unmerged commits; assert the graveyard gained the right labeled entries, the
  tree is gone, and the branch survived (unmerged) / was deleted (merged).
- **MCP** — extend the `initialize_response` / tool-list assertions; a
  `list_worktrees` golden over a fixture repo.
- **No regression of the hot path** — assert the mutating handler's main-thread
  reconcile does no git/fs walking (the heavy work is on the socket thread).

---

## 9. Delivery as a "skill" (agent adoption)

"Make the agent actually use these" is a solved question here — see the merged
MCP-instructions work (PR #482): spyc sets the MCP `initialize` `instructions`
field (`SERVER_INSTRUCTIONS`, `src/mcp/mod.rs:36`), which clients fold into the
system prompt. Prior research rejected shipping `.claude/skills/` files
(repo pollution) and CLAUDE.md edits in favor of this ephemeral, spyc-controlled
channel.

**So the "skill" here = updating `SERVER_INSTRUCTIONS`** to teach the
inventory-first, graveyard-safe workflow — e.g.: *"To clean up worktrees:
`list_worktrees` to see the board with dirty/merged status; `remove_worktree`
(safe by default — archives untracked + uncommitted edits to the graveyard and
keeps unmerged branches); recover from the graveyard, never `/tmp`."* Keep it
tight (clients truncate). Update it in the same PR that ships Phase 1.

**Follow-on (out of scope):** the backlog's "installable repo-bootstrap / tutor
skill" (`~/spyc_tutor`, lorem-ipsum fixtures, lesson goals) is a separate
feature that would *build on* this MCP surface. Tracked, not built here.

---

## 10. Order of attack

| # | PR | Scope | New facade work | Risk |
|---|----|-------|-----------------|------|
| 1 | `feat/git-branch-status` | `branch_status` + `merge_base`/`is_ancestor` + base detection (pure, tested) | yes | low |
| 1b | `fix/mcp-create-worktree-base` | `create_worktree` defaults base to PROJECT_HOME's default branch (POLA); reuses #1's resolver | small | low |
| 2 | `feat/graveyard-write-blob` | `write_blob(bytes, label)` (pure, tested) | yes | low |
| 3 | `feat/mcp-list-worktrees` | `list_worktrees` (R, socket thread) + `Worktree` status fields; update `SERVER_INSTRUCTIONS` | small | low |
| 4 | `feat/mcp-safe-remove-worktree` | safe-by-default `remove_worktree` + `branch::delete` + off-main lane + reconcile cmd; fold/alias `clean_worktree` | yes | **med** (the §5 lane split — review carefully) |
| 5 | `feat/mcp-git-read-tools` | `git_status` / `git_diff` / `git_log` (R) | small | low |
| 6 | `feat/mcp-create-worktree-ext` | `create_worktree` `base` + `open` | none | low |

Ship 1–4 for the complete safe-cleanup loop; 5–6 are additive.

## 11. Open decisions for the owner

1. **Fold `clean_worktree` into `remove_worktree`** (safe-by-default), keeping
   `clean` as an alias? (Recommend: yes.)
2. **`delete_branch` default = `auto`** (delete only if merged)? (Recommend: yes.)
3. **Unmerged-commit safety = keep the branch ref** (recommend) vs. also
   build the `git bundle` archive now (stretch)?
4. **Phase-2 read tools** (`git_diff`/`git_status`/`git_log`) in v1, or just
   `list_worktrees` + `branch_status` (enough for safe removal)?
5. **Mutating-tool reply timeout** — 30s for the archive path? Acceptable?
6. **`list_worktrees` column flags** — how much column-b/current state does the
   context snapshot actually expose to the socket thread today? (Verify before
   committing to the `is_open_b`/`is_current` fields.)
7. **`create_worktree` base** — DECIDED (owner, 2026-06-21): default to
   PROJECT_HOME's default branch, not the focused column's HEAD (POLA). The
   explicit `base` override stays. (See §3 Phase 1, §7.)

---

## 12. Docs to update (same PR as each change, per the contract)

- `ARCHITECTURE.md` — MCP section (≈326–368): new tools + the off-main mutating
  lane.
- `AGENTS.md` — "MCP tools" (≈178–210) + module index for any new `git::`/MCP
  files.
- `src/mcp/mod.rs` `SERVER_INSTRUCTIONS` — the worktree-cleanup workflow (§9).
- `CHANGELOG.md` — per shipped PR.
- This doc — annotate `✅ shipped in #NNN` as PRs land (review-campaign style).

---

## 13. Related worktree bugs (track + fix alongside)

- **Stale git markers in a background worktree column.** A linked worktree's
  `index`/`HEAD` live under `<main>/.git/worktrees/<name>/` — *outside* the
  watched working tree — so spyc's fs-event instant-marker path (#442) can't see
  commits there; a worktree's markers depend solely on the 1 Hz mtime poll
  (`compute_git_mtime_key_fast`, `src/app/state/git.rs:207`). Observed live: the
  `docs/code-review-finalize` worktree kept showing stale "changes" after its
  commit landed from an external process, so the poll isn't reconverging for a
  non-focused column. Needs a live repro (worktree in column b → external commit
  → watch the markers). Likely fix: make the background column's poll reconverge
  (or watch the resolved per-worktree gitdir). Part of the git-marker freshness
  saga (#440–#451).
- **`create_worktree` is silent in the UI.** It creates + registers the worktree
  but doesn't open a column or flash, so the user can't tell spyc did it (hit
  live this session). Add a flash (`created worktree <name> @ <path>`); consider
  auto-`open` (the Phase 3 `open: true`).

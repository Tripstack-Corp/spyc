# VSCode git-extension study — git refresh, caching & efficiency

Last reviewed: 2026-06-17
Reference point: `microsoft/vscode` @ `extensions/git/src`
(`repository.ts`, `git.ts`, `watch.ts`, `decorationProvider.ts`,
`quickDiffProvider.ts`, `repositoryCache.ts`, `operation.ts`, `model.ts`).

## Status: a consideration, not a committed plan

This doc captures a comparative read of VSCode's built-in git extension
against spyc's git-status pipeline. Nothing here is scheduled work. It
exists so that the **next** time marker staleness recurs — or someone
asks "should we change how git refresh works?" — we are arguing from a
concrete data point instead of from scratch. The headline finding
strengthens a redesign the owner has already weighed and deliberately
deferred (the git-status-owner-thread consolidation); it does not
overturn that deferral. Re-read before that conversation.

It is a snapshot: VSCode ships fast and the line numbers will drift.

## spyc's git-refresh pipeline today (the baseline)

So the comparison has something to stand on:

- **Status engine:** `gix` in-process (`src/git/status.rs::repo_status`),
  read-only — the status platform is built and consumed by iteration,
  never `.write()`/persisted. No `git` subprocess in production
  (`no_subprocess_git_in_production` guard).
- **Off-thread worker:** the heavy walk runs on a detached thread
  (`src/app/bootstrap.rs`), results land in a `Runtime` slot, a
  payloadless wake `Message` triggers a pre-recv drain
  (`apply_git_worker_result`, guarded by a generation counter so a
  superseded walk's result is dropped).
- **Freshness via mtime cache-key:** `.git/index` + `HEAD` mtimes gate
  whether to re-walk. `repo_status_stable` (#451) stats those mtimes
  *before and after* the walk and only trusts a walk whose mtimes were
  unchanged across it (racy-index detection).
- **1 Hz safety poll** (`refresh_git_state`) reads the cached
  `current_gitdir` — no gix open on the poll; gix opens only on chdir
  into a new repo and on HEAD change.
- **FSEvent path:** notify → `Message::FsEvent` → `git::excludes`
  gitignore-aware pre-filter (drops build-dir churn, carves out the
  cwd level per #453) → `is_listing_path` / `is_gitdir_status_path`.
  A watched-gitdir `index`/`HEAD` event refreshes git state
  **immediately, no debounce** (#442); working-tree edits keep a 500 ms
  trailing debounce.
- **Markers:** two-column gutter mirroring `git status -s`
  (col0 staged, col1 unstaged); row cache keyed on `list_generation`.
- The old **huge-tree throttle subsystem was removed** (#440); poll and
  cache-invalidation are now flat 1 s.

The recurring bug *class* across this history — huge-tree
misclassification, worktree-switch staleness, deferred-rewalk, the #451
racy-snapshot — is uniformly "the cache thought it was fresh when it
wasn't." Every instance lives inside the mtime cache-key layer.

## What VSCode does

### Caching: there isn't any

The central finding. VSCode keeps **no git-status cache and performs no
freshness inference**. It re-runs `git status -z` and replaces the model
wholesale, every time (`repository.ts` ~2961). There is no mtime key, no
content hash, no generation-vs-disk compare deciding "can I skip this
walk." `repositoryCache.ts` is an LRU of *recent-repository URLs* for the
"Open Recent" UI — unrelated to status freshness; it has no TTL and no
mtime, only LRU eviction.

Efficiency is bought entirely by controlling **when** the recompute is
allowed to fire, never by skipping it:

- **Debounce 1000 ms** on file-change events
  (`@debounce(1000) eventuallyUpdateWhenIdleAndWait`, `repository.ts`
  ~3191) collapses bursts.
- **Throttle / sequential** (`@throttle updateWhenIdleAndWait`,
  ~3196): a status request arriving mid-flight is queued as "next," not
  run concurrently.
- **5 s cooldown after each status** (`await timeout(5000)`, ~3200) —
  this is what makes "re-walk always" affordable under sustained churn.
- **Idle + focus gating** (`whenIdleAndFocused`, ~3203–3218): defers the
  walk until no blocking git op is running **and the window is focused**.
- **Cancellation** (`CancellationTokenSource`, ~2866–2869): a new
  `updateModelState` aborts the previous in-flight one (spyc's
  generation gate is the equivalent).
- **Initial-open concurrency cap:** `new Limiter(5)` (`model.ts` ~285)
  bounds concurrent first-status calls across many repos.

### No poll

VSCode is **purely event-driven — there is no safety poll.** It trusts
the `.git` watcher to catch the index/HEAD rename. If an event is ever
missed, the state stays stale until the next event (or a manual
refresh) — VSCode accepts that, and the focus-gating turns
window-refocus into a natural backstop.

### Watching

- Working tree watched recursively (`**`), with `.git/` filtered out.
- `.git/` watched **root-level only** — a single-level `*` glob
  (`watch.ts` ~14), *not* recursive. It never watches `objects/`,
  `packs/`, `logs/`, `refs/` deep churn. It catches the index/HEAD
  rename because those land directly in `.git/`.
- Filters `index.lock` and watchman cookies at the watch layer
  (`repository.ts` ~470) so git's own lock dance doesn't spin status.
- A **dynamic upstream-ref watcher** (`repository.ts` ~477–496) watches
  `refs/remotes/<remote>/<branch>`, re-established after every status,
  to keep ahead/behind live without polling.
- No `.gitignore` consultation at the watch layer — git's own walk does
  the filtering. (spyc filters earlier, at the FSEvent layer, to avoid
  the wakeups entirely.)

### Index-lock safety

`GIT_OPTIONAL_LOCKS=0` is set for every status invocation (`git.ts`
~2745) so background status never acquires or waits on the index lock —
it can't block (or be blocked by) a user's foreground `git add` in a
shell. **For spyc this is already N/A:** gix status is read-only and
never persists the index, so we don't fight foreground git on this axis.

### Large-repo guardrails

- `git.statusLimit` (default **10 000 changed files**, `repository.ts`
  ~2957). When status output exceeds it, the subprocess is **killed
  mid-stream** and the result sliced (`git.ts` ~2788–2793), flagged
  `didHitLimit`.
- That flips the repo into **"huge" mode** (`isRepositoryHuge`): file-
  change auto-refresh is disabled (`repository.ts` ~3178–3180); status
  only re-runs on explicit user refresh, with a warning dialog.
- Note the axis: VSCode caps on **changed-file count** — the thing that
  actually makes status slow. spyc's removed huge-tree gated on *subdir
  count*, the wrong axis, which is precisely why it misfired.
- Cheapness knobs: `git.untrackedChanges: hidden` → `-uno`
  (`git.ts` ~2748), `git.ignoreSubmodules` → `--ignore-submodules`,
  `git.similarityThreshold` gates rename detection.

### Per-file decorations (the marker analogue)

`GitDecorationProvider` holds a `Map<uri, FileDecoration>`, **rebuilds it
wholesale** on each `onDidRunGitStatus`, and fires a single change event
over the **union of old+new URIs** (`decorationProvider.ts` ~112,
~121–132) — same full-rebuild shape as spyc's `list_generation` row
cache. Directory roll-up is done by the **VSCode framework**, not the
extension (it emits file URIs only); spyc rolls up in-house in
`status.rs`.

## The consideration this raises

VSCode is a direct **existence proof** for the consolidation the owner
already sketched and deferred: collapse the three refresh paths (1 Hz
poll + listing-event + chdir) and the mtime cache-key into **one
git-status-owner thread** that re-walks on a coalesced signal and pushes
fresh results. VSCode — shipped on huge monorepos to millions — simply
does not have a freshness-cache layer, and therefore *cannot* have the
bug class that layer keeps generating. Its concrete shape if we ever
pull the trigger:

1. **Event-driven recompute, no freshness inference.** Drop the mtime
   cache-key. A coalesced signal → re-walk → replace wholesale.
2. **Time-coalesce hard.** spyc already has the 500 ms debounce and the
   `is_gitdir_status_path` immediate-path; add VSCode's idea of a short
   post-walk cooldown so sustained churn can't re-walk faster than the
   walk costs.
3. **Drop or drastically slow the poll.** Its reason-to-exist is
   "the cache got stuck" — remove the cache and a missed event just
   means "stale until the next event," same as VSCode. A slow (e.g.
   on-refocus, or 10 s) backstop replaces the 1 Hz poll.
4. **Cap on changed-file count, not subdir count,** if a large-repo
   backstop is wanted back — kill the walk past N entries and degrade to
   manual refresh, the way VSCode does.

### The genuine tradeoff (why this stays a consideration)

gix status on a ~110k-file monorepo is 200–500 ms per walk. "Re-walk on
every coalesced event" is more background CPU than the mtime-skip, which
is exactly why the owner kept the cache: *"we need to work on large
projects efficiently."* That reasoning still holds. The **new** data
point is only this: VSCode faced the same choice on larger repos and
chose re-walk-always + cooldown + a changed-file cap over a freshness
cache — i.e. it spent CPU to delete the staleness-bug surface entirely.
That is a decision to revisit deliberately, not a defect to fix
reactively. Trigger: the next time a stale-marker bug surfaces that the
racy-snapshot fix (#451) doesn't cover.

## Cache-independent wins (adoptable without the redesign)

- **`GIT_OPTIONAL_LOCKS=0` equivalent — already satisfied.** Recorded
  here so we don't re-investigate: gix status is read-only, we don't
  fight foreground git. No change needed.
- **Upstream-ref watcher.** If the branch line shows ahead/behind, watch
  `refs/remotes/<remote>/<branch>` to update it on `fetch` without
  polling — VSCode's pattern.
- **Shallow `.git` watch is correct.** VSCode watches `.git/*`
  single-level and still catches everything that matters; deep `.git`
  watching is pure noise. Confirms spyc's index/HEAD carve-out instinct.

## Feature surface (mostly out of scope)

VSCode's git extension exposes 600+ commands — blame, stash, autofetch,
worktree management, submodule status, merge-conflict editor,
cherry-pick/rebase, commit signing, branch protection, history graph,
timeline, remote publish. Almost all are git-*client* territory, not
file-manager territory, and out of scope for spyc. The only two adjacent
to spyc's "file list with markers" identity:

- **Incoming/outgoing (ahead/behind) as a live signal** — ties to the
  upstream-ref watcher above.
- **Submodule markers** — VSCode marks submodule dirs distinctly; spyc
  treats them as ordinary dirs today.

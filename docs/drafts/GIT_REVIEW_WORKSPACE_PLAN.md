# Git review workspace proposal for spyc 2.3

**Status: draft for review.** Candidate scope for 2.3, alongside Projects;
unscheduled and not a release commitment. Written 2026-10-06 against `main`
at `e1b714a62412a37f998a486161eac38d3c4f0f3f`. This proposal introduces no
implemented commands, tools, bindings, or configuration.

Spyc should let a developer select an agent's worktree, review its complete
branch change, inspect the surrounding source and history, and discuss a
precise change with the agent without leaving the session. The proposed Git
review workspace connects those steps while keeping the repository,
worktree, comparison endpoints, file, and selected lines explicit.

**Recommendation:** consider a read-only review workspace for 2.3, delivered
after Projects establishes project identity and background-result routing.
The complete candidate scope is Changes, Compare, History, source drill-down,
and shared review context over MCP. If capacity is tight, Changes and Compare
form a useful smaller release. Staging, committing, and remote operations
remain a separately assessed expansion. Gitoxide remains the preferred backend;
each new mutation workflow must demonstrate sufficient compatibility before
inclusion. Recent upstream development supports retaining this choice for the
review workspace.

The interaction design is inspired by [GitFrame](https://github.com/hiroaqii/gitframe),
[Lazygit](https://github.com/jesseduffield/lazygit),
[Tig](https://jonas.github.io/tig/), and [Magit](https://magit.vc/).
The specific influences and their limits are recorded under
[Inspired by and references](#inspired-by-and-references).

## Decisions requested

Approving the direction would establish the following boundaries. Delivery
still follows the repository's build, local testing, acceptance, and merge
workflow, one implementation slice at a time.

| Decision | Recommendation |
|---|---|
| Release placement | Candidate companion to Projects in 2.3; Projects remains the primary release objective. |
| Initial product scope | Repository-wide Changes and branch Compare, followed by History and source navigation. |
| Interaction surface | A project-owned review surface in the file-frame area, with the agent pane retained. |
| Git backend | Retain in-process `gix` for the review workspace and preserve sync-only MVU; assess any future mutation exception explicitly. |
| Agent integration | Read the exact review context and bounded content through MCP; user controls any handoff. |
| Git mutations | Assess separately; no staging, commit, checkout, fetch, or push requirement for this release. |
| Entry point | Start with the proposed `:review` command family; choose a default frame binding after a keymap audit. |

## Why this belongs in spyc

Spyc already shares a meaningful working set between the human and the agent:
paths, picks, worktrees, branches, and project context. Review is the next
step after an agent finishes changing that working set. Today the developer
can inspect selected files, but moving from the whole branch to a changed
file, its diff, the original source, and its history requires reconstructing
context across separate views or tools.

The proposed experience answers four recurring questions:

1. What did this agent change across its branch, including files outside the
   directory I happen to be browsing?
2. Which changes are committed, staged, or still being edited?
3. What did this code look like at the revision I am reviewing?
4. Can the agent explain the same hunk I am looking at, even if it continues
   editing the working tree?

This is a deliberate refinement of the older
[VSCode Git study](../archive/VSCODE_GIT_STUDY.md), which treated most Git
client features as outside the file-manager scope. Review directly supports
the [shared-working-set thesis](../../ROADMAP.md#thesis). A complete Git
porcelain would introduce a much larger product and maintenance commitment.

## Existing foundations and missing pieces

These are facts about the recorded baseline. Proposed changes appear in the
right-hand column; none should be read as a description of shipped behaviour.

| Area | Existing foundation | Required addition |
|---|---|---|
| Changes | `gd`, `gD`, and `gu` compare selection against HEAD or index; status has staged and unstaged sides. | Repository-wide catalogue with persistent file and hunk selection. |
| Diff presentation | Syntax highlighting, intraline highlighting, split/unified layouts, and narrow-width fallback. | Selection and source-location mapping attached to rendered rows. |
| Compare | Commit-show already diffs two trees; branch queries report ahead, behind, and containment. | Arbitrary endpoint comparison, merge-base resolution, and a base selector. |
| History | Recent commits are available to MCP; a SHA can open commit-show. | Paginated commit navigation linked to changed files and source. |
| Worktrees | Listing, opening, creation, claims, and safe removal are available. | Explicit review targeting and retention of per-worktree reading state. |
| Concurrency | Status has generation and repository relevance checks. | Review-specific identity, cancellation, freshness, and result checks. |
| Agent context | MCP exposes paths, selections, Git reads, and pane attribution. | A typed review selection with exact revisions and snapshot identity. |

Useful implementation anchors:

- [`src/git/model.rs`](../../src/git/model.rs): owned diff, file, hunk, and
  commit data, independent of the application and renderer.
- [`src/git/diff_model/mod.rs`](../../src/git/diff_model/mod.rs): working,
  staged, unstaged, and commit-show builders. `show_model` already contains
  a tree-to-tree comparison path.
- [`src/git/branch.rs`](../../src/git/branch.rs) and
  [`src/git/log.rs`](../../src/git/log.rs): base discovery, branch relationships,
  and the current bounded recent-log query. Branch relationships do not yet
  provide the proposed merge-base review query.
- [`src/app/git_view_session.rs`](../../src/app/git_view_session.rs): off-thread
  model and highlight construction, with retained content for layout changes.
- [`src/ui/diff_render/mod.rs`](../../src/ui/diff_render/mod.rs): reusable pure
  diff rendering. Its current output is styled lines, not a semantic selection
  map.
- [`FEATURES.md`](../../FEATURES.md#git-views--diff--show--blame): the existing
  user-facing Git behaviours that must remain compatible.

## Scope and release boundary

| Capability | Candidate 2.3 scope | Later consideration |
|---|---|---|
| Changes | All changed paths in a worktree; Working, Staged, and Unstaged scopes; visible conflicts. | Whole-file staging and unstaging. |
| Compare | Branch/ref selection; merge-base and direct endpoint comparison; explicit direction. | Saved comparison presets beyond the last-used base. |
| History | Bounded commit pages; commit details; changed files; first-parent diff with clear labelling. | Graph workbench, arbitrary commit-set editing, rename-following file history. |
| Source | Read either side of a selected file at its exact revision; explicitly open the current file. | A complete historical tree browser. |
| Worktree navigation | Review the focused worktree or an explicitly chosen existing one. | Additional branch checkout and worktree lifecycle UI. |
| Agent context | Read context and materialized review content; copy a precise reference for handoff. | Review annotations and richer agent-driven navigation. |
| Persistence | Remember compact navigation state within each project; rebuild content after recovery. | Durable review annotations and reviewed-file tracking. |
| Git writes | No new Git write operation in the core proposal. | Commit/amend, partial staging, discard, stash, and remote sync after capability review. |

Interactive rebase, cherry-pick orchestration, conflict resolution, hosting
provider integration, and automatic branch switching are outside this
proposal. Existing shell panes and external Git clients remain available.
No runtime dependency on GitFrame or another reference application is needed.

## User journeys

### Review an agent branch

The developer selects the agent's worktree and opens Compare. The header
identifies the worktree, branch, selected base ref, resolved merge-base, and
tip. The file list contains the branch's changed paths across the repository.
Moving the cursor loads a diff preview; entering a file permits hunk and line
navigation. The agent pane remains available below.

The developer opens the old or new source at the corresponding revision,
reads enough context, and returns to the same hunk. Copying the review
reference or reading it through MCP identifies exactly that comparison.
Changes made after the comparison was opened are indicated without silently
replacing the revision under review.

### Review a staging checkpoint while an agent keeps working

Changes opens in Working scope. Switching to Staged shows what the index
contains relative to HEAD; Unstaged shows subsequent changes relative to the
index. Each scope retains its selected path and reading position.

If the index or worktree changes during reading, the view marks its materialized
content as stale. Refresh rebuilds the relevant catalogue and selected file,
then restores the closest valid position. The developer can finish reading
the captured content before refreshing.

### Understand a historical change

History lists commits from a pinned starting revision. Selecting a commit
shows its metadata and changed files. Selecting a file opens its diff; opening
source reads a blob from the chosen side. A deleted or renamed path is resolved
using that side's historical path. Returning walks back through source, diff,
files, and commits without replacing the ordinary file-browser history.

### Move between projects

Switching projects preserves the review mode, target worktree, comparison,
selection, and focus within the original project. A background agent's MCP
request reads its own project's review context. Returning to a project checks
freshness before presenting its content as current. A closed project rejects
late worker results even if another project later opens the same path.

## Interaction and layout

### The review surface

Review occupies the file-frame area across its existing columns. It has its
own file/commit navigator and content viewport. It does not create a second
Commander, change either column's directory, or reuse the user's preview slot.
Closing review restores the original columns, preview, selection, geometry,
and focus. The pane divider and agent tabs remain available.

This is a new frame surface, not a modal that consumes input regardless of
focus. The existing pane-focus route continues to forward input to the child.
Transient review pickers are modal and return focus to review when dismissed.
The pure input and mouse routers must distinguish review navigation, review
content, and pane targets explicitly.

Illustrative layout; labels and exact dimensions are provisional:

```text
spyc | project: api | worktree: feature/auth | Review: Compare
base: origin/main @ a12bc34 | merge-base: b23cd45 | tip: c34de56
Changes  Compare  History             committed changes | refs changed
---------------------------------------------------------------------
Changed files                 | src/auth.rs     -18 +42
  M src/auth.rs               | old @ b23cd45   | new @ c34de56
  A tests/auth_tests.rs       | ...             | ...
  D src/legacy_auth.rs        |                 |
---------------------------------------------------------------------
agent tabs / activity / existing pane controls
agent conversation
```

Repository browsing stays in the ordinary Commander. Source drill-down is
available inside review, but it does not duplicate every file-manager feature.
At narrow widths, the navigator and content become alternating views with a
clear breadcrumb. At insufficient heights, review shows one region at a time.
Existing split-to-unified diff fallback still applies. A user can use the
normal frame zoom when they want more room.

### Commands and keys

The following command grammar is proposed, subject to parser and naming review:

| Command | Behaviour |
|---|---|
| `:review` | Open or resume review for the focused column's worktree; Changes is the initial mode. |
| `:review changes [working|staged|unstaged]` | Select the Changes scope. |
| `:review compare [base]` | Compare the chosen base's merge-base with the target worktree's captured HEAD. |
| `:review compare-direct <left> <right>` | Compare two explicit commit endpoints. |
| `:review history [rev]` | Browse history from a captured revision, defaulting to the target HEAD. |
| `:review worktree` | Choose an existing worktree without checking out or changing a Commander. |
| `:review refresh` | Re-resolve symbolic refs and reload live content, preserving valid selection. |
| `:review copy-context` | Copy the precise comparison and selected source reference. |
| `:review close` | Restore the ordinary file frame. |

Revision arguments use the existing command parser's quoting rules;
revision parsing must not invoke a shell. Exact grammar and error examples belong in the implementation slice.

Entry is a frame action under the binding taxonomy. The default shortcut is
an open decision; it must be audited against existing `g` chords. There is no
new global `1`–`4` mode switch and no consumption of Projects' `Space` keys.
Commands are registered through `COMMAND_TABLE` and remain bindable through
the existing DSL.

Inside review, `j`/`k` move through the focused list or content; `Enter` drills
into the selected item; `Tab` switches review regions; `/` searches the focused
region. `Esc` returns one review level, closing review at its root; `q` closes
the review surface. `|` retains its split/unified diff meaning. Help describes
source-side selection and hunk navigation; their default keys require the same
audit. Existing pane-focus and zoom controls retain their meanings.

Existing `gd`, `gD`, `gu`, and `gb` keep their selection-scoped quick-view
behaviour. The new workspace is available independently; changing those keys
to enter review would be a separate interaction decision.

## Comparison semantics

### Changes scopes

| Scope | Before | After | Untracked paths |
|---|---|---|---|
| Working | Captured HEAD tree | Captured worktree content | Included and labelled untracked. |
| Staged | Captured HEAD tree | Index content | Excluded unless present in the index. |
| Unstaged | Index content | Captured worktree content | Included, matching spyc's existing `gu` behaviour. |

The catalogue comes from repository status and retains separate staged and
unstaged indicators. A file can have staged changes and reverse them in the
working tree, leaving no net difference from HEAD. Working must explain that
state and offer the other scopes instead of implying that there are no changes
anywhere. An unborn HEAD is an explicit empty-tree baseline for applicable
Changes scopes; History and branch Compare explain why a commit is required.

Unmerged index entries are shown as conflicts. The initial release provides
status and readable source where available, without presenting an ordinary
two-way diff as a complete account of the conflict. No resolution action is
implied by viewing a file.

### Compare bases and direction

Merge-base mode compares `merge-base(base, tip)` to `tip`. Direct mode compares
the left endpoint to the right endpoint. These are distinct questions, and
the header labels the chosen mode and direction. They follow Git's documented
[endpoint and merge-base semantics](https://git-scm.com/docs/git-diff).

On first use, suggest the repository's integration branch using the existing
base resolver. Prefer a saved explicit choice on later visits. Do not assume
that a feature branch's tracking upstream is its integration base. If the
resolver falls back to an arbitrary local branch, or selects the tip itself,
open the base picker and explain the suggestion before using it.

The picker lists local branches and remote-tracking refs with unambiguous
names; tags and commit ids can be supplied explicitly. Remote-tracking refs
are local knowledge, not a claim about the current server state. Opening a
review does not fetch. Persist the chosen fully qualified ref where possible,
and resolve it to an object id for each new comparison snapshot.

The displayed provenance includes the selected base ref and its resolved
commit, the merge-base when used, and the tip commit. Changing refs cannot
change an already materialized comparison. A refresh creates a new snapshot.
Missing objects, shallow history, unrelated histories, and multiple best merge
bases get explicit outcomes. The initial implementation must not silently
choose one of several merge bases or substitute a direct comparison; it can
offer direct mode as an explicit user choice.

Compare contains committed changes only. Working-tree/index state is shown as
a separate summary and is reachable through Changes. An empty comparison is
distinguished from query failure, cancellation, and missing history.

### History and exact source

History starts from a captured commit id and uses bounded pages with a stable
walk order. The continuation belongs to that starting id; a moved HEAD marks
the list as outdated without mixing new commits into an old page. The
implementation must choose and document deterministic ordering, including
equal timestamps and merges.

A merge commit defaults to a clearly labelled first-parent diff, matching
the current `show_model` interpretation. Combined merge diffs and interactive
parent selection are deferred. Root commits compare with the empty tree.
Commit metadata includes full ids, author, date, and message; display shortening
never changes the identity used by queries.

Source drill-down uses the old path/blob for the old side and the new path/blob
for the new side. Removed lines open old source. Added lines open new source.
Context lines use the selected side. Opening the current worktree file is a
separate, clearly named action because its line numbers may have moved. A
missing current file does not prevent viewing its historical blob.

## Review state and MVU integration

### Ownership

The following types are conceptual responsibilities, not prescribed Rust APIs:

| State | Owner | Contents |
|---|---|---|
| Review session | Project Model | Review id, repository/worktree target, mode, query specification, selection, history, freshness. |
| Materialized review data | Project Model | Bounded owned file metadata, commit pages, semantic hunks, and captured source identity. |
| Active work | Runtime | Worker handles/channels, cancellation, queue bookkeeping, result slots, and operational caches. |
| Presentation | ViewState | Highlight caches, row maps, geometry, wrapping, and rendered lines derived from semantic data. |

Use `ProjectId` from Projects, an opaque `ReviewId`, and a snapshot/request
generation. Repository identity must distinguish a repository's common Git
directory from each worktree's Git directory and root. A filesystem path is a
locator that must be revalidated, not a sufficient lifetime identity.

A project has one foreground review session and bounded recent navigation
descriptors keyed by worktree. Selecting another worktree saves the current
descriptor and resumes that target's last state. Proposed initial retention
is eight descriptors per project; the exact count belongs in the sizing review.
Closing a project drops its review workers and cached content.

### Requests and results

Pure update logic changes review state and emits effects describing queries.
`run_effects` executes them through the established worker pattern. Workers
call `src/git/`, return owned results through Runtime slots, and wake the loop
through the existing `Message::Wake` family. No repository handle belongs in
Model or ViewState, and neither key handlers nor rendering touch the OS.

A result is applied only when its project, review, target identity, snapshot,
and request generation still match. Reopening the same path must not make an
old result relevant again. File navigation can supersede a diff request without
invalidating the catalogue; separate catalogue and content request generations
make that distinction explicit.

Use a bounded queue with latest-request replacement for superseded file
previews. Check cancellation between expensive phases and do not start a new
thread for every cursor movement. Ordinary status updates and agent/pane
input must not wait behind an unbounded history walk or diff computation.
Keep the existing status worker contract; a dedicated bounded review executor
is preferable to turning its queue into a general Git job queue.

### Module boundaries

Extend the existing Git facade with focused query modules for endpoint
resolution/comparison, paginated history, and revision source. Reuse tree-diff
and blob-diff mechanics after extracting cohesive shared helpers; do not grow
the already substantial `diff_model/mod.rs` into another catch-all module.

A cohesive review Model module owns transitions, selection, and freshness.
Matching app glue, worker glue, and pure render modules own their respective
responsibilities. Reuse existing diff/highlight routines while adding a
semantic row map: each selectable rendered row identifies its file, hunk,
side, and source line. Wrapping, intraline styling, and split layout must not
change that mapping. Never recover source locations by parsing coloured text.

The existing quick Git pagers can continue using `GitViewStream`. Sharing
query/render helpers does not require replacing the general pager or migrating
all retained stream state in the same change.

## Freshness and concurrent editing

Review has four visible content states: loading, ready, stale, and failed.
Refresh may retain the previous content with a refreshing indicator. Failure
keeps that content labelled stale and shows the cause; it never replaces it
with a success-shaped empty result.

Immutable comparisons are pinned to object ids. Symbolic-ref changes mark the
context as outdated while the materialized content remains valid for its ids.
Changes involves mutable files and index state, so it cannot promise an atomic
repository-wide snapshot while an agent writes.

For live queries, capture HEAD and index identity before computation and
verify them before publishing. Capture each loaded file's bytes once, record
a content fingerprint, and build its diff, source view, and MCP content from
those same bytes. Recheck observable file state around capture; concurrent
change produces a stale/retry outcome with bounded retries. There is no claim
that metadata checks detect every possible concurrent filesystem write.

The catalogue and loaded-file captures carry their own freshness and capture
identity. Loading an uncaptured file after a relevant change requires refresh,
rather than pretending it belongs to the old snapshot. For complete,
reproducible cross-file review, Compare against immutable commits is the
preferred mode. This distinction must be visible in both UI and MCP.

While review is visible, existing filesystem/status invalidations mark its
live captures stale. Opening a different worktree for review registers its
own bounded watch target; it must not move a Commander just to obtain watches.
Selected local refs also need invalidation coverage, including packed refs
and remote-tracking ref changes. This work reuses the current poll/watch
machinery and does not commit the deferred Git status-owner redesign.

Inactive projects do not acquire continuous review watches or a new poll.
They become freshness-unknown when deactivated and revalidate on activation
or explicit MCP refresh. Immutable materialized content can remain readable
with its provenance. A background query must not activate its project.

Refresh restores selection by semantic path identity and, where possible,
old/new blob identity and source location. If a file disappeared, move to the
nearest remaining row and explain why. A hunk position that cannot be mapped
returns to the file header. Content and catalogue swap coherently; avoid
showing a new filename above a previous file's diff.

## Large repositories and special files

The file catalogue loads metadata first. Only the selected file's content
and a small bounded cache of recent selections need full diff construction
and highlighting. History loads in pages; narrowing the list does not start
an unbounded history scan. Expensive diffstat/rename analysis must be bounded
and must disclose when a result is incomplete or detection was skipped.

Initial sizing proposals for the feasibility slice are 200 commits per page,
a 64 MiB total review-content cache, and an 8 MiB per-side automatic text-load
limit. The existing diff line budget is another bound, not a replacement for
byte limits. These are proposed defaults to measure, not performance results.
Account for captured bytes, semantic models, and highlight/row caches when
enforcing the process-wide content budget; runtime overhead is measured
separately. Use bounded retention across projects so opening many reviews does
not multiply that cache without limit.

Do not hide changed files because an earlier file exhausted the hunk budget.
The catalogue must distinguish complete, paginated, and truncated results;
each file can have its own truncated-content marker. Huge lines require a
byte/display budget even when the number of lines is small. A binary file,
submodule change, or unsupported encoding still has an informative row.

Preserve original path bytes for identity and render escaped display labels
for unusual paths. Do not turn terminal control characters in filenames,
commit messages, or source into terminal instructions. Treat symlinks and
submodule entries according to Git object type; historical source must not
follow an old symlink through the live filesystem. Respect path containment
and the existing MCP root boundary.

Sparse checkouts, renames, executable-bit changes, symlinks, binary additions,
deleted files, conflicts, and missing objects need explicit presentation.
Unsupported operations say why; they must not become an unchanged file.

## Shared review context through MCP

Add a small read-only surface with provisional names:

| Tool | Purpose |
|---|---|
| `get_review_context` | Return the caller project's foreground review identity, provenance, selected item, and freshness, or an explicit no-review result. |
| `get_review_content` | Return bounded diff or source content for a specified review, snapshot, and selection; report expiration or a changed context explicitly. |

The context includes project/review/snapshot ids; repository and worktree
identity; mode/scope; symbolic refs plus resolved full commit ids; merge-base
when relevant; old/new paths and blob or capture fingerprints; selected side,
hunk, and source-line range; and completeness/freshness flags. An opaque
selection token binds a content read to that snapshot, avoiding a race between
the agent reading context and the user moving the cursor.

MCP follows Projects' pane-to-project attribution and existing root access
rules. A supplied review id cannot authorize reading another project or an
unrelated filesystem path. If a capture is evicted, reply that it expired;
never substitute today's working file under an old token. Lazy immutable
content can be rebuilt only from the same resolved objects and must retain
the same endpoint semantics.

Reading context does not move focus, switch projects, submit a prompt, or
approve a change. The user can copy a reference and paste it into the desired
agent pane. Automatic prompt submission and new agent-navigation tools are
outside the initial scope. Existing path handoff keeps its documented behaviour;
this proposal does not repurpose `^a s`.

The UI and MCP consume the same semantic model so that a wrapped diff row,
deleted line, or renamed path resolves consistently. An agent's response
still belongs to its own conversation; this release adds no review-comment
database or remote publishing mechanism.

## Projects and recovery

This feature should attach to the project-owned state introduced by
[`PROJECTS_PLAN.md`](PROJECTS_PLAN.md), especially its identity, background
result routing, MCP attribution, and recovery format. Land those foundations
before integrating review state. Query/model work can be prepared separately
without installing a second global context mechanism.

Persist only compact descriptors: target locator, mode and scope, chosen refs,
resolved comparison ids when meaningful, selected path/source location,
layout preference, and navigation level. Do not serialize diff bodies,
highlighted lines, worker channels, or worktree-file captures.

On recovery, reconstruct the target and mark content as loading or
freshness-unknown. Rebuild immutable comparisons from their saved ids when
available; show moved refs separately. Never silently replace missing saved
commits with current HEAD. Mutable captures must be recreated and labelled as
new content. A missing or removed worktree produces a recoverable unavailable
state with a target picker, without restoring any deleted files.

New descriptor fields must be optional/defaulted in the Projects recovery
format. Old saves continue to load. For the future attach model, semantic
review state belongs with project state, caches rebuild for the terminal,
workers stay with the process, and obsolete requests are dropped.

## Gitoxide backend assessment

**Recommendation: retain Gitoxide for the proposed review workspace.** Its
recent development and the APIs already available to spyc support Changes,
Compare, History, and exact revision source. Backend replacement is not a
prerequisite for this proposal. The implementation work is primarily query
composition, bounded loading, semantic selection, and application integration.

### Evidence and dependency baseline

Assessment date: **2026-10-06**. The recorded baseline declares `gix = "0.87"`
and locks **0.87.1**. Its enabled features include status, directory walking,
blame, blob diff, revision, index, attributes, excludes, parallelism, and
worktree mutation; networking and credentials are intentionally disabled.
Published **0.88.0** is the next minor release available at this assessment.
These versions describe the assessment baseline, not a dependency upgrade
made by this plan.

Recent upstream releases contain changes relevant to a worktree-oriented
review tool:

- [0.87.0, 22 August 2026](https://github.com/GitoxideLabs/gitoxide/releases/tag/gix-v0.87.0)
  added commit signing and verification, Git notes, branch deletion with
  worktree checks, and repository-environment handling improvements.
- [0.87.1, 24 August 2026](https://github.com/GitoxideLabs/gitoxide/releases/tag/gix-v0.87.1)
  refined remote-tracking branch resolution and reference handling.
- [0.88.0, 25 September 2026](https://github.com/GitoxideLabs/gitoxide/releases/tag/gix-v0.88.0)
  improved worktree pruning, linked-worktree handling, and configuration
  editing, while introducing a broad public error API migration.

This is evidence of active development in areas spyc uses. It is not evidence
that every Git command has a complete library equivalent. Evaluate individual
workflows against the pinned source and enabled features, and recheck this
assessment when implementation starts.

### Capability assessment

| Workflow | Verified foundation | Consequence for this plan |
|---|---|---|
| Changes, history, and source | The pinned library provides status, revision walking, trees, blobs, and diff primitives; spyc already uses several of these. | Keep `gix`; extend the existing facade and owned query models. |
| Branch Compare | The pinned `Repository` API includes merge-base and multiple-merge-base queries as well as tree comparison. | No dependency upgrade is required merely to prototype Compare. Preserve the ambiguity rules above. |
| Whole-file stage/unstage | Index, object, filter, and worktree primitives exist. | Prove the complete operation with fixtures; primitive availability is not a staging compatibility claim. |
| Commit and signing | The pinned source can create commits and update refs with expected-old checks; separate signing APIs are available. | `Repository::commit` does not orchestrate hooks or automatic signing. Demonstrate the complete workflow before scheduling it. |
| Fetch | The published API provides remote connections and fetch preparation with blocking transport support. | Compatible in principle with worker execution, but needs a feature/dependency and credential review because spyc disables networking. |
| Push and sync | The published `gix::push` module describes `push.default` configuration; the upstream CLI roadmap still lists push as unfinished. | Do not infer a complete push API from the module name or commit to a gix-only sync delivery date. |

The pinned [Repository API](https://docs.rs/gix/0.87.1/gix/struct.Repository.html)
and source are the reference for the local-query and commit assessment.
The published [remote connection API](https://docs.rs/gix/0.88.0/gix/remote/struct.Connection.html),
[push module](https://docs.rs/gix/0.88.0/gix/push/index.html), and
[upstream CLI roadmap](https://github.com/GitoxideLabs/gitoxide#roadmap)
provide additional evidence for remote operations. The CLI checklist is not
an inventory of all library primitives; it must be read alongside the APIs.

### Upgrade and architecture policy

Evaluate a move to 0.88 in a dedicated maintenance change, separately from the
first review UI slice. Its public errors move to `gix::Error`/`Exn`, so inspect
existing error handling and preserve actionable causes and classifications.
Its Rust 1.88 minimum matches spyc's declared minimum at the recorded baseline.
An upgrade must pass the repository gate and relevant Git/worktree regression
fixtures; the existence of a newer release alone does not require a change
during the current release-candidate soak.

For the 2.3 review scope, the existing production restriction on Git
subprocesses remains intact. For later mutation workflows, compare the cost
of completing the gix implementation with a narrowly scoped Git CLI adapter
behind the same facade and worker effects. Such an adapter would require a
separate architectural decision and an explicit guard change before use;
this draft neither implements nor approves that exception.

Make that comparison when a required workflow has missing semantics, when
maintaining compatibility would consume disproportionate effort for a
single-developer project, or when an upstream dependency would leave its
release date uncertain. Retain gix for frequent reads even if a future write
operation uses another backend. Any alternative must preserve explicit
operation scope, repository identity, hooks/signing policy, and failure
reporting. The next section defines the evidence required for those writes.

## Git write operations as a separate expansion

The repository's [`src/git/` contract](../../src/git/mod.rs) requires
in-process Git operations. The baseline [`Cargo.toml`](../../Cargo.toml)
enables a selected `gix` feature set and leaves networking and credentials
disabled. A plan for native commits or sync therefore needs a capability
matrix against the version and features actually used by spyc. This proposal
does not infer unsupported operations from an absent application API, or
assume that a low-level object writer supplies complete Git command semantics.

| Candidate | Capability and behaviour that must be demonstrated |
|---|---|
| Whole-file stage/unstage | Correct index locking, additions/deletions, path bytes, modes, symlinks, attributes/filters, and unrelated-entry preservation. |
| Commit | Correct tree and parent selection, identity/configuration, expected-old ref update, reflog, hooks, signing, and errors that preserve the staged state. |
| Partial staging | Exact source bytes and newline semantics, index preconditions, patch application, and safe refusal when the selected content changed. |
| Discard/restore | Explicit target/content preview and recovery policy; integration with the graveyard where appropriate. |
| Fetch/push/sync | Transport, credentials, cancellation, progress, remote selection, upstream policy, and non-fast-forward handling. |

The existing `DiffModel` is for display: it stores line text without original
line endings and does not carry every precondition needed for a write. It must
not become a patch serializer by joining rendered lines. A future write model
must retain exact data and expected repository state.

Before any write, revalidate the target and expected index/HEAD/content under
the operation's locking rules. Advisory agent scope claims can explain active
work, but cannot replace filesystem/index locks or compare-and-swap ref
updates. No operation stages every changed file merely because it is visible
in a review; the user chooses its scope explicitly.

If the capability assessment exposes missing semantics, either defer the
operation or bring a separate architectural proposal for a narrowly defined
backend exception. Do not quietly add Git subprocesses or bypass hooks/signing
to ship a deceptively complete commit button. External clients in a pane are
already a useful option while this work is priced.

## Delivery slices

Each slice is reviewable independently. Runnable slices require automated
checks, an exact local build path, user testing, and acceptance before merge
and advancement, as required by `AGENTS.md`.

| Slice | Deliverable | Acceptance gate |
|---|---|---|
| 0. Design and feasibility | Confirm scope, Projects dependency, frame ownership, key audit, the dated Gitoxide assessment, pinned-version APIs, and bounded-query prototypes. | Demonstrate an immutable comparison and semantic row mapping; record large-repository measurements and chosen budgets. |
| 1. Comparison core | Resolve endpoints, enumerate changed paths, build selected-file diffs, and read revision source through `src/git/`. | Fixture parity and explicit errors for missing/unrelated/ambiguous history; no production Git subprocess. |
| 2. Changes workspace | Frame surface, repository-wide catalogue, three scopes, focus/close behaviour, lazy content, stale/refresh handling. | Correct staged/unstaged separation, agent editing during review, and exact restoration of the displaced file frame. |
| 3. Compare workflow | Base/worktree pickers, merge-base/direct modes, pinned provenance, source-side navigation, and ref-change indicators. | Review a whole agent branch without changing checkout, current directories, or pane target. |
| 4. History workflow | Bounded commit navigation, changed-file drill-down, first-parent labels, and source/back navigation. | Stable pagination and accurate root/merge/deletion/rename source cases. |
| 5. Shared context and recovery | MCP snapshot-bound reads, copied context, project switching, descriptors, and restart behaviour. | No cross-project leakage or retargeting; expired/stale contexts stay explicit. |
| 6. Release hardening | Large-repository tuning, narrow-terminal and mouse behaviour, help/docs, and end-to-end dog-fooding. | Accepted local build and documented release checklist with no correctness gate waived. |

**Full candidate:** slices 0–6, with the retained scope above.
**Reduced candidate:** Changes and Compare from slices 0–3, plus the necessary
project routing, freshness, recovery compatibility, and hardening from slices
5–6. History and new MCP review-content tools can follow. Cross-project
correctness is mandatory in either version; it cannot be cut as polish.

The slice order is a dependency plan, not an estimate of one equal-sized PR
per row. Slice 0 supplies the implementation estimate. If the review surface
threatens Projects' delivery or demands a wider pager/focus rewrite than
expected, move this feature to a later release rather than making Projects
depend on it. Git write operations are not a stretch gate for 2.3.

## Verification and release criteria

Pure transition tests cover entering/exiting review, restoring the file frame,
focus routing, project switches, result relevance, freshness, and selection
restoration. Property tests should exercise identities and message ordering,
including a project closed and reopened at the same path. Tests must assert
observable behaviour, with negative cases that demonstrate their failure.

Git integration fixtures use the existing test-support command wrapper to
construct repositories and compare results against Git. Cover empty/unborn
repositories, staged-only changes, edits after staging, cancelling net changes,
untracked files, renames/deletions, executable modes, symlinks, binary files,
submodules, conflicts, root/merge commits, detached HEAD, shallow repositories,
unrelated histories, multiple merge bases, and unusual path bytes. Pin the
comparison endpoints and normalize both sides through one comparison helper.

Rendering tests cover wide and narrow terminals, split/unified toggles,
wrapping, long lines, escaped labels, mouse hit-testing, and semantic source
mapping. Source jumps must remain correct after resize and layout changes.
Truncated, unsupported, loading, stale, and failed states need distinct output.

Concurrency tests reorder worker completion, move refs during a query, edit
or replace a selected file, switch worktrees, and close projects while reads
are in flight. MCP tests move the UI selection between context/content calls,
expire a capture, and attempt reads from a different project. No result may
silently change its identity to satisfy a request.

Performance validation records cold and warm query latency, retained memory,
queued/active jobs, input responsiveness, and idle draw rate using both spyc
and a representative large repository. Include many changed paths, large
binary files, long text lines, and rapid cursor movement. Enforce the chosen
byte/line/cache bounds and preserve zero draws at idle. Numerical latency
budgets are set from the feasibility measurements, not claimed in advance.

Release acceptance requires:

- A complete branch can be reviewed while its agent pane remains usable.
- The header and copied/MCP context identify the actual comparison and source.
- Existing quick Git views and ordinary Commander/pane controls remain usable.
- Refresh never pairs one file's name with another file's content.
- Project/worktree switching and restart never retarget an old review silently.
- Partial results and unsupported states are explicit.
- Production code keeps the Git facade, MVU, sync-only, and idle-render contracts.
- The repository's required checks pass and the user accepts the local build.

Implementation updates the affected `FEATURES.md`, `DESIGN.md`,
`CONFIGURATION.md`, `docs/KEYBINDINGS.md`, help, MCP skill/reference material,
and `AGENTS.md` module index. Architectural decisions move into
`ARCHITECTURE.md` once adopted. The draft itself does not advertise commands
as shipped or add a generated changelog entry.

## Risks and alternatives

| Risk or alternative | Assessment |
|---|---|
| Projects and review compete for the same app state/focus code. | Sequence after Projects foundations; keep query work separate and make review independently deferrable. |
| Native Git-client scope expands incrementally. | Keep a written release boundary and a separate capability decision for each mutation family. |
| Backend purity makes spyc own too much Git command behaviour. | Keep gix for review; price complete mutation semantics and consider a separately approved backend exception when justified. |
| A dependency upgrade changes errors or worktree behaviour. | Isolate the upgrade, preserve error causes, and run Git/worktree compatibility fixtures before adopting it. |
| Existing display models are mistaken for an exact patch/source representation. | Add source identity and semantic row mapping explicitly; retain raw captured bytes when exactness is required. |
| Agents keep changing the repository. | Immutable comparisons, captured live content, explicit freshness, and generation-checked application. |
| A whole-repository diff becomes expensive. | Metadata-first enumeration, per-file construction, bounded queues/caches, cancellation, and visible truncation. |
| Embed GitFrame or Lazygit as a pane only. | Low integration effort and immediately useful; it does not by itself expose the review selection through spyc's semantic MCP state. |
| Add more independent Git pagers. | Useful for small additions, but does not establish persistent file/commit navigation and shared review context. |
| Replace all current Git/pager code at once. | Avoid: existing quick views can coexist while common query/render primitives are reused. |

## Open decisions before implementation

1. **2.3 placement:** accept the full candidate, the reduced Changes/Compare
   candidate, or keep this proposal for a later release? Recommended: target
   the full candidate only after the feasibility slice and Projects estimate.
2. **Entry binding:** which frame key should open review after the collision
   audit? Recommended: keep `:review` sufficient until that audit is complete.
3. **Live refresh:** should Changes auto-refresh when the user is at the file
   list? Recommended: explicit refresh throughout v1; add auto-refresh only if
   dog-fooding demonstrates a need and selection stability is preserved.
4. **Recovery:** restore the review surface open or remember it for the next
   invocation? Recommended: restore it open when that project was saved in
   review, with content rebuilt and freshness clearly indicated.
5. **Write capability work:** schedule an assessment alongside implementation
   or after review ships? Recommended: after the core interaction is accepted;
   no commitment to ship writes in 2.3. Use the capability matrix to decide
   between gix implementation, deferral, and a separate backend-exception
   proposal; do not assume replacement of the read backend.

## Inspired by and references

These are interaction influences and technical references, not a promise of
feature parity or a proposal to copy source code. Any later code reuse needs
its own licence review and attribution. External descriptions below were
checked on 2026-10-06; spyc design choices in this document remain proposals.

| Reference | Specific influence on this proposal |
|---|---|
| [GitFrame](https://github.com/hiroaqii/gitframe#readme) | Its Changes, Repository, History, and Compare views connect code with repository/worktree/revision context. It supports merge-base comparison and bounds its ambition. Spyc adopts the contextual review idea while using its existing Commander for repository browsing. |
| [Lazygit](https://github.com/jesseduffield/lazygit#features) | Its file-to-diff workflow, hunk/line selection, and endpoint comparison are useful interaction references. Staging and history-editing breadth are outside the proposed 2.3 core. |
| [Tig manual](https://jonas.github.io/tig/doc/manual.html) | Linked commit, diff, tree, and blob views, plus explicit browsing state, inform history/source drill-down and return navigation. Its subprocess implementation is not the proposed backend. |
| [Magit](https://magit.vc/) and [status-buffer manual](https://docs.magit.vc/magit/Status-Buffer.html) | Structured repository status and actions attached to visible context inform clear scopes and focused commands. Spyc retains its own frame/pane key taxonomy. |
| [Git diff documentation](https://git-scm.com/docs/git-diff) | Defines working/index/tree comparisons and the difference between direct endpoints and merge-base comparisons. |
| [Git merge-base documentation](https://git-scm.com/docs/git-merge-base) | Defines best common ancestors and the possibility of multiple merge bases; the UI must not conceal ambiguity. |
| [Pinned Gitoxide API](https://docs.rs/gix/0.87.1/gix/), [0.87 release](https://github.com/GitoxideLabs/gitoxide/releases/tag/gix-v0.87.0), and [0.88 release](https://github.com/GitoxideLabs/gitoxide/releases/tag/gix-v0.88.0) | Dated evidence for retaining gix and assessing an independent upgrade. Use spyc's `Cargo.toml`/`Cargo.lock` and pinned source when checking complete workflows. |

Internal design references:

- [`PROJECTS_PLAN.md`](PROJECTS_PLAN.md): project identity, result routing,
  pane attribution, recovery, and release sequencing.
- [`ROADMAP.md`](../../ROADMAP.md): the shared-working-set thesis and release
  priorities.
- [`ARCHITECTURE.md`](../../ARCHITECTURE.md): MVU, effect execution, worker
  delivery, Git boundaries, and per-column state.
- [`DESIGN.md`](../../DESIGN.md): component vocabulary, frame/pane focus, and
  binding taxonomy.
- [`docs/AGENT_ORCHESTRATION.md`](../AGENT_ORCHESTRATION.md): activity and
  advisory scope coordination.
- [`PATH_HANDOFF_PLAN.md`](PATH_HANDOFF_PLAN.md): existing path handoff and
  the distinction between copying context and submitting agent instructions.
- [`VSCode Git study`](../archive/VSCODE_GIT_STUDY.md): earlier scope boundary
  and refresh/caching considerations, retained as historical reasoning.

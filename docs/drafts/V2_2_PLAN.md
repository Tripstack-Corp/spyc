# spyc 2.2 — projects-prep and the daily-driver loop

**Status:** accepted scope, sequencing draft. The seven items below are
decided; the ordering, the per-item technical detail and the exit criteria are
this document's proposal.
**Measured against:** `9df4d7a` (`main`, `2.2.0-CURRENT`).
**Predecessor:** [`docs/archive/LAUNCH_PLAN_2_0.md`](../archive/LAUNCH_PLAN_2_0.md)
(the 2.0 distribution pass). Strategy context: `ROADMAP.md` → "Road to 2.2".

## Thesis

2.2 hardens the daily-driver loop and lands the prerequisites Projects needs,
so 2.3 starts on an approved design instead of a refactor swamp.

Those two halves are not a compromise between features and hygiene. 2.1 was
the release where the tool got used, and using it produced both lists: the
bugs are the ones a person hits weekly, and the refactors are the ones 2.3
would otherwise have to do while also designing Projects. Doing them first
means the Projects PR series argues about state ownership rather than about
`state.left`.

No Projects implementation code lands in 2.2. The Projects *design doc* does.

## Scope

| # | Item | Kind | Tracking |
|---|---|---|---|
| 1 | Pane-identity transport (option B) | prep | [proposal](pane-identity-transport-proposal.md) |
| 2 | One spyc per agent — abstract the column references | prep | [#40](https://github.com/Tripstack-Corp/spyc/issues/40) |
| 3 | Configurable startup pane tabs | prep + feature | [#58](https://github.com/Tripstack-Corp/spyc/issues/58), [plan](PANE_STARTUP_TABS_PLAN.md) |
| 4 | Session forking (`^a f`) | feature | [#8](https://github.com/Tripstack-Corp/spyc/issues/8) |
| 5 | Prompt templates in `.spycrc.toml` | feature | [#71](https://github.com/Tripstack-Corp/spyc/issues/71) |
| 6 | The daily-driver bug set | fix | [#326](https://github.com/Tripstack-Corp/spyc/issues/326), [#327](https://github.com/Tripstack-Corp/spyc/issues/327), [#34](https://github.com/Tripstack-Corp/spyc/issues/34), [#22](https://github.com/Tripstack-Corp/spyc/issues/22), [#11](https://github.com/Tripstack-Corp/spyc/issues/11) |
| 7 | Author `docs/drafts/PROJECTS_PLAN.md` | design | this doc, §7 |

---

## 1. Pane-identity transport

Implement **option B** from
[`pane-identity-transport-proposal.md`](pane-identity-transport-proposal.md):
the `spyc --mcp` proxy reads `$SPYC_PANE_ID` from its own environment and
sends it in the `initialize` handshake; the server binds it to that connection
for the connection's lifetime.

The pieces are already in place on both ends. `open_pane_tab_into`
(`src/app/pane_tabs.rs`) builds `TabInfo` before the spawn precisely so
`info.id` can go into the child's env as `SPYC_PANE_ID`, and the proxy — which
the agent re-execs — inherits it. What is missing is that `mcp::run` →
`run_proxy` forwards JSONL verbatim, and read-tool dispatch resolves context
through the PID-scoped `.spyc-context-<pid>.json` file, which carries no pane
identity.

What this closes:

- **The F1 target design.** The decisions-log entry names per-pane root
  validation as where the MCP `root` override should end up, blocked on
  exactly this transport. With attribution, the session-wide allowed set stays
  as the fallback and per-pane roots *narrow* it.
- **`get_spyc_context` answering for the caller.** Today it reports the focused
  column, so an agent working in worktree X is told about worktree Y the moment
  the user browses elsewhere. This is the change a user notices tomorrow.
- **Scope-registry ownership.** `register_scope` claims are owner-labelled by
  convention; attribution lets a claim bind to a pane and `release_scope`
  refuse one the caller doesn't own.

The proposal's four conditions on B are requirements, not suggestions: bind the
id to the connection and never re-read it per call; validate it against live
tabs on receipt and drop it if unknown; keep every unattributed path working;
and say in SECURITY.md that this is attribution, not authorization. That last
one is already written — #429 added it — so this work must not contradict it.

**Migration.** Non-breaking by construction. An older `spyc --mcp` proxy — one
launched from a `.mcp.json` written by a previous release, or a binary the user
hasn't updated — omits the field, and the server treats that connection exactly
as it does today. Older proxies stay unattributed through at least one release;
nothing may require the field.

## 2. #40 — one spyc per agent

Abstract away the hardcoded `left`/`right` column references so what a column
holds can change. The `Commander` extraction (vsplit Stage 2) already did the
hard half: per-column browser state — `listing`, `cursor`, `rows`, `picks`,
`masks`, `temp_filter`, `view`, sort, `list_generation`, `git_cache`, harpoon —
is one struct, and `cur()` / `cur_mut()` / `col(side)` are the accessors. What
remains is the naming: `state.left` and `state.right` appear ~114 times in
production `src/`, `Side` is a two-variant enum (`Left`, `Right`), and
`right: Option<Commander>` encodes "there may be a second one" in the type.

The existing guard `state_left_listing_dir_uses_are_allowlisted`
(`src/app/mod_tests.rs`) already enforces the rule for the one case that bit —
a spawn/restore cwd must go through `cur()`. It allowlists `run.rs` and
`bootstrap.rs` and covers only `state.left.listing.dir`. Widening its needle is
the cheap way to measure progress here, and a way to make the refactor stick.

Render and fs-watch legitimately name a specific column, so this is not a
"remove every mention" exercise. The target is that *addressing* a column is
always by handle, so a future `Vec<Commander>` (or a per-project column set) is
a change to one place rather than 114.

Standalone value even if 2.3 never happens: the guard's own message describes
the class of bug this prevents — an op targeting column a while the user works
in column b.

## 3. #58 — configurable startup pane tabs

Per [`PANE_STARTUP_TABS_PLAN.md`](PANE_STARTUP_TABS_PLAN.md): a `.spycrc.toml`
knob that opens K tabs in the bottom pane at startup, each with a command and
an optional cwd, mirroring what `^a c` creates interactively. No splits, no
grid.

**Step zero is re-resolving the plan's file pointers**, which predate the MVU
decomposition and resolve nowhere. `open_pane_tab` is now `src/app/pane_tabs.rs`
(not `src/app/mod.rs:4646`), `App::new` is `src/app/bootstrap.rs` (not
`:875`); `PaneConfig` (`src/config/mod.rs`) and `Action::PaneTabByIndex`
(`src/keymap/action.rs`) kept their homes but not their line numbers. The plan
now carries this warning in its own header — do the pass before writing code,
not while.

Two things the plan predates and should be reconciled with:

- **`[pane] new_tab_cwd`** already decides a new tab's default cwd
  (`AppState::default_pane_cwd`). A per-tab `cwd` in config is an override of
  that, not a parallel mechanism.
- **Session restore** already round-trips a multi-tab pane. Startup tabs must
  not fight it: a `-r` resume restores what was saved; the config set is what
  you get on a *fresh* launch.

**Why it is projects-prep.** A declarative tab set — commands plus cwds, named
and reproducible — is the config half of a 2.3 project definition. Getting the
schema right once here is cheaper than migrating it later.

## 4. #8 — session forking (`^a f`)

Duplicate a pane tab so an agent conversation can branch without losing the
prior line of inquiry. The issue's own assessment — "implementable on current
plumbing" — still holds; the plumbing has grown since.

What exists: `TabInfo` carries the command, the cwd and the pinned session id;
`open_pane_tab_into` takes a `TabSlot` so a spawn can append or replace; each
agent profile owns its resume mechanics (`ResumeAction::ClaudeStdin` types
`/resume <sid>` into a fresh spawn with verify-and-retry, codex resumes by
UUID, agy by `--conversation`, zot by `--continue`); and `-r` already drives
all four. A fork is that same restore path aimed at a *live* tab's session id
instead of a saved one.

The two questions the implementation has to answer:

- **What "fork" means per agent.** Claude's `/resume` continues a conversation;
  two panes on the same session id are two clients of one conversation, not two
  branches of it. Whether spyc can offer a true branch depends on the agent, and
  where it can't, the honest behaviour is a second view that says so rather than
  a silent shared session.
- **Scrollback.** The issue asks for scrollback replayed. Where the history
  lives differs per agent and per mode — `docs/HARNESS.md` §3 is the map, and
  `^a v`'s source selection (capture vs on-disk transcript, `T` to swap) is the
  existing machinery to reuse rather than duplicate.

`^a f` is a **pane-tier** binding, so it belongs on the `^a` prefix and its
`Action::tier()` must be `Pane` — the guard
`leader_and_pane_namespaces_respect_tiers` fails the build otherwise.

## 5. #71 — prompt templates in `.spycrc.toml`

User-defined macros that send a pre-composed prompt to the focused agent with
picks / inventory / cursor substituted — the thing that turns spyc into a
keyboard-driven launcher for repeated workflows ("review these", "explain this
diff").

The two mechanisms to build on, both already load-bearing:

- **`shell::expand_percent`** is the substitution engine the `unix` DSL verb
  uses (`%` expands to the target paths, `%%` is a literal percent, and it
  *refuses* rather than silently mis-expanding a non-UTF-8 path). A prompt
  template wants the same expander, possibly a wider token set.
- **`send_selection_to_pane`** (`src/app/clipboard.rs`) is the existing
  spyc→pane text path, with the path-anchoring problem
  [`PATH_HANDOFF_PLAN.md`](PATH_HANDOFF_PLAN.md) documents (paths are anchored
  on `PROJECT_HOME`, which breaks when the agent's cwd isn't the project root).
  A template that emits paths inherits that bug. Path handoff is **not** in 2.2
  scope, so the template design has to state which anchor it uses rather than
  inherit the ambiguity.

**Binding shape.** This is a new DSL verb alongside `unix` / `command` / `lua` /
`jump`. Every one of those is `is_executing` — only `$HOME/.spycrc.toml` may
bind them — and a prompt template that can be triggered by a project-local
config would let a repo dictate what gets typed at an agent. Decide that
explicitly, and default to `is_executing`.

## 6. The daily-driver bug set

Five issues. Each paragraph below is orientation — where the behaviour lives
and what is already known — not a fix design.

### #326 — the first keystrokes into a fresh pane are dropped

A fixed-length prefix (10 characters in the reported run) never reaches the
child. Reproducible on demand: it was found re-recording the README hero GIF,
which is a scripted VHS tape. The report rules out the three obvious
explanations with evidence — not timing (3 s and 5 s sleeps, and a `Wait` on the
child's own banner, all lost the same 10 characters), not focus, and not the
child's `clear` (an instrumented `read -r` received the truncated string, so the
bytes never reached the pty).

A *fixed* prefix that no delay changes points at something that **consumes**
bytes rather than something that isn't ready yet. The spawn path is
`open_pane_tab_into` (`src/app/pane_tabs.rs`) → `Pane::spawn_with_env`
(`src/pane/mod.rs`) → `PtyHost::spawn` with `exec_replace: true`, and
`shell::pane_invocation` turns that into `$SHELL -i -c 'exec <cmd>'`. That
wrapper is an **interactive** shell doing a full rc pass on the pty before it
`exec`s — and an interactive shell's line editor flushing or reading pending
input is exactly a "consumes N bytes" mechanism. `pane_invocation` already
drops the `-i` when the pane command is itself an rc-sourcing shell (SPYC-TRAP
`pane-shell-rc-double-source`), so the invocation policy is one pure function
and cheap to vary in a test.

That is a hypothesis, not a diagnosis. The discriminating experiment is cheap:
spawn a pane running `cat > /tmp/q.txt`, type immediately, and compare the byte
count under `-i` and without it. Land the failing test first.

### #327 — a partially-failed `remove_worktree` strands the worktree

The issue carries a complete diagnosis, verified against the tree.
`remove_inner` (`src/git/worktree.rs`) removes in two non-atomic steps —
`remove_dir_all(path)`, then `remove_dir_all(admin_dir)` — and on macOS the
first returns `ENOTEMPTY` when a directory gains entries during the walk. A
`target/` with a background writer (rust-analyzer's proc-macro server, a cargo
process, an editor indexer) is that case. By the time it fails it has already
unlinked the `.git` gitfile, which is the marker both retry paths key on:
`safe_remove_worktree` (`src/app/worktree_clean.rs`) refuses with "not a git
worktree (no .git)", and `git worktree remove` refuses with "validation failed".
The branch deletion never runs either, because `safe_remove_worktree` does it
*after* `remove_force`.

So the operation is not resumable — the first failure destroys the marker a
retry would need. The issue's suggested direction (rename the worktree dir
aside first, so a failed delete leaves orphaned bytes rather than a half
worktree) is a starting point; the minimum is that a missing `.git` beside a
live admin-dir entry reads as "finish the removal", not "this was never a
worktree".

This one is worse than its frequency suggests, because
`create_worktree`/`remove_worktree` is the workflow AGENTS.md tells every agent
to use, and recovery is three manual git commands.

### #34 — Claude PTY scrollback artifacts

Half of this is already answered and the issue predates the answer.
`docs/HARNESS.md` §3 documents that **inline** claude is the one agent with two
history sources — spyc's vt100 capture (the grid, which accumulates repaint
artifacts from progress bars, spinners and cursor repositioning) and claude's
own on-disk transcript (real text, searchable). #391 shipped `T` to swap
between them, and `[pane] claude_transcript_scrollback` picks the default. For
*reading* history, the artifact-free source exists.

What remains is the live view: the CLI scrolling halfway up the pane, and `^L`
redrawing the visible screen without being able to repair the grid. The issue's
own suggestion — pin the CLI to the bottom — is a display question about
`src/pane/widget.rs` (vt100 → ratatui) and the 10,000-row parser in
`Pane::spawn_with_env`, not a scrollback-source question. Scope it to that, and
say in the issue that the reading half is closed.

### #22 + #11 — MCP takeover and multi-instance coexistence

These are one investigation. The takeover prompt
(`prompt_mcp_takeover_if_needed`, `src/lib.rs`) runs **once, at startup, before
`App::new`**, and only when `detect_existing_spyc*` finds a config in the launch
cwd already naming another PID. If nothing is found it returns `true` —
takeover permitted — and that value is stashed as `view.mcp_takeover_allowed`
for the rest of the process's life.

But agent MCP configs are written **lazily, on agent-pane launch**, not at
startup. So a second instance started in a directory with no `.mcp.json` yet is
never prompted; later it opens an agent pane, `ensure_mcp_json` runs with
`takeover_allowed: true`, and takes over silently. The instance that learns
about it is the *first* one, via the `McpCommand::TakenOver` flash
("MCP taken over by spyc PID N…", `src/app/mcp.rs`). That asymmetry is exactly
what #22 reports.

#11 is the test. `src/mcp/config.rs`'s test module covers only the deterministic
branches of `decide_takeover` — own socket, dead socket — and its comment says
the live-socket `TookOver` / `Skipped` branches "are exercised by the end-to-end
takeover behavior". There is no such test. Write it, watch it fail against the
behaviour above, then fix the prompt.

## 7. Author `docs/drafts/PROJECTS_PLAN.md`

A tracked 2.2 deliverable, design only. It is listed here because 2.3's scope
depends on it existing *and being approved* before any code lands — that is the
whole point of doing prep first.

The questions it must answer. **This plan does not answer them**; listing them
is the deliverable's acceptance criteria, not its content.

1. **Per-project Model state inventory.** Which `AppState` fields lift into a
   project struct and which stay global. `Commander` is the obvious per-project
   unit and takes its per-column state (`harpoon` included) with it; the flat
   `AppState` fields — `marks`, `inventory`, `graveyard`, `project_home`, the
   pane/tab set, `mounts`, `frecency`, the pager history — each need a call,
   with a reason.
2. **MCP socket topology for N project homes behind one process.** Today the
   socket is PID-scoped and one instance owns MCP for a directory. With several
   project homes in one process: one socket or several, how takeover and the
   orphan sweep change, and how the trusted-root sidecar (`write_root_marker`)
   works per project. Builds on the pane-identity attribution from §1 — a
   connection knows its pane, and a pane knows its project.
3. **Recovery manifest shape.** The existing session save/restore and the
   debounced crash-sufficient autosave (`Deadline::Autosave`, `autosave_action`,
   stable per-process id, `fs::write_atomic`) already round-trip one session.
   What a multi-project manifest looks like on top of that, and what `spyc -r`
   offers when several projects were open.
4. **Keymap-tier placement for project switching.** The taxonomy is
   guard-enforced (`leader_and_pane_namespaces_respect_tiers`): workspace
   operations live on the leader, so project switching is `Tier::Global` and
   belongs under `Space`. Which keys, and what happens to `Space p` / `Space P`
   (today PROJECT_HOME jump and set), needs deciding rather than assuming.
5. **The `projects` status-bar segment.** The bar is
   `🌶️ | PROJECT_HOME | SESSION | path | git | suffix` and it is already
   crowded. What the segment shows, what it displaces, and what happens at
   narrow widths.
6. **Attention/notification aggregation.** What it reuses from the shipped
   agent-awareness channel — `report_status`, the per-agent status hooks, the
   `Blocked`/`Done` transition, `Effect::Notify`, the visual bell — and what
   genuinely has to be new to answer "which agent, in which project, needs me".

## Non-goals for 2.2

- **No Projects implementation code.** The design doc, and nothing else.
- **No CounterTop revival.** `docs/archive/V1_60_PLAN.md` stays archived
  design-history.
- **No frame mirroring, no input forwarding, no headless or `--detached`
  peers.** These are the parts that fight the single-process core; window
  elimination makes them unnecessary rather than deferred.
- **No cross-process discovery files.** Nothing that has one spyc enumerate
  another.
- **No path handoff** ([#9](https://github.com/Tripstack-Corp/spyc/issues/9) /
  [#59](https://github.com/Tripstack-Corp/spyc/issues/59)). Item 5 touches the
  same seam and must name its anchor, but the general problem stays out.

## Staging

One PR per numbered item unless the tree argues otherwise. Two hard
dependencies, the rest is scheduling.

| PR | Item | Depends on | Why here |
|---|---|---|---|
| 1 | #326 — dropped first keystrokes | — | Highest-frequency bug on the dog-fooding path, and the investigation is independent of everything else. Failing test first. |
| 2 | #327 — stranded worktree | — | Same: independent, and it breaks the workflow every agent is told to use. |
| 3 | #40 — abstract the column references | — | Early, because it touches shared surfaces. Every later PR that reads a column rebases onto it rather than the reverse. |
| 4 | Pane-identity transport | — | Before anything that wants attribution. Ships the handshake field plus per-connection binding; per-pane roots and `get_spyc_context`-answers-for-the-caller can be the same PR or the next, but not before this. |
| 5 | #22 + #11 — takeover prompt + coexistence test | 4 (shared MCP surface) | The test lands red first. Sequenced after 4 so it is written against the attributed server rather than twice. |
| 6 | #58 — startup pane tabs | 3 | Config schema plus startup wiring. The pointer re-resolution is step zero *inside* this PR, not a separate one. |
| 7 | #8 — session forking | 3 | Pane-tier binding, tab duplication, per-agent resume. |
| 8 | #71 — prompt templates | — | New DSL verb; independent of the rest, so it floats to wherever there is room. |
| 9 | #34 — pin the CLI to the bottom | — | Display-layer work, unblocked, but the least certain scope of the six bugs — it may resolve to "documented, not fixed". |
| — | `PROJECTS_PLAN.md` | 4 (informs §2) | Written across the release, reviewed at the end. Not a PR in the sequence; a deliverable gating 2.3. |

Bugs are interleaved deliberately: 1 and 2 open the release so a daily driver
feels the difference immediately, and 9 sits late because its scope is the one
that might shrink on contact.

## Exit criteria

Per item, observable rather than asserted.

**#326 —** Type into a pane the instant `^a c` returns and every character
reaches the child. Pinned by a test that fails on the current tree: spawn a
pane, write bytes, compare what the child received byte-for-byte. Exit: the
VHS tape records the question it was always meant to depict, with no
compensating repaint in `fake-claude.sh`.

**#327 —** A `remove_worktree` interrupted mid-delete leaves git's view
consistent, and re-running it finishes the job. Exit: a test that fails the
first removal (a directory that regains an entry during the walk) and asserts
the second one succeeds, deletes the branch, and leaves no `.git/worktrees/`
admin dir behind.

**#40 —** Addressing a column goes through a handle everywhere it isn't
legitimately naming a specific one. Exit: the widened
`state_left_listing_dir_uses_are_allowlisted` guard passes with an allowlist
that names only render and fs-watch, each with a why.

**Pane identity —** An agent in worktree X gets X from `get_spyc_context` while
the user browses Y. Exit: an older proxy that omits the field still works,
proved by a test that drives the server with an `initialize` carrying no pane
id; and SECURITY.md's attribution-is-not-authorization paragraph still reads
true.

**#22 + #11 —** Two spyc instances in one directory coexist, and the second one
to want MCP asks before taking it. Exit: an integration test that stands up two
instances, exercises the live-socket `TookOver` and `Skipped` branches
end-to-end, and fails against today's tree.

**#58 —** `[pane] startup_tabs` in `.spycrc.toml` opens the declared tabs on a
fresh launch, and `spyc -r` still restores the saved set instead. Exit: both
paths covered, `--print-config` emits the new keys with comments, and
CONFIGURATION.md documents them in the same commit.

**#8 —** `^a f` on a live agent tab produces a second tab on that conversation,
with its history readable. Exit: the per-agent behaviour is documented in
`docs/HARNESS.md` including where it is a shared session rather than a branch —
no silent pretence.

**#71 —** A template bound in `~/.spycrc.toml` sends a composed prompt with
picks substituted, and a project-local `.spycrc.toml` cannot bind one. Exit:
the DSL verb is `is_executing`, CONFIGURATION.md documents the token set, and
the substitution refuses a non-UTF-8 path the way `expand_percent` does.

**#34 —** Either the live pane stops drifting, or the issue is closed with the
reading half documented and the display half stated as out of scope. Both are
acceptable outcomes; silently shipping neither is not.

**`PROJECTS_PLAN.md` —** All six questions in §7 answered, with a decision and
a reason for each, and the doc marked approved before 2.3 opens.

## Docs each PR must carry (same commit, not a follow-up)

Per AGENTS.md's doc-sync rule: `FEATURES.md` and `docs/KEYBINDINGS.md` +
`src/ui/help.rs` for any new binding (#8), `CONFIGURATION.md` and
`--print-config` for any new config key (#58, #71), `docs/HARNESS.md` for
per-agent behaviour (#8, #34), `SECURITY.md` for the MCP surface (item 1 —
extend, don't contradict), `AGENTS.md`'s module index for any new module, and
`ROADMAP.md`'s decisions log when a decision here supersedes one recorded
there.

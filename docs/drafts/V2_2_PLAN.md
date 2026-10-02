# spyc 2.2 — projects-prep and the daily-driver loop

**Status:** shipped, apart from §7's approval. Reconciled against `fed859b7`
on 2026-10-02: each item's section, the exit criteria and the docs list say
what shipped, and a section's plan, where kept, is the record of what was
proposed. Archived when 2.2 tags.
**Measured against:** `9df4d7a` (`main`, `2.2.0-CURRENT`).
**Predecessor:** [`docs/archive/LAUNCH_PLAN_2_0.md`](../archive/LAUNCH_PLAN_2_0.md)
(the 2.0 distribution pass). Strategy context: `ROADMAP.md` → "Road to 2.2".
**Progress (2026-10-02):** every item is on `main`. §7's deliverable,
`PROJECTS_PLAN.md`, is authored (#520) and in the owner's review. The tag waits
for the doc to ship, not for its approval: approval gates 2.3, which is where
its exit criterion always put it (ROADMAP decisions log, 2026-10-02). The one
release blocker outside the scope,
[#490](https://github.com/Tripstack-Corp/spyc/issues/490) (publishing
`spyc-vt-sys` to crates.io), is closed.

## Thesis

2.2 hardens the daily-driver loop and lands the prerequisites Projects needs,
so 2.3 starts on an approved design rather than a refactor.

Both lists came out of daily-driving 2.1: the bugs are the ones a person hits
weekly, and the refactors are the ones 2.3 would otherwise do while also
designing Projects.

No Projects implementation code lands in 2.2 — only the design doc.

The VT engine (§8) joined after the scope was accepted: the spike that was
meant to gate a 3.0 decision reported that the incumbent is unfixable on its
own timeline, and #34's engine half was already 2.2 scope. Landing it here
buys a release of dogfood soak before 3.0 makes screen reconstruction
load-bearing.

## Scope

| # | Item | Kind | Tracking | Status |
|---|---|---|---|---|
| 1 | Pane-identity transport (option B) | prep | [#491](https://github.com/Tripstack-Corp/spyc/issues/491), [proposal](pane-identity-transport-proposal.md) | shipped (#507); per-pane roots dropped |
| 2 | One spyc per agent — abstract the column references | prep | [#40](https://github.com/Tripstack-Corp/spyc/issues/40) | shipped (#504) |
| 3 | Configurable startup pane tabs | prep + feature | [#58](https://github.com/Tripstack-Corp/spyc/issues/58), [plan](../archive/PANE_STARTUP_TABS_PLAN.md) | shipped (#482) |
| 4 | Session forking (`^a F`) | feature | [#8](https://github.com/Tripstack-Corp/spyc/issues/8) | shipped (#516) |
| 5 | Prompt templates in `.spycrc.toml` | feature | [#71](https://github.com/Tripstack-Corp/spyc/issues/71) | shipped (#517) |
| 6 | The daily-driver bug set | fix | [#326](https://github.com/Tripstack-Corp/spyc/issues/326), [#327](https://github.com/Tripstack-Corp/spyc/issues/327), [#9](https://github.com/Tripstack-Corp/spyc/issues/9), [#34](https://github.com/Tripstack-Corp/spyc/issues/34), [#452](https://github.com/Tripstack-Corp/spyc/issues/452), [#22](https://github.com/Tripstack-Corp/spyc/issues/22), [#11](https://github.com/Tripstack-Corp/spyc/issues/11) | shipped: #326 (#464), #452 (#457), #34 (#465), #327 (#500), #9, #22 + #11 (#509) |
| 7 | Author `docs/drafts/PROJECTS_PLAN.md` | design | [#492](https://github.com/Tripstack-Corp/spyc/issues/492), this doc, §7 | drafted, in review |
| 8 | The VT engine — libghostty-vt replaces vt100 | prep + fix | [spike](VT_ENGINE_SPIKE.md), [#34](https://github.com/Tripstack-Corp/spyc/issues/34), [#452](https://github.com/Tripstack-Corp/spyc/issues/452), [#453](https://github.com/Tripstack-Corp/spyc/issues/453) | shipped (PRs 10–16: #457–#462, #465); the vt100 deletion (#453) is the first commit after the tag |

---

## 1. Pane-identity transport

**Shipped (#507).** A connection binds to the live tab its `initialize`
names; `get_spyc_context` adds that tab as `pane`, and the targeting tools
default to it. Per-pane root narrowing, the F1 half, is dropped: it restricts
nothing and would reject the worktrees an agent creates (ROADMAP decisions log,
2026-09-30).

Implement **option B** from
[`pane-identity-transport-proposal.md`](pane-identity-transport-proposal.md):
the `spyc --mcp` proxy reads `$SPYC_PANE_ID` from its own environment and
sends it in the `initialize` handshake; the server binds it to that connection
for the connection's lifetime.

Both ends already carry the id. `open_pane_tab_into` (`src/app/pane_tabs.rs`)
builds `TabInfo` before the spawn so `info.id` can go into the child's env as
`SPYC_PANE_ID`, and the proxy — re-exec'd by the agent — inherits it. What is
missing: `mcp::run` → `run_proxy` forwards JSONL verbatim, and read-tool
dispatch resolves context through the PID-scoped `.spyc-context-<pid>.json`
file, which carries no pane identity.

What this closes:

- **The F1 target design, decided the other way.** The decisions-log entry
  named per-pane root validation as where the MCP `root` override should end
  up, blocked on this transport. With the transport built, narrowing was
  dropped (decisions log, 2026-09-30): the id is forgeable, so it restricts
  nothing, and it would reject the worktrees an agent creates. The
  session-wide allowed set is the only one.
- **`get_spyc_context` answering for the caller.** Today it reports the focused
  column, so an agent working in worktree X is told about worktree Y whenever
  the user browses elsewhere.
- **Scope-registry ownership, as a default.** `register_scope` and
  `wait_for_scope_clear` take the caller's own tab when the call names none
  (#507). `release_scope` still drops a claim by id whoever asks: attribution,
  not authorization, as SECURITY.md says.

The proposal's four conditions on B all hold: bind the id to the connection and
never re-read it per call; validate it against live tabs on receipt and drop it
if unknown; keep every unattributed path working; and say in SECURITY.md that
this is attribution, not authorization. #429 already wrote that last paragraph,
so this work must not contradict it.

**Migration.** An older `spyc --mcp` proxy — launched from a `.mcp.json`
written by a previous release, or an un-updated binary — omits the field, and
the server treats that connection as it does today. Older proxies stay
unattributed through at least one release; nothing may require the field.

## 2. #40 — one spyc per agent

**Shipped (#504).** Widening the guard first found nine places that acted on
column `a` from column `b`, fixed separately in #502. The guard is now
`columns_are_addressed_through_handles`, and its allowlist names
`render/mod.rs`, `render/inner.rs` and `watch.rs`. The plan below is the
record.

Abstract away the hardcoded `left`/`right` column references so what a column
holds can change. The `Commander` extraction (vsplit Stage 2) did most of the
work: per-column browser state — `listing`, `cursor`, `rows`, `picks`,
`masks`, `temp_filter`, `view`, sort, `list_generation`, `git_cache`, harpoon —
is one struct, and `cur()` / `cur_mut()` / `col(side)` are the accessors. What
remains is the naming: `state.left` and `state.right` appear **~67 times in
production modules** (`#[cfg(test)]` tails stripped with
`guard_support::production_half`, the test harness excluded), `Side` is a
two-variant enum (`Left`, `Right`), and `right: Option<Commander>` encodes
"there may be a second one" in the type.

Derive that number the same way before acting on it. `grep -c` over `src/`
returns 204 — threefold the production surface, because the house convention
keeps tests in the same file. About 50 of those sit in in-file `#[cfg(test)]`
tails and ~87 in `harness_tests/` and `test_harness.rs`.

The guard `state_left_listing_dir_uses_are_allowlisted` (`src/app/mod_tests.rs`)
already enforces the rule for the case that bit: a spawn/restore cwd must go
through `cur()`. It allowlists `run.rs` and `bootstrap.rs`, and covers only
`state.left.listing.dir`. Widening its needle measures progress and keeps the
refactor from eroding.

Render and fs-watch legitimately name a specific column, so not every mention
goes. The target is that *addressing* a column is always by handle, so a future
`Vec<Commander>` or per-project column set changes one place.

This has value on its own: the guard's message describes the bug it prevents —
an op targeting column a while the user works in column b.

## 3. #58 — configurable startup pane tabs

Per [`PANE_STARTUP_TABS_PLAN.md`](../archive/PANE_STARTUP_TABS_PLAN.md): a `.spycrc.toml`
knob that opens K tabs in the bottom pane at startup, each with a command and
an optional cwd, mirroring what `^a c` creates interactively. No splits, no
grid.

**Step zero is re-resolving the plan's file pointers**, which predate the MVU
decomposition and resolve nowhere. `open_pane_tab` is now `src/app/pane_tabs.rs`
(not `src/app/mod.rs:4646`), `App::new` is `src/app/bootstrap.rs` (not `:875`);
`PaneConfig` (`src/config/mod.rs`) and `Action::PaneTabByIndex`
(`src/keymap/action.rs`) kept their homes but not their line numbers. The plan
carries this warning in its own header. Do the pass first.

Two things the plan predates:

- **`[pane] new_tab_cwd`** already decides a new tab's default cwd
  (`AppState::default_pane_cwd`). A per-tab `cwd` in config overrides that
  rather than running beside it.
- **Session restore** already round-trips a multi-tab pane. A `-r` resume
  restores what was saved; the config set applies to a fresh launch.

**Why it is projects-prep.** A declarative tab set — commands plus cwds, named
and reproducible — is the config half of a 2.3 project definition, so the
schema is worth getting right here.

## 4. #8 — session forking (`^a F`)

**Shipped (#516).** claude branches with `--resume <id> --fork-session` and codex
with `codex fork <id>`, each into a new session id, so neither answer to the
first question below is a shared session. agy and zot have no branch, and `^a F`
says so. The scrollback question answered itself: both agents replay the
branch's history on screen, and `^a v` reads it from the transcript, following
a codex fork's `history_base` back into its parent's rollout. The key is `F`
because `^a f` had become the pane-side split-height flip (#352) after the
issue was filed. `docs/HARNESS.md` §4 has the per-agent table.

Duplicate a pane tab so an agent conversation can branch without losing the
prior line of inquiry. The issue's assessment — "implementable on current
plumbing" — still holds.

What exists: `TabInfo` carries the command, the cwd and the pinned session id;
`open_pane_tab_into` takes a `TabSlot` so a spawn can append or replace; each
agent profile owns its resume mechanics (`ResumeAction::ClaudeStdin` types
`/resume <sid>` into a fresh spawn with verify-and-retry, codex resumes by
UUID, agy by `--conversation`, zot by `--continue`); and `-r` drives all four.
A fork is that restore path aimed at a live tab's session id rather than a
saved one.

Two questions the implementation has to answer:

- **What "fork" means per agent.** Claude's `/resume` continues a conversation,
  so two panes on the same session id are two clients of one conversation
  rather than two branches. Whether a true branch is available depends on the
  agent; where it isn't, say so in the UI instead of presenting a shared
  session as a fork.
- **Scrollback.** The issue asks for scrollback replayed. Where the history
  lives differs per agent and per mode — `docs/HARNESS.md` §3 is the map, and
  `^a v`'s source selection (capture vs on-disk transcript, `T` to swap) is the
  existing machinery.

`^a F` is a **pane-tier** binding, so it belongs on the `^a` prefix and its
`Action::tier()` must be `Pane`. The guard
`leader_and_pane_namespaces_respect_tiers` fails the build otherwise.

## 5. #71 — prompt templates in `.spycrc.toml`

**Shipped (#517).** A `[prompts]` table of name → text, a `prompt <name>` DSL verb,
and `:prompt [name]`. The expander is its own rather than `expand_percent`,
because that one shell-quotes for `sh -c` and anchors nothing. The template is
split at its tokens on the keypress (`%` the selection, `%i` the inventory,
`%d` the directory, `%%` a literal), which is where an empty token or a
non-UTF-8 path is refused, and rendered on delivery against the pane's fresh
cwd through the `^a s` per-path anchor. It is pasted, bracketed when the child
asked, and never submitted. Both halves of the trust question are closed:
`prompt` is `is_executing`, and a project file's `[prompts]` is dropped with a
warning, since a `$HOME` binding naming a template a repo defined would type
the repo's words.

User-defined macros that send a pre-composed prompt to the focused agent with
picks / inventory / cursor substituted — a keyboard-driven launcher for
repeated workflows ("review these", "explain this diff").

Two mechanisms to build on:

- **`shell::expand_percent`** is the substitution engine behind the `unix` DSL
  verb: `%` expands to the target paths, `%%` is a literal percent, and it
  refuses rather than silently mis-expanding a non-UTF-8 path. A prompt
  template wants the same expander, possibly with a wider token set.
- **`send_selection_to_pane`** (`src/app/clipboard.rs`) is the existing
  spyc→pane text path. Its anchoring is settled by §6 landing Option A first:
  **relative under the target pane's live cwd, absolute otherwise**. A template
  that emits paths uses that policy. This is why the two are sequenced — an ad
  hoc anchor chosen inside #71 would become handoff policy without the design
  work behind it.

**Binding shape.** This is a new DSL verb alongside `unix` / `command` / `lua` /
`jump`. All four are `is_executing`, so only `$HOME/.spycrc.toml` may bind
them. A prompt template triggered by a project-local config would let a repo
dictate what gets typed at an agent, so decide this explicitly and default to
`is_executing`.

## 6. The daily-driver bug set

Seven issues, in five subsections: #452 is #34's adapter half, and #22 + #11
were one investigation. Each subsection opens with what shipped; the rest is
the pre-fix investigation, kept as the record.

### #326 — the first keystrokes into a fresh pane are dropped

**Shipped (#464).** The cause was not a pty race. The status-hook consent
prompt, raised after the pane spawned and took focus, swallowed every key that
wasn't `y`/`n`, so a fresh agent's first words never reached it, and a `y`
among them granted consent. The investigation below is the record of what was
suspected first.

A fixed-length prefix (10 characters in the reported run) never reaches the
child. Reproducible on demand: it was found re-recording the README hero GIF,
which is a scripted VHS tape. The report rules out the three obvious
explanations with evidence — not timing (3 s and 5 s sleeps, and a `Wait` on the
child's own banner, all lost the same 10 characters), not focus, and not the
child's `clear` (an instrumented `read -r` received the truncated string, so the
bytes never reached the pty).

A fixed prefix that no delay changes suggests something is consuming the bytes
rather than not being ready for them. The spawn path is `open_pane_tab_into`
(`src/app/pane_tabs.rs`) → `Pane::spawn_with_env` (`src/pane/mod.rs`) →
`PtyHost::spawn` with `exec_replace: true`, and `shell::pane_invocation` turns
that into `$SHELL -i -c 'exec <cmd>'`. That wrapper is an interactive shell
doing a full rc pass on the pty before it `exec`s, and an interactive shell's
line editor can flush or read pending input. `pane_invocation` already drops
the `-i` when the pane command is itself an rc-sourcing shell (SPYC-TRAP
`pane-shell-rc-double-source`), so the invocation policy is one pure function
and cheap to vary in a test.

That is a hypothesis. The discriminating experiment: spawn a pane running
`cat > /tmp/q.txt`, type immediately, and compare the byte count with and
without `-i`. Land the failing test first.

### #327 — a partially-failed `remove_worktree` strands the worktree

**Shipped (#500).** The tree is renamed aside and its admin dir removed
before the delete runs, so a delete that fails partway strands bytes, never
half a worktree, and running `remove_worktree` again finishes one an older
spyc left half-removed.

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
after `remove_force`.

The operation is therefore not resumable: the first failure destroys the marker
a retry needs. The issue's suggested direction — rename the worktree dir aside
first, so a failed delete leaves orphaned bytes rather than a half worktree —
is a starting point. The minimum is that a missing `.git` beside a live
admin-dir entry reads as "finish the removal".

Frequency understates it: `create_worktree`/`remove_worktree` is the workflow
AGENTS.md tells every agent to use, and recovery is three manual git commands.

### #9 — `^a s` anchors paths on PROJECT_HOME, not on the pane's cwd

**Shipped (#501), as planned.** Paths go out relative under the receiving
pane's live cwd, read when the text is delivered, and absolute otherwise.

Option A of [`PATH_HANDOFF_PLAN.md`](PATH_HANDOFF_PLAN.md), and nothing else
from it. `send_selection_to_pane` (`src/app/clipboard.rs`) makes each selected
path relative to `project_home` and leaves everything else absolute. When the
pane's agent is working in a worktree rather than at the project root, the
relative path resolves against the wrong directory. The plan records spyc
pasting `book-org/client-api-20-contract/docs`, the agent running
`cd book-org/…` from `~/src/tripstack_platform`, and getting
`no such file or directory`.

It sits in the bug set for the same reason #327 does: it breaks in the workflow
AGENTS.md prescribes — an agent in its own worktree, driven from a spyc column
browsing somewhere else.

The infrastructure exists. `TabEntry::live_cwd` (`src/pane/tabs.rs`) tracks the
pane's actual cwd, refreshed off-thread behind a cache via
`proc_cwd::cwd_for_pid` (`readlink /proc/<pid>/cwd` on Linux, in-process
`sysinfo` on macOS since #356, `None` elsewhere). The change is an anchor swap
plus the plan's no-`~`-collapse rule — claude's `Read` wants real absolute
paths and won't reliably expand `~`, and the in-tree relative path already
carries the terseness.

It degrades safely: an unknown or stale `live_cwd` falls through to the
absolute tier, which is verbose but correct.

The rest of that document — terse-token expansion, the hooks, the four-channel
topology argument, the consumer-aware `^a s`/`^a S` split — stays under
[#59](https://github.com/Tripstack-Corp/spyc/issues/59) and out of 2.2.

### #34 — Claude PTY scrollback artifacts

**Shipped across both halves:** SGR 2 in the adapter (#457, closing #452),
and the four engine defects with the engine swap (#465).

This was the least certain scope of these bugs and the spike settled it: it
is **one adapter defect and a set of engine defects**, and they are fixed in
different stages of §8. The adapter half is split out as
[#452](https://github.com/Tripstack-Corp/spyc/issues/452); #34 is retitled to
the engine half, which is the bulk of it.

The reading half was already answered before the issue was filed.
`docs/HARNESS.md` §3 documents that inline claude is the one agent with two
history sources — spyc's terminal capture (the grid, which accumulates repaint
artifacts) and claude's own on-disk transcript (real text, searchable). #391
shipped `T` to swap between them and `[pane] claude_transcript_scrollback`
picks the default, so an artifact-free source for reading history exists.

**The adapter half ([#452](https://github.com/Tripstack-Corp/spyc/issues/452), PR 10), independent of the engine:**

- `cell_style` never reads `Cell::dim()`. SGR 2 support arrived in vt100 0.16,
  after the function was written, so a child's dim text renders at normal weight
  — flattening exactly the hierarchy agent CLIs use it for. Note the interaction
  the fix has to state: the unfocused-pane fade already spends
  `Modifier::DIM`, so on an unfocused pane content-dim and focus-dim collapse
  into one another. That is acceptable and pre-existing; it is not a reason to
  pick a different modifier.

A second adapter claim — that `PaneWidget` clobbers ratatui's wide-glyph
continuation cell — **was withdrawn** (spike report §4, dated). `set_string`
claims that column and fills it with a space itself, and vt100 reports the
continuation cell at `bg=Default`, so skipping it and writing a space into it
are byte-identical. The finding was an artifact of the harness comparing two
differently-normalized rows. Recorded here because it briefly made the adapter
half look like the headline of #34, and it is not.

**The engine half (#34, PR 15), fixed by the swap, not by us:** vt100 has no DEC
special graphics charset, so a child drawing a box with SCS renders literal
`lqqqk`; it loses a row written before a DECSTBM region is set; it retains **0**
scrollback rows under a scroll region where both other engines retain their
content, which is the codex limitation `src/agent/mod.rs:470` documents; and it
silently truncates a grapheme cluster past 18 bytes (`CONTENT_BYTES = 22`, with
`append` returning early), which is why a tag-sequence flag loses its last two
tag characters.

What is **not** in either half: the issue's own suggestion, pinning the CLI to
the bottom of the pane. The spike found no evidence that the live view drifts
for any reason other than the defects above, so that idea is dropped rather
than deferred — if drift survives stages 2 and 6, it gets a fresh issue with a
fresh reproduction.

### #22 + #11 — MCP takeover and multi-instance coexistence

**Shipped (#509), not as planned below.** The investigation found the takeover
itself was the defect: the agents' MCP entry pinned its writer's socket. It
now pins nothing, each agent reaches the spyc that launched it, and the prompt
has nothing to ask (ROADMAP decisions log, 2026-09-30). The #11 test checks
that two live instances in one directory each keep their own agents.

One investigation. The takeover prompt (`prompt_mcp_takeover_if_needed`,
`src/lib.rs`) runs once, at startup, before `App::new`, and only when
`detect_existing_spyc*` finds a config in the launch cwd already naming another
PID. If nothing is found it returns `true` — takeover permitted — and that
value is stashed as `view.mcp_takeover_allowed` for the rest of the process's
life.

Agent MCP configs are written lazily, on agent-pane launch, not at startup. So
a second instance started in a directory with no `.mcp.json` yet is never
prompted; it later opens an agent pane, `ensure_mcp_json` runs with
`takeover_allowed: true`, and it takes over silently. The instance that learns
about it is the first one, via the `McpCommand::TakenOver` flash ("MCP taken
over by spyc PID N…", `src/app/mcp.rs`). That is what #22 reports.

#11 is the test. `src/mcp/config.rs`'s test module covers only the
deterministic branches of `decide_takeover` — own socket, dead socket — and its
comment says the live-socket `TookOver` / `Skipped` branches "are exercised by
the end-to-end takeover behaviour". No such test exists. Write it, confirm it
fails against the behaviour above, then fix the prompt.

## 7. Author `docs/drafts/PROJECTS_PLAN.md`

**Authored (#520), in review.** All seven questions are answered. The owner's
first decisions are recorded in it: per-project config under `~/.config`, the
context marker out of the working tree (which landed early, #525), and
`Space !` as the attention key. Approval gates 2.3, not the 2.2 tag
(decisions log, 2026-10-02).

A tracked 2.2 deliverable, design only. 2.3's scope depends on it being written
and approved before any code lands.

The questions it must answer are listed below as its acceptance criteria. This
plan does not answer them.

1. **Per-project Model state inventory.** Which `AppState` fields lift into a
   project struct and which stay global. `Commander` is the obvious per-project
   unit and takes its per-column state (`harpoon` included) with it. The flat
   `AppState` fields — `marks`, `inventory`, `graveyard`, `project_home`, the
   pane/tab set, `mounts`, `frecency`, the pager history — each need a call and
   a reason.
2. **MCP socket topology for N project homes behind one process.** Today the
   socket is PID-scoped and one instance owns MCP for a directory. With several
   project homes in one process: one socket or several, how takeover and the
   orphan sweep change, and how the trusted-root sidecar (`write_root_marker`)
   works per project. Builds on §1's attribution — a connection knows its pane,
   and a pane knows its project.
3. **Recovery manifest shape.** Session save/restore and the debounced
   crash-sufficient autosave (`Deadline::Autosave`, `autosave_action`, stable
   per-process id, `fs::write_atomic`) already round-trip one session. What a
   multi-project manifest looks like on top of that, and what `spyc -r` offers
   when several projects were open.
4. **Keymap-tier placement for project switching.** The taxonomy is
   guard-enforced (`leader_and_pane_namespaces_respect_tiers`): workspace
   operations live on the leader, so project switching is `Tier::Global` and
   belongs under `Space`. Which keys, and what happens to `Space p` / `Space P`
   (today PROJECT_HOME jump and set).
5. **The `projects` status-bar segment.** The bar is
   `🌶️ | PROJECT_HOME | SESSION | path | git | suffix` and is already crowded.
   What the segment shows, what it displaces, and what happens at narrow
   widths.
6. **Attention/notification aggregation.** What it reuses from the shipped
   agent-awareness channel — `report_status`, the per-agent status hooks, the
   `Blocked`/`Done` transition, `Effect::Notify`, the visual bell — and what
   has to be new to answer "which agent, in which project, needs me".
7. **Attach-awareness of the state inventory.** For every field question 1
   places in a project struct or leaves global, note whether it serializes for
   a future attach snapshot or is rebuilt client-side. Question 3's recovery
   manifest is written knowing the 3.0 attach snapshot is its superset — one
   inventory, two consumers.

## 8. The VT engine — libghostty-vt replaces vt100

Full evidence in [`VT_ENGINE_SPIKE.md`](VT_ENGINE_SPIKE.md); this section is the
staging, the gate, and the decisions the stages are allowed to make.

**Why it is here and not in 3.0**, which is what the spike recommended: #34's
engine half is already 2.2 scope, so the swap was going to be touched this
release either way; the incumbent is not fixable on its own timeline (bus factor
1, 14 months silent, the panic-fix PRs open since 2021 and the scrollback
rehydration PR since 2021-01-30); and landing the engine now buys a full release
of daily dogfood soak before 3.0 makes screen reconstruction load-bearing.

### The gate (PR 13)

**Every ghostty figure in the spike was measured at ghostty `f4c68d65`, and that
commit cannot ship.** It was chosen because it is ABI-compatible with the
published `libghostty-vt-sys 0.2.1` bindings, and at that commit
`max_scrollback` is inert — retained history saturates at ~840 rows regardless
of the configured budget, which is why the report's per-pane memory figure
carries a "not comparable at face value" caveat.

The shipping pin postdates the scrollback-limits refactor, so the report's
numbers are unverified there. PR 13 re-runs the harness at the pin and must
show: a functional scrollback budget (`sbprobe`), zero panics over ≥50k
`fuzz_diff` iterations, re-graded rehydration, the two known ghostty emit bugs
re-checked, and throughput and memory re-measured. Results land as a **dated
addendum** to the spike report — appended, never a rewrite; the report records
what was measured when.

**If the re-run materially degrades rehydration fidelity or the panic count,
adoption does not proceed.** The series stops after PR 11 and the decisions log
is amended to say so. That is a real branch, not a formality. The gate is also
not one-directional: an **improvement** gets recorded on the same terms, and
the adoption figures in the addendum cite whichever mechanism 3.0 will actually
consume, labelled by mechanism. No blended numbers.

#### Both rehydration mechanisms are graded

The principle is the one that created the gate: **the measured mechanism must
be the shipped mechanism.**

The spike graded rehydration through libghostty's **VT formatter** — emit
escape sequences, replay them into a fresh terminal. The shipping pin also
carries a **snapshot API** (`ghostty_snapshot_encode` /
`ghostty_snapshot_decoder_*`) that did not exist at `f4c68d65`: purpose-built
binary state serialisation rather than a redraw stream. If 3.0 attach and 2.3
recovery will use the snapshot API, then re-running only the formatter
measurement grades the wrong thing.

So PR 13 grades both, for their distinct consumers.

**Formatter — emit to a real terminal.** Its consumer is the cross-emulator
reattach class: a client whose colour depth, cell size and graphics support
differ from the session's. Capability parameterisation is the property that
matters, and the existing criteria are unchanged.

**Snapshot API — state transport and persistence.** Its consumers are 3.0
attach and, likely, 2.3 recovery. Four new criteria:

1. **Fidelity.** Encode, decode into a fresh engine, diff row by row against
   the source — rows, cells, attributes, cursor, scrollback depth. Over the
   **same corpus** as the formatter grading, so the two are comparable rather
   than merely both present.
2. **Size** on that corpus, stated beside the formatter's byte figures.
3. **The continuation round trip.** Cut a stream mid-escape-sequence, snapshot
   with the continuation retained
   (`GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION` plus
   `..._MAX_CONTINUATION_BYTES`, exported through the
   `ghostty_terminal_continuation_*` calls), decode, re-feed the exported
   continuation followed by the remaining bytes, and assert the final state is
   identical to the uncut run. This is the parser-boundary problem answered as
   an API rather than avoided, and it has to be tested because crash recovery
   does not get to wait for a clean boundary. Include the caveat the API's own
   docs carry: set `GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES` back to zero
   after export and before writing post-snapshot input, since exporting an
   empty continuation does not itself disable tracking.
4. **Format stability**, which scopes the whole API. Cross-pin decode is **not
   assumed**, and the answer goes in the addendum whichever way it falls.
   Read at the pin, the answer is already known and it is bounded: the stream
   carries an eight-byte `"GHOSTSNP"` magic and a `u16` version, with per-record
   CRC32C — but `snapshot.h` states that "snapshot format version 1 is a work
   in progress and does not yet carry a binary-compatibility guarantee". So
   snapshots are **transport-only: same binary, same pin**, and never at-rest
   persistence across an upgrade. The version field is what makes that safe
   rather than dangerous — a stale snapshot is detectable and discardable, not
   silently misparsed. `PROJECTS_PLAN.md`'s question 7 needs exactly that
   sentence in writing, so it is recorded here rather than left to be
   rediscovered.

#### Corrections the addendum must carry

Two figures from the spike are already known to be wrong or stale, and the
addendum states so rather than leaving them to circulate:

- **Memory is parity, not an advantage.** The spike reported ghostty at
  875 KiB/pane against vt100's 9,681 KiB — but that comparison was taken where
  ghostty's `max_scrollback` was inert and it retained a quarter as much
  history. At a real budget the derived byte ceiling is **9.64 MiB/pane**
  against vt100's measured **9.68 MiB**, which is parity. The adoption case
  never leaned on memory — it rests on rehydration, robustness and throughput —
  so the addendum says that explicitly and retires the old figure.
- **~840 rows is a number two unrelated causes produce**: the inert
  `max_scrollback` at `f4c68d65`, and a default byte cap binding ahead of the
  line limit at the shipping pin. A future harness run that recognises 840 as
  "the expected ghostty number" and stops looking is the trap this series
  already fell into once.

### Decisions the stages make

**The scrollback budget mapping is a decision, not a conversion.** spyc budgets
in rows (`Pane::spawn_with_env` passes 10,000). The pin's API exposes both a
byte limit and a line limit, and the header is explicit that if both are set the
first-reached one wins. Measured before the series opened: setting only the line
limit leaves a **default byte cap** binding first, which truncates history to
~840 rows irrespective of the line limit — the same number the inert-option
commit produced, from a different cause. So **both limits are set
deliberately and neither is left at its default**: the line limit to spyc's row
budget, and the byte limit to an explicit, documented ceiling. Leaving the byte
limit alone is what produced the truncation; removing it entirely trades a
documented cap for an unbounded one, which is the wrong direction for a
long-lived pane set. With the line limit binding, a configured 10,000 retains
9,883 rows (98.8%).

Page-granularity pruning makes the limit an estimate, so the retention criterion
is **"≥ budget minus one page"** — never an exact count. PR 13 runs `sbprobe`
with the **shipped** configuration, not a probe-only one, so the number in the
addendum is the number users get.

**The threading resolution is open until PR 15 prices it.** The bindings'
`Terminal` is `!Send` — a conservative binding choice, not a C-library
constraint — and spyc parses on a dedicated worker thread behind a mutex.
Option one: confine the terminal to its worker and pass snapshots or dirty
regions out; ghostty's `begin_update`/`end` and `Dirty::{Clean,Partial,Full}`
map onto `needs_draw`'s reason codes, so this may be a restructure with a
payoff rather than a workaround. Option two: `unsafe impl Send` in
`spyc-vt-sys`, justified line-by-line against the C API's documented threading
contract at the pin, behind a `SPYC-TRAP` anchor because its failure mode is
silent UB. Both priced before either is coded; option one preferred if costs are
comparable; the choice recorded in the decisions log, not left to be inferred
from the diff.

**The MSRV does not move, unless the FFI proves otherwise.** The spike recorded
"1.88 → 1.90"; re-checked, that number is the published `libghostty-vt` /
`libghostty-vt-sys` crates' own `rust-version` declaration and nothing about
ghostty's C API or the FFI requires it. spyc does not depend on those crates —
they are ABI-incompatible with the shipping constructor, which is why PR 12
writes bindings against the pin's headers instead — so `spyc-vt-sys` declares
spyc's own 1.88 and the CI MSRV job is what proves it. A bump becomes a
recorded decision only if a needed feature demands one.

**vt100 stays compiled for 2.2, but is not selectable.** The flip makes ghostty
the engine outright; `[pane] engine` was never built and now never will be. The
fallback for the soak window is a one-line revert of the `PaneEngine` alias, and
what keeps that honest is the seam's contract suite, which runs its five tests
against **both** impls on every push. Removing vt100 — and with it the
`panic = "unwind"` profile setting's original rationale, the trait's second impl,
and a dependency — is decided (see ROADMAP's decisions log) and executes as the
first commit after 2.2 tags; [#453](https://github.com/Tripstack-Corp/spyc/issues/453)
stays open until that PR merges.

### What does not happen

No vt100 patches and no geometry floor, despite four live panic classes
reachable at spyc's `.max(1)` clamp. The swap lands this release; patching a
parser we are leaving makes spyc its fork, and a floor is a second mechanism to
maintain for one release. The `catch_unwind` net absorbs them in the meantime,
which is what PR 11's comment is corrected to say.

The spike crate stays a spike. PR 13 needs the harness's ghostty adapter updated
to the pin's constructor and that is in scope for `spikes/`, but nothing under
`spikes/` becomes a production dependency.

---

## Non-goals for 2.2

- **No Projects implementation code** — the design doc only.
- **No CounterTop revival.** `docs/archive/V1_60_PLAN.md` stays archived
  design-history.
- **No frame mirroring, no input forwarding, no headless or `--detached`
  peers.** These are the parts that fight the single-process core, and one
  process makes them unnecessary.
- **No cross-process discovery files.** Nothing that has one spyc enumerate
  another.
- **No general path handoff.**
  [#59](https://github.com/Tripstack-Corp/spyc/issues/59) stays out — terse
  tokens, the `UserPromptSubmit` hook, bracketed-paste expansion, the
  consumer-aware `^a s` / `^a S` split. Option A
  ([#9](https://github.com/Tripstack-Corp/spyc/issues/9)) is in as a bug fix,
  and §5's templates use its anchor.

## Staging

The order the series was planned in, kept as the record; the scope table's
Status column and each section's "Shipped" line say what landed and where. One
PR per numbered item unless the tree argues otherwise. Three hard
dependencies; the rest is scheduling.

| PR | Item | Depends on | Why here |
|---|---|---|---|
| 1 | #326 — dropped first keystrokes | — | Highest-frequency bug on the dog-fooding path, and independent of everything else. Failing test first. |
| 2 | #327 — stranded worktree | — | Also independent, and it breaks the workflow every agent is told to use. |
| 3 | #40 — abstract the column references | — | Early, because it touches shared surfaces. Later PRs that read a column rebase onto it. |
| 4 | Pane-identity transport | — | Before anything that wants attribution. Ships the handshake field plus per-connection binding; per-pane roots and `get_spyc_context`-answers-for-the-caller can follow in the same PR or the next. |
| 5 | #22 + #11 — takeover prompt + coexistence test | 4 (shared MCP surface) | The test lands red first. After 4 so it is written once, against the attributed server. |
| 6 | #58 — startup pane tabs | 3 | Config schema plus startup wiring. The pointer re-resolution happens inside this PR. |
| 7 | #8 — session forking | 3 | Pane-tier binding, tab duplication, per-agent resume. |
| 8 | #9 — anchor `^a s` on the pane's live cwd | — | Small and otherwise independent, but must precede #71 so templates inherit a settled anchor. |
| 9 | #71 — prompt templates | 8 | New DSL verb. Uses PR 8's anchor policy. |
| 10 | [#452](https://github.com/Tripstack-Corp/spyc/issues/452) — `cell_style` drops SGR 2 (#34 adapter half) | — | One-line fix behind a failing test written against the widget's **output** (SGR 2 in, `Modifier::DIM` in the buffer), not against vt100 internals — so the same test passes unchanged against the ghostty impl in 15. The test surviving the swap is what makes a one-line fix worth its own PR. |
| 11 | Engine — profile comment tells the truth | — | The `panic = "unwind"` rationale names a bug fixed in 0.16.2. Comment-only; no patches to a parser we are leaving, and no geometry floor. |
| 12 | Engine — `spyc-vt-sys` | 11 | New crate owning the pin, the FFI, the vendored archives + checksums, ghostty's MIT attribution. |
| 13 | Engine — harness re-run at the pin (**the gate**) | 12 | Re-measure everything at the shipping pin and append a dated addendum to the spike report. Not green ⇒ the series stops here and the docs say so. |
| 14 | Engine — extract the `Engine` trait, vt100 behind it | 11 | Behaviour-identical strangler-fig work that touches no input timing, so it is **not** gated on #326. May land any time after 11, in parallel with 12-13 if the tree makes that convenient. `insta` snapshots unchanged. |
| 15 | Engine — ghostty impl, threading, flip ([#34](https://github.com/Tripstack-Corp/spyc/issues/34) engine half) | 13, 14, **#326 merged** | The half that does restructure the pane input path, so it waits for #326 (dispatched as its own engagement) and re-verifies its test. Carries one exit criterion from 16, which landed ahead of it: **tighten `pane_engine` to never-panics-never-poisons and drop the vt100 allowance** (done). `[pane] engine` is NOT part of it — deferred to [#453](https://github.com/Tripstack-Corp/spyc/issues/453), which was already the triage on whether vt100 stays selectable; see the decisions log. The target asserts recovery rather than absence of panics only because the engine under it still panics; once the flip lands, the weaker property is a lie about the shipped engine. |
| 16 | Engine — fuzz target graduates | 14 | `fuzz_diff`'s generator into `fuzz/fuzz_targets/` on the weekly CI fuzz job. Landed early ([#462](https://github.com/Tripstack-Corp/spyc/pull/462)), against vt100 rather than the shipped engine — which is why its property is recovery, and why 15 owes the tightening. |
| — | `PROJECTS_PLAN.md` | 4 (informs §2) | Written across the release, reviewed at the end. Not a PR in the sequence; a deliverable gating 2.3. |

Bugs are interleaved on purpose. 1 and 2 open the release so a daily driver
sees the difference early. 10 no longer sits last "because its scope may shrink
on contact" — the spike removed that uncertainty, and what is left of #34 is a
small display fix (10) plus an engine swap (14).

The engine stages are ordered, with one exception the tree argued for (16
landed before 15 — see its row), and 13 is a gate, not a formality: the
figures the adoption rests on were measured at a ghostty commit that cannot
ship, so 13 either reproduces them at the shipping pin or the series ends at
11 with the decisions log amended to say adoption did not proceed. The engine work adds one cross-item ordering constraint, and only to
half of what was originally one stage: extracting the trait with vt100 behind it
(14) is behaviour-identical and touches no input timing, so it is unblocked,
while the ghostty impl, the threading restructure and the flip (15) wait for
#326 to merge. #326 is dispatched as its own engagement with its own failing
test, per this table's PR 1 — it is not pulled into the engine work.

## Exit criteria

Each criterion as it shipped, with the test that holds it. Where what shipped
differs from what was planned, the criterion says what shipped and why.

**#326 — met in code, not on tape.** Text typed into a pane the instant `^a c`
returns reaches the child. The cause was the status-hook consent prompt
swallowing those keys, not a pty race (#464).
`typing_into_a_fresh_agent_pane_is_not_eaten_by_the_consent_prompt`
(`src/app/harness_tests/pane.rs`) pins it. The planned second half is not
done: the VHS tape still depends on the compensating repaint in
`docs/assets/demo/fake-claude.sh`. Removing it waits on a re-record of the tape,
on the demo-harness branch (#473).

**#327 — met.** A `remove_worktree` interrupted mid-delete leaves git's view
consistent, and re-running it finishes the job (#500).
`a_failing_delete_strands_bytes_not_a_worktree` and
`a_live_writer_never_leaves_half_a_worktree` (`src/git/worktree.rs`) cover the
interrupted delete, the second with a real writer racing it.
`a_partially_removed_worktree_is_finished_not_refused`
(`src/app/worktree_clean.rs`) covers the retry.

**#40 — met.** A column is reached through a handle everywhere it isn't
legitimately naming a specific one (#504). The widened guard,
`columns_are_addressed_through_handles`, allowlists only render and fs-watch,
each with a why, and an entry that stops matching fails too.

**Pane identity — met.** An agent in worktree X gets X from `get_spyc_context`
while the user browses Y (#507). An older proxy that omits the id still works:
`an_initialize_without_a_pane_id_is_served_as_before`
(`src/mcp/tests/attribution.rs`). SECURITY.md's attribution-is-not-
authorization paragraph holds, and the narrowing it would have argued against
was dropped (§1).

**#22 + #11 — met, as redefined.** Two spyc instances in one directory each
keep their own agents, and there is nothing to take over, so nothing asks
(#509). The planned prompt and its `TookOver` / `Skipped` branches were the
defect, not the fix (§6). `two_spycs_in_one_directory_each_keep_their_own_agents`
(`src/mcp/tests/coexistence.rs`) is the integration test #11 asked for.

**#58 — met.** `[pane] tabs` (or `[[pane.tab]]`) opens the declared tabs on a
fresh launch, and `spyc -r` restores the saved set instead (#482):
`resume_neither_asks_nor_seeds` (`src/app/startup_tabs.rs`), plus the parse
tests in `src/config/mod.rs`. A project-local list runs only after a consent
bound to the exact list (#496). `--print-config` and CONFIGURATION.md document
the keys.

**#8 — met.** `^a F` on a claude or codex tab opens a branch of its
conversation in a new tab, a new session that starts from the old one's history
and is readable through `^a v` (#516). agy and zot can't branch and say so,
rather than opening one conversation twice. `docs/HARNESS.md` §4 documents each
agent. The key is `F`, not the planned `f` (§4).

**#9 — met.** `^a s` from a worktree pane sends a path the agent can `cat`
from its own cwd (#501): `send_selection_anchors_on_the_panes_cwd_not_project_home`
(`src/app/harness_tests/send_selection.rs`), and
`an_unknown_cwd_sends_every_path_absolute` (`src/shell/expand.rs`) for the
absolute fallback, which is not `~`-collapsed.

**#71 — met.** A template from `~/.spycrc.toml` types a composed prompt with
picks substituted, and a project-local file can neither bind one nor define one
(#517). The verb is `is_executing` (`prompt_verb_parses_and_is_executing`), a
project's `[prompts]` is dropped (`prompt_templates_come_only_from_home`), a
non-UTF-8 path is refused (`a_non_utf8_path_is_refused_where_it_is_named`), and
CONFIGURATION.md documents the tokens.

**#34 — met.** Both halves are pinned by tests. **Adapter (#457):** SGR 2 reaches the ratatui buffer as `Modifier::DIM`,
asserted against the buffer's cell styles (`sgr_2_reaches_the_buffer_as_dim`,
`src/pane/widget.rs`), because a glyph snapshot can't see a modifier.
**Engine (#465):** the four defects the spike names are gone at the shipped
engine: `scs_box_drawing_draws_boxes`, `a_row_written_before_decstbm_survives`,
`a_top_anchored_scroll_region_accumulates_scrollback` and
`a_tag_sequence_grapheme_survives_past_eighteen_bytes`
(`src/pane/engine_ghostty/tests.rs`). The issue's own suggestion (pin the CLI to
the bottom) was never a criterion; see §6.

**`PROJECTS_PLAN.md` — authored; approval open.** All seven questions in §7
answered, each with a decision and a reason (#520). The owner's review is in
progress, and approval gates 2.3, not the 2.2 tag.

## Docs each PR must carry (same commit, not a follow-up)

Per AGENTS.md's doc-sync rule: `FEATURES.md`, `docs/KEYBINDINGS.md` and
`src/ui/help.rs` for a new binding (#8) or a changed one (#9 changes what `^a s`
does, and all three describe it); `CONFIGURATION.md` and `--print-config` for a
new config key (#58, #71); `docs/HARNESS.md` for per-agent behaviour (#8, #34);
`SECURITY.md` for the MCP surface (item 1 — extend, don't contradict);
`AGENTS.md`'s module index for a new module; and `ROADMAP.md`'s decisions log
when a decision here supersedes one recorded there.

The engine stages (§8) add a few of their own. `AGENTS.md` needs the new
`spyc-vt-sys` crate in its "Other crates" index (PR 12) and the `Engine` trait
seam described where the `src/pane/` entry today says bytes are fed into a
`vt100::Parser` (PR 14). `ARCHITECTURE.md` takes the threading resolution,
because choosing between the actor shape and an `unsafe impl Send` is an
architectural decision and the losing option needs to stay refuted (PR 15) —
plus a `SPYC-TRAP` rationale section if the unsafe option wins, since the anchor
and its section share a slug. `CONFIGURATION.md` and `--print-config` take nothing:
`[pane] engine` was dropped rather than built (decisions log). `deny.toml` documents the vendored archives and
`SECURITY.md` the pin and its checksum verification (PR 12). The MSRV did not
move (§8), so `INSTALL.md` took nothing. `docs/drafts/VT_ENGINE_SPIKE.md` takes the dated addendum
(PR 13) — appended, never rewritten. `CHANGELOG.md` is generated, so what
matters is that each engine PR's **title** is typed for the section it belongs
in: `fix(pane)` for the adapter half, `feat(pane)` for the flip, `chore` for the
comment correction.

# spyc 2.2 — projects-prep and the daily-driver loop

**Status:** accepted scope, sequencing draft. The seven items below are
decided; the ordering, the per-item technical detail and the exit criteria are
this document's proposal.
**Measured against:** `9df4d7a` (`main`, `2.2.0-CURRENT`).
**Predecessor:** [`docs/archive/LAUNCH_PLAN_2_0.md`](../archive/LAUNCH_PLAN_2_0.md)
(the 2.0 distribution pass). Strategy context: `ROADMAP.md` → "Road to 2.2".

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

| # | Item | Kind | Tracking |
|---|---|---|---|
| 1 | Pane-identity transport (option B) | prep | [proposal](pane-identity-transport-proposal.md) |
| 2 | One spyc per agent — abstract the column references | prep | [#40](https://github.com/Tripstack-Corp/spyc/issues/40) |
| 3 | Configurable startup pane tabs | prep + feature | [#58](https://github.com/Tripstack-Corp/spyc/issues/58), [plan](PANE_STARTUP_TABS_PLAN.md) |
| 4 | Session forking (`^a f`) | feature | [#8](https://github.com/Tripstack-Corp/spyc/issues/8) |
| 5 | Prompt templates in `.spycrc.toml` | feature | [#71](https://github.com/Tripstack-Corp/spyc/issues/71) |
| 6 | The daily-driver bug set | fix | [#326](https://github.com/Tripstack-Corp/spyc/issues/326), [#327](https://github.com/Tripstack-Corp/spyc/issues/327), [#9](https://github.com/Tripstack-Corp/spyc/issues/9), [#34](https://github.com/Tripstack-Corp/spyc/issues/34), [#452](https://github.com/Tripstack-Corp/spyc/issues/452), [#22](https://github.com/Tripstack-Corp/spyc/issues/22), [#11](https://github.com/Tripstack-Corp/spyc/issues/11) |
| 7 | Author `docs/drafts/PROJECTS_PLAN.md` | design | this doc, §7 |
| 8 | The VT engine — libghostty-vt replaces vt100 | prep + fix | [spike](VT_ENGINE_SPIKE.md), [#34](https://github.com/Tripstack-Corp/spyc/issues/34), [#452](https://github.com/Tripstack-Corp/spyc/issues/452), [#453](https://github.com/Tripstack-Corp/spyc/issues/453) |

---

## 1. Pane-identity transport

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

- **The F1 target design.** The decisions-log entry names per-pane root
  validation as where the MCP `root` override should end up, blocked on this
  transport. With attribution, the session-wide allowed set stays as the
  fallback and per-pane roots narrow it.
- **`get_spyc_context` answering for the caller.** Today it reports the focused
  column, so an agent working in worktree X is told about worktree Y whenever
  the user browses elsewhere.
- **Scope-registry ownership.** `register_scope` claims are owner-labelled by
  convention. Attribution lets a claim bind to a pane, and `release_scope`
  refuse one the caller doesn't own.

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

Per [`PANE_STARTUP_TABS_PLAN.md`](PANE_STARTUP_TABS_PLAN.md): a `.spycrc.toml`
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

## 4. #8 — session forking (`^a f`)

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

`^a f` is a **pane-tier** binding, so it belongs on the `^a` prefix and its
`Action::tier()` must be `Pane`. The guard
`leader_and_pane_namespaces_respect_tiers` fails the build otherwise.

## 5. #71 — prompt templates in `.spycrc.toml`

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

Six issues. Each paragraph says where the behaviour lives and what is already
known. None of them is a fix design.

### #326 — the first keystrokes into a fresh pane are dropped

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

This was the least certain scope of the six bugs and the spike settled it: it
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
is amended to say so. That is a real branch, not a formality.

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

The addendum must also say, in so many words, that **~840 rows is a number two
unrelated causes produce**: the inert `max_scrollback` at `f4c68d65`, and a
default byte cap binding ahead of the line limit at the shipping pin. Both were
measured during this work. Recorded because a future harness run that recognises
840 as "the expected ghostty number" and stops looking is exactly the trap this
series already fell into once.

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

**vt100 stays selectable for 2.2.** The flip makes ghostty the default and keeps
`[pane] engine` as the fallback while it soaks. Removing vt100 — and with it the
`panic = "unwind"` profile setting's original rationale, the trait's second impl,
and a dependency — is filed as [#453](https://github.com/Tripstack-Corp/spyc/issues/453) for 2.3 triage rather than decided
here.

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

One PR per numbered item unless the tree argues otherwise. Three hard
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
| 15 | Engine — ghostty impl, threading, flip ([#34](https://github.com/Tripstack-Corp/spyc/issues/34) engine half) | 13, 14, **#326 merged** | The half that does restructure the pane input path, so it waits for #326 (dispatched as its own engagement) and re-verifies its test. |
| 16 | Engine — fuzz target graduates | 15 | `fuzz_diff`'s generator into `fuzz/fuzz_targets/` against the shipped engine, on the weekly CI fuzz job. |
| — | `PROJECTS_PLAN.md` | 4 (informs §2) | Written across the release, reviewed at the end. Not a PR in the sequence; a deliverable gating 2.3. |

Bugs are interleaved on purpose. 1 and 2 open the release so a daily driver
sees the difference early. 10 no longer sits last "because its scope may shrink
on contact" — the spike removed that uncertainty, and what is left of #34 is a
small display fix (10) plus an engine swap (14).

The engine stages are strictly ordered and 13 is a gate, not a formality: the
figures the adoption rests on were measured at a ghostty commit that cannot
ship, so 13 either reproduces them at the shipping pin or the series ends at
11 with the decisions log amended to say adoption did not proceed. The engine work adds one cross-item ordering constraint, and only to
half of what was originally one stage: extracting the trait with vt100 behind it
(14) is behaviour-identical and touches no input timing, so it is unblocked,
while the ghostty impl, the threading restructure and the flip (15) wait for
#326 to merge. #326 is dispatched as its own engagement with its own failing
test, per this table's PR 1 — it is not pulled into the engine work.

## Exit criteria

**#326 —** Type into a pane the instant `^a c` returns and every character
reaches the child. Pinned by a test that fails on the current tree: spawn a
pane, write bytes, compare what the child received byte-for-byte. Exit: the VHS
tape records the question it was meant to depict, with no compensating repaint
in `fake-claude.sh`.

**#327 —** A `remove_worktree` interrupted mid-delete leaves git's view
consistent, and re-running it finishes the job. Exit: a test that fails the
first removal (a directory that regains an entry during the walk) and asserts
the second succeeds, deletes the branch, and leaves no `.git/worktrees/` admin
dir behind.

**#40 —** Addressing a column goes through a handle everywhere it isn't
legitimately naming a specific one. Exit: the widened
`state_left_listing_dir_uses_are_allowlisted` guard passes with an allowlist
naming only render and fs-watch, each with a why.

**Pane identity —** An agent in worktree X gets X from `get_spyc_context` while
the user browses Y. Exit: an older proxy that omits the field still works,
proved by a test that drives the server with an `initialize` carrying no pane
id; and SECURITY.md's attribution-is-not-authorization paragraph still holds.

**#22 + #11 —** Two spyc instances in one directory coexist, and the second to
want MCP asks before taking it. Exit: an integration test that stands up two
instances, exercises the live-socket `TookOver` and `Skipped` branches
end-to-end, and fails against today's tree.

**#58 —** `[pane] startup_tabs` in `.spycrc.toml` opens the declared tabs on a
fresh launch, and `spyc -r` still restores the saved set instead. Exit: both
paths covered, `--print-config` emits the new keys with comments, and
CONFIGURATION.md documents them in the same commit.

**#8 —** `^a f` on a live agent tab produces a second tab on that conversation,
with its history readable. Exit: `docs/HARNESS.md` documents the per-agent
behaviour, including which agents give a shared session rather than a branch.

**#9 —** `^a s` from a worktree pane produces a path the agent can `cat` from
its own cwd without a `cd`. Exit: a test where the pane's cwd differs from
`PROJECT_HOME` asserts the relative form resolves under the pane, plus one
where `live_cwd` is unknown asserts the absolute-tier fallback. The absolute
tier is not `~`-collapsed.

**#71 —** A template bound in `~/.spycrc.toml` sends a composed prompt with
picks substituted, and a project-local `.spycrc.toml` cannot bind one. Exit:
the DSL verb is `is_executing`, CONFIGURATION.md documents the token set, and
the substitution refuses a non-UTF-8 path the way `expand_percent` does.

**#34 —** Closed across both halves, each pinned by a test that fails on the
tree before it. **Adapter (PR 10):** a cell carrying SGR 2 reaches the ratatui
buffer with `Modifier::DIM` on a focused pane. Asserted against the buffer's
cell styles rather than a glyph snapshot — a snapshot compares symbols and
cannot see a modifier at all, which is the same blind spot #493–#504 recorded
for styling. **Engine (PR 15):** the four engine
defects the spike names are gone at the default engine — SCS box drawing draws
boxes, a row written before a DECSTBM region survives, a scroll-region child
accumulates scrollback, and a tag-sequence grapheme survives past 18 bytes —
each as a differential case in the spike harness that vt100 fails and the
shipped engine passes. The issue's own suggestion (pin the CLI to the bottom)
is explicitly **not** an exit criterion; see §"#34" for why it is dropped.

**`PROJECTS_PLAN.md` —** All seven questions in §7 answered, with a decision and
a reason for each, and the doc approved before 2.3 opens.

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
and its section share a slug. `CONFIGURATION.md` and `--print-config` take
`[pane] engine` (PR 15). `deny.toml` documents the vendored archives and
`SECURITY.md` the pin and its checksum verification (PR 12). `INSTALL.md` gets
the MSRV bump (PR 12). `docs/drafts/VT_ENGINE_SPIKE.md` takes the dated addendum
(PR 13) — appended, never rewritten. `CHANGELOG.md` is generated, so what
matters is that each engine PR's **title** is typed for the section it belongs
in: `fix(pane)` for the adapter half, `feat(pane)` for the flip, `chore` for the
comment correction.

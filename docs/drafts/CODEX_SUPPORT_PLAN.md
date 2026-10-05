# Codex Support and Session HUD Plan

**Status:** Track A implementation in progress, delivered through local-build
acceptance before integration. The new HUD is proposed follow-up scope; its
design review and telemetry spike remain open. This document does not change
the accepted release gates.

**Evidence date:** 2026-10-05. Assessment of the release-candidate code and current
`main`, against the locally installed Codex CLI 0.160.0. These are historical
test conditions, not a minimum-version promise.

## Outcome

Make Codex a first-class hosted agent: its conversation, restore/fork, attention
signals, images and file navigation should be as dependable as Claude Code's.
Then add a spyc-owned session HUD beneath the active Codex pane, inspired by
claude-hud, without requiring tmux, replacing Codex's composer, or scraping its
screen for token counts.

Two tracks keep the RC manageable:

1. **Compatibility and parity:** fix reproduced regressions first; finish the
   remaining parity gaps with real-CLI evidence and regression fixtures.
2. **Session HUD:** an opt-in feature built on the corrected session reader.
   Ship useful, trustworthy local metrics first; unavailable telemetry must
   remain unavailable rather than looking like zero.

## Assessment record

### Implementation progress

- The plan and worktree-first house rule landed through PR #539.
- A1 landed through PR #540 after local-build acceptance:
  shared current/legacy
  normalization, transcript integration and identified-record deduplication.
  Four new public-renderer regressions failed against the unchanged reader.
  The focused Codex suite passed (72 tests), followed by `make check`. Deliberate
  mutations confirmed identity, reasoning exclusion and fork-history coverage;
  the correct implementation was restored. A temporary read-only smoke rendered
  the active CLI rollout and verified recent user/agent text, then was removed.
  The user confirmed working conversation scrollback in the isolated release
  build and approved it for integration. Fork history has automated coverage;
  other slices and the HUD are not implemented by this change.
- A2 landed through PR #541 after local-build acceptance.
  Save, restore and fork share token-aware option preservation; resume pinning
  distinguishes selectors from option values. Ambiguous launches are refused,
  and restore preflights before replacing any live tabs. Profile and real-PTY
  argv regressions demonstrated the original loss before the fix. Automated
  tests include shell quoting/expansion, selector safety and refusal paths;
  the installed CLI's help verifies the recognized option vocabulary. The full
  `make check` gate passed, including 2717 library tests. The user confirmed
  restoration of forked sessions in the isolated release build; their activity
  dump shows distinct explicit session ids pinned to both restored panes.
  Non-default settings have automated argv coverage, not a separate live
  settings confirmation.
- A3a landed through PR #542 after local-build acceptance:
  Codex/Claude semantic reports survive output, agy's approval
  scrape retains its existing precedence, and per-pane diagnostics separate
  installation/startup facts from received reports. Codex's `Interrupt` hook
  reports cancellation as `idle`. Hook trust is never inferred or changed.
  Working-through-silence and readiness regressions failed before the fix.
  Six focused harness tests pass, and `make check` passed with 2723 library
  tests plus integration/VT tests. Deliberate mutations verified report-history
  and pre-launch capture coverage; a negation-sensitive assertion was tightened,
  and the correct production code restored.
  During local testing, the user reviewed the new `Interrupt` hook in `/hooks`;
  Codex displayed it as trusted. Their before/after activity dumps show a
  semantic `done` report before cancellation and an authoritative `idle` report
  afterward. The latter survived newer pane output, and the other tabs retained
  their states. This verifies the observed cancellation outcome, not automatic
  hook provenance: diagnostics deliberately cannot distinguish hook reports
  from agent MCP reports. Subsequent dumps demonstrate normal
  `done` → `working` → `done` transitions, with newer output leaving the reports
  authoritative. The user accepted the build for integration. Long quiet tool
  waits have automated coverage, not a separate live confirmation.
  Question/plan tool coverage and live onboarding/lifecycle evidence remain
  A3 follow-up work; this slice does not claim the whole A3 gate is met.
- A3b1 adds bounded, pane-local reported hook-event diagnostics and replaces
  raw status-trace payload logging with sanitized metadata on
  `fix/codex-attention-lifecycle`, accepted for integration after local testing
  on 2026-10-05. The full
  `RUST_TEST_THREADS=1 make check` gate passes: 2729 library tests, one existing
  ignored test, integration tests (including three real reporter-binary tests)
  and VT-system tests. Default parallel runs hit fake-agent PTY startup flakes
  also reproduced on unchanged `main`; the cause is not established. New
  metadata, dispatch and pane-history tests were observed failing against
  scaffolds or mutations, and real reporter tests exposed missing metadata and
  raw-payload trace leakage before the fixes.
  The installed CLI remains `0.160.0`. The current official hook contract lists
  call ids for `PreToolUse`/`PostToolUse` but not `PermissionRequest`, and does
  not explicitly establish question-tool coverage. This is instrumentation for
  those gaps, not an inferred approval-answer transition or complete A3 fix.
  Existing hook definitions and trust state are unchanged. Older reporters
  remain compatible but cannot supply metadata; local tests must resolve the
  supplied build on `PATH` for the one-shot reporter as well as the host.
  The first live run received statuses without metadata. A shell-resolution
  probe reproduced interactive zsh selecting the older installed reporter
  despite the test directory's initial `PATH` precedence. A fresh session
  launched with `SHELL=/bin/sh` then received bounded event summaries:
  `PermissionRequest` reported authoritative `blocked`, a later
  `UserPromptSubmit` reported authoritative `working`, and `Stop` reported
  authoritative `done` despite newer output. Each event arrived twice; duplicate
  delivery remains unexplained and must not be hidden by heuristic deduplication.
  The intermediate message/approval sequence was not captured precisely enough
  to establish an approval-only recovery transition. Question-tool handling,
  quiet post-approval work and multi-pane live isolation remain unverified.

### Baseline

The shared foundation already works: MCP context/search/git tools are attributed
to the Codex pane, skills are installed for Codex, daemon bypass preserves
per-pane routing, and restore/fork and transcript-history infrastructure exist.
This is not a proposal to replace the agent-profile architecture.

The existing focused suites passed: `cargo test --lib codex` (64 tests) and
`cargo test --lib mcp::hooks::tests` (21 tests, with overlap). Two temporary
regression tests then failed as expected: current message records produced no
conversation text, and reconstruction discarded valid post-subcommand options.
The temporary tests were removed after assessment. Existing green tests alone
therefore do not establish current CLI compatibility.

| Priority | Finding | Evidence | Main entry points |
|---|---|---|---|
| P1 | Current conversation records are not decoded | Observed `event_msg/item_completed` user/agent messages; synthetic current-format regression failed | `src/state/codex_transcript.rs` |
| P1 | Restore/fork can discard model, profile and sandbox options | Regression failed for options following `resume`; installed CLI accepts them there | `src/agent/resume.rs`, `src/agent/mod.rs` |
| P2 | Hooks written after launch may not be active or trusted | Code inspection and current documented trust rules; fresh onboarding needs a live test | `src/mcp/hooks.rs`, `src/app/pane_tabs.rs`, `src/app/status_hooks.rs` |
| P2 | Question/plan attention is weaker than Claude's | Codex hooks cover prompt submit, permission request and stop; question-tool behaviour needs a live test | `src/mcp/hooks.rs`, `src/app/agent_status.rs` |
| P2 | Transcript discovery ignores custom `CODEX_HOME` | Both rollout discovery paths use `~/.codex/sessions`; skill installation already honours the override | `src/state/codex_transcript.rs`, `src/skill/mod.rs` |
| P2 | spyc's image gallery/paste capture lacks Codex support | Codex has no image-reader or paste-key profile overrides | `src/agent/mod.rs`, `src/app/image_gallery.rs`, `src/app/paste_capture.rs` |
| P2 | Codex composer/footer is included in output navigation | Codex uses the default all-lines output boundary; current chrome fixtures needed | `src/agent/chrome.rs`, `src/agent/mod.rs` |

P1 denotes a reproduced user-visible compatibility failure. P2 denotes a parity
gap or onboarding risk, not necessarily a release blocker. The maintainer should
choose the RC gate explicitly; this plan recommends gating on the two P1 fixes
and live restore/fork, transcript and hook smoke tests.

## Track A — compatibility and parity

### A1. Normalize current and legacy session records

- Decode current `UserMessage` and `AgentMessage` items, including their different
  content-tag casing, alongside legacy `user_message` / `agent_message` events.
- Support current command/MCP items and custom-tool calls/outputs as well as
  legacy function calls. Unknown records must be skipped safely.
- Normalize once for transcript, image and HUD consumers. Deduplicate alternate
  representations of the same item/call; do not print a tool twice because the
  rollout contains both event and wire records.
- Preserve existing budgets, ordering, fork ancestry and
  `history_base.end_byte_offset` boundaries in `src/state/codex_history.rs`.
  Completed historical tools must not become active tools in the fork's HUD.

**Acceptance:** redacted fixtures from the installed CLI and legacy fixtures
show prompts, replies and optional tools exactly once through `^a v`, including
resume and fork history. Reintroduce the failed regression as a permanent test.
Test partial/malformed records, missing fields and unknown item kinds.

### A2. Preserve launch intent through restore and fork

Replace the whitespace-and-truncate handling of `resume` / `fork` with a
token-aware transformation. Preserve executable, quoting and general options on
either side of the subcommand. Remove only the old conversation selector and
selector-specific switches; do not silently change model, profile, sandbox,
approval policy, working directory or `-c` settings. Resolve ambiguous or
unsupported command shapes explicitly rather than inventing a new launch.

**Acceptance:** table-driven tests cover ordinary launch, `resume`, `fork`,
`--last`, options before/after the subcommand, quoted values and UUID selectors.
Real CLI launches confirm the requested settings after session restore and
`^a F`. Retain default per-pane daemon isolation.

### A3. Make hook readiness and attention truthful

During A2 the user observed a stopped-looking Codex tab while the agent was
still working. Capture `:why-status` / `:activity dump` in the next live
reproduction to establish the actual source. Audit working-report supersession
by pane output and fallback-to-idle during quiet tool waits; reporting a longer
TTL alone cannot fix a report that output supersedes. Include this in A3's
acceptance rather than inferring work completion from silence.

A2 local testing also showed a transient Codex startup cycle that initially
did not accept input; the user reported that the cycle subsequently cleared.
The supplied startup-warning screen names an ignored `approval_policy` field
under the project's hook state in `~/.codex/config.toml`. Read-only inspection
confirmed the setting is misplaced inside a hook-trust table rather than at
the root or in a profile; the adjacent `trusted_hash` is a separate trust record.
This is not proof that the warning caused the startup cycle.
Check input recovery, startup/MCP readiness and hook-state compatibility
separately. Do not remove trust records or edit the user's configuration
without approval. A2's forked-session restore was subsequently accepted. The
restored-session dump shows two pane-bound MCP connections, no tool calls and
output-timing fallback after 27 seconds of silence. Installed hooks and a bound
connection alone do not establish hook execution or explain the earlier false
idle observation during work.

Treat **file installed**, **restart needed**, and **status actually reported**
as different facts. The existing installed-file check is not evidence that
Codex accepted or executed a hook. Explain next-launch requirements when a
non-live-reloading host receives hooks after it has started.

Current Codex requires project trust and trust of new/changed hook commands;
`/hooks` is the review surface. Never auto-approve a hook or bypass that trust.
Test a fresh project, already-trusted hooks, changed hook contents, worktrees,
and simultaneous spyc instances. See the [Codex hooks documentation](https://learn.chatgpt.com/docs/hooks).

Verify the current question/plan tool lifecycle and add narrowly scoped blocked
signals where supported. Do not mark every tool invocation blocked, and do not
infer that a permission request or question was approved merely because output
resumed. Keep semantic hooks/MCP ahead of timing fallback; expose the reason
and last report through the existing diagnostics.

**Acceptance:** prompt → working, permission/question → blocked, answered prompt
→ working, and turn completion → done are demonstrated with the real CLI.
Fresh onboarding reports actionable trust/restart instructions, not false
readiness. Multi-instance cleanup does not remove another instance's hooks.

### A4. Resolve the correct Codex home and session

Centralize session-root resolution and honour the effective `CODEX_HOME` of the
agent launch, including per-tab overrides. Share it across discovery, ancestry,
images and HUD collection. Pin to an explicit session id when available; retain
the existing spawn-ordered claim only as the controlled discovery fallback.
Never substitute a different session merely because its rollout is newer.

**Acceptance:** default/custom homes, two panes sharing a cwd, simultaneous
launches, resume/fork, and changed homes cannot cross-bind. Missing/unreadable
session data yields a diagnostic and fallback, not another pane's history.

### A5. Bring images into the existing gallery pipeline

Verify Codex's actual paste key on supported platforms before adding the profile
override. Add bounded decoding of current and legacy image inputs, including
local paths/data URLs where actually emitted and inherited fork history. Reuse
the existing unsent-image capture ring and image-display worker; capture must
accompany forwarding, never replace it.

**Acceptance:** a pasted image is visible before submit, then appears once as a
submitted attachment; resume/fork images remain accessible. Missing files,
oversized payloads and malformed data fail safely. This gap concerns spyc's
gallery, not Codex's ability to accept images.

### A6. Exclude Codex input chrome from output navigation

Add a verified Codex output-boundary implementation using captured current
screen fixtures. Cover composer, statusline, approval overlays and narrow panes;
do not guess a footer from one arbitrary glyph.

**Acceptance:** `gf`, `J` and output-based selection prefer paths actually
printed by the agent over paths in an unfinished prompt. Terminal scrollback
and transcript navigation retain their separate semantics.

## Track B — a native Codex session HUD

### References and positioning

The supplied screenshot is the visual brief: identity/branch/time, context and
usage meters, operating mode, then activity. [claude-hud](https://github.com/jarrodwatts/claude-hud)
also exposes tools, agents and task progress. Borrow the information hierarchy,
not its implementation or a promise that every Claude metric exists in Codex.

[ConsoleAgentHUD](https://github.com/Tomatio13/ConsoleAgentHUD) demonstrates a
rollout-tail dashboard for tmux/standalone terminals; its README explicitly calls
its Codex launch example the old format. [hud-mode](https://github.com/adrida/hud-mode/blob/main/README.md)
instead drives headless JSON engines and supplies its own prompt interface.
Neither establishes compatibility with the current hosted native TUI.

One correction to the supplied notes: Codex already has a configurable native
`/statusline`, persisted as `tui.status_line`. spyc should add cross-pane session
identity, attention and richer monitoring, not claim to introduce the first
Codex statusline. Leave the user's native statusline untouched. See the
[Codex command reference](https://learn.chatgpt.com/docs/developer-commands).

### Product proposal

- **Off by default for initial delivery.** Proposed home-config setting:
  `[pane] agent_hud = "off" | "compact" | "full"`. These values are a design,
  not existing config. Runtime control: proposed `:agent hud off|compact|full`;
  `:agent hud info` opens source/freshness diagnostics in the pager.
- **Active Codex tab only.** Keep the layout slot agent-neutral, but do not
  duplicate an installed Claude HUD in this first feature. Existing tab dots
  remain the overview of inactive sessions.
- **Compact: two rows; full: up to five.** A spyc-owned strip below the child
  screen, inside the pane region, shrinks the PTY through normal resize handling.
  No extra process, tmux pane, writable prompt bar or new default keybinding.
- **Graceful geometry.** Collapse optional fields/rows on narrow or short panes;
  preserve a usable child screen. Follow pane zoom, tabs and split changes.
  Include the strip in mouse/selection geometry so child coordinates stay right.
- **Same design language.** Theme and monochrome support, explicit text states
  as well as colour, measured display widths, and no dependency on the `A`
  engineering Activity HUD.

Illustrative hierarchy, not literal measured values:

```text
model · effort | repo (branch) | session-id | working · elapsed
Context: -- | tokens: -- | limits: not reported
Mode: approval / sandbox | hooks: installed, awaiting first report
Tools: active names and completed/failed counts
Plan: completed/total | agents: reported count
```

The last three rows belong to full mode. Omit plan/agent rows when unavailable;
do not equate parallel tools with agents. Compact mode retains attention and
identity ahead of optional counters.

### Data contract

| Field | Preferred source | Rules |
|---|---|---|
| Session identity / model / effort | Pinned `session_meta` and latest `turn_context` | Follow actual session changes, not config guesses |
| Project / branch / cwd | Existing pane cwd and in-process git cache | Do not add shell git polling; distinguish launch cwd from live cwd |
| Activity / attention | Existing semantic status pipeline | Preserve source and expiry; tool activity is supplementary, not a new dot authority |
| Approval / sandbox | Latest verified turn context | Present a summary, never an editable approval control |
| Token totals | Normalized usage records | Separate turn/thread totals; deduplicate response updates; cached input is part of input, reasoning output is not added twice |
| Context occupancy | Verified current-context metric plus window | Lifetime token totals are not context occupancy; no percentage until semantics are validated against native Codex |
| Limits / reset times | Reported rate-limit snapshot | Account/bucket-scoped, not session consumption; use reported window duration, never assume weekly |
| Tools / plan / agents | Verified lifecycle records | Bounded active/recent state; missing starts or unknown kinds are explicitly incomplete |
| Hooks / MCP | Existing observed connection/status data | Label installed/configured versus active/reported; no fabricated loaded instruction-file counts |

Local current rollouts contain `token_usage_record`, `token_count` and structured
items, but the observed rate-limit windows were null. Initial delivery must work
with `--` / "not reported". Zero is a real measurement, not the absence of one.
Track source, timestamp and completeness per metric; stale limits should look
stale. Before the first response, occupancy/limits may legitimately be unknown.
Reset occupancy correctly on compaction; retain labelled lifetime totals.

The [official app-server API](https://learn.chatgpt.com/docs/app-server) documents
token-usage notifications and account rate-limit reads/updates. A separate spike
must establish a supported, read-only way to observe the same live, isolated
CLI session before choosing that transport. Do not assume it can attach to the
existing `--no-daemon` pane. A fresh account snapshot must retain account/bucket
identity; multiple panes do not consume independent copies of that quota.

**Non-goals:** dollar-cost estimates from subscription tokens, scraping hidden
credential files or private usage endpoints, reading reasoning text for display,
new permissions, auto-approved hooks, replacing Codex with `exec`, and historical
usage heatmaps. No inferred quota or agent counts to fill empty space.

### Architecture and collection

1. Put pure record normalization/folding beneath `src/agent/` or the existing
   transcript domain, with no `App` dependency. Share it with Track A rather than
   creating a second divergent Codex JSON parser.
2. Keep pure metric snapshots keyed by stable pane id and pinned thread id in
   the Model. Runtime owns watcher handles, worker channels and read offsets;
   View owns layout and cached rendered lines.
3. Add a small `src/app/agent_hud.rs` orchestration module and a pure
   `src/ui/agent_hud.rs` renderer. Start/stop collection through effects; workers
   publish outcomes through the existing `Wake` vocabulary. Reject outcomes for
   a closed/rebound pane or an obsolete worker generation.
4. Use a bounded initial read, then incrementally tail complete JSONL records.
   Buffer an incomplete last line with a cap. Handle truncation, replacement,
   malformed/oversized lines and deleted files without blocking the loop.
   Watch the necessary rollout/parent, not the entire sessions tree repeatedly.
5. Coalesce bursts off-thread; repaint only on changed visible metrics. No
   perpetual polling or countdown timer while idle. Elapsed/reset text updates
   on events or existing active animation; finite expiry deadlines may mark
   stale data. Do not promise an idle one-second clock and zero idle wakes.
6. Clear active tool state on a new session. Ancestry enriches history, not the
   current run's tool activity. Cancel/rebind on tab lifecycle changes; disable
   collection when there is no consumer and avoid duplicate readers where a
   shared collector already serves transcript/gallery work.

Privacy and resource bounds are acceptance criteria: retain only metric fields,
bounded recent tool names/statuses and identity. No prompt/response arguments,
tool output or credentials in persisted HUD state or diagnostics. Reading must
remain read-only and fail independently of the child session.

## Delivery sequence and gates

| Slice | Deliverable | Gate |
|---|---|---|
| A1 | Current/legacy normalization and transcript regression fixes | Real captured fixtures; current `^a v` and fork history smoke |
| A2 | Safe restore/fork command reconstruction | Quoting/selector tests and real CLI launch settings |
| A3–A4 | Hook readiness/question lifecycle and home/session binding | Fresh trust onboarding; parallel-pane and custom-home isolation |
| A5–A6 | Image gallery and output-boundary parity | Platform paste smoke; current chrome and image fixtures |
| B1 | Metric/source spike and approved compact/full mockups | Verify context semantics; document unavailable limits and transport choice |
| B2 | Bounded event-driven collector and pure reducer | Replay/stream/isolation tests; no idle polling |
| B3 | Opt-in strip, config/commands and diagnostics | Layout/input snapshots, live multi-pane smoke and performance checks |

Each slice should become a focused issue/PR once scope is approved; no issue ids
are invented here. The HUD is not a prerequisite for the P1 corrections and
does not silently expand the existing RC scope.

### Validation checklist

- Fixtures: legacy/current messages, custom/function tools, duplicate records,
  token updates, missing/null limits, compaction, out-of-order/incomplete items,
  fork ancestry and unknown schema extensions. Store only redacted samples.
- Collector: partial writes, replacement/truncation, permission errors, bounded
  memory, worker cancellation, stale generations, two same-cwd tabs and two
  custom homes. One pane must never display another session's metrics.
- Render/input: compact/full/off, narrow/short terminals, zoom, preview and second
  Commander, Unicode/monochrome, mouse forwarding, scrollback and text selection.
  The strip must not consume child keystrokes or overlap its composer.
- Real CLI: startup with and without trust, permissions and questions, stop,
  resume, fork, image paste and native `/review`. The old historical review/MCP
  concern is a smoke-test target, not asserted to remain broken today.
- Performance: prove append work is proportional to new bytes, not session
  history; no filesystem scanning or repaints at idle. A corrupt rollout or
  failing optional telemetry source must not stall the TUI.
- Documentation: update `FEATURES.md`, `CONFIGURATION.md`, `docs/HARNESS.md` and
  `docs/AGENT_ORCHESTRATION.md` when behaviour ships, including supported CLI
  versions and honest fallback/unknown states. Run the relevant focused suites
  and the repository's normal pre-release checks after code changes.

### Review decisions still open

Approve the recommended P1 RC gate separately from the other parity work;
choose the release cycle for the new HUD; approve the two-row/five-row layout
and opt-in default. B1 must settle context semantics and whether fresh account
limits have a supported observation transport. Missing upstream telemetry must
not hold the useful local HUD hostage or encourage unsafe credential access.

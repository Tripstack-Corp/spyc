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
- A3b1 landed through PR #543 (`14b04924`) after local-build acceptance
  on 2026-10-05. It adds bounded, pane-local reported hook-event diagnostics
  and replaces raw status-trace payload logging with sanitized metadata. The full
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

- A3b2 landed through PR #546 (`e1b714a6`) after local-build acceptance
  on 2026-10-06, based on merged #543.
  Read-only inspection on 2026-10-06 found spyc reporters in both the project's
  `.codex/config.toml` and `.codex/hooks.json` for `UserPromptSubmit`,
  `PermissionRequest` and `Stop`. Codex independently loads both representations;
  this explains duplicate delivery for those events without suppressing reports
  heuristically. The installed CLI is now `0.160.1`; A3b1's `0.160.0` conditions
  above remain historical evidence.
  The test build migrates only owned reporters from the legacy JSON source,
  preserves other handlers even in shared matcher groups, and refuses tracked,
  malformed or unreadable legacy sources before adding canonical reporters.
  Cleanup covers legacy-only installations. JSON-only changes require restart;
  diagnostics name duplicate sources and actionable refusal reasons. No hook
  trust records are changed, and no new question or approval hooks are installed.
  Four migration regressions failed against the unchanged behaviour before the
  fix. Deliberate mutations verified JSON-only restart detection, precise TOML
  handler preservation, legacy cleanup and shared ownership. A separate
  legacy-only ownership regression reproduced a sibling-cleanup hazard when
  migration was refused; presence checks now include that reporter source.
  The full `RUST_TEST_THREADS=1 make check` gate passed: 2740 library tests,
  one existing ignored test, integration tests (including the real reporter
  tests) and VT-system tests. The user accepted the supplied local build for
  integration after the prompt/stop test below. GitHub lint, tests and packaging
  passed before the squash merge.
  The user's test instance (PID 62116) was verified running the supplied binary.
  Read-only config inspection found only the worktree guard remaining in the
  legacy JSON file, with spyc's reporters in TOML. The startup dump had a bound
  MCP connection and no semantic report. The 2026-10-06 10:19:58 UTC dump then
  showed exactly one `UserPromptSubmit` → `working` and one `Stop` → `done`,
  both for the same turn and pane, with two total status calls. The final report
  stayed authoritative after newer output. This demonstrates single delivery
  for that observed prompt/stop turn, not permission delivery or quiet recovery.
  The recovery audit found that the local Codex source at `f380b487` routes
  `request_user_input` through `PreToolUse` and successful `PostToolUse`, with the
  same call id; the handler waits for its response. Its failure/cancellation
  paths do not guarantee a post event. `PermissionRequest` still has no call id
  in the official contract. A generic tool completion cannot safely establish
  which pending approval was answered, especially with parallel calls.
  Next A3 work must capture the real question lifecycle and approval-only
  recovery, correlate question completion by pane/session/turn/call, and retain
  the blocked state for unrelated completions. Silence or pane output alone
  remains insufficient evidence. Restart the CLI after migration and review
  `/hooks`; live permission single-delivery, quiet recovery and multi-pane
  acceptance remain open. The user also observed a self-reported `blocked`
  square while this agent was still working, with output age `0.0s`; this records the symptom,
  without attributing that report to hooks rather than MCP. The separate
  [pager request #545](https://github.com/Tripstack-Corp/spyc/issues/545) will
  make `:why-status` easier to inspect. See the
  [official hook contract](https://learn.chatgpt.com/docs/hooks).

- A3b3 landed through PR #548 (`9f24dd4b`) after local-build acceptance
  on 2026-10-07, based on #546.
  The source-routing and protocol fixes passed `RUST_TEST_THREADS=1 make check`:
  2760 library tests, one existing ignored test, all integration tests (including
  four actual reporter-binary tests) and VT-system tests. Automated local native
  question and keyboard approval loops passed on the fresh build. The user
  accepted that result and requested continuing. GitHub lint, tests and
  packaging passed before the squash merge.
  The local CLI is `0.160.1`; the local source audit at `f380b487` and the
  official generic-function hook contract support narrowly scoped
  `request_user_input` start/completion instrumentation. Real native execution
  and recovery are verified with already trusted hooks. Fresh trust onboarding
  remains a separate acceptance case.
  The test build installs exact-match `PreToolUse`/`PostToolUse` reporters for
  that tool. Its completion restores `working` only after a matching start in
  the same pane, session, turn and call; another question or uncorrelated block
  keeps attention latched. Enter alone cannot clear an identified question.
  Pending correlation lives in the pure Model, is bounded to eight questions
  per pane and pruned for replaced/closed panes. No arguments or answers are
  retained. Distinct wire statuses are rejected by older hosts; a newer host
  ignores question reports from older reporters without correlation metadata.
  Diagnostics retain unapplied reports with a reason. Ordinary explicit agent
  and lifecycle reports retain their existing authority.
  All five pure correlation regressions failed against the scaffold; three
  hook/dispatch/input regressions failed against the prior behaviour. Deliberate
  mutations were detected at dispatch, the input helper and its production call,
  replaced-pane pruning and the actual reporter binary. Production was restored
  after every probe. The initial `RUST_TEST_THREADS=1 make check` gate passed:
  2751 library tests, one existing ignored test, all integration tests (including
  four actual reporter-binary tests) and VT-system tests. Live acceptance was
  pending at that point. This does not fix correlated semantic approval recovery:
  `PermissionRequest` lacks a call id, and successful completion of an unrelated
  tool cannot establish which permission was answered. Invalid/cancelled
  question calls may have no post event; `Stop`, `Interrupt` or a newer explicit
  report retires the wait. The new hooks require restart and `/hooks` review;
  spyc does not alter trust records. See the
  [official hook contract](https://learn.chatgpt.com/docs/hooks).
  The first A3b3 live attempt on 2026-10-06 did not exercise the native question
  hooks. PID 9553 was verified running the supplied `taa534jt` artifact, whose
  SHA-256 remained `0a2c9883…`. The 11:27:57, 11:28:23 and 12:03:29 UTC dumps
  contained only one `UserPromptSubmit` report and a restart-needed marker.
  The final dump showed that working report expired, with output-timing taking
  over; this is not evidence of question completion. The captured worktree config
  contained both exact-match native question hooks; reporter tracing contained
  no question invocation for the test. Read-only rollout metadata identifies
  CLI `0.160.1`, Default mode, and a `request_user_input_async` call at
  11:28:09.237 UTC. Its tool result acknowledged acceptance 43 ms later, followed
  by repeated sleep calls; no `request_user_input` call occurred. Async tool
  completion therefore cannot be used as an answer signal. That path remains
  an A3 coverage gap. The native-hook retest requires a Codex restart, `/hooks`
  review and entering `/plan` as an actual slash command before the diagnostic.
  The second attempt (PID 51304, `codex resume --last`) showed a native Plan-mode
  question open while the dot remained `working`. Dumps at 12:34:12, 12:34:25
  and 12:34:37 UTC contained no question event, only `UserPromptSubmit` after
  launch. Read-only rollout metadata confirms a native `request_user_input`
  call at 12:34:32.540 UTC. This reproduced a real source-routing defect rather
  than an async-tool mismatch.
  Local Codex source inspection found root-checkout hook routing for linked
  worktrees. The installed CLI's read-only `hooks/list` RPC independently
  confirmed it: with the six-hook worktree config temporarily restored, it
  loaded the four trusted spyc reporters from main's `.codex/config.toml` and
  omitted both question hooks. The worktree declarations were ignored. The
  temporary config was removed only after verifying its bytes were unchanged;
  no trust records or main-checkout files were manually edited.
  spyc now resolves Codex's actual hook source for installation, presence,
  consent, refcounting, re-healing and restart detection. Ordinary MCP config
  remains worktree-local. Five integration regressions reproduced the wrong
  source before the fix; they also exposed a trailing-slash consent/ownership
  key mismatch. Additional tests cover denied/unasked root consent, drift and
  tracked-source refusal. Deliberate mutations of consent, drift routing and
  the tracked-file guard each failed their regression; production was restored.
  That build still required native question and quiet-after-answer acceptance;
  the automated test below completed those cases after a protocol fix.
  Approval-only semantic recovery and async-answer coverage remain A3 gaps.
  The next test instance, PID 78174, was verified running the fresh `sf_du3_d`
  artifact. Its 13:32:34 UTC dump names the root-checkout hook source and
  records definition presence at launch. The 13:33:51 UTC dump contains one
  `UserPromptSubmit` report, with no question event. Read-only rollout metadata
  identifies Default mode and `request_user_input_async` at 13:33:37.526 UTC;
  its immediate result arrived 32 ms later. No native `request_user_input`
  occurred in that observed turn. The installed CLI's read-only `hooks/list`
  now lists all six spyc reporters at main's source, including both exact-match
  native question hooks as enabled and trusted. This confirms source discovery
  and current trust metadata, not native hook execution or answer recovery.
  A native Plan-mode retest was still required.

  On 2026-10-07, the demo worktree's installed Microsoft `tui-test` harness
  automated the real CLI in dedicated named sessions. The native Plan-mode
  question reproduced `working` while its dialogue was open. Reporter tracing
  showed `PreToolUse` was emitted and sent; MCP protocol validation rejected
  the private question signal before the App received it. A protocol regression
  failed on that rejection, then passed after both correlated wire values were
  admitted. The saved TUI driver also failed against the prior `sf_du3_d` build.
  The corrected `rmbkuxtj` artifact has SHA-256
  `9a8103ca58ba22673e172dec26d744363eb702df629e0577ef2a87862975de01`.
  Its native question loop passed: one `UserPromptSubmit`, one `PreToolUse`
  reporting `blocked` while awaiting input, one `PostToolUse` restoring
  authoritative `working` with the same pane/session/turn/call, continued
  authoritative working during the quiet sleep, and one `Stop` reporting
  `done`. The first driver attempt at completion raced final text against
  asynchronous Stop delivery; the corrected driver waits for the actual report.
  The approval loop separately verified one `PermissionRequest`, `blocked`
  while the CLI's permission dialogue was open, one-time approval of `sleep 30`,
  output-timing `working` after Enter and during the sleep, then authoritative
  `done` from one `Stop`. This validates the existing keyboard recovery path;
  it does not establish correlated semantic approval completion. The permission
  event supplied no call id. Neither loop used an agent `report_status` call,
  hook-trust approval, trust-file editing or an approval bypass.
  The repeatable driver is `scripts/codex-tui-smoke.py`; its isolated-session
  recordings, viewport captures, extracted dumps and binary manifests remain
  local artifacts. Single-delivery counts are asserted per observed test turn.

- A3b4 landed through PR #549 (`eff0bce1`) after local-build acceptance
  on 2026-10-07, based on #548.
  The real CLI `0.160.1` mixed approval → native question test failed against
  the accepted A3b3 binary on 2026-10-07. Its permission and question shared a
  turn; exactly one native start and matching completion were received. Enter
  had cleared the visible ordinary permission latch, but its correlation state
  still retained an uncorrelated blocker. The completion was recorded as
  unapplied and the dot remained blocked at output age `0.0s`.
  A permanent harness regression reproduced the same stale state before the
  fix. Prompt-settling input now retires both representations of that ordinary
  latch together. It preserves pending identified questions and overflow;
  typing or pasting does not recover an unanswered permission. The TUI driver
  includes the mixed sequence and waits for long diagnostic text to render
  before submission. The full `RUST_TEST_THREADS=1 make check` gate passed:
  2762 library tests, one existing ignored test, integration and VT-system tests.
  Deliberate production mutations were caught for model retirement, executor
  wiring and non-settling input; production was restored before the gate.
  The fresh `x5d5b_lq` artifact has SHA-256
  `4f77eb5907fd208a782ace6d6fbee6eb117ad3691393c10cd6db1874137f2da4`.
  The real-CLI mixed sequence passed: one prompt, one permission request,
  one question start/completion with matching pane/session/turn/call, and one
  Stop. Question completion restored authoritative working, retained during
  the quiet `sleep 30`, followed by authoritative done. The standalone question
  and approval loops passed against the same artifact. Approval-only working
  remained output-timing recovery after Enter, rather than a correlated semantic
  permission completion. No loop called agent `report_status`, approved hook
  trust or edited trust records. The user launched the supplied new version
  and requested continuing, accepting the build for integration. GitHub lint,
  tests and packaging passed before the squash merge. The remaining A3 gaps
  below are not closed by this slice.
  A read-only exact-version upstream audit confirms that
  [`request_user_input_async`](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/core/src/tools/handlers/request_user_input_async.rs)
  posts an async message and immediately returns acceptance without awaiting
  an answer. The
  [TUI reply parser](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/tui/src/async_question_reply.rs)
  recognizes answers in a later user message. This rules out using async tool
  completion as answer recovery, and a blocking start would misrepresent
  continuing work. The ordinary permission hook still has no call id in the
  [official contract](https://learn.chatgpt.com/docs/hooks); this slice repairs
  keyboard recovery state without inventing correlated permission completion.
  Fresh trust onboarding, async attention and live parallel-pane isolation
  remain A3 acceptance gaps.

- A3b5 landed through PR #550 (`fb5b60b9`) after local-build acceptance
  on 2026-10-07, based on #549.
  The accepted A3b4 artifact passed the real two-pane native-question loop:
  both questions blocked, answering the second restored only that pane, the
  first remained blocked until answered, and both independently reached done.
  The two MCP connections and pane/session/turn/call ids were distinct; each
  pane received one prompt, one question start/completion and one Stop. Replay
  mutations detected recovery of the unanswered pane, a changed completion
  call and cross-bound sessions.

  The user's continuing-work dump then exposed a separate false block:
  `PermissionRequest` had latched blocked after automatic review of a completed
  MCP tool, with no human answer required. An exact `0.160.1` source audit of
  [permission events](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/hooks/src/events/permission_request.rs)
  and [approval routing](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/core/src/tools/approvals.rs)
  confirms that the hook precedes hook allow/deny decisions, automatic review
  and user review. It supplies no call id, and `permission_mode` describes
  approval policy rather than reviewer identity. A real automatic-review
  `sleep 30` regression failed against the accepted A3b4 artifact with no
  dialogue open and an authoritative blocked report.

  The worktree records metadata-bearing permission reports as observations,
  leaving existing working/question reports intact. Owned hook command bytes
  remain unchanged, preserving trust hashes. A captured complete command
  approval at the viewport bottom supplies a narrow UI fallback that
  temporarily overrides non-blocked reports; those reports resume after the
  dialogue closes. Activity settling and both diagnostics share this policy.
  Explicit agent blocks and native questions retain semantic precedence.
  Clipped/customized dialogues, other approval types and async-answer attention
  remain outside this verified rule. No generic post-tool hook or invented
  permission correlation is added.

  Five permanent regressions failed before the fix, then passed. The library
  suite passed 2767 tests with one existing ignored test. The first fixed
  artifact passed the real automatic-review loop without an approval key or
  agent `report_status`. Fresh `CODEX_HOME` discovery separately confirmed that
  an untrusted project's six hook declarations are ignored, with Codex's trust
  warning and no trust configuration written. This negative probe does not
  establish execution after trust approval. The scanner call-site guard caught
  a deliberate production mutation to scrollback and the source was restored.
  An expanded complete-dialogue fixture also failed with a twelve-line region;
  the rule now reads the bounded current viewport and still requires the exact
  modal footer at its bottom, excluding prior dialogue above the composer.

  The final unpiped `RUST_TEST_THREADS=1 make check` gate passed: 2768 library
  tests, one existing ignored test, all integration tests and six VT-system
  tests. All five real-CLI loops passed against the exact final artifact:
  automatic review, human command approval, mixed approval → native question,
  standalone native question and parallel questions. Automatic review retained
  authoritative working without an approval key; human approval used the
  labelled UI fallback, then resumed the retained working report. Questions
  recovered only their matching pane/session/turn/call. The parallel run had
  two bound connections and eight reports; the unanswered pane stayed blocked
  while the other worked and finished. None called agent `report_status`,
  approved hook trust or edited trust records.
  The final binary is
  `/private/tmp/spyc-codex-a3b5-final.5yj63yev/spyc`, SHA-256
  `a809801d0a13d0dba6783efd0817bc14caf025905764891ae22bf46edef33cc1`.
  Its `test.sh` launcher selects the worktree and prepends that binary to
  `PATH`, so both the host and hook reporter use the supplied build. Local
  artifacts retain manifests, recordings, viewport captures, extracted dumps
  and step logs. The live host and MCP proxy were verified against the exact
  artifact hash. The user reported that it was working well and requested
  continuing, accepting the build for integration. GitHub lint, tests and
  packaging passed, and the exact PR head was `CLEAN` before squash merge.
  The clean main checkout was fast-forwarded to that merge. The accepted
  worktree is retained while the user's running host and panes use it.
  Full fresh-trust execution, async attention and other approval-dialogue
  formats remain A3 acceptance gaps; parallel native-question isolation passed.

- A3b6 is ready for integration on `fix/codex-edit-approval`, refreshed onto
  merged PTY input-backpressure PR #551 and main's subsequent refactors
  (`f334affc`).
  the earlier #550-based build is superseded for the next handoff.
  A real Codex `0.160.1` file-edit approval failed against A3b5: the complete
  dialogue was open but the dot remained working. Its captured modal became
  a fixture and the regression failed before the fix. The initial file-edit-only
  implementation passed 2769 library tests, all integration and VT-system tests,
  plus five native CLI loops (approve, decline, Esc dismissal, automatic review
  and mixed command approval → native question) on the earlier artifact.
  Those historical checks used trusted hooks and do not establish acceptance
  of the expanded current build.

  Review then identified uncovered MCP and network waits. Native MCP capture
  showed a single-field Allow/Cancel form, distinct from command approval.
  Continuous repainting also prevented the trailing-quiet scan from running.
  A real 40-column command capture showed native wrapping across required
  phrases and the footer. Permanent regressions reproduced each missed wait
  against the old code, including the App path with a real PTY and continuing
  output timestamps. Network and persistent MCP option fixtures come from
  exact-version upstream UI snapshots; their provenance is separate from the
  native command, file-edit and MCP captures.

  Rules now cover command, file-edit, single-field MCP-tool and network approval
  forms with known phrases and the complete footer at the viewport bottom.
  Native word wrapping is accepted; old quoted modals above the composer,
  missing required text and prefixed/clipped footers are rejected. Codex scans
  within 250 ms of the first pending repaint rather than allowing each redraw
  to postpone detection. The retained report and existing precedence policy
  are unchanged: semantic question/agent blocks still win, and permission hooks
  remain observational. Hook commands and correlation are unchanged.

  The ten focused approval regressions pass. Deliberate network-rule, footer
  boundary and scan-deadline mutations were detected, and production was restored.
  The initial refreshed full gate passed 2788 library tests, all integration
  suites and six VT-system tests. Native command approval at 40 columns, MCP
  approve/cancel at 200 columns and MCP approve at 40 columns passed on the
  #551-based combined artifact. Each held a blocked scrape for six seconds and
  recovered after answering; only approved MCP calls wrote their marker once.
  The refreshed gate on `209b7e4f` passed the same full suite. Five native
  UI checks then passed against the exact final artifact: 40-column command
  approval, 200-column MCP approve and cancel, 40-column MCP approve, and a
  repeated wide MCP approval after correcting the driver's session-name check.
  That check now compares exact names in structured JSON rather than rejecting
  a distinct session with the same name prefix. Every held modal used the
  blocked scrape source, with no semantic reports; each answer cleared it.
  Approvals wrote exactly one disposable MCP marker and cancellation wrote none.
  This verifies UI recovery, not retained semantic working or native hook delivery.
  The current Gemma model chose shell commands rather than `apply_patch` in two
  file-edit attempts; those runs do not establish a native file-edit dialogue
  on the new build. The recorded native file-edit fixture regression passes.

  The fresh binary is `/private/tmp/spyc-codex-a3b6-current.umb6tuwn/spyc`,
  SHA-256 `fe4628a0416a9d53495bb09093d85d89c470880a5e11e7a679c7a00755067ea5`.
  Its `test.sh` selects this worktree and places the exact binary/reporter on
  `PATH`. The manifest records the source head, build-time changed-file hashes,
  gate log and native UI results. On 2026-10-08, the user explicitly authorized
  proceeding with added automated TUI coverage while deferring their manual
  local test. This authorizes integration after checks; it does not establish
  manual acceptance or fresh hook execution. The UI-only driver can skip
  startup hook review with Esc, without approving or editing trust records.
  Read-only `hooks/list` currently labels six root-checkout spyc reporters
  modified; live semantic-hook recovery requires user review of that trust state.
  Generic MCP elicitation, extra-permission and stdin-write forms, a live native
  network approval, async attention and fresh-trust execution remain A3 gaps.

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

### Deferred shell-tab agent tracking

[Feature request #544](https://github.com/Tripstack-Corp/spyc/issues/544) records
the shell → known agent → shell → relaunch expectation, including bash, zsh and
other shells. Design is deferred at the maintainer's request. Direct agent tabs
and configured direct startup tabs remain the A3 scope; dynamic shell-tab
identity is a separate feature and does not expand the Codex compatibility gate.

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

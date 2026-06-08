# history-seg-docs-planning — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-docs-planning
Created: 2026-06-08T22:10:53.428375+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:10:53.428375+00:00
Role: scribe
Type: Note
Title: PR #72/#73/#74 — public-facing positioning: README MCP-from-the-pane thesis, AGENTS conventions, presentation refresh

Spec: scribe

tags: #history #docs-planning

Moment: docs-planning — Reconstructed: the public-facing surface (README lede, AGENTS conventions, presentation deck) is rewritten in one three-PR burst to a single thesis: spyc is "the noun the agent operates on," not a file manager that hosts a chat window.   [kind: convention]
When: 2026-05-11 · PR #72 (docs/readme-rewrite) commit 7c568f9 · PR #73 (docs/agents-md-conventions) commit 1c34599 · PR #74 (docs/presentation-update) commit 5497973
Recorded rationale: "Most 'AI in your terminal' tools give the agent a chat window and let you copy-paste paths back and forth. spyc puts a local MCP socket next to the file view, so the agent can ask spyc *what is the cursor on, what is staged, what is pinned, what is in this directory* … The file commander is the noun the agent operates on, not the chrome around it." — README.md, added PR #72
Inferred intent: a single positioning pass that propagates one framing across every public artifact, triggered by an external catalogue review. evidence: README.md "Why spyc?" rewrite (7c568f9); AGENTS.md sub-section explicitly attributes its rules to "External catalogue review (the *watercooler* analysis platform)" (1c34599); presentation CHANGELOG says the deck "was written around M14 (HTTP MCP era) and had drifted" (5497973). confidence: high
Supersedes: prior README lede ("A vi-keyboard-driven file commander that runs Claude Code in a split pane / and exposes itself to Claude as an MCP server") and the stale `v1.21.1` footer — both removed in 7c568f9; Cargo.toml `description` field that "had drifted to 'pairs with Claude Code' wording that pre-dated codex/gemini support" (5497973).

Reconstructed: three docs PRs land on the same day, each tightening one public surface to the MCP-from-the-pane thesis. #72 rewrites README's lede and "Why spyc?", adds a "What it is" two-pane one-pager (top pane = vim-flavoured commander; bottom pane = child process, Claude Code/codex first-class, Gemini supported), and corrects `spyc -r` to "restores each pane to its own Claude / Codex / Gemini conversation." #73 adds AGENTS.md's "Commits, merges, and CHANGELOG" sub-section — three conventions the diff attributes to watercooler's catalogue review: (1) commit subject = actual scope not its caption; (2) "Squash on merge … `main`'s `git log` becomes one commit per shipped shape"; (3) CHANGELOG bucket follows user-observable nature, not file location. #74 refreshes `docs/presentation.html` from the HTTP-MCP era (HTTP → PID-scoped Unix domain socket; stats bumped 19K→35K LOC, 358→638 tests).

Note the recursion: #73 is the recorded origin of the squash-merge convention that the deep-history contract observes ("later PRs are squash merges") — the planning cadence here writes the rule that later engineering segments inherit. The README/Cargo.toml description change is positioning that later steers the yazi competitive review (#157) and the roadmap thesis (#179), both of which re-quote "the noun the agent operates on" verbatim.

Provenance:
- 7c568f9 (PR #72 docs/readme-rewrite, 2026-05-11) — README.md lede + "Why spyc?" → "What it is" rewrite; CHANGELOG "README rewrite leading with the MCP-from-the-pane thesis"; Cargo.toml/Cargo.lock version bump.
- 1c34599 (PR #73 docs/agents-md-conventions, 2026-05-11) — AGENTS.md +31 lines "Commits, merges, and CHANGELOG"; the three conventions attributed to watercooler's catalogue review.
- 5497973 (PR #74 docs/presentation-update, 2026-05-11) — docs/presentation.html refresh (HTTP→Unix socket, stats, roadmap flips); Cargo.toml `description` updated off the "pairs with Claude Code" wording.

<!-- Entry-ID: 01KTMMKHEYZ540PV6VVZR33K79 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:11:32.410285+00:00
Role: scribe
Type: Decision
Title: PR #76/#77/#79 — V1_60_PLAN "CounterTop": recursive → siblings+mirror rewrite, then compatibility hardening

Spec: scribe

tags: #history #docs-planning

Moment: docs-planning — Reconstructed: the v1.60 "CounterTop" hub plan is filed, then rewritten within hours from a recursive-composition architecture to siblings+mirror, then hardened with a capability-negotiation compatibility layer — three PRs in one day showing the plan-churn before any code.   [kind: supersession]
When: 2026-05-12 · PR #76 (docs/v1.60-plan) commit ddd2194 · PR #77 (docs/v1.60-plan-rewrite) commit 27c6467 · PR #79 (docs/v1.60-plan-compat) commit 27f8d83
Recorded rationale (v1): "The thesis is recursive composition: spyc panes already host any program; spyc happens to be a program; therefore spyc panes already host spyc. … each workspace is a child spyc process running in a pane tab of the master." — docs/V1_60_PLAN.md, added PR #76
Recorded rationale (v2): "The architectural choice is **siblings + mirror**, not recursive composition. Each spyc owns its own pty and lives in its own terminal window … The hub is a peer that happens to be a client of every other peer's MCP socket." — docs/V1_60_PLAN.md, rewritten PR #77. CHANGELOG #77: "Design discussion with the user reframed the architecture … The recursive-composition route from yesterday's plan is recorded as considered-and-rejected."
Inferred intent: a same-day architectural reversal driven by recorded design discussion, then a review-driven robustness pass. evidence: #76 diff adds 259 lines with recursive thesis; #77 rewrites 421-line churn (261 ins / 181 del) flipping to siblings+mirror and explicitly listing recursion as "considered first; rejected after design discussion"; #79 CHANGELOG names "Two questions surfaced during plan review." confidence: high
Supersedes: PR #76's recursive-composition thesis (each workspace a child spyc in a master's pane tab) — superseded by #77's siblings+mirror within ~4.5h (merge 11:04 → 15:26). #76's discovery-file shape (`{pid, project_home, session_name, mcp_socket, …}`) is superseded by #79's `{schema_version, spyc_version, capabilities, mode, …}`.

Reconstructed: #76 files docs/V1_60_PLAN.md ("CounterTop") on a recursive thesis — the master spyc runs each workspace as a child in a pane tab, discovery + introspection over the MCP socket each child already exposes. Hours later #77 reverses it: each spyc owns its own pty in its own terminal, the hub is a peer client, "take control" = mirror the remote's render stream (`subscribe_frames`) and forward keystrokes (`send_input`), with last-keystroke-wins justified because "the OS already serializes intent." The CHANGELOG records the recursion route as considered-and-rejected after design discussion with the user. #79 then adds a Compatibility section after plan review — a per-capability behavior matrix (`status`/`frame_mirror`/`input_forward`) and the stated principle "an older peer is visible but degraded, never invisible," plus atomic publish (write `.tmp` + rename) and `notify`-watched discovery.

Note #79 also bundled an unrelated Fixed entry (`^a-j`/`^w-j` reaching the resolver from a `D`-opened pager — the meta-chord-fallthrough fix); that code change belongs to pane/chord-routing work, not this docs segment — pointer to the pane-behavior arc. The CounterTop discovery surface and frame-mirror primitives later feed the v1.70 plan, which says "the MCP socket that V1_60 used informally for peer discovery becomes a formal typed surface in V1_70."

Provenance:
- ddd2194 (PR #76 docs/v1.60-plan, 2026-05-12) — docs/V1_60_PLAN.md +259 (recursive thesis); ROADMAP.md +6; Cargo bump.
- 27c6467 (PR #77 docs/v1.60-plan-rewrite, 2026-05-12) — docs/V1_60_PLAN.md churn 261/181 (siblings+mirror); CHANGELOG rewrite entry.
- 27f8d83 (PR #79 docs/v1.60-plan-compat, 2026-05-12) — docs/V1_60_PLAN.md +136 (Compatibility matrix, schema_version/capabilities); CHANGELOG Documentation + unrelated Fixed (`^a-j` pager meta-chord fallthrough).

<!-- Entry-ID: 01KTMMMQ1VY8ZERQ3NQAF89DN4 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:12:21.395457+00:00
Role: scribe
Type: Decision
Title: PR #86 — AUTO_APPROVAL_PLAN: curate native agent permissions, reject pty interception (BUGS→plan→roadmap promotion)

Spec: scribe

tags: #history #docs-planning

Moment: docs-planning — Reconstructed: a one-line BUGS.md wish ("approve certain Claude CLI actions automatically and keep a log") is promoted into a full six-phase v1.51 plan that records a rejection: curate each agent's native permission file, never intercept the pty.   [kind: rejection]
When: 2026-05-13 · PR #86 (docs/auto-approval-plan) commit bf58312
Recorded rationale: "**Curate each agent's native permission system; do not intercept the pty.** Considered and rejected: **pty interception** … the failure mode is *silent wrong-approval*: any upstream change to the prompt format breaks our matcher, and the safest fallback (don't approve) is the worst UX. Security features should not be built on regex against another tool's UI." — docs/AUTO_APPROVAL_PLAN.md, added PR #86
Inferred intent: a planning pass that converts a vague backlog item into a scoped feature with an architecture decision and an explicit anti-pattern. evidence: bf58312 deletes the two-line BUGS.md item "feature to allow spyc to approve certain Claude CLI actions automatically and keep a log" and adds docs/AUTO_APPROVAL_PLAN.md +380; ROADMAP.md gains a v1.51 entry pointing at the plan. confidence: high
Supersedes: the BUGS.md SMALL entry tracking auto-approval (removed in bf58312) — promoted out of triage into a planned release.

Reconstructed: the plan's thesis names permission prompts as "the friction" for trusted patterns (`git status`, `cargo check`, `*.md` edits, `Read`), and frames two asks — auto-approve a curated pattern set, and keep a verifiable action log. The architectural choice is recorded as decided: read/write each agent's official settings file (Claude `.claude/settings.json`, Codex `.codex/config.toml`, Gemini TBD), let the agent itself decide whether to prompt, and build the log by reading each agent's transcript files (already done for session resume). The plan explicitly considers and rejects pty interception on a security argument — "Security features should not be built on regex against another tool's UI." Target release v1.51, "feature-shaped, not blocking v1.60 hub work" — the doc positions itself against the v1.60 CounterTop track from the prior moment.

This is the segment's clearest BUGS→plan→roadmap promotion pattern: a raw triage line becomes a doc with a decision, then a roadmap entry. The same promotion cadence recurs in the curation moment (#159/#160). Implementation of the per-agent settings curation and the `:approvals` pager would land in agent-integration engineering, not this docs segment.

Provenance:
- bf58312 (PR #86 docs/auto-approval-plan, 2026-05-13) — docs/AUTO_APPROVAL_PLAN.md +380 (thesis, decided architecture, per-agent specifics, rejection of pty interception); BUGS.md −2 (item promoted); ROADMAP.md +7 (v1.51 entry).
- 01KTMMMQ1VY8ZERQ3NQAF89DN4 (prior entry, this thread) — v1.60 hub track this plan defers behind.

<!-- Entry-ID: 01KTMMPA6HBDB91KMZPTBPNHX3 -->

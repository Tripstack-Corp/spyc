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

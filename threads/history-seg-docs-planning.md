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

---
Entry: Claude Code (caleb) 2026-06-08T22:13:43.522270+00:00
Role: scribe
Type: Note
Title: PR #92/#93 — PANE_RECOVERY_PLAN (tiered recovery) + PANE_STARTUP_TABS_PLAN, both from external-contributor analysis

Spec: scribe

tags: #history #docs-planning

Moment: docs-planning — Reconstructed: two pane plans land back-to-back, both sourced from external-contributor analysis (Caleb Howard, 2026-05-15). PANE_RECOVERY_PLAN splits "recovery" into tiers by program kind; PANE_STARTUP_TABS_PLAN adds config-driven startup tabs while deferring real splits. The same PRs dump a large batch of contributor-reported bugs into BUGS.md.   [kind: new-capability]
When: 2026-05-16 · PR #92 (docs/pane-recovery-plan) commit db70c95 · PR #93 (docs/pane-startup-tabs-plan) commit abeac38
Recorded rationale (recovery): "The 'recovery' word collapses three very different problems. … Recovery splits along **what kind of program is in the pane**. Each kind admits a different ceiling on what we can restore, and the right answer is to handle each tier explicitly." — docs/PANE_RECOVERY_PLAN.md, added PR #92
Recorded rationale (startup tabs): "A spycrc knob that opens K tabs in the bottom pane at startup, instead of just one. … No splits, no tree, no grid. Just 'open these K tabs for me when I launch.'" — docs/PANE_STARTUP_TABS_PLAN.md, added PR #93
Inferred intent: a planning pass that turns a daily-driver pain report into a tiered design, plus a small opportunistic config feature, while parking the broader bug haul in BUGS.md. evidence: both docs carry the header "Sourced from external-contributor analysis (Caleb Howard, 2026-05-15)"; db70c95 adds the plan +215 and a +37-line BUGS.md batch (hide-don't-destroy, J?, PgUp/PgDn discoverability, DSL gaps, `unmap` no-op); abeac38 adds +219 plan + ROADMAP entry. confidence: high
Supersedes: (none) — both are new docs; the startup-tabs doc explicitly distinguishes itself from session-restore (`spyc -r` round-trips `Session.tabs` already).

Reconstructed: PANE_RECOVERY_PLAN (#92) is explicit that it is distinct from the in-session hide/unhide round-trip (`F10`/`^a-\`) — "that's … a simpler fix (hide-don't-destroy; the pty stays alive)." It tiers recovery by program: Tier 1 known agent CLIs (Claude/Codex/Gemini) with first-class resume, where it proposes tightening Claude sid-capture by listening on the MCP socket for the "session created" notification instead of the banner/JSONL race; Tier 2 stateful processes that rediscover their own state on respawn; lower tiers for bare shells. The ROADMAP gains a v1.52 entry: Phase 0 cosmetic vt100-grid snapshot backdrop, Phase 1 MCP-side sid capture, Phase 2 opt-in `[pane] use_tmux`.

PANE_STARTUP_TABS_PLAN (#93) proposes a `[pane] tabs = [...]` compact form plus `[[pane.tab]]` table form (cap 9 to match `^W 1..9`), reusing the existing tab system — "No splits, no layout refactor." The ROADMAP entry notes it "also captures the larger 'real splits' ask … as a deferred future direction." This aligns with the established project convention that pane multiplicity means tabs, not tmux-style splits.

Cross-segment: the hide-don't-destroy fix this plan scopes out as "simpler" is the work that lands in the pane-behavior arc — see history-arc-03-pane-behavior (the repo's current branch at reconstruction time is `fix/pane-toggle-hide-not-destroy`). The contributor-bug batch in #92's BUGS.md (J?, DSL completeness, PgUp/PgDn) is later promoted to ROADMAP in #160 (see the triage-discipline moment).

Provenance:
- db70c95 (PR #92 docs/pane-recovery-plan, 2026-05-16) — docs/PANE_RECOVERY_PLAN.md +215; BUGS.md +37 (contributor batch); ROADMAP.md +8 (v1.52 entry). Cites src/state/sessions.rs `AgentKind` enum as the existing resume substrate.
- abeac38 (PR #93 docs/pane-startup-tabs-plan, 2026-05-16) — docs/PANE_STARTUP_TABS_PLAN.md +219; ROADMAP.md +10. Cites src/app/mod.rs:4646-4676 (`Action::PaneNewTab`), src/keymap/action.rs:114 (`PaneTabByIndex`).

<!-- Entry-ID: 01KTMMRKSSJ8EN3RC6RB36RSM3 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:14:28.414142+00:00
Role: scribe
Type: Decision
Title: PR #114 — V1_70_PLAN "Mise en Place": programmatic addressability, one protocol / three clients, rmux-inspired crate split

Spec: scribe

tags: #history #docs-planning

Moment: docs-planning — Reconstructed: the v1.70 plan reframes the MCP socket from an informal peer-discovery channel (v1.60) into a formal typed daemon protocol — stations, plates, orders, bells — consumed by one protocol across three clients (CLI / SDK / MCP), and sequences a crate split before the protocol work.   [kind: new-capability]
When: 2026-05-21 · PR #114 (docs/v1.70-mise-en-place-plan) commit 94aa3fb
Recorded rationale: "v1.70 makes spyc **programmatically addressable**. Every pane, selection, and pager view becomes a named target with a structured snapshot. External clients … issue typed *orders* against those targets and wait on typed *bells* … instead of timer-based heuristics. The MCP socket that V1_60 used informally for peer discovery becomes a formal typed surface in V1_70." — docs/V1_70_PLAN.md, added PR #114
Inferred intent: a planning pass that absorbs a competitor's (rmux) differentiators — typed daemon protocol, embeddable widget, structured snapshots — while rejecting its tmux compatibility, and stages the crate split as the enabling seam. evidence: 94aa3fb adds docs/V1_70_PLAN.md +271; doc cites rmux + its HN launch as Inspiration and says the differentiators "we want to absorb are the typed daemon protocol + embeddable widget + structured snapshots, not the tmux compatibility"; ROADMAP +10 seven-phase entry. confidence: high
Supersedes: extends v1.60's informal `subscribe_frames`/`send_input` — the plan says "Peer subscription (V1_60's `subscribe_frames`) becomes one order shape among many." Does not replace v1.60; builds the typed rail above it.

Reconstructed: the plan's architecture is recorded as decided — "one protocol, three clients": a `spyc` CLI (`spyc send-keys`, `spyc plate`), a `spyc-sdk` crate (typed async Rust, embeddable), and the existing MCP server (becomes "a thin wrapper over the daemon protocol; tool names stay the same for compatibility"). All three speak to the running spyc process itself — "there's no separate `spycd`." Kitchen vocabulary names the surfaces: Station (stable pane/picker/pager handle, survives layout changes), Plate (structured snapshot — cwd, selection, mode, exit code, prompt-ready flag), Order (typed incoming command), Bell (async ready signal replacing timer heuristics like `RESTORE_BANNER_SETTLE`). Phase 1 introduces `StationId(u32)` minted monotonically on `PaneTabs::push` / `App::set_pager`.

Cross-segment pointer: the plan states "Crate split happens *before* the protocol work" and lists single-responsibility crates (`spyc-proto`, `spyc-os`, `spyc-pty`, `spyc-ipc`, `spyc-render-core`, `spyc-core`, `spyc-server`, `spyc-cli`, `spyc-sdk`) on the rmux model. That crate-split/MVU execution is owned by history-seg-refactor-mvu — this entry is the pointer only; the decomposition that unblocks it is later sequenced into the road-to-2.0 in #179 (next-but-one moment).

Provenance:
- 94aa3fb (PR #114 docs/v1.70-mise-en-place-plan, 2026-05-21) — docs/V1_70_PLAN.md +271 (thesis, one-protocol/three-clients, kitchen vocab, seven phases, crate split); ROADMAP.md +10 (v1.70 entry). Cites rmux (github.com/helvesec/rmux) + HN item 48219918 as inspiration.
- 01KTMMMQ1VY8ZERQ3NQAF89DN4 (prior entry, this thread) — v1.60 plan whose `subscribe_frames` this formalizes.

<!-- Entry-ID: 01KTMMSRJ3DEDE2S4VWY5JHEND -->

---
Entry: Claude Code (caleb) 2026-06-08T22:15:19.923948+00:00
Role: scribe
Type: Note
Title: PR #157/#158 — YAZI_COMPETITIVE_REVIEW + roadmap follow-up: benchmark the nearest neighbour, restate the thesis, re-scope DnD/cwd

Spec: scribe

tags: #history #docs-planning

Moment: docs-planning — Reconstructed: a competitive review against Yazi (filed the same day Yazi merged its OSC 72 drag-and-drop PR) catalogues feature-by-feature standing, restates spyc's thesis, and flags a stale ROADMAP DnD entry. The same-day follow-up #158 acts on it: rewrites the DnD entry around OSC 72 + a drop-action picker and promotes cwd-export-on-quit to a working track.   [kind: convention]
When: 2026-05-28 · PR #157 (docs/yazi-competitive-review) commit 6d96e8d · PR #158 (docs/roadmap-dnd-cwd-followup) commit 3fe93f0
Recorded rationale: "Yazi is the closest neighbour in the TUI file-commander space … `ROADMAP.md` carries four Yazi-inspired entries … but there is no single place that lays out what Yazi actually does, where we overlap, and where we deliberately don't. This doc is that place. … A two-pane file commander whose distinguishing feature is a local MCP socket … The file commander is the noun the agent operates on. Yazi is not in this game." — docs/YAZI_COMPETITIVE_REVIEW.md, added PR #157
Inferred intent: a positioning/benchmarking pass that converts a competitor's daily activity into concrete roadmap edits, keeping the differentiator (MCP-from-the-pane) explicit. evidence: 6d96e8d adds the review +220, dated against "PR #4005 (drag-and-drop) merged the same day"; flags ROADMAP.md:571 DnD as "stale in two ways" (OSC 52 is clipboard not DnD; kitty-only payoff small); 3fe93f0 then rewrites that entry around OSC 72 + a drop-action picker and moves cwd-export from "Additional Ideas" up into a working track. confidence: high
Supersedes: ROADMAP.md DnD entry "files from the desktop into spyc via OSC 52 or path paste" (revised in 3fe93f0 to OSC 72 + Yazi PR #4005 reference + path-paste-first deferral); the standalone "Cwd export on quit" idea is relocated/promoted in 3fe93f0.

Reconstructed: the review benchmarks Yazi (~37k stars) feature-by-feature — async scheduler, image preview (out of scope for spyc), Lua plugins (explicit non-goal, ROADMAP.md:447), trash bin (spyc's two-tier graveyard cascading to system trash beats Yazi's single tier) — and is honest about gaps (archive extraction, bulk rename, visual-mode range pick all roadmapped-not-shipped). Its framing reasserts the README/thesis line verbatim: "The file commander is the noun the agent operates on." On DnD it recommends referencing OSC 72 and deferring native impl "until at least one more terminal ships OSC 72," shipping the cheap path-paste fallback first.

#158 executes the review's recommendations the same day: the ROADMAP DnD entry is rewritten to cite Yazi PR #4005 and OSC 72, and grows a drop-action picker design (send to lower pane as image / create new file / add to picks / open in pager) — the "send to lower pane as image" arm called out as "the spyc-shaped one Yazi doesn't have." cwd-export-on-quit is promoted up the roadmap with `Q` retaining no-export semantics. This review is the reference the project says to "re-read before any 'should we copy X?' conversation."

Provenance:
- 6d96e8d (PR #157 docs/yazi-competitive-review, 2026-05-28) — docs/YAZI_COMPETITIVE_REVIEW.md +220 (feature matrix, thesis restatement, OSC 72 analysis); ROADMAP.md DnD entry revised +6/−2. Cites src/state/graveyard.rs, multiple ROADMAP line refs.
- 3fe93f0 (PR #158 docs/roadmap-dnd-cwd-followup, 2026-05-28) — ROADMAP.md DnD entry rewrite + drop-action picker; cwd-export promoted from Additional Ideas; YAZI_COMPETITIVE_REVIEW.md tidy (53-line churn).

<!-- Entry-ID: 01KTMMVPG9JPMQ9N5CGB84KGVG -->

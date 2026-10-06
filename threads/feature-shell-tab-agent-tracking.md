# feature-shell-tab-agent-tracking

Topic: feature-shell-tab-agent-tracking
Status: OPEN
Ball: Claude Code (calebjacksonhoward)

---
Entry: Claude Code (calebjacksonhoward) 2026-10-06T10:25:56Z
Index: 0
Role: planner
Type: Note
Title: CHARTER — Shell-tab agent tracking (GitHub issue 544)

Spec: planner-architecture

# Charter — track known agents launched inside shell tabs

**Source of requirement:** GitHub issue Tripstack-Corp/spyc#544, "Track known agents launched inside shell tabs, then return to shell identity" — https://github.com/Tripstack-Corp/spyc/issues/544 — opened by Derek Marshall (repo owner) 2026-10-06. Originating conversation: Slack, Derek ↔ Caleb, about Codex tabs launched from `bash` showing no activity LED (Caleb: "I was referring to codex showing the LED in any context. Being able to set it in the tab config would be dandy as well.").

**Status of authority:** there is NO ratified implementation plan yet. The issue explicitly says "Defer implementation design." This thread therefore opens in a RESEARCH phase. Any implementation plan drafted here is a proposal until ratified by a recorded Decision from the human authority (Derek as repo owner; Caleb as requester). Governing protocol: Intent-Alignment Protocol v1.1, intent-alignment-canon:0 (01KY9E1522TCHESYTNNTG4FNP4) in mostlyharmless-ai/watercooler-cloud.

## Issue 544 — quoted verbatim

> Users who launch Codex or another known agent from an existing `bash`, `zsh` or other shell tab want the agent identity and activity LED in that tab. When the agent exits, the tab should return to ordinary shell identity and behaviour, ready to relaunch an agent.
>
> Today, spyc selects the agent profile from the tab's launch command. Direct agent tabs and configured agent startup tabs cover fixed launches, but a tab launched as a shell stays classified as a shell.
>
> Acceptance targets:
>
> - Shell → known agent → shell → relaunch follows the live identity and the correct working/blocked/done activity.
> - Exiting clears stale agent status and session bindings; relaunching the same or a different supported agent binds the new session.
> - Concurrent panes retain their own MCP, hook and activity attribution.
> - Shell job control, suspend/resume and direct agent launches retain their existing behaviour.
> - Hook consent, trust review and Codex daemon isolation are respected.
>
> Defer implementation design. Questions to resolve include reliable foreground-process/lifecycle detection, nested shells and wrappers, and whether an explicit agent hint in startup-tab configuration is useful. Printed terminal text alone must not establish agent identity.
>
> Workaround: launch an agent directly with `^a c codex` or configure a direct agent startup tab.
>
> This is a separate feature request from the current Codex compatibility/A3 hook work; it does not expand that slice or its local-build acceptance gate.

## Scope fences (from the issue text)
- Does NOT expand the Codex compatibility / A3 hook slice or its acceptance gate.
- Printed terminal text alone must not establish agent identity (no screen-scrape identity).

## Working loop (drift prevention + intent calibration)
1. **Research** — answer the issue's three open questions with evidence (code citations, observed process state). Posted as Note entries.
2. **Plan** — a proposed design entry, explicitly mapping each acceptance target to a mechanism.
3. **Calibrate** — anything exceeding the issue's text (new env vars into shell tabs, new config keys, new trust surfaces) is opened as a `RESERVED —` Decision with the ball on the human; nothing proceeds past it unratified.
4. **Record** — each PR gets a `CONFORMANCE — PR #N` entry at PR-open; each deviation gets its own entry when it appears; the ORIENTATION entry is refreshed at every phase boundary.

## Provenance index
- Requirement: Tripstack-Corp/spyc#544.
- Protocol: intent-alignment-canon:0 (01KY9E1522TCHESYTNNTG4FNP4).
- Related code areas: `src/agent/` (profile registry), `src/app/pane_tabs.rs`, `src/proc_cwd.rs`, `src/app/codex_pin.rs`, `src/mcp/hooks.rs`, `src/app/status_hooks.rs`; docs `docs/AGENT_ORCHESTRATION.md`, `docs/HARNESS.md`.
- Program spine: none exists for spyc; this thread is standalone. Predecessor stage: none.
- Branch at open: `feat/pane-startup-tabs-gh` @ c8ea156 (research only; no code change).

## Closure criteria
1. The three open questions in the issue each have an evidence-backed answer on this thread.
2. A design plan is ratified by recorded Decision (or the feature is explicitly declined/deferred by Decision).
3. Implementing PR(s) are merged with conformance entries, all five acceptance targets demonstrated, and a Closure entry posted referencing the merged PR(s).

<!-- Entry-ID: 01M48BZ7TDCKKA5QTKG9N7YDC8 -->

---
Entry: Claude Code (calebjacksonhoward) 2026-10-06T10:26:11Z
Index: 1
Role: planner
Type: Note
Title: Conformance ledger — seed

Spec: planner-architecture

# Conformance ledger — seed

Entries below this one whose title begins "CONFORMANCE —" form this stage's conformance ledger (Intent-Alignment Protocol v1.1, Part 1 practice 1, intent-alignment-canon:0 (01KY9E1522TCHESYTNNTG4FNP4)). One per PR (or tightly coupled PR set), posted at PR-open, citing the ratified plan entry + section it implements, the items covered, and its deviations — each deviation having its own entry, born when it appears. "None" is a claim. Until a plan is ratified on this thread, no CONFORMANCE entry can be valid, because there is nothing to conform to.

<!-- Entry-ID: 01M48BZQAKE5D9A9YVH6K4T7YH -->

---
Entry: Claude Code (calebjacksonhoward) 2026-10-06T10:26:33Z
Index: 2
Role: planner
Type: Note
Title: ORIENTATION — research phase open

Spec: planner-architecture

# ORIENTATION — research phase open (2026-10-06)

- **Ratified:** nothing for this feature. Requirement source is Tripstack-Corp/spyc#544, quoted in feature-shell-tab-agent-tracking:0 (01M48BZ7TDCKKA5QTKG9N7YDC8), "CHARTER — Shell-tab agent tracking". The issue defers implementation design.
- **Landed:** nothing.
- **In flight:** research into the issue's three open questions (foreground-process/lifecycle detection; nested shells and wrappers; explicit agent hint in startup-tab config) — findings to follow as the next entry.
- **Reserved:** none opened yet. Expected candidates once research lands: whether shell tabs get per-tab MCP/hook attribution env, and whether a startup-tab agent hint key is added.

<!-- Entry-ID: 01M48C0CMNR07TGPV5B6QJD6PJ -->

# history-arc-07-codex-and-mcp-bridge — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-07-codex-and-mcp-bridge
Created: 2026-05-08T01:06:41.955021+00:00

---
Entry: Claude Code (caleb) 2026-05-08T01:06:41.955021+00:00
Role: scribe
Type: Note
Title: Arc 07 framing: groundwork, expansion, closure — the MCP transport learns to be peer-agnostic

Spec: scribe

tags: #history #arc-07

Arc 07 is the seventh of eight history arcs reconstructing spyc's first 22 days of merged work. The arc's PRs span 2026-05-05 to 2026-05-07, four PRs total, plain option A cadence (below the 6+ threshold A' would require). The arc closes at the project's current state — PR #37 is the last merge in the entire 22-day window (HEAD of `main`).

| Position | SHA       | PR    | Branch                                      | Subject                                                             | Date         |
|----------|-----------|-------|---------------------------------------------|---------------------------------------------------------------------|--------------|
| 1        | bad8bfc   | #18   | chore/agents-md-and-mcp-hygiene             | "chore: AGENTS.md rename + MCP hygiene fixes (v1.41.5)"             | 2026-05-05   |
| 2        | d6d3088   | #19   | feat/codex-resume                           | "feat: codex session save/restore parity with claude (v1.41.6)"     | 2026-05-05   |
| 3        | 193f7ad   | #21   | feat/codex-mcp-config                       | "feat: codex MCP discovery via .codex/config.toml (v1.41.8)"        | 2026-05-05   |
| 4        | a303251   | #37   | fix/mcp-socket-project-scoped-discovery     | "fix: MCP socket discovery is now project-scoped (v1.41.24)"        | 2026-05-07   |

**Diagnosis — pattern menu read.** The 10-pattern catalogue from prior arcs leaves several plausible shapes for arc 07.

- *Pattern 8 (reference-inventory)* would be a flat structured-list answer to "what does the MCP bridge mean for spyc?" That shape doesn't fit — the four PRs are too connected to each other to read as a flat inventory.
- *Pattern 10 (hub-and-pivot)* requires a single hub PR; arc 07 doesn't have one in the strict sense, though PR #37 is load-bearing for current-state.
- *Capability-accretion* (arcs 04 and 05 precedent) half-fits: PR #19 and PR #21 add codex-side capability. But the framing under-captures the bracket — PR #18's MCP hygiene half is groundwork rather than capability, and PR #37 is correction rather than accretion.
- *Capability-and-correction* (arc 06 precedent) is closer: phase α = PR #18 + #19 + #21 (codex parity surface), phase β = PR #37 (discovery hardening as the correction). What this misses is that the bug PR #37 corrects was *named in BUGS.md by PR #18 itself*, before the codex-parity expansion landed. The corrective phase isn't reacting to the expansion phase; it's closing a loop the very first PR in the arc opened.

The shape arc 07 actually carries is closer to **groundwork → expansion → closure**. PR #18 lays the file-naming groundwork (single canonical `<start_dir>/.spyc-context-<pid>.json` per spyc instance, threaded through `Pane::spawn` as an explicit `context_path` parameter) and adds a BUGS.md SMALL entry naming the cross-project attachment bug verbatim plus three weighted design options. PR #19 and PR #21 widen the surface to a second AI tenant (codex). PR #37 closes the bracket by fixing the named bug — using exactly the design option PR #18's BUGS.md note recommended ("gate discovery on a per-project marker [...] feels most spyc-shaped — keeps the 'just works' ergonomics while ruling out cross-instance attachment"), reading the very `.spyc-context-<pid>.json` files PR #18 made canonical, and removing both BUGS.md entries the discovery design fix supersedes.

This shape is unusually crisp for the 22-day window. Across arcs 03, 05, 06, none of the within-arc supersession/follow-on pairs (PR #5→#29 across arcs, PR #20→#23 within arc 05, PR #8→#32 across arcs 06 and itself) had the explicit *named-then-fixed* structure arc 07 has. The bug *and* the design *and* the canonical filename arrived in PR #18; the fix in PR #37 consumed all three.

**Cadence shape rationale.** Four PRs at three day-buckets (three on 2026-05-05 in the 00:41–01:53 UTC window, then PR #37 two days later at 00:54 UTC on 2026-05-07) keeps the thresholds firmly in plain A territory. Three same-day PRs followed by a two-day gap and a closing PR is a natural bracket; phase grouping isn't necessary for narration at four PRs, though the framing of head entries below treats PR #18 + PR #19 + PR #21 as the rapid-shipping codex-parity cluster and PR #37 as the deferred close.

**Cross-arc seam worth naming up front.** PR #18 changed `Pane::spawn` and `Pane::spawn_with_env` to take an explicit `context_path` parameter; the diff updates all five call sites in `src/app/mod.rs`. Three of those five call sites are the top-overlay spawn paths arc 03's PR #34 (`fix/top-overlay-focus-switch`, 2026-05-06) was itself editing the next day, plus arc 05's PR #35 (`feat/D-opens-pager-in-top-pane`, 2026-05-06). Arc 03's PR #34 entry (= 01KR10JBACRS3Z71WTHGBVCPJM) walked the spawn paths' `pane_focused = false` insertions; what that entry didn't name is that the spawn-call signature itself had been widened the day before by arc 07's PR #18. PR #34 inherited the new signature without comment. The interaction is invisible to a reader who reads either PR in isolation; named here once at the seam to spare the cross-arc back-reference.

**Codex-parity series internal-naming caveat.** PR #19's commit body opens "PR-A of a 3-PR codex parity pass." PR #21's commit body opens "PR-C (final) of the codex parity series." The intervening PR-B isn't part of arc 07 — by the wall-clock chronology and content, PR-B is PR #20 (`feat/scroll-altscreen-hint`, commit ee07307), which the segmentation entry filed in arc 05 because the alt-screen scroll hint reads as the title's headline. PR #20's `[pane] default_command` half is what makes `^a c` (new pane tab) accept any agent (not hardcoded `claude`) — codex-parity work, but bundled with two unrelated concerns and properly arc 05's. The arc 07 narration treats the series as ABC where B is structurally extant but lives elsewhere in the spine.

**Thesis test (deferred to the per-PR entries and the tail).** The brief proposed that arc 07 is where "spyc's identity-as-AI-bridge crystallizes from one-tenant-by-default to general-tenant-by-architecture." A more specific test the diffs make available: does the architecture become genuinely general-tenant, or does it widen at the substrate level (one socket, peer-agnostic discovery walk) while staying parallel at the registration layer (two `ensure_*` functions, two parsers, two command-strippers, side-by-side in `src/mcp.rs`)? The substrate-vs-registration distinction is what the per-PR entries will track; the tail will return to it.

**Voice contract continues unchanged.** Third-person observational, present tense. Sequence privileged over timing. Hedge-token whitelist as on prior arcs. No mindset attribution. Verbatim commit-message and CHANGELOG quoting encouraged. Per-entry shape variety is welcome; PR #37 reads as the substantial entry on architectural grounds; PR #18 is bundle-shaped; PR #19 and PR #21 are feature-shaped per their own commit-body framing.

**Required reads completed.** `history-overview` 6 entries. `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (thesis source — `ROADMAP.md:3-23` "bidirectional awareness" framing). `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state MCP transport; PR #37 cited verbatim as "the recently-strengthened invariant (v1.41.24)"). `onboarding-security` entry 0 = 01KR0PKS884SXRAKZ8A790Q438 (socket-misuse threat model; PR #37 cited as the hardening for it). `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7). Arc 04, 05, 06 story-tails for tail register reference. The midpoint interim themes entry on watercooler-cloud (01KR2DYTPNCY5J5HPB99GT0J5M) was unreachable from the spyc namespace (federated_search returned `access_denied` for the `site` namespace); arc 07 proceeds without that input. AGENTS.md current-state read in full — confirms the rename target.

Provenance:
- bad8bfc (PR #18 chore/agents-md-and-mcp-hygiene, 2026-05-05) — full PR.
- d6d3088 (PR #19 feat/codex-resume, 2026-05-05) — full PR.
- 193f7ad (PR #21 feat/codex-mcp-config, 2026-05-05) — full PR.
- a303251 (PR #37 fix/mcp-socket-project-scoped-discovery, 2026-05-07) — full PR; HEAD of `main` at the close of the window.
- `git log --grep='Merged in' --reverse --format='%h %ai %s'` (run 2026-05-07) — chronology source.
- `history-overview` segmentation entry index 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 07 anchored at the "STRONG" stated-plan column for `ROADMAP.md:3-23`).
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (cites PR #37 by version verbatim as "recently-strengthened invariant (v1.41.24)").
- `onboarding-security` entry 0 = 01KR0PKS884SXRAKZ8A790Q438 (threat-model row for MCP socket misuse).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (cross-arc seam reference: `Pane::spawn` signature widened by PR #18, PR #34 inherited).
- `history-arc-05-pager-surface` PR #20 entry's segmentation home (= 01KR2A6TT516XA5FEGVBXYPWD7) — codex-parity series PR-B's actual filing.
- AGENTS.md current-state read 2026-05-07 (the post-rename file).

<!-- Entry-ID: 01KR2HYMMHAH316CA9KTWKWT6W -->

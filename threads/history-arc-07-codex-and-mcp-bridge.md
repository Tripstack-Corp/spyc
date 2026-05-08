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

---
Entry: Claude Code (caleb) 2026-05-08T01:08:23.767867+00:00
Role: scribe
Type: Note
Title: PR #18 (chore/agents-md-and-mcp-hygiene): two unequal halves — a rename and a groundwork bundle

Spec: scribe

tags: #history #arc-07

PR #18 is the first PR in arc 07 and the title is upfront that it bundles. Commit subject reads "chore: AGENTS.md rename + MCP hygiene fixes (v1.41.5)" (commit 1ab45ab, 2026-05-04 20:20:58 -0400; merge bad8bfc, 2026-05-05 00:41 UTC). The commit body opens with a single noun phrase: "Bundled cleanup pass." The diff is 15 files, +120/-53. The two halves don't share files except for `AGENTS.md` itself and the docs that reference it.

The two halves are very unequal in load-bearing weight for what arc 07 becomes, even though the title weights them as equals.

**Half 1 — the rename: `CLAUDE.md` → `AGENTS.md`.** Visible in `git show --stat` as a single line, "CLAUDE.md => AGENTS.md", and a 2-line content diff (the file's H1). Twelve other files update references — `ARCHITECTURE.md`, `CONTRIBUTING.md`, `DESIGN.md`, `ROADMAP.md`, `LAUNCH_PREP.md`, `BUGS.md`, `CHANGELOG.md`, plus three source files with comment updates (`src/sysinfo.rs` test docstrings, `src/app/mod.rs` doc-comments, etc.) and doc references in `BUGS.md`'s ### FIXED ### section that previously cited `CLAUDE.md`. The commit body names the rationale verbatim: "Renamed CLAUDE.md to AGENTS.md (cross-tool standard; recent Claude Code reads both names, behavior unchanged) and updated all references in repo docs and source comments." The CHANGELOG entry under `### Changed` echoes the framing: "Renamed the project instructions file to the cross-tool standard. Recent Claude Code reads both names so behavior is unchanged."

The rename's positioning is the part worth noticing. PR #18 lands at 2026-05-05 00:41 UTC. PR #19's commit body (the very next merge in the arc, 26 minutes later) opens with "PR-A of a 3-PR codex parity pass." PR #21's body opens "PR-C (final) of the codex parity series." The rename happens in the *first* PR of an arc whose next three PRs introduce a second AI tenant (codex). The diff sequence reads as: rename the agent-facing instruction file to the cross-tool name, then add the cross-tool agent. The commit message frames the rename as "cross-tool standard ... behavior unchanged" — a docs hygiene description that reads correctly *for the rename in isolation* but undercaptures what the rename signals in arc context. Whether the rename was in service of the codex-parity expansion or whether it happened to land in the same PR is not narratable from the commit body alone; the chronology is what makes the framing legible.

**Half 2 — the MCP hygiene bundle.** Three changes, all in `src/mcp.rs`, `src/pane/mod.rs`, `src/sysinfo.rs`, and `src/term_title.rs`. Walked in order:

1. **`ensure_mcp_json` no longer panics on a malformed `.mcp.json`.** Pre-PR, the function did `parsed.as_object_mut().unwrap().entry("mcpServers").or_insert_with(|| json!({})).as_object_mut().unwrap().insert(...)`. Two `.unwrap()`s on `.as_object_mut()`. A `.mcp.json` that was valid JSON but had the wrong shape (top-level array, top-level string, `mcpServers: []`) panicked startup. The fix replaces the unwrap chain with a shape-check ladder: `match std::fs::read_to_string` → `match serde_json::from_str` → `let top = parsed.as_object_mut(); let servers = top.and_then(...)` — at any layer, falling back to a `fresh()` closure that overwrites with a clean shape. The CHANGELOG entry under `### Fixed` names the original failure mode directly: "A file that was valid JSON but had the wrong shape (top-level array, top-level string, `mcpServers: []`) would panic instead of being safely overwritten."

2. **`Pane::spawn` / `Pane::spawn_with_env` now require an explicit `context_path` parameter.** The pre-PR signature was `spawn(command, rows, cols, cwd)`; the post-PR signature is `spawn(command, rows, cols, cwd, context_path)`. Inside the body, the `SPYC_CONTEXT` env var was set from `crate::context::context_path(cwd)` — recomputed *per pane* from the pane's cwd. The doc-comment on the new `spawn` method names what was wrong: "App writes one canonical `<start_dir>/.spyc-context-<pid>.json`, but a pane can spawn in any subdir, and recomputing from `cwd` would point at a path nobody writes." The CHANGELOG entry quotes the failure mode at user-visible level: "When a pane spawned outside `start_dir` (e.g. in `PROJECT_HOME` or a subdir), Claude Code's direct-mode MCP fallback read a path nobody writes." All five call sites in `src/app/mod.rs` are updated in-place; the pane-fallback path, the `;cmd` overlay path, the prompt-completion overlay path, the `^a c` new-pane path, and the D-key file-open overlay path each take `&self.context_path`.

3. **`term_title::wrap_*_tmux` tests now hold `env_test_lock`.** Two unit tests both mutated process-global `TMUX` and could race under parallel execution. Both now acquire `crate::state::env_test_lock()`. The CHANGELOG names this as "same flake family as the state-module tests."

**Half 2's hidden third half — the BUGS.md note.** PR #18 also adds a 13-line `### SMALL ###` entry to `BUGS.md` whose existence is what makes arc 07 *legible* as a bracketed structure. The note is added at the top of the SMALL bucket and reads verbatim:

> MCP socket discovery can attach to the wrong spyc instance. When `$SPYC_MCP_SOCK` is unset (e.g. `claude` launched outside spyc's pane, env didn't propagate, or the local `.mcp.json` was suppressed by enterprise managed-mcp), `discover_live_socket` in `src/mcp.rs:153` returns the first connectable `~/.local/state/spyc/mcp-*.sock` it finds — could be any other spyc on the host, including another user's. Conflicts with the multi-instance isolation model. Design fixes worth weighing:
> (a) require explicit `$SPYC_MCP_SOCK`, no discovery fallback;
> (b) gate discovery on a per-project marker (e.g. only accept sockets whose context file's project_root matches the caller's cwd);
> (c) include user/uid in the socket path. Option (b) feels most spyc-shaped — keeps the "just works" ergonomics while ruling out cross-instance attachment.

The commit body names this addition explicitly: "BUGS.md gains a top-of-list note for the broader MCP discovery design issue (`discover_live_socket` can attach to the wrong instance when `SPYC_MCP_SOCK` is missing) — to be addressed separately." Two days later, PR #37 implements exactly option (b) and removes this entire note. The "to be addressed separately" framing reads as load-bearing in retrospect — the bug is named, the design is recommended, the fix is deferred to its own PR. PR #37 picks up the deferred work.

**The MCP hygiene bundle's structural role for arc 07.** The Pane-spawn `context_path` change (item 2 above) is the load-bearing one for what comes later. PR #18 establishes that *App writes one canonical `<start_dir>/.spyc-context-<pid>.json`* — one file per spyc instance, at a known path, threaded explicitly through every pane spawn. Two days later, PR #37's discovery walk consumes exactly those files: the new `discover_live_socket(caller_cwd)` walks `caller_cwd` toward the filesystem root looking for `.spyc-context-<pid>.json` markers; PR #37's `read_context_pids_in_dir` parses the same filename PR #18 made canonical. If PR #18 had not threaded `context_path` through `Pane::spawn`, the markers PR #37 walks for would not reliably exist at the directory PR #37 walks from. The mechanical link is invisible from either commit message in isolation; it lives in the file's role across the two diffs.

**Drift findings flagged for the insight layer.**

- The PR title weights the rename and the MCP hygiene as equal halves ("AGENTS.md rename + MCP hygiene fixes"); the diff weights the MCP hygiene at 80+% of the substantive changes (the rename touches mostly docs and a single line per source file) and the codex-parity follow-on PRs reveal the rename's structural role in retrospect. The title formula doesn't capture the bracket the bundle is opening.
- The commit subject prefix is `chore`; the bundle includes one panic-fix (`ensure_mcp_json` shape-safety) and one direct-mode-fallback fix (`Pane::spawn` `context_path`) that would land naturally under `fix:`. The CHANGELOG places both under `### Fixed`. The chore-vs-fix prefix-vs-section split is a small-scale drift the framing register has now seen recur across arcs (PR #20 in arc 05 bundled three concerns under `feat/`; PR #18's `chore/` carrying user-visible fixes is the same shape from a different prefix).
- The BUGS.md note PR #18 adds is unusual for the 22-day window: it pre-names a design issue, weighs three solution options with explicit framing language ("Option (b) feels most spyc-shaped"), and the next-arc-PR closes both the note and a related older entry using exactly option (b). No other window-PR exhibits this pattern as cleanly. Captured for the insight layer's "pre-named-then-closed" recurrence reading.
- Three of the five `Pane::spawn` call-site updates are on overlay-spawn paths arc 03's PR #34 (next day, 2026-05-06 23:37) and arc 05's PR #35 (2026-05-06 23:53) edit further. Whether either later PR's diff-context check would surface PR #18's signature-widening as a bundling note is determinable only from the back-references in those entries; arc 03's PR #34 entry (= 01KR10JBACRS3Z71WTHGBVCPJM) walks the touch points without naming the parameter widening that ships from arc 07. Cross-arc seam already flagged in arc 07's framing entry.

Provenance:
- bad8bfc (merge PR #18 chore/agents-md-and-mcp-hygiene, 2026-05-05 00:41 UTC) — full PR.
- 1ab45ab (source commit on chore/agents-md-and-mcp-hygiene branch, 2026-05-04 20:20:58 -0400) — commit message body source.
- `git show --stat bad8bfc` — 15 files changed, +120/-53; rename appears as "CLAUDE.md => AGENTS.md".
- `git diff bad8bfc^1..bad8bfc^2 -- CHANGELOG.md` — `### Changed` (rename) and `### Fixed` (three items: `.mcp.json` panic, `Pane::spawn` context_path, TMUX env race).
- `git diff bad8bfc^1..bad8bfc^2 -- BUGS.md` — 13-line `### SMALL ###` entry added at top; quoted verbatim above.
- `git diff bad8bfc^1..bad8bfc^2 -- src/mcp.rs` — shape-check ladder for `ensure_mcp_json`, +41/-21 lines (delta +20).
- `git diff bad8bfc^1..bad8bfc^2 -- src/pane/mod.rs` — `Pane::spawn` / `spawn_with_env` signatures gain `context_path: &Path`; doc-comment quoted above.
- `git diff bad8bfc^1..bad8bfc^2 -- src/app/mod.rs` — five call-site updates threading `&self.context_path`.
- `git diff bad8bfc^1..bad8bfc^2 -- src/sysinfo.rs src/term_title.rs` — comment updates for the rename; `env_test_lock` adoption.
- `history-arc-07-codex-and-mcp-bridge` framing entry = 01KR2HYMMHAH316CA9KTWKWT6W.
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (cross-arc seam: same overlay spawn paths).
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (cross-arc seam: D-key overlay spawn path).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state cites `.spyc-context-<pid>.json` as the marker discovery walks for; PR #18 made the marker canonical).

<!-- Entry-ID: 01KR2J1R3HXNZPAHE9118BGBQJ -->

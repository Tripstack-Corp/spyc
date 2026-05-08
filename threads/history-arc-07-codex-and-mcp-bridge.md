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

---
Entry: Claude Code (caleb) 2026-05-08T01:09:58.241939+00:00
Role: scribe
Type: Note
Title: PR #19 (feat/codex-resume): a shared data model, parallel parsers, and the asymmetries the CLIs force

Spec: scribe

tags: #history #arc-07

PR #19 is the second move in arc 07 and the first to introduce codex as a peer of claude in the session-resume surface. Commit subject reads "feat: codex session save/restore parity with claude (v1.41.6)" (commit 3cbcd3f, 2026-05-04 20:53:15 -0400; merge d6d3088, 2026-05-05 01:07 UTC — twenty-six minutes after PR #18 merged). The commit body opens with "PR-A of a 3-PR codex parity pass." Diff: 8 files, +335/-61. The two source files carrying weight are `src/state/sessions.rs` (+179/-26) and `src/app/mod.rs` (+176/-26).

The PR is the right vantage to answer the brief's parity question — *shared abstraction or parallel implementation?* — because session-resume is the surface where the data model is authored fresh on this PR, the parsers branch by agent, and the restore-spawn paths use different mechanics for fundamental CLI reasons. The diff contains both shapes: shared at one layer, parallel at the next.

**The shared layer — a peer-agnostic data model.** Pre-PR, `SavedTab` carried `claude_session_id: Option<String>` and `claude_session_name: Option<String>` — claude-shaped fields with the agent baked into the name. PR #19 introduces a new `AgentKind` enum (`Claude / Codex / Other`) with `serde(rename_all = "lowercase")` for stable on-disk strings, derives `Default` to `Other`, and renames the two fields to `agent_session_id` / `agent_session_name`. The renames carry serde aliases (`alias = "claude_session_id"`, `alias = "claude_session_name"`) so older saves deserialize without migration. A new `SavedTab::effective_kind()` const-method infers `Claude` for legacy saves that have `agent_session_id` set but no `agent_kind` field — the only resume case before codex support. Two unit tests pin the back-compat: `effective_kind_infers_claude_for_legacy_saves` constructs the JSON shape an older save would produce and asserts `tab.agent_kind == AgentKind::Other && tab.effective_kind() == AgentKind::Claude`; `effective_kind_passes_through_explicit_value` asserts the explicit-Codex case round-trips.

The data-model decision — naming the field `agent_*` rather than introducing a sibling `codex_*` pair — is the shared-substrate move. The doc-comment on `AgentKind` names the two CLIs' resume mechanics in the same breath: "Drives session-save and resume-on-restore behavior — claude uses a UUID-or-name token plus `/resume` over stdin (CLI flag is regression-prone), codex uses `codex resume <UUID>` directly." The enum carries the policy, not the implementation.

**The parallel layer — two parsers, two strippers, two restore paths.** Above the shared `AgentKind`, the parsing and command-handling functions are mirrored side-by-side rather than abstracted. Five concrete examples:

1. *Two banner parsers in `src/state/sessions.rs`*. The pre-PR `extract_resume_token` becomes `extract_claude_resume_token` (rename). A new `extract_codex_resume_token` is added directly below it. Both scan `lines.iter().rev()` for a banner and return `Option<String>`; the bodies do different things — claude's parser accepts UUIDs *or* thread-name tokens (`claude --resume saffron-cumin`), codex's parser requires the result to pass `is_uuid()` (`codex_extractor_requires_uuid` test asserts the guard). The doc-comment on the codex parser names the asymmetry: "Returns just the UUID — codex doesn't have thread-name resume tokens."

2. *Two command-strippers in `src/app/mod.rs`*. The pre-PR `command_without_resume(cmd)` strips claude's `--resume <token>` from a saved command. A new `command_without_codex_resume(cmd)` is added directly below it; it strips codex's `resume [args]` subcommand and any of its flags (`--last`, `--all`, `--include-non-interactive`). The doc-comment names the parallel: "Mirrors `command_without_resume` for claude. The id we'll resume to is stored separately in `agent_session_id`." Two functions, one shared rationale.

3. *Two restore mechanics, branched at the spawn site.* The post-PR restore loop matches on `(kind, tab.agent_session_id.as_deref())`:
   - `(Claude, _) => (command_without_resume(&tab.command), None)` — spawns fresh, then queues a `pending_resume_send` typed-stdin send.
   - `(Codex, Some(sid)) => (format!("{base} resume {sid}"), Some(sid.to_string()))` — spawns `codex resume <UUID>` directly.
   - `(Codex, None) => (format!("{base} resume --last"), None)` — falls back to codex's own most-recent picker.
   - `(Other, _) => (tab.command.clone(), None)`.
   The inline comment on the branch names the asymmetry verbatim: "Codex restores by spawning `codex resume <UUID>` directly — the CLI flag works, no `/resume` stdin dance needed. Claude has a regression on the CLI flag (crashes at mount with non-empty initialMessages), so we always spawn fresh and type `/resume <sid>` once it has settled."

4. *Two dispatch helpers* — `is_codex_command(cmd)` is added next to the existing `is_claude_command(cmd)`; both check the first whitespace-separated token against a fixed string and a path-suffix. A single `detect_agent_kind(cmd)` umbrella branches between them: `if Self::is_claude_command(cmd) { Claude } else if Self::is_codex_command(cmd) { Codex } else { Other }`. The umbrella-over-pair shape is the smallest unit of shared substrate above the parallel implementations.

5. *Picker tooltip grouping*. The session-picker's per-tab tooltip line, pre-PR, was a flat `claude: name (1234abcd), other-claude-name (5678abcd)` string. Post-PR, the tooltips group by kind: `claude:foo (12345678), codex:abcdef12`. The format-by-kind branch is per-tab in the iterator (`AgentKind::Claude => match &t.agent_session_name { ... }; AgentKind::Codex => format!("codex:{short_id}"); AgentKind::Other => return None`), not abstracted into a per-kind formatter trait.

**The asymmetries the CLIs themselves force.** Three are quoted verbatim from inline comments in the diff and worth pulling out as a list because they're the seams a future maintainer hits when wiring a *third* peer (whether or not that ever happens):

- *UUID-only vs UUID-or-name*: "Codex doesn't expose a display name; UUID is what `codex resume <UUID>` consumes."
- *CLI-flag-works vs CLI-flag-regression*: "Claude has a regression on the CLI flag (crashes at mount with non-empty initialMessages), so we always spawn fresh and type `/resume <sid>` once it has settled. Codex doesn't [have that regression]."
- *No-id fallback*: codex has `codex resume --last` to pick the most-recent session for the cwd; claude has no documented equivalent in the diff. The `(Codex, None)` arm uses `--last`; the `(Claude, ?, None)` arm doesn't exist as a discrete fallback because the resolver runs before save.

**The parity question, answered for PR #19.** The data model layer is genuinely shared (one enum, one effective-kind method, two-fields-renamed-not-duplicated). The parsing and dispatch layers are parallel (two extractors, two strippers, two `is_*_command` checks, branched-by-kind restore). The shared substrate emerges naturally where the *shape* is the same (a session has one resume-id and one optional name; the kind is metadata); the parallel pattern emerges where the *mechanics* are different (the banner format, the CLI verb, the resume mechanism). What the PR doesn't do — and the doc-comments don't hide — is abstract the parsers behind a `trait AgentBannerParser` or the dispatch behind `trait AgentRestore`. Two functions side-by-side, one doc-comment naming each as "mirroring" the other, is the shape.

**Cross-arc trace — codex's exit-banner pattern**. The codex parser's tolerance loop is worth a beat for a future maintainer reading the surface. The `extract_codex_resume_token` function does *not* `strip_prefix("codex resume ")` after a fixed phrase; it does `if let Some(idx) = trimmed.find("codex resume ")` anywhere on the line, then `rest.split_whitespace().next()?.trim()` on what follows. The doc-comment names why: "Look for `codex resume <token>` anywhere on the line so we tolerate the leading 'To continue this session, run ' prefix and any trailing color-reset bytes the TUI may have left on the same render line." The TUI color-reset tolerance is the same shape as PR #5's gap-analysis suspect §3 (synchronized-output / mode-2026 tearing) — neither is the same bug, but both are TUI-byte-tolerance moves at the parse boundary. The pattern of "expect garbage from a TUI's stdout, find the marker substring rather than match the full line" is what the codex extractor inherits from the shape of stdout-as-protocol that arc 02 catalogued.

**Drift findings flagged for the insight layer.**

- The branch is `feat/codex-resume`; the commit subject reads `feat:`; CHANGELOG buckets under `### Added`. Three-way alignment. Positive-control row.
- The commit body explicitly numbers the PR ("PR-A of a 3-PR codex parity pass") and the close PR (#21) confirms ("PR-C (final)"). The intervening PR-B is not in arc 07 — the segmentation entry filed it in arc 05 (PR #20, `feat/scroll-altscreen-hint`, with `[pane] default_command` as the codex-parity-relevant half bundled under an alt-screen-hint headline). Cross-arc structural reach noted in arc 07's framing.
- The legacy field renames (`claude_session_id` → `agent_session_id`) carry serde aliases. The aliases are still in the diff after PR #21 and unchanged through PR #37 (verified at HEAD). The "rename + alias for back-compat" pattern is recurring for fields the maintainer expects on disk (sessions JSON files survive across versions); whether the aliases ever get retired is determinable only from a future version's deletion.
- The umbrella-over-pair shape (`detect_agent_kind` over `is_claude_command` + `is_codex_command`) is the smallest parametric abstraction the diff introduces. It does not extend through to the parsers or restore paths. A reader expecting parametric MCP-peer-as-trait machinery from a "codex parity pass" doesn't find it; the parity is policy-level (`AgentKind` enum), not interface-level (no trait).

Provenance:
- d6d3088 (merge PR #19 feat/codex-resume, 2026-05-05 01:07 UTC) — full PR.
- 3cbcd3f (source commit, 2026-05-04 20:53:15 -0400) — commit body source ("PR-A of a 3-PR codex parity pass").
- `git show --stat d6d3088` — 8 files changed, +335/-61.
- `git diff d6d3088^1..d6d3088^2 -- src/state/sessions.rs` — `AgentKind` enum, field renames with `serde alias`, `effective_kind()` const-method, `extract_codex_resume_token`, 5 new tests.
- `git diff d6d3088^1..d6d3088^2 -- src/app/mod.rs` — `is_codex_command`, `detect_agent_kind`, `command_without_codex_resume`, branched restore loop.
- Inline doc-comments quoted above are from the post-merge state of `src/state/sessions.rs` and `src/app/mod.rs` at this commit.
- `history-arc-07-codex-and-mcp-bridge` framing entry = 01KR2HYMMHAH316CA9KTWKWT6W.
- `history-arc-07-codex-and-mcp-bridge` PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (`agent` field-rename rationale aligns with the rename-to-cross-tool-naming move PR #18 makes for the docs file).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (PR #5's gap-analysis suspect §3, mode-2026 tearing — pattern reference for stdout-as-protocol byte tolerance).

<!-- Entry-ID: 01KR2J4M8BP0J3WKNED6EMSV1Y -->

---
Entry: Claude Code (caleb) 2026-05-08T01:11:50.009988+00:00
Role: scribe
Type: Note
Title: PR #21 (feat/codex-mcp-config): two ensure_ functions, one socket — the parity question, answered at the registration layer

Spec: scribe

tags: #history #arc-07

PR #21 closes the codex-parity series. Commit subject reads "feat: codex MCP discovery via .codex/config.toml (v1.41.8)" (commit 162fd81, 2026-05-04 21:28:22 -0400; merge 193f7ad, 2026-05-05 01:53 UTC — forty-six minutes after PR #19 merged, two hours and thirteen minutes after PR #18). The commit body opens "PR-C (final) of the codex parity series." Diff: 10 files, +324/-19. The substantial source change is concentrated in one file: `src/mcp.rs` gains 246 lines.

PR #21 is the right vantage to answer the brief's parity question at the *registration layer* — the layer where each AI peer's discovery file gets written. PR #19 answered the question at the data-model layer (shared `AgentKind`, parallel parsers/strippers). PR #21 answers it at the file-format layer.

**The single new top-level function: `ensure_codex_config_toml(dir, takeover_allowed)`.** The doc-comment opens by naming the parallel verbatim: "Codex's equivalent of `ensure_mcp_json`. Writes a stdio MCP entry for spyc into `<dir>/.codex/config.toml` so the codex CLI discovers us automatically, the same way claude does via `.mcp.json`." The function lives directly below `ensure_mcp_json` in `src/mcp.rs`. The two functions are siblings in source-order; nothing factored out.

The structural mirror is exact at the body level. The internal closure pattern is the same: a `build_entry()` closure builds the spyc-specific TOML table; a `fresh()` closure produces the whole-file fallback when the existing file is malformed; a `match std::fs::read_to_string → match toml::from_str → top.as_table_mut → entry.as_table_mut` shape-check ladder maps directly onto PR #18's `serde_json::from_str → top.as_object_mut → entry.as_object_mut` ladder. The shape-safety pattern PR #18 *retrofitted* into `ensure_mcp_json` (the pre-existing function) is the pattern PR #21 *ships pre-baked* into `ensure_codex_config_toml` (the new function). Two days, two formats, one shape-safety rule applied symmetrically.

**The TOML schema verbatim from the doc-comment.** The codex side writes `<project>/.codex/config.toml` with this shape:

```toml
[mcp_servers.spyc]
command = "spyc"
args = ["--mcp"]

[mcp_servers.spyc.env]
SPYC_MCP_SOCK = "/Users/x/.local/state/spyc/mcp-12345.sock"
```

The doc-comment names the JSON-vs-TOML mapping: codex's TOML schema is `[mcp_servers.<name>]` with `command`, `args`, and `env` keys for stdio servers, "parallel to claude's `.mcp.json` shape." Claude's `.mcp.json` shape (visible from the existing `ensure_mcp_json`) is `mcpServers.spyc` with the same three keys. Same shape, two serializations.

**The shared substrate sits below both `ensure_*` functions.** Both registration files carry `SPYC_MCP_SOCK` in their `env` block (claude's `.mcp.json`'s `mcpServers.spyc.env.SPYC_MCP_SOCK`; codex's `.codex/config.toml`'s `[mcp_servers.spyc.env].SPYC_MCP_SOCK`). Both `command` keys re-exec `spyc --mcp`. The `spyc --mcp` stdio proxy connects to a single Unix socket per spyc instance — the same socket regardless of which agent's registration file directed the proxy. The commit body names the property explicitly: "The registration re-execs `spyc --mcp` and forwards through to the same Unix socket as the claude side, so a single MCP server backs both agents."

This is the substrate layer the registration parallelism sits on top of. Two registration files, one socket, one server. The `--mcp` re-exec is the parametric layer that *makes* the substrate single-server: whichever file the agent reads, the proxy converges on the same socket.

**The takeover-prompt seam — `detect_existing_spyc_codex` and a one-line OR.** A second new function in `src/mcp.rs` mirrors `detect_existing_spyc` for the codex side: `detect_existing_spyc_codex(dir)` reads `<dir>/.codex/config.toml`, extracts `mcp_servers.spyc.env.SPYC_MCP_SOCK`, returns the PID-from-sock-path if the socket connects and isn't our own. It's the codex-parser version of the claude-parser detection. The doc-comment names the role: "Mirrors `detect_existing_spyc` for the codex side; used by startup so a single takeover prompt covers both claude and codex."

Inside `src/main.rs`, the takeover-prompt entry-point gains a one-line OR:

```rust
let Some(old_pid) =
    mcp::detect_existing_spyc(&cwd).or_else(|| mcp::detect_existing_spyc_codex(&cwd))
else {
    return true;
};
```

The inline comment above the change names the seam: "Either claude's `.mcp.json` or codex's `.codex/config.toml` can hold a stale-by-PID spyc entry; check both so the takeover prompt fires regardless of which agent the prior instance had configured." A single startup prompt now covers both peers; the per-peer detectors are the parallel pair, the prompt is the shared substrate.

**Inside `src/app/mod.rs` — the wiring at startup.** A 22-line block is added directly below the existing `ensure_mcp_json` call. The new block calls `ensure_codex_config_toml(&self.state.listing.dir, takeover_allowed)`, matches on the same `McpConfigStatus` enum (`TookOver { old_pid }`, `SkippedTakeover { old_pid }`, etc.), and flashes informational messages prefixed `codex MCP:` instead of plain `MCP:`. The two calls share `takeover_allowed` from a single startup prompt, so the user is never prompted twice.

The block's inline comment names two of the three asymmetries-versus-claude PR #21 carries:

> Codex equivalent: write `.codex/config.toml` so the codex CLI discovers spyc's MCP server the same way claude does. Both agents share the same socket; the writer just registers a stdio entry that re-execs `spyc --mcp` to proxy. Failures here flash but don't gate startup — codex isn't required. Enterprise-flavored statuses are claude-specific; codex shouldn't return them, but if it ever does we treat them as a no-op.

The "codex isn't required" framing is the policy choice that makes the codex-side `Err` arm fall through to a flash-and-continue rather than gating startup. Compare claude's: claude side similarly flashes errors. The two sides are symmetric on error-handling but the comment makes clear the *gating* asymmetry — codex isn't required for startup.

**The asymmetries the registration files force.** Three are visible in the code or doc-comments and worth pulling out for the parity catalogue:

1. *User-scope file is read but not written.* The doc-comment names this directly: "Codex reads both `~/.codex/config.toml` (user-scope) and `<cwd>/.codex/config.toml` (project-scope); we only ever write the project file to mirror claude's project-scoped behavior and avoid touching the user's main config." Claude's `.mcp.json` is project-scope only by convention (`<cwd>/.mcp.json`); codex has both scopes, and spyc respects the user-scope file as read-only.

2. *Enterprise hooks are claude-only.* From the commit body: "Enterprise policies are claude-specific; codex has no equivalent `managed-mcp.json` hook so those branches don't apply." `ensure_mcp_json` honors `deniedMcpServers` / `allowedMcpServers` from `managed-settings.json` and suppresses the `.mcp.json` write if a Jamf-deployed `managed-mcp.json` already names a `spyc` server. `ensure_codex_config_toml` carries no equivalent branches because codex has no equivalent enterprise config surface.

3. *Parent-directory creation.* `ensure_codex_config_toml` does `if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }` because `.codex/` is a subdirectory and may not exist. `ensure_mcp_json` writes `.mcp.json` directly into `dir`, no parent-creation step needed. A small implementation asymmetry that follows from the file-layout decision.

**The four new tests** live in the same `tests` module as the JSON-side tests. They mirror the JSON tests' shape-safety assertions point-by-point: `codex_config_writes_fresh_when_missing` (parallel to a JSON `mcp_json_writes_fresh_when_missing` if it existed), `codex_config_preserves_other_servers` (asserts a pre-existing `[mcp_servers.other]` from another tool survives the splice), `codex_config_fresh_rewrite_on_malformed_input` (asserts a top-level non-table `not_a_section = 1` falls back to a clean rewrite), `codex_config_rewrites_completely_invalid_toml` (asserts `}}}}{{{ this is not toml` doesn't crash). The malformed-input test's source-comment names the rule: "Top-level array (not a table) must not panic — mirror the mcp.json shape-check fix from v1.41.5." The cross-PR reference to v1.41.5 — PR #18's MCP hygiene half — is verbatim in test source.

**The parity question, answered for PR #21.** Registration is parallel: two `ensure_*` functions, two parsers, two file formats, two test sets, side-by-side in `src/mcp.rs`. Substrate is shared: one socket, one `spyc --mcp` proxy, one `McpConfigStatus` enum, one takeover prompt that ORs both detectors. The doc-comments name this distinction directly — "implementation mirrors `ensure_mcp_json`" at the function level; "single MCP server backs every supported agent" at the substrate level (verbatim from current AGENTS.md, post-PR-#21).

What the PR doesn't do, and the doc-comments don't hide, is factor a `trait DiscoveryFileWriter` or generic-over-format machinery. The TOML and JSON paths share no traits; the shape-check ladder is repeated; the build-entry closures are duplicated with format-specific construction. Two functions side-by-side, one doc-comment naming each as the other's mirror, is the shape arc 06's tail (= 01KR2GYQPQRX08SV980SPHHZ80) named for harpoon and quickselect ("two parallel-shaped pickers ... different surface, different dispatch, different action set"); arc 04's tail (= 01KR13CJ5XS5VREYA4741JHDSQ) named for the parser-rule asymmetry in `parse_porcelain_statuses`. PR #21 ships the pattern at the registration layer.

**Drift findings flagged for the insight layer.**

- The branch is `feat/codex-mcp-config`; the commit subject reads `feat:`; CHANGELOG buckets under `### Added`. Three-way alignment.
- The codex-side test `codex_config_fresh_rewrite_on_malformed_input` source-comment cross-cites the JSON-side fix PR #18 retrofitted ("mirror the mcp.json shape-check fix from v1.41.5"). The cross-PR reference *inside test source* is unusual for the 22-day window — the only other case the framing register has seen is PR #14's routing-fix immediately following PR #13's graveyard-undo (arc 08). PR #14 referenced PR #13 in commit message; PR #21's reference lives in test code-comment. Two recurrences across the 22-day window of "the next PR cites the prior PR's mechanic by version number."
- `FEATURES.md` rewrites the MCP-server section to name both peers in the heading: pre-PR "MCP server (Claude integration)" → post-PR "MCP server (Claude + Codex integration)." The change is one line in the H2; the section body grows from one paragraph to a structured list with bullets for each peer's file. The framing register has now seen FEATURES.md grow this way three times in arcs 04, 05, 06; arc 07's pattern is the same.
- The PR-C closure of the 3-PR codex-parity series adds 246 lines to `src/mcp.rs` (the file at `src/mcp.rs:1` was 2154 lines per the architecture seed; post-PR it's roughly 2400). The 9087-line `src/app/mod.rs` is well-known as "the big file" (AGENTS.md:38); `src/mcp.rs` at 2400+ post-arc-07 is the second-largest source file in the project. Whether the codex-parity work will eventually motivate factoring registration-file-writing into a sub-module is not narratable from the diffs in arc 07.

Provenance:
- 193f7ad (merge PR #21 feat/codex-mcp-config, 2026-05-05 01:53 UTC) — full PR.
- 162fd81 (source commit, 2026-05-04 21:28:22 -0400) — commit body source ("PR-C (final) of the codex parity series").
- `git show --stat 193f7ad` — 10 files changed, +324/-19; src/mcp.rs +246/-0.
- `git diff 193f7ad^1..193f7ad^2 -- src/mcp.rs` — `ensure_codex_config_toml`, `detect_existing_spyc_codex`, doc-comments quoted above, 4 new tests.
- `git diff 193f7ad^1..193f7ad^2 -- src/main.rs` — `detect_existing_spyc(&cwd).or_else(|| detect_existing_spyc_codex(&cwd))` — quoted above.
- `git diff 193f7ad^1..193f7ad^2 -- src/app/mod.rs` — 22-line block calling `ensure_codex_config_toml`; inline comment quoted above.
- `git diff 193f7ad^1..193f7ad^2 -- FEATURES.md` — H2 change; structured list of two peers with file paths.
- `git diff 193f7ad^1..193f7ad^2 -- CHANGELOG.md` — `### Added` entry quoted from the file.
- AGENTS.md (post-PR, line 9 references both peers; line 23 references "spice-themed name" for codex resume too).
- `history-arc-07-codex-and-mcp-bridge` framing entry = 01KR2HYMMHAH316CA9KTWKWT6W.
- `history-arc-07-codex-and-mcp-bridge` PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (shape-safety ladder PR #18 retrofit; PR #21's test cross-cites v1.41.5).
- `history-arc-07-codex-and-mcp-bridge` PR #19 entry = 01KR2J4M8BP0J3WKNED6EMSV1Y (parallel-vs-shared at data-model layer).
- `history-arc-04-git-integration` story-tail = 01KR13CJ5XS5VREYA4741JHDSQ (parser-rule asymmetry as a parallel-implementation precedent).
- `history-arc-06-input-and-overlays` story-tail = 01KR2GYQPQRX08SV980SPHHZ80 (parallel-shaped pickers as a precedent for the parallel-at-registration pattern).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state surface description names "Two transports share dispatch — `spyc --mcp` stdio proxy ... and the in-process socket listener" — the shared substrate above which PR #21 sits).

<!-- Entry-ID: 01KR2J81DHNG4K8NHFVN0XMD1M -->

---
Entry: Claude Code (caleb) 2026-05-08T01:14:15.159449+00:00
Role: scribe
Type: Note
Title: PR #37 (fix/mcp-socket-project-scoped-discovery): the bracket closes — bug, fall-back, cleanup, and a 21-line policy comment

Spec: scribe

tags: #history #arc-07

PR #37 is the closing move of arc 07 and the last merge in the entire 22-day window. Commit subject reads "fix: MCP socket discovery is now project-scoped (v1.41.24)" (commit 80797b8, 2026-05-06 20:35:08 -0400; merge a303251, 2026-05-07 00:54 UTC). Diff: 5 files, +246/-39, with `src/mcp.rs` carrying +229/-23 of that. The doc-comment block on the new `discover_live_socket(caller_cwd)` function is 21 lines. The function it replaces was 16 lines.

PR #37 is also the only PR in arc 07 whose architectural significance is named *outside the arc*. The `onboarding-architecture` seed (entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ) cites it verbatim as "Recently-strengthened invariant (v1.41.24)" and walks the project-scoping rule and the read-only fall-back. The `onboarding-security` seed (entry 0 = 01KR0PKS884SXRAKZ8A790Q438) cites it as the hardening for the MCP socket-misuse threat row. Arc 07's per-PR entry narrates how the project arrived at that invariant; the seed entries narrate what the invariant *is* at current state. The two views complement; arc 07 is genesis-state, the seeds are post-state.

**Three findings, walked against the diff in commit-body order.**

**1. The bug fixed: cross-project (and cross-user) MCP attachment.** The pre-PR `discover_live_socket()` function (16 lines, no parameters) opened `~/.local/state/spyc/mcp-*.sock` (with a `/tmp` fallback when `$HOME` was unset), iterated `read_dir` matches, and returned the first one that connected. The CHANGELOG entry under `### Fixed` quotes the failure verbatim:

> Previously, when `$SPYC_MCP_SOCK` wasn't set (claude launched outside spyc's pane, env didn't propagate, or enterprise managed-mcp.json suppressed the local `.mcp.json`), `discover_live_socket` scanned every `~/.local/state/spyc/mcp-*.sock` on the host and returned the first one that connected — a claude in project A could silently attach to a spyc running in project B (or, with `$HOME` unset, even another user's spyc on a shared host). Wrong-context tools and file paths flowed through, with no log line saying so.

Three concrete failure paths for `$SPYC_MCP_SOCK` going missing are named: external-launch (`claude` started outside spyc's pane), env-propagation drop, enterprise managed-mcp suppression. The `$HOME` unset case widens the threat to cross-user-on-shared-host: with `unwrap_or_else(|_| "/tmp".into())`, a claude on a shared host with no `$HOME` would have walked `/tmp` and connected to *any* user's stale-but-live spyc socket. The `onboarding-security` seed names this as "MCP socket misuse — the per-PID Unix socket exposes tool calls to whatever process can read `~/.local/state/spyc/mcp-<pid>.sock`. FS perms gate it; an attacker who's already running as your user can talk to any of your spyc instances." Pre-PR-#37, the FS-perms gate was load-bearing; the discovery scan was the bypass. Post-PR-#37, project-scoping is the second-level gate: even an attacker with FS-perms access to all sockets can't trick a stdio proxy into connecting to a stranger's spyc by manipulating the *caller's* cwd, because no `.spyc-context-<pid>.json` ancestor walk will reach a stranger's spyc.

**2. The fall-back path: read-only direct mode.** With no `.spyc-context-<pid>.json` match anywhere in the ancestor walk, `discover_live_socket` returns `None`, and the `run` function's call site falls through to `run_direct(project_root)`. The doc-comment in the file's top section names the order verbatim:

```
/// 1. `$SPYC_MCP_SOCK` (set in `.mcp.json`'s `env` block) — exact match
/// 2. Project-scoped discovery: walk `caller_cwd` upward looking for
///    `.spyc-context-<pid>.json` markers; map those PIDs to live
///    sockets. Refuses cross-project attachment (a spyc running in
///    a different project tree can no longer be picked up).
/// 3. Falls back to read-only direct mode if nothing matches.
```

The `discover_live_socket` doc-comment names the user-visible policy verbatim:

> If no match anywhere, return None. The stdio proxy falls through to read-only direct mode instead of attaching to the wrong host.

The "instead of attaching to the wrong host" framing is the policy: silence-over-wrong-attachment. A claude that can't find its spyc gets read-only-direct (a degraded surface that reads context files but can't mutate the TUI) rather than getting a *different* spyc's full surface. The degradation is intentional — the doc-comment quotes the safety rule directly: "rules that out while keeping the 'just works' ergonomic — as long as it's launched somewhere inside the spyc instance's tree."

**3. The stale-socket cleanup tightening.** Pre-PR, the cleanup was unconditional: any `UnixStream::connect` failure triggered `std::fs::remove_file(&sock)`. Post-PR, the deletion is gated on the IO error kind:

```rust
let stale = matches!(
    e.kind(),
    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound,
);
if stale {
    let _ = std::fs::remove_file(&sock);
}
```

The inline comment names the rationale verbatim:

> Only delete on "no peer there" errors — connect can also fail under transient resource pressure (EAGAIN, EMFILE) where a live peer's socket would survive the next attempt. Pruning on those would race-delete a healthy peer.

The CHANGELOG echoes: "Also tightened stale-socket cleanup: only delete on `ConnectionRefused` / `NotFound`, not on every connect error, so a transient `EAGAIN`/`EMFILE` doesn't race-delete a healthy peer's socket." The tightening reads as anticipatory rather than reactive — no commit in the 22-day window names a `EAGAIN`/`EMFILE` race-delete bug; the policy is named-and-baked-in pre-empt rather than fix-after-incident. The `mcp_log` line at the same site preserves diagnostic visibility: "stdio: discover skip {sock}: {error_kind} (stale={stale})" — the `stale=` boolean is observable at runtime, so a future incident has a log signal.

**The 21-line policy doc-comment, quoted verbatim.** The `discover_live_socket(caller_cwd)` function's doc-comment is the load-bearing policy text and worth quoting in full because it's the analogue of arc 03's PR #29 cursor-block guard policy comment that arc 03's entry (= 01KR10G02J2234D0WBMWMYC35M) found high-signal:

> /// Project-scoped discovery: walk `caller_cwd` upward looking for any
> /// `.spyc-context-<pid>.json` markers (each is written by a running
> /// spyc rooted at that directory — see `context::context_path`).
> /// The first ancestor with at least one marker is the "project
> /// boundary"; only those PIDs become candidates. We never aggregate
> /// across levels: a parent-dir spyc shouldn't shadow a child-dir spyc
> /// when both exist.
> ///
> /// Why this shape: prior to this fix, discovery scanned every socket
> /// in `~/.local/state/spyc/` and returned the first connectable one,
> /// happily attaching a claude in project A to a spyc running in
> /// project B (or even another user's spyc, depending on `$HOME`
> /// scoping). Project-scoped discovery rules that out while keeping
> /// the "claude launched outside the pane just works" ergonomic — as
> /// long as it's launched somewhere inside the spyc instance's tree.

Two structural choices are named explicitly. *First*, "we never aggregate across levels" — a parent-dir spyc and a child-dir spyc can both exist, the caller in the child sees only the child, locality wins. The unit test `collect_pids_first_ancestor_with_match_wins` pins the rule with two spyc instances at `/proj` and `/proj/inner` and a caller in `/proj/inner` asserting it sees only PID 2 (the inner spyc). *Second*, "the 'just works' ergonomic" is preserved — claude can be launched outside spyc's pane and discovery still works *as long as* the launch dir is inside the spyc instance's project tree. The ergonomic preservation is what distinguishes option (b) from option (a) ("require explicit `$SPYC_MCP_SOCK`, no discovery fallback") in PR #18's BUGS.md note.

**The arc-internal traceability — PR #18 names it, PR #37 closes it.** The unusual property of arc 07 is that the bug PR #37 fixes was named in a BUGS.md note PR #18 added, with three weighted design options, and PR #37 implements exactly the recommended option. The cross-PR traceability is visible in three places:

- *PR #18 added the entry to BUGS.md `### SMALL ###`* (verified at PR #18 entry above; quoted verbatim there). The entry recommends design (b): "gate discovery on a per-project marker (e.g. only accept sockets whose context file's project_root matches the caller's cwd) ... Option (b) feels most spyc-shaped — keeps the 'just works' ergonomics while ruling out cross-instance attachment."
- *PR #37's `discover_live_socket` implements design (b)* — gates discovery on a per-project marker. The PR's commit body opens "Replace the host-wide scan with a project-scoped walk" and the four numbered steps in the body match the function body line for line. The marker file is `.spyc-context-<pid>.json`; the *project_root* check is replaced with a *cwd-ancestor* check, which is structurally tighter (the marker file is at the spyc instance's root; the caller walks up to find it; no separate project_root field needed).
- *PR #37 removes the BUGS.md entry PR #18 added* — the diff shows a 13-line `### SMALL ###` block deletion that removes exactly the text PR #18 added two days earlier. PR #37 also removes a 2-line older entry ("something funky is happening with our MCP support - we need to ensure that multiple running spyc's don't interfere with eachother") that predates the 22-day window. A new `(fixed, v1.41.24)` block is added to `### FIXED ###` whose closing line reads: "The 'something funky is happening with our MCP support — multiple running spyc's interfering with each other' entry below is also resolved by this change." Both entries marked-and-cleared in the same diff.

The named-then-fixed pattern is the structural fact about arc 07. The framing entry (= 01KR2HYMMHAH316CA9KTWKWT6W) names it as "groundwork → expansion → closure"; this entry is where the closure half lives.

**The mechanical link to PR #18's `Pane::spawn` change.** PR #18's `context_path` parameter on `Pane::spawn` ensured that App writes one canonical `<start_dir>/.spyc-context-<pid>.json` per spyc instance; PR #37's `read_context_pids_in_dir` parses that exact filename pattern to find PIDs in the ancestor walk. The function `read_context_pids_in_dir(dir)` reads `dir`, extracts entries whose names match `.spyc-context-<pid>.json`, parses the PID via `pid_str.parse::<u32>()`, and returns the PID list. The two parser rules — what PR #18 made canonical and what PR #37 reads — are textually paired. Neither PR's commit message names the dependency on the other; it lives in the file naming convention's role across two diffs.

If PR #18 had not threaded `context_path` through every pane spawn, the markers PR #37 walks for would not reliably exist at the directory PR #37 walks from — a pane spawned outside `start_dir` would have written `.spyc-context-<pid>.json` to a different dir, and the ancestor walk from a caller in `<start_dir>` would not find it. The two diffs together form the single architectural rule "one spyc instance writes one marker at one canonical place, and discovery walks from the caller's cwd to find it"; neither diff completes the rule alone.

**The eight unit tests.** The new test suite under `// ── Project-scoped discovery ──`:

- `read_context_pids_finds_markers` — basic parser, including decoys (wrong prefix, malformed PID).
- `read_context_pids_empty_dir` — empty input.
- `collect_pids_finds_marker_in_caller_dir` — same-dir match.
- `collect_pids_walks_up_to_ancestor_marker` — walk-up case (caller in `/proj/src/sub`, marker at `/proj`).
- `collect_pids_first_ancestor_with_match_wins` — locality rule (`/proj` and `/proj/inner` both have markers; caller in `/proj/inner` sees only PID 2).
- `collect_pids_returns_all_pids_at_same_dir` — multi-instance same-dir: two PIDs at `tmp.path()` both become candidates.
- `collect_pids_no_match_returns_empty` — cross-project case (caller in `/a`, marker in `/b` only); the test source-comment notes "to make the test deterministic we anchor at project_a only" — the test is honest that the walk reaches `tmp.path()`'s ancestors (which CI machines may or may not have), so the assertion is `assert!(!pids.contains(&99))` rather than `assert!(pids.is_empty())`.
- `discover_live_socket_returns_none_without_project_marker` — end-to-end: no `.spyc-context-*.json` anywhere → returns `None`, "the cross-project bug we're fixing."

**The mcp.rs file's structural growth across arc 07.** Pre-arc-07, `src/mcp.rs` was 2154 lines (per the architecture seed). After PR #18: roughly +20 net (shape-check ladder). After PR #21: +246 (codex side). After PR #37: +206 (replace 16-line scan with the project-scoped walk + 8 tests). The post-arc-07 file is roughly 2625 lines — nearly 22% growth in one arc. The architecture seed names the file as "MCP server (`ARCHITECTURE.md:135-155`, `src/mcp.rs`, 2154 lines)"; the seed's line count is pre-arc-07. Whether the file size will eventually motivate splitting registration-file-writing or discovery into sub-modules is not narratable from the diffs in arc 07; flagged for the insight layer.

**Drift findings flagged for the insight layer.**

- The branch is `fix/mcp-socket-project-scoped-discovery`; the commit subject reads `fix:`; CHANGELOG buckets under `### Fixed`. Three-way alignment.
- The PR is co-authored: "Co-Authored-By: Claude <noreply@anthropic.com>". This is the only PR in the 22-day window the framing register has noted with a co-author trailer. Whether other PRs in the window carry one is determinable from a `git log --format=%(trailers)` scan; arc 07 names this trailer here without resolution.
- The 21-line doc-comment on `discover_live_socket` is structurally analogous to PR #29's three-condition cursor-block guard comment that arc 03 (= 01KR10G02J2234D0WBMWMYC35M) found high-signal. PR #29's comment names a specific list of alt-screen TUIs ("nvim, vim, less, htop, lazygit, claude in TUI mode"); PR #37's comment names a specific list of failure modes ("claude launched outside spyc's pane, env didn't propagate, or enterprise managed-mcp.json suppressed the local `.mcp.json`"). Both are policy-explaining doc-comments at the load-bearing function site. Recurrence-reading material for the insight layer.
- The commit body reads as a deliberate teaching-of-the-fix: numbered steps for the discovery walk, named bug-vs-fall-back-vs-cleanup structure, a closes-X line at the end. The shape is closer to a release-note than a commit message. Whether `fix:` PRs in the window typically carry this register or this is the exceptional one is determinable from a sweep of other window PRs; arc 07 names the register here without resolution.
- The CHANGELOG entry duplicates the commit body almost verbatim — the commit-body and CHANGELOG-entry pair is the only place in the 22-day window where the same explanatory paragraph is preserved at two surfaces. Other PRs typically have a terser CHANGELOG that summarizes the commit body. Captured for the insight layer's "high-load-bearing-PRs duplicate explanation" reading.

Provenance:
- a303251 (merge PR #37 fix/mcp-socket-project-scoped-discovery, 2026-05-07 00:54 UTC) — full PR; HEAD of `main` at the close of the 22-day window.
- 80797b8 (source commit, 2026-05-06 20:35:08 -0400) — commit body source; co-author trailer.
- `git show --stat a303251` — 5 files changed, +246/-39; src/mcp.rs +229/-23.
- `git diff a303251^1..a303251^2 -- src/mcp.rs` — `discover_live_socket(caller_cwd)`, `collect_project_pids`, `read_context_pids_in_dir`, doc-comments quoted above, 8 new tests.
- `git diff a303251^1..a303251^2 -- BUGS.md` — 13-line SMALL deletion (the entry PR #18 added on 2026-05-04 is removed); 2-line older SMALL deletion ("something funky"); 11-line `### FIXED ###` block added under `(fixed, v1.41.24)`.
- `git diff a303251^1..a303251^2 -- CHANGELOG.md` — 25-line `### Fixed` entry that duplicates the commit body's explanatory paragraph.
- `history-arc-07-codex-and-mcp-bridge` framing entry = 01KR2HYMMHAH316CA9KTWKWT6W (groundwork → expansion → closure framing).
- `history-arc-07-codex-and-mcp-bridge` PR #18 entry = 01KR2J1R3HXNZPAHE9118BGBQJ (BUGS.md SMALL entry; `Pane::spawn` `context_path` parameter; the marker-file canonicalization).
- `history-arc-07-codex-and-mcp-bridge` PR #19 entry = 01KR2J4M8BP0J3WKNED6EMSV1Y.
- `history-arc-07-codex-and-mcp-bridge` PR #21 entry = 01KR2J81DHNG4K8NHFVN0XMD1M (codex-side registration; PR #37's discovery walk is peer-agnostic but the codex side widens the surface PR #37 hardens).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ — names PR #37 verbatim as "Recently-strengthened invariant (v1.41.24)"; arc 07's narration is the genesis-side of that current-state framing.
- `onboarding-security` entry 0 = 01KR0PKS884SXRAKZ8A790Q438 — names PR #37 as the hardening for the MCP socket-misuse threat row.
- `history-arc-03-pane-behavior` PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (precedent for high-signal policy doc-comments at load-bearing functions).

<!-- Entry-ID: 01KR2JCF7QEJHEG30TVMWY79CQ -->

---
Entry: Claude Code (caleb) 2026-05-08T01:15:49.331130+00:00
Role: scribe
Type: Note
Title: Closure: arc 07 head entries published; thread left OPEN for tail and cross-arc references

Spec: scribe

tags: #history #arc-07

Arc 07 head entries published — framing plus four per-PR entries. The arc is the seventh of eight in the spine; arc 08 (`history-arc-08-recoverability-and-deps`, PRs #13, #14, #28, #30, #31) is the only arc still pending after this session.

**Done-definition self-check.**

- ☑ `watercooler_health` reported Healthy against the spyc code_path at session open.
- ☑ Required reads completed before any write — `history-overview` (6 entries), `onboarding-product-charter` entry 0, `onboarding-architecture` entry 0, `onboarding-security` entry 0, `history-arc-03-pane-behavior` PR #34 entry + seams-aside + story-tail (for cross-arc seam check), arc 04 / 05 / 06 story-tails for tail register reference, current-state AGENTS.md (post-rename file). Midpoint interim themes entry on `onboarding-spyc-rust-bitbucket` (watercooler-cloud namespace, entry 01KR2DYTPNCY5J5HPB99GT0J5M) was unreachable: federated_search returned `access_denied` for the `site` namespace from the spyc-side server. Arc 07 proceeded without that input; the framing entry flags this honestly.
- ☑ Diagnosis stated in framing — pattern menu walked (8, 10, capability-accretion, capability-and-correction); shape committed to is **groundwork → expansion → closure** with the unusual property that the bug PR #37 fixes was named in BUGS.md by PR #18 itself with the design fix already weighted.
- ☑ Cadence: plain option A. Four PRs is below the 6+ A' threshold; three-same-day-then-two-day-gap-then-closing-PR is the natural bracket without phase grouping.
- ☑ Thread `history-arc-07-codex-and-mcp-bridge` exists with framing + PR #18 + PR #19 + PR #21 + PR #37 + this closure (5 entries; tails follow this entry).
- ☑ PR #37 entry captures all three required findings: (1) cross-project attachment bug — quoted CHANGELOG verbatim and named the `$HOME`-unset cross-user case; (2) fall-back to read-only direct mode — quoted `discover_live_socket` doc-comment verbatim; (3) stale-socket cleanup tightening — quoted the `ConnectionRefused | NotFound` `matches!` block and the inline rationale verbatim.
- ☑ Policy comments quoted verbatim — PR #37's 21-line `discover_live_socket` doc-comment in full; PR #18's BUGS.md SMALL entry in full; PR #19's `AgentKind` doc-comment, restore-branch comment, and asymmetry-naming comments verbatim; PR #21's startup-block comment and the test source-comment cross-citing v1.41.5 verbatim.
- ☑ PR #18 rename + hygiene bundle handled — name of the rename source identified via `git show --stat` ("CLAUDE.md => AGENTS.md"); commit-body framing for the rename quoted ("cross-tool standard; recent Claude Code reads both names, behavior unchanged"); hygiene-half scope walked across `ensure_mcp_json` shape-safety, `Pane::spawn` `context_path`, and `term_title` env-race fix; the BUGS.md SMALL entry PR #18 adds is named as the third hidden-half that makes arc 07 *legible* as a bracketed structure.
- ☑ PR #19 / PR #21 parity question answered: shared abstraction at the data-model layer (one `AgentKind` enum, two-field-rename-with-aliases); parallel implementation at the parsing/dispatch/registration layers (two `extract_*` parsers, two `command_without_*` strippers, two `is_*_command` checks, two `ensure_*` registration functions, two `detect_existing_spyc*` detectors). Single shared substrate at the socket level (one socket, one `spyc --mcp` proxy, one takeover prompt). Specific code structure cited by file:line throughout.
- ☑ Each head entry follows head voice contract — third-person observational, present tense, hedge whitelist, no mindset attribution, verbatim commit-message and CHANGELOG quoting attributed by `(commit <sha>, <date>)`.
- ☑ Provenance blocks on every entry, citing real SHAs and prior-thread entry IDs.
- ☑ Tags `#history` and `#arc-07` carried on every entry. Tails (when written) will also carry `#tail`.
- ☑ No insight-layer claims — recurrences (parallel-implementation pattern, policy-comment pattern, named-then-fixed pattern) are observed factually, never named as emergent properties or forward predictions.
- ☑ Catalogue §4 question carried forward without resolution (arc 05's tail had already deferred it; arc 07 doesn't move the question).
- ☐ Tails — pending after this closure (story-tail and possibly a separate seams-aside on the substrate-vs-registration distinction; tail-count decision pending the final read of the head entries above as a unit).
- ☐ Thread left OPEN — confirmed at this closure; will remain OPEN after tails for arc-08 cross-references and the eventual insight layer.

**What this arc adds to the cross-arc network.**

- *Forward references already pulled into arc 07's head entries:* arc 03's PR #29 entry as the precedent for high-signal policy doc-comments (PR #37's 21-line doc-comment is the analogue at the discovery surface). Arc 03's PR #34 entry and arc 05's PR #35 entry as the spawn-call sites that inherited PR #18's `context_path` signature widening. Arc 04's story-tail and arc 06's story-tail as parallel-implementation precedents (parser-rule asymmetry; harpoon/quickselect parallel pickers). Arc 02's investigation entry as the precedent for stdout-as-protocol byte tolerance (PR #19's codex parser's color-reset tolerance is shape-related but not the same bug).
- *Backward references arc 07 leaves to the insight layer:* the named-then-fixed pattern (PR #18's BUGS.md SMALL → PR #37's closure of that same entry); the substrate-shared / registration-parallel split that recurs at three layers (PR #19 data-model-shared / parsers-parallel; PR #21 socket-shared / `ensure_*`-parallel; PR #37 single-socket / two-detectors-OR'd); the `src/mcp.rs` file's 22% growth in one arc and whether that motivates a future split.
- *Question carried forward without arc-07 resolution:* whether the substrate-shared / registration-parallel pattern is the *steady-state* shape (acceptable trade-off for two well-known peers with stable file formats) or a *staging-area* shape (parallel implementations now, abstracted to a `trait DiscoveryFileWriter` later if a third peer arrives). The diffs in arc 07 don't decide — and the codex-parity series itself names the pattern as "mirroring," which is descriptive rather than committed-to-permanent.

**Phase 3 (insight layer) input from arc 07.** The arc 07 narration aims to leave for the eventual insight threads:

- Drift catalogue: PR #18's `chore/` prefix carrying user-visible fixes (recurrence with PR #20 / arc 05's bundling pattern); PR #21's test-source v1.41.5 cross-citation as a rare cross-PR reference inside test code; PR #37's commit-body-as-release-note register as another rare register.
- Recurrence catalogue: parallel-implementation pattern at three layers (data model, registration, transport-detection); high-signal policy doc-comment pattern at load-bearing functions (PR #29 from arc 03; PR #37 from arc 07); named-then-fixed pattern (singular for the 22-day window, structurally crisp).
- Trajectory catalogue: substrate-vs-registration distinction crystallizes in arc 07; the substrate carries the peer-agnostic invariant (one socket, one proxy, one discovery walk), the registration layer stays peer-specific. Whether this is "AI-bridge becomes general-tenant" or "AI-bridge widens at the substrate level only" is the thesis-test result the tail will return to.
- Emergence catalogue: the `.spyc-context-<pid>.json` marker file is a small artifact across two PRs with two roles — PR #18 makes it canonical for App's own use; PR #37 reads it for cross-instance discovery. The file is now load-bearing for two distinct architectural rules. Whether emergent-naming patterns recur in spyc's small-file artifacts (sock paths, harpoon hash-keyed TOML files, graveyard UUID pairs, session JSON spice-pair names) is insight-layer fuel.

**Phase 2 (arc threads) status after arc 07.** Eight arcs proposed by the spine; seven written. The remaining arc 08 (`history-arc-08-recoverability-and-deps`, PRs #13, #14, #28, #30, #31) was named as paired-shape (file-undo half + runtime-survival half) in the segmentation entry; the panic-recovery → vt100-upgrade chain at PR #30 → PR #31 (49 minutes apart) is the segmentation entry's structural prediction worth verifying from the diffs when arc 08 is written. The drift finding "PR #5 gap-analysis suspect §3 (synchronized-output / mode-2026 tearing) might be incidentally closed by PR #31's vt100 0.16 upgrade" is also arc 08's to verify or deny.

**This thread remains OPEN.** Arc 07 is published and the tail is the immediately-next work in this same session. Closure is intentionally deferred to allow cross-arc back-references from arc 08 and from the eventual insight layer.

Provenance:
- All four PR SHAs: bad8bfc (PR #18), d6d3088 (PR #19), 193f7ad (PR #21), a303251 (PR #37).
- Prior arc-07 thread entries: framing 01KR2HYMMHAH316CA9KTWKWT6W; PR #18 01KR2J1R3HXNZPAHE9118BGBQJ; PR #19 01KR2J4M8BP0J3WKNED6EMSV1Y; PR #21 01KR2J81DHNG4K8NHFVN0XMD1M; PR #37 01KR2JCF7QEJHEG30TVMWY79CQ.
- Spine pointer: `history-overview` segmentation entry index 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 07 row).

<!-- Entry-ID: 01KR2JFB7DMP7828VVXA6KW13X -->

---
Entry: Claude Code (caleb) 2026-05-08T01:18:28.017539+00:00
Role: scribe
Type: Note
Title: Tail: arc 07 told as a story — the bracket, and what it does to the charter sentence

Spec: scribe

tags: #history #arc-07 #tail

The thing arc 07 has that no other arc in this window has is a closed bracket — and the bracket closes on a sentence that gets quietly modified along the way.

PR #18 lands at 00:41 UTC on 2026-05-05. Inside its `chore/`-prefixed bundle, three substantive things happen at once. The agent-instructions file is renamed `CLAUDE.md` to `AGENTS.md`. `Pane::spawn` and `Pane::spawn_with_env` are widened to take an explicit `context_path` parameter, threaded through every overlay and pane spawn from one canonical place: App writes one `<start_dir>/.spyc-context-<pid>.json` per spyc instance, full stop. And a 13-line note appears at the top of `BUGS.md`'s SMALL bucket, naming a bug — `discover_live_socket` returning the first connectable socket on the host without project-scoping, with `$HOME` unset widening the threat to cross-user — and weighting three design fixes, with option (b) marked "most spyc-shaped" and the ergonomic preservation rule named in the same paragraph: "keeps the 'just works' ergonomics while ruling out cross-instance attachment."

The bracket opens on that paragraph.

Twenty-six minutes later, PR #19 introduces a peer-agnostic data model — one `AgentKind` enum, two field-renames with serde aliases for old saves, one `effective_kind()` that infers Claude for legacy data — sitting above two parallel parsers, two parallel command-strippers, two restore-spawn paths that branch by kind. Forty-six minutes after that, PR #21 ships the codex-side `ensure_codex_config_toml` directly below the existing `ensure_mcp_json` in `src/mcp.rs`, with a doc-comment opening "Codex's equivalent of `ensure_mcp_json`. Writes a stdio MCP entry for spyc into `<dir>/.codex/config.toml` so the codex CLI discovers us automatically, the same way claude does via `.mcp.json`." And the load-bearing line that all of arc 07 is structurally about: "The registration re-execs `spyc --mcp` and forwards through to the same Unix socket as the claude side, so a single MCP server backs both agents."

One socket, two registration files, two parsers, two CLI mechanics. The shape of the answer to the brief's parity question — *shared abstraction or parallel implementation?* — is *both, at different layers*. The substrate is genuinely shared: one Unix socket, one `spyc --mcp` proxy, one `McpConfigStatus` enum, one takeover prompt. The registration layer is parallel: two `ensure_*` functions side-by-side, doc-commented as each other's "mirror," with no factored-out trait. The asymmetries the CLIs themselves force — codex's UUID-only resume tokens, claude's CLI-flag regression, codex's `--last` fallback that has no claude analogue, claude's enterprise-policies that have no codex analogue — those are the asymmetries that make a generic abstraction expensive and a parallel pair cheap. None of these are commit-message claims; they're code comments at the point of policy. The maintainer-authored doc-comments on `ensure_codex_config_toml` and the matched-pair `command_without_codex_resume` repeat the word *mirrors* twice. That's the chosen vocabulary for "parallel by design, not waiting for refactor."

Two days later, PR #37 closes the bracket. The function `discover_live_socket` is replaced wholesale: pre-PR, 16 lines that scanned every `mcp-*.sock` on the host and returned the first connector; post-PR, a 21-line doc-comment plus a project-scoped walk implementing exactly option (b) from PR #18's BUGS.md note. The marker file the walk reads — `.spyc-context-<pid>.json` — is the file PR #18 made canonical. The 13-line BUGS.md SMALL entry PR #18 added is removed in the same diff, alongside an older 2-line entry that predates the window ("something funky is happening with our MCP support — multiple running spyc's interfering with each other"). A new `(fixed, v1.41.24)` block is added to the FIXED section whose closing line names the older entry: "is also resolved by this change."

What makes the bracket diagnostic isn't that PR #18 named the bug and PR #37 fixed it — that's normal-and-good engineering hygiene. The diagnostic part is that PR #18 *also* made the file the fix would consume canonical, in the same chore bundle, before the codex-parity expansion that *increased the urgency* of fixing the bug ran. A bug that lets a claude in project A attach to a spyc running in project B becomes structurally worse when the very same `discover_live_socket` walk would now also let *codex* in project A attach to a spyc running in project B's claude-only or codex-only context. The expansion phase didn't *cause* the bug; the bug was already there. But the expansion increased the number of attack surfaces enough that the fix-as-deferred became fix-as-must-ship-this-arc. Whether that ordering was a planned set-up-then-knock-down or a noticed-then-acted-on shape isn't narratable from any commit body. What's visible in the diffs is that the order *is* set-up, expand, knock-down; the BUGS.md note pre-existed the codex-parity expansion that made the note mandatory; the canonical marker file PR #37 needed was already in the codebase by the time PR #19 and PR #21 widened the codepath that fed it.

Now the sentence that gets quietly modified.

The product charter's load-bearing claim, from `ROADMAP.md:3-23` and quoted at `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH, is that spyc "isn't just 'a file manager with Claude in a pane.' It's a file manager that Claude can query — current directory, cursor, picks, inventory, filter, git branch — via a standard protocol. That bidirectional awareness is the positioning that differentiates spyc from `tmux` + `claude`." The thesis names Claude singular. The thesis-anchor framing in the brief proposed that arc 07 generalizes the thesis from one-tenant to general-tenant. The diffs in arc 07 don't quite do that, and the more specific reading is worth making precise.

What arc 07 actually does is: the *substrate* generalizes (one socket, peer-agnostic discovery walk, peer-agnostic context file); the *thesis sentence* widens by one peer (the README and AGENTS.md and FEATURES.md all rewrite the MCP section to name claude *and* codex); but the *registration layer* stays parallel-per-peer rather than abstracted-over-peer. The substrate is genuinely general; the registration layer is *specifically the two peers we have*, with no traits or factored writers waiting for a third. A reader expecting the arc to ship a `trait MCPRegistrationFile` doesn't find one. A reader checking whether the architecture *could* support a third peer cheaply finds: yes, by adding a third `ensure_*_config` function alongside the existing two, parsed from a third config file format, with a third detector OR'd into the takeover prompt. Cheap, but not free; cheap, but not parametric.

The more specific thesis test result is therefore: spyc's identity-as-AI-bridge widens at the substrate to be *peer-shape-agnostic* while staying explicit at the registration layer. The single Unix socket carrying tool calls doesn't care whether the proxy that connected to it was spawned from `.mcp.json` or from `.codex/config.toml`; it serves both the same way. The `discover_live_socket` walk doesn't care which peer's stdio proxy is calling; it serves both the same way. The takeover prompt doesn't care which file PID was found in; it asks the user once. But the registration files themselves are claude-shaped or codex-shaped, with two formats, two parsers, two writers, two detectors. The general-tenant property is at the transport level, not the configuration level.

That distinction matters for what arc 07 means for the charter. The charter sentence is now slightly modifiable without breaking — Claude or codex, the substrate doesn't see a difference — but it isn't fully *generalized* in the sense that any future MCP-speaking peer drops in cheaply through a parametric interface. The charter's load-bearing word "Claude" can be read as standing for "the AI peer in the pane," with codex an example, but the AGENTS.md and FEATURES.md both still write codex out by name as a second specific peer rather than as an instance of a class. The architectural meaning of the charter is now consistent with general-tenant intent; the textual meaning is still claude-and-codex, two named peers.

The other thread arc 07 picks up — quietly, without naming it — is the implicit-primary-user thread that arcs 03 and 04 foreshadowed. Arc 03's PR #29 cursor-block-guard policy comment listed "nvim, vim, less, htop, lazygit, claude in TUI mode" as the alt-screen TUIs the guard accommodates; arc 03's tail observed that "claude in the bottom pane" had been the implicit primary user across PR #6's resize comment, PR #29's policy list, and PR #34's CHANGELOG workflow lede. Arc 04's tail noted that the implicit-primary-user shape "isn't visible in either commit but is real." Arc 07 is where claude stops being the implicit primary user and becomes one named peer of two: the `AgentKind::Claude` variant lives next to `AgentKind::Codex` as siblings in an enum; the `is_claude_command` check lives next to `is_codex_command` as parallel functions; the picker tooltip line groups by kind because there are two kinds to group by. The implicit-primary-user reading from arcs 03 and 04 is now a *legacy reading* of the data model — the `effective_kind()` method's whole job is to recognize that older saves with no kind field were claude *because that was the only resume case before codex support*. The legacy default is the artifact arc 07 leaves behind.

Whether the architecture stays this shape — substrate-shared, registration-parallel, two peers explicit — or eventually factors a third interface when a third peer arrives is a question arc 07 can't answer. What arc 07 can answer is: when a second peer arrived, the architecture widened where the widening was cheap (substrate, data model, takeover prompt) and parallelized where the asymmetry was real (registration, parsing, dispatch). The named-then-fixed bracket, the substrate-vs-registration split, and the implicit-primary-user-becomes-explicit-peer-pair are three observations that the per-PR entries above record factually; the arc as a whole is what makes them legible together.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-arc-07-codex-and-mcp-bridge` head entries 0–5 = 01KR2HYMMHAH316CA9KTWKWT6W (framing), 01KR2J1R3HXNZPAHE9118BGBQJ (PR #18), 01KR2J4M8BP0J3WKNED6EMSV1Y (PR #19), 01KR2J81DHNG4K8NHFVN0XMD1M (PR #21), 01KR2JCF7QEJHEG30TVMWY79CQ (PR #37), 01KR2JFB7DMP7828VVXA6KW13X (closure).
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (thesis source; the "Claude" singular framing this tail observes arc 07 quietly modifies).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state cites PR #37 as "recently-strengthened invariant"; the "Two transports share dispatch ... single MCP server backs both agents" framing the substrate-vs-registration distinction names).
- `onboarding-security` entry 0 = 01KR0PKS884SXRAKZ8A790Q438 (cross-project attachment as the threat row PR #37 hardens against).
- `history-arc-03-pane-behavior` PR #29 entry = 01KR10G02J2234D0WBMWMYC35M (alt-screen TUI list including "claude in TUI mode"; implicit-primary-user reading source).
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (named the implicit-primary-user thread across arc 03; arc 07 picks it up).
- `history-arc-04-git-integration` story-tail = 01KR13CJ5XS5VREYA4741JHDSQ (named the implicit-primary-user observation as "isn't visible in either commit but is real").
- `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (catalogue §4 deferred-question precedent; arc 07's substrate-vs-registration question is similarly deferred to insight layer).
- `history-arc-06-input-and-overlays` story-tail = 01KR2GYQPQRX08SV980SPHHZ80 (parallel-pickers pattern at input-dispatch layer; arc 07 sees the same shape at three new layers).

<!-- Entry-ID: 01KR2JM67RTQHQYN0223GTKH1V -->

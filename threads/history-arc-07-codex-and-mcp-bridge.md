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

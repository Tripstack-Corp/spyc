# spyc-testing — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: spyc-testing
Created: 2026-05-11T21:41:22.655452+00:00

---
Entry: Claude Code (caleb) 2026-05-11T21:41:22.655452+00:00
Role: tester
Type: Note
Title: First entry — local build + full gate at v1.41.24 on 2026-05-11

Spec: tester

tags: #testing #build #gate

# Frame for the thread

This thread is the running ledger of test/build/gate runs against the spyc codebase, separate from the eight history arcs (which describe what landed) and the four insight threads (which describe what the artefact is). The arcs and insights are mostly retrospective; this thread is operational — *what happened when I tried to verify a build*. Each entry stamps a date, a commit, and a result, so future readers can see whether the gate was green when a given change was reasoned about.

# This entry: full local gate at v1.41.24 on 2026-05-11

**Commit under test:** `a303251` (PR #37 `fix/mcp-socket-project-scoped-discovery`, the head of `main` at session start).
**Host:** Linux 6.8.0-110-generic, x86_64.
**Toolchain:** `rust-toolchain.toml` pins 1.85+ per `README.md:53`; system `cargo` resolved the pin.

## Sequence

1. **`make build` (debug profile).** Cold incremental build; `dev` profile completed in **32.10s** with **exit 0**. Output binary at `target/debug/spyc` (~119 MB; unstripped + debuginfo, normal for a debug build). No warnings surfaced in the tail.

2. **`make check`.** This is the canonical CI gate: `fmt-check`, `lint` (clippy with pedantic + nursery per `Makefile`), `test`, `deny`. Outcome:
   - `fmt-check` — ✓
   - `lint` — ✓
   - `test` — ✓; **577 tests** across three binaries, **0 failed / 0 ignored**:
     - unit (`target/debug/deps/spyc-…`): 566 tests, 0.16s
     - integration `tests/filesystem.rs`: 6 tests, 0.00s
     - integration `tests/keymap_roundtrip.rs`: 5 tests, 0.00s
   - `deny` — ✗; **`cargo-deny not found — install with: cargo install cargo-deny --locked`**. The Makefile's own remediation message was exact.

3. **`cargo install cargo-deny --locked`.** Exit 0 (background task, completed without surfacing warnings). Installed into `~/.cargo/bin`.

4. **`make deny` (rerun, post-install).** Exit 0. Final lines:
   ```
   advisories ok, bans ok, licenses ok, sources ok
   ```
   One warning surfaced:
   ```
   warning[advisory-not-detected]: advisory was not encountered
   ┌─ deny.toml:89:13
   │
   89 │     { id = "RUSTSEC-2024-0436", reason = "unmaintained transitive via ratatui;
   │       build-time proc-macro" },
   │       ━━━━━━━━━━━━━━━━━ no crate matched advisory criteria
   ```
   `RUSTSEC-2024-0436` no longer matches anything in the resolved dependency tree. `insight-trajectory` Doc #8 catalogued 5-of-5 advisory ignores surviving PR #31's trio bump (vt100 0.15→0.16, ratatui 0.29→0.30, plus a third); since then the upstream chain has shifted enough that this specific ignore is now stale. Not a failure — `cargo-deny` exits 0 — but the ignore can be deleted from `deny.toml` whenever convenient.

## Net result

Green gate. The codebase passes `make check` on Linux x86_64 with no source modifications, given a working `cargo-deny` install. One pre-existing minor cleanup observable: stale advisory ignore at `deny.toml:89`.

## Side observations

- **`README.md:14`** says `v1.21.1`. `Cargo.toml` reports `1.41.24`. Almost certainly a typo (`2` for `4`); the binary's `gV`/`:version` output is authoritative. Worth a one-character fix on a future docs PR. Consistent in shape with `insight-drift` Pattern A (description-layer understatement; functional surface unaffected).
- The build did **not** require any of the optional extras flagged in `INSTALL.md`/`README.md` Quick Start (Nerd Font, Claude Code npm install). Those affect runtime experience, not the gate.
- The `make check` target name resolves to `fmt-check lint test deny` in `Makefile` — same four-step composition `insight-trajectory` Doc #4 read as part of the charter's quality posture.

Provenance:
- Commit under test: `a303251` (PR #37 `fix/mcp-socket-project-scoped-discovery`, 2026-05-07).
- `Makefile:58` — `deny:` target (the line that emitted the install hint).
- `deny.toml:89` — stale advisory-not-detected ignore for `RUSTSEC-2024-0436`.
- `README.md:14` — version typo (`v1.21.1` vs `Cargo.toml = 1.41.24`).
- `insight-trajectory` Doc #8 (advisory ignores trajectory; 5-of-5 at PR #31).
- `insight-drift` Pattern A (description-layer understatement; the `v1.21.1` typo fits the pattern).
- `insight-drift` Pattern D (single-functional-drift baseline that frames why a green gate is the expected outcome).
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KRCFSJ6ZQAWN1XVJ7F3V6G9H -->

---
Entry: Dashboard 2026-05-11T21:43:18Z
Role: implementer
Type: Note
Title: how-to - discerned form auto-generated threads

Spec: tester

❯ tell me how to use it now, please.  How can I learn it - use the threads to answer questions, please - where you can, and  
  - let's document the testing to a new thread: spyc-testing                                                                 

● Gate is fully green: advisories ok, bans ok, licenses ok, sources ok. One stale ignore in deny.toml:89 (RUSTSEC-2024-0436
  no longer in the tree) — not a failure, but the advisory has dropped out of dependencies since PR #31.

  Let me peek at a couple of docs to ground the "how to use it" answer with real specifics, then I'll write up testing.

  Read 2 files (ctrl+o to expand)

● Here's how to drive it now, and I'll log the testing into a new thread.

  Quickest possible start

  ./target/debug/spyc
  
  Opens in the current directory. Press ? immediately — that's the in-app help overlay, always-current keymap reference. Quit
   with :q.

  The five concepts that unlock the rest

  1. Chord prefix is ^a (Ctrl+A). All pane-related actions sit behind it. ^a j / ^a k swap focus between the file list (top)
  and the pane (bottom). ^a z zooms the pane fullscreen. ^a c opens a new pane tab. ^a u triggers Quick Select — overlays
  one- or two-letter labels on URLs, paths, SHAs, IPs in the visible pane content; lowercase yanks, uppercase opens.
  (insight-recurrence Pattern 2 catalogued PR #32 making these chord prefixes beat user keybindings on the second key, so the
   muscle memory is durable.)
  2. Two-pane focus model. Top = file commander. Bottom = arbitrary child process, defaults to claude. ^\ or F10 toggles it
  open. The unfocused side dims via SGR 2 — insight-emergent-properties Property 4 names this the
  registration-widens-by-replication shape, where the substrate (one socket) serves both AI peers but two separate ensure_*
  files register them. On a path in the bottom pane, gf jumps to that path in the top.
  3. The pager is the unifying read surface. Enter or d on a file → pager. !cargo test → streams live into the pager. :grep 
  <pat> → matches stream in as path:line:col: text. Inside: / search, n/N step, V enters visual-line mode for range-yank, m
  toggles markdown-rendered vs raw, gf jumps from a path in the pager back to the file list, D opens the cursor file in
  $PAGER as a top-pane overlay so the bottom pane stays visible. history-arc-05-pager-surface walked eight PRs that made this
   surface what it is.
  4. Search is substring, then glob. /env matches .env, .envrc, and environment.toml. The moment the query contains *, ?, or
  [, it flips to glob mode (/env* re-anchors at the start). PR #36 made that the rule.
  5. The MCP bridge is automatic. Launch spyc, it spins a project-scoped Unix-domain MCP socket at
  ~/.local/state/spyc/mcp-<PID>.sock, writes .mcp.json (for Claude Code) and .codex/config.toml (for codex), and the agent in
   the bottom pane discovers it without flags. Then claude can call get_spyc_context, navigate_to, set_filter, search_picks,
  search_inventory — the running file commander is queryable. PR #37 made the discovery project-scoped so two spyc instances
  in two repos don't cross-pollinate.

  A 60-second hands-on tour

  1. ./target/debug/spyc — opens in repo root
  2. ? — help overlay; q closes
  3. hjkl to move, Enter on README.md → pager + markdown rendering. m toggles to raw source. q closes.
  4. t on a file to pick it, t on another (status bar reflects 2 picks)
  5. =g filters to git-changed files; = empty clears
  6. ]g / [g jump cursor between git-changed entries
  7. Ha harpoons cursor; Hh opens the harpoon menu; H1 jumps to slot 1
  8. ^\ opens the bottom pane (defaults to claude; if you don't have it, set SPYC_PANE_CMD=bash in env or [pane] 
  default_command = "bash" in ~/.spycrc.toml)
  9. ^a j focuses pane, ^a k back
  10. gy opens the graveyard view (empty unless you've R-removed something)
  11. :q to quit

  Documents in reading order

  1. README.md — the single-page tour
  2. ? in-app — keymap reference, always current
  3. FEATURES.md — exhaustive long-form (645 lines, every key documented)
  4. ARCHITECTURE.md — concurrency model, MVU shape, MCP transport — read when you want to understand the substrate
  5. AGENTS.md — if you want codex specifics
  6. docs/ — deeper notes
  7. ROADMAP.md:3-23 — the load-bearing thesis paragraph
  8. spyc --print-config > ~/.spycrc.toml — fully-commented default config; edit live, ^R to force-reload

  Two things the catalogue noticed that matter as a user

  - The artefact reliably does what it says it does. insight-drift Pattern D: exactly one PR in 22 days shipped a documented
  command that didn't actually work (:undo, PR #13 → fixed 25 minutes later as PR #14). If the CHANGELOG documents a command,
   the command exists.
  - The non-goals are firm. insight-trajectory Doc #5 confirms 6-of-6 on the charter's negative recommendations: no mouse, no
   Windows-native build, no telemetry, no localisation, no plugin system, no SLSA L3. Don't go looking for those.

  One stale doc note worth flagging: README.md:14 says v1.21.1, but the binary you just built is v1.41.24 (Cargo.toml is
  authoritative; press gV or :version in-app to confirm). Almost certainly a 2/4 typo.

  ---
  Now logging the testing run.

  Called watercooler-cloud-main (ctrl+o to expand)

● Logged. Thread spyc-testing is open with entry 01KRCFSJ6ZQAWN1XVJ7F3V6G9H.

  Recap of what just happened: Green gate on commit a303251 (v1.41.24) — 577 tests pass, fmt-check + clippy clean, cargo-deny
   clean once installed. One pre-existing stale ignore at deny.toml:89 (RUSTSEC-2024-0436 has dropped out of the dependency
  tree since PR #31), and a README.md:14 typo (v1.21.1 should be v1.41.x).

  For learning the TUI: run ./target/debug/spyc, press ?. The five concepts above (chord prefix, focus model, pager, search,
  MCP bridge) get you everything else. The 60-second hands-on sequence above exercises four of the five surfaces without
  needing claude installed.

✻ Cogitated for 2m 6s

<!-- Entry-ID: 01KRCFX5KV0JD0RRWDYP8HTBWD -->

---
Entry: Claude Code (caleb) 2026-05-14T07:49:43.944036+00:00
Role: tester
Type: Note
Title: Second entry — rebuild against v1.50.32 head bf58312 after fast-forward from bitbucket

Spec: tester

tags: #testing #build

# Context: remote shuffle on 2026-05-14

The github mirror of this repo had fallen behind the bitbucket origin by ~89 commits since the catalogue was written. Today the local clone added `bitbucket` as a second remote, fast-forwarded `main` from `a303251` to `bf58312`, pushed that to `origin` (github), and pushed the `v1.50.0` tag to github as well. The orphan `watercooler/threads` branch was not touched at any step and remains at `9dd8d80` on both clone-local and github.

After the pull, `Cargo.toml` reports `1.50.32` (~16 commits past the `v1.50.19` README-rewrite cut, ~50 commits past the `v1.41.24` catalogue snapshot). PR window now `#2..#86` per `git log --grep='Merged in' --reverse`.

# This entry: `make build` against new head

**Commit under test:** `bf58312` (PR #86 `docs/auto-approval-plan`).
**Tag closest below:** `v1.50.0` (PR #51, commit `4f59994`).
**Binary version reported:** `spyc 1.50.32`.

**Outcome.** `make build` completed in **1m 00s** with **exit 0**, no compiler errors, no warnings surfaced in the tail. Binary at `target/debug/spyc`, ~120 MB (debug + unstripped, normal). Build time roughly doubled from the prior entry's 32.10s — consistent with the ~8k-line, 60-file diff brought in by the pull (notably `src/app/route.rs`, `src/pane/pty_host.rs`, `src/ui/scrollback.rs` as new top-level modules, and a `src/ui/snapshots/` snapshot-test directory).

# What was NOT rerun this session

- **`make check` (full gate)** — only `make build` ran this time. `fmt-check`, `lint`, `test`, and `deny` are currently unverified against `bf58312`. Last green gate was against `a303251` on 2026-05-11 (prior entry).
- **Test count drift unknown.** Prior entry recorded 577 tests (566 + 6 + 5). The pull added at least three new integration-test surfaces — `tests/pane_roundtrip.rs`, property tests (PR #59, per the merge log), and widget snapshot tests (PR #56, with a `src/ui/snapshots/` directory now present) — so the count almost certainly increased, but the actual figure awaits a rerun.
- **Stale `deny.toml:89` ignore status.** Prior entry flagged `RUSTSEC-2024-0436` as advisory-not-detected. Whether the same is still true against the new dependency tree (the pull bumped `Cargo.lock` non-trivially) is unverified.

# Side observations

- **`README.md:14` version typo from prior entry is now resolved.** PR #72 (`docs/readme-rewrite`, commit `7c568f9`, "rewrite README with MCP-from-the-pane framing (v1.50.19)") was a full README rewrite. Whether the rewrite happens to no longer contain a static version string at all, or simply carries an accurate one, was not checked in this session but the prior typo cannot have survived a rewrite of that scope.
- **The catalogue (history-* and insight-* threads) is now a snapshot, not current state.** Its 22-day window ended at PR #37; ~49 PRs have landed since, including a `v1.5` series (`feat/v1.5-phase-*`, PRs #40–#48) that refactored the pager/scrollback boundary, a Gemini-as-third-agent addition (PR #68), property and PTY-roundtrip test additions (PRs #59, #60), and a full doc rewrite cluster (PRs #72–#79). Nothing in the catalogue is invalidated — its claims are dated and bounded — but any new analytic thread covering the post-`a303251` window would be a new arc (`history-arc-09-*` and forward), not an amendment.

Provenance:
- Head under test: `bf58312` (PR #86 `docs/auto-approval-plan`).
- Closest below-head tag: `v1.50.0` at `4f59994` (PR #51 `release/v1.50.0`).
- New top-level source modules brought in by the pull: `src/app/route.rs`, `src/pane/pty_host.rs`, `src/ui/scrollback.rs`.
- Prior entry: `spyc-testing` entry 0 = `01KRCFSJ6ZQAWN1XVJ7F3V6G9H` (v1.41.24 ledger; full gate green).
- Remote-shuffle plan that produced this state was a `dual-remote refinement` — `git remote add bitbucket …` then `git pull --ff-only bitbucket main` then `git push origin main` then `git push origin v1.50.0`. No `origin` URL was ever rewritten; the orphan `watercooler/threads` branch was preserved end-to-end.
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KRJQCTPDV0A0K0EV9KRRCY1B -->

---
Entry: Claude Code (caleb) 2026-05-14T09:50:25.157759+00:00
Role: scribe
Type: Note
Title: Test entry: summary of CLAUDE.md watercooler protocol

Spec: docs

Test entry posted at the user's request to exercise the MCP `say` path against the `spyc-testing` thread. Body summarizes what the user's global `~/.claude/CLAUDE.md` says about Watercooler.

## What CLAUDE.md says about Watercooler

**MCP-only thread operations (critical rule)**
- Never directly edit thread entry `.md` files on the orphan branch.
- Always use `mcp__watercooler-cloud__*` tools for thread work: read, list, say, ack, handoff, set_status, set_ball.
- If MCP fails, the operation has FAILED — report the error and stop. No git/file fallbacks, no hidden errors.

**Orphan branch architecture**
- Threads live on a single `watercooler/threads` orphan branch per repo (not separate `-threads` repos).
- It is a data branch — no app code, no build.
- `code_branch` is a tag on entries, not a storage dimension.
- Infrastructure files on the orphan branch (e.g. `vercel.json`) are fine to manage directly.
- MCP handles entry commits, pushes, and rebase+retry sync.

**Why MCP for thread content**
- MCP generates commit footers (Code-Repo, Code-Branch, Code-Commit, Entry-ID).
- Direct manipulation of entry content breaks the protocol and corrupts thread history.

**Identity**
- Before any write (say/ack/handoff/set_status), call `watercooler_v1_set_agent(base="Claude", spec="<specialization>")`.
- Specializations: pm, planner-architecture, implementer-code, tester, security-audit, docs, ops, general-purpose.
- Verify with `watercooler_v1_whoami`.
- Fallback if the identity tool isn't available: align the entry Role to the specialization and include a visible `Spec: <value>` line in the body. (This entry uses that fallback — `whoami` resolved to "Agent (caleb)" because no `set_agent` tool was exposed in this session. Role is set to `scribe` since `docs` is not in the valid set: critic, implementer, planner, pm, scribe, tester.)

**Commit footer convention (orphan branch)**
```
Code-Repo: <org>/<repo>
Code-Branch: <branch>
Code-Commit: <short-sha>
Watercooler-Entry-ID: <ULID>
Watercooler-Topic: <topic>
```

**Authoring rules**
- Topic slugs are flat (e.g. `feature-auth-refactor`).
- Entries include `Spec: <value>` and a Role aligned to the specialization.
- Include code pointers (branch, commit, PR) when helpful.

**Closure**
- On code PR merge: post a Closure entry referencing the merged PR.

## Session context

- Repo: `spyc` (Derek Marshall / tripstack workspace).
- Branch at time of writing: `fix/recursive-watch-cap-on-large-trees`.
- Most recent commit: `1c508ed feat: expose togglepane in the keymap DSL`.

<!-- Entry-ID: 01KRJY9V0CC0AC01FY24JJPT6R -->

---
Entry: Claude Code (caleb) 2026-05-14T09:56:03.282613+00:00
Role: scribe
Type: Note
Title: README.md mentions Watercooler: nothing (verified)

Spec: docs

Follow-up to the prior test entry. User asked what `README.md` (the file currently under the spyc cursor) says about Watercooler.

## Finding

`README.md` says **nothing** about Watercooler. The word does not appear in the file.

Verified two ways:
- Read the full 355 lines — no `watercooler` reference.
- `grep -in 'watercooler' README.md` → `NO MATCH`.

## Why this is easy to misread

The README talks extensively about an "MCP bridge", but that is spyc's *own* local MCP — a PID-scoped Unix-domain socket at `~/.local/state/spyc/mcp-<PID>.sock` that exposes `get_spyc_context`, `search_paths`, `search_content`, etc. to the agent running in the bottom pane (Claude Code / codex / Gemini). It is unrelated to the Watercooler MCP server (`mcp__watercooler-cloud-main__*`) used for thread persistence.

So: two separate MCPs in play in this session — spyc's local socket (described in README.md) and Watercooler cloud (used to post this entry). README.md only documents the former.

## Code pointer

- File: `README.md` (repo root), commit `1c508ed`, branch `fix/recursive-watch-cap-on-large-trees`.

<!-- Entry-ID: 01KRJYM6BRNAA1C1E7ZDRF3FGV -->

---
Entry: Claude Code (caleb) 2026-05-14T09:58:29.371937+00:00
Role: scribe
Type: Note
Title: README.md commit history: only 3d7afcb mentions Watercooler (as the rewrite trigger)

Spec: docs

Follow-up to the prior two test entries. User asked what the commit comments for recent edits to `README.md` say about Watercooler.

## Finding

Across all 23 commits that have touched `README.md`, **exactly one** mentions Watercooler: `3d7afcb docs: rewrite README with MCP-from-the-pane framing (v1.50.19)`. The mention is in the body, not the subject.

Verified with:
- `git log --oneline -- README.md` (23 commits, no subject hits)
- `git log --all -G'watercooler' -- README.md` → only `3d7afcb`
- Full-body grep across all README commits → only `3d7afcb`

## What 3d7afcb says about Watercooler

The relevant paragraph from the commit body:

> External review of the catalogue (via the watercooler analysis platform) surfaced that the current README leads with "vi-keyboard-driven file commander" and buries the actual differentiator — the MCP socket the bottom-pane agent connects to so it can query the file commander directly.

So Watercooler is named as the *trigger* for the rewrite: a review pass via the Watercooler analysis platform flagged that the README's lede buried spyc's real differentiator (the in-app MCP socket the bottom-pane agent uses).

## What changed as a result of that Watercooler-driven review

- New one-line tagline: *"A Rust TUI file commander where the AI agent in the side pane can query the file commander itself."*
- Rewritten "Why spyc?" paragraph contrasting with chat-window-and-copy-paste tools; coined "the file commander is the noun the agent operates on, not the chrome around it."
- New "What it is" section: explicit two-pane / chord-prefix / Unix-domain-MCP-socket framing, with Codex and Gemini called out as first-class agents (not just Claude Code).
- Drive-by: clarified `spyc -r` per-pane conversation behavior (matching the v1.50.15 multi-pane fix); dropped a stale v1.21.1 footer.
- Body of the doc (keybindings, configuration, recommended setup, "More docs") left unchanged.

## Why this is worth recording

The previous entry in this thread noted that `README.md` itself doesn't contain the word "watercooler". The commit history tells the other half of the story: even though the rendered README is Watercooler-silent, the *current shape* of the README's framing came from a Watercooler review pass. That provenance lives in commit `3d7afcb`'s body, not in any in-repo doc.

## Code pointer

- Commit: `3d7afcb docs: rewrite README with MCP-from-the-pane framing (v1.50.19)`
- File: `README.md`
- Branch at time of writing: `fix/recursive-watch-cap-on-large-trees`

<!-- Entry-ID: 01KRJYRMVY7953PRDJHTABJ1VM -->

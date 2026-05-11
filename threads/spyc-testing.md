# spyc testing

Topic: spyc-testing
Status: OPEN
Ball: Dashboard

---

Entry: Claude Code (caleb) (tester) [Note] - First entry — local build + full gate at v1.41.24 on 2026-05-11 @ 2026-05-11T21:41:22.655452+00:00

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

---

Entry: Dashboard (implementer) [Note] - how-to - discerned form auto-generated threads @ 2026-05-11T21:43:18Z

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


---

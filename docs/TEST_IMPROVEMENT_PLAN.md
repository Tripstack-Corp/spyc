# spyc testing campaign — coverage + anti-"test theater"

**Status:** all 8 clusters shipped (#426–#435); ~50 new tests, the
anti-"test theater" effect-intent seam in place, and **2 real bugs found +
fixed** (#430, #431 — both in the live pane/pty workflow, cluster 4). Two
small follow-ons remain (the deferred routing edges from cluster 6; a real
coverage-guided cargo-fuzz pass if a lib split ever lands) — see the Order of
attack table. The harness (`App::test_app`, `src/app/test_harness.rs`) +
effect-intent matchers (`app/effect.rs`) are the durable foundation for
future workflow tests.

This charter folds the original May workflow-coverage plan together with
the "Beyond Test Theater" quality RFC into one running plan.

**Goal:** raise the *value* of the suite, not just the count. Two distinct
risks, addressed in parallel:

1. **Workflow-composition gaps** — most real bugs appear where a key routes
   through `App`, focus changes, pane/pty state mutates, a task changes
   ownership, or a session restore picks an agent-specific resume path.
   Those flows need harness-level tests between tiny unit tests and full
   terminal automation.
2. **"Test theater"** — tests that confirm the code does what it already
   does (tautologies) or pin internal struct layout byte-for-byte, creating
   false confidence and refactoring paralysis instead of catching real
   regressions. (See the appendix.)

## Where we are

~1,100 tests (up from the 732 in the May assessment — the MVU migration and
the deep-review campaign added the rest). `cargo test --locked --all-targets`
passes under normal permissions; sandboxed runs can fail the Unix-socket MCP
tests with `Operation not permitted`.

**Genuine strengths (already not theater):**

- **Pure domain tests** — `src/app/state/tests/{navigation,apply}.rs` exercise
  state transitions with no mocked terminal, avoiding the brittle UI tests
  that plague TUIs.
- **Contract tests** — `tests/pane_roundtrip.rs` validates the real
  `portable-pty` ↔ `vt100` contract rather than mocking it (the gold standard).
- **Targeted edge cases** — `find_match` covers glob-anchor vs substring,
  case-insensitivity, etc.
- **Enforced invariants as tests** — the render-purity source scan
  (`app::render::purity_guard`), the `mod.rs` line ceiling, and the
  compile-checked `COMMAND_TABLE`. These are tests that catch *architecture*
  drift, not behavior tautologies.
- Property tests exist (shell quoting, ignore masks, keymap counts); 15+
  `insta` snapshots cover pager/status/prompt/list rendering.

**Symptoms to fix:**

- **Example-based tautologies** — `state_with_rows(&["a","b","c"])` encodes one
  developer's assumption about a 3-item list; misses empty lists, 10k-item
  lists, mid-action resizes.
- **Implementation coupling** — `apply.rs` pins emitted effects byte-for-byte
  (`apply_jump_start_dir_emits_change_dir` matches the whole `Effect::ChangeDir`
  struct). Add a field to `ChangeDir` and every such test breaks — the exact
  refactoring paralysis to avoid.

## Workstreams

Run like the deep-review campaign: **one PR per cluster, verify the gap first
(write the missing/failing test), then close it, gate green, merge as we go.**
The workstreams are independent — interleave by value, not strict order.

### A. Move off "test theater" (quality)

- **Property-based testing (`proptest`).** spyc's pure MVU state is the ideal
  candidate.
  - *Navigation & grid math:* for *any* list size, *any* grid dims, *any*
    start cursor, `Down(N)` then `Up(N)` stays in bounds and behaves
    predictably — instead of one fixed 3-item case.
  - *Fuzzy find (`find_match`):* random strings + queries prove it never
    panics, and that a literal substring present in the list always yields
    `Some(index)`.
- **Decouple assertions from struct layout.** Replace byte-for-byte effect
  matches with intent-level helpers:
  - *Instead of* `assert_eq!(fx, [Effect::ChangeDir { path, focus, on_ok: None, err_prefix: "chdir" }])`
  - *Use* `assert!(fx.contains_chdir_to("/tmp/test"))` — validates the
    requirement (a directory change was requested) without pinning the error
    prefix or whether `on_ok` is currently populated.
- **Spec-first / BDD.** Tests as executable spec: requirement-based names
  (`returns_none_when_search_query_is_empty`), tests mapped to `FEATURES.md` /
  `AGENTS.md`, and **negative tests** that prove what the system must *not* do
  (e.g. "the cursor index must never exceed the listing length").

### B. AI-assisted-testing rules (process)

Adopt these so we expand coverage without manufacturing theater:

1. **Never prompt "write tests for this function."** That just re-asserts
   current behavior.
2. **Prompt with the requirement.** e.g. *"property test: no sequence of
   `Up`/`Down` may leave the cursor index ≥ `inventory.len()`."*
3. **Use AI for edge-case data**, not assertions — circular symlinks,
   unreadable dirs, weird-unicode filenames fed into the existing
   `filesystem.rs` integration tests.

### C. High-risk workflow coverage (the original Phase 2)

Prove the user-visible rules at the harness level, not just the pure classifier:

- **Routing & focus:** prompt input wins over a focused pane; overlay pager
  consumes normal keys but allows configured meta chords; lower-pane pager
  routes by focus; an exited pane flashes on typing yet still takes
  pane-management chords; pane-scroll and pager modes don't both eat the same key.
- **Pane / pty:** `^a z` zoom/unzoom preserves `pane_height_pct` and restores
  prior focus; `^a v` scrollback shows scrollback-before-live; empty scrollback
  flashes rather than an empty pager; new tabs default to `PROJECT_HOME`;
  switch/close/restart/exited-tab routing preserves active-tab state. (Keep real
  pty spawning to a few smoke tests; use fake buffers / test constructors.)
- **Quick Select:** scans exactly the visible viewport; off-viewport text
  ignored; lowercase yanks, uppercase dispatches open-intent (URL/path/SHA/custom);
  query strings preserved, trailing punctuation trimmed; existing paths jump,
  missing paths flash; SHA → git-show pager; Esc closes with no side effects.
- **Background tasks:** `^Z` from a capture pager backgrounds + keeps an entry;
  post-background output is visible on resume; `:fg` / `:fg N`; `gB` / `:task N`
  / `[t` / `]t` view without taking ownership; closing an exited viewed task
  promotes its output into buffer history; divider distinguishes
  running/new/success/failure.
- **Session restore:** multiple tabs restore in order with distinct session IDs;
  cwd/labels/commands/project-home/session-name survive save→restore; Codex
  spawns `codex resume <UUID>`; Claude types `/resume <id>` after settle; Gemini
  uses the configured/listed target; legacy fields still deserialize; malformed
  agent IDs fall back to a fresh command without panicking.

### D. MCP & environment ergonomics (the original Phase 3)

Keep the real-socket tests; make failures interpretable:

- Socket server responds to initialize + tool calls; disconnect routes through
  the command channel; path traversal stays blocked for file-content reads;
  tool searches use the expected project root + ignore behavior.
- Unix-socket permission failures in restricted sandboxes produce a clear
  diagnostic pointing to "rerun under normal permissions."
- Do **not** weaken the authoritative check: `cargo test --locked
  --all-targets` under normal local permissions still runs all MCP socket tests.

### E. Rust / TUI best practices (cross-cutting)

- **Snapshots (`insta` + `TestBackend`):** extend coverage of the terminal
  output buffer to catch visual regressions without per-cell asserts.
- **Fuzzing:** brute-force mutated input for the critical parsers/mutators —
  the keymap DSL parser (`config/dsl.rs::parse`) and the fuzzy matcher
  (`state/navigation.rs::find_match`) — to expose crashes a single example
  misses. Decision history: cargo-fuzz was chosen first, but it needs a `[lib]`
  target and spyc is **bin-only** (modules are `mod` in `main.rs`) — a
  crate-wide lib+bin split wasn't worth it for one cluster, so we used
  **`proptest` in-crate** first (no nightly, runs in `make check`): both targets
  are proptest-fuzzed for panic-freedom (`find_match` in #427, the DSL parser in
  #435). **Then the real cargo-fuzz pass landed** (#436): the crate was split
  lib+bin so a standalone `fuzz/` crate can link it; `cargo +nightly fuzz run
  dsl_parse` exercises the DSL parser under libFuzzer (kept out of `make check`,
  run on demand). First run: ~690k coverage-guided execs, no crash. Then the
  pager's parsers — the richest untrusted-input surface — got targets too
  (#437): `render_markdown`, `highlight` (syntax), `word_wrap` (+ a char-boundary
  invariant). All four ran clean, no crashes. `find_match` stays on proptest
  (it's a method on a constructed `AppState`, awkward to reach from a
  free-standing libFuzzer target).
- **Organization & hygiene:** keep unit tests (`#[cfg(test)]` in-file) separate
  from integration tests (`tests/`); single-responsibility, descriptive names;
  use `.unwrap()`/`.expect()` for setup and reserve `assert!` for the behavior
  under test (don't assert on intermediate setup steps).

## Order of attack

Sequenced **one PR per cluster**, deep-review cadence: verify the gap first
(write the missing/failing test), close it, gate green (`make check`), merge as
we go (`--close-source=false`), tick the box. Front-loaded so enabling work
unblocks the rest. Tick `✅ #NNN` as each lands.

| # | Cluster | Workstream | Verify-first gap → deliverable | Status |
|---|---------|-----------|--------------------------------|--------|
| 1 | **Effect-intent matchers** (enabling) | A.2, B | The ~6 byte-for-byte `Effect` destructures in `state/tests/apply.rs` (`apply_jump_start_dir_emits_change_dir`, `apply_jump_mark_*`, `apply_climb_*`) break on any field add → matcher layer over `&[Effect]` (`effect.rs::matchers`: `change_dir()`, `read_pane_text()`), retrofit those tests onto it, codify the AI-testing rules into AGENTS.md. | ✅ #426 |
| 2 | **Pure-state property tests** | A.1, A.3 | Nav/grid covered by one fixed 3-item case → proptest: any list × grid × cursor, the negative invariant "cursor index never ≥ `len`" under any move sequence; `find_match` sound + never panics, literal substring ⇒ `Some`, matcher fast-path ≡ Unicode reference. | ✅ #427 |
| 3 | **Session restore orchestration + back-compat** | C | The disk roundtrip + per-agent `reconstruct_restore` turned out already-covered; the gaps were the *pure* dispatch/back-compat → `profile_for(kind)` round-trips every registered agent + falls back to the no-op for `Other`; the legacy `effective_kind → profile_for → reconstruct_restore` chain still resumes a pre-codex Claude tab; a Session JSON missing `name`/`project_home` deserializes; `skip_serializing_if` keeps absent ids out of the wire format. (Driving `restore_session`'s fork/exec needs a production seam — deferred to a pty smoke test in #4, not forced into a unit test.) | ✅ #428 |
| 4 | **Pane/pty workflow** | C | Real-`cat`-pane smoke tests at the App level (no fake-pane constructor exists; every `Pane` forks): `^a z` zoom forces pane focus, unzoom restores prior focus, `pane_height_pct` preserved; bare pane-open spawns in `listing.dir` + focuses pane. Writing these corrected two stale charter claims (new-tab is `listing.dir` *by design*, not PROJECT_HOME; empty scrollback flashes *and* opens the pager by design) **and surfaced a real bug** — the empty-scrollback hint is clobbered (see Bugs-found log; fixed separately). | ✅ #429 |
| 5 | **Background tasks** | C | `BackgroundTask` owns a live `PtyHost` (can't build without forking), so real-`cat`-capture smoke tests: `^Z` backgrounds + keeps the entry (Running) + closes the pager → `:fg` re-attaches it + removes it from the list; `gB`/`:task` views without taking ownership (marks viewed, clears the unread divider, pager tracks the id); `[t`/`]t` cycle with wraparound, view-only. Pure helpers (id alloc, status glyph, counts) already unit-tested in `tasks.rs`. | ✅ #432 |
| 6 | **Quick-select dispatch** | C | Scanner already covered (11 tests); the gap was the dispatch. Extracted the kind×intent **action matrix** into a pure `quick_select_action` (behaviour-preserving) → unit-tested every cell (lowercase→yank; uppercase URL→open, path→jump, SHA→git-show, custom+template→filled URL, IPv4/template-less→yank-with-hint). Overlay state machine: Esc closes with no dispatch; 2-letter uppercase-first arms open-intent + narrows; uppercase Path label dispatches the open (proved via a missing path → not-found flash, CWD-safe). The successful-jump `chdir` (`set_current_dir`) + yank/URL/git leaves stay impure (covered at the matrix level). **Deferred:** the two routing edges (exited-pane flash-yet-takes-chords; pane-scroll vs pager key overlap) → fold into a later edge-cases pass. | ✅ #433 |
| 7 | **MCP / env diagnostics** | D | Socket bind permission failures were opaque (server `bind_result?` → bare "Operation not permitted"; tests `.unwrap()` → opaque panic). Added a pure `socket_bind_error` classifier (EACCES/EPERM → "rerun under normal permissions" + path; else plain context), applied at the server bind site + unit-tested both branches; a `bind_test_socket` helper makes the two socket tests fail with a clear sandbox hint. Full-perms run unchanged (still binds + exercises the real socket). Diagnostic-only, patch bump 1.58.3. | ✅ #434 |
| 8 | **Parser fuzzing (proptest → real cargo-fuzz)** | E | First shipped **proptest in-crate** (no nightly, runs in `make check`): the keymap DSL parser (`config/dsl.rs::parse`) gets panic-freedom over arbitrary + map-biased input, "well-formed `map ^<k> <verb>` always binds"; `find_match` was already proptest-fuzzed in #2. **Then did the real thing** (#436): split the crate **lib+bin** (`src/lib.rs` owns the modules + `run()`, `main.rs` is a shim) so a `fuzz/` cargo-fuzz crate can link it, with a libFuzzer `dsl_parse` target via the `spyc::fuzz` facade. Standalone `[workspace]` keeps it out of `make check` (nightly + on-demand). **Then extended to the pager's parsers** (#437) — the richest untrusted-input surface, with a unicode/wrap/markdown bug history: `render_markdown`, `highlight` (syntax), and `word_wrap` (with a char-boundary invariant: no range may split a codepoint). All four targets ran clean — DSL 690k execs, word_wrap 917k, highlight 133k, markdown 60k, **no crashes**. | ✅ #435, #436, #437 |

## Bugs found by the campaign

The headline win: real bugs a *newly-added test* caught — not behaviour-
preserving refactors (which find nothing by design). A fix is a behaviour
change, so it ships as its own `fix:` PR (release-build + owner test), separate
from the test-only cluster PR that exposed it.

| Found by | Bug | Fixed in |
|----------|-----|----------|
| Cluster 4 — `^a v` empty-scrollback (smoke test + owner manual test) | Three issues in one branch: (1) **dead effect** — `open_pane_scroll_pager` flashed the hint then called `mount_scroll_pager`, which flashes `"scroll: on …"` in the same call (`flash_info` overwrites), so the hint never reached the user. (2) **inaccurate wording** — `"this app keeps its own history"` is false for a fresh shell and backwards for an agent (spyc *does* parse claude/agy logs via the transcript hook). (3) **dead-end mode** — it entered scroll mode even with nothing above the visible screen, trapping the user in a one-screen pager (caught on a fresh zsh during manual test). Fix: when scrollback is empty, flash an **agent-aware** hint and **stay live** (don't mount the pager); the visible screen is still on screen and `yp` yanks it. | #430 |
| Owner manual test + key-trace (during the pane/pty cluster) | **Rapid `^a n` / `^a p` eaten.** Firing the chord fast leaves Ctrl held, so the second key arrives as `^n`/`^p` (Char + CONTROL), not bare. The resolver's generic Ctrl block ran *before* the `PendingSeq::W` pane-chord block and matched `^n`/`^p` to nothing → `_ => Ignored`, resetting the pending `^a-` chord, so the tab switch was silently lost. (The harness test used *bare* keys, which always worked — only the live keystroke carries Ctrl; the key-trace showed `code=Char('n') mods=CONTROL … resolver -> Ignored`.) Fix: run the `PendingSeq::W` block before the Ctrl block, matching the completion key code-only — screen treats `^a ^n` == `^a n`. Regression tests: resolver `ctrl_a_then_ctrl_{n,p,c}_*` + the `^a ^a`→PaneLastTab guard + harness `rapid_pane_next_prev_chords_each_switch_tabs` (now Ctrl-held). | #431 |

Clusters 1–3 + 5–8 surfaced none — expected: #1 was a behaviour-preserving
retrofit; #2's four invariants held under 256 random cases each; #3 found the
session dispatch + serde back-compat correct (the disk roundtrip / per-agent
resume already had tests); #5's `^Z`/`:fg`/`gB`/cycle round-trips all behaved;
#6's quick-select dispatch matrix + state machine held; #7 + #8 were diagnostic
/ parser-fuzzing (parser panic-safe by construction). **Both bugs came from #4
(pane/pty) — where untested *live* workflow lived.** The pure / decision layers
(effects, nav, finder, session dispatch, serde, task bookkeeping, the
quick-select matrix, the DSL parser) all held up — a useful signal for where
the risk actually concentrates.

## Acceptance criteria

- `cargo test --locked --all-targets` passes locally with all new tests.
- Snapshots stay stable unless the implementation intentionally changes UI.
- New tests don't depend on the developer's real home dir, shell history,
  clipboard, a running spyc, or installed agent CLIs — temp dirs + deterministic
  fake data only.
- Real pty/socket tests are isolated and either pass under normal permissions
  or fail with a clear environment-specific message.
- The harness stays test-only; no user-visible behavior changes.

## Out of scope

- Full interactive terminal automation; performance benchmarking;
  coverage-percentage gates.
- Product behavior changes, except clearer diagnostics for environment-specific
  socket failures.
- Docs/changelog/version churn unless an implementation change makes it
  user-visible.

## Appendix — "The Rise of Test Theater" (Ben Houston, 2025)

The source article (*"The Rise of Test Theater": When AI Coders Write Tests
That Mean Nothing*) frames the quality risk this campaign targets:

- **The circularity problem:** "write tests for this function" yields
  tautologies that confirm the code does what it's written to do, not whether
  it does the *right* thing.
- **Theater vs. real testing:** generated suites create false confidence; real
  tests validate requirements, protect against regression, document intent, and
  challenge assumptions.
- **The cost:** false confidence, maintenance burden, refactoring paralysis
  (brittle tests breaking on any implementation change), missed bugs.
- **The fix:** start from specifications; write critical logic tests manually
  (TDD); use AI mainly to expand coverage, find edge cases, and implement
  property-based tests.

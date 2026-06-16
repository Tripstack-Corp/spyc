# spyc testing campaign — coverage + anti-"test theater"

**Status:** active campaign (the one after the 2026-06 deep-review
remediation, `archive/CODE_REVIEW_2026-06.md`). **Phase 1 — the `App`
workflow harness — shipped** (`App::test_app`, `src/app/test_harness.rs`).
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
- **Fuzzing (`cargo-fuzz`):** brute-force mutated input for the critical
  parsers/mutators — the keymap DSL parser (`config/dsl.rs::parse`) and the
  fuzzy matcher (`state/navigation.rs::find_match`) — to expose crashes
  `proptest` might miss. Decision: use real coverage-guided `cargo-fuzz`
  (nightly toolchain, a `fuzz/` crate); fuzz targets run **on demand**, not in
  the default `make check` gate, so the stable gate stays nightly-free.
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
| 4 | **Pane/pty workflow** | C | No `^a z`/`^a v`/new-tab/switch-close tests → `^a z` preserves `pane_height_pct`+focus; `^a v` scrollback-before-live; empty-scrollback flash; new-tab `PROJECT_HOME`; switch/close/restart routing. Fake buffers + a few real-pty smoke tests in `tests/`. | ☐ |
| 5 | **Background tasks** | C | No `^Z`/`:fg`/`gB`/divider tests → `^Z` backgrounds+keeps entry; resume shows post-bg output; `:fg`/`:fg N`; `gB`/`:task N`/`[t`/`]t` view-without-own; closing an exited viewed task promotes output; divider running/new/success/failure. | ☐ |
| 6 | **Quick-select e2e + routing edges** | C | Scanner well-covered (11 tests) but no dispatch flow → yank vs open-intent; path jump vs flash; SHA→git-show pager; Esc no-side-effects; + exited-pane flash-yet-takes-chords; pane-scroll vs pager key overlap. | ☐ |
| 7 | **MCP / env diagnostics** | D | `mcp/tests/mod.rs` (30 tests) has no sandbox-skip logic → socket EPERM/EACCES → clear "rerun under normal permissions" diagnostic, without weakening the full-perms run. | ☐ |
| 8 | **Fuzzing + snapshot expansion** | E | No fuzz targets, thin snapshot areas → `cargo-fuzz` targets for `find_match` + `config/dsl.rs::parse` (nightly, on-demand `fuzz/` crate); fill thin `insta` coverage. | ☐ |

## Bugs found by the campaign

The headline win: real bugs a *newly-added test* caught — not behaviour-
preserving refactors (which find nothing by design). A fix is a behaviour
change, so it ships as its own `fix:` PR (release-build + owner test), separate
from the test-only cluster PR that exposed it.

| Found by | Bug | Fixed in |
|----------|-----|----------|
| _(none yet)_ | Clusters 1–3 surfaced none — expected so far. #1 was a behaviour-preserving retrofit; #2's four invariants held under 256 random cases each; #3 found the session dispatch + serde back-compat correct (and the disk roundtrip / per-agent resume already had tests). Bugs are likeliest in the still-untested live-workflow clusters (4–6: pane/pty, background tasks, quick-select). | — |

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

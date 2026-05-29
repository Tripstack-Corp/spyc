# spyc test improvement plan

**Status:** plan, not yet implemented. Based on the May 2026 unit-test
assessment: `cargo test --locked --all-targets` passes with 732 tests
under normal permissions, while sandboxed runs can fail the Unix-socket
MCP tests with `Operation not permitted`.

**Goal:** reduce regressions in the dog-fooding paths where most real
bugs appear: full `App` orchestration, pane/pty behavior, Quick Select,
background tasks, session restore, and MCP socket lifecycle.

## Thesis

The existing suite is strong at isolated logic. It has broad coverage
for keymap resolution, pure state transitions, pager wrapping, path
extraction, grep/finder behavior, config parsing, and session metadata.

The remaining risk is not raw algorithm correctness. It is workflow
composition: a key routes through `App`, focus changes, pane state
mutates, a pager opens, a task changes ownership, or a session restore
chooses an agent-specific resume path. Those flows need harness-level
tests that sit between tiny unit tests and full interactive terminal
automation.

## What exists today

Current coverage is roughly:

- 732 tests total.
- Major groups: `app` 153, `ui` 139, `state` 117, `keymap` 100,
  `pane` 61, `fs` 46, `mcp` 28.
- 15 `insta` snapshots cover pager, status, prompt, and list rendering.
- Property tests exist for shell quoting, ignore masks, and keymap count
  behavior.
- Integration tests under `tests/` are intentionally small:
  filesystem basics, keymap/config round-trips, and one pane round-trip.

This is a good base. The improvement should add carefully chosen
workflow tests rather than duplicating every unit case at another layer.

## Phase 1 — App workflow harness

Add a test-only harness for realistic `App` flows.

Recommended shape:

- Keep it under `#[cfg(test)]`, either in `src/app/mod.rs` or a small
  app test-support module.
- Create isolated temp dirs and deterministic file listings.
- Provide helpers to seed files, picks, inventory, project home,
  sessions, pane tabs, task buffers, and pager state.
- Drive either `Action`s or `KeyEvent`s, depending on what each scenario
  is trying to prove.
- Expose compact assertions for mode, focus, cursor, flash message,
  pane presence, active tab, pager mount, quick-select state, task
  state, and session metadata.
- Avoid real terminal setup. Use existing state/layout/render helpers
  where possible, and keep full terminal automation out of scope.

Acceptance tests for the harness itself:

- Fresh harness starts with a deterministic cwd, listing, cursor, focus,
  and no pane.
- Harness can apply an `Action` and observe the expected `PostAction`.
- Harness can seed multiple files and assert cursor movement, filtering,
  and selection behavior.
- Harness can install fake pane/task/session data without spawning real
  agent CLIs.

## Phase 2 — High-risk workflow regressions

### Routing and focus

Add tests that prove the user-visible routing rules rather than only
the pure `app::route` classifier:

- Prompt input wins over focused pane input.
- Overlay pager consumes normal keys but allows configured meta chords.
- Lower-pane pager routes keys based on focus.
- Exited pane flashes on normal typing but still accepts pane-management
  chords.
- Pane scroll mode and pager mode do not both consume the same key.

### Pane and pty behavior

Add workflow tests around pane state transitions:

- `^a z` zooms and unzooms while preserving `pane_height_pct`.
- Zoom forces focus into the pane and restores prior focus on unzoom.
- `^a v` opens a scrollback pager with scrollback rows before live rows.
- Empty or unavailable scrollback produces the expected flash instead
  of an empty/confusing pager.
- New pane tabs default to `PROJECT_HOME` when set.
- Tab switching, close, restart, and exited-tab routing preserve the
  expected active-tab state.

Keep real pty spawning limited to a few smoke tests. Most of these can
use fake pane buffers or test-only constructors.

### Quick Select

Add tests for the end-to-end picker behavior, not just the scanner:

- Opening Quick Select scans exactly the visible pane viewport.
- Text outside the visible viewport is ignored.
- Lowercase label yanks the selected match.
- Uppercase label dispatches the open intent for URLs, paths, SHAs, and
  custom patterns.
- URL query strings are preserved and trailing sentence punctuation is
  trimmed.
- Existing paths jump the cursor; missing paths flash clearly.
- SHA open intent creates the git-show pager path.
- Escape closes the picker without side effects.

### Background tasks

Add tests around ownership and output flow:

- `^Z` from a capture pager backgrounds the running task and keeps a
  task entry.
- Output appended after backgrounding is visible when resumed.
- `:fg` resumes the most recent task; `:fg N` resumes a specific task.
- `gB`, `:task N`, `[t`, and `]t` open viewers without taking ownership.
- Closing an exited viewed task promotes rendered output into pager
  buffer history.
- Divider state distinguishes running, new output, success, and failure.

### Session restore

Add focused restore tests for agent behavior:

- Multiple saved tabs restore in order with distinct session IDs.
- Cwd, tab labels, commands, project home, and session name survive
  save/restore.
- Codex sessions spawn `codex resume <UUID>` directly.
- Claude sessions type `/resume <id>` after settle.
- Gemini records use the configured/listed resume target.
- Legacy session fields still deserialize and infer the correct agent
  kind.
- Missing or malformed agent IDs fall back to fresh command behavior
  without panicking.

## Phase 3 — MCP and environment ergonomics

The existing MCP tests are valuable because they exercise real socket
behavior. Keep them, but make failures easier to interpret.

Add or adjust tests so:

- Socket server responds to initialize and tool calls.
- Disconnect notification routes through the command channel.
- Path traversal remains blocked for file-content reads.
- Tool searches use the expected project root and ignore behavior.
- Unix-socket permission failures in restricted sandboxes produce a
  clear diagnostic that points to rerunning under normal permissions.

Do not weaken the authoritative check: `cargo test --locked
--all-targets` under normal local permissions should still run all MCP
socket tests.

## Acceptance criteria

- `cargo test --locked --all-targets` passes locally with all new tests.
- Existing snapshots remain stable unless the implementation
  intentionally changes UI output.
- New tests do not depend on the developer's real home directory, shell
  history, clipboard, running spyc instance, or installed agent CLIs.
- Tests use temp dirs and deterministic fake data.
- Real pty/socket tests are isolated and either pass under normal
  permissions or fail with clear environment-specific messaging.
- The harness is test-only and does not change user-visible behavior.

## Out of scope

- Full interactive terminal automation.
- Performance benchmarking.
- Coverage-percentage gates.
- Product behavior changes, except clearer test diagnostics for
  environment-specific socket failures.
- Documentation, changelog, or version updates unless implementation
  later changes user-visible behavior.

## Recommended order

1. Build the `App` workflow harness and add two or three smoke tests.
2. Add routing/focus workflow regressions, because they are fast and
   validate the harness.
3. Add pane zoom and scrollback tests.
4. Add Quick Select workflow tests.
5. Add background task workflow tests.
6. Add session restore tests.
7. Improve MCP socket diagnostics.

Each phase should land with its own small set of tests so failures are
easy to localize.

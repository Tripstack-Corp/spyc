# onboarding-working-map — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-working-map
Created: 2026-05-07T07:42:37.538229+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:42:37.538229+00:00
Role: scribe
Type: Note
Title: Onboarding: working map from local inspection

Spec: docs

Purpose: Per-module navigation index. spyc is a single-binary Rust crate (`Cargo.toml:10-12`); the source tree is one directory of modules under `src/`. This entry enumerates every top-level source file and the role each plays, so a contributor can find the right module on the first try.

Observed:
- Top-level workspace shape (`ls -1` at repo root):
  - Source: `src/`, `tests/`, `build.rs`, `Cargo.toml`, `Cargo.lock`.
  - Build / lint / supply-chain config: `Makefile`, `Justfile`, `bitbucket-pipelines.yml`, `deny.toml`, `rust-toolchain.toml`.
  - Docs (root level, `*.md`): `README.md`, `AGENTS.md`, `ARCHITECTURE.md`, `BUGS.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `DESIGN.md`, `FEATURES.md`, `INSTALL.md`, `LAUNCH_PREP.md`, `LICENSE`, `REFACTOR_PLAN.md`, `ROADMAP.md`, `SECURITY.md`, `TODO.md`.
  - Other dirs: `docs/` (presentation HTML, logo, screenshot), `scripts/git-hooks/` (just a `pre-commit` hook).
- Source file count: 55 `.rs` files under `src/` (`find src -name '*.rs' | wc -l` → 55). Integration test files: 2 (`tests/filesystem.rs`, `tests/keymap_roundtrip.rs`). Total `#[test]` occurrences across `src` + `tests`: 577.
- **Top-level `src/` modules** (verified by `ls -1 src/`): `app/`, `config/`, `context.rs`, `debug_log.rs`, `fs/`, `keymap/`, `key_trace.rs`, `main.rs`, `mcp.rs`, `mcp_cmd.rs`, `pane/`, `paths.rs`, `proc_cwd.rs`, `shell/`, `state/`, `sysinfo.rs`, `term_title.rs`, `ui/`.
- **Authoritative module index** lives in `AGENTS.md:38-56` and is current. Distilled here for first-read use:
  - `src/main.rs` — terminal setup/teardown, `suspend_tui` / `resume_tui`, panic hook, signal handlers, `setup_terminal`. CLI parsing via clap derive (`src/main.rs:42-73`).
  - `src/app/mod.rs` — top-level `App` struct, event loop, layout, all key dispatch. The big file (9087 lines).
  - `src/app/state.rs` — `AppState` (cursor, picks, listing, mode); domain logic with no terminal access. `apply()` returns `ApplyResult` (2671 lines).
  - `src/keymap/` — `action.rs` (Action enum), `resolver.rs`, `user.rs` (DSL), `mod.rs`. New features need an Action variant + resolver wire-up + handler in `app/mod.rs` (`AGENTS.md:60-61`, `CONTRIBUTING.md:62-71`).
  - `src/pane/` — `mod.rs` (Pane struct: spawn, I/O, scroll mode), `input.rs` (encode crossterm keys to ANSI), `widget.rs` (render `vt100::Screen` to ratatui), `quick_select.rs` (`^a u` picker), `pathref.rs` (`gf`/`gF` path extractor), `tabs.rs`.
  - `src/ui/` — list view, status bar, pager, prompt, line editor, help, theme, syntax, markdown.
  - `src/fs/` — `entry.rs`, `listing.rs`, `ops.rs`, `finder.rs` (`F` filename picker, gitignore-aware streaming walker), `grep.rs` (`:grep`, embedded ripgrep matcher, no subprocess).
  - `src/mcp.rs` — MCP server: PID-scoped Unix socket listener, stdio proxy, `.mcp.json` management, enterprise policy checking, instance takeover (2154 lines).
  - `src/mcp_cmd.rs` — Command channel types bridging MCP threads to the main event loop.
  - `src/context.rs` — Context snapshot (cwd, cursor, picks, filter, git branch, project_home, session_name) written to disk for MCP consumers.
  - `src/state/` — `cursor.rs`, `marks.rs`, `picks.rs`, `inventory.rs`, `history.rs`, `ignore.rs`, `sessions.rs`, `session_names.rs`, `harpoon.rs`, `graveyard.rs`, `health.rs`, `frecency.rs`.
  - `src/config/` — `mod.rs` (config loading), `dsl.rs` (DSL parser).
  - `src/shell/` — `mod.rs`, `expand.rs`. URL handling via the `open` crate (`AGENTS.md:50`).
  - `src/paths.rs` — XDG-compliant path resolution.
  - `src/sysinfo.rs` — RSS/PID for the `I` info overlay.
  - `src/proc_cwd.rs` — Cross-platform cwd-of-pid lookup (Linux `/proc/<pid>/cwd`, macOS `lsof -Fn`).
  - `src/term_title.rs` — Host-terminal window title; wraps OSC 2 in tmux DCS passthrough when `$TMUX` is set.
  - `src/debug_log.rs` — `spyc_debug!` macro; `$XDG_STATE_HOME/spyc/debug.log`.
  - `src/key_trace.rs` — Per-key dispatch trace (CLI flag `--key-trace`, env var `SPYC_KEY_TRACE=1`).
- **Public surface = the binary itself.** No library crate target; the only `[[bin]]` is `spyc` at `src/main.rs` (`Cargo.toml:10-12`). Public CLI flags are clap-derived in `src/main.rs:42-73`: `--resume`, `--debug`, `--key-trace`, `--mcp`, `--verbose`, `--print-config`. The MCP RPC surface is the *other* public surface and lives entirely in `src/mcp.rs` + `src/mcp_cmd.rs`.
- **Top dependencies** drive the architecture (`Cargo.toml:14-50`): `ratatui` 0.30 (rendering), `crossterm` 0.28 (terminal I/O), `clap` 4 (CLI), `notify` 6 (file watcher, macos_fsevent only), `portable-pty` 0.8 (PTY allocation), `vt100` 0.16 (terminal emulator inside the pane), `ansi-to-tui` 8, `syntect` 5 (syntax highlight), `ignore` 0.4 + `nucleo-matcher` 0.3 + `grep-searcher` 0.1 + `grep-regex` 0.1 (the `F` finder + `:grep` stack), `pulldown-cmark` 0.13 (markdown viewer), `tar` 0.4 + `zstd` 0.13 + `trash` 5 (graveyard), `uuid` 1 (v7 features), `jiff` 0.2.
- **CONTRIBUTING.md "Project structure" section (lines 161-172) is stale**: lists `src/app.rs` as a single file, but the actual layout is `src/app/` (directory module) with `mod.rs` and `state.rs`. `AGENTS.md:38` is the current source of truth.

Inferred:
- The right place to start when adding a feature is `src/keymap/action.rs` (variant + describe), then `src/keymap/resolver.rs` (binding), then either `src/app/mod.rs` (terminal-touching) or `src/app/state.rs` (pure domain) for the handler. — confidence: high — basis: explicit recipe in `AGENTS.md:60-61` and `CONTRIBUTING.md:62-71`; both name the same four-step flow.
- The natural seam for the Elm-style refactor (see `onboarding-architecture`) is widening over time: prefer State-side handlers in `src/app/state.rs` over App-side handlers in `src/app/mod.rs` whenever a feature can be expressed without terminal access. — confidence: medium — basis: `ARCHITECTURE.md:42-62` ("Update model: Elm-architecture, in progress") + `REFACTOR_PLAN.md` staged decomposition.

Next query: `watercooler_search(query="module navigation working map", thread_topic="onboarding-working-map", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-architecture` — engine principles this module index is realized against.
- `onboarding-risk-register` — picks up the `CONTRIBUTING.md` "Project structure" stale-layout drift identified here.

Provenance:
- Commands run: `find src -name '*.rs' | wc -l` (55), `find src -type f -name '*.rs' | sort`, `ls -1 src/`, `grep -rE "#\\[test\\]" --include="*.rs" src tests | wc -l` (577), `wc -l src/app/mod.rs src/mcp.rs src/app/state.rs`, `find tests -name '*.rs'` (filesystem.rs, keymap_roundtrip.rs).
- Files read: `Cargo.toml:1-50,107-109` (deps + bin), `AGENTS.md:38-56,60-61` (module index + Action recipe), `CONTRIBUTING.md:62-71,160-172` (Action flow + stale Project structure section), `src/main.rs:42-73` (CLI flags), `ARCHITECTURE.md:42-62`, `REFACTOR_PLAN.md:1-15`.
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-architecture = 01KR0P4W3ED1QZ8F44PFB2WPDZ`.

<!-- Entry-ID: 01KR0P6W0YEPJTT0C3CP48NGKV -->

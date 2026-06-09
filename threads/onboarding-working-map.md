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

---
Entry: Claude Code (caleb) 2026-06-09T05:19:37.683032+00:00
Role: scribe
Type: Note
Title: Onboarding refresh: #37 → #311 working map (decomposed src/ tree, entrypoints, MCP tool surface)

Spec: docs

Purpose: Refresh of onboarding-working-map from #37 to #311 (v1.56.0). The #37-era map listed 55 `.rs` files in a mostly-flat `src/` with two giant files (`app/mod.rs` 9087, `state.rs` 2671). The whole tree was reshaped by the MVU migration + the 800-LoC decomposition campaign: it's now 162 `.rs` files across directory modules. This entry re-enumerates the real subsystems, entrypoints, and public surfaces, and points each at its history thread.

Observed:
- **Scale churn #37→#311:** `find src -name '*.rs' | wc -l` → **162** (was 55); ~53,818 LoC; **949** test fns (was 577). The two former monoliths are gone: `app/mod.rs` is now **1009** lines (was 9087 — it's just the `App`/`Runtime`/`ViewState` struct defs + the `Message` enum + glue), and the old `app/state.rs` 2671-line file was split into `src/app/state/{apply,dispatch,git,listing,mod,navigation,selection}.rs` (campaign close at PR #307-308). Integration tests grew from 2 to 3: `tests/{filesystem,keymap_roundtrip,pane_roundtrip}.rs`.
- **Decomposed `src/` subsystem map (verified by `ls -R src`):**
  - `src/app/` — application layer (MVU). Root `mod.rs` (struct defs + `Message`). Lifecycle siblings: `bootstrap.rs` (constructor), `run.rs` (event loop), `proc.rs` (process I/O), `util.rs` (leaf helpers), `loop_steps.rs`, `scheduler.rs`. MVU machinery: `update.rs` (single `App::update` entry), `effect.rs` (the `Effect` enum + `run_effects`), `actions.rs` (`apply_inner`), `command_table.rs` (`COMMAND_TABLE`), `commands.rs`, `route.rs`, `focus.rs`. Child dirs: `key_dispatch/{confirms,prompts}`, `pager_handler/{modes,motion,pickers}`, `render/{chrome,inner,overlays}`, `state/{apply,dispatch,git,listing,navigation,selection,tests}`. Plus the pager-stream seam: `pager_stream.rs`, `grep_session.rs`, `git_view_session.rs`, `git_state.rs`, `pager_history.rs`, `tasks.rs`, `agent_status.rs`.
  - `src/git/` — in-process gix facade: `discovery.rs`, `status.rs`, `worktree.rs`, `model.rs`, `blame.rs`, `diff_model/{blob,build,mod}`. Pure infra (paths in, owned `Send` data out). Production is 100% gix; a `#[cfg(test)]` guard asserts no git subprocess in non-test code.
  - `src/ui/` — pure renderers (`model + &Theme → Vec<Line>`): `list_view`, `status`, `prompt`, `line_edit`, `help`, `theme`, `syntax`, `json`, `scrollback`, plus child dirs `markdown/{renderer,wrap,tests}`, `pager/{construct,layout,render,scroll_search,selection,tests}`, `diff_render/`, and `blame_render.rs` (the in-house git diff/show/blame view).
  - `src/agent/` — AgentProfile registry (`mod.rs` + `resume.rs`): one `AgentProfile` impl per hosted agent (claude/codex/gemini/agy/zot), `detect`/`profile_for` dispatch.
  - `src/keymap/` — `action.rs` (`Action` enum), `user.rs` (DSL), `resolver/` (binding resolution + tests).
  - `src/mcp/` — MCP server, split into `mod.rs` (facade), `server.rs` (socket transport), `protocol.rs` (JSON-RPC handlers), `config.rs` (`.mcp.json`/codex management + enterprise policy + takeover), `readers.rs` (context-file readers). (Was a single `src/mcp.rs`, 2154 lines, at #37.)
  - `src/state/` — persistence: `cursor`, `marks`, `picks`, `inventory`, `history`, `ignore`, `frecency`, `harpoon`, `graveyard`, `health`, `session_names`, `pager_positions`, the three per-agent transcript parsers (`claude_transcript`, `codex_transcript`, `agy_transcript`), and `sessions/` (dir module + tests).
  - `src/config/` — `mod.rs`, `dsl.rs`, `default.spycrc.toml`. `src/fs/` — `entry`, `listing`, `long_listing`, `ops`, `finder` (`F` picker), `grep` (`:grep`), `waking_sender` (the `fs::WakingSender` powering PagerStream wakeups). `src/pane/` — `mod` (`Pane`), `input`, `widget`, `quick_select`, `pathref`, `pty_host`, `tabs`. `src/shell/` — `mod`, `expand`.
- **Entrypoints:** the only `[[bin]]` is `spyc` at `src/main.rs` (`Cargo.toml:10-12`); `src/main.rs` (417 lines) is terminal setup/teardown + `suspend_tui`/`resume_tui` + CLI. `build.rs` (25 lines) is the build script. No library crate target yet (the `spyc-proto`/`spyc-pty`/`spyc-os` split is 2.x, not landed).
- **Public surfaces:** (1) the CLI flags in `src/main.rs`; (2) the **MCP RPC tool surface** in `src/mcp/` — verified tool names in `src/mcp/protocol.rs`: `get_spyc_context`, `navigate_to`, `pick_files`, `clear_picks`, `search_paths`, `search_inventory`, `search_content`, `search_picks`, `set_filter`, `get_file_content` (matches the `mcp__spyc__*` tools exposed to agents). The MCP module is 1574 LoC across 5 files.
- **Authoritative index is still `AGENTS.md` — and it is now current.** `AGENTS.md:38-99` already describes the decomposed tree, the three-field MVU split, the gix-migration-complete state, the `COMMAND_TABLE` registration rule, and the doc-sync checklist. Use it as the live per-module index; this entry is the history-thread cross-reference layer on top of it.

Inferred:
- The #37 working-map's "add a feature" recipe (`action.rs` → `resolver` → `app/mod.rs`/`state.rs`) is now **stale in its last step**: the handler goes in `src/app/actions.rs` (`apply_inner`) or the pure half in `AppState::apply` — **never** back into `mod.rs` (a guard test `mod_rs_stays_decomposed` enforces this), and `:`-commands register via `COMMAND_TABLE` not a hand-synced punt list. — confidence: high — basis: `AGENTS.md:83-85`.
- Each subsystem maps to a history thread, so "why is this module shaped this way" is answerable: app→`history-seg-refactor-mvu`, git→`history-seg-gix-migration`, the directory-module shape→`history-seg-module-decomposition`, agent→`history-arc-07-codex-and-mcp-bridge`, ui/pager→`history-arc-05-pager-surface`, ui/markdown→`history-seg-markdown-rendering`. — confidence: high — basis: thread titles + entry headers consulted this session.

Next query: `watercooler_search(query="module decomposition directory split src tree", thread_topic="history-seg-module-decomposition", code_path=".")`

Related:
- `onboarding-architecture` — the engine principles (MVU/gix/PagerStream) this index is realized against (also refreshed).
- `onboarding-docs-contracts` — the doc-sync surfaces; note `AGENTS.md` is now the live authoritative index.
- the history/insight corpus — `history-seg-module-decomposition` (the directory-module split, 6 entries) is the deep detail for this map; `insight-emergent-properties` covers the registration/additive-substrate growth patterns visible in the tree.

Provenance:
- Commands run: `find src -name '*.rs' | wc -l` (162), `find src -name '*.rs' | xargs wc -l | sort -rn | head` (top 1009/794/792/782), `grep -rE '#\[test\]' src tests | wc -l` (949), `ls -R src/{app,git,ui,agent,keymap,mcp,state,config,fs,pane,shell}`, `wc -l src/main.rs build.rs src/mcp/*.rs`, `grep tool-names src/mcp/protocol.rs`.
- Files read: `AGENTS.md:36-99` (current module index + conventions), `Cargo.toml:10-12` (bin target).
- History entry_ids consulted: module-decomposition campaign close (state.rs split) 01KTMMPARGNTSQB2Z67G6KBKQ0 + 800-line ceiling decision 01KTMMJVZMX3SJCBSK8YF896YP + UI/render dir split 01KTMMM1322W32NGAH5H7MWYEP + core-subsystems split 01KTMMN1CSG84TPZ4ZEFN9K62B; mcp-bridge 01KR2J1R3HXNZPAHE9118BGBQJ.
- Prior #37-era entry: 01KR0P6W0YEPJTT0C3CP48NGKV (left intact as point-in-time snapshot).

<!-- Entry-ID: 01KTND4NDENP31NJSSWC2ATZZ9 -->

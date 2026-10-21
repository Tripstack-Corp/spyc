# spyc

A vi-keyboard-driven terminal file manager in Rust, on ratatui/crossterm, in the lineage of `spy`, built to host coding agents in its pane. Single-developer project.

> **The canonical agent guide.** Claude Code loads it through `CLAUDE.md` → `@AGENTS.md`; codex, agy and others read it directly. It is in context on every turn, so it holds the rules and a map and links out for the rest, and `agents_md_stays_slim` caps its size: a feature's detail goes in [`FEATURES.md`](FEATURES.md), a module's in its own `//!` doc, a procedure in [`CONTRIBUTING.md`](CONTRIBUTING.md).

| For | Read |
|---|---|
| design decisions, `SPYC-TRAP` rationales | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| UI language, the binding taxonomy | [`DESIGN.md`](DESIGN.md) |
| what spyc does (an overview, then `docs/features/`), every key | [`FEATURES.md`](FEATURES.md), [`docs/KEYBINDINGS.md`](docs/KEYBINDINGS.md) |
| `.spycrc.toml` (an overview, then `docs/config/`): notifications, keymap DSL, Lua | [`CONFIGURATION.md`](CONFIGURATION.md) |
| activity dots, notifications, the scope registry | [`docs/AGENT_ORCHESTRATION.md`](docs/AGENT_ORCHESTRATION.md) |
| per-agent quirks: screen modes, scrollback, resume | [`docs/HARNESS.md`](docs/HARNESS.md) |
| PR workflow, merging, git traps, the prose and test rules in full | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| building and the shared target dir, releases | [`BUILD.md`](BUILD.md), [`docs/RELEASE_ENGINEERING.md`](docs/RELEASE_ENGINEERING.md) |

## What it does

spyc browses files with vi motions in one column or two (`a`, `b`) and hosts agents (`claude`, `codex`, `agy`) in a tabbed pty pane below. Each agent reaches spyc's MCP server, which lets it read and drive the TUI, manage worktrees, and report its turn as its tab's activity dot. Several projects can be open at once (#99), each with its own columns, tabs and pagers. Around that: an in-app pager (git diff/show/blame, `:grep`, markdown, images), archive browsing, picks, inventory, harpoon, a graveyard for soft deletes, session save/restore (`spyc -r`), and `.spycrc.toml` with a keymap DSL and Lua. [`FEATURES.md`](FEATURES.md) is the full reference.

## Architecture

One line per module: its `//!` doc has the rest, ARCHITECTURE.md the decisions behind it. Two guards keep the map whole. `every_app_module_is_in_the_agents_index` wants each top-level `src/app/` module as a bold **`name.rs`** entry, and `a_per_file_documented_subdir_is_documented_completely` every file of `agent_hud/`, `archive/`, `effect/` and `mouse/`.

**`src/app/`**, the MVU application layer. `App` owns three disjoint fields: `state: AppState` (the Model), `runtime: Runtime` (OS handles) and `view: ViewState` (render ephemerals). Child modules hold `impl App` blocks and read its private fields through the descendant-module rule.

- Root and types — **`mod.rs`** (the mod graph, `App`, the layer's shared enums; it doesn't grow: `mod_rs_stays_decomposed`), **`runtime.rs`** (`Runtime`; a project's tabs and streams on `ProjectRuntime`), **`view_state.rs`** (`ViewState`; a project's pagers and caches on `ProjectView`), **`message.rs`** (`Message` and `Wake`: an off-thread completion is a new `Wake` kind, never a new `Message` variant), **`matcher.rs`** (`/` search and the `=` filter).
- Loop core — **`run.rs`** (`App::run`), **`bootstrap.rs`** (`App::new`), **`update.rs`** (`App::update`, the one update entry), **`proc.rs`** (input thread, foreground exec), **`subshell_banner.rs`** (`$`'s marker line), **`util.rs`** (leaf helpers).
- **`state/`** — `AppState`, the Model: a `Commander` per column (`state/commander.rs`), `left` always and `right: Option`; `deny(unwrap_used)`. **`render/`** — the View, pure `&self`, `deny(unwrap_used)`; pre-frame settling in `prepare_*`.
- Effects — **`effect.rs`** (`Effect` and its payloads), **`effect/executor.rs`** (`run_effects`, the sole executor), **`effect/pane_text.rs`** (pane-text clipboard and navigation).
- Dispatch — **`actions.rs`** (`apply_inner`), **`command_table.rs`** (`COMMAND_TABLE`, the `:` registry), **`commands.rs`** (the terminal-touching `:` half), **`activity_diagnostics.rs`** (`:why-status`, `:activity dump`), **`key_dispatch/`** (`handle_key`, prompt editors, confirms).
- Projects (#99) — **`projects.rs`** (open, close, list and step between projects, and reopen a closed one), **`recently_closed.rs`** (which closed projects `Space j` lists, read on a worker), **`attention.rs`** (which agents need you, in every project: `Space !` and each project's roll-up), **`project_switch.rs`** (`switch_project`, re-reading the project it brings into view), **`mcp_routing.rs`** (which project an MCP call acts on: `in_project`, `land_in`, `land_closed`).
- Event loop — **`sources.rs`** (coalesce and ingest), **`pre_recv.rs`** (`drain_landed_work`, then `settle_before_recv`: a new pre-`recv` step goes in one of them, in dependency order), **`loop_steps.rs`**, **`streaming.rs`** (pull-source drains), **`pane_wake.rs`**, **`scheduler.rs`** (timers, deadlines), **`watch.rs`** (fs-watch worker, `sync_watch`).
- Pager — **`pager_handler/`** (keys, open/close, image overlay, `file_view.rs`), **`pager_stream.rs`** with **`grep_session.rs`** and **`git_view_session.rs`** (worker → streaming pager, one `stream_id` seam), **`pager_history.rs`**.
- Pure decisions — **`route.rs`** (`route_input` → `InputSink`), **`modal.rs`**, **`focus.rs`**, **`tab_completion.rs`**. Reconciles at loop bottom — **`mouse_mode.rs`** (`settle_mouse_mode`), **`process_cwd.rs`** (`settle_process_cwd`).
- **`mouse/`** — **`mod.rs`** (its types, `handle_mouse`), **`mouse/route.rs`** (pure routing; hit-tests the pointer, never keyboard focus), **`selection.rs`** (drag-select), **`scroll.rs`** (wheel), **`forward.rs`** (to a mouse-aware child), **`tab_hit.rs`** (`tab_widths`, the one source of tab geometry). ARCHITECTURE.md → "Mouse routing".
- Panes — **`pane_tabs.rs`** (tab lifecycle), **`pane_resize.rs`** (resize, zoom, spawn geometry), **`pane_input.rs`** (input queue, prompt tracking), **`pane_fork.rs`** (`^a F`), **`startup_tabs.rs`** (`[pane] tabs` and its consent), **`pane_scroll.rs`** (scroll mode, transcript scrollback), **`codex_pin.rs`** (codex rollout claims), **`shell_agents.rs`** (agents started inside shell tabs), **`navigate.rs`**, **`jump_default.rs`** (`J`'s default), **`quick_select.rs`** (`^a u`).
- Vsplit — **`vsplit.rs`** (the `^s` family, `carve_vsplit`), **`preview_ops.rs`** (preview live-reload).
- Off-thread workers (Effect → detached worker → Runtime slot → payloadless `Message` → pre-recv drain) — **`file_ops.rs`**, **`inventory_ops.rs`**, **`graveyard_ops.rs`**, **`image_ops.rs`** (every image producer's display path), **`mermaid_ops.rs`**, **`worktree_ops.rs`**, **`worktree_clean.rs`** (`safe_remove_worktree`), **`archive_ops.rs`**.
- Archive mounts — **`archive_route.rs`** (the screen: `route_archive_effect` classifies every outgoing effect, so no handler knows about mounts), **`archive.rs`** (glue) with `archive/command.rs`, `archive/screen.rs`, `archive/edits.rs`, `archive/reads.rs` and `archive/writes.rs`. ARCHITECTURE.md → "Archive mounts".
- Agents — **`agent_status.rs`**, **`agent_hud.rs`** (the native agent strip) with `agent_hud/collector.rs` and `diagnostics.rs`, **`wiring.rs`** (the `[Y/n]` and `:hooks` over `src/wiring/`), **`mcp.rs`**, **`paste_capture.rs`** (when a paste key captures the clipboard image), **`image_gallery.rs`** (`^a g`).
- Handlers and state — **`git_state.rs`**, **`harpoon.rs`**, **`graveyard.rs`**, **`clipboard.rs`**, **`config.rs`** (live reload of every project), **`lua.rs`** and **`lua_events.rs`**, **`session.rs`** (save every project), **`session_restore.rs`** (`-r`), **`startup_commands.rs`** (`spyc -c`), **`tasks.rs`** and **`capture.rs`** (`!` captures), **`find_picker.rs`**, **`prompt.rs`**, **`prompt_templates.rs`** (`[prompts]`), **`activity.rs`** (the `A` overlay), **`skill.rs`**, **`about.rs`**.

**Other crates.**

- **`src/agent/`** — one `AgentProfile` impl and one `REGISTRY` entry per hosted agent (claude, codex, agy, zot); scrape-fallback rules for states hooks can't report (`detect_rules.rs`); native chrome boundaries, so `gf`/`J` read the agent's output (`chrome/`); per-agent transcript metrics for the HUD (`codex_metrics/`, `claude_metrics/`, `agy_metrics/`) over `tail/`, a bounded follower of one session file that its own watch wakes, never a poll; `waiting.rs`, the precedence of a live `waiting` over hook `done`s.
- **`src/archive/`** — the pure archive core. A mount is an **index, not a directory**; name normalization makes a zip-slip name unrepresentable, and extraction never traverses a symlink.
- **`crates/spyc-vt-sys/`** — the FFI to libghostty-vt at a pinned commit: bindings checked in, prebuilt archives vendored for five targets (CI's test host among them) and SHA-256-checked by `build.rs`. A pin bump owes a full harness re-run; scrollback rows are the UX contract, bytes a safety valve.
- **`src/keymap/`** — the `Action` vocabulary, each with its `tier()`; resolver, DSL parser, default bindings.
- **`src/wiring/`** — the `spyc` MCP entry and status hooks, installed once in each agent's user configuration (`spyc --install-agents` / `--remove-agents`). Ownership by content, writes through symlinks, never a file deleted, nothing ever written into a project.
- **`src/skill/`** — the installable agent skill (`spyc --install-skill`); staleness keyed on a content hash, and a manifest tells spyc's update from the user's edit, which is never overwritten unprompted.
- **`src/lua/`** — embedded Lua 5.4 on its own thread, with an instruction-budget kill switch and a hard ceiling.
- **`src/pane/`** — the pty-hosted subprocess: `pty_host.rs`; `engine.rs`, the terminal-engine seam, which carries only what spyc calls; `engine_ghostty/`, the engine behind the `PaneEngine` alias; `input.rs` (key → bytes), `widget.rs`, `pathref.rs` (`gf`/`gF`).
- **`src/ui/`** — pure renderers, `model + &Theme → Vec<Line>`, and the per-frame `color_depth.rs` and `emoji_diff.rs` passes.
- **`src/fs/`** — listing, entries, file ops, `F`, `:grep`. **`src/git/`** — 100% in-process via gix, no subprocess in production (`no_subprocess_git_in_production`).
- **`src/mcp/`** — the MCP server. A connection binds at `initialize` to its agent's pane and project; `tool_schemas` is the catalogue; `SERVER_INSTRUCTIONS` stays short.
- **`src/state/`** — persisted state: marks, picks, inventory, history, sessions, harpoon, graveyard, frecency, transcript readers, the scope registry, consent records.
- Leaves — `mcp_cmd.rs`, `project_id.rs`, `context.rs`, `merge_driver.rs`, `config/`, `shell/`, `paths.rs`, `clipboard.rs`, `envset.rs`, `key_trace.rs`, `sysinfo.rs`, `proc_cwd.rs`, `proc_group.rs`, `term_title.rs`, `terminal.rs`, `notifications.rs`, `debug_log.rs`, and `main.rs`, a shim over `spyc::run()`; the guards `style_guard.rs`, `link_guard.rs`, `module_doc_guard.rs`, `size_guard.rs` and `fn_size_guard.rs`.

## MVU invariants (don't erode)

spyc is Model-View-Update. Keep these — they're what make it reason-about-able:

- **Three disjoint state types.** `state` = Model (pure domain, no OS handles); `runtime` = OS handles/channels/PtyHosts (never seen by domain logic); `view` = render ephemerals. Don't smuggle handles into the Model or domain state into Runtime.
- **One update entry.** Input flows through `App::update`; the pure transitions (`AppState::apply` / `dispatch_command` / `dispatch_prompt`) return effects as data — no OS access, unit-testable.
- **Effects are data; `run_effects` is the only executor.** Need a side effect? Add an `Effect` — never reach for the OS in a handler or the render pass.
- **Render is pure (`&self`).** Draw reads, never mutates; pre-frame settling goes in `prepare_*`. Covered by `TestBackend` + `insta` snapshots.
- **One message channel, event-driven.** Every source pushes `Message`s into one `mpsc::Receiver`; the loop blocks on `recv` (0 wakes at idle). No `event::poll` / busy-polling.
- **Dependency direction one-way.** `app` → `agent`, never the reverse; the Model never depends on `App`.

## Repository workflow

**Worktree-first for every change.** Do not edit the main checkout. This applies to code, documentation, configuration, tests, fixtures and temporary regression probes — a small change is not an exception. Read-only assessments may use the main checkout. An exception requires explicit user approval.

1. Ground yourself with `get_spyc_context` and inspect `list_worktrees`. Before creating a worktree, check that the local integration base is current.
2. Create an isolated worktree through spyc's `create_worktree`, or reuse your own task's existing worktree. Never use `git worktree` or create a task branch in the main checkout. Leave the user's view alone unless opening the worktree is wanted.
3. Claim it with `claim_worktree` and declare files with `register_scope` before editing. A scope claim does not replace worktree isolation. Pass the returned path as `root` to read tools and use an explicit worktree directory for shell commands; the pane's launch cwd may still be the main checkout.
4. Validate and hand off from that worktree, naming its branch and path. Do not commit, push or merge unless requested. For runnable changes, provide a local test build and its exact binary path after automated checks pass. The loop is **build → user tests locally → user approves → merge → next plan slice**. Do not merge or advance to the next slice before the user accepts the build. If code changes after testing, provide a fresh build for re-testing.
5. Release file scopes when done; retain the worktree claim while the task remains active. After integration, release its claim and clean up through spyc's `remove_worktree` safety checks.

If spyc or required filesystem approval is unavailable, ask before editing — do not fall back to modifying the main checkout.

## Conventions

The rules, one each. [`CONTRIBUTING.md`](CONTRIBUTING.md) has the longer forms and the incidents behind them.

- **Rust house style (load-bearing divergences — don't "modernize" away):** sync-only — `std::thread` + `mpsc`, no async runtime; off-thread work is a detached thread that wakes the loop with a `Message`. Errors are **`anyhow`**. `.unwrap()`/`.expect()` are allowed in production **with a comment stating the invariant** (`SPYC-TRAP` when the failure is silent), except under `deny(clippy::unwrap_used)` in `src/app/state/` and `src/app/render/` (write `.expect("invariant")`).
- **Action dispatch:** a new feature is an `Action` variant + a keymap binding + a handler arm in `actions.rs` (`apply_inner`) or the pure half in `AppState::apply`. Not `mod.rs`.
- **Binding taxonomy — global / frame / pane:** **GLOBAL** (worktree, project, session) on the leader (`Space`, or `^a Space` from the pane); **FRAME** (the file view) on the letter, `g`, `H` and `[`/`]` chords; **PANE** (pty pane + split) on the `^a` (`^w`) prefix. Tagged on `Action::tier()` and guarded by `leader_and_pane_namespaces_respect_tiers`. A rarely used feature gets a `:` command, not a default key (re-bindable via `map KEY command <name>`). DESIGN.md → "Binding taxonomy".
- **Keep `src/app/` modularized.** New logic goes in the matching child module or a new `src/app/<feature>.rs`, never `mod.rs` (`mod_rs_stays_decomposed`: extract, don't bump the ceiling).
- **No `.rs` over 1,000 lines of non-test code, or 3,000 in all; no function over 100 lines** (counted as clippy's `too_many_lines`; tests exempt). Extract a cohesive module or helper. `src/size_guard.rs` and `src/fn_size_guard.rs` are ratchets: a pin may only shrink, is lowered when the guard asks, deleted once under, never raised, and a moved function takes its pin along.
- **Glue stays with its types; leaves move out.** **Pure decisions get extracted and tested** (the `route.rs` / `focus.rs` template: `Copy` snapshot + pure fn + unit tests). **Refactors are behaviour-preserving**: relocations don't edit test assertions.
- **`:command` registration goes through `COMMAND_TABLE`** (`command_table.rs`): one `CommandSpec` naming its `CmdHandler::Pure` / `::App(fn)`. Symbol commands (`!`, `;`, `!!`) dispatch directly.
- **Never pipe the gate.** `make check | tail` reports `tail`'s exit status, so a failed gate reads green. Run it unpiped, or `make check-ci`, whose verdict line `=== GATE: PASS ===` / `=== GATE: FAIL ===` survives a tail.
- **Guards fail closed.** A source-scan guard splits production from tests with `guard_support::production_half`, never `split("#[cfg(test)]")`, and asserts it found something to check: a guard that silently checks nothing is worse than none.
- **No OS in the pure layers (enforced).** The Model and the draw pass don't block, spawn, read env or fork. Blocking work goes off-thread on the `graveyard_ops` template; the render half is guarded by `app::render::purity_guard` (`PURE_DRAW`).
- **Prose is Canadian English** (`colour`, `centre`, `defence`, `grey`) with the American `-ize`/`-yze` endings (`organize`, never `organise`). Prose only: code keeps upstream's names (`Color::Rgb`, `--color`). **Backticks are the contract**: the guard (`prose_is_canadian_english`) skips code spans, so config keys, flags, env vars, file names and API items always wear backticks in prose.
- **Comments state what IS, not what's planned.** No "for now" or "until X lands"; a comment explains a non-obvious decision, invariant or gotcha, never narrates the code or reasoning in progress (`comments_carry_no_reasoning_leakage`).
- **Load-bearing trap anchors (`SPYC-TRAP`).** An invariant whose failure is silent gets `// SPYC-TRAP(<slug>): <one-liner>` at the code site and a `<!-- SPYC-TRAP: <slug> -->` rationale in ARCHITECTURE.md. Read the rationale before editing; never delete an anchor without instruction (`traps_resolve_against_architecture_anchors`).
- **Reach a column by handle.** `cur()`/`cur_mut()` is where the user works, `col(side)`/`get_col(side)` a named column, `active_sides()`/`columns_mut()` every open one; only render and fs-watch name `left`/`right`. Holding a `side`, use `rebuild_rows_for(side)`: `rebuild_rows()` rebuilds `cur()` and leaves the other column drawing stale rows (`columns_are_addressed_through_handles`).
- **A flashed error carries its cause:** `{e:#}`, never `{e}`, with context at the failure site, since a bare `?` on an fs call names no path (`flashed_errors_render_their_whole_chain`).
- **No hardcoded version literals** — a `<x.y.z>` placeholder; the truth is `crate::VERSION` / `Cargo.toml`. **Don't bump the version in a PR**: `main` carries `N.M.0-CURRENT` (or an rc's `N.M.0-rc.K` during its soak) until the release PR.
- **Test the requirement, not the implementation**, and **a test is evidence only once you've watched it fail**: break what it covers, see it report, put it back. A finding naming N sites is closed when N are fixed, and a differential comparison normalizes both sides through one function. Assert intent through the matchers in `effect.rs`.
- **Repaint:** event-driven dirty frames with `needs_draw` reason codes, `needs_full_repaint` for teardown transitions, DEC 2026 around every frame, rows cached via `list_generation`; 0 draws per second at idle.
- **Pane I/O:** keys via `input::encode_key(ev, app_cursor)`, raw bytes via `pane.send_bytes()`, paste bracketed. Panes `exec` their command; `^z` suspends a non-shell tab with `SIGSTOP` (never `SIGTSTP`), and a shell tab forwards it. The invocation policy is `shell::pane_invocation` (SPYC-TRAP `pane-shell-rc-double-source`).
- **Keep docs in sync** in the same commit: the `docs/features/` page (and `FEATURES.md`'s summary when an area changes), `docs/KEYBINDINGS.md`, `src/ui/help.rs`, `CONFIGURATION.md` or its `docs/config/` page, `README.md`, `INSTALL.md` as a change touches them; `ARCHITECTURE.md` on an architectural decision, `DESIGN.md` on a UI-language one, `ROADMAP.md` for strategy only (work items are GitHub Issues). **AGENTS.md takes a new module's index line or a new rule, never a feature's detail.** **A user page says what a user sees and does; how it works goes in the module's `//!` doc or ARCHITECTURE.md**, and a new page gets a summary and a link in its hub. Guarded: links (`every_relative_markdown_link_resolves`), section references in quotes or `§N` (`every_section_reference_resolves`), hub links (`every_feature_and_config_page_is_linked_from_its_hub`), `:` commands (`every_command_is_documented_for_users`), default key bindings in `docs/KEYBINDINGS.md` and the `?` overlay (`every_default_binding_is_in_the_keymap_reference`, `…_in_the_help_overlay`), module docs (`every_module_opens_with_a_module_doc`), hub size (`hub_pages_stay_overviews`).

### Commits, merges, CHANGELOG

- **A commit subject is a conventional `type(scope): subject` naming the actual scope.** Under squash merge the **PR title is the CHANGELOG entry** (git-cliff, `cliff.toml`; the body is never parsed), so a PR spanning two types ships under one: split it.
- **Never `git reset FETCH_HEAD`**, and suspect `GIT_DIR` when a worktree resets to someone else's branch. CONTRIBUTING.md → "Git traps".
- **Squash on merge**, and merge only on `mergeStateStatus == CLEAN`, never on the check list, in a bounded loop; no `--delete-branch`. Tear down with `remove_worktree`, then `git branch -D` once the branch's own files match `main`. CONTRIBUTING.md → "Merging a PR".
- **Releases** take two steps: `make release-prep VERSION=x.y.z` on a release branch, then `make release-tag VERSION=x.y.z` on `main` once that PR merged; an rc takes the same steps and gets no CHANGELOG section. `CHANGELOG.md` must open with `cliff.toml`'s `header` exactly. docs/RELEASE_ENGINEERING.md.

## Building

```sh
cargo build / cargo build --release   # or: make release
make install      # release build + copy to ~/.local/bin
make check        # fmt + clippy + test (CI gate; supply-chain is audit.yml)
make check-ci     # the same, ending in a verdict line that survives a pipe
make lint-linux   # clippy for Linux-gated code
make package-check  # package both crates + build from the tarballs, as `cargo publish` will
make fuzz         # nightly + cargo-fuzz, on-demand (NOT in check)
make prune-target # drop dead build output from a shared target dir; run nightly
make changelog    # preview the pending CHANGELOG section
```

The crate is lib + bin: `src/lib.rs` owns every module and `run()`, so `fuzz/` (its own workspace) can link it through the `pub mod fuzz` facade. **Never `cargo clean` with a shared target dir** (`[build] target-dir` or `CARGO_TARGET_DIR`): it deletes every worktree's cache, so `make clean` refuses. `make prune-target` on a schedule is the cleanup. BUILD.md → "Build output and disk".

## MCP tools (spyc integration)

You're expected to run inside spyc's pane. Prefer spyc's tools over shell equivalents; the `spyc` skill (`spyc --install-skill`) and each tool's description have the detail.

- **Ground yourself** with `get_spyc_context`: from an agent pane it adds `pane` (your tab's cwd, worktree root and branch, which the user's view need not match) and `project`.
- **Report status** as your turn changes: `report_status` `working`, `blocked` when you need the user, `done`, or `waiting` with a `note` when background work will wake you.
- **Search and git in-process**: `search_paths`, `search_content`, `search_picks`, `search_inventory`; `git_status`, `git_log`, `git_diff`. Each read tool takes `root` to target another worktree.
- **Worktrees, never `git worktree`**: `list_worktrees` (`merged` is the safe-to-remove signal), `create_worktree` (branches off the **local** `main`: pull it first), `open_worktree`, `claim_worktree` / `release_worktree`, `remove_worktree` / `clean_worktree` (archive to the graveyard, refuse a claimed tree). They act on your project, never the directory the user is browsing.
- **Coordinate merges**: `register_scope` before touching files, `list_scopes`, `wait_for_scope_clear` before merging overlapping ones, `release_scope` when done.
- **Drive the TUI**: `navigate_to`, `pick_files` / `clear_picks`, `set_filter`, `get_file_content`.

If the spyc MCP tools are NOT available, remind the user: "I don't see the spyc MCP tools — are we running inside spyc? This project is built to be dog-fooded through the spyc pane."

## Dog-fooding context

The developer runs Claude Code in spyc's lower pane, and bugs and features are often found that way: anything affecting the agent pane experience is high priority. Develop and test from inside spyc.

## Working directory continuity

Each Bash call is a fresh subprocess in your *launch* cwd; `cd /foo` does not persist, which sends `make` and `cargo` to the wrong place. Use `cd /foo && cmd` or absolute paths (`make -C <worktree> test`), and when a command fails unexpectedly, run `pwd && ls` before retrying.

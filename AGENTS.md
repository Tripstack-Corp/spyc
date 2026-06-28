# spyc

A vi-keyboard-driven terminal file manager in Rust, on ratatui/crossterm. Inspired by SideFX's `spy`. Single-developer project.

> **Canonical agent guide** — the MVU contract, the per-module map, and the
> day-to-day conventions, in one place so every tool reads the same source
> (Claude Code loads it via `CLAUDE.md` → `@AGENTS.md`; codex / agy / others read
> it directly). Deeper design detail lives in [`ARCHITECTURE.md`](ARCHITECTURE.md);
> UI language in [`DESIGN.md`](DESIGN.md); the full feature reference in
> [`FEATURES.md`](FEATURES.md). **Keep this file slim** — it's always in context.

## What it does

One line per feature; see [`FEATURES.md`](FEATURES.md) for the full reference.

- **Vi navigation** — motions, marks, numeric prefix (`3j`, `5G`), cursor jumps.
- **Chord hints** — hold a chord prefix (`g`, `^a`, `H`, `W`, `y`, `m`, `[`/`]`, …) and after `chord_hint_delay_ms` (default 300) a which-key popup lists the continuations. The discovery surface for the dense keymap.
- **Leader / global menu** — `Space` (list focus) or `^a Space` (from the pane) opens the global/workspace menu: `Space w l|n|d` worktree list/new/delete, `Space p`/`Space P` jump/set PROJECT_HOME, `Space s` session info, `Space ?` help. `Space` is literal text to the child, so the pane path is `^a Space` (the `^a` interception wakes spyc, then `Space` enters the menu — `is_spyc_meta_when_pane_focused` + `PendingSeq::Leader`). `W l|n|d` stays a list-focus alias. (`gh` is gone → `Space p`; `gw` worktree-root jump stays as frame nav.)
- **Embedded pty pane** (horizontal split) with tabs, primarily hosting `claude` for dog-fooding. Divider shows the active tab's live cwd (`↪` when drifted). `^a z` zooms the active region (tmux-style fullscreen toggle).
- **MCP server** on a PID-scoped Unix socket — Claude (`.mcp.json`) and codex (`.codex/config.toml`) both re-exec `spyc --mcp` as a stdio proxy to the one socket. Configs written lazily on agent-pane launch, cleaned up on exit. Queries context + mutates the TUI. Multiple instances coexist (takeover prompted). See ARCHITECTURE.md → "MCP server".
- **`gf`/`gF`** — jump from pane output to the referenced file (or `file:line`); honors scroll mode.
- **`^a u` Quick Select** (wezterm-style) — label URLs / paths / SHAs / IPv4 / custom-regex matches in the visible pane; lowercase yanks, uppercase opens. Custom patterns in `.spycrc.toml` `[[scan.patterns]]`.
- **In-app pager** — search, ANSI, hex-dump, line numbers, `:N` jump, save. Closed views go on a back/forward buffer history (`:bprev`/`:bnext`, `[b`/`]b`, `gp`).
- **Shell prompt** — vi-editable, persistent history (`!` capture, `;` foreground, `$` interactive shell); `!?` history editor.
- **`:` command line** — vim-style (`:limit`, `:!cmd`, `:!!`, `:;cmd`, `:fg`, `:task`, `:grep`, `:bprev`/`:bnext`, `:why-status`, `:graveyard`, `:activity`, `:longlist`, `:filetype`, `:chmod`, `:setenv`, `:q`, …). Less-frequent features ship as `:` commands so they don't each consume a default key; re-bind one via `map KEY command <name>`.
- **Agent-activity dots** — each agent tab carries a live activity dot: P0 output-timing (spicy heat-pulse `●` while output flows, quiet `·` when idle) overridden by P1 semantic self-report via the `report_status` MCP tool (`Working`/`Blocked`/`Done`). `:why-status` explains the active tab. For *auto*-report, spyc injects `SPYC_MCP_SOCK` + a stable `SPYC_PANE_ID` into the agent pane and ships a one-shot `spyc --report-status <state>` reporter (`mcp::report_status_to_socket`). Charter: `docs/AGENT_AWARENESS_PLAN.md`.
- **Project-wide search** — `F` fuzzy filename finder (gitignore-aware walker); `:grep` content search via embedded ripgrep, streamed into a pager so `gf`/`gF` work.
- **Background tasks** — `^Z` backgrounds a running `!` capture; `:fg` resumes, `gB`/`:task N`/`[t`/`]t` peek. Rendered `[N+]`/`[N●]`/`[N✓]`/`[N✗]` in the divider.
- **`=` limit filter** — temporary glob (`=*.rs`, `=!` picks, `=git`/`=g` git status, `=h` harpoon, `=` clears).
- **Harpoon** — per-worktree pinned file/dir list (max 9). `H` chord: `Ha` append, `Hx` remove, `H1`..`H9` jump, `Hh` menu. Per-column (lives on `Commander`), keyed by worktree root.
- **Picks** (per-dir multi-select) + **inventory** (persistent file cache).
- **Graveyard** — soft-delete recovery for `R` / inventory expulsions (`<uuid>.json` + `.tar.zst`). `:graveyard` viewer (no default key — keymap-slimmed; re-bind via `map KEY command graveyard`), `p`/`P` restore, `:undo`. FIFO-cascade to trash at 500 MB.
- **Session save/restore** — auto-saved on quit with a spice name (e.g. `SAFFRON_CUMIN`); `spyc -r` resumes tabs, agent conversations, and the vsplit.
- **`PROJECT_HOME`** — sticky per-session project root (auto-set from `.git`). `Space p` jumps, `gP`/`Space P` sets, `:project` manages. New panes default here. Exposed via MCP.
- **Status bar** — `🌶️ | PROJECT_HOME | SESSION | path | git | suffix`; `[layout] status_position` flips top/bottom. Host terminal title set to `🌶️: <project> · <session>`.
- **`.spycrc.toml`** — keymap DSL, themes, ignore masks, layout, live reload. The DSL binds a key to a built-in action, a `unix` shell template, a `patternpick`/`jump`, or a `:` command (`map KEY command <name>`); `unix`/`command`/`jump` are `is_executing`, so only `$HOME` config may bind them. `spyc --print-config` emits a commented template.

## Architecture

Deep design decisions live in [`ARCHITECTURE.md`](ARCHITECTURE.md) (sync-only, MVU, repaint, persistence, MCP, vsplit, git/gix, trap anchors); UI language in [`DESIGN.md`](DESIGN.md). Below is a per-module navigation index.

**`src/app/`** — the application layer (MVU). `App` owns three disjoint fields: `state: AppState` (Model — pure domain, no OS handles), `runtime: Runtime` (OS handles/channels/PtyHosts), `view: ViewState` (render ephemerals/caches). New handler/render/command/action logic goes in the matching child module below — **never back into `mod.rs`** (ceiling-guarded). Child modules read `App`'s private fields via the descendant-module rule.

- **`mod.rs`** — module root: the three struct defs + `Message` enum + small glue (`sh_c`, `row_from_entry`, `Matcher`, `open_help`).
- Loop core — **`run.rs`** (`App::run`, dispatch/render/teardown), **`bootstrap.rs`** (`App::new`), **`proc.rs`** (input-reader thread + foreground-exec runner), **`update.rs`** (`App::update`, the single update entry), **`util.rs`** (App-independent leaf helpers).
- **`state/`** — `AppState`, the Model. Per-column browser fields (`listing`, `cursor`, `rows`, `picks`, `masks`, `temp_filter`, `view`, sort, `list_generation`) bundle into a `Commander`; `left` always present, `right: Option<Commander>` for the second column. The update path reaches the focused column via `cur()`/`cur_mut()`; render addresses `left`/`right` explicitly. App-wide state (marks, inventory, graveyard, harpoon, pane, focus, config, mode, flash, vsplit, git) stays flat. `impl` splits by domain (`navigation`/`selection`/`listing`/`git`/`apply`/`dispatch`); `deny(unwrap_used)`.
- **`render/`** — the View (pure `&self`); `deny(unwrap_used)`. `mod` = frame lifecycle (`compute_layout`, `render`, `prepare_*`); `inner`/`chrome`/`overlays` paint.
- **`effect.rs`** — the `Effect` enum + `run_effects`, the **sole** side-effect executor. Handlers return `Vec<Effect>`; only this touches the OS.
- **`command_table.rs`** — the `COMMAND_TABLE` `:`-registry; each entry names its handler (`CmdHandler::Pure` / `::App(fn)`) so an unhandled command is a build error.
- Dispatch — **`actions.rs`** (`apply_inner`, the `Action` dispatcher), **`commands.rs`** (terminal-touching `:` half), **`key_dispatch/`** (`handle_key` router + prompt editors + confirm handlers).
- Event-loop machinery — **`sources.rs`** (channel coalesce + ingest), **`loop_steps.rs`** (pre-recv drain/refresh; chord-hint settle), **`streaming.rs`** (pull-source drains; stamps `last_output_at`), **`pane_wake.rs`**, **`scheduler.rs`** (timers/deadlines incl. agent idle/anim), **`watch.rs`** (off-thread fs-watch worker).
- Pager — **`pager_handler/`** (vi-key handling, open/close hub, modal image overlay), **`pager_stream.rs`** + **`grep_session.rs`** + **`git_view_session.rs`** (the "worker → waking channel → streaming pager" abstraction; all ride one `stream_id` / `Message::PagerStreamOutput`), **`pager_history.rs`**.
- Pure decisions (snapshot + fn + tests) — **`route.rs`** (`route_input` → `InputSink`, shared by key + paste), **`modal.rs`** (the transient `Modal` axis), **`focus.rs`** (`decide_focus` / `recompute_focus`).
- Panes — **`pane_tabs.rs`** (tab lifecycle/focus), **`pane_scroll.rs`** (scroll mode + scrollback; `^a v` auto-engages an alt-screen agent's on-disk transcript), **`codex_pin.rs`** (spawn-ordered rollout claim), **`navigate.rs`**, **`quick_select.rs`** (`^a u`).
- Vsplit — **`vsplit.rs`** (`^a |` preview cycle; `^s n`/`^s x` second Commander; `cur()`/`col(side)`; `carve_vsplit` geometry), **`preview_ops.rs`** (off-thread live-reload of the split preview).
- Off-thread workers (Effect → detached worker → Runtime slot → payloadless Message → pre-recv drain) — **`file_ops.rs`** (copy/move/pipe + `OpenSpecialFile`), **`inventory_ops.rs`**, **`graveyard_ops.rs`**, **`mermaid_ops.rs`**, **`worktree_ops.rs`** (MCP worktree create/remove/clean), **`worktree_clean.rs`** (`safe_remove_worktree`, the shared safe teardown).
- App-layer result handlers — **`git_state.rs`**, **`harpoon.rs`**, **`graveyard.rs`** (`:graveyard` viewer), **`clipboard.rs`**, **`mcp.rs`**, **`agent_status.rs`** (status short-id + P0 activity derive), **`config.rs`** (live reload).
- Session / misc state — **`session.rs`** (save/restore, `-r` picker), **`tasks.rs`** + **`capture.rs`** (`!` shell-capture state), **`find_picker.rs`** + **`prompt.rs`** (small data structs), **`activity.rs`** (`A`-overlay monitor).

**Other crates:**

- **`src/agent/`** — agent profile registry; one `AgentProfile` impl per hosted agent (claude/codex/gemini/agy/zot). Adding an agent = one impl + one `REGISTRY` entry.
- **`src/keymap/`** — `Action` enum (`action.rs` — the full vocabulary of behaviors, each tagged with its `tier()`), resolver, DSL parser, default bindings.
- **`src/pane/`** — pty-hosted subprocess: `mod.rs` (`Pane`), `input.rs` (key→ANSI), `widget.rs` (vt100→ratatui), `quick_select.rs`, `pathref.rs` (`gf`/`gF`).
- **`src/ui/`** — pure renderers (`model + &Theme → Vec<Line>`): list, status bar, pager, prompt, help, theme; `syntax.rs` (syntect), `markdown/`, `diff_render/` + `blame_render.rs` (in-house git diff/show/blame, `|` toggles split/unified).
- **`src/fs/`** — listing, entry types, file ops; `finder.rs` (`F`), `grep.rs` (`:grep`).
- **`src/git/`** — git facade, **100% in-process via gix, no subprocess in production** (guard: `no_subprocess_git_in_production`). `discovery`/`status`/`branch`/`worktree`/`model`/`diff_model`/`blame`. Hot path: 1 Hz mtime poll reads the cached gitdir; gix opens only on chdir-to-new-repo + HEAD change.
- **`src/mcp/`** — MCP server (`mod`/`server`/`protocol`/`config`/`readers`). Read tools scope to the focused column's `search_root`, each with an optional `root` to target another worktree. `initialize` carries `SERVER_INSTRUCTIONS` (steers the agent to spyc tools; keep short).
- **`src/state/`** — cursor, marks, picks, inventory, history, masks, sessions + session_names, harpoon, graveyard, frecency (`J`), pager_positions, health, agent transcript readers (`*_transcript.rs` → `^a v` scrollback).
- Leaves — **`src/mcp_cmd.rs`** (MCP↔loop channel types), **`src/context.rs`** (context snapshot for MCP), **`src/config/`**, **`src/shell/`**, **`src/paths.rs`** (XDG), **`src/clipboard.rs`**, **`src/envset.rs`** (`:s` env overrides, no `unsafe`), **`src/key_trace.rs`**, **`src/sysinfo.rs`**, **`src/proc_cwd.rs`**, **`src/term_title.rs`** (OSC 2 + tmux passthrough), **`src/debug_log.rs`**, **`src/main.rs`** (thin shim over `spyc::run()`).

## MVU invariants (don't erode)

spyc is Model-View-Update. Keep these — they're what make it reason-about-able:

- **Three disjoint state types.** `state` = Model (pure domain, no OS handles); `runtime` = OS handles/channels/PtyHosts (never seen by domain logic); `view` = render ephemerals. Don't smuggle handles into the Model or domain state into Runtime.
- **One update entry.** Input flows through `App::update`; the pure transitions (`AppState::apply` / `dispatch_command` / `dispatch_prompt`) return effects as data — no OS access, unit-testable.
- **Effects are data; `run_effects` is the only executor.** Need a side effect? Add an `Effect` — never reach for the OS in a handler or the render pass.
- **Render is pure (`&self`).** Draw reads, never mutates; pre-frame settling goes in `prepare_*`. Covered by `TestBackend` + `insta` snapshots.
- **One message channel, event-driven.** Every source pushes `Message`s into one `mpsc::Receiver`; the loop blocks on `recv` (0 wakes at idle). No `event::poll` / busy-polling.
- **Dependency direction one-way.** `app` → `agent`, never the reverse; the Model never depends on `App`.

## Conventions

- **Rust house style (load-bearing divergences — don't "modernize" away):** sync-only — `std::thread` + `mpsc`, no async runtime; off-thread work is a detached thread that wakes the loop with a `Message`. Errors are **`anyhow`** (app, not library). `.unwrap()`/`.expect()` allowed in production **with a comment stating the invariant** (`SPYC-TRAP` when the failure is silent) — no blanket ban, *except* `deny(clippy::unwrap_used)` in `src/app/state/` and `src/app/render/` (write `.expect("invariant")`).
- **Action dispatch:** new feature = `Action` variant + keymap binding + handler arm in `actions.rs` (`apply_inner`) or the pure half in `AppState::apply`. Not `mod.rs`.
- **Binding taxonomy — global / frame / pane (a guarded contract):** every binding lives in one tier with one home (DESIGN.md → "Binding taxonomy"). **GLOBAL** (workspace ops — worktree/project/session) on the **leader** (`Space`, or `^a Space` from the pane); **FRAME** (the file view — git/picks/sort/marks/nav) on the letter / `g` / `H` / `[`/`]` chords; **PANE** (pty pane + split) on the `^a` (`^w`) prefix. Tagged on `Action::tier()`; the guard `leader_and_pane_namespaces_respect_tiers` fails the build if a non-`Global`/`Meta` action lands on the leader or a non-`Pane`/`Meta` action on `^a`. Policy: keep rarely-used features `:`-command-only rather than spending a default key (re-bindable via `map KEY command <name>`).
- **Keep `src/app/` modularized.** New render/key/command/action/session logic goes in the matching child module (or a new `src/app/<feature>.rs`), not appended to `mod.rs` — the pattern is a child module with `impl App {…}` reading private fields via the descendant rule. Guard: `mod_rs_stays_decomposed` (extract, don't bump the ceiling).
- **No `.rs` over ~800 lines without a solid reason.** Extract a cohesive child/sibling module (verbatim, behavior-identical). A module root holding its own *type defs* is a legit reason; a pile of helpers is not.
- **Glue stays with its types; leaves move out.** A helper building the module's own types is glue; a helper with no `App` dependency belongs in a `util`-style module.
- **Pure decisions get extracted + tested** (the `route.rs` / `focus.rs` template: `Copy` snapshot + pure fn + unit tests).
- **Refactors are behavior-preserving.** Relocations don't edit test assertions; `make check` / `make lint` / `make test` (+ `make lint-linux` for OS-gated code) stay green.
- **`:command` registration goes through `COMMAND_TABLE`** (`command_table.rs`): one `CommandSpec` entry naming its `CmdHandler::Pure` / `::App(fn)`. A missing Pure arm is caught by `command_table_*` tests; a missing App handler is a compile error. Symbol commands (`!` / `;` / `!!`) dispatch directly.
- **No OS in the pure layers (enforced).** The Model and the `&self` draw pass must not block, spawn, read env, or fork. Move blocking ops off-thread via the `graveyard_ops` template (Effect → `run_effects` spawns a detached worker → result on a Runtime slot + payloadless `Message` → pre-recv drain). The render half is a source-scan guard: `app::render::purity_guard` (add the cleaned module to `PURE_DRAW`).
- **Comments state what IS, not what's planned.** No "for now" / "until X lands" / "with the Y PR" — they rot into lies. A comment earns its place by explaining a non-obvious decision/invariant/gotcha; never narrate what the code says, never commit reasoning-in-progress. Guard: `comments_carry_no_reasoning_leakage`.
- **Load-bearing trap anchors (`SPYC-TRAP`).** The rare invariant whose failure is *silent* gets `// SPYC-TRAP(<slug>): <one-liner>` at the code site + a `<!-- SPYC-TRAP: <slug> -->` rationale section in ARCHITECTURE.md (the slug is the join key). Read the rationale before editing such code; never delete an anchor without instruction. Guard: `traps_resolve_against_architecture_anchors`. Full procedure: ARCHITECTURE.md → "Load-bearing trap anchors".
- **`state.left`/`right` is a SPECIFIC column; use `cur()` for "where the user is working."** Render + fs-watch legitimately name `left`/`right`, but a spawn cwd / restore target / op target must go through `cur()`/`cur_mut()` so a focused second commander is honored. Guard: `state_left_listing_dir_uses_are_allowlisted`.
- **No hardcoded version literals** — use a `<x.y.z>` placeholder; the source of truth is `crate::VERSION` / Cargo.toml.
- **Test the requirement, not the implementation** (charter: `docs/TEST_IMPROVEMENT_PLAN.md`): start from an invariant, add negative tests, reach for `proptest` on wide input spaces, and generate edge-case *data* (not assertions) with AI. Decouple assertions from struct layout — assert intent via the test-only matchers in `effect.rs` (`fx.change_dir()`, …), the single place that destructures (with `..`).
- **Repaint:** event-driven dirty-frame; `needs_draw` reason codes (pane=1, event=2, other=3), `needs_full_repaint` for teardown transitions; DEC 2026 sync output wraps every frame; rows/grid cached via `list_generation`. Target 0 dps at idle.
- **Pane I/O:** keys via `input::encode_key()`, raw bytes via `pane.send_bytes()`, paste wrapped in `\x1b[200~`…`\x1b[201~`. Prefix `^a` (`^w` alias); `^a ↓` sends a literal `^a` to the child.
- **Keep docs in sync (same commit, not a follow-up):** for user-visible / keybinding / status changes update the affected ones of `README.md`, `FEATURES.md`, `AGENTS.md` (the module index is guard-checked: `every_app_module_is_in_the_agents_index`), `ARCHITECTURE.md` (only on an architectural decision), `DESIGN.md` (only on a UI-language change), `ROADMAP.md`, `BACKLOG_DRAFT_NOTES.md`, `CHANGELOG.md`, `INSTALL.md`, `src/ui/help.rs`.
- **Bump version** in `Cargo.toml` on user-visible changes (patch = fix, minor = feature); see `CONTRIBUTING.md`.

### Commits, merges, CHANGELOG

- **Commit subject = actual scope, not its caption.** A commit touching a feature + a version bump says both (`feat: gemini agent + bump cargo-deny`). The body holds the long form.
- **Squash on merge** (`bkt pr merge <N> --strategy squash`) — `main`'s log becomes one commit per shipped shape.
- **`CHANGELOG.md` is git-cliff-generated from v1.57.0** (config `cliff.toml`): the section comes from the commit *type*, the line from `scope: subject` — so **the commit message _is_ the changelog entry** (a category-spanning PR wants multiple well-typed commits). v1.56.0 and earlier are frozen hand-written history, left verbatim. Preview with `make changelog`; release with `make release-tag VERSION=x.y.z`.

## Building

```sh
cargo build / cargo build --release   # or: make release
make install      # release build + copy to ~/.local/bin
make check        # fmt + clippy + test + deny (CI gate)
make fuzz         # nightly + cargo-fuzz, on-demand (NOT in check)
make changelog    # preview the pending CHANGELOG section
make release-tag VERSION=x.y.z        # bump + prepend changelog + commit + tag
```

**Crate shape: lib + bin.** `src/lib.rs` owns every module + the `run()` entry point; `src/main.rs` is a thin shim. The split lets `fuzz/` (a standalone workspace; nightly, on-demand) link the lib. New fuzz entry points go through the `pub mod fuzz` facade in `lib.rs`, not by widening module visibility.

## Roadmap

See [`ROADMAP.md`](ROADMAP.md).

## MCP tools (spyc integration)

You're expected to run inside spyc's split pane. If `get_spyc_context` is available, prefer spyc's tools over shell equivalents:

- **Ground yourself:** `get_spyc_context` (cwd, cursor, picks, filter, git branch, project_home, pid + version) — avoids asking "which file?".
- **Drive the TUI:** `navigate_to`, `pick_files` / `clear_picks`, `set_filter`; `get_file_content` (relative paths resolve against spyc's cwd).
- **Show status:** `report_status(working|blocked|done)` as your turn changes — `blocked` lights the tab dot hot-red ("needs me"). Drives the activity dot, overrides timing. Cheap + idempotent.
- **Search (prefer over `Bash rg/grep`):** `search_paths` (fuzzy filenames), `search_content` (gitignore-aware regex); plus the spyc-only `search_picks` (inside the current multi-select) and `search_inventory` (the persistent yank cache).
- **Git (prefer over shelling out):** `git_status`, `git_log`, `git_diff` (working tree vs HEAD; `cached:true` staged vs HEAD; `unstaged:true` index vs worktree; optional `paths`) — in-process.
- **Worktrees (never `git worktree`):** `list_worktrees` (branch, dirty counts, current, ahead/behind/`merged` — the safe-to-remove signal — and `locked`); `create_worktree(branch, base?, open?)` (sibling dir off the main repo); `open_worktree(path)` (opens in column b while a stays put); `remove_worktree` / `clean_worktree(path)` (safe-by-default: archives untracked + uncommitted to the graveyard, removes, deletes the branch iff merged; refuses a claimed one). Coordinate with `claim_worktree(path, reason)` / `release_worktree(path)`.
- **Scoping to another worktree:** the read tools (`search_paths` / `search_content` / `get_file_content` / `git_status` / `git_log` / `git_diff`) take an optional `root` (absolute path) to target a worktree other than the focused column — pass the path from `create_worktree` / `list_worktrees`.

If the spyc MCP tools are NOT available, remind the user: "I don't see the spyc MCP tools — are we running inside spyc? This project is built to be dog-fooded through the spyc pane."

## Dog-fooding context

The developer runs Claude Code CLI in spyc's lower pane. Bugs and features are often found through this workflow — anything affecting the Claude Code pane experience is high priority. Develop and test from inside spyc.

## Working directory continuity (you, Claude)

No shell continuity between Bash calls — each is a fresh subprocess inheriting your *original* launch cwd; `cd /foo` does **not** persist. This causes loops (`make` fails "no targets", commands run in the wrong place). Avoid it: use the compound form (`cd /foo && cmd`), prefer absolute paths (`make -C /Users/.../spyc test`), and if a `make`/`cargo` command fails unexpectedly run `pwd && ls` before retrying. spyc shows the pane's actual subprocess cwd as `── ↪ <path>` when drifted, but for Claude the process cwd never moves — only your expectation does.

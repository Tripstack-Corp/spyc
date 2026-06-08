# spyc

A vi-keyboard-driven terminal file manager written in Rust, built on ratatui/crossterm. Inspired by SideFX's `spy`. Single-developer project.

## What it does

- Vi-style navigation, marks, cursor motion, and numeric prefix (`3j`, `5G`)
- Embedded pty pane (horizontal split) with tabs for running subprocesses — primarily used to host `claude` CLI for dog-fooding. The divider line shows the active tab's *live* cwd (polled via `/proc/<pid>/cwd` on Linux, `lsof` on macOS, ~1Hz cache); when it drifts from the spawn cwd it gets a `↪` marker. `^a z` zooms the pane (tmux-style fullscreen toggle): list collapses to 0 rows, divider shows `[ZOOM]`, focus is forced into the pane and restored on un-zoom; `pane_height_pct` is preserved so the prior split returns on un-zoom.
- MCP server on a PID-scoped Unix socket — Claude Code discovers spyc via `.mcp.json`, codex via `.codex/config.toml`; both registrations re-exec `spyc --mcp` as a stdio proxy that forwards through to the same socket, so a single MCP server backs every supported agent. Both files carry `SPYC_MCP_SOCK` in their env block. Queries context (cwd, cursor, picks, filter, git branch), and can mutate the TUI (navigate, filter, pick). Multiple instances coexist; takeover is prompted (`PID N already owns MCP here. Take over? [Y/n]`) so a second spyc doesn't silently steal MCP from the first — the prompt detects either claude's or codex's prior entry. Enterprise policies are claude-specific: `deniedMcpServers`/`allowedMcpServers` in `managed-settings.json` gate the entry; if a Jamf-deployed `managed-mcp.json` already defines a server named `spyc`, the per-project `.mcp.json` write is suppressed (org config wins on the name; we'd just collide) and any prior local `spyc` entry is removed.
- `gf`/`gF` — jump from Claude's output to the referenced file (or file:line). Honors scroll mode: when scrolling, scans exactly the visible viewport (not a fixed slice).
- `^a u` — Quick Select picker (wezterm-style): scan visible pane for URLs / paths / git SHAs / IPv4 / custom-regex matches, overlay 1- or 2-letter labels, lowercase = yank to clipboard, uppercase = open (URLs → `open`/`xdg-open`, paths → cursor-jump, SHAs → `git show` in pager). Custom patterns in `.spycrc.toml` `[[scan.patterns]]` with optional `url = "https://.../{}"` template.
- In-app pager with search, ANSI rendering, hex-dump, line numbers, `:N` jump, save
- Vi-editable shell prompt with persistent history (`!` captured, `;` foreground, `$` interactive shell)
- `!?` history editor — popup with vi-editable lines, `/search`, `G`/`gg`, `:N` jump, `Ctrl+D` delete
- `:` command line — vim-style command entry (`:limit`, `:!cmd`, `:!!`, `:;cmd`, `:fg`, `:task`, `:grep`, `:bprev`, `:bnext`, `:q`)
- Project-wide search — `F` opens a fuzzy filename finder (gitignore-aware walker on a worker thread, multi-repo descent into sibling-clone subrepos); `:grep <pattern>` is a project-wide content search via the embedded ripgrep matcher (`grep-regex` + `grep-searcher`, no subprocess), streams `path:line:col: text` into a pager so `gf`/`gF` jump for free.
- Background tasks — `^Z` while a `!` capture pager is open sends the running task to the background; reader thread keeps draining output into a per-task buffer (head-truncated at 1 MB). `:fg` (or `:fg N`) resumes; `gB` / `:task N` / `[t`/`]t` open a peek "task viewer" without taking ownership. Tasks render as `[N+]`/`[N●]`/`[N✓]`/`[N✗]` in the pane divider (right-aligned, distinct color from pane tabs). On close of a viewed-and-exited task, the rendered view is promoted into buffer history.
- Pager buffer history — closed pager views go onto a back/forward stack (max 10). `:bprev`/`:bnext` walk it from the prompt; `[b`/`]b` chord walks it from inside an open pager; `gp` reopens the most-recent closed buffer from the file list. The help overlay is excluded from the stack.
- `=` limit filter — temporary glob filtering (`=*.rs`, `=!` for picks, `=git`/`=g` for files in `git status`, `=h` for harpoon, `=` clears)
- Harpoon — small per-project pinned list of file/dir pointers (max 9 slots) for muscle-memory navigation. `H` is now a chord prefix: `Ha` append, `Hx` remove, `H1`..`H9` jump (chdir + cursor), `Hh` open menu (j/k, K/J reorder, dd delete). `=h` filters the listing to harpoon entries (with ancestor dirs). Persisted at `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml` per `PROJECT_HOME`. `H` was previously an alias for `Home`; that role is now `~` / Home key only.
- Picks (per-directory multi-select) and inventory (file cache with graveyard)
- Graveyard — soft-delete recovery for `R` and inventory expulsions. Each entry is `<uuid>.json` (metadata) + `<uuid>.tar.zst` (compressed payload, file or dir tree). Mode bits / mtime preserved via tar `HeaderMode::Complete`. `gy` opens the viewer (newest first); `p` restores to cwd, `P` to original path (refuses to clobber), `dd`/`x` purges entry to system trash, `Z` purges all (confirm). `:undo` is a one-shot restore-most-recent-to-original. At startup, if the graveyard exceeds 500 MB, oldest entries FIFO-cascade to the system trash and a flash reports the count. Pre-v1.41.0 paired `<uuid>.json` + `<uuid>.dat` entries are silently ignored (no migration; major version bumps may lose recovery state).
- Session save/restore — auto-saved on quit with a spice-themed name (e.g. `SAFFRON_CUMIN`), `spyc -r` resumes tabs and agent conversations (Claude, Codex, Gemini, and Antigravity UUIDs sniffed from each agent's exit banner).
- `PROJECT_HOME` — sticky per-session project root. Auto-set when launch dir has `.git`. `gh` jumps, `gP` sets, `:project` manages. New pane tabs default their cwd to `PROJECT_HOME`. Exposed via MCP context.
- Top bar: `🌶️ | PROJECT_HOME | SESSION_NAME | path | git | suffix`. `user@host` dropped from the bar; flash with `gU` / `:whoami`, or see it in the `I` overlay. Position is configurable: `[layout] status_position = "bottom"` flips it to the last row (vim/tmux convention; useful inside tmux to avoid double status bars).
- Host terminal title is set to `🌶️: <project> · <session>` (basename of `PROJECT_HOME` · `SESSION_NAME`); pre-spyc title is restored on quit. Inside tmux the OSC 2 is wrapped in DCS passthrough so iTerm2 (etc.) sees it — needs `set -g set-titles on` in tmux for the outer-tab title to actually update.
- `.spycrc.toml` config with keymap DSL, themes, ignore masks, layout, live reload. `spyc --print-config` emits a fully-commented default template.

## Architecture

For stable architectural decisions (sync-only / `std::thread + mpsc`,
MVU shape, threading model, repaint strategy, persistence layout,
MCP transport) see [`ARCHITECTURE.md`](ARCHITECTURE.md). For UI design
language (component names, surface vocabulary, key-binding philosophy,
extension checklist) see [`DESIGN.md`](DESIGN.md). The list below is a
per-module navigation index.

- **`src/app/`** — The application layer. Decomposed from a former ~12k-line `mod.rs` monolith and migrated to MVU: `App` now owns three disjoint fields — `state: AppState` (Model), `runtime: Runtime` (OS handles/channels/PtyHosts), `view: ViewState` (render ephemerals/caches). `mod.rs` (~1k) is the module root — the three struct defs + the `Message` enum + a little glue; the constructor (`bootstrap.rs`), the event loop (`run.rs`), process I/O (`proc.rs`), and the leaf helpers (`util.rs`) are siblings. Everything below is a child module whose `impl App` methods read `App`'s private state via the descendant-module rule (so fields stay private — only the few cross-module entry points are `pub`). **New handler/render/command logic goes in the matching module below — or a new one — not back into `mod.rs`** (a test guards its line count; see Conventions).
  - **`mod.rs`** — the module root: `App` / `Runtime` / `ViewState` / `FrameLayout` struct defs, the `Message` enum, and a little glue (`sh_c` → `Effect`, `row_from_entry` → `RowData`, the `Matcher`, `open_help`).
  - **`run.rs`** — the event loop: `App::run` + its scratch-builder `run_setup`, the per-iteration `dispatch_effective` / `render_frame`, and `run_teardown`.
  - **`bootstrap.rs`** — `App::new`: config + args → the initial `Listing` / `Runtime` / `ViewState`, resolver wiring, session restore, MCP arm.
  - **`proc.rs`** — process I/O for the loop: the parkable crossterm input-reader thread (`spawn_input_reader`) and the TUI-teardown foreground-exec runner (`ForegroundExec`).
  - **`update.rs`** — `App::update(UiMsg)`, the **single update entry**: routes `Action` / `BoundAction` / `Prompt` to the pure producers and returns their effects.
  - **`util.rs`** — App-independent leaf helpers: time/byte/text formatting, path + user/host display, a capped subdir walk, a process-group kill, an untracked-file diff.
  - **`state/`** — `AppState`, the **Model**: pure domain state (cursor, picks, listing, mode, filter, focus, git display state) with no OS handles; the pure/testable Update half of MVU. `mod` holds the type defs (`AppState` + the result/`GitCache`/`PaneLayout`/`GitState`/`Focus` types) + small helpers; the `impl AppState` methods split by domain into `navigation` (cursor/jumps), `selection` (picks/marks/inventory), `listing` (rebuild/filter/chdir), `git` (status refresh/cache), `apply` (the `Action` dispatcher), and `dispatch` (`dispatch_command`/`dispatch_prompt`). Tests live in `tests/` (thematic).
  - **`command_table.rs`** — the `COMMAND_TABLE` `:`-command registry. Each entry carries its handler — `CmdHandler::Pure` (resolved in `AppState::dispatch_command`) or `CmdHandler::App(fn)` (terminal-touching, in `commands.rs`) — so a registered command with no handler is a *build error*.
  - **`effect.rs`** — the `Effect` enum + `run_effects`, the **sole** side-effect executor (clipboard, signals, send-to-pane, terminal title, foreground exec, pane-text reads, chdir). Handlers return `Vec<Effect>`; only this runs them.
  - **`render/`** — the View half: the frame lifecycle — layout (`compute_layout`), the `render` entry point, and the pre-draw settle (`prepare_frame`/`prepare_panes`/`settle_list_grid`) — lives in `mod`; the painting is delegated to `inner` (the main draw pass `render_inner`), `chrome` (pane status-line/divider, status-bar header, list-rows cache), and `overlays` (harpoon menu + activity `A` monitor).
  - **`sources.rs`** / **`loop_steps.rs`** / **`streaming.rs`** / **`pane_wake.rs`** / **`scheduler.rs`** — the event-loop machinery: channel coalescing + per-source ingest (`sources.rs`), the pre-recv drain/refresh steps (`loop_steps.rs`), streaming pull-source drains (`streaming.rs`), pane wake plumbing, and the timer/deadline scheduler.
  - **`key_dispatch/`** — `handle_key` top-level router + `apply_user` (`mod`), the prompt / vi-prompt editors (`prompts`), and the remove / graveyard-purge / Claude-crash-recover confirm handlers (`confirms`).
  - **`pager_handler/`** — the in-app pager overlay's vi-style key handling. `mod` holds the `handle_pager_key` router (delegates each input context to a sub-handler returning `Some`/`None`) + the pager open/close/build hub; `modes` (contextual `^C`, `/` search typing, `:N` jump buffer, `[`/`]` chords, placement/visual), `pickers` (jump-history / worktree / history-editor / session overlays), and `motion` (the scroll/vi-motion fall-through) hold the sub-handlers.
  - **`commands.rs`** — `App::dispatch_command`, the terminal-touching half of `:` command dispatch (the pure-domain half is `AppState::dispatch_command`).
  - **`actions.rs`** — `apply` / `apply_inner`, the `Action` dispatcher, plus post-action harpoon reconcilers.
  - **`session.rs`** — session save / restore, the `-r` picker pager, the session-info overlay.
  - **`tasks.rs`** / **`capture.rs`** — `BackgroundTask`(s) and `PendingCapture`: backgrounded and foreground `!` shell-capture state.
  - **`find_picker.rs`** / **`grep_session.rs`** / **`pager_history.rs`** / **`prompt.rs`** — small data structs for the `F` finder, `:grep` session, pager back/forward history, and the input prompt.
  - **`route.rs`** / **`focus.rs`** — pure decisions behind a `Copy` snapshot + pure fn + tests (no TUI needed): `route.rs` does `KeyEvent` → destination routing, `focus.rs` does `^W j/k` keyboard-focus selection. The template for further pure-decision extraction.
  - **`pane_tabs.rs`** / **`pane_scroll.rs`** / **`navigate.rs`** / **`quick_select.rs`** — pane tab lifecycle + focus, pane scroll mode, directory navigation, and the `^a u` quick-select yank.
  - **`git_state.rs`** / **`harpoon.rs`** / **`graveyard.rs`** / **`clipboard.rs`** / **`mcp.rs`** / **`agent_status.rs`** / **`config.rs`** — App-layer handlers for git-worker results, the harpoon menu, soft-delete restore/purge, clipboard yank routing, MCP command application, off-thread agent-status, and live config reload.
- **`src/agent/`** — Agent profile registry. One `AgentProfile` impl per hosted AI agent (claude/codex/gemini/agy); `detect(cmd)` / `profile_for(kind)` replace per-agent `match AgentKind` dispatch (detection, resume save/restore, transcript scrollback, status short-id, picker label, exit summary). Adding an agent = one impl + one `REGISTRY` entry. `AgentKind` (in `state/sessions/`) stays the persistence tag; profiles carry behavior.
- **`src/keymap/action.rs`** — `Action` enum: the full vocabulary of user-observable behaviors. Every keybinding maps to an `Action`.
- **`src/keymap/`** — Resolver, user keymap DSL parser, default bindings.
- **`src/pane/`** — Pty-hosted subprocess. `mod.rs` is the `Pane` struct (spawn, I/O, scroll mode), `input.rs` encodes crossterm keys to ANSI, `widget.rs` renders `vt100::Screen` to ratatui, `quick_select.rs` is the `^a u` picker (regex scan + label assignment over visible pane text), `pathref.rs` is `gf`/`gF`'s path extractor.
- **`src/ui/`** — Widgets: list view, status bar, pager, prompt, line editor, help, theme. Pure renderers (`model + &Theme → Vec<Line>`, no IO/gix): `syntax.rs` (syntect), `markdown/` (`mod` types + `render` entry, `renderer` event→lines state machine, `wrap` helpers, `tests`), and `diff_render/` (`mod` renderer + `tests`) / `blame_render.rs` (the in-house git diff/show/blame view — unified + side-by-side layouts over PR 7's `DiffModel`/`BlameModel`, mounted via the `git_view_session` worker; `|` toggles split⇄unified).
- **`src/fs/`** — Directory listing, entry types, file operations. `finder.rs` backs the `F` filename picker (gitignore-aware streaming walker, nucleo fuzzy match); `grep.rs` backs `:grep` (embedded ripgrep matcher streaming `path:line:col: text` matches).
- **`src/git/`** — Git integration facade: the single boundary owning every git operation, **100% in-process via `gix` (gitoxide) — no `git` subprocess in production** (migration complete). `discovery.rs` (repo-root/gitdir/branch), `status.rs` (`repo_status` index/worktree/tree walk → `StatusEntry`s + `map_to_listing`), `worktree.rs` (list/create/remove), `model.rs` (pure owned `DiffModel`/`BlameModel`/`CommitMeta`), `diff_model/` (gix→`DiffModel` for `gd`/`gD`/`show`, `gix-diff` + imara-diff hunk reassembly), `blame.rs` (gix `blame_file` → `BlameModel`). Pure infra (paths in, owned `Send` data out — no `App`, no ratatui). Diff/show/blame models are built off-thread by the `app/git_view_session` worker and rendered in-house by `ui/diff_render` + `ui/blame_render` (unified + side-by-side, word-level highlighting). App-layer git glue — worker-result application, error flashing, the git-view session/drain/toggle — stays in `src/app/{git_state,git_view_session}.rs`. **Strangler closed:** a `#[cfg(test)]` guard in `git/mod.rs` (`no_subprocess_git_in_production`) asserts zero `git`-subprocess spawns in non-test code; the only `git`-binary usages left are test fixtures that build scratch repos. **Hot-path rule:** the 1 Hz git mtime poll reads the cached `current_gitdir` (no gix open); gix opens only at chdir-into-a-new-repo + HEAD change.
- **`src/mcp/`** — MCP server (`mod` facade + `run`/socket paths, `server` socket transport, `protocol` JSON-RPC handlers, `config` `.mcp.json`/codex management + enterprise policy + instance takeover, `readers` context-file readers): PID-scoped Unix socket listener, stdio proxy for Claude Code.
- **`src/mcp_cmd.rs`** — Command channel types bridging MCP threads to the main event loop.
- **`src/context.rs`** — Context snapshot (cwd, cursor, picks, filter, git branch, project_home, session_name) written to disk for MCP consumers.
- **`src/state/`** — Cursor, marks, picks, inventory, history, ignore masks, sessions, session_names (spice-pair generator), harpoon (per-project pinned file list), graveyard (soft-delete cache as `<uuid>.json` + `<uuid>.tar.zst` pairs; FIFO cascade to system trash at 500 MB).
- **`src/config/`** — Config loading and DSL parser.
- **`src/shell/`** — Shell expansion and command execution. Cross-platform "open URL with system handler" goes through the `open` crate (`open::that_detached`), used by Quick Select's "open" intent.
- **`src/paths.rs`** — XDG-compliant path resolution for state, config, and cache directories.
- **`src/sysinfo.rs`** — System info (RSS, PID) for the `I` info overlay.
- **`src/proc_cwd.rs`** — Cross-platform "cwd of pid N" lookup (Linux `/proc/<pid>/cwd`, macOS `lsof -Fn`). Used to surface the live pane subprocess cwd in the divider.
- **`src/term_title.rs`** — Host-terminal window title (push/pop/set). Wraps OSC 2 in tmux's DCS passthrough when `$TMUX` is set so iTerm2 etc. receive the title.
- **`src/debug_log.rs`** — `spyc_debug!` macro; writes to `$XDG_STATE_HOME/spyc/debug.log`.
- **`src/main.rs`** — Terminal setup/teardown, `suspend_tui`/`resume_tui` for child processes.

## Conventions

- **Action enum dispatch**: New features get an `Action` variant, a keymap binding, and a handler arm in `src/app/actions.rs` (`apply_inner`) — or the pure-domain half in `AppState::apply`. Not in `mod.rs`.
- **Keep `src/app/` modularized (don't regrow the monolith)**: `app/mod.rs` was a ~12k-line monolith; the `REFACTOR_PLAN.md` decomposition + the MVU migration + the 800-LoC campaign carved it down to ~1k (the `App`/`Runtime`/`ViewState` defs, the `Message` enum, and a little glue — the constructor, event loop, process I/O, and leaf helpers are sibling modules). New render/key/command/action/session logic belongs in the matching child module (or a new `src/app/<feature>.rs`), **not** appended to `mod.rs`. The pattern is a child module with `impl App { … }`: child modules can read `App`'s private fields via the descendant-module rule, so you almost never need to make a field `pub` — only the handful of methods called from `app` or sibling modules. A test (`app::guard_tests::mod_rs_stays_decomposed`) fails if `mod.rs` grows past its ceiling; if you hit it, extract a module rather than bumping the number.
- **`:command` registration goes through `COMMAND_TABLE`** (`src/app/command_table.rs`): every `:`-command is one `CommandSpec { name, handler, completion }` entry, where `handler` is `CmdHandler::Pure` (resolved in `AppState::dispatch_command`) or `CmdHandler::App(fn)` (terminal-touching, in `src/app/commands.rs`). State runs first; the table drives tab-completion and the Pure→App routing, so you add a table entry plus its handler together — no hand-synced punt list. A missing **Pure** arm is caught by the `command_table_*` tests; a missing **App** handler is now a **compile error** (the handler fn-pointer is named in the entry). Symbol commands (`!`, `;`, `!!`) are dispatched directly and stay out of the table. Bitten historically on `:undo` (v1.41.1) and the `:limit`/`:`-history split — both now structurally prevented.
- **Milestone spikes**: Development proceeds in numbered milestones (M4, M6, M8, M9, M10...).
- **Repaint strategy**: Event-driven dirty-frame rendering. `needs_draw` flag with reason codes (pane=1, event=2, other=3). `needs_full_repaint` for teardown transitions (pager close, overlay close). DEC 2026 synchronized output wraps every frame. `build_rows()` and grid stabilization are cached via `list_generation` counter. Target: 0 dps at idle.
- **Pane I/O**: Keys go through `input::encode_key()`. Raw bytes use `pane.send_bytes()`. Bracketed paste wraps text in `\x1b[200~`...`\x1b[201~` before forwarding. Pane prefix is `^a` (screen-style), `^w` works as alias.
- **Keep docs in sync**: When committing changes that affect user-visible behavior, keybindings, or project status, update **all** of the following that are affected:
  - `README.md` — positioning, install instructions, keybinding tables
  - `FEATURES.md` — complete feature reference
  - `AGENTS.md` — module index, conventions, "what it does" summary
  - `ARCHITECTURE.md` — only when an *architectural decision* changes (concurrency model, MVU shape, persistence, etc.); not for routine features
  - `DESIGN.md` — only when the *UI design language* changes (a new surface type, a new naming convention, palette change); not for routine features
  - `ROADMAP.md` — move shipped items to Done, update track status
  - `BUGS.md` — move fixed bugs to FIXED section
  - `CHANGELOG.md` — add entry under Unreleased
  - `INSTALL.md` — if build/install steps change
  - `src/ui/help.rs` — if keybindings or user-facing commands change
  Do not batch doc updates as a follow-up — include them in the same commit as the code change.
- **Bump version**: Always bump the version in `Cargo.toml` when shipping user-visible changes. Patch for fixes, minor for features. See `CONTRIBUTING.md` for SemVer policy.

### Commits, merges, and CHANGELOG

External catalogue review (the *watercooler* analysis platform) caught
three recurring patterns worth correcting going forward. These aren't
human-author rules — they're observations about how *agents* working
on this repo tend to drift.

- **Commit subject = actual scope, not its caption.** If a commit
  touches both a feature and a `Cargo.toml` version bump, the subject
  should mention both: `feat: gemini agent + bump cargo-deny` rather
  than `feat: gemini agent`. Bare-feature subjects systematically
  understate diff scope (watercooler's `insight drift` — pattern:
  *commit-subject vs diff-scope understatement*). The body of the
  message can still hold the long form.

- **Squash on merge.** Use `bkt pr merge <N> --strategy squash`
  rather than `merge_commit`. `main`'s `git log` becomes one commit
  per shipped "shape" instead of the current three-entry shape (the
  feature commit, a merge commit, and the deletion of the feature
  branch). Future forensic readers — including watercooler-style
  retrospective passes — get a cleaner story per change.

- **CHANGELOG bucket = user-observable nature, not file location.**
  A version bump shipped alongside a feature still belongs under
  `Added`. A CI tweak that happens to touch `Cargo.lock` still belongs
  under `Internal`. Don't pick the bucket by which directory the diff
  landed in (watercooler's *bucket-vs-content asymmetry*). When a PR
  legitimately spans multiple user-observable categories — e.g. a
  feature plus a doc rewrite — split into multiple entries under the
  correct headers rather than dropping everything in one bucket.

## Building

```sh
cargo build            # dev build
cargo build --release  # release build
make release           # release build via Makefile
make install           # build release + copy to ~/.local/bin
make check             # fmt + clippy + test (CI gate)
make                   # see Makefile for all targets
```

## Roadmap

See `ROADMAP.md` for current plans and track status.

## MCP tools (spyc integration)

You are expected to be running inside spyc's split pane. If the
`get_spyc_context` MCP tool is available, use it proactively:

- **Before answering questions about files:** call `get_spyc_context`
  to see what the user is looking at (cwd, cursor, picks, filter,
  git branch). This avoids asking "which file?" when the answer is
  on their screen.
- **When the user asks you to organize files:** use `set_filter`,
  `pick_files`, `clear_picks`, and `navigate_to` to update the TUI
  directly rather than giving instructions for the user to do manually.
- **To read a file the user is viewing:** use `get_file_content`
  with relative paths (resolved against spyc's cwd).
- **For project-wide search:** prefer `search_paths` (fuzzy
  filenames) and `search_content` (gitignore-aware regex over file
  contents) over `Bash rg/grep`. Both are PROJECT_HOME-scoped and
  return structured JSON. Two more are uniquely spyc-shaped:
  `search_picks` searches only inside the user's currently-picked
  files (a TUI multi-select you can't see otherwise), and
  `search_inventory` searches the user's persistent yanked-cache
  across sessions.

If the spyc MCP tools are NOT available, remind the user:
"I don't see the spyc MCP tools — are we running inside spyc?
This project is built to be dog-fooded through the spyc pane."

## Dog-fooding context

The developer uses spyc with Claude Code CLI running in the lower
pane. Bugs and features are often discovered through this dog-fooding
workflow — if something affects the Claude Code pane experience, it's
high priority. Always develop and test from inside spyc.

## Working directory continuity (you, Claude)

You don't have shell continuity between Bash tool calls. Each
invocation is a fresh subprocess that inherits your *original*
launch cwd — `cd /foo` in one call does **not** persist to the
next. This is a real source of loops: `make` fails with "no
targets specified" or commands run in the wrong place, and you
keep retrying without realizing the cwd reverted.

How to avoid it:
- For one-off commands in another directory, use the compound
  form: `cd /foo && cmd`. The cd applies only to that subshell.
- Prefer absolute paths in the command itself
  (`make -C /Users/.../spyc test`).
- If a `make`/`cargo`/test command fails unexpectedly, run
  `pwd && ls` first before retrying — verify the cwd before
  diagnosing the command. If you find yourself "stuck", check
  `pwd` before anything else.

Spyc surfaces the lower pane's *actual* subprocess cwd in the
divider line as `── ↪ <path>` when it has drifted from the
spawn cwd, but for Claude specifically the process cwd never
moves — only your internal expectation does. Hence this note.

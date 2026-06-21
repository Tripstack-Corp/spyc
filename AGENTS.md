# spyc

A vi-keyboard-driven terminal file manager written in Rust, built on ratatui/crossterm. Inspired by SideFX's `spy`. Single-developer project.

## What it does

- Vi-style navigation, marks, cursor motion, and numeric prefix (`3j`, `5G`)
- Embedded pty pane (horizontal split) with tabs for running subprocesses — primarily used to host `claude` CLI for dog-fooding. The divider line shows the active tab's *live* cwd (polled via `/proc/<pid>/cwd` on Linux, `lsof` on macOS, ~1Hz cache); when it drifts from the spawn cwd it gets a `↪` marker. `^a z` zooms the **active** region (tmux-style fullscreen toggle). Pane focused → the pane fills the screen (list collapses to 0 rows, `[ZOOM]` on the divider) and a **single spyc status line stays at the top**, showing status normally and flipping to flash / chord-arming / prompt when active so messages stay visible. List focused → the list fills the screen with a **single pane tab bar kept at the bottom** (the pty runs off-screen, `[ZOOM]` in the top status bar); from there `^a <n>` fullscreens that tab, and creating a new pane reveals the split. Focus stays on whichever region was active (no forced focus), `^a j`/`^a k` are inert while zoomed (only `^a z` exits), and `pane_height_pct` is preserved so the prior split returns on un-zoom.
- MCP server on a PID-scoped Unix socket — Claude Code discovers spyc via `.mcp.json`, codex via `.codex/config.toml`; both registrations re-exec `spyc --mcp` as a stdio proxy that forwards through to the same socket, so a single MCP server backs every supported agent. Both files carry `SPYC_MCP_SOCK` in their env block. Each is written lazily when its agent pane launches (`open_pane_tab_in` → `ensure_agent_mcp_config`), not at startup, so directories where no agent is ever run don't get a stray config written into them; on exit `cleanup_written_mcp_configs` removes the entries we wrote (and an emptied file / `.codex/` dir), leaving a successor's entry, other servers/config, and any git-tracked file untouched (warning on stderr for the last). Queries context (cwd, cursor, picks, filter, git branch), and can mutate the TUI (navigate, filter, pick). Multiple instances coexist; takeover is prompted (`PID N already owns MCP here. Take over? [Y/n]`) so a second spyc doesn't silently steal MCP from the first — the prompt detects either claude's or codex's prior entry. Enterprise policies are claude-specific: `deniedMcpServers`/`allowedMcpServers` in `managed-settings.json` gate the entry; if a Jamf-deployed `managed-mcp.json` already defines a server named `spyc`, the per-project `.mcp.json` write is suppressed (org config wins on the name; we'd just collide) and any prior local `spyc` entry is removed.
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
- Harpoon — small per-**worktree** pinned list of file/dir pointers (max 9 slots) for muscle-memory navigation. `H` is now a chord prefix: `Ha` append, `Hx` remove, `H1`..`H9` jump (chdir + cursor), `Hh` open menu (j/k, K/J reorder, dd delete). `=h` filters the listing to harpoon entries (with ancestor dirs). Persisted at `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml`, keyed by the focused column's worktree root (else `PROJECT_HOME`) via `AppState::harpoon_root` — **per-column** (lives on `Commander`), so `b` in a separate worktree gets its own list and never jumps into `a`'s copy. `App::reconcile_harpoon` swaps a column's list when its root shifts (chdir into a different worktree / the `ChangeDir` effect). `H` was previously an alias for `Home`; that role is now `~` / Home key only.
- Picks (per-directory multi-select) and inventory (file cache with graveyard)
- Graveyard — soft-delete recovery for `R` and inventory expulsions. Each entry is `<uuid>.json` (metadata) + `<uuid>.tar.zst` (compressed payload, file or dir tree). Mode bits / mtime preserved via tar `HeaderMode::Complete`. `gy` opens the viewer (newest first); `p` restores to cwd, `P` to original path (refuses to clobber), `dd`/`x` purges entry to system trash, `Z` purges all (confirm). `:undo` is a one-shot restore-most-recent-to-original. At startup, if the graveyard exceeds 500 MB, oldest entries FIFO-cascade to the system trash and a flash reports the count. Pre-v1.41.0 paired `<uuid>.json` + `<uuid>.dat` entries are silently ignored (no migration; major version bumps may lose recovery state).
- Session save/restore — auto-saved on quit with a spice-themed name (e.g. `SAFFRON_CUMIN`), `spyc -r` resumes tabs and agent conversations (Claude, Codex, Gemini, and Antigravity UUIDs sniffed from each agent's exit banner; zot via `--continue`).
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
  - **`state/`** — `AppState`, the **Model**: pure domain state with no OS handles; the pure/testable Update half of MVU. The **per-browser** fields a file-commander column owns — `listing`, `cursor`, `rows`, `grid_dims`, `picks`, `masks`, `temp_filter`, `view`, `sort_order`/`sort_reversed`, `list_generation` — are bundled into a `Commander` sub-struct. `AppState.left` is always present; `right: Option<Commander>` is `None` until a second column is opened (vsplit Stage 2 PR C, the feature). The pure-Model update path (`apply`/`dispatch`/`navigation`/`selection`/`listing`) reaches the **focused** column via `cur()` / `cur_mut()` — while `right` is `None`, both resolve to `left`, so it is behavior-preserving; render addresses `left`/`right` explicitly. App-wide state (marks, inventory, graveyard, harpoon, pane, focus, config, mode, flash, vsplit, git/git_cache) stays flat on `AppState`. `mod` holds the type defs (`AppState` + `Commander` + the result/`GitCache`/`PaneLayout`/`GitState`/`Focus` types) + small helpers; the `impl AppState` methods split by domain into `navigation` (cursor/jumps), `selection` (picks/marks/inventory), `listing` (rebuild/filter/chdir), `git` (status refresh/cache), `apply` (the `Action` dispatcher), and `dispatch` (`dispatch_command`/`dispatch_prompt`). Tests live in `tests/` (thematic).
  - **`command_table.rs`** — the `COMMAND_TABLE` `:`-command registry. Each entry carries its handler — `CmdHandler::Pure` (resolved in `AppState::dispatch_command`) or `CmdHandler::App(fn)` (terminal-touching, in `commands.rs`) — so a registered command with no handler is a *build error*.
  - **`effect.rs`** — the `Effect` enum + `run_effects`, the **sole** side-effect executor (clipboard, signals, send-to-pane, terminal title, foreground exec, pane-text reads, chdir, off-thread graveyard ops). Handlers return `Vec<Effect>`; only this runs them.
  - **`render/`** — the View half: the frame lifecycle — layout (`compute_layout`), the `render` entry point, and the pre-draw settle (`prepare_frame`/`prepare_panes`/`settle_list_grid`) — lives in `mod`; the painting is delegated to `inner` (the main draw pass `render_inner`), `chrome` (pane status-line/divider, status-bar header, list-rows cache), and `overlays` (harpoon menu + activity `A` monitor).
  - **`sources.rs`** / **`loop_steps.rs`** / **`streaming.rs`** / **`pane_wake.rs`** / **`scheduler.rs`** / **`watch.rs`** — the event-loop machinery: channel coalescing + per-source ingest (`sources.rs`), the pre-recv drain/refresh steps (`loop_steps.rs`), streaming pull-source drains (`streaming.rs`), pane wake plumbing, the timer/deadline scheduler, and the off-thread fs-watch control worker (`watch.rs`, owns the `notify` watcher so its blocking recursive-watch setup never touches the loop).
  - **`key_dispatch/`** — `handle_key` top-level router + `apply_user` (`mod`), the prompt / vi-prompt editors (`prompts`), and the remove / graveyard-purge / Claude-crash-recover confirm handlers (`confirms`).
  - **`pager_handler/`** — the in-app pager overlay's vi-style key handling. `mod` holds the `handle_pager_key` router (delegates each input context to a sub-handler returning `Some`/`None`) + the pager open/close/build hub; `modes` (contextual `^C`, `/` search typing, `:N` jump buffer, `[`/`]` chords, placement/visual), `pickers` (jump-history / worktree / history-editor / session overlays), and `motion` (the scroll/vi-motion fall-through) hold the sub-handlers. A full-screen **image overlay** (mermaid diagrams) sits modally on top: `handle_image_view_key` intercepts before any pager handler and routes its own verbs (`s` save · `o` open externally · `y` copy image · `Y` copy source · `c` light/dark · `b` base64 · q/Esc/i dismiss).
  - **`mermaid_ops.rs`** — off-thread mermaid rendering for that image overlay. `Effect::RenderMermaid { mode: Open | View }` → detached worker (graveyard pattern; pure-Rust `mermaid-rs-renderer` → `resvg` raster → `ratatui-image` protocol) → `runtime.mermaid_results` → `Message::MermaidDone` → `apply_mermaid_outcomes` installs the `ImageView` (or opens the PNG externally). `Open` (`o`) writes a temp PNG + `open::that_detached`; `View` (`i`) builds a terminal-sized `Protocol` for the full-screen overlay (dark theme by default; `c` re-renders the toggle). See ARCHITECTURE.md → "Mermaid / image rendering" for the iTerm2-detected-as-Kitty + sync-update graphics gotchas.
  - **`commands.rs`** — `App::dispatch_command`, the terminal-touching half of `:` command dispatch (the pure-domain half is `AppState::dispatch_command`).
  - **`actions.rs`** — `apply` / `apply_inner`, the `Action` dispatcher, plus post-action harpoon reconcilers.
  - **`session.rs`** — session save / restore, the `-r` picker pager, the session-info overlay.
  - **`tasks.rs`** / **`capture.rs`** — `BackgroundTask`(s) and `PendingCapture`: backgrounded and foreground `!` shell-capture state.
  - **`find_picker.rs`** / **`pager_history.rs`** / **`prompt.rs`** — small data structs for the `F` finder, pager back/forward history, and the input prompt.
  - **`pager_stream.rs`** / **`grep_session.rs`** — the unified **"background worker → waking channel → streaming pager"** abstraction (off-thread read/parse is the default; see ARCHITECTURE.md). `pager_stream.rs` has the object-safe `PagerStream` trait (payload type erased per impl) + `spawn_pager_stream` (worker + empty/"computing…" pager tagged with a `stream_id`) + `drain_pager_stream` (id-gated apply via `DrainOutcome`); `grep_session.rs` is `GrepStream` + the `:grep` open. All three producers ride it through the single `stream_id` / `Message::PagerStreamOutput`: agent-transcript reads, `:grep` (`GrepStream`), and git-view diff/show/blame (`git_view_session` = `GitViewStream`, with the `|` layout toggle via `on_pager_command`).
  - **`route.rs`** / **`modal.rs`** / **`focus.rs`** — pure decisions behind a `Copy` snapshot + pure fn + tests (no TUI needed). `route.rs` does `route_input(snap, kind)` → `InputSink`: ONE routing decision shared by `handle_key` AND `handle_paste` (both dispatch on it via an exhaustive match, so keys and paste can't drift — a paste lands wherever a non-meta key would). It reads two axes: the transient `modal.rs` `Modal` (finder/capture/dismiss/quick-select/harpoon — eats all input) decided by `active_modal`, and the persistent region from the authoritative `state.focus`. `focus.rs` (`decide_focus`) picks the `Focus` for `^W j/k` and for `recompute_focus` (loop-top re-derive that keeps `state.focus` correct). The template for further pure-decision extraction (e.g. `pane_scroll::decide_scroll_source`).
  - **`pane_tabs.rs`** / **`pane_scroll.rs`** / **`codex_pin.rs`** / **`navigate.rs`** / **`quick_select.rs`** — pane tab lifecycle + focus, pane scroll mode + agent-transcript scrollback, codex session-id pinning (Option B: an off-thread `~/.codex/sessions` scan + pure spawn-ordered claim so each codex tab gets its exact rollout, kicked from the pre-recv scan), directory navigation, and the `^a u` quick-select yank. `^a v` routing (`decide_scroll_source`): an alt-screen agent with a transcript auto-engages its on-disk jsonl (read + parsed off-thread via `pager_stream`, agent prose rendered through the `ui::markdown` viewer via the shared `state::push_agent_markdown`, `r` reloads) — this is how claude's full-screen mode gets scrollback; inline it stays config-gated; a non-agent alt-screen app gets the dead-end hint; else vt100 capture. The scrollback lives in its **own** region slot (`view.scroll_pager`, separate from the top/overlay `view.pager`) so a `D` top-pane pager and a `^a v` bottom scrollback **coexist** — the pager key handlers act on the focused region's pager via the `active_pager_mut!` macro; `route_input` sends bottom-focused keys to the scrollback, top-focused keys to the top pager.
  - **`vsplit.rs`** — the vertical (left/right) file-pane split. Model state is `AppState.vsplit: Option<VSplit{width_pct, mode: TopOnly|FullHeight, focus: Side}>`; the right-region preview content is `view.right_pager` (`Mount::RightPane`). `^a |` cycles off→top-only→full-height→off (`next_vsplit` pure fn) and previews the cursor file (`pager_handler::open_right_preview`); `^a a`/`^a h` and `^a b`/`^a l` focus the a/b columns (`^a a` reclaimed from the old `PaneFocusDown` alias); `^a +`/`^a -` resize width when a column is focused. Geometry is the pure `App::carve_vsplit` post-pass over `compute_layout` (render/mod.rs); `route_input` routes right-column keys to `view.right_pager` via the `right_column_focused` snapshot bit. (Stage 1: right = a live-reloading preview, see `preview_ops.rs`; a second full file-commander on the right is the deferred Stage 2.)
  - **`preview_ops.rs`** — off-thread **live reload** of the split preview. The fs-event ingest (`sources.rs`) detects a change to the previewed file (`config::is_preview_path`, exempt from the gitignore drop) and calls `kick_preview_reload`; the worker re-runs the pure `pager_handler::build_pager_view` (markdown render + syntect) → `runtime.preview_results` → `Message::PreviewReloadDone` → `apply_preview_reloads` installs the rebuilt view preserving scroll (graveyard/mermaid worker pattern). The watched topology adds the preview's parent dir non-recursively (`watch.rs` `SyncListing.preview`, replace-on-save rationale; skipped when it's already under the recursive listing watch). A resize re-kicks to re-wrap at the new column width; an in-flight guard + `view.preview_dirty` collapse a save/resize burst to one trailing re-render.
  - **`git_state.rs`** / **`harpoon.rs`** / **`graveyard.rs`** / **`graveyard_ops.rs`** / **`clipboard.rs`** / **`mcp.rs`** / **`agent_status.rs`** / **`config.rs`** — App-layer handlers for git-worker results, the harpoon menu, the soft-delete `gy` viewer (restore/purge keys), the off-thread graveyard mutations (`graveyard_ops`: `GraveyardOp`/`GraveyardOutcome` + the `Effect::Graveyard` worker body + `apply_graveyard_outcomes` — `R` archive, `:undo` restore, `Z` purge-all run their tar/trash IO off the input thread), clipboard yank routing, MCP command application, off-thread agent-status, and live config reload.
- **`src/agent/`** — Agent profile registry. One `AgentProfile` impl per hosted AI agent (claude/codex/gemini/agy/zot); `detect(cmd)` / `profile_for(kind)` replace per-agent `match AgentKind` dispatch (detection, resume save/restore, transcript scrollback, status short-id, picker label, exit summary). Adding an agent = one impl + one `REGISTRY` entry. `AgentKind` (in `state/sessions/`) stays the persistence tag; profiles carry behavior.
- **`src/keymap/action.rs`** — `Action` enum: the full vocabulary of user-observable behaviors. Every keybinding maps to an `Action`.
- **`src/keymap/`** — Resolver, user keymap DSL parser, default bindings.
- **`src/pane/`** — Pty-hosted subprocess. `mod.rs` is the `Pane` struct (spawn, I/O, scroll mode), `input.rs` encodes crossterm keys to ANSI, `widget.rs` renders `vt100::Screen` to ratatui, `quick_select.rs` is the `^a u` picker (regex scan + label assignment over visible pane text), `pathref.rs` is `gf`/`gF`'s path extractor.
- **`src/ui/`** — Widgets: list view, status bar, pager, prompt, line editor, help, theme. Pure renderers (`model + &Theme → Vec<Line>`, no IO/gix): `syntax.rs` (syntect), `markdown/` (`mod` types + `render` entry, `renderer` event→lines state machine, `wrap` helpers, `tests`), and `diff_render/` (`mod` renderer + `tests`) / `blame_render.rs` (the in-house git diff/show/blame view — unified + side-by-side layouts over PR 7's `DiffModel`/`BlameModel`, mounted via the `git_view_session` worker; `|` toggles split⇄unified).
- **`src/fs/`** — Directory listing, entry types, file operations. `finder.rs` backs the `F` filename picker (gitignore-aware streaming walker, nucleo fuzzy match); `grep.rs` backs `:grep` (embedded ripgrep matcher streaming `path:line:col: text` matches).
- **`src/git/`** — Git integration facade: the single boundary owning every git operation, **100% in-process via `gix` (gitoxide) — no `git` subprocess in production** (migration complete). `discovery.rs` (repo-root/gitdir/branch), `status.rs` (`repo_status` index/worktree/tree walk → `StatusEntry`s + `map_to_listing`), `worktree.rs` (list/create/remove), `model.rs` (pure owned `DiffModel`/`BlameModel`/`CommitMeta`), `diff_model/` (gix→`DiffModel` for `gd`/`gD`/`show`, `gix-diff` + imara-diff hunk reassembly), `blame.rs` (gix `blame_file` → `BlameModel`). Pure infra (paths in, owned `Send` data out — no `App`, no ratatui). Diff/show/blame models are built off-thread by the `app/git_view_session` worker and rendered in-house by `ui/diff_render` + `ui/blame_render` (unified + side-by-side, word-level highlighting). App-layer git glue — the `GitViewStream` (`PagerStream` impl: off-thread `build_payload`, the in-house render, and the `|` layout toggle) + its `open_git_view` entry — stays in `src/app/{git_state,git_view_session}.rs`. **Strangler closed:** a `#[cfg(test)]` guard in `git/mod.rs` (`no_subprocess_git_in_production`) asserts zero `git`-subprocess spawns in non-test code; the only `git`-binary usages left are test fixtures that build scratch repos. **Hot-path rule:** the 1 Hz git mtime poll reads the cached `current_gitdir` (no gix open); gix opens only at chdir-into-a-new-repo + HEAD change.
- **`src/mcp/`** — MCP server (`mod` facade + `run`/socket paths, `server` socket transport, `protocol` JSON-RPC handlers, `config` `.mcp.json`/codex management + enterprise policy + instance takeover, `readers` context-file readers): PID-scoped Unix socket listener, stdio proxy for Claude Code.
- **`src/mcp_cmd.rs`** — Command channel types bridging MCP threads to the main event loop.
- **`src/context.rs`** — Context snapshot (cwd, cursor, picks, filter, git branch, project_home, search_root, session_name, pid, version) written to disk for MCP consumers. `search_root` is the focused column's worktree root, so MCP search follows the worktree (grep `F` / find use `AppState::tool_root` directly). `pid` + `version` (`crate::VERSION` = pkg version + short git SHA, also the `initialize` `serverInfo.version`) announce the running build so an MCP client can detect a stale server (expected tool missing → compare the SHA to repo HEAD → restart).
- **`src/state/`** — Cursor, marks, picks, inventory, history, ignore masks, sessions, session_names (spice-pair generator), harpoon (per-worktree pinned file list, keyed by worktree root via `AppState::harpoon_root`; lives per-column on `Commander`), graveyard (soft-delete cache as `<uuid>.json` + `<uuid>.tar.zst` pairs; FIFO cascade to system trash at 500 MB), frecency (zoxide-style directory ranking for the `J` jump prompt), pager_positions (persisted per-file pager scroll offsets, LRU-capped), health (startup validation of the persistence layer), and the agent transcript readers (`claude_transcript.rs` / `codex_transcript.rs` / `agy_transcript.rs` — on-disk jsonl → pager lines for `^a v` scrollback). The codex resolver matches a pane's *exact* rollout: it prefers the session uuid pinned to the tab at spawn (`app::codex_pin`), then a `codex resume <uuid>` command, then the rollout most-recently written during the pane's lifetime (file **mtime**, not the frozen `session_meta` start — codex appends to the original file on resume).
- **`src/config/`** — Config loading and DSL parser.
- **`src/shell/`** — Shell expansion and command execution. Cross-platform "open URL with system handler" goes through the `open` crate (`open::that_detached`), used by Quick Select's "open" intent.
- **`src/paths.rs`** — XDG-compliant path resolution for state, config, and cache directories.
- **`src/clipboard.rs`** — Cross-platform clipboard copy (`pbcopy` / `wl-copy` / `xclip` / `xsel` fan-out).
- **`src/envset.rs`** — Runtime env overrides from `:s` (setenv): a thread-safe map layered over the real environment (no `unsafe` `set_var`), merged into every spawned child.
- **`src/key_trace.rs`** — Opt-in per-key dispatch trace (`--key-trace` / `SPYC_KEY_TRACE=1`) to `/tmp/spyc-key-trace-<TIMESTAMP>.log`.
- **`src/sysinfo.rs`** — System info (RSS, PID) for the `I` info overlay.
- **`src/proc_cwd.rs`** — Cross-platform "cwd of pid N" lookup (Linux `/proc/<pid>/cwd`, macOS `lsof -Fn`). Used to surface the live pane subprocess cwd in the divider.
- **`src/term_title.rs`** — Host-terminal window title (push/pop/set). Wraps OSC 2 in tmux's DCS passthrough when `$TMUX` is set so iTerm2 etc. receive the title.
- **`src/debug_log.rs`** — `spyc_debug!` macro; writes to `/tmp/spyc-debug-<TIMESTAMP>.log` when `--debug` / `SPYC_DEBUG` is set.
- **`src/main.rs`** — Terminal setup/teardown, `suspend_tui`/`resume_tui` for child processes.

## Conventions

- **Action enum dispatch**: New features get an `Action` variant, a keymap binding, and a handler arm in `src/app/actions.rs` (`apply_inner`) — or the pure-domain half in `AppState::apply`. Not in `mod.rs`.
- **Keep `src/app/` modularized (don't regrow the monolith)**: `app/mod.rs` was a ~12k-line monolith; the `docs/archive/REFACTOR_PLAN.md` decomposition + the MVU migration + the 800-LoC campaign carved it down to ~1k (the `App`/`Runtime`/`ViewState` defs, the `Message` enum, and a little glue — the constructor, event loop, process I/O, and leaf helpers are sibling modules). New render/key/command/action/session logic belongs in the matching child module (or a new `src/app/<feature>.rs`), **not** appended to `mod.rs`. The pattern is a child module with `impl App { … }`: child modules can read `App`'s private fields via the descendant-module rule, so you almost never need to make a field `pub` — only the handful of methods called from `app` or sibling modules. A test (`app::guard_tests::mod_rs_stays_decomposed`) fails if `mod.rs` grows past its ceiling; if you hit it, extract a module rather than bumping the number.
- **`:command` registration goes through `COMMAND_TABLE`** (`src/app/command_table.rs`): every `:`-command is one `CommandSpec { name, handler, completion }` entry, where `handler` is `CmdHandler::Pure` (resolved in `AppState::dispatch_command`) or `CmdHandler::App(fn)` (terminal-touching, in `src/app/commands.rs`). State runs first; the table drives tab-completion and the Pure→App routing, so you add a table entry plus its handler together — no hand-synced punt list. A missing **Pure** arm is caught by the `command_table_*` tests; a missing **App** handler is now a **compile error** (the handler fn-pointer is named in the entry). Symbol commands (`!`, `;`, `!!`) are dispatched directly and stay out of the table. Bitten historically on `:undo` (v1.41.1) and the `:limit`/`:`-history split — both now structurally prevented.
- **No OS in the pure layers (enforced, not just documented)**: the Model (`AppState::apply`) and the draw pass (`&self` render) must not do blocking IO, spawn threads, read env, or fork subprocesses. Side effects are `Effect` data run *only* by `run_effects`; any pre-frame settling happens in the `&mut` `prepare_*` steps (`render/mod.rs`), never the draw methods (`render/inner.rs`/`chrome`/`overlays`). The June-2026 deep review (`docs/archive/CODE_REVIEW_2026-06.md`, shipped) found this contract had silently eroded — OS calls smuggled into `&self` render via interior mutability (`agent_status`/`live_cwd`/HUD), and tar/trash IO run inline in key handlers (multi-second freezes on a big `R` delete). The render half is now a **source-scan test** (`app::render::purity_guard`): a draw module containing `thread::spawn` / `std::fs::` / `read_to_string` / env reads is a test failure. To move a blocking op off-thread, copy the `graveyard_ops` template — handler emits an `Effect`, `run_effects` spawns a detached worker, the worker pushes its result onto a `Runtime` slot + wakes the loop with a *payloadless* `Message` (wired through both `sources.rs` coalesce arms + the `run.rs` dispatch `unreachable!` arm), and the pre-recv scan drains+applies — then add the now-clean module to the guard's `PURE_DRAW` list to lock the fix in. **Lesson: a documented invariant drifts unless it's a build/test failure** — reach for the guard-test / compile-error pattern (this, the `mod.rs` ceiling, `COMMAND_TABLE`) over a prose rule.
- **Test the requirement, not the implementation** (charter: `docs/TEST_IMPROVEMENT_PLAN.md`): the value of a test is in catching a *wrong* behavior, not confirming the code does what it already does. So — never "write tests for this function" (that re-asserts current behavior into a tautology); start from the requirement or invariant (`no Up/Down sequence may leave the cursor index ≥ inventory.len()`), and add **negative** tests for what the system must *not* do. Reach for `proptest` over a single hand-picked example when the input space is wide (the pure MVU state is the ideal target). Use AI to generate edge-case *data* (weird-unicode names, circular symlinks) — not the assertions. **Decouple assertions from struct layout**: don't destructure a whole `Effect`/struct field-by-field in a test (a new field then breaks every such test — refactoring paralysis); assert *intent* through the test-only matchers in `src/app/effect.rs` (`fx.change_dir()`, `fx.read_pane_text()`, …) — the single place that destructures, with `..` so new fields are transparent. Add the matcher your test needs there; don't shelve unused ones.
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

- **`CHANGELOG.md` is git-cliff-generated from v1.57.0 onward.** Entries
  are produced from the conventional-commit history by
  [git-cliff](https://git-cliff.org) (config in `cliff.toml`): the section
  comes from the commit *type* (`feat:` → Features, `fix:` → Bug Fixes,
  `refactor:`/`perf:`/`docs:`/`build:` → their sections) and the line is the
  commit's `scope: subject`. So **the commit message _is_ the changelog
  entry** — which is exactly why the first bullet (subject = actual scope)
  matters, and why a category-spanning PR wants multiple well-typed commits
  rather than one. Bitbucket "Merged in …" merge commits are filtered out.
  Entries at **v1.56.0 and earlier are frozen hand-written history** (Keep a
  Changelog `Added`/`Changed`/`Fixed`) — left verbatim, never reformatted.
  Preview the pending section with `make changelog`; cut a release with
  `make release-tag VERSION=x.y.z` (bumps `Cargo.toml`, *prepends* the new
  version's section, commits, tags `vX.Y.Z`). Both are local/release-time —
  not in CI.

## Building

```sh
cargo build            # dev build
cargo build --release  # release build
make release           # release build via Makefile
make install           # build release + copy to ~/.local/bin
make check             # fmt + clippy + test + deny (CI gate)
make fuzz              # coverage-guided fuzz (nightly + cargo-fuzz; on-demand, NOT in check)
make changelog         # preview the pending (unreleased) CHANGELOG section
make release-tag VERSION=x.y.z   # bump + prepend changelog + commit + tag
make                   # see Makefile for all targets
```

**Crate shape: lib + bin.** `src/lib.rs` is the library root — it owns every
module and the `run()` entry point; `src/main.rs` is a thin shim
(`fn main() { spyc::run() }`). The split exists so the crate also builds as a
library, which the `cargo-fuzz` targets under `fuzz/` link against (libFuzzer
targets are separate binaries). `fuzz/` is a **standalone workspace** (its own
`[workspace]`), so `cargo build` / `make check` / cargo-deny never touch it —
fuzzing needs nightly and runs on demand (`make fuzz`, or `cargo +nightly fuzz
run dsl_parse`). New fuzz entry points go through the `pub mod fuzz` facade in
`lib.rs` (raw-input wrappers that leak no internal types), not by widening
module visibility.

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
- **To spin up a git worktree:** `create_worktree(branch)` makes one
  off the focused commander's repo (sibling `<repo>.worktrees/<branch>/`)
  and returns its path — point a second column / `navigate_to` there.
- **To read a file the user is viewing:** use `get_file_content`
  with relative paths (resolved against spyc's cwd).
- **For project-wide search:** prefer `search_paths` (fuzzy
  filenames) and `search_content` (gitignore-aware regex over file
  contents) over `Bash rg/grep`. Both scope to the focused
  commander's worktree root (its repo root, else PROJECT_HOME, else
  cwd) and return structured JSON. Two more are uniquely spyc-shaped:
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

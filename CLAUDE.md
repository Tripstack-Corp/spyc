# spyc

A vi-keyboard-driven terminal file manager written in Rust, built on ratatui/crossterm. Inspired by SideFX's `spy`. Single-developer project.

## What it does

- Vi-style navigation, marks, cursor motion, and numeric prefix (`3j`, `5G`)
- Embedded pty pane (horizontal split) with tabs for running subprocesses — primarily used to host `claude` CLI for dog-fooding
- MCP server on a PID-scoped Unix socket — Claude Code discovers spyc via `.mcp.json`, queries context (cwd, cursor, picks, filter, git branch), and can mutate the TUI (navigate, filter, pick). Multiple instances coexist; enterprise policies respected.
- `gf`/`gF` — jump from Claude's output to the referenced file (or file:line)
- In-app pager with search, ANSI rendering, hex-dump, line numbers, `:N` jump, save
- Vi-editable shell prompt with persistent history (`!` captured, `;` foreground, `$` interactive shell)
- `!?` history editor — popup with vi-editable lines, `/search`, `G`/`gg`, `:N` jump, `Ctrl+D` delete
- `:` command line — vim-style command entry (`:limit`, `:!cmd`, `:!!`, `:;cmd`, `:q`)
- `=` limit filter — temporary glob filtering (`=*.rs`, `=!` for picks, `=` clears)
- Picks (per-directory multi-select) and inventory (file cache with graveyard)
- Session save/restore — auto-saved on quit, `spyc -r` resumes tabs and Claude conversations
- `.spycrc.toml` config with keymap DSL, themes, ignore masks, live reload

## Architecture

- **`src/app/mod.rs`** — Top-level `App` struct, event loop, layout, all key dispatch. This is the big file.
- **`src/app/state.rs`** — `AppState`: domain state (cursor, picks, listing, mode) separated from terminal state.
- **`src/keymap/action.rs`** — `Action` enum: the full vocabulary of user-observable behaviors. Every keybinding maps to an `Action`.
- **`src/keymap/`** — Resolver, user keymap DSL parser, default bindings.
- **`src/pane/`** — Pty-hosted subprocess. `mod.rs` is the `Pane` struct (spawn, I/O, scroll mode), `input.rs` encodes crossterm keys to ANSI, `widget.rs` renders `vt100::Screen` to ratatui.
- **`src/ui/`** — Widgets: list view, status bar, pager, prompt, line editor, help, theme.
- **`src/fs/`** — Directory listing, entry types, file operations.
- **`src/mcp.rs`** — MCP server: PID-scoped Unix socket listener, stdio proxy for Claude Code, `.mcp.json` management, enterprise policy checking, instance takeover.
- **`src/mcp_cmd.rs`** — Command channel types bridging MCP threads to the main event loop.
- **`src/context.rs`** — Context snapshot (cwd, cursor, picks, filter, git branch) written to disk for MCP consumers.
- **`src/state/`** — Cursor, marks, picks, inventory, history, ignore masks, sessions.
- **`src/config/`** — Config loading and DSL parser.
- **`src/shell/`** — Shell expansion and command execution.
- **`src/paths.rs`** — XDG-compliant path resolution for state, config, and cache directories.
- **`src/sysinfo.rs`** — System info (RSS, PID) for the `I` info overlay.
- **`src/debug_log.rs`** — `spyc_debug!` macro; writes to `$XDG_STATE_HOME/spyc/debug.log`.
- **`src/main.rs`** — Terminal setup/teardown, `suspend_tui`/`resume_tui` for child processes.

## Conventions

- **Action enum dispatch**: New features get an `Action` variant, a keymap binding, and a handler in `app.rs`.
- **Milestone spikes**: Development proceeds in numbered milestones (M4, M6, M8, M9, M10...).
- **Repaint strategy**: Event-driven dirty-frame rendering. `needs_draw` flag with reason codes (pane=1, event=2, other=3). `needs_full_repaint` for teardown transitions (pager close, overlay close). DEC 2026 synchronized output wraps every frame. `build_rows()` and grid stabilization are cached via `list_generation` counter. Target: 0 dps at idle.
- **Pane I/O**: Keys go through `input::encode_key()`. Raw bytes use `pane.send_bytes()`. Bracketed paste wraps text in `\x1b[200~`...`\x1b[201~` before forwarding. Pane prefix is `^a` (screen-style), `^w` works as alias.
- **Keep docs in sync**: When committing changes that affect user-visible behavior, keybindings, or project status, update **all** of the following that are affected:
  - `README.md` — positioning, install instructions, keybinding tables
  - `FEATURES.md` — complete feature reference
  - `CLAUDE.md` — architecture, conventions, "what it does" summary
  - `ROADMAP.md` — move shipped items to Done, update track status
  - `BUGS.md` — move fixed bugs to FIXED section
  - `CHANGELOG.md` — add entry under Unreleased
  - `INSTALL.md` — if build/install steps change
  - `src/ui/help.rs` — if keybindings or user-facing commands change
  Do not batch doc updates as a follow-up — include them in the same commit as the code change.
- **Bump version**: Always bump the version in `Cargo.toml` when shipping user-visible changes. Patch for fixes, minor for features. See `CONTRIBUTING.md` for SemVer policy.

## Building

```sh
cargo build            # dev build
cargo build --release  # release build
make release           # release build via Makefile
sudo make install      # copy to /usr/local/bin (run `make release` first)
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

If the spyc MCP tools are NOT available, remind the user:
"I don't see the spyc MCP tools — are we running inside spyc?
This project is built to be dog-fooded through the spyc pane."

## Dog-fooding context

The developer uses spyc with Claude Code CLI running in the lower
pane. Bugs and features are often discovered through this dog-fooding
workflow — if something affects the Claude Code pane experience, it's
high priority. Always develop and test from inside spyc.

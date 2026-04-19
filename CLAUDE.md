# spyc

A vi-keyboard-driven terminal file manager written in Rust, built on ratatui/crossterm. Inspired by SideFX's `spy`. Single-developer project.

## What it does

- Vi-style navigation, marks, cursor motion, and numeric prefix (`3j`, `5G`)
- Embedded pty pane (horizontal split) for running subprocesses — primarily used to host `claude` CLI for dog-fooding
- In-app pager with search, ANSI rendering, hex-dump, line numbers, `:N` jump, save
- Vi-editable shell prompt with persistent history (`!` captured, `;` foreground, `$` interactive shell)
- `!?` history editor — popup with vi-editable lines, `/search`, `G`/`gg`, `:N` jump, `Ctrl+D` delete
- `:` command line — vim-style command entry (`:limit`, `:!cmd`, `:!!`, `:;cmd`, `:q`)
- `=` limit filter — temporary glob filtering (`=*.rs`, `=!` for picks, `=` clears)
- Picks (per-directory multi-select) and inventory (file cache with graveyard)
- `.spycrc.toml` config with keymap DSL, themes, ignore masks, live reload

## Architecture

- **`src/app.rs`** — Top-level `App` struct, event loop, layout, all key dispatch. This is the big file.
- **`src/keymap/action.rs`** — `Action` enum: the full vocabulary of user-observable behaviors. Every keybinding maps to an `Action`.
- **`src/keymap/`** — Resolver, user keymap DSL parser, default bindings.
- **`src/pane/`** — Pty-hosted subprocess. `mod.rs` is the `Pane` struct (spawn, I/O, scroll mode), `input.rs` encodes crossterm keys to ANSI, `widget.rs` renders `vt100::Screen` to ratatui.
- **`src/ui/`** — Widgets: list view, status bar, pager, prompt, line editor, help, theme.
- **`src/fs/`** — Directory listing, entry types, file operations.
- **`src/state/`** — Cursor, marks, picks, inventory, history, ignore masks.
- **`src/config/`** — Config loading and DSL parser.
- **`src/shell/`** — Shell expansion and command execution.
- **`src/main.rs`** — Terminal setup/teardown, `suspend_tui`/`resume_tui` for child processes.

## Conventions

- **Action enum dispatch**: New features get an `Action` variant, a keymap binding, and a handler in `app.rs`.
- **Milestone spikes**: Development proceeds in numbered milestones (M4, M6, M8, M9, M10...).
- **Repaint strategy**: One-shot `needs_full_repaint` flag instead of per-frame `terminal.clear()`. Set at teardown transitions (pager close, overlay close) and via `^L` / `Action::Redraw`.
- **Pane I/O**: Keys go through `input::encode_key()`. Raw bytes use `pane.send_bytes()`. Bracketed paste wraps text in `\x1b[200~`...`\x1b[201~` before forwarding.
- **Keep docs in sync**: When committing changes, update `ROADMAP.md`, `FEATURES.md`, `CLAUDE.md`, and help text (`src/ui/help.rs`) if the change affects user-visible behavior, keybindings, or project status.
- **Bump version**: Always bump the version in `Cargo.toml` when shipping user-visible changes. Patch for fixes, minor for features. See `CONTRIBUTING.md` for SemVer policy.

## Building

```sh
cargo build            # dev build
cargo build --release  # release build
make                   # see Makefile for build, release, cross-compile, install, deploy targets
```

## Roadmap

See `ROADMAP.md` for current plans. Key upcoming: session forking, demo mode.

## Dog-fooding context

The developer uses spyc with Claude Code CLI running in the lower pane. Bugs and features are often discovered through this dog-fooding workflow — if something affects the Claude Code pane experience, it's high priority.

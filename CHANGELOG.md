# Changelog

All notable changes to spyc are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- Unicode-width support: CJK filenames, flags, and emoji now render
  with correct column alignment in the file list, status bar, help
  screen, and pager. Uses `unicode-width` crate.
- `CHANGELOG.md` seeded in Keep-a-Changelog format.
- `--version --verbose` dumps git SHA, build timestamp, rustc version,
  TERM, COLORTERM, and os/arch. `build.rs` embeds build info.
- **Inventory rewritten as file cache.** `y` (yank) copies file
  content into `~/.local/state/spyc/inventory/`. `p` (put) copies
  cached files to the current directory and removes from inventory.
  Regular files only — directories and special files are rejected.
- Inventory view (`i`): `t`/`Space` to tag items for partial put,
  `p` to put tagged (or all) to cwd, `x`/`d` to remove to graveyard.
- `Y` (shift-y) removes cursor file from inventory in dir view.
- Inventory persists across sessions (file-backed cache with metadata).
- Graveyard: removed inventory items are preserved in
  `~/.local/state/spyc/graveyard/` for undo safety.
- ESC exits inventory view (returns to directory view).
- Status bar always shows hidden file count (even when 0).

### Changed
- `p` in dir view now means "put inventory to cwd" (was "drop from
  inventory").

### Fixed
- Backtick (`` ` ``) now returns to the session's home directory, not
  where spyc was launched from.
- `gf`/`gF` scans the last 200 lines of scrollback (not just the
  visible viewport), so paths in large diffs are still found.

## [1.5.0] - 2026-04-18

### Added
- **MCP context handoff (M14):** spyc runs an HTTP MCP server on a
  background thread (OS-assigned port). Claude CLI connects via
  `--mcp-config` injected at pane spawn. `get_spyc_context` tool
  returns cwd, cursor, picks, inventory, filter, and git branch.
- **Conversation-aware session restore:** session save captures
  Claude Code session ID and display name. Restore spawns
  `claude --resume <sessionId>` to resume the conversation.
  Session picker shows name + short ID.
- `SPYC_CONTEXT` environment variable set in pane environment,
  pointing to `.spyc-context-<PID>.json`.
- `--mcp` CLI flag for stdio MCP server (testing/future use).
- macOS `codesign -s -` in Makefile install target.

### Fixed
- Pane tabs now stay open with `[exited]` label when the child
  process exits, so error output is readable. Any keypress dismisses.
- Session dedup no longer broken by ephemeral `--mcp-config` port
  numbers in saved commands.

## [1.4.0] - 2026-04-18

### Added
- **Bidirectional path references (M13):** `gf` jumps the file list
  to a path reference in pane output; `gF` also opens the pager at
  the referenced line.
- Path extraction handles: bare paths, `path:line:col`, backticks,
  quotes, Claude CLI patterns (`Update(path)`, `Read path`, `⎿`,
  `→`), diff headers, ANSI stripping.
- Bottom-up scan (most recent output wins), dual cwd resolution
  (pane cwd + project root).
- Works in both live and scroll mode (`g` prefix: `gg`/`gf`/`gF`).
- 35 path extraction tests.

### Fixed
- `gf`/`gF` no longer matches bare slashes as paths.
- `gf`/`gF` exits scroll mode and unfocuses pane on successful jump.

## [1.3.1] - 2026-04-17

### Fixed
- Watch `.git/index` for live git status marker updates after
  `git add`, `commit`, `checkout`, etc.

## [1.3.0] - 2026-04-17

### Added
- `:cd` command to change directory from the command line.
- `:sort` with `name`, `size`, `mtime`, `ext` modes.
- `:marks` displays current marks in a pager.
- `:set key=value` for runtime settings.
- Pager buffer history: `:bprev`/`:bnext` or `[b`/`]b` navigate
  closed pagers (max 10 in back/forward stack).

## [1.2.0] - 2026-04-16

### Changed
- Git status markers moved to left gutter (was overriding file
  colors).

## [1.1.0] - 2026-04-16

### Fixed
- File type colors no longer overridden by git status colors.

## [1.0.0] - 2026-04-15

### Changed
- Renamed from `cspy` to `spyc`.

## [0.13.0] - 2026-04-14

### Added
- `:` command line: `:limit`, `:!cmd`, `:!!`, `:;cmd`, `:q`.
- `=` limit filter: `=*.rs`, `=!` for picks only, `=` clears.
- Numeric prefix display (typing `3j` shows "3" in prompt area).
- `:N` jump-to-line in pager and history editor.

## [0.11.0] - 2026-04-13

### Added
- `!?` history picker popup with vi-editable lines, `/search`,
  `n`/`N` navigation, `G`/`gg`, `Ctrl+D` delete, deduped history.

### Fixed
- Pager/pane repaint artifact on close.

# Changelog

All notable changes to spyc are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [1.8.0] - 2026-04-19

### Added
- **Writable MCP actions.** Claude can now mutate the TUI workspace
  via five new MCP tools: `navigate_to` (chdir or focus file),
  `set_filter` (set/clear glob filter), `pick_files` (pick by glob
  patterns), `clear_picks`, and `get_file_content` (read up to 100KB
  text). The MCP server uses a command channel to the main event loop
  with one-shot reply channels and 5-second timeout. Flash messages
  (`[mcp] navigated to src/`) keep the user informed when Claude
  changes the workspace.

## [1.7.0] - 2026-04-19

### Added
- **Performance refactor.** Idle CPU dropped from ~12.5% to near-vim
  levels (~2.5%). Root cause: context file writes were triggering
  file-watcher → refresh_listing → git subprocess → redraw cycles.
  Fixes: context file excluded from watcher, context writes skipped
  when unchanged, DEC 2026 synchronized output, build_rows/grid
  computation caching, active-tab-only draw triggering, has_pending
  atomic guard on drain, increased idle poll interval.
- **Activity monitor** (`A` toggle): live overlay showing draws/sec,
  cells/sec, draw reason breakdown (pane/event/other), and poll rate.
  Piggybacks on real draws — does not force its own redraws.
- **`y` prefix for yank commands.** `yy` yanks files into inventory
  (was bare `y`), `yp` yanks visible pane output to clipboard, `yP`
  yanks the last prompt you typed into the pane to clipboard.
- **Pager `?` help:** dedicated help overlay showing all pager keybindings.
- **Exit summary:** on quit, spyc prints a one-line session summary to
  stdout (cwd, tab count, Claude session name, restore hint).
- **Pager line numbers default to on.** `l` toggles line numbers, `w`
  toggles whitespace markers (previously `l` controlled both).
- `make install` now shows verbose progress (linking stage note,
  codesign step, version in final message).

### Changed
- **Pane prefix switched to `^a` (screen-style).** `^w` still works
  as an alias. Bindings: `^a n`/`]` next tab, `^a p`/`[` prev tab,
  `^a c` new tab, `^a K`/`x` close tab, `^a P` pipe content,
  `^a r` rename, `^a s` send selection, `^a v` scroll mode.
- Focus notice uses product naming: "focus: spyc" / "focus: claude"
  (active tab label) instead of generic "focus: list" / "focus: pane".
- `git status` uses `-unormal` instead of `-uall` to avoid expensive
  recursive enumeration of untracked directories.

### Removed
- Cursor blink in the pane — was causing phantom redraws and added no
  value. Unfocused cursor now shows as a static dim block.
- Periodic `^L` refresh to Claude pane tabs — cleared draft prompts
  when focus was elsewhere.

### Fixed
- Backtick (`` ` ``) now returns to the session's home directory, not
  where spyc was launched from.
- `gf`/`gF` scans the last 200 lines of scrollback (not just the
  visible viewport), so paths in large diffs are still found.

## [1.6.0] - 2026-04-18

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
- `V` opens `$EDITOR` in the top pane (overlay) — the Claude pane below
  stays visible so you can edit while watching Claude work. `e`/`v` still
  opens the editor full-screen (suspends TUI).
- `:version` command and `gV` keybinding show the spyc version
  (previously `V`, now reassigned to edit-in-pane).

### Changed
- `p` in dir view now means "put inventory to cwd" (was "drop from
  inventory").

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

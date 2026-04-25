# Changelog

All notable changes to spyc are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [1.13.0] - 2026-04-24

### Added
- **`spyc --print-config`** — emits a fully-commented default
  `.spycrc.toml` to stdout. Every option is shown commented out at
  its default value with a one-liner explaining what it does, grouped
  by section. Bootstrap a config with
  `spyc --print-config > ~/.spycrc.toml`. Round-trip parsed in tests
  so the dump always loads cleanly with the current schema.
- **Configurable status bar position.** New `[layout]
  status_position = "top" | "bottom"` option in `.spycrc.toml`.
  Default `"top"` (stock spyc). `"bottom"` matches the vim/tmux
  convention and is the right choice when running spyc inside tmux —
  the host status line typically owns the top row, so keeping spyc's
  bar there causes a double-bar. With `"bottom"` the prompt sits one
  row above the status bar (vim cmdline-above-statusline ordering),
  consistent with both pane-open and pane-closed layouts.

## [1.12.1] - 2026-04-24

### Fixed
- **Claude session resume — verify the banner token actually exists.**
  v1.11.2 trusted the `claude --resume <id>` banner unconditionally,
  but Claude sometimes prints the banner with a session ID it never
  persisted (the user `/clear`'d or `/resume`'d to a different
  session before exit). Restore would then fail with "No
  conversation found with session ID …". Now we verify the JSONL
  exists at `~/.claude/projects/<slug>/<id>.jsonl` before saving;
  if it doesn't, we fall back to the most-recently-modified JSONL
  in the project slug — the same file `claude --resume`'s no-arg
  picker would surface first. The PID-scoped scan is now only the
  last-ditch fallback.

## [1.12.0] - 2026-04-24

### Changed
- **`!` captured commands now run under a PTY and accept input.**
  Previously `!sudo …` (or anything else that opens `/dev/tty` for
  prompts: ssh, scp, gpg, passwd) wrote its password prompt straight
  to the real terminal — bleeding "Password:" / "Sorry, try again."
  text on top of the file list and into the pager body, with no way
  to actually answer because keystrokes went to spyc's normal key
  handling. Now `!` allocates a slave PTY for the child, so
  `/dev/tty` resolves to that slave and prompt bytes flow into the
  pager via the master like any other output. While the capture is
  live, every keystroke is encoded and written to the master, so
  the user can type a password (no echo — `sudo` controls termios
  on the slave) and press Enter. New control bindings inside a
  running `!`: **^C** sends SIGINT through the tty (cancels sudo's
  prompt, etc.); **^\\** hard-kills the child if it has detached
  from the tty. Status line updated accordingly.

## [1.11.3] - 2026-04-24

### Changed
- **Home directory shortens to `~` in displayed paths.** The status
  bar path, `I` info overlay (`start dir`, `cwd`, config sources),
  `:project` display, and the on-quit exit summary now collapse a
  leading `$HOME` to `~` (e.g. `~/src/spyc` instead of
  `/Users/derek/src/spyc`). Match is anchored at directory
  boundaries so unrelated paths sharing the home prefix as a
  substring are unaffected. MCP context output is unchanged —
  consumers continue to receive absolute paths.

## [1.11.2] - 2026-04-24

### Fixed
- **Claude session resume reliability.** `spyc -r` no longer fails
  with "No conversation found with session ID …" for sessions that
  resume cleanly via `claude --resume` by hand. Root cause: the old
  resolver scanned `~/.claude/sessions/*.json`, which is a PID-scoped
  index of *running* claude processes, not resumable conversations.
  After `/compact` or `/clear` rotates the session ID, that file
  still pointed at the original (now-orphan) ID. Fix: on session
  save we now read the `Resume this session with: claude --resume
  <token>` banner Claude prints on exit straight from the pane
  scrollback. The token is the authoritative resume target and works
  for both UUID and named sessions. Falls back to the old scan only
  when no banner is captured.

## [1.11.1] - 2026-04-23

### Fixed
- **Help pager multi-column layout.** Descriptions wider than a column
  now wrap onto continuation lines that align under the description
  column (no more silent truncation at the column edge). Section
  headers stay with their bodies — a section that wouldn't fit in the
  remaining column space moves as a unit to the next column.
- **Content-to-column mapping is now static.** `j`/`k` scrolls both
  columns in lockstep against a fixed partition instead of reshuffling
  lines between columns on every scroll. `G` and the `Top`/`Bot`/`NN%`
  position indicator all share the same "longest chunk" math, so
  pressing `k` from `Bot` no longer jumps back to 91%.
- **Responsive column count.** The 2-col / 1-col choice is made from
  the actual body width (90% of terminal × borders), not the raw
  terminal width, and is re-decided whenever the window is resized
  while help is open. Help rebuilds in place with the new wrap points.

## [1.11.0] - 2026-04-23

### Added
- **`PROJECT_HOME` concept.** A sticky per-session project root,
  distinct from `start_dir` (the backtick target). Auto-set on
  startup when the launch dir contains `.git`; otherwise unset.
  New bindings: `gh` (jump), `gP` (set to current dir). Command
  line: `:project`, `:project .`, `:project <path>`, `:project clear`.
  New pane tabs default their cwd to `PROJECT_HOME` when set.
  Persisted with the session (round-trips through `spyc -r`).
- **Named sessions (spice-themed).** Every session now has a
  display name like `SAFFRON_CUMIN` or `HARISSA_SUMAC`, generated
  on creation from ~30 spice words. Shown on the top bar in
  all-caps and as the primary column in the session picker.
  Rename with `:name <NEW>`.
- **Start dir is now editable at runtime.** `gS` sets it to
  current dir; `:startdir` prints; `:startdir .` / `:startdir <path>`
  sets it. Previously only settable at spyc launch or on session
  restore.
- **`gU` / `:whoami` to flash user@host** in the status line.
  User@host also appears in the `I` info overlay.
- **MCP context exposes `project_home` and `session_name`** so
  Claude can see the sticky project root and the session label.

### Changed
- **Top bar redesign.** Drops the user@host segment (rarely useful
  once you're inside spyc). New order:
  `🌶️ | PROJECT_HOME | SESSION_NAME | path | git | suffix`.
  Truncation priority under width pressure: suffix → path-basename
  → git branch. PROJECT_HOME and SESSION_NAME are retained as the
  primary identifiers for the workspace.

## [1.9.0] - 2026-04-21

### Added
- **Frecency-based path ranking for J prompt.** The J (jump) prompt
  now learns from your navigation history. Directories are scored by
  frequency x recency (zoxide-style tiered decay). When filesystem
  completion finds no matches, frecency suggests directories you've
  visited before — type a fragment, Tab completes the best match.
  Persisted to `~/.local/state/spyc/frecency.json`, capped at 500
  entries with LRU pruning. Health check validates on startup.
- **DEC 1007 alternate scroll mode** replaces `EnableMouseCapture`.
  Scroll wheel becomes arrow keys in the alternate screen — prevents
  scrollback interference while keeping text selection working.
- **Trackpad scroll throttle.** Rate-limits rapid-fire arrow keys
  from trackpad inertia to ~25/sec (40ms gap) for smooth two-finger
  scrolling.

### Fixed
- **Tab completion for remote directories.** `~/D<tab>` no longer
  filters the current listing when completing paths in a different
  directory. Now flashes match names directly (Desktop, Documents,
  Downloads).

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

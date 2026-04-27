# Changelog

All notable changes to spyc are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Changed
- **`make install` now defaults to `~/.local/bin` (no sudo).** The
  Makefile's `PREFIX` defaults to `$HOME/.local`. To install
  system-wide, override: `sudo make install PREFIX=/usr/local`. The
  install target prints a hint if `~/.local/bin` is not on `$PATH`.
  README, INSTALL.md, and CLAUDE.md updated to reflect the new
  recommended flow.

## [1.21.0] - 2026-04-26

### Added
- **Task viewer (`gB`, `[t`/`]t`, `:task N`).** A peek view into a
  backgrounded shell task's buffer that doesn't take ownership the way
  `:fg` does. From the file list, `gB` opens the most-recent task in
  the viewer; from inside any pager `[t`/`]t` cycles through bg tasks
  by id (wraps around). `:task N` jumps to a specific task. While the
  task is running, the viewer's content auto-refreshes from the live
  buffer; the title shows `running ({Xs})` / `exit 0 ({Xs})` etc.
- **Task viewer → buffer history promotion.** When you close
  (Esc / `q`) a task viewer for a task that has *exited* and that
  you've actually viewed, spyc snapshots the current rendered view
  into the buffer-history stack and removes the task from the bg
  list. `[b` from any subsequent pager walks back to the snapshot.
  Running tasks never auto-promote -- they stay in the bg list until
  exit + view.

### Changed
- **Help overlay no longer pollutes buffer history.** Hitting `[b`
  after closing the help could surface stale help content; help is
  now flagged `no_history` so it's skipped on close.
- **`[b`/`]b` at the edge of history keeps the current pager open.**
  Previously, hitting `[b` at the start (or `]b` at the end) silently
  closed the pager because the current view was consumed before the
  empty-stack case was checked. Now the pager stays put with a flash
  ("no older buffers" / "no newer buffers"); same fix for
  `:bprev` / `:bnext`.

## [1.20.2] - 2026-04-26

### Changed
- **Background tasks render in the pane divider, not the status-bar
  suffix.** Right-aligned, growing leftward, with a distinct color
  family (blue/teal/green/red) so the numbering doesn't visually
  collide with pane tabs (yellow/amber, left-aligned). Glyphs:
  - `[N+]` running, output arrived since you last `:fg`'d (teal)
  - `[N\u{25cf}]` running, quiescent (blue)
  - `[N\u{2713}]` exited cleanly (green)
  - `[N\u{2717}]` non-zero exit / killed / crashed (red)
  Per-task `has_unread_output` flag flips true when bytes arrive
  during the bg drain, false on `:fg` -- so `+` is a real "go look
  at this" cue, not just "still alive". When the pane is hidden
  (no divider rendered), the old `bg:N\u{25cf}` status-bar suffix
  is the fallback. If too many tasks to fit on the divider,
  oldest are dropped first; newest stay visible at the right.

## [1.20.1] - 2026-04-26

### Fixed
- **`:fg` no longer flashes "unknown command: fg".**
  `AppState::dispatch_command` whitelists which colon-commands fall
  through to App's terminal-touching arms (where the v1.20.0 `:fg`
  implementation lives); `fg` wasn't on the list, so the command was
  rejected inside AppState before App ever saw it. Added `fg` and
  `fg <N>` to the passthrough list. `^Z` to background was unaffected.

## [1.20.0] - 2026-04-26

### Added
- **Background tasks (M1) -- `^Z` to background, `:fg` to resume.**
  Long captured commands (`!cargo test`, `!find ...`) no longer lock
  you out of spyc. Press `^Z` while a streaming `!` capture pager is
  open to send the task to the background; reader thread keeps
  draining output into a per-task buffer (head-truncated at 1 MB).
  `:fg` (no arg) resumes the most-recently-backgrounded task; `:fg N`
  targets a specific id. Round-trip semantics:
  - Still-running tasks resume as a streaming pager seeded with
    everything captured so far; the original task id is preserved
    across `^Z` -> `:fg` -> `^Z` cycles.
  - Already-exited tasks resume as a static pager titled
    `! cmd — exit 0 (43s)` and are removed from the background list
    on view (one-shot).
  - A task that completes while in the background fires a flash
    `task #N: cmd — exit 0 (43s)`.
  - Status-bar suffix shows `bg:N●M✓` (N running, M completed).
  - Quit confirmation counts backgrounded running tasks alongside
    pane-tab processes.
  - Already in a foreground task and `:fg` is hit? Error-flash
    `already in a foreground task — ^Z to send to background first`
    (no silent swap).

  M2-M4 (`:bg` overlay, `!&cmd` direct-launch, polish) remain on the
  ROADMAP.

## [1.19.1] - 2026-04-26

### Changed
- **`q` no longer quits** -- it's now reserved for a future vim-style
  macro recording feature (already on the roadmap as `qa ... q ... @a`).
  Pressing `q` flashes a hint ("q reserved for future macro recording
  -- Q or :q to quit") instead. Quit is still bound to `Q`, `^D`, and
  `:q`. Motivation: an accidental `q` was easy to fat-finger when
  switching from vim contexts and produced the most destructive
  possible outcome (silent quit). Help overlay updated accordingly.

## [1.19.0] - 2026-04-26

### Changed
- **`L` long listing rewritten as an aligned table.** One header row
  + one data row per file with columns: inode, mode (symbolic),
  octal, links, owner, group (resolved via `getpwuid_r` / `getgrgid_r`),
  size (human), bytes, 512B blocks, mtime, atime, ctime, birth, name.
  Symlinks render as `name -> target` in the NAME column. Column
  widths are computed once across the whole selection so everything
  aligns. Renders inside the standard centered pager (90% width, top
  edge in the usual place), not full-screen — UX consistency with
  every other pager surface.
- **Pager `fit_to_content` mode.** New flag on `PagerView` /
  `PagerRequest` that shrinks the box from the bottom: same x, y, and
  width as the standard centered pager, but height = lines + borders
  + status row (capped at 92% of the terminal). Line-number gutter is
  suppressed since it's noise for short summaries. Long listing opts
  in so a single-file table (or even a 5-row directory listing)
  doesn't sit inside a 92%-tall frame.

## [1.18.6] - 2026-04-26

### Fixed
- **Captured shell (`!cmd`) pager no longer bleeds tail of
  longer lines through shorter ones.** spawn_capture runs the
  child under a pty whose slave has `ONLCR` on by default, so
  the child's `\n` becomes `\r\n` on the master side. The
  literal `\r` survived into our buffer; when ratatui rendered
  a line followed by a shorter line, the terminal interpreted
  the `\r` as carriage-return and the new line overlaid only
  as far as it was long, leaving the tail of the prior line
  visible. (`make help` in `~/src/system_setup` was a great
  repro — short lines following a long URL line.) Now we
  normalize `\r\n` → `\n` before feeding the buffer to
  `ansi_to_tui`. Standalone `\r` is preserved so in-place
  progress-bar updates still work.

## [1.18.5] - 2026-04-26

### Fixed
- **Trailing debounce on watcher refresh.** The previous
  debounce fired 500ms after the *first* event in a burst, which
  meant chained git operations (`git add && git commit && git
  push`) would have the refresh subprocess run during a
  transient mid-burst state — sometimes returning `M  BUGS.md`
  (staged but not committed) instead of clean. Once that
  transient sample landed, no further `.git/index` rename event
  fired (the commit's later side-effects only touched lockfiles
  we filter out), so the refresh never re-ran and the top bar
  stayed stale forever. Refresh now fires 500ms after the *last*
  listing event — wait for the storm to pass, then sample. Also
  rate-limits to 500ms between refreshes regardless.

## [1.18.4] - 2026-04-26

### Changed
- **Refresh debug log now includes the dirty file list** and
  the raw `git status --porcelain` output. We saw `git_files: 1`
  after a commit that should leave 0, but the prior logging
  didn't tell us *which* file was dirty — too many possible
  explanations (race with `.spyc-context-*.tmp`, stale BUGS.md,
  some other transient). Now `refresh_listing` logs the sorted
  dirty file basenames, and `git_status` logs both branch and
  the raw porcelain string. Run with `-d`, reproduce, and we'll
  know exactly what git was reporting at refresh time.

## [1.18.3] - 2026-04-26

### Changed
- **Debug-log diagnostics in `refresh_listing`.** Logs the
  before/after `git_info` and `git_files.len()` on every refresh
  (or the `Listing::read` error if it fails). Run with `-d` to
  diagnose when a watcher event fires but the display doesn't
  appear to update.

## [1.18.2] - 2026-04-26

### Fixed
- **Git status refresh on commit, take two.** 1.18.1's directory
  watch on `.git/` was right but the path filter still missed the
  case where macOS FSEvents *coalesces* multiple intra-directory
  changes into a single event whose path is `.git/` itself rather
  than `.git/index`. `is_listing_path` now also accepts `path ==
  .git/` (treating it as "something changed in there, refresh");
  the existing `index`/`HEAD` filter still applies to file-level
  events for backends/cases that deliver them.
- **Debug log now records every watcher event** (path, listing /
  config classification, event kind) — run spyc with `-d` to send
  events to `$XDG_STATE_HOME/spyc/debug.log` for diagnosis.

## [1.18.1] - 2026-04-26

### Fixed
- **Git status now actually updates after a commit.** 1.17.6
  taught `refresh_listing` to refresh `git_info` too, but the
  watch itself was unreliable: we were watching `.git/index`
  *as a file*, and `git commit` writes
  `.git/index.lock` then atomically renames it to `.git/index`.
  The rename replaces the inode, but our watcher kept its
  handle on the old (now-deleted) inode and stopped delivering
  events. Result: top-bar `main*` and the per-file dirty
  markers stayed stale until you switched directories, even
  though `refresh_listing` was correct. spyc now watches the
  `.git/` directory non-recursively and filters events to
  `index` (status / staging) and `HEAD` (branch switch); the
  rename lands as a directory event and the refresh fires.

## [1.18.0] - 2026-04-26

### Changed
- **Pane scroll-mode indicator is much harder to miss.** The
  divider rule line and the active tab label both retint to
  `theme.pick` (typically yellow) while in scroll mode, the
  active tab label is bold-uppercased (`[1*] CLAUDE`), and the
  right-side `[SCROLL]` tag picks up the same color. Three
  redundant signals across the divider — eye lands on at least
  one no matter where you're looking.
- **Entering scroll mode no longer jumps the pane.** Previously
  `enter_scroll_mode` shifted the viewport up by one line so
  there was *some* visual cue you'd left live view; with the
  new divider treatment the shift was just noise. The flag is
  now decoupled from `scroll_offset`, so entry is purely modal
  with no content motion. Also: `j` past the live position no
  longer auto-exits scroll mode — the mode is now purely modal,
  exit explicitly with Esc / q.

## [1.17.9] - 2026-04-25

### Changed
- **Session restore stops using `claude --resume`; types
  `/resume <sid>` after launch instead.** The CLI flag triggers
  a Claude regression where the mount-time
  `useEffect(...,[],g9H(K))` reads `onSessionRestored` from
  `FXK({enabled:false})`'s return value, gets `undefined`, and
  throws `g9H is not a function` — which wedges React while
  bun keeps the pty alive. Same effect doesn't fire on a fresh
  start (initialMessages is empty), so we now spawn fresh
  `claude` and, after a 1.5s settle delay, write
  `/resume <sid>\r` to the pane. The slash-command goes through
  `tM_` (a different code path that doesn't hit the bug). The
  crash-recovery prompt from 1.17.1 stays as a safety net for
  any path we missed.

## [1.17.8] - 2026-04-25

### Fixed
- **Claude crash-recovery prompt fires reliably again.** The
  1.17.5 simplify pass added an `output_dirty` gate to the
  crash-detection scan as a hot-path optimization, but
  `output_dirty` is cleared on every render. Claude prints its
  whole crash dump in well under a second and then sits
  quiescent, so by the time `dump_grace` (3s) elapses the flag
  is `false` — we'd skip the scan forever and the prompt would
  never fire. With 1.17.7's slug fix landing, restore now
  successfully spawns `claude --resume <sid>`, which trips the
  g9H regression and crashes — and *that's* exactly the case
  the silenced prompt was supposed to catch. Reverted the gate;
  the scan is bounded to the 30-second restore window and to
  tabs with `restore_fallback` armed, so it's not a meaningful
  cost.

## [1.17.7] - 2026-04-25

### Fixed
- **Session restore for projects with `_` (or any non-alphanumeric)
  in the path.** spyc's `project_slug` only rewrote `/` to `-`, but
  Claude rewrites *any* non-alphanumeric/hyphen char (so
  `tripstack_platform` lands at
  `~/.claude/projects/-Users-…-tripstack-platform/`, not
  `…-tripstack_platform/`). spyc was looking in the wrong directory,
  finding zero JSONLs, returning `None` from
  `resolve_claude_resume_target`, and saving sessions with no
  `claude_session_id` — so `spyc -r` always spawned a fresh
  `claude` for these projects regardless of how recent the
  conversation was. `project_slug` now matches Claude's
  normalization (any non-alphanumeric char → `-`); tests cover
  underscore, dot, and space.

## [1.17.6] - 2026-04-25

### Fixed
- **Top-bar git status now updates on file changes.** The
  watcher-triggered `refresh_listing()` only refreshed
  `git_files` (per-file dirty markers next to filenames); it
  never refreshed `git_info` (the branch + dirty string in the
  top bar — e.g. `main` vs `main*`). So after editing a tracked
  file, the per-row markers updated but the top bar stayed on
  `main`; switching directories forced a `chdir` which did
  refresh `git_info`. `refresh_listing()` now also calls
  `git_status()` so the top bar tracks repo state in place.

## [1.17.5] - 2026-04-25

### Changed
- **`make install` now depends on `make release`.** No more
  separate two-step dance — `make install` builds the optimized
  binary and copies it to `$(PREFIX)/bin` in one shot. README
  and INSTALL.md updated to drop the redundant `make release`
  line. The standalone `make release` target is unchanged for
  anyone who just wants a binary in `target/release/`.

## [1.17.4] - 2026-04-25

### Fixed
- **`!` (captured shell) now runs in spyc's listing dir.** The
  `!cmd` path went through `spawn_capture`, which built its
  `CommandBuilder` without setting a `cwd` — so the child
  inherited spyc's process cwd, which can drift from the
  navigated `state.listing.dir` (and only happens to match
  because `chdir()` also calls `set_current_dir`, which is
  best-effort and silently ignored on failure). `;cmd` worked
  fine because it explicitly passed `&self.state.listing.dir`
  to `Pane::spawn`. `spawn_capture` now takes a `cwd: &Path`
  and all four callers (`!`/`:!`/`:!!`/the `!?` history
  re-execute) pass `&self.state.listing.dir`. `make` from
  the project root now finds the Makefile.

## [1.17.3] - 2026-04-25

### Changed
- **Don't write `.mcp.json` under enterprise control.** When
  `/Library/Application Support/ClaudeCode/managed-mcp.json` (macOS)
  or `/etc/claude-code/managed-mcp.json` (Linux) defines a server
  named `spyc`, Claude already knows how to reach us through the
  org config. The per-project `.mcp.json` we used to write at every
  startup just collided on the server name (Claude resolves the org
  definition; the local file is dead weight). spyc now detects the
  managed definition, skips the write entirely, and removes any
  prior `spyc` entry from an existing `.mcp.json` (preserving any
  other servers the user has added; deleting the file if it only
  contained spyc). Status flashes `MCP: enterprise-managed (skipped
  local .mcp.json)` so it's visible. The takeover prompt is
  suppressed under enterprise control too — there's nothing to
  take over.
  Note: this *only* skips the local `.mcp.json` write. The Unix
  socket server (`mcp-<pid>.sock`) still runs so the org-defined
  `spyc --mcp` proxy can connect.

## [1.17.2] - 2026-04-25

### Fixed
- **Session restore no longer corrupts itself across cycles.** A
  saved tab's `command` was captured verbatim from the spawn
  string, so a tab spawned by restore as `claude --resume <sid>`
  would on the next save persist `command =
  "claude --resume <sid>"` instead of the user's original
  `"claude"`. When `resolve_claude_resume_target` later returned
  `None` (Claude had no fresh JSONL — e.g. a wedged or never-used
  conversation), the next restore fell back to `tab.command` and
  ran `claude --resume <stale-sid>` → fail → crash dump → tab
  closed → save again with same polluted command → infinite
  degradation. Save now strips `--resume <token>` from
  `tab.command` when it's a `claude` invocation, and the restore
  path applies the same strip defensively so already-corrupted
  session files heal on first reload.

## [1.17.1] - 2026-04-25

### Changed
- **Claude crash on resume now prompts before recovering.** The
  prior auto-respawn (1.16.2) only caught the case where
  `claude --resume <sid>` exited non-zero — but Claude has a
  regression where the resume path throws an unhandled
  `g9H is not a function`, leaving bun's pty alive while React is
  wedged. spyc now also detects "alive but printed a crash dump"
  by scanning the last ~200 lines of pane scrollback for stable
  markers (`/$bunfs/root/`, `is not a function`,
  `Error: sandbox required but unavailable`) at least 3 seconds
  after spawn. On detection it pops a one-key prompt:
  `claude crash detected — start fresh and recover with /resume?
  [Y/n]`. `y/Y/Enter` kills the child and spawns a fresh `claude`
  in the same slot; anything else kills it and removes the tab so
  the wall of minified JS is gone. The prompt is gated on
  `Mode::Normal` so it doesn't preempt other UI work — if you're
  busy with another prompt or pager, detection retries next loop.

## [1.17.0] - 2026-04-25

### Added
- **Host terminal title.** spyc now sets the outer terminal's window
  title to `🌶️: <project> · <session>` (e.g. `🌶️: spyc ·
  SAFFRON_CUMIN`). `<project>` is the basename of `PROJECT_HOME` when
  set, otherwise the basename of the cwd. Session is omitted when
  there's no `SESSION_NAME`. The pre-spyc title is pushed onto the
  terminal's title stack (xterm CSI 22;0t) on startup and popped on
  quit, including from the panic handler. Inside tmux, OSC 2 is
  wrapped in tmux's DCS passthrough so the outer terminal (iTerm2,
  etc.) sees it — requires `set -g set-titles on` in tmux. Updates
  are change-only (no redundant emits per draw); after a foregrounded
  child (vim, less) returns we force a re-emit in case it clobbered
  the title.

## [1.16.2] - 2026-04-25

### Fixed
- **Session restore now recovers from a failed `claude --resume`.** If
  a tab spawned by `spyc -r` as `claude --resume <sid>` exits non-zero
  within 10 seconds of spawn (bad/missing session id, sandbox crash,
  binary mismatch, …), spyc replaces the dead tab in place with a
  fresh `claude` and flashes `automatic session restore failed. try
  with /restore`. Previously the user was left staring at whatever
  Claude dumped on its way out — for sandbox crashes that's a wall of
  minified JS. The fallback preserves any extra flags from the
  original command (e.g. `--dangerously-skip-permissions`) and only
  strips the `--resume <token>` pair, so the replacement isn't
  doomed to fail the same way.

## [1.16.1] - 2026-04-25

### Fixed
- **Claude session resume no longer saves ghost UUIDs.** The
  resolver's last-ditch fallback (`find_claude_session`, which
  reads `~/.claude/sessions/<pid>.json`) trusted the PID-scoped
  index without checking that a JSONL actually existed. Claude
  writes the index entry as soon as `claude` starts, but the
  conversation JSONL only appears on the first turn — quitting
  spyc *before that first turn* produced a saved session ID with
  no file behind it, leading to "No conversation found with
  session ID …" on `spyc -r`. `resolve_claude_resume_target` now
  applies a final `claude_jsonl_exists` guard regardless of which
  branch produced the ID; if the file isn't there, we save no ID
  and restore opens a fresh `claude`. `claude_jsonl_exists` also
  checks the canonical (symlink-resolved) cwd, so macOS
  `/var` → `/private/var` paths don't slip through. A debug-log
  line records the dropped ID for future diagnosis.

## [1.16.0] - 2026-04-24

### Added
- **Live pane cwd in the divider line.** The pane status line now
  polls the active subprocess's actual cwd via `/proc/<pid>/cwd` on
  Linux and `lsof -Fn` on macOS (1-second TTL cache, render-path
  cost is negligible). When the live cwd differs from where spyc
  launched the tab — e.g. a `bash` tab where the user `cd`'d
  somewhere — the path gets a `↪` prefix and is rendered in the
  active-tab color so it's easy to spot. The previous tilde-collapse
  for `$HOME` is preserved via `paths::display_tilde`.
- **CLAUDE.md note on shell-continuity loops.** Claude Code doesn't
  have shell continuity between Bash tool calls — `cd /foo` in one
  call doesn't persist to the next, which is a real source of
  Claude getting stuck on `make`/`cargo`/test loops. Added an
  explicit instruction in `CLAUDE.md` covering compound `cd && cmd`,
  absolute paths, and the "run `pwd && ls` first when stuck" habit.
  The live-cwd indicator helps the *user* spot drift; this note
  helps Claude avoid the trap in the first place.

## [1.15.0] - 2026-04-24

### Added
- **`g b` — git blame on the cursor file.** Runs
  `git blame --color-lines -- <file>` and shows the colored output
  in the pager. Blame is single-file by design; the selection is
  ignored. Flashes a clear error if the cursor is on a directory or
  the file isn't tracked.

### Changed
- **`g d` now includes new (untracked) files.** Previously, sitting
  on an untracked file (`?` flag) and pressing `gd` produced empty
  output and looked broken — git diff doesn't know about untracked
  files. spyc now also runs
  `git ls-files --others --exclude-standard -- <selection>` to find
  untracked content under the selection, then synthesizes an "added"
  diff per file via `git diff --no-index /dev/null <file>`. The
  unstaged diff and the new-files diff are concatenated. `gD`
  (`--cached`) is unchanged — staging untracked files is a separate
  flow. Pager title is now `git diff (+ new)` to make the difference
  visible.

## [1.14.0] - 2026-04-24

### Changed
- **MCP takeover now prompts before clobbering another instance.**
  Previously, starting a second spyc in a directory already owned by
  a live spyc silently rewrote `.mcp.json` and notified the old
  instance to disconnect — easy to do accidentally and then wonder
  why your other session lost MCP. Now spyc detects the live
  instance before entering raw mode and prompts on stderr:
  `🌶️ spyc: PID 11935 already owns MCP here. Take over? [Y/n]`.
  Default Y on empty input. Decline ("n") and the old instance keeps
  ownership; the new spyc starts normally without MCP and flashes
  `MCP: kept PID 11935 as owner (Claude here will talk to it)`.
  Non-tty stdin (CI, piped input) keeps the historical auto-takeover
  behavior — there's no one to prompt.

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

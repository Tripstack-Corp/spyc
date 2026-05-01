# Changelog

All notable changes to spyc are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- **Pane zoom (fullscreen toggle).** `^a z` (and `^w z`) zooms the
  bottom pane: the file list collapses to 0 rows and the pane fills
  the middle region between status and prompt. Tmux-style — the
  status bar and prompt row stay visible, focus is forced into the
  pane on zoom-on and the prior focus is restored on un-zoom. The
  user's preferred `pane_height_pct` is preserved untouched so the
  prior split returns exactly on un-zoom. A `[ZOOM]` tag renders in
  the divider while active. `^a +` / `^a -` are no-ops while
  zoomed (with a status flash). Closing the pane (`^a \`) clears
  the zoom flag. Requested by a daily user.

## [1.37.2] - 2026-04-30

### Fixed
- **Shell aliases and rc-file PATH entries now work in `:!cmd`,
  `;cmd`, and pane prompts.** Previously, spyc spawned `sh -c <cmd>`
  regardless of the user's `$SHELL`, and even setting `$SHELL` would
  not have helped: aliases / functions live in interactive rc files
  (`.zshrc`, `.bashrc`) which non-interactive shells don't load.
  A user running `:!gemma` (where `gemma` is an alias for a local
  `llama.cpp` invocation) got `sh: gemma: command not found`. Now
  `spawn_capture` and `Pane::spawn` resolve `$SHELL` and pass `-i`
  to shells that source rc files in interactive mode (`zsh`, `bash`,
  `fish`, `ksh`, `mksh`); POSIX `sh` / `dash` get plain `-c` since
  they don't read rc files in `-i` mode anyway. Helper lives at
  `shell::user_shell_invocation`. FEATURES.md updated to describe
  the new behavior. Tradeoff: heavy `.zshrc` / `.bashrc` setups
  (oh-my-zsh banners, p10k init) may now print init noise into
  capture pagers; well-behaved rc files gate that behind
  `[[ -t 1 ]]` / `[[ $- == *i* ]]` and stay quiet.

### Changed
- **`make install` now defaults to `~/.local/bin` (no sudo).** The
  Makefile's `PREFIX` defaults to `$HOME/.local`. To install
  system-wide, override: `sudo make install PREFIX=/usr/local`. The
  install target prints a hint if `~/.local/bin` is not on `$PATH`.
  README, INSTALL.md, and CLAUDE.md updated to reflect the new
  recommended flow.

### CI / Tooling
- **`bitbucket-pipelines.yml` now calls `make check`** instead of
  inlining its own cargo commands. The Makefile's `test` target
  runs with `--test-threads=1` to serialize XDG_STATE_HOME-mutating
  state-module tests; CI was inlining `cargo test --all-targets`
  without that flag and hitting the race, leaving CI red on `main`.
  Calling `make check` keeps CI and local on the same exact gate.
- **Pipeline `target/` cache** added alongside the existing cargo
  cache, both keyed on `Cargo.lock` + `rust-toolchain.toml`.
  Should drop pipeline compile time materially on cache hits.
- **Code-tree `cargo fmt --all` sweep** to clear pre-existing
  formatting drift in `pager.rs`, `markdown.rs`, `fs/ops.rs`,
  `line_edit.rs`, and `app/mod.rs`. No behavior changes.
- **Pre-commit hook** in `scripts/git-hooks/pre-commit`. Install
  with `make install-hooks` — runs `make check` on every commit so
  fmt / clippy / test failures surface locally instead of ~10 min
  later in CI. Bypass with `git commit --no-verify` if you must.

### Security
- **`SECURITY.md`** — honest posture doc covering threat model,
  supply-chain controls, build/install trust chain, and known
  caveats. Avoids signing/SBOM theater for an internal tool with
  no published binary distribution channel.
- **`cargo deny check`** replaces `cargo audit` in CI. Same advisory
  coverage, plus license allow-listing (only the SPDX identifiers
  present in the actual dep graph), source allow-listing
  (crates.io only — no `git = ...` deps), and bans (yanked /
  multiple-major-versions). Configuration in `deny.toml`; ignored
  advisories list a documented reason each.
- **`--locked` on every `cargo` invocation** in the Makefile and
  pipelines (test, lint, all release builds, coverage). Prevents a
  CI-time `Cargo.lock` drift from silently pulling fresh transitive
  deps; failures are loud.
- **`make dist-sign`** scaffolding for GPG-signed checksum files.
  Not used today (we don't ship prebuilt binaries); SECURITY.md
  documents the intentional gap so a future signing rollout has a
  ready landing spot.

## [1.37.3] - 2026-04-30

### Fixed
- **Stray reverse-video cell when running TUI apps in the lower pane.**
  spyc's pane renderer was unconditionally painting a reverse-video
  block at `vt100`'s cursor position, even when the child had hidden
  the cursor with DEC `?25l`. Apps like lazygit, less, and vim hide
  the cursor and draw their own selection highlight, so the overlay
  showed up as a stray "glitch" cell sitting on top of the child's
  UI -- typically wherever the child had last left its (now-hidden)
  cursor. Now suppressed when `screen.hide_cursor()` is true.
  Reported via lazygit dog-fooding in the lower pane
  (`src/pane/widget.rs`).

## [1.37.1] - 2026-04-30

### Fixed
- **Stale `+` (or any) git marker after commit/push now self-heals
  within ~1s.** The `notify`-driven FSEvents watch on `.git/` would
  occasionally miss the `.git/index.lock` → `.git/index` atomic
  rename that happens on every commit -- macOS FSEvents has a known
  soft spot for inode replacement, so the listing dir's `+` / `~` /
  `?` markers (and the top-bar branch/dirty string) could stay
  stale until the user changed directories. Added a 1Hz safety-net
  poll: when `git_info` is set (we're in a repo), the run loop
  re-runs `git_status` and `git_file_statuses` once per second and
  diffs the results against the live state. Diff-aware -- only
  bumps `list_generation` and requests a repaint when something
  actually changed, so idle dps stays at 0. Watcher path is
  unchanged; this is a backstop, not a replacement.

## [1.37.0] - 2026-04-29

### Added
- **`:pause [N]` / `:resume [N]` for backgrounded tasks.** The
  top BIGGER-pile request: pause/resume execution so you can
  swap networks, free CPU, or just stop an over-eager build to
  focus on something else. Implementation sends `SIGSTOP` /
  `SIGCONT` to the task's *process group* (negative pid via
  `libc::kill`), so subprocess trees (e.g. `make → cc → ld`)
  all halt together rather than just the direct child. No-arg
  forms target the most-recent task; numeric arg targets a
  specific id. Same UX shape as `:fg [N]` / `:task [N]`.
- **`S` / `C` keybindings inside the task viewer** (`gB` /
  `:task N`) — Stop and Continue, the hand-on-keyboard
  shorthand for `:pause` / `:resume`.
- Divider glyph `[N⏸]` for paused tasks (mixed in with the
  existing `[N+]` / `[N●]` / `[N✓]` / `[N✗]`).
- `:fg` on a paused task **auto-resumes** before re-attaching
  the streaming capture, so the user doesn't get a frozen
  foreground pager.
- `paused: bool` field added to `BackgroundTask`.

### Fixed
- Cleared the "pause and resume execution of backgrounded
  tasks" entry from BUGS.md BIGGER pile (it's the feature this
  release adds).

## [1.36.0] - 2026-04-28

### Changed
- **Markdown table cells now wrap to multiple visual rows**
  instead of truncating with `…`. v1.35.0 capped each column at
  ≤24 chars and `…`-truncated overlong content; the result was
  unreadable on tables like the README key-binding tables where
  the `Action` column has full sentences. Now: each cell is
  wrapped at its column width via the same `word_wrap_ranges`
  routine the paragraph renderer uses (par-style word boundaries
  with hard-break fallback for unbreakable tokens). The visual
  height of a row is the max wrap-rows across cells; cells that
  wrap to fewer rows are padded with blank lines so the column
  borders stay aligned. Per-span styling (`**bold**`, `*italic*`,
  `code`) preserved across wrap boundaries via `slice_spans`.
  `truncate_spans_to_width` is gone -- nothing called it after
  the cell renderer switched to wrap.

## [1.35.2] - 2026-04-28

### Fixed
- **Streaming `!cmd` capture pager auto-tail uses real viewport
  height** instead of a hardcoded 40 rows. Repro: run a long
  capture (`!cargo build` or similar) on a tall terminal; the
  pager would render ~63 rows tall but the auto-tail would only
  scroll enough to show the last 40 lines -- the bottom of the
  pager filled with `~` markers while content sat in the upper
  half. The "go to top + bottom" workaround that fixed it
  manually was just `G` reading the actual viewport height.
  Same bug affected `:fg` resume of backgrounded tasks. Fix:
  cache the rendered viewport height on `PagerView.last_viewport_h`
  (a `Cell<u16>`) during render; tick-loop auto-tail reads it
  via the new `scroll_to_bottom_auto()`. Falls back to 40 on
  the very first frame before any render has run -- harmless
  since the next frame replaces it.

## [1.35.1] - 2026-04-28

### Fixed
- **`w` / `b` / `e` / `dw` / `cw` / `^W` now respect punctuation
  boundaries**, matching vim's default `iskeyword`. Previously
  the line editor's word motions split only on whitespace, so
  `foo-bar` was treated as a single word and `dw` from position 0
  deleted the whole thing. Now `dw` on `foo-bar` deletes only
  `foo` -- the same behavior any vim user expects when editing
  paths, kebab-case identifiers, flag values, URLs, etc.
  Implemented via a `CharClass` helper (`Word` / `Punct` /
  `Space`); word motion stops at any class transition. 4 new
  unit tests cover `w`, `dw`, `cw`, `^W` against `foo-bar`.

## [1.35.0] - 2026-04-28

### Added
- **Markdown tables now render with proper borders.** v1.26.0
  punted on tables ("tables fall through unstyled — out of scope
  for v1"); the result was cell text getting mashed together as
  inline text. Now: tables get a real renderer with box-drawing
  borders (`┌┐└┘├┤┬┴┼─│`), bold headers, dim slate borders.
  Column widths computed from natural cell content, capped at
  24 chars per column, then proportionally trimmed so the whole
  table stays inside the 80-col content budget. Cells longer
  than their allotted width truncate with `…`. Inline emphasis
  inside cells (`**bold**`, `*italic*`, `code`) is preserved
  thanks to the same span-styling pipeline the rest of the
  renderer uses. 2 new tests cover border rendering and
  overlong-cell truncation.

## [1.34.1] - 2026-04-28

### Fixed
- **`/` "no matches" flash now renders inside the pager**, not on
  the spyc file-list status bar underneath. The pager search
  routed its empty-result feedback through `state.flash_error`
  which lives on the file-list pane; the message would appear
  *behind* the pager overlay where the user wasn't looking. Now
  it's set on `view.flash` so the pager's own title-bar flash
  (teal-on-amber, per v1.27.4) carries it.

## [1.34.0] - 2026-04-28

### Changed
- **History popup is now opened by `Esc Space`** (vi prompts) and
  `Esc <Space>` for `J` (also a vi prompt as of v1.33.0), not
  the v1.31.0/v1.32.0 double-Esc. The user found double-Esc
  fights Esc's "back out of something" muscle memory; Space in
  Normal mode reads more naturally as "expand into the bigger
  view." Space is unused in our line editor's Normal mode, so
  no binding conflict.
- Sequence on every prompt with history: type → Esc (enters
  Normal) → Space (opens kind-specific popup). Single Esc no
  longer escalates -- it just toggles Insert↔Normal as standard
  vi.

## [1.33.0] - 2026-04-28

### Changed
- **`J` is now a vi-line-editor prompt** (was a "simple prompt"
  with append-only buffer editing). User feedback: after pulling
  up a history entry with j/k or Up/Down, you should be able to
  *tweak* it before submitting -- e.g. recall `~/src/spyc` and
  append `/src` before Enter. The simple prompt only supported
  end-of-buffer typing + Backspace, so cursor positioning, word
  motion, mid-buffer delete etc. were all unavailable.
- Promoting J to vi-line-editor unifies its key handling with
  `!` / `;` / `:`. All four prompts now share the same model:
  - First Esc: Insert → Normal mode
  - Normal-mode `j`/`k` (or Up/Down anywhere): walk history
  - Second Esc (in Normal): open the kind-specific popup
    (`show_jump_history_popup` for J, `show_history_popup` for
    the others)
  - Full vi line editing: h/l motion, w/b/e word motion, x/D/C
    delete operators, A/I/0/$ position, etc.
- `browse_mode` field removed from `Prompt` (was added in v1.32.0
  to fake a vi-mode for the simple prompt; redundant now that J
  has the real thing).
- All four history-push routings already worked from v1.28.0;
  removed the duplicate Submit-push for Jump from the
  simple-prompt path that v1.29.3 added (handle_vi_prompt_key
  picks it up via history_for_prompt).

## [1.32.0] - 2026-04-28

### Added
- **`J` now matches the vi-prompt double-Esc pattern.** First Esc
  on a `J` prompt enters "browse mode" (no popup yet); j/k
  walks history inline like vi Normal-mode j/k. Second Esc
  (already in browse mode) opens the full jump-history popup.
  Typing any non-nav character drops out of browse mode and
  resumes normal text editing. Backspace-on-empty and `^C`
  unconditionally cancel.
  - Reverses v1.29.0/v1.29.2's behavior where Esc on an empty J
    buffer opened the popup directly. Now Esc always enters
    browse mode first; the popup is the second tap. Consistent
    with the `!`/`;`/`:` model shipped in v1.31.0.
  - `browse_mode: bool` field added to the `Prompt` struct so
    simple prompts can carry the same kind of mode-state vi
    line editors track internally.

## [1.31.0] - 2026-04-28

### Added
- **Double-Esc opens the history popup in vi prompts** (`!`,
  `;`, `:`). First Esc puts the line editor in vi Normal mode
  (existing behavior); second Esc (when already in Normal)
  opens the `!?` popup. j/k inside the popup browse, Enter
  submits, ^D deletes, q/Esc closes. Mirrors J's Esc-on-empty
  popup, generalized to any vi prompt.

### Known limitation
- The popup currently shows shell history regardless of which
  vi prompt opened it. For `!`/`;` that's correct; for `:`
  (command line, which has its own `command_history` since
  v1.28.0) the popup will show the wrong bucket -- mostly
  empty for users who don't also use `!`. Fixing requires
  parameterizing the popup helper to take a kind and routing
  the popup's submit back to the matching dispatch
  (`dispatch_command` for `:`, etc.). Tracked in ROADMAP.

## [1.30.0] - 2026-04-28

### Added
- **`Up` / `Down` in the `J` prompt cycle through jump history
  inline** (replaces the buffer with the prev/next entry, just
  like `:` and `!` already do). v1.28.0's changelog claimed this
  worked but the wiring was wrong twice over: history-push lived
  in the vi-prompt branch (which `J` doesn't use), and Up/Down
  was never registered in the simple-prompt branch at all. Now
  the simple-prompt path has its own Up/Down handler that walks
  `jump_history.prev` / `next`, with `reset_nav` on cancel /
  submit so the next `J` opens fresh at the most-recent entry.

### Fixed
- **`j` / `k` work in the jump-history popup.** v1.29.0's popup
  set `picker_cursor` but never wired the j/k → picker_move
  arms; the pager dispatch doesn't have a generic picker-nav
  fallback, each popup type has to wire its own. Added them to
  the `pending_jump_history` block so j/k navigate as expected
  (matches the session picker's pattern).

## [1.29.3] - 2026-04-28

### Fixed
- **`J` submissions actually push to `jump_history` now.** Same
  bucket of bug as v1.29.2: v1.28.0's history-push lived in
  `handle_vi_prompt_key`'s Submit arm, but `J` is a simple
  prompt that submits via `handle_prompt_key`'s Enter branch
  and never reached the editor flow. Result: every J jump
  silently *didn't* persist, the popup forever flashed "jump
  history is empty," Up/Down had nothing to recall. Push moved
  into the simple-prompt Enter handler, gated on
  `PromptKind::Jump`. New jumps now persist; the v1.29.0
  popup is finally reachable with content.

## [1.29.2] - 2026-04-28

### Fixed
- **`Esc` on empty `J` prompt actually opens the jump-history
  popup now.** v1.29.0 put the check in `handle_vi_prompt_key`,
  but `J` is a "simple prompt" (no line editor) and dispatches
  through `handle_prompt_key`'s simple-prompt branch — never
  reaching the vi-editor path where my check lived. Moved the
  check into the simple-prompt branch ahead of the generic
  Esc-cancel arm.
- **`^C` in `J` (and other simple prompts) cancels** — same
  fix shape as v1.29.0's `^C` handling, but at the
  `handle_prompt_key` simple-branch level so it actually
  reaches J / search / pattern-pick etc.

## [1.29.1] - 2026-04-28

### Changed
- **Jump-history popup uses `x` to delete** instead of `^D`,
  matching the inventory view's `x` for "remove this item."
  Consistency with the rest of the spyc surface. The `!?` shell-
  history popup keeps `^D` because it has a vi line-editor
  active, where `x` is already taken by the editor; the jump
  popup has no editor so `x` is unambiguously "delete entry."

## [1.29.0] - 2026-04-28

### Added
- **`Esc` on an empty `J` prompt opens a jump-history popup.**
  Scrollable list of every jumped-to path, newest first. `j`/`k`
  navigate, `Enter` chdirs to the cursored path (and pushes it
  to the top of MRU so the next browse surfaces it), `^D` deletes
  the entry from history, `q`/`Esc` closes. Esc on a *non-empty*
  J buffer still cancels normally -- only the empty-buffer case
  switches to the popup, since there's nothing to throw away.
- **Option+Enter sends a newline to the pane on terminals that
  support the kitty keyboard protocol.** `setup_terminal` now
  pushes `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES`
  before entering the TUI; modern terminals (Ghostty, Kitty,
  WezTerm, foot, recent Alacritty, iTerm2 with the experimental
  flag) report `Option+Enter` as an unambiguous `Alt+Enter`
  KeyEvent. Old Terminal.app silently ignores the request -- on
  that one, users still need "Use Option as Meta key" in their
  profile preferences. Also broadened `pane::input::encode_key`:
  *any* modified Enter (Alt, Ctrl, Shift, Super/Meta/Hyper) now
  folds to `\n` so weird per-terminal modifier reports all
  produce the multi-line newline Claude expects.

### Fixed
- **`^C` in a `:` / `J` / `!` / `;` prompt cancels** instead of
  flashing the "use Q to quit" hint. v1.27.1's hint was the right
  thing in normal mode but wrong in prompts where vi muscle
  memory wants `^C` ≡ `Esc`. The hint now skips Prompting mode;
  `handle_vi_prompt_key` intercepts `^C` and routes to
  `cancel_prompt`. Capture mode still forwards `^C` to the child
  as 0x03 (sudo / ssh prompts unaffected).

## [1.28.0] - 2026-04-28

### Added
- **`J` (jump to path) gets its own persistent history.** Up /
  Down in the J prompt now cycle through previously-jumped
  destinations, persisted to
  `$XDG_STATE_HOME/spyc/jump_history`. Tab-completion + frecency
  hits still work as before; this is a parallel surface for
  "take me back to that thing I jumped to yesterday."

### Fixed
- **`:` and `!` no longer share a history bucket.** Real-world
  repro: type `!make sync-all` (a shell command), later type `:`
  and press Up to recall something — the buffer surfaces
  `make sync-all`, you submit it, spyc fires "unknown command:
  make sync-all" because `:` is the vim-style command line, not
  a shell. Now `:` has its own `command_history` file
  (`$XDG_STATE_HOME/spyc/command_history`) and the four buckets
  stay fully isolated: shell (`!`/`;`), pane-tab cmd/cwd, jump,
  and command-line.

## [1.27.4] - 2026-04-28

### Changed
- **Pager flash messages now render in teal-on-default** instead
  of inheriting the amber title color. Real-world miss: the
  `truncated at 5000 lines · press p for full file in $PAGER`
  notice on a capped file rendered in the same amber as the
  filename, looking like part of the title; users (me) didn't
  read it as a separate help notice. Now the title stays amber,
  the flash segment renders in teal + BOLD with a thin space
  buffer on each side -- visually clear that "this part is a
  notice, not part of the filename." Same treatment applies to
  every flash (yank confirmations, save confirmations, "no
  source file" warnings, etc.).

## [1.27.3] - 2026-04-28

### Fixed
- **`^C` in `p` → less now interrupts less cleanly** (rather than
  appearing to be ignored). v1.27.2 stopped spyc from dying on
  Ctrl+C, but the no-op-handler approach left spyc and the child
  sharing a process group, so signals went to both. less *did*
  receive SIGINT but interactions between two processes seeing
  the same FG-group signal led to "less seems to miss the
  signal" symptoms (race-y disposition handling, signal mask
  ambiguity, etc.). Fix: proper Unix job control around
  `run_child_in_foreground`:
  - Child spawned with `process_group(0)` ⇒ becomes leader of a
    new process group (PGID == child PID).
  - After spawn, `tcsetpgrp(stdin, child_pid)` makes the child's
    group the foreground group of the controlling tty. Now `^C`
    / `^\` from the kernel go to the child *only*.
  - On wait completion, `tcsetpgrp(stdin, our_pgid)` restores
    spyc as the FG group; SIGTTOU (raised on a non-FG-group
    process calling tcsetpgrp) is now ignored permanently in
    `install_signal_handlers` so the restore call doesn't
    suspend spyc itself.
  - Same shape that bash/zsh use to launch foreground commands.
    Less, vim, and any other child now get clean signal
    delivery and behave exactly as they would in a normal
    terminal.

## [1.27.2] - 2026-04-28

### Fixed
- **`^C` in a `p`/`v`/`;` takeover no longer kills spyc.**
  Real-world repro: `p` opens `less` on a huge file; `G` jumps to
  end and triggers a long line-count; user hits `^C` to abort the
  count → less *quits entirely* AND spyc exits. In a normal
  terminal less would just stop counting and stay open.
  - Root cause: spyc runs in raw mode (kernel `ISIG` disabled,
    `^C` arrives as a key event). When suspending for the
    `p`/`v`/`;` takeover, raw mode is restored to canonical, and
    `^C` from the tty driver is delivered as `SIGINT` to the
    *whole foreground process group* — which is spyc's process
    group, since the child inherited it. Both processes get the
    signal: less handles it gracefully (interrupt the count, stay
    open), spyc dies on the default disposition. The tty session
    leader exits → kernel `SIGHUP`s remaining processes → less +
    sh die too. From the user: "spyc died on ^C in less."
  - Fix: install no-op handlers for SIGINT and SIGQUIT in spyc at
    startup. spyc receives the signal, ignores it. Per POSIX
    `execve(2)`, custom handlers are reset to `SIG_DFL` in the
    child, so less / vim / etc. receive the signal with normal
    disposition and handle it themselves. (Pure `SIG_IGN` would
    inherit across exec, breaking the child's signal handling --
    that's why a custom no-op handler is the right shape.)

## [1.27.1] - 2026-04-28

### Fixed
- **Truncation now flashes the `p` hint immediately on open.**
  v1.27.0 added a banner row at the *end* of the truncated content,
  but if the file's the first 5000 lines and the user doesn't scroll
  to the bottom, they'd never see the escape hatch. Now: a flash
  message ("truncated at N lines · press p for full file in
  $PAGER") appears in the title bar the moment a truncated view
  opens, alongside the existing footer banner.
- **Pager-help (`?`) `Esc` now dismisses just the help, not the
  underlying pager.** Before: pressing `?` pushed the active pager
  into history, opened help; pressing `Esc` then closed the help
  *and* dropped you back to the file list, requiring `[b` or `gp`
  to reopen what you were viewing. Now: `Esc` / `q` on the
  help overlay pops the previous pager from history and restores
  it as active. Help is also flagged `no_history = true` so it
  can't accidentally land in the buffer-history stack.
- **^C in spyc-normal mode flashes an explicit hint** instead of
  silently doing nothing. Real-world repro: `p` opens `$PAGER`,
  user hits ^C to abort a struggling `less`, comes back to spyc
  thinking ^C may have been "captured." Now the flash makes the
  contract explicit: `^C is not a quit binding — use Q (or :q) to
  quit, Esc to cancel modes`. Capture mode still forwards ^C to
  the running child (sudo/ssh prompts behave normally).

## [1.27.0] - 2026-04-28

### Added
- **`p` (in pager) — open in `$PAGER` (full-screen takeover).**
  Mirrors `v` / `$EDITOR`: resolves `$PAGER` (default `less`),
  suspends the TUI, runs the external pager on the current
  file, resumes spyc on quit. The right tool for full traversal
  of huge files, interactive `less`-style search, or piping
  through marks. Buffer pagers without a source path (`!cmd`
  output, `:grep` results) flash "no source file (try `s` to
  save first)" instead.

### Fixed
- **Pager no longer OOMs on huge files.** Previous behavior was
  `read_to_string(path)` + syntect over the whole content, which
  built a `Vec<Line<'static>>` with millions of styled spans on
  multi-MB CSVs/logs -- pager state ballooned to ~50× file size
  in worst cases. Now: files above 5 MB load only the first 5000
  lines (plain text, syntect skipped — that's the dominant memory
  amplifier). Title gets a `⚠ truncated · X MB` suffix; a banner
  row at the end of the truncated content points at the new `p`
  binding for full-file viewing. Markdown rendered-mode also
  skips for truncated files since rendering half a doc looks
  weird (broken refs, half-closed code fences).
- 3 new `read_truncated` tests cover under-cap, over-cap, and
  exact-cap-boundary cases.

## [1.26.3] - 2026-04-28

### Fixed
- **`!cmd` captures now advertise `TERM=dumb` instead of
  `xterm-256color`.** The capture pager only renders ANSI SGR
  colors and CR/LF intelligently; cursor positioning, alt-screen,
  and mouse codes get stripped or render as garbage. Lying about
  vt100 capability meant `!less foo`, `!vim foo`, `!htop` etc.
  would switch into alt-screen TUI mode and either freeze the
  capture or write unrenderable cursor games into the pager.
  `TERM=dumb` is the canonical "nothing fancy" signal:
  TUI programs refuse to run as TUIs (they dump to stdout or
  print a friendly error and exit cleanly), which is exactly
  what we want for capture mode. `;cmd` (foreground in the top
  pane) remains the path for genuine TUI programs.
  `FORCE_COLOR`, `CLICOLOR_FORCE`, and `COLORTERM=truecolor` are
  kept so tools that respect those (cargo, eza, bat, ripgrep)
  keep producing colored output despite `TERM=dumb`.

## [1.26.2] - 2026-04-28

### Added
- **`Y` (capital) yanks the *visible* pager content** to the
  clipboard. Lowercase `y` still yanks the source (the POLA
  default). Most useful with the Markdown viewer in rendered
  mode: `Y` gives you back the styled-but-plain rendering
  (headings with `#`, bullets, blockquote rules, 80-col wrap)
  that you can paste into chat or a doc, without having to
  toggle to source first. In all other contexts (regular files,
  capture pagers, `:grep` results) `y` and `Y` are identical
  because the visible text *is* the source. Flash text
  distinguishes the two ("yanked source" vs "yanked visible")
  so you know which one fired.

## [1.26.1] - 2026-04-28

### Changed
- **Markdown content wraps at 80 cols (par-style).** The renderer
  now word-wraps paragraphs and list items at 80 visual columns
  inside `src/ui/markdown.rs` itself (not via the pager-level wrap),
  preserving per-span styles across break points and dropping
  trailing whitespace. List-item continuation rows get a hanging
  indent that matches the bullet width, so wrapped text aligns
  under the item content rather than under outer-level bullets.
  Code blocks pass through unwrapped (their formatting matters).
  Blockquote content wraps inside the rule prefix (78 col content
  + 2 col `┃ `). The pager pane stays full-width as before; only
  the content body is bounded.
- **Line-number gutter and inline `code` are no longer washed out.**
  Both used `status_suffix + DIM` which left them barely legible
  against dark backgrounds. Line numbers drop the DIM modifier
  (`status_suffix` alone is plenty subtle); inline code switches to
  `theme.take` (teal) — semantically reads as "code" and contrasts
  cleanly with body text.

### Added
- 4 new markdown tests: long-paragraph wrap, list-item continuation
  indent, word-wrap range breaks, hard-break fallback.

## [1.26.0] - 2026-04-28

### Added
- **Markdown viewer with source ↔ rendered toggle.** `.md` /
  `.markdown` files now open in *rendered* mode by default --
  headings styled, lists with bullets, fenced code blocks
  syntect-highlighted by language, blockquotes with a left rule,
  links rendered with the destination URL appended, inline
  bold/italic/strikethrough preserved. Press `m` in the pager to
  toggle to the syntect-highlighted source view, `m` again to
  flip back. Non-Markdown files flash "not a markdown file" on
  `m`. Both views are pre-computed at file-open so the toggle is
  instant.
  - Yank (`y`) and save (`s`) always operate on the *source*
    regardless of view mode -- POLA: yanking a README should give
    you back the markdown text, not the styled rendering.
  - Search / `n` / `N` match the *active* rendering: what you see
    is what you find. Toggle to source first if you need to grep
    for raw markdown syntax.
  - Scroll resets to top on toggle (rendered/source line counts
    differ; preserving an absolute index would land somewhere
    arbitrary).
  - Tables, images, and embedded HTML are out of scope by design;
    tables fall through unstyled, images render as `[image: url]`
    placeholders, raw HTML renders as dim text.
  - Built on `pulldown-cmark` + a small `src/ui/markdown.rs`
    renderer (~370 LOC). 7 unit tests cover heading prefix,
    paragraph flow, bullet list, blockquote rule, fenced code
    block fences, link rendering, and extension detection.

## [1.25.0] - 2026-04-28

### Added
- **Pager line wrap is back, done properly this time.** v1.21.6
  removed `Paragraph::wrap` because ratatui's wrap hard-breaks
  unbreakable tokens (paths, log lines) mid-character and the
  line-number gutter didn't carry across continuation rows --
  visible misalignment like `Builde$.cs` on long paths. New impl
  pre-computes visual-width chunks ourselves with per-span style
  preservation: long lines wrap cleanly at viewport width, wide
  CJK characters and emoji count as 2 cols (same as ratatui's
  layout), continuation rows get a blank gutter so wrapped pieces
  visually align with the source line's indent, and the `$`
  end-of-line whitespace marker stays on the actual end of the
  source line (last wrapped piece). Default ON for content
  pagers (file viewers, `:grep`, `!cmd` capture, task viewer);
  explicitly OFF for the `F` finder picker where each source line
  must map 1:1 to a selectable row. Toggle: `W` (capital) in the
  pager. 5 unit tests cover hard-break, span splitting, wide
  chars, and zero-width edge case.

## [1.24.2] - 2026-04-28

### Changed
- **Custom-code reduction sweep.** Continuing the v1.24.1 jiff
  swap, replaced four more hand-rolled implementations with
  established crates after a code survey:
  - **`fs/ops.rs` — uid/gid/localtime via `uzers` + `jiff`**.
    Deleted ~70 lines of `unsafe` `getpwuid_r` / `getgrgid_r` /
    `localtime_r` libc FFI plus a duplicated date-formatter.
    `format_local_time_from_unix` now uses
    `jiff::Timestamp::from_second(..).to_zoned(system).strftime`.
  - **`state/inventory.rs` — `make_id` via `uuid::Uuid::now_v7`**.
    The previous "simple UUID-like" hex-of-nanos generator could
    collide on rapid yanks; UUIDv7 is time-ordered with random
    suffix, no collision risk.
  - **`app/mod.rs` — ANSI stripping via `strip-ansi-escapes`**.
    Replaced ~40 lines of hand-rolled CSI/OSC parsing with the
    BurntSushi-adjacent crate. Kept the spyc-specific
    `strip_crlf` 3-pass normalizer (taking the *last* CR-frame
    on a line is a deliberate UX choice).
  - **`sysinfo::epoch_secs` / `epoch_nanos`**. Six files were
    each spelling out
    `SystemTime::now().duration_since(UNIX_EPOCH)...` -- now they
    call shared helpers backed by `jiff::Timestamp::now()`.

  Net: -~110 LOC, two new tiny deps (`uzers`, `strip-ansi-escapes`,
  `uuid`), one less `unsafe` block, one less custom date algorithm.
  All 456 tests pass serially.

## [1.24.1] - 2026-04-28

### Changed
- **Date formatting moved to `jiff`.** Replaced the hand-rolled
  Howard Hinnant `civil_from_days` algorithm in `sysinfo.rs` with
  `jiff::Timestamp::now().to_zoned(UTC).strftime(...)`. Same
  output (`YYYY-MM-DD HH:MM:SS UTC`), one less algorithm we have
  to maintain. `jiff` joins the existing BurntSushi crates we
  already depend on (`grep-regex`, `grep-searcher`, `ignore`).

## [1.24.0] - 2026-04-28

### Added
- **Project-wide search MCP exposure (M3 of project-wide search).**
  Four new tools, all gitignore-aware where applicable, all
  PROJECT_HOME-scoped (cwd fallback if no project root):
  - `search_paths(query, [limit])` — fuzzy filename search via the
    same `ignore` walker + `nucleo-matcher` ranking the `F` picker
    uses. Returns a JSON array of repo-relative paths, fzf-style
    ranked. Default limit 100, max 1000.
  - `search_content(pattern, [limit])` — content search via the
    same embedded ripgrep matcher `:grep` uses (smart-case, binary
    files skipped). Returns a JSON array of `{path, line, col,
    text}`. Default limit 200, max 5000.
  - `search_picks(pattern, [limit])` — content search restricted
    to the user's currently-picked files. **Uniquely spyc-shaped**:
    picks are TUI multi-select state Claude can't see otherwise,
    so this is the only way to grep the user's intended subset.
  - `search_inventory(pattern, [limit])` — content search across
    the persistent inventory cache (yanked-into-cache files that
    survive sessions). Lets Claude grep accumulated "interesting
    files" without leaving the conversation.
- 3 new MCP roundtrip tests (search_paths, search_content,
  search_picks). 3 new fs::grep tests (search_to_vec cap,
  search_files explicit-set scoping, invalid-regex error).

## [1.23.3] - 2026-04-28

### Fixed
- **`:grep` no longer scrambles tab-separated content.** Real-world
  repro: searching `tarzan` in tripstack_platform turned hits in
  `postcodes.txt` (a TSV file) into garbled overlapping text --
  `Tarzana    California` rendered as `rzCliforn aorniarnCA`. Cause:
  ratatui counts `\t` as zero-width via `unicode-width`, but
  terminals expand it to ~8 columns, so ratatui's position
  tracking drifts from the terminal's actual cursor and content
  visibly overlaps. Fixed in `sanitize_line`: tabs now expand to
  the next 4-column boundary (chosen over 8 to keep result lines
  compact, since most paths are already deep).

## [1.23.2] - 2026-04-28

### Fixed
- **`:grep` pager gutter no longer jitters mid-scan.** The line-
  number gutter width is computed each frame from
  `ilog10(view.lines.len())`, so as results streamed in the gutter
  widened from 1→2→3→4 chars at every power-of-10 boundary -- and
  every existing visible row shifted right by one column at each
  step. Visible content also realigned weirdly when the user
  toggled `l` mid-stream. Fixed by adding `line_count_hint` to
  PagerView; streaming views (currently `:grep`) seed it with the
  result-count cap so the gutter is sized for the worst case from
  the start. Also: `:grep` now defaults to **line numbers on**
  (was off) -- the row index is the most useful column for
  navigating result lists.

## [1.23.1] - 2026-04-28

### Fixed
- **`:grep` no longer corrupts the terminal on binary files.**
  Real-world repro: running `:grep test` in a workspace with
  tracked `.docx`, `.dll`, `.jar`, `.pdf` files dumped raw bytes
  (NULs, ESCs, backspaces) into the pager, scrambling colors and
  cursor positioning. Two fixes:
  - Searcher now uses `BinaryDetection::quit(0)` -- ripgrep's
    default. The first NUL byte in a file aborts the search of
    that file, so binary blobs are skipped.
  - Matched-line text is sanitized before display: control bytes
    (everything < 0x20 except tab, plus DEL) are replaced with
    `·`, CR/LF trimmed, and lines wider than 400 chars truncated
    with `…`. Catches sourcemap blobs, base64-inlined assets, and
    text files that happen to contain ANSI escapes.
- Also added `:grep` to the AppState command passthrough list so
  the prompt parser routes it to the terminal-touching arm; without
  it, `:grep test` flashed "unknown command".

### Added
- 2 new tests: binary-file skip behavior and `sanitize_line` length
  cap + control-byte filter.

## [1.23.0] - 2026-04-28

### Added
- **`:grep <pattern>` — project-wide content search (M2 of project-
  wide search).** Embedded ripgrep matcher (`grep-regex` +
  `grep-searcher`, the BurntSushi crates ripgrep itself uses), no
  subprocess. Walks `PROJECT_HOME` (or the listing dir as fallback)
  honoring `.gitignore`, smart-case by default. Matches stream into
  a pager as `path:line:col: text` -- the same shape `gf`/`gF`
  already understand from pane output, so jumping from a hit into
  the file is free. Same multi-repo-aware walker as the `F` finder:
  pass 2 picks up sibling-clone subrepos the outer `.gitignore`
  excluded. Capped at 5000 matches; refine the pattern if you hit
  it. Pattern errors flash inline before opening an empty pager.
  Power users with custom `~/.ripgreprc` or fancy flag combinations
  can still drop down to `! rg foo` for ripgrep's full surface.
- 8 unit tests cover smart case, gitignore honored, sibling-clone
  descent, invalid-regex error, and receiver-drop cancellation.

## [1.22.2] - 2026-04-28

### Fixed
- **`F` finder descends into sibling-clone subdirs that the parent
  repo's `.gitignore` excludes.** Real-world repro:
  `~/src/tripstack_platform` is a git repo whose `.gitignore` has
  entries like `book-org/`, `content-acquisition/`, etc. -- not
  because the user doesn't want to see those files, but because
  those subdirs are *separate clones* (each with its own `.git`)
  living inside the parent dir. Pass 1 of the walker (gitignore-
  aware from the parent) correctly skipped them, but the user
  expects `F` to find files anywhere checked out under the
  workspace. Now the walker runs a second pass: when the start
  root is itself a git repo, it scans for nested `.git/`
  directories that pass 1 missed and walks each as its own
  ignore root (with `parents(false)` so the outer repo's
  gitignore doesn't bleed in). Each subrepo's own gitignore is
  still honored within its tree.

## [1.22.1] - 2026-04-28

### Changed
- **`F` finder walks on a worker thread.** v1.22.0 walked
  synchronously on F-press, blocking the picker open for ~100-200ms
  on large monorepos. The walker now runs on a background thread
  and streams candidate batches (256 paths each) into the picker
  via an `mpsc::channel`. The picker is interactive immediately
  (the user can start typing before the walk finishes), and the
  candidate count + ranked results live-update as batches arrive.
  Title shows "scanning…" while the walk is in progress; flips to
  the final count when done. Closing the picker drops the receiver,
  which makes the walker exit cleanly on its next `tx.send`.

## [1.22.0] - 2026-04-28

### Added
- **`F` project-wide fuzzy filename finder.** First milestone of
  the project-wide-search ROADMAP entry. New key in the file list
  walks `PROJECT_HOME` (or the listing dir as fallback) honoring
  `.gitignore` via the `ignore` crate, ranks candidates against
  typed input with `nucleo-matcher` (basename hits outrank
  parent-dir hits, fzf-style). Up/Down move selection, Enter
  chdirs to the matched file's parent and places the cursor on
  it; Esc cancels. Walk runs lazily on open (no persistent
  index); 100K-file cap so a monorepo doesn't load the whole
  kernel into RAM. Subsequent milestones (`:grep` content search,
  MCP `search_paths` / `search_content` / `search_picks` /
  `search_inventory` tools) remain on the ROADMAP.

## [1.21.7] - 2026-04-27

### Fixed
- **Git status markers on parent-directory rows update on
  subtree changes.** Previously, adding/modifying a file in a
  subdirectory (e.g. `docs/foo.md` while sitting at the repo
  root) didn't update the `docs/` row's git marker -- you had to
  navigate into the subdirectory before the change registered.
  Two pieces: (1) the `notify` listing watch was `NonRecursive`,
  so subtree events never reached the loop; (2) `is_listing_path`
  only matched the dir itself or direct children, so even a
  recursive watch's events would have been rejected. Now: watch
  is `Recursive`, and `is_listing_path` accepts anywhere under
  the listing dir while keeping `.git/` carved out (only `index`
  and `HEAD` direct children count, so background gc / pack /
  refs activity doesn't cascade into needless `git status`
  subprocesses). The 500ms trailing debounce already in place
  bounds the cost on noisy subtrees. macOS FSEvents handles
  recursive watches at the OS level (cheap); Linux inotify
  needs a watch per subdir, which can hit
  `fs.inotify.max_user_watches` on enormous monorepos.

## [1.21.6] - 2026-04-27

### Fixed
- **Single-column pager truncates long lines instead of wrapping.**
  The `!cmd` / task-viewer / file-view pager used
  `Paragraph::new(...).wrap(Wrap { trim: false })`, which made
  ratatui hard-break long unbreakable words (paths, log lines)
  mid-character; continuation rows don't carry their own line-
  number gutter, so the `$` whitespace marker landed mid-row and
  the gutter accounting drifted on subsequent rows ("Builde$.cs"-
  style mismatches in long `git log` output, especially with
  `w` toggled on). Behavior now matches the multi-column path
  and `less -S`: clip at the right edge. Yank / save / search
  operate on `view.lines`, so the full untruncated content
  remains available regardless of how the visual rendering
  clips.

## [1.21.5] - 2026-04-27

### Fixed
- **`!cmd` capture pager strips stray ASCII control bytes** (NUL,
  SOH, backspace, vertical tab, form feed, etc.) that ansi-to-tui
  used to pass through to ratatui. Real-world repro: a long
  `git log` whose commit-message rendering emits `\x01` (SOH)
  before each conflict-list line. The host terminal swallowed the
  byte but ratatui's width accounting didn't, drifting the rest of
  the rendered line (`Buil$er.cs`-style misalignment with `w` on).
  `strip_crlf` gained a third pass that filters 0x00-0x08,
  0x0b-0x0c, 0x0e-0x1a, 0x1c-0x1f, 0x7f while keeping `\t`, `\n`,
  and `\x1b` (ESC for ANSI sequences). Same fix path covers the
  task viewer.

## [1.21.4] - 2026-04-27

### Fixed
- **`!` captures no longer launch a sub-pager.** `git log`, `man`, and
  any tool that probes `isatty(stdout)` and defers to `$PAGER` would
  detect our slave PTY as a real TTY and invoke `less`, which then
  took the PTY hostage waiting for keystrokes *inside* spyc's
  pager. `spawn_capture` now sets `PAGER=cat`, `GIT_PAGER=cat`,
  `MANPAGER=cat` in the child env so the tools dump directly and
  spyc's pager wraps the whole result. Foreground (`;`) commands
  and pane tabs are unaffected -- they should keep paginating
  since the user owns the TTY there.

## [1.21.3] - 2026-04-27

### Fixed
- **Pasting into `!` / `;` / `:` prompts now splices at the cursor**
  instead of appending to the end of the line. The bracketed-paste
  handler used to `push_str` to the prompt buffer regardless of
  where the cursor was; now, when the prompt has a vi line editor
  attached, it calls a new `LineEditor::insert_str(&str)` that
  inserts each char at `cursor` and advances. The canonical
  `Prompt.buffer` is then synced from the editor's text. Simple
  prompts (search, mkdir, file/dir name) still append since they
  have no cursor concept. Lets you `!` `git ` ⏎-paste-back-from-`!?`
  history-Esc-`b` (move back a word)-paste-mid-cursor without
  having to retype.

## [1.21.2] - 2026-04-26

### Fixed
- **`!` capture and task viewer collapse bare `\r` progress-bar
  updates to the last frame.** `git pull` / `npm install` / `cargo
  build` use bare carriage return (no newline) to overwrite
  progress on the same line; `ansi-to-tui` doesn't process `\r`,
  so we were rendering every frame side-by-side as one super-wide
  line. `strip_crlf` gained a second pass: for each `\n`-delimited
  segment, keep only the bytes after the *last* `\r`. Live
  streaming reads the latest frame each tick, and the saved view
  shows the final clean line. ANSI sequences never embed bare
  `\r`, so the byte-level pass is safe. Five new tests cover the
  passes individually and combined.
- **Task viewer no longer shows `[EOF]` while the task is still
  running.** `build_task_viewer_for` sets `view.streaming` based
  on `TaskStatus::Running`, and the per-tick refresh now fires on
  Running → Exited transitions (not just on new bytes), so the
  title and `[EOF]` marker keep up with reality when a task
  quietly finishes mid-view.

## [1.21.1] - 2026-04-26

### Added
- **`gp` reopens the most-recently-closed pager buffer** from the
  file list. Pairs with `gB` for "go to bg-task viewer" -- both pop
  the most-recent thing of their kind. When no buffers are in
  history, flashes "no buffers in history" instead of doing nothing.

### Changed
- **New `Background tasks & buffer history` help section** groups
  `^Z`, `:fg`, `g B`, `:task N`, `[t]t`, `g p`, `:bprev`, `:bnext`,
  `[b]b`, plus the divider-glyph legend (`[N+]`/`[N●]`/`[N✓]`/`[N✗]`)
  in one place. The `g B` and `:task N` bindings used to be tucked
  inside `Shell-out & commands` next to `:fg` -- easy to miss.

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

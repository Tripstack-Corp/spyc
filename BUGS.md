### SMALL ###
- view mode in the lower pane should show line numbers and be able to yank
  based on line number range
- using editor in top pane prevents switching to bottom pane
- hitting ^c with the task pager up also sent ^c to the lower pane - causing
  the task in the lower pane to erroneously cancel
- notice of ^c while in the task viewer goes to the spyc pane instead of at the
  top of the task pager view
- D in spyc pane should open in $PAGER in the top pane
- / should match within names - it seems to assume ^ e.g. env won't match .env
- MCP socket discovery can attach to the wrong spyc instance. When
  `$SPYC_MCP_SOCK` is unset (e.g. `claude` launched outside spyc's
  pane, env didn't propagate, or the local `.mcp.json` was suppressed
  by enterprise managed-mcp), `discover_live_socket` in
  `src/mcp.rs:153` returns the first connectable
  `~/.local/state/spyc/mcp-*.sock` it finds — could be any other
  spyc on the host, including another user's. Conflicts with the
  multi-instance isolation model. Design fixes worth weighing:
  (a) require explicit `$SPYC_MCP_SOCK`, no discovery fallback;
  (b) gate discovery on a per-project marker (e.g. only accept
  sockets whose context file's project_root matches the caller's
  cwd); (c) include user/uid in the socket path. Option (b) feels
  most spyc-shaped — keeps the "just works" ergonomics while
  ruling out cross-instance attachment.
- how do we keep up with updates to crates we depend on?
- pane widget always paints a reverse-video cursor block at
  `screen.cursor_position()` even when the child has set `DEC ?25l`
  (cursor hidden). `vt100::Screen` already exposes `hide_cursor()`;
  `src/pane/widget.rs:43-54` just doesn't read it. Most likely
  cause of the rendering glitch reported when running lazygit in
  the lower pane (lazygit hides the cursor and draws its own
  selection highlight, so the stray block shows up as a cell
  inverted in the wrong place). Fix is a single guard on
  `screen.hide_cursor()` before the reverse-video paint.
- pane spawn env should advertise `COLORTERM=truecolor`. We
  already set `TERM=xterm-256color`
  (`src/pane/mod.rs:102` in pre-quickselect numbering), but
  modern apps that runtime-negotiate truecolor (lazygit/tcell,
  bat, fzf) check `$COLORTERM` first and silently downgrade to
  256-color when it's missing. Symptom: diff palettes look
  "close but slightly off" inside the pane vs. a bare terminal
  tab. One-line addition next to the existing `TERM` env line.
- it's very confusing still to remember you're in scroll mode in the bottom
  half - we need a stronger top line/bottom line marker for this
- the interactive picker tool - gum - handles our ! view very poorly - we
  should investigate why and see if we can patch
- cwd should update when we quit based on where spyc is navigated to (may
  already be mentioned in roadmap?)
- change in git state while viewing a subdirectory did not automatically get
  updated; need to try and reproduce
- something funky is happening with our MCP support - we need to ensure that
  multiple running spyc's don't interfere with eachother
- a pane that has ended should not be so easy to dismiss with ESC; it should
  require ^a-x so that the user purposefully says they are done with it e.g. I
  may still way to try and restart it
- screen should flash if I'm doing something that hits a wall - e.g. j at the
  top of a directory (the ~ in the status is not enough)
- we should be able to send control signals to running processes e.g. ^t
- cw didn't seem to be worked as expected in ! (? need to confirm - may have
  been using an old version); maybe we should put a build commit hash in the
  top right?
- `brew upgrade` parallel-download progress doesn't redraw in place inside
  the lower pane — each tick stacks 4 fresh lines (gradle / claude-code /
  python@3.14 / python@3.13) into scrollback instead of overwriting. Looks
  like brew's concurrent-downloads UI uses cursor-up to redraw N progress
  lines, and that's failing. Top suspect: lines printed at the last pty row
  scroll into scrollback before `\e[NA` runs, so the redraw lands below
  rather than over the originals (a tall iTerm window has the headroom to
  hide this; the short lower pane doesn't). Capture with
  `SPYC_PTY_DEBUG=1` set at launch and inspect `/tmp/spyc_pty_debug.bin`
  for `\e[NA` sequences to confirm vs. brew choosing line-mode.

### BIGGER ###
- task runner should be equivalent to a lower pane and be able to push down
  into lower pane ^w-_ or into the background ^w-z
- codex doesn't support vi bindings ... maybe we could control it and inject vi
  bindings into it's rataui pane
- pane forwards no mouse events to the child. spyc never calls
  `EnableMouseCapture` on the host terminal
  (`src/main.rs::setup_terminal`), and `src/pane/input.rs` has no
  encoder for `Event::Mouse(_)`. `vt100` *tracks* the mouse
  protocols when the child enables them (1000/1002/1003/1006), but
  no events ever reach the pty. Apps that default to
  `MouseEvents: true` (lazygit, htop, broot in mouse mode) look
  half-broken — clicks on panel headers / commit list / footer
  keybindings, scroll-wheel on diffs, all silently no-op. Two-layer
  fix: enable mouse capture on the host terminal *and* add the
  `Event::Mouse` arm in `pane::input::encode_key` to encode SGR
  mouse reports the child expects. Worth designing carefully
  because spyc itself doesn't want mouse events outside the pane —
  the right shape is "forward to pane only when pane is focused."
- yank last response possible? [only if claude code terminal? is there an "api"
  in CC for it to better maintain over time?
- include a SMALL model that can conversationally answer how to do stuff with
  spyc; maybe there's a good crate to sidecar with this functionality?
- while I am drafting a command for a new pane it would be nice to still be
  able to switch to another pane to check on something
- would like to be able to reorder tabs
- directories should persist masking setting / we should be able to enable
  disable masks and have an editable list of them
- yanking from the pane should support # so that you can yank the last 150
  lines, etc.
- ^v should change focus and paste to the lower pane (image paste for Claude)

### MAYBE ###
- vt100 0.15 doesn't parse `\x1b[?2026h…\x1b[?2026l` (synchronized
  output / "mode 2026"). Apps that wrap every redraw in 2026
  (lazygit/tcell, recent neovim) get partial-frame tearing during
  fast scrolls — the renderer reads a half-finished frame and
  paints it. spyc already *emits* mode 2026 itself (perf refactor,
  see FIXED entries), so the protocol is well-supported on the
  host side; only the pane's vt100 parser is behind. Either
  upgrade the `vt100` crate (the unmaintained 0.15 → a
  community-fork or alacritty's `vte` parser) or live with the
  tearing. Big dep change; defer until someone notices.
- vt100 0.15 doesn't parse OSC 8 (terminal hyperlinks). lazygit's
  footer ("Donate" / "Ask Question") and any modern tool that
  emits `\x1b]8;;url\x07label\x1b]8;;\x07` will show stray bytes
  or a label without the link. Same dep-upgrade story as mode 2026
  above; same defer rationale.
- explore swapping `ansi-to-tui` for a real vt100 emulator on captured `!`
  output (the same one the pane already uses). Today we collapse bare `\r`
  to the last frame to handle progress bars (v1.21.2), and that handles
  git/npm/cargo cleanly -- but a tool that uses cursor positioning
  (`\x1b[A`, alt-screen, etc.) would still render wonky. vt100 emulation
  would be the "correct" fix for everything; tradeoff is a per-tick screen
  grid maintained instead of a flat byte buffer, which is heavier on
  long-running captures.
- would be nice to add a "are you sure you want to interrupt?" protection with
  Claude CLI procs
- yr (yank recursive) — yank directories into inventory; would need
  recursive put and inventory UI changes
- yc (yank chmod) — preserve permissions through yank/put cycle
- count prefix doesn't work with vi line editor operators (e.g. 3cw,
  2dw) — the count is consumed by the resolver but the line editor
  doesn't read it. Low priority for a single-line editor.
- claude cli should always be pinned to bottom of the terminal - it seems to get
  scrolled halfway up sometimes and Claude PTY can get messed up — scrollback
  accumulates rendering artifacts from Claude CLI's progress bars, spinners, and
  cursor repositioning. ^L redraws the visible screen but can't fix corrupted
  scrollback. Solution t.b.d.

### FIXED ###
- (fixed, v1.41.16) Pane cursor block no longer clobbers nvim's
  beam (or any TUI's own cursor). Block is only painted when the
  pane is focused AND the child is on the main screen. nvim / vim
  / less / htop / lazygit / claude in TUI mode now render their
  own cursor unobstructed; shells / REPLs on main screen still
  get the visibility block.
- (fixed, v1.41.15) Huge directories no longer hang spyc on chdir.
  `Listing::read` caps at 50,000 entries; truncated reads flash a
  hint so the user knows the listing isn't the full picture. The
  pre-fix behavior on a 1M-entry tmp dir was a multi-minute event
  loop block that required killing the terminal.
- (fixed, v1.41.14) Staged vs unstaged changes now visually distinct
  via a two-character marker mirroring `git status -s` (col 0 =
  staged side, col 1 = unstaged side). `M ` ready-to-commit,
  ` M` working-tree-only, `MM` partially staged + further edits.
  Each char colored independently. `GitFileStatus` upgraded from a
  flat enum to a struct carrying both halves + an untracked flag.
- (fixed, v1.41.13) Unfocused side now visibly dims so the user can
  tell at a glance which half is the input target. Per-cell
  `Modifier::DIM` on the unfocused widget (file list when pane is
  focused; pane when list is focused). Cursor row's existing
  dim-bg treatment stacks on top so it stays distinguishable.
- (defensive, v1.41.12) "switching panes input doesn't work when done
  too quickly" — couldn't reproduce, but two plausible failure modes
  were addressed: (1) post-chord bounce: a focus-switch chord
  (`^a-j`/`^a-k`) now suppresses a same-key Press/Repeat within 60 ms
  so a fast chord doesn't leak a stray byte into the just-focused
  pane; (2) stranded paste: `Event::Paste` outside Prompting / with
  no pane open now flashes "paste ignored" instead of silently
  dropping. Also added `--key-trace`/`SPYC_KEY_TRACE` for diagnosing
  the next report — writes every event + dispatch decision to
  `/tmp/spyc-key-trace-<ts>.log` with elapsed-since-start timestamps.
- (fixed, v1.41.11) `]g` / `[g` jumps the cursor to the next /
  previous git-changed entry in the listing (file or directory
  carrying any of the `~`/`+`/`?`/`-`/`>` markers). Wraps. From a
  user request: "maybe you could make a jump hotkey in the
  filetree to go to the next dir/file that has a diff."
- (fixed, v1.41.9) Pane child trees now exit cleanly on `^a x` / `^a K`
  and on spyc quit. Pre-fix, closing a tab running `npm run dev`
  (or anything with subprocesses) orphaned the whole `node` →
  `esbuild` → workers tree because `portable_pty::Child`'s default
  Drop is a no-op. New `Pane::shutdown(grace)` SIGTERMs the child's
  process group (negative PID — reaches the whole tree), waits up
  to 250 ms, then SIGKILLs. Wired into both the close-tab path and
  the end of `App::run`. `Drop for Pane` is a SIGKILL safety net
  for panic / error-propagation paths so children never leak.
- (fixed, v1.41.4) `/<query>` followed by `n n n` in the help pager no
  longer pins the view at the bottom of the file. Symptom: in the
  multi-column help overlay, searching for "show" and pressing `n` a
  few times left every column scrolled to the end of its chunk with
  no match visible. Cause: `scroll_to_match` fed the global line
  index straight into `self.scroll`, but in multi-col rendering
  `scroll` is interpreted per-column (each column applies the same
  offset within its own chunk). A match in column 2 produced a
  scroll value larger than `scroll_max` and got clamped to the
  bottom. Now translates the match's global line index to a
  chunk-local offset for multi-col views; single-col is unchanged.
  Regression test pins the rule.
- (fixed, v1.41.3) `:fg` now opens the pager already populated with
  the task's buffered output and scrolled to the bottom. Symptom:
  resuming a backgrounded `cargo build` (or any chatty task)
  showed an empty pager — or, once the next chunk arrived, content
  scrolled to row 0 with the live tail off-screen — making it look
  like nothing was running. Root cause: `foreground_task`'s Running
  branch built the `PagerView` with `lines: Vec::new()` and handed
  the existing buffer straight over to `pending_capture`; the
  streaming-tick only rebuilds pager content when a NEW chunk
  arrives, so the pre-existing buffer was invisible until then,
  and even after that the at-bottom check was racing the line-count
  growth. Now seeded the same way `:task N` (peek viewer) seeds —
  render the buffer and `scroll_to_bottom_auto()` before re-attaching
  the streaming capture.
- (fixed, v1.41.2) Per-file git markers (`~`/`+`/`-`/`?`) no longer
  spuriously dirty a clean root-level file when a sibling-named
  file in a subdirectory is modified. Symptom: working in
  `tripstack_platform`, the root `AGENTS.md` rendered with `~` even
  with no local edits, because `content-acquisition/AGENTS.md` was
  modified and `git_file_statuses` collapsed every status entry to
  its basename — so the deep file's status leaked onto the root
  row. The map now only stores a basename entry for files actually
  in the listing directory; deeper entries only mark the parent
  directory. Added a parser-level test (`parse_porcelain_statuses`)
  so the regression is caught without spawning git.
- (fixed, v1.41.2) `^C` now reaches the lower-pane subprocess when
  the pane is focused. The "^C is not a quit binding" hint-flash
  was firing before the pane-forward dispatch, so `^C` to interrupt
  zsh / a running command got swallowed and the user saw a flash
  instead of a SIGINT. The guard now skips when the pane has focus;
  the spyc-normal footgun protection still fires when the file list
  has focus. Other control codes (`^T`, `^D`, etc.) were already
  forwarded — only `^C` carried the extra guard.
- (fixed, v1.37.1) Stale git markers (`+` / `~` / `?`) after a
  commit + push self-heal within ~1s instead of sticking until the
  user changes directories. Root cause: macOS FSEvents on `.git/`
  occasionally misses the `.git/index.lock` → `.git/index` atomic
  rename (inode replacement is the watcher's known soft spot), so
  the post-commit refresh never fired. The watcher is still the
  primary path; added a 1Hz safety-net poll in the run loop that
  re-runs `git_status` + `git_file_statuses` and diffs against the
  live state when we're in a repo. Diff-aware: only bumps
  `list_generation` and requests a repaint when something actually
  differed, so idle dps stays at 0. Implemented as
  `AppState::refresh_git_state(&mut self) -> bool` (state.rs)
  driven by a `last_git_poll` ticker in `App::run` (app/mod.rs).
- (fixed, v1.37.0) Backgrounded tasks can be paused and resumed.
  `:pause [N]` sends SIGSTOP to the task's process group (whole
  subprocess tree halts together: make → cc → ld); `:resume [N]`
  sends SIGCONT. Inside the task viewer (`gB` / `:task N`),
  `S` and `C` are shorthand. Paused tasks render as `[N⏸]` in
  the divider; `:fg` on a paused task auto-resumes before
  re-attaching. Useful for the canonical "switching networks"
  scenario plus any general "pause this for a sec" need (CPU
  relief, debugging another task, freeing a port).
- (fixed, v1.35.2) Streaming `!cmd` capture pager auto-tail no
  longer leaves the pager half-empty. The tick-loop calls to
  `scroll_to_bottom(40)` and `page_lines(40)` were hard-coded
  estimates from the v1.20-era code, written before the pager
  understood its own viewport size. On tall iTerm windows the
  pager would render at ~63 rows but auto-tail only enough to
  show the last 40 -- the bottom of the pager box would fill
  with `~` markers while content sat in the upper half. Same
  bug also affected resumed tasks via `:fg`. Fix: cache the
  rendered viewport height on `PagerView.last_viewport_h`
  during render; tick-loop auto-tail uses
  `scroll_to_bottom_auto()` which reads it. Falls back to 40
  on the first frame before any render has run.
- (fixed, v1.35.1) `w` / `b` / `e` / `dw` / `cw` / `^W` now
  treat punctuation as a word boundary, matching vim's default
  `iskeyword`. So `dw` on `foo-bar` from position 0 now deletes
  only `foo`, not the whole `foo-bar`. The line editor's word
  motions previously split only on whitespace, treating `foo-bar`
  as a single word -- annoying when editing paths, kebab-case
  identifiers, flag values, etc. Implemented via a `CharClass`
  helper (Word / Punct / Space) that's checked at each step;
  word motion stops at any class transition.
- (fixed, v1.35.0) Markdown tables now render with proper
  ASCII-aligned borders. Previously fall-through-unstyled --
  cells got jammed together as inline text. Now: enable
  `Options::ENABLE_TABLES` in pulldown-cmark, collect cells
  per-row, render with box-drawing borders (┌┐└┘├┤┬┴┼─│),
  bold headers, dim slate borders. Per-column widths capped
  at 24 chars and trimmed proportionally so the whole table
  stays inside the 80-col content budget. Overlong cells
  truncate with `…`.

- (fixed, v1.28.0) `J` (jump-to-path) now has its own persistent
  history bucket. Up / Down in the prompt walk previously-jumped
  destinations independently of shell-command and pane-prompt
  history. Tab + frecency completion still works as before.
- (fixed, v1.28.0) `:` (vim-style command line) and `!` (shell
  capture) no longer share a history. Repro: type
  `!make sync-all` (a real shell command). Later type `:` and
  press Up — the buffer surfaces `make sync-all`, you submit it
  as a `:` command, spyc errors with "unknown command: make
  sync-all". Now `:` has its own `command_history` file, isolated
  from `!`/`;` shell history.
- (fixed, v1.27.3) `^C` in `p` → less now reaches less cleanly.
  v1.27.2 stopped spyc dying on the signal but left spyc and the
  child sharing a process group, so SIGINT went to both and the
  signal-disposition / mask interactions between them caused
  less to appear to miss the signal. Fix: proper Unix job
  control -- child gets its own process group via
  `process_group(0)`, becomes the foreground group of the tty
  via `tcsetpgrp` for the duration of the run, parent restores
  on wait completion (SIGTTOU ignored permanently so the restore
  doesn't suspend spyc). Same pattern bash/zsh use; less / vim /
  etc. now get clean signal delivery.
- (fixed, v1.27.2) `^C` while in a `p`/`v`/`;` takeover used to
  kill both the child *and* spyc. Repro: `p` on a huge file → less
  → `G` to count lines → `^C` to abort count → spyc exits along
  with less. Cause: spyc and child shared the foreground process
  group; `^C` from tty driver hit both; spyc's default SIGINT
  disposition = terminate. Fix: install no-op handlers for SIGINT
  and SIGQUIT at startup. POSIX `execve` semantics reset custom
  handlers to `SIG_DFL` in the child, so less/vim still receive
  signals normally; spyc just ignores them.
- (fixed, v1.27.0) Pager no longer OOMs on huge files. We were
  doing `read_to_string(path)` + syntect over the whole content,
  which on a multi-MB CSV/log built a Vec<Line> with millions of
  styled spans -- pager state ballooned to ~50× the file size.
  Now: files above 5 MB load just the first 5000 lines (plain
  text, no syntect amplification) with a banner row pointing at
  the escape hatch. New `p` binding in the pager: opens the file
  in `$PAGER` (default `less`) via full TTY takeover -- mirrors
  `v` / $EDITOR. Right tool for full traversal of huge files,
  saves us from reimplementing less inside spyc.
- (fixed, v1.26.3) `!cmd` captures used to advertise
  `TERM=xterm-256color`, which lied about our actual capabilities
  -- the capture pager only renders ANSI SGR + CR/LF, no cursor
  positioning or alt-screen. So `!less foo`, `!vim foo`, `!htop`
  would happily switch into TUI mode and either freeze waiting
  for keystrokes or write unrenderable cursor games into the
  pager. Now we advertise `TERM=dumb` which makes those programs
  fall back to plain dump-to-stdout (or print a friendly "this
  terminal lacks features" error and exit). FORCE_COLOR /
  CLICOLOR_FORCE / COLORTERM kept so tools that respect those
  still produce colored output despite the dumb terminfo signal.
- (fixed, v1.25.0) Pager line wrap is back -- this time done by
  spyc instead of ratatui's `Paragraph::wrap`. Pre-computed
  visual-width chunking with per-span style preservation, so
  long unbreakable tokens hard-break cleanly without the
  v1.21.6 "Builde$.cs" misalignment. Continuation rows get a
  blank gutter (no line number, no `$` whitespace marker), so
  the wrapped pieces visually align with the source line's
  indent. Default ON for content pagers (file viewers, `:grep`,
  `!cmd` capture); explicitly OFF for picker UIs (`F` finder)
  where each source line must map 1:1 to a selectable row.
  Toggle: `W` in the pager.
- (fixed, v1.21.7) Git status markers on parent-directory rows update
  when a file changes in a subtree below. The listing watch was
  `RecursiveMode::NonRecursive` (no events for subdir changes) and
  `is_listing_path` only accepted the dir itself or direct children
  (would have rejected subtree events anyway). Now: recursive watch
  with subtree-wide acceptance, `.git/` carved out for tight
  filtering (`index`/`HEAD` only) so background gc/pack churn
  doesn't cascade. Repro: add `docs/foo.md` from outside spyc; the
  `docs/` row gets the `~` marker within ~500ms (debounce) instead
  of staying clean until you `chdir` into it.
- (fixed, v1.21.6) Single-column pager no longer wraps long lines.
  Wrap was previously on (`Wrap { trim: false }`) but ratatui
  hard-breaks long unbreakable "words" (paths, log lines)
  mid-character, and continuation rows don't carry their own
  line-number gutter -- the visual result was things like
  `Builde$.cs` (line-end `$` mid-row) on long paths in `git log`
  output. Now long lines truncate at the right edge instead,
  matching the multi-column path and `less -S` semantics. Yank /
  save / search still operate on the underlying `view.lines`, so
  the full content is always available even when the visual
  rendering is clipped.
- (fixed, v1.21.5) `!cmd` capture pager no longer renders garbled
  output for content with stray ASCII control bytes (NUL, SOH,
  backspace, vertical tab, form feed, etc.). Real-world repro: a
  long `git log` whose commit-message indent path emits `\x01` (SOH)
  before each `Conflicts:` line. ansi-to-tui passed those through,
  the host terminal consumed the byte but ratatui's width
  accounting didn't, and the rest of the rendered line drifted --
  showing things like `Buil$er.cs` (line-end marker mid-word).
  `strip_crlf` gained a third pass that filters control bytes
  (0x00-0x08, 0x0b-0x0c, 0x0e-0x1a, 0x1c-0x1f, 0x7f) while keeping
  `\t`, `\n`, and `\x1b` for ANSI sequences. Three new tests cover
  the SOH case, other control chars, and the keep-list.
- (fixed, v1.21.4) `!` captures no longer launch a sub-pager. `git log`,
  `man`, and friends probe `isatty(stdout)` and auto-invoke `$PAGER`
  (less by default) when stdout is a TTY -- which it always is for
  our captures, since we run children under a slave PTY for prompt
  handling. Less would then take the PTY hostage waiting for keys
  inside our pager. `spawn_capture` now sets `PAGER=cat`,
  `GIT_PAGER=cat`, `MANPAGER=cat` in the child env so tools dump
  directly and spyc's pager wraps the whole result. Foreground (`;`)
  commands and pane tabs are unaffected -- those should keep
  paginating since the user owns the TTY there.
- (fixed, v1.21.3) Bracketed-paste into the `!` / `;` / `:` prompt now
  splices at the cursor instead of appending to the end. The paste
  handler had `p.buffer.push_str(&clean)` regardless of cursor; now,
  when the prompt has an editor (shell prompts), it calls a new
  `LineEditor::insert_str` that inserts each char at the cursor and
  advances. Simple prompts (search, mkdir, etc.) keep the append
  behavior since they have no cursor concept. Three new unit tests
  cover splice-at-cursor in Insert mode, end-of-line paste, and
  start-of-line paste.
- (fixed, v1.21.2) `!cmd` capture pager (and task viewer) now collapse
  bare `\r` progress-bar updates to the last frame, so `git pull` /
  `npm install` / `cargo build` no longer paint dozens of partial
  frames side-by-side as one super-wide line. `strip_crlf` gained a
  second pass: for each `\n`-delimited segment, keep only the bytes
  after the last `\r` -- the same final state a real terminal would
  show. Streaming pagers re-run this every tick, so the user sees
  live progress (latest frame each redraw) and a clean final line on
  exit. ANSI escape sequences never embed bare `\r` so the byte-level
  pass is safe. (For tools that go further -- cursor positioning,
  alt-screen -- see the full vt100-emulator MAYBE item above.)
- (fixed, v1.21.2) Task viewer (`gB` / `[t]t` / `:task N`) no longer
  shows `[EOF]` while the underlying task is still running. The
  viewer's `streaming` flag now reflects task status, and the
  per-tick refresh fires on Running → Exited transitions too (not
  just on new bytes), so the title and `[EOF]` marker keep up with
  reality even when a task quietly finishes mid-view.
- (fixed, v1.19.1) `q` no longer quits -- it's reserved for the
  future macro-recording feature already on the ROADMAP. Pressing
  `q` flashes "q reserved for future macro recording -- Q or :q to
  quit" and does nothing else. `Q`, `^D`, and `:q` still quit. Avoids
  a fat-finger silent-quit when switching from vim.
- (fixed, v1.19.0) `L` long listing now emits an aligned table
  (header + one row per file) with inode, mode, octal, links, owner,
  group (names via `getpwuid_r` / `getgrgid_r`), size, bytes, 512B
  blocks, mtime, atime, ctime, birth, and name (symlinks as
  `name -> target`). Column widths computed once across the whole
  selection so rows align. Renders inside the standard centered
  pager with `fit_to_content` shrinking the height from the bottom,
  so a single-file or short listing doesn't sit inside a 92%-tall
  frame. Line-number gutter suppressed since it's noise for the
  tabular content.
- (fixed) git status updates correctly after chained git operations.
  The 500ms debounce fired from the *first* event of a burst, so
  `git add && git commit && git push` had refresh's subprocess
  sometimes land mid-burst returning `M  BUGS.md` (staged but
  not committed). After that single off-sample, no further
  `.git/index` rename fired (later side-effects only touched
  lockfiles we filter), so the bar stayed stale. Switched to
  trailing debounce — fire 500ms after the *last* event — so
  refresh only samples once the burst has settled. `git checkout`
  worked because it's a single event, not a burst.
- (fixed) pane scroll-mode is hard to miss now: divider rule
  line and active tab label retint to theme.pick (yellow) while
  scrolling, active tab label is bold-uppercased, the [SCROLL]
  tag picks up the same color. Entering scroll mode no longer
  shifts the viewport (the cue is now visual instead of
  positional), so there's no "jump" on entry.
- (fixed) session restore now sidesteps Claude's
  `--resume`-on-mount crash by spawning fresh `claude` and
  typing `/resume <sid>` after a 1.5s settle delay. The
  slash-command path doesn't hit the same broken useEffect.
- (fixed) crash-recovery prompt fires reliably again. The
  1.17.5 simplify pass gated the scan on `pane.output_dirty`,
  but that flag is cleared on every render — claude prints its
  whole dump in <1s and goes quiescent, so by the time
  `dump_grace` elapses the flag is false and the prompt would
  never fire. Reverted the gate.
- (fixed) session restore for projects with underscores (or any
  non-alphanumeric char) in the path. `project_slug` only
  rewrote `/` → `-`; Claude rewrites *any* non-alphanumeric to
  `-` (so `tripstack_platform` → `-Users-…-tripstack-platform`
  on disk, not `-…-tripstack_platform`). spyc looked in the
  wrong dir, found zero JSONLs, and saved every session with
  `claude_session_id: null`, so `spyc -r` always spawned fresh
  claude for those projects. Slug now mirrors Claude's full
  normalization.
- (fixed) top-bar git status now updates on file changes.
  `refresh_listing()` only refreshed per-file markers
  (`git_files`), never the branch/dirty string in the top bar
  (`git_info`); only `chdir` refreshed it. Editing a tracked
  file therefore left the top bar stale; switching dirs made
  it pop. `refresh_listing()` now refreshes both.
- (fixed) `!cmd` (captured shell) now runs in spyc's listing
  dir. `spawn_capture` was building its CommandBuilder with no
  cwd, so the child inherited spyc's *process* cwd — which
  drifts from the navigated `state.listing.dir` if
  `set_current_dir` ever silently fails. `;cmd` worked because
  it explicitly passed listing.dir to `Pane::spawn`. Plumbed cwd
  through `spawn_capture` and all four callers.
- (fixed) Session restore no longer rots itself through cycles.
  A tab spawned by restore as `claude --resume <sid>` had its
  `info.command` captured verbatim, then save serialized that
  string back into the session JSON. When the resolver returned
  None for `claude_session_id` on a subsequent save (no fresh
  JSONL — wedged conversation, etc.), the next restore fell back
  to the polluted `command` and ran `claude --resume <stale-sid>`
  forever. Save now strips `--resume <token>` from `command`
  when it's a `claude` invocation; restore strips defensively so
  pre-1.17.2 session files heal automatically.
- (fixed) Claude resume crash now prompts to start a fresh
  session. Claude has a regression on the resume path where an
  unhandled `g9H is not a function` wedges React but bun keeps
  the process alive — `is_closed()` never fires, so the prior
  exit-only detection missed it. spyc now also scans the pane's
  recent scrollback for `/$bunfs/root/`, `is not a function`,
  or `Error: sandbox required but unavailable` after a 3s grace
  period, and on detection asks
  "claude crash detected — start fresh and recover with /resume?
  [Y/n]". y/Y/Enter spawns a fresh `claude`; anything else
  closes the tab and the dump is off-screen.
- (superseded) Session restore now recovers from a failed
  `claude --resume`. (Replaced by the prompt-based flow above
  in 1.17.1.)
- (fixed) Claude session resume saved ghost UUIDs in the
  last-ditch fallback. `find_claude_session` reads
  `~/.claude/sessions/<pid>.json`, which Claude writes at
  startup before the JSONL exists; quitting spyc before the
  first turn produced a saved ID with nothing on disk →
  "No conversation found with session ID …" on `spyc -r`.
  `resolve_claude_resume_target` now applies a final
  `claude_jsonl_exists` guard regardless of branch — if the
  file isn't there, save no ID and let restore open a fresh
  `claude`. Also checks the canonical cwd to handle macOS
  `/var` → `/private/var` symlinks.
- (fixed) pane divider now shows the *live* cwd of the active
  subprocess (polled via `/proc/<pid>/cwd` on Linux, `lsof` on
  macOS, 1s cache). Drifted-from-spawn paths get a `↪` marker so
  a wandering bash tab is obvious. Caveat: Claude's process cwd
  never moves (each Bash call is a fresh subprocess), so this is
  a read on real cwd drift, not on Claude's internal confusion —
  for that, see the new shell-continuity note in AGENTS.md.
- (fixed) `g d` now includes untracked / new files. Previously,
  cursor on a `?`-flagged file gave empty diff output. spyc now
  also runs `git ls-files --others --exclude-standard` and
  synthesizes an "added" diff per untracked file via
  `git diff --no-index /dev/null <file>`.
- (fixed) `g b` — git blame on the cursor file. Single-file by
  design (selection ignored). Flashes a clear error if the cursor
  is on a directory.
- (fixed) MCP takeover now prompts before clobbering another live
  spyc instance: `PID N already owns MCP here. Take over? [Y/n]`
  on stderr before the TUI starts. Default Y; "n" leaves the old
  instance in charge and starts the new spyc without MCP. Non-tty
  stdin (CI / scripts) auto-takes-over.
- (fixed) Claude session resume followup: v1.11.2's banner-based
  ID was sometimes a session Claude never persisted (user /clear'd
  or /resume'd before exit). Now we verify the JSONL exists; if
  not, fall back to the most-recently-modified JSONL in the
  project slug (what `claude --resume`'s no-arg picker uses).
- (fixed) `! sudo …` (and ssh / gpg / passwd) no longer bleed
  "Password:" / "Sorry, try again." over the file list and pager.
  The captured child now runs under a slave PTY, so `/dev/tty`
  resolves to the slave and prompt bytes flow into the pager via
  the master. Typed keys are forwarded to the child while the
  capture is live, so passwords can actually be entered. ^C sends
  SIGINT through the tty (cancels the prompt cleanly); ^\\ hard-
  kills if the child detaches from the tty.
- (fixed) home directory now shortens to `~` in displayed paths
  (status bar, I overlay, :project display, exit summary). Match
  is anchored at directory boundaries.
- (fixed) Claude session resume intermittently failed with "No
  conversation found with session ID …" when the same session
  resumed fine via `claude --resume` by hand. Old resolver picked
  IDs out of `~/.claude/sessions/*.json` (PID-scoped index of
  running processes) which goes stale after /compact rotates the
  session ID. Now we read the `claude --resume <token>` banner
  Claude prints on exit straight from the pane scrollback — works
  for UUID and named sessions alike.
- (fixed) help pager multi-column layout: descriptions no longer clip at
  the column edge (per-row wrap with indented continuations); sections
  stay together across columns (no more orphan "Pane path references"
  at the bottom of col 0); the 2-col / 1-col choice is based on actual
  body width and re-decides on window resize. j/k scrolls both columns
  in lockstep against a static content partition, so columns don't
  reshuffle as you scroll; G / Bot / percentage indicator all agree
  against `longest_chunk_len - viewport`.
- (fixed) PROJECT_HOME concept added. Sticky per-session project root,
  auto-set on startup if cwd/.git exists. Bindings: `gh` jump, `gP` set.
  Command: `:project [.|<path>|clear]`. New panes default their cwd to
  PROJECT_HOME when set. Persists through session restore.
- (fixed) PROJECT_HOME basename shown in the top bar next to the pepper
  logo. Status bar also gained SESSION_NAME and dropped user@host (which
  moved to `gU`/`:whoami` and the `I` info overlay).
- (fixed) Named sessions — spice-themed display names like
  `SAFFRON_CUMIN`, generated on session creation, shown in the top bar
  (all caps) and as the primary column in the `-r` session picker.
  Rename with `:name <NEW>`.
- (fixed) start_dir (target of `) is now editable at runtime via `gS`
  and `:startdir` — previously only settable at spyc launch or on
  session restore.
- (fixed) / search in help pager disrupted the display — the search
  bar stole a viewport row which broke multi-column scroll. Now always
  reserves a dedicated search row in multi-column views so the
  viewport height stays constant. Search works in help.
- (fixed) yank full pane scrollback: `ya` copies up to 10K lines of
  scrollback + visible screen to clipboard (vs `yp` which only gets visible)
- (fixed) quit now warns about running processes: "2 running processes —
  press again to quit". Still double-press to confirm, but the flash
  message tells you what you'd be killing.
- (fixed) restart pane process: ^a R closes the active tab and respawns it
  with the same command and working directory
- (fixed) O creates a new file: prompts for filename, touches it, opens
  $EDITOR. Supports paths with subdirs (creates parents). Tab completes.
- (fixed) CLI switches like -rd didn't work — replaced hand-rolled arg parsing
  with clap derive. Combined short flags, auto-generated help, proper errors.
- (fixed) J to ~/D<tab> now shows matches for remote directories instead of
  wrongly filtering the current listing. Also added frecency-based path
  ranking — J prompt learns your most-visited dirs and suggests them on Tab.
- (fixed) mouse text selection broken by EnableMouseCapture — replaced with
  DEC 1007 alternate scroll mode, which prevents scrollback interference
  while keeping normal text selection intact
- (fixed) Tab completion for prompts (J jump, ! shell, / search, etc.)
  with filesystem path completion, double-Tab to show match list, and
  search Tab filters the listing like =PREFIX*.
- (fixed) cw stops at word end (vim convention), dw still deletes
  through trailing whitespace. word_end_exclusive + next_word_start_delete.
- (fixed) paste auto-focuses the pane — no longer surprising that text
  goes to Claude but focus stays on spyc.
- (fixed) human-friendly timer: "18m 59s" instead of "1139s".
- (fixed) pane exit status shows in tab label: "zsh [exited 0]".
- (fixed) ESC cursor reset was PEBKAC — user was hitting backtick.
- (fixed) task completion exit code already shown in pager title.
- (fixed) performance refactor: idle CPU dropped from ~12.5% to near-vim
  levels (~2.5%). Root cause was context file writes triggering
  file-watcher refresh cycles. Also added DEC 2026 synchronized output,
  build_rows/grid caching, and active-tab-only draw.
- (fixed) ^a is now the pane prefix (screen-style). ^w still works as alias.
  Bindings: ^a n/] next, ^a p/[ prev, ^a c new, ^a K/x close, ^a r rename.
- (fixed) cursor blink removed — was causing phantom redraws.
- (fixed) y prefix commands: yy yank, yp yank pane output, yP yank last prompt.
- (fixed) focus notice now uses product naming — "focus: spyc" / "focus: claude"
  (uses active tab label).
- (fixed) exit now prints session summary to stdout with cwd, tab count,
  Claude session name, and restore hint.
- (fixed) pager: `l` toggles line numbers (on by default), `w` toggles
  whitespace markers. Previously `l` controlled both.
- (fixed) we should show a count of "hidden" files due to filters
- (fixed) ESC to leave inventory
- (fixed) inventory persistent — now a file-backed cache with graveyard
- (fixed) hide the mouse pointer when not moving
- (fixed) yank / put — y yanks to cache, p puts to cwd, Y untakes
- (fixed) recovering a session knows the cwd but doesn't set that as the cwd
  from the start
- (fixed) when in a subdir of a git repo the watcher doesn't seem to work to
  monitor changes - does our .git watch work?
- (fixed) git status colors not updating after commits/checkouts — the
  500ms debounce was dropping .git/index events because needs_refresh
  was a local variable reset each loop iteration. Now persists as
  pending_refresh across iterations.
- (fixed) when in the pager and searching or yanking, the status seems to get
  reported to the main spyc pane
- (fixed) `` ` `` should return to the spyc resumed session home and not the
  directory where the user happened to open spyc from — restore_session
  now sets start_dir to the session cwd
- (fixed) marks already persist via ~/.local/state/spyc/marks.toml
- (fixed) g-f/g-F now scans last 200 lines of scrollback (not just visible
  viewport) so paths in large diffs are still found
- (fixed) V opens $EDITOR in top overlay — Claude pane stays visible below.
  Version display moved to gV and :version.
- (fixed) pane tab auto-closes too fast when child exits — error messages
  flash and vanish before you can read them. Now tabs stay open with
  [exited] label; any keypress dismisses.

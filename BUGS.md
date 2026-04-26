### SMALL ###
- q should be reserved for a future implementation of "quick" vim style macros
  / recordings; this should also be on the roadmap
- screen should flash if I'm doing something that hits a wall - e.g. j at the
  top of a directory (the ~ in the status is not enough)
- graveyard should include files that have been removed with R
- we should be able to send control signals to running processes e.g. ^t
- you can get into a weird history loop where commands are mixed with !shell
  comamands and you'll just get "unknown command" - we should preserve a unified
  history but it should preserve shell vs. spyc commands
- cw didn't seem to be worked as expected in ! (? need to confirm - may have
  been using an old version); maybe we should put a build commit hash in the
  top right?
- there should be a short cut to help jump to files affected by git status

### BIGGER ###
- ability to background running tasks and notify when exited or updates have
  happened
- would like to be able to reorder tabs
- directories should persist masking setting / we should be able to enable
  disable masks and have an editable list of them
- yanking from the pane should support # so that you can yank the last 150
  lines, etc.
- ^v should change focus and paste to the lower pane (image paste for Claude)

### MAYBE ###
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
  for that, see the new shell-continuity note in CLAUDE.md.
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

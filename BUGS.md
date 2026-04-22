### MAYBE ###
- yr (yank recursive) — yank directories into inventory; would need
  recursive put and inventory UI changes
- yc (yank chmod) — preserve permissions through yank/put cycle
- count prefix doesn't work with vi line editor operators (e.g. 3cw,
  2dw) — the count is consumed by the resolver but the line editor
  doesn't read it. Low priority for a single-line editor.

### TBD ###
- Claude PTY can get messed up — scrollback accumulates rendering artifacts
  from Claude CLI's progress bars, spinners, and cursor repositioning. ^L
  redraws the visible screen but can't fix corrupted scrollback. Solution t.b.d.
- would like to be able to reorder tabs
- (fixed) restart pane process: ^a R closes the active tab and respawns it
  with the same command and working directory
- we need a better visual indicator that we're in visual mode in the bottom pane
- graveyard should include files that have been removed with R
- screen should flash if I'm doing something that hits a wall - e.g. j at the
  top of a directory
- directories should persist masking setting / we should be able to enable
  disable masks and have an editable list of them
- you can get into a weird history loop where commands are mixed with !shell
  comamands and you'll just get "unknown command" - we should preserve a unified
  history but it should preserve shell vs. spyc commands
- ability to background running tasks and notify when done
- some shortcut to set my homedir to current directory (e.g. for
  backtick-backtick to work)
- (fixed) yank full pane scrollback: `ya` copies up to 10K lines of
  scrollback + visible screen to clipboard (vs `yp` which only gets visible)
- if claude changes its working directory - that's not reflected in the
  terminal status line - can we monitor the cwd?
- (fixed) quit now warns about running processes: "2 running processes —
  press again to quit". Still double-press to confirm, but the flash
  message tells you what you'd be killing.
- claude cli should always be pinned to bottom of the terminal - it seems to get
  scrolled halfway up sometimes
- would be nice to add a "are you sure you want to interrupt?" protection with
  Claude CLI procs
- there should be a short cut to help jump to files affected by git status
- interactive git status browser to jump to file
- (fixed) O creates a new file: prompts for filename, touches it, opens
  $EDITOR. Supports paths with subdirs (creates parents). Tab completes.

### FIXED ###
- support for named sessions
- (fixed) CLI switches like -rd didn't work — replaced hand-rolled arg parsing
  with clap derive. Combined short flags, auto-generated help, proper errors.
- we should be able to send control signals to running processes e.g. ^t
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

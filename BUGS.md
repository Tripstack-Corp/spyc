### MAYBE ###
- yr (yank recursive) — yank directories into inventory; would need
  recursive put and inventory UI changes
- yc (yank chmod) — preserve permissions through yank/put cycle

### TBD ###
- if claude changes its working directory - that's not reflected in the
  terminal status line - can we monitor the cwd?
- quit should say "are you sure? there are running process(es)" to prevent
  accidentally interrupting real work (on detecting work being done)
- claude cli should always be pinned to bottom of the terminal - it seems to get
  scrolled halfway up sometimes
- would be nice to add a "are you sure you want to interrupt?" protection with
  Claude CLI procs
- there should be a short cut to help jump to files affected by git status
- interactive git status browser to jump to file
- shortcut needed for creating a new file in EDITOR

### FIXED ###
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

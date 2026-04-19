- (fixed) `` should return to the spyc resumed session home and not the
  directory where the user happened to open spyc from — restore_session
  now sets start_dir to the session cwd
- (fixed) marks already persist via ~/.local/state/spyc/marks.toml
- (fixed) g-f/g-F now scans last 200 lines of scrollback (not just visible
  viewport) so paths in large diffs are still found
- (fixed) V opens $EDITOR in top overlay — Claude pane stays visible below.
  Version display moved to gV and :version.
- if claude changes it's working directory - that's not reflected in the
  terminal status line - can we monitor the cwd?
- quit should say "are you sure? there are running process(es)" to prevent
  accidentally interrupting real work (on detecting work being done)
- claude cli should always be pinned to bottom of the terminal - it seems to get
  scrolled halfway up sometimes
- would be nice to add a "are you sure you want to interrupt?" protection with
  Claude CLI procs
- (fixed) pane tab auto-closes too fast when child exits — error messages
  flash and vanish before you can read them. Now tabs stay open with
  [exited] label; any keypress dismisses.
- pager l should be for line numbers and we should pick something else to show
  newline chars; line numbers should default to being on
- there should be a short cut to help jump to files affected by git status
- support yp (yank prompt)
- on exit should output that the session(s) were persisted with a few details to
  give the user confidence that all is well ... e.g. spyrc & claude
- interactive git status browser to jump to file
- the "focus:" notice doesn't use our product naming
- shortcut needed for creating a new file in EDITOR
- claude cli seems to need ^l sent to it once in awhile ... we should probably
  do that
- (fixed) we should show a count of "hidden" files due to filters
- (fixed) ESC to leave inventory
- (fixed) inventory persistent — now a file-backed cache with graveyard
- (fixed )hide the mouse pointer when not moving
- (fixed) yank / put — y yanks to cache, p puts to cwd, Y untakes
- (fixed )recovering a session knows the cwd but doesn't set that as the cwd
  from the start
- (fixed )when in a subdir of a git repo the watcher doesn't seem to work to
  monitor changes - does our .git watch work?
- (leave for now) we should get rid of the cursor blinking stuff - it's wonky
- (fixed? maybe through all of the refactoring work) when in the pager and
  searching or yanking, the status seems to get reported to the main spyc pane

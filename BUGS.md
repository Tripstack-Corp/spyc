- fix yank / put - should be able to drop inventory to current directory
  (without overwriting)
- recovering a session knows the cwd but doesn't set that as the cwd from the
  start
- shortcut needed for creating a new file in EDITOR
- quit should say "are you sure? there are running process(es)" to prevent
  accidentally interrupting real work
- claude cli seems to need ^l sent to it once in awhile ... we should probably
  do that
- when in a subdir of a git repo the watcher doesn't seem to work to monitor
  changes - does our .git watch work?
- when in the pager and searching or yanking, the status seems to get reported
  to the main spyc pane
- we should get rid of the cursor blinking stuff - it's wonky
- claude cli should always be pineed to bottom of the terminal - it seems to get
  scrolled halfway up sometimes
- if claude changes it's working directory - that's not reflected in the
  terminal status line - can we monitor the cwd?
- would be nice to add a "are you sure you want to interrupt?" protection with
  Claude CLI procs
- V should open EDITOR in the spyrc only
- marks should persist
- (fixed) pane tab auto-closes too fast when child exits — error messages
  flash and vanish before you can read them. Now tabs stay open with
  [exited] label; any keypress dismisses.
- g-f/g-F would be more useful if it was not just jump to visible but most
  recently displayed - large diffs tend to push paths out of view quickly

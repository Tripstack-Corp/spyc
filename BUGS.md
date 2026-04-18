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

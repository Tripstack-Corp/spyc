# cspy roadmap

## Planned

### Context piping (M10)

Send file selections from the file manager directly into the Claude pane
as context — bridge the gap between browsing and prompting.

- Pipe current picks or inventory to the active pane tab as file paths
  or inline content (`@file`-style injection)
- `^W p` sends picks, `^W i` sends inventory items
- Support both "here are the paths" and "here are the contents" modes
- Natural fit: we're already a file manager with multi-select

### Git worktree integration (M11)

First-class worktree management from the file list — create, switch,
and delete worktrees without leaving cspy.

- `W n` create a new worktree (prompt for branch name)
- `W l` list worktrees with quick-switch
- `W d` delete a worktree (with confirmation)
- Status bar shows current worktree/branch
- Each worktree can have its own pane tabs running independent Claude
  sessions — parallel workstreams without .gitignore hacks

### Diff view in pager (M12)

Unified and side-by-side diff rendering in the pager, building on
existing ANSI color and line number support.

- `d` on a modified file (git-tracked) opens a diff view
- Unified diff with `+`/`-` line highlighting
- Side-by-side mode toggle (if terminal is wide enough)
- Search works across diff content (existing `/` infrastructure)

### Session forking

In multi-tab pane, duplicate a tab to branch a conversation — like
git branching but for Claude sessions.

- `^W f` forks the current tab: new pty with scrollback contents
  pasted in, so the new Claude instance has the prior context
- Useful for "let me try a different approach without losing this one"

### Demo mode

A guided walkthrough mode that showcases cspy's features — useful for
onboarding new users or recording screencasts. Details TBD.

### Additional Ideas
- 'v' in the pager should open the buffer in EDITOR
- session state save and recall (e.g. automatically resume claude sessions and tab state on restart / save state, etc. - does claude support named sessions or another indicator?)
- Mouse support: click to change pane focus, click tab indicators to switch tabs, click file list entries to select. Must coexist with terminal native text selection (disable mouse capture when not needed, or use modifier-key passthrough)

## Done (recent)

- **Multi-tab pane (M9)** — multiple independent pty tabs with `^W n`
  new, `^W x` close, `^W 1`..`^W 9` switch, `^W [`/`^W ]` prev/next
- Tab rename (`^W r`), activity indicators (`+`) on background tabs
- Powerline-style status bar with git branch + dirty flag
- Pager full-width rendering, yank to clipboard
- ESC in vi-normal mode cancels prompt (new-tab flow fix)
- Removed mouse capture (coexists with terminal text selection)
- Bracketed paste forwarding to pane — multi-line paste delivered as
  a single block to Claude CLI instead of line-by-line
- Pager line wrapping — long lines wrap instead of clipping
- Pane scroll mode (`^W v`) — browse 10K-line scrollback without
  interrupting the child process; save with `s`
- One-shot repaint strategy (`needs_full_repaint` flag, `^L` manual
  redraw) replacing per-frame `terminal.clear()`
- Makefile: build, release, cross-compile, install, deploy
- Pager enhancements: line numbers, save output, page-back, `[V]` tag
- Vi-editable shell prompt with persistent history
- Navigation: `''` jump-back, backtick jump to start dir
- Shell modes: `!` captured, `;` foreground
- Hex-dump view for binary files
- Embedded pty pane (M8)
- `.cspyrc.toml` config, keymap DSL, live reload

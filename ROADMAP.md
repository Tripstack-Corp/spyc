# cspy roadmap

## Planned

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
- **`:` command line** — vim-style command mode (`:Wl`, `:set`, `:q`, etc.)
  for discoverable, composable commands. Currently multi-key sequences
  (W l, ^W n, m{a-z}) show a pending indicator in the prompt line, but a
  full `:` command mode would be more powerful and self-documenting.
- **Streaming pager for `!` commands** — open the pager immediately when
  a `!` command starts and stream output live as it arrives, instead of
  waiting for the command to finish. Useful for long-running commands
  (brew install, cargo build, etc.) where progress feedback matters.
- ~~'v' in the pager should open the buffer in EDITOR~~ Done
- Session state save and recall (e.g. automatically resume claude sessions and tab state on restart / save state, etc.)
- Mouse support: click to change pane focus, click tab indicators to switch tabs, click file list entries to select. Must coexist with terminal native text selection

## Done (recent)

- **Diff view in pager (M12)** — `g d` shows unstaged diff, `g D` shows
  staged diff. Runs `git diff --color=always` and pipes through the
  existing ANSI pager. Works on cursor file or picks selection.
- **Git worktree integration (M11)** — `W l` list/switch worktrees,
  `W n` create new worktree (prompt for branch), `W d` delete worktree.
  Status bar already shows branch per worktree. Pane tabs are independent.
- **Context piping (M10)** — `^W p` pipes file contents of selection,
  `^W i` pipes inventory contents to pane as bracketed paste with
  `[file: path]` headers. `^W s` remains for paths only.
- Help overlay uses the pager (scrollable, searchable)
- Pager multi-column layout with position indicator (Top/Bot/NN%)
- Focus indicators: dim list cursor when pane focused, blinking pane
  cursor when focused, static block when not
- Alt+Enter sends newline to pane (Claude CLI multi-line input)
- Vi line editor: operator+motion (`dw`, `cw`, `db`, `d$`, `dd`, `cc`)
- Backspace on empty no longer cancels vi-mode prompts
- Force full repaint on pager close (fixes ghost character artifacts)
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

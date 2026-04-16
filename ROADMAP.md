# cspy roadmap

## Next up

### Multi-tab pane (M9)

The lower split pane currently hosts a single subprocess. The next major
feature adds tabbed panes so multiple Claude instances (or any mix of
subprocesses) can run simultaneously.

- Tab bar across the top of the pane area with switchable tabs
- Each tab is an independent pty with its own scrollback
- Tab headers show the running command or a user-assigned label
- **Attention indicator** — when a background tab's subprocess produces
  new output (or exits), its tab header gets a visual flag (bold,
  color change, or badge) so you know it needs attention
- Keybindings: `^W 1`..`^W 9` to switch tabs, `^W n` to open a new
  tab, `^W x` to close the current tab
- Scroll mode (`^W v`) applies to the focused tab

## Planned

### Demo mode

A guided walkthrough mode that showcases cspy's features — useful for
onboarding new users or recording screencasts. Details TBD.

## Done (recent)

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

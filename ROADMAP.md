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
- **`:` command extensions** — expand `:` with more commands: `:set`,
  `:sort`, `:cd`, `:marks`, etc. Currently supports `:limit`, `:!`,
  `:!!`, `:;`, `:q`.
- **Split stdout/stderr pager** — option to show stdout and stderr in
  separate horizontal panes within the pager, for commands where you
  want to see errors separately from normal output
- **Buffer history** — keep a stack of recent pager buffers (command
  output, file views, diffs) so you can cycle back through them without
  re-running commands. Like `:bprev`/`:bnext` in vim.
- Mouse support: click to change pane focus, click tab indicators to switch tabs, click file list entries to select. Must coexist with terminal native text selection

### Ideas from fbi-improved (fim)

**Pager / navigation:**
- **Page scroll overlap** — when paging up/down, keep 2-3 lines from the
  previous page visible for reading continuity (`_scroll_skip_page_fraction`)
- **Auto-scroll reading mode** — continuous scroll at configurable speed
  for hands-free reading. `while(1){scroll;sleep 2}` style.
- **Jump-back in pager** — `''` to return to where you were before the
  last search/jump (pager already has it for the file list)

**Automation / scripting:**
- **Autocommands** — user-configurable hooks triggered per file type:
  `autocmd "*.md" "preview"`, `autocmd "*.log" "tail_mode"`. Could live
  in `.cspyrc.toml` and replace hardcoded special cases.
- **Macro recording** — vim-style `qa`...`q`...`@a`. Record a sequence
  of actions (rename, move, tag) and replay on demand.
- **Startup/exit command flags** — `cspy -c "sort mtime"` to execute
  commands at startup, `-F` for exit hooks.

**Shell composability:**
- **Stdout on exit** — output picks/inventory paths to stdout when
  quitting, making cspy composable: `cspy | xargs rm`, `cspy | tar czf`.
- **`--dump-default-config`** — output the full default `.cspyrc.toml` so
  users have a complete starting point for customization.

**Status bar / display:**
- **Conditional status bar expandos** — `%?git?%branch?` shows git info
  only when in a repo. Format-string-based status bar with `%i/%l`,
  `%n`, `%F` etc.

**File management:**
- **Per-file tags/metadata** — key-value pairs attached to files that
  persist within a session, usable in filters and autocommands.
- **Background directory loading** — async listing for large trees so
  the UI stays responsive.

## Done (recent)

- **`:` command line** — vim-style command prompt with `:limit`, `:!cmd`,
  `:!!`, `:;cmd`, `:q`. Vi line editor with history.
- **`=` limit filter** — temporary glob filtering (`=*.rs`, `=!` for
  picks only, `=` clears). Status bar indicator, auto-clears on chdir.
- **`!?` history editor** — vi-editable popup with `/search`, `n`/`N`
  match navigation, `:N` jump, `G`/`gg`, `Ctrl+D` delete, instant
  trigger from `!` prompt, deduped history on load.
- **Numeric prefix display** — typing `3j` shows "3" in the prompt area.
- **`:N` jump-to-line** in pager and history editor.
- **Pager repaint fix** — force full repaint on pager open when pane is
  active, preventing stale PTY cells from bleeding through.
- Syntax highlighting in pager via syntect (base16-eighties.dark theme,
  hundreds of languages, auto-detected from file extension)
- Streaming pager for `!` commands — output streams live with hourglass
  timer, stderr merged into stdout, auto-scroll to bottom
- Session save/restore (`--resume`) — auto-save on quit, picker UI with
  j/k navigation, human-readable timestamps, dedup by cwd+tabs
- Separate pane command history with move-to-end dedup; `j`/`k` in
  normal mode cycle history without leaving normal mode
- Git file status colors in the listing (modified, added, untracked,
  deleted, renamed, conflicted) — refreshes on chdir and fs events
- Cursor returns to previous directory on climb (`u`/`-`)
- h/l at column edges clamp instead of wrapping
- Terminal resize handler: pty tabs resize immediately on `SIGWINCH`
- Pager `v` opens buffer in `$EDITOR`, returns to pager on quit
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

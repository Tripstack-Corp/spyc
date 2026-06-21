# spyc — features

spyc is a vi-keyboard-driven terminal file manager written in Rust. It's
built for developers who live in the terminal and want a fast, modal
interface for navigating files, running commands, and — critically —
working alongside AI coding assistants like Claude Code.

Inspired by SideFX's `spy` (a file manager from the Houdini VFX
ecosystem), spyc brings that same "always-open workspace" philosophy to
modern terminal workflows. The split-pane design lets you browse your
project in the top half while Claude runs in the bottom half, so file
context and AI conversation stay in the same window.

## Vi-style navigation

Everything is keyboard-driven with vi motions as the foundation.

- **h/j/k/l** movement across a multi-column file listing (h/l clamp at edges, no wrap)
- **gg / G** to jump to top or bottom
- **^B / ^F** page up and down
- **Count prefix** — `5j`, `10k`, etc. with visual display in the prompt area
- **/ search** case-insensitive substring match by default (so `/env`
  finds `.env`, `.envrc`, *and* `environment.toml`); switches to glob
  the moment the query contains `*`, `?`, or `[` — `/env*` re-anchors
  at the start when you want that
- **n / N** to repeat search forward / backward

## Directory browsing

- **d / Enter** descend into a directory, or view a text file in the pager
- **e / v** descend into a directory, or open a file in `$EDITOR` (suspends TUI)
- **V** open `$EDITOR` in the top pane — bottom pane stays visible
- **D** open the cursor file in spyc's in-app pager mounted in the
  top-pane slot — bottom pane stays visible. Same loader as `Enter`
  / `d` (syntax highlighting, markdown render, hex dump for
  binaries, truncation banner for big files) but mounted in the
  top slot instead of as a centered overlay. Workflow: `D` on a
  doc, `^a-j` into the bottom pane to do work, `^a-k` back to
  scroll the doc. Files past 5 MB fall back to `$PAGER` as a top
  overlay (streaming from disk wins for multi-GB logs)
- **u / -** climb to the parent directory (cursor returns to the dir you came from)
- **~ / Home** jump to home (`H` is the harpoon prefix — see Harpoon)
- **J** jump to any path (with `~` and `$VAR` expansion, frecency-ranked
  suggestions from visit history)
- **F** project-wide fuzzy filename finder. Walks the focused
  commander's worktree root (its repo root, else `PROJECT_HOME`,
  else the current dir) honoring `.gitignore`, ranks
  candidates against typed input via nucleo-matcher (basename hits
  outrank parent-dir hits, fzf-style). Up/Down move selection,
  Enter chdirs to the matched file's parent and places the cursor
  on it; Esc cancels. No persistent index — walks lazily on open.
- Multi-column layout that adapts to terminal width
- Color-coded entries: directories, executables, symlinks, files
- **Git status markers** — two-character left-gutter pair mirroring
  `git status -s`: column 0 = staged side, column 1 = unstaged side.
  `M ` ready-to-commit, ` M` working-tree-only, `MM` partially
  staged + further edits, ` ?` untracked, `R~` staged rename +
  unstaged tweaks, `!!` conflicted. Each char colored independently
  (modified=amber, added/untracked=green, deleted=red, renamed=
  lavender, conflicted=bold red). Directories containing changes
  are tinted too.
- **`]g` / `[g`** jump the cursor to the next / previous file or dir
  with a non-clean git status. Wraps end-to-end so the chord is
  hold-to-cycle. Flashes "no git changes in this directory" when
  there's nothing to land on.

## Git views — diff / show / blame

In-house, gix-backed git pager views (v1.56) — built in-process (no
`git` subprocess), syntax-highlighted, with word-level intra-line
change highlighting on modified lines.

- **gd** — diff vs HEAD for the selection (cursor file or picks):
  staged + unstaged + new files, everything different from HEAD
- **gD** — staged-only diff (`git diff --cached`): what would commit
- **gb** — blame the cursor file (selection ignored)
- **|** inside a diff / show view toggles **side-by-side ⇄ unified**
  layout. Side-by-side is the default, falling back to unified when
  the viewport is too narrow; a no-op for blame
- All pager keys work: `/` search, `:N` jump, visual yank, `gf`/`gF`
  path jumps, buffer history

Quick Select's uppercase **Git SHA** label opens the same in-house
`show` view for that commit (see "Quick Select" below).

## Picks and inventory

Two levels of selection for flexible file management.

**Picks** are per-directory multi-select:
- **t** toggle pick on the cursor entry
- **T** pick by glob pattern
- **^T** pick all / clear all

**Inventory** is a file cache — yanked files are copied to a local cache
(`~/.local/state/spyc/inventory/`), persisted across sessions:
- **yy** yank file(s) into inventory cache (regular files only)
- **yf** yank the cursor file's absolute path (or all picks,
  newline-separated) to the system clipboard — for one-off shell
  use without a pane (`git restore $(pbpaste)` etc.)
- **yp** yank visible pane output to the system clipboard
- **yP** yank the last prompt you typed into the pane to the clipboard
- **ya** yank the full pane scrollback (up to 10K lines) to the clipboard

  *Clipboard backend:* macOS uses built-in `pbcopy`; Linux uses
  `wl-copy` (Wayland) or `xclip` / `xsel` (X11) — see INSTALL.md
  for setup. With no helper on PATH, yanks flash an actionable
  install hint.

- **Y** remove cursor file from inventory
- **p** put all inventory files to the current directory
- **i** toggle the inventory view (replaces the file listing)
- **z** clear inventory (moves files to graveyard)

Inside the **inventory view** (`i`):
- **t / Space** tag items for partial put
- **p** put tagged items (or all) to cwd — removes from inventory
- **x / d** remove item to graveyard
- **ESC** return to directory view

Picks and inventory feed into file operations and shell commands — they
become `%` in shell expansion.

## File operations

- **c** copy selection to a destination
- **M** move selection to a destination
- **R** remove selection (with confirmation). The prompt counts
  files inside any selected directory and surfaces the total
  ("remove DIR (recursive, N files) + M file(s)?") so the blast
  radius is visible before you press `y`. Removed items go to the
  **graveyard** (see below) — recover with `gy` or `:undo`.
- **+** create a new directory
- **L** long listing -- aligned table with inode, mode, octal,
  links, owner, group, size, bytes, blocks, mtime/atime/ctime/birth,
  name (symlinks as `name -> target`). Pager height fits to content.
- **f** run `file(1)` on the selection
- **^X** chmod +x

## Split pane with multi-tab pty

The bottom half of the terminal hosts a fully independent pty — by
default, it runs `claude` (the Claude Code CLI). This is the core of
spyc's workflow: browse files above, talk to Claude below.

- **^\\ / F10** toggle the pane open/closed
- **F9** open pane with `claude --resume`
- **^a j / ^a k** switch focus between the file list and the pane
  (`^w` also works as an alias for `^a`)
- **^a s** send the current selection (file paths) to the pane as stdin
- **^a P** pipe file contents of the selection to the pane
- **^a i** pipe inventory file contents to the pane
- **^a + / ^a -** grow or shrink the pane
- **^a z** zoom the **active** region (toggle fullscreen). Pane focused →
  the pane fills the screen (list collapses to 0 rows, `[ZOOM]` on the
  divider) and a single spyc status line stays at the top (status, or
  flash / chord-arming / prompt when active). List focused → the list fills
  the screen with the pane **tab bar kept at the bottom** (`[ZOOM]` in the
  top status bar); from there `^a <n>` fullscreens that tab, and creating a
  new pane reveals the split. Focus stays where it is, `^a j` / `^a k` are
  inert while zoomed (only `^a z` exits), and the prior split is restored on
  un-zoom.
- **^a |** vertical (left/right) split of the file area. Opens (50/50) with a
  **preview of the file under the cursor** (markdown rendered) in the right
  column. Press it again **on a different file** to swap the preview to that
  file, keeping the layout; **on the same file** it cycles the shape:
  *top-only* (splits just the list; pane stays full-width below) → *full-height*
  (divider runs the whole height; pane confined under the left column) → off. A
  directory isn't previewable (it warns). `^a a` / `^a h` focus the left pane
  (a), `^a b` / `^a l` the right (b); `^a + / ^a -` resize the split width when
  a column is focused. Opinionated: exactly two file panes, labelled a/b
  (numbers stay for PTY tabs). The preview **live-reloads**: edit the file
  (in spyc's editor, an agent pane, or any external editor) and the rendered
  markdown updates on save — the re-render runs off-thread, preserving your
  scroll position, and re-wraps when you resize the column.
- **^s n** open a **second full file-commander** in the right column
  (`b`) — a complete browser, not just a preview: its own cwd, listing,
  picks, filter, sort, git markers, and harpoon. **^s x** (or **^d**,
  which closes the commander when one is open, else quits) closes it.
  `^a a`/`^a h` focus the left commander (a), `^a b`/`^a l` the right
  (b). Everything follows the focused column: navigation, `V`/`D`
  overlays, file ops (`O`/`+`/`W`), `!`/`;`/`$` shell cwd, the MCP
  context, and the per-column tools — **grep `F`**, **find**, **MCP
  search**, and **harpoon** all scope to the focused column's *worktree
  root* (so `b` in a separate worktree searches its own tree, with its
  own pinned harpoon list). **g w** jumps the focused column to its
  worktree / repo root; `PROJECT_HOME` (jumped by `g h`) stays the
  overall anchor. Git markers in each column refresh on filesystem
  events independently. (The second-commander chord is `^s`, not `^z`:
  `^z` to a shell in the bottom pane is SIGTSTP — it'd background a
  running job. `^s` is at worst terminal flow-control XOFF, which spyc's
  raw mode clears; over a flow-control-sensitive SSH/serial link it may
  still be intercepted.)
- **W l** opens a **worktree picker** — pick a git worktree and the
  *focused* column switches to it (focus `b` first to put a worktree
  there). With the MCP tools (`create_worktree` / `open_worktree` /
  `remove_worktree` / `clean_worktree`) an agent can spin up a worktree,
  open it in `b`, work, and tear it down — while `a` stays on the main
  branch.
- **^a v** enter scrollback view — browse up to 10K lines of pane
  history in the **in-app pager** (v1.5). All pager keys work: `/`
  search with `n` / `N`, `:N` jump, `V` visual line, `^v` visual
  block, `y` yanks the selection, `l` toggles line numbers, `w`
  whitespace markers. The pty keeps running off-screen — output
  you miss while reading lands in scrollback for the next view.
  `Esc` snaps back to live.
  - The fundamental limit is that full-screen TUIs do *virtual
    scrolling* inside a fixed grid — old content lives in app
    memory, not the terminal — so even a parallel vt100 parser
    can't recover it. `^a v` on a *non-agent* alt-screen app
    (vim, htop, lazygit) is therefore a dead end: it flashes a
    hint pointing at the app's own history viewer.
  - **Agent transcripts.** Agents that keep an on-disk JSONL
    transcript — Claude Code, codex
    (`~/.codex/sessions/.../rollout-*.jsonl`), and Antigravity
    (`agy`, under `~/.gemini/antigravity-cli/`) — get the actual
    conversation instead of a screen capture: user turns, agent
    replies, and tool calls rendered in the pager, titled
    `(transcript)` rather than `(history)`. Tool calls are
    labelled with their salient argument — `⚙ Bash(Find foo call
    sites)`, `⚙ Edit(src/lib.rs)` — and each result shows a dim
    one-line output preview with a `(+N lines)` count. The
    transcript is
    resolved and read **off-thread** (no UI stall on a big
    session), and **`r`** reloads it — the agent keeps appending
    while you read. If no transcript exists yet (brand-new
    session), spyc flashes a hint.
  - **Alt-screen agents always use the transcript.** When an
    agent with a transcript runs on the alternate screen (e.g.
    Claude Code's full-screen mode), `^a v` auto-engages the
    transcript view unconditionally — there's no usable vt100
    capture to fall back to, so the config gate is bypassed.
  - **Inline panes are config-gated.** When the agent runs inline
    (output scrolls into the main buffer), the verbatim terminal
    capture works, so the transcript view is a per-agent choice:
    `[pane] claude_transcript_scrollback` (off by default —
    inline claude captures fine from the terminal) and
    `[pane] agy_transcript_scrollback` (on by default). Codex
    transcripts are always on — there is no config gate.
- **Ctrl+J** newline in pane (multi-line input for Claude CLI)
- **gf** jump to a file path referenced in pane output; **gF** also
  opens the pager at the referenced line. Scans the last 200 lines of
  output (including scrollback) so paths in large diffs are still found.

### Multi-tab

Multiple tabs, each running an independent pty:

- **^a c** new tab (prompts for command and working directory)
- **^a K / ^a x** close the active tab
- **^a 1..9** switch to tab N
- **^a p / ^a [** prev tab
- **^a n / ^a ]** next tab
- **^a ^a** jump to the last-active tab (screen/tmux "last window")
- **^a r** rename the active tab
- **^a R** restart the active tab — closes it and respawns the same
  command in the same working directory
- Activity indicator (**+**) on background tabs that have new output
- **Default command** for `^a c` resolves in this order:
  `$SPYC_PANE_CMD` env var → `[pane] default_command` in
  `.spycrc.toml` → built-in `"claude"` fallback. Switch your daily
  driver to codex (or anything else) by adding
  `[pane]\ndefault_command = "codex"` to your config.

## In-app pager

A built-in pager for viewing files and command output without leaving
spyc.

- **Syntax highlighting** via syntect — source files are highlighted
  with language-aware coloring (hundreds of languages supported)
- ANSI color preservation — captured command output looks exactly right
- **Streaming output** — `!` commands show output live with an
  hourglass timer, stderr merged so build progress appears in real-time
- **/ search** within pager content, with **n / N** navigation
- **:N** jump to line N
- **l** toggle line numbers (on by default)
- **w** toggle whitespace markers (·, ↲, $)
- **W** toggle line wrap (default on for content; off for picker UIs)
- **m** toggle Markdown rendered ↔ source view (`.md`/`.markdown` only;
  flashes "not a markdown file" otherwise)
- **f** toggle full-width mode vs. centered overlay
- **v** open pager content in `$EDITOR`
- **s** save pager content to a file
- **y** yank pager content to the system clipboard (always operates
  on the *source* — yanking a rendered Markdown view gives you back
  the markdown text, not the styled rendering)
- **V** enter vi-style **visual line mode** to yank a line range:
  the anchor sets at the top visible line, `j` / `k` / `^d` / `^u`
  / `^f` / `^b` / `g` / `G` / `Home` / `End` extend the selection
  (auto-scrolling), the status footer shows `L{lo}-L{hi}` and the
  line count, `y` yanks the inclusive range to the clipboard and
  exits, `Esc` / `V` cancel
- **^v** enter **visual block (columnar) mode** — vi's rectangle.
  `j` / `k` extend rows, `h` / `l` extend columns; `y` yanks the
  rectangle (each row contributes `chars[lo_col..=hi_col]`, rows
  shorter than the range simply contribute fewer chars). `^v`
  toggles back off; `V` while in block drops down to line mode
  (vim parity). Footer reads
  `-- VISUAL BLOCK --  L{lo}-L{hi} C{lo}-C{hi}  ({rows}×{cols})`
- **x** toggle hex-dump view for binary files
- **Markdown viewer** — `.md`/`.markdown` files open in rendered
  mode by default: headings styled, lists with bullets, fenced
  code blocks syntect-highlighted by language, blockquotes with a
  left rule, links shown with the destination URL, inline
  bold/italic/strikethrough preserved. Press `m` to toggle to the
  raw source view (with full syntect highlighting of the
  markdown source itself); press `m` again to flip back. Yank
  and save always emit the source.
- Page-up/down, half-page, and vi-style scrolling

## Shell integration

Three modes of running commands, each for a different use case:

- **!** captured — run a command and stream output into the pager in
  real-time with an hourglass timer. Stderr is merged so build
  progress, errors, and output all appear together. `%` expands to
  the current selection. `^C` interrupts; `^\` hard-kills; `^Z`
  sends the task to the background (reader thread keeps draining
  output into a buffer; resume with `:fg` -- see Background tasks).
- **!!** — repeat the last captured command.
- **!?** — history editor popup. Opens instantly (no Enter needed).
  Also reachable mid-prompt: after `Esc` puts the line editor in
  Normal mode (e.g. you pressed `Esc k` to browse history), `?` (or
  `Space`) opens the same viewer — not just `!?` from a fresh prompt.
  Defaults to Normal mode — `j`/`k`/`G`/`gg` navigate, `/` search
  with `n`/`N` to jump between matches, `:N` jumps to entry N.
  Press `i` to vi-edit the highlighted command in-place.
  `Enter` executes the (possibly edited) command, `Ctrl+D` deletes
  an entry, `Esc`/`q` closes.
- **;** foreground — run an interactive command (top, vim, htop, less)
  in a top-overlay pty that replaces the file listing while the bottom
  pane stays visible. **`^a-j` / `^a-k` flip focus between the overlay
  and the bottom pane** so you can `;less docs/architecture.md`, pop
  down to claude, do work, and pop back up to scroll the doc — all
  without quitting the subprocess. The unfocused side dims; the focus
  flash says `focus: overlay` or `focus: <tab label>`.
- **$** shell — drop into `$SHELL` in the current directory.

All three modes invoke `$SHELL -i -c <cmd>` (interactive flag where
the shell supports it: zsh, bash, fish, ksh) so aliases, functions,
and rc-file PATH entries from `.zshrc` / `.bashrc` work the same way
they do in a regular terminal tab. POSIX `sh` / `dash` get plain
`-c` since they don't read rc files in interactive mode anyway.

The shell prompt uses a vi-mode line editor with persistent history
(shared across sessions), so you get `h/l/w/b/0/$` motion, `x/D/C`
editing, operator+motion (`dw`, `cw`, `db`, `d$`, `dd`, `cc`, etc.),
and `i/a/I/A` mode switching — all within the one-line prompt.
`j`/`k` in normal mode cycle through history without leaving normal
mode. Ctrl+J inserts a newline in the pane (for Claude CLI
multi-line input).

Pane command prompts (`^W n`) have their own dedicated history,
separate from shell commands — so Up/Down shows `claude`, `zsh`,
`bash` instead of mixed shell commands. The follow-up "pane cwd:"
prompt keeps a *separate* history again, so Up/Down there recalls
previously-used working directories without mixing them into the
command list. History is de-duplicated (most recent use moves to the
end).

## Command line

**`:`** opens a vim-style command prompt with vi editing and history:

- **`:cd <path>`** — change directory (`~` and `$VAR` expanded, bare `:cd` goes home)
- **`:sort <mode>`** — sort listing by `name`, `size`, `mtime`, or `ext`
  (persists across chdir); **`gs`** toggles reverse order for the current sort
- **`:marks`** — show all marks in a pager popup
- **`:set key=value`** — runtime settings (e.g. `:set sort=mtime`)
- **`:bprev`** / **`:bnext`** — navigate pager buffer history (also `[b`/`]b` in pager)
- **`:limit <glob>`** — temporary filter (e.g. `:limit *.rs`)
- **`:limit !`** — show only picked files
- **`:limit git`** / **`:limit g`** — show only files in `git status`
- **`:limit h`** / **`:limit harpoon`** — show only harpoon entries
- **`:limit`** — clear filter
- **`:!<cmd>`** — captured shell command (same as `!`)
- **`:!!`** — repeat last captured command
- **`:;<cmd>`** — foreground shell command (same as `;`)
- **`:fg`** / **`:fg N`** — resume a backgrounded task (see Background tasks)
- **`:pause`** / **`:pause N`** — pause a backgrounded task (`SIGSTOP`)
- **`:resume`** / **`:resume N`** — resume a paused task (`SIGCONT`)
- **`:grep <pattern>`** — project-wide content search via embedded
  ripgrep matcher (`grep-regex` + `grep-searcher`). Walks the
  focused commander's worktree root (its repo root, else
  `PROJECT_HOME`, else current dir) honoring `.gitignore`, smart-case
  by default. Results stream into a pager as `path:line:col: text`,
  so `gf`/`gF` jump from a hit to the file in-place. Capped at 5000
  matches; refine the pattern if you hit it. Power users: `! rg foo`
  still works for ripgrep's full flag surface.
- **`:q`** — quit

The `:` prompt shares history with other shell prompts, so Up/Down
cycles through previous commands.

## Background tasks

Long-running captured commands (`!cargo test`, `!find ...`) don't have
to lock you out of spyc.

- **^Z** while a `!` capture pager is open sends the task to the
  background. The reader thread keeps draining output into a per-task
  buffer (head-truncated at 1 MB). Tasks render in the pane divider,
  right-aligned, in a distinct color family from pane tabs:
  `[N+]` running with new output (teal), `[N●]` running quiescent
  (blue), `[N✓]` exited cleanly (green), `[N✗]` non-zero / killed
  (red). When the pane is hidden, falls back to a status-bar
  `bg:N●M✓` suffix.
- **`:fg`** resumes the most-recently-backgrounded task; **`:fg N`**
  targets a specific task id. Still-running tasks come back as a
  streaming pager seeded with everything captured so far; already-
  exited tasks come back as a static pager titled with the final
  exit code and elapsed time.
- **Task viewer (peek mode).** `gB` from the file list opens the
  most-recent task in a peek view. `[t`/`]t` while in any pager
  cycles through bg tasks by id; `:task N` jumps to a specific task.
  While the task is running, the viewer auto-refreshes from the live
  buffer. On close, *exited* tasks are promoted: snapshot pushed to
  buffer history, task dropped from the bg list. Running tasks stay
  put -- you can come back via `gB` / `[t`.
- **`:pause`** / **`:pause N`** sends `SIGSTOP` to the task's
  process group, halting the whole subprocess tree (`make → cc →
  ld` all stop together). **`:resume`** / **`:resume N`** sends
  `SIGCONT`. Useful when switching networks, freeing CPU, or
  pausing an over-eager build to focus on something else. Inside
  the task viewer, **`S`** and **`C`** are the shorthand
  equivalents. Paused tasks render as `[N⏸]` in the divider;
  `:fg` on a paused task auto-resumes before re-attaching.
- A task that completes while in the background fires a flash:
  `task #N: cmd — exit 0 (43s)`.
- **`:task-to-pane`** / **`:task-to-pane N`** promotes a
  backgrounded task to a new pane tab. The pty keeps running
  through the transition; spyc resizes it to the bottom-pane
  geometry, replays the captured buffer through a fresh vt100
  parser so the tab opens with the same content the task viewer
  was showing, and SIGCONT's the child if it was paused. Useful
  when an `!` task you started turns out to need persistent
  attention (a long-running `npm run dev`, a `cargo watch`, a
  `tail -F`) — promote it next to claude instead of shuttling
  through `:fg` / `^z`. Already-exited tasks aren't promoted
  (a dead pty would just immediately tear down the tab); use
  `:fg` for the static-output view in that case. The promoted
  tab inherits the task's TERM (`dumb`, set when the `!` capture
  spawned), so plain shells and SGR-color output work fine but
  alt-screen TUIs won't suddenly start working in the new tab.
- **`:pane-to-task`** is the symmetric inverse — moves the active
  pane tab into the background-task list without killing the
  child. Same `PtyHost` migrates between containers; same child
  PID round-trips. The new task buffer starts empty (vim's `^z`
  parity — visual context isn't recovered through the demote
  boundary). Bring it back via `:fg` or `:task-to-pane`.
- The quit confirmation (`Q`/`^D`) counts backgrounded running tasks
  alongside pane-tab processes.

## Pager buffer history

Closed pager views are saved to a history stack (up to 10). Navigate
with `:bprev`/`:bnext` from the main prompt, `[b`/`]b` (chord)
while in the pager, or **`gp`** to reopen the most-recent closed
buffer in one keystroke. The help overlay is excluded from the
stack. Walking off the end keeps the current pager open with a
flash instead of silently closing. Scroll positions are preserved.

## Quick Select — labeled overlay picker

Borrowed from [WezTerm's Quick Select][wezterm-qs]. Press **`^a u`**
to scan the visible pane for matches (URLs, file paths, git SHAs,
IPv4 addresses, plus any custom regexes from `.spycrc.toml`); each
match is overlaid with a 1- or 2-letter label.

[wezterm-qs]: https://wezterm.org/quickselect.html

- **Lowercase label** — yank the match to the clipboard, exit.
- **Uppercase label** — "open" intent:
  - **URL** → hand to the system handler (`open` on macOS,
    `xdg-open` on Linux)
  - **Path** → cursor-jump in spyc (chdir to parent + place cursor)
  - **Git SHA** → open the commit in the in-house gix-backed
    `show` view (same pager as `gd`; `|` toggles layout)
  - **Custom pattern** with a `url = "..."` template → fill `{}`
    with the match, then `open`/`xdg-open`
  - Other kinds (IPv4, custom without template) → fall back to
    yank with a flash hint
- **`q` / `Esc`** — exit without action

Scroll mode just works: scroll up to a Claude reply, hit `^a u`,
the URLs in *that* reply get labels (the picker scans exactly the
visible viewport at the user's scroll position).

Custom patterns in `.spycrc.toml`:

```toml
[[scan.patterns]]
name = "jira"
regex = '[A-Z]+-\\d+'
url = "https://tripstack.atlassian.net/browse/{}"   # optional
```

Without `url`, uppercase falls back to yank+hint. Bad regexes are
dropped at config load and noted in the debug log; a typo never
prevents spyc from starting.

## Graveyard — soft-delete recovery

Files removed with **R** (and items expelled from inventory) go to
a **graveyard** at `$XDG_STATE_HOME/spyc/graveyard/`. Each entry is
a `<uuid>.json` (metadata) + `<uuid>.tar.zst` (compressed payload)
pair. tar's `HeaderMode::Complete` captures mode bits (executable
flag, etc.), mtime, and best-effort UID/GID; restore preserves all
of them. xattrs / ACLs / macOS resource forks are NOT preserved
(out of scope for v1).

- **`gy`** / **`:graveyard`** — open the graveyard view (newest entries first)
- **`:undo`** — restore the most-recent entry to its original path
- Inside the graveyard view:
  - **`p`** — restore the cursor entry to the current dir (cwd)
  - **`P`** — restore to its original path (refuses to clobber an
    existing file; flash error and you can fall back to `p`)
  - **`dd`** / **`x`** — purge cursor entry to the system trash
    (out of spyc, into Finder / Files / etc.)
  - **`Z`** — purge ALL entries to the system trash (single-key
    confirm)
  - **`Esc`** / **`gy`** — close

When the graveyard exceeds 500 MB at startup, the **oldest entries
cascade to the system trash** (FIFO) until the total falls below
the cap. A flash tells you how many were moved. Net flow:
`R` → graveyard (compressed, undo-able from spyc) → system trash
(uncompressed, browsable from the OS).

## Marks

Vi-style named bookmarks for fast navigation:

- **m{a-z}** set a mark at the current directory + cursor position
- **'{a-z}** jump to a mark
- **''** jump back to the previous directory (like `cd -`)
- **\`** jump to the start directory (editable via `gS` or `:startdir`)

## Harpoon — pinned working set

Inspired by ThePrimeagen's neovim plugin: a small, hand-curated,
**per-project** ordered list of file (or directory) pointers for
muscle-memory navigation. Up to 9 slots. Persisted on disk per
`PROJECT_HOME`, auto-saved on every mutation.

- **Ha** harpoon the cursor file/dir (append; idempotent; capped at 9)
- **Hx** un-harpoon the cursor file/dir
- **H1**..**H9** jump to slot N — chdirs to the slot's parent and
  places the cursor on the file (or chdirs into the directory).
  Spyc lets *you* pick the verb afterwards (Enter, V, ^a s); the
  jump itself is just navigation.
- **Hh** open the harpoon menu — modal overlay where you can:
  - **j/k** move cursor / **g**/**G** first/last
  - **K/J** swap slot up/down (reorder)
  - **dd** delete the slot under the cursor (vim-style: first `d`
    arms, second `d` confirms; any other key cancels)
  - **1**..**9**/**Enter** jump and close
  - **q/Esc** close
- **=h** (or `:limit h`) limits the file list to harpoon entries.
  Ancestor directories are included automatically — if
  `src/foo/bar/hello.c` is harpooned and you're viewing `src/`,
  `foo/` shows up so you can drill in.

Persistence: `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml`
(one file per **worktree**, keyed by an absolute-path hash so two
worktrees with the same basename can't collide). The key is the
focused column's worktree root (else `PROJECT_HOME`), so a second
column in a different worktree keeps its own bookmarks — harpoon
stores absolute paths, so a shared list would jump you into the
wrong worktree's copy. Outside a repo with no `PROJECT_HOME`, the
H-prefix bindings flash a hint and bail.

Note: `H` was previously an alias for "jump to `$HOME`"; it's now
the harpoon chord prefix. The `~` key and the Home key still jump
to `$HOME`, and `gh` jumps to `PROJECT_HOME`.

## Project home & session name

Each spyc run has a **`PROJECT_HOME`** (a sticky project root) and a
**session name** (a spice-themed label like `SAFFRON_CUMIN`). Both are
shown on the top bar and persist across `spyc -r`.

- **Auto-detect** — `PROJECT_HOME` is set to the launch directory
  automatically when that directory contains a `.git` entry. No
  upward walk — the concept is explicit and predictable.
- **Keys** — `gh` jumps to `PROJECT_HOME`; `gP` sets it to the current
  directory; `gS` re-points the start directory (target of `` ` ``).
- **Commands** — `:project` prints; `:project .`, `:project <path>`,
  `:project clear` manage the value. `:startdir` manages start dir.
  `:name <NEW>` renames the session (normalized to
  `[A-Z0-9_]`). `:whoami` / `gU` flashes `user@host`.
- **New pane tabs** default their cwd to `PROJECT_HOME` when set,
  otherwise to the current listing dir.
- **Session names** are generated at session creation from ~30
  spices, joined pairwise: `CUMIN_SAFFRON`, `HARISSA_SUMAC`, etc.
  Shown as the primary column in the `-r` session picker so you can
  pick by memory instead of by timestamp.

## Ignore masks & filtering

Two toggle-able filter masks to hide clutter:

- **a** toggle mask 1 (dotfiles by default)
- **o** toggle mask 2 (build artifacts by default)

Masks are configurable in `.spycrc.toml` — you can define custom glob
patterns for each group.

**Temporary filter** (`=`): type a glob pattern to temporarily hide
non-matching files. `=!` shows only picked files. `=git` (or `=g`)
shows only files appearing in `git status` — modified, staged,
untracked, deleted, renamed, conflicted — plus parent directories
that contain such files (so you can navigate into changed subtrees).
The filter stays live as the 1Hz git poll updates `git_files`.
`=h` shows only the project's harpoon entries plus their ancestor
directories (see "Harpoon" below). `=` with an empty pattern clears
the filter. The active filter is shown in the status bar. Cleared
automatically when changing directories.

## Powerline status bar

The status bar uses powerline-style segments in this order:

- Pepper emoji (logo)
- `PROJECT_HOME` basename (hidden when unset)
- Session name in all caps (hidden when empty)
- Current path (intelligently truncated)
- Git branch with dirty flag (`main*`)
- Active state: pick counts, inventory counts, mask status, hidden
  file count, active filter

Under width pressure, segments are dropped in reverse priority:
suffix → path becomes basename → git branch. `PROJECT_HOME` and
session name are retained as the primary workspace identifiers.

`user@host` is no longer in the top bar — press `gU` (or run
`:whoami`) to flash it in the status line, or open the `I` info
overlay where it appears alongside the session name, project home,
and start directory.

Falls back to a plain text layout in mono mode.

## Focus indicators

When switching between the file list and the pane, focus is
unambiguous:

- **The whole unfocused side dims** — pane content (when the list
  has focus) or non-cursor list rows (when the pane has focus)
  render with SGR 2 / `Modifier::DIM`, so the focus target is
  obvious at a glance
- **File list cursor** dims to a muted color when the pane has focus
- **Pane cursor** shows as a reverse-video block at the pty cursor
  position when the pane is focused AND the child is on the main
  screen (plain shell / REPL). Suppressed when the pane is
  unfocused, when the child has switched to the alternate screen
  (full-screen TUIs render their own cursor — nvim's beam, vim's
  block, etc.), or when the child has explicitly hidden it
  (`\e[?25l`)
- The divider rule brightens when the pane is focused
- The divider also shows the active tab's *live* cwd (polled from
  `/proc/<pid>/cwd` on Linux, `lsof` on macOS). If the subprocess
  has `cd`'d away from where spyc launched it, the cwd is prefixed
  with `↪` and rendered in the active-tab color so it's easy to
  spot — useful when a `bash` tab has wandered.

## Configuration

`.spycrc.toml` supports per-user (`~/.spycrc.toml`) and per-project
(`.spycrc.toml` in the working directory) configuration:

- **Keymap DSL** — `map KEY action [args]` syntax to rebind any key to
  any action. Chord bindings (e.g., `^W n`) are supported.
- **Color overrides** — customize the palette for directories, cursors,
  picks, status bar segments, etc.
- **Ignore mask patterns** — define what each mask group hides.
- **Layout** — `[layout] status_position = "top" | "bottom"`. Default
  is `"top"`. `"bottom"` matches vim/tmux convention and avoids a
  double status bar when running spyc inside tmux. With `"bottom"` the
  prompt sits one row above the status bar (vim cmdline ordering).
- **Live reload** — config changes are picked up automatically without
  restarting spyc. Manual reload with **^R**.

### Bootstrapping

```sh
spyc --print-config > ~/.spycrc.toml
```

emits a fully-commented template with every option at its default —
self-documenting starting point.

## Session management

spyc auto-saves your workspace on quit and can restore it on startup.

- **Auto-save** — on quit, spyc saves the current directory, all pane
  tabs (command, label, cwd), active tab, pane height, focus state,
  the spice-themed session name, `PROJECT_HOME`, and the vertical split
  (its shape plus the second commander's cwd, or the preview file) —
  restored on `-r`, reopening column `b` where you left it.
- **`spyc --resume`** (or `-r`) — opens a session picker showing
  the session name (primary column), a human-readable timestamp
  ("just now", "2 hours ago", "3 days ago"), and the cwd.
- **j/k navigation** — browse sessions with highlighted cursor row.
  Enter to restore, n for a new session, 1-9 for direct selection.
- Sessions are de-duplicated by cwd + tab commands (most recent kept).
- Capped at 20 most recent sessions.
- **Agent session resume** — for tabs running `claude`, `codex`,
  `gemini`, `agy`, or `zot`, quitting spyc and launching with `spyc -r`
  will restore. Claude tabs spawn a fresh `claude` and type
  `/resume <id>` once it's settled (the CLI flag has a regression),
  then verify the submit landed — re-sending Enter while the command
  is still visibly unsubmitted, since Claude's async startup can eat
  a lone `\r`.
  Codex tabs spawn `codex resume <UUID>` directly. When no UUID was
  captured (pane killed before exit), codex falls back to `codex resume
  --last`, which uses codex's native cwd-filtered picker. `zot` tabs
  restore with `zot --continue` (zot's resume-most-recent-for-cwd);
  specific-session resume and transcript scrollback are a follow-up
  pending its on-disk session-file format.

## MCP server (Claude + Codex integration)

spyc runs a background MCP server on a PID-scoped Unix domain socket
(`~/.local/state/spyc/mcp-<PID>.sock`). On startup it writes two
config files so each agent discovers spyc automatically — no
`--mcp-config` flag needed:

- **`.mcp.json`** for Claude Code (JSON, `mcpServers.spyc` shape).
- **`.codex/config.toml`** for the codex CLI (TOML,
  `[mcp_servers.spyc]` shape).

Both registrations re-exec `spyc --mcp` as a stdio proxy that
forwards to the same socket, so a single server backs both agents.
Both files carry `SPYC_MCP_SOCK` in the env block.

Multiple spyc instances coexist safely; when a new instance opens
in a directory already owned by a live spyc, it prompts on stderr
before taking over (`PID N already owns MCP here. Take over?
[Y/n]`, default Y). The detection checks both `.mcp.json` and
`.codex/config.toml`. On takeover it sends a `spyc/disconnected`
notification to the old instance and rewrites both files; on
decline (`n`), the old instance keeps ownership and the new spyc
starts without MCP. Non-tty stdin (scripts/CI) auto-takes-over.
Enterprise `managed-settings.json` policies
(`deniedMcpServers`/`allowedMcpServers`) are respected for the
claude side; codex has no equivalent enterprise hook.

Claude can query and control the workspace through these tools:

**Read tools:**
- **`get_spyc_context`** -- returns cwd, cursor file, picks, inventory,
  active filter, git branch, `project_home`, `session_name`, plus the
  running spyc's `pid` and `version` (`1.59.0 (<git-sha>)`). The version
  string lets a client spot a stale server (a tool it expects is
  missing → compare the git SHA to the repo HEAD → restart spyc)
- **`get_file_content`** -- reads a file's text content (up to 100KB)

**Write tools (Claude can mutate the TUI):**
- **`navigate_to`** -- change directory or focus cursor on a file
- **`set_filter`** -- set or clear the file listing filter (glob)
- **`pick_files`** -- pick files matching glob patterns (additive)
- **`clear_picks`** -- clear all picks
- **`create_worktree(branch)`** -- create a git worktree off the
  focused commander's repo (existing branch, else a new one at HEAD)
  in a sibling `<repo>.worktrees/<branch>/` dir; returns `{branch,
  path}`. Lets a skill spin up a worktree to work in a second column
  while the first stays on its branch.
- **`remove_worktree(path)`** -- tear down a worktree by the path
  `create_worktree` returned. Refuses a dirty/locked worktree or one a
  column is currently open in; leaves the branch ref intact.
- **`clean_worktree(path)`** -- like `remove_worktree`, but instead of
  choking on untracked junk it archives the worktree's untracked files
  into the graveyard (recoverable, under `<worktree>-<timestamp>`) and
  then removes it. Still refuses uncommitted changes to *tracked* files
  (commit/stash first) and a column-occupied worktree.
- **`open_worktree(path)`** -- open the second spyc column (column `b`)
  at the worktree (re-targets `b` if already open), so the agent can
  work in it while the main column stays put. After this,
  `navigate_to` / search / `pick_files` act on `b`.

**Search tools (gitignore-aware, scoped to the focused commander's worktree root — its repo root, else PROJECT_HOME, else cwd):**
- **`search_paths(query, [limit])`** -- fuzzy filename search
  (same `ignore` walker + nucleo ranking as the `F` picker).
  Returns repo-relative paths, fzf-style ranked.
- **`search_content(pattern, [limit])`** -- regex content search
  via the embedded ripgrep matcher (same as `:grep`). Returns
  `{path, line, col, text}` objects.
- **`search_picks(pattern, [limit])`** -- *spyc-shaped*: content
  search restricted to the user's currently-picked files. Picks
  are TUI multi-select state Claude can't see otherwise, so this
  is the only way to grep the user's intended subset.
- **`search_inventory(pattern, [limit])`** -- *spyc-shaped*:
  content search over the persistent inventory cache (yanked
  files surviving across sessions). Lets Claude grep accumulated
  "interesting files" without re-establishing context.

Write actions execute on the main thread via a command channel.
Flash messages (`[mcp] navigated to src/`) inform the user when
Claude changes the workspace. The `gf`/`gF` keys complete the loop:
jump from Claude's output back to the file list.

## Info and diagnostics

- **:date** show date and time (UTC)
- **gV** show spyc version (also `:version`)
- **I** session info: PID, RSS memory usage, entry counts
- **A** activity monitor: live draws/sec, cells/sec, draw reason
  breakdown (pane/event/other), frame/render/echo peak latencies,
  bg-task / git / fs / mcp rates, pid/rss/threads, build identity —
  fixed-width so it doesn't bounce as rates rise and fall — plus an
  extended section tallying cumulative per-tool **MCP call counts**
  (every agent `tools/call`, read tools included)
- **C** toggle between color and mono themes
- **s** set an environment variable (`NAME=VALUE`)
- **:dump-scrollback** write the active pane's scrollback snapshot
  (one line per row) to `/tmp/spyc-scrollback.txt` — diagnostic for
  the `^a v` capture path when visible content seems to go missing

## Building

```sh
cargo build            # dev build
cargo build --release  # release build
make                   # see Makefile for build, release, cross-compile, install, deploy
```

Cross-compilation targets are available via the Makefile for deployment
to remote hosts.

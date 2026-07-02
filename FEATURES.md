# spyc — features

spyc is a vi-keyboard-driven terminal file manager written in Rust. It's
built for developers who live in the terminal and want a fast, modal
interface for navigating files, running commands, and — critically —
working alongside AI coding assistants like Claude Code.

In the lineage of keyboard-driven terminal file commanders like `spy`, spyc
brings an "always-open workspace" philosophy to modern terminal workflows. The split-pane design lets you browse your
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

## Chord hints (which-key popup)

spyc has a lot of chord prefixes (`g`, `^a`, `H`, `W`, `y`, `m`, `[`/`]`,
…). Rather than memorize them all, **press a prefix and wait** — after a
short pause a popup appears listing every key that completes the chord,
each with a one-line description, flowed into columns. Pressing the next
key (or `Esc`) dismisses it. It's spyc's on-demand answer to "what can I
do from here?", modeled on Neovim's which-key.

- The delay is `[layout] chord_hint_delay_ms` (default **300** ms). Set
  it to **0** to disable the popup entirely.
- The popup always reflects the *real* bindings — it's generated from the
  resolver, so it can't drift out of date (a test enforces that every
  advertised key resolves to the action it shows).
- `?` (or `F1`) still opens the full, scrollable, searchable key reference.

## Directory browsing

- **Enter** descend into a directory, or view a text file in the pager
- **e / v** descend into a directory, or open a file in `$EDITOR` (suspends TUI)
- **dd / Ndd** remove the cursor entry (+ N-1 below) to the graveyard,
  confirming with `y` (bare `d` arms the chord, vim-style; any other key
  cancels)
- **V** open `$EDITOR` in the top pane — bottom pane stays visible
- **D** open the cursor file in spyc's in-app pager mounted in the
  top-pane slot — bottom pane stays visible. Same loader as `Enter`
  (syntax highlighting, markdown render, hex dump for
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
- **gu** — unstaged diff (plain `git diff`): the index vs the working
  tree — only what changed *since* you staged. The view you want after
  staging a checkpoint and continuing to edit (e.g. while an agent keeps
  working): `gD` shows the checkpoint, `gu` shows everything since
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
  **graveyard** (see below) — recover with `:graveyard` or `:undo`.
- **+** create a new directory
- **:longlist** long listing -- aligned table with inode, mode, octal,
  links, owner, group, size, bytes, blocks, mtime/atime/ctime/birth,
  name (symlinks as `name -> target`). Pager height fits to content.
- **:filetype** run `file(1)` on the selection
- **:chmod** chmod +x

  *(`:longlist` / `:filetype` / `:chmod` are the rarely-used file ops —
  kept off the default keymap to keep it lean; bind a key if you use them,
  e.g. `map L command longlist`.)*

## Split pane with multi-tab pty

The bottom half of the terminal hosts a fully independent pty — by
default, it runs `claude` (the Claude Code CLI). This is the core of
spyc's workflow: browse files above, talk to Claude below.

- **^\\ / F10** toggle the pane open/closed
- **F9** open pane with `claude --resume`
- **^a j / ^a k** switch focus between the file list and the pane
  (`^w` also works as an alias for `^a`)
- **^a s** send the current selection (file paths) to the pane as stdin
- **^a ↓** send a literal `^a` to the pane — the prefix is otherwise
  unreachable by the child, but Claude binds `^a` (e.g. to expand notes),
  so this is the tmux-style "send-prefix" escape hatch
- **^a P** pipe file contents of the selection to the pane
- **^a i** pipe inventory file contents to the pane
- **^z** suspend/resume an **agent** pane (claude/codex/…). spyc sends
  `SIGSTOP` to the pane's process group itself and shows 💤 on the divider;
  press `^z` again to `SIGCONT` it back. (It manages the stop rather than
  letting the agent self-suspend: Claude catches `^z` and its handler, on
  macOS, looked like an exit and got the pane killed — `SIGSTOP` is uncatchable,
  so it just freezes.) A **shell** pane's `^z` is forwarded for its own job
  control, unchanged
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
- **Agent-activity dots** — each **agent** pane tab shows a live activity dot
  in the divider, from two sources:
  - **Output timing** (no hooks, no screen-scraping): a **spicy heat-pulse `●`**
    — a pepper-red → ember → orange → spark color *breath* (~4 Hz) — while
    output is flowing, fading to a quiet `·` once the agent goes silent. The
    pulse animates only while something is working, so a fully-idle pane set
    still renders at 0 fps.
  - **Semantic self-report** via the `report_status` MCP tool: a cooperative
    agent (Claude, codex, …) tells spyc when it's `working` (holds the pulse
    through a silent thinking pause — no false "idle"), `blocked` (a steady
    **hot-red square `■`** — the "which agent needs me" signal), or `done` (a
    calm teal square `■`). Shape carries meaning: a **circle `●`** is live /
    animated (working), a **square `■`** is a *settled, waiting* state (blocked
    or done) — so you can tell "needs me / finished" from "busy" at a glance.
    A live `working`/`done` report overrides the timing guess until it expires
    or the agent resumes output. **`blocked` is latched**: it stays a steady red
    square — no TTL, no output or animation revives it — until you actually
    answer the pane by pressing **Enter** in it (or the agent files a newer
    report). Non-agent tabs (a plain shell) get no dot.
  - **Auto-reporting (claude + codex; agy partial)** — so it works without the
    agent choosing to call the tool, spyc installs lifecycle hooks (prompt-submit
    → working, needs-permission/approval → blocked, turn-end → done) that run
    `spyc --report-status`. The agents share the same event idea, with per-agent
    config: **claude** writes `.claude/settings.json` (JSON, reloaded live);
    **codex** writes inline `[[hooks.*]]` into the same `.codex/config.toml` that
    already holds the MCP entry (read once at startup, so for an already-consented
    repo the hooks are written *before* codex spawns); **agy** (Antigravity)
    writes a `spyc-status` set into `.agents/hooks.json` but is **partial** —
    agy exposes no permission/approval event, so it gets `working` + `done` only,
    never the red `blocked` "needs me" square.
    **It asks first**, once per project: the first `claude`/`codex`/`agy` launch
    in a repo pops a `[Y/n]` ("let spyc show this agent's live status? writes
    hooks to `<config>`, removed on exit"), and the answer is **saved per repo** —
    never nags again. The write preserves your existing hooks/config; on exit
    spyc removes only what it added (and never a git-tracked file). The popup
    requires an explicit decision — `y` or `n`; Esc and any other key keep it up
    (it can't be dismissed accidentally), and a saved `n` is undoable, so
    **`:hooks on|on!|off`** changes a project's choice later — `on` also installs
    the hooks for an already-running agent (claude live-reloads → kicks in on the
    next message; codex/agy pick them up on their next launch). That's the undo
    for an accidental "no". If a claude live reload doesn't take (e.g. the running
    spyc is a throwaway build-dir binary whose path went stale), **`:hooks on!`**
    force-restarts the active claude pane and resumes the conversation so the
    hooks load from launch.

  **`:why-status`** flashes the active tab's state, its source (self-reported
  vs output-timing), and seconds since last output, for debugging.
- **Agent notifications** (`[notify]` config) — the "which agent needs me" ping,
  fired the instant a pane transitions (0 delay, not a timer):
  - **Desktop notification** naming the tab (e.g. *"codex needs you — tab 2 is
    blocked"*) on `Blocked` and `Done`. **On by default.** `desktop_via` picks
    how it's delivered — default **`"auto"`**: an **OSC-9 terminal escape over
    SSH** (so it pops on your *client* terminal, where your eyes are) and the OS
    notifier (`notify-rust`) locally. Force it with `"system"` (OS notifier — the
    machine spyc runs on), `"osc9"` (terminal escape — needs iTerm2/kitty/WezTerm/
    …), or `"both"`. Set `desktop = false` to silence, or `desktop_done = false`
    to be pinged only when an agent is *blocked* (not on every finished turn).
  - **Terminal bell** (`bell = true`, off by default) — rings alongside the
    notification.
  - **Visual bell** (`visual = false` to opt out; **on by default**) — a brief
    spice-heat **border pulse** (spyc's pepper→ember→orange→spark gradient)
    sweeping around the whole frame; the branded, non-reflowing attention flash.
  - **Blocked-only by default for the intrusive channels.** `Blocked` ("needs
    me") fires every enabled channel; the routine `Done` (once per *finished
    turn*) fires only channels that opt in. The quiet desktop ping does
    (`desktop_done`, on) but the interrupting bell and on-screen flash stay
    `Blocked`-only so they don't ring/strobe every turn — flip `bell_done` /
    `visual_done` to fire those on `Done` too.
  - **`suppress_focused_tab`** (**off by default**) stays quiet when the
    transitioning tab is the one you're already watching. Off by default because
    spyc's keyboard focus doesn't mean your eyes are on the terminal — with the
    agent pane focused you're usually working in another app while it runs, so
    the "needs me" / "done" ping is exactly when you want it. Set it true to mute
    the on-screen tab.
  - **`:notify test`** fires every channel on demand (bell + visual + both desktop
    mechanisms), bypassing the config gating — to verify your setup without waiting
    for a real agent transition.
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
  picks, filter, sort, git markers, and harpoon. **^s x** closes it.
  (`^d` does *not* close it — it quits, so `^d^d` with `b` open lets the
  session save + `-r` restore the split.)
  `^a a`/`^a h` focus the left commander (a), `^a b`/`^a l` the right
  (b). Everything follows the focused column: navigation, `V`/`D`
  overlays, file ops (`O`/`+`/`W`), `!`/`;`/`$` shell cwd, the MCP
  context, and the per-column tools — **grep `F`**, **find**, **MCP
  search**, and **harpoon** all scope to the focused column's *worktree
  root* (so `b` in a separate worktree searches its own tree, with its
  own pinned harpoon list). **g w** jumps the focused column to its
  worktree / repo root; `PROJECT_HOME` (jumped by `Space p`) stays the
  overall anchor. Git markers in each column refresh on filesystem
  events independently. (The second-commander chord is `^s`, not `^z`:
  `^z` to a shell in the bottom pane is SIGTSTP — it'd background a
  running job. `^s` is at worst terminal flow-control XOFF, which spyc's
  raw mode clears; over a flow-control-sensitive SSH/serial link it may
  still be intercepted.)
- **W l** opens a **worktree picker** — `j`/`k` (or arrows) move the
  highlighted row, `Enter` switches the *focused* column to it, `/`
  searches (the cursor lands on the match), and `1`-`9` quick-switch by
  number (focus `b` first to put a worktree there). With the MCP tools
  (`create_worktree` / `open_worktree` /
  `remove_worktree` / `clean_worktree`) an agent can spin up a worktree,
  open it in `b`, work, and tear it down — while `a` stays on the main
  branch.
- **^a v** enter scrollback view — browse up to 10K lines of pane
  history in the **in-app pager** (v1.5). Line numbers are on by
  default here (a gutter the live pane never shows, so it reads at a
  glance as scrolled-back rather than live; `l` toggles). All pager
  keys work: `/` search with `n` / `N`, `:N` jump, `V` visual line,
  `^v` visual block, `y` yanks the selection, `l` toggles line
  numbers, `w` whitespace markers, and (in an agent transcript) `t`
  toggles tool-call lines. The pty keeps running off-screen — output
  you miss while reading lands in scrollback for the next view. `Esc`
  snaps back to live.
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
  - **`t` toggles tool calls.** In a transcript scrollback, `t`
    shows / hides the agent's tool-use and tool-result lines (the
    `⚙`/`└` entries) so you can skim just the conversation prose,
    then bring the tool activity back. Shown by default; the choice
    persists across `^a v` re-opens for the session.
  - **`H` opens the scrollback's own help.** The transcript view has
    its own help (the keys above + a blurb on the transcript engine),
    shown in the bottom pane; pressing `H` again toggles to the full
    pager-keys help (separate but linked), and `Esc`/`q` returns to the
    scrollback. (`H` is a no-op in the *generic* overlay pager only when
    it's a bottom scrollback — there it's this transcript help.)
- **Ctrl+J** newline in pane (multi-line input for Claude CLI)
- **gf** jump to a file path referenced in pane output; **gF** also
  opens the pager at the referenced line. Scans the last 200 lines of
  output (including scrollback) so paths in large diffs are still found.

### Multi-tab

Multiple tabs, each running an independent pty:

- **^a c** new tab (prompts for command and working directory)
- **^a K / ^a x** close the active tab — confirms first (`y`/`N`) when the
  tab's child is still running, so a stray keystroke can't kill a live agent
  session; an already-exited tab closes silently
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
- **Default cwd** for a new pane (`^a c` and bare-spawn / `F9 resume`)
  follows `[pane] new_tab_cwd`: `"project_home"` (default) anchors it
  to `PROJECT_HOME`, `"browse_dir"` opens it in the focused column's
  current dir.

## In-app pager

A built-in pager for viewing files and command output without leaving
spyc.

- **Syntax highlighting** via syntect — source files are highlighted
  with language-aware coloring (hundreds of languages supported)
- ANSI color preservation — captured command output looks exactly right
- **Streaming output** — `!` commands show output live with an
  hourglass timer, stderr merged so build progress appears in real-time
- **/ search** forward / **? search** backward within pager content,
  each landing on the nearest match from the current scroll; **n / N**
  repeat in / against the search direction (vim/less semantics)
- **:N** jump to line N
- **l** toggle line numbers (on by default)
- **w** toggle whitespace markers (·, ↲, $) — including a `→` marker
  on each tab. Tabs always expand to `[pager] tab_width` columns
  (default 4) so indentation lines up whether or not markers are on;
  `w` just reveals them.
- **W** toggle line wrap (default on for content; off for picker UIs)
- **m** toggle Markdown rendered ↔ source view (`.md`/`.markdown` only;
  flashes "not a markdown file" otherwise)
- **f** toggle full-width mode vs. centered overlay
- **v** open pager content in `$EDITOR`
- **s** save pager content to a file
- **y** yank pager content to the system clipboard (always operates
  on the *source* — yanking a rendered Markdown view gives you back
  the markdown text, not the styled rendering)
- **V** vi-style **visual line mode** to yank a line range, with a
  double-tap to arm. The first `V` drops a line cursor at the top
  visible line (the whole candidate row highlights); move it with
  `j` / `k` / `gg` / `G` / `^d` / `^u` / … to the *exact* line the
  selection should start on, then a second `V` anchors the selection
  there. From the armed selection `j` / `k` / `^d` / `^u` / `^f` /
  `^b` / `g` / `G` / `Home` / `End` extend it (auto-scrolling), the
  status footer shows `L{lo}-L{hi}` and the line count, `y` yanks the
  inclusive range to the clipboard and exits, `Esc` / `V` cancel.
  (Decoupling the anchor from the top of the viewport lets a
  selection begin on a precise line.)
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
  (persists across chdir); **`:sort reverse`** toggles reverse order
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
to `$HOME`, and `Space p` jumps to `PROJECT_HOME`.

## Global menu (leader)

Workspace-level commands live behind a **leader** so they're reachable
from anywhere — including while you're typing to the agent in the bottom
pane. The leader is **`Space`** in the file list; from the pane it's
**`^a Space`** (a bare `Space` is literal text to the child, so `^a`
wakes spyc first, then `Space` enters the menu). Hold it and the
which-key popup lists the options:

- **`Space w l` / `w n` / `w d`** — worktree list / new / delete (the same
  submenu as the `W` prefix, which still works in the list).
- **`Space p`** — jump to `PROJECT_HOME`.
- **`Space P`** — set `PROJECT_HOME` to the current directory.
- **`Space s`** — session info.
- **`Space ?`** — open this help.

This is the line between *global* commands (worktree, project — they make
sense from any focus) and *frame* commands (git, picks, sort — they act on
the file view and stay on the `g` / letter chords). `gh` (old project-home
jump) is gone in favor of `Space p`; `gw` (jump to the worktree root) stays.

## Project home & session name

Each spyc run has a **`PROJECT_HOME`** (a sticky project root) and a
**session name** (a spice-themed label like `SAFFRON_CUMIN`). Both are
shown on the top bar and persist across `spyc -r`.

- **Auto-detect** — `PROJECT_HOME` is set to the launch directory
  automatically when that directory contains a `.git` entry. No
  upward walk — the concept is explicit and predictable.
- **Keys** — `Space p` jumps to `PROJECT_HOME`; `gP` (or `Space P`) sets it
  to the current directory; `gS` re-points the start directory (target of `` ` ``).
- **Commands** — `:project` prints; `:project .`, `:project <path>`,
  `:project clear` manage the value. `:startdir` manages start dir.
  `:name <NEW>` renames the session (normalized to
  `[A-Z0-9_]`). `:whoami` flashes `user@host`.
- **New pane tabs** default their cwd to `PROJECT_HOME` when set,
  otherwise to the current listing dir. Set `[pane] new_tab_cwd =
  "browse_dir"` in `.spycrc.toml` to open new panes in the current
  listing dir instead ("open here").
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

`user@host` is no longer in the top bar — run `:whoami` to flash it
in the status line, or open the `I` info
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
  any action. Chord bindings (e.g., `^W n`) are supported. Beyond the
  built-in actions, a key can run a `unix` shell template
  (`map ^P unix ps aux`), a `jump`/`patternpick`, or a **`:` command**
  (`map A command graveyard`), or a **Lua script**
  (`map z lua mymacro` → runs `~/.config/spyc/lua/mymacro.lua`). The
  less-frequent features ship as
  `:` commands with no default key (graveyard, activity monitor,
  long-list, file-type, chmod) — `--print-config` lists them as
  commented `command` examples to copy-and-enable. `unix` / `command` /
  `lua` / `jump` only take effect in `~/.spycrc.toml` (a project file can't
  bind a single-keypress code runner in an untrusted clone).
- **Lua scripting** — embed real logic in your config (`mlua`, vendored
  Lua 5.4). A `map KEY lua <name>` binding runs
  `~/.config/spyc/lua/<name>.lua`, which calls a `spyc.*` API: read context
  (`spyc.context()` / `cwd` / `cursor`); query **live** worktree/git/file/search
  state and use the result inline (`spyc.worktrees()`, `spyc.git_status()`,
  `spyc.git_log{limit=N}`, `spyc.read(path)`, `spyc.search_paths(query)`,
  `spyc.search_content(regex)` — computed synchronously on the worker, no
  main-loop round-trip, since they reuse spyc's root-scoped, thread-safe MCP
  readers; a failure — bad path, not-a-repo, invalid regex — raises a Lua
  error, "nothing here" is an empty table); drive the view (`navigate` / `pick`
  / `filter` / `report_status`), invoke any built-in action by its
  canonical snake_case name — the full keymap vocabulary, not just the
  curated DSL verbs (`spyc.action("git_blame")`, `spyc.action("down", 3)`;
  `set_mark` / `jump_mark` are excluded, since they need a mark letter with
  no sensible default — use `spyc.cmd(":…")` there) — or a `:` command
  (`spyc.cmd(":grep foo")`),
  and `notify` / `warn`. An optional `~/.config/spyc/init.lua` is a config
  platform: `spyc.map("z", fn)` binds a key to a Lua callback and
  `spyc.command("blame", fn)` registers a runtime `:` command — both fire
  the callback later (Tab-completion lists registered commands).
  `spyc.on(event, fn)` registers an **event hook** — spyc runs the callback
  (passing an `ev` table) when a low-frequency event fires: `startup` (once,
  after init.lua loads), `dir_changed` (`ev.cwd`), `project_changed`
  (`ev.project_home`), and `agent_status` (`ev.pane`, `ev.state` ∈
  working|blocked|done|idle — fired on a semantic transition, so an agent going
  `blocked`/`done` is a natural trigger, e.g.
  `spyc.on("agent_status", function(ev) if ev.state == "blocked" then spyc.notify("agent "..ev.pane.." blocked") end end)`).
  A change a Lua handler itself causes (e.g. an `on("dir_changed")` handler that
  navigates) does not re-fire the event, so hooks can't loop. High-frequency
  events (cursor/output) are intentionally not exposed. `:lua reload` (or `^R`) re-runs
  init.lua so edits take without a restart; `:lua status` reports the
  registered map/command counts. Scripts run on a dedicated worker thread
  so a runaway can't freeze the UI: if a script runs past ~1s an interactive
  **"lua '\<name\>' running Ns — keep waiting? [y/N]"** prompt pops — `N`/`Esc`
  aborts it, `y` keeps waiting (re-prompting after another second) — backed by
  an instruction-budget hook and a hard 30s ceiling, and `:lua off` /
  `--no-lua` disable the engine. `$HOME`-only (a project config can never run
  Lua, and init.lua loads only from `~/.config/spyc/`).
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

- **Crash-sufficient autosave** — beyond the quit-time save, spyc
  re-saves the session ~2s after any change (new tab, `cd`, split
  resize, …) to a stable per-session file written atomically, so a hard
  kill (`SIGKILL`, crash, laptop sleep) loses at most that couple of
  seconds rather than everything since launch. It only writes when
  something actually changed, so an idle spyc does no disk work.
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

**`[pane] codex_mcp = false`** (`.spycrc.toml`) stops spyc from registering
its MCP server for codex — an escape hatch for a codex `/review` bug
([openai/codex#25856](https://github.com/openai/codex/issues/25856)) where
codex mis-resolves the MCP tool-call approval elicitation and hangs on the
first spyc tool call. Status hooks still install (activity dots keep working);
codex just loses spyc's MCP tools. Claude is unaffected. Default on.

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
  running spyc's `pid` and `version` (`<x.y.z> (<git-sha>)`). The version
  string lets a client spot a stale server (a tool it expects is
  missing → compare the git SHA to the repo HEAD → restart spyc)
- **`get_file_content`** -- reads a file's text content (up to 100KB)

**Write tools (Claude can mutate the TUI):**
- **`report_status(status, [pane], [ttl_ms])`** -- self-report activity for
  your pane's dot: `working` / `blocked` (the "needs me" hot-red dot) / `done` /
  `idle`. Overrides spyc's output-timing guess; targets the focused tab by
  default.
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
  `navigate_to` / search / `pick_files` act on `b`. An agent-opened
  column `b` does **not** steal keyboard focus: the user keeps typing to
  the pane where the conversation is (only `^s n`, the user's own open,
  moves the keyboard into `b`).

A column is never left stranded in a **deleted worktree**: if its
directory is removed out from under it by *any* means -- spyc's own
`remove_worktree`, an external `git worktree remove`, `rm -rf`, or
another agent -- the next listing refresh snaps that column back to
PROJECT_HOME (or, if PROJECT_HOME is gone too, the nearest existing
ancestor of the dead path) with a `directory not found, …` flash, so
a pane is never stranded.

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

**Git tools (in-process gix, scoped like search; no `git` subprocess):**
- **`git_status`** -- working-tree status as structured JSON,
  `{path, staged, unstaged, untracked}` per changed path.
- **`git_log([limit])`** -- recent commits newest-first,
  `{short_id, author, time, subject}` per commit.
- **`git_diff([cached], [unstaged], [paths])`** -- unified-diff text
  at one of three scopes: the working tree (staged + unstaged +
  untracked) vs HEAD (default), `cached:true` for staged-vs-HEAD, or
  `unstaged:true` for the index vs the working tree (plain `git diff` —
  only what changed since the last `git add`, the read for a
  stage-then-keep-editing checkpoint); `paths` restricts to specific
  files/subtrees. The read for reviewing your own changes before
  committing.

**Working in another worktree (`root`):** every read/search/git tool
above (`get_file_content`, `search_paths`, `search_content`,
`git_status`, `git_log`, `git_diff`) takes an optional `root` (an
absolute path) to target a *different* worktree than the user's
focused column — pass the path returned by `create_worktree` /
`list_worktrees`. Without it the tools follow the focused column (so
an agent editing in a sibling worktree should pass `root`).

Write actions execute on the main thread via a command channel.
Flash messages (`[mcp] navigated to src/`) inform the user when
Claude changes the workspace. The `gf`/`gF` keys complete the loop:
jump from Claude's output back to the file list.

## Info and diagnostics

- **:date** show date and time (UTC)
- **gV** show spyc version (also `:version`)
- **I** session info: PID, RSS memory usage, entry counts
- **:activity** activity monitor: live draws/sec, cells/sec, draw reason
  breakdown (pane/event/other), frame/render/echo peak latencies,
  bg-task / git / fs / mcp rates, pid/rss/threads, build identity —
  fixed-width so it doesn't bounce as rates rise and fall — plus an
  extended section tallying cumulative per-tool **MCP call counts**
  (every agent `tools/call`, read tools included)
- **C** toggle between color and mono themes
- **:setenv NAME=VALUE** set an environment variable
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

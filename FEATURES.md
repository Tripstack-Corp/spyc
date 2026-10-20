# spyc — features

spyc is a vi-keyboard-driven terminal file manager written in Rust. It's
built for developers who live in the terminal and want a fast, modal
interface for navigating files, running commands, and — critically —
working alongside AI coding assistants like Claude Code.

In the lineage of keyboard-driven terminal file commanders like `spy`, spyc
brings an "always-open workspace" philosophy to modern terminal workflows. The split-pane design lets you browse your
project in the top half while Claude runs in the bottom half, so file
context and AI conversation stay in the same window.

This page is the overview. Each area below says what it covers and links to its full reference; the short ones are here in full. Every key is in [`docs/KEYBINDINGS.md`](docs/KEYBINDINGS.md), and `?` lists them inside spyc.

## Vi-style navigation

Everything is keyboard-driven with vi motions as the foundation.

- **h/j/k/l** movement across a multi-column file listing (h/l clamp at edges, no wrap)
- **gg / G** to jump to top or bottom
- **^B / ^F** page up and down
- **Count prefix** — `5j`, `10k`, etc. with visual display in the prompt area
- **/ search** case-insensitive substring match by default (so `/env`
  finds `.env`, `.envrc`, *and* `environment.toml`); switches to glob
  the moment the query contains `*`, `?`, or `[`. Anchor it with
  `^`/`$` when you want starts-with / ends-with: `/^env` finds
  `environment.toml` but not `.envrc`, `/env$` finds `.env` but not
  `.envrc`, `/^env$` is an exact match. Anchors compose with globs
  (`/^a?c$`). A `$` that doesn't trail the query stays literal, so
  `/$RECYCLE` still finds `$RECYCLE.BIN`. `=` takes the same syntax.
- **n / N** to repeat search forward / backward

## Chord hints (which-key popup)

spyc has a lot of chord prefixes (`g`, `^a`, `H`, `W`, `y`, `m`, `[`/`]`,
…). Rather than memorize them all, **press a prefix and wait** — after a
short pause a popup appears listing every key that completes the chord,
each with a one-line description, flowed into columns. Pressing the next
key (or `Esc`) dismisses it. It's spyc's on-demand answer to "what can I
do from here?", modelled on Neovim's which-key.

- The delay is `[layout] chord_hint_delay_ms` (default **300** ms). Set
  it to **0** to disable the popup entirely.
- The popup always reflects the *real* bindings — it's generated from the
  resolver, so it can't drift out of date (a test enforces that every
  advertised key resolves to the action it shows).
- `?` (or `F1`) still opens the full, scrollable, searchable key reference.

## Browsing files

`Enter` descends into a directory, opens a text file in the pager, or shows an image; the `=` limit filter narrows the listing. Picks are a per-directory multi-select, and the inventory a persistent cache of files to put down elsewhere. `c`, `M` and `R` copy, move and remove, and a removed file goes to the graveyard, from which the `:graveyard` viewer restores it. `^s |` previews a file in a second column and `^s n` opens a second file list there, and `W l` moves either column to another worktree. Marks (`m{a-z}`), harpoon's nine pinned slots (`H`) and the ignore masks (`a`, `o`) round it out.

Full reference: [Browsing files](docs/features/browsing.md).

## The agent pane

The bottom of the screen is a full terminal, running `claude` by default, with as many tabs as you open. Each agent tab carries an activity dot saying whether the agent is working, needs you, or is done. `^a z` zooms, `^a F` forks the conversation into a new tab, `^z` suspends a tab's program, `^a s` sends the selection to it, `^a v` scrolls back through its output or transcript, and `^a u` labels what it printed (URLs, paths, SHAs) to yank or open.

Full reference: [The agent pane](docs/features/pane.md).

## Agents and MCP

Every agent spyc launches reaches spyc's MCP server: tools to read the user's view, drive the file list, search, read git in-process, manage worktrees and report the agent's status. `spyc --install-skill` installs a guide that teaches an agent those tools. `:agent hud` shows a strip of the agent's own session facts below its pane, and `^a g` lists the images an agent actually received.

Full reference: [Agents and MCP](docs/features/agents.md).

## Mouse

Mouse reporting is on by default: the wheel scrolls whatever is under the pointer, a click focuses it, middle-click pastes and right-click opens the leader menu. A drag selects and copies text in a pane, the pager or any chrome row, or file names in the list, and a double-click takes a whole chrome line. `:mouse off` gives the terminal its own selection back.

Full reference: [Mouse](docs/features/mouse.md).

## The pager and git views

Files and command output open in a built-in pager with syntax highlighting, ANSI colour, search, line numbers, a hex dump and markdown rendering. Closed views go on a history (`:bprev` / `:bnext`, and `gp` reopens the last). Git diff, show and blame are pager views too, built in-process, with intra-line highlighting and a split or unified layout.

Full reference: [The pager and git views](docs/features/pager.md).

## Shell and the command line

`!` runs a command and streams its output into the pager, `;` runs it in the foreground with the terminal handed over, and `$` opens an interactive shell. `:` opens a vim-style command line, where the less frequent features live as commands rather than default keys. `^Z` on a running `!` capture sends it to the background, where `:fg` resumes it and `:task N` peeks.

Full reference: [Shell and the command line](docs/features/shell.md).

## Archives

`Enter` on a zip or tarball walks into it as if it were a directory, and `h` climbs out. Entering one extracts nothing; a member is read when you open it. Changes to an archive (delete, rename, put, edit) are recorded and written back with `:archive write`.

Full reference: [Archives](docs/features/archives.md).

## Projects and sessions

Workspace commands sit behind the leader: `Space` in the file list, `^a Space` from the pane. One spyc holds several projects (`Space n` opens one, `Space x` closes it, `Space j` lists them), each with its own columns, tabs and `PROJECT_HOME`. A session is saved as you go and on quit, and `spyc -r` restores every project's tabs and agent conversations.

Full reference: [Projects and sessions](docs/features/projects.md).

## Powerline status bar

The status bar uses powerline-style segments in this order:

- Pepper emoji (logo)
- `PROJECT_HOME` basename (hidden when unset)
- Session name in all caps (hidden when empty)
- While several projects are open, a chip for each in place of those two,
  with its agents' most urgent glyph ([Projects and sessions](docs/features/projects.md))
- Current path (intelligently truncated)
- Git branch with dirty flag (`main*`)
- The active tab's agent
- Column state, only what differs from rest: picks, inventory items, a
  mask switched from its default, the hidden-entry count, the filter, a
  sort other than by name (`[picks:3 hidden:14]`). A column at rest shows
  none of it.

Under width pressure the suffix switches to short forms (`[p:3 h:14]`)
before the path is cut; past that, the path is shortened in the middle.
`[layout] status_flags` sets how the suffix is written (`auto`, `short`,
or `full` for every field on every frame), and `:set flags=` changes it
for the rest of the run.

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
  obvious at a glance. A child's *own* SGR 2 is honoured too, so
  dim text in an agent CLI reads as dim rather than as normal
  weight; because both use the one channel a terminal has for
  "dimmer", the two are indistinguishable on an unfocused pane —
  already-dim text does not get dimmer
- **File list cursor** dims to a muted colour when the pane has focus
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
  with `↪` and rendered in the active-tab colour so it's easy to
  spot — useful when a `bash` tab has wandered.

## Configuration

`~/.spycrc.toml`, with a project's own `.spycrc.toml` layered on top, holds the keymap DSL (`map KEY action`, `unmap KEY`, and `unix`, `command`, `lua` and `prompt` bindings), prompt templates, Lua scripting, colours, ignore masks, layout and colour depth. Edits apply without a restart, and `^R` reloads by hand. `spyc --print-config > ~/.spycrc.toml` writes a commented template with every option at its default.

Full reference: [`CONFIGURATION.md`](CONFIGURATION.md).

## Info and diagnostics

- **Space a** the about page (also `:about`) — spyc's origin story,
  implementation notes, and where the name comes from, rendered in the
  in-app pager's markdown mode (`m` toggles to the source, `/` searches,
  `y` yanks). The copy is bundled into the binary, so it reads the same
  from an installed spyc as from a checkout.
- **:date** show date and time (UTC)
- **gV** show spyc version (also `:version`)
- **I** session info: PID, RSS memory usage, entry counts
- **:activity** activity monitor: live draws/sec, cells/sec, draw reason
  breakdown (pane/event/other), frame/render/echo peak latencies,
  bg-task / git / fs / mcp rates, pid/rss/threads, build identity —
  fixed-width so it doesn't bounce as rates rise and fall — plus an
  extended section tallying cumulative per-tool **MCP call counts**
  (every agent `tools/call`, read tools included), led by the live
  agent connections and how many are bound to a tab (`conn:2 bound:1`).
  `:activity dump` lists each connection with its tab (or
  `unattributed`, an older proxy), when it connected, and its call
  count, which is how to tell which agents are talking to *this*
  spyc. Transparent by
  default: only the text is painted, in each row's colour on the
  terminal background, so the file list or pane shows through the
  padding (and a click there reaches it). `:activity solid` switches
  to opaque colour bands; `:activity transparent` switches back —
  either one also shows the monitor
- **C** toggle between colour and mono themes
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

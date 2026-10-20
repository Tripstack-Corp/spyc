# Browsing files

Part of spyc's [feature reference](../../FEATURES.md). The file list and what acts on it: browsing, selection, file operations and their safety net, and the ways back to a place.

## Directory browsing

- **Enter** descend into a directory, or view a text file in the pager —
  or, on a PNG / JPEG / GIF / WebP, show the picture full-screen (`s` save,
  `y` copy image, `Y` copy path, `b` base64, `o` external viewer, `q`
  dismiss). Detected by magic bytes, so an extensionless screenshot still
  previews and a misnamed `.png` full of text opens as the text it is. Needs a
  terminal with a graphics protocol; without one, `o` still opens it.
- **e / v** descend into a directory, or open a file in `$EDITOR` (suspends TUI)
- **dd / Ndd** remove the cursor entry (+ N-1 below) to the graveyard,
  confirming with `y` (bare `d` arms the chord, vim-style; any other key
  cancels)
- **V** open `$EDITOR` in the top pane — bottom pane stays visible.
  Focus nav still works while it's open: `^a-j`/`^a-k` to/from the bottom
  pane, `^a-h`/`^a-l` between split columns (same workflow as `D` below);
  other pane commands (new/close/rename tab, send, …) stay paused while
  editing — quit the editor to use them
- **D** open the cursor file in spyc's in-app pager mounted in the
  top-pane slot — bottom pane stays visible. Same loader as `Enter`
  (syntax highlighting, markdown render, hex dump for
  binaries, truncation banner for big files) but mounted in the
  top slot instead of as a centred overlay. Workflow: `D` on a
  doc, `^a-j` into the bottom pane to do work, `^a-k` back to
  scroll the doc. Files past 5 MB fall back to `$PAGER` as a top
  overlay (streaming from disk wins for multi-GB logs)
- **u / -** climb to the parent directory (cursor returns to the dir you came from)
- **~ / Home** jump to home (`H` is the harpoon prefix — see Harpoon)
- **J** jump to any path (with `~` and `$VAR` expansion, frecency-ranked
  suggestions from visit history). With nothing typed it offers the
  newest path the active pane printed — the one `gf` would find — dimmed
  in the prompt: `Enter` goes there (a file opens its directory with the
  cursor on it), `→`/`End` loads it for editing, and typing replaces it.
  The prompt opens at once even when a suggestion sits on a mount that's
  stopped answering. In a codex pane the agent's own input box isn't read
  as output; see [output navigation](../HARNESS.md#output-navigation).
- **F** project-wide fuzzy filename finder. Walks the focused
  commander's worktree root (its repo root, else `PROJECT_HOME`,
  else the current dir) honouring `.gitignore`, and ranks what
  you type fzf-style, a basename hit above a parent-directory hit.
  Up/Down move selection, Enter chdirs to the matched file's parent
  and places the cursor on it; Esc cancels. There's no index to
  build: it walks the tree when you open it.
- Multi-column layout that adapts to terminal width
- Colour-coded entries: directories, executables, symlinks, files
- **Git status markers** — two-character left-gutter pair whose *positions*
  mirror `git status -s`: column 0 = staged side, column 1 = unstaged side.
  The glyphs are spyc's own, not git's letters: `~` modified, `+` added,
  `-` deleted, `>` renamed, `!` conflicted, `?` untracked. So `~ ` is
  ready-to-commit, ` ~` working-tree-only, `~~` partially staged with
  further edits, `>~` a staged rename with unstaged tweaks, ` ?` untracked
  (the leading space keeps `?` in the unstaged column), and `!!` conflicted
  — a conflict sets both halves. Each char is coloured independently
  (modified=amber, added/untracked=green, deleted=red, renamed=lavender,
  conflicted=bold red). Directories containing changes are tinted too.

  > **`!!` here means CONFLICTED, not ignored.** In `git status -s`, `!!` is
  > an ignored file — the least urgent state there is; in spyc's gutter it's a
  > merge conflict, the most urgent. Only the two-column *layout* is borrowed
  > from porcelain, so don't read the pair as git codes. Nothing on screen is
  > actually ambiguous — ignored files are dropped when status is decoded and
  > never reach the gutter — but the notation inverts git's urgency, which is
  > worth knowing before you skim past one.
- **`]g` / `[g`** jump the cursor to the next / previous file or dir
  with a non-clean git status. Wraps end-to-end so the chord is
  hold-to-cycle. Flashes "no git changes in this directory" when
  there's nothing to land on.

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
  install hint. `[clipboard] via` (`.spycrc.toml`) routes to an
  OSC-52 terminal escape over SSH instead; `[clipboard] command` /
  `$SPYC_CLIPBOARD` overrides the backend entirely with a
  user-supplied command — see CONFIGURATION.md. Every verb that yanks
  text — `yf`, `yP`, `yp`, `ya`, a mouse drag, `^a u` Quick Select and
  the image overlay's `Y` — goes the same way, so the SSH and override
  behaviour is the same whichever one you reach for.

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
- **L** long listing -- aligned table, name first (symlinks as
  `name -> target`), then mode, size, mtime, owner, group, links, octal,
  bytes, blocks, inode, atime/ctime/birth. The row is wider than a
  terminal, so it wraps, and with the name first a wrap only pushes the
  rarely needed columns down. Pager height fits the wrapped content. (Also reachable as `:longlist`.)
- **:filetype** run `file(1)` on the selection
- **:chmod** chmod +x

  *(`:filetype` / `:chmod` are the rarely-used file ops — kept off the
  default keymap to keep it lean; bind a key if you use them,
  e.g. `map f command filetype`.)*

## Graveyard — soft-delete recovery

Files removed with **R** (and items expelled from inventory) go to
a **graveyard** at `$XDG_STATE_HOME/spyc/graveyard/`, compressed, with
their permissions (the executable bit included), modification time and,
where it can, owner, all of which a restore puts back. Extended
attributes, ACLs and macOS resource forks are not kept.

- **`:graveyard`** — open the graveyard view (newest entries first). It
  has no default key; bind one with `map <KEY> command graveyard`
- **`:undo`** — restore the most-recent entry to its original path
- Inside the graveyard view:
  - **`p`** — restore the cursor entry to the current dir (cwd)
  - **`P`** — restore to its original path (refuses to clobber an
    existing file; flash error and you can fall back to `p`)
  - **`dd`** / **`x`** — purge cursor entry to the system trash
    (out of spyc, into Finder / Files / etc.)
  - **`Z`** — purge ALL entries to the system trash (single-key
    confirm)
  - **`Esc`** — close

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
**per-worktree** ordered list of file (or directory) pointers for
muscle-memory navigation. Up to 9 slots, saved on every change.

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

Each worktree keeps its own list, under
`$XDG_STATE_HOME/spyc/harpoon/`: the focused column's worktree root
picks it (else `PROJECT_HOME`), so a second column in another worktree
has its own slots and never jumps you into the wrong worktree's copy of
a file. Outside a repo with no `PROJECT_HOME`, the `H` keys flash a hint
and do nothing.

## Ignore masks & filtering

Two toggle-able filter masks to hide clutter:

- **a** toggle mask 1 (dotfiles by default)
- **o** toggle mask 2 (build artifacts by default)

Masks are configurable in `.spycrc.toml` — you can define custom glob
patterns for each group.

**Temporary filter** (`=`): type a glob pattern to temporarily hide
non-matching files — same matcher as `/`, so `^`/`$` anchor it
(`=^2026` keeps only names starting with `2026`). `=!` shows only
picked files. `=git` (or `=g`)
shows only files appearing in `git status` — modified, staged,
untracked, deleted, renamed, conflicted — plus parent directories
that contain such files (so you can navigate into changed subtrees),
and follows git as files change.
`=h` shows only the harpoon entries plus their ancestor
directories (see "Harpoon" above). `=` with an empty pattern clears
the filter. The active filter is shown in the status bar. Cleared
automatically when changing directories.

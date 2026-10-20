# The pager and git views

Part of spyc's [feature reference](../../FEATURES.md). The built-in pager, its buffer history, and the git views that open in it.

## In-app pager

A built-in pager for viewing files and command output without leaving
spyc.

- **Syntax highlighting** — source files are highlighted with
  language-aware colouring (hundreds of languages supported)
- ANSI colour preservation — captured command output looks exactly right
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
- **f** toggle full-width mode vs. centred overlay
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
- **^v** enter **visual block (columnar) mode** — vi's rectangle.
  `j` / `k` extend rows, `h` / `l` extend columns; `y` yanks the
  rectangle (each row gives the characters in the column range, a
  shorter row fewer of them). `^v`
  toggles back off; `V` while in block drops down to line mode
  (vim parity). Footer reads
  `-- VISUAL BLOCK --  L{lo}-L{hi} C{lo}-C{hi}  ({rows}×{cols})`
- **x** toggle hex-dump view for binary files
- **Markdown viewer** — `.md`/`.markdown` files open in rendered
  mode by default: headings styled, lists with bullets, fenced
  code blocks highlighted by language, blockquotes with a
  left rule, links shown with the destination URL, inline
  bold/italic/strikethrough preserved. Press `m` to toggle to the
  raw source view (itself highlighted as markdown); press `m`
  again to flip back. Yank
  and save always emit the source.
  Also rendered: **GFM alerts** (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`,
  `[!WARNING]`, `[!CAUTION]`) as coloured labels; **YAML front matter** as its
  own dim block, line for line, so a `SKILL.md` or agent memory file reads
  correctly; **table alignment** from the delimiter row (`|--:|:--|`); and
  ` ```mermaid ` blocks as diagrams (`o` / `i`). Code fences resolve the
  language by name or extension, so ` ```rust ` and ` ```rs ` highlight alike,
  and an attribute suffix (` ```rust,ignore `) doesn't defeat it.
- **Markdown outline folding** — in a rendered view, `za` folds or unfolds the
  section you're reading, `zM` collapses everything and `zR` expands it again;
  `[[` / `]]` jump between headings. vim's own fold keys, so the muscle memory
  carries over. A collapsed heading is marked `▸ N lines`, and a fold takes its
  nested subsections with it — `##` swallows the `###`s beneath it and stops at
  the next `##`. Turns a long README or ARCHITECTURE.md into a table of contents
  you can open one branch at a time.
- Page-up/down, half-page, and vi-style scrolling

## Pager buffer history

Closed pager views are saved to a history stack (up to 10). Navigate
with `:bprev`/`:bnext` from the main prompt, `[b`/`]b` (chord)
while in the pager, or **`gp`** to reopen the most-recent closed
buffer in one keystroke. The help overlay is excluded from the
stack. Walking off the end keeps the current pager open with a
flash instead of silently closing. Scroll positions are preserved.

## Git views — diff / show / blame

Git views built into spyc rather than run through `git`:
syntax-highlighted, with intra-line change
highlighting on modified lines — every changed region is marked, so an
unchanged token between two changed ones is never swept in. `[diff]
intraline` picks the granularity: `"char"` (default) marks just the
characters that differ, `"word"` marks the whole changed token the way
`git --word-diff` does.

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

Quick Select's uppercase **Git SHA** label opens the same `show` view
for that commit (see [Quick Select](pane.md#quick-select--labelled-overlay-picker)).

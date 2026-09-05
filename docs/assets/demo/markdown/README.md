# Markdown demo harness — WORK IN PROGRESS

Scripted clips of spyc's markdown viewing: the full-height rendered preview, live
re-render on save, outline folding, and the mermaid diagram painted in the terminal.

**Status: it films.** All five beats assert green and a full take has been shot —
about 67 seconds, ending on the mermaid diagram rendered as a real bitmap in the
terminal, with the `c` theme toggle flipping it dark to light.

Pacing is set by scaling every `delay` in the beats region together, so the rhythm
stays and only the length changes; the first cut took 91s to 67s at a factor of
0.68. Delays at or below 0.15s are keystroke gaps rather than pacing (the `g`-`g`
of a `gg`) and are left alone. The dwells that survive a cut are the ones the
viewer reads: the preview appearing, the `CAUTION` block re-rendering, the folded
table of contents, and the diagram in each theme. No video is committed here on purpose — a `.mov` belongs on a
release, not in git history, and nothing is worth keeping until we have a demo we
are sure of. Takes are written to `/private/tmp/spyc-demo/out/`.

## Layout

| Path | What it is |
| --- | --- |
| `spyc-quicktime-demo.applescript` | The real driver. Films a live iTerm2 window with QuickTime. Five beats, each asserted. |
| `demo.sh` | Earlier pipeline: `tui-test` drives spyc, `agg` renders the cast. Preview + live-reload clip. |
| `fold.sh` | Same pipeline, outline-folding clip. |
| `mermaid-beat.md` | Hand-recording recipe for the one beat no scripted recorder can capture. |
| `aurora-docs/` | The fixture doc tree the demo browses. Multiple top-level headings, on purpose — see below. |

The fixture is inside spyc's repo, so `prose_is_canadian_english` scans it like
any other prose — an American spelling in a demo doc fails the gate.

## Running it

```sh
osascript spyc-quicktime-demo.applescript dry   # rehearse, no recording
osascript spyc-quicktime-demo.applescript       # film it
./demo.sh && ./fold.sh                          # the tui-test clips
```

Paths resolve from the script's own folder. The fixture is copied to
`/private/tmp/spyc-demo/aurora-docs` per run and edited there, so the copy in this
repo stays pristine; override the staging root with `SPYC_DEMO_STAGE`, and the
harness location with `SPYC_DEMO_HARNESS_DIR` if `path to me` cannot resolve it.
The `.sh` drivers need `tui-test` (`brew tap microsoft/tui-test`, then `brew
trust`), `agg`, and `ffmpeg`. macOS asks once for Screen Recording permission.

## Why two pipelines

`tui-test` drives spyc well — a Ghostty backend, Playwright-style `expect text`,
and waits that beat vhs's blind `Sleep` against spyc's off-thread preview reload.
But **its recorder is a glyph rasterizer**, and so is vhs's: spyc emits the mermaid
bitmap (the footer reports `mermaid diagram · WxH`) and the recorder throws it
away, leaving a blank area. There is no text fallback — spyc's own
`detect_image_picker()` notes that halfblocks render nothing useful for a diagram.
So the mermaid beat needs a real screen capture of a real terminal, which is what
the AppleScript does.

It drives **iTerm2, not System Events**: exact geometry (`set columns` / `set
rows`), no focus stealing (keys go to the session, so you can keep working), and
`get text` gives readback so the script asserts instead of sleeping. spyc supports
iTerm2's image protocol explicitly (`SPYC-TRAP` `iterm-osc1337`), so diagrams paint.

The recorder is `screencapture -v -R <x,y,w,h>`, which films **only the window
rect**. Nothing else on the desktop is ever in the file, and no crop pass is
needed — a region take is already the right size. The `quicktime` recorder is kept
only for older systems: see the first trap below.

## Traps, all confirmed by experiment

Recording:

- **Do not try to script QuickTime into recording. Apple's own answer is to use
  something else.** Driven this way it puts up: *"QuickTime Player encountered an
  error while recording your screen. Try using the Screenshot app instead."*
  `screencapture` IS the Screenshot app's command line, so the harness is already
  on the prescribed path.

  Two separate things go wrong, and the error text names neither. `set r to new
  screen recording` fails with `The variable r is not defined`, which sounds like
  a typo — it is not: `new screen recording` declares **no `<result>`** in
  QuickTime's own dictionary (`QuickTimePlayerX.sdef`), while its siblings `new
  audio recording` and `new movie recording` both return a `document`. So the
  assignment can never bind, whatever else is true, and every pre-Mojave recipe
  on the web that uses `start document 1` is working around exactly this. Called
  correctly it then creates no document or window at all (`{0, 0}`) and raises
  the dialog above.

  QuickTime also holds no Screen Recording grant — its only TCC row is
  `kTCCServiceUbiquity`. Whether granting one would fix the recording error is
  **untested**, and the dialog argues against it: a TCC refusal prompts, it does
  not report a generic recording error. `screencapture -v -R` needs none of this,
  running under the terminal's own grant
  (`kTCCServiceScreenCapture | com.googlecode.iterm2`).
- **`get text of sess` returns the WHOLE SCROLLBACK** (617 lines for a 50-row
  window), and `contents` is identical. Assertions silently match stale output
  from earlier beats. Slice the last `rowCount` paragraphs.
- **Never assert on a spyc flash message** — `pane exited` fades before the poll
  sees it. Assert the persistent divider (`[exited`) instead.
- **`vim -n`**, no swap file. A stale `.swp` from an interrupted run opens vim on
  its `E325` recovery prompt and silently breaks every later beat.
- **Reserved AppleScript words** that will not compile as handler or variable
  names: `say`, `cr`, `put`, `before`. And `set rows to rows` is self-referential
  inside `tell session` — the property name collides with iTerm's own, the window
  silently stays short, and assertion strings fall below the fold. Name them
  `rowCount` / `colCount`.
- **Reuse one window**: remember `id of window` in a state file and tag the session
  name, then swap in a fresh tab. Do not try to talk the previous spyc into
  quitting with `^d` — it needs `^d^d`, and a half-quit instance eats the launch
  command as keystrokes.
- **A scaled Retina display is not 2x** — 3456px over 2056pt is 1.68 on this
  machine, so a hardcoded 2 crops the video wrong. Read `bounds` back after
  `set zoomed to true`.
- **Identifying "my" windows by screen content is unsafe.** The heuristic flagged
  the user's own Claude Code session, because that session's transcript contained
  the demo paths.
- Ghostty is a poor target on macOS: it refuses CLI launch, `+new-window` is not
  supported on this platform, and `open -na ... --args` is ignored while an
  instance is running.
- **U+FE0F breaks `tui-test`'s rasterizer.** spyc's logo is `U+1F336 U+FE0F`; bare
  `U+1F336` renders and the variation selector fails, and no font fixes it. The
  selector is load-bearing (`ui/line_select.rs`: bare `U+1F336` measures 1 cell,
  not 2), so this is not spyc's bug — render the cast with `agg` instead.
- **`expect text` needs a unique match** — pass `--no-strict` or a repeated string
  fails. `--match` is not a valid flag.

Driving spyc:

- **spyc restores pager scroll positions**, so a re-recorded take starts wherever
  the LAST take ended. Force `gg` after opening any pager or preview — and do it
  **before the assertion, not after**. Three beats here asserted on a near-top
  string first and took the top second; they passed for as long as the restored
  position happened to be the top, then all failed the moment a run ended on the
  mermaid fence at the bottom of the file. The preview was working perfectly; the
  anchor was just off-screen above.
- **A wide `graph LR` overflows the frame.** Five nodes left-to-right renders at
  3200px against a 200-column window and the last node is severed at the right
  edge. `graph TD` fits the terminal's aspect ratio — the fixture uses it.
- **`za` keys off the top of the view, not a cursor.** In a folded table of contents
  shorter than the screen the view cannot scroll, so `za` is unreachable and `j`
  does nothing. Use `]]` on the expanded doc — it scrolls the heading to the view
  top — then `za`.
- **`zM` folds under the outermost heading present**, so a doc with a single
  `# Title` collapses to one line. The fixture has several top-level headings for
  exactly this reason.
- **Focus:** `^a k` / `^a j` switch pane and list; `^a a` / `^a b` only switch file
  columns. A bare `^s` while the pane has focus goes to the child — use the `^a |`
  alias.
- **`^a c` is a two-step prompt** (command, then cwd), so two Enters.
  `$SPYC_PANE_CMD` prefills the command.
- **Quitting vim does not close the pane** — spyc keeps the tab showing
  `vim [exited 0]`. Close it with `^a x`.
- A full-height vsplit plus a pane gives list top-left, pane bottom-left, preview
  full-height right. It also confines the status bar to the left column, which
  truncates the path segment to about one character.

**Assertions must bite**, and this harness has now proved it twice.

The first fold clip asserted on `lines`, which matched the pager header
`(149 lines)` and proved nothing. Counting `▸` markers is the assertion that
actually fails when folding breaks.

Worse, the mermaid beat asserted on `mermaid diagram` — and **passed on spyc's own
refusal**, `no mermaid diagram in view`. The beat was opening `HANDBOOK.md`, which
has no fence at all, and filming a plain page of text while reporting green. That
string occurs in three places: the refusal, the rendered placeholder block
(`▣ mermaid diagram — i: view in terminal …`), and the image overlay. Only the
overlay means a diagram is on screen, so the assertion is `c theme` — a verb the
overlay offers only for a mermaid origin, and which cannot match anything else.

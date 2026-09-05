# Markdown demo harness — WORK IN PROGRESS

Scripted clips of spyc's markdown viewing: the full-height rendered preview, live
re-render on save, outline folding, and the mermaid diagram painted in the terminal.

**Status: the harness runs, the film does not exist yet.** All five beats of the
AppleScript driver assert green in rehearsal, but no take has been shot. No video
is committed here on purpose — a `.mov` belongs on a release, not in git history,
and nothing is worth keeping until we have a demo we are sure of. Recordings are
written to `/private/tmp/spyc-demo/out/`.

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

## Traps, all confirmed by experiment

Recording:

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

- **spyc restores pager scroll positions**, so a re-recorded take starts mid-file.
  Force `gg` after opening any pager or preview, or the shot is not reproducible.
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

**Assertions must bite.** The first fold clip asserted on `lines`, which matched
the pager header `(149 lines)` and proved nothing. Counting `▸` markers is the
assertion that actually fails when folding breaks.

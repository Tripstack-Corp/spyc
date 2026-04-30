# lazygit ↔ spyc pane: terminal-feature gap analysis

Branch: `worktree-investigate+lazygit-support`. Inputs:
- lazygit upstream at `lazygit-upstream/` (commit `69635f3`, "Update to tcell v3").
  Uses **its own bundled gocui** at `lazygit-upstream/pkg/gocui/` and
  vendored **tcell v3** at `lazygit-upstream/vendor/github.com/gdamore/tcell/v3/`.
- spyc pane: `src/pane/{mod,input,widget}.rs` against `vt100 = 0.15.2`
  (`Cargo.lock`).
- Host setup: `src/main.rs::setup_terminal` (lines ~293–326).
- Pane key intercept: `src/app/mod.rs::is_spyc_meta_when_pane_focused`
  (line 6808).

## Initialization fingerprint

What lazygit/tcell actually emits at startup, in order:

1. `\x1b[?1049h`  alt-screen (`tscreen.go:123`)
2. `\x1b[?7h`     auto-margin
3. `\x1b[?1h\x1b=` keypad (DECCKM + numeric→app)
4. `\x1b[c`       primary DA probe (`tscreen.go:128`)
5. `\x1b[>q`      XTGETTCAP / extended attr probe (`tscreen.go:129`)
6. `\x1b[?u`      Kitty keyboard query (`tscreen.go:133`)
7. `\x1b[?4m`     XTerm modifyOtherKeys query (`tscreen.go:136`)
8. `\x1b[?1004h`  focus reports — every frame (`tscreen.go:731`, `vt/mode.go:44`)
9. `\x1b[?2004h`  bracketed paste — every frame (`tscreen.go:732`, `vt/mode.go:48`)
10. `\x1b[?2026h … \x1b[?2026l` synchronized output **wraps every redraw**
    (`tscreen.go:827`, `vt/mode.go:49`)
11. Mouse, *if* `userConfig.Gui.MouseEvents` (default true,
    `pkg/config/user_config.go:776`):
    `\x1b[?1000h` button + `\x1b[?1003h` motion + `\x1b[?1006h` SGR
    (`tscreen.go:902–937`)
12. `gocui.OutputTrue` ⇒ `\x1b[38;2;R;G;Bm` truecolor SGR — but tcell
    *runtime-negotiates* it from terminfo + `$COLORTERM`; if neither
    advertises RGB it silently downgrades to 256-color
    (`tscreen.go:117–122`).
13. OSC 8 hyperlinks for "Donate" / "Ask Question" footer
    (`pkg/gui/style/hyperlink.go:7`).
14. OSC 52 clipboard via tcell, gated on DA1 reporting `?22` capability
    (`tscreen.go:130`, only fires if `t.hasClipboard`).

## Gap table

| Feature | lazygit needs it | spyc handles it | Likely user symptom if not | Evidence |
|---|---|---|---|---|
| Alt-screen (1049) | yes, unconditional | yes | — | `tscreen.go:123`; vt100 parses `MODE_ALTERNATE_SCREEN` (`screen.rs:7`) |
| 24-bit truecolor SGR | yes (`OutputTrue`) | yes on parse; **but** child only sees `TERM=xterm-256color`, no `COLORTERM` ⇒ tcell may downgrade to 256 | diff colors look "close but slightly off"; light-grey backgrounds banded | `pkg/gui/gui.go:813`; `src/pane/mod.rs:102` (no `COLORTERM=truecolor` in pane env) |
| 256-color fallback | yes | yes | — | vt100 parses `\x1b[38;5;Nm` |
| Mouse 1000/1002/1003/1006 | **yes by default** | **NO** at *both* layers | clicks on panel headers, diff lines, footer keybindings, scroll on touchpad — all do nothing inside lazygit | host: `src/main.rs:293–326` never calls `EnableMouseCapture`; encoder: `src/pane/input.rs` has no `KeyCode::Mouse(_)` arm and no mouse-event path at all; vt100 *tracks* the protocol but the data flow doesn't reach the pty |
| Bracketed paste (2004) | yes | yes | — | host: `src/main.rs:299`; pane forwarder: `src/app/mod.rs:1487–1492` wraps host paste in `\x1b[200~ … \x1b[201~` |
| Focus events (1004) | yes (gocui dims unfocused panel headers, drives "Random tip" timer) | **NO** | unfocused-panel dim state never flips; minor UI staleness, no functional break | host: `src/main.rs` (no `EnableFocusChange`); lazygit: `pkg/gocui/gui.go:731 Screen.EnableFocus()` |
| Synchronized output (2026) | yes — wraps every redraw | **NO** — vt100 0.15 has no parse arm for 2026 (`grep` confirms) | partial-frame tearing during fast scrolls / commit-list redraws; cursor briefly visible mid-redraw | `tscreen.go:827`; `grep '2026' vt100-0.15.2/src` → 0 hits |
| Cursor hide (DEC ?25l) | yes — tcell hides on init | vt100 **tracks** `hide_cursor()` but `src/pane/widget.rs:39–55` **always draws a reverse-video block** at the cursor cell, regardless | spurious reverse-video block sitting on some lazygit cell — almost certainly the visible "rendering" complaint | `vt100/screen.rs:680 hide_cursor()`; `src/pane/widget.rs:43–54` ignores it |
| Cursor shape (DECSCUSR `\x1b[N q`) | tcell uses on some terminals | not consulted, falls through vt100 | minor | — |
| OSC 8 hyperlinks | yes (Donate / Ask Question) | vt100 0.15 does not parse OSC 8 | hyperlinks render as raw `8;;url\` runs in the footer, OR (more likely) the OSC payload is dropped and only the visible label appears | `pkg/gui/style/hyperlink.go:7` |
| OSC 52 clipboard | conditional (DA1 must report `?22`) | spyc never answers DA1 from outside, so tcell sets `hasClipboard=false` and never emits OSC 52 | "y" / yank-from-lazygit silently no-ops in some flows | `tscreen.go:130` + `pkg/gocui/...` |
| Kitty keyboard (`CSI = 1 u`, `CSI ? u` query) | tcell queries; only enables on reply | vt100 0.15 doesn't parse; query bytes are dropped, no reply | none, gracefully degrades | `tscreen.go:133–137` |
| XTerm modifyOtherKeys (`CSI ? 4m`) | tcell queries; gated on reply | not parsed | none | `tscreen.go:136–138` |
| DA1 / `\x1b[c` and `\x1b[>q` probes | tcell sends, blocks briefly waiting | spyc/vt100 never reply; tcell times out | minor first-paint delay (~50ms) | `tscreen.go:128–129` |
| Tab / BackTab (panel cycle) | yes | yes | — | `src/pane/input.rs:64` (`\t`), `:65` (`\x1b[Z`) |
| Single-key bindings `?`, `s`, `c`, `d`, `D`, `e`, `<space>`, `<esc>` | yes | yes — forwarded as plain chars; only Ctrl+A / Ctrl+W / Ctrl+\\ / F10 / `0x1c` are intercepted | — | `src/app/mod.rs:6808 is_spyc_meta_when_pane_focused` |
| Resize (SIGWINCH) | yes (gocui re-lays-out on resize) | yes | — | `src/pane/mod.rs:214 resize()` |

## Top suspects (given the screenshot shows lazygit rendering)

The screenshot shows borders, diff colors, panel highlight, and the
selection bar all rendering correctly — so the alt-screen, SGR colors,
and basic cell grid are healthy. The user's "rendering / conflict
issues" are most likely (in descending probability):

1. **Spurious cursor block from `widget.rs`.** spyc unconditionally
   reverse-videoes the cell at `screen.cursor_position()`, even when
   the child has set DEC ?25l (cursor hidden). vt100 already exposes
   `screen.hide_cursor()`, but `src/pane/widget.rs:43–55` never reads
   it. lazygit hides the cursor and draws its own selection highlight,
   so a stray reverse-video square sits on some panel — visually
   reads exactly as "rendering glitch".

2. **No mouse, anywhere.** Mouse capture is not enabled on the host
   terminal (`src/main.rs::setup_terminal` has no
   `EnableMouseCapture`), and `src/pane/input.rs` has no encoder for
   `Event::Mouse`. lazygit defaults `MouseEvents: true` and binds
   click/scroll on every panel — to a daily user this manifests as
   "clicks and scroll-wheel don't work in lazygit", easily called a
   "conflict issue".

3. **Synchronized-output (mode 2026) tearing.** tcell wraps every
   redraw in `\x1b[?2026h … \x1b[?2026l`. vt100 0.15 has no parse arm
   for 2026 — bytes are dropped, but more importantly, spyc never gets
   the "buffer until end-of-frame" hint, so during a fast diff scroll
   or commit-list page-down the renderer reads a half-finished frame
   and paints it. Looks like flicker / a sliver of stale text under
   the new content for one frame.

Honourable mentions: OSC 8 footer text (Donate / Ask Question) likely
prints stray bytes or a wrong label; truecolor may be downgraded
because the pane env has no `COLORTERM=truecolor`; focus-event-driven
dim/undim state is permanently stuck.

## What we'd need to actually run to confirm

- Run lazygit in the lower pane against this worktree, with
  `SPYC_PTY_DEBUG=1`, and watch for the cursor-block on a panel where
  it shouldn't be (suspect #1) and stray bytes around the OSC 8 footer.
- Tail `pty.dump` (or whatever `SPYC_PTY_DEBUG` writes; check `mod.rs`)
  to confirm tcell sends `\x1b[?2026h` on every frame and that vt100
  swallows it.
- Click a panel and verify nothing reaches the pty
  (`SPYC_PTY_DEBUG` writer-side). That nails suspect #2 hands-down.
- Compare bare-terminal lazygit (truecolor diff palette) against
  in-pane lazygit screenshot to confirm truecolor downgrade or rule
  it out.

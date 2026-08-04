# Native mouse scroll — implementation plan

## Goal

Wheel/trackpad scrolling that means "scroll the thing under my pointer" — most of
all **scrollback in the agent / process panes**, which is where the current
approach fails hardest.

Today spyc enables DEC private mode 1007 (`EnableAlternateScroll`, `src/lib.rs:441`),
asking the terminal to translate wheel motion into `Up`/`Down` arrow keys. That
works for the file list (it *is* cursor motion), but on a focused pane the arrows
go straight to the child: in a shell you walk command history, in claude you walk
the prompt history. spyc can't tell a wheel tick from a real arrow press, so it
can't fix this by routing — the information is gone before it arrives. (`run.rs:183`
throttles rapid `Up`/`Down` to ~25/s precisely because of this hack.)

Opting into real mouse reporting gives us explicit `ScrollUp`/`ScrollDown` events
with **coordinates**, which is what makes correct routing possible.

## What we already have (don't rebuild)

- `Event::Mouse` is **already forwarded** by the input reader — `proc.rs:99-105`
  press-filters `Event::Key` and passes everything else through. `run.rs:211`
  drops it on the floor with `_ => {}`. So no reader changes are needed; we're
  filling in an existing hole.
- Scrollback buffers already exist per surface. Nothing new to store:
  - vt100 reverse-scroll on the live pane (`Pane::scroll_up`/`scroll_down_or_exit`,
    `src/pane/mod.rs:533`).
  - The `^a v` lower-pane scrollback **pager** (`install_lower_pane_scroll_view`,
    `pane_scroll.rs:361`).
  - `PagerView::scroll_by(delta: i32, viewport_height)` and
    `scroll_max(viewport_height)` (`ui/pager/scroll_search.rs:148,89`) — the exact
    primitives a wheel tick wants.
- `App::compute_layout(area, pane_open, pane_pct, status_position) -> FrameLayout`
  (`render/mod.rs:70`) is a **pure associated fn**. Hit-testing a pointer costs one
  call against `view.term_size` — no cached rects, no new state.

## User review required

> [!WARNING]
> **Loss of native click-drag text selection.** Any mouse capture takes selection
> away from the terminal emulator; the user must hold a bypass modifier
> (Shift on most, Option or Fn on iTerm2, Shift on Ghostty/WezTerm/kitty).
> Therefore: **opt-in, default off**, plus a `:mouse on|off` runtime toggle so
> selection can be reclaimed without editing the rc file and restarting.
> Mitigation to document alongside it: `^a u` quick-select already yanks
> URLs/paths/SHAs without the mouse, and `y` yanks in the scrollback pager.

Two decisions worth your call before implementation — see **Open decisions** at
the bottom.

## Design

### Tier 0 — ask for the *right* escape sequences

Do **not** use `crossterm::event::EnableMouseCapture`. In crossterm 0.29
(`src/event.rs:325-333`) it emits:

```
?1000h  ?1002h  ?1003h  ?1015h  ?1006h
```

`1003h` is **any-event motion reporting**: every pointer move over the terminal
becomes an input event. That matters here more than usual, because
`run.rs:164` does `ctx.draw.mark(2)` for *every* `Message::Input`, and
`coalesce_pending` (`sources.rs:88`) surfaces input **one per loop iteration,
never batched**. So enabling 1003 turns idle mouse movement into a
one-redraw-per-motion-event storm — a direct hit on the "0 dps at idle / target
event-driven repaint" invariant.

Emit exactly what we need instead, mirroring the existing
`EnableAlternateScroll` / `HideMousePointer` pattern in `src/lib.rs`:

```rust
/// DEC 1000 (button press/release) + 1006 (SGR extended coordinates).
/// Deliberately NOT 1002/1003 — motion reporting would wake the loop and
/// mark a redraw on every pointer move (`run.rs` marks draw on all Input).
struct EnableWheelReporting;   // "\x1b[?1000h\x1b[?1006h"
struct DisableWheelReporting;  // "\x1b[?1006l\x1b[?1000l"
```

1000h alone already reports the wheel (buttons 64/65) and clicks; 1006h gives
coordinates that survive past column 223. Belt-and-braces: also drop
`MouseEventKind::Moved` / `Drag` in the reader's forward filter (`proc.rs:99`),
since a few terminals report motion regardless of what we asked for.

1007 and 1000 must not both be on — a terminal honoring both could deliver the
tick twice (once as arrows, once as a mouse event). The two are mutually
exclusive by construction below.

### Tier 1 — reconcile terminal mouse state in one place

`App::reload_config` (`app/config.rs:16`) returns `()`, not `Vec<Effect>`, so the
draft's "push effects from the reload handler" doesn't fit. It also wouldn't
cover the other four transitions that have to get this right: startup
(`setup_terminal`), teardown (`restore_terminal` — note: there is no
`teardown_terminal`), the **panic hook** (`lib.rs:212`, which already restores
1007 and the kitty flags — a leaked `?1000h` would silently break the mouse in
the user's shell for the rest of the session), and the
`suspend_tui`/`resume_tui` pair around a foreground child (`p`/`v`/`;`) which
must hand mouse control to `less`/`vim` and take it back.

Use the established `settle_*` pattern (`settle_visual_bell`, `settle_autosave`,
`settle_agent_activity`, `settle_lua_events`):

```rust
// view.mouse_capture_on = what the TERMINAL is actually in.
// state.config.mouse.capture = what the user asked for.
// One idempotent reconcile at loop bottom covers startup, :mouse on|off,
// live rc reload, and resume-from-foreground.
fn settle_mouse_mode(&mut self) -> Vec<Effect>
```

with a single typed effect — not a stringly-typed raw-command escape hatch:

```rust
Effect::SetMouseMode { capture: bool },  // run_effects: execute!(terminal, …)
```

`run_effects` already receives `terminal` (`run.rs:204`), so this needs no new
plumbing. Enabling capture emits `DisableAlternateScroll + EnableWheelReporting`;
disabling emits `DisableWheelReporting + EnableAlternateScroll`. Panic hook and
`restore_terminal` emit `DisableWheelReporting` unconditionally (cheap, harmless
if never enabled). `suspend_tui` disables; `resume_tui` re-enables per config.

### Tier 2 — routing is a pure decision, not an inline `run.rs` branch

The draft put a focus check and action synthesis inline in `dispatch_effective`.
That's the shape `route.rs` was written to retire — its header documents five
shipped routing bugs (#75/#78/#80/#81 + the V-key bug) caused by exactly that.
It also gets the field wrong: focus is `self.state.focus`, not `self.view.focus`
(`route.rs:262`), and `active_pager_mut` is a **macro**, not a method
(`pager_handler/mod.rs:29`).

New module `src/app/mouse.rs`, following the `route.rs` / `focus.rs` template
(`Copy` snapshot → pure fn → unit tests):

```rust
#[derive(Clone, Copy)]
struct MouseSnapshot {
    modal: Option<Modal>,       // finder/capture/quick-select/harpoon eat mouse too
    region: Region,             // hit-tested from the pointer, NOT keyboard focus
    is_prompting: bool,
    pager_mount: Option<Mount>,
    has_scroll_pager: bool,
    pane_scrolling: bool,
    pane_closed: bool,
    pane_wants_mouse: bool,     // the child requested mouse reporting (see Tier 3)
}

enum MouseSink {
    Swallow,                    // a modal owns the screen
    ListCursor,                 // move the file-list cursor
    Pager,                      // scroll the pager under the pointer
    PaneForward,                // encode + send to the child
    PaneScrollback,             // spyc-owned pane scrollback
    PaneScrollbackMount,        // not in scrollback yet — mount it
}

const fn route_mouse(snap: MouseSnapshot, ev: WheelEvent) -> MouseSink
```

**Hit-test the pointer, not the focus.** This is the substantive UX upgrade over
the draft. Users expect the wheel to scroll what's under the cursor — that's the
whole reason a mouse feels different from `j`/`k`. `MouseEvent` carries
`column`/`row`; `compute_layout` is pure and cheap; `FrameLayout` already carves
`list`, `pane`, `divider`, `right`, `top_unit`, `prompt` (`app/mod.rs:201`).
Scrolling the pane while the keyboard is in the list — without stealing keyboard
focus — is the ergonomics win, and it makes the vsplit case (`list` vs `right`
preview) fall out for free.

Keep every non-wheel `MouseEventKind` a no-op for now (`Down`/`Up`/`Drag`/
`ScrollLeft`/`ScrollRight`). `route_mouse` matching exhaustively means
click-to-focus is a later, contained addition.

### Tier 3 — the agent pane: forward to a mouse-aware child first

The draft's answer for a focused pane was "synthesize a scrollback mount on
wheel-up". That's wrong for the primary dog-fooding case, in three ways:

1. **`^a v` is not a vt100 snapshot for an agent.** `open_pane_scroll_pager`
   (`pane_scroll.rs:160`) routes through `decide_scroll_source`: an agent with a
   `TranscriptSpec` on the **alternate screen** — i.e. claude in full-screen mode —
   *always* takes the `Transcript` branch, spawning an off-thread worker that
   tail-reads up to 4 MB of JSONL, parses per line, and markdown-renders it.
   Wheel-up would replace the live agent with a rendered conversation transcript.
   That's `^a v`'s job, not the wheel's.
2. **The vt100 branch blocks the loop.** It does `3 × sleep(10ms)` + drains to
   flush in-flight pty bytes (`pane_scroll.rs:273`). 40 ms of main-loop stall per
   mount — and if scrollback is empty it flashes and mounts *nothing*
   (`pane_scroll.rs:290`), so every subsequent wheel tick pays the 40 ms again.
   At ~30 ticks/s that is a hang.
3. **Modern agent TUIs scroll themselves.** claude/codex on the alt screen do
   their own viewport management; they just never receive mouse events today,
   because spyc has never enabled capture.

`vt100::Screen::mouse_protocol_mode()` and `mouse_protocol_encoding()` are
**public** in vt100 0.16 (`screen.rs:578,584`). So spyc can ask the child what it
wants and act accordingly:

| Pointer over pane | Child requested mouse? | Behavior |
|---|---|---|
| yes | yes (`mode != None`) | encode the event in the child's protocol/encoding, `send_bytes` — the agent scrolls itself |
| yes | no, has scrollback | spyc-owned pane scrollback (see below) |
| yes | no, alt-screen non-agent (vim/htop/less) | swallow with a one-shot hint — same dead-end `^a v` already reports |

This also fixes clicks inside the child for free, and it's the reason to prefer
forwarding over mounting: the agent's own scrollback is *better* than ours.

New: `pane::input::encode_mouse(ev, mode, encoding) -> Vec<u8>` — sits beside the
existing `encode_key` (`src/pane/input.rs`), pure, table-testable.

### Tier 4 — spyc-owned pane scrollback (non-mouse children)

For a plain shell pane the wheel should drive vt100 reverse-scroll — the *live*
screen scrolling up, no pager mount, no 40 ms stall, no transcript. `Pane` already
exposes `scroll_up(n)` / `scroll_down_or_exit(n)` / `enter_scroll_mode()` /
`exit_scroll_mode()`. Wheel-up enters scroll mode and scrolls; wheel-down scrolls
back; wheel-down **at the bottom** exits scroll mode and returns to live.

Two notes on the draft's version of this:

- Its auto-exit heuristic ("only exit if already at bottom *before* this event")
  is the right rule — keep it. Sticky bottom, one deliberate extra tick to leave.
- Don't assume `scroll_down_or_exit` implements it. Despite the name it only does
  `scroll_offset.saturating_sub(n)` (`pane/mod.rs:533`) — the "or_exit" half is
  vestigial. Either implement the exit at the call site or fix the function and
  its name; don't rely on the label.

Mounting the `^a v` **pager** from the wheel is dropped from the plan. If we ever
want it, the "hide line numbers / suppress EOF marker" polish the draft described
(`view.show_line_numbers = false`, reuse `streaming`) is the right idea, but
overloading `streaming` to mean "cosmetically suppress the EOF marker" is a lie
that will confuse the stream-drain code (`pending_scroll_to_bottom`, `stream_id`
gating in `pager_stream.rs`). Add an explicit `show_eof_marker: bool` instead.

### Tier 5 — wheel bursts must not become a redraw storm

A trackpad flick delivers dozens of events; each is one loop iteration and one
`draw.mark(2)`. Two cheap defenses:

1. **Accumulate, then apply.** In `dispatch_effective`'s new mouse arm, drain any
   immediately-available follow-on wheel events of the same direction from the
   channel and apply the summed delta once. Analogous to the existing
   `view.scroll_last` arrow throttle (`run.rs:183`) but *lossless* — sum the
   ticks rather than dropping them, so a fast flick scrolls far instead of
   scrolling slowly.
2. **Configurable step.** `[mouse] scroll_lines` (default 3). macOS trackpads and
   kitty/Ghostty already emit one event per notional line, so a hardcoded ×3
   overshoots badly for some users; a mouse wheel with detents undershoots at ×1.

### Configuration

Not a new `[terminal]` section for one bool. A `[mouse]` section, matching the
existing per-feature shape (`layout`, `pane`, `yank`, `pager`, `markdown`,
`delete`, `notify` — `config/mod.rs:34-52`) and leaving room to grow:

```toml
[mouse]
# Real mouse reporting (wheel + click). Breaks native click-drag selection —
# hold Shift (most terminals) or Option/Fn (iTerm2) to bypass. Default off.
capture = false
# Lines per wheel tick.
scroll_lines = 3
```

Plus `:mouse` (`app("mouse", …)` in `COMMAND_TABLE`, alongside `hooks` and `lua`
at `command_table.rs:110,113`) for `on` / `off` / bare-status. Runtime toggle
matters here more than usual — it's the escape hatch when a user needs selection
back *now*.

### Why not add `Action` variants

The draft proposed `Action::{ScrollUpMouse, ScrollDownMouse, PaneScrollbackMouse}`.
That's the wrong home:

- `Action::tier()` (`keymap/action.rs:242`) demands one tier per action, and the
  guard `leader_and_pane_namespaces_respect_tiers` enforces it. A wheel tick's
  tier is *context-dependent* (Frame over the list, Pane over the pane) — the one
  thing the taxonomy exists to forbid. `tier()` is also consulted at runtime to
  pause Pane-tier commands under a top overlay.
- `canonical_name()` (`action.rs:432`) is an exhaustive match and
  `action_names_round_trip` forces every variant into `action_from_name`, which
  means Lua gets `spyc.action("scroll_up_mouse")` and the DSL gets three verbs
  nobody can bind to a key. Actions are the *keymap* vocabulary; a mouse event
  isn't a keymap entry.

`route_mouse` returning a `MouseSink` that the mouse handler dispatches directly
to existing methods keeps the Action vocabulary clean. If a sink needs behavior an
`Action` already names, call `self.apply(&Action::Down(n))` — note it's `apply`
(or `update(UiMsg::Action(..))`, `update.rs:43`), not `apply_action`.

## Files touched

| File | Change |
|---|---|
| `src/lib.rs` | `Enable/DisableWheelReporting` commands; wire into `setup_terminal`, `restore_terminal`, `suspend_tui`, `resume_tui`, **panic hook** |
| `src/config/mod.rs` | `MouseConfig { capture, scroll_lines }` + `DEFAULT_TEMPLATE` block |
| `src/app/mouse.rs` | **new** — `MouseSnapshot` / `MouseSink` / `route_mouse` + hit-test + handler |
| `src/app/mod.rs` | `ViewState.mouse_capture_on` |
| `src/app/run.rs` | `Event::Mouse` arm → `handle_mouse` + burst accumulation (thin — the decision lives in `mouse.rs`) |
| `src/app/proc.rs` | drop `Moved`/`Drag` in the forward filter |
| `src/app/effect.rs` | `Effect::SetMouseMode { capture }` + executor arm |
| `src/app/scheduler.rs` *or* loop bottom | `settle_mouse_mode` |
| `src/app/commands.rs` + `command_table.rs` | `:mouse on\|off` |
| `src/pane/input.rs` | `encode_mouse(ev, mode, encoding)` |
| `src/pane/mod.rs` | expose the child's `mouse_protocol_mode`/`encoding`; fix or rename `scroll_down_or_exit` |

Docs, same commit (AGENTS.md "Keep docs in sync"): `AGENTS.md` (module index —
guarded by `every_app_module_is_in_the_agents_index`), `FEATURES.md`,
`CONFIGURATION.md` (`[mouse]` + the selection-bypass table per terminal),
`README.md`, `docs/KEYBINDINGS.md`, `src/ui/help.rs`, `CHANGELOG.md` (via a
well-typed commit subject — the commit *is* the changelog entry).

## Suggested PR split

Each is independently shippable and testable; 1–2 are inert without 3.

1. **Terminal plumbing** — wheel-reporting commands, `MouseConfig`,
   `Effect::SetMouseMode`, `settle_mouse_mode`, all five lifecycle sites,
   `:mouse on|off`. Behavior with `capture = false`: identical to today.
2. **Pure routing** — `src/app/mouse.rs`: snapshot, hit-test, `route_mouse`,
   unit tests. No wiring.
3. **Wire it up** — `Event::Mouse` arm + burst accumulation; list-cursor and
   pager sinks. Wheel works over the list, the pager, and the vsplit preview.
4. **Pane forwarding** — `encode_mouse` + `pane_wants_mouse`. Agent panes scroll
   natively. *This is the headline fix.*
5. **Pane scrollback** — vt100 reverse-scroll for non-mouse children, sticky
   bottom, exit-on-extra-tick-down.

## Verification

### Automated

- `make check` (fmt + clippy + test + deny) — the gate. Not hand-rolled
  `cargo test --all-targets`.
- `route_mouse` table tests over the snapshot matrix, mirroring `route.rs`'s
  regression matrix. Must include: each modal swallows the wheel; pointer over
  the pane while the **list** holds keyboard focus scrolls the pane; a prompt
  wins where it wins for keys; an exited tab doesn't mount anything.
- Hit-test tests against `compute_layout` for pane-open/closed × status
  top/bottom × vsplit on/off. The `status_position = "bottom"` case is the
  off-by-one trap (`FrameLayout.top_unit`'s doc comment calls it out).
- `encode_mouse` byte-exact tests per `MouseProtocolMode` × `Encoding`.
- Config round-trip: `[mouse]` deserializes, defaults hold on an absent section,
  and `--print-config` output still parses.
- Assert the emitted enable sequence contains **no** `?1003h` — that's the
  regression that would reintroduce the redraw storm, and it's invisible in
  manual testing on a still mouse.

### Manual (in spyc, `capture = true`)

1. List focus: wheel moves the cursor `scroll_lines` per tick; a fast flick
   scrolls far, not slowly (burst accumulation).
2. **The headline:** claude in the pane, keyboard focus in the **list**. Wheel
   over the pane scrolls claude's own view; keyboard focus does not move.
3. Shell pane (`zsh`), run `ls -la`; wheel up enters scroll mode and scrolls the
   live screen — no pager, no history cycling, no 40 ms hitch per tick.
4. Wheel down to the bottom, then one more tick → back to live.
5. `vim` in a pane → wheel scrolls vim (it requests mouse mode). `htop` likewise.
6. `^a v` transcript pager open → wheel scrolls the pager.
7. Vsplit: wheel over the `b` preview scrolls the preview, not column `a`.
8. `:mouse off` → selection works again immediately, wheel reverts to 1007
   arrow behavior. `:mouse on` → back. No restart.
9. Selection bypass: Shift-drag (Ghostty/WezTerm/kitty), Option-drag (iTerm2).
10. Quit spyc → wheel scrolls the shell's own scrollback (no leaked `?1000h`).
    Repeat via a forced panic → same.
11. `v` (editor) / `;` (foreground command) round-trip → the child owns the mouse,
    spyc takes it back on return.
12. Over SSH, and inside tmux with `set -g mouse on` — tmux forwards mouse to an
    app that requested it; confirm tmux isn't eating the wheel for its own
    scrollback.
13. Idle with `capture = true` and wave the pointer over the terminal: `A`
    activity overlay must show **no** draws.

## Open decisions

1. **Wheel-up on a non-mouse pane: reverse-scroll the live screen (Tier 4) or
   mount the `^a v` pager?** Plan assumes reverse-scroll — cheaper, no mode
   change, and the pager stays a deliberate `^a v` gesture. Confirm that matches
   your mental model.
2. **Ship `capture` default-off permanently, or flip it once Tier 3 lands?**
   Once agent panes forward natively, capture-on is a large win for the primary
   workflow and the only cost is click-drag selection (which has a modifier
   bypass and a `^a u` alternative). Proposal: ship off, flip in a later minor
   after dog-fooding.

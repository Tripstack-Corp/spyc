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
>
> **Decision: ship `capture = true` — default ON.** Owner call: wheel-scrolling
> the thing under the pointer is the behaviour spyc should have, and `-CURRENT` is
> where a default of this size belongs. The cost is real but bounded, and it has
> three outs: the bypass modifier, `:mouse off` at runtime, and `capture = false`
> in the rc file.
>
> Because it's default-on, **discoverability of the out is now part of the work,
> not a nicety.** The predictable support report is "spyc broke text selection in
> my terminal", from a user who has no reason to connect it to a scroll feature.
> Required, not optional:
> - `?` help (`src/ui/help.rs`) and `FEATURES.md` name `:mouse off` next to the
>   selection caveat.
> - `CONFIGURATION.md` carries the per-terminal bypass-modifier table.
> - The `CHANGELOG.md` entry leads with the selection change, not the scroll
>   feature — this is the line someone greps after their muscle memory breaks.
> - `^a u` quick-select (URLs/paths/SHAs) and `y` in the pager are the
>   mouse-free yank paths to point at.
>
> spyc has daily users (internal engineers), so expect this to generate feedback
> quickly — which is the point of landing it on `-CURRENT` rather than in a
> release.

## Scope: forward to the child, don't reimplement scrolling

The load-bearing observation is that **modern TUIs already scroll themselves**.
claude/codex/vim/htop all manage their own viewport; they simply never receive
mouse events today, because spyc has never enabled capture. So the job is mostly
*plumbing*: turn reporting on, work out what the pointer is over, and hand the
event to whoever owns that region — usually the child, in its own protocol.

That collapses the original plan's back half. spyc-owned pane scrollback and
wheel-burst coalescing are **deferred** (see *Deferred* below) because they exist
only to serve children that don't speak mouse, and they carry the most risk for
the least payoff.

What is emphatically *not* optional, and is where this will break if rushed:

1. **Disabling reporting on every exit path** — leak `?1000h` and the user's
   shell loses click-drag selection until they `reset`.
2. **Gating the forward on what the child asked for** — see Tier 3. Forwarding to
   a child that never enabled mouse mode types escape bytes into its prompt.
3. **Translating coordinates into the pane's own space** — the child believes it
   owns a grid starting at `0,0`.
4. **Keeping a list arm** — enabling real reporting *stops* the terminal's 1007
   wheel→arrow translation, so the file list stops scrolling unless spyc handles
   it. Dropping this would regress behaviour that works today.

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
    pane_closed: bool,
    pane_wants_mouse: bool,     // the child requested mouse reporting (see Tier 3)
}

enum MouseSink {
    Swallow,                    // a modal owns the screen, or the child can't use it
    ListCursor,                 // move the file-list cursor
    Pager,                      // scroll the pager under the pointer
    PaneForward,                // encode + send to the child
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

`ListCursor` is **not** a nice-to-have. Wheel-over-list works today only because
1007 has the *terminal* translate wheel into arrow keys; the moment we enable
1000 that translation stops. Ship without this arm and we trade a pane bug for a
list bug.

`route_mouse` takes the whole `MouseEventKind`, not just wheel — the three button
gestures in Tier 3b are routed by the same pure fn against the same snapshot.
`Drag` / `ScrollLeft` / `ScrollRight` stay no-ops; exhaustive matching keeps
adding them contained.

### Tier 3 — the agent pane: forward to the child (the whole point)

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
3. **The child already does this better.** Per *Scope* above, claude/codex manage
   their own viewport — they just never receive the events.

`vt100::Screen::mouse_protocol_mode()` and `mouse_protocol_encoding()` are
**public** in vt100 0.16 (`screen.rs:578,584`). So spyc can ask the child what it
wants and act accordingly:

| Pointer over pane | Child requested mouse? | Behavior |
|---|---|---|
| yes | yes (`mode != None`) | translate coordinates, encode in the child's protocol/encoding, `send_bytes` — the child scrolls itself |
| yes | no | **swallow** (optionally a one-shot hint pointing at `^a v`) |

This also fixes clicks inside the child for free, and it's the reason to prefer
forwarding over mounting: the child's own scrollback is *better* than ours.

New: `pane::input::encode_mouse(ev, mode, encoding) -> Vec<u8>` — sits beside the
existing `encode_key` (`src/pane/input.rs`), pure, table-testable.

Three requirements that are easy to skip and each produce a *worse-than-broken*
result — mouse that appears to work but misbehaves:

**1. Gate on the child's requested mode.** If `mouse_protocol_mode()` is `None`,
send nothing. This is exactly the bracketed-paste bug fixed in #170, and the
precedent is already in the tree — `Pane::bracketed_paste_enabled`
(`pane/mod.rs:446`), whose own doc spells out the failure: a shell that never
enabled the mode "would take those bytes as literal input." Forward
unconditionally and a plain `sh` prompt fills with `\e[<64;20;5M`.

**2. Translate coordinates into the pane's space.** `MouseEvent.row`/`column` are
**frame-absolute**; the child believes it occupies a grid starting at its own
`0,0` with the pane's dimensions. Subtract the pane rect's origin (from the same
`compute_layout` call the hit-test already made) before encoding. Skip this and
clicks land N rows off — which reads as the *agent's* bug, not ours.

**3. Re-encode; never relay the received bytes.** crossterm has already decoded
the sequence, and spyc asked the terminal for SGR (1006) while the child may have
requested X10 or UTF-8. `encode_mouse` must emit the child's
`mouse_protocol_encoding()`, not ours.

### Tier 3b — button gestures: click-to-focus, middle-paste, right-chord

`1000h` already reports buttons alongside the wheel (Tier 0), so this needs **no
new escape sequences** — only routing. All three are `MouseEventKind::Down(..)`.

#### Left-click — focus the surface under the pointer

Route through the **existing** `set_pane_focus(want_pane: bool)`
(`pane_tabs.rs:686`), never by assigning `state.focus`. It already encodes two
behaviours a fresh implementation would get wrong:

- **Zoom refusal.** While `pane.zoom != ZoomTarget::None` it declines with a hint,
  because the other region is collapsed off-screen. A click landing in a region
  that isn't visible must do the same thing `^a j` does.
- **Vsplit interaction.** From the right column `^a j` descends to the pane but
  *keeps* `vsplit.focus` on the right, so `^a k` climbs back to `b` rather than
  `a`. Clicking pane→list has to restore the same column the user came from.

For a click into the *other* list column, pair it with `vsplit_focus(Side)` — the
existing handler behind `Action::VsplitFocusLeft`/`Right` (`actions.rs:362`).
`recompute_focus` (`pane_tabs.rs:799`) then derives overlay/pager focus from the
`want_pane` bool as usual; nothing new decides focus.

**Click-through, not click-to-focus-then-click.** When the pane is under the
pointer and the child requested mouse, focus it *and* forward the event (Tier 3).
The pane is already live and visible, so swallowing the first click to "just
focus" would feel broken. Consequence: left-click is the child's, which is why the
other two buttons are spyc's.

Deferred within this tier: **click-to-select a list row** (move the cursor to the
clicked line). It needs a row→index mapping that doesn't exist yet — the list
renderer owns the visible window, and there is no `first_visible`/`scroll_offset`
accessor to hit-test against. Focus-only first; add row selection once that
mapping is extracted (and it must respect `list_generation`, wrapped rows, and the
vsplit column widths).

#### Middle-click — paste

Terminals normally paste PRIMARY on middle-click, but capture takes that away, so
spyc has to do it. Route it as a **paste, not as bytes**: synthesize the same path
`Event::Paste` takes (`run.rs:206` → `route_input` → `InputSink::Paste`), so it
lands wherever a paste already lands — pane, `:` command line, shell prompt — and
inherits the bracketed-paste gating from #170 (`bracketed_paste_enabled`,
`pane/mod.rs:446`) for free. Do **not** add a second paste path.

> [!NOTE]
> **This needs a clipboard *read*, which does not exist yet.** `src/clipboard.rs`
> is write-only: `copy` / `copy_image`, with text shelling out (`pbcopy`;
> a candidate list on Linux) and `arboard` used only for images. So Tier 3b adds
> the mirror — `pbpaste` on macOS, `xclip -o` / `wl-paste` on Linux, sibling to
> `spawn_and_pipe`, or `arboard::Clipboard::get_text` if we'd rather not grow the
> shell-out list. Model it as an `Effect` alongside `Effect::CopyToClipboard`
> (`effect.rs:56`) so the pure layer stays clean; note it's a subprocess spawn on
> a user gesture, which matches what yank already does.
>
> X11 PRIMARY-vs-CLIPBOARD: use the regular clipboard. PRIMARY has no macOS
> equivalent, and a gesture that pastes different content per platform is worse
> than one that's merely conventional.

#### Right-click — open a chord menu

The nicest of the three, because it turns the mouse into a *discovery* surface for
a dense keymap: right-click sets a pending chord prefix (`PendingSeq`,
`keymap/resolver/mod.rs:15`) and shows the which-key popup **immediately** — the
`chord_hint_delay_ms` debounce exists to avoid startling a keyboard user mid-chord,
and a deliberate right-click needs no such grace.

Which prefix should follow the **binding taxonomy** the repo already enforces
(AGENTS.md → "Binding taxonomy", guarded by
`leader_and_pane_namespaces_respect_tiers`) rather than being a global choice:

| Pointer over | Prefix | Rationale |
|---|---|---|
| the pane | `^a` (`PendingSeq::W`) | PANE-tier commands live there |
| the list / a column | leader (`PendingSeq::Leader`) | GLOBAL + workspace ops live there |

That way the mouse surfaces the same vocabulary the keyboard does in that region,
and neither menu offers actions the guard would reject for that tier. See **Open
decisions** — this one is worth confirming before implementation.

Right-click is **always spyc's**, never forwarded, even to a mouse-aware child;
otherwise the gesture would be unavailable exactly where the pane is focused. Same
for middle-click. Document both in the selection-caveat block, since a user who
wants the child's own right-click menu needs to know `:mouse off` is the way.

### Configuration

Not a new `[terminal]` section for one bool. A `[mouse]` section, matching the
existing per-feature shape (`layout`, `pane`, `yank`, `pager`, `markdown`,
`delete`, `notify` — `config/mod.rs:34-52`) and leaving room to grow:

```toml
[mouse]
# Real mouse reporting (wheel + click): scroll whatever is under the pointer.
# Breaks native click-drag selection — hold Shift (most terminals) or
# Option/Fn (iTerm2) to bypass, or `:mouse off` to reclaim it for this session.
# TARGET DEFAULT: true. Ships `false` through PR 1 (which has no mouse handling
# yet) and flips in PR 2's last commit, once forwarding works — see PR split.
capture = true
# Lines per wheel tick, for the surfaces spyc scrolls itself (list, pager).
# It does NOT apply to a pane forwarding to its child — the child receives one
# event per tick and decides its own step.
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

## Deferred (deliberately not in v1)

Both of these existed to serve children that don't speak mouse. They're the
highest-risk, lowest-payoff part of the original plan, and forwarding makes them
optional rather than foundational.

### Deferred: spyc-owned pane scrollback

Driving vt100 reverse-scroll (`Pane::scroll_up`) from the wheel for a plain shell
pane. **Consequence of deferring:** wheel over a non-mouse pane does nothing.
Worth stating plainly that this is still better than today, where the 1007 hack
turns the wheel into arrow keys and walks your shell history.

Two notes to keep if it's ever picked up:

- The draft's auto-exit heuristic ("only exit scroll mode if already at bottom
  *before* this event") is the right rule — sticky bottom, one deliberate extra
  tick to leave.
- Don't trust `scroll_down_or_exit`'s name: it only does
  `scroll_offset.saturating_sub(n)` (`pane/mod.rs:533`) and the "or_exit" half is
  vestigial. Pre-existing wart, unrelated to this work.

Mounting the `^a v` pager from the wheel stays rejected outright, for the reasons
in Tier 3: for an alt-screen agent `decide_scroll_source` takes the *transcript*
branch (a 4 MB tail-read + per-line parse + markdown render), and the vt100 branch
stalls the loop `3 × 10 ms` per mount — at ~30 ticks/s that's a hang. The wheel
must never mount anything.

### Deferred: wheel-burst coalescing

Summing same-direction wheel events before applying them. Two reasons this is no
longer urgent: the alarming figure it was defending against (40 ms of main-loop
stall per tick) belonged to the pager-mount idea now rejected outright; and on the
forwarding path the *child* absorbs its own burst, which is what happens in any
terminal app. spyc still marks one redraw per event, but that's the cost of any
keypress and it's bounded by the pane repaint already happening when the child
emits output.

Revisit if dog-fooding shows a trackpad flick over the **list** or a **pager**
(the spyc-owned surfaces, where each tick is a real spyc redraw) feels heavy. The
fix, if needed, is lossless accumulation — sum the ticks, don't drop them like the
existing `view.scroll_last` arrow throttle (`run.rs:183`).

## Files touched

| File | Change |
|---|---|
| `src/lib.rs` | `Enable/DisableWheelReporting` commands; wire into `setup_terminal`, `restore_terminal`, `suspend_tui`, `resume_tui`, **panic hook** |
| `src/config/mod.rs` | `MouseConfig { capture, scroll_lines }` + `DEFAULT_TEMPLATE` block |
| `src/app/mouse.rs` | **new** — `MouseSnapshot` / `MouseSink` / `route_mouse` + hit-test + handler |
| `src/app/mod.rs` | `ViewState.mouse_capture_on` |
| `src/app/run.rs` | `Event::Mouse` arm → `handle_mouse` (thin — the decision lives in `mouse.rs`) |
| `src/app/proc.rs` | drop `Moved`/`Drag` in the forward filter |
| `src/app/effect.rs` | `Effect::SetMouseMode { capture }` + executor arm |
| `src/app/scheduler.rs` *or* loop bottom | `settle_mouse_mode` |
| `src/app/commands.rs` + `command_table.rs` | `:mouse on\|off` |
| `src/pane/input.rs` | `encode_mouse(ev, mode, encoding)` — pane-relative coords, child's encoding |
| `src/pane/mod.rs` | expose the child's `mouse_protocol_mode`/`encoding`, mirroring `bracketed_paste_enabled` |

Docs, same commit (AGENTS.md "Keep docs in sync"): `AGENTS.md` (module index —
guarded by `every_app_module_is_in_the_agents_index`), `FEATURES.md`,
`CONFIGURATION.md` (`[mouse]` + the selection-bypass table per terminal),
`README.md`, `docs/KEYBINDINGS.md`, `src/ui/help.rs`, `CHANGELOG.md` (via a
well-typed commit subject — the commit *is* the changelog entry).

## Suggested PR split

Two PRs. The first is inert by design, so it can land and sit safely.

> [!IMPORTANT]
> **`capture` must ship `false` in PR 1 and flip to `true` at the END of PR 2.**
> Default-on and "PR 1 is inert" are mutually exclusive: PR 1 has no
> `Event::Mouse` arm, and enabling capture also emits `DisableAlternateScroll`
> (Tier 1). So a default-on PR 1 would land a `main` where selection is broken,
> 1007 wheel→arrow is off, and mouse events are dropped on the floor
> (`run.rs:211`) — the wheel would do *nothing at all*, strictly worse than today
> on every axis. Same destination, no broken intermediate: flip the default in the
> last commit of PR 2, once forwarding actually works.

1. **Terminal plumbing + pure routing** — wheel-reporting commands, `MouseConfig`
   (**`capture = false` for now**), `Effect::SetMouseMode`, `settle_mouse_mode` and
   all five lifecycle sites, `:mouse on|off`, plus `src/app/mouse.rs` (snapshot,
   hit-test, `route_mouse`, unit tests). No `Event::Mouse` arm yet, so behaviour is
   **identical to today**, and the routing logic lands fully tested before anything
   depends on it.
2. **Wire it up, then flip the default** — the `Event::Mouse` arm, `encode_mouse`
   (coords + child's encoding), `pane_wants_mouse` gating, and the list/pager
   sinks. This is where it starts working: wheel over an agent pane scrolls the
   agent, wheel over the list moves the cursor, wheel over a pager scrolls it.
   Final commit: `capture` default → `true`, plus the discoverability docs the
   warning above makes mandatory.

If PR 2 wants splitting further, the seam is pane-forwarding vs the
list/pager sinks — but they share the `Event::Mouse` arm, so landing them together
avoids a half-wired intermediate state where the wheel works over one surface and
silently dies over another.

## Verification

### Automated

- `make check` (fmt + clippy + test + deny) — the gate. Not hand-rolled
  `cargo test --all-targets`.
- `route_mouse` table tests over the snapshot matrix, mirroring `route.rs`'s
  regression matrix. Must include: each modal swallows the wheel; pointer over
  the pane while the **list** holds keyboard focus routes to the pane; a prompt
  wins where it wins for keys; **`pane_wants_mouse: false` yields `Swallow`, never
  `PaneForward`** (the #170 class — the one that types garbage into a shell).
- Hit-test tests against `compute_layout` for pane-open/closed × status
  top/bottom × vsplit on/off. The `status_position = "bottom"` case is the
  off-by-one trap (`FrameLayout.top_unit`'s doc comment calls it out).
- `encode_mouse` byte-exact tests per `MouseProtocolMode` × `Encoding`, including
  **coordinate translation**: a click at frame row `R` inside a pane whose rect
  starts at row `Y` must encode as pane row `R - Y`, for both `status_position`
  values (the `FrameLayout.top_unit` off-by-one trap).
- Config round-trip: `[mouse]` deserializes, defaults hold on an absent section,
  and `--print-config` output still parses.
- Assert the emitted enable sequence contains **no** `?1003h` — that's the
  regression that would reintroduce the redraw storm, and it's invisible in
  manual testing on a still mouse.

### Manual (in spyc, `capture = true`)

1. List focus: wheel moves the cursor `scroll_lines` per tick. (Confirms the 1007
   translation loss is covered — this is the arm most likely to be forgotten.)
2. **The headline:** claude in the pane, keyboard focus in the **list**. Wheel
   over the pane scrolls claude's own view; keyboard focus does not move.
3. Shell pane (`zsh`), run `ls -la`, wheel up: nothing happens, and critically **no
   escape garbage appears at the prompt** and no shell history cycling. This is
   the deferred-scrollback case; silence is the correct v1 outcome.
4. Click inside claude (e.g. a menu item) → lands where you actually clicked, not
   offset by the pane's row origin.
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
14. Default-on sanity, on a machine with **no** `.spycrc.toml`: wheel scrolls the
    thing under the pointer out of the box, and `:mouse off` restores selection
    without a restart.
15. Confirm PR 1 in isolation (before the flip) leaves `capture = false` and
    behaves exactly like today — the guard against landing the broken
    intermediate the PR-split note describes.
16. Buttons (Tier 3b): left-click the pane → pane takes focus; left-click a list
    column → that column takes focus, and from the *right* column the pane→list
    return lands back on `b`, not `a`. While `^a z`-zoomed, a click into the
    collapsed region declines with the same hint `^a j` gives.
17. Middle-click pastes into the pane, the `:` line, and the shell prompt — and a
    child that never enabled bracketed paste receives no `\e[200~` wrapper (#170).
18. Right-click over the pane shows the `^a` menu, over the list the leader menu,
    both with **no** `chord_hint_delay_ms` wait. `Esc` dismisses without acting.
19. Right/middle-click over a mouse-aware child (claude) is spyc's, not
    forwarded — confirm the child sees nothing.

## Open decisions

1. ~~**Wheel-up on a non-mouse pane: reverse-scroll or mount the pager?**~~
   **Resolved: neither, in v1.** Mounting is rejected outright (loop stalls,
   transcript hijack); reverse-scroll is deferred. Wheel over a non-mouse pane is
   a no-op — see *Deferred*.
2. ~~**Ship `capture` default-off permanently, or flip it once forwarding lands?**~~
   **Resolved: default ON**, flipped once the feature is ready rather than in a
   later minor — `-CURRENT` is the right place for a default this size. Mechanics
   in the PR split: `false` through PR 1, `true` in PR 2's final commit. The
   discoverability work in *User review required* is part of that commit, not a
   follow-up.

3. **Right-click prefix: taxonomy-aware, or always leader?** Plan proposes
   `^a` over the pane and leader over the list, so each menu offers the tier that
   region actually owns (and can't offer actions the tier guard rejects). The
   simpler alternative is *always* leader — one gesture, one menu, nothing to
   learn — at the cost of making pane commands unreachable by mouse. Worth your
   call; it's a one-line change either way.

Decisions 1 and 2 are settled; 3 is the only open one, and it doesn't block
starting PR 1.

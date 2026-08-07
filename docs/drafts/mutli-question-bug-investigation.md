# Agent pane goes deaf to input after focus round-trip

**Status:** PARKED (2026-08-07) — did not reproduce under deliberate attempt.
**Still open.** It has bitten at least twice in ordinary use, and the reporter's
"probably PEBCAK" is a hypothesis, not a finding. Candidate A is ruled out;
candidate C's underlying defect (spyc ignoring the child's DECCKM) **is fixed**,
but that closes a contract violation, not this bug — nothing yet says C was the
mechanism, and candidate B remains live. See below.
**Observed on:** spyc `v2.1.0-CURRENT (208d3ba)` — predates the #233/#234 mouse
merges, so those are cleared.
**Severity:** high. An agent pane that silently stops accepting input is a
dogfooding stopper, and it has bitten at least twice.

## Symptom

A Claude Code multi-select question is on screen (the tabbed
`AskUserQuestion` UI — several questions across tabs, numbered options, a
preview panel, footer reading `Enter to select · ↑/↓ to navigate · n to add
notes · Tab to switch questions · Esc to cancel`).

Focus leaves the pane — e.g. to another pane — and returns. From then on:

- `↑` / `↓` do nothing
- number keys do nothing
- **`^c` still works** and dismisses the question

No visible response to the dead keys. No which-key popup on screen.

`Esc` was **not** tried before `^c`. That is the single most valuable missing
datum — see "Cheap test" below.

## Ruled out (with evidence)

| Hypothesis | Verdict | Evidence |
|---|---|---|
| Child left in cooked/canonical mode | **Out** | spyc never touches pane termios — no `tcsetattr` / `cfmakeraw` / `set_raw` anywhere under `src/pane/`, `pane_tabs.rs`, `run.rs`. No mechanism exists. |
| Focus-event (DEC 1004) desync | **Out** | spyc doesn't forward focus events to panes at all. `FocusGained`/`FocusLost` appear only in `sources.rs` tests, for host-terminal events. |
| Spurious SIGWINCH on every tab switch | **Out** | `Pane::resize` (`src/pane/mod.rs:299`) early-returns on `(rows, cols) == self.host.last_size`. A tab switch with stable geometry sends nothing. |
| **A. `resolver_pending` latch** | **Out** (2026-08-07) | See below — a latch can eat one key, not deafen. |

## Live candidates

### A. `resolver_pending` latch — RULED OUT

**Verdict (2026-08-07, static analysis):** a latched `pending` can swallow
**exactly one** keystroke, never a sustained sequence. Three independent
reasons:

1. Every pending state resets on an unmatched continuation. The `Leader` arm
   (`src/keymap/resolver/mod.rs:352`) ends `_ => ResolverOutcome::Ignored`
   followed by `self.reset()`; `reset()` appears 11 times across the chord
   arms. The next key clears it.
2. With `pending` true and the pane focused, `route_input` skips every
   `!is_meta` arm and falls through to `InputSink::Resolver` — so the key
   *does* reach `feed`, which resets.
3. The only pre-`feed` swallow path, `is_post_chord_bounce`
   (`src/app/mod.rs:1363`), is bounded to a 60 ms window, the *same* keycode,
   and explicitly requires `!resolver_pending`. It cannot self-sustain.

The one place a latch genuinely persists is behind a **modal overlay pager**
(`route_input` arm 2 returns `PagerKey` even for meta keys) — but that state is
plainly visible on screen and is not what was reported.

The original reasoning is kept below for the record.

---

#### Original reasoning (superseded)


`route_input` → `is_spyc_meta_when_pane_focused` (`src/app/mod.rs:1413`)
opens with:

```rust
if resolver_pending { return true; }
```

Any key arriving while a chord is pending is claimed by spyc and **never
forwarded to the pane**. A stuck `pending` produces exactly the reported
symptom: arrows and digits dead, no visible feedback, because the resolver
silently eats unmatched continuations.

Weighted heavily because the codebase **already documents this failure
class** at `src/app/mouse.rs:444`:

> `enter_leader()` sets a pending chord, but while prompting the next key
> goes to `handle_prompt_key`, which never feeds the resolver and never
> calls `clear_chord_hint` — so `pending` latches ... and the first key
> after the prompt closes is eaten as a leader continuation. The same latch
> is reachable with no prompt at all, via a focused full-frame pager, which
> is why this is a snapshot field rather than a check on `Mode`.

Two escape routes were found and guarded (`is_prompting`, focused full-frame
pager). That comment concedes the class was never exhaustively enumerated.
"Focus left the pane and came back" is a plausible third route.

Right-click is one way in: `MouseSink::LeaderMenu` → `enter_leader()`
(`src/app/mouse.rs:778`). Relevant if pane switching is ever done by
clicking.

**Unexplained by A:** `^c` working. Under a latch `^c` is meta too. Survives
only if the first `^c` clears the chord and a second reaches the child —
plausible and easy not to notice, but unverified.

### C. spyc ignored the child's cursor-key mode (DECCKM) — **defect fixed; candidate unproven**

**The encoding defect is fixed as of this commit.** `encode_key` now takes the
pane's DECCKM state (`Pane::application_cursor`), so unmodified arrows / Home /
End go out as SS3 (`ESC O A`) to a child that set `ESC[?1h` and as CSI to every
other child; modified arrows stay CSI in both modes, per xterm. Fixed on its own
merits as a contract violation — **not** as a C1 remedy. Whether it was C1's
mechanism is still an open question, answerable only by field evidence (see
"What would falsify C" below).

The description that follows is of the pre-fix code, kept because it is what the
next recurrence has to be read against.

`vt100` tracks whether the child requested application cursor keys, and spyc
**never read it**. `encode_key` emitted the CSI form unconditionally
(`src/pane/input.rs`, asserted by its own test: `encode_key(Up) == b"\x1b[A"`).

Confirmed empirically (throwaway probe, not committed):

```
PROBE application_cursor=true spyc_sends="\u{1b}[A"
```

i.e. after the child sends `\x1b[?1h`, vt100 reports application-cursor mode
and spyc still sends `ESC [ A` where the child is waiting for `ESC O A`.

**Why this fits the report better than anything else:** it predicts the exact
asymmetry observed. Arrow keys are mode-dependent and would be silently
dropped by a strict parser; `^c` is a bare `0x03` byte, mode-independent, and
keeps working. No other candidate explains why *only* the arrows died.

It also explains why this is intermittent rather than constant: most TUI
parsers accept both forms, so the gap is normally benign. It only bites a
child that (a) sets DECCKM and (b) matches strictly.

**What is still unproven:** that Claude Code's Ink UI enters application-cursor
mode at all, and if so what triggers it. A re-render — plausibly the SIGWINCH
in candidate B — re-initialising the terminal is the obvious suspect, which
would make B and C one mechanism rather than two: *B is the trigger, C is the
reason the keys die.*

**What would falsify C as C1's explanation:** a recurrence of the deafness on a
build carrying this fix, whose `SPYC_KEY_TRACE` shows `app_cursor=` and the
emitted `bytes=` **agreeing** — `app_cursor=true` with `bytes="^[OA"`, or
`app_cursor=false` with `bytes="^[[A"`. That is spyc sending exactly what the
child declared it wants, and the keys dying anyway: C is then not the mechanism,
and **B is back in front** (it was never ruled out, and B-as-trigger /
C-as-mechanism was only ever one of the two readings).

Confirmation is the mirror image and needs the field, not the code: the deafness
simply stops recurring. Absence of evidence accrues slowly here — C1 has bitten
twice in months of daily use, so a few quiet weeks prove nothing.

### B. Ink drops its input handler on re-render

Only the **active** tab is resized (`src/app/render/mod.rs:622`,
`tabs.active_mut().resize(...)`). A tab that was inactive across a layout
change therefore takes a genuine resize — and SIGWINCH — on return. If Ink's
multi-select component loses its stdin handler on that re-render, input dies.

**Fits `^c` naturally:** Ink wires ctrl-c separately from component input.

A and B make opposite predictions, which is what makes the test below
decisive.

## Cheap test (do this first, next occurrence)

**Press `Esc` before `^c`.** Still worth one second, but note A is now ruled
out, so this discriminates less than it did: Esc restoring input would mean the
latch analysis above is wrong somewhere, which is itself worth knowing.

The higher-value observation is now: **do the arrows die while `^c` still
works?** That asymmetry is candidate C's signature. If *everything* is dead
including `^c`, C is wrong and B (or something unlisted) is in play.

With the DECCKM fix in, the asymmetry no longer points at C on its own — a
child that ignores a correctly-encoded arrow (candidate B) produces the same
shape. Only the trace separates them; capture it.

## Decisive test

`--key-trace` (or `SPYC_KEY_TRACE=1`) already logs both directions, and the
RX line records precisely the two variables in question
(`src/app/key_dispatch/mod.rs:132`):

```
RX kind=… code=Up mods=… pane_focused=true pending=Some("…")
```

with `send_key … bytes=` on forward (`src/pane/mod.rs:318`).

Reproduce with the trace armed, then read it:

- `↑` logged as RX with **no** following `send_key` → **spyc is swallowing
  it**. Ours; fully fixable. Check whether `pending=` is non-`None`.
- `send_key … bytes=` present, and the form **matches** `app_cursor=`
  (`true`⇒`^[OA`, `false`⇒`^[[A`) → **spyc sent what the child asked for and
  Claude ignored it**. Ink-side; we work around rather than fix. This is also
  what falsifies candidate C.
- `send_key … bytes=` present but the form **contradicts** `app_cursor=` → the
  DECCKM fix regressed. Ours, and the test to look at is
  `cursor_keys_follow_the_childs_declared_mode`.

Trace lands 0600 in the XDG state dir as `spyc-key-trace-<ts>.log`.

## Reproduction attempt — 2026-08-07 (negative)

| | |
|---|---|
| Terminal | macOS, iTerm2-family host; second spyc instance in a new window |
| Binary | instrumented release build off `edc27b8` (adds `app_cursor=` to the trace's send line) |
| Trace | `SPYC_KEY_TRACE=1`, armed and confirmed writing |
| Working dir | `~/src/primes_research` (deliberately a different project from the running spyc, so no MCP takeover) |
| Panes | Claude agent pane + a plain `zsh` pane |
| State | multi-question `AskUserQuestion` on screen, left unanswered |
| Focus round-trip | pane switch out and back, repeated |
| Result | **did not reproduce** — input stayed live |

Useful negative. Two things it did establish:

- The trace independently confirmed candidate A's reset behaviour live: `g`
  armed `pending=Some("g-")`, the next unmatched key resolved `Ignored` and
  cleared it to `None`. A chord latch really does die after one key.
- Whatever the trigger is, it is **not** simply "unanswered question + pane
  switch." Something else in the original sessions differed — a longer-lived
  pane, a geometry change, a specific switch method, or a state this attempt
  did not recreate.

**Not yet tried, for whoever picks this up:** switching via mouse click rather
than keyboard; a `^a z` zoom toggle between switches (the geometry change is
what makes the resize real, and a real resize is candidate B's trigger); a pane
left idle for much longer; and the original conditions — a long-running Claude
session in the reporter's main project rather than a fresh one.

The instrumented trace field (`app_cursor=`) is now on `main`, so the next
occurrence yields a decisive answer without rebuilding anything.

## Repro recipe

1. Launch spyc with the trace armed: `SPYC_KEY_TRACE=1 spyc`
2. Open a Claude Code pane and a second pane (`^a c`, or a shell tab).
3. In the Claude pane, drive it to a multi-question `AskUserQuestion` —
   see the simulation prompt below.
4. With the question on screen, switch to the other pane and back.
5. Press `↑`, `↓`, `2`.
6. Press `Esc`. Note whether input returns.
7. `^c` if still stuck. Note whether one press or two were needed.
8. Capture the trace file.

Vary step 4 between **keyboard** switching (`^a k` then back, or `^a` + tab
number) and **mouse** switching (click into the other pane, click back). If
only the mouse path reproduces, that implicates the right-click/`enter_leader`
route directly.

## Simulation prompt

The hard part of the repro is getting Claude Code to render a *multi-question*
`AskUserQuestion` with previews. It emits that shape when a request has
several genuinely orthogonal unresolved decisions whose answers change what
gets built, and it's told to settle them before writing anything.

Paste into a Claude Code pane, in any repo:

> I want to add a CLI subcommand that exports this project's config to a
> file. Before you write any code, I need you to settle the design choices
> with me — don't pick defaults and don't start implementing.
>
> Ask me about all of these at once, as separate questions with concrete
> option previews so I can compare them side by side:
>
> 1. **Output format** — JSON, TOML, or YAML. Show me a sample of what the
>    exported file would actually look like in each.
> 2. **Destination** — stdout, a fixed path, or a `--out` flag.
> 3. **Secret handling** — redact, omit, or include with a warning banner.
> 4. **Failure mode** on an unreadable config — hard error, or emit partial
>    output with warnings on stderr.
>
> Give each option a short label and a preview block. Wait for all four
> answers before doing anything else.

Four orthogonal axes, an explicit request for side-by-side previews, and an
explicit instruction not to act make the tabbed multi-question UI the natural
response. If a run comes back with a single question or plain prose, add a
fifth axis and re-run — the tabbed layout appears once there are several
questions in flight.

Leave the question **unanswered** on screen; that's the state the bug needs.

## Fix shapes (do not pick before measuring)

- **If A:** audit every path that can set `pending` without a resolver-fed
  follow-up, the way `is_prompting` and the full-frame-pager cases already
  were. Likely a snapshot field plus a guard, mirroring the existing two.
  Consider whether a focus change into a pane should reset the resolver
  outright — a pending chord has no meaning once the pane owns the keyboard.
- **If B:** suppress the redundant resize on tab re-activation, or track
  `last_size` per tab so a returning tab isn't handed a size change it
  doesn't need. The Ink bug isn't ours to fix, but the trigger is ours to
  withhold.
- **If C:** already done — `encode_key` honors the pane's DECCKM. Nothing more
  to build; the open work is confirming or falsifying it from the field.

These are entirely different changes. Picking wrong costs a PR.

# Reviewer B — app interaction layer (`src/app`: `mouse/`, selection, scroll)

Subject: `git diff v2.0.0..HEAD` (HEAD `f8dedae`, `2.1.0-CURRENT`) restricted to the
mouse / selection / scroll interaction rework in `src/app`.
Read-only review. No code was changed.

## Verdict

The mouse decomposition is genuinely good work: the F4 split (#244–#246) produced clean
seams, every child module reaches `App` through the descendant rule rather than a
cross-module reach-in, the pure/impure boundary held under four subsequent feature PRs,
and the terminal-state reconcile (`settle_mouse_mode`, 1002-not-1003, the reader-thread
`Moved` filter before the channel send) is careful enough to preserve the 0-dps-at-idle
invariant. There is **no blocker**. Two things did erode under the pressure of landing
fast: the render-purity contract now has a real draw-pass mutation that the purity guard
is structurally blind to, and both mouse plan documents were abandoned mid-campaign —
`native_scroll_plan.md` has not been touched since **#211**, before a single line of the
feature shipped, and 24 implementation PRs later it still records as "rejected" and
"deferred" things that shipped, and as "Files touched" a binding surface that never did.
The one substantive correctness bug I found is `chrome_col_at`'s missing right-hand bound,
which mis-routes left-clicks in a full-height vertical split — the default split mode when
no pane is open.

---

## Findings

### 1 — `chrome_col_at` bounds only three of four edges; a full-height vsplit's right column is swallowed by the status bar

**Severity: medium**
**File:** `src/app/mouse/selection.rs:122-126` (predicate), `src/app/mouse/mod.rs:146-151`
(the record type), `src/app/mouse/route.rs:484-491` (the precedence that consumes it)
**Introduced:** `84d2f28` (#234), widened by `6fab158` (#281)

`ChromeRow` records only `y` and `x` — no width:

```rust
pub struct ChromeRow { pub y: u16, pub x: u16, pub line: Line<'static> }
```

and the hit-test matches on the row plus a *lower* column bound only:

```rust
let row = rows.iter().find(|r| r.y == ev.row && ev.column >= r.x)?;
```

`route_mouse`'s left-press arm tests `over_chrome_row` **before** the region
(`route.rs:489`), deliberately, so chrome painted over the layout wins. That is correct
for the activity HUD. It is wrong here, because in `VsplitMode::FullHeight` the status and
prompt rects are width-clamped to the left column while the right column spans the whole
frame:

- `src/app/render/mod.rs:383-385` — `out.status.width/list.width/prompt.width = …min(left_w)`
- `src/app/render/mod.rs:399-404` — `out.right = Rect { x: right_x, y: area.y, …, height: area.height }`

The status `ChromeRow` therefore has `x == 0` and matches *every* column on row 0,
including columns 40–79 which belong to column `b`. Snapshot proof that the two genuinely
share row 0:
`src/app/render/snapshots/spyc__app__render__render_tests__snapshot_frame_vsplit_preview_full_height.snap`
— row 0 reads `🌶️ … hidd│┌  preview.md …`.

Consequences in a full-height split (`^a |` twice, or the default when no pane is open —
`src/app/vsplit.rs:80-84`):

- Left-clicking the **first row of column `b`** does not focus the column or select the
  row; it anchors a status-bar text selection.
- Dragging from there and releasing copies status-bar columns to the clipboard, silently
  replacing its contents, from a gesture the user meant as a row selection.
- The same applies to the bottom row of column `b` whenever a flash / `:` line / armed
  chord is on the prompt row (`render/inner.rs:341,357,365`).

The contrast is self-indicting: `pager_slot_at` (`src/app/pager_handler/mod.rs:411-419`)
bounds both axes correctly, and `route.rs:169-173` names it as the model this precedence
copies.

**A fix would need to** carry the drawn width on `ChromeRow` and bound the search on
`ev.column < r.x + r.width` (the width is already in hand at every `draw_chrome_line` call
site — it is `area.width`). Coverage gap to close alongside it: the two chrome tests
(`selection.rs:764,808`) fabricate rows at `x: 0` and never press outside a row's extent,
and no `route.rs` case combines `over_chrome_row: true` with `Region::RightColumn`.

### 2 — the `&self` draw pass mutates `ViewState` through a `RefCell`, and the purity guard cannot see it

**Severity: medium**
**File:** `src/app/render/inner.rs:434-457`; field at `src/app/mod.rs:976`; cleared at
`src/app/render/mod.rs:424`
**Introduced:** `84d2f28` (#234), extended by `6fab158` (#281)

`draw_chrome_line` takes `&self` and writes state the input path later reads:

```rust
pub(in crate::app) fn draw_chrome_line(&self, frame: &mut Frame, area: Rect, line: Line<'static>) {
    self.view.chrome_rows.borrow_mut().push(crate::app::mouse::ChromeRow { … });
```

AGENTS.md → MVU invariants: *"Render is pure (`&self`). Draw reads, never mutates;
pre-frame settling goes in `prepare_*`."* This is the only interior-mutability write in
the three `PURE_DRAW` modules and it is new since v2.0.0 (`git show
v2.0.0:src/app/render/inner.rs` has zero `borrow_mut` / `.set(`).

The guard did not catch it because it only scans for OS tokens
(`src/app/render/mod.rs:1233-1241`: `thread::spawn`, `std::fs::`, `read_to_string`,
`env::var`, `Command::new`, `process::id`). Nothing in `FORBIDDEN` describes the
*mutation* half of the contract, so the invariant's mechanical enforcement covers exactly
the 2026-06 violations it was written for and nothing about the shape the new code took.
That is the charter's "still covers the new code's shape rather than just its old scan
targets" question, answered: it does not.

I am not claiming the recording is unreasonable — reading exactly what was painted is what
makes the copy match the screen including truncation. The finding is that the contract now
has an unguarded exception, and the next such write will land as silently as this one did.

**A fix would need to** either (a) hoist the recording into `prepare_frame` (`&mut`), at
the cost of the "what got drawn" fidelity, or (b) accept it explicitly — document the
carve-out at the `chrome_rows` field and add mutation tokens (`borrow_mut()`, `.set(`,
`.replace(`) to `FORBIDDEN` with `chrome_rows` as the single named allowance, so the *next*
one still fails the build.

### 3 — `[mouse] invert_scroll` does not reach a mouse-aware child; the config example says it applies "everywhere"

**Severity: medium**
**Files:** `src/app/mouse/mod.rs:42-67` (the flip), `src/app/mouse/forward.rs:158-160`
(the path that skips it), `CONFIGURATION.md:253`
**Introduced:** `9e9179d` (#260)

The flip is correctly applied at exactly one decision point and guarded by a test that up
and down stay exact opposites (`mouse/mod.rs:403-417`). But `MouseSink::PaneForward` /
`FocusAndForward` never see that delta — `mouse_report` maps the raw event kind:

```rust
MouseEventKind::ScrollUp   => (64, false, false),
MouseEventKind::ScrollDown => (65, false, false),
```

So with `invert_scroll = true` the file list, the pagers and agy's synthesized keys invert,
while claude / vim / htop / any mouse-aware child scrolls the original direction. That is
the primary dog-fooding surface, and it is the exact user who reaches for this knob ("the
wheel reads backwards on my machine") who gets a half-inverted spyc.

The prose in `CONFIGURATION.md:279-289` and `src/config/default.spycrc.toml:141-148` is
accurate (it names the three surfaces). The inline TOML comment users actually copy is not:
`invert_scroll = false # true flips the wheel direction everywhere`.

**A fix would need to** decide the contract, then make one of the two true: either swap
64↔65 in `mouse_report` when `invert_scroll` is set (the config would have to be threaded
into `forward_to_child`), or narrow `CONFIGURATION.md:253` to match the prose and the
`scroll_lines` precedent, which already states that a forwarding pane is unaffected
(`src/config/mod.rs:341-343`).

### 4 — a wheel tick can reach a 30 ms main-loop sleep, which the plan ruled out in writing

**Severity: medium**
**Path:** `src/app/mouse/scroll.rs:129-132` → `src/app/pane_scroll.rs:160`, sleeping at
`src/app/pane_scroll.rs:273-277`
**Plan text contradicted:** `docs/drafts/native_scroll_plan.md:504-508`

With `[mouse] pane_scroll_view = "spyc_history"`, `decide_agent_view_action` returns
`AgentViewAction::UseSpycHistory` and the impure half calls `self.open_pane_scroll_pager()`
straight from the wheel handler. On the vt100 branch that function does:

```rust
for _ in 0..3 { active.drain_output(); std::thread::sleep(Duration::from_millis(10)); }
```

— 30 ms of main-loop stall from an input event, and if scrollback turns out to be empty it
flashes and mounts nothing (`pane_scroll.rs:290-301`), so the *next* qualifying tick pays
it again. The plan rejected precisely this:

> Mounting the `^a v` pager from the wheel stays rejected outright … the vt100 branch
> stalls the loop `3 × 10 ms` per mount — at ~30 ticks/s that's a hang. The wheel must
> never mount anything.

Reachability is real but not default: `pane_scroll_view` defaults to `native`
(`src/config/mod.rs:322-323`), the gate needs three consecutive up-ticks
(`route.rs:249,380`), and codex normally takes the off-thread `Transcript` branch — so the
stall needs `spyc_history` **plus** codex's transcript toggled off, or an alt-screen-free
agent with no transcript. The `!is_open` gate does not self-limit here either: `is_open` is
scraped for the *agent's* marker, which mounting spyc's own pager never sets. What does
limit it is that once spyc's pager is mounted, `covering_pager` routes the wheel to it.

Whether this is "the plan changed its mind, correctly" or "the plan's own reasoning was
overlooked" cannot be told from the tree, because the plan was never amended — which is
finding 5.

**A fix would need to** either move the drain-and-settle off the input path (the
`spawn_pager_stream` template is already in the same function) or gate `UseSpycHistory` on
the transcript branch only.

### 5 — plan divergence without doc update: mouse button bindings were specified in detail and shipped nowhere

**Severity: medium (docs contract)**
**Plan:** `docs/drafts/native_scroll_plan.md:398-459`, `:544`, `:585-593`
**Tree:** `grep -rn 'LeftClick\|MiddleClick\|RightClick' src/` returns nothing

The plan devotes a titled subsection ("Mouse bindings — customizable, including Lua"),
three scope calls, a `Trust::Project` RCE warning marked "must be **tested**, not assumed",
a "Files touched" row for `src/config/dsl.rs`, and half of PR 3's stated scope ("Buttons +
bindings, then flip") to `<LeftClick>` / `<MiddleClick>` / `<RightClick>` DSL tokens.
None of it exists. The three gestures are hard-wired in `route_mouse`
(`route.rs:458-468`) with no rebinding surface and no `unmap`.

Dropping it may well be right — the gestures are few and the grammar cost real. The finding
is that the plan still reads as the specification for what shipped, so the next reader
(human or agent) will believe `map <RightClick> command graveyard` works. `native_scroll_plan.md`'s
last commit is `6ba251f` (#211) — **before** `359f3a6` (#212), the first implementation PR.
Twenty-four mouse PRs landed against an unamended plan.

**A fix would need to** add a "not shipped / withdrawn" note to the subsection, or file it
as an issue and link it from there.

### 6 — two doc comments mis-merged at the pre-split source, carried verbatim through F4

**Severity: low**
**Files:** `src/app/mouse/route.rs:202-227`; `src/app/mouse/forward.rs:88-101`
**Introduced:** `84d2f28` (#234) and `7b3a551` (#229) respectively; relocated unchanged by
`6643a56` (#244) / `edc27b8` (#246)

In `route.rs`, the doc block opened for `resolver_will_see_the_next_key` runs eleven lines
and then, at line 214, restarts mid-block with `/// Whether spyc should do the selecting
over the pane itself.` — `pane_is_selectable`'s doc, spliced into the tail of its
neighbour's. The result: `pane_is_selectable` (line 220) carries both docs and
`resolver_will_see_the_next_key` (line 224) carries none, despite being the subtler of the
two (it is what stops a right-click from latching a chord that moves PROJECT_HOME).

`forward.rs` has the identical shape: lines 88-94 document `focus_region`, line 95 restarts
with `focus_pager_slot`'s doc, `focus_pager_slot` (line 102) gets both and `focus_region`
(line 117) gets none.

**A fix would need to** split each block back onto its own item. Behaviour-neutral.

### 7 — comments in the hottest module assert the opposite of what the code does

**Severity: low**
**Files:** `src/app/mouse/mod.rs:54-59`, `:233-238`; also `src/lib.rs:868`

Both copies of the comment say:

> spyc asks the terminal only for 1000 (press/release), so `Moved`/`Drag` shouldn't arrive
> at all — `proc.rs` filters them for the terminals that send them anyway. Consequence,
> deliberate: click-drag selection INSIDE a child doesn't work.

All three claims are false on HEAD. `lib.rs:532` emits `?1000h?1002h?1006h`; `proc.rs:117`
forwards `Drag` and drops only `Moved`, with its own comment explaining why; and
`forward.rs:53-63` exists specifically to deliver drags to the child so its selection works
(#224/#234). `src/lib.rs:868`'s summary line still reads "real mouse reporting
(`?1000h?1006h`)" while the body it documents emits 1002 as well.

AGENTS.md: *"Comments state what IS, not what's planned … they rot into lies."* These have.
The `comments_carry_no_reasoning_leakage` guard does not detect a factually stale comment.

**A fix would need to** delete the two stale paragraphs (the surviving `return None` arm is
self-explanatory) and update `lib.rs:868`.

### 8 — `mouse/tab_hit.rs` is absent from the AGENTS.md module index, and the guard cannot see it

**Severity: low**
**Files:** `AGENTS.md:64`; guard at `src/app/mod_tests.rs:123-149`

The `mouse/` bullet in AGENTS.md names `route.rs`, `selection.rs`, `scroll.rs`,
`forward.rs` and `mod.rs` individually. `tab_hit.rs` (218 lines, the divider tab-bar
geometry that `render/chrome.rs:104` also consumes) is not mentioned. It was added by
`309f838` (#279) after the index bullet was written.

`every_app_module_is_in_the_agents_index` reads `read_dir(root.join("src/app"))` and
`continue`s on anything without a `.rs` extension (`mod_tests.rs:127-133`), so subdirectory
modules are never checked — the guard fails open for exactly the subdirectory the campaign
created. That is a premise correction for the orchestrator's note as well as a finding.

**A fix would need to** add the bullet, and (separately, and worth more) decide whether the
guard should recurse — `src/app/mouse/`, `src/app/render/`, `src/app/state/`,
`src/app/key_dispatch/`, `src/app/pager_handler/` and `src/app/harness_tests/` are all
currently outside it.

### 9 — `docs/KEYBINDINGS.md` and `?` help carry none of the new gestures

**Severity: low**
**Files:** `docs/KEYBINDINGS.md` (no match for `mouse`/`click`/`wheel`/`drag`),
`src/ui/help.rs:280-281`

`?` help lists `:mouse on|off|auto` as a command and nothing else. `docs/KEYBINDINGS.md` —
which AGENTS.md names as "the keymap reference — mirrors `?`" and lists among the
must-update-in-the-same-commit surfaces — has zero mouse content: not click-to-focus,
middle-click paste, right-click leader, drag-to-select on any of the four surfaces,
Ctrl-drag for absolute paths, or click-a-tab-to-switch.

`FEATURES.md:676-712` is thorough and accurate, so the information exists — it is just not
where a user pressing `?` or grepping the keymap reference will find it.
`mouse_selection_plan.md:220-222` made one of these explicit: *"Use Ctrl or Alt, and state
it in `FEATURES.md` + `docs/KEYBINDINGS.md`."* Half done.

### 10 — `selection_auto_copy` was a resolved owner decision and was not implemented

**Severity: low (docs contract)**
**Plan:** `docs/drafts/mouse_selection_plan.md:255-273` — "RESOLVED … Auto-copy on release,
**with a config toggle to disable it**. Owner decision … someone who dislikes having their
clipboard overwritten by a stray drag must be able to turn it off. Default `true`."

`grep -rn selection_auto_copy src/` finds nothing. Every `finish_*` in
`mouse/selection.rs` returns a clipboard effect unconditionally. Given finding 1 — where an
ordinary click-drag in a split column silently overwrites the clipboard with status-bar
text — the missing opt-out is not purely theoretical.

### 11 — the two committed plans contradict each other on list-row selection, and the older one was never amended

**Severity: low (docs contract)**
**Files:** `docs/drafts/native_scroll_plan.md:308-318` vs
`docs/drafts/mouse_selection_plan.md:148-160`; code at `src/app/mouse/selection.rs:301-332`

`native_scroll_plan.md` records, as a decided owner call:

> **Click-to-select a list row is out of scope — not deferred, rejected.** … a near-miss
> silently moves the cursor and loses the user's place.

`begin_row_selection` does exactly the rejected thing, including the named failure mode:
`self.state.col_mut(side).cursor.index = idx;` on press. The reversal *is* recorded — in the
other plan's "Owner spec (2026-08-05) — three surfaces", which puts the file list back in
scope. But the first plan was never amended, so the repository now holds two committed
plans that disagree, and the one that reads as authoritative on routing is the stale one.

Related, same class: `mouse_selection_plan.md:232-247` still lists "Selection stability
under live output" as an **open decision** recommending option (b) freeze-on-drag. The tree
shipped option (a) clear-on-output (`src/app/streaming.rs:410-427`), with a good code
comment naming the tradeoff and citing the plan — but the plan itself still recommends
otherwise.

### 12 — a lost button release strands the drag claim, diverting later drags from the child

**Severity: low**
**Files:** `src/app/key_dispatch/mod.rs:131-133`, `src/app/mouse/mod.rs:183-208`

`handle_key` retires the three selection *payloads* (`list_selection`, `pane_selection`,
`chrome_selection`) but not the *claim* (`view.mouse_selection`). More to the point, the
claim is only cleared by a release or a subsequent button press. Terminals commonly do not
report a release that happens outside the window, so a drag that ends off-screen leaves
`mouse_selection = Some(..)` indefinitely — and that field is checked first in
`handle_mouse`, so every subsequent `Drag` is routed to a no-op `extend_*` instead of
`forward_drag`. Claude's own drag-selection is silently dead until the user clicks once
anywhere (a `Down` clears the claim and falls through).

Self-healing in one click, hence low. **A fix would need to** clear `mouse_selection`
alongside the payloads in `handle_key`, and/or treat a `Drag` whose target's payload is
`None` as a lost gesture.

### 13 — middle-click can block the loop on a clipboard helper with no timeout

**Severity: low**
**Files:** `src/app/effect.rs:722`, `src/clipboard.rs:118-158`

`Effect::PasteFromClipboard` calls `clipboard::paste()` synchronously in `run_effects`,
which spawns `pbpaste` / `wl-paste -n` / `xclip -o` / `xsel -ob` and waits with no timeout.
`xclip -o` against a dead X11 forward (a stale SSH session — a documented spyc use case)
does not return. The plan anticipated the shape ("a subprocess spawn on a user gesture,
which matches what yank already does", `native_scroll_plan.md:336-337`), and the executor
is the correct home for OS work, so this is not an MVU violation. What is new is the
trigger: with `capture = true` by default, middle-click is an easy accident, where `y` was
always deliberate.

### 14 — informational: `route.rs`'s stale section banner

**Severity: informational**
**File:** `src/app/mouse/route.rs:604`

`// ── wiring: the impure half ───` survives the split. Everything below it
(`clamp_to_area`, then the test module) is pure; the impure half moved to `forward.rs` /
`selection.rs` / `scroll.rs`. The banner now labels a half that is not in this file.

### 15 — informational: `route.rs` size trajectory against its own decomposition target

**Severity: informational**
**File:** `src/app/mouse/route.rs` — 2035 total, 621 production

`docs/drafts/mouse-rs-decomposition-proposal.md:60-64` (uncommitted working-tree draft)
budgeted `route.rs` at ~400 production lines. It landed above that and then absorbed every
subsequent decision: #264 (`decide_agent_view_action` open gate), #279 (`tab_under_pointer`),
#281 (`over_chrome_row`), #292 (`PendingViewIntent` / `pending_view_confirmed`). At 621
production lines it is at 78% of the 800-line convention and is the file that grows every
time a gesture is added — while its test half (1413 lines, 69% of the file) is what makes
`wc -l` alarming. Noted, not judged: `tab_hit.rs` was extracted rather than inlined, which
is the right instinct being exercised. The next decision to land here is the one to watch.

### 16 — informational: per-frame `Line` clone per chrome row

**File:** `src/app/render/inner.rs:447` — `line: line.clone()`. Three to five clones per
frame (status, divider, prompt, HUD, wrapped prompt rows). Negligible at spyc's frame rate;
recorded only because it is the cost side of finding 2's design.

---

## File-size trajectory — `src/app` (v2.0.0 → HEAD)

Total lines. The "pre-test" column is the line number of the first `#[cfg(test)]` and is a
*proxy* for the production half — it is wrong for files carrying a mid-file
`#[cfg(test)] mod x;` declaration (marked †; this is the exact fail-open AGENTS.md warns
about for source-scan guards, so treat those as "unknown, not small").

| File | v2.0.0 | HEAD | Δ | pre-test (proxy) |
|---|---:|---:|---:|---:|
| `harness_tests/archive.rs` | — | 3181 | new | test-only |
| **`mouse/route.rs`** | — | **2035** | new | 621 |
| `archive.rs` | — | 1508 | new | — |
| `mod.rs` (ceiling 1500) | 1367 | 1448 | +81 | 266 † |
| `effect.rs` | 1130 | 1397 | +267 | 1100 |
| `agent_status.rs` | 1229 | 1357 | +128 | 744 † |
| `render/mod.rs` | 1227 | 1284 | +57 | 824 |
| `state/tests/mod.rs` | 1047 | 1265 | +218 | test-only |
| `archive_route.rs` | — | 1204 | new | — |
| `pane_tabs.rs` | 1003 | 1154 | +151 | 1103 |
| `state/mod.rs` | 1021 | 1133 | +112 | — |
| `commands.rs` | 852 | 1045 | +193 | 979 |
| `pager_handler/mod.rs` | 899 | 1040 | +141 | 1040 |
| `run.rs` | 893 | 956 | +63 | 956 |
| `key_dispatch/mod.rs` | 829 | 925 | +96 | 791 |
| `route.rs` (keyboard) | 809 | 904 | +95 | 289 † |
| **`mouse/selection.rs`** | — | **837** | new | 399 |
| `render/overlays.rs` | 679 | 835 | +156 | 796 |
| `render/chrome.rs` | 646 | 712 | +66 | 635 |
| `render/inner.rs` | 577 | 694 | +117 | 694 |
| `mouse/mod.rs` | — | 454 | new | 368 |
| `mouse/tab_hit.rs` | — | 218 | new | 107 |
| `mouse/scroll.rs` | — | 203 | new | 203 |
| `mouse_mode.rs` | — | 198 | new | 61 |
| `mouse/forward.rs` | — | 192 | new | 192 |

Trajectory read: the mouse subsystem itself is the *best*-factored new code in the diff —
four of its six files are under 250 lines and the largest production half is 621. The
files that crossed 800 in this window without a decomposition of their own are
`key_dispatch/mod.rs` (829→925) and `route.rs` (809→904); `pane_tabs.rs`,
`pager_handler/mod.rs` and `commands.rs` were already over and grew further. `mod.rs` is at
96.5% of its guarded ceiling.

---

## Premises checked

| Premise (charter / orchestrator) | Verdict |
|---|---|
| "The `mouse.natural_scroll` toggle" | **Wrong on HEAD.** The key is `[mouse] invert_scroll`. `natural_scroll` was considered and rejected by name, with the rationale in the field doc (`src/config/mod.rs:349-360`) and in `CONFIGURATION.md:284-287`: which direction is "natural" depends on the OS trackpad setting and the terminal, so the flag would be ambiguous exactly where a user reaches for it. Substance of the question still answered — see finding 3. |
| "`src/app/mouse/` is `mod.rs`, `route.rs`, `selection.rs`, `scroll.rs`, `forward.rs`" | **Incomplete.** Six files; `tab_hit.rs` is the sixth (finding 8). |
| "Check whether `mod_rs_stays_decomposed`'s ceiling was bumped since v2.0.0" | **Not bumped.** `CEILING = 1_500` at both v2.0.0 and HEAD (`src/app/mod_tests.rs:26`). `mod.rs` grew 1367→1448 (96.5% of ceiling) — the ViewState fields for the four selection surfaces landed there, which AGENTS.md allows (type defs). No violation; worth watching. |
| "A new render module absent from `PURE_DRAW` is silently unguarded" | **No new render module landed** — `src/app/render/` is still `mod`/`inner`/`chrome`/`overlays`, all three impure-scanned ones present in `PURE_DRAW` (`render/mod.rs:1224-1228`). The real gap is orthogonal: the guard's `FORBIDDEN` list describes OS access only, not the mutation the new code introduced (finding 2). |
| "`docs/drafts/mouse-rs-decomposition-proposal.md` is uncommitted; read for intent" | Confirmed uncommitted. Its analysis (production-half counting, the four `begin`/`extend`/`finish` triples as the strongest seam, "one PR per module, verbatim, gate green between each") was followed closely; the delivered split matches the proposed shape, with `route.rs` overshooting its ~400-line budget (finding 15). |
| "Native scroll and selection against their design plans" | Both plans are **stale on HEAD**. `native_scroll_plan.md` last commit `6ba251f` (#211) — before the first implementation PR `359f3a6` (#212); 24 mouse PRs landed after it. `mouse_selection_plan.md` last commit `915da2e` (#227); 16 mouse PRs landed after it. Divergences 4, 5, 10, 11 all sit in that gap. |
| "Did post-split changes reintroduce cross-module reach-ins?" | **No.** Every mouse child reaches `App` through `impl super::super::App` (the descendant rule) and imports only `Effect` / `FrameLayout` / `PagerSlot` / `Side` / `effect::*` type names. The single outward dependency is `render/chrome.rs:104` → `mouse::tab_hit::tab_widths`, which is the deliberate one-source-of-truth for tab geometry documented at `tab_hit.rs:8-13`. |

---

## Verified correct (spot checks)

- **Terminal mouse-state reconcile.** `settle_mouse_mode` (`src/app/mouse_mode.rs:53-59`)
  compares desired-vs-actual and emits nothing on agreement, so it is free at idle. Actual
  state lives in a process global rather than `ViewState` specifically so the panic hook
  and `restore_terminal` — which cannot reach `App` — stay consistent with it; the test at
  `mouse_mode.rs:137-156` pins the caught-panic case that motivated it. `:mouse off`
  surviving a config reload is a separate override field with its own test (`:162-197`).
- **The 0-dps-at-idle invariant survived the 1002 upgrade.** `lib.rs:532` emits
  `?1000h?1002h?1006h`; `mouse_mode_seq`'s test (`lib.rs:1010-1031`) asserts 1002 present
  and `1003` absent; and `Moved` is filtered in the **reader thread before the channel
  send** (`src/app/proc.rs:117,120`), so an over-reporting terminal cannot wake the loop at
  all. This is the single most load-bearing thing in the campaign and it is right.
- **`invert_scroll` really is applied once.** `gesture_and_delta`
  (`src/app/mouse/mod.rs:42-67`) is the only place the sign is decided, and
  `the_sign_is_decided_once_for_every_consumer` (`:403-417`) asserts up == −down for both
  settings — a guard against a future per-consumer flip, not just a behaviour test. (Its
  scope excludes the forwarded pane; see finding 3.)
- **Press/release pairing.** `forward_release` (`forward.rs:73-84`) keys on
  `std::mem::take(&mut self.view.mouse_press_forwarded)`, not on where the pointer is now,
  so a child that received a press always receives its release and one that did not never
  receives an orphan. The reasoning about claude drag-selecting forever on a missing
  release is recorded at `mouse/mod.rs:174-179` and is correct.
- **Re-encoding rather than relaying.** `mouse_report` (`forward.rs:149-192`) maps each
  `MouseEventKind` to its own button code with the coordinate translation folded in, and
  its doc records the exact regression the shape prevents (a direction-flag signature that
  sent every click as wheel-down 65). Exhaustive match, so a new `MouseEventKind` is a
  compile error rather than a silent no-op.
- **One geometry source.** `frame_layout` (`render/mod.rs:470-499`) is `&self` and pure,
  and the mouse hit-test calls it rather than reassembling `compute_layout` — the doc at
  `:464-469` names the three bugs the reassembly caused (`pane_hidden`, `^a z` zoom,
  `carve_vsplit`). `region_at` (`route.rs:571-602`) checks most-specific-first with the
  `status`-before-`prompt` ordering the `top_unit` off-by-one trap requires.
- **No channel bypass.** `Event::Mouse` is one arm of the existing input match
  (`run.rs:214-219`) returning `Vec<Effect>` into `run_effects`; `git diff v2.0.0..HEAD --
  src/app | grep '^+.*mpsc::channel'` is empty. Every effect in the mouse path
  (`SendToPane`, `CopyToClipboard`, `CopyToPagerClipboard`, `PasteFromClipboard`,
  `SetMouseMode`) is returned as data.
- **Pure-decision extraction held under feature pressure.** `scroll_streak_step`,
  `decide_agent_view_action` and `pending_view_confirmed` (`route.rs:261-396`) are all
  clock-free pure fns taking `now` as a parameter, each with the shipped bug it prevents
  written into its doc (the flicker loop on a down-tick open; the `q`-per-tick storm into
  codex's composer from a direction-agnostic confirmation). This is the house template
  applied well.
- **Selection extraction details.** Pane text is trimmed per line
  (`src/pane/mod.rs:426-432`) via `contents_between`, honouring soft wrap, per
  `mouse_selection_plan.md` Tier 5. Pager text comes from `view.lines` through
  `visual_yank_text`, so it is decoration-free by construction. List paths are resolved
  from the live listing at release time (`selection.rs:360-385`), not captured at press,
  so a refresh cannot yield stale paths. OSC 52 clipboard routing (the plan's PR 5) shipped.
- **`make`-gate state.** `cargo test --lib app::mouse` — 96 passed, 0 failed.

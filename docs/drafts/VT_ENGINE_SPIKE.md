# VT engine spike — vt100 vs. wezterm-term vs. libghostty-vt

**Status:** spike complete; recommendation below is a proposal, not a decision.
**Measured against:** `55e78bd` (`main`, `2.2.0-CURRENT`), macOS 26.6.2 / arm64,
rustc 1.96.0.
**Harness:** [`spikes/vt-engine/`](../../spikes/vt-engine/) — its README carries
the run instructions and the corpus-regeneration commands. Every figure below
names the binary that produced it.
**Gates:** `ROADMAP.md` → "The 3.0 horizon: Slow Cooker (durable sessions)"
names this report by path and says scope does not commit until it reports.
That section landed as #448 *during* this spike, so the framing below follows
it (a **daemonized monolith** — one headless spyc owns the PTYs, a thin client
attaches and renders) rather than the engagement brief's zmx shadow-state-machine
shape. The difference sharpens the criterion rather than softening it: with no
shadow, the daemon's engine is the *only* copy of the screen, so its emit is
the only route to painting a freshly attached client. §5.1 prices the bug class
the ROADMAP singles out.

**Scope:** no production code changed; no main-crate `Cargo.toml` change. The
spike crate declares its own empty `[workspace]`, and `cargo package` on spyc
already skips `spikes/` because a nested package is excluded automatically
(verified: `cargo package --list` returns 302 entries, none under `spikes/`).

---

## Summary

**The engine choice is real, but it is not the most urgent thing this spike
found.**

1. **The `panic = "unwind"` mitigation in `[profile.release]` is guarding the
   wrong bug.** The DECRC-after-resize panic its comment describes was fixed
   upstream in **0.16.2, which spyc already ships**. Meanwhile four *other*
   panic classes are live in 0.16.2, all reachable at exactly the geometry
   spyc's `.max(1)` clamp produces, and structured-random escape streams panic
   vt100 in **2.87% of iterations** where ghostty panics in 0% and wezterm in
   0.03%. The net is load-bearing today — just not for the reason recorded.
2. **#34 is both an engine defect and an adapter defect, and the adapter half is
   fixable now without touching the engine.** `cell_style` never reads
   `Cell::dim()`, which vt100 gained in 0.16 *after* the function was written,
   so a child's SGR 2 renders at normal weight. That needs no engine swap.
   (A second adapter claim — a spurious space in wide-glyph continuation cells —
   **was withdrawn on 2026-09-04**; it was an artifact of this harness comparing
   two different normalizations. See §4.)
3. **For the 3.0 rehydration criterion, vt100 is disqualified on one number:
   scrollback reconstruction is 0%.** It round-trips the visible screen
   perfectly (100% rows, cells and attributes, cursor 26/26) and cannot emit a
   single row of history. `Screen::all_contents_formatted` has been an open PR
   upstream since **January 2021**.
4. **wezterm-term is not adoptable.** It is not published to crates.io at any
   version, so the only route is a git dependency — and a crate with a git
   dependency cannot be published to crates.io. That ends the option before any
   fidelity number matters. Its costs, had it been adoptable, were also the
   worst measured: +185 transitive crates, +4.13 MiB binary, and 3.7× slower
   than the incumbent.
5. **libghostty-vt is the strongest engine and the weakest dependency.** It is
   3.7× faster than vt100, panicked zero times in 50,000 fuzz iterations,
   reconstructs 99.5% of scrollback, and is the only candidate whose emission is
   parameterized by target capability. Its Rust bindings are real and
   maintained. But its C ABI broke in the eight weeks between the published
   bindings' pinned commit and `main`, **silently** — the same bindings return
   correct geometry against one commit and garbage against the other.
6. **Distribution for option C is payable.** Vendored prebuilt static archives
   are **0.59 MiB gzipped per target** (ReleaseSmall, all 179 exported symbols
   intact), so all four release targets add ~2.4 MiB to a 1.62 MiB published
   crate — comfortably inside crates.io's 10 MiB cap. The blocker is not size.

**Recommendation: fix the adapter and the vt100 pin now; do not adopt an engine
for 2.2; build the trait seam in 2.3 and adopt libghostty-vt for 3.0 behind a
`spyc-vt-sys` crate that owns the pin.** Reasoning and trade-offs in
[Recommendation](#recommendation).

---

## 1. Seam inventory

`grep -l vt100 src/` returns **31 files**. That number is misleading: most are
comments. Classifying by whether a line references a vt100 *type*
(`grep 'vt100::'`, with in-file `#[cfg(test)]` tails split off the way
`guard_support::production_half` does):

| ring | file | production type-refs | role |
|---|---|---:|---|
| **1. parser host** | `src/pane/mod.rs` | 18 | owns `Arc<Mutex<vt100::Parser>>`, the worker thread, `with_screen`/`with_screen_mut`, the child-state predicates |
| | `src/pane/pty_host.rs` | 0 | pty kernel — comments only, no vt100 types |
| **2. scrollback adapter** | `src/ui/scrollback.rs` | 4 | `&mut Screen` → `Vec<Line>`; **on the do-not-replace list** |
| **3. leak sites** | `src/pane/widget.rs` | 7 | `PaneWidget`, `cell_style`, `convert_color` |
| | `src/pane/input.rs` | 7 | `MouseProtocolMode`/`Encoding` in `encode_mouse` |
| | `src/app/util.rs` | 1 | `place_pty_cursor_from_screen(&vt100::Screen)` |
| | `src/app/tasks.rs` | 1 | `Parser::new` when promoting a background task to a pane |
| | **total** | **38** | across **6** production files |

Test-only: `src/pane/tests.rs` (4), plus 2 each in the `input.rs` and
`scrollback.rs` test tails.

**Zero type dependency, naming only:** `src/app/pane_scroll.rs` has 16 code
lines mentioning vt100 (`has_vt100_capture`, `ScrollSourcePick::Vt100`,
`mount_vt100_scrollback`) and does not reference a vt100 type. Renaming those to
`terminal_capture` / `Terminal` would cost nothing and would shrink the apparent
seam by a third. The remaining ~22 files are doc comments and module docs.

### The trait boundary a swap needs

The spike built it: [`spikes/vt-engine/src/engine.rs`](../../spikes/vt-engine/src/engine.rs).
The full API surface spyc consumes is **30 methods and 3 enums**:

- **Parser:** `new(rows, cols, scrollback)`, `process(&[u8])`, `screen()`,
  `screen_mut()`
- **Screen:** `size`, `set_size`, `scrollback`, `set_scrollback`,
  `cursor_position`, `hide_cursor`, `alternate_screen`, `bracketed_paste`,
  `application_cursor`, `mouse_protocol_mode`, `mouse_protocol_encoding`,
  `contents`, `contents_between`, `cell`, `row_wrapped`, `state_formatted`
- **Cell:** `contents`, `fgcolor`, `bgcolor`, `bold`, `italic`, `underline`,
  `inverse`, `is_wide`, `is_wide_continuation` (and `dim`, which spyc does not
  yet call — see §4)
- **Enums:** `Color` (3 variants), `MouseProtocolMode`, `MouseProtocolEncoding`

Two of these are the ones that constrain the choice:

- **`set_scrollback` / `scrollback` exist in the seam only because vt100's
  scrollback is not iterable.** `src/ui/scrollback.rs` walks history by
  mutating the view offset and restoring it, which is why its readers take
  `&mut Screen`. An engine that addresses history by coordinate makes that
  `&mut` disappear and the adapter simpler, not just different.
- **`state_formatted` is the whole rehydration API.** It is the 3.0 seam, and
  it is one method today.

This section stands on its own value: the seam is 38 lines in 6 files behind one
~30-method trait, which is small enough that the *engine* is not what makes an
engine swap expensive. What makes it expensive is threading — see §7.

---

## 2. The matrix

| | **A. vt100 0.16.2** | **B. wezterm-term 0.1.0** | **C. libghostty-vt 0.2.1** |
|---|---|---|---|
| **Fidelity** (curated corpus, 26 cases) | outlier on **all 4** content divergences | agrees with C on all 4 | agrees with B on all 4 |
| — DEC special graphics (SCS) | ✗ renders `lqqqk` | ✓ `┌───┐` | ✓ `┌───┐` |
| — custom tab stops (HTS/TBC) | ✗ ignores, uses 8-col | ✓ | ✓ |
| — grapheme clusters > 18 B | ✗ silently truncates | ✓ | ✓ |
| — DECSTBM header retention | ✗ loses the row | ✓ | ✓ |
| — scrollback under DECSTBM (codex) | ✗ 0 rows | ✓ 1 row | ✓ 1 row |
| **Rehydration** (3.0 criterion) | visible screen **100%**, scrollback **0%** | **no API at all** | content ~complete, **99.5%** scrollback |
| — capability parameterization | ✗ 4 hardcoded modes | ✗ n/a | ✓ **13 independent toggles** |
| — alt-screen flag survives | ✗ 24/26 | n/a | ✓ 26/26 |
| — known emit bugs | — | — | 2 (one-row offset; tabstops clobber cursor) |
| **Robustness** (50k fuzz iters) | **1437 panics (2.87%)** | 15 panics (0.03%) | **0 panics** |
| **Throughput** (8 KiB chunks) | 181.7 MiB/s | 49.4 MiB/s | **678.9 MiB/s** |
| **RSS / pane** (24 panes, 10k scrollback) | **9,681 KiB** | 1,425 KiB | 875 KiB\* |
| **RSS / retained row** | 2.43 KiB | **0.36 KiB** | 0.85 KiB\* |
| **Binary delta** (stripped, LTO) | +67 KiB | **+4.19 MiB** | +1.06 MiB |
| **Transitive crates added** | baseline | **+185** | **+5** |
| **Unsafe surface in spyc** | none | none | FFI via `-sys` crate; `!Send` |
| **Distribution** | ✓ plain crates.io dep | ✗ **not on crates.io** | ⚠ vendoring required (payable) |
| **Maintenance pulse** | ✗ 14 months silent, 15 open PRs | ⚠ alive, unpublished | ✓ 1122 commits / 8 weeks |
| **API stability** | stable, stagnant | stable | ✗ **breaking C ABI in 8 weeks** |

\* ghostty's per-pane and per-row memory are **not comparable at face value**:
it retained 1033 rows where the others retained 3988, because at the matched
commit `max_scrollback` is inert (see §6). Its true per-row cost is unmeasured.

---

## 3. Maintenance pulse, with citations

### A. vt100 — stagnant, with panic fixes queued

- Last release **0.16.2, 2025-07-11**; last commit `fc26fd9`, **2025-07-12** —
  ~14 months of silence as of this writing.
- Commits by year: 2019:197, 2021:104, 2023:21, 2024:1, **2025:34** (all in one
  July burst), 2026:0.
- Bus factor **1** (Jesse Luehrs).
- **15 open PRs**, oldest 2021-01-30. Three fix panics:
  [#41](https://github.com/doy/vt100-rust/pull/41) "Fix panics with small screen
  sizes", [#30](https://github.com/doy/vt100-rust/pull/30) "Fix wide-character
  and small-screen panics", [#29](https://github.com/doy/vt100-rust/pull/29)
  "Fix 1x1 grid scrolling".
- **7 open issues**, including two live panic reports against 0.16.2
  ([#37](https://github.com/doy/vt100-rust/issues/37),
  [#28](https://github.com/doy/vt100-rust/issues/28)).
- Three open PRs are things spyc has worked around by hand:
  [#27](https://github.com/doy/vt100-rust/pull/27) adds `Screen::scrollback_len()`
  (spyc hand-rolls the `set_scrollback(usize::MAX)` probe in
  `src/ui/scrollback.rs`); [#33](https://github.com/doy/vt100-rust/pull/33)
  "Fix scrollback not accumulating for top-aligned scroll regions" is
  **exactly** the codex limitation documented at `src/agent/mod.rs:470`;
  [#3](https://github.com/doy/vt100-rust/pull/3) `Screen::all_contents_formatted`
  is the scrollback rehydration API 3.0 needs, open since **2021-01-30**.

The one fix that *did* land is the one spyc already has: `7076e0f`
"fix potential cursor out of bounds when using decrc after resizing", in
`v0.16.2` and not in `v0.16.1` (`git merge-base --is-ancestor 7076e0f v0.16.2`).

### B. wezterm-term — alive upstream, unreachable downstream

Version `0.1.0`, repo active. `termwiz` (its sibling) last published 0.23.3 on
crates.io 2025-03-20; **`wezterm-term` has never been published**. Confirmed
two ways: `curl crates.io/api/v1/crates/wezterm-term` → not found; a crates.io
search for `wezterm` returns `wezterm-bidi`, `-dynamic`, `-color-types`,
`-input-types`, `-blob-leases`, and third-party forks (`tattoy-wezterm-term
0.1.0-fork.5`), but not the crate itself.

Its manifest also uses `dependency.workspace = true` throughout, so it resolves
only inside wezterm's own workspace — a git dependency is the only route. And
that route is closed, verified rather than assumed: a throwaway crate with
`wezterm-term = { git = ... }` and nothing else fails `cargo package` with

```
error: failed to verify manifest at `.../Cargo.toml`
  all dependencies must have a version requirement specified when packaging.
  the `git` specification will be removed from the dependency declaration.
```

cargo strips the `git` spec at package time and demands a registry version,
which for this crate does not exist. Depending on the third-party
`tattoy-wezterm-term` fork would sidestep that by making spyc's terminal engine
someone else's unrelated project's fork, pinned at `0.1.0-fork.5` and last
published 2025-07-11 — strictly worse than either remaining option.

### C. libghostty-vt — very active, pre-1.0, moving fast

- Ghostty: **1122 commits** between the bindings' pinned commit (`a887df42`,
  2026-07-11) and `main` (`c81f0b26`, 2026-09-03).
- Rust bindings `libghostty-vt` / `libghostty-vt-sys` 0.2.1, published
  2026-07-18 by [uzaaft/libghostty-rs](https://github.com/uzaaft/libghostty-rs),
  95,654 / 33,024 downloads. Related crates are being published weekly
  (`gpui-libghostty` 0.2.1 on 2026-09-02).
- The library's own docs are explicit: *"This library is currently in
  development and the API is not yet stable. Breaking changes are expected."*
  That is not marketing hedging — see §6.

---

## 4. #34 — engine defect or adapter defect?

**Both, and they separate cleanly.** Evidence:
[`src/bin/adapter_probe.rs`](../../spikes/vt-engine/src/bin/adapter_probe.rs),
which transcribes spyc's `PaneWidget` cell walk and `cell_style` verbatim
(`mod pane` and `mod ui` are private in `src/lib.rs`, so the spike cannot link
them) and prints the engine's state beside what the widget wrote.

### Adapter defect 1 — WITHDRAWN (2026-09-04)

**This finding was wrong. It is left here, marked, rather than deleted, because
it reached a decisions-log entry, two issues and a scope amendment before it was
caught, and a silently-vanishing claim is worse than a corrected one.**

The claim was that `PaneWidget` clobbers ratatui's wide-glyph continuation cell
by writing a space into it, rendering `"あいab"` as `"あ い ab"`.

What is actually true. Measured, and then confirmed against upstream source —
the empirical check says *that* they agree, the source says *why*:

- `Buffer::set_string(0, 0, "あ", ..)` **claims** the continuation column itself
  and fills it with `" "` at style `Reset`. Verified against a buffer pre-filled
  with `#`: after the write, col 1 is `" "`, not `#`, so `set_string` touched it.
- That is deliberate, and ratatui says so. `ratatui-core-0.1.2`,
  `src/buffer/buffer.rs:363-366`, inside `set_stringn`:

  ```rust
  // Reset following cells if multi-width (they would be hidden by the grapheme),
  while x < next_symbol {
      self[(x, y)].reset();
      x += 1;
  ```

  `Cell::reset()` restores symbol `" "` and the default style, so the buffer is
  already in exactly the state the widget's space-write puts it in — and the
  renderer hides that cell behind the wide symbol regardless.
- vt100 reports the continuation cell as `bg=Default`, **not** the head's
  background — so the style the widget writes there is `Reset` as well.
- Skipping the cell and writing a space into it are therefore **no-ops relative
  to each other three times over**: ratatui already reset the cell, the style
  written into it is `Reset` anyway, and the renderer would hide it either way.

`"あ い ab"` is the correct ratatui representation of a wide glyph: one cell for
the glyph, one claimed spacer. The original finding came from **this probe
comparing two different normalizations** — the engine row was built with wide
continuation cells *skipped* and the buffer row with them *included*, so a
correct rendering read as a spurious space. `adapter_probe` now builds both rows
identically and prints the continuation cell's symbol and style side by side;
they agree.

One real observation survives, as a non-defect: neither path paints the head's
background into the continuation column, so a wide glyph on a coloured
background has a one-column gap in spyc's buffer. That is vt100's model faithfully
reproduced — it reports `Default` for the continuation — and what the user sees
depends on whether their terminal draws the glyph's background across both
columns. Not spyc's to fix, and not evidence for a widget change.

**The lesson, which is the reason this section stays:** a differential probe
that normalizes its two sides differently manufactures findings. Compare like
with like, and when a probe reports a defect, check the probe before the code.

### Adapter defect 2 — SGR 2 (dim) is dropped

```
dim attribute: engine reports cell.dim()=true, adapter's cell_style() emits DIM modifier=false
```

vt100 0.16.0 added dim support (CHANGELOG: *"Support for dim formatting. (Daniel
Faust, #9)"*). `cell_style` reads `bold`/`italic`/`underline`/`inverse` and was
written against 0.15, which had no `dim()`. Agent CLIs use dim heavily for
secondary text, so this flattens exactly the visual hierarchy the pane is
meant to show.

### Engine defects behind the rest

```
--- DEC special graphics (SCS)
    engine row 0: "lqqqk"          <- the engine itself holds letters
    adapter row 0: "lqqqk"         <- the adapter renders them faithfully
```

vt100 does not implement `ESC ( 0`. Both other engines draw `┌───┐`
(fixture `charset-decgraphics`). Any child using SCS box drawing renders as
literal `lqqqk`/`x`/`mqqqj` in a spyc pane — garbage text, engine-side.

Also engine-side, from `differential`:

- `scroll-region-decstbm`: vt100 loses the `header` row; both others keep it.
- `captured/agent-codex`: vt100 retains **0** scrollback rows, both others
  retain 1. This confirms the limitation `src/agent/mod.rs:470` documents
  ("codex confines the transcript to a DECSTBM scroll region so lines never
  scroll off the top") is an **engine** limitation, not an inherent property of
  scroll regions — and upstream PR #33 fixes it, unmerged.
- `zwj-emoji`: vt100 truncates `🏴󠁧󠁢󠁳󠁣󠁴󠁿` after 4 of 6 tag characters, dropping
  `\u{e0074}\u{e007f}`. Root cause located: `src/cell.rs` has
  `const CONTENT_BYTES: usize = 22` and `append()` does
  `if len >= CONTENT_BYTES - 4 { return; }` — a silent drop at 18 bytes. The
  flag needs 28.

**Verdict for #34:** the issue's "scrollback accumulates artifacts" is
predominantly an **engine** problem — SCS, scroll-region content loss, zero
scrollback under DECSTBM, grapheme truncation — with one small adapter defect
alongside it (dropped dim). Recommend splitting #34: the dim fix can ship
immediately, and the rest defers to the engine decision. Note that this verdict
is weaker than the one first written here: the wide-glyph claim that made the
adapter half look like the headline did not survive checking.

---

## 5. Differential, rehydration and fuzz findings

### 5.1 Rehydration round trip — and the cross-emulator bug class

Method (`rehydrate`): feed a case, dump the engine's own state, replay that dump
into a **fresh** engine of the same geometry, diff the two screens. Graded on the
three things a reattach actually needs — reconstruction fidelity, capability
parameterization, and scrollback depth.

| | vt100 | ghostty | wezterm |
|---|---:|---:|---:|
| rows exact | **100.0%** | 57.3% | — |
| rows, ±1 viewport shift allowed | 100.0% | **98.0%** | — |
| cell text | **100.0%** | 84.3% | — |
| cell attributes | **100.0%** | 94.6% | — |
| cursor preserved | **26/26** | 24/26 | — |
| alt-screen flag preserved | 24/26 | **26/26** | — |
| **scrollback preserved** | **0 of 4229 rows (0.0%)** | **1269 of 1275 (99.5%)** | — |
| emitted bytes, all 26 cases | 12,482 B | 123,539 B | — |

wezterm-term has **no state-emit API at all**; reconstruction there would go
through termwiz's `Surface` diffing, a different shape in a different crate.
Reported as absent rather than approximated.

The two engines fail in opposite directions, and only one of the failures is
structural:

- **vt100 reconstructs the visible screen perfectly and no history whatsoever.**
  `state_formatted()` is `contents_formatted()` + `input_mode_formatted()`;
  the first covers only the visible window and the second covers exactly four
  modes (application keypad, application cursor, bracketed paste, xterm mouse).
  Scrollback is not reachable — `all_contents_formatted` is upstream PR #3, open
  since 2021-01-30. It also loses the alt-screen flag (`?1049h` is not emitted),
  which is why `captured/agent-agy` and `captured/htop` fail.
- **ghostty reconstructs essentially everything, with two narrow emit bugs.**
  The 57.3% exact figure is almost entirely **one row of viewport offset**: at
  ±1 tolerance it is 98.0%, and the offset appears on exactly the six
  scrollback-bearing cases. `rtdiff` shows it directly — replay row 0 is the
  original's row −1, scrollback 1033 vs 1032. The content is present and
  correctly ordered; the window sits one row high.

The second ghostty bug is worth naming precisely because it is *in* the feature
3.0 depends on. `with_tabstops(true)` emits the cursor restore **before** the
tab-stop reconstruction, and setting a tab stop moves the cursor
(`CSI 9 G`, `HTS`) — so the restored position is clobbered. From `grt`, the same
state round-trips to cursor `(3,0)` without the toggle and `(3,16)` with it.
The two `cursor LOST` cases above (`scroll-region-decstbm`,
`alt-screen-roundtrip`) are the same ordering shape with
`with_scrolling_region(true)`. Both are pre-1.0 emit-ordering defects, narrow
and reportable, not architectural.

#### The bug class the ROADMAP asks this spike to price

> *"reattaching from a **different** terminal emulator — cell size, kitty/sixel
> capability and colour depth all changing mid-session — is the known-hard bug
> class to price before committing to it."* — `ROADMAP.md`, 3.0 horizon

This is the capability-parameterization row of the matrix, and it is the single
widest gap between the candidates.

**vt100 cannot express the problem.** `state_formatted()` takes no arguments.
It emits whatever the captured state was, in whatever colour depth the child
originally used — so a reattach into a 256-colour terminal from a truecolour
session emits truecolour SGR, and a reattach into a terminal without kitty
graphics still gets whatever the mode state said. spyc would have to
post-process the emitted byte stream to degrade it, i.e. write a second VT
parser to fix up the first one's output. (spyc already has one narrow instance
of this shape — `src/ui/color_depth.rs`'s per-frame `downgrade_buffer` — but
that operates on a ratatui buffer it owns, not on an opaque escape stream.)

**ghostty parameterizes the emit at the source.** `FormatterOptions` carries
**13 independent toggles**, measured individually by `gfmt`:

| toggle | what a reattaching client uses it for | cost |
|---|---|---:|
| `with_format(Plain\|Vt\|Html)` | output flavour | — |
| `with_palette` | restore the session's OSC 4 palette | **5,522 B** |
| `with_modes` | DEC private modes | 0 B here |
| `with_scrolling_region` | DECSTBM | 0 B here |
| `with_tabstops` | HTS/TBC stops | +17 B (**buggy, see above**) |
| `with_cursor` | cursor position | +6 B |
| `with_style` | SGR state | +4 B |
| `with_kitty_keyboard` | keyboard-protocol flags | 0 B here |
| `with_hyperlink` | OSC 8 URIs | 0 B here |
| `with_charsets` | SCS state | 0 B here |
| `with_protection` / `with_unwrap` / `with_trim` | DECSCA; soft-wrap and padding normalization | — |

The 5,522-byte palette cost is the concrete illustration: a client reattaching
into a terminal that already has the user's palette does not want those 256
OSC 4 sequences, and a client on a different palette does. That is a per-attach
decision the engine can be *told*, rather than a property baked into the dump.
Same for kitty keyboard and hyperlinks against a client that does not speak them.

**What this does not price.** Cell-size change (a client with different pixel
metrics) affects image protocols, not the cell grid, and neither engine's
text-emit addresses it — that is `ratatui-image`/`Protocol` territory in
`src/app/image_ops.rs` and out of scope here. Sixel/kitty *graphics*
reconstruction is likewise unpriced: ghostty models kitty graphics
(`src/kitty/graphics.rs`, `set_apc_max_bytes`, placement iterators) and vt100
does not model them at all, but neither replays them through the state emit, so
a reattach loses in-pane images on both. **Flagging that as the residual
known-hard piece for `PROJECTS_PLAN.md` to answer**, since it is a
"serializes or is rebuilt client-side" question of exactly the kind that
document is being asked to settle per field.

### Curated corpus — `differential`

**20 / 26 cases at exact parity across all three engines; 14 divergences.**
The corpus is 18 synthetic cases (in `src/fixtures.rs`, as code) plus 8 captured
PTY streams.

vt100 is alone on one side of **every content divergence**; ghostty and wezterm
agree with each other in all four. The four are the engine defects in §4.

Two divergences are **pinned semantic differences**, not defects:

- `captured/agent-claude`: 50 blank cells where wezterm reports `fg=Idx(3)` and
  both vt100 and ghostty report `Default`. Erased-cell attribute retention.
- `captured/htop`: 16 blank cells where vt100 reports `fg=Idx(0) bg=Idx(2)` and
  ghostty reports `Default`. Same class, opposite pairing — so no engine is
  consistently the outlier here, and the ECMA-48 text on what SGR state an erase
  leaves behind is genuinely permissive. Pin these; do not "fix" them.

### Differential fuzz — `fuzz_diff`, 50,000 iterations, seed `0x5157c0de`

Written before any conclusion, per the house rule. Structured-random generator
(real CSI/OSC/APC/SGR/charset shapes, not uniform noise), streams split across
a chunk boundary the way pty reads deliver them, geometries including the
degenerate `rows==1`/`cols==1` that spyc's `.max(1)` clamp permits, and a
mid-stream resize on one iteration in three.

```
PANICS over 50000 iterations: 1452 total, 17 distinct message(s)
  vt100    x1433   (2.866%)  called `Option::unwrap()` on a `None` value
  vt100    x2      (0.004%)  index out of bounds: the len is 1 but the index is 1
  ... 2 more vt100 index-out-of-bounds shapes
  wezterm  x15 across 13 shapes, all `index out of bounds: len is N but index is N`
  ---- totals: vt100=1437  ghostty=0  wezterm=15
```

**vt100 panics on 2.87% of structured-random escape streams. ghostty panicked
zero times**, which is the empirical form of its documented contract that
`vt_write` *"never fails... the primary goal is to keep the terminal state
consistent and not allow malformed input to corrupt or crash."* wezterm's 15
panics are one off-by-one shape at 13 widths — a single root cause, and a real
finding worth reporting upstream if B were ever adoptable.

**Do not read the agreement rate as a fidelity verdict.** Only 7.3% of
non-panicking iterations had all three identical and 90% had no two agree —
expected, because the generator deliberately emits undefined and malformed
sequences where every implementation legitimately differs. The fuzz's value here
is the panic counts; the curated corpus is what measures fidelity.

**Promotion:** the generator is worth moving into `fuzz/fuzz_targets/` as a
single-engine "never panics" target once an engine is chosen. It found nothing
in ghostty at 50k iterations, which is the right reason to keep running it
longer under `cargo-fuzz` rather than to stop.

### The live vt100 panic classes — `vt100_panics`

Each upstream report transcribed, run under `catch_unwind` (the net
`Pane::process_bytes_safe` already provides), and annotated with whether spyc
can reach the geometry:

| case | 0.16.2 | reachable in spyc? |
|---|---|---|
| `Parser::new(0, 10, 0).process(b"a")` | **panics** | no — `.max(1)` forbids 0 |
| `Parser::new(1, 10, 0)` + wrap | **panics** | **yes** — `.max(1)` produces exactly 1 |
| `Parser::new(10, 1, 0)` + wide char | **panics** | **yes** |
| wide char split by `set_size` shrink | **panics** | **yes** |
| same setup + `ECH` over the orphan | **panics** | **yes** |
| DECSC → shrink → DECRC (issue #13) | ok | yes — **this is the one the Cargo.toml comment names** |
| scrollback > rows (issue #5) | ok | yes |

Both profiles matter and give the same answer for a different reason: in debug
three of these are `attempt to subtract with overflow`; in release
(spyc's profile, overflow-checks off) the subtraction wraps to 65535 and the
downstream `Option::unwrap()` panics anyway. **5 of 7 panic in release, 4 of
them at geometry spyc reaches.**

`src/pane/mod.rs:300-305` already defends the zero case with an explicit comment
about vt100's unguarded `rows - 1` — so someone hit this before. The clamp lands
on `1`, which is where the remaining four live.

### Why vt100 costs 9.5 MiB per pane

`size_of::<vt100::Cell>() = 32 B` (asserted in vt100's own source at
`src/cell.rs:17`) → 2,560 B per 80-column row → the measured **2.43 KiB per
retained row**, i.e. a dense uncompressed 32-byte-per-cell grid allocated for
the whole scrollback. At spyc's 10,000-row budget that is **9,681 KiB per pane**;
24 pane tabs measured **227 MiB** of RSS.

One design decision — a fixed 22-byte inline cell buffer — produces both this
memory profile and the grapheme truncation in §4. That is worth noting because
neither is fixable without changing vt100's cell representation, which is the
kind of change a project at 14 months of silence does not take.

---

## 6. Option C's real risk is not distribution — it is ABI drift

The brief expected distribution to be where C wins or dies. It is not. Measured,
distribution is payable and API stability is the problem.

### The build, as published, does not work

`libghostty-vt-sys 0.2.1`'s `build.rs`:

1. requires **`zig` on PATH**;
2. **git-clones `https://github.com/ghostty-org/ghostty` at a pinned commit into
   `OUT_DIR`** at build time;
3. then `zig build` fetches Zig's own package dependencies.

On this machine that fails twice over, and the second failure is instructive:

- The pinned commit (`a887df42`) declares `minimum_zig_version = "0.15.2"`, and
  ghostty's `requireZig` compares **major and minor for equality**
  (`current.major != required.major or current.minor != required.minor or
  current.patch < required.patch`). Homebrew's current Zig is 0.16.0 →
  rejected outright.
- Zig 0.15.2 from ziglang.org **cannot link at all on macOS 26.6.2** — its own
  build runner fails with `undefined symbol: _abort`, `_dispatch_semaphore_create`,
  `__availability_version_check`. Reproduced standalone on an empty `build.zig`,
  so it is the toolchain, not libghostty.

So option C, as published, is **currently unbuildable on a current macOS** —
caught between a ghostty pin demanding Zig 0.15.x and a Zig 0.15.x that cannot
target the OS. A `cargo install spyc` user would hit exactly this.

### And when you route around it, the ABI is silently wrong

Pointing `GHOSTTY_SOURCE_DIR` at ghostty `main` builds cleanly and produces
**garbage**:

```
asked rows=24 cols=80 sb=10000 -> rows()=Ok(10000) cols()=Ok(80) total_rows()=Ok(10000)
```

`rows()` returns `max_scrollback`. Cause, located in the headers:

- pinned `a887df42`: `ghostty_terminal_new(allocator, GhosttyTerminal*, GhosttyTerminalOptions)`
  — options passed **by value as a struct** `{uint16_t cols; uint16_t rows; size_t max_scrollback;}`,
  carrying an explicit `// TODO: Consider ABI compatibility implications of this struct.`
- `main` `c81f0b26`: `ghostty_terminal_new(allocator, GhosttyTerminal*, uint16_t cols, uint16_t rows)`
  — the struct is **gone**; the scrollback limit moved to `terminal_set`
  (`03d5fa26` "lib-vt: move scrollback limits to terminal_set", 2026-07-27) and
  on `main` is split into `SCROLLBACK_MAX_BYTES` / `SCROLLBACK_MAX_LINES`.

The published bindings pass a struct where `main` expects two scalars. **A C ABI
has no version check, so this compiles, runs, and returns wrong numbers.** I
initially guessed the `GhosttyTerminalData` enum had been renumbered; that was
wrong — values 0..32 are byte-identical and `main` only appends 33..40. The
breakage is in the constructor signature.

Every ghostty figure in this report was therefore measured against
**`f4c68d65`** (2026-07-27), the commit before the API change: it still exposes
the options struct, its data enum is a pure superset of the pinned one, and it
requires Zig 0.16.0. That pairing reports `rows()=24` correctly.

**The residual measurement caveat:** at `f4c68d65` the `max_scrollback` option is
mid-refactor and inert — ghostty retains ~840–1033 rows regardless of the budget
passed (1,000 / 10,000 / 100,000 / 1,000,000 all yield 840 in `sbprobe`). Its
scrollback *depth* is thus unmeasured here, and its per-pane memory advantage is
partly an artifact of storing a quarter as much. Its scrollback *reconstruction
fidelity* (99.5%) is unaffected, being a ratio.

### Distribution, priced

Vendoring prebuilt static archives, pinned by commit and checksummed:

| variant | archive | gzipped | `ghostty_*` symbols |
|---|---:|---:|---:|
| ReleaseFast | 9.23 MiB | 0.75 MiB\* | 179 |
| ReleaseFast, `strip -S` | 2.11 MiB | 0.75 MiB | 179 |
| **ReleaseSmall** | **1.67 MiB** | **0.59 MiB** | **179** |

\* stripped measurement; the unstripped archive compresses similarly.

A `.crate` file **is** a gzipped tarball, so the compressed column is what counts
against crates.io's **10 MiB** default upload cap. spyc's published crate is
currently **1.62 MiB** (v2.1.1, 1,701,638 B). Four release targets
(`aarch64-apple-darwin`, `x86_64-apple-darwin`, and musl `x86_64` / `aarch64`)
at 0.59 MiB each add **~2.4 MiB**, for a ~4.0 MiB crate. Well inside the cap,
and all 179 exported symbols survive both stripping and ReleaseSmall.

**Verdict: yes-with-this-mechanism.** Concretely:

1. A **`spyc-vt-sys` crate** in the Mise en Place direction owns the FFI, the
   pinned ghostty commit, the vendored per-target `.a` files, and their
   checksums. This is consistent with the decisions-log entry scoping unsafe as
   *"exceptional and isolated (a future crate split would give it a dedicated
   crate)"*.
2. **Pinned-commit policy:** the pin is a spyc-owned constant, bumped
   deliberately with a re-run of this harness — not tracked to ghostty `main`.
   The eight-week ABI break above is the argument for owning the pin rather than
   depending on someone else's.
3. **Release-workflow change:** one new job builds the four archives with
   `zig build -Demit-lib-vt=true -Doptimize=ReleaseSmall -Dapp-runtime=none` per
   target and commits them with checksums. `release.yml` already routes `*-sys`
   crates through `cargo zigbuild` for the musl/darwin cross-builds (see the
   `mlua` note in `Cargo.toml`), so a Zig toolchain is already in that pipeline —
   this adds archives, not a new toolchain class.
4. **`cargo-deny` / audit posture:** a vendored binary blob is outside
   `cargo audit`'s reach either way. Vendoring is *better* than the published
   crate's build-time `git clone`, which fetches unpinned-by-checksum code from
   a git URL during `cargo build` and is invisible to `cargo vendor` and
   `--offline`. Vendored archives are at least checksummed, reproducible, and
   reviewable in-tree. Document them in `deny.toml` and note the pin in
   `SECURITY.md`.
5. **MSRV:** `libghostty-vt` declares `rust-version = "1.90"`; spyc declares
   `1.88`. Adoption bumps spyc's MSRV by two releases.

---

## 7. Migration cost against the seam inventory

Small in the seam, non-trivial in the threading model.

**Cheap (the 38 lines):**

- `src/pane/widget.rs` (7) and `src/app/util.rs` (1) consume a `Screen` and a
  `Cell`; both go behind the trait's `Screen`/`Cell` view. ~half a day.
- `src/app/tasks.rs` (1) is one constructor call.
- `src/pane/input.rs` (7) needs the two mouse enums mapped. ghostty exposes
  mouse tracking as `is_mouse_tracking()` plus a `mouse::Format`; the
  vt100 `MouseProtocolMode`×`Encoding` pair does not map 1:1 and this needs a
  real read of ghostty's mouse module — **the one leak site with genuine design
  work in it.**
- `src/ui/scrollback.rs` (4) gets *simpler*: `Point::History` addresses
  scrollback by coordinate, so the offset-mutation walk and its `&mut Screen`
  signatures go away. The adapter stays; its innards shrink.
- `src/app/pane_scroll.rs`: 16 naming-only lines, mechanical rename.

**The real cost — `!Send`:**

```
error[E0277]: `NonNull<TerminalImpl>` cannot be sent between threads safely
note: required because it appears within the type `Terminal<'static, 'static>`
```

Verified by `sendprobe`: `vt100::Parser` is `Send`, `wezterm_term::Terminal` is
`Send`, **`libghostty_vt::Terminal` is not.** spyc's `Pane` holds
`parser: Arc<Mutex<vt100::Parser>>`, clones it into a dedicated
`parser_worker` thread that consumes pty bytes, and locks it from the render
pass. That structure is impossible with a `!Send` terminal.

Two honest notes on this:

- The `!Send` is a **conservative binding choice, not a C-library constraint**.
  The crate says so: *"we as binding authors must rather conservatively avoid
  making any assumptions beyond what is presently guaranteed by the C API."*
  And `RenderState`'s own docs describe the intended concurrent shape — *"allows
  the renderer to be safely multi-threaded (as long as a lock is held during the
  update call)"*. So the fix is an `unsafe impl Send` justified against the C
  API's actual guarantees, living in `spyc-vt-sys` — precisely what that crate
  exists for.
- ghostty's `RenderState` + dirty tracking is a *better* fit for spyc's
  event-driven repaint than the current lock-and-read-the-grid arrangement:
  `begin_update(&terminal)` / `end()` splits the terminal-access window from the
  render work, and `Dirty::{Clean, Partial, Full}` maps onto `needs_draw`'s
  reason codes. So this is a restructure with a payoff, not pure tax.

**Estimate:** ~2 days for the trait extraction and the cheap leak sites, ~1 day
for the mouse-enum mapping, ~3–5 days for the threading restructure plus the
`spyc-vt-sys` crate, plus the release-workflow archive job. Call it **1.5–2
weeks**, and note that the trait extraction alone is independently valuable and
can land first.

---

## Recommendation

**Do not adopt an engine in 2.2. Do three things instead, then adopt
libghostty-vt for 3.0.**

**Now, in 2.2 — cheap, and independent of the engine decision:**

1. **Fix the two adapter defects** (§4): consult `is_wide_continuation()` in
   `PaneWidget` before writing a space, and read `Cell::dim()` in `cell_style`.
   This is the visible half of #34 and it is a few lines. Split #34 accordingly.
2. **Correct the `panic = "unwind"` comment** in `[profile.release]`. The DECRC
   panic it names is fixed in 0.16.2; the net is still load-bearing, for four
   other classes at `rows==1`/`cols==1`/wide-char-shrink. A comment that states
   a fixed bug as the reason invites someone to remove the net.
3. **Consider carrying vt100 PRs #30 + #41 as a patch**, or clamping pane
   geometry to a 2×2 floor. Four reachable panic classes behind a `catch_unwind`
   that recovers by *throwing the grid away* (`rebuild_parser_preserving_size`)
   is a silent scrollback-loss path, not just a crash-avoidance one.

**In 2.3 — build the seam, not the swap:**

4. **Extract the `Engine` trait** (§1) with vt100 behind it. 38 lines, 6 files,
   ~30 methods; the spike's `src/engine.rs` is a starting draft. This is worth
   doing on its own merits — it makes the scrollback adapter's `&mut` an
   implementation detail, and it turns the 3.0 engine decision into a change of
   one impl rather than a change of 6 files.

**For 3.0 — libghostty-vt, behind `spyc-vt-sys`:**

It is the only candidate that can serve the 3.0 shape: 99.5% scrollback
reconstruction against vt100's 0%, and the only capability-parameterized
emission (13 toggles, so a reattaching client with 256 colours and no kitty
graphics gets a stream it can actually consume, instead of spyc having to
post-process one flavour of redraw). It is also 3.7× faster and panicked zero
times in 50,000 adversarial iterations.

**The trade-offs, stated plainly:**

- **You take on a pre-1.0 C ABI that demonstrably broke in eight weeks.** This
  is the real cost, and it is mitigated but not removed by owning the pin. Budget
  a harness re-run on every pin bump; that is what `spikes/vt-engine/` is for.
- **You take on FFI and one `unsafe impl Send`.** Isolated to `spyc-vt-sys`,
  consistent with the existing decisions-log scope for unsafe, but it is a
  genuine change to spyc's posture.
- **You take on a Zig toolchain in the release pipeline** — already there for
  `mlua`/`zstd-sys` cross-builds, but now load-bearing for a vendored artifact.
- **+1.0 MiB binary (+5.3% on 19.0 MiB) and +5 crates.** Cheap.
- **Two known emit bugs** to report upstream or work around: the one-row
  viewport offset on scrollback-bearing state, and `with_tabstops(true)`
  clobbering the restored cursor (it emits the cursor restore *before* the tab
  stops, and setting a tab stop moves the cursor). Both narrow, both
  demonstrated in `grt`, neither architectural.

**Why not stay on vt100 for 3.0:** the blocker is not the panics, which a net
already absorbs, nor the fidelity gaps, which are individually small. It is that
scrollback rehydration does not exist and the PR that would add it has been open
since January 2021 on a project with a bus factor of 1 and 14 months of silence.
3.0 makes screen reconstruction load-bearing; betting it on that PR is not a
plan.

**Why not wezterm-term:** it cannot be a dependency of a crates.io-published
crate. Everything else about it is secondary.

---

## Decisions-log entry (ready to paste)

> **VT engine for 3.0 durable sessions — libghostty-vt, deferred to 3.0 behind
> `spyc-vt-sys`.** A three-way differential spike
> (`docs/drafts/VT_ENGINE_SPIKE.md`, harness `spikes/vt-engine/`) measured
> vt100 0.16.2, wezterm-term, and libghostty-vt 0.2.1 over a 26-case corpus,
> 50,000 fuzz iterations, and a rehydration round trip. **wezterm-term is
> excluded structurally:** it is unpublished on crates.io, so it can only be
> reached by a git dependency, which cannot be published. **vt100 is excluded
> for 3.0 on rehydration:** it reconstructs the visible screen perfectly and
> **0%** of scrollback, and `Screen::all_contents_formatted` has been an open
> upstream PR since 2021-01-30 on a project with bus factor 1 and 14 months of
> silence. **libghostty-vt is adopted for 3.0**: 99.5% scrollback
> reconstruction, 13-toggle capability-parameterized emission, 678 MiB/s
> (3.7× vt100), zero panics in 50k adversarial iterations against vt100's 1,437
> (2.87%), +1.0 MiB binary and +5 crates. Distribution is by **vendored
> prebuilt static archives at a spyc-owned pinned ghostty commit** — 0.59 MiB
> gzipped per target, ~2.4 MiB for the four release targets against crates.io's
> 10 MiB cap, so `cargo install spyc` keeps working without a Zig toolchain on
> the user's machine. The accepted risk is a pre-1.0 C ABI that broke **silently**
> between the published bindings' pin and ghostty `main` eight weeks later
> (`ghostty_terminal_new` lost its options struct); spyc owns the pin and
> re-runs the spike harness on every bump. FFI and one `unsafe impl Send` (the
> bindings are conservatively `!Send`; spyc parses off-thread) are isolated in
> `spyc-vt-sys`, per this log's existing scope for unsafe. MSRV rises 1.88 → 1.90.
> **Not deferred:** two adapter defects the spike found are independent of the
> engine and ship in 2.2 — `PaneWidget` writes a literal space into every
> wide-glyph continuation cell (a visible artifact on every CJK/emoji pane line,
> and the cheap half of #34), and `cell_style` never reads `Cell::dim()`, added
> in vt100 0.16. Also corrected: `[profile.release]`'s `panic = "unwind"`
> comment names a panic fixed in 0.16.2; the net remains load-bearing for four
> *other* classes reachable at spyc's `.max(1)` geometry clamp.

---

## Appendix — how to reproduce every figure

All commands run from `spikes/vt-engine/` with `--features ghostty,wezterm`.

**No `GHOSTTY_SOURCE_DIR` and no Zig.** As of the addendum the harness links
`crates/spyc-vt-sys`'s vendored archive at the pinned commit, so it needs
neither a ghostty checkout nor a Zig toolchain. The `gprobe` and `grt` probes
are retired: `gprobe`'s finding (the silent ABI mismatch) is now a permanent
test in `spyc-vt-sys`, and `grt`'s (the tabstops cursor clobber) is fixed at the
pin. `snapshot_grade`, `sbprobe` and `membudget` replace them.

| figure | command |
|---|---|
| seam inventory | `grep -c 'vt100::' <file>`, split at the first `^#[cfg(test)]` |
| panic classes | `cargo run --release --bin vt100_panics` (and without `--release`) |
| corpus parity, 20/26 | `cargo run --release --features ghostty,wezterm --bin differential` |
| fuzz panic counts | `cargo run --release --features ghostty,wezterm --bin fuzz_diff -- 50000` |
| rehydration grades | `cargo run --release --features ghostty,wezterm --bin rehydrate` |
| throughput, RSS, cell size | `cargo run --release --features ghostty,wezterm --bin bench` |
| scrollback budget units | `cargo run --release --features ghostty,wezterm --bin sbprobe` |
| formatter option costs | `cargo run --release --features ghostty --bin gfmt` |
| snapshot grading (all 4 criteria) | `cargo run --release --features ghostty --bin snapshot_grade` |
| scrollback budget, shipped config | `cargo run --release --features ghostty --bin sbprobe` |
| memory at a FULL row budget | `cargo run --release --features ghostty,wezterm --bin membudget` |
| `Send` bounds | `cargo run --release --features wezterm --bin sendprobe` |
| #34 engine vs adapter | `cargo run --release --bin adapter_probe` |
| binary size deltas | `cd sizeprobe && cargo build --release [--features e-vt100\|e-ghostty\|e-wezterm]` |
| crate counts | `cargo tree --target all --prefix none --features <f> \| awk 'NF{print $1}' \| sort -u \| wc -l` |
| archive sizes | `find <target> -name libghostty-vt.a`, `strip -S`, `gzip -9`, `nm -g \| grep -c ' [TDSB] _ghostty_'` |
| vt100 pulse | `git log --format=%ad --date=format:%Y \| sort \| uniq -c`; `gh issue list -R doy/vt100-rust`; `gh pr list -R doy/vt100-rust` |
| ghostty pulse | `git rev-list --count a887df42..origin/main` |
| the DECRC fix shipped in 0.16.2 | `git merge-base --is-ancestor 7076e0f v0.16.2` |
| crates.io sizes | `curl -s https://crates.io/api/v1/crates/spyc \| jq '.versions[0].crate_size'` |

---

# Addendum — 2026-09-04: re-measured at the shipping pin

**Appended, not merged into the body above.** Everything before this line was
measured at ghostty `f4c68d65`, chosen for ABI compatibility with the published
`libghostty-vt` bindings — and at that commit `max_scrollback` is inert. That
commit cannot ship. This addendum re-states every figure the adoption rests on
at the pin that can, and it is the Stage-5 gate of `V2_2_PLAN.md` §8.

**Pin:** `1f5bb5769fbb5e717546073d33d3985604a315b2`, 2026-09-04.
**Through:** `crates/spyc-vt-sys` — the same bindings, the same vendored
archive and the same FFI production will link. The measured mechanism is the
shipped mechanism, which is the principle the gate exists to enforce.
**Harness:** `spikes/vt-engine/`, `--features ghostty,wezterm`.

## Verdict: the gate passes, and one figure improved materially

| gate criterion | result |
|---|---|
| scrollback budget functional, shipped config | **pass** — see below |
| zero panics over ≥50k `fuzz_diff` iterations | **pass** — 0 |
| rehydration re-graded | **pass**, and the snapshot mechanism is strictly better than the formatter |
| the two known emit bugs re-checked | one **fixed upstream**, one reduced |
| throughput and memory re-measured | **both changed**; see the corrections |

## The scrollback budget, at the shipped configuration

`sbprobe`. Both limits set deliberately — `max_lines = 10_000`,
`max_bytes = 10,109,200` (9.64 MiB) — derived by
`spyc_vt_sys::scrollback::limits_for_row_budget`. Tolerance for "approximately
respected" is one page, 440 rows at the heavy rate.

**Assertion 1 — realistic content must not bind the valve.** 15,000 rows fed
into a 10,000-row budget, so both engines must discard:

| corpus | vt100 | ghostty | floor (budget − one page) |
|---|---:|---:|---:|
| plain | 10,000 | 9,658 | 9,560 |
| heavy | 10,000 | 9,658 | 9,560 |

**Assertion 2 — pathological content must bind it.** 4,000 rows with a distinct
truecolour pair and a grapheme cluster in every cell: ghostty retained **1,613
of 3,976 available** — the valve bound. Dividing the ceiling by the retained
count, ghostty stored ~6.1 KiB/row for that content against ~3.0 KiB/row of
*input*. Storage exceeding input for per-cell styling is exactly the case a
line-only budget cannot bound, which is why both limits are set. vt100 retained
all 3,977: it has no byte ceiling to bind.

### ~840 rows is a number two unrelated causes produce

Recorded deliberately. At `f4c68d65` the retained count saturated at ~840 rows
because `max_scrollback` was inert. At the shipping pin, setting only the line
limit *also* yields ~840, because a default byte cap binds ahead of it. Same
number, different cause. A future harness run that recognises 840 as "the
expected ghostty number" and stops looking is the trap this series already fell
into once.

## Robustness — unchanged

`fuzz_diff`, 50,000 iterations, seed `0x5157c0de`:

| engine | panics | rate |
|---|---:|---:|
| vt100 | **1,437** | 2.87% |
| wezterm | 15 | 0.03% |
| **ghostty** | **0** | **0%** |

Identical to the pre-pin run. wezterm's 15 are one root cause at 13 widths,
now filed upstream as
[wezterm/wezterm#8134](https://github.com/wezterm/wezterm/issues/8134) with a
5-byte reproducer.

## Fidelity — unchanged

`differential`: **20/26 cases at exact parity** across all three engines, with
vt100 alone on one side of every content divergence. Same as pre-pin.

## Rehydration: two mechanisms, graded separately

The gate was amended before it ran, because the pin carries a **snapshot API**
(`ghostty_snapshot_encode` / `ghostty_snapshot_decoder_*`) that did not exist at
`f4c68d65`. Re-running only the formatter measurement would have graded the
wrong mechanism.

### Mechanism A — the VT formatter (`rehydrate`)

Its consumer is the cross-emulator reattach class, where capability
parameterisation is the property that matters.

| | vt100 | ghostty |
|---|---:|---:|
| rows exact | **100.0%** | 57.3% |
| rows, ±1 shift allowed | 100.0% | **98.0%** |
| cell text | **100.0%** | 84.3% |
| cell attributes | **100.0%** | 94.6% |
| cursor preserved | 26/26 | **26/26** (was 24/26) |
| alt-screen preserved | 24/26 | **26/26** |
| **scrollback preserved** | **0 of 4,229 (0.0%)** | **4,224 of 4,230 (99.9%)** |
| emitted, all cases | 12,482 B | 389,039 B |

wezterm still has no state-emit API.

vt100's 0% is unchanged and remains the disqualifying number. ghostty's 57.3%
exact is still almost entirely **one row of viewport offset** on the six
scrollback-bearing cases; at ±1 tolerance it is 98.0%, and scrollback rose from
99.5% to 99.9%. Its emitted size grew because this run enables more toggles
(tabstops, hyperlink, protection, kitty keyboard, charsets) than the pre-pin
run did — a configuration difference, not a regression.

Per-toggle costs (`gfmt`, 20x4 with two styled lines, each row is bare plus one
toggle): palette **+5,522 B**, tabstops +20, cursor +6, style +4, and
`modes` / `scrolling_region` / `pwd` / `keyboard` / `hyperlink` / `protection`
/ `kitty_keyboard` / `charsets` all +0 on this state. A toggle costing 0 has
nothing to say about *this* terminal; the point is that each is independently
switchable for a client that cannot consume it. That is the capability
parameterisation vt100's zero-argument `state_formatted()` has no equivalent of.

### Mechanism B — the snapshot API (`snapshot_grade`)

Its consumers are 3.0 attach and, likely, 2.3 recovery. Same 26-case corpus, so
the two mechanisms are comparable.

| criterion | result |
|---|---|
| rows exact | **100.0%** |
| rows, ±1 shift allowed | 100.0% |
| cell text | **100.0%** |
| cell attributes | **100.0%** |
| cursor preserved | **26/26** |
| alt-screen preserved | **26/26** |
| **scrollback preserved** | **4,230 of 4,230 — 100.0%** |
| encoded, all 26 cases | 1,232,014 B |

**No viewport offset, no attribute loss, no scrollback loss.** The snapshot
mechanism is strictly better than the formatter on every fidelity axis, and the
formatter's one-row offset does not exist here. The trade is size: ~1.23 MB
against the formatter's 389 KB over the same corpus, which is what binary state
costs against a redraw stream.

**The continuation round trip (criterion 3) survives.** A stream cut 14 bytes
into a CSI sequence, snapshotted with continuation retained, decoded, then
resumed by re-feeding the exported 6-byte continuation (`\x1b[38;2`) followed by
the remaining bytes, produces a state **row-for-row and attribute-for-attribute
identical to the uncut run**, cursor included. Crash recovery does not get to
wait for a clean parser boundary, and this is the API's answer to that, tested
rather than assumed.

One constraint that discovery surfaced, and it is a **3.0 design constraint
rather than a probe detail**: `snapshot.h` requires that continuation tracking
be enabled *before* the input that leaves the parser unfinished — encode returns
`GHOSTTY_INVALID_VALUE` otherwise. A daemon that intends to snapshot must
therefore pay for tracking continuously; it cannot opt in at crash time,
because by then the partial sequence is gone. The API's own caveat applies on
the way back too: set `CONTINUATION_MAX_BYTES` to zero after export and before
post-snapshot input, since exporting an empty continuation does not itself
disable tracking.

**Format stability (criterion 4) — bounded, and safely so.** The stream carries
an eight-byte `"GHOSTSNP"` magic and a `u16` version (**1**), with per-record
CRC32C. All three rejection cases refuse to decode, measured rather than
assumed:

| malformed input | decodes? |
|---|---|
| truncated to half its length | no |
| one byte flipped inside a CRC-covered payload | no |
| version set to 65535 | no |

But `snapshot.h` states that version 1 "does not yet carry a
binary-compatibility guarantee". So: **snapshots are transport-only — same
binary, same pin — and never at-rest persistence across an upgrade.** The
version field is what makes that safe rather than dangerous: a stale snapshot
is detectable and discardable, not silently misparsed. **`PROJECTS_PLAN.md`
question 7 should carry that sentence verbatim.**

## The two known emit bugs, re-checked at the pin

- **`with_tabstops(true)` clobbering the restored cursor: FIXED upstream.** The
  emit now places the cursor restore after the tab stops
  (`…\x1b[9G\x1bH\x1b[17G\x1bH\x1b[H`), and the cursor round-trips `(3,0)→(3,0)`
  with the toggle on. Nothing filed. This is why the formatter's cursor score
  rose from 24/26 to 26/26.
- **The one-row viewport offset: reduced, not gone.** The cursor is now correct
  and scrollback rose to 99.9%, but the offset persists on the same six
  scrollback-bearing cases. Not filed yet: it lives in the **formatter**, which
  is not the mechanism 3.0 will consume, so it is no longer on the critical
  path. Worth reporting upstream as a courtesy with the mechanism isolated, at
  the priority that implies.

## Corrections to figures in the body above

Both of these are figures the adoption cited. They are wrong at the shipping
configuration and are corrected here rather than left to circulate.

### Throughput: 2.0× the incumbent, not 3.7×

`bench`, `big-spew` replayed 40× in 8 KiB chunks:

| engine | MiB/s | vs vt100 |
|---|---:|---:|
| vt100 | 188.7 | 1.0× |
| **ghostty (shipped, ReleaseSmall)** | **375.7** | **2.0×** |
| ghostty (ReleaseFast, for reference) | 567.0 | 3.0× |
| wezterm | 48.8 | 0.26× |

The body's "678.9 MiB/s, 3.7×" was a `ReleaseFast` build. **The shipped archives
are `ReleaseSmall`, and that is forced rather than chosen:** five `ReleaseFast`
archives gzip to ~16.10 MiB, which exceeds crates.io's 10 MiB cap, while five
`ReleaseSmall` archives make a measured 3.91 MiB `.crate`. So the honest number
is 2.0×, and the ~1.5× throughput given up is the price of `cargo install spyc`
working without a Zig toolchain. Still twice the incumbent, on a path that was
never the bottleneck.

### Memory: ghostty wins by ~3.8× — and it is not parity

The body reports ghostty at 875 KiB/pane against vt100's 9,681 KiB, caveated
because ghostty retained a quarter as much history there. With the limits
working, `bench` at the same 3,988 retained rows gives ghostty **2,635
KiB/pane** against vt100's **9,699** — a real comparison at last.

`membudget` then measures both at a **full** budget, which is the number that
matters: 15,000 heavy rows into a 10,000-row budget, so both must discard.

| engine | MiB/pane | rows retained |
|---|---:|---:|
| vt100 | **26.62** | 10,000 |
| **ghostty** | **6.94** | 9,658 |
| wezterm | 7.25 | 10,000 |

ghostty's derived byte **ceiling** is 9.64 MiB/pane — a cap it does not reach
even on heavy content at a full budget, which is assertion 1 above restated in
memory terms.

**This corrects a reading that had ghostty at parity with vt100 on memory.**
That reading compared ghostty's 9.64 MiB *ceiling* against vt100's 9.68 MiB
*measurement at 3,988 rows* — a cap against a usage, at different row counts.
At equal budgets it is not parity: vt100 allocates its grid eagerly at 32 B/cell
whether or not the rows hold anything, so it costs 26.62 MiB/pane where ghostty
costs 6.94. Twenty-four pane tabs is 639 MiB against 167 MiB.

The adoption case never leaned on memory — it rests on rehydration, robustness
and fidelity — but the figure should be right in the direction it actually
points.

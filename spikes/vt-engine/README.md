# `spikes/vt-engine` — VT engine differential harness

Evidence for [`docs/drafts/VT_ENGINE_SPIKE.md`](../../docs/drafts/VT_ENGINE_SPIKE.md).
Nothing here is production code: this crate declares its own empty
`[workspace]`, so cargo never walks up into spyc's manifest and `make check` on
`main` is untouched. `cargo package` on spyc already excludes it (a nested
package is skipped automatically) — verified, no `exclude` entry needed.

## Building

Three engines, each behind a feature so a machine that cannot build one still
runs the rest.

```sh
cargo build --release                              # vt100 only
cargo build --release --features wezterm           # + wezterm-term (git dep)
GHOSTTY_SOURCE_DIR=/path/to/ghostty \
  cargo build --release --features ghostty         # + libghostty-vt
```

The harness's supported invocation is **`--features ghostty,wezterm`**, and it
is warning-clean there. A single-engine or no-engine build compiles but warns
about unused comparison machinery — a differential harness with nothing to
differentiate against has no work to do, and cfg-gating five binaries to
silence a configuration that cannot produce a result would cost more than it
buys. Build with both.

**Do not build flags into a shell variable** — zsh does not word-split unquoted
parameters, so `cargo build $ARGS` passes `--features a,b` as one argument and
fails; with `-q`/`2>/dev/null` it fails silently and you then measure a stale
binary.

### Option C needs a pinned ghostty checkout

`libghostty-vt-sys 0.2.1`'s build script wants `zig` on PATH and **git-clones
ghostty at a pinned commit into `OUT_DIR`**. That pin (`a887df42`, 2026-07-11)
requires Zig **0.15.x exactly** (ghostty's `requireZig` compares major.minor for
equality), and Zig 0.15.2 cannot link at all on macOS 26. Use
`GHOSTTY_SOURCE_DIR` to point at a checkout that is ABI-compatible with the
crate's checked-in bindings *and* buildable with Zig 0.16:

```sh
git clone --filter=blob:none https://github.com/ghostty-org/ghostty
cd ghostty && git checkout f4c68d65e5008b950c9a2aac9fa928b244dc3b99
```

`f4c68d65` (2026-07-27) is the commit before `03d5fa26` "lib-vt: move scrollback
limits to terminal_set". It still exposes `GhosttyTerminalOptions` (which the
published bindings pass by value) and its `GhosttyTerminalData` enum is a pure
superset of the pinned one. `main` is **not** usable: it replaced the options
struct with scalar `cols, rows` parameters, and the resulting ABI mismatch is
silent — see `src/bin/gprobe.rs`.

`GHOSTTY_SOURCE_DIR` is only re-read when its *value* changes
(`rerun-if-env-changed`), so add or drop a trailing slash to force a native
rebuild after changing the checkout.

## What each binary answers

| binary | question |
|---|---|
| `vt100_panics` | Which of vt100 0.16.2's reported panic classes are live, and which are reachable at spyc's `.max(1)` geometry clamp? |
| `differential` | Fed identical bytes, where do the three engines disagree on the visible screen? |
| `fuzz_diff` | Structured-random escape streams into all three: panics and divergences. Written as an adversary, before conclusions. `-- [iters] [seed]`. |
| `rehydrate` | The 3.0 criterion: dump state, replay into a fresh engine, diff. Grades exact and shift-tolerant row fidelity, cursor, alt-screen, and scrollback depth. |
| `bench` | Throughput (8 KiB chunks, the `PtyHost` reader size), RSS per pane, and `size_of::<vt100::Cell>()`. |
| `adapter_probe` | Is #34 an engine defect or an adapter defect? Transcribes spyc's `PaneWidget` cell walk and `cell_style` and compares them against the engine's own state. |
| `sbprobe` | What unit is each engine's scrollback budget in? |
| `gfmt` | What does libghostty's `Formatter` emit, and what does each option toggle cost in bytes? |
| `grt` | Focused ghostty round trip — isolates the `with_tabstops` cursor-clobber bug. |
| `gprobe` | Geometry readback; the evidence for the silent ABI mismatch against ghostty `main`. |
| `rtdiff` | Prints one case's original vs replayed viewport row by row, so a "0/24 rows" score can be read as an offset rather than a loss. `-- <fixture-name>`. |
| `sendprobe` | Which engines are `Send`? (spyc's `Pane` holds `Arc<Mutex<Parser>>` across a worker thread.) |
| `capture` | Records a PTY byte stream to `fixtures/`. |

Run them all:

```sh
export GHOSTTY_SOURCE_DIR=/path/to/ghostty
cargo run --release --features ghostty,wezterm --bin differential
cargo run --release --features ghostty,wezterm --bin fuzz_diff -- 50000
cargo run --release --features ghostty,wezterm --bin rehydrate
cargo run --release --features ghostty,wezterm --bin bench
cargo run --release --bin adapter_probe
```

## Corpus

**Synthetic** cases live in `src/fixtures.rs` as code, so a reviewer reads the
exact bytes rather than hexdumping a blob. Eighteen cases, each naming the
capability it targets.

**Captured** streams are in `fixtures/*.bin`, all at 24x80, produced by
`src/bin/capture.rs` — which reads the pty master directly, the same seam
spyc's own `SPYC_PTY_DEBUG` dump uses (`PtyHost::drain` → `append_pty_debug`).
Regenerate:

```sh
C=target/release/capture   # or wherever cargo put it
$C htop        24 80 2500  -- htop
$C shell-color 24 80 1200  -- bash -lc 'ls --color=always -la /usr/bin | head -60'
$C cargo-tree  24 80 4000  -- bash -lc 'cargo tree --target all -e features | head -4000'
$C agent-claude 24 80 6000 --send '\x0c' -- claude
$C agent-codex  24 80 6000 --send '\x0c' -- codex
$C agent-agy    24 80 6000 --send '\x0c' -- agy
$C heavy-redraw 24 80 6000  -- ./capture/heavy-redraw.sh
$C big-spew     24 80 20000 -- ./capture/big-spew.sh
```

`capture/heavy-redraw.sh` and `capture/big-spew.sh` are deterministic
generators. `heavy-redraw.sh` reproduces the pattern #34 describes — a DECSTBM
region, CR-rewritten spinner, absolutely-positioned progress bar inside
DECSC/DECRC — synthetic on purpose, because a real agent session is not
reproducible and the question is about the mechanism.

The three `agent-*` captures record **startup only**; none reached its TUI
(claude's stops at a permission warning). They are kept because they are real
interleaved bytes, but they are not "an agent under load" — that is what
`heavy-redraw` is for.

## Compiled-size probe

`sizeprobe/` is a fourth, separate workspace: one binary linked four ways
(no engine / each engine) under spyc's release profile settings, so the size
delta is comparable to what spyc's own binary would gain.

```sh
cd sizeprobe
cargo build --release                            # baseline
cargo build --release --features e-vt100
GHOSTTY_SOURCE_DIR=... cargo build --release --features e-ghostty
cargo build --release --features e-wezterm
```

## Known limitation of these numbers

ghostty's retained scrollback is capped at ~840–1033 rows regardless of the
`max_scrollback` argument, because at the matched commit that option is
mid-refactor (the next commit moves the limit to `terminal_set`, and `main`
splits it into `SCROLLBACK_MAX_BYTES` / `SCROLLBACK_MAX_LINES`). Its
per-pane memory figure is therefore not comparable to the others' at face
value; `bench` prints a per-retained-row column for that reason.

# Committed fuzz seeds

`fuzz/corpus/` is gitignored — CI restores it from an accumulating cache, and a
fresh clone has none. These are the inputs worth keeping regardless: shapes that
once broke something, or that reach a branch a mutator is unlikely to construct
on its own. `make fuzz TARGET=<t>` copies `seeds/<t>/` into `corpus/<t>` before
every run (`cp -n`, so a grown corpus is never clobbered).

A seed is not a test. It only guarantees the shape gets *executed*; whether that
execution is allowed to fail is the target's assertion. Regression coverage for a
fixed bug belongs in a unit test as well — see `src/archive/read/tests.rs` for
the containment cases.

## archive_container

The first byte of each file selects the container flavour (`% 4` →
zip / tar / tar.gz / tar.zst); the rest is the container itself.

| seed | shape |
|---|---|
| `poc_symlink_chain.tar{,.gz}` | Two symlink members that each pass a per-name containment check and compose into an escape, followed by a file member written through it. The seekable and streamed variants take different code paths to the same place. |
| `link_single_hop.tar.gz` | One symlink whose target climbs out directly — the shape a per-name check does catch. |
| `link_absolute.tar.gz` | Symlink to an absolute path. |
| `zip_symlink_out.zip` | Zip symlink member (unix mode bits in `external_attr`) pointing outside. |
| `declared_size_octmax.tar` | Header declares the octal maximum (~64 GB) with no bytes behind it — reaches the allocation that trusts the declaration. |
| `declared_size_2p62.tar` | Same idea via GNU base-256 encoding, which overflows differently. |
| `truncated_tar.tar`, `garbage_gz.tar.gz` | Malformed headers and a gz stream that decompresses to non-tar. |
| `plain.tar`, `plain.tar.gz`, `plain.zip` | Minimal well-formed archives, so the mutator has valid structure to work from. |

## pane_engine

The first two bytes pick the geometry (`rows = b0 % 40 + 1`,
`cols = b1 % 60 + 1`, so `1` is reachable and `0` is not — matching the `.max(1)`
clamp in `Pane::resize` and `pane_spawn_size`). The rest is a *script* for
`fuzz_support::escape_stream`, which selects a sequence shape per byte
(`% 12` → CSI / private mode / SGR / printable / CRLF / DECSC / DECRC / OSC /
APC / charset).

These are the shapes the VT-engine spike found worth reaching, kept so a fresh
corpus does not have to rediscover them.

| seed | shape |
|---|---|
| `one_row_wrap` | text wrapping on a 1-row screen — a live vt100 panic class |
| `one_col_wide` | a double-width glyph on a 1-column screen — likewise |
| `one_by_one` | the 1x1 corner both clamps can produce |
| `cup_past_edge` | CUP well past the right edge, then a tab set. The shape that panicked wezterm-term (wezterm/wezterm#8134); kept because it is cheap and engine-independent |
| `decsc_decrc` | save cursor, move, restore — the DECRC class `[profile.release]`'s net was originally written for |
| `scroll_region` | DECSTBM plus scrolling content, where the incumbent loses a row |
| `unterminated_osc_apc` | OSC and APC with no terminator, which is how they arrive when split across pty reads |
| `charset_switch` | SCS charset selection (DEC special graphics) |
| `mode_churn` | alt screen, bracketed paste, mouse and cursor-key modes toggled densely |
| `zwj_glyph_one_column` | the three bytes that aborted the first version of this target. A ZWJ glyph in a one-column grid panics vt100, and `libfuzzer-sys`'s hook aborted before `catch_unwind` could recover it. Kept because it is the shape that proves the hook scoping is still in place — if that regresses, this seed aborts on the first run rather than after someone rediscovers it |

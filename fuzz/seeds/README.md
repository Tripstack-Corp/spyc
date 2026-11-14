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

## jar_frame

The input is decoded twice: as one frame's body (its first byte the kind, its
second the flags, the rest the payload), and as a stream of whole frames. The
target asserts neither panics nor loops.

| seed | shape |
|---|---|
| `hello_input_upgrade.bin` | An owner's `hello` with a 16-byte token, an `input`, then an `upgrade`: three valid frames, so the mutator starts from the length prefix, the critical flag and a token that fits its own length. `proto.rs`'s ignored `write_fuzz_seed` writes it again, and `crates/spyc-jar/tests/fuzz_seeds.rs` fails when it stops decoding |

## jar_handover

The whole input is a handover blob, the one parser a jar trusts to take over a
live PTY. The target asserts `decode` never panics, and that whatever it
accepts encodes to a blob it accepts again.

| seed | shape |
|---|---|
| `sample.bin` | `handover::sample_blob()`: a valid blob with something in every field, an owner and a reader among its connections. `handover.rs`'s ignored `write_fuzz_seed` writes it again, and `crates/spyc-jar/tests/fuzz_seeds.rs` fails when it differs |

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

## taskfile_make

The whole input is a Makefile's text, lossily decoded. The target asserts the
parser never panics and lists only names a user can type as a goal: none empty,
`.`-led or `-`-led, none holding `%`, `$`, `=` or a control character, none
twice, and at most one default.

| seed | shape |
|---|---|
| `spyc_makefile.mk` | spyc's own `Makefile`: `.PHONY` before each rule, `##` labels, a `$(shell …)` variable, recipes with `$$` |
| `shell_everywhere.mk` | `$(shell touch …)` in a variable, a target name, an include path and a recipe: the never-evaluate fixture |
| `hostile_names.mk` | names a shell or make would misread: `;`, a backtick, `-rf`, `x=y`, quotes, escaped spaces, `\#` and `\:` |
| `conditionals.mk` | nested `ifeq` / `ifdef` / `ifndef` / `else ifeq`, rules in every branch |
| `continuations.mk` | backslash continuations across targets, prerequisites and a `##` label, and a CRLF rule |
| `define_and_eval.mk` | a `define` body shaped like a rule, `$(eval $(call …))`, an empty `define`, a double-colon rule twice |
| `patterns.mk` | a pattern rule, a static pattern, an archive member, a target-specific variable, a `.DEFAULT_GOAL` reference, suffix rules |
| `bsd.mk` | BSD make's `.if` / `.elif` / `.for` / `.include <…>` / `.MAIN` |
| `nul_and_controls.mk` | a NUL, a bell and an escape sequence inside target names |
| `unterminated.mk` | an unclosed `ifeq (`, `define`, `$(shell` and `${`, and bare `:` / `::` / `:=` lines |

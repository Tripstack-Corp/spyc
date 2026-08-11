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

The first byte of each file selects the container flavor (`% 4` →
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

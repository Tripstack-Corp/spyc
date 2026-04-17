# spyc

A vi-keyboard-driven file commander for macOS and Linux that pairs with
[Claude Code](https://www.anthropic.com/claude-code). Inspired by SideFX's
in-house `spy` tool — hence the name: **spy** + **c**laude = **spyc** (spicy 🌶️).

Goals:

- **Vi muscle memory.** `hjkl` to move, single-letter verbs for actions, counts
  (`5j`, `10G`), `gg`/`G`, `.` repeat. Keys never require the mouse.
- **spy parity on core verbs.** Pick / take / inventory, `%` substitution in
  shell commands, `.spyrc`-style key remapping, ignore masks.
- **Static, portable binary.** One file you can `scp` to a server.
- **Eventual Claude Code split pane.** Send selections into an embedded
  `claude` pty, get back a coding agent alongside the file list.

## Status

**M1 — Skeleton navigator.** Implemented.
**M2 — Picks, inventory, ignore masks.** Implemented.
**M3 — Shell-out with `%` substitution, EDITOR / PAGER, `$SHELL`.** Implemented.

### Keys (so far)

| Key | Action |
|---|---|
| `h` `j` `k` `l`, arrows, `<Space>`, `<Enter>` | Motion (counts supported: `5j`, `10k`) |
| `gg` / `G` | Top / bottom |
| `^B` / `^F`, PageUp/PageDown | Page up / down |
| `e` / `v` / `d` / `<Enter>` on dir | Enter directory |
| `u` / `-` | Climb to parent |
| `H` / `~` | Home directory |
| `t` | Toggle pick on current entry |
| `T` | Pick by glob pattern (prompts) |
| `^T` | Pick all / clear picks |
| `y` / `Y` | Take (picked files → inventory; else cursor file) |
| `p` | Drop cursor item from inventory |
| `i` | Toggle inventory view |
| `z` | Empty inventory |
| `a` | Toggle ignore mask 1 (default: dotfiles) |
| `o` | Toggle ignore mask 2 (default: build artifacts) |
| `!` / `;` | Prompt for a shell command (`%` → selection) |
| `$` | Drop into `$SHELL` in the current directory |
| `^W` / `^X` | `chmod +w` / `+x` on the selection |
| `d` / `<Enter>` on file | Open in `$PAGER` (default `less`, text files only) |
| `v` / `e` on file | Open in `$EDITOR` / `$VISUAL` (default `vi`) |
| `^D` / `q` / `Q` | Quit |
| `^L` | Redraw |
| `<Esc>` (in prompt) | Cancel prompt |
| `<Enter>` (in prompt) | Submit prompt |
| `^u` / `^w` (in prompt) | Clear / delete word |

The header shows live state: `picks:N inv:N m1:on m2:on`.

## Development

```
cargo run
```

## Static release builds

This repo provides a `Justfile` (install [`just`](https://github.com/casey/just))
for reproducible, statically linked release builds.

```
just build-linux-x86       # Linux x86_64, musl, static
just build-linux-arm       # Linux aarch64, musl, static
just build-macos-universal # macOS fat binary (x86_64 + arm64)
just release-all           # all of the above
```

### Why musl on Linux

Linking against glibc produces binaries that only run on systems whose glibc
is at least as new as the build host's. musl is a fully static C library, so
the resulting `spyc` runs on any Linux kernel ≥ the target triple's baseline
without any shared-library dependencies. Check with:

```
ldd target/x86_64-unknown-linux-musl/release/spyc
# -> "statically linked"
```

### Why a universal macOS binary

Apple Silicon and Intel Macs run different architectures. A single universal
(`lipo`) binary runs on both with no per-machine build step.

## Roadmap

- **M4** — `.spycrc` keymap DSL and `^R` reload.
- **M5** — Search (`/`, `n`, `N`), marks (`m{a-z}`, `'{a-z}`), `J` jump.
- **M6** — File operations (`c`, `m`, `R`, `N`, `L`).
- **M7** — Previews, git status, true-color theming, optional mouse.
- **M8** — Claude Code split pane.

## Credits

Logo uses [Twemoji](https://github.com/jdecked/twemoji) pepper artwork
(CC-BY 4.0).

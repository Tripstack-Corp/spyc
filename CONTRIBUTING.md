# Contributing to cspy

Thanks for your interest in contributing. This document covers the
workflow, standards, and conventions for the project.

## Getting started

```sh
git clone git@bitbucket.org:tripstack/cspy.git
cd cspy
make doctor    # check prerequisites (rustc, cargo, zig, etc.)
cargo build    # dev build
cargo test     # run all tests
```

See `INSTALL.md` for font and terminal setup.

## Pull request workflow

We use Bitbucket pull requests. The target branch is `main`.

1. **Create a feature branch** from `main`:
   ```sh
   git checkout -b feature/short-description
   ```

2. **Make your changes.** Follow the conventions below.

3. **Run the quality gate** before pushing:
   ```sh
   make check    # fmt + clippy + test
   ```
   All three must pass. Clippy runs with `pedantic` and `nursery`
   lints — warnings are errors in CI.

4. **Push and open a PR** on Bitbucket:
   ```sh
   git push -u origin feature/short-description
   ```
   Then create the PR at
   `https://bitbucket.org/tripstack/cspy/pull-requests/new`.

5. **PR title** should be concise (under 70 chars). Use the body for
   details. Format:
   ```
   ## Summary
   - Bullet points of what changed

   ## Test plan
   - How to verify the change
   ```

   **Bug fix PRs** should include relevant debug log output.
   Run `cspy --debug` to reproduce the issue, then attach the
   relevant lines from the `/tmp/cspy-debug-*.log` file.

6. **One approval required** before merge. Squash-merge preferred to
   keep `main` history clean.

## Code conventions

### Action dispatch

Every user-visible feature maps to an `Action` enum variant in
`src/keymap/action.rs`. The flow:

1. Add a variant to `Action` with a doc comment.
2. Add a `describe()` entry (used in the help overlay).
3. Wire the keybinding in `src/keymap/resolver.rs`.
4. Handle it in the `apply()` match in `src/app.rs`.

### Error handling

- **Never crash on user actions.** Navigation into an unreadable
  directory, failed git commands, missing files — flash an error
  and stay put.
- Use `flash_error()` for user-facing errors, not `?` propagation
  from key handlers.
- `anyhow::Result` is fine for startup and teardown where crashing
  is acceptable.

### Repaint strategy

Use the one-shot `needs_full_repaint` flag when closing overlays,
pagers, or anything that leaves ratatui's diff buffer stale. Do NOT
use `terminal.clear()` per frame.

### Pane I/O

- Keys go through `input::encode_key()`.
- Raw bytes use `pane.send_bytes()`.
- Paste uses bracketed paste: `\x1b[200~`...`\x1b[201~`.
- Resize checks `last_size` to avoid redundant ioctl/SIGWINCH.

### Performance

- Don't call `std::env::var()` in hot paths (render loop, drain
  loop). Cache at construction time.
- Skip redundant work: check if values changed before updating
  (e.g., pty resize).
- Pager and list view only build visible slices, not the full
  content.

## Keep docs in sync

When your change affects user-visible behavior, keybindings, or
project status, update **all** of:

- `FEATURES.md` — what the app does
- `ROADMAP.md` — move items to Done, add new plans
- `CLAUDE.md` — architecture and conventions
- `src/ui/help.rs` — the `?` help overlay
- `docs/presentation.html` — stats and feature lists (if significant)

This is a hard requirement, not a nice-to-have. Stale docs are bugs.

## Versioning

We use [SemVer](https://semver.org/). The version lives in
`Cargo.toml`.

- **Patch** (0.9.1): bug fixes, doc updates, minor polish
- **Minor** (0.10.0): new features, new keybindings, new milestones
- **Major** (1.0.0): stable release, public API commitment

Bump the version in your PR if your change is user-visible.

## Commit messages

Format:
```
Short summary (under 72 chars)

Optional longer description. Focus on the "why" not the "what".

Co-Authored-By: Claude <noreply@anthropic.com>
```

Include the `Co-Authored-By` trailer when Claude Code contributed.

## CI

Bitbucket Pipelines runs `make check` on every push. The pipeline
must pass before merge.

## Cross-compilation

```sh
make doctor           # verify toolchain
make release          # current platform
make deploy-fika      # Linux x86_64 → fika-vm
make dist             # all platforms → dist/
```

The Makefile touches `src/main.rs` before zigbuild to avoid stale
cross-compile caches.

## Project structure

```
src/
  app.rs          — event loop, layout, key dispatch (the big file)
  keymap/         — Action enum, resolver, user keymap DSL
  pane/           — pty subprocess, multi-tab, vt100 rendering
  ui/             — list view, pager, status bar, prompt, theme
  fs/             — directory listing, file operations
  state/          — cursor, marks, picks, inventory, history
  config/         — TOML config, DSL parser
  shell/          — shell expansion, command execution
  sysinfo.rs      — git status, worktree helpers, system info
  main.rs         — terminal setup/teardown
```

## Questions?

Open an issue on Bitbucket or reach out to Derek Marshall.

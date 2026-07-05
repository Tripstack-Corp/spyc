# Contributing to spyc

Thanks for your interest in contributing. This document covers the
workflow, standards, and conventions for the project.

## Getting started

```sh
git clone git@github.com:Tripstack-Corp/spyc.git
cd spyc
make doctor    # check prerequisites (rustc, cargo, zig, etc.)
cargo build    # dev build
cargo test     # run all tests
```

See [BUILD.md](BUILD.md) for the full toolchain, build, and
cross-compilation reference, and [INSTALL.md](INSTALL.md) for terminal
and font setup.

## Pull request workflow

We use GitHub pull requests. The target branch is `main`.

1. **Create a feature branch** from `main`:
   ```sh
   git checkout -b feature/short-description
   ```

2. **Make your changes.** Follow the conventions below.

3. **Run the quality gate** before pushing:
   ```sh
   make check    # fmt-check + lint + test + deny
   ```
   All four must pass. Clippy runs with `pedantic` and `nursery`
   lints — warnings are errors in CI. `deny` is cargo-deny
   (advisories, licenses, sources, bans). If you touched
   `cfg(target_os = "linux")` code, also run `make lint-linux` —
   host clippy compiles that code out. `make aislop` is an advisory
   AI-slop scan (net-new findings vs the baseline).

4. **Push and open a PR** on GitHub:
   ```sh
   git push -u origin feature/short-description
   gh pr create              # or open one from the GitHub UI
   ```
   The PR targets `main` at
   `https://github.com/Tripstack-Corp/spyc/pulls`.

5. **PR title** should be concise (under 70 chars). Use the body for
   details. Format:
   ```
   ## Summary
   - Bullet points of what changed

   ## Test plan
   - How to verify the change
   ```

   **Bug fix PRs** should include relevant debug log output.
   Run `spyc --debug` to reproduce the issue, then attach the
   relevant lines from the `/tmp/spyc-debug-*.log` file.

6. **Merge.** `main` is branch-protected: a PR is required and the CI
   checks (lint + tests) must pass, then **squash-merge** to keep the
   history clean. As a solo-maintained project `main` does not require a
   separate approving review (the maintainer can't approve their own PR);
   external contributions are reviewed by the maintainer before merge.

## Code conventions

### Action dispatch

Every user-visible feature maps to an `Action` enum variant in
`src/keymap/action.rs`. The flow:

1. Add a variant to `Action` with a doc comment.
2. Add a `describe()` entry (used in the help overlay).
3. Wire the keybinding in `src/keymap/resolver/mod.rs`.
4. Handle it in the matching `src/app/` child module — or, for the
   pure-domain half, in `AppState::apply` (`src/app/state/apply.rs`).

`:`-commands are `COMMAND_TABLE` entries (`src/app/command_table.rs`);
each entry carries its handler, so a registered command with no
handler is a compile error. Side effects are `Effect` variants
(`src/app/effect.rs`) returned as data — handlers never touch the OS
directly. `AGENTS.md` is the full architectural contract (MVU
invariants + conventions); read it before adding behavior.

### File size

No `.rs` file over ~800 lines without a solid reason (a module root
holding its own core type definitions qualifies; a pile of helpers
does not). When a file grows, extract a cohesive child/sibling module
— verbatim relocation, behavior-identical. `app/mod.rs` has a
ceiling-guard test (`app::guard_tests::mod_rs_stays_decomposed`):
if you hit it, extract a module, don't bump the number.

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
- `ROADMAP.md` — strategy/direction only; file or close a **GitHub Issue** for per-item work
- `AGENTS.md` — architecture and conventions
- `src/ui/help.rs` — the `?` help overlay
- `docs/assets/presentation.html` — stats and feature lists (if significant)

This is a hard requirement, not a nice-to-have. Stale docs are bugs.

## Versioning

We use [SemVer](https://semver.org/). The version lives in
`Cargo.toml` (currently 1.x).

- **Patch** (1.57.1): bug fixes, doc updates, minor polish
- **Minor** (1.58.0): new features, new keybindings, new milestones
- **Major** (2.0.0): breaking changes to config, state files, or
  the command surface

Bump the version in your PR if your change is user-visible.

## Commit messages

Conventional commits, enforced by use: since v1.57.0 the
`CHANGELOG.md` is git-cliff-generated from the commit history
(config in `cliff.toml`), so **the commit subject IS the changelog
entry**. Format:

```
type(scope): subject — the literal changelog line

Optional body. Focus on the "why" not the "what".

Co-Authored-By: Claude <noreply@anthropic.com>
```

- `type` picks the section: `feat:` → Features, `fix:` → Bug Fixes,
  `refactor:`/`perf:`/`docs:`/`build:` → theirs.
- The subject must cover the *actual* diff scope, not just its
  headline. A category-spanning PR wants multiple well-typed
  commits, not one.
- Preview the pending section with `make changelog`; cut a release
  with `make release-tag VERSION=x.y.z`.
- Include the `Co-Authored-By` trailer when Claude Code contributed.

See `AGENTS.md` ("Commits, merges, and CHANGELOG") for the longer
rationale.

## Merge-train conflicts

Because every PR bumps `version`, concurrent PRs collide on that one
`Cargo.toml` / `Cargo.lock` line. spyc ships a git merge driver
(`src/merge_driver.rs`) that resolves it automatically — it keeps the
higher semver on rebase, so a merge-train rebase only stops on a *real*
conflict. The driver is installed into the repo's git config the first
time you launch spyc in the repo (idempotent); to set it up without
launching, the `.gitattributes` driver name is `spyc-semver`. After a
driver-assisted rebase, run `cargo build` once so `Cargo.lock` (which
uses `merge=ours`) picks up the resolved version.

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs the quality gate on every PR
(fmt + clippy + deny as one job, tests in parallel) and adds a coverage gate on
merges to `main`. All jobs must pass before merge. A weekly
`.github/workflows/audit.yml` re-runs `cargo deny check advisories` against a
fresh RUSTSEC DB. CI pins the toolchain via `rust-toolchain.toml`, so it matches
your local `make check`.

## Cross-compilation

```sh
make doctor           # verify toolchain
make release          # current platform
make dist             # all platforms → dist/
```

The Makefile touches `src/main.rs` before zigbuild to avoid stale
cross-compile caches. See [BUILD.md](BUILD.md) for the full set of
cross-compile and packaging targets (`.deb`, checksums, signing).

## Project structure

```
src/
  app/            — the application layer (MVU): mod.rs is the module
                    root (App/Runtime/ViewState + Message); ~40 child
                    modules incl. state/ (the Model), render/,
                    key_dispatch/, pager_handler/, run.rs, effect.rs,
                    command_table.rs
  keymap/         — Action enum, resolver, user keymap DSL
  pane/           — pty subprocess, multi-tab, vt100 rendering
  ui/             — list view, pager, status bar, prompt, theme
  fs/             — directory listing, file operations, finder, grep
  state/          — cursor, marks, picks, inventory, history, sessions
  git/            — git facade, 100% in-process gix (no subprocess)
  agent/          — agent profiles (claude/codex/gemini/agy/zot)
  mcp/            — MCP server (PID-scoped socket + stdio proxy)
  config/         — TOML config, DSL parser
  shell/          — shell expansion, command execution
  clipboard.rs    — cross-platform clipboard copy
  proc_cwd.rs     — live "cwd of pid N" lookup for the pane divider
  sysinfo.rs      — RSS / PID / thread-count info for the I overlay
  main.rs         — terminal setup/teardown
```

See `AGENTS.md` for the full per-module index.

## Questions?

Open an issue on GitHub or reach out to Derek Marshall.

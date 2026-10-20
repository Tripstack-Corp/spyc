# Contributing to spyc

Thanks for your interest in contributing. This document covers the
workflow, standards, and conventions for the project.

## Getting started

```sh
git clone git@github.com:Tripstack-Corp/spyc.git
cd spyc
make doctor         # check prerequisites (rustc, cargo, zig, etc.)
make install-hooks  # pre-commit hook — see below
cargo build         # dev build
cargo test          # run all tests
```

**Install the pre-commit hook.** It runs `make check` before each commit, and
it does one thing nothing else can: it `unset`s git's redirect environment
(`GIT_DIR` and friends) before the gate runs, so every tool the gate invokes is
covered — not just spyc's own git calls. Skipping that once cost a multi-day
investigation, after `cargo-deny`'s advisory-db refresh inherited `GIT_DIR` from
the hook and hard-reset the branch under it.

Re-run `make install-hooks` whenever the template changes; the installed copy is
a snapshot, not a link. You don't have to track that yourself — `make check`
fails if the two have drifted (`git::commit_hook_is_current`). Bypass a single
commit with `git commit --no-verify`; don't make a habit of it.

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
   make check    # fmt-check + lint + test
   ```
   All three must pass. Clippy runs with `pedantic` and `nursery`
   lints — warnings are errors in CI. Supply-chain checks
   (`make deny` — cargo-deny advisories/licenses/sources/bans) are
   **not** in `check`: they re-fetch the RUSTSEC DB over the network,
   which is the wrong thing to do on every commit. `audit.yml` owns
   them; run `make deny` by hand if you changed a dependency. If you touched
   `cfg(target_os = "linux")` code, also run `make lint-linux` —
   host clippy compiles that code out. If you touched anything under
   `.github/workflows/`, run `make lint-workflows` (actionlint +
   shellcheck; `brew install actionlint shellcheck`) — CI runs it in
   the lint job. Prose — docs and comments alike — is
   Canadian English (`colour`, `centre`, but `organize`/`analyze`);
   `prose_is_canadian_english` enforces it, and identifiers are exempt. `make aislop` is an advisory AI-slop scan (net-new
   findings vs the baseline).

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
invariants + conventions); read it before adding behaviour.

### File size

No `.rs` file over 1,000 lines of non-test code, or 3,000 lines in
all. When a file grows, extract a cohesive child/sibling module —
verbatim relocation, behaviour-identical. Tests get the larger
allowance because they are flat lists of independent cases; past it,
split them by theme.

`src/size_guard.rs` enforces both limits as a ratchet: a file that was
over a limit when the guard landed is pinned at its length then and may
only shrink. If you hit it, extract a module; don't raise the pin. A
new pin needs a solid reason in the PR (a module root holding its own
core type definitions qualifies; a pile of helpers does not).
`app/mod.rs` keeps its own tighter ceiling
(`app::mod_tests::guard_tests::mod_rs_stays_decomposed`).

### Function length

No function over 100 lines of code: the body's lines that hold code,
so blank lines and comments are free. That is clippy's
`too_many_lines` count at its default threshold, and test code is
exempt. When a function grows, extract a cohesive helper; a long
`match` usually splits into one function per arm group.

`src/fn_size_guard.rs` enforces it the same way `size_guard.rs`
enforces file size: the functions that were over the limit when it
landed are pinned by name (`path::Type::fn`) and may only shrink. If
you rename or move a pinned function, move its pin with it.

### Error handling

- **Never crash on user actions.** Navigation into an unreadable
  directory, failed git commands, missing files — flash an error
  and stay put.
- Use `flash_error()` for user-facing errors, not `?` propagation
  from key handlers.
- `anyhow::Result` is fine for startup and teardown where crashing
  is acceptable.

**A flashed error carries its CAUSE.** anyhow's plain `{e}` prints only the outermost context and drops everything beneath — that's how a macOS privacy denial reached a user as the unactionable `chdir: reading directory <path>`. Use `{e:#}` (the alternate form renders the whole `source()` chain; harmless for a plain `io::Error`, whose Display ignores the flag). Pair it with context AT the failure site — a bare `?` on an fs call yields "No such file or directory" naming no path. Guard: `flashed_errors_render_their_whole_chain`.

### Repaint strategy

Use the one-shot `needs_full_repaint` flag when closing overlays,
pagers, or anything that leaves ratatui's diff buffer stale. Do NOT
use `terminal.clear()` per frame.

### Pane I/O

- Keys go through `input::encode_key()`, which takes the child's DECCKM state
  (`Pane::application_cursor`) so cursor keys use the form that child asked for.
- Raw bytes use `pane.send_bytes()`.
- Paste uses bracketed paste: `\x1b[200~`...`\x1b[201~`.
- Resize checks `last_size` to avoid redundant ioctl/SIGWINCH.

**Pane I/O:** keys via `input::encode_key(ev, app_cursor)` — the mode arg is the child's DECCKM (`Pane::application_cursor`), so unmodified arrows/Home/End go out as SS3 (`ESC O A`) to a child that asked for application cursor keys and CSI to everyone else; modified arrows stay CSI in both. Raw bytes via `pane.send_bytes()`, paste wrapped in `\x1b[200~`…`\x1b[201~`. Prefix `^a` (`^w` alias); `^a ↓` sends a literal `^a` to the child. Panes `exec` the command (`PtySpec.exec_replace` → `$SHELL -i -c 'exec <cmd>'`, no job-control wrapper shell), so on any **non-shell** tab `^z` toggles a spyc-managed suspend — `Effect::SignalPane` sends `SIGSTOP`/`SIGCONT` to the pty foreground group (the child), `TabInfo.suspended` → 💤. `SIGSTOP` not `SIGTSTP` (Claude catches `SIGTSTP` → its self-suspend handler trips the macOS false-exit `[exited 146]`); `exec` so no wrapper reclaims the tty on resume. Pure decision `pane_suspend_key_action`, gated on `shell::command_is_shell`: only a shell tab forwards `^z`, so its own job control still backgrounds a foreground job — a raw-mode TUI has `ISIG` off and would just discard the byte (captures/tasks keep the wrapper — no `exec`). The whole invocation policy is one pure fn, `shell::pane_invocation`: it also drops the wrapper's `-i` when the pane command is *itself* an rc-sourcing shell (`^a c` → `zsh`), because `exec` preserves the pid and a second rc pass collides with rc state keyed on it — SPYC-TRAP `pane-shell-rc-double-source`.

### Performance

- Don't call `std::env::var()` in hot paths (render loop, drain
  loop). Cache at construction time.
- Skip redundant work: check if values changed before updating
  (e.g., pty resize).
- Pager and list view only build visible slices, not the full
  content.

### Prose

**Prose is Canadian English** — `colour`, `centre`, `defence`, `travelled`, `honoured`, `grey` — but with the American `-ize`/`-yze` endings kept (`organize`, `analyze`, never `organise`). That combination is what makes it Canadian rather than British, and `-ise` is wrong here in both directions. It governs **prose only**: markdown body text and comment bodies. Code is whatever upstream named it, so `Color::Rgb`, `color_depth.rs`, `--color` and `$COLORTERM=truecolor` all stay exactly as they are. Guard: `prose_is_canadian_english` (`src/style_guard.rs`), which skips code spans, fences, blockquotes (verbatim quotes), table rows and HTML attribute values — `<p align="center">` has one legal spelling. Two words are deliberately not policed because a machine can't call them: `meter` (a gauge is a meter, only the SI unit is a metre) and the `licence`/`license`, `practice`/`practise` noun-verb splits. Exempt wholesale: `CHANGELOG.md` (git-cliff regenerates it from commit subjects), `CODE_OF_CONDUCT.md` (a vendored canonical document, worth more recognizable than house-styled) and `docs/archive/` (an archive records what was written when; re-spelling one rewrites history). **Backticks are the contract**: the guard skips code spans, so a *bare* code identifier in prose is outside its reach entirely — config keys, flags, env vars, file names and API items wear backticks in prose, always. That is not decoration. The sweep that installed this rule read a bare `[colors]` as prose, Canadianized it, and shipped a comment naming a config section that does not exist, while the same token three files away survived untouched because it was in backticks. Which is the general shape, and this was its third instance: **a checker derived from the transformer's own model inherits that model's blind spots**, so the two agreeing is not evidence — see `binds_after` in `src/style_guard.rs` for the sentence-final period that escaped both.

**Comments state what IS, not what's planned.** No "for now" / "until X lands" / "with the Y PR" — they rot into lies. A comment earns its place by explaining a non-obvious decision/invariant/gotcha; never narrate what the code says, never commit reasoning-in-progress. Guard: `comments_carry_no_reasoning_leakage` covers the reasoning-in-progress half plus the literal "for now"; the other temporal shapes are convention only, because no needle separates them from legitimate prose about when a value settles.

### Guards and tests

**Test the requirement, not the implementation** (charter: `docs/archive/TESTING_STRATEGY.md`): start from an invariant, add negative tests, reach for `proptest` on wide input spaces, and generate edge-case *data* (not assertions) with AI. Decouple assertions from struct layout — assert intent via the test-only matchers in `effect.rs` (`fx.change_dir()`, …), the single place that destructures (with `..`).

**A test is evidence only once you've watched it fail.** Break what it covers, confirm it reports, put it back. Green proves nothing about code a test never reaches: the pre-2.1 review recorded two findings closed while they were still live — one fixed at two of its three named sites, one adding a bounded function no caller used. **A finding naming N sites is closed when N are fixed**, and when the fix is a helper, pin the *call site* too (`every_declared_size_allocation_is_capped` is that shape). **A differential comparison must normalize both sides through one function** — an instrument that shares the subject's model inherits its blind spots: the VT-engine spike's `adapter_probe` skipped wide continuation cells on the engine side and not on the buffer side, and reported a correct rendering as a defect that reached a decisions-log entry and two issues before the probe itself was checked.

**A source-scan guard splits production from tests via `guard_support::production_half`** — never `split("#[cfg(test)]")`. That shortcut fails open twice: a comment *mentioning* the attribute truncates the scan (it read `render/mod.rs` as 22 production lines of 827), and a mid-file `#[cfg(test)] mod x;` hides everything after it (90% of `git/worktree.rs`). A guard that silently checks nothing is worse than no guard.

## Keep docs in sync

When your change affects user-visible behaviour, keybindings, or
project status, update **all** of:

- `docs/features/` — what the app does, one page per area; `FEATURES.md` is
  their overview, so touch its summary when an area changes
- `ROADMAP.md` — strategy/direction only; file or close a **GitHub Issue** for per-item work
- `AGENTS.md` — only a new module's index line or a new rule; a feature's detail
  goes in `FEATURES.md`, and `agents_md_stays_slim` caps the file
- `docs/KEYBINDINGS.md` — the full keymap reference (mirrors the `?` overlay)
- `src/ui/help.rs` — the `?` help overlay
- `docs/assets/presentation.html` — stats and feature lists (if significant)

This is a hard requirement, not a nice-to-have. Stale docs are bugs.

A user page (`docs/features/`, `docs/config/`) says what a user sees and
does. How it works belongs in the module's `//!` doc or `ARCHITECTURE.md`,
where the people changing it look. A new page gets a summary and a link in
its hub (`FEATURES.md` or `CONFIGURATION.md`), every `:` command is named on
a user page, and both hubs have a size cap; guards check all three.

Moving or splitting a doc? `every_relative_markdown_link_resolves`
(`src/link_guard.rs`) fails on any relative link or `#anchor` left
pointing at nothing. A prose reference such as
`ARCHITECTURE.md → "MCP server"` isn't a link, so search for those by
hand.

## Versioning

We use [SemVer](https://semver.org/). The version lives in
`Cargo.toml`.

- **Patch** (2.0.1): bug fixes, doc updates, minor polish
- **Minor** (2.1.0): new features, new keybindings, new milestones
- **Major** (3.0.0): breaking changes to config, state files, or
  the command surface

**Don't bump the version in your PR.** `main` is the CURRENT stream
(FreeBSD's `-CURRENT`; see `docs/RELEASE_ENGINEERING.md`) and carries the
*next* minor with a `-CURRENT` suffix — e.g. `2.1.0-CURRENT` while 2.1 is
in development. It stays there for the whole cycle: the release PR is
what strips the suffix down to `2.1.0`. The one exception is a release
candidate's soak, when `main` carries the rc's version (`2.1.0-rc.1`), set
by the rc's own release PR.

So a feature PR touches no version line at all. Two things follow:

- You can tell a dev build from a release at a glance — `spyc --version`
  prints `2.1.0-CURRENT (<sha>)`, and the SHA identifies the exact build,
  which is what per-PR bumps used to be for.
- Concurrent PRs can no longer collide on the version line (see
  *Merge-train conflicts* below).

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
  headline.
- **Under squash merge the PR title becomes that subject**, and
  `split_commits = false` means the body is never parsed — so
  extra well-typed commits inside a PR shape the git log, not the
  changelog. A category-spanning PR gets one section: split it into
  a PR per type when both halves are worth finding.
- Preview the pending section with `make changelog`; cut a release
  with `make release-prep` then `make release-tag` (see
  `docs/RELEASE_ENGINEERING.md` § The release cycle).
- Include the `Co-Authored-By` trailer when Claude Code contributed.

**Commit subject = actual scope, not its caption.** A commit touching a feature + a version bump says both (`feat: agy agent + bump cargo-deny`). The body holds the long form. **It must be a conventional `type(scope): subject`** — `filter_unconventional` drops anything else, so an untyped subject silently vanishes from the CHANGELOG and can't be fixed after a squash-merge (see `cliff.toml`'s retro-typing preprocessors for the cleanup that costs).

**`CHANGELOG.md` is git-cliff-generated** (config `cliff.toml`, whose `header` it must open with exactly): the section comes from the commit *type*, the line from `scope: subject` — so **the commit message _is_ the changelog entry**. Under squash merge that message is the **PR title**, and `split_commits = false` means the body is never parsed: well-typed commits *inside* a PR reach the log, not the changelog. A category-spanning PR therefore gets ONE section — split it into a PR per type, or accept that the title is what ships (#434 buried a `feat(mouse)` under `fix(graveyard)` this way). Releases before 2.0.0 are frozen in `docs/changelog/1.x.md` (v1.56.0 and earlier hand-written, left verbatim). Preview with `make changelog`; release in two steps — `make release-prep VERSION=x.y.z` on a release branch, then `make release-tag VERSION=x.y.z` on `main` once that PR merged. An rc takes the same two steps with `VERSION=x.y.z-rc.N` and gets no CHANGELOG section: `tag_pattern` matches stable tags only, so the final section covers the whole cycle.

## Merging a PR

**Squash on merge** (`gh pr merge <N> --squash`) — `main`'s log becomes one commit per shipped shape.

**Merging straight after `gh pr update-branch` races the checks.** The update pushes a new head, which re-triggers CI; merge before GitHub has registered the new runs and it refuses with `2 of 2 required status checks are expected`. Branch protection requires up-to-date *and* green, so a sequence of PRs hits this on every one after the first. **Gate on `mergeStateStatus`, never on the check list** — it is what GitHub's own merge button reads, folding up-to-date and green into one answer:

```sh
for _ in 1 2 3 4 5 6 7 8; do
  st=$(gh pr view <N> --json mergeStateStatus --jq .mergeStateStatus)
  case "$st" in
    CLEAN)           gh pr merge <N> --squash && break ;;
    BEHIND)          gh pr update-branch <N>; sleep 15 ;;
    BLOCKED|UNKNOWN|UNSTABLE) sleep 30 ;;   # checks running, or still computing
    *)               echo "stopping on $st"; break ;;   # DIRTY = conflicts
  esac
done
```

No `--delete-branch`: the repo's `delete_branch_on_merge` already removes the remote branch, and the flag also deletes the *local* one — after #497 and #503 the worktree was gone along with it before `remove_worktree` could run. Tear down with `remove_worktree`, which keeps a squash-merged branch's ref (its commit isn't an ancestor of the squash), then `git branch -D <branch>` once every file the branch touched matches `main`:

```sh
git diff -z --name-only $(git merge-base <branch> main) <branch> |
  xargs -0 git diff --stat <branch> main --   # empty == landed
```

Scoped to the branch's own files, because a whole-tree `git diff <branch> main` is never empty after the loop ran `gh pr update-branch`: the local ref lacks what that merged in (#511 showed 37 files for a 5-file PR). NUL-separated, because some tracked paths contain spaces and an unquoted list silently drops them — as does collecting the list in a zsh variable first, which zsh passes as one argument that matches nothing.

Bound the loop: `BLOCKED` covers *running* and *failed* alike, so an unbounded wait spins forever on a red PR. `gh pr checks` has three fail-open shapes and all three have shipped here — an empty list (`all` over nothing is `true`, reported as `no checks reported on the branch`), a truncated list, and the one that beat the guard against those two on both PRs of the #467-#469 train: the **previous** head's checks, still reported and still green. A check list says what you know about some commit; `mergeStateStatus` says whether this PR may merge now.

Resolve `<N>` from the head branch (`gh pr view <branch> --json number,headRefName`), never from a guess at the next number: a guessed one merged a dependabot PR nobody had approved. `UNSTABLE` means a check that isn't required is still running or has failed. `Vendored archives` runs for about ten minutes whenever `Makefile` changes, so wait it out rather than merge past it, and look at it if it stays.

## Merge-train conflicts

Since `main` sits on a static `-CURRENT` version, no PR touches the
version line and concurrent PRs can't collide on it — the conflict this
section exists for doesn't arise on CURRENT any more.

spyc still ships the git merge driver (`src/merge_driver.rs`) that
resolves it, for release branches where patch bumps resume: it keeps the
higher semver on rebase, so a merge-train rebase only stops on a *real*
conflict. The driver is installed into the repo's git config the first
time you launch spyc in the repo (idempotent); to set it up without
launching, the `.gitattributes` driver name is `spyc-semver`. After a
driver-assisted rebase, run `cargo build` once so `Cargo.lock` (which
uses `merge=ours`) picks up the resolved version.

Note the driver parses plain `MAJOR.MINOR.PATCH` only. A conflict against
a `-CURRENT` line is declined and surfaces as a real conflict — correct,
because a version-line conflict on CURRENT means something unexpected
happened and wants a human.

## Git traps

**Never `git reset FETCH_HEAD`.** A bare `git fetch` writes *every* fetched branch into `FETCH_HEAD`; `reset` consumes only the **first line**, which is alphabetical — and every line is marked `not-for-merge`, which `reset` ignores. In this repo's worktree-per-PR layout that means a session on `feat/x` resets to whatever `ci/…` or `docs/…` branch someone else pushed. Name the ref instead — `git fetch origin <branch> && git reset origin/<branch>` — or use `gh pr update-branch <N>`, which is server-side and races nothing local. (`FETCH_HEAD` is per-worktree, so the file is yours; the *content* is whatever your last bare fetch pulled.)

**A same-shaped but different-cause incident, observed and fixed 2026-08-07:** worktrees were repeatedly reset to an unrelated branch's tip mid-commit, with no one ever having typed the command above. Root cause: `git` exports `GIT_DIR` into every hook's environment, and `GIT_DIR` overrides `-C`. `cargo-deny`'s advisory-db self-update (`git -C ~/.cargo/advisory-dbs/<hash> fetch && git reset --hard FETCH_HEAD`, run by `make check`'s `deny` target from inside the pre-commit hook) inherited it, so the reset silently landed on spyc's own checked-out branch instead of the advisory-db clone it meant to update — the working tree still went to the advisory-db's cwd (a "split-brain" reset), which is why files on disk survived while the branch ref didn't. Same root cause as C9 (#248, which hit the test suite's own scratch-repo git spawns — see `src/git/test_support.rs::git_command` / `GIT_REDIRECT_ENV`). Fixed at the hook boundary (`scripts/git-hooks/pre-commit` unsets the same var list before `exec make check`) so every tool the gate invokes is covered, not just spyc's own git spawns. Re-run `make install-hooks` if your local `.git/hooks/pre-commit` predates this fix.

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs the quality gate on every PR
(fmt + clippy as one job, tests in parallel) and adds a coverage gate on
merges to `main`. All jobs must pass before merge. CI pins the toolchain via
`rust-toolchain.toml`, so it matches your local `make check`.

**The lint job also lints the workflows themselves** (`make lint-workflows`:
actionlint, plus shellcheck over every `run:` block). Worth knowing why that
earns a place in the gate: most workflows here never execute on a PR — `apt.yml`
fires on release, `release.yml` on a tag — so a break in one is invisible until
the moment it matters, and the apt channel deploys to Pages, which has no
rollback. A dependabot bump of `upload-pages-artifact` passed a fully green
check set while silently dropping a file from the published archive (#444).
Invariants no linter can infer — like that action needing
`include-hidden-files: true` — are pinned as guard tests in `src/lib.rs`
instead.

**Supply chain lives in `.github/workflows/audit.yml`**, not the PR gate: weekly
plus `workflow_dispatch`, running the full `make deny` (advisories, bans,
licenses, sources) against a freshly fetched RUSTSEC DB. It was taken off the PR
path because cargo-deny's network fetch made every commit online-dependent and
has segfaulted mid-run on the runner, failing PRs that touched no dependencies.
The cost of that: a PR adding a banned or wrongly-licensed dependency isn't
caught until the next scheduled run — so **dispatch `audit.yml` after merging a
dependency change**, and before cutting a release.

**Never pipe the gate.** `make check 2>&1 | tail -20` reports the *pager's* exit status, so a failed gate reads as green — this shipped a broken `fmt-check` to CI once. A pipeline's status is its last command's in every shell, so no target can fix it: run `make check` unpiped, use `set -o pipefail`, or run **`make check-ci`**, which ends in `=== GATE: PASS ===` / `=== GATE: FAIL ===` so the verdict survives a tail even though the exit code does not.

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
  pane/           — pty subprocess, multi-tab, terminal rendering
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

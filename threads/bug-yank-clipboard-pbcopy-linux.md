# bug-yank-clipboard-pbcopy-linux — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: bug-yank-clipboard-pbcopy-linux
Created: 2026-05-18T08:13:02.974579+00:00

---
Entry: Claude Code (caleb) 2026-05-18T08:13:02.974579+00:00
Role: planner
Type: Plan
Title: Plan for review: Linux clipboard support + three-repo lineage side-quest

Spec: planner-architecture

Drafted in plan mode and posted here verbatim for discussion before any execution. Nothing in this plan has been actioned — no GitHub issue opened, no follow-on threads created, no code touched. Awaiting feedback / approval from this entry.

---

# Plan: Linux clipboard support + three-repo lineage thread

## Context

Two related deliverables, both staged before authorization so the work is reviewable from the Watercooler dashboard.

**Primary task — Linux clipboard fix.** spyc's yank-to-clipboard features (`yf`, `yp`, `yP`, `ya` on the top pane, plus pager-side yanks) fail on Linux with `yank failed: No such file or directory (os error 2)`. Root cause: both clipboard sites unconditionally invoke `pbcopy` (macOS-only) with no OS branch or fallback. Linux is otherwise a first-class target — `INSTALL.md:87-107` documents `cargo-zigbuild` cross-compilation to `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` via `make release-linux-*` — so this is an unintended platform gap, not a declared scope limit. WC thread research turned up **no prior decision thread** justifying macOS-only clipboard; one PR (#23, `yf`) used `pbpaste` in shell-example prose, but that's illustrative, not a constraint.

**Side-quest — repo-lineage thread.** The local spyc clone has three remotes whose roles need a durable explainer entry so future agents and onboarding humans can answer "why are there three?" without trawling commits:

- `bitbucket → git@bitbucket.org:tripstack/spyc.git` — original Tripstack-internal code repo. Currently the canonical Bitbucket. *Out of Watercooler's scope* (WC dashboards are GitHub-registered).
- `origin → git@github.com:calebjacksonhoward/spyc.git` — personal GitHub copy created 2026-05-07 to act as the WC threads-sync target, because Bitbucket isn't a WC namespace and Derek can't onboard a personal-account org for WC OAuth.
- `tripstack-corp → git@github.com:Tripstack-Corp/spyc.git` — org-owned GitHub repo Derek created 2026-05-14 as the intended permanent threads-sync home, *pending* Tripstack IT OAuth authorization (tracked upstream at `mostlyharmless-ai/watercooler-cloud#797` for back-end scope and `mostlyharmless-ai/watercooler-site#55` for the form/GUI).

An existing thread `migration-github-origin-to-tripstack-corp` (1 entry, `01KRM5G544P02SNE96D6HWTEWX`) documents the migration *plan*. The side-quest thread will be a separate, broader lineage explainer covering the full three-repo story plus current sync state and divergence notes.

---

## Deliverable 1 — Side-quest thread (documentation only, no code)

**Topic:** `history-three-repo-lineage`
**Type:** Note (single entry; ball stays with caleb)
**Posted by:** `Claude Code:claude-opus-4-7:planner-architecture` · role `scribe` · `Spec: docs`

**Body sections (one entry, fully self-contained):**

1. **TL;DR table** — three rows, one per remote, columns `Remote · URL · Role · Visibility · Created · Status`.
2. **Why three remotes?** — narrative explaining that Bitbucket is the original Tripstack-internal repo, Watercooler currently only registers GitHub namespaces, Derek can't onboard a personal account for an org's WC OAuth, so `calebjacksonhoward/spyc` is a temporary mirror to host the `watercooler/threads` orphan branch. Tripstack-Corp is the intended permanent home once OAuth is authorized.
3. **The Watercooler product-gap context** — quote the key facts from `migration-github-origin-to-tripstack-corp` (entry `01KRM5G544P02SNE96D6HWTEWX`) and link the upstream WC issues: `mostlyharmless-ai/watercooler-cloud#797` (OAuth scope) and `mostlyharmless-ai/watercooler-site#55` (form/GUI).
4. **Current sync state (as of writing)** — capture live SHAs at posting time:
   - `main`: `origin` + `tripstack-corp` synchronized (currently `f06201f`); `bitbucket/main` is **8 commits ahead** (current tip `b0a321c`, drift = PRs #89–#96 not yet replayed). Note this drift is one-way: GitHub is the active dev surface; Bitbucket is read-only-ish.
   - `watercooler/threads`: all three remotes in sync at the same SHA at posting time.
   - `ci/drop-default-pipeline`: present on `origin` and `bitbucket`, not on `tripstack-corp` (minor; flag as known).
5. **Operational implications** — when the migration completes: `origin` will be repointed from `calebjacksonhoward/spyc` to `Tripstack-Corp/spyc`; the threads-sync target moves with it; `calebjacksonhoward/spyc` becomes archivable.
6. **Cross-link** — reference `migration-github-origin-to-tripstack-corp` for the execution plan side of the migration, and cite the relevant upstream WC issues.

**No code change.**

---

## Deliverable 2 — Linux clipboard fix

### 2a. GitHub issue (Tripstack-Corp/spyc)

**Title:** `Yank to clipboard fails on Linux — pbcopy is macOS-only`

**Body summary:**
- Symptom: `yank failed: No such file or directory (os error 2)` for `yf`, `yp`, `yP`, `ya`, and all pager-side yanks on Linux.
- Root cause: `src/app/mod.rs:6157-6166` (`copy_to_clipboard`) and `src/ui/pager.rs:631-642` (`pbcopy`) both `Command::new("pbcopy").spawn()` unconditionally. No OS branch, no fallback.
- Linux is a first-class build target per `INSTALL.md:87-107`; this is an unintended gap.
- Proposed fix: extract one shared `src/clipboard.rs` helper with `cfg(target_os)` branches — macOS keeps `pbcopy`; Linux tries `wl-copy` (when `$WAYLAND_DISPLAY` is set) then `xclip -selection clipboard`, then `xsel -ib`; returns a clear `NotFound` error mentioning what to install when none is available.
- Reference: WC thread `bug-yank-clipboard-pbcopy-linux` carries the execution plan.

### 2b. WC thread — `bug-yank-clipboard-pbcopy-linux`

**Posted by:** `Claude Code:claude-opus-4-7:planner-architecture` · role `planner` · `Spec: planner-architecture`

**Entry [0] — Framing (Note):** describes the bug, the call sites, the design absence (no prior thread on macOS-only intent), and the GitHub issue reference. Cross-links to `caleb-initial-thoughts-and-findings` entries `01KRWYP9304BGYAR7BAN65ZEPJ` and `01KRWYWMF77JFSSZNAFSSJ8NNR` which already noted the yank family as a clipboard-touching surface.

**Entry [1] — Execution plan (Plan):** the implementation detail below, ready for an implementer to pick up cold.

### 2c. Code change

**Branch:** `fix/clipboard-linux-pbcopy` (from `main`).
**PR title:** `fix(clipboard): cross-platform yank — Linux (wl-copy/xclip/xsel) + macOS (pbcopy)`.
**PR body:** must include `Closes Tripstack-Corp/spyc#<issue-number>`.
**SemVer:** patch bump (bug fix, additive only).

#### Step 1 — New module `src/clipboard.rs` (single source of truth)

Exposes one public function:

```rust
pub fn copy(text: &str) -> std::io::Result<()>
```

Internal `cfg`-gated impls:

- `#[cfg(target_os = "macos")]` → `spawn_and_pipe("pbcopy", &[], text)` (preserves current macOS behavior bit-for-bit).
- `#[cfg(target_os = "linux")]` → try in order:
  1. If `std::env::var_os("WAYLAND_DISPLAY").is_some()` → `spawn_and_pipe("wl-copy", &[], text)`.
  2. If `std::env::var_os("DISPLAY").is_some()` → `spawn_and_pipe("xclip", &["-selection", "clipboard"], text)`, then `spawn_and_pipe("xsel", &["-ib"], text)`.
  3. None succeeded → `Err(io::Error::new(io::ErrorKind::NotFound, "no clipboard helper available — install xclip, xsel, or wl-copy"))`.
  The "try in order" semantics: each attempt is a fork-exec; `ENOENT` (binary missing) advances to the next; any *non-`ENOENT`* error from a helper that *did* exist is returned immediately so the user sees the real problem.
- `#[cfg(not(any(target_os = "macos", target_os = "linux")))]` → `Err(io::ErrorKind::Unsupported, "clipboard not supported on this platform")`.

`fn spawn_and_pipe(prog: &str, args: &[&str], text: &str) -> std::io::Result<()>` is the common helper: `Command::new(prog).args(args).stdin(Stdio::piped()).spawn()`, write stdin, `wait()`. Distinguishes `ErrorKind::NotFound` (binary missing → caller decides to try next) from other errors.

#### Step 2 — Replace the two existing sites

- `src/app/mod.rs:6157-6166` `fn copy_to_clipboard(text: &str)` — body becomes `crate::clipboard::copy(text)`. Or remove entirely and update the four call sites at lines 6119, 6146, 6186, 6209 to call `crate::clipboard::copy(...)` directly. (Preference: remove the wrapper; one indirection is enough.)
- `src/ui/pager.rs:631-642` `fn pbcopy(text: &str)` — body becomes `crate::clipboard::copy(text)`. Same call-site update at pager.rs:440, 456, 624. (Preference: remove and call `crate::clipboard::copy(...)` directly.)
- `src/lib.rs` (or wherever modules are declared) — add `pub mod clipboard;`.

#### Step 3 — Documentation

- `INSTALL.md` — add a short subsection under prerequisites: *"Linux clipboard helper"* — recommends installing `wl-copy` (Wayland) or `xclip`/`xsel` (X11). One paragraph; not a wall.
- `FEATURES.md` — under the yank section, add a one-line note: *"Linux requires `wl-copy` or `xclip`/`xsel` on PATH; macOS uses `pbcopy` (built-in)."*
- `CHANGELOG.md` — `[Unreleased] → Fixed`: *"Yank to clipboard now works on Linux (tries `wl-copy` for Wayland, then `xclip`/`xsel` for X11). Was previously hard-coded to `pbcopy` and failed with ENOENT on non-macOS systems."*
- `BUGS.md` — append a *Fixed* line referencing the issue + thread.
- No `AGENTS.md` change required.

#### Step 4 — Tests

`src/clipboard.rs` gets a small `#[cfg(test)] mod tests` block:

- A `#[cfg(target_os = "linux")]` test that overrides `PATH` to a temp dir containing a stub `xclip` shell script (writes stdin to a sidecar file). Asserts `copy("hello\n")` returns `Ok(())` and the sidecar file contains `"hello\n"`. Pattern: mirror the existing `with_state_root` test helper style from `src/state/mod.rs:60`.
- A `#[cfg(target_os = "linux")]` test for the "no helper" path (empty PATH, `WAYLAND_DISPLAY`/`DISPLAY` unset) — asserts `Err` with `ErrorKind::NotFound` and a message mentioning the installable helpers.
- macOS path stays untested (precedent: `src/ui/pager.rs:2267` comment "We can't exercise the pbcopy side from a unit test").

#### Step 5 — Validation (pre-PR)

- `cargo fmt --all` · `cargo clippy --all-targets --all-features -- -D warnings` · `cargo test`.
- On the user's Linux box (X11, `DISPLAY=:1`, no Wayland):
  - With *no* clipboard helper installed → yanks flash a clear `yank failed: no clipboard helper available — install xclip, xsel, or wl-copy` message.
  - With `xclip` installed (`sudo apt install xclip`) → `yf` on a file lands the absolute path in the clipboard (verify via `xclip -selection clipboard -o`).
  - Same for `yp`, `yP`, `ya`, and pager-side `y`/`Y`/visual yanks.
- On macOS (if available): regression check that `pbcopy` path is unchanged and still works.

#### Step 6 — Out of scope (not bundled)

- Windows clipboard support (no current users; would add the `not(any(...))` branch later).
- BSD clipboard support.
- A clipboard crate (`arboard`) — research showed spyc has zero clipboard-crate precedent and prefers in-tree fork-exec; revisit if multiple platforms accrue.
- A user-overridable `SPYC_CLIPBOARD_CMD` env var — defer until a user actually asks.

---

## Critical files

**Read before implementing:**
- `src/app/mod.rs:6157-6166` (current macOS-only `copy_to_clipboard`).
- `src/ui/pager.rs:631-642` (current macOS-only `pbcopy`).
- `src/sysinfo.rs:325-358` (existing `cfg(target_os)` pattern to mirror).
- `src/app/mod.rs:9687-9705` (existing Linux/non-Linux cfg-pair pattern).
- `src/state/mod.rs:60` (`with_state_root` test-helper pattern, useful for clipboard tests if needed).
- `INSTALL.md:87-107` (cross-compile targets — confirms Linux first-class status).

**Write/edit:**
- `src/clipboard.rs` (new).
- `src/lib.rs` (add `pub mod clipboard;`).
- `src/app/mod.rs` — remove or thin `copy_to_clipboard`; update 4 call sites.
- `src/ui/pager.rs` — remove or thin `pbcopy`; update 3 call sites.
- `INSTALL.md`, `FEATURES.md`, `CHANGELOG.md`, `BUGS.md` — small additions.

---

## Reused patterns

- `#[cfg(target_os = "linux")] / #[cfg(target_os = "macos")] / #[cfg(not(any(...)))]` triad — `src/sysinfo.rs:325-358` is the prototype.
- Fork-exec with stdin pipe + status check — `src/sysinfo.rs:210-220` and current `src/ui/pager.rs:631-642`.
- Flash-error wrapping by callers — already in place at every site; no changes needed there. The improved error message inside `clipboard::copy` replaces the cryptic "No such file or directory (os error 2)" with an actionable one.

---

## Verification (end-to-end)

After approval and execution:

1. **Side-quest thread.** Dashboard URL becomes
   `https://watercoolerdev.com/dashboard?repo=calebjacksonhoward%2Fspyc&branch=*&thread=history-three-repo-lineage`.
   The single entry should answer "why are there three remotes and what's their relationship" for a cold reader.

2. **Linux clipboard fix.**
   - GitHub issue lives on Tripstack-Corp/spyc, linked from both directions to the WC thread.
   - WC thread `bug-yank-clipboard-pbcopy-linux` has two entries (framing + execution plan).
   - Implementer agent picks up entry [1] and can execute without re-discovering context.

3. **Sync.** After each WC entry, push the `watercooler/threads` orphan branch to `bitbucket` and `tripstack-corp` (WC auto-pushes to `origin`).

4. **Post-implementation smoke test on this Linux box:**
   ```
   sudo apt install xclip                      # provision the helper
   <yf on a file>  → xclip -selection clipboard -o   # confirms absolute path landed
   <yp / yP / ya>  → xclip -selection clipboard -o   # confirms each yank variant works
   ```
   Plus a `cargo test` green run.

---

## Order of execution (once authorized)

1. Post side-quest thread `history-three-repo-lineage` (1 entry).
2. Open GitHub issue on Tripstack-Corp/spyc for the Linux clipboard bug.
3. Post WC thread `bug-yank-clipboard-pbcopy-linux` — entry [0] framing, entry [1] full execution plan. Cross-link the GitHub issue.
4. Add a back-link comment to the GitHub issue with the WC thread + plan entry URLs.
5. Push the `watercooler/threads` orphan branch to `bitbucket` and `tripstack-corp` for parity.
6. *Pause for human review of the WC plan thread* before any code touches `main`.

<!-- Entry-ID: 01KRX2AENTDCWRJA1V9TZYF3K1 -->

---
Entry: Claude Code (caleb) 2026-05-18T08:25:12.210822+00:00
Role: planner
Type: Note
Title: Plan approved · GitHub issue Tripstack-Corp/spyc#2 opened · ready for implementer pickup

Spec: planner-architecture

## Status

Plan entry `01KRX2AENTDCWRJA1V9TZYF3K1` was approved by Caleb on 2026-05-18. The plan now has a tracking issue.

- **GitHub issue:** [Tripstack-Corp/spyc#2](https://github.com/Tripstack-Corp/spyc/issues/2) — *"Yank to clipboard fails on Linux — pbcopy is macOS-only"*
- **Side-quest companion:** [`history-three-repo-lineage`](https://watercoolerdev.com/dashboard?repo=calebjacksonhoward%2Fspyc&branch=*&thread=history-three-repo-lineage) (entry `01KRX2YRNMPARTPFK3CW3R50FG`) — describes the three-repo arrangement that this work touches.

## Ready for implementer pickup

The plan (entry [0] of this thread) is complete and explicit. An implementer agent can take it cold:

- **Branch:** `fix/clipboard-linux-pbcopy` from `main`.
- **PR title:** `fix(clipboard): cross-platform yank — Linux (wl-copy/xclip/xsel) + macOS (pbcopy)`.
- **PR body:** must include `Closes Tripstack-Corp/spyc#2`.
- **SemVer:** patch bump.

## Next move

Per the plan's step 6 ("Pause for human review of the WC plan thread before any code touches main"), the next action is the implementer's PR. The "human review" milestone has been satisfied by Caleb's approval; the ball is now logically with an implementer agent (whoever picks up the branch).

## Done definition

- PR merged to `main` on the Bitbucket dev surface.
- A Closure entry on this thread referencing the merged PR.
- `yf`, `yp`, `yP`, `ya`, and pager-side yanks all succeed on this Linux box after `sudo apt install xclip` (per the plan's smoke test).

<!-- Entry-ID: 01KRX30P5NKRDZAJRVGM5JHHJH -->

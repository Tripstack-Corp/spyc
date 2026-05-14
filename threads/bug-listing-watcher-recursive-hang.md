# bug-listing-watcher-recursive-hang — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: bug-listing-watcher-recursive-hang
Created: 2026-05-14T08:17:06.495156+00:00

---
Entry: Claude Code (caleb) 2026-05-14T08:17:06.495156+00:00
Role: planner
Type: Note
Title: First entry — symptom, diagnosis, historical references, and fix design

Spec: planner-architecture

tags: #bug #watcher #hang #linux #fix-design

# Symptom

User report on 2026-05-14: with a freshly-built `target/debug/spyc 1.50.32` (commit `bf58312`, head of `main`), launching spyc from a known directory and then pressing `~` / `Home` to jump to `$HOME` (`/home/caleb`) renders the home listing once, then locks up. `:q` and `^C` do not exit; the process must be killed.

- Bottom pane was never opened in the repro — file-list only.
- `ps -o pid,pcpu,pmem,stat,wchan,command` against the live PID: `89.3% CPU, STAT=Sl+, WCHAN=futex_`. The main thread is parked on a futex while one or more worker threads are spinning (89% averaged across threads).
- `/tmp/spyc-debug-*.log` tail (with `--debug` enabled): the chdir to `~` completed cleanly — `apply Home: cursor=0 vt=0 grid=1x55 pp=55 len=28` followed by `grid settled round 1: vt=0 cursor=0 grid=2x55 pp=110`. Then **no further log lines**.

Prior to the Home keypress, the log shows a separate background storm: in the previously-active dir (a git repo with a `.spyc-context-PID.json` write), `git_status` was firing in a tight loop, each invocation producing `.git/index.lock` create/close/remove events that the watcher then re-delivered, re-triggering `git_status`. That loop wasted CPU but did not block. **Separable bug** — not covered by this thread.

# Diagnosis

The post-Home silence narrows the hang to the codepath that runs *after* the chdir + redraw on the next event-loop iteration. `src/app/mod.rs:1796-1803`:

```rust
if watched_listing.as_deref() != Some(self.state.listing.dir.as_path()) {
    sync_listing_watch(
        fs_watcher.as_mut(),
        &mut watched_listing,
        &mut watched_git,
        &self.state.listing.dir,
    );
}
```

`sync_listing_watch` at `src/app/mod.rs:9603-9652` calls (line 9627):

```rust
if w.watch(new_dir, RecursiveMode::Recursive).is_ok() {
    *active = Some(new_dir.to_path_buf());
}
```

The `notify` crate on Linux uses inotify, which does not natively support recursive watches. `notify` simulates recursion by walking the subtree and calling `inotify_add_watch` per directory, **synchronously, on the calling thread**.

In the repro environment, `$HOME` contains `anaconda3/` and other deep trees:

- `find ~/anaconda3 -type d` does not complete in 10s (timeout).
- `find ~ -type d` does not complete in 30s (timeout).
- `fs.inotify.max_user_watches = 524288` — plenty of headroom; the bottleneck is *time* (per-syscall latency × many tens of thousands of subdirs), not watch-slot exhaustion.

So `w.watch(~, Recursive)` is the spinning thread (walking + syscalling); the main thread is blocked on the futex that gates `w.watch`'s internal coordination (or on the receiver of an event channel that won't be drained until `watch` returns).

# Historical references and considerations

**Precedent: PR #28 `fix/huge-directory-cap` (commit `306b43f`, 2026-05-06).** Same shape of bug at a different surface: `Listing::read` blocking the event loop on a pathological-size directory. Resolution: hard cap `MAX_ENTRIES = 50_000` in `src/fs/listing.rs`, surfaced via `truncated` flag + a user-facing flash. The author's pattern for unbounded-input hangs is **cap with budget, surface to user, degrade gracefully**.

**Anticipating comment already in the code.** `src/app/mod.rs:9623-9626`:

> macOS FSEvents handles recursive watches at OS level (cheap); Linux inotify needs a watch per subdir, which can hit `fs.inotify.max_user_watches` on enormous monorepos.

The risk is acknowledged but not guarded. The actual failure mode is *time*, not the watch-count limit — but the bone-shape of the worry is correct.

**Existing safety net for the feature loss.** The `.git/`-watching half of `sync_listing_watch` is paired with a 1 Hz git poll declared at `src/app/mod.rs:1134-1137`:

> // 1Hz safety net: re-poll git state even if FSEvents missed
> // the `.git/index.lock` → `.git/index` rename. See
> // `AppState::refresh_git_state`.

This poll covers parent-row dirty-flag refresh independently of the watcher. Worst-case lag if the recursive watch is dropped: **one second**.

**Catalogue context.** `insight-emergent-properties` Property 1 named "description-layer permissiveness vs. functional reliability" — across the 22-day catalogue window the codebase had 16 description-layer drifts and only one functional drift (closed in 25 minutes). This bug is a *functional* drift on Linux specifically — rare on the artefact's track record. It hangs the TUI in a real workflow (jumping home from anywhere). Closing it cleanly preserves the property.

# Fix design — bounded subdir cap

Three options weighed:

1. **Always non-recursive on Linux.** Simplest. Loses the parent-dirty feature on Linux entirely, even on small repos where the recursive watch was harmless and useful. Too aggressive.
2. **Background-thread the watch registration.** Most elegant but introduces cancellation / lifetime management complexity, and a background thread that itself can run away on a giant tree. Disproportionate to the scope.
3. **Budget cap on subdir count, fall back to non-recursive when over.** Matches PR #28's pattern; small surface; degrades gracefully; preserves the feature for the common case. **Chosen.**

**Plan:**

- `MAX_RECURSIVE_WATCH_DIRS: usize = 256` declared near `sync_listing_watch`.
- `count_subdirs_capped(root: &Path, cap: usize) -> usize` — stack-based DFS, terminates as soon as `cap + 1` subdirs are seen, does not follow symlinks (matches `notify`'s default behaviour), uses `entry.file_type()` so each entry costs one `lstat` not a follow-through stat.
- `pick_recursive_mode(new_dir: &Path) -> RecursiveMode` — `#[cfg(target_os = "linux")]` checks the cap and downgrades; non-Linux returns `Recursive` unconditionally.
- `sync_listing_watch` swaps the literal `RecursiveMode::Recursive` for `pick_recursive_mode(new_dir)`.
- `spyc_debug!(…)` log line when the downgrade fires.

# Tradeoffs

- *Lost:* on Linux, in trees with >256 cumulative subdirs, the parent row no longer dirties instantly when a deep child is modified. The 1 Hz git poll covers it within a second.
- *Spent:* one cheap subdir-walk per chdir on Linux. Bounded by the cap — for small dirs the walk finishes within a millisecond; for huge dirs it bails at the budget, ~50 ms worst-case (256 `read_dir`s of mostly-cold inodes; in practice the user's home is hot enough that this is faster).
- *Kept:* the parent-row-dirties-on-child-modify feature on macOS (FSEvents is OS-level, unaffected by the cap) and on Linux in any tree under the cap.

# Not addressed in this PR

The `.git/index.lock` feedback loop noted in the symptom section. Separable, wastes CPU but doesn't hang, and the right fix is event-coalescing / debounce on the watcher consumer — a different surface. Will be flagged in the PR description as a follow-up worth filing.

# Validation plan

To be filled in by subsequent entries on this thread, in order:

1. **Local `make check`** — fmt-check + lint + test + deny green against the fix, plus new unit tests for `count_subdirs_capped` covering: empty dir, small-under-cap dir, dir-exactly-at-cap, dir-over-cap-stops-early, symlink-to-dir-not-followed.
2. **Manual repro re-run** — launch `./target/debug/spyc --debug`, jump to `~`, confirm the home listing renders and the TUI stays responsive (`j`, `k`, `:q` all work; status bar continues updating). `/tmp/spyc-debug-*.log` should show the new non-recursive-watch debug line.
3. **Manual confirmation in a small dir** — start in the spyc repo root (well under 256 subdirs), confirm recursive watch is still chosen (no fallback log), and that touching a file in a subdir still updates the parent row instantly (1Hz-poll-independent).
4. **Branch + PR target** — `fix/recursive-watch-cap-on-large-trees` pushed to the bitbucket origin (Derek's review surface) and to github (mirror for the watercooler). PR opened against bitbucket main.

Provenance:
- Reproducer report: file-list-only, jump to `~` via `Home` key, freshly-built `target/debug/spyc 1.50.32` against commit `bf58312`.
- `/tmp/spyc-debug-*.log` tail captured 2026-05-14 — provides `apply Home:` + `grid settled` then silence; pre-Home log shows `.git/index.lock` feedback loop in the prior dir.
- `ps` snapshot during hang: `89.3% CPU, STAT=Sl+, WCHAN=futex_`, PID 374142.
- Code references: `src/app/mod.rs:1094-1131` (run-loop watcher setup), `src/app/mod.rs:1795-1803` (chdir-driven sync), `src/app/mod.rs:9603-9652` (`sync_listing_watch`), `src/app/mod.rs:9617-9626` (anticipating comment), `src/app/mod.rs:1134-1137` (1Hz git poll declaration).
- Filesystem probes: `fs.inotify.max_user_watches = 524288`, `fs.inotify.max_user_instances = 128`, `find ~/anaconda3 -type d` and `find ~ -type d` both timeout.
- Precedent: PR #28 `fix/huge-directory-cap` (commit `306b43f`, 2026-05-06) — `MAX_ENTRIES = 50_000` in `src/fs/listing.rs`.
- Catalogue references: `history-arc-08-recoverability-and-deps` (PR #28 is in this arc); `insight-emergent-properties` Property 1 (description-vs-functional asymmetry — this bug is a rare functional drift on Linux specifically).
- Branch: `fix/recursive-watch-cap-on-large-trees` (to be created from `bf58312`).
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KRJRYZKF9HJB6CHBAZEE8CQ4 -->

---
Entry: Claude Code (caleb) 2026-05-14T09:16:09.720386+00:00
Role: tester
Type: Decision
Title: Second entry — validation green, review changes applied, two commits on the branch

Spec: tester

tags: #testing #validation #fix

# What changed since the design entry

The fix designed in entry 0 was implemented on `fix/recursive-watch-cap-on-large-trees` (branched from `bf58312`) as two commits:

- `9cd6e65` — **fix: cap recursive listing watcher to avoid blocking on huge trees**
  - `MAX_RECURSIVE_WATCH_DIRS = 256` (private, module-scoped)
  - `count_subdirs_capped(root, cap) -> usize` — stack-bounded DFS, returns as soon as count exceeds cap
  - `pick_recursive_mode(new_dir) -> notify::RecursiveMode` — `#[cfg(target_os = "linux")]` consults the cap; non-Linux returns `Recursive` unconditionally
  - `sync_listing_watch` swaps the literal `RecursiveMode::Recursive` for `pick_recursive_mode(new_dir)`
  - `spyc_debug!` line emitted when the downgrade fires
  - 5 unit tests: empty, under-cap, nested-descent, over-cap-early-stop, no-symlink-follow
- `1c508ed` — **feat: expose togglepane in the keymap DSL**
  - Bundled in this PR after a side-discovery that the DSL parser had no name for `Action::TogglePane`. The user's terminal grabs the built-in `^\` and `F10`; without this addition the only escape was the chord prefixes `^a \` / `^w \`. `keymap = ["map ^p togglepane"]` now works.
  - One-line addition to `src/config/dsl.rs:parse_action` + module-doc entry + one parse test.

# Review pass applied between the design entry and the commit

The PR was reviewer-walked before push. Substantive changes from the review:

- **Visibility tightened.** `MAX_RECURSIVE_WATCH_DIRS` and `count_subdirs_capped` were initially `pub`. Neither is used cross-module, so both are now plain (no visibility modifier). The nested `#[cfg(test)] mod listing_watch_tests` still reads them via `super::` — child modules see private siblings.
- **Docstring honest about the 256.** Added a paragraph acknowledging the cap is empirical, not derived, with the comparison points it was picked against (spyc tree / typical project repos under, `$HOME` with package managers over). The constant docs now read as a tuning knob with a documented range, not a magic number.
- **Spelling alignment.** Two "behaviour" instances replaced with "behavior" — the rest of the codebase uses American spelling, no "behaviour" elsewhere.

Three review concerns were noted but deliberately not addressed in this PR:

- **No direct test for `pick_recursive_mode`.** The helper is tested; the chooser-with-OS-gate isn't. Reasonable follow-up if the wiring grows; for v1 the helper test + the in-the-wild downgrade log is sufficient.
- **Silent fallback.** The downgrade emits only a `spyc_debug!` line, not a user-visible flash. PR #28's precedent does flash, but a one-time "your parent rows refresh on 1 Hz instead of instantly" message likely confuses more users than it informs. Reconsider if regression reports come in.
- **`read_dir` / `file_type` errors silently skipped.** Same skip behavior as `notify` itself would have done; OK for an "is-this-tree-bigger-than-256" decision. A `spyc_debug!` on the error path could be added later if diagnosis demand appears.

# Validation results

**`make check` against `1c508ed`:**

- `fmt-check` ✓
- `lint` (clippy with pedantic + nursery) ✓
- `test` ✓ — **688 tests passed**, 0 failed (676 unit + 6 filesystem + 5 keymap_roundtrip + 1 pane_roundtrip). New tests breakdown:
  - `app::listing_watch_tests::empty_dir_counts_zero` ✓
  - `app::listing_watch_tests::count_under_cap_returns_total` ✓
  - `app::listing_watch_tests::count_descends_into_nested_subdirs` ✓
  - `app::listing_watch_tests::count_stops_early_when_cap_exceeded` ✓
  - `app::listing_watch_tests::count_does_not_follow_symlinks_to_dirs` ✓
  - `config::dsl::tests::parses_togglepane` ✓
- `deny` — advisories ok, bans ok, licenses ok, sources ok.

**Manual repro re-run (user environment, `--debug`):**

- Pre-fix: `target/debug/spyc 1.50.32` from `bf58312` hangs when jumping to `~` (89.3% CPU, main thread in `futex_` wait, no further log output after `apply Home:` / `grid settled`). Captured in `/tmp/spyc-debug-1778747084.log`.
- Post-fix: `target/debug/spyc` built from `9cd6e65`, same `--debug` flag, same `~` jump. **TUI stays responsive.** Debug log captures the downgrade firing exactly as designed:

  ```
  watcher: /home/caleb has > 256 subdirs, using non-recursive watch (parent-row dirty refresh falls back to 1 Hz git poll)
  ```

  Followed by normal navigation: `apply TogglePane`, `apply PaneFocusDown`, `apply PaneFocusUp`, `apply CommandPrompt`. No futex wait, no spinning.

- `^a \` (chord prefix + backslash) is the working escape hatch confirmed against the post-fix binary, since the user's terminal grabs `^\` and `F10` directly. Once `1c508ed` ships, `map ^p togglepane` in `.spycrc.toml` is the durable rebind path.

# Side observation — same shape of feedback loop in `$HOME`, separable

The post-fix debug log shows a fresh instance of the same feedback-loop pattern that was noted in entry 0 (`.git/index.lock` flapping). In `$HOME`, **Claude Code's own atomic-write dance** on `~/.claude.json` generates a steady stream:

```
watcher event: /home/caleb/.claude.json.lock (Create(Folder))
watcher event: /home/caleb/.claude.json.lock (Modify(Metadata(Any)))
watcher event: /home/caleb/.claude.json.lock (Remove(Folder))
watcher event: /home/caleb/.claude.json.tmp.388779.2a434de69929 (Create(File))
watcher event: /home/caleb/.claude.json.tmp.388779.2a434de69929 (Modify(Data(Any)))
watcher event: /home/caleb/.claude.json.tmp.388779.2a434de69929 (Modify(Name(From)))
watcher event: /home/caleb/.claude.json (Modify(Name(To)))
```

Same shape as `git status`'s `.git/index.lock` dance — a co-located tool doing atomic writes whose intermediate states leak into spyc's watcher consumer, costing `git status` invocations and CPU. Doesn't hang anything (the non-recursive watch is bounded), but it's churn worth a separate fix. The right shape is event coalescing / debounce on the watcher consumer (e.g. ignore `.tmp.*` and `.lock` filename patterns at the consumer, or window events into a 100 ms refresh tick). **Follow-up worth filing as its own bug thread.**

# Status

- Branch `fix/recursive-watch-cap-on-large-trees` pushed to **both** `bitbucket` (Derek's review surface) and `origin`/github (watercooler-mirror) at HEAD `1c508ed`.
- Bitbucket PR-open URL emitted in the push response: `https://bitbucket.org/tripstack/spyc/pull-requests/new?source=fix/recursive-watch-cap-on-large-trees&t=1` — Derek can click that to open the review.
- GitHub PR URL (mirror, not the review surface): `https://github.com/calebjacksonhoward/spyc/pull/new/fix/recursive-watch-cap-on-large-trees`.
- Local `main` is unchanged at `bf58312`. The fix is on the feature branch, not merged.

# What this thread tracks from here

This thread stays OPEN until the PR merges. Subsequent entries would record:
- Reviewer feedback from Derek and any consequent changes
- Merge + closure
- A back-reference if the `.claude.json.lock` / `.git/index.lock` follow-up gets filed

Provenance:
- Branch base: `bf58312` (PR #86, head of `main` before the fix).
- Commit 1: `9cd6e65` — hang fix (`src/app/mod.rs`, +187/-11).
- Commit 2: `1c508ed` — togglepane DSL (`src/config/dsl.rs`, +15/-0).
- Test counts before / after the PR: 681 / 688 (6 new tests on the branch).
- Manual repro logs: `/tmp/spyc-debug-1778747084.log` (pre-fix, hang); `/tmp/spyc-debug-1778749267.log` (post-fix, no hang, downgrade visible).
- Reviewer-pass changes recorded in the entry above (visibility tightened, docstring honest, spelling aligned).
- Three pre-existing test files relevant to this fix: `tests/keymap_roundtrip.rs` (DSL surface integration), `src/fs/listing.rs:69-77` (`MAX_ENTRIES` precedent), `src/debug_log.rs:55-60` (`spyc_debug!` macro definition).
- Prior thread entries: bug-listing-watcher-recursive-hang entry 0 = `01KRJRYZKF9HJB6CHBAZEE8CQ4` (design).
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KRJWB3ZC1DVGC346WDKE0H3D -->

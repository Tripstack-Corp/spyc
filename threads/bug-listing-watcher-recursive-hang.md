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

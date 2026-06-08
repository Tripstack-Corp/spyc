# history-seg-performance — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-performance
Created: 2026-06-08T22:09:58.124347+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:09:58.124347+00:00
Role: scribe
Type: Note
Title: PR #99 — Tier-0 git-poll mtime cache (stat-before-spawn)

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: the 1 Hz git safety-poll short-circuits to a stat-only path when `.git/index` and `.git/HEAD` mtimes are unchanged, eliminating the per-second `git status` subprocess on quiet repos.   [kind: new-capability]
When: 2026-05-19 · PR #99 (perf/git-status-mtime-cache) · commit 68099128
Recorded rationale: "perf: skip 1Hz git poll subprocess when index/HEAD mtimes unchanged" — commit subject. CHANGELOG (added PR #99): "The 1 Hz git poll short-circuits when `.git/index` and `.git/HEAD` haven't moved... it was firing `git status --porcelain -unormal` every second regardless — cheap on a small tree, real CPU on a ~110k-file working tree... Cache the mtime pair... on the next tick stat both files and bail before any subprocess spawns if the mtimes match."
Inferred intent: opening move of the under-load campaign — make the idle polling cost proportional to actual repo change, not wall-clock. Evidence: diff is +85 lines in `src/app/state.rs`, +4 in `src/app/mod.rs`, no symbol removed (pure addition). confidence: high
Supersedes: (none) — the poll itself predates this slice; this caps its cost.

The poll is framed in the CHANGELOG as a deliberate safety net for "the rare case where FSEvents drops the `.git/index.lock` → `.git/index` atomic-rename notification (the macOS FSEvents inode-replacement edge case)." The fix narrows that net's cost rather than removing it: cache the (index mtime, HEAD mtime) pair from the last successful poll; next tick stats both and bails before any subprocess if they match. The CHANGELOG is explicit about scope — the cache is "scoped to the poll path only — the event-driven refresh (`refresh_listing` after fsevents debounce) still recomputes unconditionally because working-tree edits change file mtimes but NOT `.git/index`/`HEAD`, and a cache hit there would silently miss ` M filename` markers." Cache is reset on chdir.

This scoping caveat (poll-path cache must not leak into the event-driven refresh) becomes a recurring constraint across the campaign — see the throttle work in PR #137 and the starvation fix in PR #156, both of which re-touch the same `refresh_listing` invalidation boundary.

Provenance:
- 68099128 (PR #99 perf/git-status-mtime-cache, 2026-05-19) — `src/app/state.rs` +85 (mtime-pair cache + stat-before-spawn), `src/app/mod.rs` +4 (poll tick wiring), CHANGELOG +19.
- CHANGELOG.md (added PR #99) — quoted rationale above.

<!-- Entry-ID: 01KTMMHXYR7E0PYD7AZTHFAR8E -->

---
Entry: Claude Code (caleb) 2026-06-08T22:10:30.838835+00:00
Role: scribe
Type: Note
Title: PR #100 — huge-tree adaptive backoff + git-status off the UI thread + zero-subprocess cached chdir

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: a single PR lands three layered git-cost reductions for very large trees: (a) `git status` moves to a background worker so cache-miss chdir no longer blocks the UI, (b) huge-tree classification triggers adaptive backoff of poll/debounce/untracked-mode, (c) a cached-repo chdir does zero git subprocesses by reading `.git/HEAD` directly.   [kind: refactor]
When: 2026-05-19 · PR #100 (perf/huge-tree-adaptive-backoff) · commit db0ea614
Recorded rationale: "perf: huge-tree adaptive backoff (Tier 1)" — commit subject. CHANGELOG (added PR #100), worker move: "`git status` runs on a background worker thread... cache misses... still blocked the UI for the 200-500 ms `git status` index walk on a ~110k-file repo. Now the chdir returns immediately... The worker is a single long-running thread holding an `mpsc` request queue... A `git_generation` counter bumps on every cache-miss send. Results carry the generation they were spawned for; if the user has navigated past that point, the result is discarded silently (no thread cancellation needed)." Backoff: "When the count exceeds `HUGE_TREE_SUBDIR_THRESHOLD` (256)... `REFRESH_QUIET`: 500 ms → 3 s... `GIT_POLL_INTERVAL`: 1 s → 10 s... untracked mode: `-unormal` → `-uno`." Cached chdir: "every chdir on a *cached* repo... does the following instead: No subprocess at all."
Inferred intent: builds directly on PR #99's mtime cache ("Tier 1" / "Following on from the huge-tree adaptive backoff" / "Combined with the Tier 0 mtime cache, the idle poll is now stat-only *and* 10× less frequent"). Evidence: largest diff in the slice — `src/app/state.rs` +398/-... , `src/app/mod.rs` +212, `src/sysinfo.rs` reworked (+127/-... for `read_head_branch`/`resolve_gitdir`). confidence: high
Supersedes: PR #99's single-slot caches — the CHANGELOG notes the prior single-slot `huge_tree_anchor` and `git_status_raw_cache` "were wiped on every anchor change," replaced here by a multi-slot `huge_tree_decisions: HashMap<PathBuf, bool>` and a self-invalidating `(repo_root, mtimes, huge)`-keyed `git_status_raw_cache`.

The diff shape reads as several pre-squash commits folded into one PR: (1) async startup — "`App::new` no longer blocks on `git_status` / `git_file_statuses`"; (2) the long-running worker thread with `git_generation` discard-on-stale semantics; (3) the multi-slot huge-tree decision cache surviving leave-and-return; (4) the zero-subprocess cached chdir using new `sysinfo::read_head_branch` ("handles attached refs and detached HEADs") and `sysinfo::resolve_gitdir` ("handles `.git` dirs *and* worktree/submodule gitfiles"). All four were folded into this one moment; no separate decision per sub-commit.

Net effect quoted verbatim: "per-chdir cost on a cached repo drops from 'hundreds of ms, user-visible' to 'single-digit ms on the worst dir read; sub-ms for git work.'" The huge-tree path makes a visible trade — "untracked files no longer get a `?` marker on huge trees" — and flashes it: "large tree (256+ subdirs) — git poll throttled, untracked markers off." The worker-thread + generation-counter pattern introduced here is the same shape later reused for off-thread vt100 parsing (see PR #141) — this is the campaign's first move of blocking work onto a background thread with a stale-result discard counter. The MVU loop interactions around the worker request/result channels cross-reference history-seg-refactor-mvu; the git plumbing cross-references history-arc-04-git-integration.

Provenance:
- db0ea614 (PR #100 perf/huge-tree-adaptive-backoff, 2026-05-19) — `src/app/state.rs` (multi-slot decision cache, raw-status cache rework), `src/app/mod.rs` +212 (worker thread, mpsc queue, generation counter), `src/sysinfo.rs` (`read_head_branch`, `resolve_gitdir`, `count_subdirs_capped` made platform-agnostic), CHANGELOG +123.
- CHANGELOG.md (added PR #100) — quoted rationale above.
- 01KTMMHXYR7E0PYD7AZTHFAR8E (PR #99 entry, this thread) — the Tier-0 cache this builds on / supersedes the single-slot version of.

<!-- Entry-ID: 01KTMMK08N55944H24EY4RM81E -->

---
Entry: Claude Code (caleb) 2026-06-08T22:11:02.704258+00:00
Role: scribe
Type: Note
Title: PR #135 — typing-burst poll cadence (opens the 2026-05-26 typing-latency push)

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: a keypress now arms a 250 ms typing-burst window that tightens the event-loop poll cadence from the 100 ms idle interval to 16 ms while a pane is open, so the first echo after an idle gap lands in ~16 ms instead of ~100 ms.   [kind: new-capability]
When: 2026-05-26 · PR #135 (perf/input-typing-burst-poll) · commit e5ac0ea0 (pre-squash 1524b694)
Recorded rationale: "perf: typing-burst poll cadence for low first-echo latency" — commit subject. CHANGELOG (added PR #135): "Reported: 'I seem to be able to type faster than the input.' Cause: `crossterm::event::poll` doesn't wake on PTY output — only on host-terminal events — so after a keystroke is sent to the child, the event loop could sit at the 100 ms idle poll for up to one full window before draining the echo. Sustained typing was fine (`pane_had_output` already drives 16 ms), but the *first* character after an idle gap had a worst-case ~100 ms latency... any keypress arms a 250 ms typing-burst window that tightens the poll cadence to 16 ms when a pane is open."
Inferred intent: this opens the concentrated 2026-05-26 responsiveness push (#135–#144, eight PRs in one day). The throughline is stated in this very entry: the proper fix is acknowledged as out of scope — "Longer-term fix (let pane output wake the main loop directly, rather than timeout polling) is the proper solution but a larger refactor." Evidence: `src/app/mod.rs` +28/-5 only; the `typing_burst` symbol introduced here (pickaxe `git log -S typing_burst` → 1524b694, 2026-05-26 07:08) is read by every subsequent PR in the cluster. confidence: high
Supersedes: (none) — additive cadence layer over the existing `pane_had_output`-driven 16 ms path.

This is the cluster's framing moment: spyc's event loop is a timeout-poll loop (`crossterm::event::poll`), and PTY output does not wake it, so latency is bounded by the poll interval. The 250 ms typing-burst window is the cheap, bounded mitigation. The CHANGELOG explicitly names the structural fix it is deferring — direct pane-output wake — which is later realized in the MVU work (pickaxe shows `2026-05-31 refactor: MVU Phase 3b PR1 — pane output wakes the channel` and `Phase 3b PR2 — delete the pane poll floor`); cross-reference history-seg-refactor-mvu.

Provenance:
- e5ac0ea0 / 1524b694 (PR #135 perf/input-typing-burst-poll, 2026-05-26) — `src/app/mod.rs` +28/-5 (typing-burst window arming, cadence tightening), CHANGELOG +18.
- CHANGELOG.md (added PR #135) — quoted rationale above.

<!-- Entry-ID: 01KTMMKR90HM2V9VZYSDFS1WCZ -->

---
Entry: Claude Code (caleb) 2026-06-08T22:11:28.549065+00:00
Role: scribe
Type: Decision
Title: PR #138 — event-driven .spyc-context.json writes (kills the 1 Hz polling write)

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: the unconditional 1 Hz `write_context()` of `.spyc-context.json` is replaced by a `context_dirty` flag set by real context-bearing event sources; the file is written at end-of-iteration only if dirty AND ≥150 ms since the last write AND not inside the 300 ms typing-burst window.   [kind: supersession]
When: 2026-05-26 · PR #138 (perf/event-driven-context-write) · commit 9f99f171 (pre-squash 58990d2c)
Recorded rationale: "perf: event-driven .spyc-context.json writes (kill the 1Hz poll)" — commit subject. CHANGELOG (added PR #138): "Reported: input echo in the claude pane feels laggy under spyc but not standalone — confirmed via the `A` monitor that spyc itself was idle... so the culprit had to be something *outside* spyc... claude's HUD plugin watches `.spyc-context.json` and re-renders on every mtime change. The old 1 Hz polling write kept yanking claude's main loop ~once a second, even when state hadn't changed... Now the write is gated on a `context_dirty` flag set by event sources that can actually affect context (keypresses, MCP commands, fs-driven refresh_listing, git worker results)."
Inferred intent: the laggy-echo symptom here is downstream, not in spyc's loop — spyc's writes were perturbing claude's file-watcher. Evidence (pickaxe + diff): `git diff 9f99f171^ 9f99f171 -- src/app/mod.rs` shows four `self.write_context();` call sites removed and replaced with `self.context_dirty = true;`, plus a new `context_dirty: bool` field whose doc says "Replaces the old 1 Hz polling re-reads on every change, so unconditional 1 Hz writes were..." and the loop tail changes from `if last_context_write.elapsed() >= Duration::from_secs(1)` to an event-driven + 300 ms-burst-guard gate. confidence: high
Supersedes: the prior 1 Hz polling `write_context()` path (verified removed in the #138 diff). This is the cluster's clearest poll→event-driven supersession, paralleling the poll-cost reductions in PR #99/#100 but on the write side.

Notable as the moment where the `A` activity monitor is used as a diagnostic instrument: the report was triaged by reading the monitor (`mcp:0/s fs:0/s git:0/s p:0`), which ruled spyc's own loop out and pointed at an external watcher. That monitor is extended one PR later (#137) with a second internals line; this entry shows why the instrumentation mattered. The `context_dirty` symbol survives into the MVU refactor (pickaxe: `2026-06-02 MVU Phase 6 PR-C4+C5 — extract... context-write to loop_steps.rs`); cross-reference history-seg-refactor-mvu.

Provenance:
- 9f99f171 / 58990d2c (PR #138 perf/event-driven-context-write, 2026-05-26) — `src/app/mod.rs` +50/-9: removes 4 `write_context()` sites, adds `context_dirty` field + 150 ms/300 ms-burst gate, CHANGELOG +22.
- CHANGELOG.md (added PR #138) — quoted rationale above.

<!-- Entry-ID: 01KTMMMSG4DXNDQZ66KVXXBE93 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:12:27.520801+00:00
Role: scribe
Type: Note
Title: PR #137 — throttle git-worker re-spawns from refresh_listing + A-monitor internals line

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: `refresh_listing` no longer invalidates the raw git-status cache (and thus re-spawns `git status`) on every debounced fs event; spawns are throttled to at most once per 10 s on huge trees (1 s on small). A second teal internals line is added to the `A` activity monitor surfacing bg-task / git-worker / fs / mcp / listing / pager state with per-second rates.   [kind: refactor]
When: 2026-05-26 · PR #137 (perf/git-worker-throttle-and-extended-activity) · commit 236c4396
Recorded rationale: "perf+feat: throttle git-worker spawns; extend A monitor with internals" — commit subject. CHANGELOG (added PR #137): "On a huge tree (e.g. a 112K-file monorepo), running spyc next to a busy agent that writes files... drove sustained ~48% CPU. Root cause: `refresh_listing` unconditionally invalidated the raw git status cache, causing every debounced file-system event in the listing dir to re-spawn `git status --porcelain -uno` (200-500 ms each on huge trees). Now throttled — at most once per 10 s on huge trees (1 s on small)." Monitor: "`git last:Nms` is the roundtrip duration of the most recent worker request (would have surfaced the CPU bug above on first glance — high spawn rate jumps out)."
Inferred intent: this directly addresses the invalidation boundary that PR #99 deliberately left uncached — "the event-driven refresh... still recomputes unconditionally." PR #137 finds that the unconditional path is itself a cost source under continuous fs activity and throttles it (without caching it). Evidence: `src/app/mod.rs` +120, `src/app/state.rs` +50, `src/ui/help.rs` +5 (documents the new monitor line). confidence: high
Supersedes: tightens the PR #99 caveat — the event-driven `refresh_listing` recompute is no longer unconditional-per-event but throttled; the stated trade is "at-most-10 s lag in working-tree ` M` markers for edits within the throttle window." (This throttle is later made smarter by PR #156, which replaces the bare throttle with a max-defer predicate.)

Two distinct concerns in one PR, folded into this moment: (1) the perf throttle (own decision), (2) the `A` monitor second line (instrumentation feature). The monitor work pairs with PR #138's diagnosis story — the entry for #138 shows the monitor being used to rule spyc out; this PR is where its internals readout gains the git-worker spawn rate / roundtrip that "would have surfaced the CPU bug above on first glance." The throttle interacts with the same `refresh_listing → git_file_statuses_cached → raw porcelain` pipeline that PR #156 later hardens with a test; cross-reference history-arc-04-git-integration.

Provenance:
- 236c4396 (PR #137 perf/git-worker-throttle-and-extended-activity, 2026-05-26) — `src/app/mod.rs` +120 (throttle gate on cache-invalidation + monitor render), `src/app/state.rs` +50 (throttle bookkeeping), `src/ui/help.rs` +5, CHANGELOG +29.
- CHANGELOG.md (added PR #137) — quoted rationale above.
- 01KTMMHXYR7E0PYD7AZTHFAR8E (PR #99 entry, this thread) — the uncached event-driven refresh boundary this throttles.

<!-- Entry-ID: 01KTMMP87YEBGTXTHDPH0BJJY0 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:13:32.054136+00:00
Role: scribe
Type: Note
Title: PR #139 + #140 — typing must stay smooth: cap pane renders, then defer active-pane vt100 drain (interim mitigations)

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: two interim mitigations target the chatty-pane typing-latency symptom by keeping pane work off the keystroke path. #139 caps pane-driven renders to ~30 dps inside the typing-burst window (skip if previous render <33 ms ago); #140 additionally skips the active pane's reader-channel drain inside the burst if it drained within the last 100 ms, freezing the vt100 grid so the next ratatui diff emits empty.   [kind: refactor]
When: 2026-05-26 · PR #139 (perf/cap-pane-render-during-input) commit 0da9f9e6 (pre-squash 95f6f9e5) · PR #140 (perf/defer-active-pane-drain-during-typing) commit 28eed537 (pre-squash d2ee951f)
Recorded rationale: #139 subject "perf: cap pane-driven renders to ~30 dps during typing burst"; CHANGELOG: "holding `j` to scroll the file list spiked spyc CPU to 90%+... the per-iteration cost wasn't channel work, it was the vt100 parse + ratatui render of the chatty pane... we skip the pane-driven render if the previous render was <33 ms ago. Bytes are still drained from the PTY (no back-pressure to claude), and key-event renders escape the cap entirely." #140 subject "perf: defer active-pane vt100 parsing during typing burst"; CHANGELOG: "every *event*-driven render still included a fresh vt100 grid update for the pane... the loop iterated only ~8 times/sec while holding `j`... Now: in the 300 ms typing-burst window, the active pane's reader-thread channel drain is skipped if we drained within the last 100 ms... the vt100 grid stays frozen... Background tabs are always drained... Only the *active* pane is deferred."
Inferred intent: both are explicitly framed as incremental, each conceding the prior was insufficient — #140's CHANGELOG: "The v1.50.82 render cap helped pane-only renders, but every *event*-driven render still included a fresh vt100 grid update." They attack the symptom (loop iteration rate collapsing under a chatty pane) by avoiding work during typing, not by removing the synchronous parse. Evidence: each is a small `src/app/mod.rs`-only diff (#139 +34/-2, #140 +31). confidence: high
Supersedes: layered on PR #135's typing-burst window. Both are themselves superseded one PR later — PR #141 removes them as "vestigial under the worker-thread parser" (then re-keeps the cap as cheap belt-and-braces). Verified: pickaxe `git log -S typing_burst` → `1932aed8 fix: drop v1.50.82/83 throttles — vestigial under worker-thread parser` (2026-05-26).

Folded as one moment (the "typing must stay smooth" interim pair): both are mitigations of the same root cause — synchronous vt100 parse + render on the main loop, cost proportional to bytes the chatty pane emitted since the last iteration. They buy responsiveness by deferring pane work out of the keystroke path while preserving no back-pressure to claude (bytes still queue in the unbounded channel). They set up #141, which removes the root cause rather than deferring it.

Provenance:
- 0da9f9e6 / 95f6f9e5 (PR #139 perf/cap-pane-render-during-input, 2026-05-26) — `src/app/mod.rs` +34/-2, CHANGELOG +20.
- 28eed537 / d2ee951f (PR #140 perf/defer-active-pane-drain-during-typing, 2026-05-26) — `src/app/mod.rs` +31, CHANGELOG +23.
- 01KTMMKR90HM2V9VZYSDFS1WCZ (PR #135 entry, this thread) — the typing-burst window both layer on.

<!-- Entry-ID: 01KTMMQEW38T70F1GDME1FDJ8M -->

---
Entry: Claude Code (caleb) 2026-06-08T22:14:06.843904+00:00
Role: scribe
Type: Decision
Title: PR #141 — move pane vt100 parsing to a worker thread (the architectural fix; retires #139/#140)

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: each `Pane` gains its own parser worker thread that consumes PTY bytes and parses them into a `Arc<Mutex<vt100::Parser>>` grid concurrently with the main loop. The main thread reads grid state via brief-locking `with_screen` / `with_screen_mut` closures, and `Pane::drain_output()` becomes a non-locking generation-counter read. Per-iteration main-thread cost is now bounded by render + input dispatch only, regardless of how many bytes the pane emits.   [kind: refactor]
When: 2026-05-26 · PR #141 (perf/vt100-parsing-on-worker-thread) · commit 4be0d38b (pre-squash incl. 5c94bbf7, 1932aed8)
Recorded rationale: "perf: move pane vt100 parsing to a worker thread" — commit subject. CHANGELOG (added PR #141): "v1.50.82/83's defer + cap mitigations helped but didn't eliminate the long-running-claude input lag. The structural problem: parsing pane bytes was synchronous on the main thread, so a chatty pane stretched every iteration body proportional to how many bytes claude had emitted since the previous iteration. Now each `Pane` owns a parser worker thread... Main thread reads grid state via `with_screen` / `with_screen_mut` closures... `Pane::drain_output()` becomes a non-locking generation-counter read. Effect: per-iteration cost on the main thread is now bounded by render + input dispatch only, regardless of how many bytes claude emits."
Inferred intent: this is the structural fix the cluster had been deferring since PR #135 ("let pane output wake the main loop directly... is the proper solution but a larger refactor"). Verified off-thread move via pickaxe + diff: `git diff 4be0d38b^ 4be0d38b -- src/pane/mod.rs` adds `use std::sync::{Arc, Mutex}`, `parser: Arc<Mutex<vt100::Parser>>`, `parser_gen: Arc<AtomicU64>`, `stop_parser: Arc<AtomicBool>`, `parser_thread: Option<thread::JoinHandle<...>>`, a `thread::spawn(... parser_worker(...))` inside `pub fn adopt(host, parser)`, and `pub fn take_host()` that "stops the parser worker and restores the byte receiver to the host." Largest cluster diff: `src/app/mod.rs` 246 changed lines, `src/pane/mod.rs` +285/-..., `src/pane/pty_host.rs` +61. confidence: high
Supersedes: (1) the inline/synchronous main-thread vt100 parse — the root cause #139/#140 only mitigated; (2) the #139/#140 throttles themselves — CHANGELOG: "Remove v1.50.82/83 throttles — they were vestigial under the worker-thread parser... they were just *delaying* the moment the main thread checked the worker's generation counter. That delay manifested as an off-by-one between keystroke and visible echo." Verified: pickaxe `1932aed8 fix: drop v1.50.82/83 throttles` is part of this PR. (The render cap is then re-added as "cheap belt-and-braces.")

This is the campaign's notable architectural move: it reuses the worker-thread + generation-counter discard pattern first introduced for git-status in PR #100, now applied to pane parsing. A folded companion commit (`5c94bbf7 fix: pane render + cursor share one parser lock`) closes a race the new concurrency opened: "the pane content was drawn under one `with_screen` lock, then `place_pty_cursor` re-acquired the lock... Between the two, the worker thread could parse a new chunk — so the cursor reflected a *newer* grid state." Cursor placement is folded into the same `with_screen` closure. The `with_screen` / `drain_output` / parser-worker surface introduced here is later reshaped by the MVU work — pickaxe shows `with_screen` and `drain_output` touched by `2026-05-31 MVU Phase 3b PR1 — pane output wakes the channel`, `2026-06-02 MVU Phase 6 PR-C3 — extract pane-output drain to streaming.rs`, and the pane-parser-lifecycle fix PR #150; cross-reference history-seg-refactor-mvu.

Provenance:
- 4be0d38b (PR #141 perf/vt100-parsing-on-worker-thread, 2026-05-26) — `src/pane/mod.rs` +285/- (parser worker, `Arc<Mutex<vt100::Parser>>`, `parser_gen`, `with_screen`/`with_screen_mut`, `adopt`, `take_host`), `src/app/mod.rs` 246 lines (drain becomes gen-counter read; throttles removed), `src/pane/pty_host.rs` +61, CHANGELOG +56.
- pickaxe `git log -S with_screen` / `-S typing_burst` — confirms 5c94bbf7 (lock unification) and 1932aed8 (throttle removal) belong to this PR.
- 01KTMMQEW38T70F1GDME1FDJ8M (PR #139+#140 entry, this thread) — the mitigations this supersedes.

<!-- Entry-ID: 01KTMMSKB3JSCEJJHN4DB5K4H2 -->

---
Entry: Claude Code (caleb) 2026-06-08T22:14:57.651023+00:00
Role: scribe
Type: Note
Title: PR #144 — cache the status-line agent short-id (was ~65% of main-thread CPU)

Spec: scribe

tags: #history #performance

Moment: performance — Reconstructed: `App::render` no longer calls `active_agent_status` every frame (which walked every `~/.claude/sessions/*.json` with a full serde_json parse and could scan a session's `*.jsonl` for `custom-title`). The resolved status string is cached with a 30 s TTL keyed on the active pane's `(kind, cwd, spawn_epoch_secs)`.   [kind: refactor]
When: 2026-05-26 · PR #144 (perf/cache-agent-status) · commit dbab8b3b
Recorded rationale: "perf: cache the status-line agent short-id (was 65% main-thread CPU)" — commit subject. CHANGELOG (added PR #144): "`App::render` called `active_agent_status` every frame, which walked every `~/.claude/sessions/*.json` with a full `serde_json::Value` parse and could then scan a session's `*.jsonl` looking for `custom-title`. For a long-running user with hundreds of accumulated session files, a symbolicated `sample` showed `serde_json::value::de::deserialize` consumed about two-thirds of the main thread. Cache the resolved status string with a 30 s TTL, keyed on the active pane's `(kind, cwd, spawn_epoch_secs)`."
Inferred intent: closes the 2026-05-26 push with the last per-frame hotspot the burst surfaced — a render-path cost unrelated to pane parsing (the prior PRs' target), this time filesystem+JSON work in the status line. Evidence: `src/app/mod.rs` +59/-5 only; TTL cache wrapper around an existing resolver. The CHANGELOG notes the resolver's own doc comment had pre-flagged it: "caller should add a per-pane TTL cache if it shows up as a hotspot — now it has." confidence: high
Supersedes: the per-frame `active_agent_status` call in `App::render` (verified by the diff replacing the direct call with a TTL-gated cache read). No prior thread moment touched this path.

This is a standalone hotspot fix, not part of the pane-parsing chain — it shares the cluster's method (profile under a long-running session, find the per-frame cost, cache behind a coarse TTL because "the status string itself changes ~never within a session") rather than its subject. With PR #141 having bounded the per-iteration parse cost, the agent-status walk became the next visible fraction of main-thread CPU; the `sample`-driven diagnosis mirrors the `A`-monitor-driven diagnosis used earlier in the cluster (see PR #138).

Provenance:
- dbab8b3b (PR #144 perf/cache-agent-status, 2026-05-26) — `src/app/mod.rs` +59/-5 (30 s TTL cache keyed on `(kind, cwd, spawn_epoch_secs)`, invalidated on tab switch / in-pane chdir), CHANGELOG +19.
- CHANGELOG.md (added PR #144) — quoted rationale above.

<!-- Entry-ID: 01KTMMTSMP1F44C1ZPY2PP8J7M -->

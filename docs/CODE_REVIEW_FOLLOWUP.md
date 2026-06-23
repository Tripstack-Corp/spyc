# Code review follow-up — finalizing the June 2026 review

**Status:** ACTIVE tracker (opened 2026-06-21). The June 2026 deep review ([`docs/archive/CODE_REVIEW_2026-06.md`](archive/CODE_REVIEW_2026-06.md)) drove a large remediation campaign (PRs #329–#424). This doc tracks what it left: the High/Medium findings deferred or never verified.

**Goal:** drive every remaining finding to a clean terminal state — **fixed** (own cluster PR) or **closed** (refuted / by-design / accepted), keeping an honest ledger.

## Phase 1 verification sweep — 2026-06-23

All 70 still-open findings were re-checked against HEAD (post-#514) by a parallel agent fleet — one verifier per finding plus an opposite-direction adversarial challenger on every consequential verdict (140 agents).

| Verdict | Count |
|---|---|
| REAL (still reproduces) | 62 |
| PARTIAL (residual after earlier work) | 5 |
| ALREADY_FIXED (closed below) | 3 |

- The challenge pass flipped **2** verdicts, both *upward* (nothing was refuted down): `git/blame.rs:49` PARTIAL→REAL (per-line String clones + no size cap still apply; only re-decode was cached), and `git/status.rs:314` ALREADY_FIXED→REAL (the verifiers disagree on whether rewrite-detection reaches porcelain parity on unstaged renames — **re-verify by hand before fixing**). Both flagged ⚠︎ below.

- `pager_handler/motion.rs:311` is confirmed **REAL** (high): `v` from a scrollback pager orphans `view.scroll_pager` and restores into `view.pager` — supersedes the earlier "partially addressed" note.

## Phase 2 — planned PR sequence

One `fix:`/`refactor:` PR per cluster (batched where small), gate-green, each `make release`-tested before merge. ✅ #514 already landed cluster 1 (pure-Model IO).

- **PR2** — Pager slot routing — orphaned / lost-scrollback pagers (incl. 2 HIGH) _(5 findings; clusters: pager-slot)_
- **PR3** — git status / diff porcelain parity + robustness _(7 findings; clusters: git-status-parity, worktree-add-rollback, perf-alloc)_
- **PR4** — Pager scroll math — u16 saturation + wrapped-row reachability _(8 findings; clusters: pager-u16-scroll, pager-wrap-math)_
- **PR5–8** — Blocking-IO off-thread sub-campaign (split by subsystem, #349 template) _(17 findings; clusters: blocking-io, effect-bypass-capture-pty)_
- **PR9** — Dedup / shared-helper cleanups _(10 findings; clusters: dedup)_
- **PR10** — Dead-code & stale-shim removal _(6 findings; clusters: dead-code)_
- **PR11** — MCP scope/robustness + path/env overlay _(3 findings; clusters: mcp)_
- **PR12** — Misc correctness batch _(11 findings; clusters: resume, other, fs-watch-topology, prompt-allowlist-drift, perf-linear-scan, perf-sort-alloc, pager-truncation-bytes, pane-vt100-recovery-size)_

**To fix: 67** (62 REAL + 5 PARTIAL).

## To fix — by cluster

### PR2 · Pager slot routing — orphaned / lost-scrollback pagers (incl. 2 HIGH)

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/pane_scroll.rs:304` | `r` reload branch in handle_pane_scroll_key checks the wrong pager slot and is dead for its stated purpose | medium | S | ✅ PR #523 |
| `src/app/pager_handler/motion.rs:311` | `v` (edit in $EDITOR) from a bottom scrollback pager closes/restores the wrong pager slot, leaving an orphaned pager that Esc/q cannot close | high | M | ✅ #520 (tested) |
| `src/app/pager_stream.rs:228` | Opening pager help (`?`) or navigating buffer history (`[b`/`]b`) while a stream is mid-flight kills the worker and permanently freezes the pager at "scanning…"/"computing…" | medium | M | REAL |
| `src/app/pane_scroll.rs:179` | Single `runtime.pager_stream` slot: starting a grep/git-view while a transcript is loading silently kills the transcript stream, leaving the scroll pager empty forever | medium | M | REAL |
| `src/pane/mod.rs:430` | recent_lines/save_to_file read one stale viewport, not the recent tail — vt100 contents() is viewport-only | high | M | REAL |

### PR3 · git status / diff porcelain parity + robustness

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/state/listing.rs:111` | Git status lookups keyed by display name never match executable files | medium | S | REAL |
| `src/git/status.rs:246` | One failed status item silently blanks git status for the whole repo | medium | S | REAL |
| `src/git/blame.rs:49` | BlameModel has no size cap and clones 3 metadata Strings per line | medium | M | REAL ⚠︎ |
| `src/git/diff_model/build.rs:89` | gd diff silently drops the deletion side of an unstaged rename | medium | M | REAL |
| `src/git/status.rs:217` | gix status-walk setup and item-decode skeleton duplicated between repo_status and collect_worktree_plan | medium | M | REAL |
| `src/git/worktree.rs:170` | worktree::add leaves partial on-disk state on failure, and the leftover state blocks retry | medium | M | REAL |
| `src/git/status.rs:314` | repo_status diverges from porcelain on unstaged renames (parity contract violation) | medium | none | REAL ⚠︎ |

### PR4 · Pager scroll math — u16 saturation + wrapped-row reachability

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/pane/mod.rs:472` | scroll_offset desyncs from vt100's clamped scrollback, making scroll-down appear dead after 'g' or over-scrolling up | medium | S | REAL |
| `src/pane/mod.rs:518` | max_scrollback() hardcodes 10_000; scroll_offset is never synced to vt100's clamp, creating a dead zone after scroll_to_top | medium | S | REAL |
| `src/ui/pager/layout.rs:131` | visual_rows underestimates actual wrapped rows (wide-char greedy waste + whitespace markers), so scroll_max clamps before the real bottom | medium | S | PARTIAL |
| `src/ui/pager/render.rs:180` | wrap_line materializes the full wrapped expansion of each visible logical line every frame — allocation churn proportional to longest line, not viewport | medium | S | REAL |
| `src/app/pager_handler/mod.rs:311` | Files under 5 MB but over 65,535 lines load fully yet are unscrollable past line 65,536 (u16 scroll saturation) | medium | M | REAL |
| `src/ui/pager/mod.rs:141` | scroll: u16 silently caps every pager at 65,535 lines — reachable content beyond that is unviewable | medium | M | REAL |
| `src/ui/pager/render.rs:125` | With wrap on, scroll is logical-line granular — visual rows of a long line beyond the first viewport_h are unreachable | medium | M | PARTIAL |
| `src/ui/pager/selection.rs:281` | Visual/placement auto-scroll assumes 1 logical line = 1 screen row; with wrap on the cursor moves off-screen without scrolling | medium | M | REAL |

### PR5–8 · Blocking-IO off-thread sub-campaign (split by subsystem, #349 template)

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/navigate.rs:160` | gF reads an attacker-controlled path fully into memory synchronously on the input thread (hang/OOM via hostile pane content) | medium | S | REAL |
| `src/clipboard.rs:111` | Clipboard helper inherits stdout/stderr (garbles the raw-mode TUI) and leaks a zombie when stdin write fails | medium | S | REAL |
| `src/fs/finder.rs:136` | find_nested_git_repos re-walks the entire subtree raw (no gitignore, no cap, no cancellation) on every F open / :grep in a git root | medium | S | PARTIAL |
| `src/fs/grep.rs:353` | search_to_vec blocks on the full repo walk even after the result limit is reached | medium | S | REAL |
| `src/pane/pty_host.rs:208` | Unbounded reader->parser channel with a reader that never stops reading: no backpressure, unbounded memory under a firehose child | medium | S | REAL |
| `src/state/sessions/mod.rs:579` | find_claude_session_name reads the entire conversation JSONL (100+ MB) into memory | medium | S | REAL |
| `src/agent/resume.rs:298` | gemini_resume_index_for runs `gemini --list-sessions` synchronously with no timeout on the session-restore path | medium | M | REAL |
| `src/app/key_dispatch/mod.rs:315` | Capture-pty writes bypass the Effect executor while sibling sinks in the same match use Effect::SendToPane | medium | M | REAL |
| `src/app/mod.rs:793` | crossterm::terminal::size() called inside key/action handlers (8 sites), against the effects-as-data contract | medium | M | REAL |
| `src/app/state/apply.rs:320` | format_long_listing and file_type_label do per-file IO inside the pure apply dispatcher | medium | M | REAL |
| `src/fs/long_listing.rs:155` | format_long_listing does an unmemoized getpwuid/getgrgid NSS lookup per row — L on a large listing can stall seconds-to-minutes on LDAP-backed machines | medium | M | REAL |
| `src/git/worktree.rs:188` | worktree::add performs a full-tree checkout synchronously on the main input thread | high | M | PARTIAL |
| `src/pane/widget.rs:37` | Parser mutex held across the whole pane draw — per-frame O(cells) set_string under the lock contends with the parser worker | medium | M | REAL |
| `src/ui/blame_render.rs:44` | render_blame joins and syntect-highlights the whole file on the main thread with no size cap | medium | M | REAL |
| `src/ui/diff_render/mod.rs:149` | Diff render syntect-highlights both full sides on the main thread, and re-highlights from scratch on every layout toggle | medium | M | REAL |
| `src/ui/pager/construct.rs:182` | Pager yank/save methods do inline OS side effects, bypassing the existing Effect::CopyToClipboard path | medium | M | REAL |
| `src/app/sources.rs:293` | Watcher-driven `refresh_listing` does a synchronous 50k-entry disk walk + allocation-heavy sort on the event-loop thread | medium | L | PARTIAL |

### PR9 · Dedup / shared-helper cleanups — ✅ done (PRs #517, #518; see closed log)

7 code-dups deduped across #517 (4) and PR #518 (3: `commands.rs:83`, `pane_scroll.rs:239`). The 2 dedup-*behavior* items (`inventory.rs:85`, `sessions/mod.rs:127`) are correctness bugs → **PR12**. `state/dispatch.rs:45` (`:limit`) → **PR12** (unifying the command + prompt paths *changes* `:limit git`/`:limit h` behavior — a fix, not a refactor). `pager_handler/pickers.rs:198` **closed**: the cursor-sync is already factored through the `sync_editor!` macro at every site; the residual `n`/`N` micro-dup is marginal and sits in a delicate interactive path — not worth a fragile extraction.

### PR10 · Dead-code & stale-shim removal — ✅ done (PR #516; see closed log)

5 of 6 done this PR. `src/mcp/protocol.rs:62` (9 doc comments detached by the verbatim mcp.rs split — needs cross-file archaeology to find where each function moved) is deferred into **PR11**, which already touches that file.

### PR11 · MCP scope/robustness + path/env overlay

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
2 done (PR #519 — see closed log: `protocol.rs:324` get_file_content scope, `paths.rs:17` envset HOME). 2 remaining:

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/mcp/server.rs:259` | One slow tool call (>20s) kills the entire MCP connection, and the server's searches are unbounded | medium | M | REAL |
| `src/mcp/protocol.rs:62` | Nine doc comments detached from their functions by the verbatim mcp.rs split (deferred here from PR10) | medium | S | REAL |

### PR12 · Misc correctness batch

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/agent/resume.rs:24` | Claude resume stripper misses -r/--continue/-c and eats the flag following a bare --resume | medium | S | REAL |
| `src/app/key_dispatch/prompts.rs:200` | Tab-completion PromptKind allowlists are hand-synced in three-plus places — the drift pattern the command table was built to kill | medium | S | REAL |
| `src/app/prompt.rs:590` | J jump prompt silently swallows errors — typo'd path gives zero feedback | medium | S | ✅ PR #522 |
| `src/app/render/chrome.rs:320` | build_rows is O(rows × delete-preview paths) — quadratic on 'delete picks' in a big directory | medium | S | REAL |
| `src/fs/listing.rs:137` | Listing::sort comparator allocates 2-4 Strings per comparison — ~1.5M+ allocations per sort of a 50k-entry directory, on the event loop | medium | S | REAL |
| `src/fs/ops.rs:52` | read_truncated caps lines but not bytes — a huge single-line file is loaded entirely into RAM on the UI thread | medium | S | ✅ PR #521 |
| `src/git/diff_model/build.rs:559` | Rename similarity recomputed with a second full-blob diff that gix already performed | medium | S | REAL |
| `src/main.rs:87` | Panic hook doesn't pop kitty keyboard-enhancement flags or alternate-scroll mode — terminal left misbehaving after a panic | medium | S | REAL |
| `src/pane/mod.rs:607` | vt100 panic recovery rebuilds the parser at the adopt-time size, and the resize coalescer guarantees it never gets corrected | medium | S | REAL |
| `src/app/run.rs:54` | Config-file watch on $HOME is permanently destroyed when the listing watch passes through the same directory | medium | M | REAL |
| `src/pane/tabs.rs:20` | Claude-specific session-restore state machine (PendingResumeSend) lives in the generic pane layer | medium | M | REAL |
| `src/state/inventory.rs:85` | Re-yanking a modified file silently keeps the stale cached content (moved from PR9 — dedup *behavior*, not code dup) | medium | S | REAL |
| `src/state/sessions/mod.rs:127` | load_sessions dedup collapses distinct resumable sessions that share cwd + commands (moved from PR9) | medium | S | REAL |
| `src/app/state/dispatch.rs:45` | :limit command and limit-prompt are drifted near-duplicates — unifying them changes `:limit git`/`:limit h` (fix, moved from PR9) | medium | S | REAL |

## Closed / resolved (running log)

**✅ #514 — cluster 1, pure-Model IO moved off-thread (2026-06-23):** `state/apply.rs:113`, `state/selection.rs:77`, `clipboard.rs:189`, `clipboard.rs:250` — inline file copies / tar+zstd archiving / blocking pipe reads replaced by `Effect::FileOp` + `Effect::Inventory` off-thread workers.

**✅ PR #516 — dead-code & stale-shim removal (2026-06-23):**
- `effect.rs:274` — removed dead `PostAction::None` + the `Default` derive + the stale comment that cited a no-longer-typechecking `ApplyResult::Post(PostAction::None)` site.
- `sysinfo.rs:42` — macOS `rss_kb()` reads RSS via the `sysinfo` crate (already used in-file), no more `ps` shell-out.
- `ui/markdown/mod.rs:136` — the keyed-line-break regex is now a function-local `LazyLock`, compiled once instead of per render.
- `ui/mod.rs:13` — dropped the blanket `#[allow(dead_code, …)]` on `line_edit` (removed the dead `is_empty`, made `new` a `const fn`, collapsed two or-patterns + a duplicate match arm).
- `state/mod.rs:84` — rewrote the stale `Update` doc comment (the "Stage 3D" migration was abandoned; the per-producer enums + `From` bridges are the settled shape).

**✅ PR #517 — dedup / shared-helper cleanups, part 1 (2026-06-23):**
- `fs/grep.rs:288` — deleted the verbatim `find_nested_git_repos` copy; `finder::find_nested_git_repos` is now the single shared definition.
- `pane/pathref.rs:130` — `strip_ansi` delegates to the `strip-ansi-escapes` crate (already a dep) instead of the handrolled CSI/OSC scanner.
- `ui/pager/construct.rs:58` — `new_plain` delegates to `new_styled` (was a verbatim 35-field copy).
- `pager_handler/mod.rs:218` — the 3 copy-pasted top-overlay pty-spawn blocks now share `spawn_top_overlay`.
(Remaining 4 dedups + 2 reclassified correctness items tracked under PR9/PR12.)

**✅ PR #518 — dedup / shared-helper cleanups, part 2 (2026-06-23):**
- `pane_scroll.rs:239` — `mount_scroll_pager` and the `mount_stream_pager` LowerPane arm now share `install_lower_pane_scroll_view` (the LowerPane flag block + scroll-mode entry; `Some(id)` marks a live stream).
- `commands.rs:83` — the `:;cmd`/`:!cmd` arms and the `;`/`!` prompt arms (`ShellCmd`/`ShellCmdCaptured`) now share `run_foreground_shell_overlay` + `run_captured_shell`; the `!`-repeat-last and per-arm display string stay in the prompt arm.
- `pager_handler/pickers.rs:198` **closed** (no change): cursor-sync already factored via the `sync_editor!` macro; residual is marginal/delicate.

**✅ PR #519 — MCP scope + path/env overlay (2026-06-23):**
- `mcp/protocol.rs:324` — `get_file_content` resolves relative paths against the **search root** (worktree root / project_home / cwd) and guards against it, matching `search_paths`/`search_content` so their repo-relative results read back. New test `get_file_content_resolves_relative_against_search_root`; the traversal guard still holds (message reworded to "outside the project root").
- `paths.rs:17` — `expand()` reads HOME from the `envset` overlay (then falls back to the process env), so an overridden HOME no longer slips past tilde expansion.
(`mcp/server.rs:259` slow-call timeout + `protocol.rs:62` doc reattachment remain in PR11.)

**✅ PR #521 — read_truncated byte ceiling (2026-06-23):**
- `fs/ops.rs:52` — the >`MAX_PAGER_BYTES` huge-file pager fallback capped lines but not bytes, so a newline-less giant file was slurped whole into RAM by `read_line`. Now bounds each read by the remaining byte budget (per-line `Take`) and reports truncated when the ceiling is hit. New test with a 5MB+ newline-less file. (Gate-verified; no live test needed.)

**✅ PR #522 — J jump error feedback (2026-06-23):**
- `prompt.rs:590` — a `J` jump to a typo'd / nonexistent path was `let _ = jump_to(..)` (silent no-op). Now flashes `jump: {e}` on the resolve error (chdir failures already flash inside `jump_to`). New harness test `jump_prompt_flashes_on_bad_path`. (Gate-verified.)

**✅ PR #523 — remove dead `r`-reload branch in handle_pane_scroll_key (2026-06-23):**
- `pane_scroll.rs:304` — the `r` branch checked `view.pager` (top slot) for a `stream_id`, but this handler only runs for raw vt100 scroll mode (`InputSink::PaneScroll`), where a stream pager is always Modal/Scrollback (→ PagerKey/`handle_pager_motion`, which owns the live `r` reload at motion.rs:354). The condition was always false → dead. Removed with a breadcrumb comment. Behavior-neutral (the body never ran).

**✅ ALREADY-FIXED — confirmed by the 2026-06-23 sweep (no action needed):**
- `src/app/mcp.rs:174` — Patterns are validated before any pick is applied; an invalid pattern errors out cleanly with zero picks applied, and the success path always calls write_context().
- `src/config/mod.rs:365` — The hand-written merge list that omitted delete_warning was replaced by a macro driving struct+merge from one field list, with a regression test asserting delete_warning merges.
- `src/mcp/server.rs:84` — Startup orphan sweep + connect-and-continue stale pruning kill the "permanent shadow"; trusted-root sidecar + project-scoped walk kill the cross-project PID-reuse attach.


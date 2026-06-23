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
| `src/app/pane_scroll.rs:304` | `r` reload branch in handle_pane_scroll_key checks the wrong pager slot and is dead for its stated purpose | medium | S | REAL |
| `src/app/pager_handler/motion.rs:311` | `v` (edit in $EDITOR) from a bottom scrollback pager closes/restores the wrong pager slot, leaving an orphaned pager that Esc/q cannot close | high | M | REAL |
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

### PR9 · Dedup / shared-helper cleanups

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/commands.rs:83` | `:;cmd` and `:!cmd` arms are near-verbatim copies of the ShellCmd / ShellCmdCaptured prompt arms | medium | S | REAL |
| `src/app/pager_handler/mod.rs:218` | Top-overlay pty spawn block copy-pasted at three sites | medium | S | REAL |
| `src/app/pager_handler/pickers.rs:198` | History-editor cursor-sync logic duplicated four times despite an existing shared helper | medium | S | REAL |
| `src/app/pane_scroll.rs:239` | mount_scroll_pager duplicates mount_stream_pager's LowerPane arm flag-for-flag | medium | S | REAL |
| `src/app/state/dispatch.rs:45` | :limit command and limit-prompt are drifted near-duplicates | medium | S | REAL |
| `src/fs/grep.rs:288` | find_nested_git_repos is a verbatim copy of finder.rs, including the 10-entry SKIP list | medium | S | REAL |
| `src/pane/pathref.rs:130` | Handrolled strip_ansi duplicates the strip_ansi_escapes crate that is already a dependency | medium | S | REAL |
| `src/state/inventory.rs:85` | Re-yanking a modified file silently keeps the stale cached content | medium | S | REAL |
| `src/state/sessions/mod.rs:127` | load_sessions dedup collapses distinct resumable sessions that share cwd + commands | medium | S | REAL |
| `src/ui/pager/construct.rs:58` | new_plain duplicates the entire 35-field PagerView initializer from new_styled | medium | S | REAL |

### PR10 · Dead-code & stale-shim removal

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/effect.rs:274` | Stale PostAction shim: PostAction::None is dead and its justifying comment cites code that no longer typechecks | medium | S | REAL |
| `src/mcp/protocol.rs:62` | Nine doc comments detached from their functions by the verbatim mcp.rs split | medium | S | REAL |
| `src/sysinfo.rs:42` | rss_kb() still shells out to `ps` on macOS while the same file already reads RSS in-process via the sysinfo crate | medium | S | REAL |
| `src/ui/markdown/mod.rs:136` | Regex compiled on every markdown render call | medium | S | REAL |
| `src/ui/mod.rs:13` | Blanket #[allow(dead_code, ...)] on line_edit hides real dead code | medium | S | REAL |
| `src/app/state/mod.rs:84` | Update doc comment describes an abandoned migration stage as pending; three transitional result enums linger | medium | M | REAL |

### PR11 · MCP scope/robustness + path/env overlay

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/mcp/protocol.rs:324` | get_file_content is cwd-scoped while the search tools are project_home-scoped, so spyc's own search results can't be read back | medium | S | REAL |
| `src/paths.rs:17` | expand() reads HOME via std::env directly, bypassing the envset overlay it uses for every other variable | medium | S | REAL |
| `src/mcp/server.rs:259` | One slow tool call (>20s) kills the entire MCP connection, and the server's searches are unbounded | medium | M | REAL |

### PR12 · Misc correctness batch

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/agent/resume.rs:24` | Claude resume stripper misses -r/--continue/-c and eats the flag following a bare --resume | medium | S | REAL |
| `src/app/key_dispatch/prompts.rs:200` | Tab-completion PromptKind allowlists are hand-synced in three-plus places — the drift pattern the command table was built to kill | medium | S | REAL |
| `src/app/prompt.rs:590` | J jump prompt silently swallows errors — typo'd path gives zero feedback | medium | S | REAL |
| `src/app/render/chrome.rs:320` | build_rows is O(rows × delete-preview paths) — quadratic on 'delete picks' in a big directory | medium | S | REAL |
| `src/fs/listing.rs:137` | Listing::sort comparator allocates 2-4 Strings per comparison — ~1.5M+ allocations per sort of a 50k-entry directory, on the event loop | medium | S | REAL |
| `src/fs/ops.rs:52` | read_truncated caps lines but not bytes — a huge single-line file is loaded entirely into RAM on the UI thread | medium | S | REAL |
| `src/git/diff_model/build.rs:559` | Rename similarity recomputed with a second full-blob diff that gix already performed | medium | S | REAL |
| `src/main.rs:87` | Panic hook doesn't pop kitty keyboard-enhancement flags or alternate-scroll mode — terminal left misbehaving after a panic | medium | S | REAL |
| `src/pane/mod.rs:607` | vt100 panic recovery rebuilds the parser at the adopt-time size, and the resize coalescer guarantees it never gets corrected | medium | S | REAL |
| `src/app/run.rs:54` | Config-file watch on $HOME is permanently destroyed when the listing watch passes through the same directory | medium | M | REAL |
| `src/pane/tabs.rs:20` | Claude-specific session-restore state machine (PendingResumeSend) lives in the generic pane layer | medium | M | REAL |

## Closed / resolved (running log)

**✅ #514 — cluster 1, pure-Model IO moved off-thread (2026-06-23):** `state/apply.rs:113`, `state/selection.rs:77`, `clipboard.rs:189`, `clipboard.rs:250` — inline file copies / tar+zstd archiving / blocking pipe reads replaced by `Effect::FileOp` + `Effect::Inventory` off-thread workers.

**✅ ALREADY-FIXED — confirmed by the 2026-06-23 sweep (no action needed):**
- `src/app/mcp.rs:174` — Patterns are validated before any pick is applied; an invalid pattern errors out cleanly with zero picks applied, and the success path always calls write_context().
- `src/config/mod.rs:365` — The hand-written merge list that omitted delete_warning was replaced by a macro driving struct+merge from one field list, with a regression test asserting delete_warning merges.
- `src/mcp/server.rs:84` — Startup orphan sweep + connect-and-continue stale pruning kill the "permanent shadow"; trusted-root sidecar + project-scoped walk kill the cross-project PID-reuse attach.


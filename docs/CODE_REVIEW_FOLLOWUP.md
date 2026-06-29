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

**Remaining: 3** (after PR #600; down from the original 67 — the 2026-06-27 re-verification closed 3 as already-fixed/by-design, #581 fixed 1, #582 fixed the lone HIGH, #583 fixed 3 cheap blocking-IO findings, #588 fixed 2 effects-as-data findings, #590 fixed the 2 MCP-robustness findings, #592 fixed inventory re-yank, #595 fixed the 2 git-view syntect-on-main-thread findings, #597 closed rename-similarity as by-design, #598 unified `:limit`, #600 stopped the session-restore dedup dropping distinct sessions). See the closed log for the trail; the open items are the `REAL`/`PARTIAL` rows below.

## To fix — by cluster

### PR2 · Pager slot routing — orphaned / lost-scrollback pagers (incl. 2 HIGH)

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/pane_scroll.rs:304` | `r` reload branch in handle_pane_scroll_key checks the wrong pager slot and is dead for its stated purpose | medium | S | ✅ PR #523 |
| `src/app/pager_handler/motion.rs:311` | `v` (edit in $EDITOR) from a bottom scrollback pager closes/restores the wrong pager slot, leaving an orphaned pager that Esc/q cannot close | high | M | ✅ #520 (tested) |
| `src/app/pager_stream.rs:228` | Opening pager help (`?`) or navigating buffer history (`[b`/`]b`) while a stream is mid-flight kills the worker and permanently freezes the pager at "scanning…"/"computing…" | medium | M | ✅ PR #538 |
| `src/app/pane_scroll.rs:179` | Single `runtime.pager_stream` slot: starting a grep/git-view while a transcript is loading silently kills the transcript stream, leaving the scroll pager empty forever | medium | M | ✅ PR #538 |
| `src/pane/mod.rs:430` | recent_lines/save_to_file read one stale viewport, not the recent tail — vt100 contents() is viewport-only | high | M | ✅ PR #538 |

### PR3 · git status / diff porcelain parity + robustness

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/state/listing.rs:111` | Git status lookups keyed by display name never match executable files | medium | S | ✅ PR #530 |
| `src/git/status.rs:246` | One failed status item silently blanks git status for the whole repo | medium | S | ✅ PR #530 |
| `src/git/blame.rs:49` | BlameModel has no size cap and clones 3 metadata Strings per line | medium | M | ✅ PR #540 |
| `src/git/diff_model/build.rs:89` | gd diff silently drops the deletion side of an unstaged rename | medium | M | ✅ PR #533 |
| `src/git/status.rs:217` | gix status-walk setup and item-decode skeleton duplicated between repo_status and collect_worktree_plan | medium | M | ✅ PR #540 |
| `src/git/worktree.rs:170` | worktree::add leaves partial on-disk state on failure, and the leftover state blocks retry | medium | M | ✅ PR #540 |
| `src/git/status.rs:314` | repo_status diverges from porcelain on unstaged renames (parity contract violation) | medium | none | ✅ PR #530 |

### PR4 · Pager scroll math — u16 saturation + wrapped-row reachability

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/pane/mod.rs:472` | scroll_offset desyncs from vt100's clamped scrollback, making scroll-down appear dead after 'g' or over-scrolling up | medium | S | ✅ PR #544 |
| `src/pane/mod.rs:518` | max_scrollback() hardcodes 10_000; scroll_offset is never synced to vt100's clamp, creating a dead zone after scroll_to_top | medium | S | ✅ PR #544 |
| `src/ui/pager/layout.rs:131` | visual_rows underestimates actual wrapped rows (wide-char greedy waste + whitespace markers), so scroll_max clamps before the real bottom | medium | S | ✅ PR #544 |
| `src/ui/pager/render.rs:180` | wrap_line materializes the full wrapped expansion of each visible logical line every frame — allocation churn proportional to longest line, not viewport | medium | S | ✅ PR #544 |
| `src/app/pager_handler/mod.rs:311` | Files under 5 MB but over 65,535 lines load fully yet are unscrollable past line 65,536 (u16 scroll saturation) | medium | M | ✅ PR #544 |
| `src/ui/pager/mod.rs:141` | scroll: u16 silently caps every pager at 65,535 lines — reachable content beyond that is unviewable | medium | M | ✅ PR #544 |
| `src/ui/pager/render.rs:125` | With wrap on, scroll is logical-line granular — visual rows of a long line beyond the first viewport_h are unreachable | medium | M | 📝 PR #544 (documented limitation) |
| `src/ui/pager/selection.rs:281` | Visual/placement auto-scroll assumes 1 logical line = 1 screen row; with wrap on the cursor moves off-screen without scrolling | medium | M | ✅ PR #544 |

### PR5–8 · Blocking-IO off-thread sub-campaign (split by subsystem, #349 template)

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/app/navigate.rs:160` | gF reads an attacker-controlled path fully into memory synchronously on the input thread (hang/OOM via hostile pane content) | medium | S | ✅ PR #550 |
| `src/clipboard.rs:111` | Clipboard helper inherits stdout/stderr (garbles the raw-mode TUI) and leaks a zombie when stdin write fails | medium | S | ✅ PR #549 |
| `src/fs/finder.rs:136` | find_nested_git_repos re-walks the entire subtree raw (no gitignore, no cap, no cancellation) on every F open / :grep in a git root | medium | S | ✅ PR #583 |
| `src/fs/grep.rs:353` | search_to_vec blocks on the full repo walk even after the result limit is reached | medium | S | ✅ PR #527 |
| `src/pane/pty_host.rs:208` | Unbounded reader->parser channel with a reader that never stops reading: no backpressure, unbounded memory under a firehose child | medium | S | REAL |
| `src/state/sessions/mod.rs:579` | find_claude_session_name reads the entire conversation JSONL (100+ MB) into memory | medium | S | ✅ PR #583 |
| `src/agent/resume.rs:298` | gemini_resume_index_for runs `gemini --list-sessions` synchronously with no timeout on the session-restore path | medium | M | ✅ PR #583 |
| `src/app/key_dispatch/mod.rs:315` | Capture-pty writes bypass the Effect executor while sibling sinks in the same match use Effect::SendToPane | medium | M | ✅ PR #581 |
| `src/app/mod.rs:793` | crossterm::terminal::size() called inside key/action handlers (8 sites), against the effects-as-data contract | medium | M | ✅ PR #588 |
| `src/app/state/apply.rs:320` | format_long_listing and file_type_label do per-file IO inside the pure apply dispatcher | medium | M | ✅ PR #548 |
| `src/fs/long_listing.rs:155` | format_long_listing does an unmemoized getpwuid/getgrgid NSS lookup per row — L on a large listing can stall seconds-to-minutes on LDAP-backed machines | medium | M | ✅ PR #547 |
| `src/git/worktree.rs:188` | worktree::add performs a full-tree checkout synchronously on the main input thread | high | M | ✅ PR #582 |
| `src/pane/widget.rs:37` | Parser mutex held across the whole pane draw — per-frame O(cells) set_string under the lock contends with the parser worker | medium | M | ✅ #581 (already-fixed: `with_screen` scopes the lock) |
| `src/ui/blame_render.rs:44` | render_blame joins and syntect-highlights the whole file on the main thread with no size cap | medium | M | ✅ PR #595 |
| `src/ui/diff_render/mod.rs:149` | Diff render syntect-highlights both full sides on the main thread, and re-highlights from scratch on every layout toggle | medium | M | ✅ PR #595 |
| `src/ui/pager/construct.rs:182` | Pager yank/save methods do inline OS side effects, bypassing the existing Effect::CopyToClipboard path | medium | M | ✅ PR #588 |
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
| `src/mcp/server.rs:259` | One slow tool call (>20s) kills the entire MCP connection, and the server's searches are unbounded | medium | M | ✅ PR #590 |
| `src/mcp/protocol.rs:62` | Nine doc comments detached from their functions by the verbatim mcp.rs split (deferred here from PR10) | medium | S | ✅ PR #590 |

### PR12 · Misc correctness batch

| Where | Finding | Sev | Eff | Verdict |
|---|---|---|---|---|
| `src/agent/resume.rs:24` | Claude resume stripper misses -r/--continue/-c and eats the flag following a bare --resume | medium | S | ✅ PR #524 |
| `src/app/key_dispatch/prompts.rs:200` | Tab-completion PromptKind allowlists are hand-synced in three-plus places — the drift pattern the command table was built to kill | medium | S | ✅ PR #528 |
| `src/app/prompt.rs:590` | J jump prompt silently swallows errors — typo'd path gives zero feedback | medium | S | ✅ PR #522 |
| `src/app/render/chrome.rs:320` | build_rows is O(rows × delete-preview paths) — quadratic on 'delete picks' in a big directory | medium | S | ✅ PR #526 |
| `src/fs/listing.rs:137` | Listing::sort comparator allocates 2-4 Strings per comparison — ~1.5M+ allocations per sort of a 50k-entry directory, on the event loop | medium | S | ✅ PR #525 |
| `src/fs/ops.rs:52` | read_truncated caps lines but not bytes — a huge single-line file is loaded entirely into RAM on the UI thread | medium | S | ✅ PR #521 |
| `src/git/diff_model/build.rs:559` | Rename similarity recomputed with a second full-blob diff that gix already performed | medium | S | ✅ PR #597 (accepted; rationale in log) |
| `src/main.rs:87` | Panic hook doesn't pop kitty keyboard-enhancement flags or alternate-scroll mode — terminal left misbehaving after a panic | medium | S | ✅ PR #529 |
| `src/pane/mod.rs:607` | vt100 panic recovery rebuilds the parser at the adopt-time size, and the resize coalescer guarantees it never gets corrected | medium | S | ✅ PR #601 |
| `src/app/run.rs:54` | Config-file watch on $HOME is permanently destroyed when the listing watch passes through the same directory | medium | M | ✅ #581 (already-fixed: watches keyed by purpose) |
| `src/pane/tabs.rs:20` | Claude-specific session-restore state machine (PendingResumeSend) lives in the generic pane layer | medium | M | ✅ #581 (closed: by-design) |
| `src/state/inventory.rs:85` | Re-yanking a modified file silently keeps the stale cached content (moved from PR9 — dedup *behavior*, not code dup) | medium | S | ✅ PR #592 |
| `src/state/sessions/mod.rs:127` | load_sessions dedup collapses distinct resumable sessions that share cwd + commands (moved from PR9) | medium | S | ✅ PR #600 |
| `src/app/state/dispatch.rs:45` | :limit command and limit-prompt are drifted near-duplicates — unifying them changes `:limit git`/`:limit h` (fix, moved from PR9) | medium | S | ✅ PR #598 |

## Closed / resolved (running log)

**✅ PR #600 — session-restore: stop dedup dropping distinct sessions (2026-06-28):**
- `state/sessions/mod.rs:127` — `load_sessions` deduped by `cwd + tab commands`, keeping only the most recent. But each session is saved to its own `<id>.json` with its own agent transcripts / vsplit, so that key only ever collapsed *genuinely-distinct* restore points (e.g. two different Claude conversations started in the same dir) — silently making the older one unrestorable from `spyc -r`. Removed the lossy dedup; every saved session now shows. `prune_old` (`MAX_SESSIONS = 20`) still bounds how many accumulate. (Owner decision: "show all distinct sessions.") Flipped the test's dedup sub-case to assert both same-cwd+commands sessions are kept, newest-first.

**✅ PR #598 — unify `:limit` command with the `=` limit prompt (2026-06-28):**
- `state/dispatch.rs:45` — the `:limit` command and the `=`-prompt (`PromptKind::Limit`) had drifted: the prompt special-cased `git`/`g`/`h`/`harpoon` (with empty-repo / empty-harpoon error checks), but the command path treated them as literal globs and set `temp_filter` unconditionally. (`apply_temp_filter` already interprets the `"git"`/`"h"` tokens, so the *filtering* worked — but `:limit git` in a repo with no changes silently set a doomed empty filter with a misleading flash, instead of erroring.) Extracted one `apply_limit_token` helper (empty→clear, `!`→picks, `git`/`g`→git, `h`/`harpoon`→harpoon, else→glob; returns whether to rebuild) used by BOTH paths, so `:limit git`/`:limit h` now behave exactly like `=git`/`=h` — matching what FEATURES.md already documented. 2 tests (git filter set; error + filter untouched without changes).

**✅ PR #597 — rename-similarity recompute is by-design; documented (2026-06-28):**
- `git/diff_model/build.rs:559` — accepted, not changed. Investigated reusing gix's rename-detection diff: the staged `gd`/`gD` path (`gix::diff::index::ChangeRef::Rewrite`) does **not** surface the `DiffLineStats` its tracker computed (verified against gix-diff 0.64 `index/mod.rs` — no `diff` field), so there's nothing to reuse there. The `show` path (`tree_with_rewrites::ChangeRef::Rewrite`) *does* expose `diff`, but its `similarity` is **byte-based** while our `blob_similarity` is **line-based** (different metric *and* potentially a different diff algorithm), so reusing only there would make `show`'s `R<n>`/`C<n>` % disagree with `gd`/`gD`'s. Deriving similarity from our own display hunks is unreliable (context-limited hunks don't give whole-file line totals). The recompute runs in the **off-thread** diff worker (`build_payload`), so the extra blob diff per *rare* renamed file costs no UI responsiveness. Kept the one pinned, uniform metric across all three views; added call-site comments at both Rewrite arms so it isn't re-flagged.

**✅ PR #595 — git-view syntax highlights computed in the worker, off the main thread (2026-06-28):**
- `ui/blame_render.rs:44` — `render_blame` re-ran syntect over the whole file on every render (mount, `f`, resize); no cache. Split into `highlight_blame` (the syntect pass) + `render_blame_highlighted` (the cheap layout half).
- `ui/diff_render/mod.rs:149` — diff/show already cached `DiffHighlight` across `|`/`f`/resize, but the highlight was computed on the **main thread** (in `drain_pending_git_view`). Now both the model and its highlight are built in the off-thread `build_payload` worker and bundled in one `GitViewContent` enum (`Diff(model,hl)` / `Show(box)` / `Blame(model,hl)`) — a model/highlight mismatch is unrepresentable. The main thread only lays the cached highlight out at the current width; no syntect on the input thread for diff/show/blame. The inline `render_diff`/`render_show`/`render_blame` are now `#[cfg(test)]`. Behavior-preserving; new test covers blame mount + `f` re-render from the cache.

**✅ PR #592 — re-yank refreshes stale inventory content (2026-06-28):**
- `state/inventory.rs:85` — `yank` early-returned when the path was already cached (`if self.contains(path) { return Ok(()) }`), so re-yanking an *edited* file silently kept the stale bytes — a later `put` wrote the old content. Now `yank` reuses the existing entry's id and overwrites its `.dat` + metadata, so a path keeps one entry (dedup intent preserved) but always caches the current content/size; an uncached path still gets a fresh UUIDv7. Test updated to edit-then-re-yank and assert the cached size + the bytes `put` delivers track the new content.

**✅ PR #590 — bound slow MCP read tools + reattach detached doc comments (2026-06-28):**
- `mcp/server.rs:259` — the stdio proxy bounds its socket read with `PROXY_IO_TIMEOUT` (20 s) and, on timeout, sends the agent an error *and then breaks the loop*, ending the whole MCP stdio process. So any single read tool that walked a large tree for >20 s killed the entire MCP connection, not just that call. The tree-walking read tools (`search_paths`/`search_content`, `git_status`/`git_log`/`git_diff`, `list_worktrees`) now run through a new `call_with_timeout` (detached thread + `recv_timeout`, the same pattern the writable-action path already uses) bounded by `READ_TOOL_TIMEOUT` — derived a few seconds below `PROXY_IO_TIMEOUT` so they can't drift apart. A slow call returns a clean JSON-RPC error server-side before the proxy gives up. No cancellation (a timed-out thread runs to completion — pure reads, harmless). The bounded-list tools (`search_picks`/`search_inventory`) and byte-capped `get_file_content` stay direct — they can't run away on tree size. 3 new tests (fast→Ok, slow→Err, `READ_TOOL_TIMEOUT < PROXY_IO_TIMEOUT` invariant).
- `mcp/protocol.rs:62` — the verbatim `mcp.rs` → `mcp/` split detached doc comments from their functions. Mapped each back against the **pre-split `mcp.rs`** (authoritative via `git show`): a rotation chain — `resolve_context_path` (→ mod.rs, was on `handle_initialize`), `search_root` (→ readers.rs, was orphaned before `MAX_LSP_MESSAGE_BYTES`), `McpConfigStatus` (→ config.rs, was on `pid_from_sock_path`), `pid_from_sock_path` (→ server.rs, was on `ensure_mcp_json`), `ensure_mcp_json` (→ config.rs, was on `handle_socket_connection`) — plus two merged blocks (`dispatch`'s doc stranded on `mod config;`; `start_socket_server`'s on `socket_bind_error`). All now sit on their real owners; legitimate later content updates (`cleanup_socket`, `clean_local_mcp_entry`, `mcp_log`) left as-is.

**✅ PR #588 — route pager yank/save through the executor + cache terminal size (2026-06-28):**
- `ui/pager/construct.rs:182` — the pager `y`/`Y`/visual yanks and the `s` save did inline `clipboard::copy` / `std::fs::write` in the motion/visual handlers, bypassing the sole effect executor. New `Effect::CopyToPagerClipboard { text, ok_msg }` and `Effect::SavePagerOutput { content }` move the IO into `run_effects`. The confirmation still lands in the **active pager's title** (new `set_active_pager_flash`), not the status bar — `Effect::CopyToClipboard` flashes the status bar, which a pager overlay would hide, so the pager keeps its own title flash + exact former messages. `PagerView` exposes pure text extractors (`source_yank_text` / `visible_yank_text` / `visual_yank_text` / `save_content`); the copy/write is the executor's.
- `app/mod.rs:793` — 8 `&self`/`&mut self` handler sites called `crossterm::terminal::size()` inline (a syscall on the input path, against effects-as-data). Cached in `ViewState.term_size`, seeded at startup and refreshed in `handle_resize`; the handlers (open_help, right_preview_body_width, pane_scroll, pager body/wrap, image render, mermaid view, resize_panes_to_layout) read the cache. Off-thread / associated-fn sites without a `self` handle (`build_pager_view`, `pane_spawn_size`, `top_overlay_size`, `git_view_body_width`, `spawn_capture`) keep the live call.

**✅ Follow-up to #583 — fix UTF-8 crash-to-skip in the session-title tail-read (2026-06-28):**
- `state/sessions/mod.rs` — #583's tail-read used `read_to_string` *after* `seek(len - 64 KB)`. A seek landing mid-UTF-8-codepoint makes `read_to_string`'s strict validation fail, so the whole file was silently dropped and the title went missing in the `spyc -r` picker — intermittent and data-dependent (ASCII files dodged it, real conversations with Unicode didn't). Now reads bytes + `String::from_utf8_lossy`, extracted into a testable `title_from_jsonl_tail` helper. Two regression tests (small-file path + a deterministic mid-codepoint seek boundary). Found by the Opus audit pass; the fast tail-read design is kept (the ≤64 KB early-title window is an accepted speed tradeoff on restore).

**✅ PR #583 — cheap blocking-IO trio: session tail-read, gemini timeout, finder depth cap (2026-06-28):**
- `state/sessions/mod.rs:579` — `find_claude_session_name` read the entire JSONL with `read_to_string`, loading 100+ MB session files into memory on the session-restore path. Now tail-reads the last 64 KB via `Seek::seek(SeekFrom::Start(len - TAIL))` + `read_to_string`; skips the leading partial line when a seek happened. The `custom-title` entry is searched in reverse so the most recent title wins regardless of file position.
- `agent/resume.rs:298` — `gemini_resume_index_for` called `Command::output()` (blocking, no timeout) on the restore path. A hung `gemini --list-sessions` (first invocation, network issues) would freeze spyc's session restore indefinitely. Now spawns the child with `Stdio::piped`, reads stdout in a detached thread, and uses `mpsc::recv_timeout(2s)` — if the child doesn't respond in time, it gets killed and the restore falls back to bare `gemini` (existing error path unchanged).
- `fs/finder.rs:136` — `find_nested_git_repos` had no depth limit; a `F` search or `:grep` launched from a shallow root (e.g. `$HOME`) could walk the entire filesystem. Added `MAX_DEPTH = 5` on the stack tuple; stops descending past 5 levels below root. Legitimate nested repos are shallower; build trees are caught by the `SKIP` list.

**✅ PR #582 — interactive `W n` worktree checkout off-thread (the lone HIGH; 2026-06-28):**
- `git/worktree.rs:188` — interactive `W n` ran its full-tree `gix` checkout synchronously on the input thread (`prompt.rs` → `git::worktree::add`), freezing the whole UI (panes, agent, input) for seconds on a big repo. The MCP `create_worktree` was already off-thread; this routes the key through the same worker. New `Effect::WorktreeCreateInteractive`; `WorktreeOutcome.reply` → `WorktreeCompletion {Mcp | InteractiveCreate}`; `apply_worktree_outcomes` chdirs the focused column into the new tree (+ flash + reconcile harpoon) when it lands — mirrors the former synchronous completion; `WorktreeJobResult.created_path` carries the new tree. Reuses the `worktree_results` slot + `Message::WorktreeJobDone` (no new variant). `W d` removal stays synchronous (plain `git::worktree::remove`, no checkout — not the HIGH). Behavior change on a daily-driver key → owner live-test before merge.

**✅ PR #581 — capture-pty writes → `Effect::SendToCapture`; 3 findings closed (2026-06-27):**
- `key_dispatch/mod.rs:315` — the `!`-capture child's keystroke + paste writes bypassed the sole effect executor (inline `capture.host.writer.write_all`) while sibling input sinks route through `Effect::SendToPane`. Added `Effect::SendToCapture { bytes }` (the capture child is a bare `PtyHost`, not a `Pane`, so it gets its own variant); `handle_capture_key` + the `handle_paste` capture arm now emit it and `run_effects` does the write. Behavior-preserving: same master writer, same `let _ =` ignore-on-error, same tick. Live-pty path → owner test (type into a `!sudo`/ssh prompt, paste into one).
- `pane/widget.rs:37` — **already fixed**: the vt100 parser lock is now taken in tight `with_screen()` closures and released before the per-cell paint, not held across the draw. No change.
- `app/run.rs:54` — **already fixed**: `watch.rs` keys each fs-watch by *purpose* (listing / git / preview / config) in a never-cleared set, so a listing walk through `$HOME` can't destroy the config watch. No change.
- `pane/tabs.rs:20` — **by-design**: `PendingResumeSend` lives in the pane layer as a deliberate pane-lifecycle tradeoff (a generic per-tab `Option`; non-Claude agents leave it `None`). Revisit only if a 2nd agent needs restore.

**✅ PR #554 — off-thread special-file open (completes the #550 follow-up; 2026-06-25):**
- `pager_handler/mod.rs` / `navigate.rs` / `file_ops.rs` — #550 made the shared pager builder *refuse* a tty/FIFO/socket and *sample* readable devices, but the open stayed **synchronous on the input thread**: an exotic non-tty blocking-read char device (`/dev/input/*`, or a regular file on a hung mount) could still freeze the UI. The owner asked for fully-off-thread "nothing ever freezes." Now a cheap stat (`pager_handler::plan_pager_open`, never blocks) splits the open: a **regular file** builds + installs **inline** (unchanged — no thread spawn, no flicker, immediate read error), while a **non-regular file** is read on the file-op worker via `FileOp::OpenSpecialFile { path, theme, open_as_rendered, wrap, dest }` → `build_pager_view` off-thread → `FileOutcome::SpecialFileOpened` → `install_pager_at_dest` at the carried `PagerDest` (`Overlay { scroll }` for Enter/`gF`, `TopPane` for `D`). A readable device samples and lands a pager a tick later; a truly-blocking one parks the worker — the input thread never blocks. The three open sites (Enter `activate`, `D` `display_in_pane`, `gF` `goto_file_navigate`) all route through `plan_pager_open`; the single worker-spawn (`App::spawn_file_op`) is shared between the `Effect::FileOp` executor arm and the `gF` executor open. Reused `Message::FileOpDone` (no new variant → sidesteps the #531 3-list-lockstep trap). **Limitation (documented):** a parked worker on a never-readable device leaks its thread + fd (no portable way to interrupt a blocking `read(2)`); the invariant held is *the UI never freezes*, not *the read always completes*. 6 new tests (worker build/refuse, drain install/flash at each dest, gF off-thread diversion); the old synchronous gF-refusal test became the `plan_pager_open`-diverts-off-thread test.

**✅ PR #550 — gF bounded file open + non-regular-file guard in the shared pager builder (2026-06-24):**
- `app/navigate.rs:160` — the `gF` (open-file-reference) handler did a raw `std::fs::read_to_string(&path)` on a path extracted from **arbitrary pane content**, slurping the whole file into memory on the input thread — a hang/OOM vector for a hostile multi-GB path. Now routes through `build_pager_view_for_file` (the same builder Enter/`D` use), which caps the read at `MAX_PAGER_BYTES` and adds syntax/markdown rendering + the truncation banner; the referenced line is applied as the scroll override. **Behavior change:** the `gF` pager is now syntax/markdown-rendered (was plain), consistent with Enter.
- `pager_handler/mod.rs build_pager_view` — **central special-file guard (owner found the sibling hole while testing: Enter on `/dev/stderr` locked up).** `build_pager_view` (the shared builder for Enter / `D` / gF / preview) called `shell::looks_like_text` → `File::open` + `read` *before* the size check; on a tty (`/dev/stderr`) that read **blocks** waiting for input. The content reads further down are already byte-capped (`read_hex_window` / `read_truncated`), so **readable streaming devices sample fine** — the only hazards are block-prone types. Added a guard before the sniff: refuse a FIFO/socket (via `metadata`, a stat that never blocks) and a terminal (open the device — char-device opens don't block — and check `IsTerminal`), but **let `/dev/zero`, `/dev/urandom`, `/dev/null`, block devices through** to the capped hex sample (owner liked sampling them). One guard covers Enter / gF / preview; gF's redundant local guard dropped. Tests: gF refuses a unix socket + opens a regular file at line + directory chdir. (Build-and-hold for owner test.) **Note: this left the open *synchronous* — an exotic *non-tty* blocking-read char device (e.g. `/dev/input/*`) could still block the input thread. Resolved in PR #554 below (the fully-off-thread `OpenSpecialFile` follow-up the owner asked for).**

**✅ PR #549 (clipboard) — null the helper's stdout/stderr + always reap (2026-06-24):**
- `clipboard.rs:111` — `spawn_and_pipe` (shared fork-exec for `pbcopy`/`wl-copy`/`xclip`/`xsel`) left the helper's stdout/stderr **inherited**, so anything it printed corrupted spyc's raw-mode alternate-screen TUI; and it `?`-returned on a `write_all` failure **before** `child.wait()`, leaking a zombie. Now spawns with `Stdio::null()` for stdout+stderr and captures the write result so the child is **always** reaped; the exit status takes precedence over a stdin-write EPIPE (a non-zero exit still halts the Linux helper cascade via `ErrorKind::Other`). Happy-path yank unchanged. New `copy_reaps_child_and_errors_when_helper_ignores_large_stdin`. (Gate-verified; happy-path-preserving, merged without live test.)

**✅ PR #548 — move `L` / file-type IO off the pure apply dispatcher and onto a worker (2026-06-24):**
- `app/state/apply.rs:320` — the `LongList` and `FileType` arms called `format_long_listing` (one `symlink_metadata` + owner/group resolution per path) and `file_type_label` (`symlink_metadata` + 512-byte magic read per path) **inside the pure `AppState::apply` dispatcher**, on the input thread — violating the no-IO-in-apply invariant and able to stall on a big selection. Now both arms are pure: they collect the target paths and emit `Effect::FileOp(FileOp::LongList { paths, title })` / `FileOp::FileType { paths }`. The existing file-op worker (`run_file_op` → `runtime.file_results` → `Message::FileOpDone`, already wired through coalesce/dispatch) runs the IO off-thread and `apply_one_file_outcome` opens the pager / flashes via a new shared `open_pager_request` helper (also used by the `Update::OpenPager` bridge). No new `Message` variant (reused `FileOpDone` — sidesteps the #531 3-list-lockstep trap). The now-dead `ApplyResult::OpenPager` variant + its `From` arm were removed (apply no longer opens pagers directly). **Behavior change:** `L` / file-type open a tick later (async) instead of synchronously — imperceptible for small dirs, keeps the UI responsive on huge selections. 4 new file_ops tests + updated apply-dispatch assertions. (Build-and-hold for owner test.)

**✅ PR #547 — memoize owner/group NSS lookups in the `L` long-listing (2026-06-24):**
- `fs/long_listing.rs:155` — `format_long_listing` resolved each row's owner/group via `uzers::get_user_by_uid` / `get_group_by_gid` (NSS lookups → network on LDAP/AD machines), once *per row*, so `L` on a big directory could stall seconds-to-minutes. Memoize per invocation with `HashMap<u32, String>` uid→owner and gid→group caches threaded into `make_long_row`: one lookup per distinct id. Behavior-identical (same names, cached). New `long_listing_memoizes_owner_group_per_id`. (Gate-verified perf; no live test.) The sibling finding `apply.rs:320` (the same formatter runs inside the *pure* apply dispatcher + on the main thread) is the next PR.

**✅ PR #544 — pager scroll math: u16 saturation, wrap-row reachability, pane scroll sync (2026-06-24):** all 8 PR4 findings (3 root causes; 7 fixed, 1 documented).
- `ui/pager/mod.rs:141` + `pager_handler/mod.rs:311` — `PagerView.scroll` was `u16`, silently capping every pager at 65 535 lines so a longer file (big log, generated source) was unreachable past line 65 536. Widened `scroll` (and `saved_alt_scroll`) to `usize`; all the scroll-domain math (`scroll_max`/`scroll_by`/`scroll_to_match`/`indicator_string`/…) widened with it, while `viewport_height` stays `u16` (a real terminal dim). Persistence (`state/pager_positions.rs`) widened `u16`→`u64` (fixed-width for the on-disk JSON; old small values still parse). New `scroll_reaches_beyond_u16_max_lines`.
- `ui/pager/layout.rs:131` — `visual_rows` used `total_width.div_ceil(width)`, which assumes perfect packing and *underestimates*: a 2-cell glyph that doesn't fit the last cell of a row is pushed whole to the next, so `scroll_max` clamped short of the true bottom on wide-char content. Rewrote it to mirror `wrap_line`'s greedy fill exactly (including the "force ≥1 char per row" guard). New `visual_rows_counts_wide_char_greedy_waste`.
- `ui/pager/selection.rs:281` — `scroll_to_keep_visible` (visual-cursor auto-scroll) assumed 1 logical line = 1 row, so under wrap the cursor slid off the bottom without the viewport following. Made it wrap-aware: count visual rows from the top to the cursor; if they overflow, walk back from the cursor accumulating visual rows to find the new top. New `scroll_to_keep_visible_is_wrap_aware`.
- `ui/pager/render.rs:180` — the render path wrapped each visible logical line into its *entire* expansion every frame (hundreds of pieces for a long line), then painted only the visible ones. Added `wrap_line_capped(line, width, max_rows)` (`wrap_line` now delegates with `usize::MAX`, byte-identical) and the renderer passes the remaining viewport budget. New `wrap_line_capped_bounds_to_visible_rows`.
- `pane/mod.rs:472` + `pane/mod.rs:518` — `max_scrollback()` hardcoded `10_000` and `scroll_offset` was never reconciled with vt100's real clamp, so after `g` (scroll-to-top set the offset to the 10k guess) scroll-down decremented a phantom counter with no visible movement until it fell below the real length (the "scroll-down dead" dead zone). `max_scrollback` now probes the real length via `scrollback_len`, and `apply_scroll` reads vt100's clamped offset back into `scroll_offset`. Live-pty (`^a-v` raw scroll) — owner-tested, not unit-tested per the campaign's pty lesson.
- `ui/pager/render.rs:125` — 📝 **documented limitation, not fixed.** `scroll` is logical-line-granular, so a *single* logical line that wraps to more visual rows than the viewport can't be scrolled *through* (the next step jumps to the following logical line). `scroll_max` already keeps the trailing lines reachable; the residual intra-line gap needs visual-row-granular scrolling (an intra-line row offset) — a scroll-model rearchitecture too risky to fold into this PR. Documented on the `PagerView::wrap` field with mitigations (toggle wrap off, or `v` to open in `$EDITOR`).

**✅ PR #540 — git: dedup status-walk config, worktree cleanup on failure, blame Arc + size cap (2026-06-24):**
- `git/status.rs:217` — `repo_status` and `collect_worktree_plan` each built the gix status platform with identical `tree_index_track_renames(Given(Rewrites::default()))` and no `index_worktree_rewrites`. Extracted `make_status_platform(repo, untracked_mode)` in `status.rs` as the single source for those parity-sensitive options; both callers now delegate to it. (Gate-verified; behavior-identical.)
- `git/worktree.rs:170` — `add` created `target/` and the admin dir with `create_dir_all`, then on any subsequent error (checkout, index write, admin file write) returned immediately, leaving both directories on disk. A retry with the same branch name would then hit the non-empty-dir guard and fail indefinitely. Extracted `materialize_worktree` (create dirs + `checkout_and_write` + cleanup); `add` calls it, and on error it removes both partial directories before returning. The branch ref (created earlier) is left in place — an existing branch is reused on retry. (Gate-verified; `materialize_worktree_cleans_up_partial_dirs_on_failure` forces a real checkout failure via a null tree id and asserts both dirs are gone — fails without the fix — plus `add_succeeds_after_a_prior_failed_attempt` end-to-end.)
- `git/blame.rs:49` — `BlameLine.short_id/author/date` were `String`, so a file where one commit owns N lines allocated 3×N `String`s in the inner loop even though the values are identical across the hunk. Changed those three fields to `Arc<str>`; the `meta_cache` stores `(Arc<str>, Arc<str>, Arc<str>)` and per-line clones are O(1) ref-count bumps. Added `MAX_BLAME_LINES = 50_000` cap with `BlameModel.truncated` flag; `blame_render` appends a note line when set. (Behavior change for very large files; new `blame_truncates_at_max_lines` + `truncated_model_appends_note` tests.)

**✅ PR #538 — pager slot routing: help-stash guard, two-slot streams, vt100 scrollback (2026-06-24):**
- `app/pager_stream.rs:228` — `drain_pager_stream` killed any stream whose slot was empty, including one sitting behind the `?` help overlay (`view.pager_help_stash`). Now `drain_one_pager_stream` checks the stash before evicting an overlay stream, so opening help mid-stream no longer permanently freezes "scanning…" / "computing…". Also fixed `nav_pager_buffer` (`[b`/`]b`): it now clears `runtime.pager_stream` + `streaming`/`stream_id` flags on the current pager before pushing it to history, so navigating away while a stream is live closes the stream cleanly instead of orphaning the slot.
- `app/pane_scroll.rs:179` — the single `runtime.pager_stream` slot served both the overlay pager (grep/git-view → `view.pager`) and the lower-pane scroll pager (transcript → `view.scroll_pager`). Opening grep while a transcript loaded always evicted the transcript stream and left the scroll pager empty forever. Split into two slots: `runtime.pager_stream` (overlay only) and `runtime.scroll_stream` (lower-pane only). `spawn_pager_stream` routes by `PagerStreamMount`; `drain_pager_stream` drains both sequentially. `stash_scrollback_pager_to_active_tab` / `restore_active_tab_scrollback_pager` updated to stash the correct slot.
- `pane/mod.rs:430` — `recent_lines` and `save_to_file` used `vt100::Screen::contents()`, which returns only the current viewport (terminal-height rows at the current scroll offset). Replaced with `ui::scrollback::lines_from_scrollback` — the existing full-scrollback page-walk — so both operations read the full scrollback tail regardless of scroll position.
Two new tests: `help_overlay_preserves_in_flight_overlay_stream`, `overlay_stream_coexists_with_scroll_stream`. (Gate-verified; live-pty behavior: concurrent transcript + grep, help-mid-stream, `[b`/`]b` while streaming.)

**✅ PR #533 — gd diff: keep the deletion side of an unstaged rename (2026-06-24):**
- `git/diff_model/build.rs:89` — the working-tree (`gd`) diff enabled `index_worktree_rewrites`, so an unstaged `mv` (tracked file renamed on disk, not staged) surfaced as a single `IwItem::Rewrite`; `collect_worktree_plan` emitted only its **destination** (an addition) and dropped the **source** — so `gd` showed the renamed file as all-new and silently lost the original's deletion. Same root cause as #530's status fix: git itself never pairs a worktree-only rename (the dest is untracked), so `git diff HEAD` reports the source DELETED + the dest ADDED. Dropped `index_worktree_rewrites` (staged renames still collapse via `tree_index_track_renames`); the unstaged rename now decomposes into a Modification{Removed} + DirectoryContents{Untracked} that the existing arms turn into a deletion + addition. The now-defensive `IwItem::Rewrite` arm decodes BOTH sides so parity holds even if rewrite detection is re-enabled. New test `working_unstaged_rename_shows_both_delete_and_add` (verified it fails on the old code). (Gate-verified.)

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

**✅ PR #524 — claude resume-flag stripper (2026-06-23):**
- `agent/resume.rs:24` — `command_without_resume` only handled `--resume` and unconditionally ate the next token. Now handles `--resume`/`-r` (drops an id arg, but not a following flag) and `--continue`/`-c` (no arg), so the fresh-session fallback is built correctly. 6 new unit tests, incl. the bare-`--resume`-eats-flag regression.

**✅ PR #525 — listing-sort key precomputation (2026-06-23):**
- `fs/listing.rs:137` — `Listing::sort`'s comparator re-ran `to_ascii_lowercase()` (a fresh allocation) on both operands every comparison — 2 Strings for Name, up to 4 for Ext — so a 50k-entry directory burned ~1.5M short-lived allocations per sort, on the event loop. Switched to decorate-sort-undecorate: a per-entry `SortKey` is built once (O(n) allocations), and the comparator only compares precomputed keys. Behavior-identical (dirs-first grouping, per-mode natural direction, `reversed` semantics all preserved); 5 new tests lock the exact ordering for every mode + reversed. (Gate-verified; pure perf.)

**✅ PR #526 — build_rows pending-delete set (2026-06-23):**
- `render/chrome.rs:320` — `build_rows_for` checked `pending_delete` with a per-row linear scan over `pending_delete_preview` (`v.iter().any(|p| p == &rd.path)`), i.e. O(rows × preview paths) — quadratic when you "delete picks" in a big directory (every picked path is also in the preview). Hoisted the preview into a `HashSet<&Path>` built once, so the per-row check is O(1) and `build_rows` stays linear. Behavior-identical; new test `build_rows_marks_only_pending_delete_paths` locks the exact set of flagged rows (and the no-preview case). (Gate-verified; pure perf.)

**✅ PR #527 — grep search_to_vec early exit (2026-06-23):**
- `fs/grep.rs:353` (the `search_to_vec` fn) — after collecting `limit` matches the loop `break`s, but the receiver stayed alive through `handle.join()`. The channel is unbounded, so the worker's sends never blocked and it crawled the *entire* repo before `join` returned (e.g. `search_content` with a small limit on a huge tree). Now drops `rx` before joining, so the worker's next batch send fails and it bails early (the same cancellation path the live grep pager already uses). Result set is identical; new test `search_to_vec_caps_across_many_files` guards the multi-file cap. (Gate-verified; pure perf.)

**✅ PR #528 — one path-completion allowlist (2026-06-23):**
- `key_dispatch/prompts.rs:200` — the set of `PromptKind`s that Tab-complete a path was duplicated across the simple-prompt and vi-prompt Tab handlers, and the two had *already drifted* (the simple list omitted the shell/command kinds). Extracted `PromptKind::wants_path_completion()` as the single allowlist; both handlers consult it. Behavior-preserving (the union equals the vi list, and the extra shell/command kinds — `;`/`!`/`:` — are always vi-editor prompts that never reach the simple handler). New test `wants_path_completion_is_the_one_allowlist` pins the exact in/out set across every kind. (Gate-verified.)

**✅ PR #529 — panic-hook terminal restore parity (2026-06-23):**
- `lib.rs` panic hook (finding `main.rs:87`) — the crash-time restore popped raw mode / alt screen / bracketed-paste / mouse pointer but, unlike the clean `restore_terminal`, never popped the **kitty keyboard-enhancement flag** or **alternate-scroll mode**, and didn't re-show the cursor. After a spyc panic those stay set for the *rest of the shell session*, corrupting key delivery / scroll-wheel for the next TUI. Added `PopKeyboardEnhancementFlags`, `DisableAlternateScroll`, and `cursor::Show` to mirror `restore_terminal`. Crash-path-only and strictly additive (cannot affect normal operation); verified by inspection against the live-proven clean teardown. (Gate-verified; the actual post-panic restore is best-effort, as the hook always was.)

**✅ PR #530 — git status parity + robustness (2026-06-23):**
- `git/status.rs:314` (⚠︎ contested → confirmed REAL by hand) — `repo_status` enabled `index_worktree_rewrites`, so a plain filesystem `mv` of a tracked file (no `git add`) collapsed to a single `R renamed` — but `git status --porcelain -unormal` reports ` D orig` + `?? renamed`: it never pairs a worktree-only rename because the destination is *untracked* and worktree-half rename detection only considers tracked entries. Dropped `index_worktree_rewrites` (staged renames still collapse to `R` via `tree_index_track_renames`, which is the half git actually does detect), and made the now-defensive `IwItem::Rewrite` arm decode source→Deleted + dest→untracked so the parity contract holds whether or not rewrite detection is on. New parity case `case08b_unstaged_rename` cross-checks against `git`.
- `git/status.rs:246` — one `Err` item from the gix status iterator made `repo_status` return `None`, blanking the *entire* repo's git markers. Now skips the single bad item (`let Ok(item) = item else { continue }`) so partial status beats none; a failure to even start the walk still returns `None`.
- `app/state/listing.rs:111` — both the render gutter (`render/chrome.rs`) and the `git` temp-filter looked up `git.files` by the decorated `display` name. `Entry::display_name` appends `*` to executables (ls -F), but the git map keys files by bare basename, so **no executable ever showed its git marker** (dirs matched because their `/` is part of the key). Added one shared `RowData::git_key()` that strips the executable `*`; both sites use it. Tests: `git_key_tests` (the predicate, incl. a `foo*`-named exec) + `build_rows_resolves_git_status_for_executables` (end-to-end). (Gate-verified.)

**✅ PR #535 — pager slot routing: help-stash guard + two-slot streams + vt100 scrollback (2026-06-24):**
- `pager_stream.rs:228` (help / `[b`/`]b` kills stream) — two fixes:
  - **Help overlay**: `drain_pager_stream` now checks `view.pager_help_stash` before killing an overlay stream on id-mismatch. The stream pager stashed behind `?` is not closed; its worker keeps running and the buffered output drains when help is dismissed. New test `help_overlay_preserves_in_flight_overlay_stream`.
  - **Buffer nav `[b`/`]b`**: `nav_pager_buffer` now clears `pager.streaming = false` and drops the stream before pushing a mid-flight pager to history, so navigating back with `]b` shows partial results rather than a "scanning…" pager with no stream to complete it.
- `pane_scroll.rs:179` (single slot kills transcript) — split `runtime.pager_stream` (overlay: grep/git-view → `view.pager`) from a new `runtime.scroll_stream` (lower-pane transcript → `view.scroll_pager`). `spawn_pager_stream` routes to the correct slot by mount type. `drain_pager_stream` drains both sequentially; tab-stash/restore now parks and restores from `scroll_stream`. New test `overlay_stream_coexists_with_scroll_stream`.
- `pane/mod.rs:430` (`recent_lines`/`save_to_file` viewport-only) — `vt100::Screen::contents()` returns only the viewport at the current scrollback offset. Both methods now use the `ui::scrollback::lines_from_scrollback` page-walk (already used by `^a v`), which walks the full scrollback + live screen via the cell API. (Gate-verified; 1328 tests green.)

**✅ ALREADY-FIXED — confirmed by the 2026-06-23 sweep (no action needed):**
- `src/app/mcp.rs:174` — Patterns are validated before any pick is applied; an invalid pattern errors out cleanly with zero picks applied, and the success path always calls write_context().
- `src/config/mod.rs:365` — The hand-written merge list that omitted delete_warning was replaced by a macro driving struct+merge from one field list, with a regression test asserting delete_warning merges.
- `src/mcp/server.rs:84` — Startup orphan sweep + connect-and-continue stale pruning kill the "permanent shadow"; trusted-root sidecar + project-scoped walk kill the cross-project PID-reuse attach.


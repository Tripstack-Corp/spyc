# Code review follow-up — finalizing the June 2026 review

**Status:** ACTIVE tracker (opened 2026-06-21). The June 2026 deep review
([`docs/archive/CODE_REVIEW_2026-06.md`](archive/CODE_REVIEW_2026-06.md)) drove a
large remediation campaign (PRs #329–#424) that closed the entire Low-severity tier
and all security / contained-correctness / dead-code / dedup work. This doc tracks
**what that campaign left**: the High/Medium findings that were deferred or never
verified.

**Goal:** drive every remaining finding to a clean terminal state — each one either
**fixed** (own cluster PR) or **closed** (refuted / by-design / accepted) — and keep
an honest ledger while doing it.

## What's left (from the archived doc, reconciled against code 2026-06-21)

Two buckets, **74 findings** total:

- **Resolve (29)** — confirmed real in the original review but never fixed. Dominated
  by the **Tier-5 "blocking-IO off-thread"** class (inline disk IO on the input/render
  thread), plus pager-slot correctness and maintainability dedups. Includes 3 of the
  4 still-open High findings.
- **Verify (45)** — the adversarial verifier fleet died on the org spend limit before
  reaching these; they're plausible-but-unchecked. Each needs a confirm/refute pass
  against current code *before* any fix.

> **Annotation drift is real — re-verify before acting.** The archived doc's
> High/Medium `###` status lines were annotated opportunistically as fixes landed, so
> they're stale: **absence of a `✅` does not mean a finding is open**, and a row here
> may already be fixed. Confirmed example: `pager_handler/motion.rs:311` reads as open
> but is **already fixed** in current code (the `pane_scroll` teardown + restore landed
> since). Treat every row below as a *candidate* until re-checked against HEAD.

## Workflow

1. Pick a finding, or a root-cause cluster of them.
2. **Open a worktree via the spyc MCP** (`create_worktree` → `open_worktree`), one
   branch per cluster — never edit in the main checkout.
3. **Verify against current code first.** If refuted / already-fixed / by-design →
   mark the row closed with a one-line reason. If real → fix it.
4. Fixes are behavior changes → own `fix:`/`refactor:` PR per root cause; build
   `make release` and let the owner test before merge (behavior PRs aren't auto-merged).
5. For the blocking-IO items, reuse the **#349 Effect+worker template**
   (`src/app/graveyard_ops.rs`): the handler emits an `Effect`, `run_effects` spawns a
   detached worker, completion returns as a `Message` drained in the pre-recv scan.
6. Tick the row (`✅ #NNN` or `closed: <reason>`) **on the PR branch**, same commit —
   annotating after merge strands it (campaign lesson).

## Priority

1. **High severity first** — `state/apply.rs:113` + `selection.rs:77` (inline tar/zstd
   + copy IO in the pure Model), `git/worktree.rs:188` (sync full-tree checkout, now
   also reachable via MCP `create_worktree`), `pane/mod.rs:430` (viewport-only tail
   read). Plus `motion.rs:311` → verify-and-close (likely already fixed).
2. **Security / correctness in the Verify bucket** — `mcp/server.rs:84` & `:259`,
   `pane/mod.rs:607`, the `git/status.rs` porcelain-parity pair, the resume strippers.
3. **The blocking-IO cluster** as a focused sub-campaign (most of the perf rows in both
   buckets) — biggest responsiveness payoff and they all share the #349 template.
4. Everything else — pager wrap/scroll math, maintainability dedups — as batched
   cluster PRs.

## Resolve — confirmed, unfixed (29)

| Where | Finding | Lens | Sev | Status |
|---|---|---|---|---|
| `src/app/pager_handler/motion.rs:311` | `v` (edit in $EDITOR) from a bottom scrollback pager closes/restores the wrong pager slot, leaving an orphaned pager that Esc/q cannot close | correctness | high | open — partially addressed; needs live-pty repro (see notes) |
| `src/app/state/apply.rs:113` | AppState::apply performs inline filesystem IO (file copies, tar+zstd archiving) through the state-module mutators, violating the pure-Model contract | maintainability | high | ✅ |
| `src/git/worktree.rs:188` | worktree::add performs a full-tree checkout synchronously on the main input thread | perf | high | open |
| `src/pane/mod.rs:430` | recent_lines/save_to_file read one stale viewport, not the recent tail — vt100 contents() is viewport-only | perf | high | open |
| `src/app/clipboard.rs:189` | pipe_content_to_pane does unbounded blocking file reads inline in the input handler | perf | medium | ✅ |
| `src/app/clipboard.rs:250` | Clipboard/file-op handlers do blocking filesystem IO inline instead of returning Effects | maintainability | medium | ✅ |
| `src/app/commands.rs:83` | `:;cmd` and `:!cmd` arms are near-verbatim copies of the ShellCmd / ShellCmdCaptured prompt arms | maintainability | medium | open |
| `src/app/effect.rs:274` | Stale PostAction shim: PostAction::None is dead and its justifying comment cites code that no longer typechecks | maintainability | medium | open |
| `src/app/key_dispatch/mod.rs:315` | Capture-pty writes bypass the Effect executor while sibling sinks in the same match use Effect::SendToPane | maintainability | medium | open |
| `src/app/key_dispatch/prompts.rs:200` | Tab-completion PromptKind allowlists are hand-synced in three-plus places — the drift pattern the command table was built to kill | maintainability | medium | open |
| `src/app/mcp.rs:174` | MCP PickFiles applies picks for valid patterns, then reports total failure and skips write_context | correctness | medium | open |
| `src/app/mod.rs:793` | crossterm::terminal::size() called inside key/action handlers (8 sites), against the effects-as-data contract | maintainability | medium | open |
| `src/app/navigate.rs:160` | gF reads an attacker-controlled path fully into memory synchronously on the input thread (hang/OOM via hostile pane content) | security | medium | open |
| `src/app/pager_handler/mod.rs:218` | Top-overlay pty spawn block copy-pasted at three sites | maintainability | medium | open |
| `src/app/pager_handler/mod.rs:311` | Files under 5 MB but over 65,535 lines load fully yet are unscrollable past line 65,536 (u16 scroll saturation) | correctness | medium | open |
| `src/app/pager_handler/pickers.rs:198` | History-editor cursor-sync logic duplicated four times despite an existing shared helper | maintainability | medium | open |
| `src/app/pager_stream.rs:228` | Opening pager help (`?`) or navigating buffer history (`[b`/`]b`) while a stream is mid-flight kills the worker and permanently freezes the pager at "scanning…"/"computing…" | correctness | medium | open |
| `src/app/pane_scroll.rs:179` | Single `runtime.pager_stream` slot: starting a grep/git-view while a transcript is loading silently kills the transcript stream, leaving the scroll pager empty forever | correctness | medium | open |
| `src/app/pane_scroll.rs:239` | mount_scroll_pager duplicates mount_stream_pager's LowerPane arm flag-for-flag | maintainability | medium | open |
| `src/app/pane_scroll.rs:304` | `r` reload branch in handle_pane_scroll_key checks the wrong pager slot and is dead for its stated purpose | maintainability | medium | open |
| `src/app/prompt.rs:590` | J jump prompt silently swallows errors — typo'd path gives zero feedback | correctness | medium | open |
| `src/app/run.rs:54` | Config-file watch on $HOME is permanently destroyed when the listing watch passes through the same directory | correctness | medium | open |
| `src/app/sources.rs:293` | Watcher-driven `refresh_listing` does a synchronous 50k-entry disk walk + allocation-heavy sort on the event-loop thread | perf | medium | open |
| `src/app/state/dispatch.rs:45` | :limit command and limit-prompt are drifted near-duplicates | maintainability | medium | open |
| `src/app/state/listing.rs:111` | Git status lookups keyed by display name never match executable files | correctness | medium | open |
| `src/app/state/mod.rs:84` | Update doc comment describes an abandoned migration stage as pending; three transitional result enums linger | maintainability | medium | open |
| `src/app/state/selection.rs:77` | Action::Take copies file contents to disk inline inside the pure apply path | maintainability | medium | ✅ |
| `src/pane/mod.rs:518` | max_scrollback() hardcodes 10_000; scroll_offset is never synced to vt100's clamp, creating a dead zone after scroll_to_top | maintainability | medium | open |
| `src/pane/tabs.rs:20` | Claude-specific session-restore state machine (PendingResumeSend) lives in the generic pane layer | maintainability | medium | open |

## Verify — unverified, confirm/refute first (45)

| Where | Finding | Lens | Sev | Status |
|---|---|---|---|---|
| `src/agent/resume.rs:24` | Claude resume stripper misses -r/--continue/-c and eats the flag following a bare --resume | correctness | medium | unverified |
| `src/agent/resume.rs:298` | gemini_resume_index_for runs `gemini --list-sessions` synchronously with no timeout on the session-restore path | correctness | medium | unverified |
| `src/app/render/chrome.rs:320` | build_rows is O(rows × delete-preview paths) — quadratic on 'delete picks' in a big directory | perf | medium | unverified |
| `src/app/state/apply.rs:320` | format_long_listing and file_type_label do per-file IO inside the pure apply dispatcher | maintainability | medium | unverified |
| `src/clipboard.rs:111` | Clipboard helper inherits stdout/stderr (garbles the raw-mode TUI) and leaks a zombie when stdin write fails | correctness | medium | unverified |
| `src/config/mod.rs:365` | `[colors] delete_warning` is parsed but never merged — user override silently ignored | correctness | medium | unverified |
| `src/fs/finder.rs:136` | find_nested_git_repos re-walks the entire subtree raw (no gitignore, no cap, no cancellation) on every F open / :grep in a git root | perf | medium | unverified |
| `src/fs/grep.rs:288` | find_nested_git_repos is a verbatim copy of finder.rs, including the 10-entry SKIP list | maintainability | medium | unverified |
| `src/fs/grep.rs:353` | search_to_vec blocks on the full repo walk even after the result limit is reached | correctness | medium | unverified |
| `src/fs/listing.rs:137` | Listing::sort comparator allocates 2-4 Strings per comparison — ~1.5M+ allocations per sort of a 50k-entry directory, on the event loop | perf | medium | unverified |
| `src/fs/long_listing.rs:155` | format_long_listing does an unmemoized getpwuid/getgrgid NSS lookup per row — L on a large listing can stall seconds-to-minutes on LDAP-backed machines | perf | medium | unverified |
| `src/fs/ops.rs:52` | read_truncated caps lines but not bytes — a huge single-line file is loaded entirely into RAM on the UI thread | correctness | medium | unverified |
| `src/git/blame.rs:49` | BlameModel has no size cap and clones 3 metadata Strings per line | perf | medium | unverified |
| `src/git/diff_model/build.rs:89` | gd diff silently drops the deletion side of an unstaged rename | correctness | medium | unverified |
| `src/git/diff_model/build.rs:559` | Rename similarity recomputed with a second full-blob diff that gix already performed | perf | medium | unverified |
| `src/git/status.rs:217` | gix status-walk setup and item-decode skeleton duplicated between repo_status and collect_worktree_plan | maintainability | medium | unverified |
| `src/git/status.rs:246` | One failed status item silently blanks git status for the whole repo | correctness | medium | unverified |
| `src/git/status.rs:314` | repo_status diverges from porcelain on unstaged renames (parity contract violation) | correctness | medium | unverified |
| `src/git/worktree.rs:170` | worktree::add leaves partial on-disk state on failure, and the leftover state blocks retry | maintainability | medium | unverified |
| `src/main.rs:87` | Panic hook doesn't pop kitty keyboard-enhancement flags or alternate-scroll mode — terminal left misbehaving after a panic | correctness | medium | unverified |
| `src/mcp/protocol.rs:62` | Nine doc comments detached from their functions by the verbatim mcp.rs split | maintainability | medium | unverified |
| `src/mcp/protocol.rs:324` | get_file_content is cwd-scoped while the search tools are project_home-scoped, so spyc's own search results can't be read back | correctness | medium | unverified |
| `src/mcp/server.rs:84` | Stale .spyc-context-<pid>.json markers permanently shadow discovery (read-only fallback) and can attach cross-project via PID reuse | correctness | medium | unverified |
| `src/mcp/server.rs:259` | One slow tool call (>20s) kills the entire MCP connection, and the server's searches are unbounded | correctness | medium | unverified |
| `src/pane/mod.rs:472` | scroll_offset desyncs from vt100's clamped scrollback, making scroll-down appear dead after 'g' or over-scrolling up | correctness | medium | unverified |
| `src/pane/mod.rs:607` | vt100 panic recovery rebuilds the parser at the adopt-time size, and the resize coalescer guarantees it never gets corrected | security | medium | unverified |
| `src/pane/pathref.rs:130` | Handrolled strip_ansi duplicates the strip_ansi_escapes crate that is already a dependency | maintainability | medium | unverified |
| `src/pane/pty_host.rs:208` | Unbounded reader->parser channel with a reader that never stops reading: no backpressure, unbounded memory under a firehose child | perf | medium | unverified |
| `src/pane/widget.rs:37` | Parser mutex held across the whole pane draw — per-frame O(cells) set_string under the lock contends with the parser worker | perf | medium | unverified |
| `src/paths.rs:17` | expand() reads HOME via std::env directly, bypassing the envset overlay it uses for every other variable | maintainability | medium | unverified |
| `src/state/inventory.rs:85` | Re-yanking a modified file silently keeps the stale cached content | correctness | medium | unverified |
| `src/state/sessions/mod.rs:127` | load_sessions dedup collapses distinct resumable sessions that share cwd + commands | correctness | medium | unverified |
| `src/state/sessions/mod.rs:579` | find_claude_session_name reads the entire conversation JSONL (100+ MB) into memory | correctness | medium | unverified |
| `src/sysinfo.rs:42` | rss_kb() still shells out to `ps` on macOS while the same file already reads RSS in-process via the sysinfo crate | maintainability | medium | unverified |
| `src/ui/blame_render.rs:44` | render_blame joins and syntect-highlights the whole file on the main thread with no size cap | perf | medium | unverified |
| `src/ui/diff_render/mod.rs:149` | Diff render syntect-highlights both full sides on the main thread, and re-highlights from scratch on every layout toggle | perf | medium | unverified |
| `src/ui/markdown/mod.rs:136` | Regex compiled on every markdown render call | maintainability | medium | unverified |
| `src/ui/mod.rs:13` | Blanket #[allow(dead_code, ...)] on line_edit hides real dead code | maintainability | medium | unverified |
| `src/ui/pager/construct.rs:58` | new_plain duplicates the entire 35-field PagerView initializer from new_styled | maintainability | medium | unverified |
| `src/ui/pager/construct.rs:182` | Pager yank/save methods do inline OS side effects, bypassing the existing Effect::CopyToClipboard path | maintainability | medium | unverified |
| `src/ui/pager/layout.rs:131` | visual_rows underestimates actual wrapped rows (wide-char greedy waste + whitespace markers), so scroll_max clamps before the real bottom | correctness | medium | unverified |
| `src/ui/pager/mod.rs:141` | scroll: u16 silently caps every pager at 65,535 lines — reachable content beyond that is unviewable | maintainability | medium | unverified |
| `src/ui/pager/render.rs:125` | With wrap on, scroll is logical-line granular — visual rows of a long line beyond the first viewport_h are unreachable | perf | medium | unverified |
| `src/ui/pager/render.rs:180` | wrap_line materializes the full wrapped expansion of each visible logical line every frame — allocation churn proportional to longest line, not viewport | perf | medium | unverified |
| `src/ui/pager/selection.rs:281` | Visual/placement auto-scroll assumes 1 logical line = 1 screen row; with wrap on the cursor moves off-screen without scrolling | correctness | medium | unverified |

## Verification notes

- **2026-06-21 — `pager_handler/motion.rs:311` (NOT closed; downgraded from "likely
  fixed").** Traced current code: the `v` arm now preserves `mount` + `pane_scroll`
  across the editor round-trip (`motion.rs:264-269`, via #467/#473) and Esc/q grew a
  `pane_scroll → close_pane_scroll_pager()` branch (`motion.rs:33`). **But** the restore
  still does `view.pane_scroll = pane_scroll; self.set_pager(view)` (`effect.rs:549/589`),
  and `set_pager` installs into the **top** slot (`view.pager`), not `view.scroll_pager`
  — the second half of the finding's recommended fix (route a `pane_scroll` view back into
  the scrollback slot) was never implemented, and `clear_pager` (`mod.rs:323`) doesn't
  touch `scroll_pager`. So a `v` from the bottom scrollback can still leave the original in
  `scroll_pager` while the edited copy lands in `view.pager`. Whether that reproduces the
  orphan/double-pager symptom needs a **live-pty repro** (this finding is in the
  harness-dependent set). Kept open.

## Resolved / closed (running log)

_(append as PRs land — `✅ #NNN <where> — <one-line reason>`)_
✅ `src/app/state/apply.rs:113` — moved to off-thread Effect::FileOp/Effect::Inventory
✅ `src/app/state/selection.rs:77` — moved to off-thread Effect::Inventory
✅ `src/app/clipboard.rs:189` — moved to off-thread FileOp::PipeContent
✅ `src/app/clipboard.rs:250` — moved file operations to off-thread workers

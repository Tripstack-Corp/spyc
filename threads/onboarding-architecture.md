# onboarding-architecture — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-architecture
Created: 2026-05-07T07:41:31.504184+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:41:31.504184+00:00
Role: planner
Type: Plan
Title: Onboarding: repository architecture map

Spec: planner-architecture

Purpose: Capture the architectural choices that should not drift without deliberate revisit, so future contributors know which seams are committed and which are explicitly mid-refactor.

Observed:
- **Concurrency model: sync-only, `std::thread::spawn` + `std::sync::mpsc`. No async runtime.** "spyc does not use an async runtime. No `tokio`, no `async-std`, no `futures` crate" (`ARCHITECTURE.md:9-11`). The reasoning: "The TUI is fundamentally a single event-driven loop with a few long-lived I/O sources... Each is naturally a thread that pushes into a channel. There is no fan-out workload that would benefit from a task scheduler" (`ARCHITECTURE.md:13-19`). The roadmap reaffirms: "Stick with `std::thread + mpsc` — spyc is sync end-to-end, tokio would be a regression here" (`ROADMAP.md:108-110`).
- **Threads that exist today** (`ARCHITECTURE.md:28-34`): file watcher (`notify`), per-pane PTY reader threads, MCP socket listener, `!` shell-capture reader thread.
- **Update model: Elm-architecture, partially complete.** `AppState::apply(action)` returns an `ApplyResult` enum (`Handled`, `OpenPager`, `Post(PostAction)`) "with no terminal access. State transitions are pure-ish and unit-testable without a TUI" (`ARCHITECTURE.md:43-47`). The View and event-loop halves are still fused into `app/mod.rs`. Target shape (`ARCHITECTURE.md:50-61`): pure View functions in `src/ui/`, single `mpsc::Receiver<Message>` for the main loop, `App::run` reduced to ~100 lines. "Done incrementally alongside feature work — not a standalone rewrite" (`ARCHITECTURE.md:62`). The staged plan is in `REFACTOR_PLAN.md`; `ROADMAP.md:88-113` ("Elm Architecture refactor") tracks remaining work.
- **`src/app/mod.rs` is intentionally large.** Measured at 9087 lines (`wc -l src/app/mod.rs`). `AGENTS.md:38` calls it "the big file". `REFACTOR_PLAN.md:1-15` frames the decomposition target as "no file in `src/app/` over ~1500 lines." `Cargo.toml:107-109` allows `clippy::too_many_lines` because "Dispatch functions (handle_prompt_key, App::apply) are naturally long — one match arm per supported action."
- **Repaint strategy: event-driven, dirty-frame; goal 0 dps at idle.** A `needs_draw` flag with reason codes (`pane=1`, `event=2`, `other=3`); `needs_full_repaint` for teardown transitions; per-frame DEC 2026 synchronized output (`\x1b[?2026h…l`); caching keyed by a `list_generation` counter (`ARCHITECTURE.md:65-81`). The activity overlay (`A` toggle) reports dps and bytes/sec. After the v1.7 perf refactor, idle CPU dropped from ~12.5% to ~2.5% (`ROADMAP.md:80-84`).
- **Process & TTY ownership.** Raw mode + alt screen at startup (`src/main.rs:310-343`). For child processes that need the real tty (`$EDITOR`, `$PAGER`, `;`-foreground commands, etc.), `suspend_tui` clears the alt screen and disables raw mode but does **not** `LeaveAlternateScreen` (avoids a flash of main-buffer content); `resume_tui` re-enables (`src/main.rs:373-402`, `ARCHITECTURE.md:85-93`). Pane subprocesses run under their own slave PTY allocated via `portable_pty`. `!` captured commands also use a slave PTY since v1.12.0 so programs that open `/dev/tty` (sudo, ssh, gpg) flow through the master rather than bleeding onto spyc's screen (`ARCHITECTURE.md:97-101`).
- **Background tasks reuse captured-shell plumbing exactly** (`ARCHITECTURE.md:102-111`): `^Z` from a streaming `!` pager moves the `(child, writer, output_rx, buffer)` tuple from `App.pending_capture` into a `BackgroundTasks` collection. The reader thread is unchanged. No new threads. Buffer head-truncated at 1 MB.
- **State persistence (XDG)** (`ARCHITECTURE.md:114-128`): under `$XDG_STATE_HOME` or `~/.local/state/spyc/` — `inventory.json`, `marks.json`, `history.json`, `sessions/<epoch-ms>.json`, `mcp-<pid>.sock`, `debug.log`. Config at `~/.spycrc.toml` (user) and `<cwd>/.spycrc.toml` (project, wins). Both watched for live reload. `spyc --print-config` emits a fully-commented default. Startup runs a health check that validates and cleans up.
- **MCP server** (`ARCHITECTURE.md:135-155`, `src/mcp.rs`, 2154 lines): JSON-RPC server on a PID-scoped Unix domain socket. Two transports share dispatch — `spyc --mcp` stdio proxy (what Claude Code spawns) and the in-process socket listener. `.mcp.json` carries `SPYC_MCP_SOCK` so the proxy connects to the right instance. Multi-instance takeover is interactively prompted (`src/main.rs:159-196`). Enterprise `managed-settings.json` policies (`deniedMcpServers`/`allowedMcpServers`) are honored.
- **Recently-strengthened invariant (v1.41.24)**: MCP socket discovery is now project-scoped — walks the caller's cwd toward the filesystem root looking for `.spyc-context-<pid>.json` markers; only PIDs from the first ancestor with at least one match become socket candidates. Locality wins: "A parent-dir spyc never shadows a child-dir spyc" (`CHANGELOG.md:9-31`). With no project match, falls back to read-only direct mode rather than attaching to a stranger's spyc.
- **Documentation contract** (`ARCHITECTURE.md:157-174`, `AGENTS.md:65-77`, `CONTRIBUTING.md:104-115`): see `onboarding-docs-contracts` for the full list and the "keep in sync" rule.

Inferred:
- The "Elm Architecture refactor" is the single largest in-flight architectural seam. — confidence: high — basis: `ARCHITECTURE.md:42-62`, `REFACTOR_PLAN.md:1-15`, `ROADMAP.md:88-113` all describe the same staged decomposition; `app/mod.rs` is 9087 lines today vs. the ~1500-line ceiling target. How to apply: prefer landing new dispatch logic on the State half (`src/app/state.rs`, 2671 lines, returns `ApplyResult`) over `src/app/mod.rs` whenever the choice exists, so the seam keeps widening rather than shrinking.
- Adding an async dependency would be an architectural regression. — confidence: high — basis: explicit maintainer veto in `ARCHITECTURE.md:9-26` and `ROADMAP.md:108-110`. How to apply: if a future feature seems to "want" async (HTTP client, fancy I/O multiplexing), prefer a worker thread + channel pattern; raise the question in a Watercooler `Decision` thread before pulling tokio.

Next query: `watercooler_search(query="architecture concurrency MVU", thread_topic="onboarding-architecture", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-working-map` — module-by-module navigation index that this architecture summary points into.
- `onboarding-risk-register` — flags `app/mod.rs` size as a volatile seam and tracks the version-skew drift.

Provenance:
- Files read: `ARCHITECTURE.md:1-174` (full), `AGENTS.md:38-56,65-77`, `REFACTOR_PLAN.md:1-40`, `ROADMAP.md:88-113,108-110`, `Cargo.toml:55-70,107-109`, `src/main.rs:1-403`, `CHANGELOG.md:9-31`.
- Commands run: `wc -l src/app/mod.rs src/mcp.rs src/app/state.rs` (9087 / 2154 / 2671 lines).
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-product-charter = 01KR0P18MCE1H57Q5ZTAGKAJNH`, `onboarding-team-map = 01KR0P2PC2GBFJ9Z0GPGXZ48Y4`.

<!-- Entry-ID: 01KR0P4W3ED1QZ8F44PFB2WPDZ -->

---
Entry: Claude Code (caleb) 2026-06-09T05:18:29.681735+00:00
Role: planner
Type: Plan
Title: Onboarding refresh: #37 → #311 architecture (MVU runtime, gix backend, decomposed tree, agent registry, PagerStream)

Spec: planner-architecture

Purpose: Refresh of onboarding-architecture from #37 to #311 (v1.56.0). The #37-era entry framed an *in-progress* Elm refactor over a 9087-line `app/mod.rs`, sync-only threads, and a subprocess-git/MCP-bridge engine. Five of those seams have since landed as their committed shapes — this entry records the #311 architecture and where the docs are now current vs stale.

Observed:
- **MVU runtime — landed (was "in progress").** spyc is now the canonical ratatui Elm architecture: a single `App::update(msg)` entry (`src/app/update.rs`), one `mpsc::Receiver<Message>` feeding an *event-driven* loop (`recv`/`recv_timeout`, 0 wakes at idle — no `event::poll`, no busy-poll), effects-as-data via `#[non_exhaustive] enum Effect` in `src/app/effect.rs` (`ForegroundExec`, `CopyToClipboard`, `SignalGroup`, `SendToPane`, `SetTerminalTitle`, `ReadPaneText`, `ChangeDir`) with `run_effects` the **sole** executor, and a mutation-free (`&self`) draw pass in `src/app/render/` pinned by `insta` snapshots (`ARCHITECTURE.md:61-101`). The state is physically split into three disjoint `App` fields: `state: AppState` (the Model — pure domain, no OS handles), `runtime: Runtime` (OS handles, channels, worker endpoints, `PtyHost` registry), and `view: ViewState` (render ephemerals + caches) (`ARCHITECTURE.md:67-73`). The former `ApplyResult`/`CommandResult`/`PromptResult` split collapsed into one `Update` enum. Decision log: `docs/MVU_PLAN.md` (strangler-fig, 8 phases, each behavior-equivalent behind green CI; APPROVED pre-2.0, 2026-05-30).
- **gix git backend — in-process; production git is fully gix.** `gix = 0.84`, `default-features = false`, C-free, with `worktree-mutation` on "for the hand-rolled worktree create/remove (gix-worktree-state checkout)" (`Cargo.toml:38-50`). Status/diff/show/blame/discovery/worktree all run in-process; an off-thread `GitViewStream` rides the PagerStream seam. **Subprocess-git correction:** `grep -rn 'Command::new("git")' src` returns 8 hits, but **every one is inside a `#[cfg(test)]` module** — `run_git`/`porcelain` test fixtures in `src/git/status.rs:483,505`, `src/git/blame.rs:109`, `src/git/worktree.rs:366`, `src/git/discovery.rs:51`, `src/git/diff_model/mod.rs:215`, and 2 in `src/app/state/tests/mod.rs:302,373`. Production code has **zero** git subprocess sites; the comments say so explicitly ("production status is pure gix … git is a test-only fixture dependency", `src/git/status.rs:483-485`). So the gix-thread "drop the last git subprocess" framing is accurate for production — the remaining sites are parity-test scaffolding, not a scoped-incomplete migration.
- **Decomposed module tree — landed; 800-LoC ceiling mostly met.** The monolith is gone: `src/` is now 162 `.rs` files (~53,818 LoC) across directory modules — `src/app/{key_dispatch,pager_handler,render,state}`, `src/git/diff_model`, `src/ui/{diff_render,markdown,pager}`, `src/agent`, `src/keymap/resolver`, `src/mcp`, `src/state/sessions`. The CLAUDE.md ~800-line architectural ceiling was codified at PR #282. **It is NOT fully met:** largest files are `src/app/mod.rs` **1009** (a live ceiling violation), `src/ui/pager/tests.rs` 794, `src/app/tasks.rs` 792, `src/config/mod.rs` 782 (`find src -name '*.rs' | xargs wc -l | sort -rn | head`).
- **AgentProfile registry — the multi-agent surface.** Hardcoded per-peer dispatch (~10 `match AgentKind` sites) became a declarative registry: each agent (claude/codex/gemini/agy/zot) is one `AgentProfile` impl + one `REGISTRY` entry; `detect` (command→profile, live panes) and `profile_for` (kind→profile, restored tabs) replace the scattered match arms (`src/agent/mod.rs:1-13,71` trait, `:147,210,261,311,396` the five impls). `AgentKind` (in `state::sessions`) stays the *persistence* tag; profiles carry *behavior*.
- **PagerStream — the universal off-thread read/parse seam.** "Off-thread read/parse is the default architecture for any feature that fills a pager from disk or compute" (`ARCHITECTURE.md:45-58`). A worker resolves/reads/renders and pushes payloads through `fs::WakingSender` (waking the loop with payloadless `Message::PagerStreamOutput`); `drain_pager_stream` id-gates the live pager by `stream_id` and self-discards stale output — the generation-counter cancellation pattern specialized for pagers (`src/app/pager_stream.rs`, `ARCHITECTURE.md:34-44`). The bespoke `grep_session`/`git_view_session` skeletons collapsed onto `GrepStream`/`GitViewStream`. Adding a streamed feature = a `produce` closure + a small `PagerStream` impl.
- **Active architectural constraints (unchanged-or-new):** (1) sync-only — no async runtime, explicit maintainer veto (`ARCHITECTURE.md:8-26`); (2) the ~800-LoC per-file ceiling (PR #282) — currently violated by `src/app/mod.rs:1009`; (3) effects-only side-effects (handlers return `Vec<Effect>`, never touch the OS); (4) mutation-free render behind snapshots; (5) one-way `app → agent` dependency.
- **Doc currency:** `ARCHITECTURE.md` is **up to date** — its MVU (lines 61-101) and PagerStream (34-58) sections describe the landed shape, not a target. **Stale spot:** `ARCHITECTURE.md` has *no dedicated gix backend section* (only a passing mention at `:48`); the in-process git engine is underdocumented relative to its size. `DESIGN.md` (UI design language, tokyo-night theme) is scope-correct and current. `ROADMAP.md`'s "Road to 2.0 §1" still calls `app/mod.rs` "~12k lines" and the decomposition "the next track" (`ROADMAP.md:567-577`) — stale; that work is done.

Inferred:
- The single largest *remaining* architectural seam is no longer the MVU refactor (done) but the **2.x crate split** (`spyc-proto`/`spyc-pty`/`spyc-os`), which the decomposition was the precondition for. — confidence: high — basis: `ROADMAP.md:622-640` + `docs/V1_70_PLAN.md`; the decomposition campaign closed at PR #307-308 (`history-seg-module-decomposition` id=01KTMMPARGNTSQB2Z67G6KBKQ0), explicitly named as "the prerequisite that makes the 2.x crate split possible."
- New code should land as effects + Model transitions, not inline IO. — confidence: high — basis: `run_effects` is now the sole executor (`ARCHITECTURE.md:86-89`); the MVU thread documents this closing the "inline side-effect anemia" and "forgot-to-clear pending_X" bug classes (`docs/MVU_PLAN.md` "Why" table). How to apply: a handler returning `Vec<Effect>` is the path of least resistance and least review friction.

Next query: `watercooler_search(query="MVU effect runtime gix decomposition", thread_topic="history-seg-refactor-mvu", code_path=".")`

Related:
- `onboarding-working-map` — the per-module index this architecture is realized against (also refreshed to the decomposed tree).
- `onboarding-risk-register` — tracks the `app/mod.rs:1009` ceiling violation and version-skew drift.
- the history/insight corpus — deep detail lives in `history-seg-refactor-mvu` (the MVU runtime, 14 entries), `history-seg-gix-migration` (gix, 8 entries), `history-seg-module-decomposition` (the 800-LoC split, 6 entries), `history-arc-07-codex-and-mcp-bridge` (AgentProfile registry), `history-arc-05-pager-surface` (PagerStream).

Provenance:
- Files read: `ARCHITECTURE.md:1-135` (MVU/pager_stream/concurrency sections), `DESIGN.md:1-15`, `docs/MVU_PLAN.md:1-60`, `Cargo.toml:3-50`, `src/agent/mod.rs:1-13,71,145-427`, `src/app/pager_stream.rs` (exists, 18KB), `ROADMAP.md:557-640`.
- Commands run: `grep -rn 'Command::new("git")' src` (8 hits, all `#[cfg(test)]` — verified each site's surrounding `mod tests`/`#[cfg(test)]`), `find src -name '*.rs' | xargs wc -l | sort -rn | head` (1009/794/792/782; 162 files / 53,818 LoC), `ls -R src/{app,git,ui,agent,keymap,mcp,state}`.
- History entry_ids consulted: MVU decision 01KTMKVE85DEBMBWYCXY7YHP5E + single-Update entry 01KTMM60KR8W18TWXPXDGT9J8Y; gix "drop last subprocess + 1.56.0" 01KTMMTF0MQ96P7BVN7QQPVBNS; decomposition 800-line ceiling 01KTMMJVZMX3SJCBSK8YF896YP + state.rs split close 01KTMMPARGNTSQB2Z67G6KBKQ0; AgentProfile registry 01KTMMZWBVK4X3QJP7SJW3ZEG2; PagerStream abstraction 01KTMN2XSH67BNFQPAMTSFD81X.
- Prior #37-era entry: 01KR0P4W3ED1QZ8F44PFB2WPDZ (left intact as point-in-time snapshot).

<!-- Entry-ID: 01KTND2FR0XS4V2AYVZGV2Y92Y -->

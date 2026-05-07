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

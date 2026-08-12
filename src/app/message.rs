//! The loop's message vocabulary: [`Message`], the single stream `App::run`
//! consumes, and [`Wake`], the kinds of "something finished off-thread".
//!
//! Extracted from `mod.rs` (the anti-monolith ceiling) as one cohesive unit of
//! type definitions. Nothing here has behavior — the coalesce that routes these
//! lives in `sources.rs` and the dispatch in `run.rs`.

use crossterm::event::Event;

use super::pane_wake::SinkId;
use super::scheduler::Deadline;
use super::state;

/// Unified message stream consumed by `App::run` (MVU Phase 1,
/// `docs/MVU_PLAN.md`). As of Phase 3d the loop is **fully event-driven** —
/// every source wakes this one channel and `run()` blocks on `recv()` with
/// no poll floor: the parkable crossterm reader feeds `Input` (+ `ReaderExited`
/// on death); the notify watcher closure feeds `FsEvent`; the git forwarder
/// feeds `GitResult` (3a); pane parser workers feed `PaneOutput` (3b); capture/
/// task reader threads feed `SinkOutput` (3c); the MCP forwarder feeds `Mcp`;
/// the finder feeds `FindOutput`; and pager-stream workers (grep / git-view /
/// transcript) feed `PagerStreamOutput` (3d). The only
/// remaining timed wakes are armed `Tick` deadlines (git poll, activity
/// rollover, capture-timer, …) — and they only SHORTEN the wait; nothing armed
/// means an unbounded block until a real message.
pub enum Message {
    /// A crossterm input event. The reader Press-filters `Key` events
    /// (only `Press`/`Repeat` are forwarded); `Paste`/`Resize`/`Focus`/
    /// `Mouse` pass through unchanged.
    Input(Event),
    /// MVU Phase 3a: a filesystem change from the notify watcher closure.
    /// Carries a bare `notify::Event` — the closure drops `Err` at the
    /// boundary, preserving the prior Ok-only drain contract. The recv arm
    /// only *buffers* it into `fs_pending`; the unchanged pre-recv drain
    /// stamps the debounce against `now_pre` (see `ingest_fs_event`).
    FsEvent(notify::Event),
    /// MVU Phase 3a: a git-worker result, routed via the forwarder thread
    /// onto the unified channel. The recv arm only *buffers* it into
    /// `git_pending`; the unchanged pre-recv drain applies it
    /// (generation-gated) via `ingest_git_result`.
    GitResult(state::GitWorkerResult),
    /// MVU Phase 3d: a writable MCP request forwarded from the socket server.
    /// Unlike the wake variants, this carries a payload (the command + its
    /// one-shot reply Sender). Buffered into `mcp_pending` by the recv
    /// pre-step + the coalesce drain; executed + replied at the pre-recv MCP
    /// drain (`execute_mcp_command` writes the context file synchronously,
    /// then `reply.send` — preserving single-connection read-after-write).
    /// MUST NOT be dropped in coalesce (the reply Sender would strand the
    /// client). Never surfaced as `Input`.
    Mcp(crate::mcp_cmd::McpRequest),
    /// MVU Phase 2: a timer/deadline elapsed. Derived by the loop's own
    /// `Scheduler`, NOT a thread. The loop never actually sends itself a
    /// `Tick` — `recv_timeout` returning `Err(Timeout)` IS the tick handler
    /// (it re-evaluates every timer predicate against the fresh `now`). The
    /// variant exists so later subscriptions can push real `Tick`s onto the
    /// single channel without re-touching the enum.
    #[allow(dead_code)]
    Tick(Deadline),
    /// Something off-thread finished, or a source has more to give. Carries no
    /// payload the loop needs — the outcome rides a `runtime.*` slot (or the
    /// producer's own channel) and the pre-recv drains pick it up — so the
    /// coalesce ALWAYS collapses one of these to a synthesized `Timeout`, in a
    /// single arm that covers every [`Wake`] kind.
    ///
    /// That single arm is the point. These used to be seventeen bare `Message`
    /// variants, and three separate matches each had to list all of them: the
    /// burst drain, the recv pre-step, and the dispatch's unreachable arm. Two
    /// were exhaustive and one had a catch-all, so a variant added to the first
    /// two and missed in the third fell through and panicked the loop — which
    /// shipped four times (#514's file/inventory ops, the Lua worker, then the
    /// clipboard pair in #402, where every text yank crashed spyc outright).
    /// Grouping them here means a new wake cannot be misrouted: there is one
    /// place to add it, and no list to forget.
    Wake(Wake),
}

/// The kinds of [`Message::Wake`] — "something happened off-thread; re-scan".
///
/// Every one is payloadless as far as the loop is concerned (the two that carry
/// a `SinkId` carry it as a trace label only, never to target the drain), which
/// is why one match arm can handle them all. **Adding a variant here needs no
/// change to the event loop**: `coalesce_pending`, `coalesce_recv` and
/// `dispatch_effective` each match `Message::Wake(..)` once, so a new kind is
/// collapsed correctly the moment it exists.
#[derive(Debug)]
pub enum Wake {
    /// MVU Phase 3b: a pane PTY output WAKEUP — never carries bytes. A
    /// lost-wakeup-safe edge from a parser worker's 0→1 `wake_pending` CAS
    /// (the worker bumps `parser_gen` first). The loop treats it purely as
    /// "wake and re-scan": it re-enters the pre-recv pane scan, which clears
    /// each `wake_pending` and re-reads `parser_gen` via `drain_output`. The
    /// `tab` labels which pane woke us (carried for 3c/Phase-5; in 3b the
    /// scan re-drains all panes, so a stale id self-discards). Buffered +
    /// collapsed in the coalesce pre-step, NEVER surfaced as `Input`.
    Pane {
        /// A trace label, read through the `{w:?}` wake log in `coalesce_recv`.
        /// The dead-code lint doesn't credit a derived `Debug`, hence the allow —
        /// this is deliberately never used to *target* a drain (they re-scan).
        #[allow(dead_code)]
        tab: SinkId,
    },
    /// MVU Phase 3c: a foreground `!` capture or a background task produced
    /// output or hit EOF — the same lost-wakeup-safe edge as `PaneOutput`,
    /// fired by the shared `PtyHost` reader thread (captures/tasks have no
    /// parser worker; the main loop drains them). Carries no bytes and no
    /// exit status: the woken pre-recv drain re-scans the capture + all
    /// running tasks and observes `newly_closed`, harvesting exit inline
    /// (the reader can't call `child.wait()` — `portable_pty` needs
    /// `&mut self`). `sink` is a trace label only (the drain re-scans all,
    /// so a stale id after a `:fg`/`^Z`/demote/promote self-discards).
    /// Buffered/collapsed; never surfaced as `Input`.
    Sink {
        /// A trace label, same contract as [`Wake::Pane`]'s `tab`.
        #[allow(dead_code)]
        sink: SinkId,
    },
    /// MVU Phase 3d: the F-finder walker produced a candidate batch or
    /// completed. Payloadless wake — the candidates ride `FindPicker.walk_rx`,
    /// re-drained by `drain_walk` (a wake after the picker closed no-ops at
    /// the `if let Some(picker)` guard). Collapsed in the coalesce pre-step;
    /// never surfaced as `Input`.
    Find,
    /// A pager-stream worker (the unified `pager_stream` abstraction — grep /
    /// git-view / transcript collapse onto it) produced a batch / its one-shot
    /// model. Payloadless wake — the payload rides the boxed stream's `rx`,
    /// re-drained by `drain_pager_stream` (id-gated against the live pager's
    /// `stream_id`, so a wake for a replaced/closed stream self-discards).
    /// Collapsed in the coalesce pre-step; never surfaced as `Input`.
    PagerStream,
    /// MVU Phase 3d: the input reader thread exited (fatal read/poll error or
    /// clean stop). Payloadless death-wake, sent AFTER `reader_done.store`
    /// (store-then-send → the loop-top Acquire-load sees the error). With the
    /// poll floor gone, this is what kicks a blocking `recv()`; the loop-top
    /// `reader_done` check then exits. Collapses to a Timeout like the other
    /// wakes; never surfaced as `Input`.
    ReaderExited,
    /// MVU Phase 6: an off-thread `active_agent_status` resolve landed a result
    /// in `agent_status_pending`. Like the git/MCP forwarders, the worker must
    /// WAKE the loop — the event-driven loop blocks on a bare `recv()` at idle,
    /// so a landed result would otherwise sit unread (and unrendered) until an
    /// unrelated event. Payloadless: collapses to a Timeout like the other
    /// re-scan wakes (drop-safe in coalesce). The redraw + apply both happen
    /// in the pre-recv scan: `apply_landed_agent_status` drains the slot into
    /// the cache and `kick_agent_status_refresh` re-arms — NOT in the draw
    /// (`active_agent_status` is a pure `&self` cache read since #346). Driven
    /// by the scan, not by this message surviving coalesce.
    AgentStatus,
    /// Tier 5: an off-thread graveyard op (archive / restore / purge-all,
    /// `Effect::Graveyard`) finished and pushed its outcome onto
    /// `runtime.graveyard_results`. Payloadless wake — the outcome rides the
    /// slot, drained unconditionally by `apply_graveyard_outcomes` in the
    /// pre-recv scan. Collapses to a Timeout like the other re-scan wakes
    /// (drop-safe in coalesce); the redraw is driven by the drain, not by this
    /// message surviving. Same shape as `AgentStatusReady`.
    Graveyard,
    /// An off-thread image render (`Effect::RenderMermaid` / `Effect::OpenImage`)
    /// finished and pushed its outcome onto `runtime.image_results`. Payloadless
    /// wake — `apply_image_outcomes` drains the slot in the pre-recv scan. Same
    /// shape as `GraveyardDone`. One message for every producer: what the render
    /// *was* rides `ImageOrigin` on the outcome, not the wake.
    Image,
    /// An off-thread archive op (`Effect::Archive`) finished and pushed its
    /// outcome onto `runtime.archive_results` — a mount, a materialized member,
    /// or a staging cleanup. Payloadless wake of the same shape as
    /// `GraveyardDone`; `apply_archive_outcomes` drains the slot in the pre-recv
    /// scan.
    Archive,
    /// An off-thread file op (`Effect::FileOp`) finished and pushed its outcome.
    FileOp,
    /// A middle-click clipboard read (`Effect::PasteFromClipboard`) finished and
    /// pushed its result onto `runtime.clipboard_paste_results`. Payloadless
    /// wake of the same shape as `GraveyardDone`; `apply_clipboard_pastes`
    /// drains the slot in the pre-recv scan and feeds the text to `handle_paste`.
    ClipboardPaste,
    /// An off-thread clipboard *write* finished and pushed its outcome onto
    /// `runtime.clipboard_copy_results`. Payloadless, same shape as
    /// `ClipboardPasteDone`; `apply_clipboard_writes` drains the slot in the
    /// pre-recv scan and flashes anything that failed.
    ClipboardCopy,
    /// An off-thread inventory op (`Effect::Inventory`) finished.
    Inventory,
    /// An off-thread MCP worktree op (create/remove/clean) finished and pushed
    /// its outcome onto `runtime.worktree_results`. Payloadless wake —
    /// `apply_worktree_outcomes` drains it in the pre-recv scan, re-applies the
    /// listing/context update, then answers the MCP client. Same shape as
    /// `ImageDone`.
    WorktreeJob,
    /// A Lua script finished on the worker thread (`runtime.lua`). Payloadless
    /// wake — `handle_lua_done` drains the worker's outcome buffer in the
    /// pre-recv scan and translates the requests into effects/actions. Same
    /// shape as `WorktreeJobDone`, except the outcomes ride the worker's own
    /// buffer (`LuaWorker::drain_outcomes`), not a `runtime.*_results` slot.
    Lua,
    /// An off-thread vertical-split preview reload (`kick_preview_reload`)
    /// finished and pushed its outcome onto `runtime.preview_results`.
    /// Payloadless wake — `apply_preview_reloads` drains the slot in the
    /// pre-recv scan. Same shape as `ImageDone`.
    PreviewReload,
    /// Option B (`codex_pin`): an off-thread `~/.codex/sessions` scan landed a
    /// rollout snapshot in `codex_pin_pending`. Payloadless wake — the snapshot
    /// rides the slot, drained by `apply_codex_session_pins` in the pre-recv
    /// scan. Collapses to a Timeout like the other re-scan wakes.
    CodexSession,
}

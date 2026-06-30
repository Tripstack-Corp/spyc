//! MVU Phase 4: the `Effect` vocabulary and the run-loop executor.
//!
//! Phase 1 proved a single side-effect seam: `PostAction::Spawn` flowed
//! out of the handler chain to the event loop, which tore down the TUI,
//! ran the child, and restored. Phase 4 generalizes that one arm into an
//! `Effect` enum so `run_effects` becomes the **sole** side-effect
//! executor. This first slice introduces only `Effect::ForegroundExec`
//! (== the old `Spawn`) behind a `From<PostAction>` shim — behavior is
//! byte-identical; later slices add clipboard / signal / send / title
//! variants. The Model/Runtime split (and class-D subscription effects
//! like `SpawnPane`) stay in Phase 5.
//!
//! `Effect` and `run_effects` live here (not `mod.rs`) to keep the event
//! loop under the anti-monolith ceiling
//! (`app::guard_tests::mod_rs_stays_decomposed`). `run_effects` is an
//! `impl App` method in this child module, so it reaches App's private
//! state and helpers (`set_pager`, `build_pager_view_for_file`) via the
//! descendant-module rule — same pattern as `actions` / `key_dispatch`.

use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::KeyEvent;

use crate::Tui;
use crate::pane::Pane;
use crate::ui::pager::PagerView;

use super::{
    App, ForegroundExec, Message, PagerReturn, PostAction, file_ops, graveyard_ops, inventory_ops,
    mermaid_ops, worktree_ops,
};

/// A side effect for the run loop to execute. Producers (handlers) return
/// a `Vec<Effect>` describing *what* should happen; `run_effects` is the
/// only place that *does* it. The empty `Vec` is the no-op (there is no
/// `None` variant) — see `From<PostAction>`.
#[derive(Debug)]
pub enum Effect {
    /// Tear the TUI down, run a child in the foreground, restore. The
    /// only TUI-tearing effect; == the former `PostAction::Spawn`.
    ForegroundExec {
        program: String,
        args: Vec<String>,
        /// Whether to pause for a keypress after the child exits so the
        /// user can read its output before the TUI is restored.
        pause_after: bool,
    },

    /// A-class. Copy `text` to the system clipboard, then flash the
    /// per-site success message reconstructed from `ok` — or a uniform
    /// `"yank failed: {e}"` on error (every status-bar yank site shares
    /// that one failure string, so it is not carried). `text` is
    /// materialized in the producer (not a pane handle) — eager grid
    /// copy-out is the regression Phase 5's `PaneSnapshot` avoids.
    CopyToClipboard { text: String, ok: ClipMsg },

    /// A-class. Copy `text` to the system clipboard, then flash `ok_msg` (on
    /// success) or `"yank failed: {e}"` (on error) in the **active pager's**
    /// title bar — the pager yank (`y`/`Y`/visual), routed through the executor
    /// instead of an inline `clipboard::copy` in the motion/visual handlers.
    /// Distinct from [`Self::CopyToClipboard`] (which flashes the status bar):
    /// the pager confirms in its own title where the user is looking. The
    /// success message is composed in the producer (it knows the line count).
    CopyToPagerClipboard { text: String, ok_msg: String },

    /// A-class. Write `content` to a timestamped `spyc_output_*.txt` in the
    /// process cwd (the focused column), then flash the saved path or the error
    /// in the active pager's title — the pager `s` save, routed through the
    /// executor instead of an inline `std::fs::write` in the motion handler (the
    /// effects-as-data contract). `content` is materialized in the producer; the
    /// write is bounded (pager output, not a streaming source).
    SavePagerOutput { content: String },

    /// A-class. Send `sig` to the task's process group (`kill_pg` applies
    /// the negative-pid / group convention), then on success toggle the
    /// task's `paused` flag and flash per `on_ok`, or flash `on_err` on
    /// failure. `pid` is derived in the producer *after* its guards — the
    /// executor cannot re-derive it (the host may have moved). The flash
    /// always lands on the status bar (`state.flash_*`), never the pager
    /// footer — true for both `:pause`/`:resume` and the pager `S`/`C`
    /// keys. `#[cfg(unix)]` because `rustix::process::Signal` is unix-only
    /// (matches `kill_pg`).
    #[cfg(unix)]
    SignalGroup {
        pid: u32,
        sig: rustix::process::Signal,
        on_ok: SigOk,
        on_err: String,
    },

    /// A-class. `^z`-toggle a bottom pane: `SIGSTOP` (`resume == false`) stops
    /// the agent's process group, or `SIGCONT` (`resume == true`) wakes it.
    /// `pgrp` is the pty's foreground group (`tcgetpgrp`) — the agent itself,
    /// since a pane execs the agent (it's the direct child / session leader, no
    /// job-control wrapper shell to reclaim the tty). **`SIGSTOP`, not
    /// `SIGTSTP`, is deliberate**: Claude catches `SIGTSTP` and runs a
    /// self-suspend handler that on macOS ends in the false-exit
    /// (`[exited 146]`); `SIGSTOP` is uncatchable, so the agent just freezes and
    /// the reader keeps blocking (never EOF, see `tasks.rs`). The
    /// `TabInfo.suspended` flip + flash run in the producer (pure state); only
    /// the signal is the effect. `#[cfg(unix)]`, like `SignalGroup`.
    #[cfg(unix)]
    SignalPane { pgrp: u32, resume: bool },

    /// A-class. Deliver `input` (pre-encoded key or pre-built bytes) to a
    /// pane, then flash `on_ok` on success / `"{err_prefix}: {e}"` on
    /// failure — each `None` means "ignore that outcome silently" (the
    /// `send_key` forwards do, matching their former `let _ = …`). The
    /// target is resolved at executor time; safe because exactly one
    /// `SendToPane` is emitted per key and no converted site switches
    /// tabs in its own body (a `Tab`/`SinkId` target is Phase 5).
    SendToPane {
        target: PaneTarget,
        input: PaneInput,
        on_ok: Option<String>,
        err_prefix: Option<&'static str>,
    },

    /// A-class. Write `bytes` (a pre-encoded key or raw paste) to the running
    /// `!`-capture child's master writer, so prompts (sudo / ssh passwords) are
    /// answered through the sole executor instead of an inline write in the
    /// key/paste handlers — the same effects-as-data routing the pane sinks use
    /// via [`Effect::SendToPane`]. The capture child is a bare `PtyHost` (not a
    /// `Pane`), so it gets its own variant; a missing `pending_capture` skips
    /// the write silently (matches the former `if let Some(capture)` guard).
    SendToCapture { bytes: Vec<u8> },

    /// A-class. Set the terminal title. The compose + dedup stay loop-side
    /// (`term_title_effect`); only the `term_title::set` IO is the effect.
    SetTerminalTitle { title: String },

    /// A-class. MVU Phase 5: read the active pane's text from the live host
    /// (a bounded, yank-gated `&self` read — never per-frame) and route it
    /// to `then`. The producer is pure-Model (it just emits this); the
    /// live-`PtyHost` read + the no-pane / empty guards + the clipboard IO
    /// all run in `run_effects`, decoupling the yank handlers from the
    /// Runtime. PR 5b adds `PaneTextKind::Pickable` + `PaneTextSink::GotoFile`
    /// for `gf`/`gF`: the executor reads the pickable lines (+ the pane's cwd)
    /// and `goto_file_navigate` resolves the path reference and chdirs to it,
    /// the whole chain synchronous in this one pass.
    ReadPaneText {
        kind: PaneTextKind,
        then: PaneTextSink,
    },

    /// C-class (synchronous, MVU Phase 5 — the chdir de-IO fork). A pure
    /// `apply()` Action arm must not call `AppState::chdir` directly: chdir
    /// does unbounded blocking IO (`canonicalize` + `Listing::read`). It
    /// emits this instead, and `run_effects` runs the IO via the shared
    /// `AppState::change_dir` — synchronously, same tick, *before* the next
    /// render, so the first post-action frame already shows the new listing
    /// (the ratified "synchronous inline `ChangeDir`" decision — there is no
    /// async `ListingLoaded`). On success: focus `focus` (by path) then flash
    /// `on_ok`; on failure: flash `"{err_prefix}: {e}"`.
    ///
    /// Scope (PR7): only the pure-Model Action arms (`gh`/Home, `gs`/start,
    /// `gp`/project-home, prev-dir, mark-jump, `..`/climb) route through this
    /// effect. Impure App-layer callers (harpoon / finder / inventory /
    /// session-restore / pager jump-history) keep calling `chdir` directly —
    /// they already live in the executor layer, so threading an effect buys
    /// no purity. `:cd` and the worktree prompts await PR9's
    /// `CommandResult`/`PromptResult` carrier widening; the MCP `jump_to`
    /// stays inline for its synchronous read-after-write reply.
    ChangeDir {
        path: PathBuf,
        focus: Option<PathBuf>,
        on_ok: Option<String>,
        err_prefix: &'static str,
    },

    /// Tier 5. Run a graveyard mutation (archive / restore / purge-all) on a
    /// detached worker thread — its tar+zstd / trash IO is proportional to the
    /// tree size and must never block the event loop. The worker pushes a
    /// `GraveyardOutcome` onto `runtime.graveyard_results` and wakes the loop
    /// with `Message::GraveyardDone`; `apply_graveyard_outcomes` (pre-recv
    /// scan) does the flash + listing/graveyard refresh. The cheap prep (which
    /// paths, which entry, the in-memory entry list) is done by the producer.
    Graveyard(graveyard_ops::GraveyardOp),

    /// Render a ` ```mermaid ` block to a PNG on a detached worker and open it in
    /// the OS image viewer — parse/layout/raster/font-load is far too heavy for
    /// the loop. The worker pushes a `MermaidOutcome` onto
    /// `runtime.mermaid_results` and wakes with `Message::MermaidDone`;
    /// `apply_mermaid_outcomes` (pre-recv scan) surfaces it in the pager status
    /// line. See `docs/MERMAID_PAGER_PLAN.md`.
    RenderMermaid(mermaid_ops::MermaidRenderOp),

    /// Tier 5. Run a file operation (copy / move / pipe) on a detached worker
    /// thread to avoid blocking the event loop. The worker pushes its outcome
    /// onto `runtime.file_results` and wakes with `Message::FileOpDone`.
    FileOp(file_ops::FileOp),

    /// Tier 5. Run an inventory mutation (yank / remove / clear / put) on a
    /// detached worker thread to avoid blocking the event loop. The worker
    /// pushes its outcome onto `runtime.inventory_results` and wakes with
    /// `Message::InventoryDone`.
    Inventory(inventory_ops::InventoryOp),

    /// Tier 5. Interactive `W n`: create a worktree for `branch` off `base`,
    /// discovering the repo from `dir`. The `gix` full-tree checkout runs on the
    /// shared worktree worker (running it inline froze the input thread — the
    /// code-review HIGH); `apply_worktree_outcomes` chdirs the focused column
    /// into the new tree when it lands. The MCP `create_worktree` already used
    /// this worker.
    WorktreeCreateInteractive {
        dir: PathBuf,
        branch: String,
        base: Option<String>,
    },
}

/// Which slice of the active pane's text to materialize (MVU Phase 5).
#[derive(Debug, Clone, Copy)]
pub enum PaneTextKind {
    /// The visible screen (`Pane::visible_lines`).
    Visible,
    /// The recent scrollback + visible screen, capped at `n` lines
    /// (`Pane::recent_lines`).
    Scrollback(usize),
    /// The path-pickable text (`Pane::pickable_text`): the visible viewport
    /// while scrolling, else the last `n` lines. Used by `gf`/`gF` with
    /// [`PaneTextSink::GotoFile`] (PR 5b).
    Pickable(usize),
}

/// Where a [`Effect::ReadPaneText`] result goes (MVU Phase 5).
#[derive(Debug)]
pub enum PaneTextSink {
    /// Copy to the system clipboard, flashing `ok` on success (same
    /// reconstruction as [`Effect::CopyToClipboard`]).
    Clipboard { ok: ClipMsg },
    /// `gf`/`gF`: extract a path reference from the read lines and navigate
    /// to it (`goto_file_navigate`). `open_at_line` (gF) also opens the file
    /// in the pager at the referenced line. Paired with
    /// [`PaneTextKind::Pickable`].
    GotoFile { open_at_line: bool },
}

/// Which pane a [`Effect::SendToPane`] targets. `Active` is the active
/// tab (`pane_tabs.active_mut()`); `Overlay` is the top overlay pty
/// (`top_overlay`). Resolved in the executor — if the target is gone the
/// send is skipped (matching the former `if let Some(…)` guards).
#[derive(Debug)]
pub enum PaneTarget {
    Active,
    Overlay,
}

/// What to deliver to the pane. `Key` routes through `Pane::send_key`
/// (preserving its key-trace logging + empty-bytes guard); `Bytes` routes
/// through `Pane::send_bytes` (its own key-trace logging) — so each former
/// call site keeps its exact write path byte-for-byte.
#[derive(Debug)]
pub enum PaneInput {
    Bytes(Vec<u8>),
    Key(KeyEvent),
}

impl PaneInput {
    /// Deliver to `pane` via the matching write path.
    fn send_to(self, pane: &mut Pane) -> Result<()> {
        match self {
            Self::Bytes(bytes) => pane.send_bytes(&bytes),
            Self::Key(key) => pane.send_key(key),
        }
    }

    /// Whether this input is an Enter / carriage-return — the explicit "I've
    /// answered the pane" signal that clears a latched `Blocked` dot. Only a
    /// real Enter keypress (`Key(Enter)`) or a bare CR/LF byte counts; typing
    /// other keys, navigating a menu, or pasting (bytes that merely *contain* a
    /// newline) leaves the dot stuck red until the user actually commits.
    fn is_enter(&self) -> bool {
        match self {
            Self::Key(k) => matches!(k.code, crossterm::event::KeyCode::Enter),
            Self::Bytes(b) => b == b"\r" || b == b"\n" || b == b"\r\n",
        }
    }
}

/// The success action for an [`Effect::SignalGroup`]: which task to toggle
/// (carries its id, re-found in the executor — same tick, so still
/// present) and the resulting paused-state + flash string.
#[cfg(unix)]
#[derive(Debug)]
pub enum SigOk {
    /// SIGSTOP succeeded → `task.paused = true`, flash
    /// `"task #{id} paused — :resume to continue"`.
    Pause(u32),
    /// SIGCONT succeeded → `task.paused = false`, flash `"task #{id} resumed"`.
    Resume(u32),
}

/// The success-flash recipe for a [`Effect::CopyToClipboard`], rich enough
/// to reconstruct each of the four status-bar yank strings byte-for-byte
/// in the executor — line counts recomputed via `lines().count()`,
/// previews via `chars().take(N)` with a byte-length-gated `…`. A bare
/// count could only reconstruct `MultiPath`.
#[derive(Debug)]
pub enum ClipMsg {
    /// `"yanked {n} lines from pane"` — `n = text.lines().count()`.
    PaneLines,
    /// `"yanked {n} lines (full scrollback)"` — `n = text.lines().count()`.
    Scrollback,
    /// `"yanked path: {preview}{…}"` — preview = `text.chars().take(80)`,
    /// `…` iff `text.len() > 80` (byte length).
    SinglePath,
    /// `"yanked {count} paths"`. `count` is carried (a path may contain a
    /// newline, so it can't be recovered from `text` reliably).
    MultiPath { count: usize },
    /// `"yanked prompt: {preview}{…}"` — preview = `text.chars().take(60)`,
    /// `…` iff `text.len() > 60` (byte length).
    Prompt,
}

impl ClipMsg {
    /// Render the success flash for a completed copy of `text`,
    /// byte-for-byte identical to the former inline yank sites.
    fn success(&self, text: &str) -> String {
        match self {
            Self::PaneLines => format!("yanked {} lines from pane", text.lines().count()),
            Self::Scrollback => format!("yanked {} lines (full scrollback)", text.lines().count()),
            Self::SinglePath => {
                let preview: String = text.chars().take(80).collect();
                let ellipsis = if text.len() > 80 { "…" } else { "" };
                format!("yanked path: {preview}{ellipsis}")
            }
            Self::MultiPath { count } => format!("yanked {count} paths"),
            Self::Prompt => {
                let preview: String = text.chars().take(60).collect();
                let ellipsis = if text.len() > 60 { "…" } else { "" };
                format!("yanked prompt: {preview}{ellipsis}")
            }
        }
    }
}

#[cfg(unix)]
impl SigOk {
    /// The task id this signal targets.
    const fn task_id(&self) -> u32 {
        match self {
            Self::Pause(id) | Self::Resume(id) => *id,
        }
    }

    /// The `paused` flag to set on success — STOP pauses, CONT resumes.
    const fn paused(&self) -> bool {
        matches!(self, Self::Pause(_))
    }

    /// The status-bar success flash, byte-identical to the former inline
    /// `pause_task` / `resume_task` sites (note the U+2014 em-dash).
    fn message(&self) -> String {
        match self {
            Self::Pause(id) => format!("task #{id} paused — :resume to continue"),
            Self::Resume(id) => format!("task #{id} resumed"),
        }
    }
}

/// Conversion from the legacy `PostAction::Spawn` carrier to the effect
/// list. The `Spawn` builders stay byte-identical and reach
/// `Effect::ForegroundExec` through this shim.
impl From<PostAction> for Vec<Effect> {
    fn from(pa: PostAction) -> Self {
        match pa {
            PostAction::Spawn {
                program,
                args,
                pause_after,
            } => vec![Effect::ForegroundExec {
                program,
                args,
                pause_after,
            }],
        }
    }
}

impl App {
    /// Execute a tick's worth of effects, in emission order. The **sole**
    /// side-effect executor for the run loop (MVU Phase 4).
    ///
    /// Borrow split: A-class effects (later slices) need `&mut self`;
    /// `ForegroundExec` needs `terminal` AND the loop-local
    /// `foreground_exec` (which owns the park/ack `Arc`s and is *not*
    /// reachable through `&mut self`), so `fg` is passed in. The three
    /// borrows are disjoint — `ForegroundExec::run` takes `&self`.
    pub(super) fn run_effects(
        &mut self,
        effects: Vec<Effect>,
        terminal: &mut Tui,
        fg: &ForegroundExec,
    ) {
        // `ForegroundExec` tears the TUI down, so it must be the sole or
        // last effect in a tick (the wider `Vec<Effect>` newly permits a
        // violation the single-`PostAction` return could not).
        debug_assert!(
            effects.iter().enumerate().all(|(i, e)| !matches!(
                e,
                Effect::ForegroundExec { .. }
            ) || i + 1 == effects.len()),
            "ForegroundExec must be the sole or last effect emitted in a tick"
        );

        for effect in effects {
            match effect {
                // A-class: copy + flash synchronously, same tick. Never
                // `?`-propagate a clipboard error — a failed copy flashes
                // and the loop survives (unlike the former inline sites,
                // which were not in `?` scope, this arm must not abort the
                // run loop on a transient backend failure).
                Effect::CopyToClipboard { text, ok } => match crate::clipboard::copy(&text) {
                    Ok(()) => self.state.flash_info(ok.success(&text)),
                    Err(e) => self.state.flash_error(format!("yank failed: {e}")),
                },
                // A-class: copy + flash the ACTIVE PAGER's title (not the status
                // bar), so a yank inside a pager confirms where the user is
                // looking — the former inline `view.flash` behavior, now after a
                // copy that runs in the executor.
                Effect::CopyToPagerClipboard { text, ok_msg } => {
                    let msg = match crate::clipboard::copy(&text) {
                        Ok(()) => ok_msg,
                        Err(e) => format!("yank failed: {e}"),
                    };
                    self.set_active_pager_flash(msg);
                }
                // A-class: write the pager body to a timestamped file in the
                // process cwd, then flash the path/error in the active pager's
                // title (mirrors the former inline `save_to_file` + `view.flash`).
                Effect::SavePagerOutput { content } => {
                    let stamp = crate::sysinfo::format_now().replace([' ', ':'], "_");
                    let stamp = stamp.trim_end_matches("_UTC");
                    let filename = format!("spyc_output_{stamp}.txt");
                    let msg = match std::env::current_dir() {
                        Ok(dir) => {
                            let path = dir.join(&filename);
                            match std::fs::write(&path, &content) {
                                Ok(()) => format!("saved: {}", path.display()),
                                Err(e) => format!("save failed: {e}"),
                            }
                        }
                        Err(e) => format!("save failed: {e}"),
                    };
                    self.set_active_pager_flash(msg);
                }
                // Read the active pane's text from the live host (bounded,
                // yank-/gf-gated), then route per `then`. The no-pane guard +
                // the pane read moved here from the former yank / gf handlers.
                // The read (lines + the pane's cwd, used by GotoFile) happens
                // behind one borrow that ends before we flash / copy / navigate
                // — byte-identical flash strings, same `Event::Key` tick.
                Effect::ReadPaneText { kind, then } => {
                    let Some((lines, pane_cwd)) = self.runtime.pane_tabs.as_mut().map(|tabs| {
                        let lines = match kind {
                            PaneTextKind::Visible => tabs.active_mut().visible_lines(),
                            PaneTextKind::Scrollback(n) => tabs.active_mut().recent_lines(n),
                            PaneTextKind::Pickable(n) => tabs.active_mut().pickable_text(n),
                        };
                        let pane_cwd = tabs.active_info().cwd.clone();
                        (lines, pane_cwd)
                    }) else {
                        self.state.flash_error("no pane open");
                        continue;
                    };
                    match then {
                        // A-class: join + copy, byte-identical to the former
                        // inline yank sites (`yp`/`ya`).
                        PaneTextSink::Clipboard { ok } => {
                            let text = lines
                                .iter()
                                .map(|l| l.trim_end())
                                .collect::<Vec<_>>()
                                .join("\n");
                            if text.trim().is_empty() {
                                // Empty-flash string is per-kind, byte-identical
                                // to the former inline yank sites for the two
                                // kinds yank actually uses (Visible / Scrollback).
                                // Pickable only ever pairs with `GotoFile`, never
                                // Clipboard — its arm here is for exhaustiveness.
                                self.state.flash_error(match kind {
                                    PaneTextKind::Visible | PaneTextKind::Pickable(_) => {
                                        "pane is empty"
                                    }
                                    PaneTextKind::Scrollback(_) => "pane scrollback is empty",
                                });
                            } else {
                                match crate::clipboard::copy(&text) {
                                    Ok(()) => self.state.flash_info(ok.success(&text)),
                                    Err(e) => {
                                        self.state.flash_error(format!("yank failed: {e}"));
                                    }
                                }
                            }
                        }
                        // C-class: gf/gF. Resolve a path reference from the
                        // pickable lines and navigate to it (synchronous, same
                        // tick); gF also opens it in the pager at the line.
                        PaneTextSink::GotoFile { open_at_line } => {
                            self.goto_file_navigate(lines, pane_cwd, open_at_line);
                        }
                    }
                }
                // A-class: deliver input to the target pane (same tick).
                // Resolve the target; if it's gone, skip silently (matches
                // the former `if let Some(…)` guards). Like the others,
                // never `?`-propagate — flash `err_prefix` (if any) and
                // survive. `on_ok`/`err_prefix` of `None` flash nothing
                // (the `send_key` forwards ignored their result).
                Effect::SendToPane {
                    target,
                    input,
                    on_ok,
                    err_prefix,
                } => {
                    // Echo-latency probe (A-monitor only): stamp when a
                    // keystroke is forwarded to the active pane; the pre-recv
                    // pane scan measures forward→echo on the agent's reply.
                    if self.view.show_activity && matches!(target, PaneTarget::Active) {
                        self.view.pane_send_at = Some(std::time::Instant::now());
                    }
                    // Only an explicit Enter answers the pane. A latched `blocked`
                    // dot stays red through navigation / typing / pastes and clears
                    // ONLY when the user commits with Enter (or a newer report) —
                    // owner spec: "leave it stuck until the user sends <ENTER>".
                    let clears_blocked = input.is_enter();
                    let result = match target {
                        PaneTarget::Active => self.runtime.pane_tabs.as_mut().map(|t| {
                            // The user pressed Enter on the active pane (answering a
                            // Yes/No permission prompt, a question, or submitting a
                            // prompt) → it no longer "needs me": drop the latched
                            // `blocked` self-report so the dot leaves red and follows
                            // the agent's resumed output again.
                            let info = t.active_info_mut();
                            if clears_blocked
                                && info.reported.is_some_and(|r| {
                                    r.status == crate::pane::AgentActivity::Blocked
                                })
                            {
                                info.reported = None;
                            }
                            input.send_to(t.active_mut())
                        }),
                        PaneTarget::Overlay => {
                            // Route to the focused column's overlay slot: `b`'s
                            // own when the right column owns the keyboard, else
                            // the left / single slot. (Focus is `Overlay` here,
                            // so this is never pane-focused.)
                            let slot = if self.focused_side() == crate::app::state::Side::Right
                                && self.runtime.top_overlay_right.is_some()
                            {
                                self.runtime.top_overlay_right.as_mut()
                            } else {
                                self.runtime.top_overlay.as_mut()
                            };
                            slot.map(|ov| input.send_to(ov))
                        }
                    };
                    match result {
                        Some(Ok(())) => {
                            if let Some(msg) = on_ok {
                                self.state.flash_info(msg);
                            }
                        }
                        Some(Err(e)) => {
                            if let Some(prefix) = err_prefix {
                                self.state.flash_error(format!("{prefix}: {e}"));
                            }
                        }
                        None => {}
                    }
                }
                // A-class: write to the running capture child's master writer
                // (raw — captures rarely enable bracketed paste). A vanished
                // `pending_capture` skips silently, matching the former inline
                // `if let Some(capture)` write in the key/paste handlers.
                Effect::SendToCapture { bytes } => {
                    if let Some(capture) = self.runtime.pending_capture.as_mut() {
                        use std::io::Write as _;
                        let _ = capture.host.writer.write_all(&bytes);
                        let _ = capture.host.writer.flush();
                    }
                }
                // A-class: the only side effect of a terminal-title update;
                // compose + dedup already happened loop-side.
                Effect::SetTerminalTitle { title } => {
                    let _ = crate::term_title::set(&title);
                }
                // C-class: the chdir de-IO fork (MVU Phase 5). The blocking
                // listing read runs here in the executor — never in the pure
                // `apply()` transition that emitted us — then focus + flash via
                // the shared `change_dir`. Synchronous: it completes before the
                // next render, so the first post-action frame shows the new dir
                // (mark-jump / `..` / `gs` / `gp` / `gh` / prev-dir).
                Effect::ChangeDir {
                    path,
                    focus,
                    on_ok,
                    err_prefix,
                } => {
                    self.state
                        .change_dir(&path, focus.as_deref(), on_ok.as_deref(), err_prefix);
                    // The chdir may have moved the focused column into a
                    // different worktree → re-key its harpoon. This effect runs
                    // in the executor AFTER `apply`'s reconcile, so without this
                    // the swap would lag a frame (synchronous chdirs inside
                    // `apply_inner` are already covered by that reconcile).
                    self.reconcile_harpoon();
                }
                // A-class: signal the group, then (on success) toggle the
                // task's paused flag — re-found by id, same tick — and
                // flash. Like the clipboard arm this never `?`-propagates;
                // a failed signal flashes `on_err` and the loop survives.
                #[cfg(unix)]
                Effect::SignalGroup {
                    pid,
                    sig,
                    on_ok,
                    on_err,
                } => match super::kill_pg(pid, sig) {
                    Ok(()) => {
                        // Re-find the task by id (same tick, so still
                        // present) and toggle its paused flag, then flash.
                        if let Some(t) = self
                            .runtime
                            .background_tasks
                            .tasks
                            .iter_mut()
                            .find(|t| t.id == on_ok.task_id())
                        {
                            t.paused = on_ok.paused();
                        }
                        self.state.flash_info(on_ok.message());
                    }
                    Err(_) => self.state.flash_error(on_err),
                },
                // Pane `^z` toggle: SIGTSTP (suspend) / SIGCONT (resume) to the
                // pane's process group. The `suspended` flip + flash already ran
                // in the producer; only signal here (a failed kill is rare — we
                // just read a live pid — so flash and let the loop survive).
                #[cfg(unix)]
                Effect::SignalPane { pgrp, resume } => {
                    // SIGSTOP (not SIGTSTP): uncatchable, so Claude can't run
                    // its self-suspend handler — which on macOS ends in the
                    // false-exit. The agent just freezes; reader keeps blocking.
                    let sig = if resume {
                        rustix::process::Signal::CONT
                    } else {
                        rustix::process::Signal::STOP
                    };
                    if super::kill_pg(pgrp, sig).is_err() {
                        self.state.flash_error("pane: signal failed");
                    }
                }
                Effect::ForegroundExec {
                    program,
                    args,
                    pause_after,
                } => {
                    // Capture the result instead of `?`-propagating it: a
                    // failed spawn (missing/misspelled $EDITOR/$SHELL) used to
                    // bubble out of run_effects → App::run and exit spyc,
                    // dropping every pane PtyHost (SIGKILL on the agent
                    // children) without saving the session. `run` already
                    // restored the TUI on the spawn-error path, so here we just
                    // flash it (below) and carry on.
                    let fg_result = fg.run(terminal, &program, &args, pause_after);
                    // --- after-work (moved verbatim from the run loop's
                    // former `if let PostAction::Spawn` call site) ---
                    // Runs regardless of the spawn result: the pager
                    // round-trip still needs unwinding (the temp file holds the
                    // pre-edit content when the editor never launched) and the
                    // listing refresh is harmless.
                    // Child may have clobbered our title; force a
                    // re-emit on next draw.
                    self.view.last_term_title = None;
                    // The listing may have changed (mv, rm, chmod, etc).
                    self.state.refresh_listing();
                    // If we were editing a pager buffer, restore it.
                    if let Some(ret) = self.view.pending_pager_return.take() {
                        match ret {
                            PagerReturn::TempFile {
                                path,
                                title,
                                scroll,
                                mount,
                                pane_scroll,
                            } => {
                                match std::fs::read_to_string(&path) {
                                    Ok(content) => {
                                        let lines: Vec<String> =
                                            content.lines().map(String::from).collect();
                                        let mut view = PagerView::new_plain(title, lines);
                                        view.scroll = scroll;
                                        view.saveable = true;
                                        view.mount = mount;
                                        view.pane_scroll = pane_scroll;
                                        // A scrollback-sourced edit returns to
                                        // its own bottom slot, not the top
                                        // `view.pager` (`set_pager`).
                                        if pane_scroll {
                                            self.restore_scroll_pager_view(view);
                                        } else {
                                            self.set_pager(view);
                                        }
                                        let _ = std::fs::remove_file(&path);
                                    }
                                    Err(e) => {
                                        // Reading the edited buffer back failed.
                                        // Do NOT delete the temp file — it holds
                                        // the user's edits; deleting it (the old
                                        // behavior) silently discarded them. Tell
                                        // the user where to recover them.
                                        self.state.flash_error(format!(
                                            "couldn't read back edits ({e}); preserved at {}",
                                            path.display()
                                        ));
                                    }
                                }
                            }
                            PagerReturn::SourceFile {
                                path,
                                scroll,
                                mount,
                                pane_scroll,
                            } => {
                                // Reuse `build_pager_view_for_file` so a
                                // markdown file edited via `v` re-renders
                                // on return. Reported by JRob: open a .md
                                // (rendered), `v` to edit, quit $EDITOR —
                                // file came back as plain text with no
                                // `m`-toggle (the inline rebuild here used
                                // `PagerView::new_plain` and skipped the
                                // markdown / alt_lines branch entirely).
                                if let Some(mut view) = self.build_pager_view_for_file(&path, None)
                                {
                                    // Override the position restored from
                                    // the per-file cache with the scroll
                                    // we explicitly stashed before
                                    // launching $EDITOR — it's the more
                                    // recent intent for this round-trip.
                                    view.scroll = scroll;
                                    view.mount = mount;
                                    view.pane_scroll = pane_scroll;
                                    if pane_scroll {
                                        self.restore_scroll_pager_view(view);
                                    } else {
                                        self.set_pager(view);
                                    }
                                }
                            }
                        }
                    }
                    // Flash a failed spawn last (so it's the message left on
                    // screen) — never fatal. The TUI and pager were already
                    // restored above.
                    if let Err(e) = fg_result {
                        self.state.flash_error(format!("{program}: {e}"));
                    }
                }
                // Tier 5: run the tar+zstd / trash IO on a detached worker —
                // never the event loop. The worker pushes its outcome onto the
                // shared slot and wakes the loop; `apply_graveyard_outcomes`
                // (pre-recv scan) does the flash + refresh. `pane_wake_tx` is
                // `None` only before `run()` / in the test harness, where there
                // is no loop to wake — the outcome still lands in the slot.
                Effect::Graveyard(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.graveyard_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    std::thread::spawn(move || {
                        let outcome = graveyard_ops::run_graveyard_op(op);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::GraveyardDone);
                        }
                    });
                }
                // Render the mermaid diagram + open it externally on a detached
                // worker (same shape as Graveyard); the outcome lands in
                // `mermaid_results` and `apply_mermaid_outcomes` flashes it.
                Effect::RenderMermaid(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.mermaid_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    // The picker (graphics-protocol capability, detected once at
                    // startup) lives in Runtime; inject it so the View mode can
                    // build a Protocol off-thread. `None` ⇒ no graphics protocol.
                    let picker = self.runtime.picker.clone();
                    std::thread::spawn(move || {
                        let outcome = mermaid_ops::render_mermaid_op(op, picker);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::MermaidDone);
                        }
                    });
                }
                // The single spawn site lives in `file_ops` (shared with the gF
                // executor open); this arm just hands it the op.
                Effect::FileOp(op) => self.spawn_file_op(op),
                // Interactive `W n`: hand the create job to the shared worktree
                // worker with an interactive completion (chdir into the new tree
                // when it lands, not an MCP reply).
                Effect::WorktreeCreateInteractive { dir, branch, base } => self.spawn_worktree_job(
                    worktree_ops::WorktreeJob::Create {
                        dir,
                        branch,
                        base,
                        open: false,
                    },
                    worktree_ops::WorktreeCompletion::InteractiveCreate,
                ),
                Effect::Inventory(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.inventory_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    std::thread::spawn(move || {
                        let outcome = inventory_ops::run_inventory_op(op);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::InventoryDone);
                        }
                    });
                }
            }
        }
    }
}

/// Test-only **intent matchers** over a slice of [`Effect`]s.
///
/// The pure `apply()` transitions return `Vec<Effect>`; a test wants to assert
/// *what was requested* — a chdir to `/tmp`, a pane read — without pinning the
/// effect's struct layout. Destructuring `Effect::ChangeDir { path, focus,
/// on_ok, err_prefix }` inline in every test means a new field breaks them all:
/// the refactoring paralysis the testing campaign targets. These matchers are
/// the **single** place that destructures (with `..`, so new fields are
/// transparent); tests read the fields they care about through the view.
///
/// Grow this as the campaign reaches each cluster — a boolean `requests_*` for
/// "an intent was emitted" checks, a `*View` when a test asserts fields. Add
/// the matcher the test needs, not a speculative shelf of unused helpers.
#[cfg(test)]
pub mod matchers {
    use std::path::Path;

    use super::{Effect, PaneTextKind, PaneTextSink};

    /// Intent assertions over `&[Effect]`. Works on a `Vec<Effect>`,
    /// `fx.as_slice()`, or a borrowed slice (the impl is on `[Effect]`).
    pub trait EffectSliceExt {
        /// The sole emitted effect as a [`ChangeDirView`], iff the slice is
        /// *exactly* one `ChangeDir`. `None` otherwise — so a stray extra
        /// effect is still caught, matching the old `[Effect::ChangeDir { .. }]`
        /// single-element patterns.
        fn change_dir(&self) -> Option<ChangeDirView<'_>>;

        /// The sole `ReadPaneText`'s `(kind, sink)`, iff the slice is exactly
        /// one `ReadPaneText`.
        fn read_pane_text(&self) -> Option<(&PaneTextKind, &PaneTextSink)>;

        /// The sole `InventoryOp`, iff the slice is exactly one `Effect::Inventory`.
        fn inventory(&self) -> Option<&crate::app::inventory_ops::InventoryOp>;
    }

    impl EffectSliceExt for [Effect] {
        fn change_dir(&self) -> Option<ChangeDirView<'_>> {
            match self {
                [
                    Effect::ChangeDir {
                        path,
                        focus,
                        on_ok,
                        err_prefix,
                        ..
                    },
                ] => Some(ChangeDirView {
                    path,
                    focus: focus.as_deref(),
                    on_ok: on_ok.as_deref(),
                    err_prefix,
                }),
                _ => None,
            }
        }

        fn read_pane_text(&self) -> Option<(&PaneTextKind, &PaneTextSink)> {
            match self {
                [Effect::ReadPaneText { kind, then, .. }] => Some((kind, then)),
                _ => None,
            }
        }

        fn inventory(&self) -> Option<&crate::app::inventory_ops::InventoryOp> {
            match self {
                [Effect::Inventory(op, ..)] => Some(op),
                _ => None,
            }
        }
    }

    /// Borrowed view over an `Effect::ChangeDir`'s fields, decoupling tests
    /// from the struct's layout: only the one destructure in `change_dir`
    /// sees the fields (via `..`), so a new field never breaks a caller.
    pub struct ChangeDirView<'a> {
        path: &'a Path,
        focus: Option<&'a Path>,
        on_ok: Option<&'a str>,
        err_prefix: &'a str,
    }

    impl ChangeDirView<'_> {
        /// The destination directory.
        pub fn path(&self) -> &Path {
            self.path
        }
        /// The row to focus after the chdir, if any.
        pub fn focus(&self) -> Option<&Path> {
            self.focus
        }
        /// The success flash, if any.
        pub fn on_ok(&self) -> Option<&str> {
            self.on_ok
        }
        /// The error-flash prefix.
        pub fn err_prefix(&self) -> &str {
            self.err_prefix
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipMsg, Effect, PaneInput, PostAction};

    #[test]
    fn pane_input_is_enter_only_for_a_real_enter() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |c| PaneInput::Key(KeyEvent::new(c, KeyModifiers::NONE));
        // The clear-the-blocked-dot trigger: a real Enter keypress or a bare CR/LF.
        assert!(key(KeyCode::Enter).is_enter());
        assert!(PaneInput::Bytes(b"\r".to_vec()).is_enter());
        assert!(PaneInput::Bytes(b"\n".to_vec()).is_enter());
        assert!(PaneInput::Bytes(b"\r\n".to_vec()).is_enter());
        // Everything else leaves a latched blocked dot stuck (owner spec):
        // navigation, typing, and pastes that merely *contain* a newline.
        assert!(!key(KeyCode::Char('y')).is_enter());
        assert!(!key(KeyCode::Down).is_enter());
        assert!(!PaneInput::Bytes(b"line1\nline2".to_vec()).is_enter());
        assert!(!PaneInput::Bytes(b"1".to_vec()).is_enter());
    }

    #[test]
    fn clip_pane_lines_count_recomputed_from_text() {
        // The count is `lines().count()` on the carried text — a trailing
        // newline does NOT add a phantom line (matches the former inline
        // `text.lines().count()`).
        assert_eq!(
            ClipMsg::PaneLines.success("a\nb\nc"),
            "yanked 3 lines from pane"
        );
        assert_eq!(
            ClipMsg::PaneLines.success("a\nb\n"),
            "yanked 2 lines from pane"
        );
        assert_eq!(
            ClipMsg::PaneLines.success("solo"),
            "yanked 1 lines from pane"
        );
    }

    #[test]
    fn clip_scrollback_string() {
        assert_eq!(
            ClipMsg::Scrollback.success("x\ny"),
            "yanked 2 lines (full scrollback)"
        );
    }

    #[test]
    fn clip_multipath_uses_carried_count_not_text() {
        // count is carried, so a path containing a newline can't inflate it.
        assert_eq!(
            ClipMsg::MultiPath { count: 3 }.success("/a\nwith newline\n/b\n/c"),
            "yanked 3 paths"
        );
    }

    #[test]
    fn clip_single_path_no_ellipsis_under_80_bytes() {
        let p = "/short/path";
        assert_eq!(ClipMsg::SinglePath.success(p), format!("yanked path: {p}"));
    }

    #[test]
    fn clip_single_path_ellipsis_over_80_bytes_ascii() {
        let p = "a".repeat(100);
        let preview: String = p.chars().take(80).collect();
        assert_eq!(
            ClipMsg::SinglePath.success(&p),
            format!("yanked path: {preview}…")
        );
    }

    #[test]
    fn clip_prompt_ellipsis_gated_on_byte_length_with_multibyte() {
        // 30 'é' chars = 60 bytes (2 bytes each). `chars().take(60)` keeps
        // all 30; the `…` gate is on BYTE length (`text.len() > 60`), so
        // 60 bytes is NOT over the threshold → no ellipsis. This pins the
        // char-preview vs byte-gate distinction the inline site had.
        let exactly_60_bytes = "é".repeat(30);
        assert_eq!(exactly_60_bytes.len(), 60);
        assert_eq!(
            ClipMsg::Prompt.success(&exactly_60_bytes),
            format!("yanked prompt: {exactly_60_bytes}")
        );

        // 31 'é' = 62 bytes > 60 → ellipsis; preview is the first 60 CHARS
        // (= all 31 here, since take(60) > 31), so the whole string then `…`.
        let over = "é".repeat(31);
        assert_eq!(over.len(), 62);
        let preview: String = over.chars().take(60).collect();
        assert_eq!(
            ClipMsg::Prompt.success(&over),
            format!("yanked prompt: {preview}…")
        );
    }

    #[test]
    fn post_action_spawn_maps_to_one_foreground_exec() {
        let effects: Vec<Effect> = PostAction::Spawn {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "echo hi".to_string()],
            pause_after: true,
        }
        .into();
        match effects.as_slice() {
            [
                Effect::ForegroundExec {
                    program,
                    args,
                    pause_after,
                },
            ] => {
                assert_eq!(program, "sh");
                assert_eq!(args, &["-c".to_string(), "echo hi".to_string()]);
                assert!(pause_after, "pause_after must flow through byte-identical");
            }
            other => panic!("expected one ForegroundExec, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn sig_pause_message_state_and_id() {
        let s = super::SigOk::Pause(7);
        assert_eq!(s.message(), "task #7 paused — :resume to continue");
        assert!(s.paused(), "STOP success sets paused = true");
        assert_eq!(s.task_id(), 7);
    }

    #[cfg(unix)]
    #[test]
    fn sig_resume_message_state_and_id() {
        let s = super::SigOk::Resume(42);
        assert_eq!(s.message(), "task #42 resumed");
        assert!(!s.paused(), "CONT success sets paused = false");
        assert_eq!(s.task_id(), 42);
    }

    #[cfg(unix)]
    #[test]
    fn sig_pause_uses_em_dash_not_ascii_hyphen() {
        // U+2014 EM DASH — pins the exact separator (the inline site used
        // it; a refactor to " - " would be a silent byte divergence).
        let m = super::SigOk::Pause(1).message();
        assert!(m.contains('\u{2014}'), "must contain the em-dash U+2014");
        assert!(!m.contains(" - "), "must not be an ASCII ' - '");
    }
}

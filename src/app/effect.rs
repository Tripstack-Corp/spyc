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

use anyhow::Result;

use crate::Tui;
use crate::ui::pager::PagerView;

use super::{App, ForegroundExec, PagerReturn, PostAction};

/// A side effect for the run loop to execute. Producers (handlers) return
/// a `Vec<Effect>` describing *what* should happen; `run_effects` is the
/// only place that *does* it. `#[non_exhaustive]` so Phase 5 can add the
/// class-D subscription variants (`SpawnPane`, `ResizePane`, …) without a
/// breaking match. The empty `Vec` is the no-op (there is no `None`
/// variant) — see `From<PostAction>`.
#[non_exhaustive]
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

/// Total conversion from the legacy `PostAction` carrier. `None` maps to
/// the empty effect list (a `From<PostAction> for Effect` could not — a
/// `From` must yield exactly one value, and there is a live
/// `ApplyResult::Post(PostAction::None)` site). The `Spawn` builders stay
/// byte-identical and reach `Effect::ForegroundExec` through this shim.
impl From<PostAction> for Vec<Effect> {
    fn from(pa: PostAction) -> Self {
        match pa {
            PostAction::None => Self::new(),
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
    ) -> Result<()> {
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
                Effect::ForegroundExec {
                    program,
                    args,
                    pause_after,
                } => {
                    fg.run(terminal, &program, &args, pause_after)?;
                    // --- after-work (moved verbatim from the run loop's
                    // former `if let PostAction::Spawn` call site) ---
                    // Child may have clobbered our title; force a
                    // re-emit on next draw.
                    self.last_term_title = None;
                    // The listing may have changed (mv, rm, chmod, etc).
                    self.state.refresh_listing();
                    // If we were editing a pager buffer, restore it.
                    if let Some(ret) = self.pending_pager_return.take() {
                        match ret {
                            PagerReturn::TempFile {
                                path,
                                title,
                                scroll,
                                mount,
                                pane_scroll,
                            } => {
                                if let Ok(content) = std::fs::read_to_string(&path) {
                                    let lines: Vec<String> =
                                        content.lines().map(String::from).collect();
                                    let mut view = PagerView::new_plain(title, lines);
                                    view.scroll = scroll;
                                    view.saveable = true;
                                    view.mount = mount;
                                    view.pane_scroll = pane_scroll;
                                    self.set_pager(view);
                                }
                                let _ = std::fs::remove_file(&path);
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
                                if let Some(mut view) = self.build_pager_view_for_file(&path) {
                                    // Override the position restored from
                                    // the per-file cache with the scroll
                                    // we explicitly stashed before
                                    // launching $EDITOR — it's the more
                                    // recent intent for this round-trip.
                                    view.scroll = scroll;
                                    view.mount = mount;
                                    view.pane_scroll = pane_scroll;
                                    self.set_pager(view);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipMsg, Effect, PostAction};

    #[test]
    fn post_action_none_maps_to_empty() {
        assert!(Vec::<Effect>::from(PostAction::None).is_empty());
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

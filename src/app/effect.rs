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
    use super::{Effect, PostAction};

    #[test]
    fn post_action_none_maps_to_empty() {
        assert!(Vec::<Effect>::from(PostAction::None).is_empty());
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
}

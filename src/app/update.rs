//! MVU Stage 3: the single input-derived `update` entry point.
//!
//! `handle_key` resolves a keystroke (or prompt submission) into a [`UiMsg`]
//! and calls [`App::update`], which routes to the per-kind handler — `apply`
//! for resolver actions, `apply_user` for user-keymap bindings,
//! `dispatch_prompt` for submitted prompts. Each handler normalizes its
//! pure-domain producer result into `state::Update` and returns the
//! `Vec<Effect>` the run loop executes (MVU Stage 3C). This replaces the three
//! separate `handle_key` dispatch sites with one entry: "where do I handle
//! this?" now has a single answer.

use anyhow::Result;

use crate::keymap::{Action, BoundAction};

use super::prompt::Prompt;
use super::{App, Effect};

/// A resolved keystroke or prompt submission, ready for [`App::update`].
///
/// Distinct from the run loop's I/O-source `Message` enum — this is the
/// intra-frame "what the user did" *after* `route`/`resolver` classification.
/// Keys are not pre-translated to semantic messages (the chord suppressor
/// reads wall-clock `elapsed()`), so `route.rs` stays the router and this
/// carries the already-classified outcome.
pub(super) enum UiMsg {
    /// A resolver-matched built-in action.
    Action(Action),
    /// A user-keymap binding (`apply_user` expands it, often to an `Action`).
    Bound(BoundAction),
    /// A submitted prompt (`:`-command, shell, jump, mkdir, worktree, …).
    Prompt(Prompt),
}

impl App {
    /// The single entry point for input-derived state transitions. Routes the
    /// `UiMsg` to the matching handler; each returns the `Vec<Effect>` the run
    /// loop executes.
    ///
    /// Prompt submission is infallible (`dispatch_prompt` returns
    /// `Vec<Effect>`, not `Result`), so its arm wraps in `Ok`; the `Result`
    /// return exists for the action path, which can propagate IO errors.
    pub(super) fn update(&mut self, msg: UiMsg) -> Result<Vec<Effect>> {
        match msg {
            UiMsg::Action(action) => self.apply(&action),
            UiMsg::Bound(bound) => self.apply_user(&bound),
            UiMsg::Prompt(prompt) => Ok(self.dispatch_prompt(prompt)),
        }
    }
}

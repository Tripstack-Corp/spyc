//! Terminal mouse-mode reconcile.
//!
//! One idempotent settle owns every transition into and out of real mouse
//! reporting, rather than each caller remembering to emit the escape sequence.
//! The `settle_*` shape mirrors `settle_visual_bell` / `settle_autosave` /
//! `settle_lua_events`: called at loop bottom, returns effects as data, touches no
//! OS itself.
//!
//! Why a reconcile rather than push-on-change: the transitions are startup,
//! `:mouse on|off`, a live config reload, and resume-from-foreground — four
//! callers, two of which (reload, resume) don't naturally know they changed
//! anything. Comparing desired-vs-actual makes all four correct by construction.

use super::{App, Effect};

impl App {
    /// Reconcile the terminal's mouse mode against `[mouse] capture`.
    ///
    /// Two values, one truth: `state.config.mouse.capture` is what the user asked
    /// for, `view.mouse_capture_on` is what the terminal is actually in. Comparing
    /// them at loop bottom covers every transition through one path — startup,
    /// `:mouse on|off`, a live `^R` config reload, and returning from a foreground
    /// child (whose `suspend_tui` handed the mouse away, recorded by the
    /// `ForegroundExec` executor resetting the flag).
    ///
    /// Emits nothing when the two already agree, so this is free at idle and
    /// preserves 0 dps — there is no deadline and no wake, just a bool compare on
    /// an iteration that was going to happen anyway.
    ///
    /// `&self`, unlike the other `settle_*` methods: the decision is a read of two
    /// fields, and the state change belongs to the executor (which records what the
    /// terminal actually accepted). That split is what makes it idempotent.
    pub(super) fn settle_mouse_mode(&self) -> Vec<Effect> {
        let want = self.state.config.mouse.capture;
        if want == self.view.mouse_capture_on {
            return Vec::new();
        }
        vec![Effect::SetMouseMode { capture: want }]
    }
}

#[cfg(test)]
mod tests {
    use super::App;
    use crate::app::Effect;

    /// The reconcile emits only on divergence — that's what makes it free to call
    /// every loop bottom, and what keeps idle at 0 dps.
    #[test]
    fn settle_emits_only_when_desired_and_actual_disagree() {
        let mut app = App::test_app(std::env::temp_dir());

        // Agreement (both off, the default) → nothing.
        assert!(!app.state.config.mouse.capture);
        assert!(!app.view.mouse_capture_on);
        assert!(app.settle_mouse_mode().is_empty(), "off/off must be silent");

        // User asks for capture → one enable.
        app.state.config.mouse.capture = true;
        let fx = app.settle_mouse_mode();
        assert!(
            matches!(fx.as_slice(), [Effect::SetMouseMode { capture: true }]),
            "want-on/actual-off must emit enable, got {fx:?}"
        );

        // The executor records what it did; the settle then goes quiet. Emitting
        // again here would re-send the escape every iteration.
        app.view.mouse_capture_on = true;
        assert!(app.settle_mouse_mode().is_empty(), "on/on must be silent");

        // `:mouse off` (or a config reload) → one disable.
        app.state.config.mouse.capture = false;
        let fx = app.settle_mouse_mode();
        assert!(
            matches!(fx.as_slice(), [Effect::SetMouseMode { capture: false }]),
            "want-off/actual-on must emit disable, got {fx:?}"
        );
    }

    /// A foreground child's `suspend_tui` hands the mouse away, and the
    /// `ForegroundExec` executor records that by clearing the flag. The settle has
    /// to notice and take it back, or the wheel silently stops working after every
    /// `v` / `;` round-trip.
    #[test]
    fn settle_reclaims_capture_after_a_foreground_child() {
        let mut app = App::test_app(std::env::temp_dir());
        app.state.config.mouse.capture = true;
        app.view.mouse_capture_on = true;
        assert!(app.settle_mouse_mode().is_empty());

        // What the executor does after `fg.run(..)` returns.
        app.view.mouse_capture_on = false;

        assert!(
            matches!(
                app.settle_mouse_mode().as_slice(),
                [Effect::SetMouseMode { capture: true }]
            ),
            "must re-enable after the child gave the tty back"
        );
    }
}

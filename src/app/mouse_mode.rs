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
    /// Whether the user wants real mouse reporting: the `:mouse` runtime override
    /// if one is set, else `[mouse] capture` from the config.
    ///
    /// The override exists because `:mouse off` has to survive a config reload.
    /// `reload_config` replaces `state.config` wholesale, so a `:mouse` that wrote
    /// `config.mouse.capture` was silently undone by any save of a watched config
    /// file — including the automatic fs-watch reload, which fires without the
    /// user doing anything deliberate. Every doc surface promises the toggle is
    /// session-scoped ("Immediate, no restart, no file edit"), and re-arming
    /// capture underneath a user who turned it off to select text is the exact
    /// surprise those words rule out.
    pub(super) fn mouse_capture_wanted(&self) -> bool {
        self.state
            .mouse_capture_override
            .unwrap_or(self.state.config.mouse.capture)
    }

    /// Reconcile the terminal's mouse mode against what the user wants.
    ///
    /// Two values, one truth: [`Self::mouse_capture_wanted`] is the request,
    /// [`crate::mouse_capture_is_on`] is what the terminal is actually in.
    /// Comparing them at loop bottom covers every transition through one path —
    /// startup, `:mouse on|off`, a live `^R` config reload, and returning from a
    /// foreground child (whose `suspend_tui` handed the mouse away).
    ///
    /// Actual-state lives in a process-global, not on `ViewState`, because the
    /// panic hook and `restore_terminal` also change it and can't reach `App` —
    /// see [`crate::mouse_capture_is_on`] for the failure that caused.
    ///
    /// Emits nothing when the two already agree, so this is free at idle and
    /// preserves 0 dps — there is no deadline and no wake, just a bool compare on
    /// an iteration that was going to happen anyway.
    ///
    /// `&self`, unlike the other `settle_*` methods: the decision is a read, and
    /// the state change belongs to the executor (which records what the terminal
    /// actually accepted). That split is what makes it idempotent.
    pub(super) fn settle_mouse_mode(&self) -> Vec<Effect> {
        let want = self.mouse_capture_wanted();
        if want == crate::mouse_capture_is_on() {
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
    ///
    /// "Actual" is a process-global written by the executor, so these tests drive
    /// it through `crate::set_mouse_capture_for_test` rather than a struct field.
    #[test]
    fn settle_emits_only_when_desired_and_actual_disagree() {
        let _lock = crate::mouse_test_lock();
        let mut app = App::test_app(std::env::temp_dir());
        crate::set_mouse_capture_for_test(false);

        // Agreement (both off, the default) → nothing.
        assert!(!app.state.config.mouse.capture);
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
        crate::set_mouse_capture_for_test(true);
        assert!(app.settle_mouse_mode().is_empty(), "on/on must be silent");

        // `:mouse off` (or a config reload) → one disable.
        app.state.config.mouse.capture = false;
        let fx = app.settle_mouse_mode();
        assert!(
            matches!(fx.as_slice(), [Effect::SetMouseMode { capture: false }]),
            "want-off/actual-on must emit disable, got {fx:?}"
        );
        crate::set_mouse_capture_for_test(false);
    }

    /// A foreground child's `suspend_tui` hands the mouse away and clears the
    /// global. The settle has to notice and take it back, or the wheel silently
    /// stops working after every `v` / `;` round-trip.
    #[test]
    fn settle_reclaims_capture_after_a_foreground_child() {
        let _lock = crate::mouse_test_lock();
        let mut app = App::test_app(std::env::temp_dir());
        app.state.config.mouse.capture = true;
        crate::set_mouse_capture_for_test(true);
        assert!(app.settle_mouse_mode().is_empty());

        // What `suspend_tui` does when the child takes the tty.
        crate::set_mouse_capture_for_test(false);

        assert!(
            matches!(
                app.settle_mouse_mode().as_slice(),
                [Effect::SetMouseMode { capture: true }]
            ),
            "must re-enable after the child gave the tty back"
        );
        crate::set_mouse_capture_for_test(false);
    }

    /// The same reclaim covers the panic hook, which is NOT exit-only: `Pane`
    /// catches a known vt100 panic (nvim leaving the alt screen) and spyc keeps
    /// running. Before the actual-state moved to a global the hook disabled
    /// reporting at the terminal while `App` still believed it was on, so the
    /// reconcile saw agreement and never restored it — mouse dead for the session,
    /// with `:mouse` reporting "on".
    #[test]
    fn settle_reclaims_capture_after_a_caught_panic_restored_the_terminal() {
        let _lock = crate::mouse_test_lock();
        let mut app = App::test_app(std::env::temp_dir());
        app.state.config.mouse.capture = true;
        crate::set_mouse_capture_for_test(true);
        assert!(app.settle_mouse_mode().is_empty());

        // What the panic hook does — reaching no App field.
        crate::set_mouse_capture_for_test(false);

        assert!(
            matches!(
                app.settle_mouse_mode().as_slice(),
                [Effect::SetMouseMode { capture: true }]
            ),
            "a caught panic must not leave the mouse dead for the session"
        );
        crate::set_mouse_capture_for_test(false);
    }

    /// `:mouse off` must survive a config reload. The override is why: writing
    /// `config.mouse.capture` instead meant any save of a watched config file —
    /// including the automatic fs-watch reload — silently re-armed capture
    /// underneath a user who turned it off to select text.
    #[test]
    fn a_runtime_override_outlives_a_config_reload() {
        let _lock = crate::mouse_test_lock();
        let mut app = App::test_app(std::env::temp_dir());
        crate::set_mouse_capture_for_test(true);
        app.state.config.mouse.capture = true;

        // `:mouse off`.
        app.state.mouse_capture_override = Some(false);
        assert!(!app.mouse_capture_wanted());
        assert!(matches!(
            app.settle_mouse_mode().as_slice(),
            [Effect::SetMouseMode { capture: false }]
        ));
        crate::set_mouse_capture_for_test(false);

        // A reload replaces the whole Config — capture is true again in it.
        app.state.config.mouse.capture = true;
        assert!(
            !app.mouse_capture_wanted(),
            "a config reload must not undo `:mouse off`"
        );
        assert!(
            app.settle_mouse_mode().is_empty(),
            "reload must not re-arm capture behind the user"
        );

        // `:mouse auto` hands control back to the config.
        app.state.mouse_capture_override = None;
        assert!(app.mouse_capture_wanted());
        assert!(matches!(
            app.settle_mouse_mode().as_slice(),
            [Effect::SetMouseMode { capture: true }]
        ));
        crate::set_mouse_capture_for_test(false);
    }
}

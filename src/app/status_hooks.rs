//! Drift re-heal for the agent status hooks.
//!
//! Installing happens once, at pane spawn. Anything that removes the config
//! afterwards — a sibling spyc's teardown, a `git clean -xfd`, a hand edit —
//! left every live agent pane reporting nothing for the rest of the session,
//! with no signal beyond the dots quietly falling back to output timing. This
//! is the reconcile that notices, and the `state::hook_owners` refcount is what
//! makes the common cause rare in the first place.
//!
//! Shaped like `settle_mouse_mode`: called at loop bottom, compares desired
//! against actual. It arms no deadline and piggybacks on an iteration that was
//! going to happen anyway, so an idle spyc stays at 0 dps and simply re-checks
//! the next time something wakes it. The [`RECHECK_EVERY`] throttle then caps
//! the cost at one small read per consented pane dir per interval of activity.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::App;
use crate::state::sessions::AgentKind;

/// How often the drift check may run. Long enough to be free during a burst of
/// keystrokes, short enough that a sibling's exit costs a turn or two of dots
/// rather than the session.
const RECHECK_EVERY: Duration = Duration::from_secs(30);

impl App {
    /// Every hook-supporting agent pane as a deduplicated `(cwd, kind)` list.
    /// Two tabs in one dir are one entry — installing is idempotent, so the
    /// duplicate only bought a redundant write.
    pub(super) fn hook_supporting_panes(&self) -> Vec<(PathBuf, AgentKind)> {
        let mut out: Vec<(PathBuf, AgentKind)> = Vec::new();
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return out;
        };
        for t in tabs.tabs() {
            let profile = crate::agent::detect(&t.info.command);
            if profile.status_hooks().is_none() {
                continue;
            }
            let entry = (t.info.cwd.clone(), profile.kind());
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
        out
    }

    /// Re-install the status hooks for any live agent pane whose config lost
    /// them. Consent is re-read per dir, so a project the user turned off with
    /// `:hooks off` is never resurrected. Returns whether anything was healed —
    /// the caller owns the redraw, since the only visible change is the flash.
    pub(super) fn settle_status_hooks(&mut self, now: Instant) -> bool {
        // No socket means the reporter has nothing to talk to (same gate as the
        // launch-time install).
        if !self.view.mcp_running || self.runtime.hook_recheck_at.is_some_and(|at| now < at) {
            return false;
        }
        self.runtime.hook_recheck_at = Some(now + RECHECK_EVERY);
        let mut healed = false;
        for (cwd, kind) in self.hook_supporting_panes() {
            let Some(support) = crate::agent::profile_for(kind).status_hooks() else {
                continue;
            };
            if support.installed(&cwd) {
                continue;
            }
            let root = super::state::find_repo_root(&cwd).unwrap_or_else(|| cwd.clone());
            if crate::state::hook_consent::consent_for(&root) != Some(true) {
                continue;
            }
            self.install_status_hooks(&cwd, kind);
            // Only reachable when they were actually missing, so this flashes
            // once per removal rather than once per interval.
            let note = if support.live_reload {
                ""
            } else {
                " — takes effect on the agent's next launch"
            };
            self.state.flash_info(format!(
                "status hooks were missing — restored {}{note}",
                support.config_label
            ));
            healed = true;
        }
        healed
    }
}

#[cfg(test)]
mod tests {
    use super::RECHECK_EVERY;
    use crate::app::App;
    use crate::pane::tabs::{PaneTabs, TabEntry, TabInfo};
    use crate::state::sessions::AgentKind;
    use std::path::Path;
    use std::time::Instant;

    /// An app holding `count` agent tabs in `dir`. Each pty actually runs `cat`
    /// (a real spawn a test can afford) while its `TabInfo` command reads
    /// `claude`, so agent detection sees an agent — the harness_tests pattern.
    fn app_with_agent_tabs(dir: &Path, count: usize) -> App {
        let mut app = App::test_app(dir.to_path_buf());
        app.view.mcp_running = true;
        for _ in 0..count {
            let wake = app.make_pane_wake();
            let pane = crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.context_path, wake)
                .expect("spawn cat");
            let entry = TabEntry::new(pane, TabInfo::new("claude", dir.to_path_buf()));
            match app.runtime.pane_tabs.as_mut() {
                Some(tabs) => tabs.push(entry),
                None => app.runtime.pane_tabs = Some(PaneTabs::new(entry)),
            }
        }
        app
    }

    /// The regression this module exists for: hooks deleted under a live pane
    /// come back. Consent is granted, so the re-heal is entitled to act.
    #[test]
    fn a_deleted_config_is_reinstalled_under_a_live_pane() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let mut app = app_with_agent_tabs(&dir, 1);
            crate::state::hook_consent::set_consent(&dir, true);
            app.install_status_hooks(&dir, AgentKind::Claude);

            let settings = dir.join(".claude").join("settings.json");
            assert!(settings.exists(), "the launch install must write them");
            std::fs::remove_file(&settings).unwrap();

            // Due immediately — nothing has armed the throttle yet.
            app.settle_status_hooks(Instant::now());
            assert!(settings.exists(), "the drift check must restore them");
        });
    }

    /// A project that said no stays no — the re-heal must not become a back
    /// door around `:hooks off`.
    #[test]
    fn a_declined_project_is_never_reinstalled() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let mut app = app_with_agent_tabs(&dir, 1);
            crate::state::hook_consent::set_consent(&dir, true);
            app.install_status_hooks(&dir, AgentKind::Claude);
            let settings = dir.join(".claude").join("settings.json");
            std::fs::remove_file(&settings).unwrap();

            crate::state::hook_consent::set_consent(&dir, false);
            app.settle_status_hooks(Instant::now());
            assert!(!settings.exists(), "a declined project must stay clean");
        });
    }

    /// The throttle is what keeps this free to call every loop bottom.
    #[test]
    fn the_check_is_throttled_between_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let mut app = app_with_agent_tabs(&dir, 1);
            crate::state::hook_consent::set_consent(&dir, true);
            app.install_status_hooks(&dir, AgentKind::Claude);
            let settings = dir.join(".claude").join("settings.json");

            let now = Instant::now();
            app.settle_status_hooks(now);
            std::fs::remove_file(&settings).unwrap();

            // Inside the window: skipped, so the file stays gone.
            app.settle_status_hooks(now + RECHECK_EVERY / 2);
            assert!(!settings.exists(), "a check must not run early");

            app.settle_status_hooks(now + RECHECK_EVERY);
            assert!(settings.exists(), "the next window must heal it");
        });
    }

    /// Without a socket the reporter has nothing to reach, so writing hooks
    /// would only dirty the project for nothing.
    #[test]
    fn no_mcp_socket_means_no_reinstall() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let mut app = app_with_agent_tabs(&dir, 1);
            crate::state::hook_consent::set_consent(&dir, true);
            app.install_status_hooks(&dir, AgentKind::Claude);
            let settings = dir.join(".claude").join("settings.json");
            std::fs::remove_file(&settings).unwrap();

            app.view.mcp_running = false;
            app.settle_status_hooks(Instant::now());
            assert!(!settings.exists(), "no socket ⇒ no write");
        });
    }

    /// Two tabs sharing a dir are one unit of work, not two.
    #[test]
    fn panes_sharing_a_dir_collapse_to_one_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let app = app_with_agent_tabs(&dir, 2);
            assert_eq!(app.hook_supporting_panes().len(), 1);
        });
    }

    /// A pane running something spyc can't wire has nothing to re-heal.
    #[test]
    fn a_non_agent_pane_is_not_hook_supporting() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let mut app = App::test_app(dir.clone());
            let wake = app.make_pane_wake();
            let pane = crate::pane::Pane::spawn("cat", 24, 80, &dir, &app.view.context_path, wake)
                .expect("spawn cat");
            let entry = TabEntry::new(pane, TabInfo::new("cat", dir.clone()));
            app.runtime.pane_tabs = Some(PaneTabs::new(entry));
            assert!(app.hook_supporting_panes().is_empty());
        });
    }
}

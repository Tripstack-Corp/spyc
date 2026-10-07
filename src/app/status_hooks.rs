//! Drift re-heal for the agent status hooks.
//!
//! Installing happens once, at pane spawn. Anything that removes the config
//! afterwards — a sibling spyc's teardown, a `git clean -xfd`, a hand edit —
//! left every live agent pane reporting nothing for the rest of the session,
//! with no signal beyond the dots quietly falling back to output timing. This
//! is the reconcile that notices, and the `state::dir_owners` refcount is what
//! makes the common cause rare in the first place.
//!
//! Shaped like `settle_mouse_mode`: called at loop bottom, compares desired
//! against actual. It arms no deadline and piggybacks on an iteration that was
//! going to happen anyway, so an idle spyc stays at 0 dps and simply re-checks
//! the next time something wakes it. The [`RECHECK_EVERY`] throttle then caps
//! the cost at one small read per consented pane dir per interval of activity.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{App, Mode, Prompt, PromptKind, state};
use crate::state::sessions::AgentKind;

/// How often the drift check may run. Long enough to be free during a burst of
/// keystrokes, short enough that a sibling's exit costs a turn or two of dots
/// rather than the session.
const RECHECK_EVERY: Duration = Duration::from_secs(30);

/// Report installation facts without treating config presence or an agent's
/// self-report as proof that a lifecycle hook was trusted and executed.
pub(super) fn status_hooks_diagnostic(info: &crate::pane::TabInfo) -> Option<String> {
    let profile = crate::agent::detect(&info.command);
    let support = profile.status_hooks()?;
    let dir = (support.config_dir)(&info.cwd);
    let presence = if support.installed(&info.cwd) {
        "reporter marker found"
    } else {
        "MISSING (`:hooks on`)"
    };
    let startup = if support.live_reload {
        "live reload supported"
    } else if info.status_hooks_restart_needed {
        "restart needed (changed after launch)"
    } else if info.status_hooks_at_spawn {
        "present at launch"
    } else {
        "not present at launch"
    };
    let verification = if profile.kind() == AgentKind::Codex {
        "execution/trust unverified; review /hooks and project trust"
    } else {
        "hook execution unverified"
    };
    let legacy = if profile.kind() == AgentKind::Codex {
        crate::mcp::codex_legacy_hook_diagnostic(&dir)
            .map(|note| format!("; {note}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let location = if profile.kind() == AgentKind::Codex && dir != info.cwd {
        format!(
            "{} or {}; linked-worktree hook declarations are ignored",
            dir.join(support.config_label).display(),
            dir.join(".codex/hooks.json").display()
        )
    } else if profile.kind() == AgentKind::Codex {
        ".codex/config.toml or .codex/hooks.json".to_string()
    } else {
        support.config_label.to_string()
    };
    Some(format!(
        "{presence} in {location}; {startup}; {verification}{legacy}"
    ))
}

impl App {
    /// After launching an agent pane that supports status hooks (claude/codex/agy):
    /// install them if this project has already consented, do nothing if it
    /// declined, else raise the first-launch consent popup (`HookConsent`).
    /// Consent is keyed by the actual hook source's project root and saved.
    /// Gated on MCP running (the hooks report over the socket) and a status-hook
    /// installer. For codex the already-consented case is also written
    /// pre-spawn ([`maybe_preinstall_startup_hooks`](Self::maybe_preinstall_startup_hooks))
    /// so the hooks are live this session; here it's a harmless idempotent re-run.
    pub(super) fn maybe_offer_status_hooks(&mut self, cmd: &str, cwd: &std::path::Path) {
        if !self.view.mcp_running {
            return;
        }
        let profile = crate::agent::detect(cmd);
        let Some(support) = profile.status_hooks() else {
            return;
        };
        let kind = profile.kind();
        let dir = (support.config_dir)(cwd);
        let root = state::find_repo_root(&dir).unwrap_or_else(|| dir.clone());
        match crate::state::hook_consent::consent_for(&root) {
            Some(true) => self.install_status_hooks(cwd, kind),
            Some(false) => {}
            None => {
                self.state.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::HookConsent {
                        root,
                        cwd: cwd.to_path_buf(),
                        agent: kind,
                    },
                    format!(
                        "Show this agent's live status on its tab? spyc will write status hooks to {} (removed when the pane exits).{}",
                        dir.join(support.config_label).display(),
                        if kind == crate::state::sessions::AgentKind::Codex {
                            " Restart Codex, then review /hooks and project trust; spyc does not approve hooks."
                        } else {
                            ""
                        }
                    ),
                ));
            }
        }
    }

    /// Pre-spawn install for agents that read their hook config only at startup
    /// (codex): when this project has already consented, write the status hooks
    /// BEFORE the pty spawns so they're live for THIS session — a post-spawn
    /// install would be missed until the agent's next launch. Live-reload agents
    /// (claude) skip this; their post-spawn `maybe_offer_status_hooks` install
    /// is picked up on the next turn. First-launch consent is still handled
    /// post-spawn (it can't pre-empt the interactive popup).
    pub(super) fn maybe_preinstall_startup_hooks(&mut self, cmd: &str, cwd: &std::path::Path) {
        if !self.view.mcp_running {
            return;
        }
        let profile = crate::agent::detect(cmd);
        let Some(support) = profile.status_hooks() else {
            return;
        };
        if support.live_reload {
            return;
        }
        let dir = (support.config_dir)(cwd);
        let root = state::find_repo_root(&dir).unwrap_or_else(|| dir.clone());
        if crate::state::hook_consent::consent_for(&root) == Some(true) {
            self.install_status_hooks(cwd, profile.kind());
        }
    }

    /// Write `kind`'s status hooks into its resolved source and record that dir
    /// for teardown (shares `mcp_config_dirs` with the `.mcp.json` cleanup).
    /// Assumes consent is already granted. A no-op for an agent without a
    /// status-hook installer; a no-op-returning write (git-tracked config)
    /// simply isn't tracked.
    ///
    /// Also claims the dir in the cross-instance owner registry, so a *sibling*
    /// spyc quitting can't delete the hooks this session's panes are still
    /// reporting through (`state::dir_owners`).
    pub(super) fn install_status_hooks(
        &mut self,
        cwd: &std::path::Path,
        kind: crate::state::sessions::AgentKind,
    ) {
        let Some(support) = crate::agent::profile_for(kind).status_hooks() else {
            return;
        };
        let dir = (support.config_dir)(cwd);
        let snapshot = || {
            let mut files = vec![std::fs::read(dir.join(support.config_label)).ok()];
            if kind == crate::state::sessions::AgentKind::Codex {
                files.push(std::fs::read(dir.join(".codex/hooks.json")).ok());
            }
            files
        };
        let before = if support.live_reload {
            None
        } else {
            Some(snapshot())
        };
        let installed = (support.ensure)(&dir);
        if before.is_some_and(|content| content != snapshot())
            && let Some(tabs) = self.runtime.pane_tabs.as_mut()
        {
            for entry in tabs.tabs_mut() {
                if crate::agent::detect(&entry.info.command).kind() == kind
                    && (support.config_dir)(&entry.info.cwd) == dir
                {
                    entry.info.status_hooks_restart_needed = true;
                }
            }
        }
        // A refused migration can leave existing reporters in use by this pane.
        // Retain their shared ownership so sibling cleanup cannot remove them.
        if installed || support.installed(cwd) {
            crate::state::dir_owners::claim(
                crate::state::dir_owners::Shared::StatusHooks,
                &dir,
                std::process::id(),
            );
            if !self.runtime.mcp_config_dirs.iter().any(|d| d == &dir) {
                self.runtime.mcp_config_dirs.push(dir);
            }
        }
    }

    /// The active agent's hook-source project root, else the focused directory's
    /// project root. Consent and `:hooks` use the same source as installation.
    pub(super) fn status_hooks_target_root(&self) -> std::path::PathBuf {
        let dir = self.runtime.pane_tabs.as_ref().map_or_else(
            || self.state.cur().listing.dir.clone(),
            |t| {
                let info = t.active_info();
                crate::agent::detect(&info.command)
                    .status_hooks()
                    .map_or_else(
                        || info.cwd.clone(),
                        |support| (support.config_dir)(&info.cwd),
                    )
            },
        );
        state::find_repo_root(&dir).unwrap_or(dir)
    }

    /// `:hooks on|off` — grant/revoke per-project consent for agent status hooks
    /// and (un)install them for every open hook-supporting pane (claude/codex)
    /// in that project, so a *running* agent gains/loses the hooks, not just
    /// future launches. Claude reloads `.claude/settings.json` live (effect on
    /// its next message); codex reads its config at startup, so an `on` for a
    /// running codex pane writes the hooks but they only apply on codex's next
    /// launch. This is the escape hatch from an accidental `no` at launch.
    pub(super) fn set_status_hooks(&mut self, enable: bool) {
        let root = self.status_hooks_target_root();
        crate::state::hook_consent::set_consent(&root, enable);
        // Hook-supporting panes whose project root matches — collect (cwd, kind)
        // first (immutable borrow) so the install/cleanup mutations don't alias.
        let panes: Vec<(std::path::PathBuf, crate::state::sessions::AgentKind)> = self
            .hook_supporting_panes()
            .into_iter()
            .filter(|(cwd, _)| state::find_repo_root(cwd).as_deref().unwrap_or(cwd) == root)
            .collect();
        for (cwd, kind) in &panes {
            if enable {
                self.install_status_hooks(cwd, *kind);
            } else if let Some(support) = crate::agent::profile_for(*kind).status_hooks() {
                // Unlike teardown, an explicit `:hooks off` removes them even
                // with a sibling spyc still claiming the dir: the user is
                // revoking consent for the project, and consent is what the
                // sibling's own re-heal consults before re-installing.
                let _ = crate::state::dir_owners::release(
                    crate::state::dir_owners::Shared::StatusHooks,
                    cwd,
                    std::process::id(),
                );
                (support.cleanup)(cwd);
                // Keep the directory for teardown: its independent MCP entry
                // can still be owned after hooks are explicitly disabled.
            }
        }
        let proj = crate::paths::display_tilde(&root);
        if enable {
            let note = Self::ephemeral_hook_binary_note();
            self.state.flash_info(format!(
                "status hook consent ON for {proj} ({} pane(s)) — claude reloads next message; restart codex and review /hooks; check `:activity dump` (`:hooks on!` restarts claude){note}",
                panes.len()
            ));
        } else {
            self.state
                .flash_info(format!("status hooks OFF for {proj}"));
        }
    }

    /// A trailing flash note (or empty) warning that the running binary lives
    /// in a `target/{debug,release}` build dir — its absolute path is baked
    /// into the hook command (`current_exe()`), so it goes stale if that tree
    /// (e.g. a throwaway worktree) is cleaned. `make install` + running from a
    /// stable path avoids it. Cheap; computed only on user-initiated enable.
    fn ephemeral_hook_binary_note() -> String {
        let in_build_dir = std::env::current_exe().is_ok_and(|exe| {
            let has = |name: &str| {
                exe.components()
                    .any(|c| c.as_os_str().to_str() == Some(name))
            };
            has("target") && (has("debug") || has("release"))
        });
        if in_build_dir {
            " — note: build-dir binary, `make install` for a stable hook path".to_string()
        } else {
            String::new()
        }
    }

    /// Every hook-supporting agent pane as a deduplicated `(source, kind)` list.
    /// Two tabs in one dir are one entry — installing is idempotent, so the
    /// duplicate only bought a redundant write.
    pub(super) fn hook_supporting_panes(&self) -> Vec<(PathBuf, AgentKind)> {
        let mut out: Vec<(PathBuf, AgentKind)> = Vec::new();
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return out;
        };
        for t in tabs.tabs() {
            let profile = crate::agent::detect(&t.info.command);
            let Some(support) = profile.status_hooks() else {
                continue;
            };
            let entry = ((support.config_dir)(&t.info.cwd), profile.kind());
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

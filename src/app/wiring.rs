//! The App half of agent wiring (#619): an agent's status hooks and its `spyc`
//! MCP entry live in that agent's own user configuration (`src/wiring/`), set
//! up once. This asks the one `[Y/n]` per agent, runs installs and removals on
//! a worker, serves `:hooks`, makes the startup pass, and says what
//! `:why-status` knows.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::message::{Message, Wake};
use super::{App, Mode, Prompt, PromptKind};
use crate::state::sessions::AgentKind;
use crate::state::wiring_consent;
use crate::wiring::{self, Env, Status};

/// What a worker does to one agent's wiring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WiringOp {
    /// Install it; then restart the claude tab whose id is `restart`
    /// (`:hooks on!`). An id, not an index: tabs can close or move while the
    /// install runs.
    Install {
        restart: Option<String>,
    },
    Remove,
}

/// A worker's result, landed on `runtime.wiring_results`.
pub(super) struct WiringOutcome {
    kind: AgentKind,
    op: WiringOp,
    before: Option<Status>,
    after: Option<Status>,
}

/// Where wiring workers land their outcomes.
pub(super) type WiringResults = Arc<Mutex<Vec<WiringOutcome>>>;

/// Whether `kind`'s status hooks are in place for an agent starting now: in
/// its user configuration, once the user wired it.
pub(super) fn hooks_present(kind: AgentKind) -> bool {
    wiring_consent::opted_in(kind)
        && Env::current()
            .and_then(|env| wiring::status(kind, &env))
            .is_some_and(|st| st.hooks.in_place())
}

/// The facts for an agent wired in user scope.
fn user_scope_facts(info: &crate::pane::TabInfo, kind: AgentKind) -> Vec<String> {
    let st = Env::current().and_then(|env| wiring::status(kind, &env));
    let mut facts = vec![match st {
        Some(st) if st.hooks.in_place() => format!(
            "reporter installed in {}",
            crate::paths::display_tilde(&st.hooks_file)
        ),
        Some(st) => format!(
            "MISSING from {} (`:hooks on {}`)",
            crate::paths::display_tilde(&st.hooks_file),
            wiring::name(kind)
        ),
        None => "MISSING: no user configuration to read".to_string(),
    }];
    let live = info.agent().status_hooks().is_some_and(|s| s.live_reload);
    facts.push(
        if live {
            "live reload supported"
        } else if info.status_hooks_restart_needed {
            "restart needed (changed after launch)"
        } else if info.status_hooks_at_spawn {
            "present at launch"
        } else {
            "not present at launch"
        }
        .to_string(),
    );
    if kind == AgentKind::Codex {
        facts.push("execution/trust unverified: review /hooks".to_string());
    } else {
        facts.push("hook execution unverified".to_string());
    }
    facts
}

/// Report installation facts without treating config presence or an agent's
/// self-report as proof that a lifecycle hook was trusted and executed.
pub(super) fn status_hooks_facts(info: &crate::pane::TabInfo) -> Option<Vec<String>> {
    let kind = info.agent().kind();
    info.agent().status_hooks()?;
    let name = wiring::name(kind);
    Some(match wiring_consent::answer(kind) {
        Some(true) => user_scope_facts(info, kind),
        Some(false) => vec![format!("off: {name} isn't wired (`:hooks on {name}`)")],
        None => vec![format!(
            "not set up: spyc asks when {name} starts (`:hooks on {name}`)"
        )],
    })
}

/// [`App::wire_at_startup`]'s work: bring each agent in `wired` up to date.
fn startup_pass(wired: &[AgentKind], env: &Env) {
    for kind in wired {
        let _ = wiring::install(*kind, env);
    }
}

/// One line per agent for `:hooks`.
fn summary() -> String {
    let each: Vec<String> = wiring::KINDS
        .into_iter()
        .map(|kind| {
            let state = match wiring_consent::answer(kind) {
                Some(true) => "on",
                Some(false) => "off",
                None => "not asked",
            };
            format!("{}: {state}", wiring::name(kind))
        })
        .collect();
    format!("{} — `:hooks on|off|ask [agent]`", each.join(" · "))
}

impl App {
    /// After an agent starts in a tab: ask, once per agent, whether to wire it.
    pub(super) fn offer_wiring(&mut self, kind: AgentKind) {
        if !self.view.mcp_running
            || !wiring::KINDS.contains(&kind)
            || wiring_consent::answer(kind).is_some()
        {
            return;
        }
        let Some(env) = Env::current() else {
            return;
        };
        let name = wiring::name(kind);
        let file = wiring::hooks_file(kind, &env)
            .map(|f| crate::paths::display_tilde(&f))
            .unwrap_or_default();
        let restart = if kind == AgentKind::Codex {
            " Restart codex afterwards, and approve the hooks in /hooks."
        } else {
            ""
        };
        self.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::HookConsent { agent: kind },
            format!(
                "Show {name}'s live status on its tab, and give it spyc's tools? spyc adds its hooks to {file} and its MCP entry to {name}'s own settings, once, for every project.{restart}"
            ),
        ));
    }

    /// Install or remove `kind`'s wiring on a worker: claude's MCP entry goes
    /// through its CLI, which can take a second. Without a loop to wake (the
    /// test harness), it runs in place.
    pub(super) fn start_wiring(&self, kind: AgentKind, op: WiringOp) {
        let results = Arc::clone(&self.runtime.wiring_results);
        let work = move || {
            let env = Env::for_spyc();
            let before = env.as_ref().and_then(|env| wiring::status(kind, env));
            let after = env.as_ref().and_then(|env| match op {
                WiringOp::Install { .. } => wiring::install(kind, env),
                WiringOp::Remove => wiring::remove(kind, env),
            });
            results
                .lock()
                .expect("a wiring worker never panics holding the lock")
                .push(WiringOutcome {
                    kind,
                    op,
                    before,
                    after,
                });
        };
        // A test pins its wiring env to its own thread, where the work must run.
        let wake = self.runtime.pane_wake_tx.clone().filter(|_| !cfg!(test));
        let Some(wake) = wake else {
            work();
            return;
        };
        std::thread::spawn(move || {
            work();
            let _ = wake.send(Message::Wake(Wake::Wiring));
        });
    }

    /// Report what wiring workers did, and flag the tabs that must restart to
    /// load hooks they read only at startup. Returns whether anything landed.
    pub(super) fn apply_wiring_outcomes(&mut self) -> bool {
        let landed: Vec<WiringOutcome> = std::mem::take(
            &mut *self
                .runtime
                .wiring_results
                .lock()
                .expect("a wiring worker never panics holding the lock"),
        );
        let any = !landed.is_empty();
        for outcome in landed {
            self.apply_wiring_outcome(outcome);
        }
        any
    }

    fn apply_wiring_outcome(&mut self, outcome: WiringOutcome) {
        let name = wiring::name(outcome.kind);
        let Some(after) = outcome.after else {
            self.state
                .flash_error(format!("{name}: no $HOME, so nothing to wire"));
            return;
        };
        let installing = outcome.op != WiringOp::Remove;
        let mut text = format!(
            "{name} {}: hooks {}; MCP {}",
            if installing { "wired" } else { "unwired" },
            wiring::describe(&after.hooks, &after.hooks_file, installing),
            wiring::describe(&after.mcp, &after.mcp_file, installing),
        );
        let live = crate::agent::profile_for(outcome.kind)
            .status_hooks()
            .is_some_and(|support| support.live_reload);
        let wrote = outcome
            .before
            .as_ref()
            .is_none_or(|before| before.hooks != wiring::Part::Current);
        if installing && wrote && after.hooks == wiring::Part::Current && !live {
            self.flag_wiring_restart(outcome.kind);
            let _ = write!(text, " — restart {name} to load them");
        }
        for note in &after.notes {
            let _ = write!(text, "; {note}");
        }
        let failed = [&after.hooks, &after.mcp]
            .iter()
            .any(|part| matches!(part, wiring::Part::Refused(_)));
        if failed {
            self.state.flash_error(text);
        } else {
            self.state.flash_info(text);
        }
        // The tab restarts in its own project, which need not be the one in
        // view by the time the install lands.
        if let WiringOp::Install {
            restart: Some(tab_id),
        } = outcome.op
            && let Some(project) = self.runtime.project_of_pane(&tab_id)
        {
            self.land_in(project, |app| app.restart_claude_tab(&tab_id));
        }
    }

    /// Mark `kind`'s tabs, in every project, as needing a restart: they read
    /// their hooks at startup, before this change.
    fn flag_wiring_restart(&mut self, kind: AgentKind) {
        let entries = self
            .runtime
            .every_tab_set_mut()
            .flat_map(|(_, tabs)| tabs.tabs_mut().iter_mut());
        for entry in entries {
            if entry.info.agent().kind() == kind {
                entry.info.status_hooks_restart_needed = true;
            }
        }
    }

    /// Record the user's answer for `kind` and act on it.
    fn set_wiring(&mut self, kind: AgentKind, answer: Option<bool>, restart: Option<String>) {
        wiring_consent::set(kind, answer);
        let name = wiring::name(kind);
        match answer {
            Some(true) => {
                self.state.flash_info(format!("wiring {name}…"));
                self.start_wiring(kind, WiringOp::Install { restart });
            }
            Some(false) => {
                self.state.flash_info(format!("unwiring {name}…"));
                self.start_wiring(kind, WiringOp::Remove);
            }
            None => self
                .state
                .flash_info(format!("spyc asks again when {name} starts")),
        }
    }

    /// `[Y/n]` answered for `kind`: `y` wires it, `n` remembers the decline.
    pub(super) fn answer_wiring(&mut self, kind: AgentKind, yes: bool) {
        let name = wiring::name(kind);
        if yes {
            self.set_wiring(kind, Some(true), None);
        } else {
            wiring_consent::set(kind, Some(false));
            self.state.flash_info(format!(
                "{name} not wired (`:hooks on {name}` to change that)"
            ));
        }
    }

    /// `:hooks [on|on!|off|ask] [agent]`. Without an agent, the active tab's.
    pub(super) fn hooks_command(&mut self, args: &str) {
        let mut words = args.split_whitespace();
        let verb = words.next().unwrap_or("status");
        if verb == "status" {
            self.state.flash_info(summary());
            return;
        }
        let active = self.runtime.project().pane_tabs.as_ref().map(|t| {
            let info = t.active_info();
            (info.id.clone(), info.agent().kind())
        });
        let kind = match words.next() {
            Some(name) => wiring::named(name),
            None => active
                .as_ref()
                .map(|(_, kind)| *kind)
                .filter(|kind| wiring::KINDS.contains(kind)),
        };
        let Some(kind) = kind else {
            self.state
                .flash_error("name an agent: `:hooks on|off|ask claude|codex|agy`");
            return;
        };
        match verb {
            "on" | "enable" | "yes" | "y" => self.set_wiring(kind, Some(true), None),
            "on!" | "enable!" => {
                let restart = active
                    .filter(|(_, active)| *active == AgentKind::Claude && kind == AgentKind::Claude)
                    .map(|(id, _)| id);
                self.set_wiring(kind, Some(true), restart);
            }
            "off" | "disable" | "no" | "n" => self.set_wiring(kind, Some(false), None),
            "ask" => self.set_wiring(kind, None, None),
            other => self.state.flash_error(format!(
                "usage: :hooks [on|on!|off|ask] [claude|codex|agy]  (got `{other}`)"
            )),
        }
    }

    /// At startup, on a worker: bring each wired agent's user configuration up
    /// to date with this spyc.
    pub(super) fn wire_at_startup(&self) {
        let wired: Vec<AgentKind> = wiring::KINDS
            .into_iter()
            .filter(|kind| wiring_consent::opted_in(*kind))
            .collect();
        std::thread::spawn(move || {
            if let Some(env) = Env::for_spyc() {
                startup_pass(&wired, &env);
            }
        });
    }

    /// A codex tab's `CODEX_HOME`, learned once its shell has run: codex reads
    /// its hooks there, so a wired codex gets them in that home too.
    pub(super) fn wire_codex_home(&self, home: &Path) {
        if !wiring_consent::opted_in(AgentKind::Codex) {
            return;
        }
        let Some(mut env) = Env::for_spyc() else {
            return;
        };
        if env.codex_home == home {
            return;
        }
        env.codex_home = home.to_path_buf();
        let before = wiring::status(AgentKind::Codex, &env);
        let after = wiring::install(AgentKind::Codex, &env);
        if before == after {
            return;
        }
        self.runtime
            .wiring_results
            .lock()
            .expect("a wiring worker never panics holding the lock")
            .push(WiringOutcome {
                kind: AgentKind::Codex,
                op: WiringOp::Install { restart: None },
                before,
                after,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    fn env(root: &Path) -> Env {
        Env {
            home: root.join("home"),
            claude_config_dir: None,
            codex_home: root.join("home/.codex"),
            codex_mcp: true,
            managed_mcp: Vec::new(),
            managed_settings: Vec::new(),
            claude_cli: None,
            spyc: "spyc".to_string(),
        }
    }

    /// Run `body` in an app with a throwaway state dir and agent configuration.
    fn with_app(body: impl FnOnce(&mut App, &Path)) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        crate::state::with_state_root(&root.join("state"), || {
            crate::wiring::with_env(env(&root), || {
                let mut app = App::test_app(root.clone());
                app.view.mcp_running = true;
                body(&mut app, &root);
            });
        });
    }

    fn asking_for(app: &App) -> Option<AgentKind> {
        match &app.state.mode {
            Mode::Prompting(Prompt {
                kind: PromptKind::HookConsent { agent },
                ..
            }) => Some(*agent),
            _ => None,
        }
    }

    /// spyc asks once per agent, not per project: an answered agent is never
    /// asked again, wherever it starts, and another agent still is.
    #[test]
    fn each_agent_is_asked_once_for_every_project() {
        with_app(|app, _| {
            app.offer_wiring(AgentKind::Claude);
            assert_eq!(asking_for(app), Some(AgentKind::Claude));
            app.answer_wiring(AgentKind::Claude, false);
            app.state.mode = Mode::Normal;

            app.offer_wiring(AgentKind::Claude);
            assert_eq!(asking_for(app), None, "claude answered");
            app.offer_wiring(AgentKind::Agy);
            assert_eq!(asking_for(app), Some(AgentKind::Agy));
            app.state.mode = Mode::Normal;
            app.offer_wiring(AgentKind::Zot);
            assert_eq!(asking_for(app), None, "zot isn't wired");
        });
    }

    /// Without an MCP socket there is nothing for the hooks to report to, so
    /// nothing is asked.
    #[test]
    fn nothing_is_asked_without_a_socket() {
        with_app(|app, _| {
            app.view.mcp_running = false;
            app.offer_wiring(AgentKind::Claude);
            assert_eq!(asking_for(app), None);
        });
    }

    /// `y` wires the agent in its user configuration, and nothing goes into the
    /// project it started in.
    #[test]
    fn yes_wires_the_agent_in_its_user_configuration() {
        with_app(|app, root| {
            let project = root.join("project");
            std::fs::create_dir_all(&project).unwrap();
            app.offer_wiring(AgentKind::Agy);
            app.state.mode = Mode::Normal;
            app.answer_wiring(AgentKind::Agy, true);
            assert!(app.apply_wiring_outcomes());
            assert_eq!(wiring_consent::answer(AgentKind::Agy), Some(true));
            assert!(
                std::fs::read_to_string(root.join("home/.gemini/config/hooks.json"))
                    .unwrap()
                    .contains("--report-status")
            );
            assert_eq!(std::fs::read_dir(&project).unwrap().count(), 0);
            assert!(hooks_present(AgentKind::Agy));
        });
    }

    /// `:hooks off codex` takes spyc's part out of codex's user configuration and
    /// remembers the answer; `:hooks ask codex` forgets it.
    #[test]
    fn hooks_off_unwires_and_ask_forgets() {
        with_app(|app, root| {
            app.hooks_command("on codex");
            assert!(app.apply_wiring_outcomes());
            let hooks = root.join("home/.codex/hooks.json");
            assert!(
                std::fs::read_to_string(&hooks)
                    .unwrap()
                    .contains("--report-status")
            );

            app.hooks_command("off codex");
            assert!(app.apply_wiring_outcomes());
            assert!(
                !std::fs::read_to_string(&hooks)
                    .unwrap()
                    .contains("--report-status")
            );
            assert_eq!(wiring_consent::answer(AgentKind::Codex), Some(false));

            app.hooks_command("ask codex");
            assert_eq!(wiring_consent::answer(AgentKind::Codex), None);
        });
    }

    #[test]
    fn hooks_without_an_agent_reports_each_one() {
        with_app(|app, _| {
            wiring_consent::set(AgentKind::Claude, Some(true));
            wiring_consent::set(AgentKind::Agy, Some(false));
            app.hooks_command("");
            let text = app.flash_text().unwrap();
            assert!(
                text.contains("claude: on")
                    && text.contains("codex: not asked")
                    && text.contains("agy: off"),
                "{text}"
            );
            app.hooks_command("on");
            assert!(app.flash_text().unwrap().contains("name an agent"));
        });
    }

    /// At startup a wired agent's user configuration is brought up to date.
    #[test]
    fn the_startup_pass_refreshes_a_wired_agent() {
        with_app(|_, root| {
            startup_pass(&[AgentKind::Claude], &env(root));
            assert!(
                std::fs::read_to_string(root.join("home/.claude/settings.json"))
                    .unwrap()
                    .contains("--report-status")
            );
        });
    }

    /// A codex tab whose `CODEX_HOME` isn't spyc's reads its hooks from that home,
    /// so a wired codex gets them there too.
    #[test]
    fn a_codex_home_learned_from_a_tab_is_wired_too() {
        with_app(|app, root| {
            wiring_consent::set(AgentKind::Codex, Some(true));
            let other = root.join("work-codex");
            app.wire_codex_home(&other);
            assert!(app.apply_wiring_outcomes());
            assert!(
                std::fs::read_to_string(other.join("hooks.json"))
                    .unwrap()
                    .contains("--report-status")
            );
            app.wire_codex_home(&other);
            assert!(
                !app.apply_wiring_outcomes(),
                "an unchanged home reports nothing"
            );
        });
    }

    /// What a restart says when it reaches a shell tab running claude: it
    /// reports instead of respawning, since a respawn would restart the shell.
    /// So a test sees which tab a restart reached without starting claude.
    const SHELL_TAB_RESTART: &str = "claude runs in a shell here";

    /// A shell tab running claude (#544), over a `cat` standing in for both.
    fn shell_running_claude(app: &mut App, dir: &Path) -> crate::pane::TabEntry {
        let wake = app.make_pane_wake();
        let pane =
            crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.project().context_path, wake)
                .expect("spawn cat");
        let mut info = crate::pane::TabInfo::new("zsh", dir);
        info.running = Some(crate::pane::RunningAgent {
            kind: AgentKind::Claude,
            pid: 1,
            started_epoch_secs: 0,
            backgrounded: false,
            on_shared_daemon: false,
        });
        crate::pane::TabEntry::new(pane, info)
    }

    /// Two claude tabs, the first one active: where `:hooks on!` is typed.
    fn two_claude_tabs(app: &mut App, dir: &Path) {
        let first = shell_running_claude(app, dir);
        let second = shell_running_claude(app, dir);
        let mut tabs = crate::pane::PaneTabs::new(first);
        tabs.push(second);
        tabs.switch_to(0);
        app.runtime.project_mut().pane_tabs = Some(tabs);
    }

    /// `:hooks on!` restarts the claude tab it was typed in once the install
    /// lands. The positive control for the case below: the restart's report is
    /// how these tests see where it went.
    #[test]
    fn hooks_on_bang_restarts_the_tab_it_was_typed_in() {
        with_app(|app, root| {
            two_claude_tabs(app, root);
            app.hooks_command("on! claude");
            app.state.flash = None;
            app.apply_wiring_outcomes();
            assert!(
                app.flash_text()
                    .unwrap_or_default()
                    .contains(SHELL_TAB_RESTART),
                "the restart never reached the tab: {:?}",
                app.flash_text()
            );
        });
    }

    /// The install takes a moment, and the tab `:hooks on!` was typed in can
    /// close meanwhile. Its restart must go nowhere, not to whichever claude
    /// tab moved into its place.
    #[test]
    fn a_restart_for_a_closed_tab_goes_to_no_other_tab() {
        with_app(|app, root| {
            two_claude_tabs(app, root);
            app.hooks_command("on! claude");
            let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("tabs");
            tabs.remove_at(0);
            app.state.flash = None;
            app.apply_wiring_outcomes();
            assert!(
                !app.flash_text()
                    .unwrap_or_default()
                    .contains(SHELL_TAB_RESTART),
                "a closed tab's restart reached the tab that took its place: {:?}",
                app.flash_text()
            );
        });
    }

    /// A second project, `yproj` under `root`, made the one the user is in.
    fn move_to_another_project(app: &mut App, root: &Path) {
        let home = root.join("yproj");
        std::fs::create_dir_all(&home).unwrap();
        let other = app.add_test_project(&home);
        app.make_active(other);
    }

    /// `:hooks on!` typed in one project restarts the claude tab it was typed
    /// in, though the install lands after the user moved to another project.
    #[test]
    fn a_restart_reaches_its_tab_in_another_project() {
        with_app(|app, root| {
            two_claude_tabs(app, root);
            app.hooks_command("on! claude");
            move_to_another_project(app, root);
            app.state.flash = None;
            app.apply_wiring_outcomes();
            assert!(
                app.flash_text()
                    .unwrap_or_default()
                    .contains(SHELL_TAB_RESTART),
                "the restart never reached the tab: {:?}",
                app.flash_text()
            );
        });
    }

    /// A codex tab (`cat` under codex's name) in the active project.
    fn codex_tab(app: &mut App, dir: &Path) {
        let wake = app.make_pane_wake();
        let pane =
            crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.project().context_path, wake)
                .expect("spawn cat");
        let entry = crate::pane::TabEntry::new(pane, crate::pane::TabInfo::new("codex", dir));
        app.runtime.project_mut().pane_tabs = Some(crate::pane::PaneTabs::new(entry));
    }

    /// The control: wiring codex, which reads its hooks only at startup, flags
    /// its tab for a restart.
    #[test]
    fn wiring_flags_a_tab_for_restart() {
        with_app(|app, root| {
            codex_tab(app, root);
            app.start_wiring(AgentKind::Codex, WiringOp::Install { restart: None });
            app.apply_wiring_outcomes();
            let tabs = app.runtime.project().pane_tabs.as_ref().expect("the tab");
            assert!(tabs.tabs()[0].info.status_hooks_restart_needed);
        });
    }

    /// Wiring an agent that reads its hooks only at startup flags its tabs in
    /// every project for a restart, not only the project in view.
    #[test]
    fn wiring_flags_a_tab_in_another_project_for_restart() {
        with_app(|app, root| {
            let first = app.state.active;
            codex_tab(app, root);
            move_to_another_project(app, root);
            app.start_wiring(AgentKind::Codex, WiringOp::Install { restart: None });
            app.apply_wiring_outcomes();
            let flagged = app.as_project(first, |app| {
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .expect("the tab")
                    .tabs()[0]
                    .info
                    .status_hooks_restart_needed
            });
            assert!(
                flagged,
                "the codex tab in the first project was never flagged"
            );
        });
    }
}

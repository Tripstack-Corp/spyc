//! App-layer glue for the installable Claude skill (`crate::skill`): the startup
//! update offer and the `:skill` command.

use crate::app::{App, Effect, Mode, Prompt, PromptKind};
use crate::skill;

impl App {
    /// Raise the `[Y/n]` update offer if there is one to make. Called once at
    /// startup; the decision of *whether* to offer is
    /// [`skill::startup_offer`] (pure, tested there).
    pub(super) fn maybe_offer_skill_update(&mut self) {
        // Don't displace a prompt already on screen (e.g. a restored session's
        // crash-recovery confirm) — the skill offer is the least urgent thing
        // spyc could ask about.
        if !matches!(self.state.mode, Mode::Normal) {
            return;
        }
        let fingerprint = skill::embedded_fingerprint();
        let statuses = skill::status_all();
        let declined = crate::state::skill_prompt::declined(&fingerprint);
        let Some(overwrites_edits) = skill::startup_offer(&statuses, declined) else {
            return;
        };
        // Name the hosts that are actually behind, so the prompt says whose skill
        // is being touched rather than a vague "your skill".
        let behind: Vec<&str> = statuses
            .iter()
            .filter(|(_, s)| {
                !matches!(
                    s,
                    skill::Status::NotInstalled | skill::Status::UpToDate { .. }
                )
            })
            .map(|(h, _)| h.label())
            .collect();
        let who = if behind.is_empty() {
            "skill".to_string()
        } else {
            format!("{} skill", behind.join(" + "))
        };
        let installed = statuses
            .iter()
            .find_map(|(_, s)| s.installed_version())
            .unwrap_or("unknown");
        let prompt = if overwrites_edits {
            format!(
                "{who} 'spyc' is out of date (installed {installed}, available {}) — but you have local edits an update would REPLACE. Update anyway?",
                skill::embedded_version()
            )
        } else {
            format!(
                "{who} 'spyc' is out of date (installed {installed}, available {}). Update it?",
                skill::embedded_version()
            )
        };
        self.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::SkillUpdate {
                fingerprint,
                overwrites_edits,
            },
            prompt,
        ));
    }

    /// `:skill [status|update|remove|ask]` — manage the installed skill.
    /// Bare `:skill` reports status, which is the common case.
    pub(super) fn cmd_skill(&mut self, arg: &str) -> Vec<Effect> {
        match arg.trim() {
            "" | "status" => {
                let embedded = skill::embedded_version();
                // One line per host, so it's clear which agents have it.
                let parts: Vec<String> = skill::status_all()
                    .iter()
                    .map(|(host, status)| {
                        let h = host.label();
                        match status {
                            skill::Status::NotInstalled => format!("{h}: not installed"),
                            skill::Status::UpToDate { version } => format!("{h}: v{version} ok"),
                            skill::Status::Stale { version } => {
                                format!("{h}: v{version} STALE")
                            }
                            skill::Status::Modified { version, stale } => {
                                if *stale {
                                    format!("{h}: v{version} EDITED + STALE")
                                } else {
                                    format!("{h}: v{version} EDITED")
                                }
                            }
                        }
                    })
                    .collect();
                self.state.flash_info(format!(
                    "spyc skill (v{embedded} available) — {} — `:skill update`",
                    parts.join(", ")
                ));
            }
            "update" | "install" => match skill::install_all(false) {
                Ok(done) if done.is_empty() => {
                    self.state.flash_error("nowhere to install the skill");
                }
                Ok(done) => {
                    // A manual update is an explicit decision, so clear any
                    // remembered decline — otherwise a later edit to the skill
                    // would stay silenced by a stale "no".
                    crate::state::skill_prompt::clear();
                    let where_ = done
                        .iter()
                        .map(|(h, _)| h.label())
                        .collect::<Vec<_>>()
                        .join(" + ");
                    self.state.flash_info(format!(
                        "spyc skill v{} installed for {where_}",
                        skill::embedded_version(),
                    ));
                }
                Err(e) => self.state.flash_error(format!("skill install failed: {e}")),
            },
            "remove" | "uninstall" => match skill::remove_all() {
                Ok(gone) if gone.is_empty() => {
                    self.state.flash_info("spyc skill was not installed");
                }
                Ok(gone) => {
                    let where_ = gone
                        .iter()
                        .map(|h| h.label())
                        .collect::<Vec<_>>()
                        .join(" + ");
                    self.state
                        .flash_info(format!("spyc skill removed from {where_}"));
                }
                Err(e) => self.state.flash_error(format!("skill remove failed: {e}")),
            },
            "ask" => {
                crate::state::skill_prompt::clear();
                self.state
                    .flash_info("skill update will be offered again on next launch");
            }
            other => self.state.flash_error(format!(
                "unknown :skill arg '{other}' — status | update | remove | ask"
            )),
        }
        Vec::new()
    }
}

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
        let status = skill::status();
        let declined = crate::state::skill_prompt::declined(&fingerprint);
        let Some(overwrites_edits) = skill::startup_offer(&status, declined) else {
            return;
        };
        let installed = status.installed_version().unwrap_or("unknown");
        let prompt = if overwrites_edits {
            format!(
                "Claude skill 'spyc' is out of date (installed {installed}, available {}) — but you have local edits an update would REPLACE. Update anyway?",
                skill::embedded_version()
            )
        } else {
            format!(
                "Claude skill 'spyc' is out of date (installed {installed}, available {}). Update it?",
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
                let status = skill::status();
                let embedded = skill::embedded_version();
                let msg = match &status {
                    skill::Status::NotInstalled => format!(
                        "spyc skill not installed (v{embedded} available) — `:skill update` or `spyc --install-skill`"
                    ),
                    skill::Status::UpToDate { version } => {
                        format!("spyc skill v{version} installed, up to date")
                    }
                    skill::Status::Stale { version } => {
                        format!(
                            "spyc skill v{version} installed, STALE (v{embedded} available) — `:skill update`"
                        )
                    }
                    skill::Status::Modified { version, stale } => {
                        let tail = if *stale {
                            format!(
                                ", and STALE (v{embedded} available) — `:skill update` REPLACES your edits"
                            )
                        } else {
                            String::new()
                        };
                        format!("spyc skill v{version} installed, locally EDITED{tail}")
                    }
                };
                self.state.flash_info(msg);
            }
            "update" | "install" => match skill::install() {
                Ok(dir) => {
                    // A manual update is an explicit decision, so clear any
                    // remembered decline — otherwise a later edit to the skill
                    // would stay silenced by a stale "no".
                    crate::state::skill_prompt::clear();
                    self.state.flash_info(format!(
                        "spyc skill v{} installed \u{2192} {}",
                        skill::embedded_version(),
                        crate::paths::display_tilde(&dir)
                    ));
                }
                Err(e) => self.state.flash_error(format!("skill install failed: {e}")),
            },
            "remove" | "uninstall" => match skill::remove() {
                Ok(true) => self.state.flash_info("spyc skill removed"),
                Ok(false) => self.state.flash_info("spyc skill was not installed"),
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

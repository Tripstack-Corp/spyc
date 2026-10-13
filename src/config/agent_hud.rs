//! The opt-in pane strip preference; collection is owned by its visible consumer.

use serde::Deserialize;

pub const HUD_STALE_AFTER_SECS: i64 = 60;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHudMode {
    #[default]
    Off,
    Compact,
    Full,
}

impl AgentHudMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Compact => "compact",
            Self::Full => "full",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "compact" => Some(Self::Compact),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, load::Trust};

    #[test]
    fn agent_hud_is_off_by_default_and_only_home_can_enable_it() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home.toml");
        let project = dir.path().join("project.toml");
        assert_eq!(Config::default().pane.agent_hud, AgentHudMode::Off);
        std::fs::write(&home, "[pane]\nagent_hud = 'full'\n").unwrap();
        std::fs::write(
            &project,
            "[pane]\nagent_hud = 'off'\npreview_pasted_images = false\n",
        )
        .unwrap();
        let mut config = Config::load_layered(Some(&home), &project).unwrap();
        assert_eq!(config.pane.agent_hud, AgentHudMode::Full);
        assert!(
            !config.pane.preview_pasted_images,
            "ordinary pane preferences still merge"
        );
        assert_eq!(
            Config::load_layered(None, &project).unwrap().pane.agent_hud,
            AgentHudMode::Off
        );
        std::fs::write(&project, "[pane]\nagent_hud = 'full'\n").unwrap();
        assert_eq!(
            Config::load_layered(None, &project).unwrap().pane.agent_hud,
            AgentHudMode::Off
        );
        std::fs::write(&home, "[pane]\nagent_hud = 'compact'\n").unwrap();
        config.load_one(&home, Trust::Trusted).unwrap();
        assert_eq!(config.pane.agent_hud, AgentHudMode::Compact);
        std::fs::write(&home, "[pane]\nagent_hud = 'typo'\n").unwrap();
        assert!(Config::load_layered(Some(&home), &project).is_err());
    }
}

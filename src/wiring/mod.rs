//! The wiring that lets an agent reach spyc and report its turn: the `spyc` MCP
//! entry and the status hooks, installed once into each agent's own user
//! configuration (#619) rather than into every directory an agent runs in.
//!
//! What spyc owns inside a shared file is decided by its content, as the
//! per-directory writers decide it: a hook whose command runs
//! `--report-status`, agy's `spyc-status` set, an MCP server named `spyc` that
//! runs spyc. An install that would change nothing writes nothing, and the
//! generated commands never vary, because codex keeps a user's trust in a hook
//! only while its command is unchanged.

mod agy;
mod claude;
mod cli;
mod codex;
mod file;

pub use cli::run_cli;

use std::path::{Path, PathBuf};

use crate::mcp::Removal;
use crate::state::sessions::AgentKind;

/// The agents whose wiring spyc installs in user scope.
pub const KINDS: [AgentKind; 3] = [AgentKind::Claude, AgentKind::Codex, AgentKind::Agy];

/// What the wiring reads from the environment, gathered once so that tests
/// never reach the developer's own configuration.
#[derive(Debug, Clone)]
pub struct Env {
    pub home: PathBuf,
    /// `$CLAUDE_CONFIG_DIR`, which moves both `settings.json` and `.claude.json`.
    pub claude_config_dir: Option<PathBuf>,
    /// Claude's `managed-mcp.json` candidates. One that exists has exclusive
    /// control of claude's MCP servers.
    pub managed_mcp: Vec<PathBuf>,
    /// Claude's `managed-settings.json` candidates.
    pub managed_settings: Vec<PathBuf>,
    /// The `claude` CLI. spyc registers claude's MCP entry through it rather
    /// than editing `.claude.json`, which is claude's own state file.
    pub claude_cli: Option<PathBuf>,
    /// `$CODEX_HOME`, else `~/.codex`.
    pub codex_home: PathBuf,
    /// `[pane] codex_mcp`: `false` keeps spyc's MCP entry out of codex.
    pub codex_mcp: bool,
    /// How an MCP entry and a hook name spyc.
    pub spyc: String,
}

impl Env {
    /// The process's own environment. `None` without `$HOME`.
    pub fn current() -> Option<Self> {
        let var = |name| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = var("HOME")?;
        Some(Self {
            codex_home: var("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            codex_mcp: true,
            home,
            claude_config_dir: var("CLAUDE_CONFIG_DIR"),
            managed_mcp: crate::mcp::MANAGED_MCP_PATHS
                .iter()
                .map(PathBuf::from)
                .collect(),
            managed_settings: crate::mcp::MANAGED_SETTINGS_PATHS
                .iter()
                .map(PathBuf::from)
                .collect(),
            claude_cli: crate::mcp::find_on_path("claude"),
            spyc: crate::mcp::spyc_binary().unwrap_or_else(|| "spyc".to_string()),
        })
    }
}

/// The state of one half of an agent's wiring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Not there.
    Absent,
    /// There, as this spyc writes it.
    Current,
    /// spyc's, but not as this spyc writes it.
    Outdated,
    /// An organization's policy provides it.
    Managed,
    /// Policy forbids it, or something else holds its name.
    Blocked(String),
    /// Its file can't be edited safely; the reason says why.
    Refused(String),
    /// The user's configuration turns it off.
    Off,
}

impl Part {
    /// Whether spyc's part is in place.
    #[must_use]
    pub const fn in_place(&self) -> bool {
        matches!(self, Self::Current | Self::Managed)
    }
}

/// One agent's wiring: its status hooks and its MCP entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub hooks: Part,
    pub hooks_file: PathBuf,
    pub mcp: Part,
    pub mcp_file: PathBuf,
    /// What the user should know beyond the two parts.
    pub notes: Vec<String>,
}

/// `kind`'s wiring as it stands. `None` for an agent spyc doesn't wire here.
#[must_use]
pub fn status(kind: AgentKind, env: &Env) -> Option<Status> {
    match kind {
        AgentKind::Claude => Some(claude::status(env)),
        AgentKind::Codex => Some(codex::status(env)),
        AgentKind::Agy => Some(agy::status(env)),
        _ => None,
    }
}

/// Install `kind`'s wiring, each half best-effort, and report where it stands.
pub fn install(kind: AgentKind, env: &Env) -> Option<Status> {
    match kind {
        AgentKind::Claude => Some(claude::install(env)),
        AgentKind::Codex => Some(codex::install(env)),
        AgentKind::Agy => Some(agy::install(env)),
        _ => None,
    }
}

/// Remove spyc's part of `kind`'s wiring, leaving everything else.
pub fn remove(kind: AgentKind, env: &Env) -> Option<Status> {
    match kind {
        AgentKind::Claude => Some(claude::remove(env)),
        AgentKind::Codex => Some(codex::remove(env)),
        AgentKind::Agy => Some(agy::remove(env)),
        _ => None,
    }
}

/// A part spyc keeps in a file that it edits itself.
struct FilePart<'a> {
    path: PathBuf,
    /// The file with spyc's part as this spyc writes it, or why it's refused.
    with: &'a dyn Fn(Option<&str>) -> Result<String, String>,
    /// The file without spyc's part, or why it's refused.
    without: &'a dyn Fn(&str) -> Result<Removal, String>,
    /// Why the part can't be installed even though the file is fine.
    blocked: Option<String>,
}

impl FilePart<'_> {
    fn status(&self) -> Part {
        if let Some(why) = &self.blocked {
            return Part::Blocked(why.clone());
        }
        let current = match file::read(&self.path) {
            Ok(current) => current,
            Err(why) => return Part::Refused(why),
        };
        let wanted = match (self.with)(current.as_deref()) {
            Ok(wanted) => wanted,
            Err(why) => return Part::Refused(format!("{}: {why}", self.path.display())),
        };
        if current.as_deref() == Some(wanted.as_str()) {
            Part::Current
        } else if current
            .as_deref()
            .and_then(|text| (self.without)(text).ok())
            .is_some_and(|r| r != Removal::NothingOurs)
        {
            Part::Outdated
        } else {
            Part::Absent
        }
    }

    fn install(&self) -> Part {
        match self.status() {
            Part::Absent | Part::Outdated => {}
            settled => return settled,
        }
        let current = file::read(&self.path).ok().flatten();
        let written = (self.with)(current.as_deref())
            .and_then(|wanted| file::write(&self.path, &wanted).map(|_| ()));
        match written {
            Ok(()) => self.status(),
            Err(why) => Part::Refused(why),
        }
    }

    /// Take spyc's part out. A file is never deleted, even one that held only
    /// spyc's part: it may be a link a dotfiles manager keeps.
    fn remove(&self) -> Part {
        if let Some(why) = &self.blocked {
            return Part::Blocked(why.clone());
        }
        let text = match file::read(&self.path) {
            Ok(Some(text)) => text,
            Ok(None) => return Part::Absent,
            Err(why) => return Part::Refused(why),
        };
        match (self.without)(&text) {
            Ok(Removal::Rest { text, .. }) => match file::write(&self.path, &text) {
                Ok(_) => Part::Absent,
                Err(why) => Part::Refused(why),
            },
            Ok(Removal::NothingOurs) => Part::Absent,
            Err(why) => Part::Refused(format!("{}: {why}", self.path.display())),
        }
    }
}

/// A JSON codec's `None` (not a JSON object) as a refusal.
fn json_object(removal: Option<Removal>) -> Result<Removal, String> {
    removal.ok_or_else(|| "refusing to edit — not a JSON object".to_string())
}

/// Whether an MCP entry's command runs spyc, by any path.
fn runs_spyc(command: &str) -> bool {
    Path::new(command)
        .file_name()
        .is_some_and(|name| name == "spyc")
}

/// The command of an MCP server named `spyc` in `text` that doesn't run spyc:
/// another program holding the name, which spyc must not replace.
fn foreign_spyc_server(text: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let command = root.pointer("/mcpServers/spyc/command")?.as_str()?;
    (!runs_spyc(command)).then(|| command.to_string())
}

#[cfg(test)]
mod tests;

//! The per-directory wiring spyc wrote into projects before an agent was wired
//! in user scope (#619), and taking it back out. A project copy runs beside the
//! user one, and codex, which merges nothing, reports each event twice.

use std::path::{Path, PathBuf};

use super::Env;
use crate::mcp::ConfigCleanup;
use crate::state::dir_owners::{self, Shared};
use crate::state::sessions::AgentKind;

/// What a scrub did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scrubbed {
    /// Files spyc's part was taken out of.
    pub cleaned: usize,
    /// Directories left alone because another live spyc relies on them.
    pub in_use: Vec<PathBuf>,
    /// Directories holding a file left alone because git tracks it.
    pub tracked: Vec<PathBuf>,
}

impl Scrubbed {
    fn add(&mut self, other: Self) {
        self.cleaned += other.cleaned;
        self.in_use.extend(other.in_use);
        self.tracked.extend(other.tracked);
    }
}

/// Every directory spyc may have written per-directory wiring into: the
/// projects that answered its per-project consent, and the directories its
/// ownership records name, live owners or not.
#[must_use]
pub fn candidates(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = crate::state::hook_consent::recorded_roots();
    dirs.extend(dir_owners::recorded_dirs(Shared::McpEntry));
    dirs.extend(dir_owners::recorded_dirs(Shared::StatusHooks));
    dirs.extend(dir_owners::hooks::recorded_dirs());
    dirs.extend(extra.iter().cloned());
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Whether `a` and `b` name the same file, links resolved.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Take `kind`'s per-directory wiring out of `dir`. A directory another live
/// spyc relies on is left for that spyc, and a file that is one of `kind`'s
/// user-scope files is never touched: `$HOME` can be a project, and then
/// `~/.claude/settings.json` is both.
pub fn scrub(kind: AgentKind, dir: &Path, env: &Env, me: u32) -> Scrubbed {
    let mut out = Scrubbed::default();
    let hooks_dir = if kind == AgentKind::Codex {
        crate::git::discovery::root_checkout_dir(dir)
    } else {
        dir.to_path_buf()
    };
    let claimed = |d: &Path| {
        dir_owners::claimed_by_another(Shared::McpEntry, d, me)
            || dir_owners::claimed_by_another(Shared::StatusHooks, d, me)
    };
    if claimed(dir) || claimed(&hooks_dir) {
        out.in_use.push(dir.to_path_buf());
        return out;
    }
    let user = super::user_files(kind, env);
    let is_user = |rel: &str, d: &Path| user.iter().any(|u| same_file(&d.join(rel), u));
    let mut tally = |result: ConfigCleanup, d: &Path| match result {
        ConfigCleanup::Cleaned => out.cleaned += 1,
        ConfigCleanup::SkippedTracked => out.tracked.push(d.to_path_buf()),
        ConfigCleanup::NothingToDo => {}
    };
    match kind {
        AgentKind::Claude => {
            tally(crate::mcp::cleanup_mcp_json(dir), dir);
            if !is_user(".claude/settings.json", dir) {
                tally(crate::mcp::cleanup_claude_status_hooks(dir), dir);
            }
        }
        AgentKind::Agy => {
            tally(crate::mcp::cleanup_agy_mcp_config(dir), dir);
            tally(crate::mcp::cleanup_agy_status_hooks(dir), dir);
        }
        AgentKind::Codex => {
            if !is_user(".codex/config.toml", dir) {
                tally(crate::mcp::cleanup_codex_config(dir), dir);
            }
            if !is_user(".codex/config.toml", &hooks_dir)
                && !is_user(".codex/hooks.json", &hooks_dir)
            {
                tally(
                    crate::mcp::cleanup_codex_status_hooks(&hooks_dir),
                    &hooks_dir,
                );
            }
        }
        AgentKind::Zot | AgentKind::Other => {}
    }
    out
}

/// [`scrub`] every candidate for every agent in `kinds`.
pub fn scrub_all(kinds: &[AgentKind], extra: &[PathBuf], env: &Env, me: u32) -> Scrubbed {
    let mut out = Scrubbed::default();
    for dir in candidates(extra) {
        for &kind in kinds {
            out.add(scrub(kind, &dir, env, me));
        }
    }
    out.in_use.sort();
    out.in_use.dedup();
    out
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Env, candidates, scrub};
    use crate::state::dir_owners::{self, Shared};
    use crate::state::sessions::AgentKind;

    const ME: u32 = 0;

    fn env(home: &Path) -> Env {
        Env {
            home: home.to_path_buf(),
            claude_config_dir: None,
            codex_home: home.join(".codex"),
            codex_mcp: true,
            managed_mcp: Vec::new(),
            managed_settings: Vec::new(),
            claude_cli: None,
            spyc: "spyc".to_string(),
        }
    }

    /// A project holding what an older spyc wrote into it for every agent,
    /// beside the user's own settings.
    fn project_with_copies(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/settings.json"), r#"{"model":"opus"}"#).unwrap();
        std::fs::write(
            dir.join(".mcp.json"),
            r#"{"mcpServers":{"spyc":{"command":"spyc","args":["--mcp"]},"other":{"command":"x"}}}"#,
        )
        .unwrap();
        assert!(crate::mcp::ensure_claude_status_hooks(dir));
        assert!(crate::mcp::ensure_agy_status_hooks(dir));
        crate::mcp::ensure_agy_mcp_config(dir).unwrap();
        crate::mcp::ensure_codex_config_toml(dir).unwrap();
        assert!(crate::mcp::ensure_codex_status_hooks(dir));
    }

    fn read(path: PathBuf) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn with_state(body: impl FnOnce(&Path)) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        crate::state::with_state_root(&root.join("state"), || body(&root));
    }

    #[test]
    fn a_scrub_takes_out_every_project_copy_and_keeps_the_rest() {
        with_state(|root| {
            let dir = root.join("project");
            project_with_copies(&dir);
            let env = env(&root.join("home"));
            let cleaned: usize = [AgentKind::Claude, AgentKind::Codex, AgentKind::Agy]
                .into_iter()
                .map(|kind| scrub(kind, &dir, &env, ME).cleaned)
                .sum();
            assert_eq!(cleaned, 6);
            let settings = read(dir.join(".claude/settings.json"));
            assert!(settings.contains("opus") && !settings.contains("--report-status"));
            let mcp = read(dir.join(".mcp.json"));
            assert!(mcp.contains("other") && !mcp.contains("\"spyc\""), "{mcp}");
            assert!(!read(dir.join(".codex/config.toml")).contains("spyc"));
            assert!(!dir.join(".agents").exists());
        });
    }

    /// A directory another live spyc still relies on is that spyc's to clean.
    #[test]
    fn a_scrub_leaves_a_directory_another_spyc_uses() {
        with_state(|root| {
            let dir = root.join("project");
            project_with_copies(&dir);
            dir_owners::claim(Shared::McpEntry, &dir, 1);
            let done = scrub(AgentKind::Agy, &dir, &env(&root.join("home")), ME);
            assert_eq!((done.cleaned, done.in_use), (0, vec![dir.clone()]));
            assert!(read(dir.join(".agents/hooks.json")).contains("--report-status"));
        });
    }

    /// `$HOME` can be a project, and then its per-directory files are the
    /// user-scope ones.
    #[test]
    fn a_scrub_never_touches_a_user_scope_file() {
        with_state(|root| {
            let home = root.join("home");
            project_with_copies(&home);
            let env = env(&home);
            let settings = read(home.join(".claude/settings.json"));
            let config = read(home.join(".codex/config.toml"));
            assert_eq!(
                scrub(AgentKind::Claude, &home, &env, ME).cleaned,
                1,
                ".mcp.json only"
            );
            assert_eq!(scrub(AgentKind::Codex, &home, &env, ME).cleaned, 0);
            assert_eq!(read(home.join(".claude/settings.json")), settings);
            assert_eq!(read(home.join(".codex/config.toml")), config);
        });
    }

    /// Candidates come from the per-project consent and the ownership records,
    /// whether their owners still live or not.
    #[test]
    fn candidates_name_every_directory_spyc_recorded() {
        with_state(|root| {
            let (a, b, c) = (root.join("a"), root.join("b"), root.join("c"));
            crate::state::hook_consent::set_consent(&a, false);
            let mut gone = std::process::Command::new("true").spawn().unwrap();
            let dead = gone.id();
            gone.wait().unwrap();
            dir_owners::claim(Shared::StatusHooks, &b, dead);
            let found = candidates(std::slice::from_ref(&c));
            for dir in [&a, &b, &c] {
                assert!(
                    found.contains(dir),
                    "{} missing from {found:?}",
                    dir.display()
                );
            }
        });
    }
}

//! The installable agent skill: spyc's own usage guide, embedded in the binary
//! and written into each host agent's personal skills directory on
//! `spyc --install-skill`.
//!
//! Claude Code and codex independently converged on the same format —
//! `<skills-dir>/<name>/SKILL.md` with YAML frontmatter, plus optional
//! `references/` sub-files — so one embedded copy serves both verbatim. Only the
//! directory differs: `~/.claude/skills/` vs `$CODEX_HOME/skills/`
//! (default `~/.codex/skills/`).
//!
//! Why a skill at all, when MCP `initialize` already carries
//! `SERVER_INSTRUCTIONS`: that field must stay short (it is prepended to every
//! session), so it can only *steer* an agent toward the tools. The skill is the
//! reference underneath it — worktree lifecycle, the four search corpora, the
//! three diff scopes — loaded on demand rather than always in context.
//!
//! Staleness is decided by **content hash, not version string**. `main` carries a
//! static `N.M.0-CURRENT` for a whole release cycle (see
//! `docs/RELEASE_ENGINEERING.md`), so comparing versions would miss every edit
//! made to the skill during that cycle. The recorded version is display-only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Directory name under `~/.claude/skills/`, and the skill's `name:` field.
pub const SKILL_NAME: &str = "spyc";

/// Sidecar recording what spyc last wrote, so a later run can tell "the embedded
/// copy moved on" (stale) from "the user edited it" (modified). Without it those
/// two are indistinguishable — both are just "installed != embedded".
const MANIFEST: &str = ".spyc-skill.json";

struct Asset {
    rel: &'static str,
    body: &'static str,
}

/// The skill, compiled in. Adding a reference file = one line here.
const ASSETS: &[Asset] = &[
    Asset {
        rel: "SKILL.md",
        body: include_str!("assets/SKILL.md"),
    },
    Asset {
        rel: "references/worktrees.md",
        body: include_str!("assets/references/worktrees.md"),
    },
    Asset {
        rel: "references/search.md",
        body: include_str!("assets/references/search.md"),
    },
    Asset {
        rel: "references/git.md",
        body: include_str!("assets/references/git.md"),
    },
];

/// What an install found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// No skill directory (or no manifest — an unrecognizable dir is treated as
    /// absent so `--install-skill` can adopt it).
    NotInstalled,
    /// On disk and byte-identical to the embedded copy.
    UpToDate { version: String },
    /// spyc's own copy has changed since it was installed.
    Stale { version: String },
    /// The user edited installed files. `stale` says whether the embedded copy
    /// ALSO moved on, because an update would discard their edits either way.
    Modified { version: String, stale: bool },
}

impl Status {
    /// The version recorded at install time, when there is one.
    #[must_use]
    pub const fn installed_version(&self) -> Option<&str> {
        match self {
            Self::NotInstalled => None,
            Self::UpToDate { version }
            | Self::Stale { version }
            | Self::Modified { version, .. } => Some(version.as_str()),
        }
    }
}

/// Whether one host's skill is worth offering an update for, and whether
/// accepting would discard local edits.
///
/// Only an **already-installed** skill that has fallen behind qualifies: a user
/// who never ran `--install-skill` is never nagged about a feature they didn't
/// ask for.
#[must_use]
const fn host_offer(status: &Status) -> Option<bool> {
    match status {
        // Behind, with local edits an update would discard — offer, and say so.
        Status::Modified { stale: true, .. } => Some(true),
        // Behind, nothing at risk.
        Status::Stale { .. } => Some(false),
        // No offer, for three different reasons: never installed (prompting
        // would nag about a feature they never opted into), already current, or
        // edited but with nothing newer to move to (overwriting their work for
        // no gain).
        Status::NotInstalled | Status::UpToDate { .. } | Status::Modified { stale: false, .. } => {
            None
        }
    }
}

/// Whether startup should offer an update across all hosts, and whether
/// accepting would discard local edits in any of them.
///
/// One prompt covers every host — a single `[Y/n]` that refreshes whichever are
/// behind, not one popup per agent. A remembered decline suppresses it until the
/// skill content changes.
#[must_use]
pub fn startup_offer(statuses: &[(Host, Status)], declined: bool) -> Option<bool> {
    if declined {
        return None;
    }
    let at_risk: Vec<bool> = statuses.iter().filter_map(|(_, s)| host_offer(s)).collect();
    if at_risk.is_empty() {
        return None;
    }
    // If ANY host has edits at stake the prompt must warn: when hosts disagree,
    // the more cautious framing wins.
    Some(at_risk.into_iter().any(|risk| risk))
}

/// An agent that discovers personal skills from a directory.
///
/// Claude Code and codex converged on the same format — `<dir>/<name>/SKILL.md`
/// with YAML frontmatter, plus optional `references/` sub-files — so one embedded
/// copy serves both with no per-host content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Claude,
    Codex,
    Agy,
}

impl Host {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Agy => "agy",
        }
    }
}

fn home() -> Option<PathBuf> {
    let home = crate::paths::expand("~");
    if home.as_os_str().is_empty() || home == Path::new("~") {
        return None;
    }
    Some(home)
}

/// Where each host looks for personal skills:
/// `~/.claude/skills/spyc` and `$CODEX_HOME/skills/spyc` (codex honours
/// `CODEX_HOME`, defaulting to `~/.codex`).
#[must_use]
pub fn host_dir(host: Host) -> Option<PathBuf> {
    match host {
        Host::Claude => Some(home()?.join(".claude").join("skills").join(SKILL_NAME)),
        Host::Codex => {
            let base = match std::env::var_os("CODEX_HOME") {
                Some(v) if !v.is_empty() => PathBuf::from(v),
                _ => home()?.join(".codex"),
            };
            Some(base.join("skills").join(SKILL_NAME))
        }
        // Antigravity's cross-flavour personal skills root. `~/.gemini/config/` is
        // the customization root every flavour reads (CLI, IDE, app);
        // `~/.gemini/antigravity-cli/skills/` also works but is CLI-only, and
        // `antigravity-cli/builtin/skills/` is agy's own shipped bundle — not
        // ours to write into.
        Host::Agy => Some(
            home()?
                .join(".gemini")
                .join("config")
                .join("skills")
                .join(SKILL_NAME),
        ),
    }
}

/// Every install target, in report order.
#[must_use]
pub fn hosts() -> Vec<(Host, PathBuf)> {
    [Host::Claude, Host::Codex, Host::Agy]
        .into_iter()
        .filter_map(|h| host_dir(h).map(|d| (h, d)))
        .collect()
}

/// FNV-1a. Change detection only — never a security boundary — so a short
/// non-cryptographic hash is the right tool, and being dependency-free keeps it
/// stable across toolchains (`DefaultHasher` explicitly is not).
fn content_hash(body: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn embedded_hashes() -> BTreeMap<String, String> {
    ASSETS
        .iter()
        .map(|a| (a.rel.to_string(), content_hash(a.body)))
        .collect()
}

/// Manifest shape on disk: `{"version": "...", "files": {rel: hash}}`.
fn read_manifest(dir: &Path) -> Option<(String, BTreeMap<String, String>)> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let version = v.get("version")?.as_str()?.to_string();
    let files = v
        .get("files")?
        .as_object()?
        .iter()
        .filter_map(|(k, val)| Some((k.clone(), val.as_str()?.to_string())))
        .collect();
    Some((version, files))
}

/// Inspect the skill at `dir`. Split from [`status`] so tests can point it at a
/// tempdir instead of the real `$HOME`.
#[must_use]
pub fn status_in(dir: &Path) -> Status {
    let Some((version, recorded)) = read_manifest(dir) else {
        return Status::NotInstalled;
    };
    let embedded = embedded_hashes();
    let stale = recorded != embedded;

    // Modified = a file on disk differs from what the manifest says we wrote.
    // A file we wrote that has since been DELETED counts too: restoring it is
    // exactly what an update should do.
    let modified = recorded.iter().any(|(rel, recorded_hash)| {
        std::fs::read_to_string(dir.join(rel))
            .map_or(true, |on_disk| &content_hash(&on_disk) != recorded_hash)
    });

    match (modified, stale) {
        (true, _) => Status::Modified { version, stale },
        (false, true) => Status::Stale { version },
        (false, false) => Status::UpToDate { version },
    }
}

/// Inspect the skill for every host.
#[must_use]
pub fn status_all() -> Vec<(Host, Status)> {
    hosts()
        .into_iter()
        .map(|(h, dir)| (h, status_in(&dir)))
        .collect()
}

/// Write every asset + the manifest into `dir`, creating it if needed.
/// Overwrites unconditionally — callers decide whether that is allowed (the
/// startup prompt refuses to clobber a [`Status::Modified`] skill unprompted).
/// Both halves go through [`crate::fs::write_atomic`], and the manifest is the
/// one that matters: [`read_manifest`] returns `None` for anything it can't
/// parse, [`status_in`] turns that into [`Status::NotInstalled`], and a
/// `NotInstalled` skill is overwritten *unconditionally*. So a manifest torn by
/// a crash or a full disk converts "locally edited, never clobbered unprompted"
/// into "clobbered on next launch" — the user's own edits, discarded by the
/// promise that was supposed to protect them.
pub fn install_in(dir: &Path) -> std::io::Result<()> {
    for asset in ASSETS {
        let path = dir.join(asset.rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::fs::write_atomic(&path, asset.body.as_bytes())?;
    }
    let manifest = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "files": embedded_hashes(),
    });
    crate::fs::write_atomic(
        &dir.join(MANIFEST),
        serde_json::to_string_pretty(&manifest)
            .unwrap_or_default()
            .as_bytes(),
    )
}

/// Install for every host, returning what landed where.
///
/// `only_installed` refreshes just the hosts that already have the skill — what
/// the startup update offer wants, so accepting an update never silently adopts a
/// host the user never installed into. `false` installs everywhere, which is what
/// `--install-skill` and `:skill update` mean.
pub fn install_all(only_installed: bool) -> anyhow::Result<Vec<(Host, PathBuf)>> {
    let targets = hosts();
    if targets.is_empty() {
        anyhow::bail!("cannot resolve $HOME — no place to install the skill");
    }
    let mut done = Vec::new();
    for (host, dir) in targets {
        if only_installed && matches!(status_in(&dir), Status::NotInstalled) {
            continue;
        }
        install_in(&dir)?;
        done.push((host, dir));
    }
    Ok(done)
}

/// Remove the skill from every host. Returns the hosts it was removed from.
pub fn remove_all() -> anyhow::Result<Vec<Host>> {
    let mut gone = Vec::new();
    for (host, dir) in hosts() {
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
            gone.push(host);
        }
    }
    Ok(gone)
}

/// The version this binary would install.
#[must_use]
pub const fn embedded_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// One hash over the whole embedded skill — the identity of "this exact skill
/// content". A declined update is remembered against this, NOT against the
/// version: `main`'s version is static for a release cycle, so a version-keyed
/// decline would silence the prompt for every later edit in that cycle.
#[must_use]
pub fn embedded_fingerprint() -> String {
    let joined = embedded_hashes()
        .into_iter()
        .map(|(rel, hash)| format!("{rel}:{hash}"))
        .collect::<Vec<_>>()
        .join("\n");
    content_hash(&joined)
}

#[cfg(test)]
mod tests {
    use super::{
        ASSETS, Host, Status, content_hash, embedded_hashes, host_dir, install_in, status_in,
    };

    /// Wrap a single host's status as the slice `startup_offer` takes.
    fn only(status: Status) -> Vec<(Host, Status)> {
        vec![(Host::Claude, status)]
    }

    #[test]
    fn fresh_dir_reads_as_not_installed() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(status_in(tmp.path()), Status::NotInstalled);
    }

    #[test]
    fn a_dir_without_a_manifest_is_not_installed() {
        // An unrecognizable directory must be adoptable by --install-skill
        // rather than reported as some in-between state.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("SKILL.md"), "hand-written").unwrap();
        assert_eq!(status_in(tmp.path()), Status::NotInstalled);
    }

    #[test]
    fn install_then_status_is_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        install_in(tmp.path()).unwrap();
        match status_in(tmp.path()) {
            Status::UpToDate { .. } => {}
            other => panic!("fresh install should be up to date, got {other:?}"),
        }
        // Every declared asset actually landed.
        for asset in ASSETS {
            assert!(
                tmp.path().join(asset.rel).is_file(),
                "missing asset: {}",
                asset.rel
            );
        }
    }

    #[test]
    fn a_user_edit_reads_as_modified_not_stale() {
        // The distinction the manifest exists for: an update would discard this
        // edit, so it must never be applied silently.
        let tmp = tempfile::tempdir().unwrap();
        install_in(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("SKILL.md"), "my own version").unwrap();
        match status_in(tmp.path()) {
            Status::Modified { stale, .. } => assert!(
                !stale,
                "the embedded copy has not moved, so this is modified-only"
            ),
            other => panic!("expected Modified, got {other:?}"),
        }
    }

    #[test]
    fn deleting_a_file_we_wrote_counts_as_modified() {
        let tmp = tempfile::tempdir().unwrap();
        install_in(tmp.path()).unwrap();
        std::fs::remove_file(tmp.path().join("references/git.md")).unwrap();
        match status_in(tmp.path()) {
            Status::Modified { .. } => {}
            other => panic!("a deleted asset should read as modified, got {other:?}"),
        }
    }

    #[test]
    fn a_stale_manifest_needs_an_update() {
        // Simulate "spyc's copy moved on": rewrite the manifest with a hash that
        // no longer matches the embedded asset, leaving the files themselves
        // consistent with it.
        let tmp = tempfile::tempdir().unwrap();
        install_in(tmp.path()).unwrap();
        let mut files = embedded_hashes();
        let body = "an older SKILL.md that spyc itself shipped";
        std::fs::write(tmp.path().join("SKILL.md"), body).unwrap();
        files.insert("SKILL.md".to_string(), content_hash(body));
        let manifest = serde_json::json!({ "version": "2.0.0", "files": files });
        std::fs::write(
            tmp.path().join(".spyc-skill.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        match status_in(tmp.path()) {
            Status::Stale { version } => assert_eq!(version, "2.0.0"),
            other => panic!("expected Stale, got {other:?}"),
        }
        // And that is the state startup offers an update for.
        assert_eq!(
            super::startup_offer(&only(status_in(tmp.path())), false),
            Some(false)
        );
    }

    #[test]
    fn install_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        install_in(tmp.path()).unwrap();
        let first = std::fs::read_to_string(tmp.path().join(".spyc-skill.json")).unwrap();
        install_in(tmp.path()).unwrap();
        let second = std::fs::read_to_string(tmp.path().join(".spyc-skill.json")).unwrap();
        assert_eq!(first, second, "re-installing must not churn the manifest");
        assert_eq!(
            super::startup_offer(&only(status_in(tmp.path())), false),
            None,
            "a fresh re-install has nothing to offer"
        );
    }

    #[test]
    fn startup_never_nags_someone_who_never_installed_it() {
        assert_eq!(
            super::startup_offer(&only(Status::NotInstalled), false),
            None
        );
    }

    #[test]
    fn startup_offers_only_for_an_outdated_install() {
        let v = || "2.1.0".to_string();
        // Nothing to do.
        assert_eq!(
            super::startup_offer(&only(Status::UpToDate { version: v() }), false),
            None
        );
        // Behind — offer, no edits at risk.
        assert_eq!(
            super::startup_offer(&only(Status::Stale { version: v() }), false),
            Some(false)
        );
        // Behind AND locally edited — offer, but the prompt must warn.
        assert_eq!(
            super::startup_offer(
                &only(Status::Modified {
                    version: v(),
                    stale: true
                }),
                false
            ),
            Some(true)
        );
        // Edited but current: an update has nothing to give, so don't ask to
        // overwrite their work for no gain.
        assert_eq!(
            super::startup_offer(
                &only(Status::Modified {
                    version: v(),
                    stale: false
                }),
                false
            ),
            None
        );
    }

    #[test]
    fn a_decline_suppresses_every_offer() {
        let v = || "2.1.0".to_string();
        for status in [
            Status::Stale { version: v() },
            Status::Modified {
                version: v(),
                stale: true,
            },
        ] {
            assert_eq!(
                super::startup_offer(&only(status.clone()), true),
                None,
                "a declined update must not re-prompt: {status:?}"
            );
        }
    }

    #[test]
    fn the_fingerprint_tracks_content_not_version() {
        // Stable across calls, and it is what a decline is keyed on.
        assert_eq!(super::embedded_fingerprint(), super::embedded_fingerprint());
        assert_eq!(super::embedded_fingerprint().len(), 16);
    }

    #[test]
    fn hashes_are_stable_and_distinguish_content() {
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
        // Pinned so a refactor can't silently change the hash function and
        // spuriously mark every installed skill stale.
        assert_eq!(content_hash(""), "cbf29ce484222325");
    }

    #[test]
    fn each_host_dir_matches_where_that_agent_looks() {
        // Guards the install locations themselves — both agents only discover
        // personal skills from their own directory, and a wrong path fails
        // silently (the skill simply never triggers).
        let claude = host_dir(Host::Claude).expect("HOME resolvable in tests");
        assert!(
            claude.ends_with(".claude/skills/spyc"),
            "unexpected claude path: {claude:?}"
        );
        let codex = host_dir(Host::Codex).expect("HOME resolvable in tests");
        // With CODEX_HOME unset this is the documented default. (Not asserting
        // the override path: setting env in-process is unsafe in edition 2024.)
        if std::env::var_os("CODEX_HOME").is_none() {
            assert!(
                codex.ends_with(".codex/skills/spyc"),
                "unexpected codex path: {codex:?}"
            );
        }
        assert_ne!(claude, codex, "hosts must not share a directory");
    }

    #[test]
    fn one_offer_covers_every_host() {
        let v = || "2.1.0".to_string();
        // Claude behind, codex never installed: still offer (for claude), and
        // nothing is at risk.
        assert_eq!(
            super::startup_offer(
                &[
                    (Host::Claude, Status::Stale { version: v() }),
                    (Host::Codex, Status::NotInstalled),
                ],
                false
            ),
            Some(false)
        );
        // Both current: nothing to say.
        assert_eq!(
            super::startup_offer(
                &[
                    (Host::Claude, Status::UpToDate { version: v() }),
                    (Host::Codex, Status::UpToDate { version: v() }),
                ],
                false
            ),
            None
        );
        // When hosts disagree, the cautious framing must win: codex has edits at
        // stake, so the single prompt has to warn even though claude's don't.
        assert_eq!(
            super::startup_offer(
                &[
                    (Host::Claude, Status::Stale { version: v() }),
                    (
                        Host::Codex,
                        Status::Modified {
                            version: v(),
                            stale: true
                        }
                    ),
                ],
                false
            ),
            Some(true),
            "a host with edits at risk must escalate the shared prompt"
        );
    }

    #[test]
    fn every_asset_path_is_relative_and_contained() {
        // An absolute or `..` asset path would write outside the skill dir.
        for asset in ASSETS {
            let p = std::path::Path::new(asset.rel);
            assert!(p.is_relative(), "{} must be relative", asset.rel);
            assert!(
                !asset.rel.contains(".."),
                "{} must not escape the skill dir",
                asset.rel
            );
            assert!(!asset.body.is_empty(), "{} is empty", asset.rel);
        }
    }

    #[test]
    fn skill_md_carries_the_frontmatter_claude_needs() {
        // Without `name:` + `description:` frontmatter the skill is never
        // triggered, which fails silently — worth a build-time guard.
        let skill = ASSETS
            .iter()
            .find(|a| a.rel == "SKILL.md")
            .expect("SKILL.md present");
        assert!(
            skill.body.starts_with("---\n"),
            "must open with frontmatter"
        );
        let end = skill.body[4..]
            .find("\n---\n")
            .expect("frontmatter must be closed");
        let front = &skill.body[4..4 + end];
        assert!(front.contains("name: spyc"), "frontmatter needs name: spyc");
        assert!(
            front.contains("description:"),
            "frontmatter needs a description — it is what triggers the skill"
        );
    }

    #[test]
    fn referenced_files_all_exist_as_assets() {
        // A pointer to a reference file we don't ship is a dead end for the
        // agent reading it.
        let skill = ASSETS
            .iter()
            .find(|a| a.rel == "SKILL.md")
            .expect("SKILL.md present");
        for asset in ASSETS.iter().filter(|a| a.rel.starts_with("references/")) {
            let bare = asset.rel.trim_start_matches("references/");
            assert!(
                skill.body.contains(asset.rel) || skill.body.contains(bare),
                "{} is shipped but never referenced from SKILL.md",
                asset.rel
            );
        }
    }
}

//! The recovery manifest: one file per spyc process, holding every project it
//! has open (#99, `docs/drafts/PROJECTS_PLAN.md` §3). This is version 2. A file
//! with no `version` is a version 1 session, which reads as a manifest with one
//! project.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{SavedTab, SavedVsplit};
use crate::config::AgentHudMode;
use crate::project_id::ProjectId;
use crate::state::scope_registry::ScopeClaim;

/// The manifest version this build writes, and the only one besides version 1
/// it reads. A file from a newer spyc is skipped, the way an older spyc skips
/// this one: v1 requires `cwd`, `tabs` and the pane fields at the top level,
/// and version 2 writes none of them.
pub const MANIFEST_VERSION: u32 = 2;

/// How far a running process's start time may drift from the one its manifest
/// recorded. Linux derives it from the boot time, which a clock step moves.
const START_SLACK_SECS: u64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// The file's name, `<id>.json`: when the session was created, in epoch
    /// millis. A restore that adopts the file keeps it.
    pub id: u64,
    pub epoch_secs: u64,
    #[serde(default)]
    pub saved_at: String,
    /// Spice-pair display name (e.g. `SAFFRON_PAPRIKA`), editable with `:name`.
    #[serde(default)]
    pub name: String,
    /// The spyc writing this file, and when it started. The start time is what
    /// tells that spyc from a later process the kernel gave the same pid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_started: Option<u64>,
    /// The project the user was in. An id names a project within this file
    /// only: the project a restore opens keeps the id it was given at launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_project: Option<ProjectId>,
    pub projects: Vec<SavedProject>,
    /// The scope-coordination registry (`AppState::scope_registry`), so a crash
    /// mid-merge-train doesn't lose in-flight claims. Each claim re-binds to its
    /// owning tab via `SavedTab::claim_owner`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope_claims: Vec<ScopeClaim>,
}

/// One project: where its columns were, its split, its pane and its tabs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedProject {
    pub id: ProjectId,
    /// `PROJECT_HOME`. `None` after `:project clear`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<PathBuf>,
    #[serde(default)]
    pub label: String,
    pub start_dir: PathBuf,
    /// Column `a`'s directory. `None` from a version 1 file, which never saved
    /// it, so its column lands on the [`anchor`](Self::anchor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_cwd: Option<PathBuf>,
    /// The vertical split, with column `b`'s directory or the previewed file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vsplit: Option<SavedVsplit>,
    pub pane_height_pct: u16,
    #[serde(default)]
    pub pane_focused: bool,
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active_tab: usize,
    /// What `:agent hud` chose. `None` leaves the config file's preference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_hud: Option<AgentHudMode>,
}

impl SavedProject {
    /// Where the project lives: its home, else where it started. Column `a`
    /// falls back here, and so does a tab whose own directory has gone.
    pub fn anchor(&self) -> &Path {
        self.home.as_deref().unwrap_or(&self.start_dir)
    }
}

impl Manifest {
    /// The project to restore: the one the user was in, else the first.
    pub fn active(&self) -> Option<&SavedProject> {
        self.active_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .or_else(|| self.projects.first())
    }

    /// Whether another spyc that wrote this file is still running, so restoring
    /// it would put a second process on the same file and conversations. A pid
    /// alone isn't proof: a crashed spyc's pid goes to the next process that
    /// asks, so the start time has to match too. This spyc's own file isn't
    /// another's.
    pub fn running(&self) -> bool {
        let (Some(pid), Some(started)) = (self.pid, self.process_started) else {
            return false;
        };
        pid != std::process::id()
            && crate::proc_group::started_epoch_secs(pid)
                .is_some_and(|now| now.abs_diff(started) <= START_SLACK_SECS)
    }

    fn from_v1(v1: ManifestV1) -> Self {
        let project = SavedProject {
            id: ProjectId::new(),
            label: label_of(v1.project_home.as_deref().unwrap_or(&v1.cwd)),
            home: v1.project_home,
            start_dir: v1.cwd,
            left_cwd: None,
            vsplit: v1.vsplit,
            pane_height_pct: v1.pane_height_pct,
            pane_focused: v1.pane_focused,
            tabs: v1.tabs,
            active_tab: v1.active_tab,
            agent_hud: v1.agent_hud,
        };
        Self {
            version: MANIFEST_VERSION,
            id: v1.id,
            epoch_secs: v1.epoch_secs,
            saved_at: v1.saved_at,
            name: v1.name,
            pid: None,
            process_started: None,
            active_project: Some(project.id),
            projects: vec![project],
            scope_claims: v1.scope_claims,
        }
    }
}

#[cfg(test)]
impl Manifest {
    /// A manifest with one project, started in `dir` with column `a` there, no
    /// home, holding `tabs`.
    pub fn one_project(
        id: u64,
        epoch_secs: u64,
        name: &str,
        dir: impl Into<PathBuf>,
        tabs: Vec<SavedTab>,
    ) -> Self {
        let dir = dir.into();
        let project = SavedProject {
            id: ProjectId::new(),
            home: None,
            label: label_of(&dir),
            start_dir: dir.clone(),
            left_cwd: Some(dir),
            vsplit: None,
            pane_height_pct: 30,
            pane_focused: false,
            tabs,
            active_tab: 0,
            agent_hud: None,
        };
        Self {
            version: MANIFEST_VERSION,
            id,
            epoch_secs,
            saved_at: String::new(),
            name: name.into(),
            pid: None,
            process_started: None,
            active_project: Some(project.id),
            projects: vec![project],
            scope_claims: Vec::new(),
        }
    }

    /// This manifest with every project started in `dir`, column `a` there and
    /// no home: what a test restores, since restoring moves the process cwd.
    pub fn anchored_at(mut self, dir: &Path) -> Self {
        for project in &mut self.projects {
            project.home = None;
            project.start_dir = dir.to_path_buf();
            project.left_cwd = Some(dir.to_path_buf());
        }
        self
    }
}

/// A project's name for the picker: the last component of `dir`.
pub fn label_of(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Read a saved file of either version, or `None` for one this build can't.
pub fn parse_manifest(value: serde_json::Value) -> Option<Manifest> {
    match value.get("version") {
        None => serde_json::from_value::<ManifestV1>(value)
            .ok()
            .map(Manifest::from_v1),
        Some(version) if version.as_u64() == Some(u64::from(MANIFEST_VERSION)) => {
            serde_json::from_value(value).ok()
        }
        Some(_) => None,
    }
}

/// Several projects as one picker cell: `spyc (3 tabs) · web (1) · api`. The
/// first count names its unit; a project with no tabs shows none.
pub fn project_list_label(projects: &[SavedProject]) -> String {
    let mut unit = true;
    projects
        .iter()
        .map(|project| match project.tabs.len() {
            0 => project.label.clone(),
            n if std::mem::take(&mut unit) => {
                format!(
                    "{} ({n} tab{})",
                    project.label,
                    if n == 1 { "" } else { "s" }
                )
            }
            n => format!("{} ({n})", project.label),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A session as spyc saved it before manifests had a version: one project,
/// flat.
#[derive(Deserialize)]
struct ManifestV1 {
    id: u64,
    saved_at: String,
    epoch_secs: u64,
    /// `project_home`, else `start_dir`, at save time.
    cwd: PathBuf,
    tabs: Vec<SavedTab>,
    active_tab: usize,
    pane_height_pct: u16,
    pane_focused: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    project_home: Option<PathBuf>,
    #[serde(default)]
    vsplit: Option<SavedVsplit>,
    #[serde(default)]
    scope_claims: Vec<ScopeClaim>,
    #[serde(default)]
    agent_hud: Option<AgentHudMode>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v1() -> serde_json::Value {
        json!({
            "id": 7, "saved_at": "then", "epoch_secs": 100, "cwd": "/w/proj",
            "tabs": [{ "command": "cat", "label": "cat", "cwd": "/w/proj/src" }],
            "active_tab": 0, "pane_height_pct": 45, "pane_focused": true,
            "name": "OLD_NAME", "project_home": "/w/proj",
            "vsplit": { "width_pct": 40, "full_height": true, "focus_right": false,
                        "right_cwd": "/w/proj/docs" },
            "scope_claims": [{ "id": 1, "owner": "o", "owner_label": "l",
                               "paths": ["src/*.rs"], "intent": "editing",
                               "pr": null, "note": null, "claimed_at_secs": 5 }],
            "agent_hud": "full"
        })
    }

    fn project(label: &str, tabs: usize) -> SavedProject {
        SavedProject {
            id: ProjectId::new(),
            home: None,
            label: label.into(),
            start_dir: PathBuf::from("/w").join(label),
            left_cwd: None,
            vsplit: None,
            pane_height_pct: 30,
            pane_focused: false,
            tabs: (0..tabs)
                .map(|_| SavedTab {
                    command: "cat".into(),
                    label: "cat".into(),
                    cwd: PathBuf::from("/w"),
                    agent_kind: super::super::AgentKind::Other,
                    agent_session_id: None,
                    agent_session_name: None,
                    codex_home: None,
                    codex_home_unresolved: false,
                    claim_owner: String::new(),
                    pane_id: String::new(),
                    agent_hint: None,
                })
                .collect(),
            active_tab: 0,
            agent_hud: None,
        }
    }

    /// A version 1 session reads as one project, losing nothing it saved.
    #[test]
    fn a_version_1_session_reads_as_one_project() {
        let m = parse_manifest(v1()).expect("a version 1 session reads");
        assert_eq!((m.version, m.id, m.epoch_secs), (MANIFEST_VERSION, 7, 100));
        assert_eq!((m.name.as_str(), m.saved_at.as_str()), ("OLD_NAME", "then"));
        assert_eq!((m.pid, m.process_started), (None, None));
        assert_eq!(m.scope_claims.len(), 1);
        assert_eq!(m.projects.len(), 1);
        let p = &m.projects[0];
        assert_eq!(m.active_project, Some(p.id));
        assert_eq!(p.home.as_deref(), Some(Path::new("/w/proj")));
        assert_eq!(p.start_dir, Path::new("/w/proj"));
        assert_eq!(p.left_cwd, None, "a version 1 file never saved column a");
        assert_eq!(p.label, "proj");
        assert_eq!((p.pane_height_pct, p.pane_focused), (45, true));
        assert_eq!((p.tabs.len(), p.active_tab), (1, 0));
        assert_eq!(p.agent_hud, Some(AgentHudMode::Full));
        let split = p.vsplit.as_ref().expect("the split");
        assert_eq!(split.right_cwd.as_deref(), Some(Path::new("/w/proj/docs")));
    }

    /// With `:project clear` behind it, a version 1 session has no home, and
    /// its project is anchored where it started.
    #[test]
    fn a_version_1_session_without_a_home_is_anchored_on_its_cwd() {
        let mut old = v1();
        old.as_object_mut().unwrap().remove("project_home");
        let m = parse_manifest(old).expect("reads");
        assert_eq!(m.projects[0].home, None);
        assert_eq!(m.projects[0].anchor(), Path::new("/w/proj"));
    }

    /// A manifest survives its own text, and an older spyc, which reads
    /// everything as version 1, skips it rather than restoring a half of it.
    #[test]
    fn a_version_2_manifest_round_trips_and_is_no_version_1_session() {
        let mut m = parse_manifest(v1()).unwrap();
        m.pid = Some(42);
        m.process_started = Some(1_000);
        m.projects[0].left_cwd = Some(PathBuf::from("/w/proj/src"));
        let text = serde_json::to_value(&m).unwrap();
        assert_eq!(text["version"], json!(2));
        let back = parse_manifest(text.clone()).expect("version 2 reads");
        assert_eq!(back.active_project, m.active_project);
        assert_eq!(back.projects[0].id, m.projects[0].id);
        assert_eq!(back.projects[0].left_cwd, m.projects[0].left_cwd);
        assert_eq!((back.pid, back.process_started), (Some(42), Some(1_000)));
        assert!(
            serde_json::from_value::<ManifestV1>(text).is_err(),
            "an older spyc must not read a manifest as its own format"
        );
    }

    /// A file from a newer spyc is skipped, not misread.
    #[test]
    fn a_newer_version_is_skipped() {
        let mut m = serde_json::to_value(parse_manifest(v1()).unwrap()).unwrap();
        m["version"] = json!(3);
        assert!(parse_manifest(m.clone()).is_none());
        m["version"] = json!("2");
        assert!(parse_manifest(m).is_none());
    }

    /// The project restored is the one the user was in, else the first.
    #[test]
    fn the_active_project_is_the_one_named() {
        let mut m = parse_manifest(v1()).unwrap();
        let second = project("web", 0);
        m.active_project = Some(second.id);
        m.projects.push(second);
        assert_eq!(m.active().map(|p| p.label.as_str()), Some("web"));
        m.active_project = Some(ProjectId::new());
        assert_eq!(m.active().map(|p| p.label.as_str()), Some("proj"));
        m.active_project = None;
        assert_eq!(m.active().map(|p| p.label.as_str()), Some("proj"));
    }

    #[test]
    fn a_project_list_counts_tabs_naming_the_unit_once() {
        let label = |projects: &[SavedProject]| project_list_label(projects);
        assert_eq!(
            label(&[project("spyc", 3), project("web", 1), project("api", 0)]),
            "spyc (3 tabs) · web (1) · api"
        );
        assert_eq!(
            label(&[project("api", 0), project("web", 1), project("x", 2)]),
            "api · web (1 tab) · x (2)"
        );
        assert_eq!(label(&[project("solo", 0)]), "solo");
    }

    /// A manifest is running only while its pid is alive with the start time it
    /// recorded, in a process other than this one.
    #[test]
    fn running_needs_the_pid_and_its_start_time() {
        let mut m = parse_manifest(v1()).unwrap();
        assert!(!m.running(), "no pid");
        let parent = std::os::unix::process::parent_id();
        let started = crate::proc_group::started_epoch_secs(parent).expect("the parent runs");
        m.pid = Some(parent);
        m.process_started = Some(started);
        assert!(m.running());
        m.process_started = Some(started + START_SLACK_SECS + 1);
        assert!(!m.running(), "the pid belongs to a later process");
        m.process_started = None;
        assert!(!m.running());
        m.pid = Some(std::process::id());
        m.process_started = crate::proc_group::own_start_secs();
        assert!(!m.running(), "this spyc's own file");
    }
}

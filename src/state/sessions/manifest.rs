//! The recovery manifest: one file per spyc process, holding every project it
//! has open (#99, `docs/archive/PROJECTS_PLAN.md` §3). This is version 3, the
//! shape of version 2 with the jar records (`jar_socket`, `tasks`, `live`)
//! that a spyc with no jars must never read. A version 2 file reads as one of
//! this version, and a file with no `version` is a version 1 session, which
//! reads as a manifest with one project.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{AgentKind, SavedColumn, SavedTab, SavedVsplit};
use crate::config::AgentHudMode;
use crate::project_id::ProjectId;
use crate::state::scope_registry::ScopeClaim;

/// The manifest version this build writes. It reads this one, version 2 and
/// version 1. A file from a newer spyc is skipped, the way an older spyc skips
/// this one: v1 requires `cwd`, `tabs` and the pane fields at the top level,
/// and versions 2 and 3 write none of them.
///
/// H1 (review 3.0): 2.4.0, the release before jars, reads version 2 and
/// restores each tab by starting its program again. A file that may name a
/// tab's jar is therefore never version 2, so 2.4.0 skips it rather than run
/// every program a second time beside its jar.
pub const MANIFEST_VERSION: u32 = 3;

/// The version before jars, which this build still reads.
const BEFORE_JARS: u32 = 2;

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
    /// The projects this spyc closed lately, newest first, one per anchor, for
    /// the switcher to reopen. A 2.3 build before them ignores the field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed_projects: Vec<ClosedProject>,
    /// The `!` captures and background tasks whose programs run in jars, for
    /// a restore to take back (S2c). A build before them ignores the field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<SavedTask>,
}

/// A capture or background task whose program runs in a jar.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SavedTask {
    pub title: String,
    pub cmd_display: String,
    /// Its jar's socket, whose jar a restore takes back; never one started
    /// afresh.
    pub jar_socket: std::path::PathBuf,
}

/// A project a spyc closed, as it was when it closed, kept so the switcher can
/// reopen it with its tabs (`docs/archive/PROJECTS_PLAN.md` §3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedProject {
    /// When it closed, in seconds since the epoch.
    pub closed_secs: u64,
    pub project: SavedProject,
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
    /// Column `a`'s cursor, picks and filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_column: Option<SavedColumn>,
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
    /// What `:setenv -p` saved here, as plain text; a plain `:setenv` never
    /// reaches the file.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
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
            left_column: None,
            pane_height_pct: v1.pane_height_pct,
            pane_focused: v1.pane_focused,
            tabs: v1.tabs,
            active_tab: v1.active_tab,
            agent_hud: v1.agent_hud,
            env: BTreeMap::new(),
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
            closed_projects: Vec::new(),
            tasks: Vec::new(),
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
            left_column: None,
            pane_height_pct: 30,
            pane_focused: false,
            tabs,
            active_tab: 0,
            agent_hud: None,
            env: BTreeMap::new(),
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
            closed_projects: Vec::new(),
            tasks: Vec::new(),
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

/// What other spycs' agents are in, which a restore must not resume into: a
/// second process in a conversation splits it.
#[derive(Debug, Default)]
pub struct Held {
    /// Each conversation saved by its uuid, with its agent.
    ids: HashSet<(AgentKind, String)>,
    /// Each conversation saved by a name, claude's, with its agent and the
    /// directory the name is claude's in: a name is unique only within it.
    names: HashSet<(AgentKind, PathBuf, String)>,
    /// Each directory an agent runs in, with its agent and codex home: a
    /// resume with no conversation to name picks that agent's newest there.
    dirs: HashSet<(AgentKind, PathBuf, Option<PathBuf>)>,
}

impl Held {
    /// What the tabs of every running spyc in `manifests` are in. A manifest
    /// no spyc runs holds nothing, and this spyc's own file isn't another's
    /// ([`Manifest::running`]). A record a spyc couldn't open runs nothing.
    pub fn by_running(manifests: &[Manifest]) -> Self {
        let mut held = Self::default();
        let tabs = manifests
            .iter()
            .filter(|manifest| manifest.running())
            .flat_map(|manifest| &manifest.projects)
            .flat_map(|project| &project.tabs)
            .filter(|tab| !tab.unopened);
        for tab in tabs {
            held.add(tab);
        }
        held
    }

    /// Record `tab`'s agent as running.
    fn add(&mut self, tab: &SavedTab) {
        let ids = [&tab.agent_session_id, &tab.agent_session_name]
            .into_iter()
            .flatten()
            .map(String::as_str);
        self.add_running(
            tab.effective_kind(),
            ids,
            &tab.cwd,
            tab.codex_home.as_deref(),
        );
    }

    /// Record an agent of `kind` as running in `cwd` under `codex_home`, in
    /// the conversations `ids` names, each by its uuid or by a name. A tab
    /// running no agent is in nothing.
    pub fn add_running<'a>(
        &mut self,
        kind: AgentKind,
        ids: impl IntoIterator<Item = &'a str>,
        cwd: &Path,
        codex_home: Option<&Path>,
    ) {
        if kind == AgentKind::Other {
            return;
        }
        for id in ids {
            if super::is_uuid(id) {
                self.ids.insert((kind, id.to_owned()));
            } else {
                self.names.insert((kind, cwd.to_path_buf(), id.to_owned()));
            }
        }
        self.dirs
            .insert((kind, cwd.to_path_buf(), codex_home.map(Path::to_path_buf)));
    }

    /// Whether resuming `tab` in `cwd` puts it in something held: the
    /// conversation it saved, by uuid or by a name, or, with none saved, its
    /// agent's newest in `cwd`, which an agent of that kind and codex home
    /// running there may be in.
    pub fn holds(&self, tab: &SavedTab, cwd: &Path) -> bool {
        let kind = tab.effective_kind();
        if tab.agent_session_id.is_none() {
            return self
                .dirs
                .contains(&(kind, cwd.to_path_buf(), tab.codex_home.clone()));
        }
        [&tab.agent_session_id, &tab.agent_session_name]
            .into_iter()
            .flatten()
            .any(|id| {
                if super::is_uuid(id) {
                    self.ids.contains(&(kind, id.clone()))
                } else {
                    self.names.contains(&(kind, cwd.to_path_buf(), id.clone()))
                }
            })
    }
}

/// A project's name for the picker: the last component of `dir`.
pub fn label_of(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Read a saved file of any version this build knows, as one of
/// [`MANIFEST_VERSION`], or `None` for one it can't.
pub fn parse_manifest(value: serde_json::Value) -> Option<Manifest> {
    match value.get("version") {
        None => serde_json::from_value::<ManifestV1>(value)
            .ok()
            .map(Manifest::from_v1),
        Some(version)
            if [MANIFEST_VERSION, BEFORE_JARS]
                .iter()
                .any(|v| version.as_u64() == Some(u64::from(*v))) =>
        {
            serde_json::from_value::<Manifest>(value)
                .ok()
                .map(|m| Manifest {
                    version: MANIFEST_VERSION,
                    ..m
                })
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

    fn agent_tab(
        kind: &str,
        id: Option<&str>,
        name: Option<&str>,
        cwd: &str,
        home: Option<&str>,
    ) -> SavedTab {
        serde_json::from_value(json!({
            "command": kind, "label": kind, "cwd": cwd, "agent_kind": kind,
            "agent_session_id": id, "agent_session_name": name, "codex_home": home
        }))
        .expect("fixture")
    }

    const U1: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";
    const U2: &str = "019e8b21-9e7c-7553-a118-d1cdada725fe";

    /// A conversation is held by its uuid wherever it is resumed, and by
    /// claude's name for it within the directory the name is claude's in,
    /// either saved in the id's place or beside it; nothing else of that
    /// agent's, or of another agent's, is.
    #[test]
    fn a_conversation_is_held_by_its_uuid_or_its_name() {
        let mut held = Held::default();
        held.add(&agent_tab("claude", Some(U1), Some("fix-auth"), "/w", None));
        held.add(&agent_tab("claude", Some("named"), None, "/w", None));
        let (at, elsewhere) = (Path::new("/w"), Path::new("/v"));
        let claude = |id, name| agent_tab("claude", Some(id), name, "/w", None);
        assert!(held.holds(&claude(U1, None), elsewhere), "a uuid anywhere");
        assert!(held.holds(&claude("fix-auth", None), at));
        assert!(
            held.holds(&claude(U2, Some("named")), at),
            "its name beside its id"
        );
        assert!(
            !held.holds(&claude("fix-auth", None), elsewhere),
            "another directory's name"
        );
        assert!(!held.holds(&claude(U2, None), at), "its own conversation");
        let codex = agent_tab("codex", Some(U1), None, "/w", None);
        assert!(!held.holds(&codex, at), "another agent's");
    }

    /// A tab with no conversation saved is held where an agent of its kind and
    /// codex home runs in the directory it spawns in, which needn't be the one
    /// it saved.
    #[test]
    fn an_idless_tab_is_held_by_its_agents_directory_and_home() {
        let mut held = Held::default();
        held.add(&agent_tab("codex", Some(U1), None, "/w", Some("/h")));
        let idless = agent_tab("codex", None, None, "/gone", Some("/h"));
        assert!(held.holds(&idless, Path::new("/w")), "it spawns in /w");
        assert!(!held.holds(&idless, Path::new("/v")));
        let other_home = agent_tab("codex", None, None, "/w", Some("/h2"));
        assert!(
            !held.holds(&other_home, Path::new("/w")),
            "another home's newest"
        );
        let zot = agent_tab("zot", None, None, "/w", None);
        assert!(!held.holds(&zot, Path::new("/w")), "no zot runs there");
        held.add(&agent_tab("zot", None, None, "/w", None));
        assert!(
            held.holds(&zot, Path::new("/w")),
            "zot's `--continue` would join it"
        );
    }

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
            left_column: None,
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
                    claude_home: None,
                    claim_owner: String::new(),
                    pane_id: String::new(),
                    agent_hint: None,
                    unopened: false,
                    jar_socket: None,
                    live: None,
                })
                .collect(),
            active_tab: 0,
            agent_hud: None,
            env: BTreeMap::new(),
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
    fn a_manifest_round_trips_and_is_no_version_1_session() {
        let mut m = parse_manifest(v1()).unwrap();
        m.pid = Some(42);
        m.process_started = Some(1_000);
        m.projects[0].left_cwd = Some(PathBuf::from("/w/proj/src"));
        let text = serde_json::to_value(&m).unwrap();
        assert_eq!(text["version"], json!(MANIFEST_VERSION));
        let back = parse_manifest(text.clone()).expect("its own version reads");
        assert_eq!(back.active_project, m.active_project);
        assert_eq!(back.projects[0].id, m.projects[0].id);
        assert_eq!(back.projects[0].left_cwd, m.projects[0].left_cwd);
        assert_eq!((back.pid, back.process_started), (Some(42), Some(1_000)));
        assert!(
            serde_json::from_value::<ManifestV1>(text).is_err(),
            "an older spyc must not read a manifest as its own format"
        );
    }

    /// A version 2 file, as 2.4.0 and the rcs before H1 wrote it, still
    /// reads, jar records and all, and is saved again as this version.
    #[test]
    fn a_version_2_manifest_reads_as_this_version() {
        let mut text = serde_json::to_value(parse_manifest(v1()).unwrap()).unwrap();
        text["version"] = json!(2);
        let m = parse_manifest(text).expect("version 2 reads");
        assert_eq!(m.version, MANIFEST_VERSION);
        assert_eq!(m.projects.len(), 1);
        assert_eq!(
            serde_json::to_value(&m).unwrap()["version"],
            json!(MANIFEST_VERSION)
        );
    }

    /// A file from a newer spyc is skipped, not misread.
    #[test]
    fn a_newer_version_is_skipped() {
        let mut m = serde_json::to_value(parse_manifest(v1()).unwrap()).unwrap();
        m["version"] = json!(MANIFEST_VERSION + 1);
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

    /// H1: every manifest spyc writes is this version, which 2.4.0 skips:
    /// each `Manifest` built in `src` (and each `Self` in its own `impl`)
    /// sets `version` to [`MANIFEST_VERSION`], and nothing sets it to
    /// anything else afterwards. Parsed, not read as text. Fails closed.
    #[test]
    fn every_manifest_written_is_this_version() {
        use syn::visit::Visit;
        /// Whether `attrs` hold `#[cfg(test)]`: the parsed form of
        /// `guard_support::production_half`, whose text cut can leave a file syn can't read.
        fn test_only(attrs: &[syn::Attribute]) -> bool {
            attrs.iter().any(|a| {
                a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
            })
        }

        /// An item's attributes, for the kinds that carry `#[cfg(test)]` here.
        fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
            match item {
                syn::Item::Fn(i) => &i.attrs,
                syn::Item::Mod(i) => &i.attrs,
                syn::Item::Impl(i) => &i.attrs,
                syn::Item::Const(i) => &i.attrs,
                syn::Item::Static(i) => &i.attrs,
                syn::Item::Use(i) => &i.attrs,
                syn::Item::Struct(i) => &i.attrs,
                syn::Item::Enum(i) => &i.attrs,
                syn::Item::Trait(i) => &i.attrs,
                syn::Item::Type(i) => &i.attrs,
                syn::Item::Macro(i) => &i.attrs,
                _ => &[],
            }
        }
        #[derive(Default)]
        struct Writers {
            in_manifest_impl: bool,
            built: usize,
            wrong: Vec<String>,
        }
        fn is_version_constant(expr: &syn::Expr) -> bool {
            matches!(expr, syn::Expr::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "MANIFEST_VERSION"))
        }
        impl<'a> Visit<'a> for Writers {
            fn visit_item(&mut self, item: &'a syn::Item) {
                if !test_only(item_attrs(item)) {
                    syn::visit::visit_item(self, item);
                }
            }
            fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
                if !test_only(&f.attrs) {
                    syn::visit::visit_impl_item_fn(self, f);
                }
            }
            fn visit_item_impl(&mut self, item: &'a syn::ItemImpl) {
                let manifest = matches!(&*item.self_ty, syn::Type::Path(t) if t.path.segments.last().is_some_and(|s| s.ident == "Manifest"));
                let was = std::mem::replace(&mut self.in_manifest_impl, manifest);
                syn::visit::visit_item_impl(self, item);
                self.in_manifest_impl = was;
            }
            fn visit_expr_struct(&mut self, expr: &'a syn::ExprStruct) {
                let name = expr.path.segments.last().map(|s| s.ident.to_string());
                let manifest = name.as_deref() == Some("Manifest")
                    || (name.as_deref() == Some("Self") && self.in_manifest_impl);
                if manifest {
                    self.built += 1;
                    let version = expr
                        .fields
                        .iter()
                        .find(|f| matches!(&f.member, syn::Member::Named(n) if n == "version"));
                    if !version.is_some_and(|f| is_version_constant(&f.expr)) {
                        self.wrong.push(quote_line(expr));
                    }
                }
                syn::visit::visit_expr_struct(self, expr);
            }
            fn visit_expr_assign(&mut self, expr: &'a syn::ExprAssign) {
                if let syn::Expr::Field(field) = &*expr.left
                    && matches!(&field.member, syn::Member::Named(n) if n == "version")
                    && !is_version_constant(&expr.right)
                {
                    self.wrong.push("an assignment to `.version`".into());
                }
                syn::visit::visit_expr_assign(self, expr);
            }
        }
        fn quote_line(expr: &syn::ExprStruct) -> String {
            format!(
                "a {} built at line {}",
                expr.path
                    .segments
                    .last()
                    .map_or_default(|s| s.ident.to_string()),
                expr.path
                    .segments
                    .first()
                    .map_or(0, |s| s.ident.span().start().line)
            )
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut writers = Writers::default();
        let mut dirs = vec![root];
        let mut wrong = Vec::new();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    if !matches!(name.as_str(), "harness_tests" | "tests") {
                        dirs.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|x| x != "rs")
                    || name == "tests.rs"
                    || name.ends_with("_tests.rs")
                {
                    continue;
                }
                let src = std::fs::read_to_string(&path).unwrap();
                let file =
                    syn::parse_file(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                if test_only(&file.attrs) {
                    continue;
                }
                writers.wrong.clear();
                writers.visit_file(&file);
                wrong.extend(
                    writers
                        .wrong
                        .iter()
                        .map(|w| format!("{}: {w}", path.display())),
                );
            }
        }
        assert!(
            writers.built >= 4,
            "premise: the scan finds the writers ({})",
            writers.built
        );
        assert!(
            wrong.is_empty(),
            "a manifest written at another version: {wrong:#?}"
        );
    }
}

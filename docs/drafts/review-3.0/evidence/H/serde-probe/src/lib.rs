pub mod v24 {
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectId(pub uuid::Uuid);
impl Serialize for ProjectId { fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.collect_str(&self.0) } }
impl<'de> Deserialize<'de> for ProjectId { fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> { let t = String::deserialize(d)?; t.parse().map(Self).map_err(serde::de::Error::custom) } }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHudMode { #[default] Off, Compact, Full }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeIntent { Editing, Merging }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScopeClaim { pub id: u64, pub owner: String, pub owner_label: String, pub paths: Vec<String>, pub intent: ScopeIntent, pub pr: Option<String>, pub note: Option<String>, pub claimed_at_secs: u64 }
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind { Claude, Codex, Agy, Zot, #[default] Other }
impl<'de> Deserialize<'de> for AgentKind { fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> { Ok(match String::deserialize(d)?.as_str() { "claude" => Self::Claude, "codex" => Self::Codex, "agy" => Self::Agy, "zot" => Self::Zot, _ => Self::Other }) } }
const fn is_false(v: &bool) -> bool { !*v }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab { 
    pub command: String, pub label: String, pub cwd: PathBuf,
    #[serde(default)] pub agent_kind: AgentKind,
    #[serde(default, alias = "claude_session_id", skip_serializing_if = "Option::is_none")] pub agent_session_id: Option<String>,
    #[serde(default, alias = "claude_session_name", skip_serializing_if = "Option::is_none")] pub agent_session_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub codex_home: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "is_false")] pub codex_home_unresolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub claude_home: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub claim_owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_hint: Option<AgentKind>,
    #[serde(default, skip_serializing_if = "is_false")] pub unopened: bool,
 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedVsplit { pub width_pct: u16, pub full_height: bool, pub focus_right: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub preview_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub right_cwd: Option<PathBuf> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedProject {
    pub id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub home: Option<PathBuf>,
    #[serde(default)] pub label: String,
    pub start_dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub left_cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub vsplit: Option<SavedVsplit>,
    
    pub pane_height_pct: u16,
    #[serde(default)] pub pane_focused: bool,
    #[serde(default)] pub tabs: Vec<SavedTab>,
    #[serde(default)] pub active_tab: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_hud: Option<AgentHudMode>,
    
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedProject { pub closed_secs: u64, pub project: SavedProject }


pub const MANIFEST_VERSION: u32 = 2;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32, pub id: u64, pub epoch_secs: u64,
    #[serde(default)] pub saved_at: String,
    #[serde(default)] pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub process_started: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub active_project: Option<ProjectId>,
    pub projects: Vec<SavedProject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub scope_claims: Vec<ScopeClaim>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub closed_projects: Vec<ClosedProject>,
    
}
pub fn parse_manifest(value: serde_json::Value) -> Option<Manifest> {
    match value.get("version") {
        None => None, // v1 path elided: not under test
        Some(v) if v.as_u64() == Some(u64::from(MANIFEST_VERSION)) => serde_json::from_value(value).map_err(|e| eprintln!("parse error: {e}")).ok(),
        Some(_) => None,
    }
}

}
pub mod m52 {

use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectId(pub uuid::Uuid);
impl Serialize for ProjectId { fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.collect_str(&self.0) } }
impl<'de> Deserialize<'de> for ProjectId { fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> { let t = String::deserialize(d)?; t.parse().map(Self).map_err(serde::de::Error::custom) } }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHudMode { #[default] Off, Compact, Full }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeIntent { Editing, Merging }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScopeClaim { pub id: u64, pub owner: String, pub owner_label: String, pub paths: Vec<String>, pub intent: ScopeIntent, pub pr: Option<String>, pub note: Option<String>, pub claimed_at_secs: u64 }
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind { Claude, Codex, Agy, Zot, #[default] Other }
impl<'de> Deserialize<'de> for AgentKind { fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> { Ok(match String::deserialize(d)?.as_str() { "claude" => Self::Claude, "codex" => Self::Codex, "agy" => Self::Agy, "zot" => Self::Zot, _ => Self::Other }) } }
const fn is_false(v: &bool) -> bool { !*v }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab { 
    pub command: String, pub label: String, pub cwd: PathBuf,
    #[serde(default)] pub agent_kind: AgentKind,
    #[serde(default, alias = "claude_session_id", skip_serializing_if = "Option::is_none")] pub agent_session_id: Option<String>,
    #[serde(default, alias = "claude_session_name", skip_serializing_if = "Option::is_none")] pub agent_session_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub codex_home: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "is_false")] pub codex_home_unresolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub claude_home: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub claim_owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")] pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_hint: Option<AgentKind>,
    #[serde(default, skip_serializing_if = "is_false")] pub unopened: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")] pub jar_socket: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub live: Option<SavedLive>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedLive { pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub blocked_on: Option<String>,
    pub left_ms: u64 }
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedColumn {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub picks: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub filter: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedVsplit { pub width_pct: u16, pub full_height: bool, pub focus_right: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub preview_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub right_cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub right_column: Option<SavedColumn> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTask { pub title: String, pub cmd_display: String, pub jar_socket: PathBuf }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedProject {
    pub id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub home: Option<PathBuf>,
    #[serde(default)] pub label: String,
    pub start_dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub left_cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub vsplit: Option<SavedVsplit>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub left_column: Option<SavedColumn>,
    pub pane_height_pct: u16,
    #[serde(default)] pub pane_focused: bool,
    #[serde(default)] pub tabs: Vec<SavedTab>,
    #[serde(default)] pub active_tab: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_hud: Option<AgentHudMode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")] pub env: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedProject { pub closed_secs: u64, pub project: SavedProject }


pub const MANIFEST_VERSION: u32 = 2;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32, pub id: u64, pub epoch_secs: u64,
    #[serde(default)] pub saved_at: String,
    #[serde(default)] pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub process_started: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub active_project: Option<ProjectId>,
    pub projects: Vec<SavedProject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub scope_claims: Vec<ScopeClaim>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub closed_projects: Vec<ClosedProject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub tasks: Vec<SavedTask>,
}
pub fn parse_manifest(value: serde_json::Value) -> Option<Manifest> {
    match value.get("version") {
        None => None, // v1 path elided: not under test
        Some(v) if v.as_u64() == Some(u64::from(MANIFEST_VERSION)) => serde_json::from_value(value).map_err(|e| eprintln!("parse error: {e}")).ok(),
        Some(_) => None,
    }
}

}


/// Copied from 52ce15b7 src/state/sessions/mod.rs.
pub fn prune_victims(mut files: Vec<(std::path::PathBuf, std::time::SystemTime)>, keep: &std::path::Path, max: usize) -> Vec<std::path::PathBuf> {
    let excess = files.len().saturating_sub(max);
    files.retain(|(path, _)| path != keep);
    files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    files.truncate(excess);
    files.into_iter().map(|(path, _)| path).collect()
}
#[cfg(test)]
mod tests {
    /// A detached session idle for days is the least recently written file, so
    /// the 21st file any spyc saves deletes it, though its spyc still runs.
    #[test]
    fn an_idle_running_sessions_file_is_pruned_by_another_spycs_save() {
        use std::time::{Duration, UNIX_EPOCH};
        let t = |s| UNIX_EPOCH + Duration::from_secs(s);
        let detached = std::path::PathBuf::from("/s/1000.json");
        let mut files = vec![(detached.clone(), t(1_000))];
        for i in 0..20u64 { files.push((format!("/s/{}.json", 2_000 + i).into(), t(2_000 + i))); }
        let just_saved = std::path::PathBuf::from("/s/2019.json");
        assert_eq!(crate::prune_victims(files, &just_saved, 20), vec![detached]);
    }

    use serde_json::json;
    fn main_shape() -> serde_json::Value {
        // A manifest as 52ce15b7's save_released writes it: one claude tab and one
        // shell tab in jars, a task in a jar, column cursor and a carried report.
        let pid = uuid::Uuid::now_v7().to_string();
        json!({
          "version": 2, "id": 1764000000000u64, "epoch_secs": 1764000000u64,
          "saved_at": "2026-11-24 10:00", "name": "ANISE_CLOVE",
          "active_project": pid,
          "projects": [{
            "id": pid, "home": "/w/proj", "label": "proj", "start_dir": "/w/proj",
            "left_cwd": "/w/proj", "left_column": {"cursor": "Cargo.toml"},
            "pane_height_pct": 40, "pane_focused": true, "active_tab": 0,
            "tabs": [
              {"command": "claude", "label": "claude", "cwd": "/w/proj", "agent_kind": "claude",
               "agent_session_id": "019e8b21-9e7c-7553-a118-d1cdada725fd", "pane_id": "tab-1",
               "jar_socket": "/tmp/u/spyc/jars/0a1b2c3d4e5f6a7b.sock",
               "live": {"status": "blocked", "blocked_on": "permission", "left_ms": 60000}},
              {"command": "sh", "label": "sh", "cwd": "/w/proj", "pane_id": "tab-2",
               "jar_socket": "/tmp/u/spyc/jars/1111111111111111.sock"}
            ]
          }],
          "tasks": [{"title": "make test", "cmd_display": "make test",
                     "jar_socket": "/tmp/u/spyc/jars/2222222222222222.sock"}]
        })
    }

    /// 2.4.0 reads a 3.0 manifest whole and sees no jar: its restore would start
    /// both tabs' programs afresh, and its next save drops every jar_socket and the task.
    #[test]
    fn v24_reads_a_main_manifest_and_sees_no_jar() {
        let sample = main_shape();
        let m52 = crate::m52::parse_manifest(sample.clone()).expect("main reads its own");
        assert_eq!(m52.projects[0].tabs[0].jar_socket.as_deref().map(|p| p.to_str().unwrap()), Some("/tmp/u/spyc/jars/0a1b2c3d4e5f6a7b.sock"));
        let old = crate::v24::parse_manifest(sample).expect("2.4.0 reads a main manifest");
        assert_eq!(old.projects[0].tabs.len(), 2);
        assert_eq!(old.projects[0].tabs[0].command, "claude");
        let resaved = serde_json::to_string(&old).unwrap();
        assert!(!resaved.contains("jar_socket"), "{resaved}");
        assert!(!resaved.contains("tasks"), "{resaved}");
        assert!(!resaved.contains("live"), "{resaved}");
        println!("2.4.0 re-save: {resaved}");
    }

    /// A future field is ignored by main's reader, and lost on its next save.
    #[test]
    fn main_drops_a_field_it_does_not_know_on_its_next_save() {
        let mut sample = main_shape();
        sample["projects"][0]["tabs"][0]["jar_role"] = json!("owner");
        sample["future_top"] = json!({"x": 1});
        let m = crate::m52::parse_manifest(sample).expect("an unknown field is ignored");
        let resaved = serde_json::to_string(&m).unwrap();
        assert!(!resaved.contains("jar_role") && !resaved.contains("future_top"));
    }

    /// One new variant in a derived enum anywhere in the file loses the whole session.
    #[test]
    fn one_unknown_variant_loses_the_whole_session() {
        let mut hud = main_shape();
        hud["projects"][0]["agent_hud"] = json!("minimal");
        assert!(crate::m52::parse_manifest(hud).is_none(), "an unknown HUD mode");
        let mut intent = main_shape();
        intent["scope_claims"] = json!([{ "id": 1, "owner": "o", "owner_label": "l", "paths": ["src/*.rs"],
            "intent": "reviewing", "pr": null, "note": null, "claimed_at_secs": 5 }]);
        assert!(crate::m52::parse_manifest(intent).is_none(), "an unknown scope intent");
        let mut kind = main_shape();
        kind["projects"][0]["tabs"][1]["agent_kind"] = json!("gemini");
        assert!(crate::m52::parse_manifest(kind).is_some(), "an unknown agent kind degrades");
        let mut ver = main_shape();
        ver["version"] = json!(3);
        assert!(crate::m52::parse_manifest(ver).is_none(), "a newer version");
    }

    /// A newer reader with a field missing: everything but version, id, epoch_secs,
    /// projects and, per project, id, start_dir and pane_height_pct, defaults.
    #[test]
    fn main_reads_a_2_4_manifest() {
        let mut sample = main_shape();
        for t in sample["projects"][0]["tabs"].as_array_mut().unwrap() { t.as_object_mut().unwrap().remove("jar_socket"); t.as_object_mut().unwrap().remove("live"); }
        sample.as_object_mut().unwrap().remove("tasks");
        sample["projects"][0].as_object_mut().unwrap().remove("left_column");
        let m = crate::m52::parse_manifest(sample).expect("main reads 2.4.0's shape");
        assert!(m.projects[0].tabs.iter().all(|t| t.jar_socket.is_none()));
    }
}

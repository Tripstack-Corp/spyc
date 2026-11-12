//! Jar spike, stage 6: a project moves from one spyc to another (U13).
//!
//! `:detach project` hands each of the project in view's jars on, held for a
//! token (R22), writes the project as a save keeps it with those tokens into
//! the state directory's `moves/`, has the MCP server close its agents'
//! connections, and drops the project without signalling its programs.
//! `:attach project` in another spyc opens the newest move there: each tab
//! presents its jar's token as it adopts it, and its agents' proxies find the
//! new owner through their jars (R30a). A move file is removed only once the
//! receiving spyc's saved session holds the project, so one that dies partway
//! leaves the move file, or its own session, to try again from.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::App;
use crate::state::sessions::SavedProject;

/// A project on its way between spycs.
#[derive(Serialize, Deserialize)]
struct Move {
    from_pid: u32,
    at_secs: u64,
    project: SavedProject,
    /// Each moved tab's pane id, and the token its jar is held for.
    tokens: BTreeMap<String, String>,
    /// The scope claims the project's agents hold: they go with them.
    #[serde(default)]
    claims: Vec<crate::state::scope_registry::ScopeClaim>,
}

fn moves_dir() -> Option<PathBuf> {
    crate::state::state_root().map(|r| r.join("moves"))
}

/// A token for one jar: random, so only the spyc handed the move file can
/// claim the jar.
fn token() -> Option<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .ok()?
        .read_exact(&mut b)
        .ok()?;
    Some(crate::pane::jar_backend::hex(&b))
}

impl App {
    /// `:detach project`.
    pub(super) fn detach_project(&mut self) {
        if !crate::pane::jar_backend::enabled() {
            self.state
                .flash_error("detaching a project needs its tabs in jars (SPYC_JARS=1)");
            return;
        }
        if self.state.open_projects().nth(1).is_none() {
            self.state
                .flash_error("the last project can't be detached: open another first");
            return;
        }
        let Some(dir) = moves_dir() else {
            self.state
                .flash_error("no state directory to leave the project in");
            return;
        };
        let id = self.state.active;
        let label = self.state.label_of(id).unwrap_or_default();
        let snapshot = self.closed_snapshot(id);
        let mut tokens = BTreeMap::new();
        for tab in &snapshot.project.tabs {
            let Some(t) = token() else { continue };
            if crate::pane::jar_backend::release(&tab.pane_id, t.as_bytes()) {
                tokens.insert(tab.pane_id.clone(), t);
            }
        }
        crate::mcp::mark_moved(tokens.keys().cloned());
        let owners: std::collections::HashSet<&str> = snapshot
            .project
            .tabs
            .iter()
            .map(|t| t.claim_owner.as_str())
            .collect();
        let (claims, kept) = std::mem::take(&mut self.state.scope_registry)
            .into_iter()
            .partition(|c| owners.contains(c.owner.as_str()));
        self.state.scope_registry = kept;
        let moved = Move {
            from_pid: std::process::id(),
            at_secs: crate::sysinfo::epoch_secs(),
            project: snapshot.project.clone(),
            tokens,
            claims,
        };
        let written = (|| -> anyhow::Result<PathBuf> {
            use std::io::Write as _;
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)?;
            let path = dir.join(format!("{}-{}.json", moved.at_secs, std::process::id()));
            let mut f = std::fs::File::options()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            f.write_all(&serde_json::to_vec_pretty(&moved)?)?;
            Ok(path)
        })();
        let Some(to) = self.landing_after(id) else {
            return;
        };
        self.switch_project(to);
        self.drop_project(id);
        self.state.forget_closed(snapshot.project.anchor());
        match written {
            Ok(_) => self.state.flash_info(format!(
                "detached project {label}: {} tab(s) wait for another spyc's :attach project",
                moved.tokens.len()
            )),
            Err(e) => self.state.flash_error(format!(
                "detached project {label}, but its move wasn't saved: {e:#}"
            )),
        }
    }

    /// `:attach project`: the newest project another spyc detached.
    pub(super) fn attach_project(&mut self) {
        let newest = moves_dir()
            .and_then(|d| std::fs::read_dir(d).ok())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .max();
        let Some(path) = newest else {
            self.state.flash_error("no detached project to attach");
            return;
        };
        let moved = std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|b| serde_json::from_slice::<Move>(&b).map_err(anyhow::Error::from));
        let moved = match moved {
            Ok(m) => m,
            Err(e) => {
                self.state.flash_error(format!("{}: {e:#}", path.display()));
                return;
            }
        };
        for (pane, t) in &moved.tokens {
            crate::pane::jar_backend::expect_claim(pane, t.as_bytes());
        }
        // The moved tabs adopt their running agents, so no conversation is
        // held against them.
        let held = crate::state::sessions::Held::default();
        let Some(opened) = self.restore_out_of_view(&moved.project, false, &held) else {
            self.state
                .flash_error("the detached project could not be opened here");
            return;
        };
        self.switch_project(opened.id);
        self.record_open_roots();
        // Each claim keeps its id, which its agent may release it by, unless
        // a claim here already has it.
        for mut claim in moved.claims.clone() {
            if self.state.scope_registry.iter().any(|c| c.id == claim.id) {
                claim.id = self
                    .state
                    .scope_registry
                    .iter()
                    .map(|c| c.id)
                    .max()
                    .unwrap_or(0)
                    + 1;
            }
            self.state.scope_registry.push(claim);
        }
        // The move file goes only once this spyc's own session holds the
        // project: at every moment one of the two says where its jars belong,
        // so a spyc that dies right after attaching leaves `spyc -r` a way
        // back to them.
        let manifest = self.build_manifest();
        if crate::state::sessions::save_session(&manifest).is_ok() {
            let _ = std::fs::remove_file(&path);
            self.state.flash_info(format!(
                "attached a project from spyc {}: {} tab(s)",
                moved.from_pid,
                moved.tokens.len()
            ));
        } else {
            self.state.flash_error(
                "attached the project, but kept its move file: this session couldn't be saved",
            );
        }
    }
}

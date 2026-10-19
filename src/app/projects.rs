//! Opening a project and moving between the open ones (#99): what the
//! leader's project keys do — `Space n`, `Space Space`, `Space 1-9`, and
//! `Space ]` / `Space [`.

use std::path::Path;

use anyhow::Context as _;

use super::state::{self, ProjectId};
use super::view_state::ProjectView;
use super::{App, Config, UserKeymap};
use crate::keymap::ProjectAction;

impl App {
    /// Go to another open project. With one project open these do nothing,
    /// and say nothing, so a user who never opens a second sees no change.
    pub(super) fn move_between_projects(&mut self, action: ProjectAction) {
        let ids: Vec<ProjectId> = self.state.open_projects().map(|p| p.id).collect();
        if ids.len() < 2 {
            return;
        }
        let here = ids
            .iter()
            .position(|id| *id == self.state.active)
            .unwrap_or(0);
        let to = match action {
            ProjectAction::Last => self
                .state
                .last_project
                .filter(|id| self.state.has_project(*id)),
            ProjectAction::Next => Some(ids[(here + 1) % ids.len()]),
            ProjectAction::Prev => Some(ids[(here + ids.len() - 1) % ids.len()]),
            ProjectAction::ByIndex(n) => {
                let to = usize::from(n).checked_sub(1).and_then(|i| ids.get(i));
                if to.is_none() {
                    self.state
                        .flash_error(format!("no project {n}: {} are open", ids.len()));
                }
                to.copied()
            }
            ProjectAction::JumpHome | ProjectAction::SetHomeHere | ProjectAction::Open => None,
        };
        if let Some(id) = to {
            self.switch_project(id);
        }
    }

    /// `Space n`'s prompt submitted: open `typed`, `~` expanded, as a project.
    pub(super) fn open_project_typed(&mut self, typed: &str) {
        let typed = typed.trim();
        if typed.is_empty() {
            return;
        }
        let path = match typed.strip_prefix('~') {
            Some(rest) => crate::config::home_dir()
                .unwrap_or_default()
                .join(rest.trim_start_matches('/')),
            None => std::path::PathBuf::from(typed),
        };
        match std::fs::canonicalize(&path).with_context(|| format!("opening {}", path.display())) {
            Ok(dir) if dir.is_dir() => self.open_project(&dir),
            Ok(dir) => self
                .state
                .flash_error(format!("project: {} is not a directory", dir.display())),
            Err(e) => self.state.flash_error(format!("project: {e:#}")),
        }
    }

    /// Open `dir`, a canonical directory, as a project and go to it, starting
    /// its startup tabs as a launch there would. A directory an open project
    /// is anchored at, or started in, is that project's, so it is gone to
    /// instead: one directory is one project's.
    pub(super) fn open_project(&mut self, dir: &Path) {
        let open = self
            .state
            .open_projects()
            .find(|p| p.home.as_deref().unwrap_or(&p.start_dir) == dir || p.start_dir == dir)
            .map(|p| p.id);
        if let Some(id) = open {
            self.switch_project(id);
            return;
        }
        let (id, problem) = self.create_project(dir);
        self.switch_project(id);
        self.write_context();
        let label = self.state.label_of(id).unwrap_or_default();
        self.state.flash_info(format!("opened project {label}"));
        self.start_startup_tabs(false);
        if let Some(problem) = problem {
            self.state.flash_error(problem);
        }
    }

    /// A new project homed and started at `dir`, out of view, and what was
    /// wrong with its config, if anything:
    /// - its config read from `dir` as a launch there reads it, else the
    ///   defaults;
    /// - its keymap that config's bindings and `init.lua`'s maps;
    /// - column `a` at `dir`, unread, since the switch to it reads it on a
    ///   worker, with its repo root, harpoon and branch resolved now, so the
    ///   switch's git walk has a root to walk.
    fn create_project(&mut self, dir: &Path) -> (ProjectId, Option<String>) {
        let (config, problem) = match Config::load_default(dir) {
            Ok(config) => (config, None),
            Err(e) => (Config::default(), Some(format!("config error: {e:#}"))),
        };
        let mut user_keymap = UserKeymap::from_bindings(config.bindings.clone());
        self.install_lua_maps(&mut user_keymap);
        let left = state::Commander::unread_at(dir, &config);
        let project = state::Project::new(
            Some(dir.to_path_buf()),
            left,
            dir.to_path_buf(),
            config,
            user_keymap,
            super::bootstrap::initial_pane(),
        );
        let id = project.id;
        self.state.insert_project(project);
        self.runtime.insert_project(id);
        let context = crate::context::process_context_path(dir, id);
        self.view
            .insert_project(id, ProjectView::new(context, true));
        self.in_project(id, |app| {
            app.state.update_repo_root(state::Side::Left, dir);
            app.reconcile_harpoon();
            app.state.col_mut(state::Side::Left).git.info =
                app.state.compute_git_info_fast(state::Side::Left);
        });
        (id, problem)
    }
}

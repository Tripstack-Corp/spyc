//! A project's home and start directory, and the rule that one directory is
//! one open project's home (#99), so which project a directory belongs to is
//! never in doubt.

use std::path::{Path, PathBuf};

use super::{AppState, Project, ProjectId};

impl AppState {
    /// The open project other than `except` whose home is `dir`: one directory
    /// is one project's home, so which project a directory belongs to is
    /// never in doubt. A project with no home (`:project clear`) is anchored
    /// at its `start_dir`.
    pub fn home_taken(&self, dir: &Path, except: ProjectId) -> Option<ProjectId> {
        self.open_projects()
            .find(|p| p.id != except && p.home.as_deref().unwrap_or(&p.start_dir) == dir)
            .map(|p| p.id)
    }

    /// Re-home the active project to `dir`, refused when another open project
    /// has it ([`Self::home_taken`]).
    pub fn set_project_home(&mut self, dir: PathBuf) {
        if self.refuse_taken(&dir) {
            return;
        }
        self.flash_info(format!("PROJECT_HOME: {}", dir.display()));
        self.active_project_mut().home = Some(dir);
    }

    /// Clear the active project's home, leaving it anchored at its start
    /// directory: refused when that is another open project's home.
    pub fn clear_project_home(&mut self) {
        let start = self.active_project().start_dir.clone();
        if self.refuse_taken(&start) {
            return;
        }
        self.active_project_mut().home = None;
        self.flash_info("PROJECT_HOME cleared");
    }

    /// Move the active project's start directory (`gS`, `:startdir`). A
    /// project with no home is anchored there, so for one that is refused when
    /// `dir` is another open project's home.
    pub fn move_start_dir(&mut self, dir: PathBuf) {
        if self.active_project().home.is_none() && self.refuse_taken(&dir) {
            return;
        }
        self.flash_info(format!("start dir: {}", dir.display()));
        self.active_project_mut().start_dir = dir;
    }

    /// Whether `dir` is another open project's home, saying so when it is.
    fn refuse_taken(&mut self, dir: &Path) -> bool {
        let Some(other) = self.home_taken(dir, self.active) else {
            return false;
        };
        let label = self.project(other).map(Project::label).unwrap_or_default();
        self.flash_error(format!(
            "{} is already project {label}'s home",
            dir.display()
        ));
        true
    }
}

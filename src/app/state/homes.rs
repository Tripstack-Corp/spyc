//! A project's home and start directory, and the rules that one directory is
//! one open project's home and one open project's start dir (#99), so which
//! project a directory belongs to is never in doubt.

use std::path::{Path, PathBuf};

use super::{AppState, Project, ProjectId};

impl AppState {
    /// Project `id`'s label, taken from its home, else its start dir; for a
    /// project closed this session, the label it had.
    pub fn label_of(&self, id: ProjectId) -> Option<String> {
        self.project(id)
            .map(Project::label)
            .or_else(|| self.closed_labels.get(&id).cloned())
    }

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
        if let Some(taken) = self.anchored_elsewhere(&dir) {
            self.flash_error(format!("{} is already {taken}", dir.display()));
            return;
        }
        self.flash_info(format!("PROJECT_HOME: {}", dir.display()));
        self.active_project_mut().home = Some(dir);
    }

    /// Clear the active project's home, leaving it anchored at its start
    /// directory: refused when that is another open project's home.
    pub fn clear_project_home(&mut self) {
        let start = self.active_project().start_dir.clone();
        if let Some(taken) = self.anchored_elsewhere(&start) {
            self.flash_error(format!(
                "PROJECT_HOME not cleared: it would fall back to the start dir {}, which is \
                 already {taken}",
                start.display()
            ));
            return;
        }
        self.active_project_mut().home = None;
        self.flash_info("PROJECT_HOME cleared");
    }

    /// Move the active project's start directory (`gS`, `:startdir`), refused
    /// onto another open project's: a start dir is the root an agent's cwd
    /// binds to its project by (`App::project_holding`), so two projects there
    /// would leave which one an agent belongs to to their order. A project
    /// with no home is anchored there too, so for one it is also refused onto
    /// another project's home.
    pub fn move_start_dir(&mut self, dir: PathBuf) {
        let taken = match self.start_taken(&dir) {
            Some(other) => Some(format!("project {}'s start dir", other.label())),
            None if self.active_project().home.is_none() => self.anchored_elsewhere(&dir),
            None => None,
        };
        if let Some(taken) = taken {
            self.flash_error(format!("{} is already {taken}", dir.display()));
            return;
        }
        self.flash_info(format!("start dir: {}", dir.display()));
        self.active_project_mut().start_dir = dir;
    }

    /// The open project other than the active one whose start dir is `dir`.
    fn start_taken(&self, dir: &Path) -> Option<&Project> {
        self.open_projects()
            .find(|p| p.id != self.active && p.start_dir == dir)
    }

    /// What another open project has `dir` as ([`Self::home_taken`]), said
    /// for a refusal: its home, or the start dir a project with no home is
    /// anchored at.
    fn anchored_elsewhere(&self, dir: &Path) -> Option<String> {
        let other = self.project(self.home_taken(dir, self.active)?)?;
        let label = other.label();
        Some(match other.home {
            Some(_) => format!("project {label}'s home"),
            None => format!("project {label}'s start dir (it has no home)"),
        })
    }
}

//! Opening a project, closing one, and moving between the open ones (#99):
//! what the leader's project keys do — `Space n`, `Space x`, `Space j`,
//! `Space Space`, `Space 1-9`, and `Space ]` / `Space [` — and how the dumps
//! that cover every project head and name them.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;

use super::state::{self, ProjectId};
use super::view_state::ProjectView;
use super::{App, Config, Mode, Pane, PaneTabs, Prompt, PromptKind, UserKeymap};
use crate::keymap::ProjectAction;
use crate::mcp_cmd::McpResponse;
use crate::ui::{pager, project_chips};
use ratatui::text::{Line, Span};

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
            ProjectAction::JumpHome
            | ProjectAction::SetHomeHere
            | ProjectAction::Open
            | ProjectAction::Close
            | ProjectAction::List
            | ProjectAction::Attention => None,
        };
        if let Some(id) = to {
            self.switch_project(id);
        }
    }

    /// Each open project in switcher order, headed for a dump that covers them
    /// all (`project web`, the one in view marked), with its tabs if it has
    /// any. A dump draws the headings only while several are open.
    pub(super) fn tabs_by_project(&self) -> Vec<(String, Option<&PaneTabs>)> {
        let tabs: HashMap<ProjectId, &PaneTabs> = self.runtime.every_tab_set().collect();
        self.state
            .open_projects()
            .map(|p| {
                let mark = if p.id == self.state.active {
                    " (in view)"
                } else {
                    ""
                };
                (
                    format!("project {}{mark}", p.label()),
                    tabs.get(&p.id).copied(),
                )
            })
            .collect()
    }

    /// ` in web` for a dump line about something in project `id`, while
    /// several projects are open, and ` in web (closed)` once it has closed;
    /// nothing with one open.
    pub(super) fn in_project_note(&self, id: ProjectId) -> String {
        match self.state.label_of(id) {
            Some(label) if !self.state.has_project(id) => format!(" in {label} (closed)"),
            Some(label) if self.state.open_projects().nth(1).is_some() => format!(" in {label}"),
            _ => String::new(),
        }
    }

    /// `Space j`: list the open projects in switcher order, to go to one. Each
    /// row is numbered, with its agents' glyph as its chip in the status bar
    /// has it, how many tabs it has and its home; the one in view is marked
    /// and highlighted. `j`/`k` and Enter, or `1`-`9`, pick
    /// (`switch_to_pending_project`).
    pub(super) fn open_project_switcher(&mut self) {
        self.clear_pending_pickers();
        let label_w = self
            .state
            .open_projects()
            .map(|p| crate::ui::display_width(&p.label()))
            .max()
            .unwrap_or(0);
        let tabs: HashMap<ProjectId, usize> = self
            .runtime
            .every_tab_set()
            .map(|(id, tabs)| (id, tabs.tabs().len()))
            .collect();
        let active = self.state.active;
        let attention = self.project_attention();
        let pulse = self.spicy_pulse_color(self.view.agent_anim_frame);
        let several = self.state.open_projects().nth(1).is_some();
        let (ids, lines): (Vec<ProjectId>, Vec<Line<'static>>) = self
            .state
            .open_projects()
            .enumerate()
            .map(|(i, p)| {
                let tabs = tabs.get(&p.id).copied().unwrap_or(0);
                let glyph = several.then(|| {
                    project_chips::cell(attention.get(&p.id).copied(), pulse, &self.view.theme)
                });
                let row = project_row(i, p, label_w, tabs, p.id == active, glyph);
                (p.id, row)
            })
            .unzip();
        let start = ids.iter().position(|id| *id == active).unwrap_or(0);
        self.state.pending_projects = Some(ids);
        let mut view = pager::PagerView::new_styled(
            "projects — j/k+Enter or 1-9 to go, / to search, q to close",
            lines,
        );
        view.picker_cursor = Some(start);
        view.no_history = true;
        view.show_line_numbers = false;
        view.wrap = false;
        self.set_pager(view);
    }

    /// Go to the switcher's row `idx` and close the switcher, the pager of the
    /// project being left. Past the end of the list nothing happens, and the
    /// switcher stays open.
    pub(super) fn switch_to_pending_project(&mut self, idx: usize) {
        let Some(id) = self
            .state
            .pending_projects
            .as_ref()
            .and_then(|ids| ids.get(idx))
            .copied()
        else {
            return;
        };
        self.clear_pager();
        self.state.pending_projects = None;
        self.view.needs_full_repaint = true;
        if self.state.has_project(id) {
            self.switch_project(id);
        }
    }

    /// `Space n`'s prompt submitted: open `typed` as a project, read as `J`
    /// and Tab completion read a path: `~` and `$VAR` expanded, and a
    /// relative one against the focused column.
    pub(super) fn open_project_typed(&mut self, typed: &str) {
        let typed = typed.trim();
        if typed.is_empty() {
            return;
        }
        let path = crate::paths::expand(typed);
        let path = if path.is_absolute() {
            path
        } else {
            self.state.cur().listing.dir.join(path)
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
        if let Some(id) = self.project_at(dir) {
            if id == self.state.active {
                let label = self.state.label_of(id).unwrap_or_default();
                self.state.flash_info(format!("already in project {label}"));
            }
            self.switch_project(id);
            return;
        }
        let (id, problem) = self.create_project(dir, dir);
        self.switch_project(id);
        self.write_context();
        let label = self.state.label_of(id).unwrap_or_default();
        self.state.flash_info(format!("opened project {label}"));
        self.start_startup_tabs(false);
        if let Some(problem) = problem {
            self.state.flash_error(problem);
        }
    }

    /// The open project anchored at, or started in, `dir`, a canonical
    /// directory. A project's own home and start dir are compared canonical
    /// too, since either can name `dir` through a symlink.
    fn project_at(&self, dir: &Path) -> Option<ProjectId> {
        let names_dir = |p: &Path| p == dir || std::fs::canonicalize(p).is_ok_and(|p| p == dir);
        self.state
            .open_projects()
            .find(|p| names_dir(p.anchor()) || names_dir(&p.start_dir))
            .map(|p| p.id)
    }

    /// A new project homed and started at `dir`, out of view, and what was
    /// wrong with its config, if anything:
    /// - its config read from `dir` as a launch there reads it, with the
    ///   user's own file for `anchor` (the home a restore gives it, else
    ///   `dir`), else the defaults;
    /// - its keymap that config's bindings and `init.lua`'s maps;
    /// - column `a` at `dir`, unread, since the switch to it reads it on a
    ///   worker, with its repo root, harpoon and branch resolved now, so the
    ///   switch's git walk has a root to walk.
    pub(super) fn create_project(
        &mut self,
        dir: &Path,
        anchor: &Path,
    ) -> (ProjectId, Option<String>) {
        let files = crate::config::project_file::scan();
        let (config, problem) = match Config::load_for_project(dir, anchor, &files) {
            Ok(config) => {
                let warned = (!config.warnings.is_empty())
                    .then(|| format!("config: {}", config.warnings.join("; ")));
                (config, warned)
            }
            Err(e) => (Config::default(), Some(format!("config error: {e:#}"))),
        };
        let mut user_keymap = UserKeymap::from_bindings(config.bindings.clone());
        self.install_lua_maps(&mut user_keymap);
        let left = state::Commander::unread_at(dir, &config);
        let mut project = state::Project::new(
            Some(dir.to_path_buf()),
            left,
            dir.to_path_buf(),
            config,
            user_keymap,
            super::bootstrap::initial_pane(),
        );
        project.config_key = Some((dir.to_path_buf(), anchor.to_path_buf()));
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

    /// `Space x`: close the project in view. The only project open isn't
    /// closed from the leader, since `:q` closes it, nor one from inside the
    /// editor the close would end, as pane commands aren't. One with a tab or
    /// an editor whose child still runs asks first, since closing ends them.
    pub(super) fn close_project(&mut self) {
        if self.state.open_projects().nth(1).is_none() {
            self.state.flash_error("the last project closes with :q");
            return;
        }
        if matches!(self.state.active_project().focus, state::Focus::Overlay) {
            self.state
                .flash_info("quit the editor first: closing the project ends it");
            return;
        }
        let id = self.state.active;
        let running = self.running_children();
        if running == 0 {
            self.close_project_now(id);
            return;
        }
        let label = self.state.label_of(id).unwrap_or_default();
        self.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::CloseProject(id),
            format!("close project '{label}' ({running} running)? [y/N] "),
        ));
    }

    /// Close project `id`, one of several open: from the project in view the
    /// user goes to the one they were in last, else a neighbour, first.
    pub(super) fn close_project_now(&mut self, id: ProjectId) {
        if id == self.state.active {
            let Some(to) = self.landing_after(id) else {
                return;
            };
            self.switch_project(to);
        }
        let label = self.state.label_of(id).unwrap_or_default();
        self.drop_project(id);
        self.state.flash_info(format!("closed project {label}"));
    }

    /// Drop project `id`, out of view, as closing it does:
    /// - its tabs' and editors' children are shut down off the loop thread,
    ///   as `^a x` shuts a tab's down, and its pager streams end;
    /// - its tabs' scope claims go, and a wait one of them parked is answered;
    /// - its context file is removed, and discovery's record of the roots
    ///   rewritten without it;
    /// - it leaves all three maps, its label kept for the work it left
    ///   running, and `Space Space` no longer goes to it.
    pub(super) fn drop_project(&mut self, id: ProjectId) {
        assert_ne!(
            id, self.state.active,
            "a project is dropped once the user is out of it"
        );
        let mut owners = HashSet::new();
        if let Some(mut project) = self.runtime.remove_project(id) {
            if let Some(tabs) = project.pane_tabs.as_mut() {
                owners.extend(tabs.tabs().iter().map(|t| t.info.claim_owner.clone()));
                while tabs.remove_at(0) {}
            }
            for overlay in [project.top_overlay, project.top_overlay_right]
                .into_iter()
                .flatten()
            {
                overlay.shutdown_detached(Duration::from_millis(250));
            }
        }
        self.state
            .scope_registry
            .retain(|c| !owners.contains(&c.owner));
        let (closed, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.runtime.scope_waiters)
            .into_iter()
            .partition(|w| owners.contains(&w.owner));
        self.runtime.scope_waiters = waiting;
        for wait in closed {
            let _ = wait.reply.send(McpResponse::Error {
                message: "the project this agent belongs to has closed".into(),
            });
        }
        if let Some(view) = self.view.remove_project(id) {
            crate::context::remove_context_file(&view.context_path);
        }
        self.state.remove_project(id);
        if self.state.last_project == Some(id) {
            self.state.last_project = None;
        }
        self.record_open_roots();
    }

    /// Where the user lands as project `id`, the one in view, closes: the
    /// project they were in last, else the next one, else the one before.
    fn landing_after(&self, id: ProjectId) -> Option<ProjectId> {
        let ids: Vec<ProjectId> = self.state.open_projects().map(|p| p.id).collect();
        let here = ids.iter().position(|p| *p == id)?;
        self.state
            .last_project
            .filter(|last| *last != id && self.state.has_project(*last))
            .or_else(|| ids.get(here + 1).copied())
            .or_else(|| here.checked_sub(1).map(|i| ids[i]))
    }

    /// How many of the project in view's tabs and editors still have a child
    /// running, as `^a x` judges one: what closing the project ends.
    fn running_children(&self) -> usize {
        let project = self.runtime.project();
        let live = |pane: &Pane| !pane.is_closed() && pane.exit_status().is_none();
        let tabs = project
            .pane_tabs
            .iter()
            .flat_map(PaneTabs::tabs)
            .filter(|tab| live(&tab.pane))
            .count();
        let overlays = [&project.top_overlay, &project.top_overlay_right]
            .into_iter()
            .flatten()
            .filter(|pane| live(pane))
            .count();
        tabs + overlays
    }
}

/// One switcher row: `[n] ■ label  k tabs  ← current  home`, with `glyph` (a
/// blank for a project with nothing to say) while several projects are open,
/// and none with one, the marker on the project in view and the label padded
/// to `label_w` columns, the widest of the list's. The home comes last,
/// since it is the one column of no fixed width: a long one is cut at the
/// pager's edge rather than pushing the marker off it. It is tilde-collapsed,
/// as every displayed path is.
fn project_row(
    i: usize,
    project: &state::Project,
    label_w: usize,
    tabs: usize,
    current: bool,
    glyph: Option<Span<'static>>,
) -> Line<'static> {
    let label = project.label();
    let pad = " ".repeat(label_w.saturating_sub(crate::ui::display_width(&label)));
    let tabs = if tabs == 1 {
        "1 tab".to_string()
    } else {
        format!("{tabs} tabs")
    };
    let mut spans = vec![Span::raw(format!("  [{}] ", i + 1))];
    spans.extend(glyph);
    spans.push(Span::raw(format!(
        " {label}{pad}  {tabs:<8} {:<10} {}",
        if current { "← current" } else { "" },
        crate::paths::display_tilde(project.anchor()),
    )));
    Line::from(spans)
}

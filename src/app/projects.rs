//! Opening a project, closing one, reopening one closed lately, and moving
//! between the open ones (#99): what the leader's project keys do — `Space
//! n`, `Space x`, `Space j`, `Space Space`, `Space 1-9`, and `Space ]` /
//! `Space [` — and how the dumps that cover every project head and name them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;

use super::state::{self, ProjectId, SwitcherRow};
use super::view_state::ProjectView;
use super::{App, Config, Mode, Pane, PaneTabs, Prompt, PromptKind, UserKeymap};
use crate::keymap::ProjectAction;
use crate::mcp_cmd::McpResponse;
use crate::state::sessions::{ClosedProject, Held, SavedProject};
use crate::ui::{pager, project_chips};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

/// The switcher's title, and once closed projects are listed under the open
/// ones, [`REOPEN_TITLE`].
const SWITCHER_TITLE: &str = "projects — j/k+Enter or 1-9 to go, / to search, q to close";
const REOPEN_TITLE: &str = "projects — j/k+Enter or 1-9 to go or reopen, / to search, q to close";

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

    /// `Space j`: list the open projects in switcher order, to go to one, and
    /// under them the projects closed lately, to reopen one. The closed rows
    /// are read on a worker and follow the open ones once they land
    /// (`spawn_closed_rows`), so the switcher opens at once and its numbers
    /// and highlight don't move. The one in view is highlighted. `j`/`k` and
    /// Enter, or `1`-`9`, pick (`switch_to_pending_project`).
    pub(super) fn open_project_switcher(&mut self) {
        self.clear_pending_pickers();
        let rows: Vec<SwitcherRow> = self
            .state
            .open_projects()
            .map(|p| SwitcherRow::Open(p.id))
            .collect();
        let active = self.state.active;
        let start = rows
            .iter()
            .position(|row| matches!(row, SwitcherRow::Open(id) if *id == active))
            .unwrap_or(0);
        let lines = self.switcher_lines(&rows);
        self.state.pending_projects = Some(rows);
        let mut view = pager::PagerView::new_styled(SWITCHER_TITLE, lines);
        view.picker_cursor = Some(start);
        view.no_history = true;
        view.show_line_numbers = false;
        view.wrap = false;
        self.set_pager(view);
        self.spawn_closed_rows();
    }

    /// Add `closed` to the open switcher, after the projects already listed,
    /// and draw every row again, padded to the widest label of them all. The
    /// switcher is the pager with its title, so rows read for one another
    /// pager has since covered land nowhere. Returns whether the switcher
    /// changed.
    pub(super) fn show_closed_rows(&mut self, closed: Vec<ClosedProject>) -> bool {
        let pager = self.view.project().pager.as_ref();
        if pager.is_none_or(|view| view.title != SWITCHER_TITLE) {
            return false;
        }
        let Some(mut rows) = self.state.pending_projects.take() else {
            return false;
        };
        rows.extend(closed.into_iter().map(|c| SwitcherRow::Closed(Box::new(c))));
        let lines = self.switcher_lines(&rows);
        self.state.pending_projects = Some(rows);
        if let Some(view) = self.view.project_mut().pager.as_mut() {
            view.lines = lines;
            REOPEN_TITLE.clone_into(&mut view.title);
        }
        true
    }

    /// The switcher's lines for `rows` ([`project_row`]). An open project has
    /// its agents' glyph as its chip in the status bar has it, how many tabs
    /// it has and its home, the one in view marked; a closed one is dimmed,
    /// with the tabs it saved and how long ago it closed.
    fn switcher_lines(&self, rows: &[SwitcherRow]) -> Vec<Line<'static>> {
        let tabs: HashMap<ProjectId, usize> = self
            .runtime
            .every_tab_set()
            .map(|(id, tabs)| (id, tabs.tabs().len()))
            .collect();
        let attention = self.project_attention();
        let pulse = self.spicy_pulse_color(self.view.agent_anim_frame);
        let several = self.state.open_projects().nth(1).is_some();
        let now = crate::sysinfo::epoch_secs();
        let texts: Vec<RowText> = rows
            .iter()
            .map(|row| match row {
                SwitcherRow::Open(id) => {
                    self.state
                        .project(*id)
                        .map_or_else(RowText::default, |p| RowText {
                            label: p.label(),
                            home: p.anchor().to_path_buf(),
                            tabs: tabs.get(id).copied().unwrap_or(0),
                            marker: if *id == self.state.active {
                                "← current"
                            } else {
                                ""
                            }
                            .to_owned(),
                            glyph: several.then(|| {
                                project_chips::cell(
                                    attention.get(id).copied(),
                                    pulse,
                                    &self.view.theme,
                                )
                            }),
                            dim: false,
                        })
                }
                SwitcherRow::Closed(closed) => RowText {
                    label: super::session_restore::saved_label(&closed.project),
                    home: closed.project.anchor().to_path_buf(),
                    tabs: closed.project.tabs.len(),
                    marker: super::recently_closed::closed_ago(closed.closed_secs, now),
                    glyph: several.then(|| Span::raw(" ")),
                    dim: true,
                },
            })
            .collect();
        let label_w = texts
            .iter()
            .map(|text| crate::ui::display_width(&text.label))
            .max()
            .unwrap_or(0);
        texts
            .into_iter()
            .enumerate()
            .map(|(i, text)| project_row(i, text, label_w))
            .collect()
    }

    /// Go to the switcher's row `idx`, or reopen the closed project it lists,
    /// and close the switcher, the pager of the project being left. Past the
    /// end of the list nothing happens, and the switcher stays open.
    pub(super) fn switch_to_pending_project(&mut self, idx: usize) {
        let Some(row) = self
            .state
            .pending_projects
            .as_ref()
            .and_then(|rows| rows.get(idx))
            .cloned()
        else {
            return;
        };
        self.clear_pager();
        self.state.pending_projects = None;
        self.view.needs_full_repaint = true;
        match row {
            SwitcherRow::Open(id) if self.state.has_project(id) => self.switch_project(id),
            SwitcherRow::Open(_) => {}
            SwitcherRow::Closed(closed) => self.reopen_project(&closed.project),
        }
    }

    /// Reopen `saved`, a project closed lately, beside the open ones and go to
    /// it, as `-r` opens a project out of view: its column `a`, its tabs and
    /// its split as it saved them, and its context file written. A tab that
    /// would resume into a conversation an agent is in starts fresh
    /// ([`Self::held_conversations`]). A project open again meanwhile is gone
    /// to instead, and one with no directory left stays closed; the flash
    /// says which.
    fn reopen_project(&mut self, saved: &SavedProject) {
        let label = super::session_restore::saved_label(saved);
        if let Some(id) = self.project_at_saved(saved) {
            self.state.forget_closed(saved.anchor());
            self.switch_project(id);
            self.state.flash_info(format!("project {label} is open"));
            return;
        }
        if super::session_restore::opens_at(saved).is_none() {
            self.state.flash_error(format!(
                "project {label} not reopened: its directories are gone"
            ));
            return;
        }
        let held = self.held_conversations();
        let Some(opened) = self.restore_out_of_view(saved, false, &held) else {
            self.state
                .flash_error(format!("project {label} could not be reopened"));
            return;
        };
        self.switch_project(opened.id);
        self.record_open_roots();
        self.state.forget_closed(saved.anchor());
        let mut parts = vec![format!(
            "reopened project {label} ({})",
            tab_count(
                self.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .map_or(0, |t| t.tabs().len())
            )
        )];
        if !opened.held.is_empty() {
            parts.push(format!(
                "{} tab(s) start fresh: another agent is in their conversations",
                opened.held.len()
            ));
        }
        let unopened = self.state.active_project().deferred_tabs.len();
        if unopened > 0 {
            parts.push(format!(
                "{unopened} tab(s) unopened, kept saved — session info shows reasons"
            ));
        }
        parts.extend(opened.problem);
        let text = parts.join("; ");
        if parts.len() > 1 {
            self.state.flash_error(text);
        } else {
            self.state.flash_info(text);
        }
    }

    /// What a reopened project's tabs mustn't resume into: the conversations
    /// every other running spyc's agents are in (`Held::by_running`), and
    /// those this spyc's own live tabs are in, each by the conversation it is
    /// pinned to and, for an agent that resumes its newest, by its directory.
    fn held_conversations(&self) -> Held {
        let mut held = Held::by_running(&crate::state::sessions::load_manifests());
        let live = self
            .runtime
            .every_tab_set()
            .flat_map(|(_, tabs)| tabs.tabs())
            .filter(|tab| !tab.pane.is_closed() && tab.pane.exit_status().is_none());
        for tab in live {
            held.add_running(
                tab.info.agent().kind(),
                tab.info.pinned_session_id(),
                &tab.info.cwd,
                tab.info.codex_home.as_deref(),
            );
        }
        held
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
        self.state
            .closed_projects
            .retain(|closed| !names_dir(closed.project.anchor(), dir));
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
    /// directory ([`names_dir`]).
    fn project_at(&self, dir: &Path) -> Option<ProjectId> {
        self.state
            .open_projects()
            .find(|p| names_dir(p.anchor(), dir) || names_dir(&p.start_dir, dir))
            .map(|p| p.id)
    }

    /// The open project `saved` can't open beside: the one anchored where it
    /// is, or started where it started, compared canonical ([`opened_in`]).
    pub(super) fn project_at_saved(&self, saved: &SavedProject) -> Option<ProjectId> {
        let (anchor, start) = saved_dirs(saved);
        self.state
            .open_projects()
            .find(|p| names_dir(p.anchor(), &anchor) || names_dir(&p.start_dir, &start))
            .map(|p| p.id)
    }

    /// Every open project's anchor and start dir, for [`opened_in`] off the
    /// loop thread.
    pub(super) fn open_dirs(&self) -> Vec<(PathBuf, PathBuf)> {
        self.state
            .open_projects()
            .map(|p| (p.anchor().to_path_buf(), p.start_dir.clone()))
            .collect()
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
    ///   running, and `Space Space` no longer goes to it;
    /// - it is kept as a save writes it, taken before its tabs shut down, for
    ///   `Space j` to reopen ([`Self::closed_snapshot`]).
    pub(super) fn drop_project(&mut self, id: ProjectId) {
        assert_ne!(
            id, self.state.active,
            "a project is dropped once the user is out of it"
        );
        let snapshot = self.closed_snapshot(id);
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
        self.state.remember_closed(snapshot);
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

/// Whether `path`, an open project's home or start dir, names `dir`, a
/// canonical directory: either can name it through a symlink.
fn names_dir(path: &Path, dir: &Path) -> bool {
    path == dir || std::fs::canonicalize(path).is_ok_and(|path| path == dir)
}

/// `saved`'s anchor and start dir, each canonical where it still resolves.
fn saved_dirs(saved: &SavedProject) -> (PathBuf, PathBuf) {
    let canonical = |dir: &Path| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    (canonical(saved.anchor()), canonical(&saved.start_dir))
}

/// Whether one of `open`, the open projects' anchors and start dirs
/// ([`App::open_dirs`]), has `saved` open: one anchored where it is, or
/// started where it started. A home is one project's, and a start dir is one
/// project's (`AppState::home_taken`, `move_start_dir`); `saved` anchored at
/// another's start dir, which `:project` allows, still opens.
pub(super) fn opened_in(open: &[(PathBuf, PathBuf)], saved: &SavedProject) -> bool {
    let (anchor, start) = saved_dirs(saved);
    open.iter().any(|(open_anchor, open_start)| {
        names_dir(open_anchor, &anchor) || names_dir(open_start, &start)
    })
}

/// What one switcher row shows besides its number.
#[derive(Default)]
struct RowText {
    label: String,
    home: PathBuf,
    tabs: usize,
    /// `← current` on the project in view, how long ago a closed one closed.
    marker: String,
    /// The project's agents' glyph (a blank for one with nothing to say)
    /// while several projects are open, and none with one.
    glyph: Option<Span<'static>>,
    /// A closed project's row is dimmed.
    dim: bool,
}

/// One switcher row: `[n] ■ label  k tabs  ← current  home`, the label
/// padded to `label_w` columns, the widest of the list's. The home comes
/// last, since it is the one column of no fixed width: a long one is cut at
/// the pager's edge rather than pushing the marker off it. It is
/// tilde-collapsed, as every displayed path is.
fn project_row(i: usize, text: RowText, label_w: usize) -> Line<'static> {
    let pad = " ".repeat(label_w.saturating_sub(crate::ui::display_width(&text.label)));
    let mut spans = vec![Span::raw(format!("  [{}] ", i + 1))];
    spans.extend(text.glyph);
    spans.push(Span::raw(format!(
        " {}{pad}  {:<8} {:<11} {}",
        text.label,
        tab_count(text.tabs),
        text.marker,
        crate::paths::display_tilde(&text.home),
    )));
    if text.dim {
        for span in &mut spans {
            span.style = span.style.add_modifier(Modifier::DIM);
        }
    }
    Line::from(spans)
}

/// `1 tab`, `3 tabs`.
fn tab_count(tabs: usize) -> String {
    if tabs == 1 {
        "1 tab".to_owned()
    } else {
        format!("{tabs} tabs")
    }
}

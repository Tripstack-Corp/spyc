//! The make menu. `Space m`, `:make` and a Makefile row's `make target …` list
//! a Makefile's targets (`crate::taskfile`), and a row runs one as a `!`
//! capture through [`App::start_capture_in`], so the live pager, its menu,
//! `^Z`, `:fg` and the task viewer all apply to it.
//!
//! Reading the Makefile is I/O, so it runs off the loop: `Space m` returns an
//! `Effect::ReadMakefile`, a detached worker calls `taskfile::discover`, and
//! the pre-recv drain opens the menu. A read that lands after the user has
//! moved on (a newer read asked, another menu or a prompt open, another
//! project in view) is dropped. Nothing is read on a cursor move, so nothing
//! is cached.
//!
//! A row's key is `:make <target>`, and the row and the typed command both run
//! through [`App::run_make`]. A typed `:make` takes each word as its own
//! argument, targets and `NAME=value` alike. A target name is the Makefile's
//! text and a typed word the user's, so every word of the command is
//! shell-quoted. A list taller than the frame keeps the rows it can draw and
//! ends in one naming how many more `:make` reaches.
//!
//! The open menu keeps every target the read found, in [`MakeMenu`] on its
//! `MenuState`, with a filter the user types: each printable character but
//! `?` (no target name holds one, so it still opens the key reference). The
//! rows are rebuilt from the targets the filter matches, as the list's `=`
//! filter matches a name ([`Matcher`]), and fitted to the frame again on each
//! keystroke, so the filter reaches the targets the fit had cut off.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use super::matcher::Matcher;
use super::mouse::chord_hit::chord_hint_layout;
use super::state::ProjectId;
use super::view_state::CommandRow;
use super::{App, Effect, HintEntry, Mode, View, Wake};
use crate::fs::EntryKind;
use crate::keymap::Action;
use crate::taskfile::{MakeTarget, Taskfile};

/// A Makefile to read off the loop: the file or directory `path` names, its
/// includes held inside `root`, for the project that asked, as its
/// `request`th read.
#[derive(Debug)]
pub struct MakefileRead {
    path: PathBuf,
    root: PathBuf,
    origin: ProjectId,
    request: u64,
}

/// What a read found, back on the loop.
pub struct MakefileLanded {
    origin: ProjectId,
    request: u64,
    found: Result<Taskfile, String>,
}

/// What a make menu row or a typed `:make` runs: make's `args`, a menu row's
/// target or the words typed, from `dir`, naming `file` with `-f` when plain
/// `make` there would read another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakeRun {
    pub dir: PathBuf,
    pub file: Option<String>,
    pub args: Vec<String>,
}

impl MakeRun {
    /// `make [-f FILE] ARGS…`, each word quoted for the shell.
    pub fn command(&self) -> String {
        let quote = crate::shell::shell_quote;
        let mut words = vec!["make".to_string()];
        if let Some(file) = &self.file {
            words.push("-f".to_string());
            words.push(quote(file));
        }
        words.extend(self.args.iter().map(|arg| quote(arg)));
        words.join(" ")
    }
}

/// An open make menu's source: the Makefile's targets, where they run from,
/// and the filter typed over them.
#[derive(Debug, Clone)]
pub struct MakeMenu {
    /// The Makefile's file name, which titles the menu.
    name: String,
    dir: PathBuf,
    /// The `-f` a run names, when plain `make` in `dir` would read another.
    file: Option<String>,
    targets: Vec<MakeTarget>,
    filter: String,
}

impl MakeMenu {
    /// The title, and the rows of the targets the filter matches as an
    /// `area`-sized frame draws them; one that runs nothing when none does.
    fn rows(&self, area: Rect) -> (String, Vec<(String, String, HintEntry)>) {
        let matcher = Matcher::build(&self.filter);
        let mut rows: Vec<(String, String, HintEntry)> = self
            .targets
            .iter()
            .filter(|target| matcher.matches(&target.name))
            .map(|target| self.row(target))
            .collect();
        if rows.is_empty() {
            let none = if self.filter.is_empty() {
                "make: no explicit targets".to_string()
            } else {
                format!("make: no target matches \"{}\"", self.filter)
            };
            rows.push((String::new(), none, HintEntry::Command(None)));
        }
        let title = if self.filter.is_empty() {
            format!("make · {}", self.name)
        } else {
            format!("make · {} · {}", self.name, self.filter)
        };
        (title, fit(rows, area))
    }

    /// `target`'s row: its `:make` key, its label, and the run it fires.
    fn row(&self, target: &MakeTarget) -> (String, String, HintEntry) {
        let run = MakeRun {
            dir: self.dir.clone(),
            file: self.file.clone(),
            args: vec![target.name.clone()],
        };
        (
            format!(":make {}", target.name),
            row_label(target),
            HintEntry::Command(Some(CommandRow::Make(run))),
        )
    }
}

impl App {
    /// `Space m`: read the Makefile under the cursor (a file or a link, never
    /// a FIFO or a device), else the focused column's directory's, off the
    /// loop.
    pub(super) fn open_make_menu(&mut self) -> Vec<Effect> {
        let c = self.state.cur();
        let dir = c.listing.dir.clone();
        let on_makefile = (c.view == View::Dir)
            .then(|| c.rows.get(c.cursor.index))
            .flatten()
            .filter(|row| {
                !row.deleted
                    && matches!(
                        row.kind,
                        EntryKind::File | EntryKind::Executable | EntryKind::Symlink
                    )
                    && crate::taskfile::is_makefile(&row.path)
            })
            .map(|row| row.path.clone());
        let root = c
            .git_cache
            .current_repo_root
            .clone()
            .unwrap_or_else(|| dir.clone());
        self.runtime.make_request += 1;
        vec![Effect::ReadMakefile(MakefileRead {
            path: on_makefile.unwrap_or(dir),
            root,
            origin: self.state.active,
            request: self.runtime.make_request,
        })]
    }

    /// `Effect::ReadMakefile`'s executor half: the read, on a detached worker.
    pub(super) fn spawn_makefile_read(&self, read: MakefileRead) {
        let MakefileRead {
            path,
            root,
            origin,
            request,
        } = read;
        let sender = self
            .runtime
            .make_results
            .sender(self.runtime.pane_wake_tx.clone(), Wake::Makefile);
        super::landing::spawn_worker(
            sender,
            move || MakefileLanded {
                origin,
                request,
                found: crate::taskfile::discover(&path, &root).map_err(|e| format!("{e:#}")),
            },
            move |message| MakefileLanded {
                origin,
                request,
                found: Err(format!("make: the read stopped: {message}")),
            },
        );
    }

    /// Pre-recv drain: open the menu a landed read found, or flash why there
    /// is none, unless the user has moved on, and run what the opened menu
    /// returns. Whether the frame changed.
    pub(super) fn apply_makefile_reads(
        &mut self,
        terminal: &mut crate::Tui,
        foreground_exec: &super::ForegroundExec,
    ) -> bool {
        let mut changed = false;
        let mut fx = Vec::new();
        for landed in self.runtime.make_results.drain() {
            let moved_on = landed.request != self.runtime.make_request
                || landed.origin != self.state.active
                || self.view.chord_hint.is_some()
                || matches!(self.state.mode, Mode::Prompting(_));
            if moved_on {
                continue;
            }
            changed = true;
            match landed.found {
                Ok(taskfile) => fx.extend(self.show_make_menu(taskfile)),
                Err(why) => self.state.flash_error(why),
            }
        }
        if !fx.is_empty() {
            self.run_effects(fx, terminal, foreground_exec);
        }
        changed
    }

    /// The menu of `taskfile`'s targets, titled `make · <file name>`.
    fn show_make_menu(&mut self, taskfile: Taskfile) -> Option<Effect> {
        let name = taskfile.path.file_name().map_or_else(
            || taskfile.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let dir = taskfile
            .path
            .parent()
            .map_or_else(|| self.state.cur().listing.dir.clone(), Path::to_path_buf);
        let file = (!taskfile.is_default).then(|| name.clone());
        let make = MakeMenu {
            name,
            dir,
            file,
            targets: taskfile.targets,
            filter: String::new(),
        };
        let (title, rows) = make.rows(self.frame_area());
        let fx = self.show_menu(title, rows);
        if let Some(menu) = self.view.chord_hint.as_mut().and_then(|h| h.menu.as_mut()) {
            menu.make = Some(make);
        }
        fx
    }

    /// The whole frame, as the menu is drawn over it.
    const fn frame_area(&self) -> Rect {
        let (width, height) = self.view.term_size;
        Rect::new(0, 0, width, height)
    }

    /// A key the open make menu's filter takes: a printable character but
    /// `?` adds to it, `Backspace` takes one off, and `Esc` clears it. Whether
    /// the key was taken: never in another menu, nor `Esc` with no filter,
    /// which closes the menu.
    pub(super) fn make_filter_key(&mut self, key: KeyEvent) -> bool {
        let Some(make) = self
            .view
            .chord_hint
            .as_mut()
            .and_then(|hint| hint.menu.as_mut())
            .and_then(|menu| menu.make.as_mut())
        else {
            return false;
        };
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(c) if plain && c != '?' && !c.is_control() => make.filter.push(c),
            KeyCode::Backspace if plain && make.filter.is_empty() => return true,
            KeyCode::Backspace if plain => {
                make.filter.pop();
            }
            KeyCode::Esc if !make.filter.is_empty() => make.filter.clear(),
            _ => return false,
        }
        self.refit_make_menu();
        true
    }

    /// Rebuild the open make menu's rows from its targets and filter, fitted
    /// to the frame, and highlight the first while a filter is typed, so
    /// `Enter` runs it.
    fn refit_make_menu(&mut self) {
        let area = self.frame_area();
        let Some(hint) = self.view.chord_hint.as_mut() else {
            return;
        };
        let Some(menu) = hint.menu.as_mut() else {
            return;
        };
        let Some(make) = menu.make.as_ref() else {
            return;
        };
        let (title, rows) = make.rows(area);
        menu.selected = (!make.filter.is_empty()).then_some(0);
        let (rows, entries) = rows
            .into_iter()
            .map(|(keys, label, entry)| ((keys, label), entry))
            .unzip();
        hint.title = title;
        hint.rows = rows;
        hint.entries = entries;
    }

    /// Run `run`'s target as a `!` capture from its directory: what a make
    /// menu row and `:make <target>` both do.
    pub(super) fn run_make(&mut self, run: &MakeRun) {
        let command = run.command();
        self.start_capture_in(&command, &command, &command, &run.dir);
    }
}

/// `:make [args…]`: with words, run make with each as its own argument, from
/// the focused column's directory, where plain `make` picks the Makefile;
/// bare, open the menu.
pub(super) fn cmd_make(app: &mut App, args: &str) -> Vec<Effect> {
    if args.is_empty() {
        return app.apply(&Action::MakeMenu).unwrap_or_else(|e| {
            app.state.flash_error(format!("make: {e:#}"));
            Vec::new()
        });
    }
    let dir = app.state.cur().listing.dir.clone();
    if app.state.mounts.contains(&dir)
        && let Some(why) = super::state::archive::refusal(&Action::MakeMenu)
    {
        app.state.flash_error(why);
        return Vec::new();
    }
    if args.chars().any(char::is_control) {
        app.state
            .flash_error("make: a target can't hold a control character");
        return Vec::new();
    }
    let run = MakeRun {
        dir,
        file: None,
        args: args.split_whitespace().map(str::to_string).collect(),
    };
    app.run_make(&run);
    Vec::new()
}

/// A target's row label: its own, the default goal's marked, and nothing a
/// terminal would read as a control sequence.
fn row_label(target: &MakeTarget) -> String {
    let label: String = target
        .label
        .as_deref()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    match (label.is_empty(), target.default) {
        (_, false) => label,
        (true, true) => "(default)".to_string(),
        (false, true) => format!("{label} (default)"),
    }
}

/// The rows an `area`-sized frame draws whole: all of them, or as many as fit
/// before a closing row that names how many more `:make <target>` reaches and
/// runs nothing. A row the frame cut off could still be chosen by the arrows,
/// out of sight.
fn fit(mut rows: Vec<(String, String, HintEntry)>, area: Rect) -> Vec<(String, String, HintEntry)> {
    if all_drawn(&rows, area) {
        return rows;
    }
    let total = rows.len();
    for keep in (0..total).rev() {
        let more = (
            String::new(),
            format!("… {} more: :make <target>", total - keep),
            HintEntry::Command(None),
        );
        let mut tried = rows[..keep].to_vec();
        tried.push(more.clone());
        if all_drawn(&tried, area) {
            rows.truncate(keep);
            rows.push(more);
            return rows;
        }
    }
    rows
}

/// Whether every row's key line is drawn, laid out as the renderer lays them
/// out. With no room for a box nothing is drawn, and nothing is cut.
fn all_drawn(rows: &[(String, String, HintEntry)], area: Rect) -> bool {
    let refs: Vec<(&str, &str)> = rows
        .iter()
        .map(|(keys, label, _)| (keys.as_str(), label.as_str()))
        .collect();
    let Some(layout) = chord_hint_layout(&refs, area, None, None) else {
        return true;
    };
    let body_h = usize::from(layout.rect.height.saturating_sub(2));
    let body_w = usize::from(layout.rect.width.saturating_sub(2));
    let mut drawn = vec![false; rows.len()];
    for (c, column) in layout.columns.iter().enumerate() {
        if (c + 1) * layout.col_w > body_w {
            break;
        }
        for line in column.iter().take(body_h).filter(|line| line.first) {
            drawn[line.entry] = true;
        }
    }
    drawn.into_iter().all(|d| d)
}

//! Which closed projects the switcher (`Space j`) lists after the open ones
//! (#99, `docs/archive/PROJECTS_PLAN.md` §3): this spyc's own, and every
//! project a spyc that has quit had open or had closed, newest first and one
//! per anchor. The saved sessions are read, and their spycs and directories
//! looked at, on a worker, so the switcher opens at once with the open
//! projects and the closed rows follow it.

use std::path::Path;
use std::sync::{Arc, PoisonError};

use super::state::{ProjectId, RECENTLY_CLOSED, newest_per_anchor};
use super::{App, Message, Wake};
use crate::state::sessions::{ClosedProject, Manifest, SavedProject};

/// What one read of the saved sessions found for a switcher.
pub(super) struct ClosedRows {
    /// Which switcher it is for (`Runtime::closed_rows_seq`).
    seq: u64,
    /// The project the switcher opened in.
    project: ProjectId,
    rows: Vec<ClosedProject>,
}

/// The closed projects to list, newest first: `own`, this spyc's, and from
/// each of `saved` but this spyc's own file (`own_id`), which `own` is newer
/// than, the projects a spyc that has quit had open, as closed when it last
/// saved, and those it had closed. A spyc still running has its projects open
/// and its closed ones to reopen itself, so its file adds none, and no
/// project anchored or started where one of its open ones is is listed. A
/// file naming no pid, as 2.2 wrote them, can't say whether its spyc has
/// quit, so it adds none either. Nor is a project open here listed
/// (`taken`), or one with no directory left to open at (`can_open`). Each
/// anchor's newest is kept before those are dropped, and no more than
/// [`RECENTLY_CLOSED`].
pub(super) fn recently_closed(
    own: &[ClosedProject],
    saved: &[(Manifest, bool)],
    own_id: Option<u64>,
    taken: impl Fn(&SavedProject) -> bool,
    can_open: impl Fn(&SavedProject) -> bool,
) -> Vec<ClosedProject> {
    let mut candidates = own.to_vec();
    let mut elsewhere: Vec<(&Path, &Path)> = Vec::new();
    for (manifest, running) in saved.iter().filter(|(m, _)| Some(m.id) != own_id) {
        if *running {
            elsewhere.extend(
                manifest
                    .projects
                    .iter()
                    .map(|p| (p.anchor(), p.start_dir.as_path())),
            );
            continue;
        }
        if manifest.pid.is_none() {
            continue;
        }
        candidates.extend(manifest.projects.iter().map(|project| ClosedProject {
            closed_secs: manifest.epoch_secs,
            project: project.clone(),
        }));
        candidates.extend_from_slice(&manifest.closed_projects);
    }
    let mut rows = newest_per_anchor(candidates);
    rows.retain(|closed| {
        let (anchor, start) = (closed.project.anchor(), closed.project.start_dir.as_path());
        !elsewhere.iter().any(|(a, s)| *a == anchor || *s == start)
            && !taken(&closed.project)
            && can_open(&closed.project)
    });
    rows.truncate(RECENTLY_CLOSED);
    rows
}

/// How long before `now` a project closed, for its switcher row: `closed <1m`,
/// `closed 5m`, `closed 3h`, `closed 2d`.
pub(super) fn closed_ago(closed_secs: u64, now: u64) -> String {
    let secs = now.saturating_sub(closed_secs);
    match secs {
        0..60 => "closed <1m".to_owned(),
        60..3_600 => format!("closed {}m", secs / 60),
        3_600..86_400 => format!("closed {}h", secs / 3_600),
        _ => format!("closed {}d", secs / 86_400),
    }
}

impl App {
    /// Read the closed rows for the switcher just opened, on a worker
    /// ([`recently_closed`]): the saved sessions, whether each one's spyc
    /// still runs, and which projects have a directory left. A switcher
    /// opened since supersedes one still being read.
    pub(super) fn spawn_closed_rows(&mut self) {
        self.runtime.closed_rows_seq += 1;
        let seq = self.runtime.closed_rows_seq;
        let project = self.state.active;
        let own = self.state.closed_projects.clone();
        let own_id = self.state.session_id;
        let open = self.open_dirs();
        let dir = crate::state::sessions::sessions_dir();
        let results = Arc::clone(&self.runtime.closed_rows_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let saved: Vec<(Manifest, bool)> = dir
                .as_deref()
                .map(crate::state::sessions::load_manifests_in)
                .unwrap_or_default()
                .into_iter()
                .map(|manifest| {
                    let running = manifest.running();
                    (manifest, running)
                })
                .collect();
            let rows = recently_closed(
                &own,
                &saved,
                own_id,
                |saved| super::projects::opened_in(&open, saved),
                |saved| super::session_restore::opens_at(saved).is_some(),
            );
            results
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(ClosedRows { seq, project, rows });
            if let Some(tx) = wake {
                let _ = tx.send(Message::Wake(Wake::ClosedRows));
            }
        });
    }

    /// Add the rows read for the switcher opened last to it, while it is
    /// still open where it opened. Drained every pre-recv scan, so the slot
    /// never holds rows for a later switcher. Returns whether the frame
    /// changed.
    pub(crate) fn apply_closed_rows(&mut self) -> bool {
        let landed = std::mem::take(
            &mut *self
                .runtime
                .closed_rows_results
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        let latest = self.runtime.closed_rows_seq;
        let Some(found) = landed.into_iter().find(|rows| rows.seq == latest) else {
            return false;
        };
        if found.rows.is_empty() || found.project != self.state.active {
            return false;
        }
        self.show_closed_rows(found.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::sessions::parse_manifest;
    use serde_json::json;
    use std::path::PathBuf;

    fn project(dir: &str) -> serde_json::Value {
        json!({
            "id": ProjectId::new(), "home": dir, "label": dir, "start_dir": dir,
            "pane_height_pct": 30
        })
    }

    fn closed(dir: &str, closed_secs: u64) -> ClosedProject {
        serde_json::from_value(json!({ "closed_secs": closed_secs, "project": project(dir) }))
            .expect("a closed project")
    }

    /// A saved session `id`, last saved at `epoch_secs`, with `open` open and
    /// `shut` closed.
    fn session(id: u64, epoch_secs: u64, open: &[&str], shut: &[(&str, u64)]) -> Manifest {
        let projects: Vec<_> = open.iter().map(|dir| project(dir)).collect();
        let closed: Vec<_> = shut
            .iter()
            .map(|(dir, secs)| json!({ "closed_secs": secs, "project": project(dir) }))
            .collect();
        parse_manifest(json!({
            "version": 2, "id": id, "epoch_secs": epoch_secs, "pid": 1, "process_started": 1,
            "projects": projects, "closed_projects": closed
        }))
        .expect("a session")
    }

    fn listed(rows: &[ClosedProject]) -> Vec<(PathBuf, u64)> {
        rows.iter()
            .map(|c| (c.project.anchor().to_path_buf(), c.closed_secs))
            .collect()
    }

    fn pairs(rows: &[(&str, u64)]) -> Vec<(PathBuf, u64)> {
        rows.iter().map(|(d, s)| (PathBuf::from(d), *s)).collect()
    }

    fn list(own: &[ClosedProject], saved: &[(Manifest, bool)]) -> Vec<(PathBuf, u64)> {
        listed(&recently_closed(own, saved, Some(1), |_| false, |_| true))
    }

    /// This spyc's closed projects, an earlier spyc's open ones as closed when
    /// it saved, and its closed ones, newest first.
    #[test]
    fn own_and_quit_spycs_projects_are_listed_newest_first() {
        let own = [closed("/a", 50)];
        let saved = [(session(2, 40, &["/b"], &[("/c", 60)]), false)];
        assert_eq!(
            list(&own, &saved),
            pairs(&[("/c", 60), ("/a", 50), ("/b", 40)])
        );
    }

    /// A running spyc's file adds nothing, and none of its open projects is
    /// listed from elsewhere, by anchor or start dir.
    #[test]
    fn a_running_spycs_projects_are_not_listed() {
        let own = [closed("/a", 50), closed("/b", 50)];
        let mut running = session(2, 90, &["/a"], &[("/c", 80)]);
        running.projects[0].home = None;
        running.projects[0].start_dir = PathBuf::from("/a");
        let mut started_in_b = session(3, 90, &["/elsewhere"], &[]);
        started_in_b.projects[0].start_dir = PathBuf::from("/b");
        let saved = [(running, true), (started_in_b, true)];
        assert_eq!(list(&own, &saved), Vec::<(PathBuf, u64)>::new());
    }

    /// A file naming no pid, as 2.2 wrote them, may be a spyc still running,
    /// and its agents' conversations aren't held, so it adds nothing.
    #[test]
    fn a_file_naming_no_pid_adds_nothing() {
        let mut old = session(2, 90, &["/a"], &[("/b", 80)]);
        old.pid = None;
        assert!(list(&[], &[(old, false)]).is_empty());
    }

    /// This spyc's own file is older than its own state, so it adds nothing.
    #[test]
    fn this_spycs_own_file_is_skipped() {
        let saved = [(session(1, 90, &["/a"], &[("/b", 80)]), false)];
        assert!(list(&[], &saved).is_empty());
    }

    /// The newest close of an anchor is the one listed, even when a newer one
    /// is then dropped as open, so an older one never shows through.
    #[test]
    fn each_anchor_is_listed_once_as_it_closed_last() {
        let own = [closed("/a", 50)];
        let mut newest = session(2, 70, &["/a"], &[]);
        newest.projects[0].start_dir = PathBuf::from("/s");
        let saved = [
            (newest, false),
            (session(3, 30, &["/b"], &[("/a", 20)]), false),
        ];
        assert_eq!(list(&own, &saved), pairs(&[("/a", 70), ("/b", 30)]));
        let rows = recently_closed(
            &own,
            &saved,
            Some(1),
            |p| p.start_dir == Path::new("/s"),
            |_| true,
        );
        assert_eq!(listed(&rows), pairs(&[("/b", 30)]));
    }

    /// An app with its switcher open on its one project, and nothing read.
    fn switcher_open() -> App {
        let mut app = App::test_app(std::env::temp_dir());
        app.open_project_switcher();
        // The worker the switcher started lands in the slot replaced here.
        app.runtime.closed_rows_results = Arc::default();
        app
    }

    fn land(app: &App, seq: u64, project: ProjectId) {
        app.runtime
            .closed_rows_results
            .lock()
            .unwrap()
            .push(ClosedRows {
                seq,
                project,
                rows: vec![closed("/a", 1)],
            });
    }

    fn rows(app: &App) -> usize {
        app.state.pending_projects.as_ref().map_or(0, Vec::len)
    }

    /// The rows read for the switcher open now are listed in it.
    #[test]
    fn the_rows_read_for_this_switcher_are_listed() {
        let mut app = switcher_open();
        let (seq, here) = (app.runtime.closed_rows_seq, app.state.active);
        land(&app, seq, here);
        assert!(app.apply_closed_rows());
        assert_eq!(rows(&app), 2);
        let pager = app.view.project().pager.as_ref().expect("the switcher");
        assert!(pager.title.contains("reopen"), "{}", pager.title);
    }

    /// Rows read for a switcher opened before this one, or in another
    /// project, or once the switcher has closed, are dropped.
    #[test]
    fn rows_read_for_another_switcher_are_dropped() {
        let mut app = switcher_open();
        let (seq, here) = (app.runtime.closed_rows_seq, app.state.active);
        land(&app, seq - 1, here);
        land(&app, seq, ProjectId::new());
        assert!(!app.apply_closed_rows());
        assert_eq!(rows(&app), 1);
        app.clear_pending_pickers();
        land(&app, seq, here);
        assert!(!app.apply_closed_rows());
        assert!(app.runtime.closed_rows_results.lock().unwrap().is_empty());
    }

    /// Rows read for a switcher the help covers land in it, under the help,
    /// so `q` brings the switcher back with them.
    #[test]
    fn rows_land_in_a_switcher_under_the_help() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = switcher_open();
        let (seq, here) = (app.runtime.closed_rows_seq, app.state.active);
        let press = |app: &mut App, c: char| {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .unwrap();
        };
        press(&mut app, 'H');
        land(&app, seq, here);
        assert!(!app.apply_closed_rows(), "the help in view changed");
        press(&mut app, 'q');
        assert_eq!(rows(&app), 2);
        let pager = app.view.project().pager.as_ref().expect("the switcher");
        assert_eq!(pager.lines.len(), 2, "the closed row isn't drawn");
    }

    /// Rows read for a switcher another pager has since covered land in
    /// neither.
    #[test]
    fn rows_never_land_in_a_pager_over_the_switcher() {
        let mut app = switcher_open();
        let (seq, here) = (app.runtime.closed_rows_seq, app.state.active);
        app.set_pager(crate::ui::pager::PagerView::new_plain(
            "other",
            vec!["x".to_owned()],
        ));
        land(&app, seq, here);
        assert!(!app.apply_closed_rows());
        let pager = app.view.project().pager.as_ref().expect("the other pager");
        assert_eq!((pager.title.as_str(), pager.lines.len()), ("other", 1));
        assert_eq!(rows(&app), 1);
    }

    /// A project open here, or with no directory left, isn't listed, and the
    /// rest are cut to five after those are dropped.
    #[test]
    fn only_five_that_can_be_reopened_are_listed() {
        let own: Vec<_> = ["/open", "/gone", "/1", "/2", "/3", "/4", "/5", "/6"]
            .into_iter()
            .enumerate()
            .map(|(i, dir)| closed(dir, 100 - i as u64))
            .collect();
        let rows = recently_closed(
            &own,
            &[],
            None,
            |p| p.anchor() == Path::new("/open"),
            |p| p.anchor() != Path::new("/gone"),
        );
        assert_eq!(
            listed(&rows),
            pairs(&[("/1", 98), ("/2", 97), ("/3", 96), ("/4", 95), ("/5", 94)])
        );
    }

    #[test]
    fn how_long_ago_reads_in_the_largest_whole_unit() {
        let now = 1_000_000;
        for (ago, said) in [
            (0, "closed <1m"),
            (59, "closed <1m"),
            (60, "closed 1m"),
            (3_599, "closed 59m"),
            (3_600, "closed 1h"),
            (86_399, "closed 23h"),
            (86_400, "closed 1d"),
            (10 * 86_400, "closed 10d"),
        ] {
            assert_eq!(closed_ago(now - ago, now), said, "{ago}s");
        }
        assert_eq!(closed_ago(now + 5, now), "closed <1m", "a clock behind");
    }
}

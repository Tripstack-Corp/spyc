//! The worktree picker (`W l`): every worktree of the focused column's repo,
//! the main one first and then the one updated last, each with when that was
//! ([`crate::git::last_updated`]). Dating a tree walks it, so the list is read
//! and dated on a worker, and the picker opens when it lands.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::git::worktree::Worktree;
use crate::ui::pager;

use super::state::ProjectId;
use super::{App, Wake};

const TITLE: &str = "git worktrees — j/k+Enter or 1-9 to switch, / to search, q to close";

/// What one read of the worktrees found for a `W l`.
pub(super) struct WorktreeRows {
    /// Which `W l` it is for (`Runtime::worktree_rows_seq`).
    seq: u64,
    /// The project and directory it was asked in.
    project: ProjectId,
    dir: PathBuf,
    /// Each worktree's path and row, in [`main_then_newest`]'s order; `None`
    /// outside a repo.
    rows: Option<Vec<(PathBuf, String)>>,
}

/// Order `dated`, as [`crate::git::worktree::list`] gives it, the way the
/// picker lists it: git's first entry stays first, the main worktree (or a
/// bare repo's own), the source the rest branch from; then the one updated
/// last, a tree with no time last.
fn main_then_newest(dated: &mut [(Worktree, Option<SystemTime>)]) {
    if let Some((_, linked)) = dated.split_first_mut() {
        linked.sort_by_key(|(_, at)| std::cmp::Reverse(*at));
    }
}

/// The rows for `dir`'s repo, dated as of `now` in `tz`: each tree is walked
/// on a thread of its own.
fn read_rows(
    dir: &Path,
    now: SystemTime,
    tz: &jiff::tz::TimeZone,
) -> Option<Vec<(PathBuf, String)>> {
    let worktrees = crate::git::worktree::list(dir)?;
    let times: Vec<Option<SystemTime>> = std::thread::scope(|scope| {
        let mut dating = Vec::with_capacity(worktrees.len());
        for (i, wt) in worktrees.iter().enumerate() {
            // A bare repo's entry, git's first, has no working tree to date.
            let bare = i == 0 && !wt.primary;
            dating.push(scope.spawn(move || {
                (!bare)
                    .then(|| crate::git::last_updated::last_updated(&wt.path))
                    .flatten()
            }));
        }
        dating
            .into_iter()
            .map(|thread| thread.join().ok().flatten())
            .collect()
    });
    let mut dated: Vec<_> = worktrees.into_iter().zip(times).collect();
    main_then_newest(&mut dated);
    Some(
        dated
            .iter()
            .enumerate()
            .map(|(i, (wt, at))| {
                let updated = updated_label(*at, now, tz);
                (wt.path.clone(), worktree_row(i, wt, &updated, dir))
            })
            .collect(),
    )
}

/// When a tree was updated, local time then how long ago:
/// `2026-11-14 17:50   12m ago`, or `—` for a tree with no time.
fn updated_label(at: Option<SystemTime>, now: SystemTime, tz: &jiff::tz::TimeZone) -> String {
    let Some(at) = at else {
        return "—".to_owned();
    };
    let local = jiff::Timestamp::try_from(at).map_or_else(
        |_| String::new(),
        |ts| {
            ts.to_zoned_datetime(tz.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        },
    );
    let secs = now.duration_since(at).unwrap_or_default().as_secs();
    let ago = match secs {
        0..60 => "<1m".to_owned(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    };
    format!("{local} {ago:>4} ago")
}

/// One `W l` picker row: `[n]  branch  head  updated  path`, marked
/// ` (main worktree)` for the repo's main checkout and ` ← current` for the
/// one the focused column sits in. The branch column names whatever is
/// checked out there, so without the main marker a main worktree holding a
/// feature branch reads exactly like a linked one — and then "where do I
/// switch back to?" has no answer on screen.
///
/// The path is tilde-collapsed like every other user-facing path display; the
/// switch target comes from `state.pending_worktrees`, which keeps the real
/// absolute paths, so shortening the label can't misdirect a jump.
fn worktree_row(i: usize, wt: &Worktree, updated: &str, cur_dir: &Path) -> String {
    format!(
        "  [{}]  {:<30} {:>8}  {:<25} {}{}{}",
        i + 1,
        wt.branch,
        wt.head,
        updated,
        crate::paths::display_tilde(&wt.path),
        if wt.primary { " (main worktree)" } else { "" },
        if wt.path == cur_dir {
            " ← current"
        } else {
            ""
        },
    )
}

impl App {
    /// W l — read and date the focused column's worktrees on a worker
    /// ([`read_rows`]); the picker opens when they land
    /// ([`App::apply_worktree_rows`]). A `W l` since supersedes one still
    /// being read.
    pub fn worktree_list(&mut self) {
        self.clear_pending_pickers();
        self.runtime.worktree_rows_seq += 1;
        let seq = self.runtime.worktree_rows_seq;
        let project = self.state.active;
        let dir = self.state.cur().listing.dir.clone();
        let results = self
            .runtime
            .worktree_rows_results
            .sender(self.runtime.pane_wake_tx.clone(), Wake::WorktreeRows);
        let cooking = self.start_cooking();
        std::thread::spawn(move || {
            let _cooking = cooking;
            let tz = jiff::tz::TimeZone::system();
            let rows = read_rows(&dir, SystemTime::now(), &tz);
            let _ = results.send(WorktreeRows {
                seq,
                project,
                dir,
                rows,
            });
        });
    }

    /// Open the picker on the rows read for the `W l` pressed last, while
    /// its project is in view and its column is still where it was asked.
    /// Drained every pre-recv scan. Returns whether the frame changed.
    pub(crate) fn apply_worktree_rows(&mut self) -> bool {
        let landed = self.runtime.worktree_rows_results.drain();
        let latest = self.runtime.worktree_rows_seq;
        let Some(found) = landed.into_iter().find(|read| read.seq == latest) else {
            return false;
        };
        if found.project != self.state.active || found.dir != self.state.cur().listing.dir {
            return false;
        }
        let Some(rows) = found.rows else {
            self.state
                .flash_error("not in a git repository (or no worktrees)");
            return true;
        };
        // The cursor starts on the worktree the focused column is in, so the
        // highlight reflects "where I am", else on the first row.
        let start = rows
            .iter()
            .position(|(path, _)| *path == found.dir)
            .unwrap_or(0);
        let (paths, lines): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        self.clear_pending_pickers();
        self.state.pending_worktrees = Some(paths);
        let mut view = pager::PagerView::new_plain(TITLE, lines);
        // A highlighted picker row (j/k move it, Enter switches to it); `/`
        // search syncs the cursor to its match (see
        // `handle_pager_search_typing`). Mirrors the jump-history popup.
        view.picker_cursor = Some(start);
        view.picker = Some(pager::PickerKind::Worktrees);
        view.no_history = true;
        view.show_line_numbers = false;
        view.wrap = false;
        self.set_pager(view);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn wt(path: &str, branch: &str, primary: bool) -> Worktree {
        Worktree {
            path: std::path::PathBuf::from(path),
            head: "abc1234".to_string(),
            branch: branch.to_string(),
            primary,
        }
    }

    /// The whole point of the marker: the main worktree is identifiable even
    /// when the branch checked out in it isn't the default one.
    #[test]
    fn main_worktree_row_is_marked_whatever_branch_it_holds() {
        let row = worktree_row(
            0,
            &wt("/src/proj", "fold-rule-8", true),
            "—",
            Path::new("/nope"),
        );
        assert!(
            row.contains("(main worktree)"),
            "main worktree row must say so: {row}"
        );
        assert!(row.contains("fold-rule-8"), "branch still shown: {row}");
    }

    #[test]
    fn linked_worktree_row_is_not_marked_main() {
        let row = super::worktree_row(
            1,
            &wt("/src/proj.worktrees/feat", "feat", false),
            "—",
            Path::new("/nope"),
        );
        assert!(
            !row.contains("(main worktree)"),
            "a linked worktree is not the main one: {row}"
        );
    }

    /// A bare repo has no working tree to mark, and its row already reads
    /// `(bare)` — so the marker must not claim otherwise.
    #[test]
    fn bare_repo_row_is_not_marked_main() {
        let row = worktree_row(
            0,
            &wt("/src/proj.git", "(bare)", false),
            "—",
            Path::new("/nope"),
        );
        assert!(row.contains("(bare)"), "bare label kept: {row}");
        assert!(
            !row.contains("(main worktree)"),
            "a bare repo has no main worktree: {row}"
        );
    }

    /// Paths under `$HOME` collapse to `~/…` like every other user-facing path
    /// display (DESIGN.md), and the markers still land after the shortened path.
    #[test]
    fn row_path_is_tilde_collapsed() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let home = home.to_string_lossy().trim_end_matches('/').to_string();
        let path = format!("{home}/src/proj");
        let row = worktree_row(0, &wt(&path, "fold-rule-8", true), "—", Path::new(&path));
        assert!(
            row.contains("~/src/proj (main worktree) ← current"),
            "path should be tilde-collapsed with markers intact: {row}"
        );
        assert!(!row.contains(&home), "absolute home prefix leaked: {row}");
    }

    #[test]
    fn row_path_outside_home_is_left_absolute() {
        let row = worktree_row(
            0,
            &wt("/srv/checkout", "main", true),
            "—",
            Path::new("/nope"),
        );
        assert!(
            row.contains("/srv/checkout"),
            "a non-home path stays as-is: {row}"
        );
    }

    #[test]
    fn both_markers_appear_when_the_user_is_in_the_main_worktree() {
        let row = worktree_row(
            0,
            &wt("/src/proj", "main", true),
            "—",
            Path::new("/src/proj"),
        );
        assert!(
            row.ends_with("(main worktree) ← current"),
            "main marker precedes the current marker: {row}"
        );
    }

    #[test]
    fn the_main_worktree_comes_first_then_the_tree_updated_last() {
        let at = |secs| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        let mut dated = vec![
            (wt("/main", "main", true), at(100)),
            (wt("/gone", "gone", false), None),
            (wt("/new", "new", false), at(300)),
            (wt("/mid", "mid", false), at(200)),
            (wt("/old", "old", false), at(50)),
        ];
        main_then_newest(&mut dated);
        let order: Vec<_> = dated.iter().map(|(wt, _)| wt.branch.as_str()).collect();
        assert_eq!(order, ["main", "new", "mid", "old", "gone"]);
    }

    /// The main worktree leads even when it holds another branch, or is the
    /// one updated least.
    #[test]
    fn the_main_worktree_leads_whatever_it_holds_and_however_old() {
        let at = |secs| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        let mut dated = vec![
            (wt("/main", "feat/x", true), None),
            (wt("/new", "new", false), at(300)),
        ];
        main_then_newest(&mut dated);
        assert!(dated[0].0.primary, "{:?}", dated[0].0.path);
    }

    #[test]
    fn the_updated_column_reads_local_time_then_how_long_ago() {
        let tz = jiff::tz::TimeZone::fixed(jiff::tz::Offset::from_hours(-5).unwrap());
        let at = SystemTime::UNIX_EPOCH + Duration::from_mins(29_386_000);
        let label = |later| updated_label(Some(at), at + Duration::from_secs(later), &tz);
        assert_eq!(label(30), "2025-11-14 17:40  <1m ago");
        assert_eq!(label(12 * 60), "2025-11-14 17:40  12m ago");
        assert_eq!(label(3 * 3_600), "2025-11-14 17:40   3h ago");
        assert_eq!(label(40 * 86_400), "2025-11-14 17:40  40d ago");
        assert_eq!(updated_label(None, at, &tz), "—");
    }
}

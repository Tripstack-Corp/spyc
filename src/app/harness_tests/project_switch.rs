//! #99 step 5d: a switch to another project brings it up to date, and what
//! the project left behind was doing carries on.
//!
//! Two projects in one `App`: `x`, where the user starts, and `y`. Neither the
//! watcher nor the git poll looks at a project the user isn't in, so whatever
//! changed in one while they were away is news when they come back.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;
use crate::app::pager_stream::{DrainOutcome, PagerStream, PagerStreamMount, RenderCtx};
use crate::ui::pager::PagerView;
use state::Side;

struct Two {
    app: App,
    x: state::ProjectId,
    y: state::ProjectId,
    x_home: PathBuf,
    y_home: PathBuf,
}

/// `x` is active and lists `xproj/`; `y` lists `yproj/`.
fn with_two(test: impl FnOnce(&mut Two)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, x_home, y_home) = (base.join("state"), base.join("xproj"), base.join("yproj"));
    for d in [&state_dir, &x_home, &y_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(x_home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &x_home);
        let y = app.add_test_project(&y_home);
        test(&mut Two {
            app,
            x,
            y,
            x_home,
            y_home,
        });
    });
}

/// Poll `done` for at most five seconds.
fn within_5s(app: &mut App, mut done: impl FnMut(&mut App) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if done(app) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether the project in view lists `name` in `side`'s column, in both its
/// listing and the rows drawn from it.
fn listed(app: &App, side: Side, name: &str) -> bool {
    let col = app.state.col(side);
    col.listing.entries.iter().any(|e| e.name == name)
        && col
            .rows
            .iter()
            .any(|r| r.path.file_name().is_some_and(|n| n == name))
}

// ── Pager streams ────────────────────────────────────────────────────────────

/// A stream that appends each line its worker sends and finishes when the
/// worker returns: the shape of `:grep` and the transcript read.
struct Lines {
    id: u32,
    rx: mpsc::Receiver<String>,
}

impl PagerStream for Lines {
    fn id(&self) -> u32 {
        self.id
    }

    fn drain(&mut self, view: &mut PagerView, _ctx: &RenderCtx) -> DrainOutcome {
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(line) => {
                    view.lines.push(ratatui::text::Line::from(line));
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    return if changed {
                        DrainOutcome::Changed
                    } else {
                        DrainOutcome::Idle
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    view.streaming = false;
                    return DrainOutcome::Finished;
                }
            }
        }
    }
}

/// Start a stream in the project in view, mounted as `mount`, whose worker
/// sends `lines`.
fn stream_lines(app: &mut App, mount: PagerStreamMount, lines: &[&str]) {
    let lines: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
    app.spawn_pager_stream(
        mount,
        move |tx: crate::fs::WakingSender<String>| {
            for line in lines {
                let _ = tx.send(line);
            }
        },
        |id, rx| Box::new(Lines { id, rx }),
    );
}

fn overlay() -> PagerStreamMount {
    PagerStreamMount::Overlay {
        title: "grep".into(),
        line_count_hint: None,
    }
}

fn lower_pane() -> PagerStreamMount {
    PagerStreamMount::LowerPane {
        title: " claude (transcript)".into(),
    }
}

fn text_of(view: Option<&PagerView>) -> Vec<String> {
    view.map(|v| v.lines.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}

/// Drain the project in view's streams until its pager (or, with `scroll`,
/// its scroll pager) holds `line`.
fn drained_into(app: &mut App, scroll: bool, line: &str) -> bool {
    within_5s(app, |app| {
        app.drain_pager_stream();
        let view = if scroll {
            app.view.project().scroll_pager.as_ref()
        } else {
            app.view.project().pager.as_ref()
        };
        text_of(view).iter().any(|l| l == line)
    })
}

/// The control: a stream in the project in view fills its pager.
#[test]
fn a_stream_in_the_project_in_view_fills_its_pager() {
    with_two(|t| {
        let app = &mut t.app;
        stream_lines(app, overlay(), &["one", "two"]);
        assert!(drained_into(app, false, "two"));
    });
}

/// A `:grep` still running when the user leaves fills its pager while
/// they are away, or on their return; it is never cut off.
#[test]
fn a_stream_left_running_in_x_fills_its_pager_on_return() {
    with_two(|t| {
        let app = &mut t.app;
        stream_lines(app, overlay(), &["one", "two"]);
        app.switch_project(t.y);
        app.drain_pager_stream();
        app.switch_project(t.x);
        assert!(
            drained_into(app, false, "two"),
            "X's pager was cut off: {:?}",
            text_of(app.view.project().pager.as_ref())
        );
    });
}

/// The same for the lower pane's transcript read, which streams on a slot of
/// its own.
#[test]
fn a_transcript_left_loading_in_x_fills_its_pager_on_return() {
    with_two(|t| {
        let app = &mut t.app;
        app.open_pane_tab_in("cat", &t.x_home);
        stream_lines(app, lower_pane(), &["one", "two"]);
        app.switch_project(t.y);
        app.drain_pager_stream();
        app.switch_project(t.x);
        assert!(
            drained_into(app, true, "two"),
            "X's transcript was cut off: {:?}",
            text_of(app.view.project().scroll_pager.as_ref())
        );
    });
}

/// A transcript parked on a tab of Y's (the user switched tabs while it
/// loaded) belongs to that tab: closing a tab in X leaves it alone.
#[test]
fn a_transcript_parked_in_y_survives_a_tab_closing_in_x() {
    with_two(|t| {
        let app = &mut t.app;
        app.switch_project(t.y);
        app.open_pane_tab_in("cat", &t.y_home);
        stream_lines(app, lower_pane(), &["one", "two"]);
        app.stash_scrollback_pager_to_active_tab();
        app.switch_project(t.x);
        app.open_pane_tab_in("cat", &t.x_home);
        app.close_active_tab_now();
        app.switch_project(t.y);
        app.restore_active_tab_scrollback_pager();
        assert!(
            drained_into(app, true, "two"),
            "Y's parked transcript was dropped: {:?}",
            text_of(app.view.project().scroll_pager.as_ref())
        );
    });
}

// ── What changed while the user was away ─────────────────────────────────────

/// Land the project in view's listing reads until `side` lists `name`, for at
/// most five seconds: a switch reads the arriving project's directories on a
/// worker.
fn relisted(app: &mut App, side: Side, name: &str) -> bool {
    within_5s(app, |app| {
        let _ = app.apply_file_outcomes();
        listed(app, side, name)
    })
}

/// A file made in X while the user was in Y is listed when they come back.
#[test]
fn a_file_made_in_x_while_away_is_listed_on_return() {
    with_two(|t| {
        let app = &mut t.app;
        app.switch_project(t.y);
        std::fs::write(t.x_home.join("fresh.txt"), "x").unwrap();
        app.switch_project(t.x);
        assert!(relisted(app, Side::Left, "fresh.txt"));
    });
}

/// Both of X's columns are re-read, not only the focused one.
#[test]
fn both_of_xs_columns_are_reread_on_return() {
    with_two(|t| {
        let app = &mut t.app;
        let sub = t.x_home.join("sub");
        std::fs::create_dir(&sub).unwrap();
        app.open_second_commander_at(&sub);
        app.switch_project(t.y);
        std::fs::write(t.x_home.join("left.txt"), "x").unwrap();
        std::fs::write(sub.join("right.txt"), "x").unwrap();
        app.switch_project(t.x);
        assert!(relisted(app, Side::Left, "left.txt"), "a was not re-read");
        assert!(relisted(app, Side::Right, "right.txt"), "b was not re-read");
    });
}

/// A listing read X started before the user left can't land over the one
/// made when they came back: it was read first, so it misses what changed
/// since.
#[test]
fn a_listing_read_before_the_trip_never_replaces_the_one_after() {
    with_two(|t| {
        let app = &mut t.app;
        app.spawn_listing_refresh();
        assert!(
            within_5s(app, |app| !app
                .runtime
                .file_results
                .lock()
                .unwrap()
                .is_empty()),
            "X's read never landed"
        );
        std::fs::write(t.x_home.join("fresh.txt"), "x").unwrap();
        app.switch_project(t.y);
        app.switch_project(t.x);
        assert!(
            relisted(app, Side::Left, "fresh.txt"),
            "X's read from before the trip replaced the listing made on return"
        );
    });
}

/// The switch's read still lands once the column's rows were rebuilt for
/// something else, as a git result landing first rebuilds them.
#[test]
fn the_switchs_read_lands_after_an_unrelated_row_rebuild() {
    with_two(|t| {
        let app = &mut t.app;
        app.switch_project(t.y);
        std::fs::write(t.x_home.join("fresh.txt"), "x").unwrap();
        app.switch_project(t.x);
        app.state.rebuild_rows_for(Side::Left);
        assert!(relisted(app, Side::Left, "fresh.txt"));
    });
}

/// A column moved right after the switch keeps where it went: the switch's
/// read of where it was lands later, and is dropped.
#[test]
fn a_column_moved_after_the_switch_is_not_put_back_by_its_read() {
    with_two(|t| {
        let app = &mut t.app;
        let (sub, other) = (t.x_home.join("sub"), t.x_home.join("other"));
        for d in [&sub, &other] {
            std::fs::create_dir(d).unwrap();
        }
        std::fs::write(other.join("there.txt"), "x").unwrap();
        // `b` keeps the keyboard, so moving `a` leaves the process cwd alone.
        app.open_second_commander_at(&sub);
        app.switch_project(t.y);
        std::fs::write(sub.join("right.txt"), "x").unwrap();
        app.switch_project(t.x);
        assert_eq!(app.state.focused_side(), Side::Right);
        app.state.chdir_side(Side::Left, &other).unwrap();
        // `b`'s read and `a`'s land together.
        assert!(
            relisted(app, Side::Right, "right.txt"),
            "the switch's reads never landed"
        );
        let left = &app.state.col(Side::Left).listing;
        assert_eq!(left.dir, other, "the switch's read put `a` back");
        assert!(left.entries.iter().any(|e| e.name == "there.txt"));
    });
}

/// An edit made in X's tree while the user was away shows in its git markers
/// when they come back, though it moved neither `.git/index` nor `HEAD`, the
/// two files the poll's mtime key watches.
#[test]
fn an_edit_in_x_while_away_shows_in_its_git_markers_on_return() {
    with_two(|t| {
        let app = &mut t.app;
        let run_git = crate::git::test_support::run_git;
        std::fs::write(t.x_home.join("a.txt"), "v1\n").unwrap();
        run_git(&t.x_home, &["init", "-q", "--initial-branch=main"]);
        run_git(&t.x_home, &["add", "a.txt"]);
        run_git(&t.x_home, &["commit", "-q", "-m", "v1"]);
        let x_home = t.x_home.clone();
        app.state.update_repo_root(Side::Left, &x_home);
        app.state.refresh_git_state();
        let dirty = |app: &App| {
            app.state
                .col(Side::Left)
                .git
                .files
                .get("a.txt")
                .is_some_and(|s| !s.is_clean())
        };
        assert!(!dirty(app), "a.txt starts clean");
        app.switch_project(t.y);
        std::fs::write(t.x_home.join("a.txt"), "v2, and longer\n").unwrap();
        app.switch_project(t.x);
        assert!(dirty(app), "X's markers still show a.txt clean");
    });
}

/// A file X previews in its split was saved while the user was away: the
/// preview shows what is on disk when they come back.
#[test]
fn xs_preview_shows_a_save_made_while_away() {
    with_two(|t| {
        let app = &mut t.app;
        let doc = t.x_home.join("doc.txt");
        std::fs::write(&doc, "old\n").unwrap();
        let mut view = PagerView::new_plain("doc.txt", vec!["old".to_string()]);
        view.source_path = Some(doc.clone());
        app.view.project_mut().right_pager = Some(view);
        app.switch_project(t.y);
        std::fs::write(&doc, "new\n").unwrap();
        app.switch_project(t.x);
        assert!(
            within_5s(app, |app| {
                app.apply_preview_reloads();
                text_of(app.view.project().right_pager.as_ref())
                    .iter()
                    .any(|l| l == "new")
            }),
            "X's preview still shows {:?}",
            text_of(app.view.project().right_pager.as_ref())
        );
    });
}

/// The colours in force are the arriving project's own (`[colors]` in its
/// `.spycrc.toml`), and the user's colours-off toggle is theirs across a
/// switch.
#[test]
fn the_arriving_projects_colours_are_in_force() {
    with_two(|t| {
        let app = &mut t.app;
        app.state.active_project_mut().config.colors.dir = Some("#ff0000".into());
        app.switch_project(t.y);
        assert_eq!(
            app.view.theme.dir,
            crate::ui::theme::Theme::default().dir,
            "Y has no colours of its own"
        );
        app.view.theme = app.view.theme.toggled();
        app.switch_project(t.x);
        assert_eq!(
            app.view.theme.dir,
            ratatui::style::Color::Rgb(0xff, 0, 0),
            "X's [colors] are not in force"
        );
        assert!(app.view.theme.mono, "the colours-off toggle was lost");
    });
}

/// A terminal-scrollback snapshot armed in X is abandoned by a switch, as it
/// is by a tab switch, never taken of Y's tab.
#[test]
fn a_scrollback_snapshot_armed_in_x_is_never_taken_of_ys_tab() {
    with_two(|t| {
        let app = &mut t.app;
        app.open_pane_tab_in("cat", &t.x_home);
        app.open_pane_scroll_pager();
        app.open_tab_in_project(t.y, &t.y_home);
        app.switch_project(t.y);
        app.state.flash = None;
        let mut ctx = RunCtx::for_test();
        app.settle_pane_scroll(Instant::now() + Duration::from_secs(5), &mut ctx);
        assert_eq!(app.flash_text(), None, "a snapshot was taken of Y's tab");
        assert!(app.view.project().scroll_pager.is_none());
    });
}

/// What X's agents read about it, its context file, is where the user left
/// it, though they switched away before the write's typing-burst wait ran out.
#[test]
fn xs_context_file_holds_where_the_user_left_it() {
    with_two(|t| {
        let app = &mut t.app;
        for name in ["a.txt", "b.txt"] {
            std::fs::write(t.x_home.join(name), "x").unwrap();
        }
        app.state.refresh_listing();
        let b = app
            .state
            .cur()
            .rows
            .iter()
            .position(|r| r.path.ends_with("b.txt"))
            .expect("b.txt is listed");
        app.state.cur_mut().cursor.index = b;
        // What a keypress does (`App::run`); the write waits for typing to stop.
        app.view.project_mut().context_dirty = true;
        let path = app.view.project().context_path.clone();
        app.switch_project(t.y);
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["cursor_file"], "b.txt", "X's context file is stale");
    });
}

/// A terminal resize while the user was away lays X's help out again for
/// the new width, as it does the project in view's.
#[test]
fn a_resize_while_away_lays_xs_help_out_again() {
    with_two(|t| {
        let app = &mut t.app;
        app.handle_resize(80, 24);
        app.open_help();
        let columns = |app: &App| app.view.project().pager.as_ref().map(|p| p.columns);
        assert_eq!(columns(app), Some(1), "help opens in one column at 80 wide");
        app.switch_project(t.y);
        app.handle_resize(240, 60);
        app.switch_project(t.x);
        assert_eq!(
            columns(app),
            Some(2),
            "X's help is laid out for the old width"
        );
    });
}

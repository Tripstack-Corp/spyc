//! Actions taken in column `b` act on `b`. Each of these read or wrote
//! `state.left` — column `a` — while the keyboard was in `b`.

use super::*;
use crate::app::graveyard_ops::GraveyardOutcome;
use crate::state::graveyard::Graveyard;

/// Columns `a` and `b` holding the named files, `b` open and focused.
fn two_columns(
    tmp: &std::path::Path,
    a_files: &[&str],
    b_files: &[&str],
) -> (App, std::path::PathBuf, std::path::PathBuf) {
    let root = std::fs::canonicalize(tmp).unwrap();
    let (a, b) = (root.join("a"), root.join("b"));
    for (dir, files) in [(&a, a_files), (&b, b_files)] {
        std::fs::create_dir(dir).unwrap();
        for name in files {
            std::fs::write(dir.join(name), name).unwrap();
        }
    }
    let mut app = App::test_app(a.clone());
    app.state.refresh_listing(); // `test_app` doesn't list its dir
    app.open_second_commander_at(&b);
    assert_eq!(app.focused_side(), state::Side::Right);
    (app, a, b)
}

fn b_col(app: &App) -> &state::Commander {
    app.state.right.as_ref().expect("b is open")
}

fn bury(tmp: &std::path::Path, names: &[&str]) {
    let buried = tmp.join("buried");
    std::fs::create_dir_all(&buried).unwrap();
    for name in names {
        let p = buried.join(name);
        std::fs::write(&p, name).unwrap();
        Graveyard::write_entry(&p).unwrap();
    }
}

/// The graveyard opened in `b` navigates `b`'s cursor over `b`'s rows, and
/// `p` restores the entry under THAT cursor. Moving by `a`'s row count froze
/// `j` whenever `a` was shorter, and restore/purge indexed the graveyard
/// with `a`'s cursor — a purge of whichever entry `a`'s file row lined up with.
#[test]
fn the_graveyard_in_column_b_moves_and_restores_bs_cursor_entry() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, _a, b) = two_columns(tmp.path(), &[], &[]);
        bury(tmp.path(), &["one.txt", "two.txt", "three.txt"]);
        app.state.left.grid_dims.rows_per_col = 20;
        app.state.right.as_mut().unwrap().grid_dims.rows_per_col = 20;
        app.state.open_graveyard_view();
        assert!(matches!(b_col(&app).view, View::Graveyard));
        assert_eq!(b_col(&app).rows.len(), 3);

        app.handle_key(key('j')).unwrap();
        assert_eq!(b_col(&app).cursor.index, 1, "j moves b over b's own rows");
        app.handle_key(key('G')).unwrap();
        assert_eq!(b_col(&app).cursor.index, 2, "G lands on b's last row");
        assert_eq!(app.state.left.cursor.index, 0, "a's cursor never moved");

        let under_cursor = app.state.graveyard[2].filename.clone();
        app.handle_key(key('p')).unwrap();
        assert!(
            b.join(&under_cursor).exists(),
            "p restored the entry under b's cursor ({under_cursor}) into b; flash: {:?}",
            app.flash_text()
        );
    });
}

/// An undo that lands while `b` shows the graveyard refreshes `b`'s view.
/// The refresh ran only when `a` showed the graveyard.
#[test]
fn an_undo_landing_refreshes_the_graveyard_open_in_column_b() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, _a, b) = two_columns(tmp.path(), &[], &[]);
        bury(tmp.path(), &["x", "y", "z"]);
        app.state.open_graveyard_view();
        app.state.right.as_mut().unwrap().cursor.index = 2;

        // The undo worker restored one entry and deleted it from the graveyard.
        let restored = app.state.graveyard[2].clone();
        Graveyard::delete_entry(&restored);
        app.runtime
            .graveyard_results
            .lock()
            .unwrap()
            .push(GraveyardOutcome::Restored {
                filename: restored.filename,
                dest: b,
                result: Ok(()),
            });
        app.apply_graveyard_outcomes();

        assert_eq!(
            b_col(&app).rows.len(),
            2,
            "b's view dropped the restored entry"
        );
        assert_eq!(
            b_col(&app).cursor.index,
            1,
            "and b's cursor was clamped into it"
        );
    });
}

/// A graveyard change landing off-thread leaves a column showing a directory
/// alone. It clamped `a`'s cursor to the graveyard's length whatever `a`
/// showed, so a cascade landing after launch snapped `a`'s cursor upward.
#[test]
fn a_landed_graveyard_change_leaves_a_directory_cursor_alone() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, _a, _b) = two_columns(tmp.path(), &["1", "2", "3", "4"], &[]);
        bury(tmp.path(), &["x"]);
        assert_eq!(app.state.left.rows.len(), 4);
        app.state.left.cursor.index = 3;

        app.runtime
            .graveyard_results
            .lock()
            .unwrap()
            .push(GraveyardOutcome::Cascaded {
                trashed: 1,
                errors: 0,
            });
        app.apply_graveyard_outcomes();

        assert_eq!(app.state.left.cursor.index, 3, "a lists a directory");
    });
}

/// Confirming `R` in `b` clears `b`'s picks — the ones just removed — and
/// leaves `a`'s alone. It cleared `a`'s, so an unrelated selection in the
/// other column vanished and `b` kept picks naming deleted files.
#[test]
fn confirming_a_remove_in_column_b_clears_bs_picks_not_as() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, a, b) = two_columns(tmp.path(), &["keep.txt"], &["gone.txt"]);
        let (in_a, in_b) = (a.join("keep.txt"), b.join("gone.txt"));
        app.state.left.picks.insert(&in_a);
        app.state.right.as_mut().unwrap().picks.insert(&in_b);

        app.apply(&Action::RemovePrompt(None)).unwrap();
        assert_eq!(
            app.state.pending_delete_preview.as_deref(),
            Some(&[in_b][..]),
            "the prompt targets b's picks"
        );
        app.handle_key(key('y')).unwrap();

        assert!(
            b_col(&app).picks.is_empty(),
            "b's removed picks are cleared"
        );
        assert!(
            app.state.left.picks.iter().any(|p| p == &in_a),
            "a's picks survive a remove in b"
        );
    });
}

/// Tab in a `/` search typed in `b` turns the term into `b`'s filter. It set
/// `a`'s, then rebuilt `b`'s rows — so the filter appeared nowhere visible
/// and silently narrowed the other column.
#[test]
fn tab_in_a_search_in_column_b_filters_b() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, _a, _b) = two_columns(tmp.path(), &[], &[]);
        app.apply(&Action::SearchPrompt).unwrap();
        app.handle_key(key('f')).unwrap();
        app.handle_key(key('o')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()))
            .unwrap();

        assert_eq!(b_col(&app).temp_filter.as_deref(), Some("fo*"));
        assert_eq!(app.state.left.temp_filter, None, "a is not filtered");
    });
}

/// Reloading the config re-applies `[[ignore_masks]]` to every open column,
/// and each column redraws without what its masks now hide. Only `a` was
/// reset, so `b` kept the masks it was opened with until it closed.
#[test]
fn a_config_reload_updates_the_masks_of_both_columns() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, a, _b) = two_columns(tmp.path(), &[], &["x.zzmask", "kept"]);
        assert_eq!(b_col(&app).rows.len(), 2);
        std::fs::write(
            a.join(".spycrc.toml"),
            "[[ignore_masks]]\ngroup = 2\npatterns = [\"*.zzmask\"]\nenabled = true\n",
        )
        .unwrap();

        app.reload_config();

        assert!(
            app.state.left.masks.hides("x.zzmask"),
            "a picked up the new mask; flash: {:?}",
            app.flash_text()
        );
        assert!(b_col(&app).masks.hides("x.zzmask"), "so did b");
        let shown: Vec<&str> = b_col(&app)
            .rows
            .iter()
            .map(|r| r.display.as_str())
            .collect();
        assert_eq!(shown, ["kept"], "b redrew without the masked file");
    });
}

/// With no PROJECT_HOME the terminal title names the focused column's
/// directory, as the status bar does. It named `a`'s.
#[test]
fn the_terminal_title_falls_back_to_the_focused_columns_dir() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, _a, _b) = two_columns(tmp.path(), &[], &[]);
        app.state.project_home = None;
        app.state.session_name = None;

        assert!(app.term_title_effect().is_some());

        let title = app.view.last_term_title.as_deref().expect("title composed");
        assert!(title.ends_with(": b"), "{title}");
    });
}

/// Session info reports the focused column throughout. It printed `b`'s cwd
/// beside `a`'s entry, visible and pick counts.
#[test]
fn session_info_counts_the_column_it_names() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let (mut app, _a, b) = two_columns(tmp.path(), &[], &["one", "two", "three"]);
        app.state
            .right
            .as_mut()
            .unwrap()
            .picks
            .insert(&b.join("one"));

        app.show_session_info();

        let pager = app.view.pager.as_ref().expect("session info opened");
        let text: Vec<String> = pager
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let field = |name: &str| {
            text.iter()
                .find(|l| l.starts_with(name))
                .unwrap_or_else(|| panic!("no {name} line in {text:?}"))
                .clone()
        };
        assert_eq!(field("entries"), "entries  : 3");
        assert_eq!(field("visible"), "visible  : 3");
        assert_eq!(field("picks"), "picks    : 1");
    });
}

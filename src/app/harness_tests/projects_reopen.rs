//! #99 step 8b: the switcher (`Space j`) lists the projects closed lately, in
//! this spyc and in earlier ones that have quit, and reopens the one picked
//! with its tabs.
//!
//! The fixtures are `projects_open.rs`'s: `x` is where the user starts, `y` is
//! open beside it, and `zproj/` is a directory no project has yet. Saved
//! sessions are written and read as JSON, never through the Rust types.

use std::path::{Path, PathBuf};

use super::manifest_v2::{
    crate_root, dead_pid, in_temp_home, manifest, project, read_manifest, started_at, tab,
    write_manifest,
};
use super::projects_close::leader_from_pane;
use super::projects_open::{Three, leader, open, with_three};
use super::*;
use ratatui::style::Modifier;
use serde_json::{Value, json};

const P1: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5b01";
const P2: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5b02";
const P3: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5b03";
const P4: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5b04";
const P5: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5b05";
const UUID: &str = "019e8b21-9e7c-7553-a118-d1cdada72701";
const UUID2: &str = "019e8b21-9e7c-7553-a118-d1cdada72702";
const UUID3: &str = "019e8b21-9e7c-7553-a118-d1cdada72703";

/// `manifest` as written by a spyc that has quit: a pid no process holds.
fn quit(mut manifest: Value) -> Value {
    manifest["pid"] = json!(dead_pid());
    manifest["process_started"] = json!(1_700_000_000);
    manifest
}

/// Where `with_three` keeps spyc's state.
fn state_root(t: &Three) -> PathBuf {
    t.home.parent().expect("the fixture's base").join("state")
}

/// A directory `name` beside the fixture's projects, made unless `make` is
/// false.
fn sibling(t: &Three, name: &str, make: bool) -> PathBuf {
    let dir = t.home.parent().expect("the fixture's base").join(name);
    if make {
        std::fs::create_dir_all(&dir).unwrap();
    }
    dir
}

/// The switcher's rows, while it is open in the project in view.
fn rows(app: &App) -> Vec<String> {
    app.view
        .project()
        .pager
        .as_ref()
        .map_or_else(Vec::new, |v| {
            v.lines.iter().map(ToString::to_string).collect()
        })
}

/// The switcher's row listing `label` as closed, once it has landed.
fn closed_row(app: &mut App, label: &str) -> Option<String> {
    let is_it = |row: &String| row.contains(label) && row.contains("closed");
    app.land_closed_rows(|app| rows(app).iter().any(&is_it));
    rows(app).into_iter().find(is_it)
}

/// The switcher's closed rows.
fn closed_rows(app: &App) -> Vec<String> {
    rows(app)
        .into_iter()
        .filter(|row| row.contains("closed"))
        .collect()
}

/// Save the session as quitting does, and read the file back.
fn saved(app: &mut App, state_root: &Path) -> Value {
    app.save_session();
    read_manifest(state_root, app.state.session_id.expect("a session id"))
}

/// The homes of the closed projects a saved session keeps.
fn closed_homes(saved: &Value) -> Vec<Value> {
    saved["closed_projects"]
        .as_array()
        .map(|closed| {
            closed
                .iter()
                .map(|c| c["project"]["home"].clone())
                .collect()
        })
        .unwrap_or_default()
}

/// A `cat` tab in project `id`, so quitting saves the session, with the
/// project's file list keeping the keyboard.
fn tab_to_save(app: &mut App, id: state::ProjectId, cwd: &Path) {
    app.open_tab_in_project(id, cwd);
    app.as_project(id, |app| app.state.set_focus_pane_or_list(false));
}

fn press_enter(app: &mut App) {
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
}

/// The project in view's tabs' commands.
fn commands(app: &App) -> Vec<String> {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .map(|tabs| tabs.tabs().iter().map(|t| t.info.command.clone()).collect())
        .unwrap_or_default()
}

/// `Space x`, then `Space j`: the closed project is listed after the open
/// one, numbered on and dimmed, with its tabs as it saved them, and its
/// number reopens it as it closed: the user in it, column `a` where it was,
/// its tab running its command again, and its context file written for its
/// agents.
#[test]
fn a_closed_project_is_listed_and_its_number_reopens_it() {
    with_three(|t| {
        let (y, y_home) = (t.y, t.y_home.clone());
        let sub = y_home.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let app = &mut t.app;
        app.open_tab_in_project(y, &y_home);
        app.as_project(y, |app| {
            app.state.active_project_mut().left.listing =
                crate::fs::listing::Listing::read(&sub).expect("sub lists");
        });
        leader(app, '2');
        leader_from_pane(app, 'x');
        app.handle_key(key('y')).unwrap();
        assert!(
            !app.state.has_project(y),
            "y is open: {:?}",
            app.flash_text()
        );

        leader(app, 'j');
        let row =
            closed_row(app, "yproj").unwrap_or_else(|| panic!("no closed row: {:#?}", rows(app)));
        assert!(row.contains("[2]") && row.contains("1 tab"), "{row}");
        let pager = app.view.project().pager.as_ref().expect("the switcher");
        let line = pager
            .lines
            .iter()
            .find(|line| line.to_string() == row)
            .expect("the row");
        assert!(
            line.spans.iter().all(|span| {
                line.style
                    .patch(span.style)
                    .add_modifier
                    .contains(Modifier::DIM)
            }),
            "the closed row isn't dimmed: {line:?}"
        );

        app.handle_key(key('2')).unwrap();
        assert_eq!(
            app.state.open_projects().count(),
            2,
            "{:?}",
            app.flash_text()
        );
        assert_eq!(
            app.state.active_project().home.as_deref(),
            Some(y_home.as_path())
        );
        assert_eq!(app.state.col(state::Side::Left).listing.dir, sub);
        assert_eq!(commands(app), ["cat"]);
        let tabs = app.runtime.project().pane_tabs.as_ref().expect("its tab");
        assert!(!tabs.tabs()[0].pane.is_closed(), "its tab isn't running");
        assert!(app.view.project().context_path.exists(), "no context file");
        assert!(app.state.pending_projects.is_none(), "the switcher is open");
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("reopened project yproj"), "{flash}");
    });
}

/// A reopened project is no longer kept as closed: the next save has it open,
/// and not closed too.
#[test]
fn a_reopened_project_is_no_longer_kept_as_closed() {
    with_three(|t| {
        let state_root = state_root(t);
        let (x, x_home) = (t.x, t.x_home.clone());
        let app = &mut t.app;
        app.state.session_id = Some(500);
        tab_to_save(app, x, &x_home);
        leader(app, '2');
        leader(app, 'x');
        leader(app, 'j');
        assert!(closed_row(app, "yproj").is_some(), "{:#?}", rows(app));
        app.handle_key(key('2')).unwrap();
        let saved = saved(app, &state_root);
        assert!(closed_homes(&saved).is_empty(), "{saved:#}");
        assert_eq!(saved["projects"].as_array().map(Vec::len), Some(2));
    });
}

/// A project a spyc that has quit had open is listed as closed when that spyc
/// last saved, and Enter on it opens it here with its tabs.
#[test]
fn a_project_an_earlier_spyc_had_open_is_listed_and_reopens() {
    with_three(|t| {
        let (state_root, z_home) = (state_root(t), t.z_home.clone());
        let then = crate::sysinfo::epoch_secs() - 2 * 3600;
        let tabs = vec![tab("cat", &z_home)];
        let projects = vec![project(P1, &z_home, &z_home, tabs)];
        write_manifest(
            &state_root,
            7,
            &quit(manifest(7, "EARLIER", then, P1, projects)),
        );
        let app = &mut t.app;
        leader(app, 'j');
        let row =
            closed_row(app, "zproj").unwrap_or_else(|| panic!("no closed row: {:#?}", rows(app)));
        assert!(
            row.contains("[3]") && row.contains("closed 2h") && row.contains("1 tab"),
            "{row}"
        );
        app.handle_key(key('j')).unwrap();
        app.handle_key(key('j')).unwrap();
        press_enter(app);
        assert_eq!(
            app.state.open_projects().count(),
            3,
            "{:?}",
            app.flash_text()
        );
        assert_eq!(
            app.state.active_project().home.as_deref(),
            Some(z_home.as_path())
        );
        assert_eq!(commands(app), ["cat"]);
    });
}

/// Only a project that can be reopened is listed: an earlier spyc's open and
/// closed ones, but none another running spyc has, open or closed, nor one
/// open here, nor one whose directories have all gone, nor one in this spyc's
/// own file, whose state is newer.
#[test]
fn only_a_project_that_can_be_reopened_is_listed() {
    with_three(|t| {
        let state_root = state_root(t);
        let [w_home, t_home, u_home, v_home] =
            ["wproj", "tproj", "uproj", "vproj"].map(|name| sibling(t, name, true));
        let gone = sibling(t, "goneproj", false);
        let then = crate::sysinfo::epoch_secs() - 600;
        let closed = |id: &str, home: &Path| json!([{ "closed_secs": then - 60, "project": project(id, home, home, vec![]) }]);
        let mut earlier = manifest(
            7,
            "EARLIER",
            then,
            P1,
            vec![
                project(P1, &w_home, &w_home, vec![]),
                project(P2, &t.x_home, &t.x_home, vec![]),
                project(P3, &gone, &gone, vec![]),
            ],
        );
        earlier["closed_projects"] = closed(P4, &t_home);
        write_manifest(&state_root, 7, &quit(earlier));
        let live = std::os::unix::process::parent_id();
        let z_home = t.z_home.clone();
        let mut running = manifest(
            8,
            "RUNNING",
            then,
            P1,
            vec![project(P1, &z_home, &z_home, vec![])],
        );
        running["pid"] = json!(live);
        running["process_started"] = json!(started_at(live));
        running["closed_projects"] = closed(P5, &u_home);
        write_manifest(&state_root, 8, &running);
        let mine = manifest(
            9,
            "MINE",
            then,
            P1,
            vec![project(P1, &v_home, &v_home, vec![])],
        );
        write_manifest(&state_root, 9, &quit(mine));
        let app = &mut t.app;
        app.state.session_id = Some(9);

        leader(app, 'j');
        assert!(closed_row(app, "wproj").is_some(), "{:#?}", rows(app));
        assert!(closed_row(app, "tproj").is_some(), "{:#?}", rows(app));
        assert_eq!(closed_rows(app).len(), 2, "{:#?}", rows(app));
    });
}

/// A closed project is saved with the session, apart from the open ones, with
/// when it closed.
#[test]
fn a_closed_project_is_saved_with_the_session() {
    with_three(|t| {
        let state_root = state_root(t);
        let (x, x_home, y_home) = (t.x, t.x_home.clone(), t.y_home.clone());
        let app = &mut t.app;
        app.state.session_id = Some(500);
        tab_to_save(app, x, &x_home);
        let before = crate::sysinfo::epoch_secs();
        leader(app, '2');
        leader(app, 'x');
        assert!(!app.state.has_project(t.y), "{:?}", app.flash_text());

        let saved = saved(app, &state_root);
        let open: Vec<&Value> = saved["projects"]
            .as_array()
            .expect("projects")
            .iter()
            .map(|p| &p["home"])
            .collect();
        assert_eq!(open, [&json!(x_home)]);
        assert_eq!(closed_homes(&saved), [json!(y_home)], "{saved:#}");
        let closed_secs = saved["closed_projects"][0]["closed_secs"].as_u64();
        assert!(closed_secs.is_some_and(|s| s >= before), "{saved:#}");
    });
}

/// A restore that adopts a file takes its closed projects too: the next save
/// over it keeps them, and the switcher lists them.
#[test]
fn an_adopted_sessions_closed_projects_are_kept() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let w_home = std::fs::canonicalize(tmp.path()).unwrap().join("wproj");
        std::fs::create_dir_all(&w_home).unwrap();
        let root = crate_root();
        let mut json = manifest(
            600,
            "ADOPTED",
            1_700_000_000,
            P1,
            vec![project(P1, &root, &root, vec![])],
        );
        json["closed_projects"] = json!([{
            "closed_secs": 1_700_000_000, "project": project(P2, &w_home, &w_home, vec![])
        }]);
        write_manifest(state_root, 600, &json);
        let mut app = App::test_app(root);
        app.restore_session(&parsed(json));
        assert_eq!(app.state.session_id, Some(600), "{:?}", app.flash_text());

        let saved = saved(&mut app, state_root);
        assert_eq!(closed_homes(&saved), [json!(w_home)], "{saved:#}");
        leader(&mut app, 'j');
        assert!(closed_row(&mut app, "wproj").is_some(), "{:#?}", rows(&app));
    });
}

/// `Space n` onto a closed project's home opens it afresh and forgets the
/// closed one, and a project closed twice is kept once, as it closed last.
#[test]
fn a_project_closed_twice_is_kept_once() {
    with_three(|t| {
        let state_root = state_root(t);
        let (x, x_home, y_home) = (t.x, t.x_home.clone(), t.y_home.clone());
        let app = &mut t.app;
        app.state.session_id = Some(500);
        tab_to_save(app, x, &x_home);
        leader(app, '2');
        leader(app, 'x');
        assert_eq!(closed_homes(&saved(app, &state_root)), [json!(y_home)]);

        open(app, &y_home);
        assert_eq!(
            app.state.active_project().home.as_deref(),
            Some(y_home.as_path())
        );
        let reopened = saved(app, &state_root);
        assert!(closed_homes(&reopened).is_empty(), "{reopened:#}");

        leader(app, 'x');
        assert_eq!(
            app.state.open_projects().count(),
            1,
            "{:?}",
            app.flash_text()
        );
        let closed = saved(app, &state_root);
        assert_eq!(closed_homes(&closed), [json!(y_home)], "{closed:#}");
    });
}

/// A closed project whose directories have gone since the switcher listed it
/// isn't reopened, and the flash says why.
#[test]
fn a_closed_project_gone_since_it_was_listed_says_so() {
    with_three(|t| {
        let (x, y_home) = (t.x, t.y_home.clone());
        let app = &mut t.app;
        leader(app, '2');
        leader(app, 'x');
        leader(app, 'j');
        assert!(closed_row(app, "yproj").is_some(), "{:#?}", rows(app));
        std::fs::remove_dir_all(&y_home).unwrap();

        app.handle_key(key('2')).unwrap();
        assert_eq!(app.state.open_projects().count(), 1);
        assert_eq!(app.state.active, x);
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("yproj") && flash.contains("gone"), "{flash}");
    });
}

/// Reopening never resumes a conversation a tab of this spyc is in, which
/// would put two agents in it: that tab's agent starts fresh, the flash says
/// so, and the others resume.
#[test]
fn a_reopened_tab_whose_conversation_a_tab_here_is_in_starts_fresh() {
    with_three(|t| {
        let (state_root, x, x_home) = (state_root(t), t.x, t.x_home.clone());
        let base = t.home.parent().expect("the fixture's base").to_path_buf();
        let w_home = sibling(t, "wproj", true);
        let codex = |sub: &str, sid: &str| {
            json!({
                "command": fake_agent(&base.join(sub), "codex"), "label": sub, "cwd": w_home,
                "agent_kind": "codex", "agent_session_id": sid,
                "codex_home": base.join("codex-home")
            })
        };
        let tabs = vec![codex("held", UUID), codex("free", UUID2)];
        let then = crate::sysinfo::epoch_secs() - 600;
        let projects = vec![project(P1, &w_home, &w_home, tabs)];
        write_manifest(
            &state_root,
            7,
            &quit(manifest(7, "EARLIER", then, P1, projects)),
        );
        let mine = fake_agent(&base.join("mine"), "codex");
        let app = &mut t.app;
        app.as_project(x, |app| {
            assert!(app.open_pane_tab_in(&mine.display().to_string(), &x_home));
            let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
            let tab = tabs.tabs_mut().last_mut().expect("a tab");
            tab.info.codex_session_id = Some(UUID.to_owned());
        });

        leader_from_pane(app, 'j');
        assert!(closed_row(app, "wproj").is_some(), "{:#?}", rows(app));
        app.handle_key(key('3')).unwrap();
        assert_eq!(
            app.state.active_project().home.as_deref(),
            Some(w_home.as_path()),
            "{:?}",
            app.flash_text()
        );
        let held = agent_argv(&base.join("held"), "codex");
        assert!(!held.iter().any(|arg| arg == "resume"), "{held:?}");
        let free = agent_argv(&base.join("free"), "codex");
        assert!(
            free.windows(2).any(|w| w[0] == "resume" && w[1] == UUID2),
            "{free:?}"
        );
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("1 tab(s) start fresh"), "{flash}");
    });
}

/// Reopening never resumes a conversation another running spyc's agent is
/// in, either: that tab's agent starts fresh, and the others resume.
#[test]
fn a_reopened_tab_whose_conversation_another_spyc_is_in_starts_fresh() {
    with_three(|t| {
        let state_root = state_root(t);
        let base = t.home.parent().expect("the fixture's base").to_path_buf();
        let [w_home, r_home] = ["wproj", "rproj"].map(|name| sibling(t, name, true));
        let codex = |sub: &str, sid: &str, cwd: &Path| {
            json!({
                "command": fake_agent(&base.join(sub), "codex"), "label": sub, "cwd": cwd,
                "agent_kind": "codex", "agent_session_id": sid,
                "codex_home": base.join("codex-home")
            })
        };
        let then = crate::sysinfo::epoch_secs() - 600;
        let tabs = vec![
            codex("theirs", UUID3, &w_home),
            codex("free", UUID2, &w_home),
        ];
        let projects = vec![project(P1, &w_home, &w_home, tabs)];
        write_manifest(
            &state_root,
            7,
            &quit(manifest(7, "EARLIER", then, P1, projects)),
        );
        let live = std::os::unix::process::parent_id();
        let tabs = vec![codex("holder", UUID3, &r_home)];
        let mut running = manifest(
            8,
            "RUNNING",
            then,
            P2,
            vec![project(P2, &r_home, &r_home, tabs)],
        );
        running["pid"] = json!(live);
        running["process_started"] = json!(started_at(live));
        write_manifest(&state_root, 8, &running);
        let app = &mut t.app;

        leader(app, 'j');
        assert!(closed_row(app, "wproj").is_some(), "{:#?}", rows(app));
        app.handle_key(key('3')).unwrap();
        let theirs = agent_argv(&base.join("theirs"), "codex");
        assert!(!theirs.iter().any(|arg| arg == "resume"), "{theirs:?}");
        let free = agent_argv(&base.join("free"), "codex");
        assert!(
            free.windows(2).any(|w| w[0] == "resume" && w[1] == UUID2),
            "{free:?}"
        );
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("1 tab(s) start fresh"), "{flash}");
    });
}

/// A reopened codex tab that never learned its conversation would resume the
/// newest in its directory (`resume --last`), so it starts fresh where a
/// codex tab here runs in that directory under the same home.
#[test]
fn a_reopened_idless_codex_where_a_codex_here_runs_starts_fresh() {
    with_three(|t| {
        let (state_root, x) = (state_root(t), t.x);
        let base = t.home.parent().expect("the fixture's base").to_path_buf();
        let w_home = sibling(t, "wproj", true);
        let codex_home = base.join("codex-home");
        let idless = json!({
            "command": fake_agent(&base.join("idless"), "codex"), "label": "idless",
            "cwd": w_home, "agent_kind": "codex", "codex_home": codex_home
        });
        let then = crate::sysinfo::epoch_secs() - 600;
        let projects = vec![project(P1, &w_home, &w_home, vec![idless])];
        write_manifest(
            &state_root,
            7,
            &quit(manifest(7, "EARLIER", then, P1, projects)),
        );
        let mine = fake_agent(&base.join("mine"), "codex");
        let app = &mut t.app;
        app.as_project(x, |app| {
            assert!(app.open_pane_tab_in(&mine.display().to_string(), &w_home));
            let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
            tabs.tabs_mut().last_mut().expect("a tab").info.codex_home = Some(codex_home.clone());
        });

        leader_from_pane(app, 'j');
        assert!(closed_row(app, "wproj").is_some(), "{:#?}", rows(app));
        app.handle_key(key('3')).unwrap();
        let argv = agent_argv(&base.join("idless"), "codex");
        assert!(!argv.iter().any(|arg| arg == "resume"), "{argv:?}");
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("1 tab(s) start fresh"), "{flash}");
    });
}

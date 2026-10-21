//! The recovery manifest, version 2 (#99 step 4, `docs/drafts/PROJECTS_PLAN.md`
//! §3): one file holding every project, column `a`'s directory, and the pid of
//! the spyc writing it.
//!
//! These read and write the file on disk as JSON, never through the Rust types,
//! and drive the picker as the user does. Every restore lands column `a` in the
//! crate root, where the test process already runs: restoring moves the process
//! cwd, and a temp dir there would leave parallel tests without one.

use super::*;
use crossterm::event::KeyCode;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const P1: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5a6b";
const P2: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5a6c";
const P3: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5a6d";
const UUID: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn write_manifest(state_root: &Path, id: u64, manifest: &Value) {
    let dir = state_root.join("sessions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();
}

fn read_manifest(state_root: &Path, id: u64) -> Value {
    let path = state_root.join("sessions").join(format!("{id}.json"));
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

/// An older restore point at the crate root, so the picker opens even while
/// the manifest under test doesn't parse: its absence then fails a named
/// assertion rather than a helper.
fn write_v1_decoy(state_root: &Path) {
    write_manifest(
        state_root,
        400,
        &json!({
            "id": 400, "name": "V1_DECOY", "saved_at": "", "epoch_secs": 1_600_000_000,
            "cwd": crate_root(), "tabs": [], "active_tab": 0,
            "pane_height_pct": 30, "pane_focused": false
        }),
    );
}

fn tab(label: &str, cwd: &Path) -> Value {
    json!({ "command": "cat", "label": label, "cwd": cwd })
}

/// One project, with column `a` at `left_cwd`.
fn project(id: &str, home: &Path, left_cwd: &Path, tabs: Vec<Value>) -> Value {
    json!({
        "id": id, "home": home, "label": home.file_name().unwrap().to_string_lossy(),
        "start_dir": home, "left_cwd": left_cwd,
        "pane_height_pct": 40, "pane_focused": false, "tabs": tabs, "active_tab": 0
    })
}

fn manifest(id: u64, name: &str, epoch_secs: u64, active: &str, projects: Vec<Value>) -> Value {
    json!({
        "version": 2, "id": id, "name": name, "saved_at": "", "epoch_secs": epoch_secs,
        "active_project": active, "projects": projects
    })
}

/// Open the `-r` picker and return its lines.
fn picker_lines(app: &mut App) -> Vec<String> {
    app.show_session_picker();
    let flash = app.flash_text().map(str::to_owned);
    let pager = app.view.project().pager.as_ref();
    assert!(pager.is_some(), "the picker opens: {flash:?}");
    pager
        .unwrap()
        .lines
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn listed<'a>(lines: &'a [String], name: &str) -> &'a str {
    lines
        .iter()
        .find(|line| line.contains(name))
        .unwrap_or_else(|| panic!("the picker lists {name}: {lines:#?}"))
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_pager_key(KeyEvent::new(code, KeyModifiers::empty()));
}

fn tab_count(app: &App) -> usize {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .map_or(0, |tabs| tabs.tabs().len())
}

fn the_only_session_file(state_root: &Path) -> Option<Value> {
    let dir = state_root.join("sessions");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert!(files.len() <= 1, "one session file: {files:?}");
    let path = files.pop()?;
    serde_json::from_slice(&std::fs::read(path).unwrap()).ok()
}

/// When `pid` started, in seconds since the epoch.
fn started_at(pid: u32) -> u64 {
    use sysinfo_crate::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid)
        .expect("the process is running")
        .start_time()
}

/// A pid no process holds: a child's, once it has been reaped.
fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// Today's save, written before manifests had a version, still restores: the
/// session's id and name, its home, its start directory and its tab. Column `a`
/// lands on its `cwd`, the only directory a version 1 file has.
#[test]
fn a_version_1_session_still_restores() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        write_manifest(
            &state_root,
            501,
            &json!({
                "id": 501, "name": "V1_CONTROL", "saved_at": "", "epoch_secs": 1_700_000_000,
                "cwd": crate_root(), "project_home": crate_root(),
                "tabs": [tab("kept", &crate_root())], "active_tab": 0,
                "pane_height_pct": 40, "pane_focused": false
            }),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        let lines = picker_lines(&mut app);
        listed(&lines, "V1_CONTROL");
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(app.state.session_id, Some(501));
        assert_eq!(app.state.session_name.as_deref(), Some("V1_CONTROL"));
        assert_eq!(app.state.cur().listing.dir, crate_root());
        assert_eq!(app.state.active_project().start_dir, crate_root());
        assert_eq!(app.state.active_project().home, Some(crate_root()));
        assert_eq!(app.state.active_project().pane.pane_height_pct, 40);
        assert_eq!(tab_count(&app), 1);
    });
}

/// A save is a version 2 manifest: it names its process and lists the project
/// with column `a`'s own directory, not just the project's home.
#[test]
fn a_save_writes_a_version_2_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let home = std::fs::canonicalize(tmp.path()).unwrap().join("proj");
        let sub = home.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let mut app = App::test_app(home.clone());
        app.state.session_id = Some(9001);
        app.state.active_project_mut().home = Some(home.clone());
        app.open_pane_tab_in("cat", &home);
        app.state.active_project_mut().left.listing =
            crate::fs::listing::Listing::empty(sub.clone());
        app.save_session();

        let saved = read_manifest(&state_root, 9001);
        assert_eq!(
            saved["version"],
            json!(2),
            "a version 2 manifest: {saved:#}"
        );
        assert_eq!(saved["pid"], json!(std::process::id()));
        let projects = saved["projects"].as_array().cloned().unwrap_or_default();
        assert_eq!(projects.len(), 1, "one project: {saved:#}");
        let project = &projects[0];
        assert_eq!(project["left_cwd"], json!(sub), "column a's directory");
        assert_eq!(project["home"], json!(home));
        assert_eq!(project["start_dir"], json!(home));
        assert_eq!(project["label"], json!("proj"));
        assert_eq!(project["tabs"].as_array().map(Vec::len), Some(1));
        assert_eq!(saved["active_project"], project["id"]);
    });
}

/// `-r` puts column `a` back in the directory it was in, apart from the
/// project's home and start directory, and the restore takes the manifest's id
/// so later saves overwrite that file.
#[test]
fn a_restore_puts_column_a_back_where_it_was() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let home = std::fs::canonicalize(tmp.path()).unwrap().join("proj");
        let launched = home.join("launched");
        std::fs::create_dir_all(&launched).unwrap();
        write_v1_decoy(&state_root);
        let mut saved = project(P1, &home, &crate_root(), vec![tab("kept", &home)]);
        saved["start_dir"] = json!(launched);
        write_manifest(
            &state_root,
            502,
            &manifest(502, "V2_RESTORE", 1_700_000_100, P1, vec![saved]),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        let lines = picker_lines(&mut app);
        listed(&lines, "V2_RESTORE");
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(app.state.cur().listing.dir, crate_root(), "column a");
        assert_eq!(app.state.active_project().start_dir, launched);
        assert_eq!(app.state.active_project().home, Some(home));
        assert_eq!(tab_count(&app), 1);
        assert_eq!(
            app.state.session_id,
            Some(502),
            "the restore adopts the file"
        );
    });
}

/// A column `a` whose directory has gone lands on the project's home instead.
#[test]
fn a_gone_column_a_lands_on_the_projects_home() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        write_v1_decoy(&state_root);
        let gone = tmp.path().join("gone");
        write_manifest(
            &state_root,
            503,
            &manifest(
                503,
                "V2_GONE",
                1_700_000_100,
                P1,
                vec![project(P1, &crate_root(), &gone, Vec::new())],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        let lines = picker_lines(&mut app);
        listed(&lines, "V2_GONE");
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(app.state.cur().listing.dir, crate_root());
        assert_eq!(app.state.session_id, Some(503));
    });
}

/// Every open project is saved with its own tabs and column `a`, and the
/// manifest names the one the user is in.
#[test]
fn every_open_project_is_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (a, b) = (base.join("a"), base.join("b"));
        let b_deep = b.join("deep");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b_deep).unwrap();
        let mut app = App::test_app(a.clone());
        app.state.session_id = Some(9001);
        app.open_pane_tab_in("cat", &a);
        let b_id = app.add_test_project(&b);
        app.open_tab_in_project(b_id, &b);
        app.open_tab_in_project(b_id, &b);
        app.as_project(b_id, |app| {
            app.state.active_project_mut().left.listing =
                crate::fs::listing::Listing::empty(b_deep.clone());
        });
        app.save_session();

        let saved = read_manifest(&state_root, 9001);
        let projects = saved["projects"].as_array().cloned().unwrap_or_default();
        assert_eq!(projects.len(), 2, "both projects: {saved:#}");
        assert_eq!(saved["active_project"], projects[0]["id"], "a is active");
        assert_eq!(projects[0]["left_cwd"], json!(a));
        assert_eq!(projects[0]["tabs"].as_array().map(Vec::len), Some(1));
        assert_eq!(projects[1]["home"], json!(b));
        assert_eq!(projects[1]["left_cwd"], json!(b_deep));
        assert_eq!(projects[1]["tabs"].as_array().map(Vec::len), Some(2));
        let summary = app.exit_summary.as_deref().unwrap_or_default();
        assert!(summary.contains("a (1 tab) · b (2)"), "{summary}");
    });
}

/// A tab in a project the user isn't in is still something to restore.
#[test]
fn a_background_projects_tab_is_worth_saving() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut app = App::test_app(a);
        let b_id = app.add_test_project(&b);
        app.open_tab_in_project(b_id, &b);
        app.save_session();

        let saved = the_only_session_file(&state_root);
        assert!(saved.is_some(), "a session file: {:?}", app.exit_summary);
    });
}

/// Each open project's home, in switcher order.
fn homes(app: &App) -> Vec<Option<PathBuf>> {
    app.state.open_projects().map(|p| p.home.clone()).collect()
}

/// The open project homed at `home`.
fn homed_at(app: &App, home: &Path) -> crate::project_id::ProjectId {
    app.state
        .open_projects()
        .find(|p| p.home.as_deref() == Some(home))
        .map_or_else(
            || panic!("no project homed at {}: {:?}", home.display(), homes(app)),
            |p| p.id,
        )
}

/// Project `id`'s tabs, by label.
fn tab_labels(app: &mut App, id: crate::project_id::ProjectId) -> Vec<String> {
    app.as_project(id, |app| {
        app.runtime
            .project()
            .pane_tabs
            .as_ref()
            .map_or_else(Vec::new, |tabs| {
                tabs.tabs().iter().map(|t| t.info.label.clone()).collect()
            })
    })
}

/// Run `body` with spyc's state under `tmp/state` and a home of `tmp/home`
/// holding no rc, so a project the restore opens reads no real config.
fn in_temp_home(tmp: &Path, body: impl FnOnce(&Path)) {
    let (state_root, home) = (tmp.join("state"), tmp.join("home"));
    std::fs::create_dir_all(&home).unwrap();
    crate::config::with_home_root(&home, || {
        crate::state::with_state_root(&state_root, || body(&state_root));
    });
}

/// `-r` opens every project in the manifest, each with its own tabs and
/// column `a`, in the order it saved them, and lands in the one the user was
/// in. A project out of view gets its context file, so its agents' tools
/// answer for it.
#[test]
fn a_two_project_manifest_restores_every_project() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let other = std::fs::canonicalize(tmp.path()).unwrap().join("other");
        let other_left = other.join("src");
        std::fs::create_dir_all(&other_left).unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "TWO_PROJECTS",
                1_700_000_100,
                P2,
                vec![
                    project(P1, &other, &other_left, vec![tab("theirs", &other)]),
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        let lines = picker_lines(&mut app);
        listed(&lines, "TWO_PROJECTS");
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(
            homes(&app),
            [Some(other.clone()), Some(crate_root())],
            "both projects, in the order they were saved"
        );
        assert_eq!(app.state.active_project().home, Some(crate_root()));
        assert_eq!(app.state.cur().listing.dir, crate_root());
        let mine = app.state.active;
        assert_eq!(tab_labels(&mut app, mine), ["mine"]);
        let theirs = homed_at(&app, &other);
        assert_eq!(tab_labels(&mut app, theirs), ["theirs"]);
        assert_eq!(
            app.as_project(theirs, |app| app.state.cur().listing.dir.clone()),
            other_left,
            "its column a is back where it was"
        );
        assert!(
            app.as_project(theirs, |app| app.view.project().context_path.exists()),
            "its agents have a context file to read"
        );
        assert_eq!(app.flash_text(), Some("session restored: 2 projects"));
    });
}

/// With every project opened, the restore adopts the file: later saves
/// overwrite it with every project, not just the one in view.
#[test]
fn restoring_every_project_adopts_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let other = std::fs::canonicalize(tmp.path()).unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "TWO_PROJECTS",
                1_700_000_100,
                P2,
                vec![
                    project(P1, &other, &other, vec![tab("theirs", &other)]),
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.session_id = Some(9001);
        app.state.session_name = Some("OWN_NAME".into());
        picker_lines(&mut app);
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(app.state.session_id, Some(504), "the file's id");
        assert_eq!(app.state.session_name.as_deref(), Some("TWO_PROJECTS"));
        app.save_session();
        let file = read_manifest(state_root, 504);
        let labels: Vec<_> = file["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["tabs"][0]["label"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(labels, ["theirs", "mine"], "every project, in order");
        assert!(
            !state_root.join("sessions/9001.json").exists(),
            "no second file"
        );
    });
}

/// A project whose directory has gone can't open. The rest do, as a copy: the
/// file keeps the one that didn't, and the flash names it.
#[test]
fn a_manifest_with_a_gone_project_restores_the_rest_as_a_copy() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (other, vanished) = (base.join("other"), base.join("vanished"));
        std::fs::create_dir_all(&other).unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "THREE_PROJECTS",
                1_700_000_100,
                P2,
                vec![
                    project(P1, &other, &other, vec![tab("theirs", &other)]),
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                    project(P3, &vanished, &vanished, vec![tab("lost", &other)]),
                ],
            ),
        );
        let source = std::fs::read(state_root.join("sessions/504.json")).unwrap();
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.session_id = Some(9001);
        picker_lines(&mut app);
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(homes(&app), [Some(other), Some(crate_root())]);
        assert_eq!(app.state.session_id, Some(9001), "a copy keeps its own id");
        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(
            flash.contains("as a copy") && flash.contains("vanished"),
            "{flash}"
        );
        app.save_session();
        assert_eq!(
            std::fs::read(state_root.join("sessions/504.json")).unwrap(),
            source,
            "the manifest is left as it was"
        );
        let own = read_manifest(state_root, 9001);
        assert_eq!(own["projects"].as_array().map(Vec::len), Some(2));
    });
}

/// A project out of view whose column `a` was inside an archive comes back
/// beside it, cursor on it, and the project in view doesn't move.
#[test]
fn a_project_out_of_view_inside_an_archive_comes_back_beside_it() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let other = std::fs::canonicalize(tmp.path()).unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let archive = other.join("bundle.zip");
        std::fs::write(&archive, b"a container").unwrap();
        std::fs::write(other.join("aaa.txt"), b"listed first").unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "IN_A_ZIP",
                1_700_000_100,
                P2,
                vec![
                    project(P1, &other, &archive.join("inner"), Vec::new()),
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        picker_lines(&mut app);
        let effects =
            app.handle_pager_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::empty()));
        assert!(
            !effects
                .iter()
                .any(|fx| matches!(fx, crate::app::Effect::ChangeDir { .. })),
            "nothing moves the project in view: {effects:?}"
        );
        assert_eq!(app.state.cur().listing.dir, crate_root());
        let theirs = homed_at(&app, &other);
        let (dir, cursor) = app.as_project(theirs, |app| {
            let col = app.state.cur();
            (
                col.listing.dir.clone(),
                col.rows.get(col.cursor.index).map(|r| r.path.clone()),
            )
        });
        assert_eq!(dir, other);
        assert_eq!(cursor, Some(archive));
    });
}

/// A project out of view whose tabs couldn't all open keeps them saved, and
/// the flash and session info count them with the rest.
#[test]
fn a_project_out_of_views_unopened_tabs_are_counted() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let other = std::fs::canonicalize(tmp.path()).unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let refused = json!({
            "command": "codex --future-option", "label": "refused", "cwd": other,
            "agent_kind": "codex", "agent_session_id": UUID,
            "codex_home": other.join("codex-home")
        });
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "PARTIAL",
                1_700_000_100,
                P2,
                vec![
                    project(P1, &other, &other, vec![refused]),
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        picker_lines(&mut app);
        press(&mut app, KeyCode::Char('1'));

        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(flash.contains("1 tab(s) unopened"), "{flash}");
        let theirs = homed_at(&app, &other);
        assert_eq!(
            app.as_project(theirs, |app| app.state.active_project().deferred_tabs.len()),
            1
        );
        app.show_session_info();
        let info: Vec<String> = app
            .view
            .project()
            .pager
            .as_ref()
            .expect("session info")
            .lines
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            info.iter().any(|l| l.contains("refused")),
            "session info lists it: {info:#?}"
        );
    });
}

/// A project out of view whose `.spycrc.toml` doesn't parse still opens, on
/// the defaults, and the flash names it with the error.
#[test]
fn a_restored_projects_config_error_is_named() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let other = std::fs::canonicalize(tmp.path()).unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join(".spycrc.toml"), "[layout\n").unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "BAD_RC",
                1_700_000_100,
                P2,
                vec![
                    project(P1, &other, &other, Vec::new()),
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        picker_lines(&mut app);
        press(&mut app, KeyCode::Char('1'));

        homed_at(&app, &other);
        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(
            flash.starts_with("session restored: 2 projects; other: config error"),
            "{flash}"
        );
    });
}

/// #99 step 7a: a project out of view whose home a file under `projects/`
/// names is configured by that file as it opens, named for it, though its
/// start directory is below its home.
#[test]
fn a_restored_project_out_of_view_is_configured_by_its_file() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (other, config) = (base.join("other"), base.join("config"));
        let sub = other.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(config.join("projects")).unwrap();
        std::fs::write(
            config.join("projects").join("work.toml"),
            format!("home = {:?}\n", other.display().to_string()),
        )
        .unwrap();
        let mut theirs = project(P1, &other, &sub, Vec::new());
        theirs["start_dir"] = json!(sub);
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "FILED",
                1_700_000_100,
                P2,
                vec![
                    theirs,
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                ],
            ),
        );
        crate::state::with_config_root(&config, || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            picker_lines(&mut app);
            press(&mut app, KeyCode::Char('1'));
            let theirs = homed_at(&app, &other);
            assert_eq!(app.state.label_of(theirs).as_deref(), Some("work"));
        });
    });
}

/// `-r` into the project spyc started in brings in the file that names the
/// home it restores, at the next settle.
#[test]
fn a_restore_into_the_launch_project_reads_its_file() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (other, config) = (base.join("other"), base.join("config"));
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir_all(config.join("projects")).unwrap();
        std::fs::write(
            config.join("projects").join("work.toml"),
            format!("home = {:?}\n", other.display().to_string()),
        )
        .unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "ONE",
                1_700_000_100,
                P1,
                vec![project(P1, &other, &other, Vec::new())],
            ),
        );
        crate::state::with_config_root(&config, || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            // As a launch records its config's inputs.
            let inputs = app.state.active_project().config_inputs();
            app.state.active_project_mut().config_key = Some(inputs);
            picker_lines(&mut app);
            press(&mut app, KeyCode::Char('1'));
            assert!(app.settle_project_configs());
            assert_eq!(app.state.active_project().label(), "work");
        });
    });
}

/// When the project the user was in has gone (its worktree removed after its
/// merge, say), the first project that can comes back in view and the rest
/// beside it, as a copy that names the one that didn't open.
#[test]
fn with_the_active_project_gone_the_rest_restore_as_a_copy() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (other, vanished) = (base.join("other"), base.join("vanished"));
        std::fs::create_dir_all(&other).unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "LEFT_A_WORKTREE",
                1_700_000_100,
                P1,
                vec![
                    project(P1, &vanished, &vanished, vec![tab("lost", &other)]),
                    project(P2, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                    project(P3, &other, &other, vec![tab("theirs", &other)]),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.session_id = Some(9001);
        picker_lines(&mut app);
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(homes(&app), [Some(crate_root()), Some(other)]);
        assert_eq!(app.state.active_project().home, Some(crate_root()));
        assert_eq!(app.state.session_id, Some(9001), "a copy keeps its own id");
        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(
            flash.contains("2 of 3") && flash.contains("vanished"),
            "{flash}"
        );
    });
}

/// A copy of a running session starts every project's agents fresh, not just
/// those of the project in view: none takes that spyc's conversations, tab
/// ids or claim owners.
#[test]
fn a_running_copys_projects_out_of_view_start_their_agents_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let other = dir.join("other");
        std::fs::create_dir_all(&other).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let live = std::os::unix::process::parent_id();
        let agent = json!({
            "command": command, "label": "agent", "cwd": other,
            "agent_kind": "codex", "agent_session_id": UUID,
            "codex_home": dir.join("codex-home"),
            "claim_owner": "their-owner", "pane_id": "their-pane"
        });
        let mut saved = manifest(
            507,
            "LIVE_TWO",
            1_700_000_100,
            P2,
            vec![
                project(P1, &other, &other, vec![agent]),
                project(P2, &crate_root(), &crate_root(), Vec::new()),
            ],
        );
        saved["pid"] = json!(live);
        saved["process_started"] = json!(started_at(live));
        write_v1_decoy(state_root);
        write_manifest(state_root, 507, &saved);
        let mut app = App::test_app(dir.clone());
        picker_lines(&mut app);
        press(&mut app, KeyCode::Char('1'));

        let theirs = homed_at(&app, &other);
        let (id, owner) = app.as_project(theirs, |app| {
            let tabs = app.runtime.project().pane_tabs.as_ref().expect("the tab");
            let info = &tabs.tabs()[0].info;
            (info.id.clone(), info.claim_owner.clone())
        });
        assert_ne!(id, "their-pane");
        assert_ne!(owner, "their-owner");
        let argv = agent_argv(&dir, "codex");
        assert!(
            !argv.iter().any(|arg| arg == "resume" || arg == UUID),
            "the agent starts fresh: {argv:?}"
        );
    });
}

/// A home is one project's, so a second project of the file at the same home
/// stays shut, and the restore is a copy.
#[test]
fn a_second_project_at_the_same_home_stays_shut() {
    let tmp = tempfile::tempdir().unwrap();
    in_temp_home(tmp.path(), |state_root| {
        let other = std::fs::canonicalize(tmp.path()).unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        write_v1_decoy(state_root);
        write_manifest(
            state_root,
            504,
            &manifest(
                504,
                "TWICE_HOMED",
                1_700_000_100,
                P1,
                vec![
                    project(P1, &crate_root(), &crate_root(), vec![tab("mine", &other)]),
                    project(P2, &crate_root(), &other, vec![tab("again", &other)]),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.session_id = Some(9001);
        picker_lines(&mut app);
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(homes(&app), [Some(crate_root())]);
        assert_eq!(app.state.session_id, Some(9001), "a copy keeps its own id");
    });
}

/// With several projects in a manifest, its row lists them, each with its tab
/// count, in place of one directory.
#[test]
fn the_picker_lists_a_manifests_projects() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (spyc, web, api) = (base.join("spyc"), base.join("web"), base.join("api"));
        write_v1_decoy(&state_root);
        write_manifest(
            &state_root,
            505,
            &manifest(
                505,
                "THREE_PROJECTS",
                1_700_000_100,
                P1,
                vec![
                    project(
                        P1,
                        &spyc,
                        &spyc,
                        vec![tab("a", &spyc), tab("b", &spyc), tab("c", &spyc)],
                    ),
                    project(P2, &web, &web, vec![tab("d", &web)]),
                    project(P3, &api, &api, Vec::new()),
                ],
            ),
        );
        let mut app = App::test_app(tmp.path().to_path_buf());
        let lines = picker_lines(&mut app);
        let row = listed(&lines, "THREE_PROJECTS");
        assert!(
            row.contains("spyc (3 tabs) · web (1) · api"),
            "the projects, each with its tabs: {row}"
        );
    });
}

/// A manifest whose spyc is still running says so. Its pid alone isn't proof:
/// a pid outlives its process, so one now held by a process that started at
/// another time, or by none, isn't running.
#[test]
fn the_picker_marks_a_manifest_whose_spyc_is_running() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let live = std::os::unix::process::parent_id();
        let rows = [
            (601, "LIVE_ONE", live, started_at(live)),
            (602, "REUSED_PID", live, 1),
            (603, "DEAD_PID", dead_pid(), started_at(live)),
        ];
        write_v1_decoy(&state_root);
        for (id, name, pid, started) in rows {
            let mut saved = manifest(
                id,
                name,
                1_700_000_000 + id,
                P1,
                vec![project(P1, &crate_root(), &crate_root(), Vec::new())],
            );
            saved["pid"] = json!(pid);
            saved["process_started"] = json!(started);
            write_manifest(&state_root, id, &saved);
        }
        let mut app = App::test_app(tmp.path().to_path_buf());
        let lines = picker_lines(&mut app);
        assert!(listed(&lines, "LIVE_ONE").contains("running"), "{lines:#?}");
        for name in ["REUSED_PID", "DEAD_PID", "V1_DECOY"] {
            assert!(!listed(&lines, name).contains("running"), "{lines:#?}");
        }
    });
}

/// Restoring a manifest another spyc is still running makes a copy, and the
/// copy takes nothing that spyc's agents still hold: not its file, its name,
/// its tab ids, its scope claims, or its conversations, which start fresh.
#[test]
fn restoring_a_running_manifest_makes_a_copy_whose_agents_start_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let command = fake_agent(&dir, "codex").display().to_string();
        let live = std::os::unix::process::parent_id();
        let agent = json!({
            "command": command, "label": "agent", "cwd": dir,
            "agent_kind": "codex", "agent_session_id": UUID,
            "codex_home": dir.join("codex-home"),
            "claim_owner": "their-owner", "pane_id": "their-pane"
        });
        let mut saved = manifest(
            506,
            "LIVE_SOURCE",
            1_700_000_100,
            P1,
            vec![project(P1, &crate_root(), &crate_root(), vec![agent])],
        );
        saved["pid"] = json!(live);
        saved["process_started"] = json!(started_at(live));
        saved["scope_claims"] = json!([{
            "id": 7, "owner": "their-owner", "owner_label": "agent",
            "paths": ["src/*.rs"], "intent": "editing", "pr": null, "note": null,
            "claimed_at_secs": 1
        }]);
        write_v1_decoy(&state_root);
        write_manifest(&state_root, 506, &saved);
        let mut app = App::test_app(dir.clone());
        app.state.session_id = Some(9001);
        app.state.session_name = Some("OWN_NAME".into());
        let lines = picker_lines(&mut app);
        listed(&lines, "LIVE_SOURCE");
        press(&mut app, KeyCode::Char('1'));

        assert_eq!(app.state.session_id, Some(9001), "its own file");
        assert_eq!(app.state.session_name.as_deref(), Some("OWN_NAME"));
        assert!(app.state.scope_registry.is_empty(), "no claims of theirs");
        let tabs = app.runtime.project().pane_tabs.as_ref().expect("the tab");
        let info = &tabs.tabs()[0].info;
        assert_ne!(info.id, "their-pane");
        assert_ne!(info.claim_owner, "their-owner");
        let argv = agent_argv(&dir, "codex");
        assert!(
            !argv.iter().any(|arg| arg == "resume" || arg == UUID),
            "the agent starts fresh: {argv:?}"
        );
    });
}

/// Tabs a restore couldn't open stay saved with the project they were restored
/// into, never with another open project.
#[test]
fn unopened_tabs_stay_with_their_project() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let saved = parsed(json!({
            "id": 700, "name": "PARTIAL", "saved_at": "", "epoch_secs": 1_700_000_000,
            "cwd": crate_root(),
            "tabs": [
                { "command": "codex --future-option", "label": "refused", "cwd": dir,
                  "agent_kind": "codex", "agent_session_id": UUID,
                  "codex_home": dir.join("codex-home") },
                tab("kept", &dir)
            ],
            "active_tab": 1, "pane_height_pct": 30, "pane_focused": false
        }));
        let mut app = App::test_app(dir.clone());
        app.restore_session(&saved);
        assert_eq!(app.state.active_project().deferred_tabs.len(), 1);
        let b = dir.join("b");
        std::fs::create_dir_all(&b).unwrap();
        let b_id = app.add_test_project(&b);
        app.open_tab_in_project(b_id, &b);
        app.save_session();

        let file = read_manifest(&state_root, 700);
        let labels = |project: usize| -> Vec<String> {
            file["projects"][project]["tabs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tab| tab["label"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(labels(0), ["refused", "kept"]);
        assert_eq!(labels(1), ["cat"]);
    });
}

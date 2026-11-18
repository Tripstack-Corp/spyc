//! `:setenv`: given `NAME=VALUE` it sets the variable for the project in
//! view, bare it asks, and with `-p` a saved session keeps it. What a program
//! started there sees is the evidence, so each test reads the variable back
//! from a tab.

use std::path::Path;
use std::time::{Duration, Instant};

use super::manifest_v2::read_manifest;
use super::projects_open::with_three;
use super::*;

/// What a tab started in `dir`, in the project in view, sees as `$name`:
/// `unset` when it has none.
fn seen_by_a_tab(app: &mut App, dir: &Path, name: &str) -> String {
    let out = dir.join(format!("{name}.seen"));
    let _ = std::fs::remove_file(&out);
    let cmd = seen_command(dir, name);
    assert!(app.open_pane_tab_in(&cmd, dir), "{:?}", app.flash_text());
    read_when_written(&out)
}

/// A command that writes what it sees as `$name` to `dir/<name>.seen`.
fn seen_command(dir: &Path, name: &str) -> String {
    let out = dir.join(format!("{name}.seen"));
    let script = format!(
        "printf %s \"${{{name}-unset}}\" > {tmp} && mv {tmp} {out}",
        tmp = crate::shell::shell_quote(&format!("{}.tmp", out.display())),
        out = crate::shell::shell_quote(&out.display().to_string()),
    );
    format!("sh -c {}", crate::shell::shell_quote(&script))
}

fn read_when_written(out: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(seen) = std::fs::read_to_string(out) {
            return seen;
        }
        assert!(Instant::now() < deadline, "nothing wrote {}", out.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn with_dir(test: impl FnOnce(&mut App, &Path)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());
        test(&mut app, &dir);
    });
}

#[test]
fn setenv_with_an_assignment_sets_it_without_asking() {
    with_dir(|app, dir| {
        let fx = app.dispatch_command("setenv SPYC_SETENV_ARG_TEST=1");
        assert!(fx.is_empty(), "{fx:?}");
        assert!(
            !matches!(app.state.mode, Mode::Prompting(_)),
            "a prompt opened for a value already given"
        );
        assert_eq!(app.flash_text(), Some("setenv SPYC_SETENV_ARG_TEST=1"));
        assert_eq!(seen_by_a_tab(app, dir, "SPYC_SETENV_ARG_TEST"), "1");
    });
}

/// The value is everything after the first `=`, spaces and `=` included.
#[test]
fn setenv_keeps_the_whole_value() {
    with_dir(|app, dir| {
        app.dispatch_command("setenv SPYC_SETENV_VALUE_TEST=a b=c");
        assert_eq!(seen_by_a_tab(app, dir, "SPYC_SETENV_VALUE_TEST"), "a b=c");
    });
}

#[test]
fn bare_setenv_asks_for_the_assignment() {
    let mut app = App::test_app(std::env::temp_dir());
    app.dispatch_command("setenv");
    assert!(
        matches!(
            app.state.mode,
            Mode::Prompting(Prompt {
                kind: PromptKind::SetEnv,
                ..
            })
        ),
        "no setenv prompt"
    );
}

#[test]
fn setenv_without_an_equals_sign_says_so() {
    with_dir(|app, dir| {
        app.dispatch_command("setenv SPYC_SETENV_NO_VALUE_TEST");
        assert!(!matches!(app.state.mode, Mode::Prompting(_)));
        assert_eq!(app.flash_text(), Some("setenv: expected NAME=VALUE"));
        assert_eq!(
            seen_by_a_tab(app, dir, "SPYC_SETENV_NO_VALUE_TEST"),
            "unset"
        );
    });
}

/// A variable is the project's: a tab in another project doesn't get it, and
/// one in its own still does after a trip away.
#[test]
fn a_variable_set_in_one_project_stays_out_of_another() {
    with_three(|t| {
        t.app.dispatch_command("setenv SPYC_SETENV_PROJECT_TEST=x");
        t.app.switch_project(t.y);
        let (y_home, x_home) = (t.y_home.clone(), t.x_home.clone());
        assert_eq!(
            seen_by_a_tab(&mut t.app, &y_home, "SPYC_SETENV_PROJECT_TEST"),
            "unset",
            "project y never set it"
        );
        t.app.switch_project(t.x);
        assert_eq!(
            seen_by_a_tab(&mut t.app, &x_home, "SPYC_SETENV_PROJECT_TEST"),
            "x"
        );
    });
}

/// The saved session file of `t`'s app, as JSON.
fn saved(t: &mut super::projects_open::Three) -> serde_json::Value {
    t.app.state.session_id = Some(9311);
    t.app.save_session();
    read_manifest(&t.home.parent().unwrap().join("state"), 9311)
}

/// The saved project homed at `home`.
fn saved_project<'a>(saved: &'a serde_json::Value, home: &Path) -> &'a serde_json::Value {
    saved["projects"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["home"] == serde_json::json!(home)))
        .unwrap_or_else(|| panic!("{} not saved: {saved:#}", home.display()))
}

/// `-p` saves the variable with its project, and says it is saved as plain
/// text; a project that saved none has no variables in the file.
#[test]
fn setenv_p_saves_the_variable_with_its_project() {
    with_three(|t| {
        t.app
            .dispatch_command("setenv -p SPYC_SETENV_SAVED_TEST=kept");
        let flash = t.app.flash_text().unwrap_or_default().to_owned();
        assert!(
            flash.starts_with("setenv SPYC_SETENV_SAVED_TEST=kept"),
            "{flash}"
        );
        assert!(flash.contains("plain text"), "{flash}");
        let x_home = t.x_home.clone();
        assert_eq!(
            seen_by_a_tab(&mut t.app, &x_home, "SPYC_SETENV_SAVED_TEST"),
            "kept"
        );
        let saved = saved(t);
        assert_eq!(
            saved_project(&saved, &t.x_home)["env"],
            serde_json::json!({"SPYC_SETENV_SAVED_TEST": "kept"}),
        );
        assert!(saved_project(&saved, &t.y_home).get("env").is_none());
    });
}

/// Without `-p` nothing reaches the file, and a plain `:setenv` of a saved
/// variable takes it back out: the last one says.
#[test]
fn plain_setenv_is_never_saved() {
    with_three(|t| {
        let x_home = t.x_home.clone();
        assert!(t.app.open_pane_tab_in("cat", &x_home));
        t.app.dispatch_command("setenv SPYC_SETENV_RUN_TEST=secret");
        t.app
            .dispatch_command("setenv -p SPYC_SETENV_FLIP_TEST=one");
        t.app.dispatch_command("setenv SPYC_SETENV_FLIP_TEST=two");
        let saved = saved(t);
        assert!(
            saved_project(&saved, &t.x_home).get("env").is_none(),
            "{saved:#}"
        );
        assert!(!saved.to_string().contains("secret"), "{saved:#}");
    });
}

/// A `-p` variable is reason enough to save a session, and autosave notices
/// it.
#[test]
fn a_saved_variable_alone_is_saved_and_autosaved() {
    with_three(|t| {
        let before = t.app.session_fingerprint();
        t.app.dispatch_command("setenv -p SPYC_SETENV_ALONE_TEST=1");
        assert_ne!(
            t.app.session_fingerprint(),
            before,
            "autosave would miss it"
        );
        let saved = saved(t);
        assert_eq!(
            saved_project(&saved, &t.x_home)["env"],
            serde_json::json!({"SPYC_SETENV_ALONE_TEST": "1"}),
        );
    });
}

/// spyc's own reads (`$EDITOR`, `$SHELL`, …) follow the project in view.
#[test]
fn spycs_own_reads_follow_the_project_in_view() {
    with_three(|t| {
        t.app.dispatch_command("setenv SPYC_SETENV_OWN_TEST=x");
        t.app.settle_env();
        assert_eq!(
            crate::envset::var("SPYC_SETENV_OWN_TEST").as_deref(),
            Some("x")
        );
        t.app.switch_project(t.y);
        assert_eq!(crate::envset::var("SPYC_SETENV_OWN_TEST"), None);
        t.app.switch_project(t.x);
        assert_eq!(
            crate::envset::var("SPYC_SETENV_OWN_TEST").as_deref(),
            Some("x")
        );
    });
}

/// `-r` gives a project its variables back before its tabs start, so a
/// restored tab sees them.
#[test]
fn a_restored_project_starts_its_tabs_with_its_variables() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    crate::state::with_state_root(&state_root, || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let name = "SPYC_SETENV_RESTORED_TEST";
        let mut app = App::test_app(dir.clone());
        app.state.session_id = Some(9312);
        assert!(app.open_pane_tab_in(&seen_command(&dir, name), &dir));
        assert_eq!(
            read_when_written(&dir.join(format!("{name}.seen"))),
            "unset"
        );
        app.save_session();
        drop(app);
        std::fs::remove_file(dir.join(format!("{name}.seen"))).unwrap();

        let mut saved = read_manifest(&state_root, 9312);
        saved["projects"][0]["env"] = serde_json::json!({ name: "back" });
        let manifest = crate::state::sessions::parse_manifest(saved).expect("parses");
        let mut again = App::test_app(dir.clone());
        again.restore_session(&manifest);
        assert_eq!(read_when_written(&dir.join(format!("{name}.seen"))), "back");
        again.save_session();
        assert_eq!(
            read_manifest(&state_root, 9312)["projects"][0]["env"],
            serde_json::json!({ name: "back" }),
            "a restored variable stays saved"
        );
    });
}

/// `:unsetenv NAME` takes a variable back out of the project.
#[test]
fn unsetenv_removes_the_variable() {
    with_dir(|app, dir| {
        app.dispatch_command("setenv SPYC_SETENV_UNSET_TEST=1");
        app.dispatch_command("unsetenv SPYC_SETENV_UNSET_TEST");
        assert_eq!(app.flash_text(), Some("unsetenv SPYC_SETENV_UNSET_TEST"));
        assert_eq!(seen_by_a_tab(app, dir, "SPYC_SETENV_UNSET_TEST"), "unset");
    });
}

/// A tab whose program runs in a jar gets them too: the jar starts it.
#[test]
fn a_jarred_tab_gets_the_projects_variables() {
    use super::jar_start::{built_jar, jar_app, land, short_dir};
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        app.dispatch_command("setenv SPYC_SETENV_JAR_TEST=jarred");
        assert_eq!(
            seen_by_a_tab(&mut app, dir.path(), "SPYC_SETENV_JAR_TEST"),
            "jarred"
        );
        land(&mut app);
    });
}

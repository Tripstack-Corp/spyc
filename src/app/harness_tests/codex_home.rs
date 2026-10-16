//! Pane-scoped home and session binding at the actual App call sites.
use super::*;
use std::path::{Path, PathBuf};

const SID: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";

fn launch(app: &mut App, cwd: &Path, home: &Path) {
    std::fs::create_dir_all(home).unwrap();
    let command = fake_agent(cwd, "codex").display().to_string();
    assert!(app.open_pane_tab_in_home(&command, cwd, Some(home)));
    agent_argv(cwd, "codex");
    let child_home = std::fs::read_to_string(cwd.join("codex.argv.home")).unwrap();
    assert_eq!(
        PathBuf::from(child_home.trim()),
        home.canonicalize().unwrap()
    );
    std::fs::remove_file(cwd.join("codex.argv")).unwrap();
}

fn rollout(home: &Path, message: &str) {
    let day = home.join("sessions/2026/10/09");
    std::fs::create_dir_all(&day).unwrap();
    let timestamp = jiff::Timestamp::now().to_string();
    let body = [
        serde_json::json!({"type":"session_meta","payload":{"id":SID,"cwd":home.parent().unwrap(),"timestamp":timestamp}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":message}}),
    ].iter().map(serde_json::Value::to_string).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::write(day.join(format!("rollout-now-{SID}.jsonl")), body).unwrap();
}

fn wait_for_scan(app: &App) {
    for _ in 0..500 {
        if app.runtime.codex_pin_pending.lock().unwrap().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the home-scoped scan did not land");
}

#[test]
fn codex_home_is_captured_and_preserved_by_fork_restore_and_both_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("custom");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .codex_session_id = Some(SID.into());

        app.fork_active_tab();
        assert_eq!(agent_argv(&cwd, "codex"), ["--no-daemon", "fork", SID]);
        assert_eq!(
            std::fs::read_to_string(cwd.join("codex.argv.home"))
                .unwrap()
                .trim(),
            home.to_str().unwrap()
        );
        std::fs::remove_file(cwd.join("codex.argv")).unwrap();

        app.restart_active_tab_now();
        agent_argv(&cwd, "codex");
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .codex_home
                .as_ref(),
            Some(&home)
        );
        std::fs::remove_file(cwd.join("codex.argv")).unwrap();

        let command = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .command
            .clone();
        assert!(app.spawn_agent_into_tab(1, &command, &cwd, None));
        agent_argv(&cwd, "codex");
        assert_eq!(
            std::fs::read_to_string(cwd.join("codex.argv.home"))
                .unwrap()
                .trim(),
            home.to_str().unwrap()
        );
        std::fs::remove_file(cwd.join("codex.argv")).unwrap();

        let session: crate::state::sessions::Session = serde_json::from_value(serde_json::json!({
            "id":1,"saved_at":"","epoch_secs":0,"cwd":env!("CARGO_MANIFEST_DIR"),
            "tabs":[{"command":command,"label":"codex","cwd":cwd,"agent_kind":"codex","agent_session_id":SID,"codex_home":home}],
            "active_tab":0,"pane_height_pct":50,"pane_focused":true
        })).unwrap();
        app.restore_session(&session);
        assert_eq!(agent_argv(&cwd, "codex"), ["--no-daemon", "resume", SID]);
        assert_eq!(
            std::fs::read_to_string(cwd.join("codex.argv.home"))
                .unwrap()
                .trim(),
            home.to_str().unwrap()
        );
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .codex_home
                .as_ref(),
            Some(&home)
        );
    });
}

#[test]
fn codex_scan_keeps_claims_separate_for_two_homes_in_one_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let first = cwd.join("first");
        let second = cwd.join("second");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &first);
        launch(&mut app, &cwd, &second);
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .tabs_mut()[0]
            .info
            .codex_session_id = Some(SID.into());
        rollout(&first, "first home's history");
        rollout(&second, "second home's history");
        app.kick_codex_session_scan();
        wait_for_scan(&app);
        app.apply_codex_session_pins();
        let tabs = app.runtime.project().pane_tabs.as_ref().unwrap().tabs();
        assert_eq!(tabs[0].info.codex_session_id.as_deref(), Some(SID));
        assert_eq!(tabs[1].info.codex_session_id.as_deref(), Some(SID));
        assert_eq!(tabs[1].info.codex_home.as_ref(), Some(&second));
    });
}

#[test]
fn codex_scan_cannot_pin_a_replacement_pane_from_an_old_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("custom");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        rollout(&home, "old pane");
        app.kick_codex_session_scan();
        wait_for_scan(&app);
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .id = "replacement-pane".into();
        app.apply_codex_session_pins();
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .codex_session_id,
            None
        );
    });
}

#[test]
fn codex_scrollback_reads_only_the_active_panes_home() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let home = cwd.join("custom");
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &home);
        rollout(&home, "this pane's custom history");
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .codex_session_id = Some(SID.into());
        app.open_pane_scroll_pager();
        for _ in 0..500 {
            app.drain_pager_stream();
            if app
                .view
                .scroll_pager
                .as_ref()
                .is_some_and(|pager| !pager.streaming)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let pager = app
            .view
            .scroll_pager
            .as_ref()
            .expect("the custom-home transcript is mounted");
        let text = pager
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("this pane's custom history"), "{text}");
    });
}

#[test]
fn codex_home_is_the_environment_after_shell_rc() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let after_rc = cwd.join("after-rc");
        std::fs::create_dir(&after_rc).unwrap();
        let shell = cwd.join("rc-shell");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\nexport HOME={}\nexport CODEX_HOME={}\nexec /bin/sh \"$@\"\n",
                crate::shell::shell_quote(cwd.to_str().unwrap()),
                crate::shell::shell_quote(after_rc.to_str().unwrap()),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        crate::shell::with_test_shell(shell.to_str().unwrap(), || {
            let mut app = App::test_app(cwd.clone());
            let command = format!("{} resume {SID}", fake_agent(&cwd, "codex").display());
            assert!(app.open_pane_tab_in(&command, &cwd));
            agent_argv(&cwd, "codex");
            if app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .codex_home
                .is_none()
            {
                app.kick_codex_session_scan();
                wait_for_scan(&app);
                app.apply_codex_session_pins();
            }
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .codex_home
                    .as_ref(),
                Some(&after_rc)
            );
            let changed = cwd.join("changed-home");
            std::fs::create_dir(&changed).unwrap();
            std::fs::write(
                &shell,
                format!(
                    "#!/bin/sh\nexport HOME={}\nexport CODEX_HOME={}\nexec /bin/sh \"$@\"\n",
                    crate::shell::shell_quote(cwd.to_str().unwrap()),
                    crate::shell::shell_quote(changed.to_str().unwrap()),
                ),
            )
            .unwrap();
            std::fs::remove_file(cwd.join("codex.argv")).unwrap();
            app.fork_active_tab();
            agent_argv(&cwd, "codex");
            assert_eq!(
                std::fs::read_to_string(cwd.join("codex.argv.home"))
                    .unwrap()
                    .trim(),
                after_rc.to_str().unwrap()
            );
            std::fs::remove_file(cwd.join("codex.argv")).unwrap();
            app.restart_active_tab_now();
            agent_argv(&cwd, "codex");
            assert_eq!(
                std::fs::read_to_string(cwd.join("codex.argv.home"))
                    .unwrap()
                    .trim(),
                after_rc.to_str().unwrap()
            );
        });
    });
}

#[test]
fn codex_without_a_known_home_cannot_fork_or_restart_into_another_home() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let cwd = tmp.path().canonicalize().unwrap();
        let mut app = App::test_app(cwd.clone());
        launch(&mut app, &cwd, &cwd.join("custom"));
        let info = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut();
        info.codex_home = None;
        info.codex_session_id = Some(SID.into());
        let id = info.id.clone();
        app.fork_active_tab();
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .tabs()
                .len(),
            1
        );
        assert!(app.flash_text().unwrap().contains("home is not identified"));
        app.restart_active_tab_now();
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .id,
            id
        );
        let command = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .command
            .clone();
        assert!(!app.spawn_agent_into_tab(0, &command, &cwd, None));
        assert_eq!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .id,
            id
        );
    });
}

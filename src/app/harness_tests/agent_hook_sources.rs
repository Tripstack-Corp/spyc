use super::agent_readiness::{agent_app, dump};
use super::*;

fn linked_hook_repo(parent: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let main = parent.join("main");
    std::fs::create_dir(&main).unwrap();
    crate::git::test_support::run_git(&main, &["init", "-q", "--initial-branch=main"]);
    std::fs::write(main.join("README"), "fixture\n").unwrap();
    crate::git::test_support::run_git(&main, &["add", "README"]);
    crate::git::test_support::run_git(&main, &["commit", "-q", "-m", "fixture"]);
    let main = std::fs::canonicalize(main).unwrap();
    let linked = crate::git::worktree::add(&main, "question-fixture", None).unwrap();
    (main, linked)
}

#[test]
fn codex_linked_hooks_install_claim_and_cleanup_the_root_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (main, linked) = linked_hook_repo(tmp.path());
        std::fs::create_dir(main.join(".codex")).unwrap();
        let config = main.join(".codex/config.toml");
        std::fs::write(
            &config,
            "[mcp_servers.user]\ncommand='keep'\n[hooks.state.user]\ntrusted_hash='preserve'\n",
        )
        .unwrap();
        let mut app = agent_app(&linked, "codex");
        app.install_status_hooks(&linked, crate::state::sessions::AgentKind::Codex);
        let contents = std::fs::read_to_string(&config).unwrap();
        assert!(
            contents.contains("codex-question-start"),
            "native hooks belong in root checkout"
        );
        assert!(
            !linked.join(".codex/config.toml").exists(),
            "installation must not write ignored declarations"
        );
        let info = app.runtime.pane_tabs.as_ref().unwrap().active_info();
        let explanation = crate::app::status_hooks::status_hooks_diagnostic(info).unwrap();
        assert!(
            explanation.contains(&main.display().to_string()),
            "{explanation}"
        );
        crate::state::dir_owners::claim(crate::state::dir_owners::Shared::StatusHooks, &main, 1);
        app.cleanup_written_mcp_configs();
        assert!(
            std::fs::read_to_string(&config)
                .unwrap()
                .contains("--report-status"),
            "sibling still owns shared hooks"
        );
        assert!(crate::state::dir_owners::release(
            crate::state::dir_owners::Shared::StatusHooks,
            &main,
            1
        ));
        crate::mcp::cleanup_codex_status_hooks(&main);
        let after = std::fs::read_to_string(&config).unwrap();
        assert!(!after.contains("--report-status"));
        assert!(after.contains("preserve"));
        assert!(after.contains("keep"));
    });
}

#[test]
fn codex_linked_hooks_ignore_unused_worktree_markers() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (_main, linked) = linked_hook_repo(tmp.path());
        assert!(crate::mcp::ensure_codex_status_hooks(&linked));
        let support = crate::agent::profile_for(crate::state::sessions::AgentKind::Codex)
            .status_hooks()
            .unwrap();
        assert!(
            !support.installed(&linked),
            "ignored worktree declarations cannot prove installed hooks"
        );
        let mut app = agent_app(&linked, "codex");
        assert!(dump(&mut app).contains("MISSING (`:hooks on`)"));
    });
}

#[test]
fn codex_linked_hooks_preinstall_uses_consent_for_the_actual_source() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (main, linked) = linked_hook_repo(tmp.path());
        crate::state::hook_consent::set_consent(&main, true);
        crate::state::hook_consent::set_consent(&linked, false);
        let mut app = App::test_app(linked.clone());
        app.view.mcp_running = true;
        let command = fake_agent(tmp.path(), "codex").display().to_string();
        assert!(app.open_pane_tab_in(&command, &linked));
        assert!(
            main.join(".codex/config.toml").exists(),
            "pre-spawn installation needs canonical-source consent"
        );
        assert!(
            app.runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .status_hooks_at_spawn
        );
        let ordinary = std::fs::read_to_string(linked.join(".codex/config.toml")).unwrap();
        assert!(
            ordinary.contains("mcp_servers"),
            "ordinary config stays worktree-local"
        );
        assert!(!ordinary.contains("--report-status"));
        assert_eq!(app.status_hooks_target_root(), main);
        app.set_status_hooks(false);
        assert!(
            !crate::agent::profile_for(crate::state::sessions::AgentKind::Codex)
                .status_hooks()
                .unwrap()
                .installed(&linked)
        );
        assert!(
            linked.join(".codex/config.toml").exists(),
            "disabling hooks keeps the MCP entry"
        );
        app.cleanup_written_mcp_configs();
        assert!(
            !linked.join(".codex/config.toml").exists(),
            "teardown still owns the MCP entry"
        );
    });
}

#[test]
fn codex_linked_hooks_source_change_marks_every_affected_tab_for_restart() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (main, linked) = linked_hook_repo(tmp.path());
        let mut app = agent_app(&main, "codex");
        assert!(app.open_pane_tab_in("cat", &linked));
        app.runtime
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .command = "codex".into();
        app.install_status_hooks(&linked, crate::state::sessions::AgentKind::Codex);
        assert!(
            app.runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .tabs()
                .iter()
                .all(|t| t.info.status_hooks_restart_needed)
        );
    });
}

#[test]
fn codex_linked_hooks_map_subdirectories_without_redirecting_other_agents() {
    let tmp = tempfile::tempdir().unwrap();
    let (main, linked) = linked_hook_repo(tmp.path());
    let sub = linked.join("nested");
    std::fs::create_dir(&sub).unwrap();
    let profile = crate::agent::profile_for(crate::state::sessions::AgentKind::Codex);
    assert_eq!(
        (profile.status_hooks().unwrap().config_dir)(&sub),
        main.join("nested")
    );
    assert_eq!(crate::git::discovery::root_checkout_dir(&main), main);
    assert_eq!(
        crate::git::discovery::root_checkout_dir(tmp.path()),
        tmp.path()
    );
    for kind in [
        crate::state::sessions::AgentKind::Claude,
        crate::state::sessions::AgentKind::Agy,
    ] {
        assert_eq!(
            (crate::agent::profile_for(kind)
                .status_hooks()
                .unwrap()
                .config_dir)(&sub),
            sub
        );
    }
}

#[test]
fn codex_linked_hooks_worktree_consent_never_grants_root_checkout_consent() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (main, linked) = linked_hook_repo(tmp.path());
        crate::state::hook_consent::set_consent(&linked, true);
        let command = fake_agent(tmp.path(), "codex").display().to_string();
        let mut app = App::test_app(linked.clone());
        app.view.mcp_running = true;
        assert!(app.open_pane_tab_in(&command, &linked));
        assert!(
            !main.join(".codex/config.toml").exists(),
            "worktree consent cannot authorize another source"
        );
        let Mode::Prompting(ref prompt) = app.state.mode else {
            panic!("actual source needs consent");
        };
        assert!(matches!(&prompt.kind, PromptKind::HookConsent { root, .. } if root == &main));
        assert!(
            prompt
                .prefix
                .contains(&main.join(".codex/config.toml").display().to_string()),
            "consent names the file it will write"
        );
        crate::state::hook_consent::set_consent(&main, false);
        app.maybe_preinstall_startup_hooks(&command, &linked);
        assert!(!main.join(".codex/config.toml").exists());
    });
}

#[test]
fn codex_linked_hooks_reheal_consults_the_root_checkout_and_its_consent() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (main, linked) = linked_hook_repo(tmp.path());
        crate::state::hook_consent::set_consent(&main, true);
        crate::state::hook_consent::set_consent(&linked, false);
        crate::mcp::ensure_codex_status_hooks(&linked);
        let ignored = std::fs::read(linked.join(".codex/config.toml")).unwrap();
        let mut app = agent_app(&linked, "codex");
        app.view.mcp_running = true;
        let now = std::time::Instant::now();
        assert!(app.settle_status_hooks(now));
        let config = main.join(".codex/config.toml");
        assert!(
            std::fs::read_to_string(&config)
                .unwrap()
                .contains("codex-question-start")
        );
        assert_eq!(
            std::fs::read(linked.join(".codex/config.toml")).unwrap(),
            ignored
        );
        std::fs::remove_file(config).unwrap();
        crate::state::hook_consent::set_consent(&main, false);
        assert!(!app.settle_status_hooks(now + std::time::Duration::from_secs(31)));
        assert!(
            !main.join(".codex/config.toml").exists(),
            "revoked root consent stays revoked"
        );
    });
}

#[test]
fn codex_linked_hooks_refuse_a_tracked_root_checkout_config() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let (main, linked) = linked_hook_repo(tmp.path());
        std::fs::create_dir(main.join(".codex")).unwrap();
        let config = main.join(".codex/config.toml");
        let original = "model = 'user-choice'\n";
        std::fs::write(&config, original).unwrap();
        crate::git::test_support::run_git(&main, &["add", ".codex/config.toml"]);
        let mut app = agent_app(&linked, "codex");
        app.install_status_hooks(&linked, crate::state::sessions::AgentKind::Codex);
        assert_eq!(std::fs::read_to_string(config).unwrap(), original);
        assert!(!linked.join(".codex/config.toml").exists());
        assert!(dump(&mut app).contains("MISSING (`:hooks on`)"));
    });
}

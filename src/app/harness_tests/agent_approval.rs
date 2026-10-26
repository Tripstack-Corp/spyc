use super::agent_readiness::{agent_app, dump};
use super::*;
use crate::mcp_cmd::{McpCommand, McpResponse};
use crate::pane::AgentActivity;
use crate::state::sessions::AgentKind;
use std::time::{Duration, Instant};

fn report(app: &mut App, status: &str, event: Option<serde_json::Value>) {
    let result = app.execute_mcp_command(McpCommand::ReportStatus {
        pane_id: None,
        pane: None,
        status: status.into(),
        ttl_ms: Some(120_000),
        note: None,
        session_id: Some("session-1".into()),
        hook_event: event
            .as_ref()
            .and_then(crate::agent::status_hook::StatusHookEvent::from_value),
    });
    assert!(matches!(result, McpResponse::Ok { .. }), "{result:?}");
}

fn permission() -> serde_json::Value {
    serde_json::json!({"hook_event_name":"PermissionRequest", "tool_name":"Bash", "turn_id":"turn-1"})
}

#[test]
fn automatic_codex_permission_review_is_observed_without_latching_blocked() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        report(&mut app, "working", None);
        let original = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info()
            .reported
            .clone()
            .unwrap();
        report(&mut app, "blocked", Some(permission()));
        let now = original.at + Duration::from_secs(30);
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .last_output_at = Some(original.at + Duration::from_secs(1));
        app.settle_agent_activity(now, &mut RunCtx::for_test());
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        assert_eq!(info.activity, AgentActivity::Working);
        assert_eq!(info.reported.clone().unwrap().at, original.at);
        let explanation = dump(&mut app);
        assert!(
            explanation.contains("source: SELF-REPORT status=working"),
            "{explanation}"
        );
        assert!(
            explanation.contains("not applied: PermissionRequest precedes review"),
            "{explanation}"
        );
        assert!(explanation.contains("event=PermissionRequest status=blocked tool=Bash"));
    });
}

#[test]
fn permission_observation_does_not_invent_working_or_change_claude_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (command, expected) in [
            ("codex", AgentActivity::Idle),
            ("claude", AgentActivity::Blocked),
        ] {
            let mut app = agent_app(tmp.path(), command);
            report(&mut app, "blocked", Some(permission()));
            app.settle_agent_activity(Instant::now(), &mut RunCtx::for_test());
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .activity,
                expected
            );
        }
    });
}

#[test]
fn codex_permission_observation_preserves_question_correlation() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = agent_app(tmp.path(), "codex");
        let event = |name, call| {
            Some(
                serde_json::json!({"hook_event_name":name, "tool_name":"request_user_input", "turn_id":"turn-1", "tool_use_id":call}),
            )
        };
        report(
            &mut app,
            "codex-question-start",
            event("PreToolUse", "call-1"),
        );
        report(&mut app, "blocked", Some(permission()));
        report(
            &mut app,
            "codex-question-end",
            event("PostToolUse", "other-call"),
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
        report(
            &mut app,
            "codex-question-end",
            event("PostToolUse", "call-1"),
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
        // An agent block during the question still needs its own answer or newer report.
        report(
            &mut app,
            "codex-question-start",
            event("PreToolUse", "call-2"),
        );
        report(&mut app, "blocked", None);
        report(
            &mut app,
            "codex-question-end",
            event("PostToolUse", "call-2"),
        );
        assert!(dump(&mut app).contains("source: SELF-REPORT status=blocked"));
    });
}

#[test]
fn visible_codex_approval_temporarily_overrides_working_with_honest_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for (name, blocker) in [
            ("codex", "awaiting command approval"),
            ("agy", "awaiting tool-execution approval"),
        ] {
            let mut app = agent_app(tmp.path(), name);
            report(&mut app, "working", None);
            let at = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .reported
                .clone()
                .unwrap()
                .at;
            {
                let info = app
                    .runtime
                    .project_mut()
                    .pane_tabs
                    .as_mut()
                    .unwrap()
                    .active_info_mut();
                info.last_output_at = Some(at + Duration::from_secs(1));
                info.scrape_status = Some((AgentActivity::Blocked, Some(blocker)));
            }
            app.settle_agent_activity(at + Duration::from_secs(30), &mut RunCtx::for_test());
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .activity,
                AgentActivity::Blocked,
                "{name}: visible approval must override working"
            );
            assert!(dump(&mut app).contains(&format!(
                "source: SCRAPE-FALLBACK status=blocked ({blocker})"
            )));
            assert!(
                super::agent_readiness::why_status(&mut app).contains("blocked (scrape-fallback:")
            );
            let info = app
                .runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut();
            assert_eq!(
                info.reported.clone().unwrap().at,
                at,
                "{name}: dialogue must not discard the silent-work report"
            );
            info.scrape_status = None;
            app.settle_agent_activity(at + Duration::from_secs(60), &mut RunCtx::for_test());
            assert_eq!(
                app.runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .activity,
                AgentActivity::Working,
                "{name}: working must resume once approval clears"
            );
            assert!(dump(&mut app).contains("source: SELF-REPORT status=working"));
        }
    });
}

#[test]
fn codex_approval_detector_requires_a_complete_dialogue_at_the_viewport_bottom() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    let text = include_str!("../../../tests/fixtures/codex-approval-exec.txt");
    let lines = |s: &str| s.lines().map(String::from).collect::<Vec<_>>();
    let positive = crate::agent::detect_rules::scan(&lines(text), rules);
    assert_eq!(
        positive,
        Some((AgentActivity::Blocked, Some("awaiting command approval")))
    );
    let detail = (0..20)
        .map(|index| format!("  command detail line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let expanded = text.replace("  $ sleep 30", &detail);
    assert_eq!(
        crate::agent::detect_rules::scan(&lines(&expanded), rules),
        positive,
        "a complete visible dialogue with long command details still needs the user"
    );
    for rejected in [
        text.replace("Yes, proceed", ""),
        text.replace("Would you like to run the following command?", ""),
        text.replace("Press enter to confirm or esc to cancel", ""),
        format!("{text}\n› Ask Codex to do anything\nGPT | Vim: Insert"),
        "The approval says: Would you like to run the following command?".into(),
    ] {
        assert_eq!(
            crate::agent::detect_rules::scan(&lines(&rejected), rules),
            None,
            "{rejected}"
        );
    }
    assert_eq!(
        crate::agent::detect_rules::scan(
            &lines(&text.replace("› 1.", "  1.").replace("  3.", "› 3.")),
            rules
        ),
        positive
    );
}

#[test]
fn approval_scan_is_wired_to_the_current_viewport_and_loop() {
    let status = crate::guard_support::production_half(include_str!("../agent_status.rs"));
    let scan = status
        .split("pub(crate) fn settle_scrape_quiet(")
        .nth(1)
        .unwrap()
        .split("pub(crate) fn settle_agent_activity(")
        .next()
        .unwrap();
    assert!(scan.contains("let lines = entry.pane.visible_lines();"));
    assert!(scan.contains("crate::agent::detect_rules::scan(&lines, rules)"));
    assert!(
        !scan.contains("recent_lines("),
        "old approval history cannot establish a current wait"
    );
    let run = crate::guard_support::production_half(include_str!("../pre_recv.rs"));
    assert!(run.contains("self.settle_scrape_quiet(now_pre, ctx)"));
}

#[test]
fn codex_file_edit_approval_requires_a_complete_current_dialogue() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    let text = include_str!("../../../tests/fixtures/codex-approval-edit.txt");
    let scan = |text: &str| {
        crate::agent::detect_rules::scan(&text.lines().map(String::from).collect::<Vec<_>>(), rules)
    };
    let expected = Some((AgentActivity::Blocked, Some("awaiting file-edit approval")));
    assert_eq!(scan(text), expected);
    for phrase in [
        "Would you like to make the following edits?",
        "Yes, proceed",
        "Yes, and don't ask again for these files",
        "No, and tell Codex what to do differently",
        "Press enter to confirm or esc to cancel",
    ] {
        assert_eq!(scan(&text.replace(phrase, "")), None, "missing {phrase}");
    }
    assert_eq!(
        scan(&format!(
            "{text}\n› Ask Codex to do anything\nGPT | Vim: Insert"
        )),
        None,
        "an old dialogue above the composer is not a current wait"
    );
    assert_eq!(
        scan(&text.replace("› 1.", "  1.").replace("  3.", "› 3.")),
        expected,
        "choosing decline still needs the user until submitted"
    );
    let details = (0..20)
        .map(|index| format!("  edit detail line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        scan(&text.replace("  Description: Apply proposed file edits", &details)),
        expected,
        "a complete long dialogue still needs the user"
    );
}

#[test]
fn codex_mcp_and_network_waits_use_recorded_native_dialogues() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    for (text, reason, phrases, footer) in [
        (
            include_str!("../../../tests/fixtures/codex-approval-mcp.txt"),
            "awaiting MCP tool approval",
            &[
                "Field 1/1",
                "Run the tool and continue",
                "Cancel this tool call",
            ][..],
            "enter to submit | esc to cancel",
        ),
        (
            include_str!("../../../tests/fixtures/codex-approval-mcp-narrow.txt"),
            "awaiting MCP tool approval",
            &[
                "Field 1/1",
                "Run the tool and continue",
                "Cancel this tool call",
            ][..],
            "enter to submit | esc to cancel",
        ),
        (
            include_str!("../../../tests/fixtures/codex-approval-mcp-session.txt"),
            "awaiting MCP tool approval",
            &[
                "Field 1/1",
                "Run the tool and continue",
                "Cancel this tool call",
            ][..],
            "enter to submit | esc to cancel",
        ),
        (
            include_str!("../../../tests/fixtures/codex-approval-network.txt"),
            "awaiting network approval",
            &[
                "Do you want to approve network access to",
                "Yes, just this once",
                "No, and tell Codex what to do differently",
            ][..],
            "Press enter to confirm or esc to cancel",
        ),
    ] {
        let scan = |text: &str| {
            crate::agent::detect_rules::scan(
                &text.lines().map(String::from).collect::<Vec<_>>(),
                rules,
            )
        };
        let expected = Some((AgentActivity::Blocked, Some(reason)));
        assert_eq!(scan(text), expected, "{text}");
        for phrase in phrases.iter().copied().chain(std::iter::once(footer)) {
            assert_eq!(scan(&text.replace(phrase, "")), None, "missing {phrase}");
        }
        assert_eq!(
            scan(&format!(
                "{text}\n› Ask Codex to do anything\nGPT | Vim: Insert"
            )),
            None,
            "quoted modal above the composer"
        );
    }
}

#[test]
fn codex_approval_scan_has_a_deadline_despite_continuous_redraws() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        assert!(app.open_pane_tab_in("sh -c 'stty raw -echo; printf READY; exec cat'", tmp.path()));
        let wait_screen = |app: &mut App, expected: &str| {
            // Startup is not the approval deadline under test. Parallel PTY
            // launches can take seconds; keep the same budget as agent_argv.
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let pane = &mut app
                    .runtime
                    .project_mut()
                    .pane_tabs
                    .as_mut()
                    .unwrap()
                    .tabs_mut()[0]
                    .pane;
                pane.drain_output();
                let visible = pane.visible_lines().join("\n");
                if visible.contains(expected) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "screen did not contain {expected}: {visible}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        wait_screen(&mut app, "READY");
        let fixture = include_str!("../../../tests/fixtures/codex-approval-exec.txt");
        {
            let entry = &mut app
                .runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .tabs_mut()[0];
            entry.info.command = "codex".into();
            entry.pane.resize(30, 100).unwrap();
            entry
                .pane
                .send_bytes(format!("\x1b[2J\x1b[H{}", fixture.replace('\n', "\r\n")).as_bytes())
                .unwrap();
        }
        wait_screen(&mut app, "Press enter to confirm or esc to cancel");
        report(&mut app, "working", None);
        let base = Instant::now();
        let mut ctx = RunCtx::for_test();
        for elapsed in [0, 100, 200, 300] {
            let now = base + Duration::from_millis(elapsed);
            {
                let info = app
                    .runtime
                    .project_mut()
                    .pane_tabs
                    .as_mut()
                    .unwrap()
                    .active_info_mut();
                info.last_output_at = Some(now);
                info.scrape_dirty = true;
            }
            app.settle_scrape_quiet(now, &mut ctx);
            app.settle_agent_activity(now, &mut ctx);
        }
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        assert_eq!(
            info.activity,
            AgentActivity::Blocked,
            "a repaint cannot postpone every approval scan"
        );
        assert_eq!(
            info.reported.clone().unwrap().status,
            AgentActivity::Working,
            "retain working for answer recovery"
        );
        assert!(!info.scrape_dirty, "the deadline consumes the pending scan");
        assert!(
            dump(&mut app)
                .contains("source: SCRAPE-FALLBACK status=blocked (awaiting command approval)")
        );
    });
}

#[test]
fn codex_command_approval_handles_recorded_native_wrapping() {
    let rules = crate::agent::profile_for(AgentKind::Codex).detection_rules();
    let text = include_str!("../../../tests/fixtures/codex-approval-exec-narrow.txt");
    let scan = |text: &str| {
        crate::agent::detect_rules::scan(&text.lines().map(String::from).collect::<Vec<_>>(), rules)
    };
    let expected = Some((AgentActivity::Blocked, Some("awaiting command approval")));
    assert_eq!(scan(text), expected);
    for required in [
        "Would you like",
        "Yes, proceed",
        "No, and tell",
        "Press enter",
        "  cancel",
    ] {
        assert_eq!(
            scan(&text.replace(required, "")),
            None,
            "missing {required}"
        );
    }
    assert_eq!(scan(&format!("{text}\n› Ask Codex to do anything")), None);
    assert_eq!(
        scan(&text.replace("Press enter", "Quoted footer: Press enter")),
        None
    );
}

/// `str::replace` for a fixture: panics when `from` is absent, so a recaptured
/// fixture can't leave a case quietly testing the unedited screen.
trait Replaced {
    fn replaced(&self, from: &str, to: &str) -> String;
}

impl Replaced for str {
    fn replaced(&self, from: &str, to: &str) -> String {
        assert!(
            self.contains(from),
            "the fixture no longer contains {from:?}"
        );
        self.replace(from, to)
    }
}

#[test]
fn agy_approval_detector_recognizes_captured_live_approval_screen() {
    let rules = crate::agent::profile_for(AgentKind::Agy).detection_rules();
    let text = include_str!("../../../tests/fixtures/agy-output/approval-command.txt");
    let narrow = include_str!("../../../tests/fixtures/agy-output/approval-command-narrow.txt");
    let edit = include_str!("../../../tests/fixtures/agy-output/approval-edit.txt");
    let file = include_str!("../../../tests/fixtures/agy-output/approval-file.txt");
    let scan = |text: &str| {
        crate::agent::detect_rules::scan(&text.lines().map(String::from).collect::<Vec<_>>(), rules)
    };
    let expected = Some((
        AgentActivity::Blocked,
        Some("awaiting tool-execution approval"),
    ));
    for fixture in [text, narrow, edit, file] {
        assert_eq!(scan(fixture), expected);
    }
    assert_eq!(
        scan(
            &text
                .replaced("> 1. Yes, run command", "  1. Yes, run command")
                .replaced("  4. No, cancel", "> 4. No, cancel")
        ),
        expected,
        "moving the selection cursor to another option still needs the user"
    );
    for cancel_key in ["ctrl+c to cancel", "[esc] to cancel", "ctrl+] to cancel"] {
        assert_eq!(
            scan(&text.replaced("\nesc to cancel", &format!("\n{cancel_key}"))),
            expected,
            "a custom or bracketed cancel key ({cancel_key}) formatted from `%s to cancel` still matches"
        );
    }
    for auto_approve in [
        "alt+a to auto-approve file edits",
        "[shift+tab] to auto-approve file edits",
    ] {
        assert_eq!(
            scan(&edit.replaced("shift+tab to auto-approve file edits", auto_approve)),
            expected,
            "a custom or bracketed key token before `to auto-approve file edits` ({auto_approve}) still matches"
        );
    }
    assert_eq!(
        scan(
            &narrow
                .replaced(
                    "   uname -m",
                    "   git -C /Users/example/src/spyc\n  -C worktree status\n  > /tmp/out.txt"
                )
                .replaced(
                    "start with 'uname'",
                    "-C /Users/example/src/spyc\n> /tmp/out.txt'"
                )
        ),
        expected,
        "wrapped command and option continuation rows starting with `-` or `>` still match"
    );
    assert_eq!(
        scan(&narrow.replaced(
            "  ↑/↓ Navigate · tab Amend · ctrl+g edit/expand command",
            "  ↑/↓ Navigate · tab Amend · ctrl+g\nedit/expand command"
        )),
        expected,
        "a narrow pane that wraps the `↑/↓ Navigate` row across two lines still matches"
    );
    assert_eq!(
        scan(&text.replaced(
            "────────────────────────────────────────────────────────────────────────────────────────────────────\n",
            ""
        )),
        expected,
        "a dialog drawn without the rule under its header (older agy) still matches"
    );
    for required in [
        "Command\n",
        "Requesting permission for:",
        "Run this command?",
        "1. Yes, run command",
        "4. No, cancel",
        "↑/↓ Navigate",
        "esc to cancel",
    ] {
        assert_eq!(
            scan(&text.replaced(required, "")),
            None,
            "missing {required} must not match"
        );
    }
    assert_eq!(
        scan(&text.replaced("  2. Yes, and always allow", "  3. Yes, and always allow")),
        None,
        "out-of-order option numbering must not match"
    );
    assert_eq!(
        scan(&text.replaced("  4. No, cancel", "  4. Maybe later")),
        None,
        "options must end with a No choice"
    );
}

#[test]
fn agy_approval_detector_rejects_output_mentions_grep_diff_and_answered_prompts() {
    let rules = crate::agent::profile_for(AgentKind::Agy).detection_rules();
    let text = include_str!("../../../tests/fixtures/agy-output/approval-command.txt");
    let edit = include_str!("../../../tests/fixtures/agy-output/approval-edit.txt");
    let scan = |lines: &[&str]| {
        let owned = lines.iter().copied().map(String::from).collect::<Vec<_>>();
        crate::agent::detect_rules::scan(&owned, rules)
    };
    let scan_text = |s: &str| {
        crate::agent::detect_rules::scan(&s.lines().map(String::from).collect::<Vec<_>>(), rules)
    };

    // 1. Ordinary file-view output showing src/agent/mod.rs above the live working footer.
    assert_eq!(
        scan(&[
            "● Explored 1 file (src/agent/mod.rs) (ctrl+o to expand)",
            "731: static AGY_DETECTION_RULES: &[DetectionRule] = &[DetectionRule {",
            "732:     matcher: Matcher::All(&[\"Requesting permission for:\", \"Do you want to proceed?\"]),",
            "733:     state: AgentActivity::Blocked,",
            "734: }];",
            "  esc to cancel                                  accept-edits · Gemini 4 Argon · high",
            "  ~/src/spyc (main)                                                      sandbox: off",
        ]),
        None,
        "file view of src/agent/mod.rs above the working footer must not trigger Blocked"
    );

    // 2. A grep line containing both phrases above the live working footer.
    assert_eq!(
        scan(&[
            "src/agent/mod.rs:732:    matcher: Matcher::All(&[\"Requesting permission for:\", \"Do you want to proceed?\"]),",
            "  esc to cancel                                  accept-edits · Gemini 4 Argon · high",
            "  ~/src/spyc (main)                                                      sandbox: off",
        ]),
        None,
        "grep output above the working footer must not trigger Blocked"
    );

    // 3. A git diff showing the phrases above the live working footer.
    assert_eq!(
        scan(&[
            "diff --git a/src/agent/mod.rs b/src/agent/mod.rs",
            "@@ -730,5 +730,5 @@",
            "-    matcher: Matcher::All(&[\"Requesting permission for:\", \"Do you want to proceed?\"]),",
            "+    matcher: Matcher::AgyApproval,",
            "  esc to cancel                                  accept-edits · Gemini 4 Argon · high",
            "  ~/src/spyc (main)                                                      sandbox: off",
        ]),
        None,
        "git diff above the working footer must not trigger Blocked"
    );

    // 4. An answered prompt left on screen after the next turn starts.
    assert_eq!(
        scan(&[
            "Command",
            "────────────────────────────────────────────────────────────────────────────────",
            "Requesting permission for:",
            "  uname -a",
            "Do you want to proceed?",
            "> 1. Yes",
            "  2. No",
            "  ↑/↓ Navigate · tab Amend",
            "  esc to cancel                                  Gemini 4 Argon · high",
            "○ Thought for 2s (Running checks)",
            "● Ran (cargo test --lib)",
            "  esc to cancel                                  Gemini 4 Argon · high",
        ]),
        None,
        "answered prompt above a new turn must not stay Blocked"
    );

    // 5. A complete dialog indented two columns (as agy indents prose and tool output),
    // both above a spinner + working footer and directly above the working footer.
    let indented_dialog = [
        "● Explored 1 file (tests/fixtures/agy-output/approval-command.txt)",
        "  Command",
        "  ────────────────────────────────────────────────────────────────────────────────",
        "  Requesting permission for:",
        "    gh pr list --limit 10",
        "  Do you want to proceed?",
        "  > 1. Yes, run command",
        "    2. No, cancel",
        "  ↑/↓ Navigate · tab Amend · ctrl+g edit/expand command",
    ];
    let mut with_spinner = indented_dialog.to_vec();
    with_spinner.push("○ Thought for 2s (Inspecting fixture)");
    with_spinner.push(
        "  esc to cancel                                  accept-edits · Gemini 4 Argon · high",
    );
    assert_eq!(
        scan(&with_spinner),
        None,
        "complete dialog indented two columns above spinner and working footer must not match"
    );
    let mut directly_above_footer = indented_dialog.to_vec();
    directly_above_footer.push(
        "  esc to cancel                                  accept-edits · Gemini 4 Argon · high",
    );
    assert_eq!(
        scan(&directly_above_footer),
        None,
        "complete dialog indented two columns directly above the working footer must not match"
    );

    // 6. Each column-0 anchored row in a command or pending-edit dialog must fail when indented.
    for col0_row in ["Command", "Requesting permission for:", "Run this command?"] {
        assert_eq!(
            scan_text(&text.replaced(&format!("\n{col0_row}\n"), &format!("\n  {col0_row}\n"))),
            None,
            "indenting `{col0_row}` by two columns must reject the dialog"
        );
    }
    for col0_edit_row in ["Pending edit", "Accept this file edit?"] {
        assert_eq!(
            scan_text(&edit.replaced(
                &format!("\n{col0_edit_row}\n"),
                &format!("\n  {col0_edit_row}\n")
            )),
            None,
            "indenting `{col0_edit_row}` by two columns must reject the edit dialog"
        );
    }

    // 7. Turn output (`● `, `○ `) or a box rule inside `target` or edit `body` must be rejected.
    let rule80 = "─".repeat(80);
    for intruder in ["● Ran (git status)", "○ Thought for 2s", rule80.as_str()] {
        assert_eq!(
            scan_text(&text.replaced(
                "   uname -a && sleep 8",
                &format!("   uname -a && sleep 8\n{intruder}")
            )),
            None,
            "command target containing `{intruder}` must not match"
        );
        assert_eq!(
            scan_text(&edit.replaced("      1u8", &format!("      1u8\n{intruder}"))),
            None,
            "pending-edit body containing `{intruder}` must not match"
        );
    }

    // 8. Prose words before ` to cancel` or ` to auto-approve file edits` are not key tokens.
    for non_key in ["how to cancel", "click to cancel"] {
        assert_eq!(
            scan_text(&text.replaced("\nesc to cancel", &format!("\n{non_key}"))),
            None,
            "prose `{non_key}` must not match cancel footer"
        );
    }
    assert_eq!(
        scan_text(&edit.replaced(
            "shift+tab to auto-approve file edits",
            "how to auto-approve file edits"
        )),
        None,
        "prose before `to auto-approve file edits` must not match"
    );
}

/// A spinner below a dialog that was answered but is still drawn is the turn
/// running on, not the `↑/↓ Navigate` row's wrapped tail.
#[test]
fn agy_spinner_below_an_answered_dialog_is_not_a_live_approval() {
    let rules = crate::agent::profile_for(AgentKind::Agy).detection_rules();
    let narrow = include_str!("../../../tests/fixtures/agy-output/approval-command-narrow.txt");
    let answered = narrow.replaced(
        "  ↑/↓ Navigate · tab Amend · ctrl+g edit/expand command\n",
        "  ↑/↓ Navigate · tab Amend · ctrl+g edit/expand command\n⣟  Running command... (esc to cancel, 4s)\n",
    );
    assert_ne!(answered, narrow);
    assert_eq!(
        crate::agent::detect_rules::scan(
            &answered.lines().map(String::from).collect::<Vec<_>>(),
            rules
        ),
        None
    );
}

/// Pumps the first tab until its screen contains `expected`.
fn wait_screen(app: &mut App, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pane = &mut app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .tabs_mut()[0]
            .pane;
        pane.drain_output();
        let visible = pane.visible_lines().join("\n");
        if visible.contains(expected) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "screen did not contain {expected}: {visible}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens a tab whose command reads as agy and draws `fixture` on its 30 × 132
/// screen from the top-left, through a pty running `cat`.
fn draw_agy_fixture(app: &mut App, dir: &std::path::Path, fixture: &str) {
    assert!(app.open_pane_tab_in("sh -c 'stty raw -echo; printf READY; exec cat'", dir));
    wait_screen(app, "READY");
    {
        let entry = &mut app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .tabs_mut()[0];
        entry.info.command = "agy".into();
        entry.pane.resize(30, 132).unwrap();
        entry
            .pane
            .send_bytes(format!("\x1b[2J\x1b[H{}", fixture.replace('\n', "\r\n")).as_bytes())
            .unwrap();
    }
    wait_screen(app, "· high");
}

/// Reports `working`, then runs the quiet scrape over the screen.
fn settle_scrape_behind_working(app: &mut App) {
    report(app, "working", None);
    let base = Instant::now();
    let mut ctx = RunCtx::for_test();
    {
        let info = app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut();
        info.last_output_at = Some(base);
        info.scrape_dirty = true;
    }
    let scan_at = base + Duration::from_millis(300);
    app.settle_scrape_quiet(scan_at, &mut ctx);
    app.settle_agent_activity(scan_at, &mut ctx);
}

#[test]
fn agy_approval_scrape_runs_behind_a_live_working_report() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let fixture = include_str!("../../../tests/fixtures/agy-output/approval-command.txt");
        draw_agy_fixture(&mut app, tmp.path(), fixture);
        settle_scrape_behind_working(&mut app);
        let info = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active_info();
        assert_eq!(
            info.activity,
            AgentActivity::Blocked,
            "settle_scrape_quiet behind a live agy working report must surface Blocked"
        );
        assert_eq!(
            info.reported.clone().unwrap().status,
            AgentActivity::Working,
            "retain live working report behind scrape override"
        );
        assert!(!info.scrape_dirty, "quiet scan consumes scrape_dirty");
        assert!(
            dump(&mut app).contains(
                "source: SCRAPE-FALLBACK status=blocked (awaiting tool-execution approval)"
            )
        );
    });
}

const AGY_MCP: &str = include_str!("../../../tests/fixtures/agy-output/approval-mcp.txt");
const AGY_FILE_ACCESS: &str =
    include_str!("../../../tests/fixtures/agy-output/approval-file-access.txt");
const AGY_QUESTION_NARROW: &str =
    include_str!("../../../tests/fixtures/agy-output/approval-question-narrow.txt");

fn agy_scan(text: &str) -> Option<(AgentActivity, Option<&'static str>)> {
    let rules = crate::agent::profile_for(AgentKind::Agy).detection_rules();
    crate::agent::detect_rules::scan(&text.lines().map(String::from).collect::<Vec<_>>(), rules)
}

/// How many of `text`'s rows `gf`, `J` and `^a u` read as agy's output.
fn agy_output_rows(text: &str) -> usize {
    crate::agent::chrome::agy_output_len(&text.lines().map(String::from).collect::<Vec<_>>())
}

/// The index of `text`'s last row that is exactly `header`.
fn header_row(text: &str, header: &str) -> usize {
    text.lines()
        .collect::<Vec<_>>()
        .iter()
        .rposition(|&row| row == header)
        .unwrap_or_else(|| panic!("the fixture has no `{header}` row"))
}

/// agy 1.4.7 asks before an MCP tool call or a file read with a header, its
/// rule and the target, and no `Requesting permission for:` row.
#[test]
fn agy_mcp_and_file_access_approvals_scrape_as_blocked() {
    let expected = Some((
        AgentActivity::Blocked,
        Some("awaiting tool-execution approval"),
    ));
    assert_eq!(agy_scan(AGY_MCP), expected);
    assert_eq!(agy_scan(AGY_FILE_ACCESS), expected);
    assert_eq!(
        agy_scan(&AGY_FILE_ACCESS.replaced(
            "Read:\n/Users/example/src/spyc/src/printed.r\ns\n",
            "Read: /Users/example/src/spyc/src/printed.rs\n"
        )),
        expected,
        "a target on one row still matches"
    );
    assert_eq!(
        agy_scan(
            &AGY_MCP
                .replaced("> 1. Yes, allow tool call", "  1. Yes, allow tool call")
                .replaced("  4. No, deny tool call", "> 4. No, deny tool call")
        ),
        expected,
        "moving the cursor to another option still needs the user"
    );
}

/// Each anchor of the header, rule and target dialog is required.
#[test]
fn agy_mcp_and_file_access_lookalikes_are_not_approvals() {
    let rule = "─".repeat(100);
    for (what, text) in [
        (
            "an indented `MCP` header",
            AGY_MCP.replaced("\nMCP\n", "\n  MCP\n"),
        ),
        (
            "an indented `File access` header",
            AGY_FILE_ACCESS.replaced("\nFile access\n", "\n  File access\n"),
        ),
        (
            "an unknown header",
            AGY_MCP.replaced("\nMCP\n", "\nNotes\n"),
        ),
        (
            "no rule under the header",
            AGY_MCP.replaced(&format!("\nMCP\n{rule}\n"), "\nMCP\n"),
        ),
        ("no target", AGY_MCP.replaced("\nspyc/list_scopes\n", "\n")),
        (
            "a turn row in the target",
            AGY_MCP.replaced(
                "\nspyc/list_scopes\n",
                "\nspyc/list_scopes\n● Ran (git status)\n",
            ),
        ),
        (
            "an indented question",
            AGY_FILE_ACCESS.replaced(
                "\nAllow access to this file?\n",
                "\n  Allow access to this file?\n",
            ),
        ),
    ] {
        assert_eq!(agy_scan(&text), None, "{what} must not match");
        assert_eq!(
            agy_output_rows(&text),
            text.lines().count(),
            "{what} must stay output"
        );
    }
}

/// A `Question N/M:` prompt too long for the pane wraps onto a second row at
/// column 0, above the options.
#[test]
fn agy_wrapped_question_is_not_output() {
    let header = header_row(AGY_QUESTION_NARROW, "Question");
    assert_eq!(agy_output_rows(AGY_QUESTION_NARROW), header);
    assert_eq!(
        agy_output_rows(&AGY_QUESTION_NARROW.replaced(
            "\nits accent colour?\n",
            "\nits accent colour, for the\nheading text?\n"
        )),
        header,
        "a prompt wrapped onto three rows is a question too"
    );
}

/// The wrapped prompt's rows sit at column 0 and hold no turn row.
#[test]
fn agy_wrapped_question_lookalikes_stay_output() {
    let rule = "─".repeat(70);
    for (what, text) in [
        (
            "an indented continuation row",
            AGY_QUESTION_NARROW.replaced("\nits accent colour?\n", "\n  its accent colour?\n"),
        ),
        (
            "a turn row in the prompt",
            AGY_QUESTION_NARROW.replaced(
                "\nits accent colour?\n",
                "\nits accent colour?\n● Ran (git status)\n",
            ),
        ),
        (
            "an indented header",
            AGY_QUESTION_NARROW.replaced("\nQuestion\n", "\n  Question\n"),
        ),
        (
            "no rule under the header",
            AGY_QUESTION_NARROW.replaced(&format!("\nQuestion\n{rule}\n"), "\nQuestion\n"),
        ),
    ] {
        assert_eq!(
            agy_output_rows(&text),
            text.lines().count(),
            "{what} must stay output"
        );
    }
}

/// `^a u` labels what agy printed above each dialog and nothing in it.
#[test]
fn agy_quick_select_leaves_out_mcp_file_access_and_wrapped_question_dialogs() {
    for (fixture, header) in [
        (AGY_MCP.to_owned(), "MCP"),
        (AGY_FILE_ACCESS.to_owned(), "File access"),
        (
            AGY_QUESTION_NARROW.replaced("● Ran (uname -m)", "● Ran (cat src/printed.rs)"),
            "Question",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            draw_agy_fixture(&mut app, tmp.path(), &fixture);
            app.open_quick_select();
            let header = header_row(&fixture, header);
            let matches = &app
                .view
                .quick_select
                .as_ref()
                .expect("the output above the dialog holds a path")
                .matches;
            let in_dialog: Vec<_> = matches.iter().filter(|m| m.row >= header).collect();
            assert!(
                in_dialog.is_empty(),
                "`{}` dialog rows got labels: {in_dialog:?}",
                fixture.lines().nth(header).unwrap_or_default()
            );
        });
    }
}

/// The MCP and file access approvals turn agy's dot red behind its `working`
/// report.
#[test]
fn agy_mcp_and_file_access_approvals_turn_the_dot_blocked() {
    for fixture in [AGY_MCP, AGY_FILE_ACCESS] {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            draw_agy_fixture(&mut app, tmp.path(), fixture);
            settle_scrape_behind_working(&mut app);
            let activity = app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity;
            assert_eq!(activity, AgentActivity::Blocked);
            assert!(dump(&mut app).contains(
                "source: SCRAPE-FALLBACK status=blocked (awaiting tool-execution approval)"
            ));
        });
    }
}

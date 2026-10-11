use super::*;

fn render_records(records: &[serde_json::Value], show_tools: bool) -> String {
    let directory = tempfile::tempdir().expect("temporary transcript directory");
    let path = directory.path().join("rollout.jsonl");
    let text = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, text).expect("write transcript fixture");
    render_transcript(&path, &Theme::default(), None, show_tools)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn current_completed_messages_render_without_wire_instructions_or_reasoning() {
    let records = [
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"injected instructions"}]}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"user-1","content":[{"type":"text","text":"current user prompt"},{"type":"image","url":"not text"},{"type":"text","text":"second paragraph"}]}}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","id":"reason-1","raw_content":["private reasoning"]}}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"agent-1","content":[{"type":"Text","text":"current agent reply"}],"phase":"final_answer"}}}),
    ];
    let rendered = render_records(&records, false);
    assert!(rendered.contains("current user prompt"), "{rendered}");
    assert!(rendered.contains("second paragraph"), "{rendered}");
    assert!(rendered.contains("current agent reply"), "{rendered}");
    for ignored in ["injected instructions", "private reasoning", "not text"] {
        assert!(!rendered.contains(ignored), "{rendered}");
    }
}

#[test]
fn current_tools_and_custom_calls_render_and_can_be_hidden() {
    let records = [
        serde_json::json!({"type":"response_item","payload":{"type":"custom_tool_call","call_id":"patch-1","name":"apply_patch","input":"redacted patch input"}}),
        serde_json::json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"patch-1","output":[{"type":"text","text":"patch applied"}]}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"command-1","command":"printf redacted","status":"completed","aggregated_output":"command completed\nsecond line","exit_code":0}}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"McpToolCall","id":"mcp-1","server":"spyc","tool":"search_content","arguments":{"pattern":"needle"},"status":"completed","result":{"content":[{"type":"text","text":"search completed"}]}}}}),
    ];
    let rendered = render_records(&records, true);
    for expected in [
        "apply_patch(",
        "redacted patch input",
        "patch applied",
        "printf redacted",
        "command completed",
        "spyc.search_content(",
        "needle",
        "search completed",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
    assert!(!rendered.contains("second line"), "{rendered}");
    assert!(render_records(&records, false).is_empty());
}

#[test]
fn completed_records_and_alternate_tool_representations_render_once_by_identity() {
    let message = serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"agent-1","content":[{"type":"Text","text":"unique reply"}]}}});
    let native = serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"command-1","command":"unique command","aggregated_output":"unique output"}}});
    let records = [
        message.clone(),
        message,
        native.clone(),
        native,
        serde_json::json!({"type":"response_item","payload":{"type":"function_call","call_id":"command-1","name":"exec_command","arguments":"unique command"}}),
        serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"command-1","output":"unique output"}}),
    ];
    let rendered = render_records(&records, true);
    for expected in ["unique reply", "unique command", "unique output"] {
        assert_eq!(rendered.matches(expected).count(), 1, "{rendered}");
    }
}

#[test]
fn identical_text_with_distinct_or_missing_ids_is_not_deduplicated() {
    let records = [
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"user-1","content":[{"type":"text","text":"repeated prompt"}]}}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"user-2","content":[{"type":"text","text":"repeated prompt"}]}}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"legacy repeated"}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"legacy repeated"}}),
    ];
    let rendered = render_records(&records, false);
    assert_eq!(rendered.matches("repeated prompt").count(), 2, "{rendered}");
    assert_eq!(rendered.matches("legacy repeated").count(), 2, "{rendered}");
}

#[test]
fn record_ids_are_scoped_to_each_rollout_in_inherited_history() {
    let call = serde_json::json!({"type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"shell","arguments":"repeated command"}});
    let records = [
        serde_json::json!({"type":"session_meta","payload":{"id":"parent"}}),
        call.clone(),
        serde_json::json!({"type":"session_meta","payload":{"id":"child"}}),
        call,
    ];
    let rendered = render_records(&records, true);
    assert_eq!(
        rendered.matches("repeated command").count(),
        2,
        "{rendered}"
    );
}

fn recorded_shell_record() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/codex-shell-rollout.jsonl"
    ))
    .expect("recorded command fixture")
}

#[test]
fn recorded_argv_command_and_output_are_visible_in_the_public_transcript() {
    let rendered = render_records(&[recorded_shell_record()], true);
    assert!(rendered.contains("shell("), "{rendered}");
    assert!(rendered.contains("/bin/zsh -lc"), "{rendered}");
    assert!(rendered.contains("codex --version"), "{rendered}");
    assert!(rendered.contains("WARNING: proceeding"), "{rendered}");
}

#[test]
fn recorded_argv_commands_keep_identity_deduplication_and_tool_visibility() {
    let command = recorded_shell_record();
    let mut distinct = command.clone();
    distinct["payload"]["item"]["id"] = serde_json::json!("command-version-two");
    let prompt = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"recorded history prompt"}});
    let records = [prompt, command.clone(), command, distinct];
    let rendered = render_records(&records, true);
    assert_eq!(rendered.matches("codex --version").count(), 2, "{rendered}");
    assert_eq!(
        rendered.matches("WARNING: proceeding").count(),
        2,
        "{rendered}"
    );
    let hidden = render_records(&records, false);
    assert!(hidden.contains("recorded history prompt"), "{hidden}");
    assert!(!hidden.contains("codex --version"), "{hidden}");
    assert!(!hidden.contains("WARNING"), "{hidden}");
}

#[test]
fn argv_display_preserves_argument_boundaries_and_refuses_malformed_records() {
    let mut record = recorded_shell_record();
    record["payload"]["item"]["command"] =
        serde_json::json!(["printf", "%s", "a b", "", "$HOME;literal", "é"]);
    let decoded = crate::agent::codex_records::decode(&record);
    assert_eq!(
        decoded.len(),
        2,
        "valid argv must retain both command and output"
    );
    let RecordKind::ToolCall { arguments, .. } = &decoded[0].kind else {
        panic!("expected recorded command")
    };
    assert_eq!(
        shlex::split(arguments).unwrap(),
        ["printf", "%s", "a b", "", "$HOME;literal", "é"]
    );
    assert!(arguments.contains("'a b'"), "{arguments}");
    assert!(arguments.contains("''"), "{arguments}");
    assert!(arguments.contains("'$HOME;literal'"), "{arguments}");
    for malformed in [
        serde_json::json!([]),
        serde_json::json!(["echo", null]),
        serde_json::json!(["echo", 42]),
        serde_json::json!({"cmd":"echo"}),
        serde_json::json!(["echo", "\u{0}"]),
    ] {
        record["payload"]["item"]["command"] = malformed;
        assert!(
            crate::agent::codex_records::decode(&record).is_empty(),
            "{record}"
        );
    }
}

const HOME_SESSION: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";
const OTHER_SESSION: &str = "019e8b21-9e7c-7553-a118-d1cdada725fe";

fn home_rollout(home: &Path, uuid: &str) -> PathBuf {
    let day = home.join("sessions/2026/10/09");
    std::fs::create_dir_all(&day).unwrap();
    let path = day.join(format!("rollout-2026-10-09T12-00-00-{uuid}.jsonl"));
    std::fs::write(
        &path,
        serde_json::json!({
            "type":"session_meta", "payload": {
                "id": uuid, "cwd":"/repo", "timestamp":"2026-10-09T12:00:00Z"
            }
        })
        .to_string()
            + "\n",
    )
    .unwrap();
    path
}

fn home_query<'a>(
    home: Option<&'a Path>,
    command: &'a str,
    id: Option<&'a str>,
) -> crate::agent::TranscriptQuery<'a> {
    crate::agent::TranscriptQuery {
        cwd: Path::new("/repo"),
        spawn_epoch_secs: 0,
        command,
        session_id: id,
        codex_home: home,
    }
}

#[test]
fn codex_lookup_uses_the_panes_custom_home_for_pinned_and_resumed_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("first");
    let second = tmp.path().join("second");
    let expected = home_rollout(&first, HOME_SESSION);
    let other = home_rollout(&second, HOME_SESSION);
    let command = format!("codex resume {HOME_SESSION}");
    assert_eq!(
        resolve_active_rollout(home_query(Some(&first), "codex", Some(HOME_SESSION))),
        Some(expected.clone())
    );
    assert_eq!(
        resolve_active_rollout(home_query(Some(&first), &command, None)),
        Some(expected)
    );
    assert_eq!(
        resolve_active_rollout(home_query(Some(&second), "codex", Some(HOME_SESSION))),
        Some(other)
    );
    assert_eq!(
        scan_rollout_metas(&first)
            .iter()
            .map(|r| r.uuid.as_str())
            .collect::<Vec<_>>(),
        [HOME_SESSION]
    );
}

#[test]
fn codex_lookup_never_substitutes_another_rollout_when_identity_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    home_rollout(tmp.path(), OTHER_SESSION);
    let resume = format!("codex resume {HOME_SESSION}");
    for query in [
        home_query(Some(tmp.path()), "codex", Some(HOME_SESSION)),
        home_query(Some(tmp.path()), &resume, None),
        home_query(Some(tmp.path()), "codex", None),
        home_query(Some(tmp.path()), "codex resume --last", None),
        home_query(None, "codex", Some(OTHER_SESSION)),
    ] {
        assert_eq!(resolve_active_rollout(query), None);
    }
}

#[test]
fn codex_lookup_requires_the_exact_trailing_uuid() {
    let tmp = tempfile::tempdir().unwrap();
    let decoy = home_rollout(tmp.path(), OTHER_SESSION);
    std::fs::rename(
        &decoy,
        decoy.with_file_name(format!("rollout-{HOME_SESSION}-{OTHER_SESSION}.jsonl")),
    )
    .unwrap();
    assert_eq!(
        resolve_active_rollout(home_query(Some(tmp.path()), "codex", Some(HOME_SESSION))),
        None
    );
}

#[test]
fn codex_missing_or_unreadable_session_tree_stays_unbound() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");
    assert_eq!(
        resolve_active_rollout(home_query(Some(&missing), "codex", Some(HOME_SESSION))),
        None
    );
    std::fs::write(tmp.path().join("sessions"), "a file, not a session tree").unwrap();
    assert_eq!(
        resolve_active_rollout(home_query(Some(tmp.path()), "codex", Some(HOME_SESSION))),
        None
    );
    assert!(scan_rollout_metas(tmp.path()).is_empty());
}

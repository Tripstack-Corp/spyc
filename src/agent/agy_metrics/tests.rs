use std::path::PathBuf;

use serde_json::json;

use super::records::{Event, decode};
use super::stream::Reader;
use super::*;

const SESSION: &str = "test-session-123";

fn binding(home: PathBuf) -> Binding {
    Binding {
        pane_id: "pane-1".to_owned(),
        session_id: SESSION.to_owned(),
        agy_home: home,
        generation: 1,
    }
}

fn status_of(snap: &Snapshot, name: &str) -> ToolStatus {
    snap.tools
        .iter()
        .find(|t| t.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no {name} call"))
        .status
}

#[test]
fn planner_response_extracts_tokens_and_preserves_spyc_tools() {
    let raw = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "input_tokens": 100,
        "cache_read_tokens": 50,
        "output_tokens": 25,
        "tool_calls": [
            {
                "name": "run_command",
                "parameters": {"command": "echo secret"}
            },
            {
                "name": "mcp_spyc_get_context",
                "parameters": {}
            },
            {
                "name": "mcp_private_custom_tool",
                "parameters": {}
            }
        ]
    });

    let event = decode(&raw).expect("decode planner response");
    match event {
        Event::PlannerResponse {
            step_index,
            usage,
            calls,
            ..
        } => {
            assert_eq!(step_index, Some(1));
            let u = usage.expect("usage");
            assert_eq!(u.input, 100);
            assert_eq!(u.cache_read, 50);
            assert_eq!(u.output, 25);
            assert_eq!(u.context(), 150);

            assert_eq!(calls.len(), 3);
            assert_eq!(calls[0].name.as_deref(), Some("run_command"));
            // spyc's own MCP tool name is preserved:
            assert_eq!(calls[1].name.as_deref(), Some("mcp_spyc_get_context"));
            // other MCP tool names are redacted:
            assert_eq!(calls[2].name.as_deref(), Some("mcp"));
        }
        _ => panic!("expected PlannerResponse"),
    }
}

#[test]
fn tool_result_and_checkpoint_records_decode_cleanly() {
    let generic_done = json!({
        "type": "GENERIC",
        "step_index": 2,
        "created_at": "2026-10-14T08:00:01Z",
        "status": "DONE"
    });
    match decode(&generic_done).expect("decode generic") {
        Event::ToolResult { failed, .. } => assert!(!failed),
        _ => panic!("expected ToolResult"),
    }

    let generic_error = json!({
        "type": "GENERIC",
        "step_index": 3,
        "created_at": "2026-10-14T08:00:02Z",
        "status": "ERROR"
    });
    match decode(&generic_error).expect("decode generic error") {
        Event::ToolResult { failed, .. } => assert!(failed),
        _ => panic!("expected ToolResult"),
    }

    let checkpoint = json!({
        "type": "CHECKPOINT",
        "created_at": "2026-10-14T08:00:03Z"
    });
    match decode(&checkpoint).expect("decode checkpoint") {
        Event::Checkpoint { .. } => {}
        _ => panic!("expected Checkpoint"),
    }
}

#[test]
fn user_settings_change_extracts_model() {
    let settings = json!({
        "type": "USER_INPUT",
        "content": "<USER_SETTINGS_CHANGE>\nThe user changed setting `Model Selection` from gemini-2.5-flash to gemini-2.5-pro. No need to respond to this message, just acknowledge it and continue with the previous task.\n</USER_SETTINGS_CHANGE>"
    });
    match decode(&settings).expect("decode model") {
        Event::Model { model, .. } => assert_eq!(model, "gemini-2.5-pro"),
        _ => panic!("expected Model"),
    }
}

#[test]
fn reducer_accumulates_usage_and_deduplicates_repeated_planner_steps() {
    let mut reducer = Reducer::default();

    let record1 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "input_tokens": 100,
        "cache_read_tokens": 50,
        "output_tokens": 20
    });
    reducer.fold(decode(&record1).unwrap());

    let snap = reducer.snapshot();
    assert_eq!(snap.messages, 1);
    assert_eq!(snap.totals.input, 100);
    assert_eq!(snap.totals.cache_read, 50);
    assert_eq!(snap.totals.output, 20);
    assert_eq!(snap.context_tokens.as_ref().unwrap().value, 150);
    assert_eq!(snap.request.as_ref().unwrap().value.context(), 150);

    // Replay with identical step_index (e.g. streaming update or re-read)
    let record1_update = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:01Z",
        "input_tokens": 100,
        "cache_read_tokens": 50,
        "output_tokens": 25
    });
    reducer.fold(decode(&record1_update).unwrap());

    let snap = reducer.snapshot();
    // Messages count does not increment on duplicate step
    assert_eq!(snap.messages, 1);
    assert_eq!(snap.totals.output, 25);

    // Next step
    let record2 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 2,
        "created_at": "2026-10-14T08:00:05Z",
        "input_tokens": 200,
        "cache_read_tokens": 100,
        "output_tokens": 30
    });
    reducer.fold(decode(&record2).unwrap());

    let snap = reducer.snapshot();
    assert_eq!(snap.messages, 2);
    assert_eq!(snap.totals.input, 300);
    assert_eq!(snap.totals.cache_read, 150);
    assert_eq!(snap.totals.output, 55);
    assert_eq!(snap.context_tokens.as_ref().unwrap().value, 300);
}

#[test]
fn tools_track_lifecycle_failures_and_subagent_calls() {
    let mut reducer = Reducer::default();

    let step = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "input_tokens": 10,
        "cache_read_tokens": 5,
        "output_tokens": 2,
        "tool_calls": [
            {"name": "run_command"},
            {"name": "invoke_subagent"}
        ]
    });
    reducer.fold(decode(&step).unwrap());

    let snap = reducer.snapshot();
    assert_eq!(snap.subagent_calls, 1);
    assert_eq!(snap.tools.len(), 2);
    assert_eq!(snap.tools[0].status, ToolStatus::Started);
    assert_eq!(snap.tools[1].status, ToolStatus::Started);

    // First tool succeeds
    let generic1 = json!({
        "type": "GENERIC",
        "step_index": 2,
        "created_at": "2026-10-14T08:00:01Z",
        "status": "DONE"
    });
    reducer.fold(decode(&generic1).unwrap());

    // Second tool fails
    let generic2 = json!({
        "type": "GENERIC",
        "step_index": 3,
        "created_at": "2026-10-14T08:00:02Z",
        "status": "ERROR"
    });
    reducer.fold(decode(&generic2).unwrap());

    let snap = reducer.snapshot();
    assert_eq!(snap.tools[0].status, ToolStatus::Completed);
    assert_eq!(snap.tools[1].status, ToolStatus::Failed);

    let failed_count = snap
        .tool_counts
        .iter()
        .find(|c| c.name == "invoke_subagent")
        .expect("invoke_subagent count");
    assert_eq!(failed_count.calls, 1);
    assert_eq!(failed_count.failed, 1);

    let ok_count = snap
        .tool_counts
        .iter()
        .find(|c| c.name == "run_command")
        .expect("run_command count");
    assert_eq!(ok_count.calls, 1);
    assert_eq!(ok_count.failed, 0);
}

#[test]
fn compaction_clears_context_until_the_next_turn() {
    let mut reducer = Reducer::default();

    let step = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "input_tokens": 100,
        "cache_read_tokens": 50,
        "output_tokens": 20
    });
    reducer.fold(decode(&step).unwrap());
    assert!(reducer.snapshot().context_tokens.is_some());

    let checkpoint = json!({
        "type": "CHECKPOINT",
        "created_at": "2026-10-14T08:00:10Z"
    });
    reducer.fold(decode(&checkpoint).unwrap());

    let snap = reducer.snapshot();
    assert_eq!(snap.compactions, 1);
    assert!(snap.context_tokens.is_none());
    assert!(snap.compaction.is_some());

    // Next step restores context_tokens
    let step2 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 2,
        "created_at": "2026-10-14T08:00:15Z",
        "input_tokens": 40,
        "cache_read_tokens": 10,
        "output_tokens": 5
    });
    reducer.fold(decode(&step2).unwrap());

    let snap = reducer.snapshot();
    assert_eq!(snap.context_tokens.as_ref().unwrap().value, 50);
}

#[test]
fn stream_reader_tails_appended_transcript_lines() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().to_path_buf();
    let transcript = crate::state::sessions::agy_transcript_path(&home, SESSION);
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();

    let line1 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "input_tokens": 10,
        "cache_read_tokens": 2,
        "output_tokens": 3
    })
    .to_string()
        + "\n";

    std::fs::write(&transcript, line1).unwrap();

    let b = binding(home);
    let mut reader = Reader::new(b, transcript.clone());
    let update = reader.refresh();
    assert_eq!(update.availability, Availability::Available);
    assert_eq!(update.snapshot.messages, 1);
    assert_eq!(update.snapshot.totals.input, 10);

    // Append a second line
    let line2 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 2,
        "created_at": "2026-10-14T08:00:05Z",
        "input_tokens": 20,
        "cache_read_tokens": 5,
        "output_tokens": 7
    })
    .to_string()
        + "\n";

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    file.write_all(line2.as_bytes()).unwrap();
    drop(file);

    let update2 = reader.refresh();
    assert_eq!(update2.availability, Availability::Available);
    assert_eq!(update2.snapshot.messages, 2);
    assert_eq!(update2.snapshot.totals.input, 30);
}

#[test]
fn update_matches_only_its_own_binding() {
    let b1 = binding(PathBuf::from("/test/home"));
    let b2 = binding(PathBuf::from("/other/home"));
    let update = Update {
        binding: b1.clone(),
        availability: Availability::Available,
        tail_only: false,
        snapshot: Snapshot::default(),
    };
    assert!(update.for_binding(Some(&b1)).is_some());
    assert!(update.for_binding(Some(&b2)).is_none());
    assert!(update.for_binding(None).is_none());
}

#[test]
fn results_pair_by_step_index_so_later_turns_do_not_steal_background_results() {
    let mut reducer = Reducer::default();

    // Turn 1: planner issues run_command (background)
    let p1 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 31,
        "created_at": "2026-10-14T08:00:00Z",
        "tool_calls": [{"name": "run_command"}]
    });
    reducer.fold(decode(&p1).unwrap());

    // Step 32: RUN_COMMAND is RUNNING
    let r1 = json!({
        "type": "RUN_COMMAND",
        "step_index": 32,
        "status": "RUNNING",
        "created_at": "2026-10-14T08:00:01Z"
    });
    if let Some(ev) = decode(&r1) {
        reducer.fold(ev);
    }

    // Turn 2: planner issues view_file
    let p2 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 34,
        "created_at": "2026-10-14T08:00:05Z",
        "tool_calls": [{"name": "view_file"}]
    });
    reducer.fold(decode(&p2).unwrap());

    // Step 35: view_file completes with GENERIC DONE
    let r2 = json!({
        "type": "GENERIC",
        "step_index": 35,
        "status": "DONE",
        "created_at": "2026-10-14T08:00:06Z"
    });
    reducer.fold(decode(&r2).unwrap());

    let snap = reducer.snapshot();
    // Step 35 must complete view_file (from step 34), NOT run_command (from step 31)!
    let rc = snap
        .tools
        .iter()
        .find(|t| t.name.as_deref() == Some("run_command"))
        .unwrap();
    let vf = snap
        .tools
        .iter()
        .find(|t| t.name.as_deref() == Some("view_file"))
        .unwrap();
    assert_eq!(
        rc.status,
        ToolStatus::Completed,
        "run_command must be Completed on RUNNING"
    );
    assert_eq!(
        vf.status,
        ToolStatus::Completed,
        "view_file must be Completed"
    );
}

#[test]
fn replayed_planner_response_does_not_duplicate_tools_or_counts() {
    let mut reducer = Reducer::default();

    let p = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "tool_calls": [{"name": "run_command"}]
    });
    reducer.fold(decode(&p).unwrap());

    // Replay identical planner response
    reducer.fold(decode(&p).unwrap());

    let snap = reducer.snapshot();
    let count = snap
        .tool_counts
        .iter()
        .find(|c| c.name == "run_command")
        .unwrap();
    assert_eq!(
        count.calls, 1,
        "replayed planner response must not double count calls"
    );
    assert_eq!(
        snap.tools.len(),
        1,
        "replayed planner response must not duplicate tools"
    );
}

#[test]
fn call_mcp_tool_unwraps_spyc_server_calls_and_redacts_others() {
    let p = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "tool_calls": [
            {
                "name": "call_mcp_tool",
                "args": {
                    "ServerName": "\"spyc\"",
                    "ToolName": "\"get_spyc_context\""
                }
            },
            {
                "name": "call_mcp_tool",
                "args": {
                    "ServerName": "\"spyc\"",
                    "ToolName": "\"navigate_to\""
                }
            },
            {
                "name": "call_mcp_tool",
                "args": {
                    "ServerName": "\"other_mcp_server\"",
                    "ToolName": "\"do_something_private\""
                }
            }
        ]
    });
    let event = decode(&p).expect("decode");
    match event {
        Event::PlannerResponse { calls, .. } => {
            assert_eq!(calls.len(), 3);
            assert_eq!(calls[0].name.as_deref(), Some("mcp_spyc_get_spyc_context"));
            assert_eq!(calls[1].name.as_deref(), Some("mcp_spyc_navigate_to"));
            assert_eq!(calls[2].name.as_deref(), Some("mcp"));
        }
        _ => panic!("expected PlannerResponse"),
    }
}

#[test]
fn initial_checkpoint_0_is_ignored_and_same_second_response_restores_context() {
    let mut reducer = Reducer::default();

    let p1 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 1,
        "created_at": "2026-10-14T08:00:00Z",
        "input_tokens": 100,
        "cache_read_tokens": 50,
        "output_tokens": 20
    });
    reducer.fold(decode(&p1).unwrap());
    assert_eq!(
        reducer.snapshot().context_tokens.as_ref().unwrap().value,
        150
    );

    // Initial CHECKPOINT 0 is not a compaction
    let chk0 = json!({
        "type": "CHECKPOINT",
        "step_index": 2,
        "created_at": "2026-10-14T08:00:01Z",
        "content": "{{ CHECKPOINT 0 }}\n **The earlier parts of this conversation..."
    });
    assert!(decode(&chk0).is_none(), "CHECKPOINT 0 must be ignored");

    let snap = reducer.snapshot();
    assert_eq!(snap.compactions, 0);
    assert_eq!(snap.context_tokens.as_ref().unwrap().value, 150);

    // Real compaction
    let chk1 = json!({
        "type": "CHECKPOINT",
        "step_index": 10,
        "created_at": "2026-10-14T08:00:10Z",
        "content": "# Resuming from a compaction\n\nProgress summary..."
    });
    reducer.fold(decode(&chk1).expect("decode real compaction"));

    let snap = reducer.snapshot();
    assert_eq!(snap.compactions, 1);
    assert!(snap.context_tokens.is_none());

    // Next planner response in the SAME SECOND
    let p2 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 11,
        "created_at": "2026-10-14T08:00:10Z",
        "input_tokens": 40,
        "cache_read_tokens": 10,
        "output_tokens": 5
    });
    reducer.fold(decode(&p2).unwrap());

    let snap = reducer.snapshot();
    assert_eq!(
        snap.context_tokens.as_ref().unwrap().value,
        50,
        "same-second response restores context"
    );
}

#[test]
fn user_settings_model_records_timestamp() {
    let mut reducer = Reducer::default();

    let settings = json!({
        "type": "USER_INPUT",
        "created_at": "2026-10-14T08:05:00Z",
        "content": "<USER_SETTINGS_CHANGE>\nThe user changed setting `Model Selection` from gemini-2.5-flash to gemini-2.5-pro. No need to respond to this message, just acknowledge it and continue with the previous task.\n</USER_SETTINGS_CHANGE>"
    });
    reducer.fold(decode(&settings).expect("decode model"));

    let snap = reducer.snapshot();
    let mode = snap.mode.expect("mode present");
    assert_eq!(mode.value.model.as_deref(), Some("gemini-2.5-pro"));
    assert_eq!(mode.at.to_string(), "2026-10-14T08:05:00Z");
}

#[test]
fn an_early_result_before_its_call_is_held_and_matched_when_the_call_arrives() {
    let mut reducer = Reducer::default();

    // Out-of-order result before call:
    // ERROR_MESSAGE step 8 appears in transcript before PLANNER_RESPONSE step 7.
    let r_err8 = json!({
        "type": "ERROR_MESSAGE",
        "step_index": 8,
        "status": "ERROR",
        "created_at": "2026-10-14T10:00:05Z"
    });
    reducer.fold(decode(&r_err8).expect("decode error_message"));

    let r_plan7 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 7,
        "created_at": "2026-10-14T10:00:06Z",
        "input_tokens": 100,
        "cache_read_tokens": 20,
        "output_tokens": 30,
        "tool_calls": [
            { "name": "manage_task" }
        ]
    });
    reducer.fold(decode(&r_plan7).expect("decode plan7"));

    let snap = reducer.snapshot();
    assert_eq!(snap.tools.len(), 1);
    assert_eq!(snap.tools[0].name.as_deref(), Some("manage_task"));
    assert_eq!(snap.tools[0].status, ToolStatus::Failed);
    assert_eq!(snap.tool_counts.len(), 1);
    assert_eq!(snap.tool_counts[0].calls, 1);
    assert_eq!(snap.tool_counts[0].failed, 1);
}

#[test]
fn unanswered_call_is_settled_when_the_next_turn_arrives() {
    let mut reducer = Reducer::default();

    let r_plan30 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 30,
        "created_at": "2026-10-14T10:00:30Z",
        "input_tokens": 250,
        "cache_read_tokens": 50,
        "output_tokens": 60,
        "tool_calls": [
            { "name": "view_file" },
            { "name": "run_command" }
        ]
    });
    reducer.fold(decode(&r_plan30).expect("decode plan30"));

    // Only view_file gets a result; run_command never gets a result record
    let r_vf31 = json!({
        "type": "VIEW_FILE",
        "step_index": 31,
        "status": "DONE",
        "created_at": "2026-10-14T10:00:31Z"
    });
    reducer.fold(decode(&r_vf31).expect("decode vf31"));

    let snap_after_31 = reducer.snapshot();
    let rc30 = snap_after_31
        .tools
        .iter()
        .find(|t| t.name.as_deref() == Some("run_command"))
        .expect("run_command step 30");
    assert_eq!(
        rc30.status,
        ToolStatus::Started,
        "must be Started before next turn"
    );

    // Subsequent planner response closes older unreturned call from step 30
    let r_plan40 = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 40,
        "created_at": "2026-10-14T10:00:40Z",
        "input_tokens": 300,
        "cache_read_tokens": 60,
        "output_tokens": 70,
        "tool_calls": [{ "name": "grep_search" }]
    });
    reducer.fold(decode(&r_plan40).expect("decode plan40"));

    let snap_after_40 = reducer.snapshot();
    assert_eq!(
        status_of(&snap_after_40, "run_command"),
        ToolStatus::Completed,
        "unmatched tool closed by next turn"
    );
    assert_eq!(
        status_of(&snap_after_40, "grep_search"),
        ToolStatus::Started,
        "the new turn's own call is still running"
    );
}

#[test]
fn a_running_record_means_the_call_returned() {
    let mut reducer = Reducer::default();
    let plan = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 60,
        "created_at": "2026-10-14T10:01:00Z",
        "tool_calls": [{ "name": "run_command" }, { "name": "view_file" }]
    });
    reducer.fold(decode(&plan).expect("decode plan60"));
    assert_eq!(
        status_of(&reducer.snapshot(), "run_command"),
        ToolStatus::Started
    );

    let running = json!({
        "type": "RUN_COMMAND",
        "step_index": 61,
        "status": "RUNNING",
        "created_at": "2026-10-14T10:01:02Z"
    });
    reducer.fold(decode(&running).expect("decode rc61"));
    let snap = reducer.snapshot();
    assert_eq!(
        status_of(&snap, "run_command"),
        ToolStatus::Completed,
        "RUNNING status must count as tool completed without waiting for subsequent records"
    );
    assert_eq!(
        status_of(&snap, "view_file"),
        ToolStatus::Started,
        "only its own call settles"
    );
    let count = snap
        .tool_counts
        .iter()
        .find(|c| c.name == "run_command")
        .expect("run_command counted");
    assert_eq!((count.calls, count.failed), (1, 0));
}

#[test]
fn typed_results_match_by_kind_and_untyped_results_by_step() {
    let mut reducer = Reducer::default();
    let plan = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 20,
        "created_at": "2026-10-14T10:00:20Z",
        "tool_calls": [
            { "name": "grep_search" },
            { "name": "custom_tool" },
            { "name": "view_file" },
            { "name": "replace_file_content" },
            { "name": "list_dir" }
        ]
    });
    reducer.fold(decode(&plan).expect("decode plan20"));

    let result = |typ: &str, step: u64, status: &str| {
        json!({
            "type": typ,
            "step_index": step,
            "status": status,
            "created_at": format!("2026-10-14T10:00:{step}Z")
        })
    };
    reducer.fold(decode(&result("VIEW_FILE", 21, "DONE")).expect("decode vf21"));
    let snap = reducer.snapshot();
    assert_eq!(status_of(&snap, "view_file"), ToolStatus::Completed);
    assert_eq!(
        status_of(&snap, "grep_search"),
        ToolStatus::Started,
        "a typed result answers its own kind, not the first call"
    );

    for (typ, step, status) in [
        ("CODE_ACTION", 22, "DONE"),
        ("LIST_DIRECTORY", 23, "ERROR"),
        ("ERROR_MESSAGE", 24, "DONE"),
        ("GENERIC", 25, "DONE"),
    ] {
        reducer.fold(decode(&result(typ, step, status)).expect("decode result"));
    }
    let snap = reducer.snapshot();
    assert_eq!(
        status_of(&snap, "replace_file_content"),
        ToolStatus::Completed
    );
    assert_eq!(status_of(&snap, "list_dir"), ToolStatus::Failed);
    assert_eq!(
        status_of(&snap, "grep_search"),
        ToolStatus::Failed,
        "an ERROR_MESSAGE fails the first call still waiting"
    );
    assert_eq!(status_of(&snap, "custom_tool"), ToolStatus::Completed);
    let failed = |name: &str| {
        snap.tool_counts
            .iter()
            .find(|c| c.name == name)
            .map_or(0, |c| c.failed)
    };
    assert_eq!((failed("list_dir"), failed("grep_search")), (1, 1));
}

#[test]
fn only_a_leading_checkpoint_0_is_not_a_compaction() {
    let mut reducer = Reducer::default();
    for content in [
        "{{ CHECKPOINT 0 }}",
        "{{ CHECKPOINT 0 }}\nSummary of early conversation",
        "{{  CHECKPOINT 0  }}\nExtra whitespace",
        "{{CHECKPOINT 0}}",
    ] {
        let initial = json!({
            "type": "CHECKPOINT",
            "step_index": 2,
            "created_at": "2026-10-14T10:00:01Z",
            "content": content
        });
        assert!(
            decode(&initial).is_none(),
            "prefix matching must drop initial checkpoint: {content}"
        );
    }

    let plan = |step: u64, at: &str, input: u64, cache_read: u64| {
        json!({
            "type": "PLANNER_RESPONSE",
            "step_index": step,
            "created_at": at,
            "input_tokens": input,
            "cache_read_tokens": cache_read,
            "output_tokens": 30,
            "tool_calls": []
        })
    };
    reducer.fold(decode(&plan(10, "2026-10-14T10:00:10Z", 100, 20)).expect("decode plan10"));
    assert_eq!(
        reducer.snapshot().context_tokens.map(|s| s.value),
        Some(120)
    );

    // Newer agy writes no header, and this summary mentions the marker.
    let summary = json!({
        "type": "CHECKPOINT",
        "step_index": 45,
        "created_at": "2026-10-14T10:00:45Z",
        "content": "Compacted conversation context. Prior CHECKPOINT 0 reached earlier."
    });
    reducer.fold(decode(&summary).expect("decode cp45"));
    assert_eq!(
        reducer.snapshot().compactions,
        1,
        "newer-format compaction must be counted"
    );
    assert!(
        reducer.snapshot().context_tokens.is_none(),
        "context tokens cleared after compaction"
    );

    let numbered = json!({
        "type": "CHECKPOINT",
        "step_index": 47,
        "created_at": "2026-10-14T10:00:47Z",
        "content": "{{ CHECKPOINT 1 }}\nSecond compaction performed."
    });
    reducer.fold(decode(&numbered).expect("must decode CHECKPOINT 1"));
    assert_eq!(reducer.snapshot().compactions, 2);

    reducer.fold(decode(&plan(50, "2026-10-14T10:00:50Z", 120, 10)).expect("decode plan50"));
    assert_eq!(
        reducer.snapshot().context_tokens.map(|s| s.value),
        Some(130)
    );
}

#[test]
fn current_agy_tool_names_match_typed_results_and_parameters_mcp_calls() {
    let mut reducer = Reducer::default();
    let plan = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": 10,
        "created_at": "2026-10-14T10:00:10Z",
        "tool_calls": [
            { "name": "grep_search" },
            { "name": "read_file" },
            { "name": "edit_file" },
            { "name": "write_file" },
            { "name": "manage_subagents" },
            {
                "name": "call_mcp_tool",
                "parameters": {
                    "ServerName": "\"spyc\"",
                    "ToolName": "\"get_spyc_context\""
                }
            }
        ]
    });
    reducer.fold(decode(&plan).expect("decode plan10"));

    let result = |typ: &str, step: u64, status: &str| {
        json!({
            "type": typ,
            "step_index": step,
            "status": status,
            "created_at": format!("2026-10-14T10:00:{step}Z")
        })
    };
    reducer.fold(decode(&result("VIEW_FILE", 11, "DONE")).expect("decode vf11"));
    reducer.fold(decode(&result("CODE_ACTION", 12, "DONE")).expect("decode ca12"));
    reducer.fold(decode(&result("CODE_ACTION", 13, "ERROR")).expect("decode ca13"));
    reducer.fold(decode(&result("INVOKE_SUBAGENT", 14, "DONE")).expect("decode sub14"));

    let snap = reducer.snapshot();
    assert_eq!(status_of(&snap, "read_file"), ToolStatus::Completed);
    assert_eq!(status_of(&snap, "edit_file"), ToolStatus::Completed);
    assert_eq!(status_of(&snap, "write_file"), ToolStatus::Failed);
    assert_eq!(status_of(&snap, "manage_subagents"), ToolStatus::Completed);
    assert_eq!(
        status_of(&snap, "grep_search"),
        ToolStatus::Started,
        "typed results must match their own tool names rather than falling back to the first call"
    );
    assert_eq!(
        status_of(&snap, "mcp_spyc_get_spyc_context"),
        ToolStatus::Started,
        "call_mcp_tool with parameters must unwrap spyc tool name"
    );
}

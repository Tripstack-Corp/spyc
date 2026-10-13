use std::io::Write;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::*;
use crate::agent::codex_records::metrics;

pub(super) const THREAD: &str = "01a0f326-77c0-7821-9646-973bfc172364";

pub(super) fn binding(home: PathBuf) -> Binding {
    Binding {
        pane_id: "pane-a".to_owned(),
        thread_id: THREAD.to_owned(),
        codex_home: home,
        generation: 1,
    }
}

pub(super) fn record(kind: &str, payload: Value, second: u8) -> Value {
    json!({"type": kind, "timestamp": format!("2026-10-12T12:00:{second:02}Z"), "payload": payload})
}

pub(super) fn usage(total: u64, second: u8) -> Value {
    record(
        "token_usage_record",
        json!({"thread_id":THREAD,"session_id":THREAD,"turn_id":"turn-a",
        "usage":{"total_tokens":total,"input_tokens":8,"cached_input_tokens":4,"output_tokens":2,"reasoning_output_tokens":1},
        "turn_token_usage":{"total_tokens":total + 10}, "thread_token_usage":{"total_tokens":total + 100}}),
        second,
    )
}

pub(super) fn append(path: &std::path::Path, value: &Value) {
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{value}"
    )
    .unwrap();
}

pub(super) fn rollout() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let day = home.path().join("sessions/2026/10/12");
    std::fs::create_dir_all(&day).unwrap();
    let path = day.join(format!("rollout-2026-10-12T12-00-00-{THREAD}.jsonl"));
    std::fs::write(&path, header()).unwrap();
    (home, path)
}

pub(super) fn header() -> String {
    json!({"type":"session_meta","payload":{"id":THREAD,"history_base":{"thread_id":"parent-private"}}}).to_string() + "\n"
}

fn fold(reducer: &mut Reducer, value: &Value) {
    if let Some(event) = metrics::decode(value, THREAD) {
        reducer.fold(event);
    }
}

#[test]
fn usage_replay_replaces_snapshots_without_adding_subsets_or_families() {
    let mut reducer = Reducer::default();
    let first = usage(10, 1);
    fold(&mut reducer, &first);
    fold(&mut reducer, &first);
    fold(&mut reducer, &usage(20, 3));
    fold(&mut reducer, &usage(15, 2));
    fold(
        &mut reducer,
        &record(
            "event_msg",
            json!({"type":"token_count","info":{
        "last_token_usage":{"total_tokens":200}, "total_token_usage":{"total_tokens":900}, "model_context_window":475_000},
        "rate_limits":{"limit_id":"codex","primary":null,"secondary":null}}),
            4,
        ),
    );
    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.request.as_ref().unwrap().value.total, Some(20));
    assert_eq!(snapshot.turn.as_ref().unwrap().value.total, Some(30));
    assert_eq!(snapshot.thread.as_ref().unwrap().value.total, Some(120));
    assert_eq!(
        snapshot.legacy_thread.as_ref().unwrap().value.total,
        Some(900)
    );
    assert_eq!(
        snapshot.request.as_ref().unwrap().value.cached_input,
        Some(4)
    );
    assert_eq!(
        snapshot.request.as_ref().unwrap().value.reasoning_output,
        Some(1)
    );
    assert_eq!(snapshot.context_percent, None);
    assert_eq!(snapshot.limits.unwrap().value, LimitState::NotReported);
}

#[test]
fn unknown_invalid_and_zero_are_distinct_and_private_content_is_not_retained() {
    let mut reducer = Reducer::default();
    let private = "PRIVATE_SENTINEL";
    fold(
        &mut reducer,
        &record(
            "turn_context",
            json!({"model":"gpt-6.1-sol","effort":"xhigh",
        "approval_policy":"on-request","sandbox_policy":{"type":"workspace-write","paths":[private]},"developer_instructions":private}),
            1,
        ),
    );
    let mut value = usage(0, 2);
    value["payload"]["usage"] = json!({"total_tokens":0,"input_tokens":-1,"output_tokens":true,"cached_input_tokens":null,"secret":private});
    value["payload"]["arguments"] = json!(private);
    fold(&mut reducer, &value);
    fold(
        &mut reducer,
        &record("world_state", json!({"credentials":private}), 3),
    );
    let snapshot = reducer.snapshot();
    let tokens = &snapshot.request.as_ref().unwrap().value;
    assert_eq!(tokens.total, Some(0));
    assert_eq!(tokens.input, None);
    assert_eq!(tokens.output, None);
    assert!(!format!("{snapshot:?}").contains(private));
    assert!(!format!("{snapshot:?}").contains("arguments"));
}

#[test]
fn rollover_and_compaction_clear_current_data_but_keep_lifetime_samples() {
    let mut reducer = Reducer::default();
    fold(&mut reducer, &usage(10, 1));
    let legacy = record(
        "event_msg",
        json!({"type":"token_count","info":{"last_token_usage":{"total_tokens":80},"total_token_usage":{"total_tokens":500}}}),
        2,
    );
    fold(&mut reducer, &legacy);
    fold(
        &mut reducer,
        &record(
            "compacted",
            json!({"window_number":2,"latest_token_usage_record":usage(999, 3),"replacement_history":"PRIVATE_SENTINEL"}),
            3,
        ),
    );
    fold(&mut reducer, &legacy);
    assert!(reducer.snapshot().legacy_last.is_none());
    assert_eq!(reducer.snapshot().thread.unwrap().value.total, Some(110));
    assert_eq!(
        reducer.snapshot().legacy_thread.unwrap().value.total,
        Some(500)
    );
    fold(
        &mut reducer,
        &record(
            "turn_context",
            json!({"turn_id":"turn-b","model":"new-model"}),
            4,
        ),
    );
    fold(&mut reducer, &usage(90, 2));
    assert!(reducer.snapshot().request.is_none());
    assert!(reducer.snapshot().turn.is_none());
    assert_eq!(
        reducer.snapshot().mode.unwrap().value.model.as_deref(),
        Some("new-model")
    );
    assert_eq!(reducer.snapshot().compaction.unwrap().value, 2);
}

#[test]
fn tool_identity_deduplicates_reorders_and_bounds_without_retaining_arguments() {
    let mut reducer = Reducer::default();
    let completed = record(
        "event_msg",
        json!({"type":"item_completed","item":{"type":"CommandExecution","id":"call-a",
        "command":"PRIVATE_SENTINEL","aggregated_output":"PRIVATE_SENTINEL","exit_code":1,"started_at_ms":10,"completed_at_ms":30}}),
        3,
    );
    fold(&mut reducer, &completed);
    fold(&mut reducer, &completed);
    fold(
        &mut reducer,
        &record(
            "response_item",
            json!({"type":"function_call","id":"wire-a","call_id":"call-a","name":"shell","arguments":"PRIVATE_SENTINEL"}),
            1,
        ),
    );
    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.tools.len(), 1);
    assert_eq!(snapshot.tools[0].status, ToolStatus::Failed);
    assert_eq!(snapshot.tools[0].elapsed_ms, Some(20));
    assert!(!snapshot.tools_complete);
    assert!(!format!("{snapshot:?}").contains("PRIVATE_SENTINEL"));
    for index in 0..300 {
        fold(
            &mut reducer,
            &record(
                "response_item",
                json!({"type":"custom_tool_call","call_id":format!("call-{index}"),"name":"patch","input":"PRIVATE_SENTINEL"}),
                4,
            ),
        );
    }
    assert_eq!(reducer.snapshot().tools.len(), 64);
}

#[test]
fn foreign_records_and_closed_or_rebound_panes_cannot_supply_metrics() {
    let mut foreign = usage(10, 1);
    foreign["payload"]["thread_id"] = json!("other-thread");
    foreign["payload"]["session_id"] = json!("other-thread");
    assert!(metrics::decode(&foreign, THREAD).is_none());
    let (home, path) = rollout();
    append(&path, &usage(10, 1));
    let live = binding(home.path().to_owned());
    let update = stream::Reader::new(live.clone(), path).refresh();
    assert!(update.for_binding(Some(&live)).is_some());
    assert!(update.for_binding(None).is_none());
    for changed in [
        Binding {
            pane_id: "pane-b".to_owned(),
            ..live.clone()
        },
        Binding {
            thread_id: "other-thread".to_owned(),
            ..live.clone()
        },
        Binding {
            codex_home: PathBuf::from("/other-home"),
            ..live.clone()
        },
        Binding {
            generation: 2,
            ..live
        },
    ] {
        assert!(update.for_binding(Some(&changed)).is_none());
    }
}

#[test]
fn partial_writes_are_invisible_until_the_newline_and_chunking_is_equivalent() {
    let encoded = usage(42, 1).to_string() + "\n";
    for width in [1, 2, 7, 63, 255, encoded.len()] {
        let (home, path) = rollout();
        let mut reader = stream::Reader::new(binding(home.path().to_owned()), path.clone());
        reader.refresh();
        for chunk in encoded.as_bytes()[..encoded.len() - 1].chunks(width) {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(chunk)
                .unwrap();
            assert!(reader.refresh().snapshot.request.is_none());
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert_eq!(
            reader.refresh().snapshot.request.unwrap().value.total,
            Some(42)
        );
    }
}

#[test]
fn malformed_and_oversized_lines_do_not_hide_later_samples_or_grow_pending_memory() {
    let (home, path) = rollout();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b"not-json\n").unwrap();
    let oversized = record(
        "turn_context",
        json!({"model":"PRIVATE_SENTINEL","padding":"x".repeat(stream::MAX_LINE_BYTES)}),
        1,
    );
    writeln!(file, "{oversized}").unwrap();
    append(&path, &usage(12, 2));
    let mut reader = stream::Reader::new(binding(home.path().to_owned()), path);
    let snapshot = reader.refresh().snapshot;
    assert!(snapshot.mode.is_none());
    assert_eq!(snapshot.request.unwrap().value.total, Some(12));
}

#[test]
fn bounded_initial_tail_and_append_work_do_not_reread_history() {
    let (home, path) = rollout();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(&vec![b'x'; stream::MAX_TAIL_BYTES as usize + 100])
        .unwrap();
    file.write_all(b"\n").unwrap();
    append(&path, &usage(12, 1));
    let mut reader = stream::Reader::new(binding(home.path().to_owned()), path.clone());
    let first = reader.refresh();
    assert!(first.tail_only);
    assert_eq!(first.snapshot.thread.unwrap().value.total, Some(112));
    assert!(reader.bytes_read <= stream::MAX_TAIL_BYTES + stream::MAX_LINE_BYTES as u64 + 128);
    let before = reader.bytes_read;
    append(&path, &usage(22, 2));
    assert_eq!(
        reader.refresh().snapshot.request.unwrap().value.total,
        Some(22)
    );
    assert!(reader.bytes_read - before < stream::MAX_LINE_BYTES as u64 + 1024);
}

#[test]
fn truncation_replacement_deletion_and_cross_home_paths_clear_old_state() {
    let (home, path) = rollout();
    append(&path, &usage(10, 1));
    let mut reader = stream::Reader::new(binding(home.path().to_owned()), path.clone());
    assert!(reader.refresh().snapshot.thread.is_some());
    std::fs::write(&path, header()).unwrap();
    assert!(reader.refresh().snapshot.thread.is_none());
    append(&path, &usage(20, 2));
    assert!(reader.refresh().snapshot.thread.is_some());
    let replacement = path.with_extension("replacement");
    std::fs::write(&replacement, header()).unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert!(reader.refresh().snapshot.thread.is_none());
    std::fs::remove_file(&path).unwrap();
    let missing = reader.refresh();
    assert_eq!(missing.availability, Availability::Missing);
    assert!(missing.snapshot.thread.is_none());
    let (_other_home, other_path) = rollout();
    let mut wrong_home = stream::Reader::new(binding(home.path().to_owned()), other_path);
    assert_eq!(
        wrong_home.refresh().availability,
        Availability::IdentityMismatch
    );
}

#[test]
fn wrong_header_and_nonregular_sources_are_refused_without_reading_them_as_metrics() {
    let (home, path) = rollout();
    std::fs::write(
        &path,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"other-thread\"}}\n",
    )
    .unwrap();
    append(&path, &usage(10, 1));
    let mut reader = stream::Reader::new(binding(home.path().to_owned()), path);
    assert_eq!(
        reader.refresh().availability,
        Availability::IdentityMismatch
    );
    assert!(reader.refresh().snapshot.thread.is_none());
    let directory = home.path().join("sessions/directory");
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        stream::Reader::new(binding(home.path().to_owned()), directory)
            .refresh()
            .availability,
        Availability::Unreadable
    );
}

#[test]
fn late_tool_starts_enrich_completed_names_without_reopening_or_hiding_failure() {
    let mut reducer = Reducer::default();
    fold(
        &mut reducer,
        &record(
            "response_item",
            json!({"type":"function_call_output","call_id":"call-a","output":"PRIVATE_SENTINEL"}),
            3,
        ),
    );
    fold(
        &mut reducer,
        &record(
            "response_item",
            json!({"type":"function_call","call_id":"call-a","name":"shell","arguments":"PRIVATE_SENTINEL"}),
            1,
        ),
    );
    assert_eq!(reducer.snapshot().tools[0].name.as_deref(), Some("shell"));
    assert_eq!(reducer.snapshot().tools[0].status, ToolStatus::Completed);
    fold(
        &mut reducer,
        &record(
            "event_msg",
            json!({"type":"item_completed","item":{"type":"CommandExecution","id":"call-a","exit_code":1}}),
            4,
        ),
    );
    fold(
        &mut reducer,
        &record(
            "response_item",
            json!({"type":"function_call_output","call_id":"call-a","output":"private"}),
            5,
        ),
    );
    assert_eq!(reducer.snapshot().tools[0].status, ToolStatus::Failed);
}

#[test]
fn populated_unverified_limits_are_labelled_unsupported_and_samples_have_freshness() {
    let mut reducer = Reducer::default();
    fold(
        &mut reducer,
        &record(
            "event_msg",
            json!({"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":99,"resets_at":123}}}),
            1,
        ),
    );
    let sample = reducer.snapshot().limits.unwrap();
    assert_eq!(sample.value, LimitState::UnsupportedWindow);
    assert!(!sample.is_stale_at("2026-10-12T12:00:10Z".parse().unwrap(), 30));
    assert!(sample.is_stale_at("2026-10-12T12:01:00Z".parse().unwrap(), 30));
}

#[test]
fn same_inode_truncate_and_regrow_rebuilds_even_when_new_file_is_longer() {
    let (home, path) = rollout();
    append(&path, &usage(90, 5));
    let mut reader = stream::Reader::new(binding(home.path().to_owned()), path.clone());
    assert_eq!(
        reader.refresh().snapshot.request.unwrap().value.total,
        Some(90)
    );
    std::fs::write(
        &path,
        header() + &" ".repeat(1000) + "\n" + &(usage(10, 1).to_string() + "\n"),
    )
    .unwrap();
    assert_eq!(
        reader.refresh().snapshot.request.unwrap().value.total,
        Some(10)
    );
}

#[test]
fn fifo_and_symlink_escape_fail_without_waiting_for_a_writer() {
    let (home, _) = rollout();
    let fifo = home.path().join("sessions/fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        stream::Reader::new(binding(home.path().to_owned()), fifo)
            .refresh()
            .availability,
        Availability::Unreadable
    );
    let (_other_home, other) = rollout();
    let link = home.path().join("sessions/foreign");
    std::os::unix::fs::symlink(other, &link).unwrap();
    assert_eq!(
        stream::Reader::new(binding(home.path().to_owned()), link)
            .refresh()
            .availability,
        Availability::IdentityMismatch
    );
}

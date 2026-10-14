use serde_json::{Value, json};

use super::records::{Event, decode};
use super::reducer::MAX_TOOLS;
use super::*;

const SESSION: &str = "c1a0de00-0000-4000-8000-000000000001";

fn time(second: u32) -> String {
    format!("2026-10-13T12:{:02}:{:02}Z", second / 60, second % 60)
}

fn usage(input: u64, write: u64, read: u64, output: u64) -> Value {
    json!({"input_tokens": input, "cache_creation_input_tokens": write,
           "cache_read_input_tokens": read, "output_tokens": output,
           "service_tier": "standard", "server_tool_use": {"web_search_requests": 0}})
}

/// One assistant record, as Claude Code writes one per content block.
fn assistant(id: &str, second: u32, usage: Value, content: Value) -> Value {
    json!({"type": "assistant", "sessionId": SESSION, "timestamp": time(second),
           "isSidechain": false, "effort": "xhigh", "requestedModel": "claude-opus-5-5",
           "message": {"id": id, "model": "claude-opus-5-5", "role": "assistant",
                       "usage": usage, "content": content}})
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!([{"type": "tool_use", "id": id, "name": name, "input": input}])
}

fn result(second: u32, id: &str, is_error: bool) -> Value {
    json!({"type": "user", "sessionId": SESSION, "timestamp": time(second),
           "message": {"role": "user", "content": [
               {"type": "tool_result", "tool_use_id": id, "is_error": is_error, "content": "out"}]}})
}

/// A `user` record carrying a `TaskCreate`/`TaskUpdate`-shaped
/// `toolUseResult`, alongside the one tool_result block real transcripts
/// always pair it with.
fn task_result(second: u32, id: &str, outcome: Value) -> Value {
    json!({"type": "user", "sessionId": SESSION, "timestamp": time(second),
           "toolUseResult": outcome,
           "message": {"content": [
               {"type": "tool_result", "tool_use_id": id, "content": "ok"}]}})
}

fn compaction(second: u32, pre_tokens: u64) -> Value {
    json!({"type": "system", "subtype": "compact_boundary", "sessionId": SESSION,
           "timestamp": time(second), "content": "Conversation compacted",
           "compactMetadata": {"trigger": "auto", "preTokens": pre_tokens}})
}

fn fold_all(values: &[Value]) -> Snapshot {
    let mut reducer = Reducer::default();
    for value in values {
        if let Some(event) = decode(value, SESSION) {
            reducer.fold(event);
        }
    }
    reducer.snapshot()
}

/// The spike found 192 of 246 messages written more than once, so summing
/// records instead of messages inflates every total about 2.4 times.
#[test]
fn a_message_written_once_per_block_counts_once() {
    let first = usage(2, 30, 400, 5);
    let snapshot = fold_all(&[
        assistant(
            "msg-1",
            1,
            first.clone(),
            json!([{"type": "thinking", "thinking": "x"}]),
        ),
        assistant(
            "msg-1",
            1,
            first.clone(),
            json!([{"type": "text", "text": "x"}]),
        ),
        assistant("msg-1", 2, first, tool_use("call-1", "Bash", json!({}))),
        result(3, "call-1", false),
        assistant("msg-2", 4, usage(1, 10, 500, 7), json!([])),
    ]);
    assert_eq!(snapshot.messages, 2);
    assert_eq!(
        snapshot.totals,
        Usage {
            input: 3,
            cache_write: 40,
            cache_read: 900,
            output: 12
        }
    );
    assert_eq!(snapshot.request.unwrap().value.output, 7);
    assert_eq!(snapshot.context_tokens.unwrap().value, 1 + 10 + 500);
}

/// A repeat that reports different usage replaces what the message counted,
/// whether it follows at once or after other messages.
#[test]
fn a_repeat_with_new_usage_replaces_its_count() {
    let snapshot = fold_all(&[
        assistant("msg-1", 1, usage(2, 30, 400, 5), json!([])),
        assistant("msg-2", 2, usage(1, 1, 1, 1), json!([])),
        assistant("msg-1", 3, usage(2, 30, 400, 50), json!([])),
    ]);
    assert_eq!(snapshot.messages, 2);
    assert_eq!(snapshot.totals.output, 51);
}

/// After a compaction the context's size is unknown until the next request,
/// and a record from before the boundary can't restore it. Lifetime totals
/// carry on across it.
#[test]
fn a_compaction_ends_what_is_known_about_the_context_but_not_the_totals() {
    let before = [
        assistant("msg-1", 1, usage(2, 30, 440_000, 5), json!([])),
        compaction(10, 442_230),
    ];
    let snapshot = fold_all(&before);
    assert_eq!(snapshot.context_tokens, None);
    assert_eq!(snapshot.request.as_ref().unwrap().value.cache_read, 440_000);
    assert_eq!(snapshot.compactions, 1);
    let latest = snapshot.compaction.unwrap().value;
    assert_eq!(
        (latest.trigger.as_deref(), latest.pre_tokens),
        (Some("auto"), Some(442_230))
    );

    let late = assistant("msg-0", 5, usage(1, 1, 1, 1), json!([]));
    let snapshot = fold_all(&[before[0].clone(), before[1].clone(), late]);
    assert_eq!(snapshot.context_tokens, None, "a pre-boundary record");

    let after = assistant("msg-2", 20, usage(3, 9_000, 20_000, 8), json!([]));
    let snapshot = fold_all(&[before[0].clone(), before[1].clone(), after]);
    assert_eq!(snapshot.context_tokens.unwrap().value, 29_003);
    assert_eq!(snapshot.totals.cache_read, 460_000);
    assert_eq!(snapshot.messages, 2);
}

#[test]
fn a_tool_call_follows_its_result() {
    let snapshot = fold_all(&[
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("ok", "Read", json!({})),
        ),
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("bad", "Bash", json!({})),
        ),
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("open", "Edit", json!({})),
        ),
        result(2, "ok", false),
        result(3, "bad", true),
        result(4, "bad", false),
    ]);
    let status = |name: &str| {
        snapshot
            .tools
            .iter()
            .find(|tool| tool.name.as_deref() == Some(name))
            .unwrap()
            .status
    };
    assert_eq!(status("Read"), ToolStatus::Completed);
    assert_eq!(status("Bash"), ToolStatus::Failed, "a failure stays failed");
    assert_eq!(status("Edit"), ToolStatus::Started);
    assert!(snapshot.tools_complete);
    let names: Vec<_> = snapshot.tools.iter().map(|t| t.name.clone()).collect();
    assert_eq!(
        names.last().unwrap().as_deref(),
        Some("Bash"),
        "most recently changed last"
    );
}

/// A result for a call the read never saw, and an evicted call, both leave
/// the list short of exhaustive, and say so.
#[test]
fn the_tool_list_says_when_it_is_not_exhaustive() {
    assert!(!fold_all(&[result(1, "before-the-read", false)]).tools_complete);

    // Ids count down, so the oldest call is the last map key, not the first.
    let calls: Vec<Value> = (0..=MAX_TOOLS)
        .map(|n| {
            let id = format!("call-{:03}", MAX_TOOLS - n);
            assistant(
                &format!("msg-{n}"),
                u32::try_from(n).unwrap(),
                usage(1, 1, 1, 1),
                tool_use(&id, "Bash", json!({})),
            )
        })
        .collect();
    let snapshot = fold_all(&calls);
    assert_eq!(snapshot.tools.len(), MAX_TOOLS);
    assert!(!snapshot.tools_complete);
    assert_eq!(
        snapshot.tools.first().unwrap().at,
        time(1).parse().unwrap(),
        "the oldest call is the one evicted"
    );
}

#[test]
fn subagents_and_background_work_count_calls_not_records() {
    let agent = tool_use(
        "agent-1",
        "Agent",
        json!({"subagent_type": "general-purpose", "run_in_background": true}),
    );
    let shell = tool_use(
        "shell-1",
        "Bash",
        json!({"command": "gh run watch", "run_in_background": true}),
    );
    let snapshot = fold_all(&[
        assistant("msg-1", 1, usage(1, 1, 1, 1), agent.clone()),
        assistant("msg-1", 1, usage(1, 1, 1, 1), agent),
        assistant("msg-1", 1, usage(1, 1, 1, 1), shell),
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("p", "Read", json!({})),
        ),
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("old", "Task", json!({})),
        ),
    ]);
    assert_eq!(snapshot.subagent_calls, 2);
    assert_eq!(snapshot.background_calls, 2);
    assert!(snapshot.tools.iter().filter(|t| t.background).count() == 2);
}

#[test]
fn the_plan_is_the_latest_todo_list() {
    let todos = |statuses: &[&str]| {
        let todos: Vec<Value> = statuses
            .iter()
            .map(|status| json!({"content": "x", "status": status, "activeForm": "x"}))
            .collect();
        json!({ "todos": todos })
    };
    let snapshot = fold_all(&[
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("t1", "TodoWrite", todos(&["pending", "pending", "pending"])),
        ),
        assistant(
            "msg-2",
            2,
            usage(1, 1, 1, 1),
            tool_use(
                "t2",
                "TodoWrite",
                todos(&["completed", "in_progress", "pending", "mystery"]),
            ),
        ),
    ]);
    assert_eq!(
        snapshot.plan.unwrap().value,
        Plan {
            pending: 1,
            in_progress: 1,
            completed: 1
        }
    );
    assert_eq!(fold_all(&[]).plan, None, "no list is not an empty plan");
}

/// `TaskCreate` and `TaskUpdate` tally the same way `TodoWrite` does, folded
/// from the exact session this driver independently confirmed against:
/// 22 creates, 20 pending→in_progress, 20 in_progress→completed, 1 direct
/// pending→completed.
#[test]
fn task_create_and_update_fold_into_the_plan_count() {
    let created = |n: u32| task_result(n, &format!("c{n}"), json!({"task": {"id": n.to_string()}}));
    let transition = |n: u32, from: &str, to: &str| {
        task_result(
            n,
            &format!("u{n}"),
            json!({"success": true, "taskId": "1", "updatedFields": ["status"],
                   "statusChange": {"from": from, "to": to}}),
        )
    };
    let mut records: Vec<Value> = (0..22).map(created).collect();
    records.extend((100..120).map(|n| transition(n, "pending", "in_progress")));
    records.extend((200..220).map(|n| transition(n, "in_progress", "completed")));
    records.push(transition(300, "pending", "completed"));
    let plan = fold_all(&records).plan.unwrap().value;
    assert_eq!(
        plan,
        Plan {
            pending: 1,
            in_progress: 0,
            completed: 21
        }
    );
}

/// A description-only update carries no `statusChange` at all; it moves
/// nothing.
#[test]
fn a_task_update_with_no_status_change_moves_nothing() {
    let records = [
        task_result(1, "c1", json!({"task": {"id": "1"}})),
        task_result(
            2,
            "u1",
            json!({"success": true, "taskId": "1", "updatedFields": ["description"]}),
        ),
    ];
    assert_eq!(
        fold_all(&records).plan.unwrap().value,
        Plan {
            pending: 1,
            in_progress: 0,
            completed: 0
        }
    );
}

/// `Monitor`'s own result also carries a `taskId` (an unrelated background-job
/// id) — matched by shape, not by which tool ran, so it contributes nothing.
#[test]
fn an_unrelated_taskid_is_not_a_task_event() {
    let monitor = task_result(
        1,
        "m1",
        json!({"taskId": "bns6w2d3o", "timeoutMs": 600_000, "persistent": false}),
    );
    assert_eq!(fold_all(&[monitor]).plan, None);
}

/// `success: false` is its own signal, independent of the block's own
/// `is_error`: a transition it reports didn't actually happen.
#[test]
fn an_unsuccessful_task_update_does_not_move_the_plan() {
    let records = [
        task_result(1, "c1", json!({"task": {"id": "1"}})),
        task_result(
            2,
            "u1",
            json!({"success": false, "statusChange": {"from": "pending", "to": "in_progress"}}),
        ),
    ];
    assert_eq!(
        fold_all(&records).plan.unwrap().value,
        Plan {
            pending: 1,
            in_progress: 0,
            completed: 0
        },
        "the create stands; the unsuccessful update does not"
    );
}

/// An errored call's result isn't trusted for a transition, even if it still
/// carries a `toolUseResult`-shaped payload.
#[test]
fn an_errored_task_result_does_not_move_the_plan() {
    let errored = json!({"type": "user", "sessionId": SESSION, "timestamp": time(1),
        "toolUseResult": {"task": {"id": "1"}},
        "message": {"content": [
            {"type": "tool_result", "tool_use_id": "c1", "is_error": true, "content": "denied"}]}});
    assert_eq!(fold_all(&[errored]).plan, None);
}

/// A status this closed enum doesn't name is dropped, not guessed into one
/// of the three buckets.
#[test]
fn an_unrecognized_status_is_dropped_not_guessed() {
    let records = [
        task_result(1, "c1", json!({"task": {"id": "1"}})),
        task_result(
            2,
            "u1",
            json!({"success": true, "statusChange": {"from": "pending", "to": "blocked"}}),
        ),
    ];
    let plan = fold_all(&records).plan.unwrap().value;
    assert_eq!(
        plan,
        Plan {
            pending: 0,
            in_progress: 0,
            completed: 0
        },
        "the move out of pending is real even though the target bucket isn't known"
    );
}

/// The permission-mode record carries no timestamp; the records around it
/// date it.
#[test]
fn the_mode_follows_the_latest_record_of_each_kind() {
    let permission = |mode: &str| json!({"type": "permission-mode", "permissionMode": mode, "sessionId": SESSION});
    let mut second = assistant("msg-2", 9, usage(1, 1, 1, 1), json!([]));
    second["message"]["model"] = json!("claude-sonnet-5-5");
    let snapshot = fold_all(&[
        permission("default"),
        assistant("msg-1", 5, usage(1, 1, 1, 1), json!([])),
        permission("auto"),
        second,
    ]);
    let mode = snapshot.mode.unwrap();
    assert_eq!(mode.value.model.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(mode.value.effort.as_deref(), Some("xhigh"));
    assert_eq!(mode.value.permission_mode.as_deref(), Some("auto"));
    assert_eq!(mode.at, time(9).parse().unwrap());

    assert_eq!(
        fold_all(&[permission("plan")]).mode,
        None,
        "nothing dates a mode before the first timed record"
    );
}

/// Another session's records, a subagent's, and the records that carry
/// content but no metrics all contribute nothing.
#[test]
fn only_the_sessions_own_main_thread_counts() {
    let mut other = assistant("msg-x", 1, usage(9, 9, 9, 9), json!([]));
    other["sessionId"] = json!("another-session");
    let mut sidechain = assistant("msg-y", 1, usage(9, 9, 9, 9), json!([]));
    sidechain["isSidechain"] = json!(true);
    let mut untagged = assistant("msg-z", 1, usage(9, 9, 9, 9), json!([]));
    untagged.as_object_mut().unwrap().remove("sessionId");
    for value in [other, sidechain, untagged] {
        assert_eq!(decode(&value, SESSION), None, "{value}");
    }
}

/// The projection keeps labels, counts and ids; every field that carries
/// conversation content is dropped, including from a `Debug` dump.
#[test]
fn no_conversation_content_survives_the_projection() {
    let content = json!([
        {"type": "thinking", "thinking": "PRIVATE_REASONING_SENTINEL"},
        {"type": "text", "text": "PRIVATE_RESPONSE_SENTINEL"},
        {"type": "tool_use", "id": "call-1", "name": "Bash",
         "input": {"command": "PRIVATE_ARGUMENT_SENTINEL", "description": "PRIVATE_DESC_SENTINEL"}},
        {"type": "tool_use", "id": "call-2", "name": "TodoWrite",
         "input": {"todos": [{"content": "PRIVATE_TODO_SENTINEL", "status": "pending"}]}},
        {"type": "tool_use", "id": "call-3", "name": "TaskCreate",
         "input": {"subject": "PRIVATE_TASK_SUBJECT_SENTINEL", "description": "PRIVATE_TASK_DESC_SENTINEL",
                   "activeForm": "PRIVATE_TASK_FORM_SENTINEL"}},
    ]);
    let records = [
        assistant("msg-1", 1, usage(1, 1, 1, 1), content),
        json!({"type": "user", "sessionId": SESSION, "timestamp": time(2),
               "cwd": "/PRIVATE_PATH_SENTINEL", "toolUseResult": {"stdout": "PRIVATE_OUTPUT_SENTINEL"},
               "message": {"content": [{"type": "tool_result", "tool_use_id": "call-1",
                                        "content": "PRIVATE_RESULT_SENTINEL"}]}}),
        task_result(
            5,
            "call-3",
            json!({"task": {"id": "9", "subject": "PRIVATE_TASK_RESULT_SENTINEL",
                                                 "description": "PRIVATE_TASK_RESULT_DESC_SENTINEL"}}),
        ),
        task_result(
            6,
            "call-4",
            json!({"success": true, "taskId": "9",
                                        "statusChange": {"from": "pending", "to": "in_progress"}}),
        ),
        json!({"type": "user", "sessionId": SESSION, "timestamp": time(3),
               "message": {"content": "PRIVATE_PROMPT_SENTINEL"}}),
        json!({"type": "system", "subtype": "compact_boundary", "sessionId": SESSION,
               "timestamp": time(4), "content": "PRIVATE_SUMMARY_SENTINEL",
               "compactMetadata": {"trigger": "manual", "preTokens": 10}}),
        json!({"type": "ai-title", "aiTitle": "PRIVATE_TITLE_SENTINEL", "sessionId": SESSION}),
    ];
    let events: Vec<Event> = records.iter().filter_map(|r| decode(r, SESSION)).collect();
    assert_eq!(
        events.len(),
        5,
        "the prompt and the title contribute nothing"
    );
    let dump = format!("{events:?} {:?}", fold_all(&records));
    assert!(!dump.contains("SENTINEL"), "{dump}");
    assert_eq!(
        fold_all(&records).plan.unwrap().value,
        Plan {
            pending: 0,
            in_progress: 1,
            completed: 0
        },
        "the create and the transition still counted"
    );
}

/// A field the transcript didn't report stays unknown; it never becomes zero.
#[test]
fn absence_and_invalid_values_do_not_become_zero() {
    let mut partial = usage(1, 1, 1, 1);
    partial
        .as_object_mut()
        .unwrap()
        .remove("cache_read_input_tokens");
    let mut negative = usage(1, 1, 1, 1);
    negative["output_tokens"] = json!(-5);
    let snapshot = fold_all(&[
        assistant("msg-1", 1, partial, json!([])),
        assistant("msg-2", 2, negative, json!([])),
    ]);
    assert_eq!(snapshot.request, None);
    assert_eq!(snapshot.context_tokens, None);
    assert_eq!(snapshot.messages, 0);

    let mut bad = assistant("msg-3", 3, usage(1, 1, 1, 1), json!([]));
    bad["message"]["model"] = json!("claude\u{1b}[31m");
    bad["effort"] = json!("x".repeat(81));
    let snapshot = fold_all(&[bad]);
    assert_eq!(snapshot.mode, None, "unprintable labels are not a mode");
    assert_eq!(snapshot.messages, 1);
}

/// An MCP tool's name carries its server's, which can be private.
#[test]
fn only_spycs_mcp_server_keeps_its_name() {
    let content = json!([
        {"type": "tool_use", "id": "call-1", "name": "mcp__PRIVATE_SERVER__lookup", "input": {}},
        {"type": "tool_use", "id": "call-2", "name": "mcp__spycish__lookup", "input": {}},
        {"type": "tool_use", "id": "call-3", "name": "mcp__spyc__report_status", "input": {}},
        {"type": "tool_use", "id": "call-4", "name": "Bash", "input": {}},
    ]);
    let snapshot = fold_all(&[assistant("msg-1", 1, usage(1, 1, 1, 1), content)]);
    let names: Vec<_> = snapshot
        .tools
        .iter()
        .map(|tool| tool.name.as_deref())
        .collect();
    assert_eq!(
        names,
        [
            Some("mcp"),
            Some("mcp"),
            Some("mcp__spyc__report_status"),
            Some("Bash")
        ]
    );
}

#[test]
fn the_home_is_the_config_dir_else_dot_claude_and_never_relative() {
    let os = |s: &str| Some(std::ffi::OsString::from(s));
    let path = |s: &str| Some(std::path::PathBuf::from(s));
    assert_eq!(home(os("/etc/claude"), os("/home/u")), path("/etc/claude"));
    assert_eq!(home(os(""), os("/home/u")), path("/home/u/.claude"));
    assert_eq!(home(None, os("/home/u")), path("/home/u/.claude"));
    assert_eq!(home(os("claude"), os("/home/u")), None);
    assert_eq!(home(None, os("home")), None);
    assert_eq!(home(None, os("")), None);
    assert_eq!(home(None, None), None);
}

/// A call counts once however many records repeat its block, and a failure
/// counts against the tool that made it, once.
#[test]
fn tool_counts_tally_calls_and_failures_by_tool() {
    let snapshot = fold_all(&[
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("c1", "Bash", json!({})),
        ),
        assistant(
            "msg-1",
            1,
            usage(1, 1, 1, 1),
            tool_use("c1", "Bash", json!({})),
        ),
        assistant(
            "msg-2",
            2,
            usage(1, 1, 1, 1),
            tool_use("c2", "Bash", json!({})),
        ),
        assistant(
            "msg-3",
            3,
            usage(1, 1, 1, 1),
            tool_use("c3", "Read", json!({})),
        ),
        result(4, "c1", false),
        result(5, "c2", true),
        result(6, "c2", true),
        result(7, "never-started", true),
    ]);
    let counts: Vec<_> = snapshot
        .tool_counts
        .iter()
        .map(|c| (c.name.as_str(), c.calls, c.failed))
        .collect();
    assert_eq!(counts, [("Bash", 2, 1), ("Read", 1, 0)]);
}

#[test]
fn tool_counts_stay_bounded_and_lose_no_call() {
    let records: Vec<Value> = (0..40)
        .map(|i| {
            assistant(
                &format!("msg-{i}"),
                i,
                usage(1, 1, 1, 1),
                tool_use(&format!("c{i}"), &format!("Tool{i}"), json!({})),
            )
        })
        .collect();
    let snapshot = fold_all(&records);
    assert_eq!(snapshot.tool_counts.len(), MAX_TOOL_NAMES + 1);
    let other = snapshot
        .tool_counts
        .iter()
        .find(|c| c.name == OTHER_TOOLS)
        .unwrap();
    assert_eq!(other.calls as usize, 40 - MAX_TOOL_NAMES);
    let total: u32 = snapshot.tool_counts.iter().map(|c| c.calls).sum();
    assert_eq!(total, 40);
    assert_eq!(
        snapshot.tool_counts[0].name, OTHER_TOOLS,
        "most called first"
    );
}

//! How long each MCP call may take, on both sides of the socket. The server
//! answers a `tools/call` within [`reply_budget`], with its result or with its
//! own timed-out error; the stdio proxy waits [`proxy_wait`] for that answer,
//! which is always longer, so the server's answer reaches the agent first.

use std::time::Duration;

use serde_json::Value;

use super::PROXY_IO_TIMEOUT;

/// Per-call ceiling for the read tools that walk the filesystem / git (search,
/// git status / log / diff, worktree listing). Kept a few seconds below
/// `PROXY_IO_TIMEOUT` so a slow call fails *server-side* with a clean JSON-RPC
/// error first. Derived from `PROXY_IO_TIMEOUT` so the two can't drift apart.
pub(super) const READ_TOOL_TIMEOUT: Duration = {
    let proxy_secs = PROXY_IO_TIMEOUT.as_secs();
    Duration::from_secs(if proxy_secs > 5 {
        proxy_secs - 5
    } else {
        proxy_secs
    })
};

/// How long the server waits for the main loop's answer to a command.
const LOOP_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Worktree mutations can run a status walk + tar.zst archive off the main
/// loop (§5 of docs/archive/WORKTREE_MCP_PLAN.md); a large tree easily outlasts
/// the interactive 5 s window.
const WORKTREE_REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// P2 `wait_for_scope_clear` bounds: the default wait when the caller gives no
/// `timeout_ms`, and a hard ceiling so a wedged waiter can't pin a socket thread
/// forever.
const DEFAULT_SCOPE_WAIT_MS: u64 = 300_000;
const MAX_SCOPE_WAIT_MS: u64 = 600_000;

/// How much longer the proxy waits than the server's budget for the call.
const PROXY_MARGIN: Duration = Duration::from_secs(5);

/// The scope wait `args` asks for, in ms, clamped to the ceiling.
pub(super) fn scope_wait_ms(args: &Value) -> u64 {
    args["timeout_ms"]
        .as_u64()
        .unwrap_or(DEFAULT_SCOPE_WAIT_MS)
        .min(MAX_SCOPE_WAIT_MS)
}

/// How long the server may take to answer `tools/call` `name` with `args`.
pub(super) fn reply_budget(name: &str, args: &Value) -> Duration {
    match name {
        "create_worktree" | "remove_worktree" | "clean_worktree" => WORKTREE_REPLY_TIMEOUT,
        // The loop parks this and replies (cleared/timed_out) by `timeout_ms`;
        // wait a hair longer so that reply always arrives.
        "wait_for_scope_clear" => {
            Duration::from_millis(scope_wait_ms(args)) + Duration::from_secs(2)
        }
        "search_paths" | "search_content" | "list_worktrees" | "git_status" | "git_log"
        | "git_diff" => READ_TOOL_TIMEOUT,
        _ => LOOP_REPLY_TIMEOUT,
    }
}

/// How long the stdio proxy waits for the answer to `request`: past the
/// server's budget for it, and never less than `floor`.
pub(super) fn proxy_wait(request: &Value, floor: Duration) -> Duration {
    // SPYC-TRAP(proxy-outwaits-server-replies): the proxy ends the agent's
    // connection when it gives up, so it must outwait every reply budget.
    if request["method"] != "tools/call" {
        return floor;
    }
    let params = &request["params"];
    let budget = reply_budget(
        params["name"].as_str().unwrap_or_default(),
        &params["arguments"],
    );
    floor.max(budget + PROXY_MARGIN)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(name: &str, arguments: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": name, "arguments": arguments}})
    }

    /// Every tool spyc offers, with the longest wait it can ask for: the
    /// proxy outwaits the server's budget for it.
    #[test]
    fn the_proxy_outwaits_every_tools_reply_budget() {
        let tools = crate::mcp::tool_schemas::tools();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .expect("the tool list")
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"wait_for_scope_clear"), "{names:?}");
        let longest = json!({"timeout_ms": u64::MAX, "paths": ["src/**"]});
        for name in names {
            let budget = reply_budget(name, &longest);
            let wait = proxy_wait(&call(name, longest.clone()), PROXY_IO_TIMEOUT);
            assert!(
                wait > budget,
                "{name}: the proxy waits {wait:?}, the server {budget:?}"
            );
        }
    }

    #[test]
    fn a_scope_wait_is_clamped_and_waited_out() {
        assert_eq!(scope_wait_ms(&json!({})), DEFAULT_SCOPE_WAIT_MS);
        assert_eq!(scope_wait_ms(&json!({"timeout_ms": 1_000})), 1_000);
        assert_eq!(
            scope_wait_ms(&json!({"timeout_ms": u64::MAX})),
            MAX_SCOPE_WAIT_MS
        );
        let wait = proxy_wait(
            &call("wait_for_scope_clear", json!({"timeout_ms": 1_000})),
            Duration::ZERO,
        );
        assert_eq!(wait, Duration::from_secs(1 + 2) + PROXY_MARGIN);
    }

    /// Anything but a tool call, and a fast tool, waits the floor: a server
    /// that never answers still ends the connection in good time.
    #[test]
    fn other_requests_wait_the_floor() {
        let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        assert_eq!(proxy_wait(&ping, PROXY_IO_TIMEOUT), PROXY_IO_TIMEOUT);
        assert_eq!(proxy_wait(&Value::Null, PROXY_IO_TIMEOUT), PROXY_IO_TIMEOUT);
        let fast = call("report_status", json!({"status": "working"}));
        assert_eq!(proxy_wait(&fast, PROXY_IO_TIMEOUT), PROXY_IO_TIMEOUT);
    }

    /// The read-tool deadline must sit strictly below the proxy's socket
    /// timeout, or a slow call races the proxy and the connection dies anyway.
    #[test]
    fn read_tool_timeout_stays_below_proxy_timeout() {
        assert!(READ_TOOL_TIMEOUT < PROXY_IO_TIMEOUT);
    }
}

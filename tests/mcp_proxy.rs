#![cfg(unix)]
//! `spyc --mcp` as a stdio proxy: which spyc it reaches, and the pane it names
//! in `initialize`.
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const SHARED_DAEMON: &[&str] = &["codex", "app-server", "--managed-daemon"];

/// Run `spyc --mcp` in `cwd` under a parent whose argv is `parent_args`, as
/// codex's shared daemon runs a session's MCP server under itself.
fn spawn_mcp(parent_args: &[&str], cwd: &Path, state_home: &Path) -> Command {
    let mut script = vec!["-c", "\"$0\" --mcp; :", env!("CARGO_BIN_EXE_spyc")];
    script.extend_from_slice(parent_args);
    let mut command = Command::new("sh");
    command
        .args(&script)
        .current_dir(cwd)
        .env("XDG_STATE_HOME", state_home)
        .env("HOME", state_home)
        .env_remove("SPYC_MCP_SOCK")
        .env_remove("SPYC_PANE_ID")
        .env_remove("SPYC_CONTEXT")
        .env_remove("SPYC_MCP_DEBUG")
        .stdin(Stdio::piped());
    command
}

fn reap(mut parent: Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while parent.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = parent.kill();
    let _ = parent.wait();
}

/// The spyc a proxy connected to.
#[derive(Debug, PartialEq, Eq)]
enum Reached {
    /// The one its inherited `SPYC_MCP_SOCK` names.
    Inherited,
    /// The one rooted at its cwd, found by discovery.
    Discovered,
}

/// Run `spyc --mcp` under a parent whose argv is `parent_args`, inheriting
/// `SPYC_MCP_SOCK` and `SPYC_PANE_ID=tab-1` from one live spyc while another
/// is rooted at its cwd; send it an `initialize` and return the spyc it
/// reached and what it forwarded there.
fn forwarded_initialize(parent_args: &[&str]) -> (Reached, Value) {
    let temp = tempfile::Builder::new()
        .prefix("spyc-proxy-")
        .tempdir_in("/tmp")
        .unwrap();
    let inherited_socket = temp.path().join("inherited.sock");
    let inherited = UnixListener::bind(&inherited_socket).unwrap();
    // A spyc rooted at the project, registered as `record_root` registers
    // one: a socket and a root sidecar in the state dir, under a live pid.
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let state = temp.path().join("state/spyc");
    std::fs::create_dir_all(&state).unwrap();
    let pid = std::process::id();
    std::fs::write(
        state.join(format!("mcp-{pid}.root")),
        project.to_str().unwrap(),
    )
    .unwrap();
    let discovered = UnixListener::bind(state.join(format!("mcp-{pid}.sock"))).unwrap();
    inherited.set_nonblocking(true).unwrap();
    discovered.set_nonblocking(true).unwrap();

    let mut parent = spawn_mcp(parent_args, &project, &temp.path().join("state"))
        .env("SPYC_MCP_SOCK", &inherited_socket)
        .env("SPYC_PANE_ID", "tab-1")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = parent.stdin.take().unwrap();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let (reached, stream): (Reached, UnixStream) = loop {
        if let Ok((stream, _)) = inherited.accept() {
            break (Reached::Inherited, stream);
        }
        if let Ok((stream, _)) = discovered.accept() {
            break (Reached::Discovered, stream);
        }
        assert!(Instant::now() < deadline, "the proxy reached no spyc");
        std::thread::sleep(Duration::from_millis(10));
    };
    // macOS hands an accepted socket its listener's non-blocking flag.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut length = None;
    loop {
        let mut header = String::new();
        assert!(reader.read_line(&mut header).unwrap() > 0, "proxy closed");
        if header == "\r\n" {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut body = vec![0; length.unwrap()];
    reader.read_exact(&mut body).unwrap();
    drop(stdin);
    drop(reader);
    reap(parent);
    (reached, serde_json::from_slice(&body).unwrap())
}

/// The proxy reaches the spyc that launched its agent and names its pane, so
/// the connection binds to its tab.
#[test]
fn the_proxy_reaches_its_spyc_and_names_its_pane() {
    let (reached, initialize) = forwarded_initialize(&["agent"]);
    assert_eq!(reached, Reached::Inherited);
    assert_eq!(
        initialize["params"]["_meta"]["spyc/paneId"], "tab-1",
        "{initialize}"
    );
}

/// Under codex's shared daemon the inherited socket and pane are those of
/// whichever tab started the daemon, which can belong to another spyc. The
/// daemon starts a session's MCP server in that session's cwd, so the proxy
/// finds its spyc there and names no pane.
#[test]
fn under_codex_shared_daemon_the_proxy_reaches_the_spyc_rooted_at_its_cwd() {
    let (reached, initialize) = forwarded_initialize(SHARED_DAEMON);
    assert_eq!(reached, Reached::Discovered);
    assert!(
        initialize["params"]["_meta"]["spyc/paneId"].is_null(),
        "{initialize}"
    );
}

/// What `get_spyc_context` answers when no spyc is reachable, from a
/// `spyc --mcp` that inherited `SPYC_CONTEXT`. `launched` gives it the pane env
/// of an agent spyc launched, whose spyc has since gone.
fn context_with_no_spyc_reachable(parent_args: &[&str], launched: bool) -> String {
    let temp = tempfile::Builder::new()
        .prefix("spyc-direct-")
        .tempdir_in("/tmp")
        .unwrap();
    let context = temp.path().join("starter-context.json");
    std::fs::write(&context, r#"{"cwd":"/starter/project","picks":[]}"#).unwrap();
    let mut command = spawn_mcp(parent_args, temp.path(), &temp.path().join("state"));
    command.env("SPYC_CONTEXT", &context);
    if launched {
        command
            .env("SPYC_MCP_SOCK", temp.path().join("gone.sock"))
            .env("SPYC_PANE_ID", "tab-1");
    }
    let mut parent = command.stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = parent.stdin.take().unwrap();
    stdin
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"get_spyc_context\",\"arguments\":{}}}\n",
        )
        .unwrap();
    drop(stdin);
    let mut out = String::new();
    parent
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    reap(parent);
    out
}

/// An agent spyc launched keeps read-only tools after its spyc has gone, served
/// from the context file its pane named. A session on codex's shared daemon
/// may not be one spyc launched, and with no spyc rooted at it gets no tools.
#[test]
fn a_launched_agent_outliving_its_spyc_keeps_read_only_tools() {
    let answer = context_with_no_spyc_reachable(&["agent"], true);
    assert!(answer.contains("/starter/project"), "{answer}");

    let answer = context_with_no_spyc_reachable(SHARED_DAEMON, true);
    let reply: Value = serde_json::from_str(answer.trim()).unwrap();
    assert_eq!(reply["id"], 1, "{answer}");
    assert!(reply["error"].is_object(), "{answer}");
    assert!(!answer.contains("/starter/project"), "{answer}");
}

/// An agent spyc didn't launch gets a server with nothing in it, even inside
/// the tree a running spyc is rooted at: no tools to call and no instructions
/// steering it toward them. Wired into an agent's user configuration, spyc's
/// entry starts in every session of that agent, and a spyc rooted at `$HOME`
/// would otherwise answer every one of them.
#[test]
fn an_agent_spyc_did_not_launch_gets_no_tools() {
    let temp = tempfile::Builder::new()
        .prefix("spyc-outside-")
        .tempdir_in("/tmp")
        .unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let state = temp.path().join("state/spyc");
    std::fs::create_dir_all(&state).unwrap();
    let pid = std::process::id();
    std::fs::write(
        state.join(format!("mcp-{pid}.root")),
        project.to_str().unwrap(),
    )
    .unwrap();
    let running = UnixListener::bind(state.join(format!("mcp-{pid}.sock"))).unwrap();
    running.set_nonblocking(true).unwrap();

    let mut parent = spawn_mcp(&["agent"], &project, &temp.path().join("state"))
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = parent.stdin.take().unwrap();
    stdin
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n\
              {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
              {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n\
              {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"navigate_to\",\"arguments\":{\"path\":\"/\"}}}\n",
        )
        .unwrap();
    drop(stdin);
    let mut out = String::new();
    parent
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    reap(parent);

    match running.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("it reached the running spyc: {other:?}"),
    }
    let replies: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 3, "{out}");
    assert!(
        replies[0]["result"]["capabilities"]["tools"].is_object(),
        "{out}"
    );
    assert!(replies[0]["result"].get("instructions").is_none(), "{out}");
    assert_eq!(
        replies[1]["result"]["tools"],
        serde_json::json!([]),
        "{out}"
    );
    assert!(replies[2]["error"].is_object(), "{out}");
}

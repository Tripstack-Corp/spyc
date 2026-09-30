//! Two spycs in one directory (#22, #11): every agent reaches the spyc that
//! launched it, and neither spyc takes the directory from the other.

use std::sync::mpsc::{self, Receiver};

use super::*;

#[derive(Clone, Copy, Debug)]
enum Agent {
    Claude,
    Codex,
    Agy,
}

/// The `SPYC_MCP_SOCK` an agent's `spyc --mcp` proxy starts with, given the
/// `spyc` entry in its config and the socket its pane's env names. Models the
/// agents as probed on 2026-09-30 (Claude Code, codex-cli 0.158.0, agy 1.1.26):
/// claude and agy start an MCP server with their own environment under the
/// entry's `env`; codex clears the environment down to a fixed allow-list,
/// adds the names the entry's `env_vars` lists, then applies `env`.
fn socket_the_proxy_sees(agent: Agent, entry: &Value, pane_sock: &str) -> Option<String> {
    let pinned = entry["env"]["SPYC_MCP_SOCK"].as_str().map(String::from);
    let inherited = match agent {
        Agent::Claude | Agent::Agy => true,
        Agent::Codex => entry["env_vars"]
            .as_array()
            .is_some_and(|names| names.iter().any(|n| n == "SPYC_MCP_SOCK")),
    };
    pinned.or_else(|| inherited.then(|| pane_sock.to_string()))
}

fn entry(agent: Agent, dir: &Path) -> Value {
    match agent {
        Agent::Claude | Agent::Agy => {
            let file = match agent {
                Agent::Claude => dir.join(".mcp.json"),
                _ => dir.join(".agents/mcp_config.json"),
            };
            let v: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
            v["mcpServers"]["spyc"].clone()
        }
        Agent::Codex => {
            let text = std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap();
            let v: toml::Value = toml::from_str(&text).unwrap();
            serde_json::to_value(&v["mcp_servers"]["spyc"]).unwrap()
        }
    }
}

/// What a spyc of today writes: its own socket, pinned in the entry.
fn pinned_entry(agent: Agent, dir: &Path, sock: &str) {
    match agent {
        Agent::Claude | Agent::Agy => {
            let file = match agent {
                Agent::Claude => dir.join(".mcp.json"),
                _ => dir.join(".agents/mcp_config.json"),
            };
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            let v = json!({"mcpServers": {"spyc": {
                "command": "spyc", "args": ["--mcp"], "env": {"SPYC_MCP_SOCK": sock}}}});
            std::fs::write(file, v.to_string()).unwrap();
        }
        Agent::Codex => {
            std::fs::create_dir_all(dir.join(".codex")).unwrap();
            std::fs::write(
                dir.join(".codex/config.toml"),
                format!(
                    "[mcp_servers.spyc]\ncommand = \"spyc\"\nargs = [\"--mcp\"]\n\
                     [mcp_servers.spyc.env]\nSPYC_MCP_SOCK = \"{sock}\"\n"
                ),
            )
            .unwrap();
        }
    }
}

/// A live spyc socket that records the method of every message it receives.
fn live_sibling(sock: &Path) -> Receiver<String> {
    let listener = bind_test_socket(sock);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut reader = io::BufReader::new(stream);
                while let Ok(msg) = read_lsp_message(&mut reader) {
                    let v: Value = serde_json::from_str(&msg).unwrap_or_default();
                    let _ = tx.send(v["method"].as_str().unwrap_or("").to_string());
                }
            });
        }
    });
    rx
}

/// Sibling `a` configured the directory and has agents there. This spyc (`b`)
/// then launches an agent of each kind in the same directory. Afterwards an
/// agent `a` launches must still reach `a`, one `b` launches must reach `b`,
/// and `a` must not have been told it lost anything. On the tree this test was
/// written against, `b` rewrote each entry to pin its own socket and sent `a`
/// `spyc/disconnected`, so `a`'s next agent silently talked to `b`.
#[test]
fn two_spycs_in_one_directory_each_keep_their_own_agents() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = tmp.path().join("proj");
        std::fs::create_dir(&dir).unwrap();
        let a_sock = tmp.path().join("mcp-4242.sock");
        let a_heard = live_sibling(&a_sock);
        let a = a_sock.to_string_lossy().into_owned();
        let b = crate::mcp::socket_path()
            .expect("a state dir")
            .to_string_lossy()
            .into_owned();

        for agent in [Agent::Claude, Agent::Codex, Agent::Agy] {
            pinned_entry(agent, &dir, &a);
            match agent {
                // Past `ensure_mcp_json`'s enterprise gate, which reads the
                // host's managed config and would make this host-dependent.
                Agent::Claude => {
                    super::super::config::ensure_spyc_in_mcp_json(&dir.join(".mcp.json"))
                        .map(|_| ())
                }
                Agent::Codex => ensure_codex_config_toml(&dir).map(|_| ()),
                Agent::Agy => ensure_agy_mcp_config(&dir).map(|_| ()),
            }
            .unwrap();

            let written = entry(agent, &dir);
            assert_eq!(
                socket_the_proxy_sees(agent, &written, &a).as_deref(),
                Some(a.as_str()),
                "{agent:?}: an agent `a` launches reaches `a`: {written}"
            );
            assert_eq!(
                socket_the_proxy_sees(agent, &written, &b).as_deref(),
                Some(b.as_str()),
                "{agent:?}: an agent `b` launches reaches `b`: {written}"
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
        let told: Vec<String> = a_heard.try_iter().filter(|m| !m.is_empty()).collect();
        assert!(told.is_empty(), "`a` was sent {told:?}");
    });
}

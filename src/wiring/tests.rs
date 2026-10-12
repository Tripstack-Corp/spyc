use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Env, Part, install, remove, status};
use crate::state::sessions::AgentKind;

/// A stand-in for the `claude` CLI: logs each call's arguments and keeps
/// `.claude.json` the way `claude mcp add-json` / `remove` would.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "$*" >> "$HOME/claude-calls"
file="${CLAUDE_CONFIG_DIR:-$HOME}/.claude.json"
case "$2" in
  add-json) printf '{"mcpServers":{"%s":%s}}\n' "$5" "$6" > "$file" ;;
  remove) printf '{"mcpServers":{}}\n' > "$file" ;;
esac
"#;

struct Sandbox {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    env: Env,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let cli = root.join("bin/claude");
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(&cli, FAKE_CLAUDE).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = Env {
            home,
            claude_config_dir: None,
            managed_mcp: vec![root.join("managed/managed-mcp.json")],
            managed_settings: vec![root.join("managed/managed-settings.json")],
            claude_cli: Some(cli),
            spyc: "spyc".to_string(),
        };
        Self {
            _tmp: tmp,
            root,
            env,
        }
    }

    fn home(&self, rel: &str) -> PathBuf {
        self.env.home.join(rel)
    }

    fn put(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn claude_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.home("claude-calls"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn commands(v: &Value) -> String {
    v.to_string()
}

#[test]
fn claude_hooks_go_into_user_settings_beside_the_users_own() {
    let s = Sandbox::new();
    let settings = s.home(".claude/settings.json");
    s.put(
        &settings,
        r#"{"permissions":{"allow":["Bash"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo bye"}]}]}}"#,
    );
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert_eq!(st.hooks, Part::Current, "{st:?}");
    assert_eq!(st.hooks_file, settings);
    let v = json(&settings);
    assert_eq!(v["permissions"], json!({"allow": ["Bash"]}));
    let all = commands(&v);
    assert!(all.contains("echo bye"), "{all}");
    assert!(all.contains("spyc --report-status working"), "{all}");
    assert_eq!(
        status(AgentKind::Claude, &s.env).unwrap().hooks,
        Part::Current
    );
}

/// The user's own key order survives an install and a removal: a dotfiles repo
/// would otherwise show the whole file rewritten.
#[test]
fn the_users_key_order_survives() {
    let s = Sandbox::new();
    let settings = s.home(".claude/settings.json");
    s.put(
        &settings,
        r#"{"theme":"dark","model":"opus","env":{"Z":"1","A":"2"},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo bye"}]}]}}"#,
    );
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    install(AgentKind::Claude, &s.env).unwrap();
    let v = json(&settings);
    assert_eq!(keys(&v), ["theme", "model", "env", "hooks"]);
    assert_eq!(keys(&v["env"]), ["Z", "A"]);
    remove(AgentKind::Claude, &s.env).unwrap();
    let v = json(&settings);
    assert_eq!(keys(&v), ["theme", "model", "env", "hooks"]);
    assert_eq!(keys(&v["hooks"]), ["Stop"]);
}

/// An install that changes nothing writes nothing: claude reloads its settings
/// on every change, and a dotfiles repo would show a diff.
#[test]
fn a_second_install_writes_nothing() {
    let s = Sandbox::new();
    install(AgentKind::Claude, &s.env).unwrap();
    install(AgentKind::Agy, &s.env).unwrap();
    let files = [
        s.home(".claude/settings.json"),
        s.home(".gemini/config/hooks.json"),
        s.home(".gemini/config/mcp_config.json"),
    ];
    let inodes: Vec<u64> = files
        .iter()
        .map(|f| std::fs::metadata(f).unwrap().ino())
        .collect();
    install(AgentKind::Claude, &s.env).unwrap();
    install(AgentKind::Agy, &s.env).unwrap();
    for (file, ino) in files.iter().zip(inodes) {
        assert_eq!(
            std::fs::metadata(file).unwrap().ino(),
            ino,
            "{}",
            file.display()
        );
    }
}

/// A dotfiles manager keeps `settings.json` as a link into its repo. Replacing
/// the link with a file would cut the user's config off from that repo.
#[test]
fn a_linked_settings_file_stays_a_link() {
    let s = Sandbox::new();
    let kept = s.root.join("dotfiles/settings.json");
    s.put(&kept, r#"{"model":"opus"}"#);
    let settings = s.home(".claude/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&kept, &settings).unwrap();

    assert_eq!(
        install(AgentKind::Claude, &s.env).unwrap().hooks,
        Part::Current
    );
    assert!(settings.is_symlink(), "the link was replaced");
    let v = json(&kept);
    assert_eq!(v["model"], "opus");
    assert!(commands(&v).contains("--report-status"));
}

#[test]
fn an_edited_file_keeps_its_mode_and_a_new_one_is_readable() {
    let s = Sandbox::new();
    let settings = s.home(".claude/settings.json");
    s.put(&settings, "{}");
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o600)).unwrap();
    install(AgentKind::Claude, &s.env).unwrap();
    install(AgentKind::Agy, &s.env).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&settings), 0o600);
    assert_eq!(mode(&s.home(".gemini/config/hooks.json")), 0o644);
}

#[test]
fn a_link_to_nothing_is_refused_and_left_alone() {
    let s = Sandbox::new();
    let settings = s.home(".claude/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(s.root.join("gone.json"), &settings).unwrap();
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert!(matches!(st.hooks, Part::Refused(_)), "{st:?}");
    assert!(settings.is_symlink());
    assert!(!s.root.join("gone.json").exists());
}

/// A file that won't parse still holds the user's settings; rewriting it from
/// an empty document would delete them.
#[test]
fn an_unparseable_file_is_refused_byte_for_byte() {
    let s = Sandbox::new();
    let broken = "{ \"model\": \"opus\",";
    let files = [
        s.home(".claude/settings.json"),
        s.home(".gemini/config/hooks.json"),
        s.home(".gemini/config/mcp_config.json"),
    ];
    for f in &files {
        s.put(f, broken);
    }
    let claude = install(AgentKind::Claude, &s.env).unwrap();
    let agy = install(AgentKind::Agy, &s.env).unwrap();
    for part in [&claude.hooks, &agy.hooks, &agy.mcp] {
        assert!(matches!(part, Part::Refused(_)), "{part:?}");
    }
    for f in &files {
        assert_eq!(
            std::fs::read_to_string(f).unwrap(),
            broken,
            "{}",
            f.display()
        );
    }
}

/// Removal takes out only spyc's part, and never deletes the file: it may be a
/// link a dotfiles manager keeps.
#[test]
fn remove_takes_out_only_spyc_and_keeps_the_file() {
    let s = Sandbox::new();
    let settings = s.home(".claude/settings.json");
    s.put(&settings, r#"{"model":"opus"}"#);
    install(AgentKind::Claude, &s.env).unwrap();
    install(AgentKind::Agy, &s.env).unwrap();

    let claude = remove(AgentKind::Claude, &s.env).unwrap();
    let agy = remove(AgentKind::Agy, &s.env).unwrap();
    assert_eq!(claude.hooks, Part::Absent);
    assert_eq!((&agy.hooks, &agy.mcp), (&Part::Absent, &Part::Absent));
    assert_eq!(json(&settings), json!({"model": "opus"}));
    assert_eq!(json(&s.home(".gemini/config/hooks.json")), json!({}));
    assert_eq!(
        json(&s.home(".gemini/config/mcp_config.json")),
        json!({"mcpServers": {}})
    );
}

#[test]
fn claude_mcp_is_registered_and_removed_through_its_cli() {
    let s = Sandbox::new();
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert_eq!(st.mcp, Part::Current, "{st:?}");
    assert_eq!(
        s.claude_calls(),
        [r#"mcp add-json -s user spyc {"type":"stdio","command":"spyc","args":["--mcp"]}"#]
    );
    install(AgentKind::Claude, &s.env).unwrap();
    assert_eq!(s.claude_calls().len(), 1, "a current entry is left alone");

    assert_eq!(remove(AgentKind::Claude, &s.env).unwrap().mcp, Part::Absent);
    assert_eq!(s.claude_calls()[1], "mcp remove -s user spyc");
}

#[test]
fn an_older_claude_entry_is_replaced() {
    let s = Sandbox::new();
    s.put(
        &s.home(".claude.json"),
        r#"{"numStartups":7,"mcpServers":{"spyc":{"command":"/old/bin/spyc","args":["--mcp"]}}}"#,
    );
    assert_eq!(
        status(AgentKind::Claude, &s.env).unwrap().mcp,
        Part::Outdated
    );
    assert_eq!(
        install(AgentKind::Claude, &s.env).unwrap().mcp,
        Part::Current
    );
    let calls = s.claude_calls();
    assert_eq!(calls[0], "mcp remove -s user spyc");
    assert!(
        calls[1].starts_with("mcp add-json -s user spyc "),
        "{calls:?}"
    );
}

/// An organization's `managed-mcp.json` has exclusive control of claude's MCP
/// servers: spyc never runs the CLI, and still installs the hooks.
#[test]
fn a_managed_mcp_file_leaves_the_entry_to_the_organization() {
    let s = Sandbox::new();
    let managed = s.env.managed_mcp[0].clone();
    s.put(
        &managed,
        r#"{"mcpServers":{"spyc":{"command":"spyc","args":["--mcp"]}}}"#,
    );
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert_eq!((&st.hooks, &st.mcp), (&Part::Current, &Part::Managed));

    s.put(&managed, r#"{"mcpServers":{"other":{"url":"https://x"}}}"#);
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert!(matches!(st.mcp, Part::Blocked(_)), "{st:?}");
    assert!(s.claude_calls().is_empty(), "{:?}", s.claude_calls());
}

#[test]
fn managed_settings_can_block_either_half() {
    let s = Sandbox::new();
    s.put(
        &s.env.managed_settings[0],
        r#"{"allowManagedHooksOnly":true,"deniedMcpServers":[{"serverName":"spyc"}]}"#,
    );
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert!(matches!(st.hooks, Part::Blocked(_)), "{st:?}");
    assert!(matches!(st.mcp, Part::Blocked(_)), "{st:?}");
    assert!(!s.home(".claude/settings.json").exists());
    assert!(s.claude_calls().is_empty());
}

/// An MCP server named `spyc` that runs something else isn't spyc's to replace
/// or remove.
#[test]
fn a_foreign_server_named_spyc_is_never_touched() {
    let s = Sandbox::new();
    s.put(
        &s.home(".claude.json"),
        r#"{"mcpServers":{"spyc":{"command":"/opt/other/thing","args":[]}}}"#,
    );
    let agy_mcp = s.home(".gemini/config/mcp_config.json");
    let foreign = r#"{"mcpServers":{"spyc":{"command":"/opt/other/thing"}}}"#;
    s.put(&agy_mcp, foreign);

    for st in [
        install(AgentKind::Claude, &s.env).unwrap(),
        remove(AgentKind::Claude, &s.env).unwrap(),
    ] {
        assert!(matches!(st.mcp, Part::Blocked(_)), "{st:?}");
    }
    for st in [
        install(AgentKind::Agy, &s.env).unwrap(),
        remove(AgentKind::Agy, &s.env).unwrap(),
    ] {
        assert!(matches!(st.mcp, Part::Blocked(_)), "{st:?}");
    }
    assert!(s.claude_calls().is_empty());
    assert_eq!(std::fs::read_to_string(&agy_mcp).unwrap(), foreign);
}

#[test]
fn without_the_claude_cli_the_hooks_still_install() {
    let mut s = Sandbox::new();
    s.env.claude_cli = None;
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert_eq!(st.hooks, Part::Current);
    let Part::Refused(why) = st.mcp else {
        panic!("{st:?}");
    };
    assert!(why.contains("claude mcp add-json -s user spyc"), "{why}");
}

#[test]
fn claude_config_dir_moves_both_files() {
    let mut s = Sandbox::new();
    let dir = s.root.join("claude-config");
    s.env.claude_config_dir = Some(dir.clone());
    let st = install(AgentKind::Claude, &s.env).unwrap();
    assert_eq!(st.hooks_file, dir.join("settings.json"));
    assert_eq!(st.mcp_file, dir.join(".claude.json"));
    assert_eq!((&st.hooks, &st.mcp), (&Part::Current, &Part::Current));
    assert!(!s.home(".claude").exists());
}

#[test]
fn agy_keeps_the_users_sets_and_servers() {
    let s = Sandbox::new();
    let hooks = s.home(".gemini/config/hooks.json");
    let mcp = s.home(".gemini/config/mcp_config.json");
    s.put(
        &hooks,
        r#"{"linter":{"Stop":[{"type":"command","command":"./x.sh"}]}}"#,
    );
    s.put(&mcp, r#"{"mcpServers":{"other":{"command":"other"}}}"#);
    let st = install(AgentKind::Agy, &s.env).unwrap();
    assert_eq!((&st.hooks, &st.mcp), (&Part::Current, &Part::Current));
    assert!(json(&hooks)["linter"].is_object());
    assert!(json(&hooks)["spyc-status"].is_object());
    assert_eq!(json(&mcp)["mcpServers"]["other"]["command"], "other");
    assert_eq!(json(&mcp)["mcpServers"]["spyc"]["command"], "spyc");
}

/// agy leaves its user `mcp_config.json` as an empty file.
#[test]
fn an_empty_agy_mcp_config_is_filled_in() {
    let s = Sandbox::new();
    let mcp = s.home(".gemini/config/mcp_config.json");
    s.put(&mcp, "");
    assert_eq!(install(AgentKind::Agy, &s.env).unwrap().mcp, Part::Current);
    assert_eq!(json(&mcp)["mcpServers"]["spyc"]["args"], json!(["--mcp"]));
}

#[test]
fn only_claude_and_agy_are_wired_here() {
    let s = Sandbox::new();
    for kind in [AgentKind::Codex, AgentKind::Zot, AgentKind::Other] {
        assert!(status(kind, &s.env).is_none());
        assert!(install(kind, &s.env).is_none());
    }
    assert!(!s.home(".codex").exists());
}

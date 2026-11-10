//! Which credential a Claude Code session sends its requests with, decided the
//! way Claude Code decides it, so the HUD names the saved login's
//! organization only while that login is the one in use.
//!
//! Claude Code passes over its saved `/login` for a third-party provider
//! (`CLAUDE_CODE_USE_BEDROCK` and its kin), `ANTHROPIC_AUTH_TOKEN`, a
//! `CLAUDE_CODE_OAUTH_TOKEN` (a login, though maybe another account's), an
//! `ANTHROPIC_API_KEY` the user approved at its prompt, a key file descriptor,
//! or an `apiKeyHelper` setting. It records an approval as the key's last 20
//! characters in its config file's `customApiKeyResponses`; that suffix is
//! compared here and never leaves the process. The status hook runs in
//! claude's own environment, so [`detect`] runs there and only the
//! [`Credential`] is reported.
//!
//! Not followed: an `ant` CLI profile, a host-managed provider, settings
//! passed with `--settings` or served remotely, and `managed-settings.d`
//! drop-ins. Under `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` the hook may not see
//! the credentials at all, so the answer is [`Credential::Unknown`].

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::claude_metrics::{self, account};

/// A settings or config file past this size isn't read.
const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// How much of a key Claude Code records an answer under.
const KEY_SUFFIX: usize = 20;

/// The providers that take a session off Anthropic's API, in Claude Code's
/// order.
const PROVIDERS: [(&str, Credential); 7] = [
    ("CLAUDE_CODE_USE_BEDROCK", Credential::Bedrock),
    ("CLAUDE_CODE_USE_VERTEX", Credential::Vertex),
    ("CLAUDE_CODE_USE_FOUNDRY", Credential::Foundry),
    ("CLAUDE_CODE_USE_ANTHROPIC_AWS", Credential::AnthropicAws),
    (
        "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
        Credential::AnthropicGoogleCloud,
    ),
    ("CLAUDE_CODE_USE_MANTLE", Credential::Mantle),
    ("CLAUDE_CODE_USE_GATEWAY", Credential::Gateway),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Credential {
    /// The saved `/login`, whose organization the config file names.
    Login,
    ApiKey,
    KeyHelper,
    AuthToken,
    OauthToken,
    Bedrock,
    Vertex,
    Foundry,
    AnthropicAws,
    AnthropicGoogleCloud,
    Mantle,
    Gateway,
    /// The hook's environment was scrubbed of credentials.
    Unknown,
}

impl Credential {
    /// What the HUD calls it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::ApiKey => "api key",
            Self::KeyHelper => "key helper",
            Self::AuthToken => "auth token",
            Self::OauthToken => "oauth token",
            Self::Bedrock => "bedrock",
            Self::Vertex => "vertex",
            Self::Foundry => "foundry",
            Self::AnthropicAws => "aws",
            Self::AnthropicGoogleCloud => "google cloud",
            Self::Mantle => "mantle",
            Self::Gateway => "gateway",
            Self::Unknown => "unknown",
        }
    }
}

/// Claude Code's reading of a flag: `1`, `true`, `yes` or `on`, in any case.
fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The part of a key Claude Code records an answer under.
fn key_suffix(key: &str) -> String {
    let key = key.trim();
    let skip = key.chars().count().saturating_sub(KEY_SUFFIX);
    key.chars().skip(skip).collect()
}

/// The credential in use, from the session's environment `env`, whether the
/// user approved the key whose suffix `approved` is given, and whether any
/// settings file names an `apiKeyHelper`. Each is asked only when it decides.
pub fn decide(
    env: impl Fn(&str) -> Option<String>,
    approved: impl FnOnce(&str) -> bool,
    helper: impl FnOnce() -> bool,
) -> Credential {
    let on = |name: &str| env(name).is_some_and(|value| truthy(&value));
    let set = |name: &str| env(name).is_some_and(|value| !value.trim().is_empty());
    if on("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB") {
        return Credential::Unknown;
    }
    if let Some((_, provider)) = PROVIDERS.iter().find(|(name, _)| on(name)) {
        return *provider;
    }
    if set("ANTHROPIC_AUTH_TOKEN") {
        return Credential::AuthToken;
    }
    if set("CLAUDE_CODE_OAUTH_TOKEN") || set("CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR") {
        return Credential::OauthToken;
    }
    if let Some(key) = env("ANTHROPIC_API_KEY").filter(|key| !key.trim().is_empty())
        && approved(&key_suffix(&key))
    {
        return Credential::ApiKey;
    }
    if set("CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR") {
        return Credential::ApiKey;
    }
    if helper() {
        return Credential::KeyHelper;
    }
    Credential::Login
}

/// [`decide`] for this process: the status hook, in claude's environment.
pub fn detect() -> Credential {
    let home = claude_metrics::home(
        std::env::var_os("CLAUDE_CONFIG_DIR"),
        std::env::var_os("HOME"),
    );
    let project = std::env::var_os("CLAUDE_PROJECT_DIR").map(PathBuf::from);
    decide(
        |name| std::env::var(name).ok(),
        |suffix| {
            home.as_deref()
                .is_some_and(|home| key_approved(&account::config_file(home), suffix))
        },
        || settings_files(home.as_deref(), project.as_deref()).any(|file| names_helper(&file)),
    )
}

fn read_capped(file: &Path) -> Option<Vec<u8>> {
    let mut text = Vec::new();
    std::fs::File::open(file)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_BYTES).then_some(text)
}

#[derive(Deserialize)]
struct Responses {
    #[serde(rename = "customApiKeyResponses")]
    responses: Option<Approved>,
}

#[derive(Deserialize)]
struct Approved {
    approved: Option<Vec<serde_json::Value>>,
}

/// Whether config file `file` records the key ending in `suffix` as approved.
fn key_approved(file: &Path, suffix: &str) -> bool {
    read_capped(file)
        .and_then(|text| serde_json::from_slice::<Responses>(&text).ok())
        .and_then(|config| config.responses?.approved)
        .is_some_and(|approved| approved.iter().any(|entry| entry.as_str() == Some(suffix)))
}

#[derive(Deserialize)]
struct Settings {
    #[serde(rename = "apiKeyHelper")]
    api_key_helper: Option<serde_json::Value>,
}

/// Whether settings file `file` names an `apiKeyHelper` command.
fn names_helper(file: &Path) -> bool {
    read_capped(file)
        .and_then(|text| serde_json::from_slice::<Settings>(&text).ok())
        .and_then(|settings| settings.api_key_helper)
        .is_some_and(|helper| helper.as_str().is_some_and(|cmd| !cmd.trim().is_empty()))
}

/// The settings files Claude Code reads an `apiKeyHelper` from: managed, the
/// user's in its configuration directory, and the project's shared and local.
fn settings_files(home: Option<&Path>, project: Option<&Path>) -> impl Iterator<Item = PathBuf> {
    let managed = if cfg!(target_os = "macos") {
        "/Library/Application Support/ClaudeCode/managed-settings.json"
    } else {
        "/etc/claude-code/managed-settings.json"
    };
    let project = project.filter(|dir| dir.is_absolute());
    std::iter::once(PathBuf::from(managed))
        .chain(home.map(|home| home.join("settings.json")))
        .chain(project.map(|dir| dir.join(".claude/settings.json")))
        .chain(project.map(|dir| dir.join(".claude/settings.local.json")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const KEY: &str = "sk-ant-api03-not-a-real-key-0123456789abcdefghij";

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn with(pairs: &[(&str, &str)], approved: bool, helper: bool) -> Credential {
        decide(env(pairs), |_| approved, || helper)
    }

    #[test]
    fn with_nothing_else_set_the_saved_login_is_used() {
        assert_eq!(with(&[], false, false), Credential::Login);
        assert_eq!(
            with(&[("CLAUDE_CODE_USE_BEDROCK", "0")], false, false),
            Credential::Login,
            "a flag that is off"
        );
        assert_eq!(
            with(&[("ANTHROPIC_AUTH_TOKEN", "  ")], false, false),
            Credential::Login,
            "a blank token"
        );
    }

    /// An `ANTHROPIC_API_KEY` counts once the user said yes to it at Claude
    /// Code's prompt; one declined, or not yet answered, leaves the login.
    #[test]
    fn an_api_key_wins_only_once_approved() {
        let key = [("ANTHROPIC_API_KEY", KEY)];
        assert_eq!(with(&key, true, false), Credential::ApiKey);
        assert_eq!(with(&key, false, false), Credential::Login);
        let mut asked = None;
        decide(
            env(&key),
            |suffix| {
                asked = Some(suffix.to_owned());
                false
            },
            || false,
        );
        assert_eq!(asked.as_deref(), Some("0123456789abcdefghij"));
    }

    #[test]
    fn providers_tokens_and_helpers_each_pass_over_the_login() {
        for (name, value, expected) in [
            ("CLAUDE_CODE_USE_BEDROCK", "1", Credential::Bedrock),
            ("CLAUDE_CODE_USE_VERTEX", "true", Credential::Vertex),
            ("CLAUDE_CODE_USE_FOUNDRY", "YES", Credential::Foundry),
            (
                "CLAUDE_CODE_USE_ANTHROPIC_AWS",
                "on",
                Credential::AnthropicAws,
            ),
            (
                "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
                "1",
                Credential::AnthropicGoogleCloud,
            ),
            ("CLAUDE_CODE_USE_MANTLE", "1", Credential::Mantle),
            ("CLAUDE_CODE_USE_GATEWAY", "1", Credential::Gateway),
            ("ANTHROPIC_AUTH_TOKEN", "token", Credential::AuthToken),
            ("CLAUDE_CODE_OAUTH_TOKEN", "token", Credential::OauthToken),
            (
                "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
                "3",
                Credential::OauthToken,
            ),
            (
                "CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR",
                "3",
                Credential::ApiKey,
            ),
            ("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB", "1", Credential::Unknown),
        ] {
            assert_eq!(with(&[(name, value)], false, false), expected, "{name}");
        }
        assert_eq!(with(&[], false, true), Credential::KeyHelper);
        assert_eq!(
            with(
                &[("CLAUDE_CODE_USE_VERTEX", "1"), ("ANTHROPIC_API_KEY", KEY)],
                true,
                true
            ),
            Credential::Vertex,
            "a provider outranks a key"
        );
    }

    #[test]
    fn the_wire_form_is_a_stable_name_and_an_unknown_one_is_refused() {
        let wire = serde_json::to_value(Credential::AnthropicGoogleCloud).unwrap();
        assert_eq!(wire, "anthropic-google-cloud");
        assert_eq!(
            serde_json::from_value::<Credential>(serde_json::json!("api-key")).unwrap(),
            Credential::ApiKey
        );
        assert!(serde_json::from_value::<Credential>(serde_json::json!("sk-ant-x")).is_err());
    }

    #[test]
    fn an_approval_is_read_from_the_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        assert!(!key_approved(&file, "0123456789abcdefghij"), "no file");
        std::fs::write(
            &file,
            serde_json::json!({"customApiKeyResponses":
                {"approved": ["0123456789abcdefghij"], "rejected": ["zzzzzzzzzzzzzzzzzzzz"]}})
            .to_string(),
        )
        .unwrap();
        assert!(key_approved(&file, "0123456789abcdefghij"));
        assert!(!key_approved(&file, "zzzzzzzzzzzzzzzzzzzz"), "a rejection");
        assert!(!key_approved(&file, "aaaaaaaaaaaaaaaaaaaa"), "never asked");
    }

    #[test]
    fn a_helper_is_read_from_any_settings_file() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("claude-home");
        let project = root.path().join("project");
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let files: Vec<PathBuf> = settings_files(Some(&home), Some(&project)).collect();
        assert_eq!(
            files[1..],
            [
                home.join("settings.json"),
                project.join(".claude/settings.json"),
                project.join(".claude/settings.local.json"),
            ]
        );
        let local = project.join(".claude/settings.local.json");
        std::fs::write(&local, r#"{"apiKeyHelper": ""}"#).unwrap();
        assert!(!names_helper(&local), "an empty command");
        std::fs::write(&local, r#"{"apiKeyHelper": "~/bin/key"}"#).unwrap();
        assert!(names_helper(&local));
        assert!(
            settings_files(Some(&home), Some(Path::new("relative")))
                .all(|file| !file.starts_with("relative")),
            "a relative project dir names nothing"
        );
    }
}

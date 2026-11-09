//! The organization a Claude Code configuration directory is logged into.
//!
//! It is `oauthAccount.organizationName` in Claude Code's global config file,
//! and nothing else from it is kept. The file holds every project's history
//! and Claude Code rewrites it often, so it is read whole on a worker, under a
//! cap, and only when the watch says it changed.
//!
//! Claude Code keeps that file at `$CLAUDE_CONFIG_DIR/.claude.json`, or at
//! `~/.claude.json` beside a default `~/.claude`, and reads a legacy
//! `<config dir>/.config.json` in preference to either. The home a tab
//! carries doesn't say whether `CLAUDE_CONFIG_DIR` named it, so a home called
//! `.claude` with no `.claude.json` of its own reads its parent's.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::agent::tail::watch::{self as tail, Refresh};

/// A config file past this size isn't read.
const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// A name longer than this, in characters, isn't one.
const MAX_NAME: usize = 128;

/// The file Claude Code reads its account from, for configuration directory
/// `home`. Touches the filesystem: call it on a worker.
pub fn config_file(home: &Path) -> PathBuf {
    let legacy = home.join(".config.json");
    if std::fs::metadata(&legacy).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
        return legacy;
    }
    let own = home.join(".claude.json");
    if home.file_name().is_some_and(|name| name == ".claude") && !own.is_file() {
        return home
            .parent()
            .map_or(own, |parent| parent.join(".claude.json"));
    }
    own
}

#[derive(Deserialize)]
struct Global {
    #[serde(rename = "oauthAccount")]
    oauth_account: Option<OAuthAccount>,
}

#[derive(Deserialize)]
struct OAuthAccount {
    #[serde(rename = "organizationName")]
    organization_name: Option<String>,
}

/// The organization `file` names, and the bytes read for it.
///
/// None for a missing, oversized or malformed file, an API-key setup with no
/// `oauthAccount`, or a name that is empty, too long, or holds a control
/// character.
pub fn organization(file: &Path) -> (Option<String>, u64) {
    let mut text = Vec::new();
    let read = std::fs::File::open(file)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut text))
        .is_ok();
    let bytes = text.len() as u64;
    if !read || bytes > MAX_BYTES {
        return (None, bytes);
    }
    let name = serde_json::from_slice::<Global>(&text)
        .ok()
        .and_then(|global| global.oauth_account?.organization_name)
        .filter(|name| {
            !name.trim().is_empty()
                && name.chars().count() <= MAX_NAME
                && !name.chars().any(char::is_control)
        });
    (name, bytes)
}

struct Reader {
    file: PathBuf,
    bytes_read: u64,
    refreshes: u64,
}

impl Refresh for Reader {
    type Update = Option<String>;

    fn refresh(&mut self) -> Option<String> {
        let (name, bytes) = organization(&self.file);
        self.bytes_read = self.bytes_read.saturating_add(bytes);
        self.refreshes = self.refreshes.saturating_add(1);
        name
    }

    fn watch_failed(&self) -> Option<String> {
        None
    }

    fn work(&self) -> (u64, u64) {
        (self.bytes_read, self.refreshes)
    }
}

/// The organization of one configuration directory, read again each time its
/// config file changes, a replacement included.
pub struct Subscription(tail::Subscription<Option<String>>);

impl Subscription {
    /// Start on a worker: [`config_file`] reads the filesystem.
    pub fn start(home: &Path, wake: impl Fn() + Send + 'static) -> Self {
        let file = config_file(home);
        let reader = Reader {
            file: file.clone(),
            bytes_read: 0,
            refreshes: 0,
        };
        Self(tail::Subscription::start(file, reader, wake))
    }

    /// The organization, when it has changed since the last take: `Some(None)`
    /// once it names none.
    pub fn take(&self) -> Option<Option<String>> {
        self.0.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;
    use std::time::Duration;

    fn account(org: &str) -> String {
        json!({"numStartups": 3, "projects": {"/somewhere": {"history": []}},
            "oauthAccount": {"emailAddress": "someone@example.com",
                "organizationName": org, "organizationRole": "user"}})
        .to_string()
    }

    #[test]
    fn the_organization_is_read_and_nothing_else_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        std::fs::write(&file, account("Acme Corp")).unwrap();
        assert_eq!(organization(&file).0.as_deref(), Some("Acme Corp"));
    }

    #[test]
    fn no_login_a_bad_file_or_a_bad_name_names_no_organization() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        assert_eq!(organization(&file).0, None, "missing");
        for text in [
            json!({"primaryApiKey": "sk-not-read"}).to_string(),
            json!({"oauthAccount": {"emailAddress": "someone@example.com"}}).to_string(),
            "{\"oauthAccount\": {\"organizationName\": \"Acme".to_owned(),
            account(""),
            account("   "),
            account("Acme\u{1b}[2J"),
            account(&"a".repeat(MAX_NAME + 1)),
        ] {
            std::fs::write(&file, &text).unwrap();
            assert_eq!(organization(&file).0, None, "{text}");
        }
        std::fs::write(&file, account(&"a".repeat(MAX_NAME))).unwrap();
        assert!(organization(&file).0.is_some(), "the longest name is kept");
    }

    #[test]
    fn a_file_past_the_cap_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        let padding = "x".repeat(usize::try_from(MAX_BYTES).unwrap());
        let text = json!({"oauthAccount": {"organizationName": "Acme"}, "pad": padding});
        std::fs::write(&file, text.to_string()).unwrap();
        assert_eq!(organization(&file).0, None);
    }

    /// Where Claude Code looks: a set `CLAUDE_CONFIG_DIR` holds its own file;
    /// the default `~/.claude` reads `~/.claude.json`; a legacy `.config.json`
    /// wins over both.
    #[test]
    fn the_config_file_is_where_claude_code_reads_it() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile-work");
        std::fs::create_dir(&profile).unwrap();
        assert_eq!(config_file(&profile), profile.join(".claude.json"));

        let default = root.path().join(".claude");
        std::fs::create_dir(&default).unwrap();
        assert_eq!(config_file(&default), root.path().join(".claude.json"));
        std::fs::write(default.join(".claude.json"), "{}").unwrap();
        assert_eq!(
            config_file(&default),
            default.join(".claude.json"),
            "CLAUDE_CONFIG_DIR=~/.claude keeps its file inside"
        );

        std::fs::write(profile.join(".config.json"), "").unwrap();
        assert_eq!(
            config_file(&profile),
            profile.join(".claude.json"),
            "an empty legacy file is passed over"
        );
        std::fs::write(profile.join(".config.json"), "{}").unwrap();
        assert_eq!(config_file(&profile), profile.join(".config.json"));
    }

    fn next(sub: &Subscription, woke: &mpsc::Receiver<()>) -> Option<String> {
        loop {
            woke.recv_timeout(Duration::from_secs(10))
                .expect("the subscription never woke");
            if let Some(update) = sub.take() {
                return update;
            }
        }
    }

    /// A `/login` to another organization replaces the file; a logout drops
    /// the account. Each is seen without being asked.
    #[test]
    fn a_new_login_or_a_logout_is_followed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        std::fs::write(&file, account("Acme Corp")).unwrap();
        let (tx, woke) = mpsc::channel();
        let sub = Subscription::start(dir.path(), move || {
            let _ = tx.send(());
        });
        assert_eq!(next(&sub, &woke).as_deref(), Some("Acme Corp"));

        let replacement = dir.path().join(".claude.json.tmp");
        std::fs::write(&replacement, account("Beta Labs")).unwrap();
        std::fs::rename(&replacement, &file).unwrap();
        assert_eq!(next(&sub, &woke).as_deref(), Some("Beta Labs"));

        std::fs::write(&file, json!({"numStartups": 4}).to_string()).unwrap();
        assert_eq!(next(&sub, &woke), None);
    }
}

//! The subagents one Claude Code session runs, from `<session>/subagents/`
//! beside its transcript.
//!
//! Each one's `agent-<id>.meta.json` names its type, the `Agent` call that
//! started it and whether it has finished, and its `agent-<id>.jsonl` holds its
//! own records, which say the tool it runs now. Nothing else is kept: not a
//! metadata file's `description`, nor any record's prompt, input or result.
//!
//! A subagent is followed while it runs and for [`ENDED_SHOWN`] after; one that
//! ended before that is read once and never again.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use jiff::Timestamp;
use serde::Deserialize;

use crate::agent::tail::watch::{self as tail, Refresh, Watched};
use crate::agent::tail::{Follow, Tail, reader::unavailable};

use super::{Availability, records};

/// How long a subagent that has finished stays listed.
pub const ENDED_SHOWN: Duration = Duration::from_secs(60);
/// The most entries of the directory one read looks at: a long session can
/// have run hundreds of subagents.
const MAX_ENTRIES: usize = 2048;
/// A metadata file is a few hundred bytes; a larger one isn't one.
const MAX_META_BYTES: u64 = 64 * 1024;
/// The most running subagents whose files are watched. Past it, a change to
/// any watched path still rereads them all.
const MAX_WATCHED: usize = 8;
/// The most calls without a result one subagent is followed with.
const MAX_PENDING: usize = 64;
/// The longest type kept: `general-purpose`, a custom agent's name.
const MAX_TYPE: usize = 64;

/// One subagent, as the HUD shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subagent {
    /// Its `agentType`: `Explore`, `general-purpose`.
    pub agent_type: Option<String>,
    /// The main thread's `Agent` call that started it.
    pub call: Option<String>,
    /// The tool it runs now: its latest call without a result.
    pub tool: Option<String>,
    /// When it finished, dated by its metadata's last write.
    pub ended: Option<Timestamp>,
}

/// The three keys of a metadata file that are read. serde skips every other
/// one, `description` among them, without keeping it.
#[derive(Deserialize)]
struct Meta {
    #[serde(rename = "agentType")]
    agent_type: Option<serde_json::Value>,
    #[serde(rename = "toolUseId")]
    tool_use_id: Option<serde_json::Value>,
    finished: Option<serde_json::Value>,
}

/// One subagent's records: the calls it made that haven't returned, newest last.
struct Sidechain {
    /// `<claude_home>/projects`: nothing outside it is read.
    root: PathBuf,
    pending: Vec<(String, Option<String>)>,
}

impl Follow for Sidechain {
    fn admits(&self, path: &Path) -> Result<(), Availability> {
        let root = self.root.canonicalize().map_err(unavailable)?;
        let path = path.canonicalize().map_err(unavailable)?;
        if path.starts_with(root) {
            Ok(())
        } else {
            Err(Availability::IdentityMismatch)
        }
    }

    fn verify(&mut self, _file: &mut std::fs::File) -> Result<u64, Availability> {
        Ok(0)
    }

    fn line(&mut self, line: &[u8]) {
        let Ok(value) = serde_json::from_slice(line) else {
            return;
        };
        match records::decode_sidechain(&value) {
            Some(records::Event::Assistant { calls, .. }) => {
                for call in calls {
                    self.pending.retain(|(id, _)| *id != call.id);
                    self.pending.push((call.id, call.name));
                }
                let over = self.pending.len().saturating_sub(MAX_PENDING);
                self.pending.drain(..over);
            }
            Some(records::Event::Results { results, .. }) => {
                self.pending
                    .retain(|(id, _)| !results.iter().any(|(done, _)| done == id));
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.pending.clear();
    }
}

struct Known {
    /// Its metadata's first write seen, which orders the list.
    started: SystemTime,
    meta_modified: SystemTime,
    agent_type: Option<String>,
    call: Option<String>,
    ended: Option<SystemTime>,
    /// Its records, read while it runs and for [`ENDED_SHOWN`] after.
    follow: Option<Tail<Sidechain>>,
}

impl Known {
    /// Ended long enough ago that nothing about it changes what is shown.
    fn settled(&self, now: SystemTime) -> bool {
        self.ended
            .is_some_and(|end| now.duration_since(end).unwrap_or_default() > ENDED_SHOWN)
    }
}

pub struct Reader {
    /// `<claude_home>/projects`.
    root: PathBuf,
    /// `<claude_home>/projects/<project>/<session>`: the transcript's path
    /// without its `.jsonl`.
    session: PathBuf,
    known: BTreeMap<String, Known>,
    /// Diagnostic work counters; they do not cause changed-metric notifications.
    pub bytes_read: u64,
    pub refreshes: u64,
}

impl Reader {
    pub fn new(claude_home: &Path, transcript: &Path) -> Self {
        Self {
            root: claude_home.join("projects"),
            session: transcript.with_extension(""),
            known: BTreeMap::new(),
            bytes_read: 0,
            refreshes: 0,
        }
    }

    fn dir(&self) -> PathBuf {
        self.session.join("subagents")
    }

    /// The subagents running, and those that finished within [`ENDED_SHOWN`],
    /// in the order they started.
    pub fn refresh(&mut self) -> Vec<Subagent> {
        self.refreshes = self.refreshes.saturating_add(1);
        let now = SystemTime::now();
        self.scan(now);
        let mut read = 0;
        for known in self.known.values_mut() {
            if known.settled(now) {
                known.follow = None;
            } else if let Some(tail) = known.follow.as_mut() {
                tail.refresh();
                read += tail.bytes_read;
            }
        }
        self.bytes_read = read;
        let mut listed: Vec<&Known> = self.known.values().filter(|k| !k.settled(now)).collect();
        listed.sort_by_key(|known| known.started);
        listed
            .into_iter()
            .map(|known| Subagent {
                agent_type: known.agent_type.clone(),
                call: known.call.clone(),
                tool: known
                    .follow
                    .as_ref()
                    .and_then(|tail| tail.follow.pending.last())
                    .and_then(|(_, name)| name.clone()),
                ended: known.ended.and_then(|end| Timestamp::try_from(end).ok()),
            })
            .collect()
    }

    /// Read the metadata of each subagent new since the last read, or whose
    /// metadata changed, unless it ended long ago.
    fn scan(&mut self, now: SystemTime) {
        let dir = self.dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.take(MAX_ENTRIES).flatten() {
            let name = entry.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|name| name.strip_prefix("agent-")?.strip_suffix(".meta.json"))
                .filter(|id| is_id(id))
            else {
                continue;
            };
            if self.known.get(id).is_some_and(|known| known.settled(now)) {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
                continue;
            };
            if self
                .known
                .get(id)
                .is_some_and(|known| known.meta_modified == modified)
            {
                continue;
            }
            let Some(meta) = read_meta(&entry.path()) else {
                continue;
            };
            let known = self.known.entry(id.to_owned()).or_insert_with(|| Known {
                started: modified,
                meta_modified: modified,
                agent_type: None,
                call: None,
                ended: None,
                follow: None,
            });
            known.meta_modified = modified;
            known.agent_type = meta.agent_type.as_ref().and_then(agent_type);
            known.call = meta
                .tool_use_id
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .filter(|call| is_id(call))
                .map(str::to_owned);
            known.ended =
                (meta.finished == Some(serde_json::Value::Bool(true))).then_some(modified);
            if known.follow.is_none() && !known.settled(now) {
                let sidechain = Sidechain {
                    root: self.root.clone(),
                    pending: Vec::new(),
                };
                known.follow = Some(Tail::new(dir.join(format!("agent-{id}.jsonl")), sidechain));
            }
        }
    }
}

/// A metadata file's three keys, if it is one.
fn read_meta(path: &Path) -> Option<Meta> {
    let mut text = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_META_BYTES + 1)
        .read_to_end(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_META_BYTES {
        return None;
    }
    serde_json::from_slice(&text).ok()
}

/// A subagent's type, if it reads as one: a short name, no control characters.
fn agent_type(value: &serde_json::Value) -> Option<String> {
    let text = value.as_str()?;
    (!text.is_empty()
        && text.chars().count() <= MAX_TYPE
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.' | ' ')))
    .then(|| text.to_owned())
}

/// An agent's or a call's id: ASCII letters, digits, `-` and `_`.
fn is_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

impl Refresh for Reader {
    type Update = Vec<Subagent>;

    fn refresh(&mut self) -> Vec<Subagent> {
        Self::refresh(self)
    }

    fn watch_failed(&self) -> Vec<Subagent> {
        Vec::new()
    }

    fn work(&self) -> (u64, u64) {
        (self.bytes_read, self.refreshes)
    }
}

impl Watched for Reader {
    /// The directories a subagent's files appear in, each created as the
    /// session's first subagent starts, and the records and metadata of the
    /// newest running ones.
    fn watch_paths(&self) -> Vec<PathBuf> {
        let dir = self.dir();
        let mut paths: Vec<PathBuf> = [self.session.parent(), Some(&self.session), Some(&dir)]
            .into_iter()
            .flatten()
            .map(Path::to_path_buf)
            .collect();
        let mut running: Vec<(&String, &Known)> = self
            .known
            .iter()
            .filter(|(_, known)| known.ended.is_none())
            .collect();
        running.sort_by_key(|(_, known)| std::cmp::Reverse(known.started));
        for (id, _) in running.into_iter().take(MAX_WATCHED) {
            paths.push(dir.join(format!("agent-{id}.jsonl")));
            paths.push(dir.join(format!("agent-{id}.meta.json")));
        }
        paths
    }
}

/// A session's subagents, followed on a worker: an explicit consumer owns it,
/// and nothing runs at idle.
pub struct Subscription(tail::Subscription<Vec<Subagent>>);

impl Subscription {
    /// Follow the subagents of the session whose transcript is `transcript`,
    /// read from `claude_home`.
    pub fn start(claude_home: &Path, transcript: &Path, wake: impl Fn() + Send + 'static) -> Self {
        Self(tail::Subscription::start_set(
            Reader::new(claude_home, transcript),
            wake,
        ))
    }

    pub fn take(&self) -> Option<Vec<Subagent>> {
        self.0.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    /// A transcript under `home`, and its session's `subagents/` directory.
    fn session(home: &Path) -> (PathBuf, PathBuf) {
        let project = home.join("projects/-work");
        let dir = project.join("session-1/subagents");
        std::fs::create_dir_all(&dir).unwrap();
        let transcript = project.join("session-1.jsonl");
        std::fs::write(&transcript, "").unwrap();
        (transcript, dir)
    }

    fn meta(dir: &Path, id: &str, kind: &str, finished: bool) {
        let mut meta = json!({"agentType": kind, "toolUseId": format!("toolu_{id}"),
            "description": "private-description"});
        if finished {
            meta["finished"] = json!(true);
        }
        std::fs::write(dir.join(format!("agent-{id}.meta.json")), meta.to_string()).unwrap();
    }

    fn record(dir: &Path, id: &str, record: &serde_json::Value) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("agent-{id}.jsonl")))
            .unwrap();
        writeln!(file, "{record}").unwrap();
    }

    fn call(dir: &Path, id: &str, call: &str, tool: &str) {
        record(
            dir,
            id,
            &json!({"type": "assistant", "isSidechain": true, "timestamp": "2026-11-07T12:00:00Z",
                "message": {"id": format!("msg-{call}"), "content": [{"type": "tool_use",
                    "id": call, "name": tool, "input": {"command": "private-input"}}]}}),
        );
    }

    fn result(dir: &Path, id: &str, call: &str) {
        record(
            dir,
            id,
            &json!({"type": "user", "isSidechain": true, "timestamp": "2026-11-07T12:00:01Z",
                "message": {"content": [{"type": "tool_result", "tool_use_id": call,
                    "content": "private-output"}]}}),
        );
    }

    fn age(path: &Path, seconds: u64) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(seconds))
            .unwrap();
    }

    /// The tool a subagent runs now is its latest call without a result:
    /// once that returns, the one before it if it hasn't, else none.
    #[test]
    fn the_tool_it_runs_now_is_its_latest_call_without_a_result() {
        let tmp = tempfile::tempdir().unwrap();
        let (transcript, dir) = session(tmp.path());
        meta(&dir, "a1", "Explore", false);
        call(&dir, "a1", "g1", "Grep");
        call(&dir, "a1", "r1", "Read");
        let mut reader = Reader::new(tmp.path(), &transcript);
        assert_eq!(reader.refresh()[0].tool.as_deref(), Some("Read"));
        result(&dir, "a1", "r1");
        assert_eq!(reader.refresh()[0].tool.as_deref(), Some("Grep"));
        result(&dir, "a1", "g1");
        let listed = reader.refresh();
        assert_eq!(listed[0].tool, None);
        assert_eq!(listed[0].agent_type.as_deref(), Some("Explore"));
        assert_eq!(listed[0].call.as_deref(), Some("toolu_a1"));
        assert!(!format!("{listed:?}").contains("private"), "{listed:?}");
    }

    /// One that finished over a minute ago is not listed, its records are no
    /// longer followed, and its metadata is not read again.
    #[test]
    fn one_that_ended_a_minute_ago_is_read_once_and_left_out() {
        let tmp = tempfile::tempdir().unwrap();
        let (transcript, dir) = session(tmp.path());
        meta(&dir, "a1", "Explore", true);
        age(&dir.join("agent-a1.meta.json"), 120);
        meta(&dir, "a2", "Plan", true);
        let mut reader = Reader::new(tmp.path(), &transcript);
        let listed = reader.refresh();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].agent_type.as_deref(), Some("Plan"));
        assert!(listed[0].ended.is_some());
        assert!(reader.known["a1"].follow.is_none());

        // Rewritten as running, it would be listed if it were read again.
        meta(&dir, "a1", "Explore", false);
        assert!(
            reader
                .refresh()
                .iter()
                .all(|agent| agent.agent_type.as_deref() != Some("Explore"))
        );
    }

    /// What is watched: the directories each subagent's files appear in, and
    /// the running subagents' own files, not a finished one's.
    #[test]
    fn it_watches_the_directories_and_the_running_subagents_files() {
        let tmp = tempfile::tempdir().unwrap();
        let (transcript, dir) = session(tmp.path());
        meta(&dir, "a1", "Explore", false);
        meta(&dir, "a2", "Plan", true);
        let mut reader = Reader::new(tmp.path(), &transcript);
        reader.refresh();
        let paths = reader.watch_paths();
        let session = transcript.with_extension("");
        for expected in [
            session.parent().unwrap().to_path_buf(),
            session.clone(),
            dir.clone(),
            dir.join("agent-a1.jsonl"),
            dir.join("agent-a1.meta.json"),
        ] {
            assert!(paths.contains(&expected), "{expected:?} in {paths:?}");
        }
        assert!(
            !paths
                .iter()
                .any(|path| path.to_string_lossy().contains("agent-a2")),
            "{paths:?}"
        );
    }

    /// A metadata file too large to be one, or whose keys aren't what one
    /// holds, lists nothing.
    #[test]
    fn a_metadata_file_that_isnt_one_lists_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (transcript, dir) = session(tmp.path());
        // Valid, and one byte past the limit: read whole, it would parse.
        let limit = usize::try_from(MAX_META_BYTES).unwrap();
        let shell = json!({"agentType": "Explore", "pad": ""}).to_string();
        let padded = json!({"agentType": "Explore", "pad": "x".repeat(limit + 1 - shell.len())});
        assert_eq!(padded.to_string().len(), limit + 1);
        std::fs::write(dir.join("agent-a1.meta.json"), padded.to_string()).unwrap();
        std::fs::write(dir.join("agent-a2.meta.json"), "not json").unwrap();
        std::fs::write(
            dir.join("agent-a3.meta.json"),
            json!({"agentType": "bad\u{1b}[31mtype", "toolUseId": "has space"}).to_string(),
        )
        .unwrap();
        let mut reader = Reader::new(tmp.path(), &transcript);
        let listed = reader.refresh();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].agent_type, None);
        assert_eq!(listed[0].call, None);
    }
}

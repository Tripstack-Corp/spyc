//! The recap of a tab whose agent finished a turn while it was out of view
//! (#919), flashed as the tab comes back into view:
//! `1 turn · 3 tools (Edit 2, Bash 1) · 1 file changed · done 4m ago`.
//!
//! `ActivityScan::settle_attention` keeps the tab's [`Away`](crate::pane::tabs::Away)
//! record and raises `Effect::AwayRecap` as an unseen finish is seen. The
//! executor gathers where the tab works and spawns the worker. That reads the
//! tool calls since the tab was left from a Claude tab's transcript, and the
//! files its worktree changed since then. The pre-recv drain flashes the line
//! if that tab is still the one in view. It names tools, never what they acted
//! on, and quotes no agent text.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Instant, SystemTime};

use crate::agent::claude_metrics::records;
use crate::state::sessions::AgentKind;

use super::{App, Wake};

/// The first read of a transcript's end, doubled until it reaches back to
/// when the tab was left or reaches [`MAX_READ`].
const FIRST_READ: u64 = 1024 * 1024;
const MAX_READ: u64 = 32 * 1024 * 1024;
/// The most tools a recap names; the rest count in its total.
const NAMED_TOOLS: usize = 2;

/// A tab seen again after its agent finished `turns` turns out of view, from
/// when it left to the last finish: what `Effect::AwayRecap` carries.
#[derive(Debug, Clone)]
pub struct Absence {
    pub pane_id: String,
    pub turns: u32,
    pub left: Instant,
    pub done: Instant,
}

/// What the worker needs, gathered on the loop.
struct Job {
    pane_id: String,
    turns: u32,
    left: SystemTime,
    done: SystemTime,
    cwd: PathBuf,
    /// A Claude tab's home and session, whose transcript says its tool calls.
    transcript: Option<(PathBuf, String)>,
}

/// A recap for the tab `pane_id`, ready to flash.
pub(super) struct Recap {
    pane_id: String,
    line: String,
}

impl App {
    /// `Effect::AwayRecap`'s executor half: the tab's cwd and transcript, then
    /// a detached worker that lands its recap on `runtime.away_recaps`.
    pub(super) fn start_away_recap(&self, absence: Absence) {
        let Absence {
            pane_id,
            turns,
            left,
            done,
        } = absence;
        let Some((_, entry)) = self.runtime.tab_of_pane(&pane_id) else {
            return;
        };
        let info = &entry.info;
        let transcript = (info.agent().kind() == AgentKind::Claude)
            .then(|| {
                let home = info.claude_home.clone().or_else(|| {
                    self.state
                        .active_project()
                        .pane
                        .agent_hud
                        .claude_home
                        .clone()
                })?;
                Some((home, info.live_session_id.clone()?))
            })
            .flatten();
        let now = SystemTime::now();
        let at = |instant: Instant| now.checked_sub(instant.elapsed()).unwrap_or(now);
        let job = Job {
            pane_id,
            turns,
            left: at(left),
            done: at(done),
            cwd: entry.live_cwd(),
            transcript,
        };
        let results = self
            .runtime
            .away_recaps
            .sender(self.runtime.pane_wake_tx.clone(), Wake::AwayRecap);
        std::thread::spawn(move || {
            let _ = results.send(job.run());
        });
    }

    /// Pre-recv drain: flash each landed recap whose tab is still the one in
    /// view. Whether one was flashed.
    pub(super) fn apply_away_recaps(&mut self) -> bool {
        let mut shown = false;
        for recap in self.runtime.away_recaps.drain() {
            let in_view = self.in_view_tab().and_then(|index| {
                let tabs = self.runtime.project().pane_tabs.as_ref()?;
                tabs.tabs().get(index).map(|entry| entry.info.id.clone())
            });
            if in_view.as_deref() == Some(recap.pane_id.as_str()) {
                self.state.flash_info(recap.line);
                shown = true;
            }
        }
        shown
    }
}

impl Job {
    fn run(self) -> Recap {
        let tools = self
            .transcript
            .as_ref()
            .and_then(|(home, session)| tools_since(home, session, self.left));
        let files = files_changed_since(&self.cwd, self.left);
        let ago = SystemTime::now()
            .duration_since(self.done)
            .unwrap_or_default()
            .as_secs();
        Recap {
            pane_id: self.pane_id,
            line: line(self.turns, tools.as_ref(), files, ago),
        }
    }
}

/// How many times each tool was called since `since`, from the end of the
/// session's transcript, and whether that end reached back so far.
struct Tools {
    counts: Vec<(String, u32)>,
    complete: bool,
}

/// The tool calls in `session`'s transcript under `home` made since `since`,
/// by name, most called first. Reads the transcript's end, longer each time,
/// until it reaches back past `since`.
fn tools_since(home: &Path, session: &str, since: SystemTime) -> Option<Tools> {
    let path =
        super::agent_hud::collector::find_transcript(home, session, &AtomicBool::new(false))?;
    let since = jiff::Timestamp::try_from(since).ok()?;
    let mut file = std::fs::File::open(&path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut read = FIRST_READ.min(len);
    loop {
        file.seek(SeekFrom::Start(len - read)).ok()?;
        let mut bytes = Vec::new();
        (&mut file).take(read).read_to_end(&mut bytes).ok()?;
        let (counts, earliest) = count_calls(&bytes, read < len, session, since);
        let reached = read == len || earliest.is_some_and(|at| at < since);
        if reached || read >= MAX_READ {
            return Some(Tools {
                counts,
                complete: reached,
            });
        }
        read = (read * 2).min(len).min(MAX_READ);
    }
}

/// The calls made since `since` in `bytes`, a transcript's end, by name, and
/// the earliest record time seen. A read that starts mid-file skips its first,
/// partial line.
fn count_calls(
    bytes: &[u8],
    partial_first: bool,
    session: &str,
    since: jiff::Timestamp,
) -> (Vec<(String, u32)>, Option<jiff::Timestamp>) {
    let mut seen = HashSet::new();
    let mut counts: HashMap<String, u32> = HashMap::new();
    let mut earliest = None;
    let lines = bytes
        .split(|byte| *byte == b'\n')
        .skip(usize::from(partial_first));
    for line in lines {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let Some(records::Event::Assistant { at, calls, .. }) = records::decode(&value, session)
        else {
            continue;
        };
        earliest = Some(earliest.map_or(at, |e: jiff::Timestamp| e.min(at)));
        if at < since {
            continue;
        }
        for call in calls {
            if seen.insert(call.id) {
                let name = call.name.unwrap_or_else(|| "?".to_owned());
                *counts.entry(name).or_default() += 1;
            }
        }
    }
    let mut counts: Vec<(String, u32)> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    (counts, earliest)
}

/// How many files the worktree `cwd` is in has changed since `since`: those
/// git calls changed whose last write is since then. `None` outside a
/// repository.
fn files_changed_since(cwd: &Path, since: SystemTime) -> Option<usize> {
    let root = super::state::find_repo_root(cwd)?;
    let entries = crate::git::status::repo_status(&root)?;
    Some(
        entries
            .iter()
            .filter(|entry| {
                std::fs::symlink_metadata(root.join(&entry.rela_path))
                    .and_then(|meta| meta.modified())
                    .is_ok_and(|modified| modified >= since)
            })
            .count(),
    )
}

/// The recap's line, each part only when it says something:
/// `2 turns · 14 tools (Edit 6, Bash 5) · 6 files changed · done 12m ago`.
fn line(turns: u32, tools: Option<&Tools>, files: Option<usize>, ago_secs: u64) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut parts = vec![plural(turns as usize, "turn", "turns")];
    if let Some(tools) = tools.filter(|t| !t.counts.is_empty()) {
        let total: u32 = tools.counts.iter().map(|(_, n)| n).sum();
        let named: Vec<String> = tools
            .counts
            .iter()
            .take(NAMED_TOOLS)
            .map(|(name, n)| format!("{name} {n}"))
            .collect();
        let at_least = if tools.complete { "" } else { "≥" };
        parts.push(format!(
            "{at_least}{} ({})",
            plural(total as usize, "tool", "tools"),
            named.join(", ")
        ));
    }
    if let Some(files) = files.filter(|n| *n > 0) {
        parts.push(format!("{} changed", plural(files, "file", "files")));
    }
    parts.push(format!("done {} ago", short_ago(ago_secs)));
    parts.join(" · ")
}

/// `45s`, `12m`, `3h`, `2d`.
fn short_ago(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(counts: &[(&str, u32)], complete: bool) -> Tools {
        Tools {
            counts: counts.iter().map(|(n, c)| ((*n).to_owned(), *c)).collect(),
            complete,
        }
    }

    /// Each part says something or is left out, and the tools name the two
    /// most called; a transcript read that didn't reach back says `≥`.
    #[test]
    fn the_recap_says_only_what_it_knows() {
        assert_eq!(line(1, None, None, 5), "1 turn · done 5s ago");
        assert_eq!(
            line(
                3,
                Some(&tools(&[("Edit", 6), ("Bash", 5), ("Read", 3)], true)),
                Some(6),
                720
            ),
            "3 turns · 14 tools (Edit 6, Bash 5) · 6 files changed · done 12m ago"
        );
        assert_eq!(
            line(1, Some(&tools(&[("Edit", 1)], false)), Some(0), 7200),
            "1 turn · ≥1 tool (Edit 1) · done 2h ago"
        );
        assert_eq!(
            line(2, Some(&tools(&[], true)), Some(1), 90_000),
            "2 turns · 1 file changed · done 1d ago"
        );
    }

    /// Calls count once by id, from the time asked for on, and a read that
    /// starts mid-file skips its first line.
    #[test]
    fn calls_count_once_from_when_the_tab_was_left() {
        let session = "c1a0de00-0000-4000-8000-000000000001";
        let record = |id: &str, tool: &str, at: &str| {
            serde_json::json!({"type": "assistant", "sessionId": session, "timestamp": at,
                "message": {"id": format!("m-{id}"), "content": [
                    {"type": "tool_use", "id": id, "name": tool, "input": {}}]}})
            .to_string()
        };
        let text = [
            "{\"partial".to_owned(),
            record("a", "Read", "2026-11-07T10:00:00Z"),
            record("b", "Edit", "2026-11-07T11:00:00Z"),
            record("b", "Edit", "2026-11-07T11:00:00Z"),
            record("c", "Bash", "2026-11-07T11:01:00Z"),
        ]
        .join("\n");
        let since: jiff::Timestamp = "2026-11-07T10:30:00Z".parse().unwrap();
        let (counts, earliest) = count_calls(text.as_bytes(), true, session, since);
        assert_eq!(counts, [("Bash".to_owned(), 1), ("Edit".to_owned(), 1)]);
        assert_eq!(earliest, Some("2026-11-07T10:00:00Z".parse().unwrap()));
    }
}

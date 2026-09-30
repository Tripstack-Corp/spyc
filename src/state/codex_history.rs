//! The history a forked codex thread inherited.
//!
//! `codex fork` doesn't copy the parent's turns into the new rollout. The
//! fork's `session_meta` carries `history_base`: the parent thread's id and the
//! byte offset in the parent's rollout where the fork branched (codex 0.158).
//! A transcript read from the fork's own file alone starts at the fork, which
//! is the history the user forked to keep.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// How many forks back [`read_with_history`] follows. Deeper than this is a
/// cycle codex didn't write; the turns found by then still show.
const MAX_FORK_DEPTH: usize = 32;

/// Read `path`'s rollout text, preceded by each ancestor's up to where its
/// child forked, oldest first. `budget` caps the whole chain and is spent from
/// the newest end, so a fork shows the same tail an unforked rollout would.
pub fn read_with_history(path: &Path, budget: u64) -> std::io::Result<String> {
    let own = crate::state::read_tail_lossy(path, budget)?;
    let mut left = budget.saturating_sub(own.len() as u64);
    let mut chunks = vec![own];
    let mut seen = HashSet::from([path.to_path_buf()]);
    let mut child = path.to_path_buf();
    while left > 0 && seen.len() <= MAX_FORK_DEPTH {
        let Some((parent, end)) = fork_base(&child) else {
            break;
        };
        if !seen.insert(parent.clone()) {
            break;
        }
        let Ok(text) = crate::state::read_prefix_tail_lossy(&parent, end, left) else {
            break;
        };
        left = left.saturating_sub(text.len() as u64);
        chunks.push(text);
        child = parent;
    }
    let mut out = String::new();
    for chunk in chunks.iter().rev() {
        out.push_str(chunk);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    }
    Ok(out)
}

/// The rollout `path` forked from, and the byte offset in it where the fork
/// branched, from `path`'s `session_meta.history_base`.
fn fork_base(path: &Path) -> Option<(PathBuf, u64)> {
    use std::io::BufRead;
    let mut first = String::new();
    std::io::BufReader::new(std::fs::File::open(path).ok()?)
        .read_line(&mut first)
        .ok()?;
    let meta: serde_json::Value = serde_json::from_str(first.trim()).ok()?;
    let base = &meta["payload"]["history_base"];
    let thread = base["thread_id"].as_str()?;
    let end = base["end_byte_offset"].as_u64()?;
    // Rollouts live at `<sessions>/YYYY/MM/DD/rollout-*.jsonl`.
    let sessions = path.ancestors().nth(4)?;
    let parent = super::codex_transcript::find_rollout_by_uuid(sessions, thread)?;
    Some((parent, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARENT: &str = "01a0f326-77c0-7821-9646-973bfc172364";
    const FORK: &str = "01a0f367-35cc-7ad0-bef6-b4d87f0b127f";
    const GRANDCHILD: &str = "01a0f400-0000-7000-8000-000000000001";

    fn meta(id: &str, base: Option<(&str, usize)>) -> String {
        let base = base.map_or(String::new(), |(thread, end)| {
            format!(r#","history_base":{{"thread_id":"{thread}","end_byte_offset":{end}}}"#)
        });
        format!(
            r#"{{"type":"session_meta","payload":{{"id":"{id}","timestamp":"2026-09-30T17:40:22.641Z","cwd":"/tmp"{base}}}}}"#
        ) + "\n"
    }

    fn user(text: &str) -> String {
        format!(r#"{{"type":"event_msg","payload":{{"type":"user_message","message":"{text}"}}}}"#)
            + "\n"
    }

    /// Write a rollout where codex keeps it: `<sessions>/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
    fn rollout(sessions: &Path, uuid: &str, body: &str) -> PathBuf {
        let day = sessions.join("2026/09/30");
        std::fs::create_dir_all(&day).expect("create day dir");
        let path = day.join(format!("rollout-2026-09-30T13-40-22-{uuid}.jsonl"));
        std::fs::write(&path, body).expect("write rollout");
        path
    }

    /// The user prompts in `text`, in order.
    fn prompts(text: &str) -> Vec<String> {
        text.lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["payload"]["type"] == "user_message")
            .filter_map(|v| v["payload"]["message"].as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn a_fork_reads_its_parent_up_to_the_branch_point_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let before = meta(PARENT, None) + &user("inherited one") + &user("inherited two");
        rollout(
            tmp.path(),
            PARENT,
            &(before.clone() + &user("after the fork, parent only")),
        );
        let fork = rollout(
            tmp.path(),
            FORK,
            &(meta(FORK, Some((PARENT, before.len()))) + &user("the fork's own")),
        );
        let text = read_with_history(&fork, 1 << 20).expect("read");
        assert_eq!(
            prompts(&text),
            ["inherited one", "inherited two", "the fork's own"]
        );
    }

    #[test]
    fn a_fork_of_a_fork_reads_the_whole_chain() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = meta(PARENT, None) + &user("root");
        rollout(tmp.path(), PARENT, &root);
        let mid = meta(FORK, Some((PARENT, root.len()))) + &user("middle");
        rollout(tmp.path(), FORK, &mid);
        let leaf = rollout(
            tmp.path(),
            GRANDCHILD,
            &(meta(GRANDCHILD, Some((FORK, mid.len()))) + &user("leaf")),
        );
        let text = read_with_history(&leaf, 1 << 20).expect("read");
        assert_eq!(prompts(&text), ["root", "middle", "leaf"]);
    }

    #[test]
    fn a_forked_tab_s_transcript_shows_what_it_inherited() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let before = meta(PARENT, None) + &user("inherited");
        rollout(tmp.path(), PARENT, &before);
        let fork = rollout(
            tmp.path(),
            FORK,
            &(meta(FORK, Some((PARENT, before.len()))) + &user("the fork's own")),
        );
        let lines = super::super::codex_transcript::render_transcript(
            &fork,
            &crate::ui::theme::Theme::default(),
            None,
            true,
        );
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("inherited"), "{text}");
        assert!(text.contains("the fork's own"), "{text}");
    }

    #[test]
    fn an_unforked_rollout_reads_as_itself() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = rollout(tmp.path(), PARENT, &(meta(PARENT, None) + &user("only")));
        let text = read_with_history(&path, 1 << 20).expect("read");
        assert_eq!(prompts(&text), ["only"]);
    }

    #[test]
    fn a_missing_parent_leaves_the_fork_readable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let fork = rollout(
            tmp.path(),
            FORK,
            &(meta(FORK, Some((PARENT, 500))) + &user("the fork's own")),
        );
        let text = read_with_history(&fork, 1 << 20).expect("read");
        assert_eq!(prompts(&text), ["the fork's own"]);
    }

    #[test]
    fn a_fork_naming_itself_terminates() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let body = meta(FORK, Some((FORK, 10_000))) + &user("the fork's own");
        let fork = rollout(tmp.path(), FORK, &body);
        let text = read_with_history(&fork, 1 << 20).expect("read");
        assert_eq!(prompts(&text), ["the fork's own"]);
    }

    #[test]
    fn the_budget_is_spent_from_the_newest_end() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let before = meta(PARENT, None) + &user("oldest") + &user("newest inherited");
        rollout(tmp.path(), PARENT, &before);
        let own = meta(FORK, Some((PARENT, before.len()))) + &user("the fork's own");
        let fork = rollout(tmp.path(), FORK, &own);
        // Room for the fork's file and the parent's last line, not its first.
        let budget = (own.len() + user("newest inherited").len()) as u64;
        let text = read_with_history(&fork, budget).expect("read");
        assert_eq!(prompts(&text), ["newest inherited", "the fork's own"]);
    }
}

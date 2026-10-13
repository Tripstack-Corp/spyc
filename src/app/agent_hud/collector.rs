//! One cancellable startup lookup and one exact-file subscription: a Codex
//! rollout or a Claude Code transcript. The lookup is bounded and happens once
//! per binding; a missing source needs a new consumer.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::agent::tail::Availability;
use crate::agent::{claude_metrics, codex_metrics};
use crate::app::state::agent_hud::{Binding, Update};
use crate::app::{Message, Wake};

const MAX_LOOKUP_ENTRIES: usize = 65_536;

enum Subscription {
    Codex(codex_metrics::watch::Subscription),
    Claude(claude_metrics::watch::Subscription),
}

impl Subscription {
    fn start(binding: Binding, path: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        match binding {
            Binding::Codex(b) => {
                Self::Codex(codex_metrics::watch::Subscription::start(b, path, wake))
            }
            Binding::Claude(b) => {
                Self::Claude(claude_metrics::watch::Subscription::start(b, path, wake))
            }
        }
    }

    fn take(&self) -> Option<Update> {
        match self {
            Self::Codex(s) => s.take().map(|u| Update::Codex(Box::new(u))),
            Self::Claude(s) => s.take().map(|u| Update::Claude(Box::new(u))),
        }
    }

    fn work(&self) -> (u64, u64) {
        match self {
            Self::Codex(s) => s.work(),
            Self::Claude(s) => s.work(),
        }
    }
}

pub(in crate::app) struct Collector {
    pub(super) binding: Binding,
    ready: mpsc::Receiver<Result<Subscription, Availability>>,
    subscription: Option<Subscription>,
    cancelled: Arc<AtomicBool>,
    startup_finished: bool,
}

impl Collector {
    pub(super) fn start(binding: Binding, wake: Option<mpsc::Sender<Message>>) -> Self {
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let worker_binding = binding.clone();
        std::thread::spawn(move || {
            let path = match &worker_binding {
                Binding::Codex(b) => find_pinned(&b.codex_home, &b.thread_id, &worker_cancelled),
                Binding::Claude(b) => {
                    find_transcript(&b.claude_home, &b.session_id, &worker_cancelled)
                }
            };
            if worker_cancelled.load(Ordering::Acquire) {
                return;
            }
            let result = path.map_or(Err(Availability::Missing), |path| {
                let changed = wake.clone();
                Ok(Subscription::start(worker_binding, path, move || {
                    notify(changed.as_ref());
                }))
            });
            if worker_cancelled.load(Ordering::Acquire) {
                return;
            }
            if ready_tx.send(result).is_ok() {
                notify(wake.as_ref());
            }
        });
        Self {
            binding,
            ready,
            subscription: None,
            cancelled,
            startup_finished: false,
        }
    }

    pub(super) fn take(&mut self) -> Option<Update> {
        if !self.startup_finished {
            let result = match self.ready.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => Err(Availability::WatchFailed),
            };
            self.startup_finished = true;
            match result {
                Ok(subscription) => self.subscription = Some(subscription),
                Err(availability) => {
                    return Some(Update::unavailable(self.binding.clone(), availability));
                }
            }
        }
        self.subscription.as_ref().and_then(Subscription::take)
    }

    pub(super) fn work(&self) -> (u64, u64) {
        self.subscription
            .as_ref()
            .map_or((0, 0), Subscription::work)
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        // Dropping the subscription cancels; pending startup never joins the UI.
        self.subscription = None;
    }
}

fn notify(wake: Option<&mpsc::Sender<Message>>) {
    if let Some(tx) = wake {
        let _ = tx.send(Message::Wake(Wake::AgentHud));
    }
}

fn find_pinned(home: &Path, thread: &str, cancelled: &AtomicBool) -> Option<PathBuf> {
    if !home.is_absolute() || !crate::state::sessions::is_uuid(thread) {
        return None;
    }
    let home = home.canonicalize().ok()?;
    let sessions = home.join("sessions").canonicalize().ok()?;
    if !sessions.starts_with(&home) {
        return None;
    }
    let suffix = format!("-{thread}.jsonl");
    let mut stack = vec![(std::fs::read_dir(sessions).ok()?, 0u8)];
    let mut entries = 0usize;
    let mut matched = None;
    while let Some((directory, depth)) = stack.last_mut() {
        let depth = *depth;
        let Some(entry) = directory.next() else {
            stack.pop();
            continue;
        };
        entries += 1;
        if entries > MAX_LOOKUP_ENTRIES || cancelled.load(Ordering::Acquire) {
            return None;
        }
        let Ok(entry) = entry else { continue };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if depth < 3 && kind.is_dir() {
            if let Ok(list) = std::fs::read_dir(entry.path()) {
                stack.push((list, depth + 1));
            }
        } else if depth == 3 && kind.is_file() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with("rollout-") && name.ends_with(&suffix) {
                // The bounded B2 reader verifies containment and session_meta.
                if matched.is_some() {
                    return None;
                }
                matched = Some(entry.path());
            }
        }
    }
    matched
}

/// `<home>/projects/<project>/<session>.jsonl`: the project is the directory
/// Claude Code started in, which the pane's cwd may have left, so every
/// project is asked for the exact name. A session in two projects is not
/// guessed between.
fn find_transcript(home: &Path, session: &str, cancelled: &AtomicBool) -> Option<PathBuf> {
    if !home.is_absolute() || !crate::state::sessions::is_uuid(session) {
        return None;
    }
    let home = home.canonicalize().ok()?;
    let projects = home.join("projects").canonicalize().ok()?;
    if !projects.starts_with(&home) {
        return None;
    }
    let name = format!("{session}.jsonl");
    let mut matched = None;
    for (seen, entry) in std::fs::read_dir(&projects).ok()?.enumerate() {
        if seen >= MAX_LOOKUP_ENTRIES || cancelled.load(Ordering::Acquire) {
            return None;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path().join(&name);
        // The bounded reader verifies containment and every record's session.
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
            if matched.is_some() {
                return None;
            }
            matched = Some(path);
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;
    const THREAD: &str = "01a133b2-dc16-74c8-af3c-123456789abc";

    #[test]
    fn lookup_rejects_ambiguous_copies_escapes_deep_paths_and_cancellation() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let day = home.path().join("sessions/2026/10/13");
        std::fs::create_dir_all(&day).unwrap();
        let file = day.join(format!("rollout-first-{THREAD}.jsonl"));
        std::fs::write(&file, "header is verified by the reader").unwrap();
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            find_pinned(home.path(), THREAD, &cancelled).map(|p| p.canonicalize().unwrap()),
            Some(file.canonicalize().unwrap())
        );
        let copy = day.join(format!("rollout-copy-{THREAD}.jsonl"));
        std::fs::copy(&file, &copy).unwrap();
        assert!(
            find_pinned(home.path(), THREAD, &cancelled).is_none(),
            "duplicate ids cannot be guessed"
        );
        std::fs::remove_file(copy).unwrap();
        cancelled.store(true, Ordering::Release);
        assert!(find_pinned(home.path(), THREAD, &cancelled).is_none());
        cancelled.store(false, Ordering::Release);
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(day.join("too-deep")).unwrap();
        std::fs::write(day.join("too-deep").join(file.file_name().unwrap()), "x").unwrap();
        assert!(find_pinned(home.path(), THREAD, &cancelled).is_none());
        let escaped = outside.path().join(file.file_name().unwrap());
        std::fs::write(escaped, "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("sessions/2026/10/14"))
            .unwrap();
        assert!(find_pinned(home.path(), THREAD, &cancelled).is_none());
        assert!(find_pinned(Path::new("relative"), THREAD, &cancelled).is_none());
        assert!(find_pinned(home.path(), "../not-a-thread", &cancelled).is_none());
    }

    #[test]
    fn transcript_lookup_finds_one_exact_session_and_never_guesses() {
        const SESSION: &str = "c1a0de00-0000-4000-8000-000000000001";
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let name = format!("{SESSION}.jsonl");
        let started_in = home.path().join("projects/-src-spyc");
        std::fs::create_dir_all(started_in.join(SESSION).join("subagents")).unwrap();
        std::fs::write(started_in.join(SESSION).join("subagents").join(&name), "").unwrap();
        let cancelled = AtomicBool::new(false);
        assert!(
            find_transcript(home.path(), SESSION, &cancelled).is_none(),
            "a subagent's transcript is not the session's"
        );
        let file = started_in.join(&name);
        std::fs::write(&file, "").unwrap();
        std::fs::create_dir_all(home.path().join("projects/-elsewhere").join(&name)).unwrap();
        assert_eq!(
            find_transcript(home.path(), SESSION, &cancelled).map(|p| p.canonicalize().unwrap()),
            Some(file.canonicalize().unwrap()),
            "found from any project, and a directory by the name is no transcript"
        );
        let copy = home.path().join("projects/-moved");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join(&name), "").unwrap();
        assert!(
            find_transcript(home.path(), SESSION, &cancelled).is_none(),
            "a session in two projects is not guessed between"
        );
        std::fs::remove_file(copy.join(&name)).unwrap();
        cancelled.store(true, Ordering::Release);
        assert!(find_transcript(home.path(), SESSION, &cancelled).is_none());
        cancelled.store(false, Ordering::Release);
        assert!(find_transcript(Path::new("relative"), SESSION, &cancelled).is_none());
        assert!(find_transcript(home.path(), "../../etc/passwd", &cancelled).is_none());
        let escaped = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("-p")).unwrap();
        std::fs::write(outside.path().join("-p").join(&name), "").unwrap();
        std::os::unix::fs::symlink(outside.path(), escaped.path().join("projects")).unwrap();
        assert!(
            find_transcript(escaped.path(), SESSION, &cancelled).is_none(),
            "projects outside the home are not read"
        );
    }
}

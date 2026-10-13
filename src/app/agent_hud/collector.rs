//! One cancellable startup lookup and one exact-rollout subscription. The lookup
//! is bounded and happens once per binding; a missing source needs a new consumer.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::agent::codex_metrics::{Availability, Binding, Snapshot, Update, watch::Subscription};
use crate::app::{Message, Wake};

const MAX_LOOKUP_ENTRIES: usize = 65_536;

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
            let path = find_pinned(
                &worker_binding.codex_home,
                &worker_binding.thread_id,
                &worker_cancelled,
            );
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
                    return Some(Update {
                        binding: self.binding.clone(),
                        availability,
                        tail_only: false,
                        snapshot: Snapshot::default(),
                    });
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
}

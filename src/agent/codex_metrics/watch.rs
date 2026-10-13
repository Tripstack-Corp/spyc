//! An explicit consumer owns the subscription. One non-recursive parent watch
//! survives rollout replacement; a bounded mailbox coalesces bursts.
//!
//! No timer, account request, directory scan or filesystem read runs at idle.

#[cfg(any(test, not(target_os = "macos")))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};

#[cfg(any(test, not(target_os = "macos")))]
use notify::{
    EventKind,
    event::{AccessKind, AccessMode},
};

#[cfg(not(target_os = "macos"))]
use notify::{RecursiveMode, Watcher};

#[cfg(target_os = "macos")]
mod native;

use super::{Availability, Binding, Snapshot, Update, stream::Reader};

pub struct Subscription {
    latest: Arc<Mailbox>,
    signal: mpsc::SyncSender<()>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct Mailbox {
    latest: Mutex<Option<Update>>,
    bytes_read: AtomicU64,
    refreshes: AtomicU64,
}

impl Subscription {
    /// Start from a side-effect executor or external consumer. The worker sets up
    /// all reads, then calls `wake` only when the bounded mailbox becomes ready.
    pub fn start(
        binding: Binding,
        pinned_rollout: PathBuf,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let (signal, received) = mpsc::sync_channel(1);
        let latest = Arc::new(Mailbox::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_latest = latest.clone();
        let worker_cancelled = cancelled.clone();
        let worker_signal = signal.clone();
        std::thread::spawn(move || {
            run(
                binding,
                pinned_rollout,
                worker_signal,
                received,
                worker_latest,
                worker_cancelled,
                wake,
            );
        });
        Self {
            latest,
            signal,
            cancelled,
        }
    }

    pub fn take(&self) -> Option<Update> {
        // The worker cannot poison this lock: parsing and consumer callbacks run outside it.
        self.latest
            .latest
            .lock()
            .expect("telemetry mailbox lock")
            .take()
    }

    pub fn work(&self) -> (u64, u64) {
        (
            self.latest.bytes_read.load(Ordering::Acquire),
            self.latest.refreshes.load(Ordering::Acquire),
        )
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.signal.try_send(());
    }
}

#[cfg(any(test, not(target_os = "macos")))]
fn relevant(kind: EventKind, paths: &[PathBuf], rollout: &Path) -> bool {
    !matches!(
        kind,
        EventKind::Access(
            AccessKind::Open(_) | AccessKind::Read | AccessKind::Close(AccessMode::Read)
        )
    ) && paths
        .iter()
        .any(|path| path == rollout || Some(path.as_path()) == rollout.parent())
}

fn run(
    binding: Binding,
    path: PathBuf,
    signal: mpsc::SyncSender<()>,
    received: mpsc::Receiver<()>,
    latest: Arc<Mailbox>,
    cancelled: Arc<AtomicBool>,
    wake: impl Fn(),
) {
    // Watch backends report canonical paths (notably /private/var on macOS).
    let parent = path.parent().and_then(|p| p.canonicalize().ok());
    #[cfg(not(target_os = "macos"))]
    let watched_path = parent
        .as_ref()
        .zip(path.file_name())
        .map_or_else(|| path.clone(), |(parent, name)| parent.join(name));
    #[cfg(target_os = "macos")]
    let watcher = parent
        .as_deref()
        .and_then(|parent| native::NativeWatch::start(parent, path.clone(), signal).ok());
    #[cfg(target_os = "macos")]
    let installed = watcher.is_some();
    #[cfg(not(target_os = "macos"))]
    let watcher = {
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if match event {
                    Ok(event) => relevant(event.kind, &event.paths, &watched_path),
                    Err(_) => true,
                } {
                    let _ = signal.try_send(());
                }
            })
            .ok();
        let ready = parent
            .as_deref()
            .zip(watcher.as_mut())
            .is_some_and(|(parent, watcher)| {
                watcher.watch(parent, RecursiveMode::NonRecursive).is_ok()
            });
        if ready { watcher } else { None }
    };
    #[cfg(not(target_os = "macos"))]
    let installed = watcher.is_some();
    if !installed {
        publish(
            &latest,
            Update {
                binding,
                availability: Availability::WatchFailed,
                tail_only: false,
                snapshot: Snapshot::default(),
            },
            &wake,
        );
        return;
    }
    let mut reader = Reader::new(binding, path);
    let mut previous = None;
    loop {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        #[cfg(target_os = "macos")]
        if watcher.as_ref().is_some_and(native::NativeWatch::failed) {
            publish(
                &latest,
                Update {
                    binding: reader.refresh().binding,
                    availability: Availability::WatchFailed,
                    tail_only: false,
                    snapshot: Snapshot::default(),
                },
                &wake,
            );
            break;
        }
        let update = reader.refresh();
        latest
            .bytes_read
            .store(reader.bytes_read, Ordering::Release);
        latest.refreshes.store(reader.refreshes, Ordering::Release);
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        if previous.as_ref() != Some(&update) {
            previous = Some(update.clone());
            publish(&latest, update, &wake);
        }
        if received.recv().is_err() {
            break;
        }
    }
}

fn publish(latest: &Mailbox, update: Update, wake: &impl Fn()) {
    let ready = {
        // The only mutation under this lock replaces a bounded, already-folded snapshot.
        let mut slot = latest.latest.lock().expect("telemetry mailbox lock");
        let ready = slot.is_none();
        *slot = Some(update);
        ready
    };
    if ready {
        wake();
    }
}

#[cfg(test)]
mod tests;

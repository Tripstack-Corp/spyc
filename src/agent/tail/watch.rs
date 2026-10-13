//! An explicit consumer owns the subscription. One non-recursive parent watch
//! survives the file's replacement; a bounded mailbox coalesces bursts.

#[cfg(any(test, not(target_os = "macos")))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};

#[cfg(any(test, not(target_os = "macos")))]
use notify::EventKind;

#[cfg(not(target_os = "macos"))]
use notify::{RecursiveMode, Watcher};

#[cfg(target_os = "macos")]
use super::native;

/// What a subscription's worker refreshes on each wake.
pub trait Refresh: Send + 'static {
    type Update: Clone + PartialEq + Send + 'static;
    /// Read what changed and fold it.
    fn refresh(&mut self) -> Self::Update;
    /// What to publish when the watch can't be installed, or fails.
    fn watch_failed(&self) -> Self::Update;
    /// Bytes read and refreshes run, for diagnostics.
    fn work(&self) -> (u64, u64);
}

pub struct Subscription<U> {
    latest: Arc<Mailbox<U>>,
    signal: mpsc::SyncSender<()>,
    cancelled: Arc<AtomicBool>,
}

/// The single slot a worker publishes into; visible for the agents' own
/// watch tests.
pub struct Mailbox<U> {
    pub latest: Mutex<Option<U>>,
    bytes_read: AtomicU64,
    refreshes: AtomicU64,
}

impl<U> Default for Mailbox<U> {
    fn default() -> Self {
        Self {
            latest: Mutex::new(None),
            bytes_read: AtomicU64::new(0),
            refreshes: AtomicU64::new(0),
        }
    }
}

impl<U: Send + 'static> Subscription<U> {
    /// Start from a side-effect executor or external consumer. The worker sets up
    /// all reads, then calls `wake` only when the bounded mailbox becomes ready.
    pub fn start<R: Refresh<Update = U>>(
        path: PathBuf,
        source: R,
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
                path,
                source,
                worker_signal,
                received,
                &worker_latest,
                &worker_cancelled,
                wake,
            );
        });
        Self {
            latest,
            signal,
            cancelled,
        }
    }

    pub fn take(&self) -> Option<U> {
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

impl<U> Drop for Subscription<U> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.signal.try_send(());
    }
}

/// Whether a `notify` event can have changed `file`. No access event does: a
/// write reports its own `Modify`, and the close that follows (inotify's
/// `IN_CLOSE_WRITE`) adds nothing to it, so counting both read every append
/// twice.
#[cfg(any(test, not(target_os = "macos")))]
pub fn relevant(kind: EventKind, paths: &[PathBuf], file: &Path) -> bool {
    !matches!(kind, EventKind::Access(_))
        && paths
            .iter()
            .any(|path| path == file || Some(path.as_path()) == file.parent())
}

fn run<R: Refresh>(
    path: PathBuf,
    mut source: R,
    signal: mpsc::SyncSender<()>,
    received: mpsc::Receiver<()>,
    latest: &Mailbox<R::Update>,
    cancelled: &AtomicBool,
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
        publish(latest, source.watch_failed(), &wake);
        return;
    }
    let mut previous = None;
    loop {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        #[cfg(target_os = "macos")]
        if watcher.as_ref().is_some_and(native::NativeWatch::failed) {
            publish(latest, source.watch_failed(), &wake);
            break;
        }
        let update = source.refresh();
        let (bytes_read, refreshes) = source.work();
        latest.bytes_read.store(bytes_read, Ordering::Release);
        latest.refreshes.store(refreshes, Ordering::Release);
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        if previous.as_ref() != Some(&update) {
            previous = Some(update.clone());
            publish(latest, update, &wake);
        }
        if received.recv().is_err() {
            break;
        }
    }
}

pub fn publish<U>(latest: &Mailbox<U>, update: U, wake: &impl Fn()) {
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

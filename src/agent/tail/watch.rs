//! An explicit consumer owns the subscription. One non-recursive parent watch
//! survives the file's replacement; a bounded mailbox coalesces bursts. A
//! [`Watched`] source instead names, after each read, the paths to watch, which
//! it learns as it reads ([`Subscription::start_set`]).

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

/// A [`Refresh`] source that says, after each read, which files and
/// directories a change to would change what it reads next: ones it learned of
/// as it read, some not created yet. Each is watched once it exists.
pub trait Watched: Refresh {
    fn watch_paths(&self) -> Vec<PathBuf>;
}

/// How many reads in a row may each arm a new path before the worker waits for
/// a change: what was armed just now may have changed before it was, and only
/// a read sees that.
const REARM_READS: usize = 4;

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

    /// [`Self::start`] for a [`Watched`] source: no one file, but the paths
    /// the source names after each read.
    pub fn start_set<R: Watched<Update = U>>(source: R, wake: impl Fn() + Send + 'static) -> Self {
        let (signal, received) = mpsc::sync_channel(1);
        let latest = Arc::new(Mailbox::default());
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_latest = latest.clone();
        let worker_cancelled = cancelled.clone();
        let worker_signal = signal.clone();
        std::thread::spawn(move || {
            run_set(
                source,
                worker_signal,
                &received,
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

fn run_set<R: Watched>(
    mut source: R,
    signal: mpsc::SyncSender<()>,
    received: &mpsc::Receiver<()>,
    latest: &Mailbox<R::Update>,
    cancelled: &AtomicBool,
    wake: impl Fn(),
) {
    let Some(mut watch) = new_set_watch(signal) else {
        publish(latest, source.watch_failed(), &wake);
        return;
    };
    let mut previous = None;
    let mut rearmed = 0;
    loop {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        if watch.failed() {
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
        if watch.set(&source.watch_paths()) && rearmed < REARM_READS {
            rearmed += 1;
            continue;
        }
        rearmed = 0;
        if received.recv().is_err() {
            break;
        }
    }
}

#[cfg(target_os = "macos")]
use native::SetWatch;

#[cfg(target_os = "macos")]
fn new_set_watch(signal: mpsc::SyncSender<()>) -> Option<SetWatch> {
    SetWatch::start(signal).ok()
}

#[cfg(not(target_os = "macos"))]
fn new_set_watch(signal: mpsc::SyncSender<()>) -> Option<SetWatch> {
    SetWatch::start(signal)
}

/// Watches on the directories of a set of paths: a directory's watch reports
/// its files' writes too, so a file is watched through its directory.
#[cfg(not(target_os = "macos"))]
struct SetWatch {
    watcher: notify::RecommendedWatcher,
    watched: std::collections::BTreeSet<PathBuf>,
}

#[cfg(not(target_os = "macos"))]
impl SetWatch {
    fn start(signal: mpsc::SyncSender<()>) -> Option<Self> {
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_err()
                || event.is_ok_and(|event| !matches!(event.kind, EventKind::Access(_)))
            {
                let _ = signal.try_send(());
            }
        })
        .ok()?;
        Some(Self {
            watcher,
            watched: std::collections::BTreeSet::new(),
        })
    }

    const fn failed(&self) -> bool {
        false
    }

    fn set(&mut self, paths: &[PathBuf]) -> bool {
        let wanted: std::collections::BTreeSet<PathBuf> = paths
            .iter()
            .filter_map(|path| {
                if path.is_dir() {
                    Some(path.clone())
                } else {
                    path.parent()
                        .filter(|dir| dir.is_dir())
                        .map(Path::to_path_buf)
                }
            })
            .collect();
        let watcher = &mut self.watcher;
        self.watched.retain(|dir| {
            let keep = wanted.contains(dir);
            if !keep {
                let _ = watcher.unwatch(dir);
            }
            keep
        });
        let mut armed = false;
        for dir in wanted {
            if !self.watched.contains(&dir)
                && self
                    .watcher
                    .watch(&dir, RecursiveMode::NonRecursive)
                    .is_ok()
            {
                self.watched.insert(dir);
                armed = true;
            }
        }
        armed
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

//! A jarred pane's snapshots: when its parser takes one, and how it reaches
//! the jar.
//!
//! A snapshot follows a second of quiet, and every five seconds while output
//! flows, so a gap in a long stream restores from a recent one (R17e). Two at
//! a time across every pane (R17d): thirty at once held each parser's lock
//! many times as long. Each is stored in both forms (decision I), at the jar
//! offset of the last byte it reflects (finding 8). The binary form carries
//! the build and pin that made it, since only those can decode it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use spyc_jar::proto::{Msg, SNAPSHOT_BINARY, SNAPSHOT_VT, Snapshot};

use super::link::Link;
use crate::pane::PaneEngine;
use crate::pane::engine::{Engine, TerminalScreen};

const QUIET: Duration = Duration::from_secs(1);
const WHILE_FLOWING: Duration = Duration::from_secs(5);
const AT_ONCE: usize = 2;
/// Queued bytes past which the jar isn't keeping up, and a snapshot waits.
const BACKLOG: usize = 4 << 20;
/// The scrollback a portable form is built with: a pane's own.
const SCROLLBACK_ROWS: usize = 10_000;

/// Where a jarred pane's snapshots go.
#[derive(Clone)]
pub struct SnapshotSink(pub(super) Arc<Link>);

impl SnapshotSink {
    /// Store both forms of a snapshot of the first `parsed` bytes the pane
    /// was handed.
    pub fn store(
        &self,
        parsed: u64,
        (rows, cols): (u16, u16),
        binary: &[u8],
        portable: Option<Vec<u8>>,
    ) {
        // SPYC-TRAP(jar-snapshot-offset): the jar's offset, not `parsed`, which
        // a gap and its resynchronizing byte set apart from it.
        let offset = self
            .0
            .offsets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .at(parsed);
        let forms = [
            (SNAPSHOT_BINARY, Some(stamp(binary))),
            (SNAPSHOT_VT, portable),
        ];
        for (format, blob) in forms {
            if let Some(blob) = blob {
                let _ = self.0.out.send(&Msg::StoreSnapshot(Snapshot {
                    format,
                    offset,
                    rows,
                    cols,
                    blob,
                }));
            }
        }
    }

    /// Whether the jar is behind on what was sent: a snapshot waits.
    fn backlogged(&self) -> bool {
        self.0.out.queued() > BACKLOG
    }
}

/// A parser worker's schedule of snapshots.
pub struct Snapshotter {
    sink: SnapshotSink,
    parsed: u64,
    taken: u64,
    last_taken: Instant,
    last_bytes: Instant,
}

impl Snapshotter {
    pub fn new(sink: SnapshotSink) -> Self {
        let now = Instant::now();
        Self {
            sink,
            parsed: 0,
            taken: 0,
            last_taken: now,
            last_bytes: now,
        }
    }

    /// `n` more bytes were parsed.
    pub fn parsed(&mut self, n: usize) {
        self.parsed += n as u64;
        self.last_bytes = Instant::now();
    }

    /// Take a snapshot if one is due. `settled` is false while the program
    /// holds a synchronized update open: the screen is mid-frame.
    pub fn tick(&mut self, parser: &Mutex<PaneEngine>, settled: bool) {
        let since = self.last_taken.elapsed();
        let due = (self.last_bytes.elapsed() >= QUIET && since >= QUIET) || since >= WHILE_FLOWING;
        if self.parsed == self.taken || !settled || !due || self.sink.backlogged() {
            return;
        }
        let Some(_turn) = Turn::take() else {
            return;
        };
        self.last_taken = Instant::now();
        let p = parser.lock().unwrap_or_else(PoisonError::into_inner);
        let size = p.screen().size();
        let binary = p.encode_snapshot();
        drop(p);
        let Some(binary) = binary else {
            return;
        };
        let portable = PaneEngine::portable_from_binary(&binary, SCROLLBACK_ROWS);
        self.sink.store(self.parsed, size, &binary, portable);
        self.taken = self.parsed;
    }
}

/// One of the few snapshots that may be taken at once; `None` while they're
/// all in use, and the pane tries again on its next pass.
struct Turn;

static RUNNING: AtomicUsize = AtomicUsize::new(0);

impl Turn {
    fn take() -> Option<Self> {
        RUNNING
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < AT_ONCE).then_some(n + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A pane's engine at `rows` x `cols`, from an adopted jar's `snapshot`:
/// the binary form when this build made it, else the portable form's replay.
/// One that won't restore leaves the engine blank, and the replay fills it.
pub fn engine_from(snapshot: &Snapshot, (rows, cols): (u16, u16)) -> PaneEngine {
    let mut e = <PaneEngine as Engine>::new(rows, cols, SCROLLBACK_ROWS);
    let restored = match snapshot.format {
        SNAPSHOT_BINARY => unstamp(&snapshot.blob).is_some_and(|b| e.restore_snapshot(b)),
        SNAPSHOT_VT => {
            e.restore_portable(&snapshot.blob, snapshot.rows, snapshot.cols);
            true
        }
        _ => false,
    };
    if !restored {
        crate::spyc_debug!("jar snapshot (format {}) didn't restore", snapshot.format);
    }
    if e.screen().size() != (rows, cols) {
        e.screen_mut().set_size(rows, cols);
    }
    e
}

/// What made a binary snapshot: this spyc and the libghostty pin it links.
fn build() -> String {
    format!(
        "spyc {} {} ghostty {}",
        env!("CARGO_PKG_VERSION"),
        env!("SPYC_GIT_SHA"),
        spyc_vt_sys::pin::GHOSTTY_COMMIT
    )
}

pub(super) const STAMP_MAGIC: &[u8] = b"spyc-snapshot\0";

/// `blob` with the build that made it ahead of it.
pub(super) fn stamp(blob: &[u8]) -> Vec<u8> {
    let build = build();
    let len = u16::try_from(build.len()).expect("a build name fits in u16");
    [STAMP_MAGIC, &len.to_be_bytes(), build.as_bytes(), blob].concat()
}

/// The snapshot inside `stamped`, when this build made it.
pub(super) fn unstamp(stamped: &[u8]) -> Option<&[u8]> {
    let rest = stamped.strip_prefix(STAMP_MAGIC)?;
    let (len, rest) = rest.split_first_chunk::<2>()?;
    let (build, blob) = rest.split_at_checked(usize::from(u16::from_be_bytes(*len)))?;
    (build == self::build().as_bytes()).then_some(blob)
}

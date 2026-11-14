//! What a jar is, apart from its threads and descriptors.
//!
//! Its configuration, the event log of what its program printed and how big
//! its terminal was, the lease and the exit, and each connection's place in
//! the log. A [`Handover`](crate::handover::Handover) carries all of it across
//! `exec`.
//!
//! The log never parses a byte. Offsets number the bytes the program printed;
//! sequence numbers order the events, so a resize at an offset already sent is
//! still sent once.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::proto::{Role, Snapshot};

/// How a jar was started, kept across upgrades.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub socket: PathBuf,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub rows: u16,
    pub cols: u16,
    /// Bytes of output the log holds at most.
    pub ring_cap: usize,
    /// How far a live owner may fall behind before the program waits (R11).
    pub window: u64,
    /// How long an owner may stay silent before its lease lapses (R21).
    pub deadline: Duration,
    /// How long an ended program's jar waits for an owner to collect the exit.
    pub linger_after_exit: Duration,
    /// R32a: how long the program runs on with no owner before the jar ends
    /// it, as a closed terminal would; `None` waits for an owner however long.
    pub linger: Option<Duration>,
    /// R31: this jar's instance, random at its start and kept across
    /// upgrades, and when it started, in seconds since the epoch.
    pub instance: u64,
    pub created: u64,
}

/// One entry in the log, at the offset of the next byte after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Out(u64, Vec<u8>),
    Resized(u64, u16, u16),
}

impl Event {
    pub(crate) const fn offset(&self) -> u64 {
        match self {
            Self::Out(o, _) | Self::Resized(o, _, _) => *o,
        }
    }
}

/// The event log: every event at or after `base`, numbered by sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log {
    pub events: VecDeque<(u64, Event)>,
    pub next_seq: u64,
    /// Offset of the next byte the program prints.
    pub head: u64,
    /// Offset of the oldest byte still held; earlier ones are gone.
    pub base: u64,
    /// Bytes of output the events hold.
    pub held: usize,
    pub cap: usize,
}

impl Log {
    pub(crate) fn push(&mut self, event: Event) {
        if let Event::Out(_, bytes) = &event {
            self.head += bytes.len() as u64;
            self.held += bytes.len();
        }
        self.events.push_back((self.next_seq, event));
        self.next_seq += 1;
        self.trim(0);
    }

    /// Drop events wholly below `keep_from`, then the oldest until the log
    /// fits its cap.
    pub(crate) fn trim(&mut self, keep_from: u64) {
        while let Some((_, front)) = self.events.front() {
            // An output is wholly before `keep_from` once it ends at or before
            // it; a resize only when it happened strictly before it, since one
            // at `keep_from` lays out every byte from there on.
            let below = match front {
                Event::Out(o, b) => o + b.len() as u64 <= keep_from,
                Event::Resized(o, ..) => *o < keep_from,
            };
            if !below && self.held <= self.cap {
                break;
            }
            if let Some((_, Event::Out(o, b))) = self.events.pop_front() {
                self.held -= b.len();
                self.base = o + b.len() as u64;
            }
        }
        if self.events.is_empty() {
            self.base = self.head;
        }
    }

    /// The first sequence number to send to a client replaying from `from`.
    pub(crate) fn seq_for(&self, from: u64) -> u64 {
        self.events
            .iter()
            .find(|(_, e)| match e {
                Event::Out(o, b) => o + b.len() as u64 > from,
                Event::Resized(o, ..) => *o >= from,
            })
            .map_or(self.next_seq, |(seq, _)| *seq)
    }
}

/// The jar's mutable state, under its one lock.
#[derive(Clone, Debug)]
pub struct State {
    pub log: Log,
    /// The connection that holds the lease.
    pub owner: Option<u64>,
    /// The owner has parsed everything before this offset.
    pub acked: u64,
    /// The latest snapshot of each format (R17a): the binary one for the same
    /// build, the portable one for an upgrade across a pin bump.
    pub snapshots: Vec<Snapshot>,
    /// The program's exit: its code, and whether a signal ended it.
    pub exit: Option<(i32, bool)>,
    pub exit_acked: bool,
    pub pty_eof: bool,
    pub rows: u16,
    pub cols: u16,
    pub next_conn: u64,
    pub ownerless_since: Option<Instant>,
    /// The PTY's foreground process group, 0 until read.
    pub fg: u32,
    /// Whether any owner has connected yet.
    pub had_owner: bool,
    /// R26: what a `hello` must present for any role but `Lookup`. Its
    /// starter hands it over on standard input; a release (R22) replaces it.
    pub token: Vec<u8>,
    /// R18: the owner's key/value store.
    pub info: Vec<(String, String)>,
}

/// What a connection is, apart from its socket: enough to resume it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnState {
    pub id: u64,
    pub role: Role,
    /// Where its writer is in the log, as a sequence number and an offset;
    /// `None` until its first `Replay`.
    pub cursor: Option<(u64, u64)>,
    pub exit_sent: bool,
    pub fg_sent: u32,
}

//! Bounded reads of one exact pinned rollout, through the shared
//! [`crate::agent::tail`] reader. Filesystem work belongs on the collector
//! worker; neither this reader nor its reducer discovers sessions.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::agent::tail::{Follow, Tail, reader::unavailable};
pub use crate::agent::tail::{MAX_LINE_BYTES, MAX_TAIL_BYTES};

use super::{Availability, Binding, Reducer, Update};

/// A Codex rollout: under `codex_home/sessions`, headed by the thread's
/// `session_meta`, one record per line.
struct Rollout {
    codex_home: PathBuf,
    thread: String,
    reducer: Reducer,
}

impl Follow for Rollout {
    fn admits(&self, path: &Path) -> Result<(), Availability> {
        if !self.codex_home.is_absolute() {
            return Err(Availability::IdentityMismatch);
        }
        let root = self
            .codex_home
            .join("sessions")
            .canonicalize()
            .map_err(unavailable)?;
        let path = path.canonicalize().map_err(unavailable)?;
        if !path.starts_with(root) {
            return Err(Availability::IdentityMismatch);
        }
        Ok(())
    }

    fn verify(&mut self, file: &mut File) -> Result<u64, Availability> {
        file.seek(SeekFrom::Start(0)).map_err(unavailable)?;
        let mut header = Vec::new();
        let mut reader = BufReader::new(file.take((MAX_LINE_BYTES + 1) as u64));
        let count = reader.read_until(b'\n', &mut header).map_err(unavailable)?;
        // Include read-ahead, not just bytes consumed by read_until.
        let read = (count + reader.buffer().len()) as u64;
        if count > MAX_LINE_BYTES || header.last() != Some(&b'\n') {
            return Err(Availability::InvalidHeader);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&header).map_err(|_| Availability::InvalidHeader)?;
        if value["type"] != "session_meta" || value["payload"]["id"].as_str() != Some(&self.thread)
        {
            return Err(Availability::IdentityMismatch);
        }
        Ok(read)
    }

    fn line(&mut self, line: &[u8]) {
        if let Ok(value) = serde_json::from_slice(line)
            && let Some(event) = crate::agent::codex_records::metrics::decode(&value, &self.thread)
        {
            self.reducer.fold(event);
        }
    }

    fn reset(&mut self) {
        self.reducer = Reducer::default();
    }
}

pub struct Reader {
    binding: Binding,
    tail: Tail<Rollout>,
    /// Diagnostic work counters; they do not cause changed-metric notifications.
    pub bytes_read: u64,
    pub refreshes: u64,
}

impl Reader {
    pub fn new(binding: Binding, pinned_rollout: PathBuf) -> Self {
        let rollout = Rollout {
            codex_home: binding.codex_home.clone(),
            thread: binding.thread_id.clone(),
            reducer: Reducer::default(),
        };
        Self {
            binding,
            tail: Tail::new(pinned_rollout, rollout),
            bytes_read: 0,
            refreshes: 0,
        }
    }

    pub fn refresh(&mut self) -> Update {
        let availability = self.tail.refresh();
        self.bytes_read = self.tail.bytes_read;
        self.refreshes = self.tail.refreshes;
        Update {
            binding: self.binding.clone(),
            availability,
            tail_only: self.tail.tail_only(),
            snapshot: self.tail.follow.reducer.snapshot(),
        }
    }

    pub(super) const fn binding(&self) -> &Binding {
        &self.binding
    }
}

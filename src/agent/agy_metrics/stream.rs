//! Bounded reads of one exact agy transcript.
//!
//! They go through the shared [`crate::agent::tail`] reader. Filesystem work
//! belongs on the collector worker; neither this reader nor its reducer
//! discovers sessions.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::agent::tail::{Follow, Tail, reader::unavailable};

use super::{Availability, Binding, Reducer, Update, records};

/// An agy transcript: `<agy_home>/.gemini/antigravity-cli/brain/<session>/.system_generated/logs/transcript.jsonl`,
/// one record per line, with no header record.
struct Transcript {
    agy_home: PathBuf,
    session: String,
    reducer: Reducer,
}

impl Follow for Transcript {
    fn admits(&self, path: &Path) -> Result<(), Availability> {
        if !self.agy_home.is_absolute() {
            return Err(Availability::IdentityMismatch);
        }
        let root = self
            .agy_home
            .join(".gemini/antigravity-cli/brain")
            .canonicalize()
            .map_err(unavailable)?;
        let path = path.canonicalize().map_err(unavailable)?;
        let expected = crate::state::sessions::agy_transcript_path(&self.agy_home, &self.session);
        let expected = expected.canonicalize().map_err(unavailable)?;
        if path != expected || !path.starts_with(root) {
            return Err(Availability::IdentityMismatch);
        }
        Ok(())
    }

    fn verify(&mut self, _file: &mut File) -> Result<u64, Availability> {
        Ok(0)
    }

    fn line(&mut self, line: &[u8]) {
        if let Ok(value) = serde_json::from_slice(line)
            && let Some(event) = records::decode(&value)
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
    tail: Tail<Transcript>,
    /// Diagnostic work counters; they do not cause changed-metric notifications.
    pub bytes_read: u64,
    pub refreshes: u64,
}

impl Reader {
    pub fn new(binding: Binding, transcript: PathBuf) -> Self {
        let follow = Transcript {
            agy_home: binding.agy_home.clone(),
            session: binding.session_id.clone(),
            reducer: Reducer::default(),
        };
        Self {
            binding,
            tail: Tail::new(transcript, follow),
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

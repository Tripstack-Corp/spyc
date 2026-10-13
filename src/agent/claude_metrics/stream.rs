//! Bounded reads of one exact Claude Code transcript.
//!
//! They go through the shared [`crate::agent::tail`] reader. Filesystem work
//! belongs on the collector worker; neither this reader nor its reducer
//! discovers sessions.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::agent::tail::{Follow, Tail, reader::unavailable};

use super::{Availability, Binding, Reducer, Update, records};

/// A Claude Code transcript: `<claude_home>/projects/<project>/<session>.jsonl`,
/// one record per line, with no header record. The file's name is the session
/// it holds, and `records::decode` checks every record's `sessionId` as well.
struct Transcript {
    claude_home: PathBuf,
    session: String,
    reducer: Reducer,
}

impl Follow for Transcript {
    fn admits(&self, path: &Path) -> Result<(), Availability> {
        if !self.claude_home.is_absolute() {
            return Err(Availability::IdentityMismatch);
        }
        let root = self
            .claude_home
            .join("projects")
            .canonicalize()
            .map_err(unavailable)?;
        let path = path.canonicalize().map_err(unavailable)?;
        let named = path
            .file_name()
            .is_some_and(|name| name.to_str() == Some(&format!("{}.jsonl", self.session)));
        if !path.starts_with(root) || !named {
            return Err(Availability::IdentityMismatch);
        }
        Ok(())
    }

    fn verify(&mut self, _file: &mut File) -> Result<u64, Availability> {
        Ok(0)
    }

    fn line(&mut self, line: &[u8]) {
        if let Ok(value) = serde_json::from_slice(line)
            && let Some(event) = records::decode(&value, &self.session)
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
            claude_home: binding.claude_home.clone(),
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

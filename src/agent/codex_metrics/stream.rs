//! Bounded reads of one exact pinned rollout. Filesystem work belongs on the
//! collector worker; neither this reader nor its reducer discovers sessions.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

use super::{Availability, Binding, Reducer, Update};

pub const MAX_TAIL_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 256 * 1024;
const CHUNK_BYTES: usize = 64 * 1024;
const ANCHOR_BYTES: u64 = 64;

#[derive(Default)]
struct Lines {
    pending: Vec<u8>,
    discarding: bool,
}

impl Lines {
    fn feed(&mut self, bytes: &[u8], reducer: &mut Reducer, thread: &str) {
        for fragment in bytes.split_inclusive(|byte| *byte == b'\n') {
            let complete = fragment.last() == Some(&b'\n');
            if !self.discarding {
                if self.pending.len().saturating_add(fragment.len()) > MAX_LINE_BYTES {
                    self.pending.clear();
                    self.discarding = true;
                } else {
                    self.pending.extend_from_slice(fragment);
                }
            }
            if complete {
                if !self.discarding
                    && let Ok(value) = serde_json::from_slice(&self.pending)
                    && let Some(event) =
                        crate::agent::codex_records::metrics::decode(&value, thread)
                {
                    reducer.fold(event);
                }
                self.pending.clear();
                self.discarding = false;
            }
        }
    }
}

pub struct Reader {
    binding: Binding,
    path: PathBuf,
    identity: Option<(u64, u64)>,
    offset: u64,
    anchor: Vec<u8>,
    lines: Lines,
    reducer: Reducer,
    tail_only: bool,
    /// Diagnostic work counters; they do not cause changed-metric notifications.
    pub bytes_read: u64,
    pub refreshes: u64,
}

impl Reader {
    pub fn new(binding: Binding, pinned_rollout: PathBuf) -> Self {
        Self {
            binding,
            path: pinned_rollout,
            identity: None,
            offset: 0,
            anchor: Vec::new(),
            lines: Lines::default(),
            reducer: Reducer::default(),
            tail_only: false,
            bytes_read: 0,
            refreshes: 0,
        }
    }

    pub fn refresh(&mut self) -> Update {
        self.refreshes = self.refreshes.saturating_add(1);
        let availability = match self.read_available() {
            Ok(()) => Availability::Available,
            Err(availability) => {
                self.reset();
                availability
            }
        };
        Update {
            binding: self.binding.clone(),
            availability,
            tail_only: self.tail_only,
            snapshot: self.reducer.snapshot(),
        }
    }

    fn reset(&mut self) {
        self.identity = None;
        self.offset = 0;
        self.anchor.clear();
        self.lines = Lines::default();
        self.reducer = Reducer::default();
        self.tail_only = false;
    }

    fn open(&self) -> Result<File, Availability> {
        if !self.binding.codex_home.is_absolute() || !self.path.is_absolute() {
            return Err(Availability::IdentityMismatch);
        }
        let root = self
            .binding
            .codex_home
            .join("sessions")
            .canonicalize()
            .map_err(unavailable)?;
        let path = self.path.canonicalize().map_err(unavailable)?;
        if !path.starts_with(root) {
            return Err(Availability::IdentityMismatch);
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(&self.path)
            .map_err(unavailable)?;
        if !file.metadata().map_err(unavailable)?.is_file() {
            return Err(Availability::Unreadable);
        }
        Ok(file)
    }

    fn read_available(&mut self) -> Result<(), Availability> {
        let mut file = self.open()?;
        let metadata = file.metadata().map_err(unavailable)?;
        self.check_header(&mut file)?;
        let identity = (metadata.dev(), metadata.ino());
        let replace = self.identity != Some(identity)
            || metadata.len() < self.offset
            || !self.anchor_matches(&mut file)?;
        if replace {
            self.reset();
            self.identity = Some(identity);
        }
        self.prepare_offset(&metadata);
        file.seek(SeekFrom::Start(self.offset))
            .map_err(unavailable)?;
        // Length is captured before the read. Concurrent appends arrive on the
        // already-installed watch, and cannot turn one wake into unbounded work.
        let mut remaining = metadata
            .len()
            .saturating_sub(self.offset)
            .min(MAX_TAIL_BYTES);
        let mut chunk = vec![0u8; CHUNK_BYTES].into_boxed_slice();
        while remaining > 0 {
            let count = file
                .read(&mut chunk[..remaining.min(CHUNK_BYTES as u64) as usize])
                .map_err(unavailable)?;
            if count == 0 {
                break;
            }
            self.lines
                .feed(&chunk[..count], &mut self.reducer, &self.binding.thread_id);
            self.offset += count as u64;
            self.bytes_read = self.bytes_read.saturating_add(count as u64);
            remaining -= count as u64;
        }
        self.anchor = read_anchor(&mut file, self.offset)?;
        self.bytes_read = self.bytes_read.saturating_add(self.anchor.len() as u64);
        Ok(())
    }

    fn prepare_offset(&mut self, metadata: &Metadata) {
        if metadata.len().saturating_sub(self.offset) > MAX_TAIL_BYTES {
            // A gap invalidates active/recent state. Tail snapshots remain useful,
            // but cannot establish an exhaustive tool lifecycle.
            self.reducer = Reducer::default();
            self.offset = metadata.len() - MAX_TAIL_BYTES;
            self.lines = Lines {
                pending: Vec::new(),
                discarding: true,
            };
            self.tail_only = true;
        }
    }

    fn check_header(&mut self, file: &mut File) -> Result<(), Availability> {
        file.seek(SeekFrom::Start(0)).map_err(unavailable)?;
        let mut header = Vec::new();
        let mut reader = BufReader::new(file.take((MAX_LINE_BYTES + 1) as u64));
        let count = reader.read_until(b'\n', &mut header).map_err(unavailable)?;
        // Include read-ahead, not just bytes consumed by read_until.
        self.bytes_read = self
            .bytes_read
            .saturating_add((count + reader.buffer().len()) as u64);
        if count > MAX_LINE_BYTES || header.last() != Some(&b'\n') {
            return Err(Availability::InvalidHeader);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&header).map_err(|_| Availability::InvalidHeader)?;
        if value["type"] != "session_meta"
            || value["payload"]["id"].as_str() != Some(&self.binding.thread_id)
        {
            return Err(Availability::IdentityMismatch);
        }
        Ok(())
    }

    fn anchor_matches(&mut self, file: &mut File) -> Result<bool, Availability> {
        let anchor = read_anchor(file, self.offset)?;
        self.bytes_read = self.bytes_read.saturating_add(anchor.len() as u64);
        Ok(anchor == self.anchor)
    }
}

fn read_anchor(file: &mut File, offset: u64) -> Result<Vec<u8>, Availability> {
    let count = offset.min(ANCHOR_BYTES);
    file.seek(SeekFrom::Start(offset - count))
        .map_err(unavailable)?;
    let mut bytes = vec![0; count as usize];
    file.read_exact(&mut bytes).map_err(unavailable)?;
    Ok(bytes)
}

fn unavailable(error: std::io::Error) -> Availability {
    if error.kind() == std::io::ErrorKind::NotFound {
        Availability::Missing
    } else {
        Availability::Unreadable
    }
}

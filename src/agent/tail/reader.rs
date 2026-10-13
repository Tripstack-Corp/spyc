//! Bounded reads of one exact session file. Filesystem work belongs on the
//! collector worker; neither this reader nor what it feeds discovers sessions.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::Availability;

pub const MAX_TAIL_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 256 * 1024;
const CHUNK_BYTES: usize = 64 * 1024;
const ANCHOR_BYTES: u64 = 64;

/// What one agent's session file means, and how to tell it is the expected one.
pub trait Follow {
    /// Whether `path` may be read at all, checked before every open.
    fn admits(&self, path: &Path) -> Result<(), Availability>;
    /// Checked on every read, before a byte is folded. Returns how many bytes
    /// the check itself read, for the work counters.
    fn verify(&mut self, file: &mut File) -> Result<u64, Availability>;
    /// One complete line, newline included, at most [`MAX_LINE_BYTES`].
    fn line(&mut self, line: &[u8]);
    /// Forget everything folded: the file was replaced, truncated or skipped.
    fn reset(&mut self);
}

#[derive(Default)]
struct Lines {
    pending: Vec<u8>,
    discarding: bool,
}

impl Lines {
    fn feed(&mut self, bytes: &[u8], follow: &mut impl Follow) {
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
                if !self.discarding {
                    follow.line(&self.pending);
                }
                self.pending.clear();
                self.discarding = false;
            }
        }
    }
}

/// Incremental reads of one file. Appends read only the new bytes, plus a
/// bounded anchor check; a replaced, truncated or rewritten file starts over,
/// and a backlog past [`MAX_TAIL_BYTES`] is skipped to its tail.
pub struct Tail<F> {
    path: PathBuf,
    pub follow: F,
    identity: Option<(u64, u64)>,
    offset: u64,
    anchor: Vec<u8>,
    lines: Lines,
    skipped_backlog: bool,
    /// Diagnostic work counters; they do not cause changed-metric notifications.
    pub bytes_read: u64,
    pub refreshes: u64,
}

impl<F: Follow> Tail<F> {
    pub fn new(path: PathBuf, follow: F) -> Self {
        Self {
            path,
            follow,
            identity: None,
            offset: 0,
            anchor: Vec::new(),
            lines: Lines::default(),
            skipped_backlog: false,
            bytes_read: 0,
            refreshes: 0,
        }
    }

    /// Whether what was folded began past the file's start.
    pub const fn tail_only(&self) -> bool {
        self.skipped_backlog
    }

    /// Fold what was appended since the last refresh. A file that can't be
    /// read as the expected session's resets everything folded.
    pub fn refresh(&mut self) -> Availability {
        self.refreshes = self.refreshes.saturating_add(1);
        match self.read_available() {
            Ok(()) => Availability::Available,
            Err(availability) => {
                self.reset();
                availability
            }
        }
    }

    fn reset(&mut self) {
        self.identity = None;
        self.offset = 0;
        self.anchor.clear();
        self.lines = Lines::default();
        self.follow.reset();
        self.skipped_backlog = false;
    }

    fn open(&self) -> Result<File, Availability> {
        if !self.path.is_absolute() {
            return Err(Availability::IdentityMismatch);
        }
        self.follow.admits(&self.path)?;
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
        let verified = self.follow.verify(&mut file)?;
        self.bytes_read = self.bytes_read.saturating_add(verified);
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
            self.lines.feed(&chunk[..count], &mut self.follow);
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
            self.follow.reset();
            self.offset = metadata.len() - MAX_TAIL_BYTES;
            self.lines = Lines {
                pending: Vec::new(),
                discarding: true,
            };
            self.skipped_backlog = true;
        }
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

pub fn unavailable(error: std::io::Error) -> Availability {
    if error.kind() == std::io::ErrorKind::NotFound {
        Availability::Missing
    } else {
        Availability::Unreadable
    }
}

//! What spyc's own stderr has said this run, as the `:stderr` log last read
//! it, and how much of it the user has seen: the status bar's `⚠N` counts the
//! rest.

use std::sync::Arc;

/// One reading of the log.
#[derive(Clone, Debug, Default)]
pub struct StderrSnapshot {
    /// The newest lines, oldest first; a line still being written comes last.
    pub lines: Arc<[String]>,
    /// Lines written this run, one still being written included. More than
    /// `lines` holds once the oldest have been dropped.
    pub total: u64,
    /// Bytes written this run: what tells one reading from the next.
    pub bytes: u64,
    /// Whether new output still arrives. False once the log's watch failed.
    pub following: bool,
}

impl PartialEq for StderrSnapshot {
    /// Two readings with the same byte count hold the same lines.
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes && self.following == other.following
    }
}

#[derive(Debug, Default)]
pub struct StderrLog {
    pub latest: StderrSnapshot,
    /// `latest.total` when the user last looked.
    seen: u64,
}

impl StderrLog {
    /// Lines written since the user last looked.
    pub const fn unread(&self) -> u64 {
        self.latest.total.saturating_sub(self.seen)
    }

    /// The user is looking at everything read so far.
    pub const fn mark_seen(&mut self) {
        self.seen = self.latest.total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(total: u64) -> StderrSnapshot {
        StderrSnapshot {
            total,
            bytes: total * 10,
            following: true,
            ..StderrSnapshot::default()
        }
    }

    #[test]
    fn what_arrives_after_a_look_is_unread_until_the_next() {
        let mut log = StderrLog::default();
        assert_eq!(log.unread(), 0);
        log.latest = reading(3);
        assert_eq!(log.unread(), 3);
        log.mark_seen();
        assert_eq!(log.unread(), 0);
        log.latest = reading(5);
        assert_eq!(log.unread(), 2);
    }
}

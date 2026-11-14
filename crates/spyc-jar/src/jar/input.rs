//! The program's input: what the owner types and the replies it sends,
//! written to the PTY by a thread of its own.
//!
//! A program that stops reading its terminal fills the PTY's buffer, and a
//! write to it waits. Here only this thread waits, polling so it can still
//! park for an upgrade, and the owner's connection goes on being heard: its
//! pings, its signals, its going (review finding 4). What the program hasn't
//! taken belongs to the owner that sent it and goes with that owner, as a
//! terminal's unsent keys go when the terminal closes.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::sync::PoisonError;

use super::{Shared, TICK, Who, ready};
use crate::proto;

/// Bytes on their way to the PTY, and the connection that sent them.
#[derive(Default)]
pub struct Inbox {
    bytes: VecDeque<u8>,
    from: u64,
    /// Counts the times the inbox was emptied unwritten, so a write that
    /// raced it doesn't take bytes queued since.
    dropped: u64,
}

/// The most the inbox holds when more input arrives: past it the program
/// isn't reading, and the owner hears so.
const INBOX_CAP: usize = proto::MAX_FRAME;

/// The most written to the PTY at once.
const CHUNK: usize = 4096;

impl Shared {
    /// Queue `bytes` from connection `id` for the program. False when the
    /// program has left more than [`INBOX_CAP`] unread.
    pub(crate) fn queue_input(&self, id: u64, bytes: &[u8]) -> bool {
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        if inbox.bytes.len() > INBOX_CAP {
            return false;
        }
        inbox.from = id;
        inbox.bytes.extend(bytes);
        drop(inbox);
        self.inbox_cv.notify_all();
        true
    }

    /// Forget what connection `id` sent that the program never took.
    pub(crate) fn drop_input(&self, id: u64) {
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        if inbox.from == id && !inbox.bytes.is_empty() {
            inbox.bytes.clear();
            inbox.dropped += 1;
        }
    }

    /// Whether input is still on its way: an upgrade can't carry it.
    pub(crate) fn input_pending(&self) -> bool {
        !self
            .inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bytes
            .is_empty()
    }
}

/// Write the inbox to the PTY as the program makes room, parking for an
/// upgrade between writes.
pub(super) fn feed_pty(shared: &Shared, mut pty: File) {
    loop {
        if shared.upgrading() {
            shared.park(Who::Input);
            continue;
        }
        let (chunk, dropped): (Vec<u8>, u64) = {
            let inbox = shared.inbox.lock().unwrap_or_else(PoisonError::into_inner);
            if inbox.bytes.is_empty() {
                let _ = shared.inbox_cv.wait_timeout(inbox, TICK);
                continue;
            }
            (
                inbox.bytes.iter().take(CHUNK).copied().collect(),
                inbox.dropped,
            )
        };
        if !ready(&pty, rustix::event::PollFlags::OUT, TICK) {
            continue;
        }
        let taken = match pty.write(&chunk) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            // The program's side has closed: nothing will read it now.
            Err(_) => usize::MAX,
        };
        let mut inbox = shared.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        if inbox.dropped == dropped {
            let taken = taken.min(inbox.bytes.len());
            inbox.bytes.drain(..taken);
        }
    }
}

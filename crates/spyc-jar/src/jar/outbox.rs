//! What a connection's writer sends ahead of the log: replies, notices and
//! refusals.
//!
//! Only a connection's writer writes to its socket, so no other thread waits
//! on a client that has stopped reading, and no frame is ever written into
//! the middle of another. A lookup has no writer: its one thread asks one
//! thing and writes the answer itself.

use std::collections::VecDeque;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use super::{Conn, Shared};
use crate::proto::{self, Msg, Role};

/// Encoded frames waiting for a connection's writer.
#[derive(Default)]
pub struct Outbox {
    frames: VecDeque<Vec<u8>>,
    bytes: usize,
}

/// The most a connection's outbox holds when a frame is added: a client that
/// has left this much unread loses its connection.
const OUTBOX_CAP: usize = proto::MAX_FRAME;

impl Conn {
    /// Send `msg` on this connection. A client that has let its outbox fill
    /// is closed, and a lookup that won't take its answer too.
    pub(crate) fn send(&self, shared: &Shared, msg: &Msg) {
        // SPYC-TRAP(jar-one-writer): only the writer writes a connection's
        // socket; a thread that writes it under a lock wedges the jar.
        if self.role == Role::Lookup {
            let mut sock = &*self.sock;
            if proto::write_msg(&mut sock, msg).is_err() {
                self.close();
            }
            return;
        }
        let frame = proto::encode(msg);
        {
            let mut out = self.outbox.lock().unwrap_or_else(PoisonError::into_inner);
            if out.bytes > OUTBOX_CAP {
                drop(out);
                self.close();
                return;
            }
            out.bytes += frame.len();
            out.frames.push_back(frame);
        }
        // The writer looks at its outbox under the state's lock, then waits
        // on the jar's condition: taking that lock here means the wake can't
        // fall between the two.
        drop(shared.lock());
        shared.cv.notify_all();
    }

    /// Put back, ahead of everything, what a flush couldn't write: the rest
    /// of a frame already begun, then the frames after it. The writer sends
    /// it first, so the frames run on unbroken.
    pub(crate) fn put_back(&self, bytes: Vec<u8>) {
        let mut out = self.outbox.lock().unwrap_or_else(PoisonError::into_inner);
        out.bytes += bytes.len();
        out.frames.push_front(bytes);
    }

    /// Everything waiting in the outbox, in order, as one write.
    pub(crate) fn take_outbox(&self) -> Vec<u8> {
        let mut out = self.outbox.lock().unwrap_or_else(PoisonError::into_inner);
        out.bytes = 0;
        out.frames.drain(..).flatten().collect()
    }

    /// End the connection: its reader and writer see the socket shut, and
    /// any write blocked on it returns.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
    }
}

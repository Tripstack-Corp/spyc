//! The client's end of a jar's socket: connect, `hello`, then send and receive
//! frames. One thread may read while another writes.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::proto::{self, Msg, Role};

pub struct Client {
    read: std::io::BufReader<UnixStream>,
    timeout: Option<Duration>,
    write: Arc<Mutex<UnixStream>>,
}

impl Client {
    /// Connect to the jar at `socket` as `role`, retrying until `wait` passes
    /// so a jar still starting is waited for. Returns the jar's `Welcome`, or
    /// its refusal as an error.
    pub fn connect(socket: &Path, role: Role, wait: Duration) -> anyhow::Result<(Self, Msg)> {
        let until = Instant::now() + wait;
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(s) => break s,
                Err(e) if Instant::now() >= until => {
                    return Err(anyhow::anyhow!("connect {}: {e}", socket.display()));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        let mut client = Self {
            read: std::io::BufReader::with_capacity(1 << 16, stream.try_clone()?),
            timeout: None,
            write: Arc::new(Mutex::new(stream)),
        };
        client.send(&Msg::Hello {
            version: proto::VERSION,
            role,
        })?;
        match client.recv(Duration::from_secs(5))? {
            Some(w @ Msg::Welcome { .. }) => Ok((client, w)),
            Some(Msg::Refused { why }) => Err(anyhow::anyhow!("refused: {why}")),
            other => Err(anyhow::anyhow!("expected a welcome, got {other:?}")),
        }
    }

    pub fn send(&self, msg: &Msg) -> io::Result<()> {
        let mut w = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        proto::write_msg(&mut *w, msg)
    }

    /// Bytes written as they are: a frame this version can't build, as a
    /// newer client would send it.
    pub fn send_raw(&self, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write as _;
        let mut w = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        w.write_all(bytes)?;
        w.flush()
    }

    /// The next frame, or `None` when `wait` passes first.
    pub fn recv(&mut self, wait: Duration) -> anyhow::Result<Option<Msg>> {
        if self.timeout != Some(wait) {
            self.read.get_ref().set_read_timeout(Some(wait))?;
            self.timeout = Some(wait);
        }
        loop {
            match proto::read_msg(&mut self.read) {
                Ok(Some(m)) => return Ok(Some(m)),
                Ok(None) => continue,
                Err(proto::ReadError::Io(e))
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Close both directions, as a crash would.
    pub fn drop_connection(self) {
        let _ = self.read.get_ref().shutdown(std::net::Shutdown::Both);
    }

    /// Split into the receiving half, for one reading thread, and a sending
    /// half any thread may clone.
    pub fn split(self) -> (Receiver, Sender) {
        let write = self_write(&self);
        (Receiver(self), Sender(write))
    }
}

fn self_write(c: &Client) -> Arc<Mutex<UnixStream>> {
    Arc::clone(&c.write)
}

/// The receiving half of a [`Client`].
pub struct Receiver(Client);

impl Receiver {
    pub fn recv(&mut self, wait: Duration) -> anyhow::Result<Option<Msg>> {
        self.0.recv(wait)
    }
}

/// The sending half of a [`Client`].
#[derive(Clone, Debug)]
pub struct Sender(Arc<Mutex<UnixStream>>);

impl Sender {
    pub fn send(&self, msg: &Msg) -> io::Result<()> {
        let mut w = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        proto::write_msg(&mut *w, msg)
    }

    /// Close the connection, which ends the receiving half too.
    pub fn close(&self) {
        let w = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = w.shutdown(std::net::Shutdown::Both);
    }
}

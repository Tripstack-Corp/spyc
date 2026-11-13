//! The client's end of a jar's socket: connect, `hello`, then send and receive
//! frames. One thread may read while another writes.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::proto::{self, Msg, Role};

/// A jar's refusal of a `hello`: its reason (`proto::reason`) and text. A
/// caller tells "another spyc owns it" from "this token is stale" by the
/// reason, through `anyhow::Error::downcast_ref`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: u16,
    pub why: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "refused: {}", self.why)
    }
}

impl std::error::Error for Refusal {}

pub struct Client {
    read: std::io::BufReader<UnixStream>,
    timeout: Option<Duration>,
    write: Arc<Mutex<UnixStream>>,
}

impl Client {
    /// Connect to the jar at `socket` as `role`, presenting its `token`
    /// (R26), and retrying until `wait` passes so a jar still starting is
    /// waited for. Returns the jar's `Welcome`, or its refusal as an error.
    pub fn connect(
        socket: &Path,
        role: Role,
        wait: Duration,
        token: &[u8],
    ) -> anyhow::Result<(Self, Msg)> {
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
            token: token.to_vec(),
        })?;
        match client.recv(Duration::from_secs(5))? {
            Some(w @ Msg::Welcome { .. }) => Ok((client, w)),
            Some(Msg::Refused { reason, why }) => Err(Refusal { reason, why }.into()),
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

/// R31: ask the jar at `socket` to describe itself, as a [`Role::Lookup`],
/// which needs no token.
pub fn describe(socket: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let (mut c, _) = Client::connect(socket, Role::Lookup, Duration::from_secs(2), &[])?;
    c.send(&Msg::Describe)?;
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        if let Some(Msg::Description { fields }) = c.recv(Duration::from_millis(200))? {
            return Ok(fields);
        }
    }
    anyhow::bail!("{} didn't describe itself", socket.display())
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
    /// Send over `client`'s connection from now on, every clone of this
    /// sender included, and return its receiving half: a reconnect that the
    /// rest of the client never sees.
    pub fn rebind(&self, client: Client) -> io::Result<Receiver> {
        let stream = client
            .write
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .try_clone()?;
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = stream;
        Ok(Receiver(client))
    }
}

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

/// A jar's token as a file holds it: the text, without surrounding
/// whitespace.
pub fn read_token_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    let text = std::fs::read(path).map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    Ok(text.trim_ascii().to_vec())
}

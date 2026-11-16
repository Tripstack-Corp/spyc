//! The client's end of a jar's socket: connect, `hello`, then send and receive
//! frames. One thread may read while another writes.
//!
//! An owner that must never wait on its jar splits into an [`Outbox`]: frames
//! queue for a writer thread of their own, which pings whenever it has been
//! idle, so a jar that stops reading stalls nothing but that thread
//! (finding 4).

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
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

/// The least a connection waits for its welcome.
const WELCOME_WAIT: Duration = Duration::from_secs(5);

impl Client {
    /// Connect to the jar at `socket` as `role`, presenting its `token`
    /// (R26), and retrying until `wait` passes so a jar still starting is
    /// waited for. Returns the jar's `Welcome`, or its refusal as an error.
    /// The welcome is waited for as long as `wait`, and never less than
    /// [`WELCOME_WAIT`]: a jar that accepted may still be busy.
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
        match client.recv(wait.max(WELCOME_WAIT))? {
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
                Ok(None) => {}
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

impl Client {
    /// Split into the receiving half and an [`Outbox`] whose writer pings
    /// after `idle` with nothing to send, and gives up on a jar whose socket
    /// takes no bytes for `stall`.
    pub fn split_queued(self, idle: Duration, stall: Duration) -> io::Result<(Receiver, Outbox)> {
        let sock = Arc::new(
            self.write
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .try_clone()?,
        );
        sock.set_write_timeout(Some(stall))?;
        let (queue, frames) = mpsc::channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let writer = (Arc::clone(&sock), Arc::clone(&queued));
        std::thread::Builder::new()
            .name("jar-outbox".into())
            .spawn(move || write_queued(&writer.0, &frames, &writer.1, idle))?;
        Ok((
            Receiver(self),
            Outbox {
                queue,
                sock,
                queued,
            },
        ))
    }
}

enum Queued {
    Frame(Vec<u8>),
    Close,
}

/// The sending half of an owner's connection: [`Outbox::send`] queues and
/// returns, and one writer thread does every write.
#[derive(Clone, Debug)]
pub struct Outbox {
    queue: mpsc::Sender<Queued>,
    sock: Arc<UnixStream>,
    queued: Arc<AtomicUsize>,
}

impl std::fmt::Debug for Queued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frame(b) => write!(f, "Frame({} bytes)", b.len()),
            Self::Close => f.write_str("Close"),
        }
    }
}

impl Outbox {
    /// Queue `msg`. An error only when it is over the frame limit or the
    /// connection has ended.
    pub fn send(&self, msg: &Msg) -> io::Result<()> {
        let frame = proto::encode(msg);
        if frame.len() - 4 > proto::MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("a frame of {} bytes, over the limit", frame.len() - 4),
            ));
        }
        let len = frame.len();
        self.queued.fetch_add(len, Ordering::AcqRel);
        self.queue.send(Queued::Frame(frame)).map_err(|_| {
            self.queued.fetch_sub(len, Ordering::AcqRel);
            io::Error::from(io::ErrorKind::BrokenPipe)
        })
    }

    /// The bytes queued and not yet written.
    pub fn queued(&self) -> usize {
        self.queued.load(Ordering::Acquire)
    }

    /// End the connection once what is queued has been written.
    pub fn close(&self) {
        if self.queue.send(Queued::Close).is_err() {
            let _ = self.sock.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// The writer: each queued frame in order, a ping after `idle` with none,
/// and the connection shut once a write fails or times out.
fn write_queued(
    sock: &UnixStream,
    frames: &mpsc::Receiver<Queued>,
    queued: &AtomicUsize,
    idle: Duration,
) {
    use std::io::Write as _;
    let mut pings = 0u64;
    loop {
        let (frame, counted) = match frames.recv_timeout(idle) {
            Ok(Queued::Frame(f)) => {
                let n = f.len();
                (f, n)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                pings += 1;
                (proto::encode(&Msg::Ping { n: pings }), 0)
            }
            Ok(Queued::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let written = (&*sock).write_all(&frame);
        queued.fetch_sub(counted, Ordering::AcqRel);
        if written.is_err() {
            break;
        }
    }
    let _ = sock.shutdown(std::net::Shutdown::Both);
}

fn self_write(c: &Client) -> Arc<Mutex<UnixStream>> {
    Arc::clone(&c.write)
}

/// Whether something accepts a connection at `socket` at once.
///
/// It never waits, so a socket whose backlog is full (a jar not accepting)
/// is answered for without the hang a blocking connect has on Linux. A
/// refusal says nothing on its own: macOS refuses a full backlog too.
pub fn answers_now(socket: &Path) -> bool {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketType};
    let Ok(addr) = SocketAddrUnix::new(socket) else {
        return false;
    };
    let Ok(sock) = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None) else {
        return false;
    };
    if rustix::io::ioctl_fionbio(&sock, true).is_err() {
        return false;
    }
    rustix::net::connect(&sock, &addr).is_ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A client on one end of a socket pair, and the jar's end.
    fn pair() -> (Client, UnixStream) {
        let (ours, jar) = UnixStream::pair().unwrap();
        let client = Client {
            read: std::io::BufReader::new(ours.try_clone().unwrap()),
            timeout: None,
            write: Arc::new(Mutex::new(ours)),
        };
        (client, jar)
    }

    fn next(jar: &mut UnixStream) -> Option<Msg> {
        jar.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        proto::read_msg(jar).unwrap_or_default()
    }

    /// Finding 4: a jar that reads nothing holds up no sender. Sixty-four
    /// MiB is far past what the socket's buffer takes.
    #[test]
    fn a_send_returns_while_the_jar_reads_nothing() {
        let (client, _jar) = pair();
        let (_rx, out) = client
            .split_queued(Duration::from_secs(60), Duration::from_secs(60))
            .unwrap();
        let at = Instant::now();
        for _ in 0..64 {
            out.send(&Msg::Input {
                bytes: vec![b'x'; 1 << 20],
            })
            .unwrap();
        }
        assert!(
            at.elapsed() < Duration::from_secs(2),
            "sends waited on the socket: {:?}",
            at.elapsed()
        );
        assert!(out.queued() > 32 << 20, "the frames wait in the queue");
    }

    /// Pings keep the jar's deadline when the owner has nothing to say.
    #[test]
    fn an_idle_outbox_pings() {
        let (client, mut jar) = pair();
        let (_rx, _out) = client
            .split_queued(Duration::from_millis(50), Duration::from_secs(5))
            .unwrap();
        assert_eq!(next(&mut jar), Some(Msg::Ping { n: 1 }));
        assert_eq!(next(&mut jar), Some(Msg::Ping { n: 2 }));
    }

    #[test]
    fn close_writes_what_was_queued_first() {
        let (client, mut jar) = pair();
        let (_rx, out) = client
            .split_queued(Duration::from_secs(60), Duration::from_secs(5))
            .unwrap();
        out.send(&Msg::Ack { offset: 1 }).unwrap();
        out.send(&Msg::Ack { offset: 2 }).unwrap();
        out.close();
        assert_eq!(next(&mut jar), Some(Msg::Ack { offset: 1 }));
        assert_eq!(next(&mut jar), Some(Msg::Ack { offset: 2 }));
        assert_eq!(next(&mut jar), None, "then the connection ends");
    }

    /// A jar that takes no bytes for `stall` loses the connection, and the
    /// sender hears so rather than queueing for ever.
    #[test]
    fn a_stalled_jar_ends_the_connection() {
        let (client, _jar) = pair();
        let (_rx, out) = client
            .split_queued(Duration::from_secs(60), Duration::from_millis(200))
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let mut ended = false;
        while Instant::now() < until {
            if out
                .send(&Msg::Input {
                    bytes: vec![b'x'; 1 << 16],
                })
                .is_err()
            {
                ended = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            ended,
            "a send still queued 5 s after the jar stopped reading"
        );
    }

    #[test]
    fn a_frame_over_the_limit_is_refused_at_once() {
        let (client, _jar) = pair();
        let (_rx, out) = client
            .split_queued(Duration::from_secs(60), Duration::from_secs(5))
            .unwrap();
        let e = out
            .send(&Msg::Input {
                bytes: vec![0; proto::MAX_FRAME],
            })
            .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(out.queued(), 0);
    }
}

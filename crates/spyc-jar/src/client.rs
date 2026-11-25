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

/// How long the rest of a frame may take once its first byte is in, however
/// short the caller's wait: a jar mid-frame is writing, and only one gone
/// silent that long is given up on.
const REST_OF_FRAME: Duration = Duration::from_secs(3600);

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
        Self::connect_waiting(socket, role, wait, wait.max(WELCOME_WAIT), token)
    }

    /// [`Self::connect`], with the welcome waited for no longer than `wait`
    /// either: for a question whose asker can't wait, and asks again later.
    pub fn connect_within(
        socket: &Path,
        role: Role,
        wait: Duration,
        token: &[u8],
    ) -> anyhow::Result<(Self, Msg)> {
        Self::connect_waiting(socket, role, wait, wait, token)
    }

    fn connect_waiting(
        socket: &Path,
        role: Role,
        wait: Duration,
        welcome: Duration,
        token: &[u8],
    ) -> anyhow::Result<(Self, Msg)> {
        let until = Instant::now() + wait;
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(s) => break s,
                Err(e) if Instant::now() >= until || gone(socket, &e) => {
                    return Err(anyhow::anyhow!("connect {}: {e}", socket.display()));
                }
                Err(_) => std::thread::sleep(CONNECT_RETRY),
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
        match client.recv(welcome)? {
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

    /// The next frame, or `None` when `wait` passes before one begins. Once
    /// a frame's first byte is in, the rest is read under [`REST_OF_FRAME`]:
    /// a wait that ran out partway through would lose what was read of it.
    pub fn recv(&mut self, wait: Duration) -> anyhow::Result<Option<Msg>> {
        use std::io::BufRead as _;
        let until = Instant::now() + wait;
        let mut left = wait;
        // SPYC-TRAP(jar-frame-whole): the wait bounds a frame's start, never
        // its rest; a timeout mid-frame loses what was read of it.
        loop {
            if self.read.buffer().is_empty() {
                // A socket takes no zero timeout: the least is a millisecond.
                // macOS refuses any timeout on a socket its peer has closed
                // (a lookup's ends with its answer), whose reads don't wait.
                if let Err(e) = self.read_timeout(left.max(crate::limits::SHORTEST_WAIT))
                    && !self.reads_at_once()
                {
                    return Err(e.into());
                }
                match self.read.fill_buf() {
                    Ok([]) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                    Ok(_) => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        return Ok(None);
                    }
                    // B12: Linux fails a read with a timeout that a signal
                    // interrupts, `SA_RESTART` or not, and every one a stop
                    // and continue finds waiting. The connection is as it
                    // was: the wait goes on to its end, never past it.
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                        left = until.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            return Ok(None);
                        }
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            // macOS refuses a timeout on a socket its peer has closed, and
            // then the rest of the frame is in the buffer or never coming.
            let _ = self.read_timeout(wait.max(REST_OF_FRAME));
            match proto::read_msg(&mut self.read) {
                Ok(Some(m)) => return Ok(Some(m)),
                Ok(None) => left = wait,
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Whether a read returns at once: bytes are waiting, or the peer has
    /// gone.
    fn reads_at_once(&self) -> bool {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        let mut fds = [PollFd::new(self.read.get_ref(), PollFlags::IN)];
        let now = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        poll(&mut fds, Some(&now)).is_ok_and(|n| n > 0)
    }

    fn read_timeout(&mut self, wait: Duration) -> io::Result<()> {
        if self.timeout != Some(wait) {
            self.read.get_ref().set_read_timeout(Some(wait))?;
            self.timeout = Some(wait);
        }
        Ok(())
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
        crate::threads::start("jar-outbox", move || {
            write_queued(&writer.0, &frames, &writer.1, idle);
        })?;
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

    /// End the connection now, as a crash would: nothing still queued is
    /// written, and the jar sees it close however much was waiting.
    pub fn abort(&self) {
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
        let _ = self.queue.send(Queued::Close);
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

/// Whether a refused connect to `socket` means its jar has gone: nothing
/// holds its lock, so no jar runs there, and trying again would only wait
/// out the deadline. A jar still starting holds its lock before it listens.
fn gone(socket: &Path, e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::ConnectionRefused
        && crate::lock::state(socket) == crate::lock::LockState::Free
}

/// How long [`describe`] waits to reach a jar, and then for its answer.
const DESCRIBE_WAIT: Duration = Duration::from_secs(2);
/// How often [`describe`] looks for the answer.
const DESCRIBE_POLL: Duration = Duration::from_millis(200);
/// How often a client tries again to reach a jar it couldn't yet.
const CONNECT_RETRY: Duration = Duration::from_millis(10);

/// R31: ask the jar at `socket` to describe itself, as a [`Role::Lookup`],
/// which needs no token.
pub fn describe(socket: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let (mut c, _) = Client::connect(socket, Role::Lookup, DESCRIBE_WAIT, &[])?;
    c.send(&Msg::Describe)?;
    let until = Instant::now() + DESCRIBE_WAIT;
    while Instant::now() < until {
        if let Some(Msg::Description { fields }) = c.recv(DESCRIBE_POLL)? {
            return Ok(fields);
        }
    }
    anyhow::bail!("{} didn't describe itself", socket.display())
}

/// [`describe`], each wait bounded by `wait`: an error when the jar doesn't
/// answer in time.
pub fn describe_within(socket: &Path, wait: Duration) -> anyhow::Result<Vec<(String, String)>> {
    let until = Instant::now() + wait;
    let (mut c, _) = Client::connect_within(socket, Role::Lookup, wait, &[])?;
    c.send(&Msg::Describe)?;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            anyhow::bail!("{} didn't describe itself", socket.display());
        }
        if let Some(Msg::Description { fields }) = c.recv(left.min(DESCRIBE_POLL))? {
            return Ok(fields);
        }
    }
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

    /// A refused connect to a jar whose lock is free gives up then: no jar
    /// runs there, and retrying only waited out the deadline (20 s for a
    /// tab taken back), so a tab whose jar died was marked `[jar lost]`
    /// that much later, and under load past what a caller waits.
    #[test]
    fn a_connect_to_a_jar_that_has_gone_gives_up_at_once() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("jar-gone-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("g.sock");
        // A jar that died: its socket's path and its lock file are left,
        // nothing listens, and nothing holds the lock.
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        std::fs::write(crate::lock::path(&socket), b"").unwrap();
        assert_eq!(
            crate::lock::state(&socket),
            crate::lock::LockState::Free,
            "premise"
        );
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let tried = Client::connect(&socket, Role::Lookup, Duration::from_secs(3600), b"");
            let _ = tx.send(tried.map(|_| ()).map_err(|e| format!("{e:#}")));
        });
        let tried = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the connect to a jar that has gone still waits");
        assert!(tried.is_err(), "{tried:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A jar that answered and closed before the client asked again (a
    /// lookup's, which ends with its one answer) still has its answer read:
    /// macOS refuses a new timeout on a socket its peer has closed, and that
    /// is no reason to lose what's in it.
    #[test]
    fn an_answer_from_a_jar_that_closed_is_still_read() {
        let (mut client, mut jar) = pair();
        assert_eq!(client.recv(Duration::from_millis(50)).unwrap(), None);
        proto::write_msg(&mut jar, &Msg::Pong { n: 7 }).unwrap();
        drop(jar);
        // Another wait than the last: the socket is asked for a new timeout.
        assert_eq!(
            client.recv(Duration::from_millis(200)).unwrap(),
            Some(Msg::Pong { n: 7 })
        );
    }

    /// Finding 4: a jar that reads nothing holds up no sender. Sixty-four
    /// MiB is far past what the socket's buffer takes.
    #[test]
    fn a_send_returns_while_the_jar_reads_nothing() {
        let (client, _jar) = pair();
        let (_rx, out) = client
            .split_queued(Duration::from_secs(60), Duration::from_secs(60))
            .unwrap();
        // Every send returns with the jar reading nothing, and what the
        // socket can't take waits in the queue: a send that waited on the
        // socket would hold here until the stall ended the connection.
        for _ in 0..64 {
            out.send(&Msg::Input {
                bytes: vec![b'x'; 1 << 20],
            })
            .unwrap();
        }
        assert!(out.queued() > 32 << 20, "the frames wait in the queue");
    }

    /// A wait that runs out partway through a frame loses none of it: the
    /// wait bounds a frame's start, never its rest. A zero wait is a wait
    /// that runs out, not an error.
    #[test]
    fn a_frame_split_across_a_wait_arrives_whole() {
        use std::io::Write as _;
        let (mut client, mut jar) = pair();
        assert_eq!(client.recv(Duration::ZERO).unwrap(), None);
        let msg = Msg::Output {
            offset: 7,
            bytes: b"split across the wait".to_vec(),
        };
        let frame = proto::encode(&msg);
        jar.write_all(&frame[..3]).unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            jar.write_all(&frame[3..]).unwrap();
            std::thread::sleep(Duration::from_secs(5));
        });
        assert_eq!(
            client.recv(Duration::from_millis(50)).unwrap(),
            Some(msg),
            "begun inside the wait, finished after it"
        );
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

    /// An aborted connection ends at once, what was queued unwritten: the
    /// jar hears it close, not the frames still waiting behind a full socket.
    #[test]
    fn abort_ends_the_connection_at_once() {
        let (client, mut jar) = pair();
        let (_rx, out) = client
            .split_queued(Duration::from_secs(60), Duration::from_secs(60))
            .unwrap();
        for _ in 0..16 {
            out.send(&Msg::Input {
                bytes: vec![b'x'; 1 << 20],
            })
            .unwrap();
        }
        out.abort();
        jar.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut read = 0usize;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            match std::io::Read::read(&mut jar, &mut buf) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(e) => panic!("the connection didn't end: {e}"),
            }
        }
        assert!(read < 16 << 20, "what was queued was written: {read} bytes");
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

//! The attach client, which relays a real terminal to a jar.
//!
//! `spyc-jar attach SOCKET TOKEN_FILE` reaches a jar as a terminal or `ssh host
//! spyc-jar attach …` would reach spyc's outer jar. It puts the terminal in raw mode, sends what is typed as input, writes
//! output back, and on detach (`^\`) or the program's exit sends a fixed reset:
//! a relay that doesn't parse can't know which modes the program turned on.
//! What was typed before it attached is dropped, as typed blind.
//!
//! Its frames go out through an [`Outbox`](crate::client::Outbox), whose
//! writer pings the jar whenever it has had nothing to send. A write to a
//! terminal that has stopped reading (a terminal app paused while its
//! computer sleeps) waits on the output thread alone, and the jar still
//! hears from its owner inside the deadline.
//!
//! A client that isn't running can't ping: stopped, or a whole computer
//! asleep but for one of its background wakes. The jar drops it once its
//! deadline passes and records why (`owner_left`), and the client, running
//! again, finds its connection closed, asks why, and attaches again when the
//! jar dropped it for silence. Any other end, a detach among them, ends it.
//! It replays from what the terminal last showed, so nothing printed
//! meanwhile is lost, and spyc, told a terminal attached, repaints.

use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use anyhow::Context;
use rustix::termios::{self, OptionalActions};

use crate::client::{Client, Outbox, Receiver, Refusal};
use crate::proto::{Msg, Role, reason};
use crate::state::{OWNER_LEFT, OwnerLeft};

/// How long `attach` waits to reach its jar.
const ATTACH_WAIT: Duration = Duration::from_secs(5);
/// How often an attached client with nothing to send pings its jar.
const ATTACH_PING: Duration = Duration::from_secs(2);
/// A jar whose socket takes no bytes for this long has stopped: past the
/// longest it reads nothing, parked for an upgrade.
const WRITE_STALL: Duration = Duration::from_secs(20);

// SPYC-TRAP(jar-outwait): the client is heard from inside the jar's deadline,
// and outwaits the jar's longest silence before taking it for gone.
const _: () = {
    assert!(ATTACH_PING.as_millis() < crate::limits::OWNER_DEADLINE.as_millis());
    assert!(WRITE_STALL.as_millis() > crate::limits::PARKED_AT_MOST.as_millis());
};

/// How often an attached client looks at its terminal's size.
const SIZE_POLL: Duration = Duration::from_millis(250);

/// How soon a client the jar dropped for silence asks again, while the jar
/// still holds its old lease.
const RECONNECT_RETRY: Duration = Duration::from_millis(50);

/// `^\`, as dtach uses.
const DETACH: u8 = 0x1c;

/// Leave the alternate screen; end mouse reporting, alternate scroll and
/// bracketed paste; pop the kitty keyboard flags; show the cursor; reset SGR.
const RESET: &[u8] =
    b"\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1007l\x1b[?2004l\x1b[<10u\x1b[?25h\x1b[0m";

/// Attach this terminal to the jar at `socket` until detach or exit.
pub fn run(socket: &Path, token: &[u8]) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let saved = termios::tcgetattr(stdin.as_fd())?;
    let mut raw = saved.clone();
    raw.make_raw();
    termios::tcsetattr(stdin.as_fd(), OptionalActions::Now, &raw)?;
    // Keys typed before the program could be seen were typed blind: they
    // would land wherever its focus is now. Drop them, never queue them.
    let _ = termios::tcflush(stdin.as_fd(), termios::QueueSelector::IFlush);
    let ended = relay(socket, token);
    let mut out = std::io::stdout();
    let _ = out.write_all(RESET);
    let _ = out.flush();
    let _ = termios::tcsetattr(stdin.as_fd(), OptionalActions::Now, &saved);
    let why = ended?;
    eprintln!("[{why}]");
    Ok(())
}

/// The key the attached client sets in its jar (R18): `ssh` when this
/// terminal reaches its machine over SSH, else `local`.
pub const TERMINAL_KEY: &str = "terminal";

/// [`TERMINAL_KEY`]'s value for a terminal that is, or isn't, over SSH.
pub const fn terminal_kind(ssh: bool) -> &'static str {
    if ssh { "ssh" } else { "local" }
}

fn size() -> (u16, u16) {
    termios::tcgetwinsize(std::io::stdout().as_fd()).map_or((24, 80), |w| (w.ws_row, w.ws_col))
}

const fn resize((rows, cols): (u16, u16)) -> Msg {
    Msg::Resize {
        rows,
        cols,
        px_w: 0,
        px_h: 0,
    }
}

/// What ends a connection, told to the relay by the thread that saw it.
enum Ended {
    /// The relay is over, and why.
    Leave(&'static str),
    /// Connection `n` closed under the client.
    Lost(u64),
}

/// The connection keys and sizes go out on, `None` between connections.
type Link = Arc<Mutex<Option<Outbox>>>;

fn lock(link: &Link) -> std::sync::MutexGuard<'_, Option<Outbox>> {
    link.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Relay until detach, exit or a lost connection; why it ended. A
/// connection the jar dropped for silence is made again.
fn relay(socket: &Path, token: &[u8]) -> anyhow::Result<&'static str> {
    let (mut client, welcome) = Client::connect(socket, Role::Owner, ATTACH_WAIT, token)?;
    let Msg::Welcome { head, .. } = welcome else {
        anyhow::bail!("expected a welcome");
    };
    // Where the terminal is in the program's output, kept across connections.
    let written = Arc::new(AtomicU64::new(head));
    let link: Link = Arc::default();
    let (ended_tx, ended) = mpsc::channel();
    relay_input(&link, ended_tx.clone())?;
    relay_size(&link)?;
    for n in 0.. {
        let tx = start(client, n, &written, &ended_tx)?;
        *lock(&link) = Some(tx.clone());
        let end = loop {
            match ended.recv() {
                Ok(Ended::Lost(k)) if k != n => {}
                Ok(end) => break end,
                Err(_) => break Ended::Leave("ended"),
            }
        };
        lock(&link).take();
        tx.close();
        match end {
            Ended::Leave(why) => return Ok(why),
            Ended::Lost(_) => match reconnect(socket, token, &ended) {
                Ok(next) => client = next,
                Err(why) => return Ok(why),
            },
        }
    }
    Ok("ended")
}

/// Begin connection `n`: name the terminal, replay what it hasn't shown, give
/// its size, and relay the output on a thread of its own.
fn start(
    client: Client,
    n: u64,
    written: &Arc<AtomicU64>,
    ended: &mpsc::Sender<Ended>,
) -> anyhow::Result<Outbox> {
    let (rx, tx) = client.split_queued(ATTACH_PING, WRITE_STALL)?;
    // What kind of terminal this is, sent before any input: the program reads
    // it once a terminal attaches (spyc routes OSC 52 and 9 by it).
    tx.send(&Msg::SetInfo {
        key: TERMINAL_KEY.into(),
        value: terminal_kind(std::env::var_os("SSH_CONNECTION").is_some()).into(),
    })?;
    tx.send(&Msg::Replay {
        from: written.load(Ordering::SeqCst),
    })?;
    // The jar tells its program this owner attached (`attached`), and the
    // program repaints for it; this terminal's size is all it needs from here.
    tx.send(&resize(size()))?;
    relay_output(rx, tx.clone(), n, Arc::clone(written), ended.clone())?;
    Ok(tx)
}

/// Output to the terminal, acknowledged once written; what ends connection
/// `n` is told to the relay.
fn relay_output(
    mut rx: Receiver,
    tx: Outbox,
    n: u64,
    written: Arc<AtomicU64>,
    ended: mpsc::Sender<Ended>,
) -> anyhow::Result<()> {
    crate::threads::start("attach-output", move || {
        let mut out = std::io::stdout();
        let end = loop {
            match rx.recv(ATTACH_PING) {
                Ok(Some(Msg::Output { offset, bytes })) => {
                    if out.write_all(&bytes).and_then(|()| out.flush()).is_err() {
                        break Ended::Leave("the terminal went away");
                    }
                    let shown = offset + bytes.len() as u64;
                    written.store(shown, Ordering::SeqCst);
                    let _ = tx.send(&Msg::Ack { offset: shown });
                }
                Ok(Some(Msg::Exited { .. })) => {
                    let _ = tx.send(&Msg::AckExit);
                    break Ended::Leave("the program exited");
                }
                // The outbox pings while there is nothing to send.
                Ok(Some(_) | None) => {}
                Err(_) => break Ended::Lost(n),
            }
        };
        let _ = ended.send(end);
    })
    .context("a thread for the program's output")?;
    Ok(())
}

/// Input to the program; `^\` detaches. Keys typed between connections
/// were typed blind, and go nowhere.
fn relay_input(link: &Link, ended: mpsc::Sender<Ended>) -> anyhow::Result<()> {
    let link = Arc::clone(link);
    crate::threads::start("attach-input", move || {
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 4096];
        let why = loop {
            let n = match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break "input closed",
                Ok(n) => n,
            };
            let detach = buf[..n].iter().position(|b| *b == DETACH);
            let keys = &buf[..detach.unwrap_or(n)];
            if !keys.is_empty()
                && let Some(tx) = lock(&link).as_ref()
            {
                let _ = tx.send(&Msg::Input {
                    bytes: keys.to_vec(),
                });
            }
            if detach.is_some() {
                break "detached";
            }
        };
        let _ = ended.send(Ended::Leave(why));
    })
    .context("a thread for the terminal's input")?;
    Ok(())
}

/// Size changes, polled: a SIGWINCH handler would need `unsafe`.
fn relay_size(link: &Link) -> anyhow::Result<()> {
    let link = Arc::clone(link);
    crate::threads::start("attach-size", move || {
        let mut last = size();
        loop {
            std::thread::sleep(SIZE_POLL);
            let now = size();
            if now != last {
                last = now;
                if let Some(tx) = lock(&link).as_ref() {
                    let _ = tx.send(&resize(now));
                }
            }
        }
    })
    .context("a thread for the terminal's size")?;
    Ok(())
}

/// A lost connection: attach again when the jar dropped this client for
/// silence (`owner_left` is `silent`), as when the computer slept with the
/// client stopped; else, or when it can't, why the relay ends. A client
/// that sends is heard inside the deadline, so each loss took the jar's
/// whole deadline of silence, and the reconnects can't spin.
fn reconnect(
    socket: &Path,
    token: &[u8],
    ended: &mpsc::Receiver<Ended>,
) -> Result<Client, &'static str> {
    let left = crate::client::describe(socket).ok().and_then(|fields| {
        fields
            .into_iter()
            .find(|(k, _)| k == OWNER_LEFT)
            .and_then(|(_, v)| OwnerLeft::from_word(&v))
    });
    match left {
        Some(OwnerLeft::Silent) => {}
        Some(OwnerLeft::Detached) => return Err("detached"),
        Some(OwnerLeft::Closed) | None => return Err("the connection closed"),
    }
    // The jar names the reason before it releases the lease, so it may still
    // hold this client's for a moment.
    let until = Instant::now() + ATTACH_WAIT;
    loop {
        if let Ok(Ended::Leave(why)) = ended.try_recv() {
            return Err(why);
        }
        let refused = match Client::connect(socket, Role::Owner, ATTACH_WAIT, token) {
            Ok((client, Msg::Welcome { .. })) => return Ok(client),
            Ok(_) => return Err("the connection closed"),
            Err(e) => e.downcast_ref::<Refusal>().map(|r| r.reason),
        };
        match refused {
            Some(reason::OWNER_PRESENT) if Instant::now() < until => {
                std::thread::sleep(RECONNECT_RETRY);
            }
            Some(reason::OWNER_PRESENT) => return Err("another terminal attached"),
            Some(reason::PROGRAM_EXITED) => return Err("the program exited"),
            _ => return Err("the connection closed"),
        }
    }
}

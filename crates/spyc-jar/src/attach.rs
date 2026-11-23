//! The attach client, which relays a real terminal to a jar.
//!
//! `spyc-jar attach SOCKET TOKEN_FILE` reaches a jar as a terminal or `ssh host
//! spyc-jar attach …` would reach spyc's outer jar. It puts the terminal in raw mode, sends what is typed as input, writes
//! output back, and on detach (`^\`) or the program's exit sends a fixed reset:
//! a relay that doesn't parse can't know which modes the program turned on.
//! What was typed before it attached is dropped, as typed blind.

use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::path::Path;
use std::time::Duration;

use rustix::termios::{self, OptionalActions};

use crate::client::Client;
use crate::proto::{Msg, Role};

/// How long `attach` waits to reach its jar.
const ATTACH_WAIT: Duration = Duration::from_secs(5);
/// How often an attached client with nothing to show pings its jar.
const ATTACH_PING: Duration = Duration::from_secs(2);
/// How often an attached client looks at its terminal's size.
const SIZE_POLL: Duration = Duration::from_millis(250);

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

/// Relay until detach, exit or a lost connection; why it ended.
fn relay(socket: &Path, token: &[u8]) -> anyhow::Result<&'static str> {
    let (client, welcome) = Client::connect(socket, Role::Owner, ATTACH_WAIT, token)?;
    let Msg::Welcome { head, .. } = welcome else {
        anyhow::bail!("expected a welcome");
    };
    let (mut rx, tx) = client.split();
    // What kind of terminal this is, sent before any input: the program reads
    // it once a terminal attaches (spyc routes OSC 52 and 9 by it).
    tx.send(&Msg::SetInfo {
        key: TERMINAL_KEY.into(),
        value: terminal_kind(std::env::var_os("SSH_CONNECTION").is_some()).into(),
    })?;
    tx.send(&Msg::Replay { from: head })?;
    // The jar tells its program this owner attached (`attached`), and the
    // program repaints for it; this terminal's size is all it needs from here.
    let (rows, cols) = size();
    let resize = |r: u16, c: u16| Msg::Resize {
        rows: r,
        cols: c,
        px_w: 0,
        px_h: 0,
    };
    tx.send(&resize(rows, cols))?;

    let (done_tx, done_rx) = std::sync::mpsc::channel::<&'static str>();
    {
        // Output to the terminal, acknowledged once written.
        let tx = tx.clone();
        let done = done_tx.clone();
        std::thread::spawn(move || {
            let mut out = std::io::stdout();
            let why = loop {
                match rx.recv(ATTACH_PING) {
                    Ok(Some(Msg::Output { offset, bytes })) => {
                        if out.write_all(&bytes).and_then(|()| out.flush()).is_err() {
                            break "the terminal went away";
                        }
                        let _ = tx.send(&Msg::Ack {
                            offset: offset + bytes.len() as u64,
                        });
                    }
                    Ok(Some(Msg::Exited { .. })) => {
                        let _ = tx.send(&Msg::AckExit);
                        break "the program exited";
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        if tx.send(&Msg::Ping { n: 0 }).is_err() {
                            break "the connection closed";
                        }
                    }
                    Err(_) => break "the connection closed",
                }
            };
            let _ = done.send(why);
        });
    }
    {
        // Size changes, polled: a SIGWINCH handler would need `unsafe`.
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut last = size();
            loop {
                std::thread::sleep(SIZE_POLL);
                let now = size();
                if now != last {
                    last = now;
                    if tx.send(&resize(now.0, now.1)).is_err() {
                        return;
                    }
                }
            }
        });
    }
    {
        // Input to the program; `^\` detaches.
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 4096];
            let why = loop {
                let n = match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => break "input closed",
                    Ok(n) => n,
                };
                if let Some(i) = buf[..n].iter().position(|b| *b == DETACH) {
                    if i > 0 {
                        let _ = tx.send(&Msg::Input {
                            bytes: buf[..i].to_vec(),
                        });
                    }
                    break "detached";
                }
                if tx
                    .send(&Msg::Input {
                        bytes: buf[..n].to_vec(),
                    })
                    .is_err()
                {
                    break "the connection closed";
                }
            };
            let _ = done_tx.send(why);
        });
    }
    let why = done_rx.recv().unwrap_or("ended");
    tx.close();
    Ok(why)
}

//! The image-protocol probe against a pty that plays the terminal. The probe
//! ends on the terminal's DA1 reply, keeps what was answered before it, and
//! leaves no reader on stdin once it returns: stage 2b of the jar spike found
//! the reader `ratatui-image` leaves behind taking a later terminal's first
//! keys.
//!
//! Each test runs in a copy of the test binary whose stdin and stdout are a
//! pty's, since the probe reads the process's own stdin. The copy reports over
//! a Unix socket, and types its key only when told to, so the key arrives
//! after the probe has returned.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// Set in the copy of the test binary, naming the socket it reports to.
const CHILD: &str = "SPYC_IMAGE_PROBE_CHILD";

/// Environment that steers the probe away from asking the terminal.
const STEERING_ENV: [&str; 6] = [
    "TERM_PROGRAM",
    "LC_TERMINAL",
    "TMUX",
    "WEZTERM_EXECUTABLE",
    "KONSOLE_VERSION",
    "KITTY_WINDOW_ID",
];

/// A query the probe sends, as the fake terminal reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Query {
    /// `ESC _ G … ESC \`, a kitty graphics query.
    Kitty,
    /// `ESC [ c`, Primary Device Attributes.
    Da1,
    /// `ESC [ 16 t`, the cell size in pixels.
    CellSize,
    /// `ESC [ 5 n`, Device Status Report.
    Status,
    /// Any other escape sequence.
    Other,
}

/// What the fake terminal answers.
#[derive(Clone, Copy)]
struct Answers {
    kitty: bool,
    cell_size: Option<(u16, u16)>,
    da1: bool,
    status: bool,
    /// Type `k` in the same write as the DA1 reply, rather than once the
    /// probe has returned.
    key_with_da1: bool,
}

impl Answers {
    const NOTHING: Self = Self {
        kitty: false,
        cell_size: None,
        da1: false,
        status: false,
        key_with_da1: false,
    };

    fn reply(self, query: Query) -> Option<Vec<u8>> {
        match query {
            Query::Kitty if self.kitty => Some(b"\x1b_Gi=31;OK\x1b\\".to_vec()),
            Query::CellSize => self
                .cell_size
                .map(|(w, h)| format!("\x1b[6;{h};{w}t").into_bytes()),
            Query::Da1 if self.da1 => {
                let mut reply = b"\x1b[?62;22c".to_vec();
                if self.key_with_da1 {
                    reply.push(b'k');
                }
                Some(reply)
            }
            Query::Status if self.status => Some(b"\x1b[0n".to_vec()),
            _ => None,
        }
    }
}

/// The first complete query in `buf`, and how many bytes it and what came
/// before it take; `None` while the next one is incomplete.
fn next_query(buf: &[u8]) -> Option<(Query, usize)> {
    let start = buf.iter().position(|&b| b == 0x1b)?;
    let seq = &buf[start..];
    let (query, len) = match seq.get(1)? {
        b'_' => {
            let end = seq.windows(2).position(|w| w == b"\x1b\\")?;
            (Query::Kitty, end + 2)
        }
        b'[' => {
            let fin = seq[2..].iter().position(|b| (0x40..=0x7e).contains(b))? + 2;
            let query = match &seq[2..=fin] {
                b"c" => Query::Da1,
                b"16t" => Query::CellSize,
                b"5n" => Query::Status,
                _ => Query::Other,
            };
            (query, fin + 1)
        }
        _ => (Query::Other, 1),
    };
    Some((query, start + len))
}

/// What the copy of the test binary reported.
struct Report {
    /// `probed <protocol> <w>x<h>`, or `probed none`.
    probed: String,
    /// The key spyc's reader got after the probe, or `none`.
    key: String,
}

/// The copy's half: probe as spyc does at launch, report, then read the key
/// the fake terminal types next through crossterm, as spyc's input thread does.
fn child_side(sock: &Path) {
    crossterm::terminal::enable_raw_mode().expect("raw mode on the pty");
    let picker = super::detect_image_picker();
    let mut conn = UnixStream::connect(sock).expect("the test's socket");
    let probed = picker.map_or_else(
        || "probed none".to_string(),
        |p| {
            let font = p.font_size();
            format!(
                "probed {:?} {}x{}",
                p.protocol_type(),
                font.width,
                font.height
            )
        },
    );
    writeln!(conn, "{probed}").expect("report the probe");
    let mut go = [0u8];
    conn.read_exact(&mut go).expect("the go-ahead");
    let key = if event::poll(Duration::from_secs(5)).unwrap_or(false) {
        match event::read() {
            Ok(Event::Key(k)) => format!("{:?}", k.code),
            other => format!("{other:?}"),
        }
    } else {
        "none".to_string()
    };
    writeln!(conn, "{key}").expect("report the key");
    let _ = crossterm::terminal::disable_raw_mode();
}

/// Runs `test` in a copy of the test binary on a pty that answers as
/// `answers` says, types `k` once the probe has returned, and reports.
fn run_on_a_fake_terminal(test: &str, answers: Answers) -> Report {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("probe.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    listener.set_nonblocking(true).unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        test,
        "--include-ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    cmd.env(CHILD, &sock);
    for var in STEERING_ENV {
        cmd.env_remove(var);
    }
    cmd.cwd(dir.path());
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);

    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let mut reader = pair.master.try_clone_reader().unwrap();
    let terminal = {
        let writer = Arc::clone(&writer);
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            let mut at = 0;
            let mut chunk = [0u8; 4096];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&chunk[..n]);
                while let Some((query, used)) = next_query(&seen[at..]) {
                    at += used;
                    if let Some(reply) = answers.reply(query) {
                        writer.lock().unwrap().write_all(&reply).unwrap();
                    }
                }
            }
            String::from_utf8_lossy(&seen).into_owned()
        })
    };

    let deadline = Instant::now() + Duration::from_secs(60);
    let conn = loop {
        match listener.accept() {
            Ok((conn, _)) => break conn,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(status) = child.try_wait().unwrap() {
                    drop(pair.master);
                    panic!(
                        "the copy exited ({status:?}) before reporting:\n{}",
                        terminal.join().unwrap()
                    );
                }
                assert!(Instant::now() < deadline, "the copy never reported");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("accept: {e}"),
        }
    };
    conn.set_nonblocking(false).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let mut lines = BufReader::new(conn.try_clone().unwrap());
    let mut probed = String::new();
    lines.read_line(&mut probed).unwrap();
    if !answers.key_with_da1 {
        writer.lock().unwrap().write_all(b"k").unwrap();
    }
    (&conn).write_all(b"g").unwrap();
    let mut key = String::new();
    lines.read_line(&mut key).unwrap();
    child.wait().unwrap();
    drop(pair.master);
    drop(writer);
    let _ = terminal.join();
    Report {
        probed: probed.trim().to_string(),
        key: key.trim().to_string(),
    }
}

/// A terminal that answers nothing: the probe gives up, and the key typed
/// after it reaches spyc's reader, not a reader the probe left on stdin.
#[test]
fn a_key_typed_after_an_unanswered_probe_reaches_spyc() {
    if let Some(sock) = std::env::var_os(CHILD) {
        return child_side(Path::new(&sock));
    }
    let report = run_on_a_fake_terminal(
        "terminal::probe_tests::a_key_typed_after_an_unanswered_probe_reaches_spyc",
        Answers::NOTHING,
    );
    assert_eq!(report.key, "Char('k')", "the key after the probe was lost");
}

/// A terminal that answers the kitty query, the cell size and DA1, but not a
/// status report: the probe ends on the DA1 reply with what came before it,
/// rather than waiting on a timer and falling back to defaults.
#[test]
fn the_probe_ends_on_the_da1_reply_with_what_was_answered_before_it() {
    if let Some(sock) = std::env::var_os(CHILD) {
        return child_side(Path::new(&sock));
    }
    let report = run_on_a_fake_terminal(
        "terminal::probe_tests::the_probe_ends_on_the_da1_reply_with_what_was_answered_before_it",
        Answers {
            kitty: true,
            cell_size: Some((7, 15)),
            da1: true,
            status: false,
            key_with_da1: false,
        },
    );
    assert_eq!(report.probed, "probed Kitty 7x15");
    assert_eq!(report.key, "Char('k')", "the key after the probe was lost");
}

/// A terminal that answers every query: the probe reports its protocol and
/// cell size, and the key typed after it reaches spyc. This one passes on the
/// probe the two above fail on, so the fake terminal is known to work.
#[test]
fn a_terminal_that_answers_everything_gives_its_protocol_and_cell_size() {
    if let Some(sock) = std::env::var_os(CHILD) {
        return child_side(Path::new(&sock));
    }
    let report = run_on_a_fake_terminal(
        "terminal::probe_tests::a_terminal_that_answers_everything_gives_its_protocol_and_cell_size",
        Answers {
            kitty: true,
            cell_size: Some((7, 15)),
            da1: true,
            status: true,
            key_with_da1: false,
        },
    );
    assert_eq!(report.probed, "probed Kitty 7x15");
    assert_eq!(report.key, "Char('k')");
}

/// A key typed in the same write as the DA1 reply: the probe stops at the
/// reply's last byte, so the key is left for spyc's reader.
#[test]
fn a_key_right_behind_the_da1_reply_is_left_for_spyc() {
    if let Some(sock) = std::env::var_os(CHILD) {
        return child_side(Path::new(&sock));
    }
    let report = run_on_a_fake_terminal(
        "terminal::probe_tests::a_key_right_behind_the_da1_reply_is_left_for_spyc",
        Answers {
            kitty: true,
            cell_size: Some((7, 15)),
            da1: true,
            status: false,
            key_with_da1: true,
        },
    );
    assert_eq!(report.probed, "probed Kitty 7x15");
    assert_eq!(report.key, "Char('k')", "the probe took the key");
}

/// The fake terminal reads each query whole, and waits for one cut short.
#[test]
fn the_fake_terminal_reads_the_queries_it_answers() {
    let batch = b"text\x1b_Gi=31,a=q;AAAA\x1b\\\x1b[c\x1b[16t\x1b[5n\x1b[?u";
    let mut at = 0;
    let mut seen = Vec::new();
    while let Some((query, used)) = next_query(&batch[at..]) {
        seen.push(query);
        at += used;
    }
    assert_eq!(
        seen,
        [
            Query::Kitty,
            Query::Da1,
            Query::CellSize,
            Query::Status,
            Query::Other
        ]
    );
    assert_eq!(at, batch.len());
    assert_eq!(next_query(b"\x1b[16"), None);
    assert_eq!(next_query(b"\x1b_Gi=31;OK"), None);
}

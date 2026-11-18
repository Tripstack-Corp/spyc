//! The image-protocol probe: which graphics protocol the terminal speaks and
//! its cell size in pixels, asked on the caller's thread.
//!
//! It sends its queries with DA1 (`CSI c`) last. A terminal answers in order,
//! so the DA1 reply ends the probe holding every answer it will get, and
//! [`BACKSTOP`] ends it only for a terminal that answers nothing. It reads a
//! byte at a time, so a key typed after the DA1 reply stays for spyc's input
//! reader, and it reads only while it runs. `ratatui-image`'s own probe reads
//! on a thread that outlives its timer, which took a later terminal's first
//! keys; its parser and `Picker` are kept.

use std::io::{self, Write};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use ratatui_image::FontSize;
use ratatui_image::picker::cap_parser::{Parser, Response};
use ratatui_image::picker::{Picker, ProtocolType};

/// How long the probe waits on a terminal that answers nothing, DA1 included.
pub const BACKSTOP: Duration = Duration::from_secs(2);

/// What the environment says about the terminal, read by the caller.
#[derive(Debug, Clone, Copy, Default)]
pub struct Steer {
    /// Inside tmux: the queries go through its passthrough.
    pub tmux: bool,
    /// WezTerm or Konsole: kitty and sixel go unused, as `ratatui-image` has
    /// it, since neither draws them well.
    pub no_kitty_or_sixel: bool,
}

/// Asks the terminal on `output` and reads its answers from `input`, which
/// must be in raw mode, until the DA1 reply or `backstop`. Errs only when the
/// queries can't be written.
pub fn probe(
    input: impl AsFd,
    output: &mut (impl Write + AsFd),
    steer: Steer,
    backstop: Duration,
) -> io::Result<Picker> {
    output.write_all(query(steer).as_bytes())?;
    output.flush()?;
    let deadline = Instant::now() + backstop;
    let mut replies = Replies::default();
    let mut byte = [0u8; 1];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !readable(&input, left) {
            break;
        }
        match rustix::io::read(&input, &mut byte) {
            Ok(1) => {
                if replies.push(byte[0]) {
                    break;
                }
            }
            Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => {}
            Ok(_) | Err(_) => break,
        }
    }
    Ok(picker(&replies.responses, steer, window_cell(&*output)))
}

/// The queries, DA1 last.
fn query(steer: Steer) -> String {
    let (start, esc, end) = Parser::tmux_start_escape_end(steer.tmux);
    let kitty = if steer.no_kitty_or_sixel {
        String::new()
    } else {
        format!("{esc}_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA{esc}\\")
    };
    format!("{start}{kitty}{esc}[16t{esc}[c{end}")
}

/// Whether `fd` has a byte to read within `wait`.
fn readable(fd: impl AsFd, wait: Duration) -> bool {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    let ts = Timespec {
        tv_sec: i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(wait.subsec_nanos()),
    };
    matches!(poll(&mut fds, Some(&ts)), Ok(n) if n > 0)
}

/// The terminal's answers, taken a byte at a time.
#[derive(Default)]
struct Replies {
    parser: Parser,
    responses: Vec<Response>,
    /// The escape sequence being read, to spot the DA1 reply.
    seq: Vec<u8>,
}

impl Replies {
    /// Takes one byte; true once it ends the DA1 reply.
    fn push(&mut self, byte: u8) -> bool {
        self.responses.extend(self.parser.push(char::from(byte)));
        if byte == 0x1b {
            self.seq.clear();
        }
        if byte == 0x1b || !self.seq.is_empty() {
            self.seq.push(byte);
        }
        if self.seq.len() > 64 {
            self.seq.clear();
        }
        is_da1_reply(&self.seq)
    }
}

/// `ESC [ ? <params> c`.
fn is_da1_reply(seq: &[u8]) -> bool {
    seq.strip_prefix(b"\x1b[?")
        .and_then(|rest| rest.strip_suffix(b"c"))
        .is_some_and(|params| params.iter().all(|b| b.is_ascii_digit() || *b == b';'))
}

/// The picker the answers describe, as `Picker::from_query_stdio` builds it: a
/// protocol the terminal answered for, else one the environment names, at the
/// cell size it answered, else the one the window's size gives.
fn picker(responses: &[Response], steer: Steer, window_cell: Option<FontSize>) -> Picker {
    let answered = if steer.no_kitty_or_sixel {
        None
    } else if responses.contains(&Response::Kitty) {
        Some(ProtocolType::Kitty)
    } else if responses.contains(&Response::Sixel) {
        Some(ProtocolType::Sixel)
    } else {
        None
    };
    let cell = responses
        .iter()
        .find_map(|r| match r {
            Response::CellSize(Some((w, h))) => Some(FontSize::new(*w, *h)),
            _ => None,
        })
        .or(window_cell);
    let Some(cell) = cell else {
        return Picker::halfblocks();
    };
    #[expect(
        deprecated,
        reason = "its replacement, `from_query_stdio`, is the probe this module replaces"
    )]
    let mut picker = Picker::from_fontsize(cell);
    if let Some(protocol) = answered {
        picker.set_protocol_type(protocol);
    }
    picker
}

/// The cell size the window's size in pixels gives, when the terminal reports it.
fn window_cell(fd: impl AsFd) -> Option<FontSize> {
    let ws = rustix::termios::tcgetwinsize(fd).ok()?;
    if ws.ws_xpixel == 0 || ws.ws_ypixel == 0 || ws.ws_col == 0 || ws.ws_row == 0 {
        return None;
    }
    Some(FontSize::new(
        ws.ws_xpixel / ws.ws_col,
        ws.ws_ypixel / ws.ws_row,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(picker: &Picker) -> (u16, u16) {
        let f = picker.font_size();
        (f.width, f.height)
    }

    fn read_all(replies: &mut Replies, bytes: &[u8]) -> Option<usize> {
        bytes.iter().position(|&b| replies.push(b))
    }

    #[test]
    fn the_queries_end_with_da1() {
        let q = query(Steer::default());
        assert!(q.starts_with("\x1b_Gi=31"), "{q:?}");
        assert!(q.ends_with("\x1b[16t\x1b[c"), "{q:?}");
        let q = query(Steer {
            no_kitty_or_sixel: true,
            ..Steer::default()
        });
        assert_eq!(q, "\x1b[16t\x1b[c");
        let q = query(Steer {
            tmux: true,
            no_kitty_or_sixel: true,
        });
        assert_eq!(q, "\x1bPtmux;\x1b\x1b[16t\x1b\x1b[c\x1b\\");
    }

    #[test]
    fn the_da1_reply_ends_the_answers_and_nothing_before_it_does() {
        let answers = b"\x1b_Gi=31;OK\x1b\\\x1b[6;15;7t\x1b[0n\x1b[?62;4;22ck";
        let mut replies = Replies::default();
        let end = read_all(&mut replies, answers).expect("the DA1 reply ends it");
        assert_eq!(
            &answers[end + 1..],
            b"k",
            "it ends on the reply's last byte"
        );
        assert!(replies.responses.contains(&Response::Kitty));
        assert!(replies.responses.contains(&Response::Sixel));
        assert!(
            replies
                .responses
                .contains(&Response::CellSize(Some((7, 15))))
        );
    }

    #[test]
    fn a_csi_that_isnt_a_da1_reply_doesnt_end_it() {
        for seq in [
            &b"\x1b[?1;2u"[..],
            b"\x1b[c",
            b"\x1b[?62;xc",
            b"\x1b[6;15;7t",
        ] {
            assert_eq!(read_all(&mut Replies::default(), seq), None, "{seq:?}");
        }
    }

    #[test]
    fn the_picker_takes_the_answered_protocol_and_cell_size() {
        let kitty = picker(
            &[
                Response::Sixel,
                Response::Kitty,
                Response::CellSize(Some((7, 15))),
            ],
            Steer::default(),
            Some(FontSize::new(9, 18)),
        );
        assert_eq!(kitty.protocol_type(), ProtocolType::Kitty);
        assert_eq!(cell(&kitty), (7, 15));
        let sixel = picker(
            &[Response::Sixel, Response::CellSize(Some((7, 15)))],
            Steer::default(),
            None,
        );
        assert_eq!(sixel.protocol_type(), ProtocolType::Sixel);
    }

    #[test]
    fn the_window_gives_the_cell_size_when_the_terminal_doesnt() {
        let p = picker(
            &[Response::CellSize(None)],
            Steer::default(),
            Some(FontSize::new(9, 18)),
        );
        assert_eq!(cell(&p), (9, 18));
    }

    #[test]
    fn with_no_cell_size_it_falls_back_to_halfblocks() {
        let p = picker(&[Response::Kitty], Steer::default(), None);
        assert_eq!(p.protocol_type(), ProtocolType::Halfblocks);
    }

    #[test]
    fn wezterm_and_konsole_never_get_kitty_or_sixel() {
        let steer = Steer {
            no_kitty_or_sixel: true,
            ..Steer::default()
        };
        let p = picker(
            &[
                Response::Kitty,
                Response::Sixel,
                Response::CellSize(Some((7, 15))),
            ],
            steer,
            None,
        );
        assert_ne!(p.protocol_type(), ProtocolType::Kitty);
        assert_ne!(p.protocol_type(), ProtocolType::Sixel);
    }
}

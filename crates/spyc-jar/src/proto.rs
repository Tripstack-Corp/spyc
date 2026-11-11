//! The jar's frames: `[len: u32 BE][kind: u8][flags: u8][payload]`, where `len`
//! counts kind, flags and payload. Every integer is big-endian. A message the
//! reader doesn't know is skipped if its flags don't mark it critical, and
//! refused if they do (`JAR_PROTOCOL.md` → "How the protocol grows").

use std::io::{self, Read, Write};

/// The protocol version a `hello` carries.
pub const VERSION: u16 = 1;
/// The largest frame either side accepts, payload included.
pub const MAX_FRAME: usize = 16 << 20;
/// Set in a frame's flags when a reader that doesn't know its kind must refuse
/// it rather than skip it.
pub const CRITICAL: u8 = 0x80;

/// What a connection asks to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner = 1,
    Reader = 2,
}

/// The signals a client may ask the jar to deliver to its program's group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JarSignal {
    Interrupt = 1,
    Terminate = 2,
    Kill = 3,
    Stop = 4,
    Continue = 5,
    Hangup = 6,
}

impl JarSignal {
    pub const fn from_wire(n: i32) -> Option<Self> {
        Some(match n {
            1 => Self::Interrupt,
            2 => Self::Terminate,
            3 => Self::Kill,
            4 => Self::Stop,
            5 => Self::Continue,
            6 => Self::Hangup,
            _ => return None,
        })
    }
}

/// The two snapshot formats (R17a): the engine's binary snapshot, valid for
/// the same build only, and its VT formatter emit, portable across versions.
pub const SNAPSHOT_BINARY: u16 = 1;
pub const SNAPSHOT_VT: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub format: u16,
    pub offset: u64,
    pub rows: u16,
    pub cols: u16,
    pub blob: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    // Client to jar.
    Hello {
        version: u16,
        role: Role,
    },
    Input {
        bytes: Vec<u8>,
    },
    Resize {
        rows: u16,
        cols: u16,
        px_w: u16,
        px_h: u16,
    },
    /// The owner has parsed every byte before `offset` (R11).
    Ack {
        offset: u64,
    },
    /// Stream the log from `from` on.
    Replay {
        from: u64,
    },
    /// One of the [`JarSignal`]s, by its protocol number: OS signal numbers
    /// differ between macOS and Linux, so they never travel.
    Signal {
        signal: i32,
    },
    StoreSnapshot(Snapshot),
    FetchSnapshot,
    Ping {
        n: u64,
    },
    AckExit,
    // Jar to client.
    Welcome {
        version: u16,
        child_pid: u32,
        head: u64,
        base: u64,
        rows: u16,
        cols: u16,
    },
    Output {
        offset: u64,
        bytes: Vec<u8>,
    },
    /// The PTY took this size at `offset` (R9a).
    Resized {
        offset: u64,
        rows: u16,
        cols: u16,
    },
    /// Bytes `from..to` were dropped (R12a).
    Gap {
        from: u64,
        to: u64,
    },
    Snapshot(Option<Snapshot>),
    Pong {
        n: u64,
    },
    /// The program ended; `code` is its exit code, or the signal that killed it
    /// when `signaled`.
    Exited {
        code: i32,
        signaled: bool,
    },
    Refused {
        why: String,
    },
    Unsupported {
        kind: u8,
    },
}

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    TooLarge(usize),
    Malformed(&'static str),
    /// A critical kind this side doesn't know.
    UnknownCritical(u8),
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::TooLarge(n) => write!(f, "frame of {n} bytes is over the limit"),
            Self::Malformed(what) => write!(f, "malformed frame: {what}"),
            Self::UnknownCritical(k) => write!(f, "unknown critical frame kind {k}"),
        }
    }
}

impl std::error::Error for ReadError {}

// Kinds. Client to jar below 64, jar to client from 64.
const HELLO: u8 = 1;
const INPUT: u8 = 2;
const RESIZE: u8 = 3;
const ACK: u8 = 4;
const REPLAY: u8 = 5;
const SIGNAL: u8 = 6;
const STORE_SNAPSHOT: u8 = 7;
const FETCH_SNAPSHOT: u8 = 8;
const PING: u8 = 9;
const ACK_EXIT: u8 = 10;
const WELCOME: u8 = 64;
const OUTPUT: u8 = 65;
const RESIZED: u8 = 66;
const GAP: u8 = 67;
const SNAPSHOT: u8 = 68;
const PONG: u8 = 69;
const EXITED: u8 = 70;
const REFUSED: u8 = 71;
const UNSUPPORTED: u8 = 72;

/// Every kind this version knows is critical: a reader that doesn't know one
/// can't safely carry on without it.
fn kind_and_payload(msg: &Msg) -> (u8, Vec<u8>) {
    let mut p = Vec::new();
    let kind = match msg {
        Msg::Hello { version, role } => {
            p.extend(version.to_be_bytes());
            p.push(*role as u8);
            HELLO
        }
        Msg::Input { bytes } => {
            p.extend(bytes);
            INPUT
        }
        Msg::Resize {
            rows,
            cols,
            px_w,
            px_h,
        } => {
            for v in [rows, cols, px_w, px_h] {
                p.extend(v.to_be_bytes());
            }
            RESIZE
        }
        Msg::Ack { offset } => {
            p.extend(offset.to_be_bytes());
            ACK
        }
        Msg::Replay { from } => {
            p.extend(from.to_be_bytes());
            REPLAY
        }
        Msg::Signal { signal } => {
            p.extend(signal.to_be_bytes());
            SIGNAL
        }
        Msg::StoreSnapshot(s) => {
            put_snapshot(&mut p, s);
            STORE_SNAPSHOT
        }
        Msg::FetchSnapshot => FETCH_SNAPSHOT,
        Msg::Ping { n } => {
            p.extend(n.to_be_bytes());
            PING
        }
        Msg::AckExit => ACK_EXIT,
        Msg::Welcome {
            version,
            child_pid,
            head,
            base,
            rows,
            cols,
        } => {
            p.extend(version.to_be_bytes());
            p.extend(child_pid.to_be_bytes());
            p.extend(head.to_be_bytes());
            p.extend(base.to_be_bytes());
            p.extend(rows.to_be_bytes());
            p.extend(cols.to_be_bytes());
            WELCOME
        }
        Msg::Output { offset, bytes } => {
            p.extend(offset.to_be_bytes());
            p.extend(bytes);
            OUTPUT
        }
        Msg::Resized { offset, rows, cols } => {
            p.extend(offset.to_be_bytes());
            p.extend(rows.to_be_bytes());
            p.extend(cols.to_be_bytes());
            RESIZED
        }
        Msg::Gap { from, to } => {
            p.extend(from.to_be_bytes());
            p.extend(to.to_be_bytes());
            GAP
        }
        Msg::Snapshot(s) => {
            if let Some(s) = s {
                p.push(1);
                put_snapshot(&mut p, s);
            } else {
                p.push(0);
            }
            SNAPSHOT
        }
        Msg::Pong { n } => {
            p.extend(n.to_be_bytes());
            PONG
        }
        Msg::Exited { code, signaled } => {
            p.extend(code.to_be_bytes());
            p.push(u8::from(*signaled));
            EXITED
        }
        Msg::Refused { why } => {
            p.extend(why.as_bytes());
            REFUSED
        }
        Msg::Unsupported { kind } => {
            p.push(*kind);
            UNSUPPORTED
        }
    };
    (kind, p)
}

fn put_snapshot(p: &mut Vec<u8>, s: &Snapshot) {
    p.extend(s.format.to_be_bytes());
    p.extend(s.offset.to_be_bytes());
    p.extend(s.rows.to_be_bytes());
    p.extend(s.cols.to_be_bytes());
    p.extend(&s.blob);
}

/// Encode `msg` as one frame.
pub fn encode(msg: &Msg) -> Vec<u8> {
    let (kind, payload) = kind_and_payload(msg);
    let len = u32::try_from(payload.len() + 2).expect("a frame fits in u32");
    let mut out = Vec::with_capacity(payload.len() + 6);
    out.extend(len.to_be_bytes());
    out.push(kind);
    out.push(CRITICAL);
    out.extend(payload);
    out
}

pub fn write_msg(w: &mut impl Write, msg: &Msg) -> io::Result<()> {
    w.write_all(&encode(msg))?;
    w.flush()
}

/// Read one frame. `Ok(None)` is an ancillary frame of a kind this side
/// doesn't know, already skipped.
pub fn read_msg(r: &mut impl Read) -> Result<Option<Msg>, ReadError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(ReadError::TooLarge(len));
    }
    if len < 2 {
        return Err(ReadError::Malformed("frame shorter than its header"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    decode(buf[0], buf[1], &buf[2..])
}

/// Decode one frame's body. Never panics, whatever the bytes.
pub fn decode(kind: u8, flags: u8, payload: &[u8]) -> Result<Option<Msg>, ReadError> {
    let mut c = Cursor(payload);
    let msg = match kind {
        HELLO => {
            let version = c.u16()?;
            let role = match c.u8()? {
                1 => Role::Owner,
                2 => Role::Reader,
                _ => return Err(ReadError::Malformed("unknown role")),
            };
            Msg::Hello { version, role }
        }
        INPUT => Msg::Input { bytes: c.rest() },
        RESIZE => Msg::Resize {
            rows: c.u16()?,
            cols: c.u16()?,
            px_w: c.u16()?,
            px_h: c.u16()?,
        },
        ACK => Msg::Ack { offset: c.u64()? },
        REPLAY => Msg::Replay { from: c.u64()? },
        SIGNAL => Msg::Signal { signal: c.i32()? },
        STORE_SNAPSHOT => Msg::StoreSnapshot(c.snapshot()?),
        FETCH_SNAPSHOT => Msg::FetchSnapshot,
        PING => Msg::Ping { n: c.u64()? },
        ACK_EXIT => Msg::AckExit,
        WELCOME => Msg::Welcome {
            version: c.u16()?,
            child_pid: c.u32()?,
            head: c.u64()?,
            base: c.u64()?,
            rows: c.u16()?,
            cols: c.u16()?,
        },
        OUTPUT => Msg::Output {
            offset: c.u64()?,
            bytes: c.rest(),
        },
        RESIZED => Msg::Resized {
            offset: c.u64()?,
            rows: c.u16()?,
            cols: c.u16()?,
        },
        GAP => Msg::Gap {
            from: c.u64()?,
            to: c.u64()?,
        },
        SNAPSHOT => match c.u8()? {
            0 => Msg::Snapshot(None),
            1 => Msg::Snapshot(Some(c.snapshot()?)),
            _ => return Err(ReadError::Malformed("snapshot presence flag")),
        },
        PONG => Msg::Pong { n: c.u64()? },
        EXITED => Msg::Exited {
            code: c.i32()?,
            signaled: c.u8()? != 0,
        },
        REFUSED => Msg::Refused {
            why: String::from_utf8_lossy(&c.rest()).into_owned(),
        },
        UNSUPPORTED => Msg::Unsupported { kind: c.u8()? },
        other if flags & CRITICAL != 0 => return Err(ReadError::UnknownCritical(other)),
        _ => return Ok(None),
    };
    Ok(Some(msg))
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], ReadError> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .ok_or(ReadError::Malformed("payload too short"))?;
        self.0 = rest;
        Ok(*head)
    }
    fn u8(&mut self) -> Result<u8, ReadError> {
        Ok(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, ReadError> {
        Ok(u16::from_be_bytes(self.take()?))
    }
    fn u32(&mut self) -> Result<u32, ReadError> {
        Ok(u32::from_be_bytes(self.take()?))
    }
    fn i32(&mut self) -> Result<i32, ReadError> {
        Ok(i32::from_be_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, ReadError> {
        Ok(u64::from_be_bytes(self.take()?))
    }
    fn rest(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.0).to_vec()
    }
    fn snapshot(&mut self) -> Result<Snapshot, ReadError> {
        Ok(Snapshot {
            format: self.u16()?,
            offset: self.u64()?,
            rows: self.u16()?,
            cols: self.u16()?,
            blob: self.rest(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_kind() -> Vec<Msg> {
        let snap = Snapshot {
            format: SNAPSHOT_VT,
            offset: 9,
            rows: 24,
            cols: 80,
            blob: b"\x1b[H".to_vec(),
        };
        vec![
            Msg::Hello {
                version: VERSION,
                role: Role::Owner,
            },
            Msg::Hello {
                version: VERSION,
                role: Role::Reader,
            },
            Msg::Input {
                bytes: b"\x1b[A".to_vec(),
            },
            Msg::Resize {
                rows: 40,
                cols: 120,
                px_w: 1200,
                px_h: 800,
            },
            Msg::Ack { offset: u64::MAX },
            Msg::Replay { from: 3 },
            Msg::Signal { signal: -2 },
            Msg::StoreSnapshot(snap.clone()),
            Msg::FetchSnapshot,
            Msg::Ping { n: 7 },
            Msg::AckExit,
            Msg::Welcome {
                version: VERSION,
                child_pid: 42,
                head: 100,
                base: 4,
                rows: 24,
                cols: 80,
            },
            Msg::Output {
                offset: 5,
                bytes: vec![0, 255, 10],
            },
            Msg::Resized {
                offset: 6,
                rows: 1,
                cols: 2,
            },
            Msg::Gap { from: 0, to: 8 },
            Msg::Snapshot(Some(snap)),
            Msg::Snapshot(None),
            Msg::Pong { n: 7 },
            Msg::Exited {
                code: 9,
                signaled: true,
            },
            Msg::Refused {
                why: "an owner is connected".into(),
            },
            Msg::Unsupported { kind: 200 },
        ]
    }

    #[test]
    fn every_kind_round_trips() {
        for msg in every_kind() {
            let frame = encode(&msg);
            let back = read_msg(&mut frame.as_slice()).expect("decodes");
            assert_eq!(back, Some(msg));
        }
    }

    #[test]
    fn an_unknown_ancillary_kind_is_skipped_and_a_critical_one_refused() {
        assert!(matches!(decode(250, 0, b"anything"), Ok(None)));
        assert!(matches!(
            decode(250, CRITICAL, b""),
            Err(ReadError::UnknownCritical(250))
        ));
    }

    #[test]
    fn an_oversized_or_short_frame_is_refused_before_reading_it() {
        let huge = u32::try_from(MAX_FRAME + 1).unwrap().to_be_bytes();
        assert!(matches!(
            read_msg(&mut huge.as_slice()),
            Err(ReadError::TooLarge(_))
        ));
        assert!(matches!(
            read_msg(&mut [0, 0, 0, 1, 1].as_slice()),
            Err(ReadError::Malformed(_))
        ));
    }

    /// A cheap stand-in for the fuzz target: random and truncated frames never
    /// panic the decoder.
    #[test]
    fn decoding_arbitrary_bytes_never_panics() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..200_000 {
            let len = (next() % 40) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let kind = next() as u8;
            let _ = decode(kind, next() as u8, &bytes);
        }
        for msg in every_kind() {
            let frame = encode(&msg);
            for cut in 0..frame.len() {
                let _ = read_msg(&mut &frame[..cut]);
            }
        }
    }
}

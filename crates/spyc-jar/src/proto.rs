//! The frames on the jar's socket, and the messages they carry.
//!
//! A frame is `[len: u32 BE][kind: u8][flags: u8][payload]`, where `len`
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
/// The most a `hello` may be: a jar reads no more before it knows who is
/// asking.
pub const MAX_HELLO: usize = 64 << 10;

/// Why a request was refused: the reason `refused` leads with. A client acts on
/// the reason; the text after it is for people. A reason this version doesn't
/// know reads as [`reason::UNSPECIFIED`].
pub mod reason {
    pub const UNSPECIFIED: u16 = 0;
    /// A `hello` whose token isn't the jar's.
    pub const WRONG_TOKEN: u16 = 1;
    /// An owner's `hello` while the jar has one.
    pub const OWNER_PRESENT: u16 = 2;
    /// A `hello` naming no version this jar speaks.
    pub const VERSION_NOT_SERVED: u16 = 3;
    /// A request this connection's role may not make.
    pub const ROLE_NOT_PERMITTED: u16 = 4;
    /// An upgrade that can't start now: one is under way, or the jar couldn't
    /// reach a safe point. Worth asking again.
    pub const UPGRADE_BUSY: u16 = 5;
    /// The new binary is missing or rejected the handover; the jar runs on.
    pub const UPGRADE_REJECTED: u16 = 6;
    /// The upgrade failed after the new binary accepted the handover; the jar
    /// runs on.
    pub const UPGRADE_FAILED: u16 = 7;
    /// A request this jar can't act on: no `hello` first, a value out of
    /// range, or a kind a client doesn't send.
    pub const INVALID: u16 = 8;
    /// A signal for a program that has already ended.
    pub const PROGRAM_EXITED: u16 = 9;
}

/// What a connection asks to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner = 1,
    Reader = 2,
    /// R30a: asks only `Describe` and `Ping`, and needs no token: what a
    /// jar says of itself reveals nothing to type into or watch it with.
    Lookup = 3,
    /// The jar's own program, known by its pid rather than a token: it hears
    /// when a client attaches and detaches, and may ask the jar to detach it.
    Program = 4,
}

impl Role {
    pub const fn from_wire(n: u8) -> Option<Self> {
        Some(match n {
            1 => Self::Owner,
            2 => Self::Reader,
            3 => Self::Lookup,
            4 => Self::Program,
            _ => return None,
        })
    }
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
        /// R26: the jar's token, which every role but [`Role::Lookup`] must
        /// present. On the wire it is length-prefixed (`u16`), and whatever
        /// follows it is left for capabilities (R34), which a jar that knows
        /// none reads past.
        token: Vec<u8>,
    },
    Input {
        bytes: Vec<u8>,
    },
    /// The owner's answer to one of the program's terminal queries (R15):
    /// written to the PTY as input is, but its own kind, so a replayed query
    /// is never answered by mistake.
    Reply {
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
        reason: u16,
        why: String,
    },
    Unsupported {
        kind: u8,
    },
    /// The PTY's foreground process group changed (R4): `tcgetpgrp`, which
    /// only the master's holder can read.
    Foreground {
        pgid: u32,
    },
    /// Stage 3 (R33): replace the jar's binary with the one at `binary`, which
    /// must first accept the jar's handover blob. Only a connection that
    /// presented the token may ask.
    Upgrade {
        binary: String,
    },
    /// Sent to every connection by a jar that has just taken over from its
    /// previous binary: `build` names the binary now running. Ancillary, so a
    /// client that doesn't know it skips it.
    Upgraded {
        build: String,
    },
    /// R31: ask the jar to describe itself. Any role may, [`Role::Lookup`]
    /// included.
    Describe,
    /// R22: the owner hands the jar on. The jar ends this connection and
    /// `token` becomes its token: the old one opens nothing from then on.
    Release {
        token: Vec<u8>,
    },
    /// R18: one entry in the jar's key/value store, which the owner writes.
    /// `owner_mcp` is where the owner's MCP socket is (R30a); the jar forgets
    /// it when the owner goes.
    SetInfo {
        key: String,
        value: String,
    },
    /// The jar's answer: `key`, `value` pairs, so a newer jar can add keys an
    /// older client shows without understanding.
    Description {
        fields: Vec<(String, String)>,
    },
    /// The program asks its jar to end its owner's connection, once
    /// everything the program printed before has reached the owner.
    Detach,
    /// To the program: an owner connected.
    Attached,
    /// To the program: its owner went.
    Detached,
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
const REPLY: u8 = 11;
const UPGRADE: u8 = 12;
const DESCRIBE: u8 = 13;
const RELEASE: u8 = 14;
const SET_INFO: u8 = 15;
const DETACH: u8 = 16;
const WELCOME: u8 = 64;
const OUTPUT: u8 = 65;
const RESIZED: u8 = 66;
const GAP: u8 = 67;
const SNAPSHOT: u8 = 68;
const PONG: u8 = 69;
const EXITED: u8 = 70;
const REFUSED: u8 = 71;
const UNSUPPORTED: u8 = 72;
const FOREGROUND: u8 = 73;
const UPGRADED: u8 = 74;
const DESCRIPTION: u8 = 75;
const ATTACHED: u8 = 76;
const DETACHED: u8 = 77;

/// Every kind this version knows is critical: a reader that doesn't know one
/// can't safely carry on without it.
fn kind_and_payload(msg: &Msg) -> (u8, Vec<u8>) {
    match msg {
        Msg::Hello {
            version,
            role,
            token,
        } => (HELLO, hello_payload(*version, *role, token)),
        Msg::Release { token } => (RELEASE, token.clone()),
        Msg::SetInfo { key, value } => (
            SET_INFO,
            [&len_u32(key.len())[..], key.as_bytes(), value.as_bytes()].concat(),
        ),
        Msg::Input { bytes } => (INPUT, bytes.clone()),
        Msg::Reply { bytes } => (REPLY, bytes.clone()),
        Msg::Resize {
            rows,
            cols,
            px_w,
            px_h,
        } => (
            RESIZE,
            [rows, cols, px_w, px_h].map(|v| v.to_be_bytes()).concat(),
        ),
        Msg::Ack { offset } => (ACK, offset.to_be_bytes().to_vec()),
        Msg::Replay { from } => (REPLAY, from.to_be_bytes().to_vec()),
        Msg::Signal { signal } => (SIGNAL, signal.to_be_bytes().to_vec()),
        Msg::StoreSnapshot(s) => (STORE_SNAPSHOT, snapshot_payload(s)),
        Msg::FetchSnapshot => (FETCH_SNAPSHOT, Vec::new()),
        Msg::Ping { n } => (PING, n.to_be_bytes().to_vec()),
        Msg::AckExit => (ACK_EXIT, Vec::new()),
        Msg::Welcome {
            version,
            child_pid,
            head,
            base,
            rows,
            cols,
        } => (
            WELCOME,
            [
                &version.to_be_bytes()[..],
                &child_pid.to_be_bytes(),
                &head.to_be_bytes(),
                &base.to_be_bytes(),
                &rows.to_be_bytes(),
                &cols.to_be_bytes(),
            ]
            .concat(),
        ),
        Msg::Output { offset, bytes } => (OUTPUT, [&offset.to_be_bytes()[..], bytes].concat()),
        Msg::Resized { offset, rows, cols } => (
            RESIZED,
            [
                &offset.to_be_bytes()[..],
                &rows.to_be_bytes(),
                &cols.to_be_bytes(),
            ]
            .concat(),
        ),
        Msg::Gap { from, to } => (GAP, [from.to_be_bytes(), to.to_be_bytes()].concat()),
        Msg::Snapshot(None) => (SNAPSHOT, vec![0]),
        Msg::Snapshot(Some(s)) => (SNAPSHOT, [&[1][..], &snapshot_payload(s)].concat()),
        Msg::Pong { n } => (PONG, n.to_be_bytes().to_vec()),
        Msg::Exited { code, signaled } => (
            EXITED,
            [&code.to_be_bytes()[..], &[u8::from(*signaled)]].concat(),
        ),
        Msg::Refused { reason, why } => (
            REFUSED,
            [&reason.to_be_bytes()[..], why.as_bytes()].concat(),
        ),
        Msg::Detach => (DETACH, Vec::new()),
        Msg::Attached => (ATTACHED, Vec::new()),
        Msg::Detached => (DETACHED, Vec::new()),
        Msg::Upgrade { binary } => (UPGRADE, binary.as_bytes().to_vec()),
        Msg::Upgraded { build } => (UPGRADED, build.as_bytes().to_vec()),
        Msg::Describe => (DESCRIBE, Vec::new()),
        Msg::Description { fields } => (DESCRIPTION, description_payload(fields)),
        Msg::Unsupported { kind } => (UNSUPPORTED, vec![*kind]),
        Msg::Foreground { pgid } => (FOREGROUND, pgid.to_be_bytes().to_vec()),
    }
}

/// A `hello`: the version, the role, then the token behind its `u16` length,
/// cut at the most that length can say.
fn hello_payload(version: u16, role: Role, token: &[u8]) -> Vec<u8> {
    let token = &token[..token.len().min(usize::from(u16::MAX))];
    let len = u16::try_from(token.len()).unwrap_or(u16::MAX);
    [
        &version.to_be_bytes()[..],
        &[role as u8],
        &len.to_be_bytes(),
        token,
    ]
    .concat()
}

/// Each key and value of a description behind its `u32` length.
fn description_payload(fields: &[(String, String)]) -> Vec<u8> {
    let mut p = Vec::new();
    for s in fields.iter().flat_map(|(k, v)| [k, v]) {
        p.extend(len_u32(s.len()));
        p.extend(s.as_bytes());
    }
    p
}

/// A length as the `u32` the wire carries, saturating.
fn len_u32(n: usize) -> [u8; 4] {
    u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes()
}

fn snapshot_payload(s: &Snapshot) -> Vec<u8> {
    [
        &s.format.to_be_bytes()[..],
        &s.offset.to_be_bytes(),
        &s.rows.to_be_bytes(),
        &s.cols.to_be_bytes(),
        &s.blob,
    ]
    .concat()
}

/// Encode `msg` as one frame.
pub fn encode(msg: &Msg) -> Vec<u8> {
    let (kind, payload) = kind_and_payload(msg);
    let len = u32::try_from(payload.len() + 2).expect("a frame fits in u32");
    let mut out = Vec::with_capacity(payload.len() + 6);
    out.extend(len.to_be_bytes());
    out.push(kind);
    // Everything is critical but the notice a newer jar sends unasked.
    out.push(if kind == UPGRADED { 0 } else { CRITICAL });
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
    read_msg_max(r, MAX_FRAME)
}

/// [`read_msg`] for a frame of at most `max` bytes.
pub fn read_msg_max(r: &mut impl Read, max: usize) -> Result<Option<Msg>, ReadError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max.min(MAX_FRAME) {
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
        HELLO => c.hello()?,
        RELEASE => Msg::Release { token: c.rest() },
        SET_INFO => c.set_info()?,
        INPUT => Msg::Input { bytes: c.rest() },
        REPLY => Msg::Reply { bytes: c.rest() },
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
            reason: c.u16()?,
            why: String::from_utf8_lossy(&c.rest()).into_owned(),
        },
        DETACH => Msg::Detach,
        ATTACHED => Msg::Attached,
        DETACHED => Msg::Detached,
        UNSUPPORTED => Msg::Unsupported { kind: c.u8()? },
        UPGRADE => Msg::Upgrade {
            binary: String::from_utf8(c.rest()).map_err(|_| ReadError::Malformed("binary path"))?,
        },
        UPGRADED => Msg::Upgraded {
            build: String::from_utf8_lossy(&c.rest()).into_owned(),
        },
        DESCRIBE => Msg::Describe,
        DESCRIPTION => c.description()?,
        FOREGROUND => Msg::Foreground { pgid: c.u32()? },
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
    fn hello(&mut self) -> Result<Msg, ReadError> {
        let version = self.u16()?;
        let role = Role::from_wire(self.u8()?).ok_or(ReadError::Malformed("unknown role"))?;
        let n = usize::from(self.u16()?);
        // What follows the token is for capabilities (R34).
        let (token, _) = self
            .0
            .split_at_checked(n)
            .ok_or(ReadError::Malformed("hello token"))?;
        Ok(Msg::Hello {
            version,
            role,
            token: token.to_vec(),
        })
    }
    fn set_info(&mut self) -> Result<Msg, ReadError> {
        let n = self.u32()? as usize;
        if n > self.0.len() {
            return Err(ReadError::Malformed("info key"));
        }
        let (k, v) = self.0.split_at(n);
        Ok(Msg::SetInfo {
            key: String::from_utf8_lossy(k).into_owned(),
            value: String::from_utf8_lossy(v).into_owned(),
        })
    }
    fn description(&mut self) -> Result<Msg, ReadError> {
        let mut fields = Vec::new();
        while !self.0.is_empty() {
            let k = self.text("description field")?;
            let v = self.text("description field")?;
            fields.push((k, v));
        }
        Ok(Msg::Description { fields })
    }
    /// Text behind its `u32` length, read lossily.
    fn text(&mut self, what: &'static str) -> Result<String, ReadError> {
        let n = self.u32()? as usize;
        if n > self.0.len() {
            return Err(ReadError::Malformed(what));
        }
        let (s, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(String::from_utf8_lossy(s).into_owned())
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
                token: Vec::new(),
            },
            Msg::Hello {
                version: VERSION,
                role: Role::Reader,
                token: b"token".to_vec(),
            },
            Msg::Hello {
                version: VERSION,
                role: Role::Lookup,
                token: Vec::new(),
            },
            Msg::Release {
                token: b"token".to_vec(),
            },
            Msg::SetInfo {
                key: "owner_mcp".into(),
                value: "/tmp/spyc/mcp.sock".into(),
            },
            Msg::Input {
                bytes: b"\x1b[A".to_vec(),
            },
            Msg::Reply {
                bytes: b"\x1b[?62c".to_vec(),
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
                reason: reason::OWNER_PRESENT,
                why: "an owner is connected".into(),
            },
            Msg::Hello {
                version: VERSION,
                role: Role::Program,
                token: Vec::new(),
            },
            Msg::Detach,
            Msg::Attached,
            Msg::Detached,
            Msg::Unsupported { kind: 200 },
            Msg::Foreground { pgid: 4242 },
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

/// `SPYC_JAR_SEED_DIR=fuzz/seeds/jar_frame cargo test -p spyc-jar --lib
/// write_fuzz_seed -- --ignored` refreshes the fuzz seed: an owner's `hello`,
/// then `input`, then `upgrade`, as one stream.
#[cfg(test)]
#[test]
#[ignore = "writes the fuzz seed"]
fn write_fuzz_seed() {
    let dir = std::env::var("SPYC_JAR_SEED_DIR").expect("SPYC_JAR_SEED_DIR");
    let stream = [
        Msg::Hello {
            version: VERSION,
            role: Role::Owner,
            token: b"0123456789abcdef".to_vec(),
        },
        Msg::Input {
            bytes: b"ls\r".to_vec(),
        },
        Msg::Upgrade {
            binary: "/tmp/jar-next".into(),
        },
    ]
    .iter()
    .flat_map(encode)
    .collect::<Vec<u8>>();
    std::fs::write(
        std::path::PathBuf::from(dir).join("hello_input_upgrade.bin"),
        stream,
    )
    .expect("write the seed");
}

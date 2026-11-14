//! The blob a jar hands the binary that replaces it across `exec` (R33).
//!
//! The new binary validates it first, as `spyc-jar validate-handover`
//! with the blob on standard input, and the old jar `exec`s only when that
//! succeeds. A malformed blob that got through would take over a live PTY, so
//! [`decode`] checks every count against what is left, every offset against
//! the log, and every connection against the descriptors that came with it,
//! and never panics.
//!
//! Layout, big-endian throughout: the magic, a `u16` version, then the jar's
//! configuration, its program, its log, its state and its connections. The
//! descriptors travel beside it, in order: the listener, the PTY master, then
//! one per connection.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::proto::{Role, Snapshot};
use crate::state::{Config, ConnState, Event, Log, State};

pub const MAGIC: &[u8; 8] = b"SPYCJARH";
pub const VERSION: u16 = 1;
/// The most connections a blob may carry: one `SCM_RIGHTS` message holds them
/// all, and Linux caps one at 253 descriptors.
pub const MAX_CONNS: usize = 200;

/// Everything a jar is, apart from its descriptors.
pub struct Handover {
    pub(crate) cfg: Config,
    pub(crate) child_pid: u32,
    pub(crate) state: State,
    pub(crate) conns: Vec<ConnState>,
}

/// Why a blob was refused.
#[derive(Debug)]
pub struct Invalid(pub &'static str);

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid handover blob: {}", self.0)
    }
}

impl std::error::Error for Invalid {}

/// Whether `blob` is one this binary would take over from, as
/// `validate-handover` asks.
pub fn validate(blob: &[u8]) -> Result<(), Invalid> {
    decode(blob).map(|_| ())
}

pub fn encode(h: &Handover) -> Vec<u8> {
    encode_parts(&h.cfg, h.child_pid, &h.state, &h.conns)
}

pub(crate) fn encode_parts(
    cfg: &Config,
    child_pid: u32,
    st: &State,
    conns: &[ConnState],
) -> Vec<u8> {
    let mut w = Vec::new();
    w.extend(MAGIC);
    put_u16(&mut w, VERSION);
    let c = cfg;
    put_bytes(&mut w, c.socket.as_os_str().as_encoded_bytes());
    put_u32(&mut w, u32::try_from(c.argv.len()).unwrap_or(u32::MAX));
    for a in &c.argv {
        put_bytes(&mut w, a.as_bytes());
    }
    match &c.cwd {
        Some(d) => {
            w.push(1);
            put_bytes(&mut w, d.as_os_str().as_encoded_bytes());
        }
        None => w.push(0),
    }
    put_u16(&mut w, c.rows);
    put_u16(&mut w, c.cols);
    put_u64(&mut w, c.ring_cap as u64);
    put_u64(&mut w, c.window);
    put_u64(&mut w, millis(c.deadline));
    put_u64(&mut w, millis(c.linger_after_exit));
    put_opt_u64(&mut w, c.linger.map(millis));
    put_u64(&mut w, c.instance);
    put_u64(&mut w, c.created);
    put_u32(&mut w, child_pid);

    put_u64(&mut w, st.log.next_seq);
    put_u64(&mut w, st.log.head);
    put_u64(&mut w, st.log.base);
    put_u32(
        &mut w,
        u32::try_from(st.log.events.len()).unwrap_or(u32::MAX),
    );
    for (seq, e) in &st.log.events {
        put_u64(&mut w, *seq);
        match e {
            Event::Out(o, b) => {
                w.push(0);
                put_u64(&mut w, *o);
                put_bytes(&mut w, b);
            }
            Event::Resized(o, r, c) => {
                w.push(1);
                put_u64(&mut w, *o);
                put_u16(&mut w, *r);
                put_u16(&mut w, *c);
            }
        }
    }
    put_opt_u64(&mut w, st.owner);
    put_u64(&mut w, st.acked);
    put_u32(
        &mut w,
        u32::try_from(st.snapshots.len()).unwrap_or(u32::MAX),
    );
    for s in &st.snapshots {
        put_u16(&mut w, s.format);
        put_u64(&mut w, s.offset);
        put_u16(&mut w, s.rows);
        put_u16(&mut w, s.cols);
        put_bytes(&mut w, &s.blob);
    }
    match st.exit {
        Some((code, signaled)) => {
            w.push(1);
            w.extend(code.to_be_bytes());
            w.push(u8::from(signaled));
        }
        None => w.push(0),
    }
    w.push(u8::from(st.exit_acked));
    w.push(u8::from(st.pty_eof));
    put_u16(&mut w, st.rows);
    put_u16(&mut w, st.cols);
    put_u64(&mut w, st.next_conn);
    put_opt_u64(&mut w, st.ownerless_since.map(|t| millis(t.elapsed())));
    put_u32(&mut w, st.fg);
    w.push(u8::from(st.had_owner));
    put_bytes(&mut w, &st.token);
    put_u32(&mut w, u32::try_from(st.info.len()).unwrap_or(u32::MAX));
    for (k, v) in &st.info {
        put_bytes(&mut w, k.as_bytes());
        put_bytes(&mut w, v.as_bytes());
    }

    put_u32(&mut w, u32::try_from(conns.len()).unwrap_or(u32::MAX));
    for c in conns {
        put_u64(&mut w, c.id);
        w.push(c.role as u8);
        match c.cursor {
            Some((seq, from)) => {
                w.push(1);
                put_u64(&mut w, seq);
                put_u64(&mut w, from);
            }
            None => w.push(0),
        }
        w.push(u8::from(c.exit_sent));
        put_u32(&mut w, c.fg_sent);
    }
    w
}

pub fn decode(blob: &[u8]) -> Result<Handover, Invalid> {
    let mut r = Reader(blob);
    if r.take::<8>()? != *MAGIC {
        return Err(Invalid("not a handover blob"));
    }
    if r.u16()? != VERSION {
        return Err(Invalid("a handover version this binary doesn't take"));
    }
    let cfg = r.config()?;
    let child_pid = r.u32()?;
    if child_pid == 0 || i32::try_from(child_pid).is_err() {
        return Err(Invalid("program pid"));
    }
    let log = r.log(cfg.ring_cap)?;
    let state = r.state(log)?;
    let conns = r.conns(&state)?;
    if !r.0.is_empty() {
        return Err(Invalid("bytes after the end"));
    }
    if let Some(o) = state.owner
        && !conns.iter().any(|c| c.id == o && c.role == Role::Owner)
    {
        return Err(Invalid("the owner isn't one of the connections"));
    }
    Ok(Handover {
        cfg,
        child_pid,
        state,
        conns,
    })
}

/// The blob's sections, in the order [`encode_parts`] writes them.
impl Reader<'_> {
    fn config(&mut self) -> Result<Config, Invalid> {
        let socket = PathBuf::from(self.string()?);
        let n_args = self.count(4)?;
        if n_args == 0 {
            return Err(Invalid("no program"));
        }
        let argv = (0..n_args)
            .map(|_| self.string())
            .collect::<Result<Vec<_>, _>>()?;
        let cwd = match self.u8()? {
            0 => None,
            1 => Some(PathBuf::from(self.string()?)),
            _ => return Err(Invalid("cwd flag")),
        };
        Ok(Config {
            socket,
            argv,
            cwd,
            rows: self.u16()?,
            cols: self.u16()?,
            ring_cap: usize::try_from(self.u64()?).map_err(|_| Invalid("ring size"))?,
            window: self.u64()?,
            deadline: Duration::from_millis(self.u64()?),
            linger_after_exit: Duration::from_millis(self.u64()?),
            linger: self.opt_u64()?.map(Duration::from_millis),
            instance: self.u64()?,
            created: self.u64()?,
        })
    }

    /// The log, whose outputs must tile it from its base to its head.
    fn log(&mut self, cap: usize) -> Result<Log, Invalid> {
        let next_seq = self.u64()?;
        let head = self.u64()?;
        let base = self.u64()?;
        if base > head {
            return Err(Invalid("log base past its head"));
        }
        let n_events = self.count(17)?;
        let mut events = VecDeque::with_capacity(n_events);
        let (mut last_seq, mut at, mut bytes_held) = (None::<u64>, base, 0usize);
        for _ in 0..n_events {
            let seq = self.u64()?;
            if last_seq.is_some_and(|l| seq <= l) || seq >= next_seq {
                return Err(Invalid("event sequence"));
            }
            last_seq = Some(seq);
            let e = match self.u8()? {
                0 => {
                    let o = self.u64()?;
                    let b = self.bytes()?;
                    // Outputs tile the log from its base, in order.
                    if o != at {
                        return Err(Invalid("output offset"));
                    }
                    at = o
                        .checked_add(b.len() as u64)
                        .ok_or(Invalid("output offset"))?;
                    bytes_held += b.len();
                    Event::Out(o, b)
                }
                1 => {
                    let o = self.u64()?;
                    if o < at || o > head {
                        return Err(Invalid("resize offset"));
                    }
                    Event::Resized(o, self.u16()?, self.u16()?)
                }
                _ => return Err(Invalid("event kind")),
            };
            events.push_back((seq, e));
        }
        if at != head {
            return Err(Invalid("log doesn't reach its head"));
        }
        Ok(Log {
            events,
            next_seq,
            head,
            base,
            held: bytes_held,
            cap,
        })
    }

    /// The lease, the snapshots, the exit and the rest of the jar's state.
    fn state(&mut self, log: Log) -> Result<State, Invalid> {
        let owner = self.opt_u64()?;
        let acked = self.u64()?;
        if acked > log.head {
            return Err(Invalid("acknowledged past the head"));
        }
        let snapshots = self.snapshots(log.head)?;
        let exit = match self.u8()? {
            0 => None,
            1 => Some((i32::from_be_bytes(self.take()?), self.flag()?)),
            _ => return Err(Invalid("exit flag")),
        };
        let exit_acked = self.flag()?;
        let pty_eof = self.flag()?;
        let rows = self.u16()?;
        let cols = self.u16()?;
        let next_conn = self.u64()?;
        let now = Instant::now();
        let ownerless_since = self
            .opt_u64()?
            .map(|ms| now.checked_sub(Duration::from_millis(ms)).unwrap_or(now));
        let fg = self.u32()?;
        let had_owner = self.flag()?;
        let token = self.bytes()?;
        if token.is_empty() {
            return Err(Invalid("no token"));
        }
        let n_info = self.count(8)?;
        let info = (0..n_info)
            .map(|_| Ok((self.string()?, self.string()?)))
            .collect::<Result<Vec<_>, Invalid>>()?;
        Ok(State {
            log,
            owner,
            acked,
            snapshots,
            exit,
            exit_acked,
            pty_eof,
            rows,
            cols,
            next_conn,
            ownerless_since,
            fg,
            had_owner,
            token,
            info,
        })
    }

    fn snapshots(&mut self, head: u64) -> Result<Vec<Snapshot>, Invalid> {
        let n = self.count(18)?;
        let mut snapshots = Vec::with_capacity(n);
        for _ in 0..n {
            let s = Snapshot {
                format: self.u16()?,
                offset: self.u64()?,
                rows: self.u16()?,
                cols: self.u16()?,
                blob: self.bytes()?,
            };
            if s.offset > head {
                return Err(Invalid("snapshot past the head"));
            }
            snapshots.push(s);
        }
        Ok(snapshots)
    }

    /// The connections, each with an id the jar handed out and a cursor
    /// inside its log.
    fn conns(&mut self, st: &State) -> Result<Vec<ConnState>, Invalid> {
        let n = self.count(15)?;
        if n > MAX_CONNS {
            return Err(Invalid("too many connections"));
        }
        let mut conns: Vec<ConnState> = Vec::with_capacity(n);
        for _ in 0..n {
            let id = self.u64()?;
            if id >= st.next_conn || conns.iter().any(|c| c.id == id) {
                return Err(Invalid("connection id"));
            }
            let role = Role::from_wire(self.u8()?).ok_or(Invalid("connection role"))?;
            let cursor = match self.u8()? {
                0 => None,
                1 => {
                    let (seq, from) = (self.u64()?, self.u64()?);
                    if seq > st.log.next_seq || from > st.log.head {
                        return Err(Invalid("connection cursor"));
                    }
                    Some((seq, from))
                }
                _ => return Err(Invalid("cursor flag")),
            };
            conns.push(ConnState {
                id,
                role,
                cursor,
                exit_sent: self.flag()?,
                fg_sent: self.u32()?,
            });
        }
        Ok(conns)
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn put_u16(w: &mut Vec<u8>, v: u16) {
    w.extend(v.to_be_bytes());
}
fn put_u32(w: &mut Vec<u8>, v: u32) {
    w.extend(v.to_be_bytes());
}
fn put_u64(w: &mut Vec<u8>, v: u64) {
    w.extend(v.to_be_bytes());
}
fn put_opt_u64(w: &mut Vec<u8>, v: Option<u64>) {
    match v {
        Some(v) => {
            w.push(1);
            put_u64(w, v);
        }
        None => w.push(0),
    }
}
fn put_bytes(w: &mut Vec<u8>, b: &[u8]) {
    put_u32(w, u32::try_from(b.len()).unwrap_or(u32::MAX));
    w.extend(b);
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Invalid> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .ok_or(Invalid("truncated"))?;
        self.0 = rest;
        Ok(*head)
    }
    fn u8(&mut self) -> Result<u8, Invalid> {
        Ok(self.take::<1>()?[0])
    }
    fn flag(&mut self) -> Result<bool, Invalid> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Invalid("flag")),
        }
    }
    fn u16(&mut self) -> Result<u16, Invalid> {
        Ok(u16::from_be_bytes(self.take()?))
    }
    fn u32(&mut self) -> Result<u32, Invalid> {
        Ok(u32::from_be_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, Invalid> {
        Ok(u64::from_be_bytes(self.take()?))
    }
    fn opt_u64(&mut self) -> Result<Option<u64>, Invalid> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(Invalid("option flag")),
        }
    }
    /// A count of items at least `min` bytes each: refused when what's left
    /// can't hold that many, so a forged count never allocates.
    fn count(&mut self, min: usize) -> Result<usize, Invalid> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min) > self.0.len() {
            return Err(Invalid("count larger than the blob"));
        }
        Ok(n)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, Invalid> {
        let n = self.u32()? as usize;
        if n > self.0.len() {
            return Err(Invalid("length larger than the blob"));
        }
        let (b, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(b.to_vec())
    }
    fn string(&mut self) -> Result<String, Invalid> {
        String::from_utf8(self.bytes()?).map_err(|_| Invalid("not UTF-8"))
    }
}

/// A valid blob with something in every field: three outputs, a resize, two
/// snapshots, an exit and two connections, one of them the owner. The unit
/// tests and the fuzz seeds start from it.
pub fn sample_blob() -> Vec<u8> {
    let cfg = Config {
        socket: PathBuf::from("/tmp/spyc/jars/sample.sock"),
        argv: vec!["/bin/sh".into(), "-c".into(), "echo hi".into()],
        cwd: Some(PathBuf::from("/tmp")),
        rows: 40,
        cols: 120,
        ring_cap: 4 << 20,
        window: 1 << 20,
        deadline: Duration::from_secs(10),
        linger_after_exit: Duration::from_secs(60),
        linger: Some(Duration::from_secs(300)),
        instance: 0x1234_5678_9abc_def0,
        created: 1_794_000_000,
    };
    let mut events = VecDeque::new();
    events.push_back((3, Event::Out(100, b"hello ".to_vec())));
    events.push_back((4, Event::Resized(106, 30, 100)));
    events.push_back((5, Event::Out(106, b"world\r\n".to_vec())));
    events.push_back((6, Event::Out(113, b"\x1b[1mbold\x1b[0m".to_vec())));
    let state = State {
        log: Log {
            events,
            next_seq: 7,
            head: 125,
            base: 100,
            held: 25,
            cap: 4 << 20,
        },
        owner: Some(2),
        acked: 110,
        snapshots: vec![
            Snapshot {
                format: crate::proto::SNAPSHOT_BINARY,
                offset: 106,
                rows: 30,
                cols: 100,
                blob: vec![1, 2, 3],
            },
            Snapshot {
                format: crate::proto::SNAPSHOT_VT,
                offset: 106,
                rows: 30,
                cols: 100,
                blob: b"\x1b[Hhello ".to_vec(),
            },
        ],
        exit: Some((0, false)),
        exit_acked: false,
        pty_eof: true,
        rows: 30,
        cols: 100,
        next_conn: 4,
        ownerless_since: None,
        fg: 4242,
        had_owner: true,
        token: b"0123456789abcdef".to_vec(),
        info: vec![("owner_mcp".into(), "/tmp/spyc/mcp.sock".into())],
    };
    let conns = vec![
        ConnState {
            id: 2,
            role: Role::Owner,
            cursor: Some((7, 125)),
            exit_sent: false,
            fg_sent: 4242,
        },
        ConnState {
            id: 3,
            role: Role::Reader,
            cursor: None,
            exit_sent: false,
            fg_sent: 0,
        },
    ];
    encode_parts(&cfg, 4321, &state, &conns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_round_trips() {
        let blob = sample_blob();
        let h = decode(&blob).expect("the sample decodes");
        assert_eq!(encode(&h), blob);
    }

    #[test]
    fn every_truncation_is_refused() {
        let blob = sample_blob();
        for n in 0..blob.len() {
            assert!(decode(&blob[..n]).is_err(), "a blob cut at {n} decoded");
        }
        let mut longer = blob;
        longer.push(0);
        assert!(decode(&longer).is_err(), "trailing bytes decoded");
    }

    #[test]
    fn mutations_never_panic_and_what_decodes_re_encodes() {
        let blob = sample_blob();
        let mut seed: u64 = 0x5eed;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 33) as usize
        };
        for _ in 0..50_000 {
            let mut b = blob.clone();
            for _ in 0..=(next() % 4) {
                let i = next() % b.len();
                match next() % 3 {
                    0 => b[i] = next() as u8,
                    1 => {
                        b.remove(i);
                    }
                    _ => b.insert(i, next() as u8),
                }
            }
            if let Ok(h) = decode(&b) {
                decode(&encode(&h)).expect("what decodes, re-encodes");
            }
        }
    }

    #[test]
    fn a_forged_count_is_refused_without_allocating() {
        // The argv count sits after the magic, the version and the socket.
        let blob = sample_blob();
        let at = 8 + 2 + 4 + "/tmp/spyc/jars/sample.sock".len();
        let mut b = blob;
        b[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            decode(&b),
            Err(Invalid("count larger than the blob"))
        ));
    }

    #[test]
    fn an_owner_missing_from_the_connections_is_refused() {
        let mut h = decode(&sample_blob()).expect("decodes");
        h.state.owner = Some(3);
        assert!(decode(&encode(&h)).is_err());
        h.state.owner = None;
        assert!(decode(&encode(&h)).is_ok());
    }
}

/// `SPYC_JAR_SEED_DIR=fuzz/seeds/jar_handover cargo test -p spyc-jar --lib
/// write_fuzz_seed -- --ignored` refreshes the fuzz seed.
#[cfg(test)]
#[test]
#[ignore = "writes the fuzz seed"]
fn write_fuzz_seed() {
    let dir = std::env::var("SPYC_JAR_SEED_DIR").expect("SPYC_JAR_SEED_DIR");
    std::fs::write(PathBuf::from(dir).join("sample.bin"), sample_blob()).expect("write the seed");
}

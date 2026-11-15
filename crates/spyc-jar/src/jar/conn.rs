//! A connection after its `hello`: its reader, which acts on what the client
//! sends as its role allows, and its writer, which streams it the log from
//! its cursor.

use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::accept::tell_program;
use super::describe::describe;
use super::life::deliver;
use super::{By, Conn, Shared, TICK, Who, readable};
use crate::proto::{self, Msg, Role, reason};
use crate::state::{Event, State};

/// The program asked to be detached: end the owner's connection once
/// everything the program printed until now has gone out on it, or after a
/// second, so its last words (its terminal restored) reach the client.
pub(super) fn detach_owner(shared: &Arc<Shared>) {
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        // What the program wrote before asking is in the PTY by now, but
        // perhaps not yet in the log: wait for the PTY to drain first.
        let until = Instant::now() + Duration::from_millis(500);
        while readable(&shared.master, Duration::ZERO) && Instant::now() < until {
            thread::sleep(Duration::from_millis(5));
        }
        let (owner, head) = {
            let st = shared.lock();
            (st.owner, st.log.head)
        };
        let Some(conn) = owner.and_then(|id| shared.conns().get(&id).cloned()) else {
            return;
        };
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            let sent = conn
                .cursor
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some_and(|(_, from)| from >= head);
            if sent {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        conn.close();
    });
}

/// A connection after its `hello`: a writer thread streaming the log, and
/// this thread reading what the client sends. A lookup has no writer.
pub(super) fn run_conn(shared: &Arc<Shared>, id: u64, conn: &Arc<Conn>) {
    let owner = conn.role == Role::Owner;
    let lookup = conn.role == Role::Lookup;
    let writer = (!lookup).then(|| {
        let shared = Arc::clone(shared);
        let conn = Arc::clone(conn);
        thread::spawn(move || {
            stream_log(&shared, id, &conn);
            conn.writer_done.store(true, Ordering::SeqCst);
            shared.park_cv.notify_all();
        })
    });
    let send = |msg: &Msg| conn.send(shared, msg);
    let mut last_heard = Instant::now();
    loop {
        if shared.upgrading() {
            shared.park(Who::ConnRead(id));
            last_heard = Instant::now();
            continue;
        }
        if !readable(&*conn.sock, TICK) {
            if owner && last_heard.elapsed() >= shared.cfg.deadline {
                break;
            }
            // A lookup asks one thing, at once.
            if lookup && last_heard.elapsed() >= LOOKUP_DEADLINE {
                break;
            }
            continue;
        }
        // Silence between frames is timed by `last_heard`; a frame that
        // has begun arrives whole by its deadline.
        let msg = match proto::read_msg(&mut By::frame(&conn.sock)) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(proto::ReadError::UnknownCritical(kind)) => {
                send(&Msg::Unsupported { kind });
                continue;
            }
            Err(_) => break,
        };
        last_heard = Instant::now();
        let permitted = match conn.role {
            Role::Lookup => matches!(msg, Msg::Describe | Msg::Ping { .. }),
            Role::Program => matches!(msg, Msg::Describe | Msg::Ping { .. } | Msg::Detach),
            Role::Owner | Role::Reader => !matches!(msg, Msg::Detach),
        };
        if !permitted {
            send(&Msg::Refused {
                reason: reason::ROLE_NOT_PERMITTED,
                why: format!("a {:?} connection can't send that", conn.role),
            });
            continue;
        }
        if matches!(act(shared, id, conn, msg), Next::End) {
            break;
        }
    }
    conn.close();
    release(shared, id);
    shared.cv.notify_all();
    if let Some(writer) = writer {
        let _ = writer.join();
    }
}

/// What the reader does after a message.
enum Next {
    Read,
    End,
}

/// Act on one message a connection sent, as its role allows.
fn act(shared: &Arc<Shared>, id: u64, conn: &Arc<Conn>, msg: Msg) -> Next {
    let send = |msg: &Msg| conn.send(shared, msg);
    let owner = conn.role == Role::Owner;
    let lookup = conn.role == Role::Lookup;
    let program = conn.role == Role::Program;
    match msg {
        Msg::Replay { from: asked } => {
            let st = shared.lock();
            let gap_to = (asked < st.log.base).then_some(st.log.base);
            let from = asked.max(st.log.base).min(st.log.head);
            let seq = st.log.seq_for(from);
            drop(st);
            if let Some(to) = gap_to {
                send(&Msg::Gap { from: asked, to });
            }
            *conn.cursor.lock().unwrap_or_else(PoisonError::into_inner) = Some((seq, from));
            shared.cv.notify_all();
        }
        Msg::Ping { n } => {
            send(&Msg::Pong { n });
            if lookup {
                return Next::End;
            }
        }
        Msg::Detach => detach_owner(shared),
        // Every snapshot held, then `Snapshot(None)` to end the list.
        Msg::FetchSnapshot => {
            let snaps = shared.lock().snapshots.clone();
            for s in snaps {
                send(&Msg::Snapshot(Some(s)));
            }
            send(&Msg::Snapshot(None));
        }
        Msg::Upgrade { binary } => crate::upgrade::begin(shared, Arc::clone(conn), binary),
        Msg::Release { token } if owner && token.len() < MIN_TOKEN => {
            send(&Msg::Refused {
                reason: reason::INVALID,
                why: format!("a token is {MIN_TOKEN} bytes or more"),
            });
        }
        Msg::Release { token } if owner => {
            let mut st = shared.lock();
            st.token = token;
            // The successor gets the time a first owner gets before the
            // linger starts (finding 17): a pane that doesn't linger
            // survives being handed on.
            st.had_owner = false;
            drop(st);
            // The old token's holders go with it: every reader is closed.
            for (other, c) in shared.conns().iter() {
                if *other != id && c.role == Role::Reader {
                    c.close();
                }
            }
            // Nothing this connection sends after its release may act on
            // the jar: whatever follows, a `Kill` from its tab closing
            // among them, is never read.
            return Next::End;
        }
        Msg::SetInfo { key, value } if owner => {
            let mut st = shared.lock();
            st.info.retain(|(k, _)| *k != key);
            st.info.push((key, value));
        }
        Msg::Describe => {
            send(&Msg::Description {
                fields: describe(shared),
            });
            if lookup {
                return Next::End;
            }
        }
        _ if !owner || program => {
            send(&Msg::Refused {
                reason: reason::ROLE_NOT_PERMITTED,
                why: "a reader can only watch".into(),
            });
        }
        other => act_as_owner(shared, id, conn, other),
    }
    Next::Read
}

/// The messages only the owner may send: they reach the program.
fn act_as_owner(shared: &Shared, id: u64, conn: &Conn, msg: Msg) {
    let send = |msg: &Msg| conn.send(shared, msg);
    match msg {
        Msg::Input { bytes } | Msg::Reply { bytes } => {
            if !shared.queue_input(id, &bytes) {
                send(&Msg::Refused {
                    reason: reason::INVALID,
                    why: "the program isn't reading its input".into(),
                });
            }
        }
        Msg::Resize {
            rows,
            cols,
            px_w,
            px_h,
        } => {
            let size = rustix::termios::Winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: px_w,
                ws_ypixel: px_h,
            };
            if rustix::termios::tcsetwinsize(&shared.master, size).is_ok() {
                let mut st = shared.lock();
                st.rows = rows;
                st.cols = cols;
                let at = st.log.head;
                st.log.push(Event::Resized(at, rows, cols));
                drop(st);
                shared.cv.notify_all();
            }
        }
        Msg::Ack { offset } => {
            let mut st = shared.lock();
            st.acked = st.acked.max(offset.min(st.log.head));
            drop(st);
            shared.cv.notify_all();
        }
        Msg::Signal { .. } if shared.lock().exit.is_some() => {
            send(&Msg::Refused {
                reason: reason::PROGRAM_EXITED,
                why: "the program has ended".into(),
            });
        }
        Msg::Signal { signal } => deliver(shared.child_pid(), signal),
        // Handed back, a snapshot takes one byte more than it was stored in:
        // one that would come back over the limit is refused now, rather
        // than end the connection that fetches it.
        Msg::StoreSnapshot(snap)
            if proto::encode(&Msg::Snapshot(Some(snap.clone()))).len() - 4 > proto::MAX_FRAME =>
        {
            send(&Msg::Refused {
                reason: reason::INVALID,
                why: "a snapshot too big to hand back".into(),
            });
        }
        Msg::StoreSnapshot(snap) if snap.offset > shared.lock().log.head => {
            send(&Msg::Refused {
                reason: reason::INVALID,
                why: "a snapshot past what the program printed".into(),
            });
        }
        Msg::StoreSnapshot(snap) => {
            let mut st = shared.lock();
            st.snapshots.retain(|s| s.format != snap.format);
            st.snapshots.push(snap);
            // Keep the bytes after the oldest snapshot: either may be used.
            let keep = st.snapshots.iter().map(|s| s.offset).min().unwrap_or(0);
            let keep = keep.min(st.log.head);
            st.log.trim(keep);
        }
        Msg::AckExit => {
            shared.lock().exit_acked = true;
            shared.cv.notify_all();
        }
        other => {
            send(&Msg::Refused {
                reason: reason::INVALID,
                why: format!("not a client message: {other:?}"),
            });
        }
    }
}

/// A connection ended: it leaves the registry, and if it owned the jar, the
/// jar has no owner now.
pub(super) fn release(shared: &Shared, id: u64) {
    shared.conns().remove(&id);
    let mut st = shared.lock();
    let was_owner = st.owner == Some(id);
    if was_owner {
        st.owner = None;
        st.ownerless_since = Some(Instant::now());
        st.info.retain(|(k, _)| k != "owner_mcp");
    }
    drop(st);
    if was_owner {
        shared.drop_input(id);
        tell_program(shared, &Msg::Detached);
    }
    shared.cv.notify_all();
    shared.park_cv.notify_all();
}

/// How long a lookup may wait to ask its one question, or to take its answer.
pub(super) const LOOKUP_DEADLINE: Duration = Duration::from_secs(5);

/// The shortest token a release may set (R26): an empty one would open the
/// jar to anyone.
pub const MIN_TOKEN: usize = 16;

/// Stream the log to one connection from its cursor, after whatever waits in
/// its outbox, then the exit once everything before it has gone out.
///
/// A write that makes no progress for the jar's deadline ends the
/// connection, and the cursor moves only after its bytes have gone, so a
/// parked writer's cursor is exactly what was sent.
fn stream_log(shared: &Shared, id: u64, conn: &Conn) {
    let _ = conn.sock.set_write_timeout(Some(shared.cfg.deadline));
    loop {
        let Some((queued, batch)) = next_batch(shared, id, conn) else {
            return;
        };
        let mut bytes = queued;
        if let Some((frames, _)) = &batch {
            for f in frames {
                bytes.extend(proto::encode(f));
            }
        }
        if (&*conn.sock).write_all(&bytes).is_err() {
            conn.close();
            return;
        }
        let Some((frames, next)) = batch else {
            continue;
        };
        if frames.iter().any(|f| matches!(f, Msg::Exited { .. })) {
            conn.exit_sent.store(true, Ordering::SeqCst);
        }
        let mut c = conn.cursor.lock().unwrap_or_else(PoisonError::into_inner);
        if c.is_some_and(|(seq, _)| seq <= next.0) {
            *c = Some(next);
        }
    }
}

/// Frames for the connection's next write, and where its cursor stands after
/// them: what waits in its outbox, then the log from its cursor. Waits until
/// there is something, parking for an upgrade; `None` once the connection has
/// closed.
#[expect(
    clippy::type_complexity,
    reason = "the outbox's bytes, then the log's frames and the cursor after them"
)]
fn next_batch(
    shared: &Shared,
    id: u64,
    conn: &Conn,
) -> Option<(Vec<u8>, Option<(Vec<Msg>, (u64, u64))>)> {
    let mut st = shared.lock();
    loop {
        if conn.closed.load(Ordering::SeqCst) {
            return None;
        }
        if shared.upgrading() {
            drop(st);
            shared.park(Who::ConnWrite(id));
            st = shared.lock();
            continue;
        }
        let queued = conn.take_outbox();
        let pos = *conn.cursor.lock().unwrap_or_else(PoisonError::into_inner);
        let mut batch = None;
        if let Some((seq, from)) = pos {
            // A reader that fell behind the ring is told what it missed (R12a),
            // as a replay from before the ring is, and goes on from the oldest
            // byte still held.
            let missed = (from < st.log.base).then_some(Msg::Gap {
                from,
                to: st.log.base,
            });
            let (seq, from) = if missed.is_some() {
                (st.log.seq_for(st.log.base), st.log.base)
            } else {
                (seq, from)
            };
            let pending = seq < st.log.next_seq;
            let exit_due = !conn.exit_sent.load(Ordering::SeqCst)
                && st.exit.is_some()
                && st.pty_eof
                && !pending;
            let fg_due = st.fg != conn.fg_sent.load(Ordering::SeqCst);
            if pending || exit_due || fg_due || missed.is_some() {
                let (mut frames, next) =
                    collect(&st, seq, from, exit_due.then(|| st.exit).flatten());
                if fg_due {
                    frames.insert(0, Msg::Foreground { pgid: st.fg });
                    conn.fg_sent.store(st.fg, Ordering::SeqCst);
                }
                frames.splice(0..0, missed);
                batch = Some((frames, next));
            }
        }
        if !queued.is_empty() || batch.is_some() {
            return Some((queued, batch));
        }
        st = shared
            .cv
            .wait_timeout(st, Duration::from_millis(500))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// The frames from `seq` (at byte offset `from`) to the end of the log, and
/// where the cursor stands after them.
fn collect(st: &State, seq: u64, from: u64, exit: Option<(i32, bool)>) -> (Vec<Msg>, (u64, u64)) {
    let mut frames = Vec::new();
    let mut at = from;
    for (s, e) in &st.log.events {
        if *s < seq {
            continue;
        }
        match e {
            Event::Out(o, b) => {
                let end = o + b.len() as u64;
                if end <= at {
                    continue;
                }
                let skip = usize::try_from(at.saturating_sub(*o)).unwrap_or(0);
                // Adjacent output goes out as one frame: a PTY hands over
                // small chunks, and a frame each costs the reader syscalls.
                if let Some(Msg::Output { offset, bytes }) = frames.last_mut()
                    && *offset + bytes.len() as u64 == at.max(*o)
                    && bytes.len() < 1 << 20
                {
                    bytes.extend_from_slice(&b[skip..]);
                } else {
                    frames.push(Msg::Output {
                        offset: at.max(*o),
                        bytes: b[skip..].to_vec(),
                    });
                }
                at = end;
            }
            Event::Resized(o, rows, cols) => {
                frames.push(Msg::Resized {
                    offset: *o,
                    rows: *rows,
                    cols: *cols,
                });
                at = at.max(e.offset());
            }
        }
    }
    if let Some((code, signaled)) = exit {
        frames.push(Msg::Exited { code, signaled });
    }
    (frames, (st.log.next_seq, at))
}

//! A jar for tests that speaks the protocol and does nothing else. It
//! welcomes one owner, naming whatever pid the test gives as its program,
//! hands the test each frame it receives, and answers a `Kill` or a
//! `Terminate` with the program's exit, as a jar whose program died does.
//! Given a bystander's pid, it shows whether spyc asked the jar for a signal
//! or sent one to the number (SPYC-TRAP `jar-signals-through-the-jar`).

use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::proto::{self, JarSignal, Msg, Role};

use super::{ReplayGate, Resume, connect, offsets, test_dir};
use crate::pane::pty_host::PtyHost;

/// How long one frame may take to arrive.
pub const WAIT: Duration = Duration::from_secs(5);

/// The jar's side of the one connection, and its directory.
pub struct FakeJar {
    frames: mpsc::Receiver<Msg>,
    to_owner: UnixStream,
    /// Listening, so the jar counts as running. Dropped before the
    /// directory, whose sweep then finds no jar to end.
    _listener: UnixListener,
    _dir: test_dir::TestDir,
}

impl FakeJar {
    /// The next frame that isn't a ping.
    pub fn next(&self) -> Msg {
        let until = Instant::now() + WAIT;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.frames.recv_timeout(left) {
                Ok(Msg::Ping { .. }) => {}
                Ok(m) => return m,
                Err(e) => panic!("the jar heard nothing more: {e}"),
            }
        }
    }

    /// The jar goes, as one that was killed does: its connection closes,
    /// every end of it.
    pub fn hang_up(self) {
        let _ = self.to_owner.shutdown(std::net::Shutdown::Both);
    }

    pub fn send(&mut self, msg: &Msg) {
        proto::write_msg(&mut self.to_owner, msg).expect("the owner is connected");
    }
}

/// A host whose program runs in a fake jar that names `pid` as its program.
pub fn host_in_fake_jar(pid: u32) -> (FakeJar, PtyHost) {
    let dir = test_dir::TestDir::new("jf");
    let socket = dir.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).expect("bind the fake jar's socket");
    let accepting = listener.try_clone().expect("clone the listener");
    let (tx, frames) = mpsc::channel();
    let (conn_tx, conn_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut conn, _)) = accepting.accept() else {
            return;
        };
        drop(accepting);
        let Ok(Some(Msg::Hello { .. })) = proto::read_msg(&mut conn) else {
            return;
        };
        let welcome = Msg::Welcome {
            version: proto::VERSION,
            child_pid: pid,
            head: 0,
            base: 0,
            rows: 24,
            cols: 80,
        };
        if proto::write_msg(&mut conn, &welcome).is_err() {
            return;
        }
        let Ok(mut answer) = conn.try_clone() else {
            return;
        };
        if conn_tx
            .send(conn.try_clone().expect("clone the connection"))
            .is_err()
        {
            return;
        }
        while let Ok(Some(m)) = proto::read_msg(&mut conn) {
            if let Msg::Signal { signal } = m
                && let Some(JarSignal::Kill | JarSignal::Terminate) = JarSignal::from_wire(signal)
            {
                let _ = proto::write_msg(
                    &mut answer,
                    &Msg::Exited {
                        code: 9,
                        signaled: true,
                    },
                );
            }
            if tx.send(m).is_err() {
                break;
            }
        }
    });
    let (client, welcome) =
        Client::connect(&socket, Role::Owner, WAIT, b"token").expect("the fake jar answers");
    let Msg::Welcome { child_pid, .. } = welcome else {
        panic!("no welcome: {welcome:?}");
    };
    let fresh = Resume {
        pid: child_pid,
        replay: 0,
        offsets: offsets::Offsets::new(0),
        gate: ReplayGate::open(),
    };
    let jarred = connect(
        client,
        fresh,
        &socket,
        (24, 80),
        super::repaint::REPAINT_HOLD,
    )
    .expect("take the fake jar");
    let host = PtyHost::around_jar(jarred, (24, 80)).expect("a host around the fake jar");
    let to_owner = conn_rx
        .recv_timeout(WAIT)
        .expect("the fake jar's connection");
    let jar = FakeJar {
        frames,
        to_owner,
        _listener: listener,
        _dir: dir,
    };
    assert_eq!(
        jar.next(),
        Msg::Replay { from: 0 },
        "premise: the owner asks for its log"
    );
    (jar, host)
}

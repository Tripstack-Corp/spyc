//! Q4: an old jar runs under a new spyc. `SPYC_JAR_OLD_BIN` names a `spyc-jar`
//! built from an earlier commit (`spikes/jar-stage-2/old_jar.sh` builds one),
//! and this crate's client, the new spyc's side of the socket, drives it.
//! Ignored by default because it needs that binary:
//! `SPYC_JAR_OLD_BIN=… cargo test -p spyc-jar --test compat -- --ignored`.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::proto::{self, Msg, Role};

const WAIT: Duration = Duration::from_secs(10);

fn frame(kind: u8, flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = u32::try_from(payload.len() + 2)
        .expect("small")
        .to_be_bytes()
        .to_vec();
    f.push(kind);
    f.push(flags);
    f.extend(payload);
    f
}

/// Read until a frame `want` accepts, or fail.
fn until(client: &mut Client, mut want: impl FnMut(&Msg) -> bool) -> Msg {
    let end = Instant::now() + WAIT;
    loop {
        assert!(Instant::now() < end, "timed out");
        if let Some(m) = client.recv(Duration::from_millis(200)).expect("recv")
            && want(&m)
        {
            return m;
        }
    }
}

#[test]
#[ignore = "needs SPYC_JAR_OLD_BIN, a jar built from an earlier commit"]
fn a_new_client_drives_an_old_jar() {
    let old = std::env::var("SPYC_JAR_OLD_BIN").expect("SPYC_JAR_OLD_BIN names an old jar binary");
    let dir = tempfile::Builder::new()
        .prefix("jar")
        .tempdir_in("/tmp")
        .expect("tempdir");
    let socket = dir.path().join("j.sock");
    let mut jar = Command::new(&old)
        .args(["--wire", "1"])
        .arg("--socket")
        .arg(&socket)
        .args(["--", "/bin/sh", "-c", "while read l; do echo got=$l; done"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the old jar");
    let _reap = Reap(&mut jar);

    // What a newer spyc may send in its hello: capabilities after the fields
    // version 1 defines. The old jar reads the fields it knows and welcomes it.
    let mut raw = loop {
        if let Ok(s) = UnixStream::connect(&socket) {
            break s;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    raw.write_all(&frame(
        1,
        proto::CRITICAL,
        &[0, 1, 2, b'c', b'a', b'p', b's'],
    ))
    .expect("hello");
    let mut head = [0u8; 6];
    raw.set_read_timeout(Some(WAIT)).expect("timeout");
    raw.read_exact(&mut head)
        .expect("an answer to a longer hello");
    assert_eq!(
        head[4], 64,
        "the old jar welcomes a hello with extra fields"
    );
    drop(raw);

    let (mut owner, welcome) =
        Client::connect(&socket, Role::Owner, WAIT, b"an-old-jar-ignores-this").expect("connect");
    let Msg::Welcome { child_pid, .. } = welcome else {
        panic!("expected a welcome");
    };
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    owner
        .send(&Msg::Input {
            bytes: b"one\n".to_vec(),
        })
        .expect("input");
    let mut text = Vec::new();
    until(&mut owner, |m| {
        if let Msg::Output { bytes, .. } = m {
            text.extend(bytes);
        }
        String::from_utf8_lossy(&text).contains("got=one")
    });

    // A critical message the old jar doesn't know: refused as unsupported,
    // and the connection carries on.
    owner
        .send(&Msg::Upgrade {
            binary: "/nonexistent".into(),
        })
        .expect("upgrade");
    let m = until(&mut owner, |m| {
        matches!(m, Msg::Unsupported { .. } | Msg::Refused { .. })
    });
    assert!(matches!(m, Msg::Unsupported { kind: 12 }), "{m:?}");

    // An ancillary one it doesn't know: skipped without a word.
    owner
        .send_raw(&frame(60, 0, b"a future notice"))
        .expect("ancillary");
    owner.send(&Msg::Ping { n: 7 }).expect("ping");
    until(&mut owner, |m| matches!(m, Msg::Pong { n: 7 }));
    owner
        .send(&Msg::Input {
            bytes: b"two\n".to_vec(),
        })
        .expect("input");
    until(&mut owner, |m| {
        if let Msg::Output { bytes, .. } = m {
            text.extend(bytes);
        }
        String::from_utf8_lossy(&text).contains("got=two")
    });

    if let Some(pid) = i32::try_from(child_pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
}

/// Ends the jar however the test ends: its program too, through the jar's
/// own exit taking the PTY with it.
struct Reap<'a>(&'a mut std::process::Child);

impl Drop for Reap<'_> {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

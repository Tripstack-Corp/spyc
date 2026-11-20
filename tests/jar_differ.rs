#![cfg(unix)]
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

//! S3: upgrades between this jar and one built to differ, both ways. The
//! other jar is this source built with `--features differ`: wire and
//! handover-blob version 2, and a frame version 1 doesn't know.
//! `SPYC_JAR_DIFFER_BIN` names it; `make test-jar-differ` builds it and runs
//! these, as `make test` and CI do. ("Meeting another version" in
//! `docs/drafts/JAR_PROTOCOL.md`.)

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::proto::{self, Msg, Role, reason};

const WAIT: Duration = Duration::from_secs(60);
const TOKEN: &[u8] = b"differ-test-token-0123456789";

/// A running jar, ended when the test is.
struct Jar {
    _dir: tempfile::TempDir,
    socket: PathBuf,
    process: Child,
}

impl Drop for Jar {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl Jar {
    /// A jar from `binary`, started as spyc starts one, at wire version 1,
    /// running a program that answers each line.
    fn start(binary: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("jd")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let socket = dir.path().join("j.sock");
        let mut process = Command::new(binary)
            .args(["--wire", "1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(
                &spyc_jar::cli::Start {
                    token: TOKEN.to_vec(),
                    socket: socket.clone(),
                    cwd: None,
                    rows: 24,
                    cols: 80,
                    hold: None,
                    exit_linger: None,
                    argv: ["/bin/sh", "-c", "while read l; do echo got=$l; done"]
                        .map(String::from)
                        .to_vec(),
                }
                .encode(),
            )
            .unwrap();
        Self {
            _dir: dir,
            socket,
            process,
        }
    }

    fn connect(&self, role: Role) -> (Client, Msg) {
        Client::connect(&self.socket, role, WAIT, TOKEN).unwrap()
    }

    fn described(&self, key: &str) -> Option<String> {
        spyc_jar::client::describe(&self.socket)
            .ok()?
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
}

fn differ() -> PathBuf {
    PathBuf::from(
        std::env::var("SPYC_JAR_DIFFER_BIN")
            .expect("SPYC_JAR_DIFFER_BIN names a jar built to differ"),
    )
}

fn stock() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_spyc-jar"))
}

/// Send `line` to the program and wait for its answer, acknowledging output
/// as an owner does.
fn echoes(client: &mut Client, line: &str) {
    client
        .send(&Msg::Input {
            bytes: format!("{line}\n").into_bytes(),
        })
        .unwrap();
    let want = format!("got={line}");
    let mut text = Vec::new();
    let until = Instant::now() + WAIT;
    while !String::from_utf8_lossy(&text).contains(&want) {
        assert!(
            Instant::now() < until,
            "no {want}: {:?}",
            String::from_utf8_lossy(&text)
        );
        if let Some(Msg::Output { offset, bytes }) =
            client.recv(Duration::from_millis(100)).unwrap()
        {
            let _ = client.send(&Msg::Ack {
                offset: offset + bytes.len() as u64,
            });
            text.extend(bytes);
        }
    }
}

/// Ask `client`'s jar to upgrade to `binary`: its build, or its refusal.
fn upgrade(client: &mut Client, binary: &Path) -> Result<String, (u16, String)> {
    client
        .send(&Msg::Upgrade {
            binary: binary.display().to_string(),
        })
        .unwrap();
    let until = Instant::now() + WAIT;
    let mut n = 0;
    loop {
        assert!(Instant::now() < until, "no answer to the upgrade");
        n += 1;
        let _ = client.send(&Msg::Ping { n });
        match client.recv(Duration::from_millis(100)).unwrap() {
            Some(Msg::Upgraded { build }) => return Ok(build),
            Some(Msg::Refused { reason, why }) => return Err((reason, why)),
            Some(Msg::Output { offset, bytes }) => {
                let _ = client.send(&Msg::Ack {
                    offset: offset + bytes.len() as u64,
                });
            }
            _ => {}
        }
    }
}

/// A raw `hello` naming `version`, as a lookup; the version the welcome
/// answers.
fn welcomed_at(socket: &Path, version: u16) -> u16 {
    let mut s = UnixStream::connect(socket).unwrap();
    s.set_read_timeout(Some(WAIT)).unwrap();
    let mut payload = version.to_be_bytes().to_vec();
    payload.extend([Role::Lookup as u8, 0, 0]);
    let mut frame = u32::try_from(payload.len() + 2)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    frame.extend([1, proto::CRITICAL]);
    frame.extend(payload);
    s.write_all(&frame).unwrap();
    match proto::read_msg(&mut s) {
        Ok(Some(Msg::Welcome { version, .. })) => version,
        other => panic!("no welcome: {other:?}"),
    }
}

/// This jar answers a `hello` from a newer client at version 1, the one the
/// connection then speaks.
#[test]
fn a_version_1_jar_welcomes_a_newer_hello_at_1() {
    let jar = Jar::start(&stock());
    let _owner = jar.connect(Role::Owner);
    assert_eq!(welcomed_at(&jar.socket, 2), 1);
}

/// Up: a version-1 jar upgrades into one built to differ, with the same
/// program, and its connection goes on at version 1, the differing frame
/// skipped. The new jar speaks version 2 to a client that does.
#[test]
#[ignore = "needs SPYC_JAR_DIFFER_BIN: `make test-jar-differ`"]
fn a_version_1_jar_upgrades_into_one_built_to_differ() {
    let jar = Jar::start(&stock());
    let (mut owner, welcome) = jar.connect(Role::Owner);
    assert!(
        matches!(welcome, Msg::Welcome { version: 1, .. }),
        "{welcome:?}"
    );
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    echoes(&mut owner, "one");
    let program = jar.described("program_pid");
    let pid = jar.described("pid");

    upgrade(&mut owner, &differ()).expect("the upgrade into the jar built to differ");
    echoes(&mut owner, "two");
    assert_eq!(jar.described("program_pid"), program, "the same program");
    assert_eq!(jar.described("pid"), pid, "the same jar");

    // A version-1 client now: welcomed at 1, the differing frame skipped.
    let (mut reader, welcome) = jar.connect(Role::Reader);
    assert!(
        matches!(welcome, Msg::Welcome { version: 1, .. }),
        "{welcome:?}"
    );
    reader.send(&Msg::Replay { from: 0 }).unwrap();
    let until = Instant::now() + WAIT;
    let mut text = Vec::new();
    while !String::from_utf8_lossy(&text).contains("got=two") {
        assert!(Instant::now() < until, "the reader never caught up");
        if let Some(Msg::Output { bytes, .. }) = reader.recv(Duration::from_millis(100)).unwrap() {
            text.extend(bytes);
        }
    }
    assert_eq!(welcomed_at(&jar.socket, 2), 2, "it speaks version 2");
}

/// Down: a jar built to differ won't upgrade into this one, whose validator
/// refuses its version-2 blob; it runs on, and serves a version-1 client
/// throughout.
#[test]
#[ignore = "needs SPYC_JAR_DIFFER_BIN: `make test-jar-differ`"]
fn a_jar_built_to_differ_refuses_a_downgrade_and_runs_on() {
    let jar = Jar::start(&differ());
    let (mut owner, welcome) = jar.connect(Role::Owner);
    assert!(
        matches!(welcome, Msg::Welcome { version: 1, .. }),
        "{welcome:?}"
    );
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    echoes(&mut owner, "one");
    let refused = upgrade(&mut owner, &stock()).expect_err("a downgrade");
    assert_eq!(refused.0, reason::UPGRADE_REJECTED, "{refused:?}");
    echoes(&mut owner, "two");
}

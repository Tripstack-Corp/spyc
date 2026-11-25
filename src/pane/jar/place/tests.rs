//! The checks and sweeps of [`super`]: where this user's jars and sessions
//! are trusted, and which of their sockets and tokens are swept.

use std::path::Path;
use std::time::Duration;

use super::*;

/// A place in `dir`, as `Place::for_this_user` lays one out.
fn place_in(dir: &Path) -> Place {
    Place {
        binary: dir.join("spyc-jar"),
        dirs: vec![dir.join("spyc"), dir.join("spyc").join("jars")],
        tokens: dir.join("tokens"),
        hold: HOLD,
        repaint_hold: Duration::from_secs(2),
    }
}

/// A socket at `path` whose jar is gone: its file left behind, its lock
/// free.
fn stale_socket(path: &Path) {
    drop(std::os::unix::net::UnixListener::bind(path).unwrap());
    std::fs::write(spyc_jar::lock::path(path), b"").unwrap();
    assert_eq!(socket_state(path), SocketState::Stale, "premise");
}

/// A session's jar that has gone is swept as a pane's is (the 3.0 review,
/// E7): its socket, its lock file and its token.
#[test]
fn a_gone_sessions_socket_and_token_are_swept() {
    let dir = super::super::test_dir::TestDir::new("ps");
    let place = place_in(dir.path());
    let sessions = place.prepare_sessions().unwrap();
    let socket = sessions.join("CUMIN_TEST.sock");
    stale_socket(&socket);
    let token = super::super::tokens::new().unwrap();
    super::super::tokens::save(&place, &socket, &token).unwrap();
    assert!(
        super::super::tokens::load(&place, &socket).is_some(),
        "premise"
    );

    sweep(&place, Duration::ZERO);

    assert!(!socket.exists(), "the gone session's socket is left");
    assert!(
        super::super::tokens::load(&place, &socket).is_none(),
        "the gone session's token is kept"
    );
}

/// The jar binary this build made.
fn built_jar() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap();
    super::super::warmed(
        exe.ancestors()
            .skip(1)
            .map(|d| d.join("spyc-jar"))
            .find(|j| j.is_file())
            .expect("a built spyc-jar: make test builds it"),
    )
}

/// A pane's program, `cat`, in a real jar in `place`.
fn jarred(place: &Place, cwd: &Path) -> crate::pane::pty_host::PtyHost {
    crate::pane::pty_host::PtyHost::spawn(crate::pane::pty_host::PtySpec {
        project_env: &[],
        command: "exec cat",
        rows: 24,
        cols: 80,
        cwd,
        env: &[],
        term: "dumb",
        nudge_winch: false,
        debug_dump: false,
        exec_replace: false,
        capture_launch_env: false,
        record: None,
        jar: Some(super::super::Spec {
            place: place.clone(),
            name: "trust-test".into(),
            adopt: None,
            retake: false,
            exit_linger: None,
            watch: None,
        }),
    })
    .unwrap()
}

fn set_mode(dir: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Poll until `done` holds, or fail naming what never happened.
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(std::time::Instant::now() < until, "never: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A jar's directory another user could have made, or could write in, holds
/// no orphan of this spyc's (the 3.0 review, E2): the orphan finder reads
/// it as it reads none.
#[test]
#[ignore = "red until E2: fix/socket-trust"]
fn the_orphan_finder_trusts_no_directory_open_to_others() {
    let dir = super::super::test_dir::TestDir::new("pt");
    let place = Place {
        binary: built_jar(),
        ..place_in(dir.path())
    };
    let host = jarred(&place, dir.path());
    let socket = host.jar_socket().unwrap().to_path_buf();
    host.sever_jar();
    std::mem::forget(host);
    eventually("the jar is an orphan", || {
        super::super::orphans::find(Some(place.clone()))
            .iter()
            .any(|o| o.socket == socket)
    });

    set_mode(place.sockets(), 0o777);
    let found = super::super::orphans::find(Some(place.clone()));
    set_mode(place.sockets(), 0o700);

    assert!(
        found.is_empty(),
        "an orphan found in a directory open to others: {found:?}"
    );
}

/// A retake of a tab whose jar is in a directory another user could write in
/// leaves the tab detached at once, saying why (the 3.0 review, E2): the
/// ownership check's refusal is evidence about the socket, not a silence, so
/// the retake doesn't ask it again for as long as the jar runs (#1399).
#[test]
#[ignore = "red until E2: fix/socket-trust"]
fn a_retake_under_a_directory_open_to_others_is_detached_at_once() {
    use std::io::Read;
    let dir = super::super::test_dir::TestDir::new("pr");
    let place = Place {
        binary: built_jar(),
        ..place_in(dir.path())
    };
    let host = jarred(&place, dir.path());
    let socket = host.jar_socket().unwrap().to_path_buf();
    host.sever_jar();
    std::mem::forget(host);
    let spec = super::super::Spec {
        place: place.clone(),
        name: "trust-test".into(),
        adopt: Some(socket.clone()),
        retake: true,
        exit_linger: None,
        watch: None,
    };
    set_mode(place.sockets(), 0o777);
    let (tx, rx) = std::sync::mpsc::channel();
    let asked = socket.clone();
    std::thread::spawn(move || {
        let back = super::super::take::back(&spec, &asked, (24, 80));
        let _ = tx.send(back.map_err(|e| format!("{e:#}")));
    });
    // The bound only stops the test: a retake that came back has decided.
    let back = rx.recv_timeout(Duration::from_secs(10));
    set_mode(place.sockets(), 0o700);
    // The test's own jar and program: end them, which ends a retake still
    // asking too.
    if let Ok(fields) = spyc_jar::client::describe(&socket) {
        for key in ["program_pid", "pid"] {
            let pid = fields
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| v.parse::<i32>().ok())
                .and_then(rustix::process::Pid::from_raw);
            if let Some(pid) = pid {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
        }
    }
    let jarred = back
        .expect("the retake was still asking: the refusal was taken for a silence")
        .expect("a retake")
        .expect("premise: the jar runs");
    assert!(
        jarred.adoption.is_none() && jarred.signals.is_none(),
        "taken back from a directory open to others"
    );
    let mut screen = String::new();
    jarred
        .master
        .try_clone_reader()
        .unwrap()
        .read_to_string(&mut screen)
        .unwrap();
    assert!(screen.contains("open to others"), "{screen:?}");
}

/// An agent's MCP proxy follows its pane's jar only from a place that is
/// this user's alone, the runtime directory's `spyc` included, as spawn
/// requires (the 3.0 review, E2).
#[test]
#[ignore = "red until E2: fix/socket-trust"]
fn the_mcp_proxy_follows_no_jar_under_a_directory_open_to_others() {
    let dir = super::super::test_dir::TestDir::new("pt");
    crate::state::with_state_root(dir.path(), || {
        let place = Place {
            binary: built_jar(),
            ..place_in(dir.path())
        };
        let host = jarred(&place, dir.path());
        let socket = host.jar_socket().unwrap().to_path_buf();
        eventually("the jar names its owner's MCP socket", || {
            super::super::owner_mcp(&socket).is_some()
        });

        set_mode(&place.dirs[0], 0o777);
        let followed = super::super::owner_mcp(&socket);
        set_mode(&place.dirs[0], 0o700);

        assert_eq!(
            followed, None,
            "followed a jar under a directory open to others"
        );
        drop(host);
    });
}

/// A link in the sessions directory names no session, whatever it points at:
/// only a socket of this user's does (the 3.0 review, E2).
#[test]
#[ignore = "red until E2: fix/socket-trust"]
fn a_link_in_the_sessions_directory_names_no_session() {
    let dir = super::super::test_dir::TestDir::new("pt");
    let place = Place {
        binary: built_jar(),
        ..place_in(dir.path())
    };
    let sessions = place.prepare_sessions().unwrap();
    let host = jarred(&place, dir.path());
    let real = host.jar_socket().unwrap().to_path_buf();
    let link = sessions.join("LINKED_TEST.sock");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(
        socket_state(&link),
        SocketState::Live,
        "premise: it answers"
    );

    let found = super::super::attach::running(&sessions);

    assert!(
        found.iter().all(|r| r.socket != link),
        "a link taken for a session: {found:?}"
    );
    drop(host);
}

/// Every contact with a jar's socket goes through the one ownership check in
/// `place` (the 3.0 review, E2): no production code outside it connects,
/// looks a jar up or attaches by itself. Fails closed: `place` must hold
/// each call it wraps, and the scan must have read spyc's sources.
#[test]
#[ignore = "red until E2: fix/socket-trust"]
fn every_jar_socket_is_reached_through_the_ownership_check() {
    const CALLS: &[&str] = &[
        "Client::connect(",
        "client::describe(",
        "spyc_jar::attach::run(",
        "answers_now(",
    ];
    const PLACE: &str = "src/pane/jar/place.rs";
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files: Vec<String> = crate::guard_support::repo_files(root, &["rs"])
        .into_iter()
        .filter(|p| p.starts_with("src/"))
        .filter(|p| {
            let name = p.rsplit('/').next().unwrap_or("");
            name != "tests.rs"
                && name != "test_support.rs"
                && name != "test_dir.rs"
                && name != "fake.rs"
                && !name.ends_with("_tests.rs")
                && !p.contains("_tests/")
                && !p.contains("/tests/")
        })
        .collect();
    assert!(
        files.len() > 200,
        "only {} source files read: the guard reads the wrong place",
        files.len()
    );
    let mut offenders = Vec::new();
    let mut wrapped = vec![false; CALLS.len()];
    for path in &files {
        let src = std::fs::read_to_string(root.join(path)).unwrap();
        let production = crate::guard_support::production_half(&src);
        for (n, line) in production.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for (i, call) in CALLS.iter().enumerate() {
                if !code.contains(call) {
                    continue;
                }
                if path == PLACE {
                    wrapped[i] = true;
                } else {
                    offenders.push(format!("{path}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a jar's socket reached around the ownership check in {PLACE}:\n{}",
        offenders.join("\n")
    );
    let missing: Vec<_> = CALLS
        .iter()
        .zip(&wrapped)
        .filter(|(_, w)| !**w)
        .map(|(c, _)| *c)
        .collect();
    assert!(
        missing.is_empty(),
        "{PLACE} wraps none of {missing:?}: the guard checks nothing"
    );
}

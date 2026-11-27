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

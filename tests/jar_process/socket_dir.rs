//! A jar whose socket was deleted binds it again only in a directory that is
//! its user's alone (the 3.0 review, E2): one open to others, which another
//! user could have made where the jar's was, isn't trusted, and the jar says
//! so on its standard error rather than listen there.

use super::*;

#[test]
fn a_jar_never_binds_its_socket_again_in_a_directory_open_to_others() {
    use std::os::unix::fs::PermissionsExt;
    let mut jar = Jar::start(&[], &sh("sleep 1000"));
    let _owner = jar.connect(Role::Owner);
    let dir = jar.socket.parent().expect("its directory").to_path_buf();
    // Opened first: the jar looks at its socket every two seconds, and one
    // that found it gone before the directory opened would bind there at once.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).expect("open it");
    std::fs::remove_file(&jar.socket).expect("delete the socket");
    let said = || std::fs::read_to_string(jar.stderr.path()).unwrap_or_default();
    let until = Instant::now() + WAIT;
    loop {
        assert!(
            !jar.socket.exists(),
            "the jar listened again in a directory open to others"
        );
        if said().contains("open to others") {
            break;
        }
        assert!(
            Instant::now() < until,
            "the jar never said why it doesn't listen again: {:?}",
            said()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("close it");
    eventually("the socket back in its private directory", || {
        jar.socket.exists()
    });
}

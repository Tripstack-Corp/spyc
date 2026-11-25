//! `spyc/upgrade` is answered before anything starts (H7): the session's spyc
//! checks what an upgrade needs and says why it won't, rather than take the
//! request and fail where only its own screen shows it.

use super::*;
use crate::mcp_cmd::{McpCommand, McpResponse};

/// With no newer spyc installed, the request is refused, saying so, and no
/// upgrade is left pending.
#[test]
fn an_upgrade_asked_with_no_newer_install_is_refused_before_it_starts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let answer = app.execute_mcp_command(McpCommand::Upgrade);
        assert!(
            matches!(&answer, McpResponse::Error { message } if message.contains("no newer spyc")),
            "{answer:?}"
        );
        assert!(!app.runtime.upgrade_pending, "an upgrade was left pending");
    });
}

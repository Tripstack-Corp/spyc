//! The control requests on spyc's MCP socket (the 3.0 review, E1):
//! `spyc/shutdown`, `spyc/release` and `spyc/upgrade`. Every process of the
//! user reaches that socket, a hosted agent's shell included, so each request
//! must carry the token of the session jar spyc runs in, which the loop checks
//! (`McpCommand::Authorize`) before the request is acted on; a spyc in no
//! session jar of its own takes none. Each is a request, answered: a refusal
//! with why, so the asking `spyc attach`, `spyc shutdown` or `:attach
//! project` says it (`spyc/upgrade` was once a notification, whose refusal
//! reached its sender as nothing: H7).

use std::io::{self, Write};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{Value, json};

use super::super::framing::{send_error, send_result};
use super::{RELEASE_METHOD, RELEASE_WAIT, UPGRADE_METHOD, ask_within, tell};
use crate::mcp_cmd::{McpCommand, McpRequest};

/// How long the socket thread waits for the loop's word on a token.
const AUTHORIZE_WAIT: Duration = Duration::from_secs(5);

/// How long it waits for the loop to say whether it will upgrade: a look at
/// what spyc runs from and at its tabs, nothing started.
const UPGRADE_ANSWER_WAIT: Duration = Duration::from_secs(5);

/// Whether the loop lets `method` be acted on, carrying `params`' token;
/// why not, when it doesn't.
fn authorized(tx: &Sender<McpRequest>, method: &str, params: &Value) -> Result<(), String> {
    let token = params["token"].as_str().map(String::from);
    let ask = McpCommand::Authorize {
        method: method.into(),
        token,
    };
    ask_within(tx, ask, AUTHORIZE_WAIT).map(|_| ())
}

/// `spyc/shutdown`, `spyc/release` or `spyc/upgrade`, answered on `w`:
/// refused, with why, unless authorized; an upgrade is answered
/// `{"upgrading": true}` once the loop has checked what it needs, else why
/// not.
pub(super) fn request(
    w: &mut impl Write,
    id: &Value,
    method: &str,
    params: &Value,
    cmd_tx: Option<&Sender<McpRequest>>,
) -> io::Result<()> {
    let Some(tx) = cmd_tx else {
        return send_error(w, id.clone(), -32000, "spyc didn't answer");
    };
    if let Err(why) = authorized(tx, method, params) {
        return send_error(w, id.clone(), -32000, &why);
    }
    let asked = match method {
        RELEASE_METHOD => Some((McpCommand::Release, RELEASE_WAIT)),
        UPGRADE_METHOD => Some((McpCommand::Upgrade, UPGRADE_ANSWER_WAIT)),
        _ => None,
    };
    if let Some((command, wait)) = asked {
        return match ask_within(tx, command, wait) {
            Ok(answer) => send_result(w, id, answer),
            Err(why) => send_error(w, id.clone(), -32000, &why),
        };
    }
    tell(tx, McpCommand::Shutdown);
    send_result(w, id, json!({}))
}

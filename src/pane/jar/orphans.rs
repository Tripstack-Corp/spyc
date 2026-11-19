//! Jars no spyc owns (S2c): live jars whose owner went without a successor,
//! whose token this spyc keeps, each as it describes itself (R31) and by
//! what it holds (R18). A spyc's start offers to restore or discard them
//! (`app::orphan_jars`); a discard ends one and its program.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;
use spyc_jar::client::Client;
use spyc_jar::proto::{JarSignal, Msg, Role};

use super::Place;
use super::place::{SocketState, socket_state};

/// A jar no spyc owns, as it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub socket: PathBuf,
    /// `pane`, `task` or `capture`; empty for a jar never told.
    pub is: String,
    pub label: String,
    /// What its tab or task ran; empty for a jar never told.
    pub command: String,
    pub agent: String,
    pub project: String,
    pub project_home: String,
    pub pane_id: String,
    /// Where its program started.
    pub cwd: String,
}

/// The jars in `place` (this user's when `None`) that no spyc owns. Waits
/// on the store's check, for a place, and on each jar's answer: never on the
/// loop.
pub fn find(place: Option<Place>) -> Vec<Orphan> {
    let Some(place) = place.or_else(|| Place::for_this_user().ok()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(place.sockets()) else {
        return Vec::new();
    };
    let mut sockets: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sock"))
        .collect();
    sockets.sort();
    sockets
        .into_iter()
        .filter_map(|socket| orphan(&place, socket))
        .collect()
}

/// The jar at `socket`, when it runs, has no owner, and its token is here.
fn orphan(place: &Place, socket: PathBuf) -> Option<Orphan> {
    if socket_state(&socket) != SocketState::Live {
        return None;
    }
    super::tokens::load(place, &socket)?;
    let fields = spyc_jar::client::describe(&socket).ok()?;
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map_or_else(String::new, |(_, v)| v.clone())
    };
    if !get("owner").starts_with("none") {
        return None;
    }
    Some(Orphan {
        socket,
        is: get("is"),
        label: get("label"),
        command: get("command"),
        agent: get("agent"),
        project: get("project"),
        project_home: get("project_home"),
        pane_id: get("pane_id"),
        cwd: get("cwd"),
    })
}

/// How long a discarded program has to end on a hangup before it's killed.
const HANGUP_GRACE: Duration = Duration::from_secs(2);
/// How long a discard waits on the jar: to connect, then for its program's
/// exit once killed.
const DISCARD_WAIT: Duration = Duration::from_secs(5);
/// How often a discard looks for the program's exit, within [`DISCARD_WAIT`].
const DISCARD_POLL: Duration = Duration::from_millis(200);

/// End the jar at `socket` in `place` (this user's when `None`), and its
/// program: a hangup, then a kill if it hasn't ended, its exit collected so
/// the jar ends. Blocks: never on the loop.
pub fn discard(place: Option<Place>, socket: &Path) -> anyhow::Result<()> {
    let place = match place {
        Some(place) => place,
        None => Place::for_this_user()?,
    };
    let token = super::tokens::load(&place, socket)
        .with_context(|| format!("no token here for the jar at {}", socket.display()))?;
    let (mut jar, welcome) = Client::connect(socket, Role::Owner, DISCARD_WAIT, &token)?;
    // The program's exit comes in the stream an owner asks for; nothing it
    // printed before is wanted.
    let head = match welcome {
        Msg::Welcome { head, .. } => head,
        _ => 0,
    };
    jar.send(&Msg::Replay { from: head })?;
    jar.send(&Msg::Signal {
        signal: JarSignal::Hangup as i32,
    })?;
    let hung_up = Instant::now();
    let mut killed: Option<Instant> = None;
    loop {
        if let Some(Msg::Exited { .. }) = jar.recv(DISCARD_POLL)? {
            let _ = jar.send(&Msg::AckExit);
            break;
        }
        match killed {
            None if hung_up.elapsed() >= HANGUP_GRACE => {
                jar.send(&Msg::Signal {
                    signal: JarSignal::Kill as i32,
                })?;
                killed = Some(Instant::now());
            }
            Some(at) if at.elapsed() >= DISCARD_WAIT => {
                anyhow::bail!("the program in the jar at {} didn't end", socket.display())
            }
            _ => {}
        }
    }
    super::tokens::forget(&place, socket);
    Ok(())
}

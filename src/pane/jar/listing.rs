//! `spyc --jars`: this user's jars, each as it describes itself (R31) and by
//! what it holds (R18), and the sockets whose jars are gone. It only reads:
//! nothing is started, ended or removed.

use std::path::{Path, PathBuf};

use super::place::{SocketState, socket_dirs, socket_state};

/// Print every jar in this user's socket directory.
pub fn print() {
    let dir = socket_dirs(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("TMPDIR"),
        rustix::process::getuid().as_raw(),
    )
    .pop()
    .unwrap_or_default();
    println!("jars in {}:", dir.display());
    let sockets = sockets_in(&dir);
    if sockets.is_empty() {
        println!("  none");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for socket in sockets {
        println!("  {}", line(&socket, now));
    }
}

pub(super) fn sockets_in(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sock"))
        .collect();
    found.sort();
    found
}

/// One jar's line, `now` in seconds since the epoch.
fn line(socket: &Path, now: u64) -> String {
    let id = socket
        .file_stem()
        .map_or_default(|s| s.to_string_lossy().into_owned());
    if socket_state(socket) == SocketState::Stale {
        return format!("{id}  stale: its jar is gone");
    }
    let fields = match spyc_jar::client::describe(socket) {
        Ok(fields) => fields,
        Err(e) => return format!("{id}  no answer ({e:#})"),
    };
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    };
    let what = match get("label") {
        "" => get("program").to_string(),
        label => format!(
            "{label} ({}, {} in {})",
            get("is"),
            get("agent"),
            get("project")
        ),
    };
    let up = get("created").parse::<u64>().map_or_else(
        |_| String::new(),
        |at| {
            let s = now.saturating_sub(at);
            format!("{}h{:02}m", s / 3600, s / 60 % 60)
        },
    );
    // C12: the jar's own pid, the one a user ends a hung jar by, and the
    // owning spyc's.
    let owner_pid = match get("owner_pid") {
        "" => String::new(),
        pid => format!(" (pid {pid})"),
    };
    format!(
        "{id}  {what}  pid {}  jar pid {}  up {up}  build {}  owner {}{owner_pid}  linger {}  log {}",
        get("program_pid"),
        get("pid"),
        get("build"),
        get("owner"),
        get("linger"),
        get("log")
    )
}

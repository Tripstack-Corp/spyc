//! `spyc attach [NAME]` (S4): attach this terminal to a session's own jar
//! (`super::session`), found by the session's name in the sessions
//! directory. With no name it takes the one session running, or asks which of
//! several. A session attached elsewhere is taken over: its client, the
//! `owner_pid` its jar describes, is sent SIGTERM, and a `spyc attach`
//! restores its terminal on the way out, as spyc does when killed. A jar
//! accepts only its user's processes, so that pid is the user's own.
//!
//! A session whose spyc runs from the path this one does, and started before
//! the file there was replaced, runs an install it hasn't taken: `spyc
//! attach` offers to have it `:upgrade` first, by a notification on its MCP
//! socket.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;

use super::place::{SocketState, socket_state, this_users_sessions, tokens_dir};

/// How long a takeover waits for the old client to let go: past the longest
/// a jar is silent, parked for an upgrade (asserted in `super`), so a busy
/// jar isn't taken for one that kept its client.
pub(super) const TAKEOVER_WAIT: Duration = Duration::from_secs(20);

/// How long `spyc attach` waits for the session's spyc to come back upgraded
/// before it attaches anyway: a spyc's start, not a wait on a jar.
const UPGRADE_WAIT: Duration = Duration::from_secs(20);

/// How often a takeover looks at the jar while it waits.
const TAKEOVER_POLL: Duration = Duration::from_millis(50);

/// A session whose own jar runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub name: String,
    pub socket: PathBuf,
    /// The pid of the client attached to it, when one is.
    pub client: Option<i32>,
}

/// `spyc attach [NAME]`: attach to the session `name`, or the one running,
/// until this terminal detaches or spyc quits.
pub fn run(name: Option<&str>) -> anyhow::Result<()> {
    let running = running(&this_users_sessions());
    let chosen = match choose(&running, name).map_err(anyhow::Error::msg)? {
        Some(one) => one.clone(),
        None => ask(
            &running,
            &mut std::io::stdin().lock(),
            &mut std::io::stderr(),
        )?
        .clone(),
    };
    let token = super::tokens::load_in(&tokens_dir()?, &chosen.socket)
        .with_context(|| format!("no token kept for {}", chosen.name))?;
    if let Some(pid) = chosen.client {
        take_over(&chosen, pid)?;
    }
    if let Some((program, at)) = behind(&chosen.socket)
        && agrees(
            &format!(
                "spyc: {} runs a spyc older than the one installed at {}. Upgrade it? [Y/n] ",
                chosen.name,
                at.display()
            ),
            &mut std::io::stdin().lock(),
            &mut std::io::stderr(),
        )?
    {
        upgrade_first(&chosen.name, program)?;
    }
    crate::terminal::restore_on_signal();
    spyc_jar::attach::run(&chosen.socket, &token)
}

/// The pid of the session's spyc and the path it runs from, when the file there
/// was replaced after it started: an install it hasn't taken. Only for one
/// that runs from where this spyc does.
fn behind(socket: &Path) -> Option<(u32, PathBuf)> {
    let fields = spyc_jar::client::describe(socket).ok()?;
    let field = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    let pid: u32 = field("program_pid")?.parse().ok()?;
    let here = crate::paths::installed_spyc().ok()?;
    if Path::new(field("program")?) != here {
        return None;
    }
    let started = crate::proc_group::started_epoch_secs(pid)?;
    // The later of its modification and inode change times: a copy can carry
    // the source's modification time, and a new file's inode changes when it
    // is made.
    let installed = {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(&here).ok()?;
        u64::try_from(meta.mtime().max(meta.ctime())).ok()?
    };
    replaced_since(started, installed).then_some((pid, here))
}

/// Whether a file modified at `installed` came after a process started at
/// `started`, both in epoch seconds, past the second a start time is rounded
/// to.
const fn replaced_since(started: u64, installed: u64) -> bool {
    installed > started.saturating_add(1)
}

/// Have the session's spyc `:upgrade`, and wait for the new one before the
/// terminal attaches, so no key is typed into a spyc as it restarts. The old
/// spyc removes its MCP socket before its `exec` and the new one makes it
/// again: a socket newer than the one there now, taking a connection, is the
/// new spyc. One that doesn't come is attached to anyway, with a note.
fn upgrade_first(name: &str, pid: u32) -> anyhow::Result<()> {
    let socket =
        crate::mcp::socket_path_for(pid).context("no state directory for spyc's socket")?;
    let made = |path: &Path| {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path)
            .ok()
            .map(|m| (m.ino(), m.ctime(), m.ctime_nsec()))
    };
    let before = made(&socket);
    crate::mcp::ask_upgrade(pid)?;
    let until = Instant::now() + UPGRADE_WAIT;
    loop {
        let now = made(&socket);
        if now.is_some()
            && now != before
            && std::os::unix::net::UnixStream::connect(&socket).is_ok()
        {
            return Ok(());
        }
        if Instant::now() >= until {
            eprintln!("spyc: {name}'s spyc hasn't come back upgraded yet; attaching anyway");
            return Ok(());
        }
        std::thread::sleep(TAKEOVER_POLL);
    }
}

/// Ask `question` on `output` and read the answer from `input`: yes unless
/// it starts with `n`.
fn agrees(
    question: &str,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> anyhow::Result<bool> {
    write!(output, "{question}")?;
    output.flush()?;
    let mut line = String::new();
    input.read_line(&mut line)?;
    Ok(!line.trim_start().to_ascii_lowercase().starts_with('n'))
}

/// The sessions whose own jars run in `sessions`, by name.
pub fn running(sessions: &Path) -> Vec<Running> {
    let mut found: Vec<Running> = std::fs::read_dir(sessions)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let socket = entry.path();
            let name = session_name(&socket)?;
            (socket_state(&socket) == SocketState::Live).then(|| Running {
                client: client_of(&socket).ok().flatten(),
                name,
                socket,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The session a socket in the sessions directory holds.
fn session_name(socket: &Path) -> Option<String> {
    if socket.extension()? != "sock" {
        return None;
    }
    socket.file_stem()?.to_str().map(String::from)
}

/// The pid of the client attached to the jar at `socket`, as it describes it.
fn client_of(socket: &Path) -> anyhow::Result<Option<i32>> {
    let fields = spyc_jar::client::describe(socket)?;
    let field = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    Ok((field("owner") == Some("connected"))
        .then(|| field("owner_pid")?.parse().ok())
        .flatten())
}

/// Which of `running` `name` means: `Ok(None)` when there are several and
/// no name, for the user to choose; an error says what there is instead.
pub fn choose<'a>(
    running: &'a [Running],
    name: Option<&str>,
) -> Result<Option<&'a Running>, String> {
    let names = || {
        running
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (name, running) {
        (Some(name), _) => match running.iter().find(|r| r.name == name) {
            Some(one) => Ok(Some(one)),
            None if running.is_empty() => {
                Err(format!("no session named {name} runs in a jar of its own"))
            }
            None => Err(format!(
                "no session named {name} runs in a jar of its own; these do: {}",
                names()
            )),
        },
        (None, []) => Err("no session runs in a jar of its own".into()),
        (None, [one]) => Ok(Some(one)),
        (None, _) => Ok(None),
    }
}

/// List `running` on `output` and read which one from `input`: its number
/// or its name.
pub fn ask<'a>(
    running: &'a [Running],
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> anyhow::Result<&'a Running> {
    for (i, r) in running.iter().enumerate() {
        let attached = if r.client.is_some() {
            "  (attached elsewhere)"
        } else {
            ""
        };
        writeln!(output, "{:>3}  {}{attached}", i + 1, r.name)?;
    }
    write!(output, "Attach to which? ")?;
    output.flush()?;
    let mut line = String::new();
    input.read_line(&mut line)?;
    let answer = line.trim();
    answer
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| running.get(i))
        .or_else(|| running.iter().find(|r| r.name == answer))
        .with_context(|| format!("{answer:?} is none of them"))
}

/// End the client attached to `session`, and wait until its jar has let it go.
fn take_over(session: &Running, pid: i32) -> anyhow::Result<()> {
    let target = rustix::process::Pid::from_raw(pid)
        .with_context(|| format!("{} names no client pid", session.name))?;
    eprintln!(
        "spyc: {} is attached elsewhere (pid {pid}); taking it over",
        session.name
    );
    match rustix::process::kill_process(target, rustix::process::Signal::TERM) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => {}
        Err(e) => {
            return Err(e).with_context(|| {
                format!("end the client attached to {} (pid {pid})", session.name)
            });
        }
    }
    let until = Instant::now() + TAKEOVER_WAIT;
    loop {
        match client_of(&session.socket) {
            Ok(Some(still)) if still == pid => {}
            Ok(_) => return Ok(()),
            Err(_) => {}
        }
        anyhow::ensure!(
            Instant::now() < until,
            "the client attached to {} (pid {pid}) didn't let go",
            session.name
        );
        std::thread::sleep(TAKEOVER_POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str, client: Option<i32>) -> Running {
        Running {
            name: name.into(),
            socket: PathBuf::from(format!("/run/spyc/sessions/{name}.sock")),
            client,
        }
    }

    #[test]
    fn a_name_picks_its_session_and_an_unknown_one_lists_the_rest() {
        let running = [session("ANISE", None), session("CLOVE", Some(7))];
        assert_eq!(choose(&running, Some("CLOVE")), Ok(Some(&running[1])));
        let err = choose(&running, Some("SAGE")).unwrap_err();
        assert!(
            err.contains("SAGE") && err.contains("ANISE, CLOVE"),
            "{err}"
        );
        assert!(choose(&[], Some("SAGE")).unwrap_err().contains("SAGE"));
    }

    #[test]
    fn no_name_takes_the_only_session_and_asks_among_several() {
        let one = [session("ANISE", None)];
        assert_eq!(choose(&one, None), Ok(Some(&one[0])));
        let two = [session("ANISE", None), session("CLOVE", None)];
        assert_eq!(choose(&two, None), Ok(None));
        assert!(choose(&[], None).is_err());
    }

    #[test]
    fn asking_takes_a_number_or_a_name() {
        let running = [session("ANISE", None), session("CLOVE", Some(7))];
        let mut shown = Vec::new();
        let picked = ask(&running, &mut &b"2\n"[..], &mut shown).unwrap();
        assert_eq!(picked.name, "CLOVE");
        let shown = String::from_utf8(shown).unwrap();
        assert!(shown.contains("  1  ANISE\n"), "{shown}");
        assert!(
            shown.contains("  2  CLOVE  (attached elsewhere)\n"),
            "{shown}"
        );
        let picked = ask(&running, &mut &b"ANISE\n"[..], &mut Vec::new()).unwrap();
        assert_eq!(picked.name, "ANISE");
        assert!(ask(&running, &mut &b"3\n"[..], &mut Vec::new()).is_err());
    }

    #[test]
    fn a_file_replaced_after_the_start_is_an_install_not_taken() {
        assert!(replaced_since(1_000, 1_100));
        assert!(!replaced_since(1_000, 900));
        assert!(!replaced_since(1_000, 1_001), "within the start's rounding");
    }

    #[test]
    fn anything_but_no_agrees() {
        let says = |answer: &str| agrees("?", &mut answer.as_bytes(), &mut Vec::new()).unwrap();
        assert!(says("\n"));
        assert!(says("y\n"));
        assert!(says("Yes\n"));
        assert!(!says("n\n"));
        assert!(!says("No\n"));
    }

    #[test]
    fn only_a_socket_names_a_session() {
        assert_eq!(
            session_name(Path::new("/s/ANISE.sock")),
            Some("ANISE".into())
        );
        assert_eq!(session_name(Path::new("/s/ANISE.lock")), None);
        assert_eq!(session_name(Path::new("/s/ANISE")), None);
    }
}

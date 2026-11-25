//! `spyc attach [NAME]` (S4): attach this terminal to a session's own jar
//! (`super::session`), found in the sessions directory by the name the
//! session goes by or by its address, the name it started as. With no name it
//! takes the one session running, or asks which of several. A session attached elsewhere is taken over: its client, the
//! `owner_pid` its jar describes, is sent SIGTERM, and a `spyc attach`
//! restores its terminal on the way out, as spyc does when killed. A jar
//! accepts only its user's processes, so that pid is the user's own. The jar
//! is asked again once any question is answered, and the pid is signalled
//! only while the jar still names it and it is the same process, by its start
//! time (`take_over`): a pid read before a question may be another
//! process's by its answer.
//!
//! A session whose spyc runs from the path this one does, and finds a newer
//! spyc installed there, as `:upgrade` would, runs an install it hasn't
//! taken: `spyc attach` asks it (`spyc/behind`) and offers to have it
//! `:upgrade` first, by a notification on its MCP socket. Its start time
//! can't say, since an upgrade in place keeps the pid; it is only the answer
//! for a spyc that predates the question.
//!
//! The terminal's SSH agent goes with it: `spyc attach` names it, and the
//! session's spyc points its tabs there (`pane::ssh_agent`).
//!
//! It refuses to run inside spyc, as tmux refuses to nest: from a pane, or
//! from anything a spyc or a pane's program started, the session would show
//! inside one of its own panes, and every key and every frame would go round.
//! `--force` attaches anyway, as `^d`'s handover does in the terminal spyc
//! already holds.

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
    /// The name it goes by.
    pub name: String,
    /// Its socket's name, the one it started as.
    pub address: String,
    pub socket: PathBuf,
    /// The pid of the client attached to it, when one is.
    pub client: Option<i32>,
}

/// `spyc attach [NAME]`: attach to the session `name`, or the one running,
/// until this terminal detaches or spyc quits.
pub fn run(name: Option<&str>, force: bool) -> anyhow::Result<()> {
    let pane = std::env::var_os("SPYC_PANE_ID");
    let jar = std::env::var_os("SPYC_JAR_SOCKET").map(PathBuf::from);
    if !force && let Some(why) = inside_spyc(pane.as_deref(), jar.as_deref()) {
        anyhow::bail!(
            "{why}: attaching here would show spyc inside itself. `spyc attach --force` attaches anyway"
        );
    }
    let running = running(&this_users_sessions());
    let chosen = match choose(&running, name).map_err(anyhow::Error::msg)? {
        Some(one) => one.clone(),
        None => ask(
            &running,
            "Attach to which? ",
            &mut std::io::stdin().lock(),
            &mut std::io::stderr(),
        )?
        .clone(),
    };
    let token = super::tokens::load_in(&tokens_dir()?, &chosen.socket)
        .with_context(|| format!("no token kept for {}", chosen.name))?;
    follow_this_agent(&chosen.address);
    take_over(&chosen)?;
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

/// Why this shell runs inside spyc, where attaching would nest a session in
/// itself: `pane` (`SPYC_PANE_ID`) is set, or `jar` (`SPYC_JAR_SOCKET`) names
/// a jar that runs, or one that can't be told from a gone one. `None` when
/// neither says so.
fn inside_spyc(pane: Option<&std::ffi::OsStr>, jar: Option<&Path>) -> Option<String> {
    if pane.is_some_and(|p| !p.is_empty()) {
        return Some("this shell runs in a spyc pane (`SPYC_PANE_ID` is set)".into());
    }
    let jar = jar.filter(|j| !j.as_os_str().is_empty())?;
    match socket_state(jar) {
        SocketState::Live => Some(format!("this shell runs in a spyc jar ({})", jar.display())),
        SocketState::Unknown => Some(format!(
            "this shell may run in a spyc jar ({}), which can't be told from a gone one",
            jar.display()
        )),
        SocketState::Stale | SocketState::Missing => None,
    }
}

/// Point the agent link of the session at `address` at this terminal's SSH
/// agent, which its spyc points its tabs at once this terminal attaches
/// (`pane::ssh_agent`). With no agent here, the tabs keep theirs.
fn follow_this_agent(address: &str) {
    use crate::pane::ssh_agent;
    let (Some(dir), Some(agent)) = (ssh_agent::links_dir(), ssh_agent::from_env()) else {
        return;
    };
    let link = ssh_agent::session_link(&dir, address);
    if let Err(e) = agent.and_then(|a| ssh_agent::point(&link, &a)) {
        eprintln!("spyc: the session's tabs keep the SSH agent they have: {e:#}");
    }
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
    if !runs_from(field("program")?, &here) {
        return None;
    }
    // The spyc knows what it runs: an upgrade in place keeps its pid, and so
    // the start time below.
    if let Some(behind) = crate::mcp::ask_behind(pid) {
        return behind.then_some((pid, here));
    }
    // A spyc that predates the question: its start time is the best there is.
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

/// Whether a session jar's `program`, its command line joined by spaces,
/// runs the spyc at `here`: that binary alone, or with the arguments a `^d`
/// gives it (`--handed-over-by`).
fn runs_from(program: &str, here: &Path) -> bool {
    let here = here.display().to_string();
    program == here
        || program
            .strip_prefix(&here)
            .is_some_and(|rest| rest.starts_with(' '))
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

/// This user's sessions whose own jars run, by name.
pub fn running_here() -> Vec<Running> {
    running(&this_users_sessions())
}

/// The sessions whose own jars run in `sessions`, by name.
pub fn running(sessions: &Path) -> Vec<Running> {
    let mut found: Vec<Running> = std::fs::read_dir(sessions)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let socket = entry.path();
            let address = address_of(&socket)?;
            if socket_state(&socket) != SocketState::Live || ended(&socket) {
                return None;
            }
            Some(Running {
                client: client_of(&socket).ok().flatten(),
                name: super::session::shown_name(&socket).unwrap_or_else(|| address.clone()),
                address,
                socket,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Whether the jar at `socket` holds only its program's exit: a session's jar
/// keeps that a while for an owner, once its spyc has quit.
pub(super) fn ended(socket: &Path) -> bool {
    spyc_jar::client::describe(socket).is_ok_and(|fields| fields.iter().any(|(k, _)| k == "exit"))
}

/// Whether a session other than the one at `own` (an address) runs in
/// `sessions` going by `name` or at that address. Reads no jar, so a jar
/// that holds only its spyc's exit still counts.
pub fn named_elsewhere(sessions: &Path, name: &str, own: Option<&str>) -> bool {
    std::fs::read_dir(sessions)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|socket| Some((address_of(&socket)?, socket)))
        .filter(|(address, _)| Some(address.as_str()) != own)
        .filter(|(_, socket)| socket_state(socket) == SocketState::Live)
        .any(|(address, socket)| {
            address == name || super::session::shown_name(&socket).as_deref() == Some(name)
        })
}

/// Whether `running` is the session `name` means: the name it goes by or its
/// address, as typed or as `:name` would spell it.
fn answers_to(running: &Running, name: &str) -> bool {
    let spelled = crate::state::session_names::normalize(name);
    [Some(name), spelled.as_deref()]
        .into_iter()
        .flatten()
        .any(|n| running.name == n || running.address == n)
}

/// The address of the session a socket in the sessions directory holds: its
/// file's name.
fn address_of(socket: &Path) -> Option<String> {
    if socket.extension()? != "sock" {
        return None;
    }
    socket.file_stem()?.to_str().map(String::from)
}

/// The client attached to `session` now, read from its jar after any
/// question, and when it started: `None` with none attached. A pid is the
/// jar's client only while the jar names it, so the jar is asked on each
/// side of reading the start time.
fn attached_now(session: &Running) -> anyhow::Result<Option<(i32, u64)>> {
    let until = Instant::now() + TAKEOVER_WAIT;
    loop {
        if let Ok(first) = client_of(&session.socket) {
            let Some(pid) = first else {
                return Ok(None);
            };
            if let Some(started) = started_secs(pid)
                && client_of(&session.socket).ok().flatten() == Some(pid)
            {
                return Ok(Some((pid, started)));
            }
        }
        anyhow::ensure!(
            Instant::now() < until,
            "can't tell which terminal is attached to {}; run `spyc attach` again",
            session.name
        );
        std::thread::sleep(TAKEOVER_POLL);
    }
}

/// When process `pid` started, in epoch seconds.
fn started_secs(pid: i32) -> Option<u64> {
    u32::try_from(pid)
        .ok()
        .and_then(crate::proc_group::started_epoch_secs)
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
        (Some(name), _) => match running.iter().find(|r| answers_to(r, name)) {
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

/// List `running` on `output`, ask `question`, and read which one from
/// `input`: its number or its name.
pub fn ask<'a>(
    running: &'a [Running],
    question: &str,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> anyhow::Result<&'a Running> {
    for (i, r) in running.iter().enumerate() {
        let attached = if r.client.is_some() {
            "  (attached elsewhere)"
        } else {
            ""
        };
        let was = if r.name == r.address {
            String::new()
        } else {
            format!(" ({})", r.address)
        };
        writeln!(output, "{:>3}  {}{was}{attached}", i + 1, r.name)?;
    }
    write!(output, "{question}")?;
    output.flush()?;
    let mut line = String::new();
    input.read_line(&mut line)?;
    let answer = line.trim();
    answer
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| running.get(i))
        .or_else(|| running.iter().find(|r| answers_to(r, answer)))
        .with_context(|| format!("{answer:?} is none of them"))
}

/// End the client attached to `session` now, if one is, and wait until its
/// jar has let it go.
fn take_over(session: &Running) -> anyhow::Result<()> {
    let Some((pid, started)) = attached_now(session)? else {
        return Ok(());
    };
    let target = rustix::process::Pid::from_raw(pid)
        .with_context(|| format!("{} names no client pid", session.name))?;
    eprintln!(
        "spyc: {} is attached elsewhere (pid {pid}); taking it over",
        session.name
    );
    // SPYC-TRAP(takeover-signals-the-client-now): the process the jar named,
    // by its start time, just before the signal; one gone since needs none.
    let signalled = if started_secs(pid) == Some(started) {
        rustix::process::kill_process(target, rustix::process::Signal::TERM)
    } else {
        Ok(())
    };
    match signalled {
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
            Ok(Some(other)) => anyhow::bail!(
                "another terminal attached to {} meanwhile (pid {other}); `spyc attach` takes it over",
                session.name
            ),
            Ok(None) => return Ok(()),
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

    #[test]
    fn a_pane_or_a_jar_that_runs_says_this_shell_is_inside_spyc() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone.sock");
        assert!(inside_spyc(Some("pane-1".as_ref()), None).is_some());
        assert!(inside_spyc(Some("pane-1".as_ref()), Some(&gone)).is_some());
        assert_eq!(inside_spyc(Some("".as_ref()), None), None);
        assert_eq!(inside_spyc(None, Some(&gone)), None, "a jar long gone");
        assert_eq!(inside_spyc(None, Some(Path::new(""))), None);
        assert_eq!(inside_spyc(None, None), None);
    }

    fn session(name: &str, client: Option<i32>) -> Running {
        Running {
            name: name.into(),
            address: name.into(),
            socket: PathBuf::from(format!("/run/spyc/sessions/{name}.sock")),
            client,
        }
    }

    /// Session `address`, renamed `name`.
    fn renamed(address: &str, name: &str) -> Running {
        Running {
            name: name.into(),
            ..session(address, None)
        }
    }

    #[test]
    fn a_renamed_session_answers_to_its_name_and_its_address() {
        let running = [session("ANISE", None), renamed("CLOVE", "GENERAL_WORK")];
        assert_eq!(
            choose(&running, Some("GENERAL_WORK")),
            Ok(Some(&running[1]))
        );
        assert_eq!(
            choose(&running, Some("general work")),
            Ok(Some(&running[1]))
        );
        assert_eq!(choose(&running, Some("CLOVE")), Ok(Some(&running[1])));
        let err = choose(&running, Some("SAGE")).unwrap_err();
        assert!(err.contains("ANISE, GENERAL_WORK"), "{err}");
        let mut shown = Vec::new();
        let picked = ask(&running, "?", &mut &b"general_work\n"[..], &mut shown).unwrap();
        assert_eq!(picked.address, "CLOVE");
        let shown = String::from_utf8(shown).unwrap();
        assert!(shown.contains("  2  GENERAL_WORK (CLOVE)\n"), "{shown}");
    }

    #[test]
    fn a_name_is_taken_by_another_live_session_by_its_name_or_address() {
        let dir = tempfile::tempdir().unwrap();
        let live = |address: &str| {
            std::os::unix::net::UnixListener::bind(dir.path().join(format!("{address}.sock")))
                .unwrap()
        };
        let (_anise, _clove) = (live("ANISE"), live("CLOVE"));
        super::super::session::write_name(&dir.path().join("CLOVE.sock"), "GENERAL_WORK").unwrap();
        std::fs::write(dir.path().join("GONE.sock"), "").unwrap();
        let taken = |name: &str, own: Option<&str>| named_elsewhere(dir.path(), name, own);
        assert!(taken("GENERAL_WORK", Some("ANISE")));
        assert!(taken("ANISE", Some("CLOVE")));
        assert!(taken("CLOVE", Some("ANISE")), "a renamed session's address");
        assert!(!taken("GENERAL_WORK", Some("CLOVE")), "its own name");
        assert!(!taken("CLOVE", Some("CLOVE")), "its own address");
        assert!(!taken("GINGER", None));
        assert!(!taken("GONE", None), "a socket no jar answers at");
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
        let picked = ask(&running, "Attach to which? ", &mut &b"2\n"[..], &mut shown).unwrap();
        assert_eq!(picked.name, "CLOVE");
        let shown = String::from_utf8(shown).unwrap();
        assert!(shown.contains("  1  ANISE\n"), "{shown}");
        assert!(
            shown.contains("  2  CLOVE  (attached elsewhere)\n"),
            "{shown}"
        );
        let picked = ask(
            &running,
            "Attach to which? ",
            &mut &b"ANISE\n"[..],
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(picked.name, "ANISE");
        assert!(
            ask(
                &running,
                "Attach to which? ",
                &mut &b"3\n"[..],
                &mut Vec::new()
            )
            .is_err()
        );
    }

    #[test]
    fn a_session_runs_the_installed_spyc_with_or_without_a_handovers_arguments() {
        let here = Path::new("/home/u/.local/bin/spyc");
        assert!(runs_from("/home/u/.local/bin/spyc", here));
        assert!(runs_from(
            "/home/u/.local/bin/spyc --handed-over-by 4242",
            here
        ));
        assert!(!runs_from("/home/u/.local/bin/spyc-old", here));
        assert!(!runs_from("/opt/spyc --handed-over-by 4242", here));
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
        assert_eq!(address_of(Path::new("/s/ANISE.sock")), Some("ANISE".into()));
        assert_eq!(address_of(Path::new("/s/ANISE.lock")), None);
        assert_eq!(address_of(Path::new("/s/ANISE.name")), None);
        assert_eq!(address_of(Path::new("/s/ANISE")), None);
    }
}

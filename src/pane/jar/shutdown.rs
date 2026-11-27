//! `spyc shutdown [NAME]`: end a session that runs in its own jar, as `:q`
//! in it would, from outside it. The session is chosen as `spyc attach`
//! chooses one. It names the tabs that end with it, the pane jars whose owner
//! is the session's spyc, and asks first unless `-y`. The session's spyc is
//! asked on its MCP socket (`spyc/shutdown`), and one too old to know the
//! request says so rather than nothing. It saves the
//! session and quits, ending its tabs. Its jar keeps the exit a while for an
//! owner, so the wait is for the exit, not for the jar.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context;

use super::attach::{Running, ask, choose, ended, running_in};
use super::place::{SocketState, socket_dirs, socket_state, this_users_sessions};

/// How long `spyc shutdown` waits for the session's spyc to end: its quit,
/// which ends every tab, each given its `SIGHUP` grace.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(30);

/// How often it looks at the session's jar while it waits.
const SHUTDOWN_POLL: Duration = Duration::from_millis(50);

/// `spyc shutdown [NAME] [-y]`.
pub fn run(name: Option<&str>, yes: bool) -> anyhow::Result<()> {
    let running = running_in(&this_users_sessions())?;
    let chosen = match choose(&running, name).map_err(anyhow::Error::msg)? {
        Some(one) => one.clone(),
        None => ask(
            &running,
            "Shut down which? ",
            &mut std::io::stdin().lock(),
            &mut std::io::stderr(),
        )?
        .clone(),
    };
    let pid = program_pid(&chosen.socket)
        .with_context(|| format!("{}'s jar names no spyc", chosen.name))?;
    let tabs = tabs_of(pid);
    if !yes
        && !confirmed(
            &chosen,
            &tabs,
            &mut std::io::stdin().lock(),
            &mut std::io::stderr(),
        )?
    {
        eprintln!("spyc: {} runs on", chosen.name);
        return Ok(());
    }
    let token = super::session::token_of(&chosen.socket)
        .with_context(|| format!("no token kept for {}", chosen.name))?;
    crate::mcp::ask_shutdown(pid, &token).with_context(|| format!("shut down {}", chosen.name))?;
    let until = Instant::now() + SHUTDOWN_WAIT;
    while socket_state(&chosen.socket) == SocketState::Live && !ended(&chosen.socket) {
        anyhow::ensure!(
            Instant::now() < until,
            "{}'s spyc (pid {pid}) hasn't ended",
            chosen.name
        );
        std::thread::sleep(SHUTDOWN_POLL);
    }
    super::session::forget_token(&chosen.socket);
    eprintln!("spyc: {} has shut down", chosen.name);
    Ok(())
}

/// Say what ends with `session` and ask: no unless the answer starts with `y`.
fn confirmed(
    session: &Running,
    tabs: &[String],
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> anyhow::Result<bool> {
    write!(output, "{}", what_ends(session, tabs))?;
    write!(output, "Shut it down? [y/N] ")?;
    output.flush()?;
    let mut line = String::new();
    input.read_line(&mut line)?;
    Ok(line.trim_start().to_ascii_lowercase().starts_with('y'))
}

/// The lines that name what a shutdown of `session` ends.
fn what_ends(session: &Running, tabs: &[String]) -> String {
    let count = match tabs.len() {
        1 => "1 tab".to_string(),
        n => format!("{n} tabs"),
    };
    let mut out = format!(
        "spyc: shutting down {} ends its spyc and {count}",
        session.name
    );
    if session.client.is_some() {
        out.push_str(", and leaves the terminal attached to it");
    }
    out.push_str(if tabs.is_empty() { ".\n" } else { ":\n" });
    out.extend(tabs.iter().map(|tab| format!("  {tab}\n")));
    out
}

/// The pid of the spyc a session's jar runs.
pub fn program_pid(socket: &Path) -> Option<i32> {
    field(&super::place::describe(socket).ok()?, "program_pid")?
        .parse()
        .ok()
}

/// The tabs spyc `pid` owns: each pane jar whose owner it is, by what the jar
/// says it holds.
fn tabs_of(pid: i32) -> Vec<String> {
    let dir = socket_dirs(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("TMPDIR"),
        rustix::process::getuid().as_raw(),
    )
    .pop()
    .unwrap_or_default();
    let owner = pid.to_string();
    super::place::sockets_in(&dir)
        .unwrap_or_default()
        .iter()
        .filter(|socket| socket_state(socket) == SocketState::Live)
        .filter_map(|socket| super::place::describe(socket).ok())
        .filter(|fields| field(fields, "owner_pid") == Some(owner.as_str()))
        .map(|fields| tab_line(&fields))
        .collect()
}

/// A tab as its jar describes it: its label, else its program, and its
/// project.
fn tab_line(fields: &[(String, String)]) -> String {
    let what = field(fields, "label")
        .filter(|l| !l.is_empty())
        .or_else(|| field(fields, "program"))
        .unwrap_or("a tab");
    match field(fields, "project").filter(|p| !p.is_empty()) {
        Some(project) => format!("{what}  in {project}"),
        None => what.to_string(),
    }
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(client: Option<i32>) -> Running {
        Running {
            name: "CORIANDER_CLOVE".into(),
            address: "CORIANDER_CLOVE".into(),
            socket: "/run/spyc/sessions/CORIANDER_CLOVE.sock".into(),
            client,
        }
    }

    fn answer(text: &str) -> bool {
        let mut out = Vec::new();
        confirmed(
            &session(None),
            &[],
            &mut std::io::Cursor::new(text),
            &mut out,
        )
        .unwrap()
    }

    #[test]
    fn only_a_yes_shuts_down() {
        assert!(answer("y\n"));
        assert!(answer(" Yes\n"));
        for no in ["\n", "", "n\n", "no\n", "sure\n"] {
            assert!(!answer(no), "{no:?} shut it down");
        }
    }

    #[test]
    fn it_names_each_tab_and_a_terminal_still_attached() {
        let tabs = ["claude  in spyc".to_string(), "zsh".to_string()];
        let said = what_ends(&session(Some(42)), &tabs);
        assert_eq!(
            said,
            "spyc: shutting down CORIANDER_CLOVE ends its spyc and 2 tabs, \
             and leaves the terminal attached to it:\n  claude  in spyc\n  zsh\n"
        );
        assert_eq!(
            what_ends(&session(None), &[]),
            "spyc: shutting down CORIANDER_CLOVE ends its spyc and 0 tabs.\n"
        );
    }

    #[test]
    fn a_tab_is_its_label_else_its_program() {
        let f = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        };
        assert_eq!(
            tab_line(&f(&[
                ("label", "claude"),
                ("program", "/bin/claude"),
                ("project", "spyc")
            ])),
            "claude  in spyc"
        );
        assert_eq!(
            tab_line(&f(&[("label", ""), ("program", "/bin/zsh")])),
            "/bin/zsh"
        );
        assert_eq!(tab_line(&f(&[])), "a tab");
    }
}

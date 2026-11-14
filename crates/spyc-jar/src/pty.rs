//! The program's PTY, opened and spawned with nothing `unsafe`.
//!
//! rustix opens the pair, and the program starts through `spyc-jar
//! pty-child`, which leaves
//! the jar's session, takes the PTY as its controlling terminal and `exec`s
//! the program, all as safe calls. The master stays a plain descriptor, so it
//! can cross an upgrade's `exec` (R33).

use std::fs::File;
use std::os::fd::OwnedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Context;
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

use crate::state::Config;

/// Open a PTY at the configured size and start the program on it: the
/// master, and the program's pid.
pub(crate) fn spawn(cfg: &Config) -> anyhow::Result<(OwnedFd, u32)> {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).context("open a PTY")?;
    rustix::io::fcntl_setfd(&master, rustix::io::FdFlags::CLOEXEC)?;
    grantpt(&master).context("grantpt")?;
    unlockpt(&master).context("unlockpt")?;
    let name = ptsname(&master, Vec::new()).context("ptsname")?;
    let name = name.to_str().context("the PTY's name isn't UTF-8")?;
    // Not as the jar's controlling terminal: the jar leads its own session.
    let slave = File::options()
        .read(true)
        .write(true)
        .custom_flags(i32::try_from(rustix::fs::OFlags::NOCTTY.bits()).context("O_NOCTTY")?)
        .open(name)
        .with_context(|| format!("open {name}"))?;
    rustix::termios::tcsetwinsize(
        &master,
        rustix::termios::Winsize {
            ws_row: cfg.rows,
            ws_col: cfg.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        },
    )
    .context("size the PTY")?;
    let mut cmd = Command::new(std::env::current_exe().context("the jar's own binary")?);
    cmd.arg("pty-child").arg("--").args(&cfg.argv);
    cmd.stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave);
    if let Some(cwd) = &cfg.cwd {
        cmd.current_dir(cwd);
    }
    // Where the program, and anything it starts, finds this jar: the program
    // connects as role `program` to hear of attaches, and an agent's MCP proxy
    // asks the jar where its owner is, without working out the path again
    // from an environment that may have changed.
    cmd.env("SPYC_JAR_SOCKET", &cfg.socket);
    let child = cmd.spawn().context("spawn the program")?;
    Ok((master, child.id()))
}

/// `spyc-jar pty-child -- PROGRAM [ARGS…]`, with the PTY's slave on standard
/// input, output and error: a new session, the PTY as its controlling
/// terminal, then the program in this process's place.
pub fn child(argv: &[String]) -> anyhow::Result<()> {
    let program = argv.first().context("pty-child: no program")?;
    rustix::process::setsid().context("pty-child: setsid")?;
    rustix::process::ioctl_tiocsctty(std::io::stdin()).context("pty-child: TIOCSCTTY")?;
    let err = Command::new(program).args(&argv[1..]).exec();
    Err(anyhow::anyhow!("{program}: {err}"))
}

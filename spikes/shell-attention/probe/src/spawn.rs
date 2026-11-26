//! The PTY and the shell on it, arranged as `crates/spyc-jar/src/pty.rs`
//! arranges a program: the master opened `O_NOCTTY` and close-on-exec, the
//! slave opened `O_NOCTTY`, and the child started through this binary's
//! `pty-child`, which calls `setsid`, takes the slave as its controlling
//! terminal with `TIOCSCTTY` and `exec`s the program. The prober is then the
//! program's parent, as the jar is.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Context;
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

pub struct Pty {
    pub master: OwnedFd,
    pub slave_path: String,
    pub child: std::process::Child,
}

pub fn spawn(argv: &[String], env: &[(String, String)], cwd: &str) -> anyhow::Result<Pty> {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).context("open a PTY")?;
    rustix::io::fcntl_setfd(&master, rustix::io::FdFlags::CLOEXEC)?;
    grantpt(&master).context("grantpt")?;
    unlockpt(&master).context("unlockpt")?;
    let name = ptsname(&master, Vec::new()).context("ptsname")?;
    let name = name.to_str().context("PTY name")?.to_string();
    let slave = File::options()
        .read(true)
        .write(true)
        .custom_flags(i32::try_from(rustix::fs::OFlags::NOCTTY.bits())?)
        .open(&name)
        .with_context(|| format!("open {name}"))?;
    rustix::termios::tcsetwinsize(
        &master,
        rustix::termios::Winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        },
    )?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("pty-child").arg("--").args(argv);
    cmd.stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave);
    cmd.env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd);
    let child = cmd.spawn().context("spawn the shell")?;
    Ok(Pty {
        master,
        slave_path: name,
        child,
    })
}

/// `pty-child -- PROGRAM [ARGS…]`: `pty.rs`'s `child`, line for line.
pub fn child(argv: &[String]) -> anyhow::Result<()> {
    let program = argv.first().context("pty-child: no program")?;
    rustix::process::setsid().context("pty-child: setsid")?;
    rustix::process::ioctl_tiocsctty(std::io::stdin()).context("pty-child: TIOCSCTTY")?;
    let err = Command::new(program).args(&argv[1..]).exec();
    Err(anyhow::anyhow!("{program}: {err}"))
}

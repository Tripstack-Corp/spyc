//! `spyc-jar --socket PATH [options] -- PROGRAM [ARGS…]`
//!
//! Options: `--cwd DIR`, `--rows N`, `--cols N`, `--ring BYTES`,
//! `--window BYTES`, `--deadline-ms N`, `--linger-ms N`.

#![forbid(unsafe_code)]

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use spyc_jar::jar::{self, Config};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut cfg = Config {
        socket: PathBuf::new(),
        argv: Vec::new(),
        cwd: None,
        rows: 24,
        cols: 80,
        ring_cap: 4 << 20,
        window: 1 << 20,
        deadline: Duration::from_secs(10),
        linger_after_exit: Duration::from_secs(60),
        attach_signal: false,
    };
    if std::env::args().nth(1).as_deref() == Some("attach") {
        let socket = std::env::args()
            .nth(2)
            .context("usage: spyc-jar attach SOCKET")?;
        return spyc_jar::attach::run(std::path::Path::new(&socket));
    }
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match a.as_str() {
            "--socket" => cfg.socket = value("--socket")?.into(),
            "--cwd" => cfg.cwd = Some(value("--cwd")?.into()),
            "--rows" => cfg.rows = value("--rows")?.parse()?,
            "--cols" => cfg.cols = value("--cols")?.parse()?,
            "--ring" => cfg.ring_cap = value("--ring")?.parse()?,
            "--window" => cfg.window = value("--window")?.parse()?,
            "--deadline-ms" => {
                cfg.deadline = Duration::from_millis(value("--deadline-ms")?.parse()?)
            }
            "--attach-signal" => cfg.attach_signal = true,
            "--linger-ms" => {
                cfg.linger_after_exit = Duration::from_millis(value("--linger-ms")?.parse()?);
            }
            "--" => {
                cfg.argv = args.by_ref().collect();
                break;
            }
            other => bail!("unknown argument {other}"),
        }
    }
    if cfg.socket.as_os_str().is_empty() || cfg.argv.is_empty() {
        bail!("usage: spyc-jar --socket PATH [options] -- PROGRAM [ARGS…]");
    }
    // Leave the session that started us, so closing that terminal or the end of
    // the spyc that spawned us reaches neither the jar nor its program.
    // A jar that can't leave its session would die with that terminal, which
    // is the one thing it exists not to do, so it refuses to start rather than
    // run in an indeterminate state. (`setsid` fails for a process group
    // leader, such as a job a shell started with `&`.)
    rustix::process::setsid().context(
        "spyc-jar: can't start its own session (started as a process group leader?); refusing to run",
    )?;

    if let Some(dir) = cfg.socket.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    if cfg.socket.exists() {
        if UnixStream::connect(&cfg.socket).is_ok() {
            bail!("a jar is already listening on {}", cfg.socket.display());
        }
        std::fs::remove_file(&cfg.socket)?;
    }
    let listener = UnixListener::bind(&cfg.socket)
        .with_context(|| format!("bind {}", cfg.socket.display()))?;
    std::fs::set_permissions(&cfg.socket, std::fs::Permissions::from_mode(0o600))?;
    jar::run(cfg, listener)
}

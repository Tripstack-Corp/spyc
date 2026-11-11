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
        linger: None,
        instance: instance(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    match std::env::args().nth(1).as_deref() {
        Some("pty-child") => {
            let argv: Vec<String> = std::env::args().skip(3).collect();
            return spyc_jar::pty::child(&argv);
        }
        Some("validate-handover") => return spyc_jar::upgrade::validate_stdin(),
        Some("resume-handover") => return spyc_jar::upgrade::resume(),
        Some("describe") => {
            let socket = std::env::args().nth(2).context("usage: spyc-jar describe SOCKET")?;
            for (k, v) in spyc_jar::client::describe(std::path::Path::new(&socket))? {
                println!("{k}: {v}");
            }
            return Ok(());
        }
        Some("upgrade") => {
            let mut a = std::env::args().skip(2);
            let (Some(socket), Some(binary)) = (a.next(), a.next()) else {
                bail!("usage: spyc-jar upgrade SOCKET BINARY");
            };
            return upgrade(std::path::Path::new(&socket), binary);
        }
        _ => {}
    }
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
            // R32a: end the program after this long with no owner; 0 ends it
            // as soon as its owner goes.
            "--linger-ownerless-ms" => {
                cfg.linger = Some(Duration::from_millis(value("--linger-ownerless-ms")?.parse()?));
            }
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

/// `spyc-jar upgrade SOCKET BINARY`: ask the jar at `SOCKET` to replace its
/// binary with `BINARY`, and report how long its program went unserved.
fn upgrade(socket: &std::path::Path, binary: String) -> anyhow::Result<()> {
    use spyc_jar::client::Client;
    use spyc_jar::proto::{Msg, Role};
    let (mut c, _) = Client::connect(socket, Role::Reader, Duration::from_secs(5))?;
    let t0 = std::time::Instant::now();
    c.send(&Msg::Upgrade { binary })?;
    loop {
        match c.recv(Duration::from_secs(10))? {
            Some(Msg::Upgraded { build }) => {
                println!("upgraded in {:.1} ms to {build}", t0.elapsed().as_secs_f64() * 1000.0);
                return Ok(());
            }
            Some(Msg::Refused { why }) => bail!("{why}"),
            Some(_) => {}
            None => bail!("no answer within 10 s"),
        }
    }
}

/// A random instance id, from the kernel's randomness by way of std's hasher
/// seed: nothing secret rides on it, it only tells two jars apart.
fn instance() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.finish()
}

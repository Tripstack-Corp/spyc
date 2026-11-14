//! The `spyc-jar` command line, which spyc's `spyc-jar` binary runs.
//!
//! `spyc-jar --wire N --socket PATH [options] -- PROGRAM [ARGS…] < TOKEN`
//!
//! Options: `--cwd DIR`, `--rows N`, `--cols N`, `--ring BYTES`,
//! `--window BYTES`, `--deadline-ms N`, `--linger-ms N`,
//! `--linger-ownerless-ms N`.
//!
//! `--wire N` names the protocol version its starter speaks. A jar that
//! doesn't speak it exits with [`WIRE_NOT_SERVED`] before it reads anything
//! else or starts the program, whatever else the command line says, so a spyc
//! that finds a newer or older `spyc-jar` beside it can tell, and run the
//! program another way.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use crate::jar;
use crate::state::Config;
use anyhow::{Context, bail};

/// The exit status of a jar asked to start for a wire version it doesn't
/// speak (or for none).
const WIRE_NOT_SERVED: i32 = 75;

/// Run `spyc-jar` with this process's arguments: a subcommand, or a jar.
pub fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("pty-child") => {
            let program: Vec<String> = std::env::args().skip(3).collect();
            crate::pty::child(&program)
        }
        Some("validate-handover") => crate::upgrade::validate_stdin(),
        Some("resume-handover") => crate::upgrade::resume(),
        Some("hold-handover") => crate::upgrade::hold_stdin(),
        Some("program-events") => program_events(),
        Some("describe") => {
            let socket = std::env::args()
                .nth(2)
                .context("usage: spyc-jar describe SOCKET")?;
            for (k, v) in crate::client::describe(std::path::Path::new(&socket))? {
                println!("{k}: {v}");
            }
            Ok(())
        }
        Some("upgrade") => {
            let mut a = std::env::args().skip(2);
            let (Some(socket), Some(token), Some(binary)) = (a.next(), a.next(), a.next()) else {
                bail!("usage: spyc-jar upgrade SOCKET TOKEN_FILE BINARY");
            };
            let token = crate::client::read_token_file(std::path::Path::new(&token))?;
            upgrade(std::path::Path::new(&socket), &token, binary)
        }
        Some("attach") => {
            let (Some(socket), Some(token)) = (std::env::args().nth(2), std::env::args().nth(3))
            else {
                bail!("usage: spyc-jar attach SOCKET TOKEN_FILE");
            };
            let token = crate::client::read_token_file(std::path::Path::new(&token))?;
            crate::attach::run(std::path::Path::new(&socket), &token)
        }
        _ => run_jar(),
    }
}

/// Start a jar: check the wire version, read the flags and the token, leave
/// the starter's session, bind the socket and run the program.
fn run_jar() -> anyhow::Result<()> {
    // Before anything else is read: the flags after it are this version's.
    let all: Vec<String> = std::env::args().collect();
    let wire = all
        .iter()
        .position(|a| a == "--wire")
        .and_then(|i| all.get(i + 1))
        .and_then(|v| v.parse::<u16>().ok());
    if wire != Some(crate::proto::VERSION) {
        eprintln!(
            "spyc-jar: speaks wire version {}, asked for {wire:?}",
            crate::proto::VERSION
        );
        std::process::exit(WIRE_NOT_SERVED);
    }
    let cfg = parse(std::env::args().skip(1))?;
    // Leave the session that started us, so closing that terminal or the end of
    // the spyc that spawned us reaches neither the jar nor its program.
    // A jar that can't leave its session would die with that terminal, which
    // is the one thing it exists not to do, so it refuses to start rather than
    // run in an indeterminate state. (`setsid` fails for a process group
    // leader, such as a job a shell started with `&`.)
    let token = read_token()?;
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
    jar::run(cfg, listener, token)
}

/// The jar's configuration from its flags.
fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Config> {
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
        linger: None,
        instance: instance(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match a.as_str() {
            "--wire" => {
                value("--wire")?;
            }
            "--socket" => cfg.socket = value("--socket")?.into(),
            "--cwd" => cfg.cwd = Some(value("--cwd")?.into()),
            "--rows" => cfg.rows = value("--rows")?.parse()?,
            "--cols" => cfg.cols = value("--cols")?.parse()?,
            "--ring" => cfg.ring_cap = value("--ring")?.parse()?,
            "--window" => cfg.window = value("--window")?.parse()?,
            "--deadline-ms" => {
                cfg.deadline = Duration::from_millis(value("--deadline-ms")?.parse()?);
            }
            // R32a: end the program after this long with no owner; 0 ends it
            // as soon as its owner goes.
            "--linger-ownerless-ms" => {
                cfg.linger = Some(Duration::from_millis(
                    value("--linger-ownerless-ms")?.parse()?,
                ));
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
    Ok(cfg)
}

/// R26: the token its starter hands the jar on stdin, never in argv or the
/// environment, which other processes can read. A jar without one refuses to
/// run: every client but a lookup would be refused.
fn read_token() -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    // SPYC-TRAP(jar-token-stdin): never from argv or the environment, which
    // other processes, the jar's own program among them, can read.
    let mut buf = Vec::new();
    std::io::stdin()
        .take(1024)
        .read_to_end(&mut buf)
        .context("spyc-jar: reading its token from stdin")?;
    let token = buf.trim_ascii().to_vec();
    if token.len() < 16 {
        bail!("spyc-jar: no token on stdin (16 bytes or more); refusing to run");
    }
    Ok(token)
}

/// `spyc-jar upgrade SOCKET TOKEN_FILE BINARY`: ask the jar at `SOCKET` to
/// replace its binary with `BINARY`, and report how long its program went
/// unserved.
fn upgrade(socket: &std::path::Path, token: &[u8], binary: String) -> anyhow::Result<()> {
    use crate::client::Client;
    use crate::proto::{Msg, Role};
    let (mut c, _) = Client::connect(socket, Role::Reader, Duration::from_secs(5), token)?;
    let t0 = std::time::Instant::now();
    c.send(&Msg::Upgrade { binary })?;
    loop {
        match c.recv(Duration::from_secs(10))? {
            Some(Msg::Upgraded { build }) => {
                println!(
                    "upgraded in {:.1} ms to {build}",
                    t0.elapsed().as_secs_f64() * 1000.0
                );
                return Ok(());
            }
            Some(Msg::Refused { why, .. }) => bail!("{why}"),
            Some(_) => {}
            None => bail!("no answer within 10 s"),
        }
    }
}

/// `spyc-jar program-events`, a stand-in program for tests: it connects to
/// its jar as role `program`, prints each event it hears as a line, and asks
/// the jar to detach its owner when a line `detach` arrives on its input.
fn program_events() -> anyhow::Result<()> {
    use crate::client::Client;
    use crate::proto::{Msg, Role};
    let socket = std::env::var_os("SPYC_JAR_SOCKET").context("no SPYC_JAR_SOCKET")?;
    let (client, _) = Client::connect(
        std::path::Path::new(&socket),
        Role::Program,
        Duration::from_secs(5),
        b"",
    )?;
    println!("program: connected");
    let (mut rx, tx) = client.split();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            if line.is_ok_and(|l| l.trim() == "detach") {
                println!("program: bye");
                let _ = tx.send(&Msg::Detach);
            }
        }
    });
    loop {
        match rx.recv(Duration::from_secs(60)) {
            Ok(Some(Msg::Attached)) => println!("program: attached"),
            Ok(Some(Msg::Detached)) => println!("program: detached"),
            Ok(_) => {}
            Err(_) => return Ok(()),
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

//! The `spyc-jar` command line, which spyc's `spyc-jar` binary runs.
//!
//! `spyc-jar --wire N [options] < START`
//!
//! `START` is the token, the socket, the program and how it starts
//! ([`Start`]), on standard input, not the command line, which `ps` shows
//! everyone for the jar's whole life: a tab's command may carry a prompt. A
//! starter may name the jar's first argument (`argv[0]`) as it likes, as spyc
//! does with the program's name.
//!
//! Options, which spyc leaves at their defaults: `--ring BYTES`,
//! `--window BYTES`, `--deadline-ms N`, `--linger-ms N`,
//! `--linger-ownerless-ms N`.
//!
//! `--wire N` names the protocol version its starter speaks. A jar that
//! doesn't speak it exits with [`WIRE_NOT_SERVED`] before it reads anything
//! else or starts the program, whatever else the command line says, so a spyc
//! that finds a newer or older `spyc-jar` beside it can tell, and run the
//! program another way.
//!
//! `PATH`'s directory must be the user's own, `0700` and not a link, or the
//! jar refuses to run: anyone else who could write there could put a socket
//! of theirs in its place.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use crate::jar;
use crate::state::Config;
use anyhow::{Context, bail};

/// The exit status of a jar asked to start for a wire version it doesn't
/// speak (or for none).
pub const WIRE_NOT_SERVED: i32 = 75;

/// The git commit the running binary was built from, as the binary that
/// calls [`main_built`] knows it; empty when it doesn't say.
pub fn commit() -> &'static str {
    BUILD.get().map_or("", |b| b.0)
}

/// The commit and build time the running binary was built with.
static BUILD: std::sync::OnceLock<(&'static str, &'static str)> = std::sync::OnceLock::new();

/// [`main`], for a binary that knows the commit it was built from (which
/// `describe` gives as `commit`) and when: `build-info` prints both, so spyc
/// can tell its own build's `spyc-jar` from another's.
pub fn main_built(commit: &'static str, built: &'static str) -> anyhow::Result<()> {
    let _ = BUILD.set((commit, built));
    main()
}

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
        Some("build-info") => {
            let (commit, built) = BUILD.get().copied().unwrap_or(("", ""));
            println!("{commit}\n{built}");
            Ok(())
        }
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
            // A jar takes an absolute path only.
            let binary = std::path::absolute(&binary)?.display().to_string();
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
    let mut cfg = parse(std::env::args().skip(1))?;
    // Leave the session that started us, so closing that terminal or the end of
    // the spyc that spawned us reaches neither the jar nor its program.
    // A jar that can't leave its session would die with that terminal, which
    // is the one thing it exists not to do, so it refuses to start rather than
    // run in an indeterminate state. (`setsid` fails for a process group
    // leader, such as a job a shell started with `&`.)
    let start = read_start()?;
    (cfg.socket, cfg.cwd, cfg.argv) = (start.socket, start.cwd, start.argv);
    (cfg.rows, cfg.cols, cfg.start_on_owner) = (start.rows, start.cols, start.hold);
    if let Some(linger) = start.exit_linger {
        cfg.linger_after_exit = linger;
    }
    rustix::process::setsid().context(
        "spyc-jar: can't start its own session (started as a process group leader?); refusing to run",
    )?;

    if let Some(dir) = cfg.socket.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        private_dir(dir)?;
    }
    // Before the socket: a jar whose lock is taken is a jar from then on.
    crate::lock::take(&cfg.socket)?;
    if cfg.socket.exists() {
        if UnixStream::connect(&cfg.socket).is_ok() {
            bail!("a jar is already listening on {}", cfg.socket.display());
        }
        std::fs::remove_file(&cfg.socket)?;
    }
    let listener = UnixListener::bind(&cfg.socket)
        .with_context(|| format!("bind {}", cfg.socket.display()))?;
    std::fs::set_permissions(&cfg.socket, std::fs::Permissions::from_mode(0o600))?;
    jar::run(cfg, listener, start.token)
}

/// A jar listens only in a directory that is this user's alone: anyone else
/// who could write there could put their own socket in its place (finding 14).
fn private_dir(dir: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = dir
        .symlink_metadata()
        .with_context(|| format!("look at {}", dir.display()))?;
    let uid = rustix::process::getuid().as_raw();
    if !meta.file_type().is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        bail!(
            "spyc-jar: {} must be a directory of uid {uid}'s, mode 0700 and not a link; refusing to run",
            dir.display()
        );
    }
    Ok(())
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
        deadline: crate::limits::OWNER_DEADLINE,
        linger_after_exit: Duration::from_secs(60),
        linger: None,
        instance: instance(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        start_on_owner: None,
    };
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match a.as_str() {
            "--wire" => {
                value("--wire")?;
            }
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
            "--" => bail!("a jar's program goes on its standard input, after its token"),
            other => bail!("unknown argument {other}"),
        }
    }
    // R11: a live owner may be a window plus one read behind, and all of it
    // must stay in the ring, or the log drops bytes the owner was never sent
    // (review finding 18).
    if cfg.window.saturating_add(jar::PTY_READ as u64) > cfg.ring_cap as u64 {
        bail!(
            "a window of {} bytes needs a ring of at least {}, not {}",
            cfg.window,
            cfg.window.saturating_add(jar::PTY_READ as u64),
            cfg.ring_cap
        );
    }
    Ok(cfg)
}

/// The most a jar reads from its standard input: as much as `exec` takes on
/// macOS.
const START_MAX: u64 = 1 << 20;

/// What a jar's starter writes to its standard input before closing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// R26: what every client but a lookup presents; 16 bytes or more.
    pub token: Vec<u8>,
    /// Where the jar listens, an absolute path: its program is handed it.
    pub socket: PathBuf,
    /// Where the program starts; `None` for the jar's own directory.
    pub cwd: Option<PathBuf>,
    /// The PTY's size when the program starts.
    pub rows: u16,
    pub cols: u16,
    /// Decision M: the program starts when the first owner's `hello` is
    /// accepted, and the jar ends having started nothing if none comes within
    /// this long; `None` starts it at once.
    pub hold: Option<Duration>,
    /// How long the program's output and exit are kept once it has ended,
    /// for an owner that isn't there; `None` for the jar's default.
    /// [`UNTIL_READ`] keeps them until an owner collects the exit.
    pub exit_linger: Option<Duration>,
    /// The program and its arguments.
    pub argv: Vec<String>,
}

/// An exit linger with no end: the output and exit wait for an owner to
/// collect them (a `!` capture's).
pub const UNTIL_READ: Duration = Duration::from_millis(u64::MAX);

impl Start {
    /// The start as bytes: the token, the socket, the directory (empty for
    /// none), rows, columns, the hold and the exit linger in milliseconds
    /// (each empty for none), how many arguments there are, then the
    /// arguments, each ended by a NUL, numbers in decimal. The count tells a
    /// start cut short from a shorter program.
    pub fn encode(&self) -> Vec<u8> {
        use std::os::unix::ffi::OsStrExt;
        let ms = |d: Option<Duration>| d.map(|d| d.as_millis().to_string());
        let (hold, exit_linger) = (ms(self.hold), ms(self.exit_linger));
        let (rows, cols) = (self.rows.to_string(), self.cols.to_string());
        let count = self.argv.len().to_string();
        let head: [&[u8]; 8] = [
            &self.token,
            self.socket.as_os_str().as_bytes(),
            self.cwd.as_ref().map_or(&[], |c| c.as_os_str().as_bytes()),
            rows.as_bytes(),
            cols.as_bytes(),
            hold.as_ref().map_or(&[], |h| h.as_bytes()),
            exit_linger.as_ref().map_or(&[], |l| l.as_bytes()),
            count.as_bytes(),
        ];
        let mut out = Vec::new();
        for field in head
            .into_iter()
            .chain(self.argv.iter().map(String::as_bytes))
        {
            out.extend_from_slice(field);
            out.push(0);
        }
        out
    }

    /// [`Start::encode`]'s bytes, or why they aren't a start: a token under 16
    /// bytes, a socket not absolute, a field without its NUL, a number that
    /// isn't one, arguments
    /// other than the count says, no program, or an argument not UTF-8.
    pub fn parse(input: &[u8]) -> anyhow::Result<Self> {
        use std::os::unix::ffi::OsStringExt;
        let number = |field: &[u8], what: &str| -> anyhow::Result<u64> {
            std::str::from_utf8(field)
                .ok()
                .and_then(|f| f.parse().ok())
                .with_context(|| format!("its {what} isn't a number"))
        };
        let mut fields: Vec<&[u8]> = input.split(|&b| b == 0).collect();
        if fields.pop().is_none_or(|last| !last.is_empty()) {
            bail!("its last field has no NUL");
        }
        let [
            token,
            socket,
            cwd,
            rows,
            cols,
            hold,
            exit_linger,
            count,
            argv @ ..,
        ] = fields.as_slice()
        else {
            bail!("it ends before its arguments");
        };
        let token = token.trim_ascii().to_vec();
        if token.len() < 16 {
            bail!("no token (16 bytes or more)");
        }
        let socket = PathBuf::from(std::ffi::OsString::from_vec(socket.to_vec()));
        if !socket.is_absolute() {
            bail!("its socket, {}, isn't an absolute path", socket.display());
        }
        let count = number(count, "count")?;
        if argv.len() as u64 != count {
            bail!("{} arguments, not the {count} its count says", argv.len());
        }
        if argv.is_empty() {
            bail!("no program");
        }
        Ok(Self {
            token,
            socket,
            cwd: (!cwd.is_empty()).then(|| std::ffi::OsString::from_vec(cwd.to_vec()).into()),
            rows: u16::try_from(number(rows, "rows")?).context("rows")?,
            cols: u16::try_from(number(cols, "columns")?).context("columns")?,
            hold: if hold.is_empty() {
                None
            } else {
                Some(Duration::from_millis(number(hold, "hold")?))
            },
            exit_linger: if exit_linger.is_empty() {
                None
            } else {
                Some(Duration::from_millis(number(exit_linger, "exit linger")?))
            },
            argv: argv
                .iter()
                .map(|a| String::from_utf8(a.to_vec()).context("an argument isn't UTF-8"))
                .collect::<anyhow::Result<_>>()?,
        })
    }
}

/// R26: the token its starter hands the jar on stdin, never in argv or the
/// environment, which other processes can read, and the program with it. A
/// jar without them refuses to run: every client but a lookup would be
/// refused.
fn read_start() -> anyhow::Result<Start> {
    use std::io::Read;
    // SPYC-TRAP(jar-token-stdin): never from argv or the environment, which
    // other processes, the jar's own program among them, can read.
    let mut buf = Vec::new();
    std::io::stdin()
        .take(START_MAX + 1)
        .read_to_end(&mut buf)
        .context("spyc-jar: reading its start from stdin")?;
    if buf.len() as u64 > START_MAX {
        bail!("spyc-jar: more than {START_MAX} bytes on stdin; refusing to run");
    }
    Start::parse(&buf).context("spyc-jar: no start on stdin; refusing to run")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn start(argv: &[&str]) -> Start {
        Start {
            token: b"0123456789abcdef0123".to_vec(),
            socket: "/run/j.sock".into(),
            cwd: None,
            rows: 24,
            cols: 80,
            hold: None,
            exit_linger: None,
            argv: argv.iter().map(|a| (*a).to_string()).collect(),
        }
    }

    /// What a starter writes is what the jar reads: every field, spaces and
    /// all, with and without a directory and a hold.
    #[test]
    fn a_start_reads_back_what_its_starter_wrote() {
        let plain = start(&["/bin/sh", "-c", "printf '%s' \"a b\"; exec cat"]);
        let full = Start {
            cwd: Some("/a dir/x".into()),
            rows: 61,
            cols: 397,
            hold: Some(Duration::from_secs(30)),
            exit_linger: Some(UNTIL_READ),
            ..plain.clone()
        };
        for sent in [plain, full] {
            assert_eq!(Start::parse(&sent.encode()).unwrap(), sent);
        }
    }

    /// A start cut short, run on, or missing its token or program, starts
    /// nothing: never a program short of its last argument.
    #[test]
    fn a_short_or_partial_start_is_refused() {
        let whole = start(&["/bin/sh", "-c", "exit 3"]).encode();
        for cut in 0..whole.len() {
            assert!(Start::parse(&whole[..cut]).is_err(), "{:?}", &whole[..cut]);
        }
        assert!(
            Start::parse(&[&whole[..], b"x"].concat()).is_err(),
            "bytes after the start"
        );
        let refused = [
            Start {
                token: b"short".to_vec(),
                ..start(&["/bin/cat"])
            },
            start(&[]),
            Start {
                socket: "j.sock".into(),
                ..start(&["/bin/cat"])
            },
        ];
        for bad in refused {
            assert!(Start::parse(&bad.encode()).is_err(), "{bad:?}");
        }
        let mut not_utf8 = start(&["/bin/cat", "x"]).encode();
        let x = not_utf8.len() - 2;
        not_utf8[x] = 0xff;
        assert!(Start::parse(&not_utf8).is_err(), "an argument not UTF-8");
        let huge = b"0123456789abcdef0123\0/j.sock\0\x0070000\x0080\0\0\x001\0/bin/cat\0";
        assert!(Start::parse(&huge[..]).is_err(), "rows past a u16");
        let fine = b"0123456789abcdef0123\0/j.sock\0\x0060000\x0080\0\0\x001\0/bin/cat\0";
        assert!(
            Start::parse(&fine[..]).is_ok(),
            "premise: only the rows are wrong"
        );
    }
}

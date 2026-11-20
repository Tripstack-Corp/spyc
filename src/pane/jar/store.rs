//! spyc's store of jar binaries (decision L): every jar starts from an entry
//! here, `<state>/jar-bin/<version>-<hash>/spyc-jar`, never from the
//! `spyc-jar` beside spyc, which an install may replace while spyc runs.
//! `<hash>` is the first 12 hex digits of the binary's SHA-256, so two builds
//! of one version differ, and `ps` shows which version a jar runs.
//!
//! At spyc's start a worker off the loop copies the `spyc-jar` beside spyc in,
//! hashing as it writes and writing the entry whole, then runs the copy once
//! as `spyc-jar build-info`. That checks the copy is this build's, not one a
//! package upgrade landed meanwhile, and pays a new file's first run (about
//! 263 ms on macOS) away from any pane. The first jar waits for it. An entry
//! that exists already is hashed again, never rewritten. The directory holds
//! `spyc_jar::store::MARKER`, which is how a jar knows it runs from a store.
//!
//! Then a sweep, which nothing waits on: an entry goes only when no live jar
//! reports it as its `build`, it isn't this spyc's, and it has gone a day
//! untouched; when any live jar won't describe itself (one mid-upgrade, say),
//! nothing goes.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};
use spyc_jar::store::{ENTRY_FILE, MARKER};

/// What the jars this session start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binary {
    /// An entry of the store, checked.
    Entry(PathBuf),
    /// The `spyc-jar` beside spyc, which runs as this build: the store
    /// couldn't hold a copy that runs (a state directory mounted `noexec`,
    /// say).
    Beside(PathBuf, String),
    /// No usable jar this session: the `spyc-jar` beside spyc is missing,
    /// can't run, or isn't this build's.
    None(String),
}

/// The build a `spyc-jar` must say it is, as `build-info` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build<'a> {
    pub commit: &'a str,
    pub built: &'a str,
}

impl Build<'_> {
    /// Whether `build-info` said this build: its commit, then its time.
    fn said_by(&self, (commit, built): &(String, String)) -> bool {
        commit == self.commit && built == self.built
    }
}

/// This spyc's build.
pub const OURS: Build<'static> = Build {
    commit: env!("SPYC_GIT_SHA"),
    built: env!("SPYC_BUILD_TIME"),
};

/// How long an entry may go untouched before a sweep may take it.
pub const UNTOUCHED: Duration = Duration::from_hours(24);
/// How long `build-info` has to answer.
const BUILD_INFO_WAIT: Duration = Duration::from_secs(10);
/// How often `build-info` is looked at, within [`BUILD_INFO_WAIT`].
const BUILD_INFO_POLL: Duration = Duration::from_millis(5);

/// Copy `source` into the store at `root`, check it, and say what jars start
/// from: the entry, or `source` when the store can't hold a copy that runs,
/// or nothing when `source` is missing, can't run or isn't `build`'s.
pub fn install(root: &Path, source: &Path, build: Build) -> Binary {
    let entry = match put(root, source) {
        Ok(entry) => entry,
        Err(e) => return beside(source, build, format!("{e:#}")),
    };
    match build_info(&entry) {
        Ok(said) if build.said_by(&said) => Binary::Entry(entry),
        Ok(said) => Binary::None(another_build(source, &said, build)),
        Err(e) => beside(source, build, format!("{e:#}")),
    }
}

/// `source` itself, when the store has no copy that runs (`why`): only if it
/// runs as `build`.
fn beside(source: &Path, build: Build, why: String) -> Binary {
    if !source.is_file() {
        return Binary::None(format!("there is no {}", source.display()));
    }
    match build_info(source) {
        Ok(said) if build.said_by(&said) => Binary::Beside(source.to_path_buf(), why),
        Ok(said) => Binary::None(another_build(source, &said, build)),
        Err(e) => Binary::None(format!("{} can't run ({e:#})", source.display())),
    }
}

fn another_build(source: &Path, (commit, built): &(String, String), build: Build) -> String {
    format!(
        "{} is another build ({commit} {built}, not {} {}): was spyc upgraded while it started?",
        source.display(),
        build.commit,
        build.built
    )
}

/// The store's entry for `source`, written whole if it isn't there yet.
fn put(root: &Path, source: &Path) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).with_context(|| format!("create {}", root.display())),
    }
    spyc_jar::store::private_dir(root)?;
    let marker = root.join(MARKER);
    if marker.symlink_metadata().is_err() {
        std::fs::write(&marker, b"").with_context(|| format!("write {}", marker.display()))?;
    }
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let incoming = root.join(format!(".incoming.{}.{nanos:x}", std::process::id()));
    let hash = (|| -> anyhow::Result<String> {
        let mut from =
            std::fs::File::open(source).with_context(|| format!("open {}", source.display()))?;
        let mut to = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&incoming)
            .with_context(|| format!("create {}", incoming.display()))?;
        let hash = copy_hashing(&mut from, &mut to)?;
        to.sync_all()?;
        Ok(hash)
    })();
    let hash = match hash {
        Ok(h) => h,
        Err(e) => {
            let _ = std::fs::remove_file(&incoming);
            return Err(e);
        }
    };
    let dir = root.join(entry_name(&hash));
    let file = dir.join(ENTRY_FILE);
    if file.symlink_metadata().is_ok() {
        let _ = std::fs::remove_file(&incoming);
        let again = copy_hashing(
            &mut std::fs::File::open(&file).with_context(|| format!("open {}", file.display()))?,
            &mut std::io::sink(),
        )?;
        if again != hash {
            bail!("{} isn't the binary its name says", file.display());
        }
        touch(&dir);
        return Ok(file);
    }
    let placed = (|| -> anyhow::Result<()> {
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).with_context(|| format!("create {}", dir.display())),
        }
        std::fs::set_permissions(&incoming, std::fs::Permissions::from_mode(0o555))?;
        std::fs::rename(&incoming, &file)
            .with_context(|| format!("rename into {}", file.display()))?;
        if let Ok(d) = std::fs::File::open(&dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if let Err(e) = placed {
        let _ = std::fs::remove_file(&incoming);
        return Err(e);
    }
    Ok(file)
}

/// The entry of a binary whose SHA-256 is `sha256`, in hex: the jar's
/// version and the hash's first 12 digits.
fn entry_name(sha256: &str) -> String {
    format!(
        "{}-{}",
        spyc_jar::PACKAGE_VERSION,
        sha256.get(..12).unwrap_or(sha256)
    )
}

/// Copy `from` to `to`, returning the SHA-256 of what went, in hex.
fn copy_hashing(from: &mut impl Read, to: &mut impl Write) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        to.write_all(&buf[..n])?;
    }
    to.flush()?;
    Ok(hasher.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    }))
}

/// What `binary build-info` says it is: its commit and build time.
fn build_info(binary: &Path) -> anyhow::Result<(String, String)> {
    let mut child = std::process::Command::new(binary)
        .arg("build-info")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("run {}", binary.display()))?;
    let until = std::time::Instant::now() + BUILD_INFO_WAIT;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if std::time::Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{} build-info didn't answer", binary.display());
        }
        std::thread::sleep(BUILD_INFO_POLL);
    };
    let mut out = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut out);
    }
    if !status.success() {
        bail!("{} build-info failed: {status}", binary.display());
    }
    let mut lines = out.lines();
    Ok((
        lines.next().unwrap_or_default().to_string(),
        lines.next().unwrap_or_default().to_string(),
    ))
}

/// Mark the entry directory `dir` as just used, for the sweep.
pub fn touch(dir: &Path) {
    use rustix::fs::{AtFlags, CWD, Timespec, Timestamps, UTIME_NOW};
    let now = Timespec {
        tv_sec: 0,
        tv_nsec: UTIME_NOW,
    };
    let _ = rustix::fs::utimensat(
        CWD,
        dir,
        &Timestamps {
            last_access: now,
            last_modification: now,
        },
        AtFlags::empty(),
    );
}

/// Remove the entries of the store at `root` that no live jar in `sockets`
/// reports, that aren't `ours`, and that have gone `untouched` long; touch
/// the ones a live jar reports. Nothing goes when a live jar won't say what
/// it runs. Returns the entries removed.
pub fn sweep(root: &Path, sockets: &Path, ours: &Path, untouched: Duration) -> Vec<PathBuf> {
    let Some(reported) = reported_builds(sockets) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for dir in entries.filter_map(Result::ok).map(|e| e.path()) {
        let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if name.starts_with('.') || !dir.is_dir() {
            continue;
        }
        if reported.contains(&name) {
            touch(&dir);
            continue;
        }
        let old = dir
            .symlink_metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|age| age >= untouched));
        if old && ours.parent() != Some(dir.as_path()) && std::fs::remove_dir_all(&dir).is_ok() {
            removed.push(dir);
        }
    }
    removed
}

/// The `build` every live jar in `sockets` reports; `None` when one that
/// answers a connection won't describe itself.
fn reported_builds(sockets: &Path) -> Option<std::collections::HashSet<String>> {
    let mut builds = std::collections::HashSet::new();
    let Ok(entries) = std::fs::read_dir(sockets) else {
        return Some(builds);
    };
    for sock in entries.filter_map(Result::ok).map(|e| e.path()) {
        use super::place::SocketState;
        if sock.extension().is_none_or(|x| x != "sock") {
            continue;
        }
        match super::place::socket_state(&sock) {
            SocketState::Live => {}
            // A jar that may run from an entry this can't name.
            SocketState::Unknown => return None,
            SocketState::Stale | SocketState::Missing => continue,
        }
        let fields = spyc_jar::client::describe(&sock).ok()?;
        if let Some((_, b)) = fields.into_iter().find(|(k, _)| k == "build") {
            builds.insert(b);
        }
    }
    Some(builds)
}

/// The store's outcome for this session, once the worker has it.
static READY: Mutex<Option<Binary>> = Mutex::new(None);
static READY_CV: Condvar = Condvar::new();
static STARTED: std::sync::Once = std::sync::Once::new();

/// Start the store's worker, once a session: copy, check, then sweep.
pub fn start(root: PathBuf, source: PathBuf, sockets: PathBuf) {
    STARTED.call_once(|| {
        std::thread::spawn(move || {
            let binary = install(&root, &source, OURS);
            match &binary {
                Binary::Entry(_) => {}
                Binary::Beside(at, why) => eprintln!(
                    "spyc: jars start from {} this session; the store's copy can't run ({why})",
                    at.display()
                ),
                Binary::None(why) => eprintln!("spyc: no usable spyc-jar this session: {why}"),
            }
            let ours = match &binary {
                Binary::Entry(e) => e.clone(),
                _ => PathBuf::new(),
            };
            *READY.lock().unwrap_or_else(PoisonError::into_inner) = Some(binary);
            READY_CV.notify_all();
            let removed = sweep(&root, &sockets, &ours, UNTOUCHED);
            crate::spyc_debug!("jar store sweep: {} entries removed", removed.len());
        });
    });
}

/// What jars start from this session, once the worker [`start`]ed has it;
/// `None` before, without waiting.
pub fn ready() -> Option<Binary> {
    READY.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// What jars start from this session, waiting for the worker [`start`]ed.
pub fn binary() -> Binary {
    let mut ready = READY.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(b) = ready.as_ref() {
            return b.clone();
        }
        ready = READY_CV.wait(ready).unwrap_or_else(PoisonError::into_inner);
    }
}

//! A jar's token (R26): 32 random bytes as hex, handed to the jar on its
//! standard input and kept `0600` under spyc's state, by the socket's name, so
//! a spyc after a crash can take its jars back. Never in an argument or the
//! environment, where any process of the user's can read it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::place::Place;

/// A fresh token.
pub(super) fn new() -> anyhow::Result<Vec<u8>> {
    let mut raw = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut raw))
        .context("read a jar token from /dev/urandom")?;
    Ok(raw
        .iter()
        .fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        })
        .into_bytes())
}

fn path(place: &Place, socket: &Path) -> Option<PathBuf> {
    Some(place.tokens.join(socket.file_stem()?))
}

pub(super) fn save(place: &Place, socket: &Path, token: &[u8]) -> anyhow::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let path = path(place, socket).context("a socket without a name")?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&place.tokens)
        .with_context(|| format!("create {}", place.tokens.display()))?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("write {}", path.display()))?;
    f.write_all(token)?;
    Ok(())
}

/// The token saved for the jar at `socket`.
pub(super) fn load(place: &Place, socket: &Path) -> Option<Vec<u8>> {
    spyc_jar::client::read_token_file(&path(place, socket)?).ok()
}

pub(super) fn forget(place: &Place, socket: &Path) {
    if let Some(p) = path(place, socket) {
        let _ = std::fs::remove_file(p);
    }
}

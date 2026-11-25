//! Syscalls the probe itself makes while observing, counted at each call site:
//! the cost table's syscall column. File reads are opened, read to the end and
//! closed by hand so every `read` is counted; a directory listing counts its
//! `open`, `close` and an estimate of two `getdents`, which `strace -c`
//! checks on Linux (README, "Cost").

use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};

static CALLS: AtomicU64 = AtomicU64::new(0);

pub fn add(n: u64) {
    CALLS.fetch_add(n, Ordering::Relaxed);
}

pub fn take() -> u64 {
    CALLS.swap(0, Ordering::Relaxed)
}

/// `errno`'s name for an I/O error, or its message.
pub fn errno(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(1) => "EPERM".into(),
        Some(2) => "ENOENT".into(),
        Some(3) => "ESRCH".into(),
        Some(5) => "EIO".into(),
        Some(6) => "ENXIO".into(),
        Some(13) => "EACCES".into(),
        Some(22) => "EINVAL".into(),
        Some(25) => "ENOTTY".into(),
        Some(n) => format!("errno {n}"),
        None => e.to_string(),
    }
}

pub fn read_file(path: &str) -> Result<String, String> {
    add(1);
    let mut f = std::fs::File::open(path).map_err(|e| errno(&e))?;
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    let r = loop {
        add(1);
        match f.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) => break Err(errno(&e)),
        }
    };
    add(1);
    drop(f);
    r.map(|()| String::from_utf8_lossy(&out).into_owned())
}

pub fn readlink(path: &str) -> Result<String, String> {
    add(1);
    std::fs::read_link(path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| errno(&e))
}

pub fn list_dir(path: &str) -> Result<Vec<String>, String> {
    add(4);
    let rd = std::fs::read_dir(path).map_err(|e| errno(&e))?;
    Ok(rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect())
}

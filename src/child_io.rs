//! A child process's input and output as files, never pipes, wherever a
//! reader waits for their end (slice A's finding, review 3.0 slice B). spyc
//! spawns from many threads, and Rust on macOS marks a pipe close-on-exec
//! only a moment after making it: a child another thread spawns in that
//! moment keeps the pipe's writer, and a reader waiting for the end waits as
//! long as that child lives. A file is close-on-exec from the first, nameless
//! from the first, and ends where its bytes do, whoever else holds it.

use std::fs::File;
use std::io::{self, Read, Seek, Write};

/// `bytes` as a nameless file at its start, to hand a child as its standard
/// input. Made `0600` and removed as it is made, so nothing else can open
/// it: a jar's token can travel this way.
pub fn input(bytes: &[u8]) -> io::Result<File> {
    let mut file = tempfile::tempfile()?;
    file.write_all(bytes)?;
    file.rewind()?;
    Ok(file)
}

/// A nameless file for a child to write to, and the same file to read it
/// back with [`written`] once the child has gone.
pub fn output() -> io::Result<(File, File)> {
    let file = tempfile::tempfile()?;
    let theirs = file.try_clone()?;
    Ok((file, theirs))
}

/// What a child wrote to `file`, up to `cap` bytes.
pub fn written(file: &mut File, cap: u64) -> io::Result<Vec<u8>> {
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(cap).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A child reads its input to the end and writes its output where spyc
    /// reads it back, while another process holds both files open: neither
    /// end waits on it.
    #[test]
    fn a_childs_input_and_output_end_whoever_else_holds_them() {
        let given = input(b"in to out").unwrap();
        let (mut out, theirs) = output().unwrap();
        let holder = std::process::Command::new("/bin/sleep")
            .arg("600")
            .stdin(given.try_clone().unwrap())
            .stdout(theirs.try_clone().unwrap())
            .spawn()
            .unwrap();
        let status = std::process::Command::new("/bin/cat")
            .stdin(given)
            .stdout(theirs)
            .status()
            .unwrap();
        let mut holder = holder;
        let _ = holder.kill();
        let _ = holder.wait();
        assert!(status.success());
        assert_eq!(written(&mut out, 1 << 20).unwrap(), b"in to out");
    }
}

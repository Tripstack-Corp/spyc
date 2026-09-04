//! Capture raw PTY byte streams as spike fixtures.
//!
//! Same seam spyc's own `SPYC_PTY_DEBUG` uses (`PtyHost::drain` →
//! `append_pty_debug`): whatever the child writes to the pty master, verbatim,
//! before any emulator touches it. Capturing here rather than from a running
//! spyc keeps the geometry fixed and the result regenerable by anyone.
//!
//! Usage:
//!   capture <name> <rows> <cols> <settle_ms> -- <cmd> [args...]
//!   capture <name> <rows> <cols> <settle_ms> --send '<bytes>' -- <cmd> ...
//!
//! `--send` accepts `\xNN`, `\e`, `\r`, `\n`, `\t` escapes and is written to the
//! child after an initial settle, then the stream is read for another settle
//! window. Capture always ends on a timer, never on EOF: a TUI does not exit.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sep = args
        .iter()
        .position(|a| a == "--")
        .expect("need `--` before the command");
    let head = &args[..sep];
    let cmd_args = &args[sep + 1..];
    assert!(!cmd_args.is_empty(), "no command after `--`");

    let name = &head[0];
    let rows: u16 = head[1].parse()?;
    let cols: u16 = head[2].parse()?;
    let settle = Duration::from_millis(head[3].parse()?);
    let send = head
        .iter()
        .position(|a| a == "--send")
        .map(|i| unescape(&head[i + 1]));

    let pty = native_pty_system();
    let pair = pty.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut builder = CommandBuilder::new(&cmd_args[0]);
    for a in &cmd_args[1..] {
        builder.arg(a);
    }
    // A fixed TERM keeps the child's capability probing deterministic across
    // machines; xterm-256color is what spyc's `PtySpec` sets.
    builder.env("TERM", "xterm-256color");
    builder.env("COLUMNS", cols.to_string());
    builder.env("LINES", rows.to_string());
    builder.cwd(std::env::current_dir()?);

    let mut child = pair.slave.spawn_command(builder)?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut out: Vec<u8> = Vec::new();
    let deadline = Instant::now() + settle;
    while Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            out.extend_from_slice(&chunk);
        }
    }

    if let Some(bytes) = send {
        let mut writer = pair.master.take_writer()?;
        writer.write_all(&bytes)?;
        writer.flush()?;
        drop(writer);
        let deadline = Instant::now() + settle;
        while Instant::now() < deadline {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
                out.extend_from_slice(&chunk);
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    let path = std::path::Path::new("fixtures").join(format!("{name}.bin"));
    std::fs::write(&path, &out)?;
    println!(
        "{:<28} {:>9} bytes  {rows}x{cols}  -> {}",
        name,
        out.len(),
        path.display()
    );
    Ok(())
}

/// Expand the escapes the shell would otherwise eat.
fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            match b[i] {
                b'e' => out.push(0x1b),
                b'r' => out.push(b'\r'),
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'\\' => out.push(b'\\'),
                b'x' if i + 2 < b.len() => {
                    let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("00");
                    out.push(u8::from_str_radix(hex, 16).unwrap_or(0));
                    i += 2;
                }
                other => out.push(other),
            }
            i += 1;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

//! End-to-end pty roundtrip integration test.
//!
//! Spawns `cat` via `portable-pty`, writes a line of bytes to the
//! master, reads what cat echoes back, parses through `vt100`, and
//! asserts the rendered screen contains the expected text.
//!
//! Validates the integration contract spyc relies on (`portable-pty`
//! pty plumbing + `vt100` parser) without going through any
//! spyc-internal wiring. If a future portable-pty release stops
//! delivering CRLF-translated bytes, or vt100 changes how it lays out
//! cells, this test trips. Spyc's own pane code has unit tests that
//! cover the wrapper layer.

#![cfg(unix)]

use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

#[test]
fn cat_roundtrip_renders_input_in_vt100_screen() {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");

    let mut child = pair
        .slave
        .spawn_command(CommandBuilder::new("cat"))
        .expect("spawn cat");
    drop(pair.slave);

    let mut writer = pair.master.take_writer().expect("take_writer");
    let mut reader = pair.master.try_clone_reader().expect("try_clone_reader");

    // Drain in a thread so the master never back-pressures cat.
    // Reads return EOF once the master fd closes (after we drop it
    // below) plus the slave side closes (when cat exits).
    let drain = thread::spawn(move || -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
        }
        out
    });

    // The pty slave is in canonical (cooked) mode by default, so the
    // line discipline echoes input back to the master AND delivers
    // the line to cat after \n. ^D (EOT, 0x04) signals EOF so cat
    // exits cleanly.
    writer.write_all(b"hello, spyc!\n").expect("write line");
    writer.write_all(b"\x04").expect("write EOF");
    drop(writer);

    // Bound the wait so a hung cat can't hang CI forever. cat with no
    // file argument exits as soon as it sees EOF on stdin.
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < exit_deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);

    let bytes = drain.join().expect("drain thread joined");
    assert!(
        !bytes.is_empty(),
        "expected pty to deliver some bytes; got nothing"
    );

    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(&bytes);

    // Cooked-mode line discipline echoes the typed line, and cat
    // re-emits it after the \n. Either echo lands on row 0 — if it
    // doesn't, the integration is broken regardless of which layer.
    let screen = parser.screen();
    let row0: String = (0..80)
        .filter_map(|c| screen.cell(0, c))
        .map(|c| {
            let s = c.contents();
            if s.is_empty() {
                " ".to_string()
            } else {
                s.to_string()
            }
        })
        .collect();

    assert!(
        row0.starts_with("hello, spyc!"),
        "row 0 was {:?}; raw bytes were {:?}",
        row0,
        String::from_utf8_lossy(&bytes)
    );
}

//! `spyc-jar`, stage 2 of the jar spike (`docs/drafts/PANE_JAR_SPIKE.md`): a
//! process that holds one pane's program and its PTY, relays bytes to the spyc
//! that owns it, and outlives that spyc. It never parses terminal output.
//!
//! - [`proto`]: the frames on the jar's socket.
//! - [`jar`]: the jar itself — the program, the event log, the owner's lease.
//! - [`client`]: the owner's end, for spyc and the tests.
//! - [`attach`]: stage 2b's attach client, a raw relay to a real terminal.
//! - [`pty`]: the program's PTY, opened and spawned without `unsafe`.
//! - [`handover`] and [`upgrade`]: stage 3, the jar replacing its own binary
//!   (R33).

#![forbid(unsafe_code)]

pub mod attach;
pub mod client;
pub mod handover;
pub mod jar;
pub mod proto;
pub mod pty;
pub mod upgrade;

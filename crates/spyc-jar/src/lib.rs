//! `spyc-jar`: the process that holds one spyc pane's program and its PTY,
//! relays its bytes to the spyc that owns it, and outlives that spyc. It never
//! parses terminal output. The design is `docs/drafts/JAR_PROTOCOL.md`, whose
//! version 1 is frozen.
//!
//! - [`proto`]: the frames on the jar's socket, and the messages they carry.
//! - [`state`]: what a jar is, apart from its threads and descriptors.
//! - [`jar`]: the jar itself: the program, the event log, the owner's lease.
//! - [`pty`]: the program's PTY, opened and spawned without `unsafe`.
//! - [`handover`] and [`upgrade`]: the jar replacing its own binary across
//!   `exec` (R33), and the blob that carries it.
//! - [`client`]: the owner's end of the socket, for spyc and the tests.
//! - [`attach`]: the attach client, a raw relay to a real terminal.
//! - [`cli`]: the `spyc-jar` command line.
//!
//! `tests/wire_v1.rs` pins version 1's bytes; the `jar_frame` and
//! `jar_handover` fuzz targets hammer both parsers.

#![forbid(unsafe_code)]

pub mod attach;
pub mod cli;
pub mod client;
pub mod handover;
pub mod jar;
pub mod proto;
pub mod pty;
pub mod state;
pub mod upgrade;

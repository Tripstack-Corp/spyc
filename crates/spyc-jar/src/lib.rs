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
//!   `exec` (R33), and the blob that carries it; [`store`]: the one file an
//!   upgrade runs; `held`: a new image that couldn't resume, holding the
//!   program it was handed.
//! - [`lock`]: the lock a jar holds beside its socket for its life, which
//!   tells a busy jar from a gone one.
//! - [`client`]: the owner's end of the socket, for spyc and the tests.
//! - [`attach`]: the attach client, a raw relay to a real terminal.
//! - [`cli`]: the `spyc-jar` command line.
//!
//! `tests/wire_v1.rs` pins version 1's bytes; the `jar_frame` and
//! `jar_handover` fuzz targets hammer both parsers.

#![forbid(unsafe_code)]

/// This crate's version, as `describe` gives it and spyc names a store entry.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod attach;
pub mod cli;
pub mod client;
pub mod handover;
mod held;
pub mod jar;
pub mod lock;
pub mod proto;
pub mod pty;
pub mod state;
pub mod store;
pub mod upgrade;

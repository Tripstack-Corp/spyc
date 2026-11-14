//! `spyc-jar`: the process that holds one spyc pane's program and its PTY,
//! relays its bytes to the spyc that owns it, and outlives that spyc. It never
//! parses terminal output. The design is `docs/drafts/JAR_PROTOCOL.md`, whose
//! version 1 is frozen.
//!
//! - [`proto`]: the frames on the jar's socket, and the messages they carry.
//! - [`state`]: what a jar is, apart from its threads and descriptors.
//! - [`handover`]: that state as the blob a jar hands the binary replacing it
//!   across `exec` (R33).
//!
//! `tests/wire_v1.rs` pins version 1's bytes; the `jar_frame` and
//! `jar_handover` fuzz targets hammer both parsers.

#![forbid(unsafe_code)]

pub mod handover;
pub mod proto;
pub mod state;

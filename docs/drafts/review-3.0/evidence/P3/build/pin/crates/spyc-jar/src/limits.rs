//! The time limits a jar keeps that its clients rely on.
//!
//! A client that waits on a jar for less than the jar's longest silence takes
//! a busy jar for a gone one, and one heard from less often than the jar's
//! deadline loses its lease (`JAR_PROTOCOL.md` → "A jar's time limits"). spyc
//! checks its own waits against these when it compiles.

use std::time::Duration;

use crate::upgrade::{
    FIRST_VALIDATION, FLUSH_DEADLINE, HOLDER_ANSWER, OUTPUT_GRACE, PARK_DEADLINE, PARKED_VALIDATION,
};

/// The longest a jar reads nothing from its clients and answers none of them.
///
/// Parked for an upgrade: from reaching the park through the flush, the
/// parked validation of the new binary and its output, to the holder's
/// answer. A client's frames queue meanwhile and are read after.
pub const PARKED_AT_MOST: Duration = PARK_DEADLINE
    .saturating_add(FLUSH_DEADLINE)
    .saturating_add(PARKED_VALIDATION)
    .saturating_add(OUTPUT_GRACE)
    .saturating_add(HOLDER_ANSWER);

/// The longest a jar takes to answer an upgrade with `upgraded` or `refused`.
///
/// The first validation of the new binary and its output, while the program
/// is still served, then the parked path. A requester that waits less reports
/// a failed upgrade that then succeeds.
pub const UPGRADE_AT_MOST: Duration = FIRST_VALIDATION
    .saturating_add(OUTPUT_GRACE)
    .saturating_add(PARKED_AT_MOST);

/// The least a client's computed wait may be: a socket takes a zero timeout
/// as none, which waits for ever.
pub const SHORTEST_WAIT: Duration = Duration::from_millis(1);

/// How long a jar goes without hearing from its owner before it ends the
/// owner's lease.
///
/// Unless its start says otherwise (`--deadline-ms`): an owner with nothing
/// to send pings more often.
pub const OWNER_DEADLINE: Duration = Duration::from_secs(10);

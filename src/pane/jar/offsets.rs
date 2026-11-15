//! Which jar offset the bytes a pane has parsed reach (finding 8).
//!
//! The pane counts the bytes it parses; the jar numbers the bytes it logged.
//! The two drift apart at a gap, where the jar dropped bytes the pane never
//! saw, and at the resynchronizing byte the pane is handed before what follows
//! a gap, which the jar never sent. A snapshot is stored at the jar offset of
//! the last byte it reflects, or a later replay from it would apply bytes it
//! already holds.

/// Runs of bytes handed to the pane: from the `handed`-th byte on, they are
/// the jar's bytes from `at`, until the next run.
#[derive(Debug)]
pub struct Offsets {
    runs: Vec<Run>,
    handed: u64,
    expected: u64,
}

#[derive(Debug, Clone, Copy)]
struct Run {
    handed: u64,
    at: u64,
}

impl Offsets {
    /// The pane's first byte will be the jar's byte `from`.
    pub fn new(from: u64) -> Self {
        Self {
            runs: vec![Run {
                handed: 0,
                at: from,
            }],
            handed: 0,
            expected: from,
        }
    }

    /// The pane's first byte will be the jar's byte `from`, which follows a
    /// cut: the pane holds none of what came before it.
    pub fn after_cut(from: u64) -> Self {
        Self {
            expected: u64::MAX,
            ..Self::new(from)
        }
    }

    /// The jar's bytes `offset..offset + len` are handed over next. Returns
    /// whether they don't follow the last ones, in which case one
    /// resynchronizing byte is handed over first.
    pub fn hand(&mut self, offset: u64, len: u64) -> bool {
        let jumped = offset != self.expected;
        if jumped {
            self.handed += 1;
            self.runs.push(Run {
                handed: self.handed,
                at: offset,
            });
        }
        self.handed += len;
        self.expected = offset + len;
        jumped
    }

    /// The jar offset just past the last of the first `parsed` bytes handed
    /// over. `parsed` only grows, so the runs before it are dropped.
    pub fn at(&mut self, parsed: u64) -> u64 {
        let last = self
            .runs
            .iter()
            .rposition(|r| r.handed <= parsed)
            .unwrap_or(0);
        self.runs.drain(..last);
        let run = self.runs[0];
        run.at + parsed.saturating_sub(run.handed)
    }
}

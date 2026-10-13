//! A Codex rollout's subscription, on the shared [`crate::agent::tail::watch`]
//! worker: an explicit consumer owns it, and nothing runs at idle.

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
use notify::EventKind;

use crate::agent::tail::watch::{self as tail, Refresh};
#[cfg(test)]
use crate::agent::tail::watch::{Mailbox, publish, relevant};

use super::{Availability, Binding, Snapshot, Update, stream::Reader};

pub struct Subscription(tail::Subscription<Update>);

impl Subscription {
    /// Start from a side-effect executor or external consumer. The worker sets up
    /// all reads, then calls `wake` only when the bounded mailbox becomes ready.
    pub fn start(
        binding: Binding,
        pinned_rollout: PathBuf,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let reader = Reader::new(binding, pinned_rollout.clone());
        Self(tail::Subscription::start(pinned_rollout, reader, wake))
    }

    pub fn take(&self) -> Option<Update> {
        self.0.take()
    }

    pub fn work(&self) -> (u64, u64) {
        self.0.work()
    }
}

impl Refresh for Reader {
    type Update = Update;

    fn refresh(&mut self) -> Update {
        Self::refresh(self)
    }

    fn watch_failed(&self) -> Update {
        Update {
            binding: self.binding().clone(),
            availability: Availability::WatchFailed,
            tail_only: false,
            snapshot: Snapshot::default(),
        }
    }

    fn work(&self) -> (u64, u64) {
        (self.bytes_read, self.refreshes)
    }
}

#[cfg(test)]
mod tests;

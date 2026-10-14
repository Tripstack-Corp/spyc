//! An agy transcript's subscription, on the shared
//! [`crate::agent::tail::watch`] worker: an explicit consumer owns it, and
//! nothing runs at idle.

use std::path::PathBuf;

use crate::agent::tail::watch::{self as tail, Refresh};

use super::{Availability, Binding, Snapshot, Update, stream::Reader};

pub struct Subscription(tail::Subscription<Update>);

impl Subscription {
    /// Start from a side-effect executor or external consumer. The worker sets up
    /// all reads, then calls `wake` only when the bounded mailbox becomes ready.
    pub fn start(binding: Binding, transcript: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        let reader = Reader::new(binding, transcript.clone());
        Self(tail::Subscription::start(transcript, reader, wake))
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

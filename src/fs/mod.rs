//! Filesystem model and operations.

pub mod entry;
pub mod finder;
pub mod grep;
pub mod listing;
pub mod ops;

pub use entry::{Entry, EntryKind, target_is_dir};
pub use listing::Listing;

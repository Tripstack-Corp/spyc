//! Key sequence resolution.
//!
//! Handles counts (e.g. `5j`), motion/verb keys, vi-style marks (`m{a-z}` /
//! `'{a-z}`), search (`/`, `n`), and user-defined maps from `.spycrc` (parsed
//! by the `config::dsl` module). Operators (e.g. `d{motion}`) are not modelled.

pub mod action;
pub mod resolver;
pub mod user;

pub use action::Action;
#[cfg(test)]
pub use action::Tier;
pub use resolver::{ChordEntry, Resolver, ResolverOutcome};
pub use user::{BoundAction, UserKeymap};

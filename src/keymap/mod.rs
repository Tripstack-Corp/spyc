//! Key sequence resolution.
//!
//! Designed to grow: today it handles counts (e.g. `5j`) and a small set of
//! motion/verb keys. Future milestones will add operators, marks (`m{a-z}` /
//! `'{a-z}`), search (`/`, `n`, `N`), and user-defined maps from `.cspyrc`.

pub mod action;
pub mod resolver;

pub use action::Action;
pub use resolver::{Resolver, ResolverOutcome};

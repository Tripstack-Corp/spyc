//! Git integration facade.
//!
//! The single boundary between spyc and any git backend. Today every
//! function here shells out to the `git` binary — relocated verbatim from
//! `sysinfo.rs` / `app/util.rs` / `app/git_state.rs` so there is exactly
//! one place that owns `Command::new("git")`. The gitoxide migration then
//! swaps each backend *in place* behind this seam, one domain at a time,
//! so the call sites never change again.
//!
//! The facade is pure infrastructure: it takes paths and returns owned
//! data / bytes. It has no `App` dependency and never touches ratatui, so
//! `app` depends on `git` and never the reverse (the CLAUDE.md one-way
//! dependency rule).

pub mod diff;
pub mod status;
pub mod worktree;

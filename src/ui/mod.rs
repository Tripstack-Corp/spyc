//! Rendering. Layout decisions live in `layout`; individual widgets in
//! `list_view`, `status`, and `prompt`. Shared colors in `theme`.

pub mod help;
#[allow(
    dead_code,
    clippy::unnested_or_patterns,
    clippy::missing_const_for_fn,
    clippy::match_same_arms
)]
pub mod line_edit;
pub mod list_view;
pub mod pager;
pub mod prompt;
pub mod status;
pub mod theme;

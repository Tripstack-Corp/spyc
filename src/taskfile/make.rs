//! A Makefile's targets, read from its text and never evaluated.

/// One target a Makefile names explicitly, which `make <name>` builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakeTarget {
    pub name: String,
    /// What the Makefile says the target does: a trailing `## text` on the
    /// rule's line, else the first comment line directly above the rule.
    pub label: Option<String>,
    /// Declared `.PHONY`.
    pub phony: bool,
    /// The one plain `make` builds.
    pub default: bool,
}

/// The explicit targets `text` declares, `.PHONY` ones first, each group in
/// source order.
#[allow(clippy::missing_const_for_fn, reason = "a stub")]
pub fn targets(_text: &str) -> Vec<MakeTarget> {
    Vec::new()
}

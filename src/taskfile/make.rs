//! A Makefile's targets, read from its text and never evaluated.
//!
//! `make -p` would print the rules make sees, but it evaluates `$(shell …)` as
//! it reads, and a Makefile is someone else's text. `makefile-lossless` parses
//! it instead, every variant's syntax, conditionals, `define` and line
//! continuations included, and nothing here expands a reference: a name that
//! holds one is left out, as is anything else a user can't type as a goal.
//!
//! [`scan`] reads one file; [`walk`] puts files together in the order make
//! reads them, an `include` spliced in where it stands, so the default goal
//! and the source order hold across files. [`targets`] is the two over one
//! text that follows no `include`.

use makefile_lossless::{DefaultGoal, Makefile, MakefileItem, MakefileVariant, Rule, TextRange};

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
pub fn targets(text: &str) -> Vec<MakeTarget> {
    let mut found = Assembly::default();
    walk(scan(text), &mut found, &mut |_| None);
    found.finish()
}

/// What one file says, its `include`s not yet followed.
#[derive(Debug, Default)]
pub struct Scan {
    /// The listable targets of its rules and its `include`s, in source order.
    items: Vec<(usize, Item)>,
    /// What `.PHONY` names.
    phony: Vec<String>,
    goal: Option<Goal>,
}

#[derive(Debug)]
enum Item {
    Target {
        name: String,
        label: Option<String>,
    },
    /// An `include`'s file names, as written.
    Include(Vec<String>),
}

/// The default goal a file sets.
#[derive(Debug)]
enum Goal {
    /// `.DEFAULT_GOAL`'s value, which wins wherever it is set.
    Named(String),
    /// The first target of its first rule that can be the goal, at an offset.
    First(String, usize),
}

/// Read one file's rules, `include`s and default goal.
pub fn scan(text: &str) -> Scan {
    let makefile = Makefile::parse(text).tree();
    let comments: Vec<TextRange> = makefile.comment_ranges().collect();
    let mut items = Vec::new();
    for rule in makefile.rules() {
        let item = MakefileItem::Rule(rule.clone());
        let at = offset(item.text_range());
        if rule.scoped_assignment().is_some() || rule.static_pattern().is_some() {
            continue;
        }
        let label = trailing_label(text, at, &comments).or_else(|| doc_label(&item));
        items.extend(rule.targets().filter(|n| listable(n)).map(|name| {
            let label = label.clone();
            (at, Item::Target { name, label })
        }));
    }
    for include in makefile.includes() {
        let at = offset(MakefileItem::Include(include.clone()).text_range());
        items.push((at, Item::Include(include.paths().collect())));
    }
    // Stable, so one rule's targets keep their order.
    items.sort_by_key(|(at, _)| *at);
    Scan {
        items,
        phony: makefile.phony_targets().collect(),
        goal: first_goal(&makefile),
    }
}

/// The earliest way `makefile` may set the default goal. Conditionals can give
/// it several; nothing is evaluated to choose, so the first is taken.
fn first_goal(makefile: &Makefile) -> Option<Goal> {
    let candidate = makefile
        .default_goal_candidates(MakefileVariant::GNUMake)
        .into_iter()
        .next()?;
    match candidate {
        DefaultGoal::FirstTarget { rule, target } => Some(Goal::First(target, rule_offset(rule))),
        DefaultGoal::Variable { value, .. } => value
            .split_whitespace()
            .next()
            .map(|n| Goal::Named(n.into())),
        _ => None,
    }
}

fn rule_offset(rule: Rule) -> usize {
    offset(MakefileItem::Rule(rule).text_range())
}

fn offset(range: TextRange) -> usize {
    u32::from(range.start()) as usize
}

/// A name a user can type as a goal: no pattern, no reference, no `.`-led
/// special target, no archive member, nothing make would read as an option or
/// a variable assignment, and nothing a shell word can't carry.
fn listable(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['.', '-'])
        && !name.contains(['%', '$', '(', ')', '*', '?', '[', '=', '\\'])
        && !name.chars().any(char::is_control)
}

/// The `## text` that ends the logical line a rule starts on at `at`.
fn trailing_label(text: &str, at: usize, comments: &[TextRange]) -> Option<String> {
    let end = line_end(text, at);
    comments
        .iter()
        .map(|r| (offset(*r), u32::from(r.end()) as usize))
        .find(|(start, _)| (at..end).contains(start))
        .and_then(|(start, end)| text.get(start..end))
        .and_then(|comment| comment.strip_prefix("##"))
        .map(|label| label.trim_start_matches('#').trim().to_string())
        .filter(|label| !label.is_empty())
}

/// The end of the line starting at `at`, past any backslash continuations.
fn line_end(text: &str, at: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = at;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let before = text[at..i].trim_end_matches('\r');
            if !before.ends_with('\\') {
                return i;
            }
        }
        i += 1;
    }
    bytes.len()
}

/// The first non-empty line of the comment directly above a rule.
fn doc_label(item: &MakefileItem) -> Option<String> {
    item.doc_comments()
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .find(|line| !line.is_empty())
}

/// The targets of every file read so far, in reading order, and what decides
/// their order and marks.
#[derive(Debug, Default)]
pub struct Assembly {
    /// Each name once, with the first label any of its rules gave.
    listed: Vec<(String, Option<String>)>,
    phony: Vec<String>,
    /// `.DEFAULT_GOAL`, the first file's that sets it.
    named: Option<String>,
    /// The first target read that can be the goal.
    first: Option<String>,
}

impl Assembly {
    fn add(&mut self, name: String, label: Option<String>) {
        match self.listed.iter_mut().find(|(n, _)| *n == name) {
            Some((_, have)) => {
                if have.is_none() {
                    *have = label;
                }
            }
            None => self.listed.push((name, label)),
        }
    }

    /// The listed targets, `.PHONY` first, the default goal marked.
    pub fn finish(self) -> Vec<MakeTarget> {
        let default = self.named.or(self.first);
        let (phony, rest): (Vec<_>, Vec<_>) = self
            .listed
            .into_iter()
            .map(|(name, label)| MakeTarget {
                phony: self.phony.contains(&name),
                default: default.as_ref() == Some(&name),
                name,
                label,
            })
            .partition(|t| t.phony);
        phony.into_iter().chain(rest).collect()
    }
}

/// Add `scan`'s targets to `found` in reading order. `follow` reads the file an
/// `include` names, or declines it; each one read is walked in its place.
pub fn walk(scan: Scan, found: &mut Assembly, follow: &mut dyn FnMut(&str) -> Option<Scan>) {
    let Scan { items, phony, goal } = scan;
    found.phony.extend(phony);
    let (mut first, mut first_at) = (None, usize::MAX);
    match goal {
        Some(Goal::Named(name)) => {
            found.named.get_or_insert(name);
        }
        Some(Goal::First(name, at)) => (first, first_at) = (Some(name), at),
        None => {}
    }
    for (at, item) in items {
        if at >= first_at
            && let Some(name) = first.take()
        {
            found.first.get_or_insert(name);
        }
        match item {
            Item::Target { name, label } => found.add(name, label),
            Item::Include(paths) => {
                for path in paths {
                    if let Some(inner) = follow(&path) {
                        walk(inner, found, follow);
                    }
                }
            }
        }
    }
    if let Some(name) = first {
        found.first.get_or_insert(name);
    }
}

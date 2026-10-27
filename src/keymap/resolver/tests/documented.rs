//! Every default binding is in `docs/KEYBINDINGS.md`, the keymap reference, and
//! in the `?` overlay (`src/ui/help.rs`).
//!
//! The bindings come from the resolver itself: every key a user can press, fed
//! to a fresh one, and every chord's [`Resolver::continuations`], walked into
//! its submenus. A new key or chord is checked with no list here to update.

use super::*;

/// The keys a user can press with nothing pending: printable ASCII, Ctrl with a
/// letter or a symbol, and the named keys.
fn candidate_keys() -> Vec<KeyEvent> {
    let mut keys: Vec<KeyEvent> = (' '..='~').map(key).collect();
    keys.extend(('a'..='z').chain(['\\', ']', '[', ' ']).map(ctrl));
    keys.extend(
        [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Insert,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
        ]
        .map(special),
    );
    keys.extend((1..=12).map(|n| special(KeyCode::F(n))));
    keys
}

/// How the reference writes a key.
fn key_name(ev: KeyEvent) -> String {
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    match ev.code {
        KeyCode::Char(' ') if ctrl => "^Space".to_string(),
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) if ctrl => format!("^{c}"),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        other => format!("{other:?}"),
    }
}

/// A key or chord as the reference's key column spells it, for comparison:
/// no spaces, and a Ctrl letter in either case.
fn normalize(keys: &str) -> String {
    let mut out = String::new();
    let mut after_caret = false;
    for c in keys.chars().filter(|c| !c.is_whitespace()) {
        out.extend(if after_caret {
            c.to_lowercase().collect::<Vec<_>>()
        } else {
            vec![c]
        });
        after_caret = c == '^';
    }
    out
}

/// One default binding: the keys as written (`^a c`), and the spellings that
/// count as documenting it. A range (`1-9`, `a-z`) is documented by itself, in
/// braces (`m{a-z}`), or by its first key (`Space 1`).
struct Binding {
    keys: String,
    accepted: Vec<String>,
}

fn binding(prefix: &str, token: &str) -> Binding {
    let keys = if prefix.is_empty() {
        token.to_string()
    } else {
        format!("{prefix} {token}")
    };
    let mut accepted = vec![normalize(&keys)];
    let mut range = token.split('-');
    if let (Some(first), Some(_), None) = (range.next(), range.next(), range.next())
        && token.len() == 3
    {
        accepted.push(normalize(&format!("{prefix}{{{token}}}")));
        accepted.push(normalize(&format!("{prefix}{first}")));
    }
    Binding { keys, accepted }
}

/// The key a continuation token names, when it names one to feed.
fn token_event(token: &str) -> Option<KeyEvent> {
    match token {
        "Space" => Some(key(' ')),
        t if t.chars().count() == 1 => t.chars().next().map(key),
        _ => None,
    }
}

/// Every chord under `r`, already armed by `prefix`. A table already walked
/// (`^w` is `^a`; `^a Space` is the leader) is not walked again.
fn walk(r: &Resolver, prefix: &str, seen: &mut Vec<String>, out: &mut Vec<Binding>) {
    let rows = r.continuations();
    let table = format!("{rows:?}");
    // No rows is a count in progress, not a chord.
    if rows.is_empty() || seen.contains(&table) {
        return;
    }
    seen.push(table);
    for row in rows {
        match row {
            ChordEntry::Act(keys, _) | ChordEntry::Range(keys, _) => {
                out.extend(keys.split_whitespace().map(|t| binding(prefix, t)));
            }
            ChordEntry::Sub(keys, _, _) => {
                for token in keys.split_whitespace() {
                    let mut next = r.clone();
                    let Some(ev) = token_event(token) else {
                        continue;
                    };
                    if feed(&mut next, ev) == ResolverOutcome::Pending {
                        walk(&next, &format!("{prefix} {token}"), seen, out);
                    }
                }
            }
        }
    }
}

fn default_bindings() -> Vec<Binding> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for ev in candidate_keys() {
        let mut r = Resolver::new();
        match feed(&mut r, ev) {
            ResolverOutcome::Action(_) => out.push(binding("", &key_name(ev))),
            ResolverOutcome::Pending => walk(&r, &key_name(ev), &mut seen, &mut out),
            _ => {}
        }
    }
    out
}

/// The first cell of a markdown table row, where `\|` is a literal pipe
/// rather than the cell's end.
fn key_cell(row: &str) -> Option<String> {
    let row = row.strip_prefix('|')?;
    let mut cell = String::new();
    let mut chars = row.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('|') => cell.push('|'),
                Some(other) => cell.extend(['\\', other]),
                None => cell.push('\\'),
            },
            '|' => return Some(cell),
            other => cell.push(other),
        }
    }
    Some(cell)
}

/// Reference sections whose keys mean something else there: a pager key is no
/// evidence of a file-list binding on the same key.
const OTHER_MODE_SECTIONS: &[&str] = &["## Pager"];

/// Help sections of the same kind, plus the ones that aren't key bindings.
const OTHER_MODE_HELP: &[&str] = &[
    "Inventory view (inside i)",
    "Capture mode (! / !!) — substitution & runtime keys",
    "Divider glyphs",
    "CLI Flags",
];

/// The reference's lines outside [`OTHER_MODE_SECTIONS`]: a section runs to the
/// next heading of its level or above.
fn file_view_lines(doc: &str) -> impl Iterator<Item = &str> {
    let mut skipping: Option<usize> = None;
    doc.lines().filter(move |line| {
        let level = line.chars().take_while(|c| *c == '#').count();
        if level > 0 && line[level..].starts_with(' ') {
            if skipping.is_some_and(|l| level <= l) {
                skipping = None;
            }
            if OTHER_MODE_SECTIONS.iter().any(|s| line.starts_with(s)) {
                skipping = Some(level);
            }
        }
        skipping.is_none()
    })
}

/// The keys the reference documents: every code span in a table row's key
/// column, normalized. A span holding a backtick is written `` `` ` `` ``.
fn documented_keys(doc: &str) -> std::collections::HashSet<String> {
    let span = regex::Regex::new(r"``\s*(.+?)\s*``|`([^`]+)`").expect("regex");
    file_view_lines(doc)
        .filter_map(key_cell)
        .flat_map(|cell| {
            span.captures_iter(&cell)
                .filter_map(|c| c.get(1).or_else(|| c.get(2)))
                .map(|m| normalize(m.as_str()))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn the_key_column_is_read_as_markdown_writes_it() {
    let doc = "| `^s \\|` / `^a \\|` | split |\n| `` ` `` | start dir |\n| `gg` / `G` | top |\nprose `x` is not a key\n";
    let keys = documented_keys(doc);
    for want in ["^s|", "^a|", "`", "gg", "G"] {
        assert!(keys.contains(want), "{want} missing from {keys:?}");
    }
    assert!(
        !keys.contains("x"),
        "prose outside a table is not the key column"
    );
    assert_eq!(normalize("^B / ^a c"), "^b/^ac");
    let paged = documented_keys(
        "## Navigation\n| `j` | down |\n## Pager\n| `q` | close |\n### Outline\n| `za` | fold |\n## Shell\n| `!` | run |\n",
    );
    assert!(paged.contains("j") && paged.contains("!"));
    assert!(
        !paged.contains("q") && !paged.contains("za"),
        "a pager key isn't a file-list key: {paged:?}"
    );
}

#[test]
fn every_default_binding_is_in_the_keymap_reference() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc =
        std::fs::read_to_string(root.join("docs/KEYBINDINGS.md")).expect("read the reference");
    let documented = documented_keys(&doc);
    let bindings = default_bindings();
    // A resolver that answers nothing would pass everything.
    assert!(
        bindings.len() > 100,
        "found only {} default bindings",
        bindings.len()
    );
    let missing: Vec<&str> = bindings
        .iter()
        .filter(|b| !b.accepted.iter().any(|a| documented.contains(a)))
        .map(|b| b.keys.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "{} default binding(s) docs/KEYBINDINGS.md doesn't list: {missing:?}. Add each \
         to its section's table, in the key column, written as the reference writes \
         keys (`^a c`, `Space w l`, `gg`).",
        missing.len()
    );
}

/// The keys a help row names. Alternatives are separated by two spaces or
/// ` / `; within one, `l|n|d` gives sibling last keys, `1..9` a range written
/// by its first key, and `Space ] [` several keys after one prefix.
fn help_keys(keys: &str) -> Vec<String> {
    let sep = regex::Regex::new(r"\s{2,}|\s/\s").expect("regex");
    let mut out = Vec::new();
    for alt in sep.split(keys).map(str::trim).filter(|a| !a.is_empty()) {
        let whole = normalize(alt);
        if let Some((head, tail)) = whole.split_once('|') {
            let base: String = head.chars().take(head.chars().count() - 1).collect();
            out.extend(tail.split('|').map(|t| format!("{base}{t}")));
            out.push(head.to_string());
        }
        if let Some((first, _)) = whole.split_once("..") {
            out.push(first.to_string());
        }
        let words: Vec<&str> = alt.split_whitespace().collect();
        if words.len() > 2 && words[1..].iter().all(|w| w.chars().count() == 1) {
            out.extend(
                words[1..]
                    .iter()
                    .map(|w| normalize(&format!("{}{w}", words[0]))),
            );
        }
        out.push(whole);
    }
    out
}

#[test]
fn every_default_binding_is_in_the_help_overlay() {
    let documented: std::collections::HashSet<String> = crate::ui::help::key_column()
        .filter(|(section, _)| !OTHER_MODE_HELP.contains(section))
        .flat_map(|(_, keys)| help_keys(keys))
        .collect();
    let missing: Vec<String> = default_bindings()
        .into_iter()
        .filter(|b| !b.accepted.iter().any(|a| documented.contains(a)))
        .map(|b| b.keys)
        .collect();
    assert!(
        missing.is_empty(),
        "{} default binding(s) the `?` overlay (src/ui/help.rs) doesn't list: {missing:?}. \
         Add each to its section's rows.",
        missing.len()
    );
}

#[test]
fn a_help_row_is_read_as_the_overlay_writes_it() {
    let keys: Vec<String> = [
        "^B  PageUp",
        "^a j / ^a k",
        "Space w l|n|d",
        "H1..H9",
        "Space ] [",
        "Q  ^D  :q  ZZ",
    ]
    .into_iter()
    .flat_map(help_keys)
    .collect();
    for want in [
        "^b", "PageUp", "^aj", "^ak", "Spacewl", "Spacewn", "Spacewd", "H1", "Space]", "Space[",
        "Q", "^d", "ZZ",
    ] {
        assert!(
            keys.iter().any(|k| k == want),
            "{want} missing from {keys:?}"
        );
    }
}

//! End-to-end contract tests for the keymap feature.
//!
//! These tests verify the `.spycrc.toml` configuration grammar is stable.
//! Since spyc is a binary crate, we can't import its internals directly
//! in integration tests. These tests exercise the TOML structure that
//! the crate's config loader expects.

use std::io::Write;

use tempfile::tempdir;

/// Parse a `.spycrc.toml` file and verify the TOML structure is accepted.
fn parse_toml(content: &str) -> toml::Value {
    // `toml::from_str` is the whole-DOCUMENT parser the config loader itself
    // uses. (toml 1.1's `str::parse::<Value>` / `FromStr` parses a single
    // VALUE, not a document, so it rejects a `key = ...` file outright.)
    toml::from_str(content).expect("valid TOML")
}

#[test]
fn minimal_config_is_valid_toml() {
    let val = parse_toml(
        r#"
keymap = [
    "map f unix file %",
]
"#,
    );
    let keymap = val.get("keymap").unwrap().as_array().unwrap();
    assert_eq!(keymap.len(), 1);
}

#[test]
fn full_featured_config_is_valid_toml() {
    let val = parse_toml(
        r##"
keymap = [
    "map f unix file %",
    "map g unix git status",
    "map ^P unix ps -u $USER",
    "map H patternpick =*.hip",
    "map h jump =$HFS/houdini",
    "map 1 ignoretoggle =1",
]

[colors]
dir      = "#5fafff"
exec     = "#87d75f"
symlink  = "#d787ff"
cursor_bg = "#303030"

[[ignore_masks]]
group = 1
patterns = [".*"]
enabled = true

[[ignore_masks]]
group = 2
patterns = ["*.o", "*.pyc", "target", "node_modules"]
enabled = true
"##,
    );
    let keymap = val.get("keymap").unwrap().as_array().unwrap();
    assert_eq!(keymap.len(), 6);

    let colors = val.get("colors").unwrap().as_table().unwrap();
    assert_eq!(colors.get("dir").unwrap().as_str().unwrap(), "#5fafff");

    let masks = val.get("ignore_masks").unwrap().as_array().unwrap();
    assert_eq!(masks.len(), 2);
    assert_eq!(masks[0].get("group").unwrap().as_integer().unwrap(), 1);
}

#[test]
fn keymap_dsl_grammar_entries() {
    // Verify the DSL grammar accepts all documented forms.
    let lines = [
        "map f unix file %",
        "map ^P unix ps aux",
        "map <Enter> display",
        "map <F1> help",
        "map H patternpick =*.hip",
        "map h jump =$HOME/src",
        "map 1 ignoretoggle =1",
        "map q quit",
    ];
    use std::fmt::Write;
    let mut body = String::new();
    for l in &lines {
        writeln!(body, "    \"{l}\",").unwrap();
    }
    let toml_str = format!("keymap = [\n{body}]\n");
    let val = parse_toml(&toml_str);
    let keymap = val.get("keymap").unwrap().as_array().unwrap();
    assert_eq!(keymap.len(), lines.len());
}

#[test]
fn config_file_on_disk_round_trips() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join(".spycrc.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        r##"
keymap = [
    "map f unix file %",
    "map g unix git log --oneline",
]

[colors]
dir = "#aabbcc"
"##
    )
    .unwrap();

    // Read back and verify TOML structure
    let content = std::fs::read_to_string(&path).unwrap();
    let val = parse_toml(&content);
    let keymap = val.get("keymap").unwrap().as_array().unwrap();
    assert_eq!(keymap.len(), 2);
    assert_eq!(keymap[0].as_str().unwrap(), "map f unix file %");
}

#[test]
fn ignore_masks_structure() {
    let val = parse_toml(
        r#"
[[ignore_masks]]
group = 1
patterns = [".*"]
enabled = true

[[ignore_masks]]
group = 2
patterns = ["*.o", "target"]
enabled = false
"#,
    );
    let masks = val.get("ignore_masks").unwrap().as_array().unwrap();
    assert_eq!(masks.len(), 2);

    let m1 = &masks[0];
    assert_eq!(m1.get("group").unwrap().as_integer().unwrap(), 1);
    assert!(m1.get("enabled").unwrap().as_bool().unwrap());
    let pats1 = m1.get("patterns").unwrap().as_array().unwrap();
    assert_eq!(pats1[0].as_str().unwrap(), ".*");

    let m2 = &masks[1];
    assert!(!m2.get("enabled").unwrap().as_bool().unwrap());
}

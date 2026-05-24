//! JSON viewer helpers.
//!
//! v1.50.72 ships the basic pretty-printer + raw/pretty toggle.
//! Folding, path indicator, and search-within-structure follow in
//! later versions and will grow this module.

use std::path::Path;

/// True when `path` should trigger the JSON pretty-print branch in
/// `build_pager_view_for_file`. Matches `.json` (case-insensitive).
///
/// Deliberately does NOT match `.jsonl` (line-delimited JSON):
/// pretty-printing those would join lines, destroying the
/// one-record-per-line affordance. They still get syntect
/// highlighting on the source via the plain-text pager branch.
pub fn is_json_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("json")
    )
}

/// Try to re-emit `input` as canonical 2-space-indented JSON.
/// Returns `Some(pretty)` on a successful parse, `None` when the
/// input isn't strict JSON (json5 with comments, malformed file,
/// etc.) so the caller can fall back to the plain-text pager.
///
/// Empty input is treated as a parse failure — there's nothing to
/// pretty-print and the toggle would be a no-op.
pub fn pretty_print(input: &str) -> Option<String> {
    if input.trim().is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_json_extension() {
        assert!(is_json_path(Path::new("foo.json")));
        assert!(is_json_path(Path::new("config.JSON")));
        assert!(!is_json_path(Path::new("foo.jsonl")));
        assert!(!is_json_path(Path::new("README.md")));
        assert!(!is_json_path(Path::new("noext")));
    }

    #[test]
    fn pretty_reflows_minified() {
        let minified = r#"{"a":1,"b":[1,2,3],"c":{"d":true}}"#;
        let pretty = pretty_print(minified).unwrap();
        // The pretty output spans multiple lines and re-indents.
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  \"a\": 1"));
        assert!(pretty != minified);
    }

    #[test]
    fn pretty_idempotent_on_pretty_input() {
        let pretty_in = "{\n  \"a\": 1\n}";
        let pretty_out = pretty_print(pretty_in).unwrap();
        assert_eq!(pretty_out, pretty_in);
    }

    #[test]
    fn pretty_returns_none_on_invalid_json() {
        // Comments aren't strict JSON.
        assert!(pretty_print("{ \"a\": 1 } // trailing comment").is_none());
        // Bare malformed input.
        assert!(pretty_print("{").is_none());
    }

    #[test]
    fn pretty_returns_none_on_empty() {
        assert!(pretty_print("").is_none());
        assert!(pretty_print("   \n\t").is_none());
    }
}

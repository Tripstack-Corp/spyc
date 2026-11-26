//! No build tells a release from the rc before it (the 3.0 review's S6):
//! an rc soaks the code its release ships only if nothing decides what a
//! build does by its version. `pane::jar::honoured()` did, and a build
//! versioned 3.0.0 would have run every tab outside a jar.
//!
//! So every read of spyc's version is named here, by its file, the item it
//! sits in and its text, with why; each prints or names the version whole.
//! Any other read fails, however it would test the version (a `'-'`, a
//! `"-beta."`, a `cfg` a build script sets from `CARGO_PKG_VERSION_PRE`),
//! and so does a listed line that moves to another item or is gone. Build
//! scripts are read too. What counts as a read: a name in `VERSION_NAMES`
//! (the `CARGO_PKG_` variables, and each constant or function that returns
//! the version); spyc's `VERSION` by any path, a `use` of it included;
//! `env!` or `option_env!` of anything but a plain string, so a name built
//! with `concat!` can't hide; a build script's `env::var` of anything but a
//! plain string; and a manifest (`Cargo.toml`, `Cargo.lock`,
//! `CARGO_MANIFEST_DIR`), which holds the version too. What it can't see: the
//! version reached some other way (a file it doesn't name, a process's
//! output), or a listed reader's result tested by its caller.

use std::path::Path;

/// Names that read spyc's version, or a variable of its manifest.
const VERSION_NAMES: &[&str] = &[
    "CARGO_PKG",
    "PACKAGE_VERSION",
    "crate::VERSION",
    "spyc::VERSION",
    "SERVER_VERSION",
    "XTVERSION",
    "embedded_version",
];

/// Every production line that reads the version: its file, the item it sits
/// in (`""` before the file's first), its text, and why it may.
const READERS: &[(&str, &str, &str, &str)] = &[
    (
        "src/lib.rs",
        "VERSION",
        r#"pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SPYC_GIT_SHA"), ")");"#,
        "spyc's version and commit, for display",
    ),
    (
        "src/lib.rs",
        "VERSION",
        "version = VERSION,",
        "clap's `--version` text",
    ),
    (
        "src/lib.rs",
        "run",
        r#"println!("\u{1f336}\u{fe0f} spyc {}", env!("CARGO_PKG_VERSION"));"#,
        "`spyc --version`",
    ),
    (
        "src/lib.rs",
        "install_skill",
        "skill::embedded_version(),",
        "`--install-skill` prints the version it installed",
    ),
    (
        "src/app/activity_diagnostics.rs",
        "activity_dump_lines",
        "crate::VERSION,",
        "`:activity dump` names the build",
    ),
    (
        "src/app/commands.rs",
        "why_git_lines",
        "crate::VERSION,",
        "`:why-git` names the build",
    ),
    (
        "src/app/commands.rs",
        "agent_registry_lines",
        "crate::VERSION,",
        "the agent registry report names the build",
    ),
    (
        "src/app/commands.rs",
        "agent_list_lines",
        "crate::VERSION,",
        "the agent list names the build",
    ),
    (
        "src/app/events.rs",
        "started_note",
        "crate::VERSION,",
        "`:events` names the build",
    ),
    (
        "src/app/mcp.rs",
        "snapshot_context",
        "version: crate::VERSION.to_string(),",
        "the context an agent reads",
    ),
    (
        "src/app/render/activity_hud.rs",
        "render_activity_hud",
        "crate::VERSION,",
        "drawn in the activity overlay",
    ),
    (
        "src/app/session.rs",
        "show_session_info",
        r#"lines.push(format!("\u{1f336}\u{fe0f} spyc {}", crate::VERSION));"#,
        "the session info names the build",
    ),
    (
        "src/app/state/apply.rs",
        "apply",
        r#"self.flash_info(format!("\u{1f336}\u{fe0f} spyc {}", crate::VERSION));"#,
        "the version flash",
    ),
    (
        "src/app/state/dispatch.rs",
        "dispatch_command",
        r#"self.flash_info(format!("\u{1f336}\u{fe0f} spyc {}", crate::VERSION));"#,
        "`:version`",
    ),
    (
        "src/app/skill.rs",
        "maybe_offer_skill_update",
        "let prompt = offer_text(&who, installed, skill::embedded_version(), overwrites_edits);",
        "the skill offer names both versions; staleness is a content hash",
    ),
    (
        "src/app/skill.rs",
        "cmd_skill",
        "let embedded = skill::embedded_version();",
        "`:skill status` prints it",
    ),
    (
        "src/app/skill.rs",
        "cmd_skill",
        "skill::embedded_version(),",
        "`:skill update` prints it",
    ),
    (
        "src/crash_report.rs",
        "capture",
        "version: crate::VERSION,",
        "a crash report names the build",
    ),
    (
        "src/crash_report.rs",
        "url_with",
        r#"env!("CARGO_PKG_REPOSITORY"),"#,
        "the bug-report link: the repository, not the version",
    ),
    (
        "src/mcp/mod.rs",
        "SERVER_VERSION",
        "const SERVER_VERSION: &str = crate::VERSION;",
        "the MCP server's `serverInfo`",
    ),
    (
        "src/mcp/protocol.rs",
        "",
        "CONTEXT_URI, PROTOCOL_VERSION, SERVER_INSTRUCTIONS, SERVER_NAME, SERVER_VERSION, mcp_log,",
        "an import",
    ),
    (
        "src/mcp/protocol.rs",
        "handle_initialize",
        r#""version": SERVER_VERSION"#,
        "sent to an MCP client",
    ),
    (
        "src/mcp/server.rs",
        "inert_reply",
        r#""serverInfo": { "name": super::SERVER_NAME, "version": super::SERVER_VERSION },"#,
        "sent to an MCP client",
    ),
    (
        "src/pane/engine_ghostty/replies.rs",
        "XTVERSION",
        r#"const XTVERSION: &str = concat!("spyc ", env!("CARGO_PKG_VERSION"));"#,
        "the answer to a program's XTVERSION query",
    ),
    (
        "src/pane/engine_ghostty/replies.rs",
        "xtversion",
        "ptr: XTVERSION.as_ptr(),",
        "handed to the terminal",
    ),
    (
        "src/pane/engine_ghostty/replies.rs",
        "xtversion",
        "len: XTVERSION.len(),",
        "handed to the terminal",
    ),
    (
        "src/pane/jar/snapshots.rs",
        "build",
        r#"env!("CARGO_PKG_VERSION"),"#,
        "a binary snapshot's build stamp, compared whole",
    ),
    (
        "src/pane/jar/store.rs",
        "entry_name",
        "spyc_jar::PACKAGE_VERSION,",
        "a store entry's name, compared whole",
    ),
    (
        "src/skill/mod.rs",
        "install_in",
        r#""version": env!("CARGO_PKG_VERSION"),"#,
        "written into the installed skill's manifest",
    ),
    (
        "src/skill/mod.rs",
        "embedded_version",
        "pub const fn embedded_version() -> &'static str {",
        "the skill's version, listed by name above",
    ),
    (
        "src/skill/mod.rs",
        "embedded_version",
        r#"env!("CARGO_PKG_VERSION")"#,
        "its body",
    ),
    (
        "crates/spyc-jar/src/lib.rs",
        "PACKAGE_VERSION",
        r#"pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");"#,
        "the jar's version",
    ),
    (
        "crates/spyc-jar/src/jar/describe.rs",
        "describe",
        "crate::PACKAGE_VERSION,",
        "a description's `jar`, printed",
    ),
    (
        "crates/spyc-jar/src/held.rs",
        "describe",
        r#"format!("{} {}", crate::PACKAGE_VERSION, image.display()),"#,
        "a held jar's description, printed",
    ),
    (
        "crates/spyc-vt-sys/build.rs",
        "main",
        r#"let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));"#,
        "finds the vendored archives beside its manifest",
    ),
];

/// The item a line opens, if it opens one: a `fn`, `const` or `static`,
/// past its visibility and qualifiers.
fn item_opened(code: &str) -> Option<&str> {
    let mut words = code
        .split(|c: char| c.is_whitespace() || c == '(' || c == '<' || c == ':' || c == '=')
        .filter(|w| !w.is_empty())
        .peekable();
    while let Some(w) = words.next() {
        match w {
            "pub" | "async" | "unsafe" | "extern" | "crate)" | "super)" | "default" => {}
            _ if w.starts_with('"') => {}
            "const" if words.peek() == Some(&"fn") => {}
            "const"
                if words
                    .peek()
                    .is_some_and(|n| ["unsafe", "async", "extern"].contains(n)) => {}
            "fn" | "const" | "static" => {
                return words.next().filter(|n| {
                    n.chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                });
            }
            _ => return None,
        }
    }
    None
}

/// Whether `code` calls `call` with anything but a string literal first.
fn computed_argument(code: &str, call: &str) -> bool {
    code.match_indices(call)
        .any(|(at, _)| !code[at + call.len()..].trim_start().starts_with('"'))
}

/// Whether `code` names spyc's `VERSION` constant by any path: `crate::`,
/// `super::super::`, a `use` of it, or bare in `src/lib.rs`. The jar's wire
/// version, `proto::VERSION`, is another constant.
fn names_version(code: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    code.match_indices("VERSION").any(|(at, _)| {
        let before = &code[..at];
        let after = &code[at + "VERSION".len()..];
        !before.chars().next_back().is_some_and(ident)
            && !after.chars().next().is_some_and(ident)
            && !before.ends_with("proto::")
    })
}

/// Whether a production line reads the version, by the rules above.
fn reads_the_version(code: &str, build_script: bool, spyc_crate: bool) -> bool {
    let named = VERSION_NAMES.iter().any(|name| {
        code.match_indices(name).any(|(at, _)| {
            !code[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    });
    named
        || (spyc_crate && names_version(code))
        || ["Cargo.toml", "Cargo.lock", "CARGO_MANIFEST_DIR"]
            .iter()
            .any(|m| code.contains(m))
        || computed_argument(code, "env!(")
        || (build_script
            && (computed_argument(code, "env::var(") || computed_argument(code, "env::var_os(")))
}

/// Every file a `#[cfg(test)] mod name;` declares: test code, whatever its
/// name.
fn test_only_modules(root: &Path, files: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        let path = Path::new(f);
        let dir = match path.file_name().and_then(|n| n.to_str()) {
            Some("mod.rs" | "lib.rs" | "main.rs") => path.parent().map(Path::to_path_buf),
            _ => path
                .parent()
                .map(|p| p.join(path.file_stem().unwrap_or_default())),
        };
        let Some(dir) = dir else { continue };
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        while let Some(line) = lines.next() {
            if line != "#[cfg(test)]" {
                continue;
            }
            let Some(name) = lines
                .next()
                .and_then(|l| {
                    l.strip_prefix("mod ")
                        .or_else(|| l.strip_prefix("pub mod "))
                })
                .and_then(|l| l.strip_suffix(';'))
            else {
                continue;
            };
            for candidate in [
                dir.join(format!("{name}.rs")),
                dir.join(name).join("mod.rs"),
            ] {
                let candidate = candidate.to_string_lossy().replace('\\', "/");
                if files.contains(&candidate) {
                    found.push(candidate);
                }
            }
        }
    }
    found
}

#[test]
#[ignore = "red until S6 (fix/jars-on-release) removes pane::jar::honoured()"]
fn no_build_tells_a_release_from_its_rc() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let in_crate = |f: &str, sub: &str| {
        f.strip_prefix("crates/")
            .is_some_and(|r| r.split_once('/').is_some_and(|(_, p)| p.starts_with(sub)))
    };
    let scanned = |f: &str| {
        if f == file!() {
            return false;
        }
        let build_script = f == "build.rs" || in_crate(f, "build.rs");
        let stem = Path::new(f)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let test_only = stem == "tests"
            || stem.ends_with("_tests")
            || stem.starts_with("test_")
            || f.contains("/tests/")
            || f.contains("harness_tests/");
        (f.starts_with("src/") || in_crate(f, "src/") || build_script) && !test_only
    };
    let all = crate::guard_support::repo_files(root, &["rs"]);
    let test_modules = test_only_modules(root, &all);
    assert!(
        test_modules.iter().any(|m| m == "src/style_guard.rs"),
        "premise: a module declared only for tests is known as one"
    );
    let files: Vec<String> = all
        .into_iter()
        .filter(|f| scanned(f) && !test_modules.contains(f))
        .collect();
    for premise in ["build.rs", "src/lib.rs", "crates/spyc-jar/src/lib.rs"] {
        assert!(
            files.iter().any(|f| f == premise),
            "premise: the scan reads {premise}"
        );
    }
    let mut found = Vec::new();
    for f in &files {
        let build_script = f.ends_with("build.rs");
        // Only spyc's own crate can name spyc's `VERSION`.
        let spyc_crate = !f.starts_with("crates/");
        let text = std::fs::read_to_string(root.join(f)).unwrap();
        let mut item = String::new();
        for line in crate::guard_support::production_half(&text).lines() {
            let code = line.trim();
            if code.starts_with("//") || code.starts_with("#[doc") {
                continue;
            }
            if let Some(opened) = item_opened(code) {
                item = opened.to_string();
            }
            if reads_the_version(code, build_script, spyc_crate) {
                found.push((f.clone(), item.clone(), code.to_string()));
            }
        }
    }
    let mut listed: Vec<(String, String, String)> = READERS
        .iter()
        .map(|(f, i, l, _)| ((*f).to_string(), (*i).to_string(), (*l).to_string()))
        .collect();
    found.sort();
    listed.sort();
    let unlisted: Vec<_> = found.iter().filter(|r| !listed.contains(r)).collect();
    let gone: Vec<_> = listed.iter().filter(|r| !found.contains(r)).collect();
    assert!(
        unlisted.is_empty(),
        "production code reads spyc's version where nothing says why, so a release \
         could run differently from its rc: {unlisted:#?}\nnot found where listed: {gone:#?}"
    );
    assert_eq!(
        found, listed,
        "a listed reader of the version is gone, moved or doubled: {gone:#?}"
    );
}

#[test]
fn the_guard_knows_an_item_and_a_computed_name() {
    assert_eq!(
        item_opened(r#"const unsafe extern "C" fn xtversion("#),
        Some("xtversion")
    );
    assert_eq!(
        item_opened("pub(crate) fn rc_build() -> bool {"),
        Some("rc_build")
    );
    assert_eq!(
        item_opened("pub const fn embedded_version() -> &'static str {"),
        Some("embedded_version")
    );
    assert_eq!(item_opened("pub const VERSION: &str = x;"), Some("VERSION"));
    assert_eq!(item_opened("let v = f(x);"), None);
    assert!(reads_the_version(
        r#"env!(concat!("CARGO_", "PKG_VERSION")).contains('-')"#,
        false,
        true
    ));
    assert!(reads_the_version(
        r#"include_str!("../../../Cargo.toml").contains("-rc.")"#,
        false,
        true
    ));
    assert!(reads_the_version(
        "let v = std::env::var(name);",
        true,
        true
    ));
    assert!(!reads_the_version(
        "let v = std::env::var(name);",
        false,
        true
    ));
    assert!(!reads_the_version(r#"env!("SPYC_GIT_SHA")"#, false, true));
    assert!(reads_the_version(
        "super::super::VERSION.contains('-')",
        false,
        true
    ));
    assert!(reads_the_version("use crate::{VERSION};", false, true));
    assert!(reads_the_version("VERSION.contains(\"-rc.\")", false, true));
    assert!(!reads_the_version("spyc_jar::proto::VERSION", false, true));
    assert!(!reads_the_version("MANIFEST_VERSION", false, true));
    assert!(!reads_the_version(
        "put_u16(&mut w, VERSION);",
        false,
        false
    ));
}

//! `.spycrc.toml` loader and runtime configuration.
//!
//! Two files are consulted in order:
//!   1. `$HOME/.spycrc.toml` — per-user defaults.
//!   2. `<cwd>/.spycrc.toml` — per-project overrides (win).
//!
//! Both are optional. Anything missing falls back to built-in defaults.
//!
//! See `dsl` for the `map KEY action [args]` line grammar.

pub mod dsl;

/// Embedded default config template — emitted by `spyc --print-config`.
/// Every option commented out at its default value, with a one-liner
/// explaining what it does. Round-trip parsed in tests so the dump
/// always loads cleanly with the current `Config` schema.
pub const DEFAULT_TEMPLATE: &str = include_str!("default.spycrc.toml");

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::keymap::user::UserBinding;

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Keymap bindings parsed from the `keymap = [...]` array.
    pub bindings: Vec<UserBinding>,

    /// Color palette overrides.
    pub colors: ColorOverrides,

    /// Layout overrides (status bar position, etc.).
    pub layout: LayoutConfig,

    /// Pane / pty defaults.
    pub pane: PaneConfig,

    /// Yank / clipboard behavior knobs.
    pub yank: YankConfig,

    /// Markdown viewer behavior knobs.
    pub markdown: MarkdownConfig,

    /// Delete-action behavior knobs.
    pub delete: DeleteConfig,

    /// Ignore-mask definitions. When non-empty, they replace the
    /// built-in defaults wholesale.
    pub ignore_masks: Vec<IgnoreMask>,

    /// User-defined Quick Select patterns appended to the built-in
    /// set (URL, path, SHA, IPv4). Bad regexes are dropped at load
    /// time with a warning rather than failing the whole config.
    pub scan_patterns: Vec<crate::pane::quick_select::CustomPattern>,

    /// File paths we actually loaded from (for the watcher to track).
    pub sources: Vec<PathBuf>,
}

/// Where the status bar lives. Defaults to `Top` (matches stock spyc).
/// `Bottom` is convenient when running inside tmux/screen — the host
/// status line typically owns the top row, so spyc's bar moving to the
/// bottom (vim/tmux convention) prevents a double-bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusPosition {
    #[default]
    Top,
    Bottom,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutConfig {
    /// `"top"` (default) or `"bottom"`. With `"bottom"` the prompt
    /// sits one row above the status bar (vim-style cmdline-above-
    /// statusline ordering).
    pub status_position: StatusPosition,
}

/// On-disk shape of `[layout]`. Each field is `Option` so we can tell
/// "user didn't set this" apart from "user set this to the default" —
/// otherwise a project file with no `[layout]` would clobber a value
/// from the user file with the Deserialize default.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileLayout {
    #[serde(default)]
    status_position: Option<StatusPosition>,
}

/// Pane / pty defaults. Currently just the default command for `^a c`.
#[derive(Debug, Clone, Default)]
pub struct PaneConfig {
    /// Default command pre-filled into the `^a c` (new tab) prompt.
    /// Falls back to `"claude"` when both this and `$SPYC_PANE_CMD`
    /// are unset, preserving long-standing behavior. The env var
    /// still wins so users can override per-shell on the fly.
    pub default_command: Option<String>,
}

/// On-disk shape of `[pane]`. `Option` for the same "didn't set"
/// distinguishability as `[layout]`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePane {
    #[serde(default)]
    default_command: Option<String>,
}

/// Yank / clipboard knobs.
#[derive(Debug, Clone)]
pub struct YankConfig {
    /// When true, pager yanks (`y` / `Y` / visual-mode `y`) prepend a
    /// short header that identifies the source — the pager's title
    /// (e.g. `!cargo build`, `task #3: cargo test`, or the filename).
    /// Pasting the captured output elsewhere keeps the "what was
    /// running" context with the content. Default true.
    pub include_pager_title: bool,
}

impl Default for YankConfig {
    fn default() -> Self {
        Self {
            include_pager_title: true,
        }
    }
}

/// On-disk shape of `[yank]`. `Option` for the same "didn't set"
/// distinguishability as the other tables.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileYank {
    #[serde(default)]
    include_pager_title: Option<bool>,
}

/// Markdown viewer knobs.
#[derive(Debug, Clone)]
pub struct MarkdownConfig {
    /// When true (default), opening a `.md` / `.markdown` file via the
    /// pager lands in the *rendered* view; `m` toggles to source.
    /// When false, the pager opens in source view and `m` toggles to
    /// rendered. Per-side scroll memory works the same either way.
    pub open_as_rendered: bool,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            open_as_rendered: true,
        }
    }
}

/// `[delete]` — confirmation behavior for `R` / `dd` removal.
#[derive(Debug, Clone)]
pub struct DeleteConfig {
    /// When true (default), `R` and `dd` show a `y/N` confirmation
    /// prompt before moving anything to the graveyard. Setting to
    /// false enables "yolo mode" — deletions fire immediately on
    /// `dd` / `R`, no prompt, no warning highlight. The graveyard
    /// is still the destination either way, so `gy` can recover.
    pub confirm: bool,
}

impl Default for DeleteConfig {
    fn default() -> Self {
        Self { confirm: true }
    }
}

/// On-disk shape of `[delete]`. `Option` for "didn't set" disambig
/// — letting the user write `[delete]` with no body still keeps
/// defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDelete {
    confirm: Option<bool>,
}

/// On-disk shape of `[markdown]`. `Option` for "didn't set" disambig.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileMarkdown {
    #[serde(default)]
    open_as_rendered: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorOverrides {
    pub dir: Option<String>,
    pub exec: Option<String>,
    pub symlink: Option<String>,
    pub file: Option<String>,
    pub other: Option<String>,
    pub cursor_bg: Option<String>,
    pub cursor_fg: Option<String>,
    pub pick: Option<String>,
    pub take: Option<String>,
    pub status_user: Option<String>,
    pub status_path: Option<String>,
    pub status_suffix: Option<String>,
    pub prompt_prefix: Option<String>,
    pub delete_warning: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // wired in the next task
pub struct IgnoreMask {
    pub group: u8,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

/// On-disk shape of a single `.spycrc.toml`. We parse each file into one
/// of these, then merge them into the final `Config`.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[serde(default)]
    relax_search: bool,
    #[serde(default)]
    relax_prompt: bool,
    #[serde(default)]
    keymap: Vec<String>,
    #[serde(default)]
    colors: ColorOverrides,
    #[serde(default)]
    layout: FileLayout,
    #[serde(default)]
    pane: FilePane,
    #[serde(default)]
    yank: FileYank,
    #[serde(default)]
    markdown: FileMarkdown,
    #[serde(default)]
    delete: FileDelete,
    #[serde(default)]
    ignore_masks: Vec<IgnoreMask>,
    #[serde(default)]
    scan: ScanConfig,
}

/// On-disk shape of `[scan]`. Holds Quick Select pattern
/// definitions; bad regexes are reported and dropped at load
/// rather than failing the whole config.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanConfig {
    #[serde(default)]
    patterns: Vec<ScanPatternFile>,
}

/// On-disk shape of one `[[scan.patterns]]` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanPatternFile {
    name: String,
    regex: String,
    #[serde(default)]
    url: Option<String>,
}

impl Config {
    /// Load and merge the standard config file locations. Missing files
    /// are silently skipped; broken TOML / DSL returns an `Err`.
    pub fn load_default(cwd: &Path) -> anyhow::Result<Self> {
        let user = home_dir().map(|h| h.join(".spycrc.toml"));
        let project = cwd.join(".spycrc.toml");
        Self::load_from(&[user.as_deref(), Some(&project)])
    }

    /// Load from an explicit list of candidate paths. Later paths override
    /// earlier ones for settings; keymap bindings and ignore masks are
    /// **appended** in order so both user and project can contribute.
    pub fn load_from(paths: &[Option<&Path>]) -> anyhow::Result<Self> {
        let mut cfg = Self::default();
        for path in paths.iter().flatten() {
            if !path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
            let file: FileConfig = toml::from_str(&text)
                .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
            cfg.merge_file(file, path)?;
        }
        Ok(cfg)
    }

    fn merge_file(&mut self, file: FileConfig, source: &Path) -> anyhow::Result<()> {
        self.sources.push(source.to_path_buf());

        // Settings: later wins. (Currently just bools — no-op placeholders
        // until we wire them in.)
        let _ = file.relax_search;
        let _ = file.relax_prompt;

        // Colors: any Some() overrides the accumulated value.
        merge_color(&mut self.colors.dir, file.colors.dir);
        merge_color(&mut self.colors.exec, file.colors.exec);
        merge_color(&mut self.colors.symlink, file.colors.symlink);
        merge_color(&mut self.colors.file, file.colors.file);
        merge_color(&mut self.colors.other, file.colors.other);
        merge_color(&mut self.colors.cursor_bg, file.colors.cursor_bg);
        merge_color(&mut self.colors.cursor_fg, file.colors.cursor_fg);
        merge_color(&mut self.colors.pick, file.colors.pick);
        merge_color(&mut self.colors.take, file.colors.take);
        merge_color(&mut self.colors.status_user, file.colors.status_user);
        merge_color(&mut self.colors.status_path, file.colors.status_path);
        merge_color(&mut self.colors.status_suffix, file.colors.status_suffix);
        merge_color(&mut self.colors.prompt_prefix, file.colors.prompt_prefix);

        // Layout: per-field merge — only overwrite when the file
        // explicitly set the value (Some). Otherwise a project file
        // with no `[layout]` would clobber a value the user file set.
        if let Some(pos) = file.layout.status_position {
            self.layout.status_position = pos;
        }

        // Pane: per-field merge for the same reason.
        if let Some(cmd) = file.pane.default_command {
            self.pane.default_command = Some(cmd);
        }

        // Yank: per-field merge.
        if let Some(b) = file.yank.include_pager_title {
            self.yank.include_pager_title = b;
        }

        // Markdown: per-field merge.
        if let Some(b) = file.markdown.open_as_rendered {
            self.markdown.open_as_rendered = b;
        }

        // Delete: per-field merge.
        if let Some(b) = file.delete.confirm {
            self.delete.confirm = b;
        }

        // Ignore masks: append.
        self.ignore_masks.extend(file.ignore_masks);

        // Scan patterns: append. A bad regex is logged and skipped
        // rather than failing the whole config — one user-typed
        // typo shouldn't lock them out of starting spyc.
        for p in file.scan.patterns {
            match regex::Regex::new(&p.regex) {
                Ok(re) => self
                    .scan_patterns
                    .push(crate::pane::quick_select::CustomPattern {
                        name: p.name,
                        regex: re,
                        url_template: p.url,
                    }),
                Err(e) => {
                    crate::spyc_debug!(
                        "{}: scan pattern {:?}: bad regex — {e}",
                        source.display(),
                        p.name
                    );
                }
            }
        }

        // Keymap: parse each line, append.
        for (i, line) in file.keymap.iter().enumerate() {
            let parsed = dsl::parse(line)
                .map_err(|e| anyhow::anyhow!("{}: keymap[{i}]: {e}", source.display()))?;
            if let Some(binding) = parsed {
                self.bindings.push(binding);
            }
        }
        Ok(())
    }
}

fn merge_color(dst: &mut Option<String>, src: Option<String>) {
    if src.is_some() {
        *dst = src;
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn default_template_round_trips() {
        // The dump emitted by `spyc --print-config` must always parse
        // cleanly with the current schema — every option is commented
        // out so the parsed FileConfig equals FileConfig::default().
        let file: FileConfig = toml::from_str(super::DEFAULT_TEMPLATE).unwrap();
        assert!(file.keymap.is_empty());
        assert!(file.colors.dir.is_none());
        assert!(file.ignore_masks.is_empty());
        assert!(file.layout.status_position.is_none());
        assert!(file.pane.default_command.is_none());
        assert!(file.yank.include_pager_title.is_none());
        assert!(file.markdown.open_as_rendered.is_none());
    }

    #[test]
    fn markdown_open_as_rendered_defaults_to_true() {
        assert!(Config::default().markdown.open_as_rendered);
    }

    #[test]
    fn parses_markdown_open_as_rendered_false() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("rc.toml");
        std::fs::write(&path, "[markdown]\nopen_as_rendered = false\n").unwrap();
        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert!(!cfg.markdown.open_as_rendered);
    }

    #[test]
    fn project_without_markdown_does_not_clobber_user_markdown() {
        let tmp = tempdir().unwrap();
        let user = tmp.path().join("user.toml");
        let project = tmp.path().join("project.toml");
        std::fs::write(&user, "[markdown]\nopen_as_rendered = false\n").unwrap();
        std::fs::write(&project, "[colors]\ndir = \"#abcdef\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&user), Some(&project)]).unwrap();
        assert!(!cfg.markdown.open_as_rendered);
    }

    #[test]
    fn yank_include_pager_title_defaults_to_true() {
        assert!(Config::default().yank.include_pager_title);
    }

    #[test]
    fn parses_yank_include_pager_title_false() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("rc.toml");
        std::fs::write(&path, "[yank]\ninclude_pager_title = false\n").unwrap();
        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert!(!cfg.yank.include_pager_title);
    }

    #[test]
    fn project_without_yank_does_not_clobber_user_yank() {
        let tmp = tempdir().unwrap();
        let user = tmp.path().join("user.toml");
        let project = tmp.path().join("project.toml");
        std::fs::write(&user, "[yank]\ninclude_pager_title = false\n").unwrap();
        std::fs::write(&project, "[colors]\ndir = \"#abcdef\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&user), Some(&project)]).unwrap();
        assert!(!cfg.yank.include_pager_title);
    }

    #[test]
    fn parses_pane_default_command() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("rc.toml");
        std::fs::write(&path, "[pane]\ndefault_command = \"codex\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert_eq!(cfg.pane.default_command.as_deref(), Some("codex"));
    }

    #[test]
    fn project_without_pane_does_not_clobber_user_pane() {
        let tmp = tempdir().unwrap();
        let user = tmp.path().join("user.toml");
        let project = tmp.path().join("project.toml");
        std::fs::write(&user, "[pane]\ndefault_command = \"codex\"\n").unwrap();
        // Project file has no [pane] — must not reset to default.
        std::fs::write(&project, "[colors]\ndir = \"#abcdef\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&user), Some(&project)]).unwrap();
        assert_eq!(cfg.pane.default_command.as_deref(), Some("codex"));
    }

    #[test]
    fn project_pane_overrides_user_pane() {
        let tmp = tempdir().unwrap();
        let user = tmp.path().join("user.toml");
        let project = tmp.path().join("project.toml");
        std::fs::write(&user, "[pane]\ndefault_command = \"codex\"\n").unwrap();
        std::fs::write(&project, "[pane]\ndefault_command = \"bash --login\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&user), Some(&project)]).unwrap();
        assert_eq!(cfg.pane.default_command.as_deref(), Some("bash --login"));
    }

    #[test]
    fn project_without_layout_does_not_clobber_user_layout() {
        let tmp = tempdir().unwrap();
        let user = tmp.path().join("user.toml");
        let project = tmp.path().join("project.toml");
        std::fs::write(&user, "[layout]\nstatus_position = \"bottom\"\n").unwrap();
        // Project file has no [layout] — must not reset to default.
        std::fs::write(&project, "[colors]\ndir = \"#abcdef\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&user), Some(&project)]).unwrap();
        assert_eq!(cfg.layout.status_position, StatusPosition::Bottom);
        assert_eq!(cfg.colors.dir.as_deref(), Some("#abcdef"));
    }

    #[test]
    fn parses_bottom_status_position() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("rc.toml");
        std::fs::write(&path, "[layout]\nstatus_position = \"bottom\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert_eq!(cfg.layout.status_position, StatusPosition::Bottom);
    }

    #[test]
    fn loads_empty_config_when_no_files() {
        let tmp = tempdir().unwrap();
        let cfg = Config::load_from(&[Some(&tmp.path().join("nope.toml"))]).unwrap();
        assert!(cfg.bindings.is_empty());
        assert!(cfg.sources.is_empty());
    }

    #[test]
    fn parses_keymap_and_colors() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("spycrc.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        // Top-level scalar/array fields must come before any [table] or
        // they get parsed as members of the preceding table.
        writeln!(
            f,
            r##"
keymap = [
    "map f unix file %",
    "map H patternpick =*.hip",
]

[colors]
dir  = "#aabbcc"
exec = "#112233"
"##
        )
        .unwrap();

        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert_eq!(cfg.colors.dir.as_deref(), Some("#aabbcc"));
        assert_eq!(cfg.colors.exec.as_deref(), Some("#112233"));
        assert_eq!(cfg.bindings.len(), 2);
    }

    // ── DSL → Resolver → Action round-trip tests ──────────────────

    #[test]
    fn dsl_unix_binding_round_trips_through_resolver() {
        use crate::keymap::resolver::{Resolver, ResolverOutcome};
        use crate::keymap::user::{BoundAction, UserKeymap};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let binding = dsl::parse("map f unix file %").unwrap().unwrap();
        let keymap = UserKeymap::from_bindings(vec![binding]);
        let mut resolver = Resolver::new();

        let ev = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        let out = resolver.feed(ev, &keymap);
        assert_eq!(
            out,
            ResolverOutcome::User(BoundAction::UnixCmd("file %".to_string()))
        );
    }

    #[test]
    fn dsl_ctrl_binding_round_trips() {
        use crate::keymap::resolver::{Resolver, ResolverOutcome};
        use crate::keymap::user::{BoundAction, UserKeymap};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let binding = dsl::parse("map ^P unix ps aux").unwrap().unwrap();
        let keymap = UserKeymap::from_bindings(vec![binding]);
        let mut resolver = Resolver::new();

        let ev = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        let out = resolver.feed(ev, &keymap);
        assert_eq!(
            out,
            ResolverOutcome::User(BoundAction::UnixCmd("ps aux".to_string()))
        );
    }

    #[test]
    fn dsl_jump_binding_round_trips() {
        use crate::keymap::resolver::{Resolver, ResolverOutcome};
        use crate::keymap::user::{BoundAction, UserKeymap};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let binding = dsl::parse("map X jump =/usr/local").unwrap().unwrap();
        let keymap = UserKeymap::from_bindings(vec![binding]);
        let mut resolver = Resolver::new();

        let ev = KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE);
        let out = resolver.feed(ev, &keymap);
        assert_eq!(
            out,
            ResolverOutcome::User(BoundAction::Jump("/usr/local".to_string()))
        );
    }

    #[test]
    fn dsl_plain_action_round_trips() {
        use crate::keymap::action::Action;
        use crate::keymap::resolver::{Resolver, ResolverOutcome};
        use crate::keymap::user::{BoundAction, UserKeymap};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let binding = dsl::parse("map X quit").unwrap().unwrap();
        let keymap = UserKeymap::from_bindings(vec![binding]);
        let mut resolver = Resolver::new();

        let ev = KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE);
        let out = resolver.feed(ev, &keymap);
        assert_eq!(out, ResolverOutcome::User(BoundAction::Plain(Action::Quit)));
    }

    #[test]
    fn full_config_file_round_trip() {
        use crate::keymap::resolver::{Resolver, ResolverOutcome};
        use crate::keymap::user::{BoundAction, UserKeymap};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let tmp = tempdir().unwrap();
        let path = tmp.path().join("test.toml");
        std::fs::write(
            &path,
            r#"
keymap = [
    "map f unix file %",
    "map g unix git status",
]
"#,
        )
        .unwrap();

        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        let keymap = UserKeymap::from_bindings(cfg.bindings);
        let mut resolver = Resolver::new();

        // 'f' should fire the first binding
        let out = resolver.feed(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
            &keymap,
        );
        assert_eq!(
            out,
            ResolverOutcome::User(BoundAction::UnixCmd("file %".to_string()))
        );

        // 'g' should fire the second (overrides built-in 'g' pending)
        let out = resolver.feed(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            &keymap,
        );
        assert_eq!(
            out,
            ResolverOutcome::User(BoundAction::UnixCmd("git status".to_string()))
        );
    }

    #[test]
    fn project_overrides_user_colors_but_appends_keymap() {
        let tmp = tempdir().unwrap();
        let user = tmp.path().join("user.toml");
        let proj = tmp.path().join("proj.toml");
        std::fs::write(
            &user,
            r##"
keymap = ["map f unix file %"]

[colors]
dir = "#111111"
"##,
        )
        .unwrap();
        std::fs::write(
            &proj,
            r##"
keymap = ["map g unix git status"]

[colors]
dir = "#222222"
"##,
        )
        .unwrap();
        let cfg = Config::load_from(&[Some(&user), Some(&proj)]).unwrap();
        assert_eq!(cfg.colors.dir.as_deref(), Some("#222222"));
        assert_eq!(cfg.bindings.len(), 2);
    }
}

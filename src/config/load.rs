//! Reading the config files: which ones, at what trust, and how each one
//! merges into the accumulated [`Config`].

use std::path::Path;

use super::{Config, FileConfig, FilePaneTab, PaneTabConfig, ProjectTabs, dsl, home_dir};

/// Trust level of a config file. A `Project` (`<cwd>/.spycrc.toml`) file is
/// attacker-controllable, so its executing keymap bindings are dropped;
/// `Trusted` (`$HOME/.spycrc.toml`, or explicit caller-supplied paths) is
/// honoured in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Trust {
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
    /// arbitrary `jump`. Those are dropped from the project file, and its
    /// startup tabs, which spawn at launch, are held in `pane.project_tabs`
    /// until the user approves them. Cosmetic/behavioural settings
    /// (`[colors]`, `[layout]`, …) and plain rebindings are still honoured.
    pub fn load_default(cwd: &Path) -> anyhow::Result<Self> {
        let user = home_dir().map(|h| h.join(".spycrc.toml"));
        Self::load_layered(user.as_deref(), &cwd.join(".spycrc.toml"))
    }

    /// `load_default`'s trust assignment, over explicit paths, so tests
    /// exercise the production layering without reading the real `$HOME`.
    pub(super) fn load_layered(user: Option<&Path>, project: &Path) -> anyhow::Result<Self> {
        let mut cfg = Self::default();
        if let Some(u) = user {
            cfg.load_one(u, Trust::Trusted)?;
        }
        cfg.load_one(project, Trust::Project)?;
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
        if let Some(b) = file.pane.preview_pasted_images {
            self.pane.preview_pasted_images = b;
        }
        if let Some(b) = file.pane.codex_mcp {
            self.pane.codex_mcp = b;
        }
        if let Some(b) = file.pane.codex_daemon {
            self.pane.codex_daemon = b;
        }
        // Startup tabs: the two forms are mutually exclusive within one
        // file — merging them would hide an ambiguity, so surface it as a
        // hard error at load (PANE_STARTUP_TABS_PLAN.md). Whichever form
        // is present REPLACES any earlier file's tab list wholesale,
        // matching the other pane fields.
        //
        // They spawn their commands at launch, so an untrusted rc's list is
        // held apart in `project_tabs` and runs only once the user approves
        // it (`app::startup_tabs`). A malformed one is warned, not errored:
        // an error would drop the trusted $HOME config too.
        let declared = startup_tabs_from(file.pane.tabs, file.pane.tab, source);
        match trust {
            Trust::Trusted => {
                if let Some(tabs) = declared? {
                    self.pane.tabs = tabs;
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

        // Yank: per-field merge.
        if let Some(b) = file.yank.include_pager_title {
            self.yank.include_pager_title = b;
        }

        // Pager: per-field merge. Clamp tab_width to >= 1 so a 0 in the
        // config can't make tabs render as zero-width (invisible).
        if let Some(w) = file.pager.tab_width {
            self.pager.tab_width = w.max(1);
        }

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

        // Clipboard: per-field merge.
        if let Some(v) = file.clipboard.via {
            self.clipboard.via = v;
        }
        if let Some(c) = file.clipboard.command {
            self.clipboard.command = Some(c);
        }
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

        // Prompt templates are text typed at an agent, so a project rc may not
        // define one: it would put the repo's words behind a key the user bound.
        if trust == Trust::Trusted {
            self.prompts.extend(file.prompts);
        } else if !file.prompts.is_empty() {
            self.warnings.push(format!(
                "{}: [prompts] ignored — only ~/.spycrc.toml may define prompt templates",
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
            })
            .collect(),
        (None, Some(entries)) => entries
            .into_iter()
            .map(|e| PaneTabConfig {
                command: e.command,
                cwd: e.cwd,
                label: e.label,
            })
            .collect(),
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

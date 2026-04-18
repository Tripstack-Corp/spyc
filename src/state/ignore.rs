use glob::Pattern;

/// A single ignore mask: a list of glob patterns plus an on/off switch.
#[derive(Debug, Clone)]
pub struct Mask {
    pub patterns: Vec<Pattern>,
    pub enabled: bool,
}

impl Mask {
    pub fn new(patterns: &[&str], enabled: bool) -> Self {
        let patterns = patterns
            .iter()
            .filter_map(|p| Pattern::new(p).ok())
            .collect();
        Self { patterns, enabled }
    }

    pub fn matches(&self, name: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(name))
    }
}

/// Two independent ignore masks, like spy's `ignoremask[1]` / `ignoremask[2]`.
/// Mask 1 defaults to "dotfiles"; Mask 2 defaults to "build artifacts".
#[derive(Debug, Clone)]
pub struct IgnoreMasks {
    pub mask1: Mask,
    pub mask2: Mask,
}

impl Default for IgnoreMasks {
    fn default() -> Self {
        Self {
            // Dotfiles — on by default so the listing is not cluttered.
            mask1: Mask::new(&[".*"], true),
            // Build/VCS artifacts — on by default for the same reason. Flip
            // off with `o` when you need to see them.
            mask2: Mask::new(
                &[
                    "*.o",
                    "*.pyc",
                    "target",
                    "node_modules",
                    ".git",
                    "CVS",
                    "RCS",
                    "Makedepend",
                    "tags",
                ],
                true,
            ),
        }
    }
}

impl IgnoreMasks {
    pub fn hides(&self, name: &str) -> bool {
        (self.mask1.enabled && self.mask1.matches(name))
            || (self.mask2.enabled && self.mask2.matches(name))
    }

    pub const fn toggle_mask1(&mut self) {
        self.mask1.enabled = !self.mask1.enabled;
    }

    pub const fn toggle_mask2(&mut self) {
        self.mask2.enabled = !self.mask2.enabled;
    }

    /// Replace a group's built-in patterns with the `[[ignore_masks]]`
    /// entries from `.spycrc.toml`. When multiple entries target the same
    /// group, their patterns are unioned and `enabled = any(enabled)`.
    /// Groups without any config entry keep their built-in defaults.
    pub fn apply_config(&mut self, configs: &[crate::config::IgnoreMask]) {
        let group1: Vec<&crate::config::IgnoreMask> =
            configs.iter().filter(|m| m.group == 1).collect();
        let group2: Vec<&crate::config::IgnoreMask> =
            configs.iter().filter(|m| m.group == 2).collect();

        if !group1.is_empty() {
            let pats: Vec<&str> = group1
                .iter()
                .flat_map(|m| m.patterns.iter().map(String::as_str))
                .collect();
            let enabled = group1.iter().any(|m| m.enabled);
            self.mask1 = Mask::new(&pats, enabled);
        }
        if !group2.is_empty() {
            let pats: Vec<&str> = group2
                .iter()
                .flat_map(|m| m.patterns.iter().map(String::as_str))
                .collect();
            let enabled = group2.iter().any(|m| m.enabled);
            self.mask2 = Mask::new(&pats, enabled);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_matches_glob() {
        let m = Mask::new(&["*.o", "*.pyc"], true);
        assert!(m.matches("foo.o"));
        assert!(m.matches("bar.pyc"));
        assert!(!m.matches("foo.rs"));
    }

    #[test]
    fn mask_matches_dotfiles() {
        let m = Mask::new(&[".*"], true);
        assert!(m.matches(".git"));
        assert!(m.matches(".bashrc"));
        assert!(!m.matches("README"));
    }

    #[test]
    fn invalid_patterns_are_skipped() {
        // Unclosed bracket is an invalid glob
        let m = Mask::new(&["[invalid", "*.txt"], true);
        assert!(m.matches("foo.txt"));
        // The invalid pattern was silently dropped
        assert!(!m.matches("[invalid"));
    }

    #[test]
    fn default_masks_hide_dotfiles_and_artifacts() {
        let masks = IgnoreMasks::default();
        assert!(masks.hides(".git"));
        assert!(masks.hides(".bashrc"));
        assert!(masks.hides("foo.o"));
        assert!(masks.hides("node_modules"));
        assert!(!masks.hides("README.md"));
        assert!(!masks.hides("Cargo.toml"));
    }

    #[test]
    fn toggle_mask1() {
        let mut masks = IgnoreMasks::default();
        assert!(masks.mask1.enabled);
        masks.toggle_mask1();
        assert!(!masks.mask1.enabled);
        // With mask1 off, dotfiles not matched by mask2 become visible
        assert!(!masks.hides(".bashrc"));
        // .git is in mask2's list too, so it stays hidden
        assert!(masks.hides(".git"));
        // mask2 still active
        assert!(masks.hides("foo.o"));
    }

    #[test]
    fn toggle_mask2() {
        let mut masks = IgnoreMasks::default();
        assert!(masks.mask2.enabled);
        masks.toggle_mask2();
        assert!(!masks.mask2.enabled);
        // Build artifacts visible now
        assert!(!masks.hides("foo.o"));
        assert!(!masks.hides("node_modules"));
        // mask1 still active
        assert!(masks.hides(".hidden"));
    }

    #[test]
    fn hides_requires_enabled() {
        let mut masks = IgnoreMasks::default();
        masks.toggle_mask1();
        masks.toggle_mask2();
        // Both off — nothing hidden
        assert!(!masks.hides(".git"));
        assert!(!masks.hides("foo.o"));
    }

    #[test]
    fn apply_config_replaces_group() {
        let mut masks = IgnoreMasks::default();
        let configs = vec![crate::config::IgnoreMask {
            group: 1,
            patterns: vec!["*.log".to_string()],
            enabled: true,
        }];
        masks.apply_config(&configs);
        // mask1 now matches .log, not dotfiles
        assert!(masks.hides("app.log"));
        // .bashrc was only in mask1 (dotfile pattern), now gone
        assert!(!masks.hides(".bashrc"));
        // .git is still in mask2's explicit list
        assert!(masks.hides(".git"));
        // mask2 unchanged
        assert!(masks.hides("foo.o"));
    }

    #[test]
    fn apply_config_unions_same_group() {
        let mut masks = IgnoreMasks::default();
        let configs = vec![
            crate::config::IgnoreMask {
                group: 2,
                patterns: vec!["*.log".to_string()],
                enabled: false,
            },
            crate::config::IgnoreMask {
                group: 2,
                patterns: vec!["*.tmp".to_string()],
                enabled: true,
            },
        ];
        masks.apply_config(&configs);
        // enabled = any(enabled) = true
        assert!(masks.mask2.enabled);
        assert!(masks.hides("app.log"));
        assert!(masks.hides("data.tmp"));
    }

    #[test]
    fn apply_config_no_entries_keeps_defaults() {
        let mut masks = IgnoreMasks::default();
        masks.apply_config(&[]);
        // Defaults still active
        assert!(masks.hides(".git"));
        assert!(masks.hides("foo.o"));
    }
}

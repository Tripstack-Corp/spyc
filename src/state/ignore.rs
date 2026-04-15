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

    pub fn toggle_mask1(&mut self) {
        self.mask1.enabled = !self.mask1.enabled;
    }

    pub fn toggle_mask2(&mut self) {
        self.mask2.enabled = !self.mask2.enabled;
    }

    /// Replace a group's built-in patterns with the `[[ignore_masks]]`
    /// entries from `.cspyrc.toml`. When multiple entries target the same
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

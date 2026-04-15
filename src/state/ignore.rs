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
}

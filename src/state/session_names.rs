//! Spice-themed session name generator.
//!
//! Combinations like `SAFFRON_PAPRIKA` or `CARDAMOM_SUMAC` — assigned on
//! session creation so the top-line and session picker always have a
//! memorable handle. ~30² = ~900 pairings; collision-safe enough for a
//! single user's saved-session list (capped at 20).

const SPICES: &[&str] = &[
    "CUMIN",
    "SAFFRON",
    "CARDAMOM",
    "PAPRIKA",
    "TURMERIC",
    "CORIANDER",
    "GINGER",
    "CLOVE",
    "FENNEL",
    "NUTMEG",
    "ANISE",
    "CAYENNE",
    "THYME",
    "OREGANO",
    "SUMAC",
    "ZAATAR",
    "HARISSA",
    "MACE",
    "CHILI",
    "CINNAMON",
    "BASIL",
    "ROSEMARY",
    "SAGE",
    "TARRAGON",
    "JUNIPER",
    "GARAM",
    "DUKKAH",
    "WASABI",
    "FENUGREEK",
    "CHICORY",
];

/// Generate a random spice-pair session name like `SAFFRON_PAPRIKA`.
/// Two distinct spices, separated by an underscore.
#[must_use]
pub fn generate() -> String {
    let seed = seed();
    let a = (seed as usize) % SPICES.len();
    // Use a different slice of the seed so a and b aren't correlated.
    let mut b = ((seed >> 24) as usize) % SPICES.len();
    if b == a {
        b = (b + 1) % SPICES.len();
    }
    format!("{}_{}", SPICES[a], SPICES[b])
}

/// Normalize user input for `:name` — uppercase, keep `[A-Z0-9_]`,
/// replace anything else with `_`, collapse runs of underscores, and
/// trim leading/trailing underscores. Returns `None` if empty after
/// normalization.
#[must_use]
pub fn normalize(input: &str) -> Option<String> {
    let mut out = String::with_capacity(input.len());
    let mut prev_us = true;
    for ch in input.chars() {
        let c = ch.to_ascii_uppercase();
        let keep = c.is_ascii_alphanumeric();
        if keep {
            out.push(c);
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn seed() -> u64 {
    // Nanoseconds since the epoch — good enough entropy for a picker
    // label. XOR in the PID to decorrelate siblings spawned in the
    // same nanosecond.
    let ns = crate::sysinfo::epoch_nanos();
    let mixed = (ns as u64) ^ ((ns >> 64) as u64);
    mixed ^ u64::from(std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_has_two_parts_with_underscore() {
        let name = generate();
        let parts: Vec<&str> = name.split('_').collect();
        assert_eq!(parts.len(), 2, "expected FOO_BAR, got {name}");
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
        assert_ne!(parts[0], parts[1], "spices should differ");
    }

    #[test]
    fn generate_uses_known_spices() {
        let name = generate();
        for part in name.split('_') {
            assert!(SPICES.contains(&part), "unexpected spice {part} in {name}");
        }
    }

    #[test]
    fn normalize_uppercases_and_underscores() {
        assert_eq!(normalize("my project"), Some("MY_PROJECT".to_string()));
        assert_eq!(normalize("a-b!c"), Some("A_B_C".to_string()));
        assert_eq!(normalize("  trim  me  "), Some("TRIM_ME".to_string()));
    }

    #[test]
    fn normalize_empty_is_none() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("   "), None);
        assert_eq!(normalize("!!!"), None);
    }

    #[test]
    fn normalize_collapses_runs() {
        assert_eq!(normalize("a   b"), Some("A_B".to_string()));
        assert_eq!(normalize("a___b"), Some("A_B".to_string()));
    }

    #[test]
    fn normalize_keeps_digits() {
        assert_eq!(normalize("v2 feature"), Some("V2_FEATURE".to_string()));
    }
}

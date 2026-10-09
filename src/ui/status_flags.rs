//! The status bar's state suffix: the column state worth a glance, in words
//! that fit.
//!
//! A field at its resting value says nothing. No picks, an empty inventory,
//! both masks at their configured default, sort by name: printing those every
//! frame is what made the old `[picks:0 inv:0 m1:on m2:on hidden:1 sort:name]`
//! as wide as a path. So the default shows only what differs, `[hidden:1]`
//! there, and switches to short forms (`[h:1]`) only when the full words would
//! cut the path.

use serde::Deserialize;

use crate::fs::listing::SortMode;

/// How the suffix is written: `[layout] status_flags`, or `:set flags=` for
/// the rest of the run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagsMode {
    /// Only what differs from the resting state, in words, or in short forms
    /// when the words would cut the path.
    #[default]
    Auto,
    /// Only what differs from the resting state, always in short forms.
    Short,
    /// Every field on every frame, as the bar was before these modes.
    Full,
}

impl FlagsMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "short" => Some(Self::Short),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

impl std::fmt::Display for FlagsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Short => "short",
            Self::Full => "full",
        })
    }
}

/// One ignore mask: whether it's on now, and whether it's on by default.
#[derive(Debug, Clone, Copy)]
pub struct MaskFlag {
    pub on: bool,
    pub default_on: bool,
}

/// The focused column's state, as the suffix reports it.
pub struct FlagState<'a> {
    pub picks: usize,
    pub inventory: usize,
    pub masks: [MaskFlag; 2],
    /// The `=` / `:limit` filter; `"!"` is the picks-only limit.
    pub limit: Option<&'a str>,
    /// Entries the masks and the filter keep off the list.
    pub hidden: usize,
    pub sort: SortMode,
    pub sort_reversed: bool,
    /// Worded elsewhere and shown as they are: an archive mount's badge, and
    /// background tasks when there is no pane divider to carry them.
    pub archive: Option<&'a str>,
    pub bg: Option<&'a str>,
}

/// The suffix as the bar may draw it. `short` is the fallback the renderer
/// takes when `full` would cut the path; `None` when there is nothing shorter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suffix {
    pub full: String,
    pub short: Option<String>,
}

impl Suffix {
    /// A suffix with one form, for views whose suffix isn't column state.
    pub const fn fixed(text: String) -> Self {
        Self {
            full: text,
            short: None,
        }
    }
}

/// Write `state`'s suffix in `mode`.
pub fn suffix(state: &FlagState<'_>, mode: FlagsMode) -> Suffix {
    let flags = news(state);
    let words = bracket(flags.iter().map(|(full, _)| full.as_str()));
    let short = bracket(flags.iter().map(|(_, short)| short.as_str()));
    match mode {
        FlagsMode::Full => Suffix::fixed(legacy(state)),
        FlagsMode::Short => Suffix::fixed(short),
        FlagsMode::Auto => Suffix {
            short: (short != words).then_some(short),
            full: words,
        },
    }
}

/// The fields that differ from the resting state, each as `(words, short)`,
/// in the order the old bar used.
fn news(s: &FlagState<'_>) -> Vec<(String, String)> {
    let both = |text: String| (text.clone(), text);
    let mut out = Vec::new();
    if s.picks > 0 {
        out.push((format!("picks:{}", s.picks), format!("p:{}", s.picks)));
    }
    if s.inventory > 0 {
        out.push((format!("inv:{}", s.inventory), format!("i:{}", s.inventory)));
    }
    for (i, m) in s.masks.iter().enumerate() {
        if m.on != m.default_on {
            let state = if m.on { "on" } else { "off" };
            out.push(both(format!("m{}:{state}", i + 1)));
        }
    }
    match s.limit {
        Some("!") => out.push(("limit:picks".into(), "l:picks".into())),
        Some(f) => out.push((format!("limit:{f}"), format!("l:{f}"))),
        None => {}
    }
    if s.hidden > 0 {
        out.push((format!("hidden:{}", s.hidden), format!("h:{}", s.hidden)));
    }
    if s.sort != SortMode::Name || s.sort_reversed {
        let arrow = if s.sort_reversed { "\u{2191}" } else { "" };
        let code = match s.sort {
            SortMode::Name => 'n',
            SortMode::Size => 's',
            SortMode::Mtime => 't',
            SortMode::Ext => 'e',
        };
        out.push((
            format!("sort:{}{arrow}", s.sort),
            format!("s:{code}{arrow}"),
        ));
    }
    out.extend(s.archive.map(|a| both(a.to_string())));
    out.extend(s.bg.map(|b| both(b.to_string())));
    out
}

/// `[a b c]`, or nothing at all when there's nothing to say.
fn bracket<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let joined = parts.collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        joined
    } else {
        format!("[{joined}]")
    }
}

/// Every field, as the bar wrote it before [`FlagsMode`] existed.
fn legacy(s: &FlagState<'_>) -> String {
    let on_off = |b: bool| if b { "on" } else { "off" };
    let limit = match s.limit {
        Some("!") => " limit:picks".to_string(),
        Some(f) => format!(" limit:{f}"),
        None => String::new(),
    };
    let arrow = if s.sort_reversed { "\u{2191}" } else { "" };
    format!(
        "[picks:{} inv:{} m1:{} m2:{}{limit} hidden:{} sort:{}{arrow}{}{}]",
        s.picks,
        s.inventory,
        on_off(s.masks[0].on),
        on_off(s.masks[1].on),
        s.hidden,
        s.sort,
        s.archive.map_or_else(String::new, |a| format!(" {a}")),
        s.bg.map_or_else(String::new, |b| format!(" {b}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASKS_AT_DEFAULT: [MaskFlag; 2] = [
        MaskFlag {
            on: true,
            default_on: true,
        },
        MaskFlag {
            on: true,
            default_on: true,
        },
    ];

    fn resting() -> FlagState<'static> {
        FlagState {
            picks: 0,
            inventory: 0,
            masks: MASKS_AT_DEFAULT,
            limit: None,
            hidden: 0,
            sort: SortMode::Name,
            sort_reversed: false,
            archive: None,
            bg: None,
        }
    }

    fn auto(s: &FlagState<'_>) -> Suffix {
        suffix(s, FlagsMode::Auto)
    }

    #[test]
    fn a_column_at_rest_says_nothing() {
        assert_eq!(auto(&resting()).full, "");
        assert_eq!(suffix(&resting(), FlagsMode::Short).full, "");
    }

    /// The owner's example: only the hidden count differs from the resting
    /// state, so it's all the bar shows.
    #[test]
    fn only_what_differs_is_shown() {
        let s = FlagState {
            hidden: 1,
            ..resting()
        };
        assert_eq!(
            auto(&s),
            Suffix {
                full: "[hidden:1]".into(),
                short: Some("[h:1]".into()),
            }
        );
    }

    #[test]
    fn every_field_has_a_full_and_a_short_form() {
        let s = FlagState {
            picks: 3,
            inventory: 2,
            limit: Some("*.rs"),
            hidden: 4,
            sort: SortMode::Mtime,
            sort_reversed: true,
            ..resting()
        };
        assert_eq!(
            auto(&s),
            Suffix {
                full: "[picks:3 inv:2 limit:*.rs hidden:4 sort:mtime\u{2191}]".into(),
                short: Some("[p:3 i:2 l:*.rs h:4 s:t\u{2191}]".into()),
            }
        );
        assert_eq!(
            suffix(&s, FlagsMode::Short),
            Suffix::fixed("[p:3 i:2 l:*.rs h:4 s:t\u{2191}]".into())
        );
    }

    /// A mask counts as news when it differs from its configured default, not
    /// from "on": with a mask that starts off, `on` is the state to report.
    #[test]
    fn a_mask_is_shown_when_it_differs_from_its_default() {
        let s = FlagState {
            masks: [
                MaskFlag {
                    on: false,
                    default_on: true,
                },
                MaskFlag {
                    on: true,
                    default_on: false,
                },
            ],
            ..resting()
        };
        assert_eq!(auto(&s).full, "[m1:off m2:on]");
    }

    #[test]
    fn a_reversed_name_sort_is_not_the_resting_sort() {
        let s = FlagState {
            sort_reversed: true,
            ..resting()
        };
        assert_eq!(auto(&s).full, "[sort:name\u{2191}]");
        assert_eq!(auto(&s).short.as_deref(), Some("[s:n\u{2191}]"));
    }

    #[test]
    fn the_picks_limit_and_the_worded_extras_pass_through() {
        let s = FlagState {
            limit: Some("!"),
            archive: Some("zip ro"),
            bg: Some("bg:1\u{25cf}"),
            ..resting()
        };
        assert_eq!(auto(&s).full, "[limit:picks zip ro bg:1\u{25cf}]");
        assert_eq!(
            auto(&s).short.as_deref(),
            Some("[l:picks zip ro bg:1\u{25cf}]")
        );
    }

    /// `full` is the escape hatch to the old bar, so it must reproduce it.
    #[test]
    fn full_mode_is_the_old_bar() {
        let s = FlagState {
            hidden: 1,
            ..resting()
        };
        assert_eq!(
            suffix(&s, FlagsMode::Full),
            Suffix::fixed("[picks:0 inv:0 m1:on m2:on hidden:1 sort:name]".into())
        );
    }

    #[test]
    fn modes_parse_and_print_the_same_words() {
        for m in [FlagsMode::Auto, FlagsMode::Short, FlagsMode::Full] {
            assert_eq!(FlagsMode::parse(&m.to_string()), Some(m));
        }
        assert_eq!(FlagsMode::parse("compact"), None);
    }
}

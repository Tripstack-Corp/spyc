//! What a panic that ends the TUI leaves behind (#932): a report in the state
//! dir's `crash/`, and a link that opens a pre-filled GitHub issue. spyc
//! opens nothing and sends nothing; the user reads the file and follows the
//! link if they choose.
//!
//! Neither names the user's files. A report holds the version, the platform,
//! the terminal's own variables, where the panic happened, its message and
//! the latest actions by name. The message is spyc's text, but an error's
//! context can carry a path, so the link puts `<path>` for every path- or
//! file-shaped word and for the home and working directories; the file keeps
//! the original for the user to review.
//!
//! The actions are a fixed ring beside the worker panics, pushed by
//! `App::apply`, so the hook reads them without reaching into the Model. The
//! report and the link are pure functions of a [`Snapshot`].

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::PathBuf;

/// How many of the latest actions a report names.
const ACTIONS_KEPT: usize = 20;

/// GitHub turns away an issue link much past 8 KiB.
const URL_MAX: usize = 8000;

#[cfg(not(test))]
static ACTIONS: std::sync::Mutex<VecDeque<&'static str>> = std::sync::Mutex::new(VecDeque::new());

#[cfg(test)]
thread_local! {
    static ACTIONS: std::cell::RefCell<VecDeque<&'static str>> =
        const { std::cell::RefCell::new(VecDeque::new()) };
}

/// Note an action by its name (`Action::canonical_name`), for a report to list.
pub fn note_action(name: &'static str) {
    let push = |actions: &mut VecDeque<&'static str>| {
        if actions.len() == ACTIONS_KEPT {
            actions.pop_front();
        }
        actions.push_back(name);
    };
    #[cfg(not(test))]
    push(
        &mut ACTIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    #[cfg(test)]
    ACTIONS.with_borrow_mut(push);
}

/// The latest actions, oldest first. The hook can't wait: a panic while the
/// ring was held leaves it out.
fn recent_actions() -> Vec<&'static str> {
    #[cfg(not(test))]
    {
        match ACTIONS.try_lock() {
            Ok(actions) => actions.iter().copied().collect(),
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().iter().copied().collect(),
            Err(std::sync::TryLockError::WouldBlock) => Vec::new(),
        }
    }
    #[cfg(test)]
    {
        ACTIONS.with_borrow(|actions| actions.iter().copied().collect())
    }
}

/// A panic's message, or a stand-in when its payload isn't text.
pub fn panic_message<'a>(info: &'a std::panic::PanicHookInfo<'_>) -> &'a str {
    info.payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic with no message")
}

/// What a report says, captured when the panic happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub when: String,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub term_program_version: Option<String>,
    pub multiplexer: Option<&'static str>,
    pub ssh: bool,
    pub size: Option<(u16, u16)>,
    pub thread: String,
    pub location: Option<String>,
    pub message: String,
    pub actions: Vec<&'static str>,
}

impl Snapshot {
    /// The panic `info` describes, in the environment it happened in.
    fn capture(info: &std::panic::PanicHookInfo<'_>) -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let has = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        let multiplexer = if has("TMUX") {
            Some("tmux")
        } else if has("ZELLIJ") {
            Some("zellij")
        } else if has("STY") {
            Some("screen")
        } else if has("SPYC_PANE_ID") {
            Some("a spyc pane")
        } else {
            None
        };
        Self {
            when: crate::sysinfo::format_now(),
            version: crate::VERSION,
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            term: var("TERM"),
            term_program: var("TERM_PROGRAM"),
            term_program_version: var("TERM_PROGRAM_VERSION"),
            multiplexer,
            ssh: has("SSH_CONNECTION") || has("SSH_TTY"),
            size: crossterm::terminal::size().ok(),
            thread: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string(),
            location: info.location().map(ToString::to_string),
            message: panic_message(info).to_string(),
            actions: recent_actions(),
        }
    }

    /// The terminal, platform, multiplexer and SSH on one line, the issue
    /// template's "Terminal + OS".
    fn environment(&self) -> String {
        let mut terminal = match (&self.term_program, &self.term_program_version) {
            (Some(p), Some(v)) => format!("{p} {v}"),
            (Some(p), None) => p.clone(),
            (None, _) => "an unnamed terminal".to_string(),
        };
        if let Some(term) = &self.term {
            let _ = write!(terminal, " (TERM={term})");
        }
        if let Some((cols, rows)) = self.size {
            let _ = write!(terminal, ", {cols}x{rows}");
        }
        format!(
            "{terminal}, {} {}, {}, {}",
            self.os,
            self.arch,
            self.multiplexer
                .map_or("no multiplexer".to_string(), |m| format!("in {m}")),
            if self.ssh { "over SSH" } else { "local" },
        )
    }

    /// The report, with `location` and `message` as given.
    fn text_with(&self, location: &str, message: &str, actions: &[&str]) -> String {
        let actions = if actions.is_empty() {
            "none".to_string()
        } else {
            actions.join(", ")
        };
        format!(
            "spyc crash report\n\
             when: {}\n\
             version: {}\n\
             environment: {}\n\
             thread: {}\n\
             at: {location}\n\
             message: {message}\n\
             latest actions, oldest first: {actions}\n",
            self.when,
            self.version,
            self.environment(),
            self.thread,
        )
    }

    /// The report the file holds.
    pub fn report(&self) -> String {
        let location = self.location.as_deref().unwrap_or("unknown");
        self.text_with(location, &self.message, &self.actions)
    }

    /// A link to a new issue from the bug template, its version, environment
    /// and log filled in. `private` are words to keep out of it, the home and
    /// working directories; path- and file-shaped words go too. Under
    /// [`URL_MAX`]: the oldest actions go first, then the message's end.
    pub fn issue_url(&self, private: &[String]) -> String {
        let location = self
            .location
            .as_deref()
            .map_or_else(|| "unknown".to_string(), short_location);
        let mut message: Vec<char> = scrub(&self.message, private).chars().collect();
        let mut actions = self.actions.as_slice();
        loop {
            let text: String = message.iter().collect();
            let url = self.url_with(&self.text_with(&location, &text, actions));
            if url.len() <= URL_MAX {
                return url;
            }
            if actions.is_empty() {
                message.truncate(message.len() / 2);
                message.push('…');
            } else {
                actions = &actions[1..];
            }
        }
    }

    fn url_with(&self, log: &str) -> String {
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
        let field = |s: &str| utf8_percent_encode(s, NON_ALPHANUMERIC).to_string();
        format!(
            "{}/issues/new?template=bug_report.yml&version={}&env={}&logs={}",
            env!("CARGO_PKG_REPOSITORY"),
            field(self.version),
            field(&self.environment()),
            field(log),
        )
    }
}

/// `text` with each of `private` and each path- or file-shaped word put as
/// `<path>`.
fn scrub(text: &str, private: &[String]) -> String {
    let mut text = text.to_string();
    let mut private: Vec<&String> = private.iter().filter(|p| p.len() > 1).collect();
    // The longest first, so a home inside a working directory doesn't leave
    // the rest of it behind.
    private.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for p in private {
        text = text.replace(p.as_str(), "<path>");
    }
    text.split(' ')
        .map(|word| if names_a_file(word) { "<path>" } else { word })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `word` looks like a path (`/`, `\\` or a leading `~`) or a file
/// name (`notes.txt`), past any punctuation around it.
fn names_a_file(word: &str) -> bool {
    let word = word.trim_matches(|c: char| "\"'`()[]{},;:".contains(c));
    if word.contains(['/', '\\']) || word.starts_with('~') {
        return true;
    }
    let word = word.trim_end_matches('.');
    word.rsplit_once('.').is_some_and(|(stem, ext)| {
        stem.chars().any(char::is_alphabetic)
            && (1..=8).contains(&ext.len())
            && ext.chars().all(char::is_alphanumeric)
            && ext.chars().any(char::is_alphabetic)
    })
}

/// A panic location without the directories it was built in: spyc's own
/// `src/…` as it is; for a dependency's absolute path, from its crate's
/// directory (`ratatui-0.29.0/src/buffer.rs:12:5`), or the file name.
fn short_location(location: &str) -> String {
    if !location.starts_with('/') && !location.contains(":\\") {
        return location.to_string();
    }
    let parts: Vec<&str> = location.split(['/', '\\']).collect();
    let from = parts
        .iter()
        .rposition(|p| *p == "src")
        .map_or(parts.len().saturating_sub(1), |src| src.saturating_sub(1));
    parts[from..].join("/")
}

/// Write `snapshot`'s report to `crash/<when>.txt` in the state dir,
/// owner-only like the debug log.
fn write_report(snapshot: &Snapshot) -> Option<PathBuf> {
    use std::io::Write as _;
    let root = crate::state::state_root()?;
    let dir = root.join("crash");
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir).ok()?;
    let stamp = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y%m%dT%H%M%SZ")
        .to_string();
    let name = format!("crash/{stamp}-{}.txt", std::process::id());
    let mut file = crate::state::open_state_file_truncate(&name)?;
    file.write_all(snapshot.report().as_bytes()).ok()?;
    Some(dir.join(name.trim_start_matches("crash/")))
}

/// The panic hook's report, once the terminal is the shell's again: write the
/// file, then say where it is and print the link.
pub fn report(info: &std::panic::PanicHookInfo<'_>) {
    let snapshot = Snapshot::capture(info);
    let private: Vec<String> = [
        std::env::var("HOME").ok(),
        std::env::current_dir()
            .ok()
            .map(|d| d.to_string_lossy().into_owned()),
    ]
    .into_iter()
    .flatten()
    .collect();
    match write_report(&snapshot) {
        Some(path) => eprintln!(
            "\nspyc crashed. A report is in {}\n\
             It names none of your files or what was on screen; read it before you share it.",
            path.display()
        ),
        None => eprintln!("\nspyc crashed, and couldn't write a report to its state directory."),
    }
    eprintln!(
        "To report the crash, open this link. It fills in a new GitHub issue, and \
         nothing is sent until you submit it:\n{}",
        snapshot.issue_url(&private)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            when: "2026-11-23 20:00:00 UTC".to_string(),
            version: "3.0.0-CURRENT (abcdef12)",
            os: "macos",
            arch: "aarch64",
            term: Some("xterm-256color".to_string()),
            term_program: Some("WezTerm".to_string()),
            term_program_version: Some("20240203".to_string()),
            multiplexer: Some("tmux"),
            ssh: false,
            size: Some((200, 50)),
            thread: "main".to_string(),
            location: Some("src/app/actions.rs:120:9".to_string()),
            message: "index out of bounds: the len is 3 but the index is 5".to_string(),
            actions: vec!["down", "enter_or_display", "pane_new_tab"],
        }
    }

    fn decoded(url: &str) -> String {
        percent_encoding::percent_decode_str(url)
            .decode_utf8_lossy()
            .into_owned()
    }

    #[test]
    fn the_ring_keeps_the_latest_actions_oldest_first() {
        for _ in 0..ACTIONS_KEPT {
            note_action("down");
        }
        note_action("up");
        note_action("help");
        let actions = recent_actions();
        assert_eq!(actions.len(), ACTIONS_KEPT);
        assert_eq!(&actions[ACTIONS_KEPT - 2..], ["up", "help"]);
    }

    #[test]
    fn a_report_says_what_crashed_where_and_after_what() {
        let report = snapshot().report();
        for want in [
            "version: 3.0.0-CURRENT (abcdef12)",
            "environment: WezTerm 20240203 (TERM=xterm-256color), 200x50, macos aarch64, in tmux, local",
            "thread: main",
            "at: src/app/actions.rs:120:9",
            "message: index out of bounds: the len is 3 but the index is 5",
            "latest actions, oldest first: down, enter_or_display, pane_new_tab",
        ] {
            assert!(report.contains(want), "{want:?} not in:\n{report}");
        }
    }

    #[test]
    fn the_link_fills_in_the_bug_template() {
        let url = snapshot().issue_url(&[]);
        assert!(
            url.starts_with(concat!(
                env!("CARGO_PKG_REPOSITORY"),
                "/issues/new?template=bug_report.yml&version="
            )),
            "{url}"
        );
        let text = decoded(&url);
        assert!(text.contains("&env=WezTerm 20240203"), "{text}");
        assert!(text.contains("&logs=spyc crash report\n"), "{text}");
        assert!(text.contains("message: index out of bounds"), "{text}");
    }

    /// The link holds nothing of the session's: not the home or working
    /// directory, and no path or file name a message carried. The file keeps
    /// the message as it was.
    #[test]
    fn the_link_names_no_home_working_directory_or_file() {
        let mut s = snapshot();
        s.message = "failed to read /Users/alice smith/work/secret-plans.txt: denied; \
                     also ~/notes.md and 'budget.xlsx', in /Users/alice smith/work"
            .to_string();
        s.location = Some(
            "/Users/alice/.cargo/registry/src/index.crates.io-1/ratatui-0.29.0/src/buffer.rs:12:5"
                .to_string(),
        );
        // A home with a space: "smith" alone is no path-shaped word.
        let private = [
            "/Users/alice smith".to_string(),
            "/Users/alice smith/work".to_string(),
        ];
        let text = decoded(&s.issue_url(&private));
        for gone in [
            "alice",
            "smith",
            "/Users",
            "work",
            "secret-plans",
            "notes.md",
            "budget",
        ] {
            assert!(!text.contains(gone), "{gone:?} in:\n{text}");
        }
        assert!(
            text.contains("at: ratatui-0.29.0/src/buffer.rs:12:5"),
            "{text}"
        );
        assert!(text.contains("failed to read <path> denied"), "{text}");
        assert!(
            s.report()
                .contains("/Users/alice smith/work/secret-plans.txt")
        );
    }

    #[test]
    fn the_link_stays_under_githubs_limit() {
        let mut s = snapshot();
        s.message = "a long message ".repeat(2000);
        s.actions = vec!["pane_scroll_enter"; ACTIONS_KEPT];
        let url = s.issue_url(&[]);
        assert!(url.len() <= URL_MAX, "{}", url.len());
        assert!(decoded(&url).contains("message: a long message"));
    }

    #[test]
    fn words_that_name_files_are_told_from_words_that_dont() {
        for path in [
            "/tmp/x",
            "~/a",
            "notes.txt",
            "'main.rs'",
            "a\\b",
            "Cargo.toml,",
        ] {
            assert!(names_a_file(path), "{path}");
        }
        for word in ["0.29", "len", "Option::unwrap()", "3.", "index", "v1.2.3"] {
            assert!(!names_a_file(word), "{word}");
        }
    }

    #[test]
    fn a_location_keeps_spycs_own_path_and_drops_a_build_directory() {
        assert_eq!(short_location("src/app/run.rs:10:5"), "src/app/run.rs:10:5");
        assert_eq!(
            short_location("/rustc/abc123/library/core/src/slice/index.rs:9:5"),
            "core/src/slice/index.rs:9:5"
        );
        assert_eq!(short_location("/home/u/x.rs:1:1"), "x.rs:1:1");
    }
}

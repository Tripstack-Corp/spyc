//! Transient and persistent UI state (cursor, picks, inventory, masks, marks).

use std::cell::RefCell;
use std::path::PathBuf;

pub mod agy_transcript;
pub mod claude_transcript;
pub mod codex_transcript;
pub mod cursor;
pub mod frecency;
pub mod graveyard;
pub mod harpoon;
pub mod health;
#[allow(dead_code, clippy::question_mark)]
pub mod history;
pub mod hook_consent;
pub mod ignore;
pub mod inventory;
pub mod marks;
pub mod pager_positions;
pub mod picks;
pub mod scope_registry;
pub mod session_names;
pub mod sessions;

pub use cursor::Cursor;
pub use frecency::Frecency;
pub use harpoon::Harpoon;
pub use history::History;
pub use ignore::IgnoreMasks;
pub use inventory::Inventory;
pub use marks::{Mark, Marks};
pub use picks::Picks;

thread_local! {
    static STATE_ROOT_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static CONFIG_ROOT_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Resolve the spyc state-root directory (the equivalent of
/// `$XDG_STATE_HOME/spyc`). Every persistent state module appends its
/// own subdirectory (`harpoon`, `sessions`, `graveyard`, …) under
/// this root.
///
/// Resolution order:
/// 1. Per-thread test override (see `with_state_root`).
/// 2. `$XDG_STATE_HOME/spyc`.
/// 3. `$HOME/.local/state/spyc`.
/// 4. `None` on exotic systems with neither.
///
/// The thread-local override lets parallel tests isolate from each
/// other without mutating process-global env vars — every previous
/// test pattern (`unsafe { set_var("XDG_STATE_HOME", …) }`) collapses
/// into a scoped `with_state_root` call.
pub fn state_root() -> Option<PathBuf> {
    if let Some(p) = STATE_ROOT_OVERRIDE.with(|c| c.borrow().clone()) {
        return Some(p);
    }
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(xdg).join("spyc"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/spyc"))
}

/// Resolve the spyc config-root directory (the equivalent of
/// `$XDG_CONFIG_HOME/spyc`). This is where **user-authored** configuration
/// lives — the Lua entry point (`init.lua`) and the `lua/` script dir hang
/// off it. Distinct from [`state_root`]: config is hand-edited and lives
/// under `~/.config`; state is app-managed and lives under `~/.local/state`.
///
/// Resolution order:
/// 1. Per-thread test override (see `with_config_root`).
/// 2. `$XDG_CONFIG_HOME/spyc`.
/// 3. `$HOME/.config/spyc`.
/// 4. `None` on exotic systems with neither.
///
/// Mirrors [`state_root`]'s thread-local override so parallel tests can
/// isolate without mutating process-global env vars.
pub fn config_root() -> Option<PathBuf> {
    if let Some(p) = CONFIG_ROOT_OVERRIDE.with(|c| c.borrow().clone()) {
        return Some(p);
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("spyc"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/spyc"))
}

/// Path of a file named `name` directly under the state root, if one
/// resolves. Does not create anything — for display / existence checks.
pub fn state_file_path(name: &str) -> Option<PathBuf> {
    state_root().map(|d| d.join(name))
}

/// Open `<state_root>/<name>` for writing as an **owner-only** file,
/// creating the state dir. This is the safe replacement for the old fixed
/// `/tmp/spyc-*` debug/log paths: `/tmp` is world-writable, so on a shared
/// machine another user could pre-create the file (capturing our output in
/// a file they own) or plant a symlink to redirect our writes to a
/// victim-owned path. The XDG state dir is owner-owned; `0600` keeps the
/// contents unreadable by other users and `O_NOFOLLOW` refuses to open the
/// final component if it's a symlink. Returns `None` when no state dir
/// resolves (no `$HOME`/`$XDG_STATE_HOME`) or the open fails.
///
/// `open_state_file_append` keeps existing content (logs); `_truncate`
/// replaces it (one-shot dumps).
#[cfg(unix)]
fn open_state_file(name: &str, write: rustix::fs::OFlags) -> Option<std::fs::File> {
    use rustix::fs::{Mode, OFlags};
    let dir = state_root()?;
    std::fs::create_dir_all(&dir).ok()?;
    let fd = rustix::fs::open(
        dir.join(name),
        OFlags::CREATE | OFlags::WRONLY | OFlags::NOFOLLOW | write,
        Mode::RUSR | Mode::WUSR, // 0600 (applied only when CREATE makes the file)
    )
    .ok()?;
    Some(std::fs::File::from(fd))
}

#[cfg(unix)]
pub fn open_state_file_append(name: &str) -> Option<std::fs::File> {
    open_state_file(name, rustix::fs::OFlags::APPEND)
}

#[cfg(unix)]
pub fn open_state_file_truncate(name: &str) -> Option<std::fs::File> {
    open_state_file(name, rustix::fs::OFlags::TRUNC)
}

/// Non-unix fallback (spyc targets Linux/macOS; this keeps the crate
/// buildable elsewhere without the mode/O_NOFOLLOW hardening).
#[cfg(not(unix))]
fn open_state_file(name: &str, truncate: bool) -> Option<std::fs::File> {
    let dir = state_root()?;
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(!truncate)
        .truncate(truncate)
        .open(dir.join(name))
        .ok()
}

#[cfg(not(unix))]
pub fn open_state_file_append(name: &str) -> Option<std::fs::File> {
    open_state_file(name, false)
}

#[cfg(not(unix))]
pub fn open_state_file_truncate(name: &str) -> Option<std::fs::File> {
    open_state_file(name, true)
}

/// Cap on bytes read from an agent transcript for `^a v` scrollback.
/// Real Claude conversation JSONLs reach 100+ MB; reading the whole
/// file froze the render thread and allocated hundreds of MB.
/// Scrollback only needs recent history, so we read the tail.
pub const MAX_TRANSCRIPT_TAIL_BYTES: u64 = 4 * 1024 * 1024;

/// Read at most the last `max_bytes` of `path` as UTF-8 (lossy). When
/// the file exceeds the cap, the leading partial line is dropped so
/// callers always parse whole lines. Returns an io error only on
/// open/metadata/seek/read failure.
pub fn read_tail_lossy(path: &std::path::Path, max_bytes: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    if start == 0 {
        // Whole file fits in the budget — return it verbatim.
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        return Ok(String::from_utf8_lossy(&buf).into_owned());
    }
    // Seek to one byte *before* the window so we can tell whether the window
    // begins exactly at a line boundary: read from `start - 1` and then drop
    // everything up to and including the first '\n'. If `start - 1` is itself
    // a '\n' (window starts a fresh line), that newline is at index 0 and we
    // keep the whole first in-window line; otherwise we land mid-line and the
    // first '\n' correctly bounds the partial head we discard. (`\n` is one
    // byte, so `nl + 1` is always a char boundary.)
    f.seek(SeekFrom::Start(start - 1))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(match text.find('\n') {
        Some(nl) => text[nl + 1..].to_string(),
        None => String::new(),
    })
}

/// Append an agent prose block to a transcript view, rendered through
/// the Markdown viewer so headings / lists / code / emphasis show as
/// formatting instead of raw `#` / `**` source. Inserts a single blank
/// separator before the block (unless one is already pending) and sets
/// `last_was_blank` to whether the rendered block ended on a blank
/// line, so the caller's inter-turn spacing stays single-blank.
/// `width` is the pager body-width hint (cells) for prose/table reflow;
/// `None` falls back to the renderer's default. Empty bodies are a
/// no-op. Shared by the claude / codex / agy transcript renderers — the
/// only structured-conversation lines that are Markdown source (user
/// prompts and tool calls stay plain, agent-styled).
pub fn push_agent_markdown(
    out: &mut Vec<ratatui::text::Line<'static>>,
    last_was_blank: &mut bool,
    body: &str,
    theme: &crate::ui::theme::Theme,
    width: Option<usize>,
) {
    if body.is_empty() {
        return;
    }
    if !*last_was_blank {
        out.push(ratatui::text::Line::from(""));
    }
    out.extend(crate::ui::markdown::render(body, theme, width));
    *last_was_blank = out
        .last()
        .is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty()));
}

/// Append a blank separator line unless the previous line was already
/// blank (collapses runs to a single blank). Shared by the claude / codex
/// / agy transcript renderers.
pub fn push_transcript_blank(
    out: &mut Vec<ratatui::text::Line<'static>>,
    last_was_blank: &mut bool,
) {
    if !*last_was_blank {
        out.push(ratatui::text::Line::from(""));
        *last_was_blank = true;
    }
}

/// Render a user prompt block: a single blank separator, then each line
/// prefixed with `❯ ` (continuation lines indented two spaces), all in
/// `user_style` (the agent-prompt style: `theme.prompt_prefix` + BOLD).
/// Empty text is a no-op. Shared by the claude / codex / agy transcript
/// renderers — the only structured-conversation lines rendered this way.
pub fn push_transcript_prompt(
    out: &mut Vec<ratatui::text::Line<'static>>,
    last_was_blank: &mut bool,
    text: &str,
    user_style: ratatui::style::Style,
) {
    if text.is_empty() {
        return;
    }
    push_transcript_blank(out, last_was_blank);
    for (i, body) in text.lines().enumerate() {
        let prefix = if i == 0 { "❯ " } else { "  " };
        out.push(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(prefix, user_style),
            ratatui::text::Span::styled(body.to_string(), user_style),
        ]));
    }
    *last_was_blank = false;
}

/// Char-boundary-safe truncation with a `…` suffix, for one-line
/// transcript summaries (tool labels, result previews). Shared by the
/// claude / codex transcript renderers.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Test-only: run `body` with `state_root()` pinned to `root`. The
/// override is unwound when `body` returns *or panics* (RAII guard).
#[cfg(test)]
pub fn with_state_root<R>(root: &std::path::Path, body: impl FnOnce() -> R) -> R {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            STATE_ROOT_OVERRIDE.with(|c| *c.borrow_mut() = None);
        }
    }
    STATE_ROOT_OVERRIDE.with(|c| *c.borrow_mut() = Some(root.to_path_buf()));
    let _g = Guard;
    body()
}

/// Test-only: run `body` with `config_root()` pinned to `root`. The
/// override is unwound when `body` returns *or panics* (RAII guard).
/// Mirrors [`with_state_root`].
#[cfg(test)]
pub fn with_config_root<R>(root: &std::path::Path, body: impl FnOnce() -> R) -> R {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            CONFIG_ROOT_OVERRIDE.with(|c| *c.borrow_mut() = None);
        }
    }
    CONFIG_ROOT_OVERRIDE.with(|c| *c.borrow_mut() = Some(root.to_path_buf()));
    let _g = Guard;
    body()
}

#[cfg(test)]
mod tests {
    use super::{push_transcript_prompt, read_tail_lossy};
    use std::io::Write;

    /// The thread-local override pins `config_root()` for the duration of the
    /// body and is gone afterward (RAII unwind), so parallel tests don't leak
    /// into one another. Touches no process-global env.
    #[test]
    fn config_root_override_pins_and_unwinds() {
        let tmp = tempfile::tempdir().unwrap();
        super::with_config_root(tmp.path(), || {
            assert_eq!(super::config_root().as_deref(), Some(tmp.path()));
        });
        // Override unwound: resolution falls back to env, never our tempdir.
        assert_ne!(super::config_root().as_deref(), Some(tmp.path()));
    }

    /// `config_root` and `state_root` are independent axes — overriding one
    /// must never pin the other (they resolve to different XDG bases).
    #[test]
    fn config_root_independent_of_state_root() {
        let cfg = tempfile::tempdir().unwrap();
        super::with_config_root(cfg.path(), || {
            assert_eq!(super::config_root().as_deref(), Some(cfg.path()));
            assert_ne!(super::state_root().as_deref(), Some(cfg.path()));
        });
    }

    #[test]
    fn transcript_prompt_prefixes_and_collapses_blank() {
        let style = ratatui::style::Style::default();
        let mut out = Vec::new();
        let mut last_was_blank = true; // leading blank suppressed
        push_transcript_prompt(&mut out, &mut last_was_blank, "one\ntwo", style);
        // No leading blank (last_was_blank was true); first line `❯ `, rest `  `.
        let glyphs: Vec<String> = out
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(glyphs, vec!["❯ one", "  two"]);
        assert!(!last_was_blank);
    }

    #[test]
    fn transcript_prompt_empty_is_noop() {
        // Unified across claude/codex/agy: an empty prompt adds nothing and
        // leaves `last_was_blank` untouched (no spurious separator).
        let mut out = Vec::new();
        let mut last_was_blank = false;
        push_transcript_prompt(
            &mut out,
            &mut last_was_blank,
            "",
            ratatui::style::Style::default(),
        );
        assert!(out.is_empty());
        assert!(!last_was_blank);
    }

    #[test]
    fn tail_returns_whole_small_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "a\nb\nc\n").unwrap();
        assert_eq!(read_tail_lossy(f.path(), 1024).unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn tail_drops_partial_leading_line() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for i in 0..10 {
            writeln!(f, "{i:04}").unwrap(); // 10 lines of "NNNN\n", 5 bytes each
        }
        // 22 bytes from a 50-byte file seeks mid-line; the partial head
        // must be dropped so every retained line is whole.
        let got = read_tail_lossy(f.path(), 22).unwrap();
        assert!(got.len() as u64 <= 22);
        assert!(got.ends_with("0009\n"));
        assert!(
            got.lines().all(|l| l.len() == 4),
            "no partial leading line: {got:?}"
        );
    }

    #[test]
    fn tail_keeps_whole_line_when_window_starts_at_line_boundary() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for i in 0..10 {
            writeln!(f, "{i:04}").unwrap(); // 5 bytes each, 50 total
        }
        // max_bytes = 25 makes the window start at byte 25 = the first byte of
        // "0005", a clean line boundary. The whole "0005" line must be kept,
        // not mistaken for a partial head and discarded.
        let got = read_tail_lossy(f.path(), 25).unwrap();
        assert!(got.starts_with("0005\n"), "kept the boundary line: {got:?}");
        assert!(got.ends_with("0009\n"));
        assert!(got.lines().all(|l| l.len() == 4));
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_owner_only_and_appends() {
        use super::{open_state_file_append, with_state_root};
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        with_state_root(tmp.path(), || {
            {
                let mut f = open_state_file_append("log.txt").unwrap();
                writeln!(f, "one").unwrap();
            }
            {
                let mut f = open_state_file_append("log.txt").unwrap();
                writeln!(f, "two").unwrap();
            }
            let p = tmp.path().join("log.txt");
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "must be created 0600 (owner-only)");
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\ntwo\n");
        });
    }

    #[cfg(unix)]
    #[test]
    fn state_file_truncate_replaces_content() {
        use super::{open_state_file_truncate, with_state_root};
        let tmp = tempfile::tempdir().unwrap();
        with_state_root(tmp.path(), || {
            {
                let mut f = open_state_file_truncate("dump.txt").unwrap();
                std::io::Write::write_all(&mut f, b"first dump").unwrap();
            }
            {
                let mut f = open_state_file_truncate("dump.txt").unwrap();
                std::io::Write::write_all(&mut f, b"second").unwrap();
            }
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("dump.txt")).unwrap(),
                "second"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn state_file_refuses_symlink() {
        use super::{open_state_file_append, with_state_root};
        let tmp = tempfile::tempdir().unwrap();
        with_state_root(tmp.path(), || {
            let target = tmp.path().join("victim");
            std::fs::write(&target, b"private").unwrap();
            std::os::unix::fs::symlink(&target, tmp.path().join("link.log")).unwrap();
            // O_NOFOLLOW: opening through a planted symlink must fail, so we
            // never append to (or truncate) an attacker-chosen target.
            assert!(open_state_file_append("link.log").is_none());
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "private");
        });
    }
}

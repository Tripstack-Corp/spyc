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
pub mod hook_owners;
pub mod ignore;
pub mod inventory;
pub mod marks;
pub mod pager_positions;
pub mod picks;
pub mod scope_registry;
pub mod session_names;
pub mod sessions;
pub mod skill_prompt;
pub mod transcript_images;

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

/// Cap on a SINGLE line of a transcript being streamed whole (the `^a g` image
/// index, which needs every record rather than a tail).
///
/// A JSONL line is one record, and an image record is the big one: ~0.5 MB of
/// base64 per image, a handful per message. 32 MB is far above anything a real
/// conversation produces and far below what a file with no newlines at all
/// would cost — `BufRead::lines` on such a file allocates the whole thing.
pub const MAX_TRANSCRIPT_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Read one `\n`-terminated line, bounded.
///
/// Returns `Ok(Some(true))` for a line that fit (in `buf`, newline trimmed),
/// `Ok(Some(false))` for one that exceeded `cap` — `buf` is cleared and the
/// reader is advanced past the offending line, so the caller stays in sync and
/// can simply skip it — and `Ok(None)` at EOF.
///
/// Exists because `BufRead::lines()` / `read_until` grow without limit: a
/// corrupt or hostile file with no newline in it is read entirely into memory.
pub fn read_line_capped<R: std::io::BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    cap: usize,
) -> std::io::Result<Option<bool>> {
    use std::io::{BufRead, Read};
    buf.clear();
    // `cap + 1`: reading that many bytes without a newline proves the line is
    // over the cap, rather than merely reaching it exactly.
    let read = reader
        .by_ref()
        .take(cap as u64 + 1)
        .read_until(b'\n', buf)?;
    if read == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        return Ok(Some(true));
    }
    // No newline. Either EOF (a final unterminated line, which is fine and
    // fits) or the cap stopped us mid-line.
    if read <= cap {
        return Ok(Some(true));
    }
    // Over the cap: drop it and step to the next newline in bounded chunks, so
    // skipping a huge line costs no more memory than reading a normal one.
    buf.clear();
    const SKIP_CHUNK: u64 = 64 * 1024;
    let mut scratch = Vec::with_capacity(SKIP_CHUNK as usize);
    loop {
        scratch.clear();
        let n = reader
            .by_ref()
            .take(SKIP_CHUNK)
            .read_until(b'\n', &mut scratch)?;
        if n == 0 || scratch.last() == Some(&b'\n') {
            return Ok(Some(false));
        }
    }
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
mod state_writes_are_atomic {
    //! Persistent state must be replaced via [`crate::fs::write_atomic`], not
    //! `std::fs::write`: a crash mid-write truncates the real file, and the
    //! startup health check's only recourse is to discard it. Losing marks or
    //! a session snapshot to a power cut is exactly what the temp-file+rename
    //! dance prevents.
    //!
    //! A one-time grep proves today is clean and nothing else; this scan is
    //! what stops the next straggler. It found three on the day it was written
    //! (`inventory`, `harpoon`, `skill_prompt`) that a hand-audit had missed.
    //!
    //! Scoped to `src/state/`, where the writers of the XDG state root live.
    //! Test fixtures legitimately use `fs::write` to build scratch files, so —
    //! following `no_subprocess_git_in_production` — only the portion of each
    //! file before its first `#[cfg(test)]` is scanned, and whole-file test
    //! modules are skipped.
    use std::path::Path;

    /// Deliberate exceptions: relative paths under `src/`, each with the
    /// reason it writes non-atomically. Empty is the healthy state.
    const ALLOWED: &[(&str, &str)] = &[];

    // Split so this literal can't trip its own scan.
    const RAW_WRITE: &str = concat!("fs::write", "(");

    fn is_test_file(path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let in_tests_dir = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "tests" || n.ends_with("_tests"));
        name == "tests.rs"
            || name == "test_support.rs"
            || name.ends_with("_tests.rs")
            || in_tests_dir
    }

    fn scan(dir: &Path, src_root: &Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read state dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                scan(&path, src_root, offenders);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") || is_test_file(&path) {
                continue;
            }
            let rel = path
                .strip_prefix(src_root)
                .unwrap_or(&path)
                .display()
                .to_string();
            if ALLOWED.iter().any(|(p, _)| *p == rel) {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read .rs");
            let production = crate::guard_support::production_half(&text);
            if production.contains(RAW_WRITE) {
                offenders.push(rel);
            }
        }
    }

    #[test]
    fn persistent_state_is_written_atomically() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        scan(&src.join("state"), &src, &mut offenders);
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "persistent state must go through crate::fs::write_atomic — a torn \
             write costs the user their marks/session. Offenders: {offenders:?}. \
             If a site genuinely must write non-atomically, add it to ALLOWED \
             with a reason."
        );
    }
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

#[cfg(test)]
mod capped_line_tests {
    use super::{MAX_TRANSCRIPT_LINE_BYTES, read_line_capped};
    use std::io::BufReader;

    /// Read every line through the capped reader, marking the skipped ones.
    fn read_all(data: &[u8], cap: usize) -> Vec<Option<String>> {
        let mut r = BufReader::new(data);
        let mut buf = Vec::new();
        let mut out = Vec::new();
        while let Ok(Some(fit)) = read_line_capped(&mut r, &mut buf, cap) {
            out.push(fit.then(|| String::from_utf8_lossy(&buf).into_owned()));
        }
        out
    }

    #[test]
    fn ordinary_lines_round_trip() {
        assert_eq!(
            read_all(b"one\ntwo\nthree\n", 1024),
            vec![Some("one".into()), Some("two".into()), Some("three".into())]
        );
    }

    /// A final line with no trailing newline is still a line, not a truncation.
    #[test]
    fn a_missing_final_newline_still_yields_the_line() {
        assert_eq!(
            read_all(b"a\nb", 1024),
            vec![Some("a".into()), Some("b".into())]
        );
    }

    #[test]
    fn crlf_is_trimmed() {
        assert_eq!(
            read_all(b"a\r\nb\r\n", 1024),
            vec![Some("a".into()), Some("b".into())]
        );
    }

    /// The whole point: an over-cap line is skipped AND the reader resyncs, so
    /// the lines after it are still read correctly. Getting this wrong would
    /// silently mis-split the rest of the file.
    #[test]
    fn an_oversized_line_is_skipped_and_the_reader_resyncs() {
        let mut data = Vec::new();
        data.extend_from_slice(b"before\n");
        data.extend(std::iter::repeat_n(b'x', 5000));
        data.push(b'\n');
        data.extend_from_slice(b"after\n");
        assert_eq!(
            read_all(&data, 100),
            vec![Some("before".into()), None, Some("after".into())],
            "the huge line is dropped, not mis-split"
        );
    }

    /// A file with NO newline at all is the case `BufRead::lines()` reads
    /// whole. Bounded here: it yields one skip, not one enormous allocation.
    #[test]
    fn a_file_with_no_newline_is_bounded() {
        let data: Vec<u8> = std::iter::repeat_n(b'x', 100_000).collect();
        assert_eq!(read_all(&data, 1024), vec![None]);
    }

    /// Exact-boundary behaviour: a line of exactly `cap` bytes fits; `cap + 1`
    /// does not. Off-by-one here would reject legitimate records.
    #[test]
    fn the_cap_boundary_is_inclusive() {
        let at_cap = format!("{}\n", "x".repeat(64));
        assert_eq!(read_all(at_cap.as_bytes(), 64), vec![Some("x".repeat(64))]);
        let over = format!("{}\n", "x".repeat(65));
        assert_eq!(read_all(over.as_bytes(), 64), vec![None]);
    }

    /// The shipped cap must clear a realistic image record by a wide margin —
    /// too tight and the gallery silently loses real screenshots.
    #[test]
    fn the_shipped_cap_clears_a_realistic_image_record() {
        // ~0.7 MB of base64 per image was the largest seen in a real
        // transcript; even ten in one record must fit.
        const { assert!(MAX_TRANSCRIPT_LINE_BYTES > 10 * 700_000) };
    }
}

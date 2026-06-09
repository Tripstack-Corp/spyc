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
pub mod ignore;
pub mod inventory;
pub mod marks;
pub mod pager_positions;
pub mod picks;
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
    if start > 0 {
        f.seek(SeekFrom::Start(start))?;
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    if start == 0 {
        return Ok(text);
    }
    // Seek landed mid-line; discard the partial head so the first
    // parsed line is whole. (`\n` is one byte, so `nl + 1` is always
    // a char boundary.)
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

#[cfg(test)]
mod tests {
    use super::read_tail_lossy;
    use std::io::Write;

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
}

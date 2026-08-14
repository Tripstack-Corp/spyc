//! Leaf helper functions with no `App`/`Runtime` dependency: time/byte/text
//! formatting, path + user/host display, a process-group kill, an
//! untracked-file diff, the pure key/refresh predicates the loop consults, and
//! the pty cursor placement the render pass calls. Relocated from `app/mod.rs`
//! (800-LoC campaign); the app-domain glue (`sh_c` → `Effect`,
//! `row_from_entry` → `RowData`) stays in mod.rs.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;

/// Place the terminal cursor where a pty child put it, in frame coordinates.
///
/// Takes the child's screen and the rect it was drawn into rather than reaching
/// for either, which is what lets the `&self` render pass call it.
pub(super) fn place_pty_cursor_from_screen(
    frame: &mut Frame,
    screen: &vt100::Screen,
    rect: ratatui::layout::Rect,
) {
    if screen.hide_cursor() {
        return;
    }
    let (cy, cx) = screen.cursor_position();
    if u32::from(cy) >= u32::from(rect.height) || u32::from(cx) >= u32::from(rect.width) {
        return;
    }
    let x = rect.x + cx;
    let y = rect.y + cy;
    frame.set_cursor_position((x, y));
}

/// How long after a focus-switch chord (`^a-j` / `^a-k`) a same-key
/// Press/Repeat is treated as a stray bounce and dropped. Covers
/// system key-repeat (~30-50 ms) and kitty-keyboard Repeat events.
pub(super) const POST_CHORD_BOUNCE_WINDOW: Duration = Duration::from_millis(60);

/// Whether `key` is a stray bounce of a just-completed focus-switch
/// chord that should be swallowed (rather than leaked to the now-
/// focused pane child).
///
/// `resolver_pending` is the resolver's state *before* this key is
/// fed: when a chord is already mid-flight (the user pressed `^a`
/// again), `key` is a legitimate chord completion, not a bounce — so
/// we must not swallow it. Without this clause, rapid repeated
/// `^a-j` / `^a-k` lost every chord after the first (the second `j`/`k`
/// landed inside the bounce window and was dropped before reaching the
/// resolver).
pub(super) fn is_post_chord_bounce(
    stamp: Option<(std::time::Instant, KeyCode)>,
    key: KeyEvent,
    resolver_pending: bool,
) -> bool {
    let Some((at, code)) = stamp else {
        return false;
    };
    at.elapsed() < POST_CHORD_BOUNCE_WINDOW
        && key.code == code
        && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        && key.modifiers.is_empty()
        && !resolver_pending
}

/// Decide whether the watcher-driven `refresh_listing` should fire
/// this loop iteration.
///
/// A pure trailing-edge debounce (`now - last_event_at >= refresh_quiet`)
/// gets starved under continuous fs activity — cargo writing into
/// `target/`, claude/agent file streams, IDE autosave bursts — because
/// every new event resets `last_event_at` and the quiet window never
/// arrives. So we ALSO cap the wait at `max_defer` from the *first*
/// event of the current busy stretch, ensuring per-file markers can't
/// stay stale forever just because the FS won't go quiet.
pub(super) fn should_fire_refresh(
    last_event_at: Option<std::time::Instant>,
    last_refresh: std::time::Instant,
    first_event_after_refresh: Option<std::time::Instant>,
    now: std::time::Instant,
    refresh_quiet: Duration,
    max_defer: Duration,
) -> bool {
    let Some(at) = last_event_at else {
        return false;
    };
    let trailing_quiet = now.duration_since(at) >= refresh_quiet;
    let max_wait_exceeded =
        first_event_after_refresh.is_some_and(|first| now.duration_since(first) >= max_defer);
    let rate_ok = now.duration_since(last_refresh) >= refresh_quiet;
    (trailing_quiet || max_wait_exceeded) && rate_ok
}

/// Keys we intercept even when the pane is focused.
pub(super) const fn is_spyc_meta_when_pane_focused(key: KeyEvent, resolver_pending: bool) -> bool {
    use crossterm::event::KeyModifiers;
    // Continuation of a multi-key spyc sequence must stay with spyc.
    if resolver_pending {
        return true;
    }
    // Raw FS byte or F10 — always the pane toggle.
    if matches!(key.code, KeyCode::F(10) | KeyCode::Char('\x1c')) {
        return true;
    }
    // Ctrl-\ (toggle), Ctrl-W (vim pane prefix), Ctrl-A (screen prefix).
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('\\' | 'w' | 'W' | 'a' | 'A'))
}

/// Format a `Duration` in seconds as a compact human string for
/// the activity-monitor uptime field. Forms:
/// - `< 1 m`: `Ns`
/// - `< 1 h`: `Nm Ns`
/// - `< 1 d`: `Nh NNm`
/// - `>= 1 d`: `Nd Nh`
pub fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86_400 {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86_400, (secs % 86_400) / 3600)
    }
}

/// Like [`format_uptime`] but always carries seconds, for the live
/// "running…" timer on streaming captures where the user watches the count
/// tick. Deliberately diverges past one hour: `format_uptime` coarsens to
/// `Nh NNm` (then `Nd Nh`) for a static uptime field, whereas this keeps
/// `Nh Nm Ns` so the seconds stay visible on a long-running command.
/// Forms: `Ns` / `Nm Ns` / `Nh Nm Ns`.
pub fn format_elapsed_hms(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

/// Build the EOF marker line appended to captures / finished tasks
/// so the "command finished" indicator stays visible at the bottom
/// of the pager even when content fills the viewport. `tail` is
/// rendered after the literal `[EOF — `; pass the exit string
/// (`"exit 0"`, `"killed"`, `"error: ..."`) or any other short
/// status the caller wants surfaced.
pub fn eof_marker_line(tail: &str) -> ratatui::text::Line<'static> {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    Line::from(Span::styled(
        format!("[EOF — {tail}]"),
        Style::default().add_modifier(Modifier::DIM),
    ))
}

/// Normalize captured pty output for the pager.
///
/// Three passes:
///
/// 1. CRLF (`\r\n`) → LF (`\n`). The pty's slave side enables ONLCR by
///    default, so a child writing `\n` produces `\r\n` on the master
///    we read from. Without this, ratatui rendering interprets the
///    literal `\r` as carriage return and shorter following lines
///    overlay just the prefix of longer prior ones.
/// 2. Bare `\r` collapse. `git pull`, `npm`, `cargo`, etc. use bare
///    `\r` (no newline) to overwrite a progress line on the same
///    terminal row -- `Counting: 18%\rCounting: 27%\rCounting: 100%`.
///    Real terminals handle this; `ansi-to-tui` does not, so without
///    a fix we render every frame side-by-side as one super-wide
///    line. For each `\n`-delimited segment, we keep only the text
///    after the *last* `\r` -- the same final state a real terminal
///    would show. Streaming pagers re-run this every tick, so the
///    user sees live progress (latest frame each redraw).
/// 3. Strip stray ASCII control bytes that aren't whitespace or ANSI
///    escape. Some `git log` commit messages, mboxen, and old-school
///    formatter output carry `\b` (man-page bold trick), `\v`, `\f`,
///    NUL, etc. ratatui can't render them and the host terminal may
///    treat them as cursor controls (backspacing, line-feeding) when
///    we send the bytes through, which fragments rendered Lines and
///    leaves "Buil$er.cs"-style misalignment. We drop them so output
///    is predictable. Kept: `\t` (TAB), `\n` (LF), `\x1b` (ESC for
///    ANSI sequences). Dropped: 0x00-0x08, 0x0B-0x0C, 0x0E-0x1A,
///    0x1C-0x1F, 0x7F.
///
/// ANSI escape sequences never embed bare `\r` and never embed the
/// other control bytes pass 3 strips, so the byte-level passes are
/// safe.
pub fn strip_crlf(bytes: &[u8]) -> Vec<u8> {
    // Pass 1: \r\n -> \n.
    let mut step1 = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            step1.push(b'\n');
            i += 2;
        } else {
            step1.push(bytes[i]);
            i += 1;
        }
    }
    // Pass 2: collapse bare \r within each line to the last frame.
    let step2: Vec<u8> = if step1.contains(&b'\r') {
        let mut out = Vec::with_capacity(step1.len());
        let mut first = true;
        for line in step1.split(|&b| b == b'\n') {
            if !first {
                out.push(b'\n');
            }
            first = false;
            let start = line.iter().rposition(|&b| b == b'\r').map_or(0, |i| i + 1);
            out.extend_from_slice(&line[start..]);
        }
        out
    } else {
        step1
    };
    // Pass 3: drop other ASCII control bytes (keep \t, \n, ESC).
    step2
        .into_iter()
        .filter(|b| {
            !matches!(
                b,
                0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f | 0x7f
            )
        })
        .collect()
}

/// Turn an accumulated pty buffer into pager lines: normalize CRLF / bare CR
/// via [`strip_crlf`], then ANSI-parse, falling back to empty on a parse
/// error. Shared by the background-task pagers (`:fg` re-attach, the static
/// exited-task view, the live task viewer) and the streaming-capture rebuild,
/// which all built `strip_crlf(buf) → into_text().unwrap_or_default().lines`
/// by hand.
pub fn buffer_to_lines(buffer: &[u8]) -> Vec<ratatui::text::Line<'static>> {
    use ansi_to_tui::IntoText;
    strip_crlf(buffer)
        .as_slice()
        .into_text()
        .unwrap_or_default()
        .lines
}

/// `kill(-pid, sig)` — signal the process group leadered by `pid`.
/// portable-pty calls `setsid` on spawn, so the child IS the group
/// leader; negative-pid targets reach grandchildren too. Returns the
/// underlying syscall result so background-task callers can flash
/// the user-facing success/failure message.
///
/// `Pid::from_raw` rejects zero (which would mean "current process
/// group" — a footgun if the child id was somehow 0); on that path
/// we synthesize an `ESRCH` so the caller flashes the same "failed"
/// branch as a real kill failure.
#[cfg(unix)]
pub fn kill_pg(pid: u32, sig: rustix::process::Signal) -> rustix::io::Result<()> {
    match rustix::process::Pid::from_raw(pid as i32) {
        Some(rpid) => rustix::process::kill_process_group(rpid, sig),
        None => Err(rustix::io::Errno::SRCH),
    }
}

/// Last segment of a path as a displayable String, falling back to the full
/// display if the path has no terminating file-name component (root, `..`).
pub fn path_basename_display(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

pub fn user_host_string() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let host = hostname_best_effort();
    format!("{user}@{host}")
}

fn hostname_best_effort() -> String {
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.is_empty()
    {
        return h;
    }
    if let Some(node) = system_nodename()
        && !node.is_empty()
    {
        return node;
    }
    "localhost".to_string()
}

/// `uname(2)`'s `nodename` is the kernel's `gethostname` value — read it
/// with a syscall instead of fork-execing the `hostname` binary.
///
/// Not cfg-gated: spyc targets unix + WSL only (Windows-native was explicitly
/// rejected), so a non-unix build is expected to fail here the way it already
/// fails on `uzers`, `rustix::termios` and the pty layer.
fn system_nodename() -> Option<String> {
    rustix::system::uname()
        .nodename()
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Strip ANSI escape sequences from a string and drop remaining
/// non-printable control bytes, leaving only displayable text. Used
/// to sanitize captured pane-prompt buffers before yanking.
pub fn strip_ansi_escapes(s: &str) -> String {
    let stripped = strip_ansi_escapes::strip_str(s);
    stripped
        .chars()
        .filter(|&c| c >= ' ' || c == '\n' || c == '\t')
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `user_host_string` always yields `user@host` with both halves
    /// non-empty — host falls back to `localhost`, user to `user`.
    #[test]
    fn user_host_string_has_nonempty_user_and_host() {
        let s = user_host_string();
        let (user, host) = s.split_once('@').expect("user@host shape");
        assert!(!user.is_empty(), "user half empty: {s}");
        assert!(!host.is_empty(), "host half empty: {s}");
    }

    /// `buffer_to_lines` normalizes CRLF and bare-CR progress overwrites the
    /// same way `strip_crlf` does, then yields one pager line per `\n`.
    #[test]
    fn buffer_to_lines_normalizes_and_splits() {
        let lines = buffer_to_lines(b"a\r\nb\nc");
        let plain: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(plain, vec!["a", "b", "c"]);
    }
}

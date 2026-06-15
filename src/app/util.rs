//! Leaf helper functions with no `App`/`Runtime` dependency: time/byte/text
//! formatting, path + user/host display, a capped subdir walk, a process-group
//! kill, and an untracked-file diff. Relocated from `app/mod.rs` (800-LoC
//! campaign); the app-domain glue (`sh_c` → `Effect`, `row_from_entry` →
//! `RowData`) stays in mod.rs.

use std::path::Path;

/// Count *every* subdir under `root` (no gitignore awareness), terminating
/// as soon as the running count exceeds `cap`. Sole caller is the **Linux**
/// `pick_recursive_mode` recursive-watch gate (see `MAX_RECURSIVE_WATCH_DIRS`):
/// `notify` registers an inotify watch per directory regardless of
/// `.gitignore`, so that decision must count what's actually on disk. The
/// huge-tree *git* decision instead uses [`count_unignored_subdirs_capped`],
/// which counts only what `git status` walks.
///
/// Iterative DFS over a stack rather than a recursive call or an
/// internal BFS. For an "is the count over `cap`" decision the order
/// doesn't matter; the DFS form keeps stack memory bounded by `cap`
/// (we stop pushing immediately on overflow).
///
/// Does not follow symlinks: `DirEntry::file_type()` is `lstat`-based
/// on Unix, so a symlink-to-dir reports as a symlink (not a dir) and
/// is not pushed onto the walk stack. This matches `notify`'s default
/// behavior — its recursive walker does not chase symlinks either, so
/// the count we produce here tracks what `notify` would have walked.
#[cfg(any(target_os = "linux", test))]
pub fn count_subdirs_capped(root: &Path, cap: usize) -> usize {
    let mut count = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.filter_map(Result::ok) {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                count += 1;
                if count > cap {
                    return count;
                }
                stack.push(entry.path());
            }
        }
    }
    count
}

/// Like [`count_subdirs_capped`], but counts only the directories `git
/// status` would actually traverse — honoring `.gitignore` and the
/// always-ignored `.git`. This is the right size signal for the
/// **huge-tree git-poll throttle**: a Rust checkout's `target/`, an agent's
/// `.claude/`, a JS project's `node_modules/`, etc. hold thousands of
/// subdirs that git never walks, so counting them (as the raw walk does)
/// mis-classifies an otherwise small repo as huge and backs its git polling
/// off to 10 s — making the dirty markers visibly lag real git state.
///
/// Terminates as soon as the running count exceeds `cap` (dropping the
/// iterator stops the walk), so it stays bounded on a genuinely huge tree.
/// Symlinks are not followed. Built on the same `ignore` crate that powers
/// the project-wide finder, so the exclusions match what the user sees there.
///
/// The raw [`count_subdirs_capped`] is still correct for the Linux
/// recursive-watch gate: `notify` registers a watch per directory with no
/// gitignore awareness, so that decision must count every dir on disk.
pub fn count_unignored_subdirs_capped(root: &Path, cap: usize) -> usize {
    let mut count = 0usize;
    for entry in ignore::WalkBuilder::new(root).build() {
        let Ok(entry) = entry else { continue };
        // Depth 0 is `root` itself; count only descendant directories.
        if entry.depth() == 0 {
            continue;
        }
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            count += 1;
            if count > cap {
                return count;
            }
        }
    }
    count
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
#[cfg(unix)]
fn system_nodename() -> Option<String> {
    rustix::system::uname()
        .nodename()
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Non-unix fallback: spyc ships only unix builds, but keep the binary
/// shell-out so any non-unix compile retains its hostname behavior.
#[cfg(not(unix))]
fn system_nodename() -> Option<String> {
    let out = std::process::Command::new("hostname").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
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

    /// The gitignore-aware count must skip a gitignored build dir
    /// (`target/`) and a hidden agent dir (`.claude/`) — the dirs that were
    /// mis-flagging real repos (spyc itself) as huge. A `.git` dir is present
    /// so the `ignore` crate applies the repo's `.gitignore`.
    #[test]
    fn count_unignored_skips_gitignored_and_hidden_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        // Counted: src, src/a, src/b.
        std::fs::create_dir_all(root.join("src/a")).unwrap();
        std::fs::create_dir_all(root.join("src/b")).unwrap();
        // Excluded: gitignored build output + hidden agent dir.
        std::fs::create_dir_all(root.join("target/x/y")).unwrap();
        std::fs::create_dir_all(root.join(".claude/z")).unwrap();

        assert_eq!(count_unignored_subdirs_capped(root, 100), 3);
        // The raw walk, by contrast, sees every dir on disk.
        assert!(count_subdirs_capped(root, 100) >= 8);
    }

    /// Bounded like the raw walk: stops once the count exceeds `cap`.
    #[test]
    fn count_unignored_terminates_past_cap() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::create_dir(tmp.path().join(format!("d{i}"))).unwrap();
        }
        let n = count_unignored_subdirs_capped(tmp.path(), 3);
        assert!(n > 3 && n <= 11, "expected just past cap, got {n}");
    }

    /// `user_host_string` always yields `user@host` with both halves
    /// non-empty — host falls back to `localhost`, user to `user`.
    #[test]
    fn user_host_string_has_nonempty_user_and_host() {
        let s = user_host_string();
        let (user, host) = s.split_once('@').expect("user@host shape");
        assert!(!user.is_empty(), "user half empty: {s}");
        assert!(!host.is_empty(), "host half empty: {s}");
    }
}

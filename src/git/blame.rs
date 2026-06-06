//! `git blame` as a structured model, built on gix's `blame_file` (no
//! subprocess).
//!
//! Produces one [`BlameLine`](crate::git::model::BlameLine) per line of the
//! file, each annotated with the short commit id, author name, and author
//! date of the commit that introduced it. The blame is taken against the
//! current `HEAD` commit (matching plain `git blame <file>`).
//!
//! Wired into the live UI by PR 8b (`app/git_view_session.rs` builds this
//! model off-thread and renders it in-house via [`crate::ui::blame_render`]);
//! the old `git blame` subprocess path was deleted in PR 9.

use crate::git::model::{BlameLine, BlameModel};
use std::collections::HashMap;
use std::path::Path;

/// Blame `path` (repo-relative, forward slashes) at the repo rooted at
/// `repo_root`, against the current `HEAD`. Returns one entry per line in file
/// order, or `None` if the repo can't be opened, `HEAD` can't be resolved, or
/// the blame fails (e.g. the path isn't tracked at `HEAD`).
pub fn blame(repo_root: &Path, path: &str) -> Option<BlameModel> {
    use gix::bstr::{BStr, ByteSlice};

    let repo = gix::open(repo_root).ok()?;
    let head = repo.head_id().ok()?.detach();

    let outcome = repo
        .blame_file(
            BStr::new(path.as_bytes()),
            head,
            gix::repository::blame_file::Options::default(),
        )
        .ok()?;

    // Cache commit metadata (short id, author, date) per commit so a file
    // touched by N commits decodes each commit once, not once per line.
    let mut meta_cache: HashMap<gix::ObjectId, (String, String, String)> = HashMap::new();
    let mut lines: Vec<BlameLine> = Vec::new();

    for (entry, entry_lines) in outcome.entries_with_lines() {
        let (short_id, author, date) = meta_cache
            .entry(entry.commit_id)
            .or_insert_with(|| commit_meta(&repo, entry.commit_id))
            .clone();
        // `start_in_blamed_file` is the 0-based first line of this hunk in the
        // blamed file; line numbers in the model are 1-based.
        let base = entry.start_in_blamed_file;
        for (offset, text) in entry_lines.iter().enumerate() {
            lines.push(BlameLine {
                short_id: short_id.clone(),
                author: author.clone(),
                date: date.clone(),
                lineno: base + offset as u32 + 1,
                text: text
                    .to_str_lossy()
                    .trim_end_matches(['\n', '\r'])
                    .to_owned(),
            });
        }
    }

    // gix produces entries in blamed-file order, but be defensive and sort by
    // line number so the model is always in file order regardless.
    lines.sort_by_key(|l| l.lineno);

    Some(BlameModel {
        path: path.to_string(),
        lines,
    })
}

/// Decode a commit's `(short_id, author_name, author_date)`. On any decode
/// failure the short id is still returned (it's just the hash) with empty
/// author/date, so a partially-corrupt history degrades gracefully rather
/// than dropping lines.
fn commit_meta(repo: &gix::Repository, id: gix::ObjectId) -> (String, String, String) {
    use gix::bstr::ByteSlice;

    // A fixed 7-char short id (git's default width) so parity against
    // `git blame --porcelain` is a clean prefix compare. `ObjectId` derefs to
    // `oid`, whose `to_hex_with_len` gives the truncated hex.
    let short_id = id.to_hex_with_len(7).to_string();
    let Ok(commit) = repo.find_commit(id) else {
        return (short_id, String::new(), String::new());
    };
    let (author, date) = match commit.author() {
        Ok(sig) => {
            let name = sig.name.to_str_lossy().into_owned();
            let date = sig
                .time()
                .ok()
                .map(crate::git::diff_model::format_git_time_pub)
                .unwrap_or_default();
            (name, date)
        }
        Err(_) => (String::new(), String::new()),
    };
    (short_id, author, date)
}

#[cfg(test)]
mod tests {
    use super::blame;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// Hermetic `git`, run from a stable cwd via `git -C <dir>` (parallel-suite
    /// CWD-thrash safe). Mirrors `worktree::tests::run_git`.
    fn run_git(dir: &Path, args: &[&str]) -> String {
        let dir_str = dir.to_str().expect("utf8 dir");
        let mut last_err = String::new();
        for attempt in 0..3u32 {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir_str)
                .args(args)
                .current_dir(std::env::temp_dir())
                .env("GIT_AUTHOR_NAME", "Ada")
                .env("GIT_AUTHOR_EMAIL", "ada@example.com")
                .env("GIT_COMMITTER_NAME", "Ada")
                .env("GIT_COMMITTER_EMAIL", "ada@example.com")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("spawn git");
            if out.status.success() {
                return String::from_utf8(out.stdout).expect("utf8 stdout");
            }
            last_err = String::from_utf8_lossy(&out.stderr).into_owned();
            std::thread::sleep(std::time::Duration::from_millis(
                50 * u64::from(attempt + 1),
            ));
        }
        panic!("git {args:?} failed after 3 attempts: {last_err}");
    }

    fn init_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        (tmp, root)
    }

    /// Parse `git blame --porcelain` into a 1-based `lineno → full-40-hex SHA`
    /// map. In porcelain, a line beginning with a 40-hex SHA + a space starts a
    /// group header `"<sha> <orig-lineno> <final-lineno> [<count>]"`; the actual
    /// file content for that line is the following line that begins with a TAB.
    fn porcelain_line_shas(porcelain: &str) -> HashMap<u32, String> {
        let mut map = HashMap::new();
        let mut cur_sha: Option<String> = None;
        let mut cur_lineno: Option<u32> = None;
        for line in porcelain.lines() {
            if let Some(tab_rest) = line.strip_prefix('\t') {
                let _ = tab_rest; // content line
                if let (Some(sha), Some(no)) = (cur_sha.clone(), cur_lineno) {
                    map.insert(no, sha);
                }
                continue;
            }
            // A header line: "<40-hex> <orig> <final> [<count>]".
            let mut parts = line.split(' ');
            if let Some(first) = parts.next()
                && first.len() == 40
                && first.chars().all(|c| c.is_ascii_hexdigit())
            {
                // The second field is orig line, the third the final
                // (blamed-file) line number.
                let _orig = parts.next();
                if let Some(final_no) = parts.next().and_then(|s| s.parse::<u32>().ok()) {
                    cur_sha = Some(first.to_string());
                    cur_lineno = Some(final_no);
                }
            }
        }
        map
    }

    #[test]
    fn blame_matches_git_porcelain_across_two_commits() {
        let (_t, root) = init_repo();
        // Commit 1: three lines.
        std::fs::write(root.join("f.txt"), "one\ntwo\nthree\n").unwrap();
        run_git(&root, &["add", "f.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        // Commit 2: change the middle line and append a fourth.
        std::fs::write(root.join("f.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        run_git(&root, &["add", "f.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c2"]);

        let model = blame(&root, "f.txt").expect("blame model");
        assert_eq!(model.path, "f.txt");
        assert_eq!(model.lines.len(), 4, "four lines: {:?}", model.lines);

        // Line content + line numbers.
        let texts: Vec<&str> = model.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["one", "TWO", "three", "four"]);
        for (i, l) in model.lines.iter().enumerate() {
            assert_eq!(l.lineno, i as u32 + 1);
            assert_eq!(l.short_id.len(), 7);
            assert_eq!(l.author, "Ada");
            assert!(!l.date.is_empty());
        }

        // Parity: each line's short id must be the 7-prefix of git's porcelain
        // commit for that line.
        let porcelain = run_git(&root, &["blame", "--porcelain", "f.txt"]);
        let git_shas = porcelain_line_shas(&porcelain);
        assert_eq!(
            git_shas.len(),
            4,
            "porcelain should map 4 lines: {porcelain}"
        );
        for l in &model.lines {
            let git_sha = git_shas
                .get(&l.lineno)
                .unwrap_or_else(|| panic!("no porcelain sha for line {}", l.lineno));
            assert!(
                git_sha.starts_with(&l.short_id),
                "line {} blame mismatch: model {} vs git {}",
                l.lineno,
                l.short_id,
                git_sha
            );
        }

        // Specifically: lines 1 and 3 come from c1, lines 2 and 4 from c2 — so
        // line 1's commit differs from line 2's.
        assert_ne!(model.lines[0].short_id, model.lines[1].short_id);
        assert_eq!(model.lines[0].short_id, model.lines[2].short_id);
        assert_eq!(model.lines[1].short_id, model.lines[3].short_id);
    }

    #[test]
    fn blame_none_for_untracked_path() {
        let (_t, root) = init_repo();
        std::fs::write(root.join("f.txt"), "x\n").unwrap();
        run_git(&root, &["add", "f.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        // A path that doesn't exist at HEAD → None, not a panic.
        assert!(blame(&root, "nope.txt").is_none());
    }
}

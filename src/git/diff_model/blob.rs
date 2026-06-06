//! Shared blob/hunk/commit mechanics for the diff-model builders.
//!
//! The per-scope builders in [`super::build`] assemble *which* blobs to diff;
//! this module turns a set-up resource cache into a [`DiffKind`] (binary marker
//! or context-bearing text hunks), computes rename/copy similarity, formats
//! commit metadata, and enforces the global line cap. All of it is gix +
//! `imara-diff` with no `git` subprocess.

use crate::git::model::{CommitMeta, DiffKind, DiffLine, FileDiff, Hunk, LineOrigin};
use std::path::Path;

/// The number of unchanged lines kept on each side of a changed region, the
/// same default `git diff` uses (`-U3`). Adjacent changed regions closer than
/// `2 * CONTEXT_LINES` are fused into one hunk so their context doesn't
/// overlap, matching git's hunk-coalescing.
pub const CONTEXT_LINES: u32 = 3;

/// The total cap on `DiffLine`s across the whole `DiffModel`. A diff that hits
/// this is truncated (later files/hunks dropped) and `DiffModel::truncated` is
/// set, so a pathological diff can't blow up memory on the render side.
pub const MAX_DIFF_LINES: usize = 50_000;

/// Build the [`CommitMeta`] for a commit: full/short id, author name+email, a
/// human author date, and the subject / body split of the message.
pub fn commit_meta(commit: &gix::Commit<'_>) -> Option<CommitMeta> {
    use gix::bstr::ByteSlice;

    let id = commit.id().to_hex().to_string();
    let short_id = commit.id().to_hex_with_len(7).to_string();
    let author = commit.author().ok()?;
    let name = author.name.to_str_lossy().into_owned();
    let email = author.email.to_str_lossy().into_owned();
    let time = author.time().ok().map(format_git_time).unwrap_or_default();
    let message = commit.message().ok()?;
    let subject = message.summary().to_str_lossy().trim_end().to_owned();
    let body = message
        .body()
        .map(|b| b.to_str_lossy().trim().to_owned())
        .unwrap_or_default();

    Some(CommitMeta {
        id,
        short_id,
        author: name,
        email,
        time,
        subject,
        body,
    })
}

/// Format a gix commit/author time as `YYYY-MM-DD HH:MM:SS ±HH:MM` in the
/// recorded timezone offset (matching `git`'s default date display), via jiff.
/// Shared with [`crate::git::blame`] for the blame date column. (`pub`, not
/// `pub(crate)`: the enclosing `git` module is private, so clippy's
/// `redundant_pub_crate` rejects `pub(crate)` here.)
pub fn format_git_time_pub(time: gix::date::Time) -> String {
    format_git_time(time)
}

/// Format a gix commit/author time as `YYYY-MM-DD HH:MM:SS ±HH:MM` in the
/// recorded timezone offset (matching `git`'s default date display), via jiff.
fn format_git_time(time: gix::date::Time) -> String {
    let secs = time.seconds;
    let offset = jiff::tz::Offset::from_seconds(time.offset).unwrap_or(jiff::tz::Offset::UTC);
    let Ok(ts) = jiff::Timestamp::from_second(secs) else {
        return secs.to_string();
    };
    let zoned = ts.to_zoned(jiff::tz::TimeZone::fixed(offset));
    jiff::fmt::strtime::format("%Y-%m-%d %H:%M:%S %:z", &zoned).unwrap_or_else(|_| secs.to_string())
}

/// Set one side (`OldOrSource` if `is_new` is false, else `NewOrDestination`)
/// of the resource cache from a blob id + entry kind + relative path. Errors
/// are swallowed — a failed `set_resource` leaves that side unset, and
/// [`diff_kind_from_cache`] then reports an empty text diff rather than crash.
pub fn set_blob(
    rc: &mut gix::diff::blob::Platform,
    repo: &gix::Repository,
    id: gix::ObjectId,
    kind: gix::object::tree::EntryKind,
    rela_path: &gix::bstr::BStr,
    is_new: bool,
) {
    let resource_kind = if is_new {
        gix::diff::blob::ResourceKind::NewOrDestination
    } else {
        gix::diff::blob::ResourceKind::OldOrSource
    };
    let _ = rc.set_resource(id, kind, rela_path, resource_kind, &repo.objects);
}

/// Read the [`DiffKind`] (binary marker, or text hunks) from a resource cache
/// whose old + new resources have already been set. Detects binary via gix's
/// own classification and otherwise builds context-bearing hunks from the
/// `imara-diff` token ranges.
pub fn diff_kind_from_cache(rc: &mut gix::diff::blob::Platform) -> DiffKind {
    use gix::diff::blob::platform::prepare_diff::Operation;

    rc.options.skip_internal_diff_if_external_is_configured = false;
    let Ok(prep) = rc.prepare_diff() else {
        return DiffKind::Text(Vec::new());
    };
    match prep.operation {
        // A binary side, or an external-diff driver we don't run, yields no
        // structured line diff → mark binary.
        Operation::SourceOrDestinationIsBinary | Operation::ExternalCommand { .. } => {
            DiffKind::Binary
        }
        Operation::InternalDiff { algorithm } => {
            let input = prep.interned_input();
            let diff = gix::diff::blob::diff_with_slider_heuristics(algorithm, &input);
            DiffKind::Text(lines_to_hunks(&diff, &input))
        }
    }
}

/// Turn an `imara-diff` [`Diff`](gix::diff::blob::Diff) plus its
/// [`InternedInput`](gix::diff::blob::InternedInput) into git-style [`Hunk`]s
/// with `CONTEXT_LINES` of surrounding context, fusing changed regions that
/// would otherwise share context, and 1-based line numbers.
fn lines_to_hunks(
    diff: &gix::diff::blob::Diff,
    input: &gix::diff::blob::InternedInput<&[u8]>,
) -> Vec<Hunk> {
    use gix::bstr::ByteSlice;

    // The raw changed regions (token ranges) from imara-diff, in order.
    let regions: Vec<gix::diff::blob::Hunk> = diff.hunks().collect();
    if regions.is_empty() {
        return Vec::new();
    }

    let before_text = |tok: u32| -> String {
        input.interner[input.before[tok as usize]]
            .as_bstr()
            .to_str_lossy()
            .into_owned()
    };
    let after_text = |tok: u32| -> String {
        input.interner[input.after[tok as usize]]
            .as_bstr()
            .to_str_lossy()
            .into_owned()
    };

    let total_before = input.before.len() as u32;
    let mut hunks: Vec<Hunk> = Vec::new();

    // Group regions whose context windows touch/overlap into one displayed
    // hunk (git's hunk coalescing at 2*context).
    let mut i = 0;
    while i < regions.len() {
        let mut group_end = i;
        while group_end + 1 < regions.len() {
            let cur_after_end = regions[group_end].before.end;
            let next_before_start = regions[group_end + 1].before.start;
            if next_before_start.saturating_sub(cur_after_end) <= 2 * CONTEXT_LINES {
                group_end += 1;
            } else {
                break;
            }
        }

        let first = &regions[i];
        let last = &regions[group_end];

        // Old-side span of the group, padded with context but clamped.
        let old_ctx_start = first.before.start.saturating_sub(CONTEXT_LINES);
        let old_ctx_end = (last.before.end + CONTEXT_LINES).min(total_before);
        // New-side start is derived from the old-side start via the leading
        // context offset (context lines exist identically on both sides).
        let new_ctx_start = first
            .after
            .start
            .saturating_sub(first.before.start - old_ctx_start);

        let mut lines: Vec<DiffLine> = Vec::new();
        let mut old_cursor = old_ctx_start;
        let mut new_cursor = new_ctx_start;

        for r in &regions[i..=group_end] {
            // Leading context up to this region's old start. Context lines are
            // identical on both sides, so the new cursor advances in lockstep;
            // it's resynced to `r.after.end` right after the additions, so we
            // only need to advance `old_cursor` here.
            while old_cursor < r.before.start {
                lines.push(DiffLine {
                    origin: LineOrigin::Context,
                    text: before_text(old_cursor),
                });
                old_cursor += 1;
            }
            // Removals.
            for tok in r.before.start..r.before.end {
                lines.push(DiffLine {
                    origin: LineOrigin::Remove,
                    text: before_text(tok),
                });
            }
            old_cursor = r.before.end;
            // Additions.
            for tok in r.after.start..r.after.end {
                lines.push(DiffLine {
                    origin: LineOrigin::Add,
                    text: after_text(tok),
                });
            }
            new_cursor = r.after.end;
        }
        // Trailing context after the last region.
        while old_cursor < old_ctx_end {
            lines.push(DiffLine {
                origin: LineOrigin::Context,
                text: before_text(old_cursor),
            });
            old_cursor += 1;
            new_cursor += 1;
        }

        let old_lines = old_cursor - old_ctx_start;
        let new_lines = new_cursor - new_ctx_start;
        hunks.push(Hunk {
            // git uses 1-based line numbers; a zero-length side keeps start at
            // the line before the change (which is `*_ctx_start`, already the
            // 0-based index → +1 unless the side is empty).
            old_start: if old_lines == 0 {
                old_ctx_start
            } else {
                old_ctx_start + 1
            },
            old_lines,
            new_start: if new_lines == 0 {
                new_ctx_start
            } else {
                new_ctx_start + 1
            },
            new_lines,
            lines,
        });

        i = group_end + 1;
    }

    hunks
}

/// Estimate the rename/copy similarity percentage (0–100) between two blobs,
/// matching the value `git` reports in `R<n>`/`C<n>`. Uses the same line-based
/// ratio gix's rewrite tracker uses; returns 100 when the two ids are equal.
pub fn blob_similarity(repo: &gix::Repository, old: gix::ObjectId, new: gix::ObjectId) -> u8 {
    if old == new {
        return 100;
    }
    let Ok(old_obj) = repo.find_object(old) else {
        return 0;
    };
    let Ok(new_obj) = repo.find_object(new) else {
        return 0;
    };
    let input =
        gix::diff::blob::InternedInput::new(old_obj.data.as_slice(), new_obj.data.as_slice());
    similarity_from_input(&input)
}

/// Similarity (0–100) between a blob in the object DB and a worktree file,
/// matching `git`'s `R<n>`/`C<n>`. Falls back to 0 if either side can't be
/// read.
pub fn worktree_similarity(repo: &gix::Repository, old: gix::ObjectId, new_file: &Path) -> u8 {
    let Ok(old_obj) = repo.find_object(old) else {
        return 0;
    };
    let Ok(new_bytes) = std::fs::read(new_file) else {
        return 0;
    };
    let input = gix::diff::blob::InternedInput::new(old_obj.data.as_slice(), new_bytes.as_slice());
    similarity_from_input(&input)
}

/// The shared similarity ratio from an interned line input: `1 - changed/total`
/// rounded to a percent, where `changed = max(removals, additions)` and
/// `total = max(before, after)`. Empty-vs-empty is a perfect match (100).
fn similarity_from_input(input: &gix::diff::blob::InternedInput<&[u8]>) -> u8 {
    let total = input.before.len().max(input.after.len()) as u32;
    if total == 0 {
        return 100;
    }
    let diff = gix::diff::blob::Diff::compute(gix::diff::blob::Algorithm::Histogram, input);
    let changed = diff.count_removals().max(diff.count_additions());
    let ratio = 1.0 - (f64::from(changed) / f64::from(total));
    (ratio.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// Look up a blob's `(ObjectId, EntryKind)` at a repo-relative path in a tree,
/// or `None` if the path is absent or not a blob.
pub fn tree_blob_at(
    tree: &gix::Tree<'_>,
    path: &str,
) -> Option<(gix::ObjectId, gix::object::tree::EntryKind)> {
    let entry = tree.lookup_entry_by_path(Path::new(path)).ok()??;
    let kind = entry.mode().kind();
    Some((entry.object_id(), kind))
}

/// Whether a repo-relative `path` is selected by the (possibly empty) filter
/// `paths`: empty filter selects everything; otherwise a path is selected if
/// it equals or is under one of the filter entries.
pub fn path_selected(path: &str, paths: &[String]) -> bool {
    if paths.is_empty() {
        return true;
    }
    paths.iter().any(|p| {
        let p = p.trim_end_matches('/');
        path == p || path.starts_with(&format!("{p}/"))
    })
}

/// Account a finished `FileDiff` against the global line budget. The file
/// itself is always kept whole; when its lines exhaust the budget this sets
/// `truncated` so the caller stops appending *further* files. Keeps the whole
/// `DiffModel` to roughly [`MAX_DIFF_LINES`] (plus one over-budget file) so a
/// pathological diff can't balloon memory on the render side.
pub fn account_file(file: &FileDiff, budget: &mut usize, truncated: &mut bool) {
    let count = match &file.kind {
        DiffKind::Text(hunks) => hunks.iter().map(|h| h.lines.len()).sum::<usize>(),
        DiffKind::Binary | DiffKind::Submodule { .. } => 0,
    };
    if count >= *budget {
        *budget = 0;
        *truncated = true;
    } else {
        *budget -= count;
    }
}

#[cfg(test)]
mod tests {
    use super::path_selected;

    #[test]
    fn path_selected_empty_filter_selects_all() {
        assert!(path_selected("a/b.rs", &[]));
    }

    #[test]
    fn path_selected_exact_and_subtree() {
        let f = ["sub".to_string()];
        assert!(path_selected("sub", &f), "exact dir name");
        assert!(path_selected("sub/g.txt", &f), "file under dir");
        assert!(!path_selected("subway.txt", &f), "prefix-but-not-subtree");
        assert!(!path_selected("other/x", &f));
    }

    #[test]
    fn path_selected_trailing_slash_normalized() {
        let f = ["sub/".to_string()];
        assert!(path_selected("sub/g.txt", &f));
    }
}

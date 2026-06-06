//! Pure, owned data model for git diff / show / blame.
//!
//! These types are the structured foundation the in-house renderer
//! ([`crate::ui::diff_render`] / [`crate::ui::blame_render`]) styles into pager
//! lines, replacing the colored-bytes-from-`git` subprocess path (deleted in
//! PR 9). They are deliberately free of any gix types: every field is an owned
//! `String` / number / plain enum, so the model is `Send`, comparable in
//! tests, and carries no borrow back into the repository. The builders that
//! populate them live in [`crate::git::diff_model`] (diff/show) and
//! [`crate::git::blame`].

/// A whole diff: the per-file changes plus a flag for whether we stopped
/// early because the diff exceeded the builder's line cap.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffModel {
    /// One entry per changed path, in the order gix produced them.
    pub files: Vec<FileDiff>,
    /// `true` if the total line count hit the cap and later hunks/files were
    /// dropped — the renderer shows a "diff truncated" marker.
    pub truncated: bool,
}

/// One changed file within a [`DiffModel`].
///
/// `old_path` / `new_path` follow git's convention: a modification has both
/// set to the same path; an addition has only `new_path`; a deletion only
/// `old_path`; a rename/copy has both (source and destination).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// The path on the "old" side (`None` for a pure addition).
    pub old_path: Option<String>,
    /// The path on the "new" side (`None` for a pure deletion).
    pub new_path: Option<String>,
    /// How the file changed (added / deleted / modified / renamed / …).
    pub status: FileStatus,
    /// The textual hunks, or a binary/submodule marker.
    pub kind: DiffKind,
    /// A lowercase language hint derived from the file extension (e.g.
    /// `"rs"`, `"md"`), for the renderer's syntax styling. Empty if unknown.
    pub lang_hint: String,
}

/// The high-level classification of a [`FileDiff`], mirroring git's
/// status letters (`A`/`D`/`M`/`R`/`C`/`T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// A new file (`A`).
    Added,
    /// A removed file (`D`).
    Deleted,
    /// Content changed in place (`M`).
    Modified,
    /// Renamed from `old_path` to `new_path` (`R`), with the similarity
    /// percentage git would report (0–100).
    Renamed {
        /// Similarity percentage (0–100), matching `git`'s `R<n>` value.
        similarity: u8,
    },
    /// Copied from `old_path` to `new_path` (`C`).
    Copied {
        /// Similarity percentage (0–100), matching `git`'s `C<n>` value.
        similarity: u8,
    },
    /// The file's type changed (e.g. regular file ↔ symlink) (`T`).
    TypeChange,
}

/// The body of a [`FileDiff`]: text hunks, a binary marker, or a submodule
/// pointer change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffKind {
    /// A textual diff broken into hunks (possibly empty for a pure
    /// mode/rename change with identical content).
    Text(Vec<Hunk>),
    /// One or both sides are binary; no line diff is produced.
    Binary,
    /// A submodule whose recorded commit changed.
    Submodule {
        /// The old submodule commit (hex), or empty if newly added.
        old: String,
        /// The new submodule commit (hex), or empty if removed.
        new: String,
    },
}

/// A contiguous region of change with surrounding context, matching the
/// `@@ -old_start,old_lines +new_start,new_lines @@` header git emits.
///
/// Line numbers are 1-based (git's convention). A hunk that is a pure
/// addition has `old_lines == 0` and `old_start` set to the line *before*
/// which the addition occurs (again matching git).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// 1-based first line on the old side (0 only for an empty old side).
    pub old_start: u32,
    /// Number of old-side lines the hunk spans.
    pub old_lines: u32,
    /// 1-based first line on the new side (0 only for an empty new side).
    pub new_start: u32,
    /// Number of new-side lines the hunk spans.
    pub new_lines: u32,
    /// The hunk's lines (context + added + removed) in display order.
    pub lines: Vec<DiffLine>,
}

/// A single line within a [`Hunk`]. `text` is the line content **without**
/// the leading `+`/`-`/space marker and **without** the trailing newline —
/// the renderer adds the marker and styling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// Whether the line is unchanged context, added, or removed.
    pub origin: LineOrigin,
    /// The line content (no marker, no trailing newline).
    pub text: String,
}

/// Which side of the diff a [`DiffLine`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrigin {
    /// An unchanged line, present on both sides.
    Context,
    /// A line only on the new side (`+`).
    Add,
    /// A line only on the old side (`-`).
    Remove,
}

/// Commit metadata for the `git show` view, all owned strings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitMeta {
    /// Full 40-char hex commit id.
    pub id: String,
    /// 7-char short id.
    pub short_id: String,
    /// Author name.
    pub author: String,
    /// Author email.
    pub email: String,
    /// Human-readable author date (e.g. `2026-06-05 14:30:00 -04:00`).
    pub time: String,
    /// The first line of the commit message.
    pub subject: String,
    /// The remainder of the commit message (after the blank line), trimmed.
    pub body: String,
}

/// A whole-file blame: each line annotated with the commit that introduced
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameModel {
    /// The repo-relative path that was blamed.
    pub path: String,
    /// One entry per line of the file, in file order.
    pub lines: Vec<BlameLine>,
}

/// One line of a [`BlameModel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// 7-char short id of the commit that introduced this line.
    pub short_id: String,
    /// Author name of that commit.
    pub author: String,
    /// Human-readable author date of that commit.
    pub date: String,
    /// 1-based line number in the blamed file.
    pub lineno: u32,
    /// The line content (without the trailing newline).
    pub text: String,
}

/// Derive a lowercase language hint from a path's extension, for the
/// renderer's syntax styling. Returns the extension lowercased, or empty if
/// the path has none.
#[must_use]
pub fn lang_hint_for(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

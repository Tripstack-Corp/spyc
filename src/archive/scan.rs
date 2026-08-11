//! What was odd about an archive, and what that means we may do with it.
//!
//! The indexer counts oddities as it walks ([`IndexFacts`]); this turns them
//! into a [`Capability`] and a list of warnings. Pure — the numbers come in,
//! the verdict goes out.

use super::ArchiveFormat;
use super::index::{ArchiveIndex, Reject};

/// Everything unusual seen while indexing. Defaults to "nothing odd".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexFacts {
    /// Members dropped for a `..` component — the slip attempt.
    pub traversal_names: usize,
    /// Members dropped for having no usable name.
    pub empty_names: usize,
    /// Members dropped for a NUL in the name.
    pub nul_names: usize,
    /// Members whose leading `/` was stripped (sloppy, not dangerous).
    pub absolute_names: usize,
    /// Members whose `\` separators were reinterpreted as `/`.
    pub backslash_names: usize,
    /// Members whose name collided with an earlier one (last won).
    pub duplicates: usize,
    /// Directories the archive omitted and we synthesized.
    pub implied_dirs: usize,
    /// Members that differ from another only by case.
    pub case_collisions: usize,
    /// zip members we can't decrypt.
    pub encrypted: usize,
    /// zip members using a compression method we don't decode.
    pub unsupported_method: usize,
    /// tar hardlink members.
    pub hardlinks: usize,
    /// tar members that are neither file, dir, nor symlink (fifo, device).
    pub specials: usize,
    /// Symlink members whose target escapes the mount, listed but never created
    /// on disk.
    pub escaping_links: usize,
    /// Members whose destination was only reachable by traversing a symlink, so
    /// nothing was written for them. Distinct from [`Self::escaping_links`]: that
    /// counts a link we refused to *create*, this counts an ordinary member we
    /// refused to write *through* one.
    pub link_traversals: usize,
}

impl IndexFacts {
    pub const fn record_reject(&mut self, reject: Reject) {
        match reject {
            Reject::Traversal => self.traversal_names += 1,
            Reject::Empty => self.empty_names += 1,
            Reject::Nul => self.nul_names += 1,
        }
    }

    /// Members that never made it into the index at all.
    pub const fn skipped(&self) -> usize {
        self.traversal_names + self.empty_names + self.nul_names
    }

    /// tar members whose shape a rebuilt header can't reproduce.
    pub const fn unrebuildable(&self) -> usize {
        self.hardlinks + self.specials
    }
}

/// What spyc will let the user do with a mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    ReadWrite,
    /// Browsing and extracting are fine; repacking is refused, with this reason.
    ReadOnly(String),
}

impl Capability {
    pub const fn is_writable(&self) -> bool {
        matches!(self, Self::ReadWrite)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::ReadWrite => None,
            Self::ReadOnly(why) => Some(why),
        }
    }
}

/// Decide whether a mount may be written back.
///
/// The rule is one idea: **a repack must be able to reproduce every member it
/// isn't deliberately changing.** Anything we dropped or merged on the way in
/// fails that, because rewriting the archive would silently lose it.
///
/// Encrypted and unknown-method zip members do *not* fail it — a zip repack
/// copies untouched members as compressed bytes (`raw_copy_file`), so they
/// survive verbatim even though spyc can't read them. tar has no such escape:
/// its members are rebuilt from captured header fields, so a hardlink or device
/// node — which those fields can't express — makes the archive read-only.
pub fn assess(facts: &IndexFacts, format: ArchiveFormat) -> Capability {
    if facts.skipped() > 0 {
        return Capability::ReadOnly(format!(
            "{} member(s) had unsafe paths and were skipped — rewriting would drop them",
            facts.skipped()
        ));
    }
    if facts.duplicates > 0 {
        return Capability::ReadOnly(format!(
            "{} duplicate member name(s) — rewriting would drop the shadowed copies",
            facts.duplicates
        ));
    }
    // Same rule as a skipped name: we declined to extract these, so a repack
    // can't be trusted to reproduce them. For a streamed mount the staged copy
    // was the only one outside the container, which makes the loss certain
    // rather than merely possible.
    if facts.link_traversals > 0 {
        return Capability::ReadOnly(format!(
            "{} member(s) were reachable only through a symlink and were not \
             extracted — rewriting would drop them",
            facts.link_traversals
        ));
    }
    if format.is_tar() && facts.unrebuildable() > 0 {
        return Capability::ReadOnly(format!(
            "{} member(s) are hardlinks or device nodes spyc can't rebuild",
            facts.unrebuildable()
        ));
    }
    Capability::ReadWrite
}

/// Human-readable notes about a mount, for the mount flash and `:archive info`.
/// Empty when the archive is unremarkable.
pub fn warnings(facts: &IndexFacts, index: &ArchiveIndex) -> Vec<String> {
    let mut out = Vec::new();
    if facts.traversal_names > 0 {
        out.push(format!(
            "{} member(s) skipped: `..` path traversal",
            facts.traversal_names
        ));
    }
    if facts.empty_names + facts.nul_names > 0 {
        out.push(format!(
            "{} member(s) skipped: unusable name",
            facts.empty_names + facts.nul_names
        ));
    }
    if facts.absolute_names > 0 {
        out.push(format!(
            "{} absolute member path(s) made relative",
            facts.absolute_names
        ));
    }
    if facts.backslash_names > 0 {
        out.push(format!(
            "{} member(s) used `\\` separators",
            facts.backslash_names
        ));
    }
    if facts.duplicates > 0 {
        out.push(format!(
            "{} duplicate member name(s) — showing the last",
            facts.duplicates
        ));
    }
    if facts.case_collisions > 0 {
        out.push(format!(
            "{} member(s) differ only by case",
            facts.case_collisions
        ));
    }
    if facts.encrypted > 0 {
        out.push(format!("{} member(s) are encrypted", facts.encrypted));
    }
    if facts.unsupported_method > 0 {
        out.push(format!(
            "{} member(s) use an unsupported compression method",
            facts.unsupported_method
        ));
    }
    if facts.unrebuildable() > 0 {
        out.push(format!(
            "{} hardlink/device member(s)",
            facts.unrebuildable()
        ));
    }
    if facts.escaping_links > 0 {
        out.push(format!(
            "{} symlink(s) point outside the archive and were not created",
            facts.escaping_links
        ));
    }
    if facts.link_traversals > 0 {
        out.push(format!(
            "{} member(s) reachable only through a symlink and not extracted",
            facts.link_traversals
        ));
    }
    if index.truncated {
        out.push(format!(
            "index capped at {} members — the archive has more",
            index.entries.len()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_facts() -> IndexFacts {
        IndexFacts::default()
    }

    fn empty_index() -> ArchiveIndex {
        ArchiveIndex::empty(std::path::PathBuf::from("/src/a.zip"), ArchiveFormat::Zip)
    }

    #[test]
    fn an_ordinary_archive_is_writable() {
        assert_eq!(
            assess(&zip_facts(), ArchiveFormat::Zip),
            Capability::ReadWrite
        );
        assert_eq!(
            assess(&zip_facts(), ArchiveFormat::Tar),
            Capability::ReadWrite
        );
    }

    /// The core rule: if we dropped a member on the way in, we must not rewrite
    /// the archive — the rewrite would make the loss permanent.
    #[test]
    fn skipped_members_make_a_mount_read_only() {
        let facts = IndexFacts {
            traversal_names: 2,
            ..Default::default()
        };
        let cap = assess(&facts, ArchiveFormat::Zip);
        assert!(!cap.is_writable());
        assert!(cap.reason().unwrap().contains("unsafe paths"));
    }

    #[test]
    fn duplicate_names_make_a_mount_read_only() {
        let facts = IndexFacts {
            duplicates: 1,
            ..Default::default()
        };
        assert!(!assess(&facts, ArchiveFormat::Zip).is_writable());
    }

    /// A zip repack copies untouched members as compressed bytes, so members we
    /// cannot *read* still survive a rewrite — no reason to refuse the write.
    #[test]
    fn unreadable_zip_members_do_not_block_writing() {
        let facts = IndexFacts {
            encrypted: 3,
            unsupported_method: 1,
            ..Default::default()
        };
        assert_eq!(assess(&facts, ArchiveFormat::Zip), Capability::ReadWrite);
    }

    /// tar members are rebuilt from captured fields, and those fields cannot
    /// express a hardlink or a device node.
    #[test]
    fn unrebuildable_tar_members_make_a_mount_read_only() {
        let facts = IndexFacts {
            hardlinks: 1,
            ..Default::default()
        };
        for format in [
            ArchiveFormat::Tar,
            ArchiveFormat::TarGz,
            ArchiveFormat::TarZst,
        ] {
            let cap = assess(&facts, format);
            assert!(!cap.is_writable(), "{format:?}");
            assert!(cap.reason().unwrap().contains("hardlinks"));
        }
        // The same members in a zip are impossible, and the rule doesn't apply.
        assert!(assess(&facts, ArchiveFormat::Zip).is_writable());
    }

    #[test]
    fn cosmetic_fixups_do_not_block_writing() {
        let facts = IndexFacts {
            absolute_names: 4,
            backslash_names: 9,
            implied_dirs: 12,
            case_collisions: 1,
            ..Default::default()
        };
        assert_eq!(assess(&facts, ArchiveFormat::Zip), Capability::ReadWrite);
    }

    #[test]
    fn an_unremarkable_archive_warns_about_nothing() {
        let index = empty_index();
        assert!(warnings(&zip_facts(), &index).is_empty());
    }

    #[test]
    fn every_oddity_is_reported_once() {
        let index = empty_index();
        let facts = IndexFacts {
            traversal_names: 1,
            empty_names: 1,
            nul_names: 1,
            absolute_names: 1,
            backslash_names: 1,
            duplicates: 1,
            implied_dirs: 1,
            case_collisions: 1,
            encrypted: 1,
            unsupported_method: 1,
            hardlinks: 1,
            specials: 1,
            escaping_links: 1,
            link_traversals: 1,
        };
        let notes = warnings(&facts, &index);
        // Empty + NUL names share a line; implied dirs are normal and silent.
        assert_eq!(notes.len(), 11, "{notes:#?}");
        assert!(notes.iter().any(|n| n.contains("traversal")));
        assert!(notes.iter().any(|n| n.contains("unusable name")));
        assert!(notes.iter().any(|n| n.contains("only by case")));
        assert!(notes.iter().any(|n| n.contains("encrypted")));
        assert!(
            notes
                .iter()
                .any(|n| n.contains("reachable only through a symlink")),
            "a refused member must be reported, not just counted"
        );
    }

    /// A member we declined to extract is the same problem as one we dropped for
    /// an unsafe name: the repack has nothing to reproduce it from.
    #[test]
    fn a_member_refused_for_traversing_a_link_blocks_write_back() {
        let facts = IndexFacts {
            link_traversals: 1,
            ..IndexFacts::default()
        };
        let verdict = assess(&facts, ArchiveFormat::TarGz);
        assert!(!verdict.is_writable());
        assert!(
            verdict.reason().is_some_and(|r| r.contains("symlink")),
            "the refusal must say why: {verdict:?}"
        );
    }

    #[test]
    fn truncation_is_always_worth_saying() {
        let mut index = empty_index();
        index.truncated = true;
        let notes = warnings(&zip_facts(), &index);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("capped"));
    }
}

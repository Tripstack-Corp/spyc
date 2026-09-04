# Archive browsing with full editing

> **Shipped (v2.1.0, 2026-08-18) — archived as historical record.** The whole
> charter landed: mount, listing, read-out, write-back, nesting, MCP member
> reads, and session/mark round-trips inside a mount (#301, #305, #307, #310,
> #311, #314, #317, #319, #328, #333, #334), followed by the fixes hand-driving
> turned up (#312, #320, #321, #329, #330, #335–#339, #341, #343, #347, #348,
> #376, #377, #392–#399, #410, #412, #414, #417). The refusals the plan
> specified are the refusals that shipped — creating entries, copying a
> directory in, moving across the boundary, `:grep`, `F`, marks, harpoon and
> shell. [Issue #149](https://github.com/Tripstack-Corp/spyc/issues/149) is
> closed. The living reference is AGENTS.md → `src/archive/` and
> ARCHITECTURE.md → "Archive mounts"; this file is the design argument behind
> them.

**Status:** design approved (2026-08-08), shipped v2.1.0 — see the banner.
Charter for [issue #149](https://github.com/Tripstack-Corp/spyc/issues/149) —
"support navigating into zip/tarball's with full editing/yanking capabilities …
minimize disk space requirements (warn on potential issues) and keep
interactions responsive."

## The problem, and why it's structural

`Enter` on a `.zip` hex-dumps it today. Making it navigable is not a listing
change: `listing.dir` is assumed to be a real OS directory — the process cwd
follows it, gix discovery walks it, fs-watch watches it — and every feature that
*touches* a file either goes through `std::fs` or hands the path to another
process (`$EDITOR`, `o`, `!cmd`, the pane, ripgrep). Those last ones can't be
intercepted at all without a real file on disk.

So archive contents must become real bytes at the moment something reads them.
The only open questions are *how few* of them, *when*, and how the write path
gets back into the container without ever putting the original at risk.

FUSE — how every other TUI file manager solves this — is unavailable to us:
macFUSE is a signed system extension, unshippable for a `brew` / `cargo install`
binary.

## Prior art

| Tool | Model | What we take |
|---|---|---|
| [Midnight Commander `extfs`/`tarfs`](https://github.com/MidnightCommander/mc/blob/master/src/vfs/extfs/helpers/README) | Index the archive (`list`), extract **one file at a time** (`copyout`), delegate writes to the archiver (`copyin`, `rm`, `mkdir`, `rmdir`). Path syntax `foo.tar.gz#utar/sub`. | The whole shape: index-driven listing + per-entry materialize + writes as *archive* ops, not filesystem ops. |
| [Total Commander WCX plugins](https://ghisler.github.io/WCX-SDK/overview.htm) | `OpenArchive`/`ReadHeader`/`ProcessFile` to read; `PackFiles`/`DeleteFiles` to write. | Read and write are separate verbs against the container — never "mutate a tree and reconcile later". |
| [7-Zip FM / WinRAR](https://forums.ultraedit.com/open-edit-files-within-zip-archives-t12040.html) | Extract the opened file to temp, run the editor, then ask *"update the archive?"* — 7-Zip on editor exit, WinRAR by watching the temp file's size/mtime. | Deferred, prompted write-back. WinRAR's property-watch is the one that survives an edit we didn't launch (an agent in the pane). |
| [vifm](https://wiki.vifm.info/index.php/How_to_extract_files_from_archives_efficiently) / nnn / [Yazi](https://github.com/dawsers/fuse-archive.yazi) | No native support — shell out to FUSE (`archivemount`, `fuse-zip`, `fuse-archive`); mount on enter, unmount on leave. | The UX contract: enter → you're in; leave → you're back on the archive file. |
| [google/fuse-archive](https://github.com/google/fuse-archive), `fuse-zip`, avfs, gvfs `archive://` | **Read-only, deliberately.** | Read-only is the respectable fallback for anything we can't round-trip faithfully. |
| [archivemount](https://github.com/cybernoid/archivemount) — the one read-write FUSE | Recreates the whole archive at unmount, moving the original to `archive.orig`. README: *"THERE IS ALSO NO GUARANTEE THAT DATA IS WRITTEN CORRECTLY. DO NOT TRUST THIS SOFTWARE!"* | The failure mode to engineer around: verify before you replace, never write in place. |
| [ratarmount](https://github.com/mxmlnkn/ratarmount) / `indexed_gzip` / `indexed_zstd` | Random access into a *compressed* tar needs a seek-point index built by a full pass. | Confirms a compressed tar can't be listed without decompressing end to end — extract-as-you-stream is the efficient choice there, not a shortcut. |

## Libraries

No Rust crate provides an editable archive VFS; that part is ours. What does
exist and pulls its weight:

- **[`zip` 8.x](https://docs.rs/zip/latest/zip/write/struct.ZipWriter.html)** —
  `raw_copy_file` / `raw_copy_file_rename` / `merge_archive` copy untouched
  entries **without recompressing**, so a repack is I/O-bound rather than a
  re-zip. There is deliberately no delete API (neither has libzip): removing or
  renaming means writing a new archive, exactly as `zip -d` and 7-Zip do.
  libzip's own documented practice — temp file, then atomic replace — is the
  pattern we follow.
- **`tar` 0.4** (already a dep) — `Archive::entries()` streams headers;
  `Builder` with `HeaderMode::Complete` re-emits captured headers verbatim,
  preserving mode/uid/gid/mtime/symlinks. `src/state/graveyard.rs` already does
  this.
- **`zstd`** (already a dep), **`flate2`** (already in `Cargo.lock` via gix;
  becomes a direct dep, pure-Rust `miniz_oxide` backend), **`infer`** (already a
  dep — magic-byte detection, used by `file_type_label`).
- In-repo: `fs::atomic::write_atomic`'s temp-then-rename, `Graveyard::write_entry`
  for the pre-repack snapshot (so `:undo` recovers an edit the user regrets), and
  the `graveyard_ops` Effect → worker → Runtime slot → payloadless Message →
  pre-recv drain template for every blocking step.

`zip` needs `default-features = false`, not just tidying: its default set is 14
features wide and includes `bzip2` (a C binding), `lzma`/`xz`, `ppmd`,
`deflate-zopfli` and `aes-crypto`. v1 enables exactly
`["deflate-flate2-zlib-rs", "zstd"]` — deflate through the `zlib-rs` already in
the tree via gix, zstd through the crate already in the tree — so no new C
dependency enters the build. Confirm with `cargo tree -d` and `cargo deny check`
when the dep lands.

## Decisions

1. **A mount is an index, not a directory.** `Enter` on an archive registers an
   `ArchiveMount` and chdirs the column to the archive file's own path:
   `/src/foo.zip` becomes browsable, `/src/foo.zip/sub/a.txt` is a row. No
   sentinel syntax (`#utar`, `!/`) — a path inside a mount is found with
   `path.ancestors().find(|a| mounts.get(a))`, and `parent()`/`join()` keep
   working. Entering a 5 GB zip writes **zero bytes to disk**.
2. **Extraction is per-format, because the formats differ.** Seekable containers
   (zip, uncompressed `.tar`) are indexed only — zip's central directory costs
   nothing to read — and a single entry is materialized on access. A compressed
   tar (`.tar.gz`, `.tar.zst`) gets one streaming pass that indexes *and*
   extracts as it goes, since a second pass per file would re-decompress the
   whole stream. Its disk cost is inherent to the format; the budget, the cache
   cap, and the up-front warning are the mitigations.
3. **Write-back is deferred, prompted, and verified.** Never in place, never
   without asking. See "Write path" below.
4. **Formats for v1:** zip family (`.zip`, `.jar`, `.whl`, `.epub`, `.docx` — one
   container), `.tar`, `.tar.gz`/`.tgz`, `.tar.zst`/`.tzst`.
5. **No new default keybinding.** `Enter`/`e` mount; `h`/`..` climbs out; `D`
   keeps the raw hex view as the escape hatch; everything else is `:archive`,
   per the keymap-slimming policy.

## Architecture

Three seams carry the feature.

### 1. Index-driven listing

`chdir_side` / `refresh_listing` (`src/app/state/listing.rs`) grow an in-mount
branch: build the `Listing` from `ArchiveIndex` + the pending change journal
instead of `Listing::read`, and skip `canonicalize` / `set_current_dir` / the git
walk. The process cwd stays at the archive's parent (so a `!` capture or pane
spawn has somewhere real to stand); git state is cleared, since per-file markers
are meaningless in there.

Everything downstream — sort, masks, `=` filter, picks, cursor, render, mouse,
vsplit geometry — works unchanged, because it all reads `rows`. `L` gets
*richer*: modes and owners come from the archive headers rather than a stat.

### 2. One screen in `run_effects`

Effects are already the sole path to the OS, so a pure
`route_archive_effect(&Effect, &Mounts) -> ArchiveSink` (the `route_input` /
`route_mouse` template: `Copy` snapshot, pure fn, exhaustive match, unit tests)
classifies every outgoing effect:

- `PassThrough` — nothing in it touches a mount.
- `Materialize { paths, then }` — extract, then re-run the original effect with
  real paths substituted.
- `Translate(ArchiveOp)` — an in-mount mutation becomes an archive op.
- `Refuse(reason)` — flashed, with the reason.

This single seam covers all four verbs #149 asks for, because each is already an
effect: yank → `Inventory(Yank)`, put → `Inventory(Put)`, `R` →
`Graveyard(Archive)`, rename/copy/move → `FileOp(Copy/Move/RenameEach)`. The
exhaustive match means a future path-bearing `Effect` variant is a **build
error**, not a silent hole.

Three sites bypass effects and get audited by hand: `plan_pager_open`,
`display_in_pane`, `edit_in_pane` (the last builds an `$EDITOR <path>` command
line as opaque bytes for the pane, so it can't be screened).

### 3. Change journal + a pure repack plan

In-mount mutations land in a journal (`Added` / `Deleted` / `Renamed` /
`Replaced`), so deleting a 500 MB zip entry never extracts it.
`plan_repack(index, journal, staged) -> Vec<RepackStep>` — `RawCopy` |
`FromStaging` | `Skip` — is pure and proptest-able: no entry silently lost, no
duplicate output name, every index entry accounted for.

Edits made outside spyc's own ops (an agent in the pane, `$EDITOR`) are caught
WinRAR-style: each staged file's `(size, mtime)` is recorded at materialize time
and compared at repack time.

### Staging

`~/.local/state/spyc/archives/<pid>-<hash>/`, FIFO-capped like the graveyard,
removed on unmount/quit, with a startup orphan sweep keyed on `pid_alive` (the
`src/mcp/` artifact-sweep precedent). All extraction and repacking runs
off-thread via `Effect::Archive(ArchiveOp)`, so a 2 GB tarball never blocks the
loop; a streaming mount is cancellable with `Esc`.

### Write path

Leaving a dirty mount (climb out, quit, or `:archive write`) prompts `[Y/n]`
naming the change counts. Then, in order:

1. free-space precheck (a repack needs ~1× the archive size);
2. optional `Graveyard::write_entry` snapshot of the original — the *undo*
   affordance, not the correctness mechanism, so it's default-on only under
   `snapshot_max_mb` (a snapshot costs another archive-size copy);
3. write a temp file beside the original, raw-copying untouched entries;
4. **re-open the temp and verify its index against the plan**;
5. atomic rename.

A failure at any step leaves the original byte-identical.

### Refusals, stated rather than half-working

- **vsplit live preview** — it auto-runs on every cursor move, so materializing
  there would silently extract the whole archive.
- **marks / harpoon** — they bookmark a location that won't exist later.
- **`:grep` / `F`** — refused until PR 3, then they materialize the whole mount
  under the budget rather than silently under-reporting.
- **Session save** de-virtualizes the cwd to the archive's parent.

### Read-only when we can't round-trip

A safety scan on mount computes `Capability::{ReadWrite, ReadOnly(reason),
Refuse(reason)}`: encrypted zip entries, unsupported compression methods,
hardlink/sparse/device tar entries, duplicate names, and case-collisions all
demote to read-only. The status bar shows `RO`; `:archive info` names the reason.

### Warnings on mount

Uncompressed size vs. free space and vs. budget; compression-ratio and
entry-count bombs; zip-slip (absolute or `..` entry paths, symlinks escaping the
mount — skipped and listed); non-UTF-8 or backslash-separated zip names; index
truncation past `max_entries`.

## Config surface

```toml
[archive]
enable = true            # Enter on an archive mounts it
staging_cache_mb = 1024  # FIFO cascade cap on the staging root
extract_budget_mb = 512  # ceiling for a streamed (compressed-tar) mount
warn_over_mb = 128       # confirm before mounting past this uncompressed size
max_entries = 200000     # index cap; the listing marks itself truncated
max_depth = 2            # nested archive mounts
write_back = "ask"       # ask | never | always
snapshot_max_mb = 64     # graveyard-snapshot the original below this size
```

## Staged PRs

Each is independently shippable and green; docs ride the commit that changes
behaviour (`FEATURES.md`, `docs/KEYBINDINGS.md`, `src/ui/help.rs`, `CHANGELOG.md`,
`README.md`, `CONFIGURATION.md`, plus the guard-checked module index in
`AGENTS.md`).

- **PR 0 — charter.** This document + a `ROADMAP.md` entry.
- **PR 1 — `src/archive/`, pure core.** No user-visible change. `mod.rs` (format
  detect, `Capability`), `index.rs` (`ArchiveIndex`, `IndexEntry`,
  `Locator::{ZipIndex, TarOffset, Staged}`), `scan.rs` (safety scan → warnings +
  capability), `listing.rs` (`listing_for(index, journal, inner_dir)`),
  `journal.rs` (`Change`, `plan_repack`), `budget.rs`, plus `read.rs` / `write.rs`
  IO halves called only from the worker. Fixtures are built in a `tempfile` with
  the `zip`/`tar` writers — no committed binary blobs. Proptests on `plan_repack`
  and the path-safety scan.
- **PR 2 — mount + navigate (read-only).** `src/app/archive_ops.rs` (worker),
  `src/app/archive.rs` (App glue: mount/unmount, staging lifecycle + orphan
  sweep, status text, `:archive info|list|unmount`), `Effect::Archive`, the
  in-mount branches in `src/app/state/listing.rs`, the `activate` dispatch in
  `src/app/navigate.rs`, climb-out in `climb()`, the `[archive]` config section +
  `--print-config` template, the over-threshold `[Y/n]`, and the refusals above.
- **PR 3 — materialize on demand.** `Materialize { then }` wired for pager open,
  `$EDITOR`, yank, copy/move out, pipe, filetype; the three non-effect sites
  audited; `:grep` / `F` materialize the mount under budget. Completes "yanking".
- **PR 4 — write back.** Journal + `Translate(ArchiveOp)` for put / delete /
  rename / copy-in, the dirty badge, the prompt on leave/quit, `:archive
  write|discard`, and the verified atomic repack (zip via `raw_copy_file` /
  `merge_archive`; tar rebuilt from captured headers).
- **PR 5 — reach.** Nested archives with a depth cap, de-virtualized persistence
  (session cwd, marks, harpoon), MCP `get_file_content` / `search_content` inside
  a mount.

## Verification

- `make check` unpiped (or `make check-ci`) at every PR; `make lint-linux` for
  the free-space call.
- Pure halves: unit + proptest as above. `route_archive_effect` gets an
  exhaustive per-`Effect`-variant test, so a new path-bearing variant fails the
  build.
- Render: `insta` snapshots for the in-mount status bar (mount name, `RO`, dirty
  badge) and an in-mount listing.
- Round-trip harness: build a fixture zip and `.tar.zst` in a tempdir → mount →
  assert zero staging bytes for the zip → materialize one entry → delete another
  → repack → re-open with the crate readers and assert the entry set, contents,
  and preserved modes. Assert the original is byte-identical when a repack step
  is made to fail.
- Dogfood: `make release`, then in spyc — `Enter` a source tarball and a large
  zip, page a file inside, `y` an entry and `p` it outside, `R` an entry, climb
  out, accept the prompt, verify with `unzip -t` / `tar -tf`. Confirm 0 dps at
  idle inside a mount and a live loop while a big tarball streams.

## Non-goals

`.tar.xz` / `.tar.bz2` (the pure-Rust decoders are decode-only, so those mounts
could never be repacked — revisit on demand), 7z, rar, FUSE, seek-point indexes
for compressed tars (the ratarmount trick; no Rust equivalent for gzip), and
password prompts for encrypted zips.

# Reviewer A — the archive subsystem (`src/archive`, `src/app/archive*`)

Subject: `git diff v2.0.0..HEAD` @ `f8dedae`, version `2.1.0-CURRENT`.
Scope: `src/archive/` (5 modules + `read/`), `src/app/archive.rs`,
`src/app/archive_ops.rs`, `src/app/archive_route.rs`, `src/app/state/archive.rs`,
`tests/archive_roundtrip.rs`, `fuzz/fuzz_targets/archive_name.rs`, and the MCP
member-read path in `src/mcp/{protocol,readers}.rs`.

---

## ⚠️ READ THIS FIRST

**A `.tar.gz` of 424 bytes writes an attacker-chosen absolute path on the user's
filesystem the moment the user presses `Enter` on it. No prompt, no warning, no
`escaping_links` count, `Capability::ReadWrite`. I built it and ran it against
HEAD's own `read::stream_mount`; the write landed at `/tmp/spyc-A-PWNED.txt`.**
See BLOCKER-1.

**Shipped 2.0.x binaries are NOT affected** — `src/archive/` does not exist at
`v2.0.0` (`git ls-tree v2.0.0 src/archive` is empty). This is an unreleased
defect on `main`. It is not a live hole in anything a user has installed; it is a
hole that ships with 2.1.0 if the tag is cut as-is. No emergency advisory is
needed; the tag must not go out.

---

## Summary verdict

The *name* half of this subsystem is genuinely good: `index::normalize` rejects
`..`, NUL and empty names on the way in, is proptested and fuzzed, and I could
not construct a member name that escapes it. The *link* half is not: symlink
targets are checked with a purely lexical depth counter that assumes every
component it walks is a real directory, so a chain of symlinks each of which
passes the check composes into one that escapes the staging root entirely —
arbitrary file write, automatic on mount for a compressed tar. Separately, the
subsystem trusts sizes declared by the container in three places where they are
attacker-controlled: a lying zip header defeats the bomb gate, the MCP read cap
and the read allocation, and an absurd tar size field aborts the process outright
(`memory allocation of 4611686018427387904 bytes failed`, SIGABRT). The
budget/`assess`/repack-ordering designs are sound and well tested; the failures
are all in the layer that turns a parsed entry into bytes on disk.

Verdict: **hold the tag.** One blocker, three highs.

---

## Findings

### BLOCKER-1 — a symlink ladder escapes the staging root; arbitrary file write on mount

`src/archive/read/mod.rs:414` (`link_stays_inside`), consumed at
`src/archive/read/mod.rs:252` (`write_member`) and `src/archive/read/mod.rs:304`
(`materialize`). Introduced `3c61ac7` — *feat(archive): archive-browsing core —
index, journal, listing, safety (#301)*.

**Severity: blocker.**

`link_stays_inside` decides whether a symlink member may be created by counting
lexical depth:

```rust
let mut depth = if link_dir.is_empty() { 0 } else { link_dir.split('/').count() as i64 };
for part in target.split('/') {
    match part {
        "" | "." => {}
        ".." => { depth -= 1; if depth < 0 { return false; } }
        _ => depth += 1,
    }
}
true
```

Each non-`..` component contributes `+1` on the assumption that it is a real
directory. If that component is *itself a symlink pointing upward*, the accounting
is wrong. Every individual link in the following set passes the check; the chain
does not:

| member | target | lexical depth | actually resolves to |
|---|---|---|---|
| `a/hop0` | `..` | 1 → 0 ✓ | `<staging>` |
| `a/hop1` | `hop0/..` | 1 → 2 → 1 ✓ | parent of `<staging>` |
| `a/hop2` | `hop1/..` | 1 → 2 → 1 ✓ | grandparent |
| … | | | one real level per hop |

Then a plain **file** member named `a/hop12/tmp/spyc-A-PWNED.txt` is written:
`write_member` does `create_parent(dest)` (`create_dir_all` follows the links)
and `File::create(dest)`, landing the bytes at `/tmp/spyc-A-PWNED.txt`.

**Reachability, two paths:**

1. **Automatic, no user action beyond `Enter`** — `.tar.gz` / `.tar.zst`.
   `stream_mount` (`read/mod.rs:129`) extracts every member as it indexes, in
   archive order, which the attacker controls. `preflight_streamed`
   (`app/archive_ops.rs:441`) sees a tiny compressed size and returns `Proceed`,
   so there is no confirmation prompt. This is the blocker.
2. **One user action** — `.zip` / `.tar`. `materialize` (`read/mod.rs:277`) has
   the identical check, and `ArchiveOp::MaterializeMany`
   (`app/archive_ops.rs:216`) walks the selection in index order, which is
   `inner`-sorted — so naming the hops `a/hop0…a/hop3` puts them ahead of
   `a/hop2/OUTSIDE/pwned.txt`. A multi-pick yank / copy-out of a directory's
   contents escapes.

**Evidence (run against HEAD's library, macOS 25.6):**

```
archive size: 424 bytes
stream_mount OK: 16 entries, escaping_links=0, traversal=0
staging root = /var/folders/.../staging/mount0
ARBITRARY ABSOLUTE WRITE SUCCEEDED: true
content at /tmp/spyc-A-PWNED.txt: "ARBITRARY WRITE FROM A tar.gz MEMBER\n"
capability = ReadWrite
warnings   = []
```

and for the zip/`materialize` path:

```
index order: ["a", "a/hop0", "a/hop1", "a/hop2", "a/hop2/OUTSIDE",
              "a/hop2/OUTSIDE/pwned.txt", "a/hop3"]
ZIP ESCAPE OUTSIDE STAGING: true
```

**Blast radius.** The real staging root is
`<state_root>/archives/<pid>-<hash>` (`app/archive.rs:1346`), so four hops reach
`$HOME`. From there the interesting targets are not data but code:
`~/.config/spyc/.spycrc.toml` is **live-reloaded** and `$HOME`-owned config is
the tier permitted to bind `unix` / `command` / `lua` (`is_executing`), and
`~/.claude/settings.json` carries hooks. Downloading a tarball and pressing
`Enter` on it is therefore a plausible path from "browse an archive" to "run my
shell command", which is the single most user-visible workflow this feature adds.

**Aggravating:** the mount is reported `ReadWrite` with `warnings == []` and
`facts.escaping_links == 0`, because no *individual* link failed the check. The
user is told nothing.

**Related corollary:** `write::verify` (`src/archive/write.rs:344`) re-extracts a
written tar into a `tempfile::tempdir()` via the same `stream_mount`, and
`write_tar` (`write.rs:239-243`) copies an archived symlink's `link_target`
across verbatim — so a repack can both preserve and re-trigger the ladder.

**What a fix needs to do (do not implement here).** Stop deciding containment
lexically. The check has to be against the *resolved* path at creation time —
e.g. resolve the destination's parent (`std::fs::canonicalize` on the parent, or
an `openat`/`O_NOFOLLOW` walk from the staging root) and require the result to be
under the staging root before creating either a link or a file, for `write_member`
**and** `materialize`. Whatever the mechanism, the invariant to assert is "the
final inode is under `staging_root`", not "the name looks like it is". A cheaper
stopgap that closes the automatic path: refuse to create *any* symlink whose
target contains a `..` component, and count it in `escaping_links` (which should
in turn demote `Capability`, see LOW-1). The regression test must be the
**ladder**, not the single-hop `../../../etc/passwd` shape that
`a_symlink_escaping_the_mount_is_listed_but_never_created`
(`read/tests.rs:360`) already covers — that test passes today and the escape
works anyway.

---

### HIGH-1 — a declared member size aborts the process (`memory allocation … failed`)

`src/archive/read/mod.rs:359` (`read_tar_member`), same shape at
`src/archive/read/mod.rs:351` (`read_zip_member`) and `src/archive/write.rs:318`
(`read_at`). Introduced `3c61ac7` (#301).

**Severity: high.**

```rust
let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
f.take(size).read_to_end(&mut bytes)?;
```

`size` is `header.size()` straight out of the tar header — a GNU base-256 size
field holds up to 2^63. The `take()` bounds the *read*; the `with_capacity`
bounds nothing. `Vec::<u8>::with_capacity(1 << 62)` does not panic — it calls
`handle_alloc_error`, which **aborts**. Not an unwind: the panic hook at
`src/lib.rs:248` that restores the terminal never runs, so the user is dropped
back into a raw-mode shell.

**Evidence** (`.tar` with one header declaring `1 << 62` and no body, then
`member_bytes`):

```
--- probe tar: status=ExitStatus(unix_wait_status(6)) ---
stdout: tar index size = 4611686018427387904
stderr: memory allocation of 4611686018427387904 bytes failed
```

Triggered by any read of that member — `Enter`, `y`, `L`, `^a p`, `f` — i.e. one
keystroke on a malformed or hostile `.tar`. The MCP path is *not* affected: it
gates on `entry.size > MAX_READ_BYTES` first (`src/mcp/readers.rs:244`). The TUI
path has no equivalent gate.

**What a fix needs to do.** Never size an allocation from a container-declared
number. Either clamp the hint to the container's own file length (which is a hard
upper bound for a seekable format), or drop `with_capacity` and let `read_to_end`
grow, or — better — stream to the temp file rather than through a `Vec` at all
(`materialize` already writes through `NamedTempFile`; only the intermediate
buffer is unnecessary).

---

### HIGH-2 — a zip's declared uncompressed size is trusted everywhere and enforced nowhere

`src/archive/read/mod.rs:347` (`read_zip_member`), consumed by
`src/archive/budget.rs:110` (`decide_mount`) and `src/mcp/readers.rs:244`.
Introduced `3c61ac7` (#301).

**Severity: high.**

`index_zip` records `member.size()` from the central directory. That number is
attacker-controlled and the zip crate does **not** bound decompressed output by
it (`Decompressor::Deflated` is a bare `flate2::bufread::DeflateDecoder`;
`uncompressed_size` is only used for the LZMA variant —
`zip-8.6.0/src/read/readers.rs:265`). `read_to_end` therefore runs to the end of
the compressed extent regardless of what the header claimed.

Everything that is supposed to protect the user reads the claim:

- `decide_mount` (`budget.rs:110`) computes `total_uncompressed` and the bomb
  ratio from it — a zip declaring 1 byte per member is `Proceed`, never
  `Confirm("expands N×")`.
- `read_member_from_archive` (`mcp/readers.rs:244`) refuses `entry.size >
  MAX_READ_BYTES` (100 KB) and *then* reads — so an agent's `get_file_content`
  can pull an arbitrarily large member into memory and into the MCP response.
- `ArchiveIndex::subtree_bytes` (`index.rs:424`) is the same number.

**Evidence** (a 1 MB member with the 32-bit uncompressed-size fields patched to
1; CRC untouched so the read still validates):

```
index says size=1  total_uncompressed=1  compressed=1098
member_bytes returned 1000000 bytes (declared 1) -> read is NOT bounded by the index
decide_mount -> Proceed
```

Deflate's ceiling is ~1032:1, so a 10 MB zip declaring "1 byte" reads ~10 GB into
a `Vec` on the archive worker thread. That is an OOM abort (same non-unwinding
failure as HIGH-1), reached by pressing `Enter` on a member.

**What a fix needs to do.** Bound the *read*, not the header: wrap the member
reader in a `Take` at `min(declared, hard_cap)` and treat overflow as a corrupt
member (the CRC check will disagree with the truncation, which is the honest
signal). The bomb gate needs a second, size-independent signal — the ratio of
`compressed_size` to the number of members, or an enforced per-read ceiling — so
`decide_mount` is not the only thing standing between the user and a bomb.

---

### HIGH-3 — the MCP member read bypasses the root containment check and follows staging symlinks

`src/mcp/protocol.rs:634-643` and `src/mcp/readers.rs:210-218`. Introduced
`8094f1b` — *feat(archive): let an agent read a member of a mounted archive
(#314)*.

**Severity: high** (medium in isolation; high because BLOCKER-1 makes the
symlink precondition reachable).

In `get_file_content`, the archive branch runs and **returns** before the
`canonicalize` + `starts_with(canonical_root)` traversal guard:

```rust
let member = read_member_content(&resolved, ctx_path).or_else(…);
if let Some(result) = member { return …; }          // ← returns here
// Canonicalize to resolve symlinks and ".." components, then verify …
```

and `read_member_content` prefers the staged copy with
`staging.join(&inner)` + `staged.is_file()` — `is_file()` **follows symlinks**.
So with the ladder from BLOCKER-1 in place, `get_file_content("<archive>/a/hop12/etc/passwd")`
resolves out of the mount and returns the file. `member_in_mount`'s comment
(`readers.rs:184-186`) argues the join is safe because `inner` is normalized;
that is true of the *name* and says nothing about the *tree* the name is joined
onto.

Independently of the ladder, this branch widens the read scope by design: any
path under any mount root is readable regardless of `effective_root` /
`allowed_roots`, so an agent can read out of an archive the user happens to be
browsing in `~/Downloads` even when its root is the repo. That is documented
behaviour (AGENTS.md, MCP tools section) and arguably intended; it is worth
saying out loud that F1's root validation does **not** hold through this path.

**What a fix needs to do.** After resolving a staged path, verify containment the
same way the non-archive branch does — canonicalize and require the result to be
under the mount's `staging_root` — and reject rather than fall through. Whether
the mount-root scope widening is acceptable is a policy call for the MCP owner,
but it should be a stated decision rather than a consequence of an early
`return`.

---

### MEDIUM-1 — `stream_mount` and `restage_missing` ignore `case_rank`; the wrong member's bytes are served

`src/archive/read/mod.rs:168-174` and `src/archive/read/mod.rs:227-230`, against
`src/archive/index.rs:69-75` (`IndexEntry::staging_rel`). Introduced `3c61ac7`
(#301); `restage_missing` `a35802a` (#335).

**Severity: medium.**

`staging_rel()` exists so that two members differing only by case do not clobber
each other on a case-insensitive volume — every default macOS volume, i.e. the
platform this is developed on. Every *reader* uses it (`materialize:278`,
`mount.staging_path`, `write_tar:254`). Both *writers* use `clean.inner`
instead:

```rust
write_member(&mut entry, &draft, &clean.inner, staging_root, &mut builder.facts)?;   // :168
…
// `staging_rel` is what a reader looks under, so a case-colliding member is
// restored where its own reader expects it.
if staging_root.join(&clean.inner).exists() { continue; }                            // :227
write_member(&mut entry, &draft, &clean.inner, staging_root, &mut facts)?;           // :230
```

The comment at `:227` states the opposite of what the code does — an AGENTS.md
"comments state what IS" violation on top of the bug. (`restage_missing` has no
index in hand, so it *cannot* know `case_rank`; the comment is asserting a
property the function is structurally unable to have.)

**Evidence** (a `.tar.gz` holding `a/README` = "UPPER" and `a/readme` = "lower"):

```
case_collisions = 1
  a/README rank=0 staging_rel=a/README            exists=true  bytes=Some("lower")
  a/readme rank=1 staging_rel=.spyc-case-1/a/readme exists=false bytes=None
  on disk at a/README: Some("lower")
```

So on macOS: reading `a/README` silently returns the *other* member's content,
and `a/readme` is unreadable ("staged bytes are missing") and unwritable (the
repack's `std::fs::read(staging_rel)` fails). On a case-sensitive volume the
first symptom disappears and the second remains. The user is warned only that
"1 member(s) differ only by case", which does not describe what happens.

**What a fix needs to do.** Have the streaming writer place members at
`staging_rel()`. That requires knowing the rank during the pass, which the
current design defers to `IndexBuilder::finish` — either rank incrementally as
members are pushed, or do a rename pass at `finish`. `restage_missing` needs the
index (or the mount) passed in so it can ask for `staging_rel()` too.

---

### MEDIUM-2 — the `.spyc-case-N` staging namespace is reachable from a member name

`src/archive/index.rs:69-75`. Introduced `3c61ac7` (#301).

**Severity: medium.**

`staging_rel()` builds `.spyc-case-{rank}/{inner}` for a case-colliding member.
Nothing stops an archive from containing a member *literally named*
`.spyc-case-1/a/readme` — its own `staging_rel()` (rank 0) is the same path.
`materialize` returns early on `dest.exists()`, so whichever is extracted first
wins and the other silently serves those bytes.

**Evidence** (zip with `a/README`, `a/readme`, `.spyc-case-1/a/readme`):

```
  .spyc-case-1/a/readme rank=0 -> .spyc-case-1/a/readme content=Some("DECOY")
  a/README              rank=0 -> a/README              content=Some("RANK0")
  a/readme              rank=1 -> .spyc-case-1/a/readme content=Some("DECOY")
```

Reading `a/readme` yields the decoy. Contrived, but it is content substitution
under attacker control and the fix is cheap.

**What a fix needs to do.** Make the escape namespace unreachable from member
names — e.g. put case-ranked copies under a sibling of the mount's member tree
(`<staging_root>/.case/<rank>/…` where `<staging_root>/.case` is never a member
root) rather than under a reserved prefix inside it; or reject/re-rank a member
whose first component matches the reserved prefix.

---

### MEDIUM-3 — no fuzz target reaches any container parser

`fuzz/fuzz_targets/archive_name.rs`, `src/lib.rs:62-70`. Introduced in-range.

**Severity: medium.**

The premise in the charter is half wrong and half right, so it is worth stating
precisely. A seventh fuzz target **does** exist (`archive_name`), and its header
comment correctly identifies member names as attacker-controlled. But it targets
`index::normalize` — a pure `&str → Result` function that is *already* covered by
two proptests in `src/archive/index.rs:731-763`, one of which asserts exactly the
same invariant. It is the cheapest and best-tested surface in the subsystem.

Nothing fuzzes `detect`, `index_zip`, `index_tar`, `stream_mount`,
`materialize`, or `repack`/`verify` — the functions that actually consume
attacker-controlled *bytes*. HIGH-1 (a SIGABRT from a single tar header field) is
precisely the bug class a structure-aware target over `index_tar` +
`member_bytes` finds on the first thousand executions.

**What a fix needs to do.** Add at least one target that takes `&[u8]`, writes it
to a temp file, and drives `detect_at` → `index_seekable` → `member_bytes` on
every entry, asserting only "does not panic/abort". A second over `stream_mount`
into a scratch dir, asserting "nothing was created outside the scratch dir",
would have caught BLOCKER-1 as well.

---

### LOW-1 — an escaping symlink is a cosmetic warning, not a capability demotion

`src/archive/scan.rs:94-114` (`assess`) vs `src/archive/scan.rs:171-176`
(`warnings`). Introduced `3c61ac7` (#301).

**Severity: low.**

`assess`'s stated rule is "a repack must be able to reproduce every member it
isn't deliberately changing", and `facts.skipped() > 0` (a rejected `..` name)
correctly demotes the mount to `ReadOnly`. `facts.escaping_links > 0` — a member
that *tried* tar-slip and was not created on disk — does not demote and does not
affect `Capability` at all; it only adds a line to `warnings`. That is
inconsistent on its own terms (the link was not reproduced on disk, though it is
re-emitted by `write_tar:239`), and it means an archive that demonstrably
attempted an escape is still offered as writable. It also means the counter
carries no weight, which matters if a fix for BLOCKER-1 routes ladder detection
through it.

---

### LOW-2 — extraction rewrites permission bits: read-only members come out writable, `0o000` members come out unreadable

`src/archive/read/mod.rs:465-472` (`apply_mode`). Introduced `3c61ac7` (#301).
Intersects icebox #48.

**Severity: low.** This is the "what does the new code actually do" answer.

```rust
let perms = std::fs::Permissions::from_mode((mode & 0o777) | 0o200);
```

- **setuid / setgid / sticky are dropped** (`& 0o777`). Good, and the one thing
  that would have been a real escalation.
- **`0o444` becomes `0o644`.** The forced owner-write is justified by "a repack
  couldn't overwrite its staged copy" — but the same staged file is what `c`
  copy-out hands the user, and `std::fs::copy` (`src/fs/ops.rs:110`) preserves
  permissions, so the user's copy of a read-only member is writable.
- **`0o000` becomes `0o200`** — write-only, so the pager's own read of the file
  it just extracted fails with EACCES. Measured: `nomode.txt index mode=100000
  on disk=200 readable=false`.
- **`0o777` is preserved and carried out verbatim**, so a hostile archive can
  place a world-writable file in the user's tree via copy-out.

A fix would separate the two roles: keep the staged copy owner-writable for the
repack's benefit, and apply the archive's real mode at copy-out time (or at
minimum stop making a `0o000` member unreadable).

---

### LOW-3 — a put/copy-in row lists as `size=0`, `kind=File` (confirms the open dogfood issue)

`src/archive/listing.rs:54-71`, `src/app/archive.rs:1376-1399`
(`current_staged_stats`), `src/app/archive.rs:528` (`record_staged`).

**Severity: low.** **Status: CONFIRMED live on HEAD.**

The listing row for a pending addition reads its size and kind from
`staged.get(added)`:

```rust
let stat = staged.get(added);
kind: if stat.is_some_and(|s| s.is_dir) { EntryKind::Dir } else { EntryKind::File },
size: stat.map_or(0, |s| s.size),
mtime: stat.map_or(SystemTime::UNIX_EPOCH, |s| s.mtime),
```

`mount.staged` is only ever filled from two places: `current_staged_stats`, which
iterates `mount.index.entries` — and an addition is by definition *not* in the
index (`mount.rs:30` says so explicitly) — and `record_staged`, whose only two
call sites are the `Materialized` / `MaterializedMany` outcomes
(`app/archive.rs:336,341`). A copy-in goes through
`ArchiveSink::RewriteAndRecord` → `Effect::FileOp(Copy)` and never reaches
either. So `stat` is always `None` for an addition: size 0, kind File, mtime
epoch.

A fix needs `record_staged` (or an equivalent) to run on the copy-in / inventory-put
outcome, keyed by journal path.

---

### LOW-4 — `synthesize_implied_dirs` appends past the entry cap

`src/archive/index.rs:297-327`. Introduced `3c61ac7` (#301).

**Severity: low.** `IndexBuilder::push` stops at `cap`, but `finish` then pushes
every implied ancestor with no cap check. An archive of 200 000 deeply-nested
members can produce an index well past `[archive] max_entries`, which the
`index capped at N members` warning then misreports. Bound the synthesis by the
same cap (and say so when it bites).

### LOW-5 — `[archive] max_entries` does not bound the zip crate's own central-directory parse

`src/archive/read/mod.rs:49` (`ZipArchive::new` runs before the cap applies).

**Severity: low.** `zip-8.6.0/src/read/zip_archive.rs:200` materializes one
`ZipFileData` (a large struct: three boxed strings/slices, two `Arc<[u8]>`, a
`Vec<ExtraField>`) per central-directory record, for *every* record, before spyc
sees the first entry. The crate does guard the pre-allocation against a lying
`number_of_files` (`:183`, `:196`) so a small file cannot force a huge
reservation — but a genuinely large central directory is fully resident before
`max_entries` truncates anything. Worth a sentence in the config docs, which
currently read as though the cap bounds the cost.

### INFO-1 — `\` is unconditionally a separator

`src/archive/index.rs:168-173`. A tar written on Unix may legitimately contain a
member named `a\b.txt`; `normalize` turns it into `a/b.txt`, the mount shows a
directory that does not exist in the archive, and a repack writes the split name
back. The rationale in the doc comment (Windows-written zips) is real; the
trade-off is one-directional and lossy and is not currently surfaced (the
`backslash_names` warning says "used `\` separators", which reads as a fix rather
than a reinterpretation). A Windows drive prefix (`C:\evil`) normalizes to the
inert relative `C:/evil` on Unix; spyc is unix-targeted so this is informational
only.

### INFO-2 — write-back verification of a compressed tar re-extracts it with an unlimited budget

`src/archive/write.rs:344-356`. `verify` calls `stream_mount(..., u64::MAX, ...)`
into a `tempdir`, so writing back a tar.gz near `extract_budget_mb` needs a
second full copy of the expanded tree and ignores the knob that governed the
mount. Correct as a verification strategy; the budget argument should probably be
the configured one.

---

## Premises checked

- **"If there is no fuzz target for the archive parser entry points, that is a
  finding."** Half wrong, half right, and the orchestrator's correction was
  itself worth checking. `fuzz/fuzz_targets/archive_name.rs` exists and is
  in-range — but it drives `index::normalize`, which is the single most-tested
  function in the subsystem (two proptests in `index.rs:731-763` assert the same
  invariant). Zero coverage of `detect` / `index_zip` / `index_tar` /
  `stream_mount` / `materialize` / `repack`. See MEDIUM-3.
- **"does `tests/archive_roundtrip.rs` cover malformed input at all or only
  round-trips well-formed archives?"** Confirmed: `tests/archive_roundtrip.rs`
  is round-trip only (three tests, all well-formed). But "no hostile-input tests"
  would be wrong — `src/archive/read/tests.rs:326` and `:360` cover a `..` member
  and a single-hop escaping symlink, and `read/tests.rs:50` even builds a raw
  header to bypass `tar-rs`'s own validation, which is exactly the right
  instinct. The gap is that both hostile tests cover the *single-hop* shape; the
  composed shape is what beats the check.
- **"zip-slip / path traversal on extract … try to construct an entry that beats
  it."** I could not. `index::normalize` is sound: `..` anywhere is `Reject::Traversal`,
  NUL is rejected, a leading `/` is stripped and flagged, `\` is unified, and the
  result is asserted relative-and-clean by proptest *and* by the fuzz target.
  Every escape I found is via **symlinks**, not names. The AGENTS.md claim that
  "zip-slip is structurally impossible for the rest of the feature" is true of
  names and false of links.
- **"`create_worktree` dies inside a mount" (still open per the dogfood notes).**
  **Wrong on HEAD.** Fixed by `d979964` (#341): `AppState::worktree_anchor`
  (`src/app/state/mod.rs:872-879`) falls back to `tool_root` when the focused
  column's dir is inside a mount, and `plan_worktree_job`
  (`src/app/worktree_ops.rs:215`) uses it. The note can be closed.
- **"put rows list size=0/File" (still open).** **Confirmed live.** See LOW-3.
- **"Decompression bombs … is there any bound on nesting?"** Yes and it is
  correct — `[archive] max_depth` (default 2) is enforced at
  `src/app/archive.rs:98-102` with the depth carried on the mount rather than
  derived. Bounds on expanded size exist for streamed formats and are enforced
  *during* the pass (`read/mod.rs:160-166`); for seekable formats the only bound
  is the declared-size gate, which HIGH-2 defeats.
- **"what happens on a 4-million-entry central directory?"** `max_entries`
  (200 000) truncates the index and the listing says so, but the truncation
  happens after the zip crate has already materialized every record — LOW-5. The
  renderer itself is fine: rows come from `children_of`, which is a prefix range
  over a sorted slice.

---

## Verified correct — do not "fix" these

- **`index::normalize` (`index.rs:164-191`)** and its two proptests. Sound; I
  attacked it and failed. Rejecting on the way in rather than guarding later is
  the right shape and the module doc says so honestly.
- **The prefix-range lookups (`index.rs:397-420`).** `subtree` appends the `/`
  before partitioning, and `a_sibling_sharing_a_prefix_is_not_treated_as_a_child`
  pins the `a.txt` vs `a/…` ordering hazard (`.` is 0x2E, `/` is 0x2F). Correct
  and non-obvious; the test earns its place.
- **`budget::decide_mount` (`budget.rs:110-168`).** Refusal outranks
  confirmation; the `at_least` wording keeps a streamed lower bound from reading
  as a fact; `is_bomb` requires `size_is_exact` so a ratio is never guessed from
  a floor; `compressed_size.max(1)` and `ratio_is_knowable` both guard the
  divide. The tests cover each of these individually. This module is the best
  thing in the subsystem.
- **`write::repack`'s order of operations (`write.rs:50-129`).** Precheck free
  space → write a temp *in the archive's own directory* → `verify` by reading it
  back → graveyard snapshot → `persist` (rename). Snapshot after verification and
  before rename is deliberate and the comment explains why. `verify` genuinely
  re-indexes and diffs against the plan, naming missing and extra members.
- **`scan::assess` (`scan.rs:94-114`).** "A repack must be able to reproduce
  every member it isn't deliberately changing" is the right rule, and the
  zip-raw-copy carve-out for encrypted / unknown-method members is correctly
  reasoned (they survive `raw_copy_file_rename` verbatim, so they are no reason
  to refuse a write) while tar's rebuilt-header limitation correctly demotes.
- **`Mounts::resolve` longest-match (`mount.rs:197-202`)** and
  `member_of`'s exclusion of the mount root (`mount.rs:219-221`). The "a
  container is not a member of itself" distinction is subtle, load-bearing for
  nesting, and pinned by `a_mount_root_is_not_one_of_its_own_members`.
- **`MemberRef` / `member_at` (`mount.rs:32-147`).** Modelling "archived" and
  "added" as the two ways a path inside a mount can be real, in one place, is the
  correct factoring; the doc comment names the bug it fixed.
- **`archive_route`'s exhaustive match over path-bearing effects
  (`archive_route.rs:131`).** Making a new path-carrying `Effect` a build error
  is exactly right for a screen like this. `rename_within` / `rename_change`
  derive their targets through `inner_of`, which rejects non-`Normal`
  components — so a rename cannot introduce a traversal name into a written
  archive. I checked; it can't.
- **`reset_staging` riding inside `ArchiveOp::Mount`
  (`archive_ops.rs:70-77,184-187`).** The reasoning — a separate `Clean` op would
  race the extraction from its own thread — is correct, and doing both on the one
  worker thread is the only ordering guarantee available.
- **`record_staged` keyed by journal path, not staging path
  (`archive.rs:528-549`).** The comment about `.spyc-case-N` giving one member two
  names is right, and this is the *reader* side that MEDIUM-1's writers fail to
  match. Do not "simplify" this to the staging path.
- **`read_head` (`mod.rs:114-129`).** Short reads and short files handled
  correctly; `read` returning less than asked without EOF is accounted for.
- **`AppState::worktree_anchor` (`state/mod.rs:872-879`).** The mount guard is
  correct and closes the dogfood note.
- **`zip_mtime`'s UTC choice (`read/mod.rs:479-501`)** and the two-second DOS
  resolution test. Deliberate, documented, and the "impossible date does not
  panic" test is the right negative.

---

## Reproduction material

All probes were built as an out-of-tree crate depending on `spyc` by path (the
repository was not modified) and run against `f8dedae` on macOS 25.6 (Darwin,
case-insensitive APFS):

- `scratchpad/poc/src/main.rs` — BLOCKER-1 (tar.gz absolute write; zip
  `materialize` ladder), HIGH-2 (patched zip size), MEDIUM-1 (case collision).
- `scratchpad/poc/src/bin/panicprobe.rs` — HIGH-1 (SIGABRT from a tar size
  field), re-run in a subprocess so the abort is observable.
- `scratchpad/poc/src/bin/misc.rs` — MEDIUM-2 (`.spyc-case-N` collision), LOW-2
  (permission bits).

The one file written outside a tempdir (`/tmp/spyc-A-PWNED.txt`, by BLOCKER-1's
demonstration) was removed after the run.

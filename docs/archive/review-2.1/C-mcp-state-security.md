# Reviewer C — MCP, state, and the security posture

Subject: `git diff v2.0.0..HEAD` (HEAD `f8dedae`, `2.1.0-CURRENT`) restricted to
`src/mcp/`, `src/state/`, `SECURITY.md`, `scripts/git-hooks/`, and the guards that
police them. Read-only review; nothing in the main checkout was modified.

## Verdict

Nothing here holds the tag. The three hardening items the charter names — root
validation on the `root` override (#237), the one state-root resolver (#239), and
the pre-commit `GIT_DIR` sanitization — all **landed after v2.0.0** and are intact
and tested on HEAD; the later archive work threaded through them rather than around
them. The real problems are in the *narrative* rather than the code: `SECURITY.md`
makes two claims about CI that stopped being true inside this same diff (cargo-deny
left the per-commit path in #273, the fuzz targets gained a weekly schedule in
#240), and one claim about the MCP surface that was never quite true — the read-tool
`root` validation is bypassable in two auto-approved tool calls via `navigate_to`,
because `allowed_roots` admits whatever `cwd` the context file happens to name.
Two guards under-check against their own documented invariant: the git-env-hygiene
guard accepts a 1-of-9 variable strip (ten sites in the tree do exactly that), and
the installed pre-commit hook has no staleness story at all — the fix for an
incident that silently reset developers' branches is delivered by a `make` target
nobody is told to re-run.

---

## MCP tool → root-validation table (enumerated from `src/mcp/protocol.rs` on HEAD)

25 tools declared (`protocol.rs:192-573`), 25 dispatched (`protocol.rs:605-1021`);
the two lists match, and `tools_list_response` (`src/mcp/tests/mod.rs:122-158`)
pins the count and order.

### Read tools (socket thread, no `App`)

| # | Tool | Takes `root`? | Validated? | How |
|---|------|---------------|-----------|-----|
| 1 | `get_spyc_context` | no | n/a | echoes the context file |
| 2 | `get_file_content` | yes | **yes** | `effective_root` (`protocol.rs:622`) + `canonicalize` + `starts_with(root)` (`protocol.rs:649-659`). **Caveat**: the archive-member branch (`protocol.rs:635-646`) runs *before* both checks — see F5/F6 |
| 3 | `search_paths` | yes | **yes** | `effective_root` (`protocol.rs:668`) + `mount_refusal` (`:672`) |
| 4 | `search_content` | yes | **yes** | `effective_root` (`protocol.rs:695`) + `mount_refusal` (`:699`) |
| 5 | `search_picks` | no | n/a | reads picks out of the context file (spyc's own state) |
| 6 | `search_inventory` | no | n/a | reads the inventory out of the context file |
| 7 | `list_worktrees` | no | n/a | `search_root(ctx)` only (`readers.rs:406-408`) |
| 8 | `git_status` | yes | **yes** | `effective_root` (`protocol.rs:744`) |
| 9 | `git_log` | yes | **yes** | `effective_root` (`protocol.rs:755`) |
| 10 | `git_diff` | yes | **yes** | `effective_root` (`protocol.rs:765`) |
| 11 | `claim_worktree` | no (`path`) | **no** | `resolve_worktree_path` (`readers.rs:467-476`) takes any absolute path; writes a git lock file there |
| 12 | `release_worktree` | no (`path`) | **no** | same resolver; removes a lock file |

`effective_root` → `allowed_roots` → `is_within_allowed` is `readers.rs:18-104`.
Every tool that accepts `root` routes through it; no read tool takes `root` and
skips it. That half of F1 is clean.

### Write tools (forwarded to the main loop over the command channel)

| # | Tool | Path arg | Validated? | Consequence |
|---|------|----------|-----------|-------------|
| 13 | `navigate_to` | `path` | **no** (`app/mcp.rs:336`, `state/navigation.rs:242-267`) | chdirs the user's column anywhere; `~`/`$VAR` expanded. **Widens `allowed_roots`** — F1 |
| 14 | `set_filter` | — | n/a | model edit |
| 15 | `pick_files` | — | n/a | model edit |
| 16 | `clear_picks` | — | n/a | model edit |
| 17 | `create_worktree` | `branch`/`base` | bounded | path derived from the repo, not supplied |
| 18 | `remove_worktree` | `path` | **no** (`app/mcp.rs:295-314`) | archives + deletes any dir containing `.git` (`worktree_clean.rs:52-65`) |
| 19 | `clean_worktree` | `path` | **no** | alias of 18 |
| 20 | `open_worktree` | `path` | **no** (`app/mcp.rs:437-454`) | opens column b anywhere; **widens `allowed_roots`** |
| 21 | `report_status` | `pane_id`/`pane` | no caller auth | any socket peer sets any pane's dot |
| 22 | `register_scope` | `pane_id`/`pane` | no caller auth | advisory |
| 23 | `list_scopes` | — | n/a | advisory |
| 24 | `release_scope` | `id` | **deliberately none** | documented: "No ownership check" (`protocol.rs:252`) |
| 25 | `wait_for_scope_clear` | `pane_id`/`pane` | no caller auth | advisory |

---

## Findings

### F1 — `SECURITY.md`'s "an agent cannot point a read tool at an arbitrary directory" is false; `navigate_to` widens the allowed set — **medium**

`SECURITY.md:50-54`:

> That argument is **validated against a set of roots spyc knows about**, so an
> agent cannot point a read tool at an arbitrary directory.

`allowed_roots` (`src/mcp/readers.rs:52-91`) is built from three sources, and the
third is the context file's own cursor-tracking fields:

```rust
// readers.rs:78-88
for key in ["search_root", "project_home", "cwd"] {
    if let Some(s) = v[key].as_str() && !s.is_empty() { push(PathBuf::from(s)); }
}
```

`navigate_to` (tool 13) sets that `cwd` to any path the agent names, with no
scoping check — `AppState::jump_to` (`src/app/state/navigation.rs:242-267`)
canonicalizes and chdirs, nothing more — and `execute_mcp_command` then calls
`write_context()` **synchronously** on the same turn (`src/app/mcp.rs:349`,
deliberately, so a follow-up read isn't stale). So:

1. `navigate_to("/etc")` → context `cwd` becomes `/etc`
2. `search_content(pattern, root="/etc")` → `is_within_allowed` matches → allowed

Both calls are the kind agent harnesses auto-approve, which is exactly the
bypass-of-a-boundary-the-user-believes-in that the rationale in `SECURITY.md:51-54`
and the ROADMAP decisions log invoke to justify the validation. `open_worktree`
(tool 20) is a quieter second path to the same widening.

The same paragraph's next sentence ("That validation is a guardrail, not a sandbox")
is the honest framing; the sentence before it overstates. A fix needs to either
(a) drop the categorical claim and say the guardrail is advisory and cursor-widened,
or (b) freeze the allowed set at launch rather than re-deriving it from mutable
context fields.

**Severity note**: not a privilege escalation — the same agent has shell reach and
the navigation is visible to the user (`[mcp] navigated to …` flash). The finding
is that a security document asserts a property the code does not have.

### F2 — `SECURITY.md` says cargo-deny "runs on every CI build"; it has not since #273 — **medium**

`SECURITY.md:70-73`:

> **`cargo deny check`** runs on every CI build (advisories, licenses, sources,
> bans).

On HEAD, `ci.yml` does not invoke cargo-deny at all (`.github/workflows/ci.yml:57`
says so explicitly), `Makefile:39-45` documents that `deny` is deliberately out of
`check`, and `audit.yml` is the sole owner — running Mondays 06:00 UTC plus manual
dispatch (`.github/workflows/audit.yml:3-14, 24-28`). Introduced by `ba874c2`
`ci(deny): move the supply-chain gate off the commit path into audit.yml (#273)`,
inside this diff; `SECURITY.md` was last touched by `6abc6d7` (#236), earlier in
the same range, and was not updated.

`audit.yml:10-14` states the trade honestly ("a PR introducing a banned /
wrongly-licensed / duplicate dependency is not caught at PR time"). That sentence
belongs in `SECURITY.md`'s "known caveats", where a consumer reading the security
posture will find it. As written, the document overstates the supply-chain gate on
the axis a reader most cares about.

A fix must correct the "every CI build" claim and add the PR-time gap as a caveat.

### F3 — the git-env-hygiene guard accepts a 1-of-9 strip, and ten sites take it up on that — **medium**

`GIT_REDIRECT_ENV` (`src/git/test_support.rs:38-48`) names **nine** variables, and
the pre-commit hook unsets the same nine (`scripts/git-hooks/pre-commit:21-23`),
with both files carrying "keep the two in sync". The guard that enforces this on
test git spawns looks for exactly one of them:

```rust
// src/git/mod.rs:123
let strip = concat!("env_remove(", "\"GIT_DIR\")");
```

and its failure message offers a 1-var strip as a sufficient remedy
(`src/git/mod.rs:164`: "add `.env_remove(\"GIT_DIR\")`"). Ten hand-rolled spawn
sites strip only three (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`) instead of
routing through `git::test_support::git_command`:

- `src/merge_driver.rs:291-293`
- `src/mcp/config.rs:1094-1096`
- `src/app/harness_tests/per_column.rs:219-221, 294-296, 385-387`
- `src/app/state/tests/mod.rs:697-699, 770-772, 858-860, 937-939, 1130-1132`

The guard passes them all. The remaining six inherit: `GIT_OBJECT_DIRECTORY` and
`GIT_ALTERNATE_OBJECT_DIRECTORIES` in particular apply independently of `GIT_DIR`
and would send a scratch repo's writes into the developer's real object store —
the same failure family as the 2026-08-07 incident this guard was written for
(`src/git/mod.rs:109-118` cites it). This is the codebase's own stated anti-pattern:
a guard that passes while checking a fraction of what it claims.

A fix would make the guard require the full `GIT_REDIRECT_ENV` list (or, better,
require the spawn to go through `git_command` and exempt the stripping site), and
convert the ten sites.

### F4 — no staleness story for the installed pre-commit hook — **medium**

The `GIT_DIR` sanitization is present and complete-as-designed in the template
(`scripts/git-hooks/pre-commit:21-23`, all nine variables, `unset` before
`exec make check` so every tool the gate invokes is covered). Nothing tells a
contributor their **installed** copy predates it:

- `make install-hooks` (`Makefile:429-433`) is a bare `install -m 755`; no version
  stamp, no comparison, no re-check.
- `make check` does not compare `.git/hooks/pre-commit` against the template.
- No test does (`grep -rn "git-hooks\|hooks/pre-commit" src/ tests/` → nothing).
- `CONTRIBUTING.md` does not mention the hook at all.
- The only instruction is AGENTS.md's prose ("Re-run `make install-hooks` if your
  local `.git/hooks/pre-commit` predates this fix") — advice a human must read and
  act on, in a file whose whole point is that it is *long*.

Live evidence from this checkout: `diff .git/hooks/pre-commit scripts/git-hooks/pre-commit`
reports a difference on HEAD. It happens to be comment-only (the `unset` line is
byte-identical, so this machine is safe), but that is luck — the same drift with a
missing `unset` would be equally invisible. The failure mode being protected against
is silent branch reset plus index corruption, which is precisely the class that
needs a mechanical signal rather than a docs sentence.

A fix would add a staleness check somewhere on the commit path — the simplest being
a first line in the hook that compares itself to the tracked template and prints a
"re-run `make install-hooks`" warning, or a `make check` step that does so.

### F5 — `get_file_content`'s archive-member fallback ignores an explicit `root` — **low**

```rust
// src/mcp/protocol.rs:635-640
let member = read_member_content(&resolved, ctx_path).or_else(|| {
    let alt = read_cwd_from_context(ctx_path).join(path_str);
    (!Path::new(path_str).is_absolute() && alt != resolved)
        .then(|| read_member_content(&alt, ctx_path))
        .flatten()
});
```

The `.or_else` re-resolves the relative path against the context's `cwd` even when
the agent passed an explicit `root`. An agent working in a sibling worktree
(`root=<worktree>`, `path="src/main.rs"`) gets the *archive member* instead of the
worktree file whenever the user happens to be browsing a container that holds a
matching member — silently, with a success result and no indication which file it
read. `root` is the agent saying "not where the user is"; the fallback should be
suppressed when it is present.

### F6 — the archive-member read path is not bounded by `effective_root` — **low**

`read_member_content` (`readers.rs:210-218`) runs before the
`canonicalize` + `starts_with(canonical_root)` check (`protocol.rs:649-659`), and
is bounded only by "this path is inside a mount spyc has open". A mount can be
anywhere the user browsed to, including outside every allowed root. Traversal
*within* a mount is structurally prevented (`member_in_mount` → `archive::index::normalize`,
`readers.rs:186-188`, covered by `a_traversing_path_is_not_treated_as_a_member`,
`src/mcp/tests/mod.rs:1383`), so this is a scope question, not a traversal one, and
the exposure is limited to containers the user opened. Worth an explicit decision
(and a sentence in the doc) rather than an accident of ordering.

### F7 — new persistent state outside `src/state/` is outside the atomic-write guard — **low**

`state_writes_are_atomic` (`src/state/mod.rs:371-450`) scans `src/state/` only, for
the literal `fs::write(`, with an empty `ALLOWED` list. Checked every store the
orchestrator listed:

| Store | Location | Atomic? |
|---|---|---|
| `hook_owners` | `src/state/hook_owners.rs:46` | ✅ `write_atomic` |
| `hook_consent` | `src/state/hook_consent.rs:47` | ✅ |
| `skill_prompt` | `src/state/skill_prompt.rs:42` | ✅ |
| `scope_registry` | `src/state/scope_registry.rs` | no disk writes (rides sessions) |
| `transcript_images` | `src/state/transcript_images.rs` | read-only indexer |
| archive journal | `src/archive/journal.rs` | in-memory |
| archive staging | `src/app/archive.rs:1337-1350` | under `state_root()/archives`, file payloads |
| claude/codex/agy hooks | `src/mcp/hooks.rs:187, 314, 376, 479, 592, 660` | ✅ |
| `.mcp.json` / codex config | `src/mcp/config.rs:316, 480, 585, 670` | ✅ |

Two persistent writers landed since 2.0 that the guard cannot see:

1. **`src/skill/mod.rs:274` and `:280`** — the skill assets and the
   `.spyc-skill.json` manifest, both plain `std::fs::write`. `read_manifest`
   (`:218-229`) returns `None` on an unparseable manifest, which `status_in`
   (`:234-237`) turns into `Status::NotInstalled` — and a `NotInstalled` skill is
   overwritten unconditionally by `--install-skill` / `:skill update`
   (`install_in`'s doc comment, `:265-267`). A torn manifest write therefore
   converts "locally edited, never clobbered unprompted" into "clobbered".
   The write ordering compounds it: assets first, manifest last (`:269-284`), so a
   crash between them leaves file hashes disagreeing with an older manifest →
   `Status::Modified` → spyc reports the user's skill as hand-edited when it was
   spyc's own half-write.
2. **`src/mcp/server.rs:159`** — the trusted-root sidecar, plain `std::fs::write`.
   Pre-existing; a torn write reads back as an untrusted marker, which fails safe.

Neither violates the guard's *stated* scope ("where the writers of the XDG state
root live"), so this is a coverage gap rather than a violated rule. A fix would
either widen the scan or state the scope boundary where a future author will hit it.

Secondary: the guard greps one literal, `concat!("fs::write", "(")` (`:395`). A
state module that switched to `File::create` + `write_all`, `OpenOptions`, or
`serde_json::to_writer` would pass silently. `src/state/graveyard.rs:154` already
uses `File::create` for the `.tar.zst` payload — that one is a deliberate,
documented exception (metadata written last, `:198-202`; orphans reaped by the
health check), but it means the "empty `ALLOWED` is the healthy state" comment
(`:392`) is describing a narrower property than it reads as.

### F8 — `SECURITY.md`'s fuzzing caveat is stale in the safe direction — **low**

`SECURITY.md:178-184` says the fuzz targets "run on demand via `make fuzz`" and
"**Nothing runs them on a schedule**, so in practice they only execute when someone
remembers." `.github/workflows/fuzz.yml` did not exist at v2.0.0 and now runs them
weekly (Mondays 07:00 UTC, `fuzz.yml:18-21`), with persisted corpora — and its own
header points back at `SECURITY.md`'s threat model. Added by `328477b`
`ci(fuzz): run the fuzz targets weekly instead of never (review F5) (#240)`.

Understating is less harmful than F2's overstating, but a security document that is
wrong about its own CI in both directions is one a reader stops trusting.

### F9 — `SECURITY.md` calls the MCP socket "filesystem-default"; it is deliberately 0700 — **low**

`SECURITY.md:191-194`:

> **MCP socket permissions are filesystem-default.**

`start_socket_server` wraps the bind in `umask(0o077)` specifically so the socket
lands owner-only (`src/mcp/server.rs:397-402`, with a comment saying so), and
refuses to serve at all rather than fall back to a world-writable `/tmp`
(`:381-387`). The bullet's second and third sentences (same-user processes can
reach it; user-process isolation is the boundary) are correct. Only the headline is
wrong, and it happens to be the sentence a skimmer reads.

### F10 — no attribution on `report_status` / scope claims, and no paragraph about it — **informational**

Any process that can open the socket can set any pane's activity dot
(`report_status` accepts an arbitrary `pane_id`/`pane`, `protocol.rs:840-841`,
resolved by `resolve_report_target`, `src/app/mcp.rs:790-808`), register a scope on
another pane's behalf, and release any claim by id — the last of which is a
documented product decision ("No ownership check — a lead agent or the user may
clear a stale claim on someone's behalf", `protocol.rs:252`).

That is a coherent design for an advisory, single-user registry. The charter asked
whether an "attribution vs authorization" paragraph landed in `SECURITY.md`: it did
not (see premises). Stating it explicitly would cost one paragraph and would stop
the P2 registry from being read as an access-control mechanism by the next person
who builds on it.

### F11 — nothing structurally forces a *new* read tool to validate `root` — **informational**

`effective_root` is called by discipline at seven sites. `tools_list_response`
(`src/mcp/tests/mod.rs:122`) pins the tool count at 25, so adding a tool does force
a test edit — a useful nudge, but it asserts names, not validation. In a codebase
where the keymap tiers, the `:` command table, the module index, and the trap
anchors all have build-failing guards, the root-validation invariant is the one
riding on review attention. Given that #237 exists precisely because the invariant
was absent for a while, a guard (e.g. "every tool whose schema declares a `root`
property must reach `effective_root`") would be in keeping.

### F12 — `allowed_roots`' trusted root is spyc's launch directory, and every descendant of it — **informational**

The trusted anchor is `ctx_path.parent()` (`readers.rs:61-63`), and `ctx_path` is
`<start_dir>/.spyc-context-<pid>.json` (`src/app/bootstrap.rs:214` →
`src/context.rs:74-77`). `is_within_allowed` accepts any descendant
(`readers.rs:98-104`, `cand.starts_with(&root)`). Launch spyc from `$HOME` — an
ordinary thing to do with a file manager — and the entire home directory, `.ssh`
and `.aws` included, is an allowed root for every read tool. This is a consequence
of the design being about *scoping* rather than *confinement*, and it is consistent
with `SECURITY.md:56-59`; it is worth one sentence in the doc because it is the
difference between "validated against a set of roots" sounding tight and being
tight.

---

## Premises checked

- **"F1 root validation: still enforced … has it eroded?"** — Wrong framing. Root
  validation did not exist at v2.0.0: `effective_root` there checked only
  `p.is_dir()` and argued in its own doc comment that confinement "grants no new
  capability" (`git show v2.0.0:src/mcp/readers.rs`, lines 10-28). It was **added**
  by `d86dcb4` (#237) inside this diff. Nothing has eroded it since; every
  `root`-taking tool routes through it, including the tools the archive work
  touched.
- **"The cursor-independence invariant still holds?"** — Yes, and it is tested:
  `allowed_root_survives_the_user_browsing_elsewhere` (`src/mcp/readers.rs:982-1013`)
  puts every cursor field on an unrelated project and confirms the agent's own
  worktree is still accepted via the launch-dir anchor. The symlink case is also
  covered (`:1016-1038`). The corollary the invariant buys — that a *moving* cursor
  can only ever **widen** the set — is F1.
- **"Any tool added since (archive reads, the release batch)?"** — No tools were
  added. The count is 25 at v2.0.0 and 25 on HEAD. The archive work added a branch
  inside `get_file_content` and a refusal in the two search tools, not new surface.
- **"`state_writes_are_atomic` … has new state quietly landed outside the guard's
  coverage list?"** — The guard has no coverage *list*; it recursively scans
  `src/state/` with an empty `ALLOWED` (`src/state/mod.rs:392`) and passes on HEAD
  (verified: `cargo test --lib -- state_writes_are_atomic` → ok). Every new store in
  `src/state/` uses `write_atomic`. New non-atomic persistent writers landed
  *outside* `src/state/` — see F7.
- **"the attribution-vs-authorization paragraph if it landed"** — It did not land.
  `SECURITY.md` has no such paragraph; the closest text is the threat-model bullet
  about socket misuse by another local process (`:28-34`) and the
  not-a-privilege-boundary section (`:46-59`), neither of which mentions that tool
  arguments naming a pane or a claim are unauthenticated. See F10.
- **"F6: still one resolver? Any new code reading `$HOME` or `/tmp` directly?"** —
  One resolver holds: `mcp::state_dir()` delegates to `crate::state::state_root()`
  (`src/mcp/mod.rs:120-122`), tested by `mcp_state_dir_is_the_one_state_root`
  (`src/mcp/tests/mod.rs:678`), and the archive staging root added since 2.0 goes
  through it too (`src/app/archive.rs:1337-1339`). **No new** direct `$HOME`/`/tmp`
  reads landed in this diff. Pre-existing ones, all out of the diff and all
  non-state: `src/paths.rs` (tilde expansion — that *is* the path resolver),
  `src/config/mod.rs:1027`, `src/app/config.rs:61`, `src/app/bootstrap.rs:36`
  (cwd-inaccessible fallback), the agent transcript readers (`~/.claude`,
  `~/.codex`, `~/.gemini` — agent-owned paths), and `src/ui/syntax.rs:67-76`, which
  duplicates `state::config_root()`'s XDG logic for `syntaxes/`. That last one is a
  genuine second config-root resolver, but it predates 2.0 and is out of scope here.
- **"Hook template: … complete (all nine redirect variables)"** — The hook and
  `GIT_REDIRECT_ENV` list the same nine, byte-identical in order
  (`scripts/git-hooks/pre-commit:21-23` vs `src/git/test_support.rs:38-48`); no
  divergence between the two lists. The nine cover git's repo-location set. Two
  variables git exports into hooks are absent from both and are worth a look, though
  neither is a directory redirect: `GIT_CONFIG_PARAMETERS` (how `git -c k=v`
  propagates to subprocesses — a config-injection channel with the same
  "overrides what `-C` intended" shape) and `GIT_DISCOVERY_ACROSS_FILESYSTEM`.
  Flagging as a note rather than a finding; the nine cover the incident class.
- **"…matched by an installed-hook staleness story"** — There is no staleness
  story. See F4.

## Verified correct (spot-checks, no action)

- **`effective_root` rejects an out-of-session root and names the allowed set** —
  `readers.rs:26-38`; tested for `/` and for an unrelated real directory
  (`readers.rs:946-979`). The error text is actionable, which matters: a bare
  refusal sends the agent to unscoped `Bash rg`, which is bypass rather than safety.
- **`get_file_content` traversal defence** — `canonicalize` then
  `starts_with(canonical_root)` (`protocol.rs:649-659`), tested end-to-end through
  `dispatch` with `../secret.txt` (`src/mcp/tests/mod.rs:874-942`).
- **Archive member normalization** — `member_in_mount` runs every member name
  through `archive::index::normalize` before joining staging (`readers.rs:179-194`),
  so a crafted member path cannot escape the mount; longest-mount-wins handles
  nesting. Tested (`src/mcp/tests/mod.rs:1383`).
- **`read_lsp_message` bounds an untrusted `Content-Length`** — 64 MiB cap before
  `vec![0u8; len]` (`protocol.rs:1038-1084`), tested with a 64 GiB header
  (`:1151-1160`). Unframed JSON is reported as `InvalidData` rather than dropped
  silently, and the socket server surfaces it to the TUI (`server.rs:496-513`).
- **Socket posture** — `umask(0o077)` around the bind, restored immediately
  (`server.rs:399-401`); refuses to serve rather than fall back to `/tmp`
  (`:381-387`); stale-socket pruning only on `ConnectionRefused`/`NotFound`, never
  on transient `EAGAIN`/`EMFILE` (`server.rs:62-80`); accept-error backoff instead
  of a busy loop (`:421-431`).
- **Planted-marker defence** — discovery requires an owner-private
  `mcp-<pid>.root` sidecar whose recorded root canonically matches the marker's
  directory, and a missing sidecar fails closed (`server.rs:106-146`). Tested from
  both directions (`collect_pids_rejects_planted_marker_rooted_elsewhere`,
  `collect_pids_requires_a_trusted_root_sidecar`, `src/mcp/tests/mod.rs:834, 852`).
- **`open_state_file`** — `O_NOFOLLOW` + `0600` in the XDG state dir
  (`src/state/mod.rs:110-122`), with a test that a planted symlink is refused and
  the victim file untouched (`:596-608`). `mcp.log` rides this rather than the old
  `/tmp/spyc-mcp.log` (`src/mcp/mod.rs:84-92`), and body logging is opt-in behind
  `SPYC_MCP_DEBUG` precisely because a `get_file_content` response is a whole file
  (`:94-100`).
- **`viewer_scratch_path`** — `SPYC-TRAP(viewer-temp-symlink)`, `tempfile`-random
  dir narrowed to `0700` (`src/app/image_ops.rs:347-385`). The right treatment of
  the shared-`/tmp` symlink class, and worth citing as the pattern F7's skill writer
  is missing.
- **`production_half`** — the shared splitter is correct on both documented
  failure shapes and biases to false positives where it cannot brace-match
  (`src/guard_support.rs:36-85`, four tests). Both consumers use it
  (`src/state/mod.rs:429`, `src/git/mod.rs:86`); the fail-open `split("#[cfg(test)]")`
  shortcut appears nowhere.
- **Status-hook consent** — installation is gated on a persisted per-project
  `[Y/n]` (`src/state/hook_consent.rs`, checked at `src/app/status_hooks.rs:69`), a
  git-tracked `settings.json` is never written (`src/mcp/hooks.rs:160-162`), an
  unparseable existing config is refused rather than replaced with an empty
  document (`hooks.rs:190-200` and the `*_refuses_to_overwrite_invalid_*` tests),
  and the hook command is shell-quoted (`hooks.rs:117-120`). Teardown is refcounted
  through `hook_owners` so one instance's exit cannot blind another's
  (`src/state/hook_owners.rs`, three tests).
- **Read-tool timeouts** — `READ_TOOL_TIMEOUT` is derived from `PROXY_IO_TIMEOUT`
  so the two cannot drift, with a test asserting the ordering
  (`protocol.rs:27-34, 1183-1186`); `wait_for_scope_clear` is capped at 10 minutes
  and its socket-side reply timeout is derived from the same bound
  (`protocol.rs:40-41, 1001-1007`).

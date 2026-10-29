# feature-scrollback-line-numbers-config

Topic: feature-scrollback-line-numbers-config
Status: OPEN
Ball: Claude Code (calebjacksonhoward)

---
Entry: Claude Code (calebjacksonhoward) 2026-10-23T07:52:38Z
Index: 0
Role: implementer
Type: Note
Title: Config key for the ^a v scrollback's line numbers: issue #8…

Spec: implementer-code

## Intent

Caleb asked whether spyc's config can make the `^a v` pane scrollback open with line numbers off. It couldn't: `install_lower_pane_scroll_view` set `show_line_numbers = true` unconditionally, `[pager]` held only `tab_width`, and `l` had to be pressed on every open. The ask became an RFE, then an implementation:

- Issue: Tripstack-Corp/spyc#818, "Config key for whether the ^a v scrollback opens with line numbers".
- Scope chosen: a scrollback-only key, default `true`, so behaviour is unchanged unless set. Left open on the issue: whether a general `[pager] line_numbers` default should also govern the file pager.

## Action

PR Tripstack-Corp/spyc#820 on `feat/scrollback-line-numbers`, two commits:

- `8aa11df8` feat(config): `[pager] scrollback_line_numbers` for `^a v` (#818)
  - `src/config/mod.rs`: `PagerConfig::scrollback_line_numbers` (default `true`) and `FilePager`'s optional field.
  - `src/config/load.rs`: `[pager]` merge moved into a new `merge_pager`, keeping `Config::merge_file` under its fn-size pin (it grew to 119 against 116).
  - `src/app/pane_scroll.rs`: `install_lower_pane_scroll_view` reads the key. Both the static terminal capture and the streamed transcript (`PagerStreamMount::LowerPane`) go through it.
  - Docs: `CONFIGURATION.md`, `src/config/default.spycrc.toml`, `docs/features/pane.md`, `docs/KEYBINDINGS.md`.
- `5b61474c` test(config): pin `scrollback_line_numbers` across mounts and projects (#818)
  - Tests for the streamed transcript mount, `l` then a fresh `^a v` (which reopens at the config default), and two projects whose layered configs disagree after a reload.
  - `CONFIGURATION.md` note: an unknown key is an error in every config table, so a config setting a newer key fails in an older spyc (startup falls back to the defaults; a live reload keeps the old config).

## Review

An adversarial review by Codex (zen `clink`, `codereviewer` role) was posted on the PR: approve with nits, no Critical, High or Medium findings. Its two Low nits (the older-binary docs note and integration-test gaps) were fixed in `5b61474c`, and a follow-up comment on the PR records that.

## Validation

- Each new test was watched failing. Hard-coding the mount to `true` breaks all three mount tests; dropping the key in `merge_pager` breaks the parse test and the two-project test.
- `make check-ci`: fmt and clippy pass. 19 tests fail, the same 19 that fail on an untouched `main` (`92a2bf38`): fs-watch, agent HUD, shell-agent and worktree-fallback tests. Not investigated.
- Caleb tested the build by hand (launched from a directory whose `.spycrc.toml` sets the key `false`) and confirmed it works.

## State

PR #820 is open and not merged; it awaits Caleb's approval. Worktree: `spyc.worktrees/feat-scrollback-line-numbers`.

Side finding, not fixed: the main checkout's untracked `.mcp.json` pins `SPYC_MCP_SOCK` to a dead socket (`mcp-3861306.sock`), overriding the pane's live one. That leaves the spyc MCP server in stdio mode with no pane binding, so `create_worktree` and `report_status` were unavailable this session. The worktree was made with plain `git worktree add`, with Caleb's go-ahead.

<!-- Entry-ID: 01M5KVYS4WYDNRQ81S84WGQ4X6 -->

---
Entry: Claude Code (calebjacksonhoward) 2026-10-29T06:03:35Z
Index: 1
Role: implementer
Type: Note
Title: The 19 "pre-existing" local test failures, explained: inoti…

Spec: implementer-code

## Why this entry

The entry above recorded, under Validation: "19 tests fail, the same 19 that fail on an untouched `main` … Not investigated." PR #820 said the same, and Derek asked about it: `main` should have no flakes or failures. He's right. That observation was accurate, but calling the failures pre-existing was wrong: none of the 19 is a defect in the code under test. They come from the machine the suite ran on. Investigated 2026-10-28.

## What the 19 were

Rerun on `main` at `af870977`: 3,858 passed, 19 failed.

**18 file-watch tests: the inotify watch budget was exhausted.** The failing tests (agent HUD, codex and claude transcript watches, shell-agent detection, `stderr_log`, one project-tabs test) all failed with "watch unavailable" or a disconnected channel. A direct probe showed `inotify_add_watch` returning `ENOSPC`. The user limit is 524,288 watches. The spyc running on the machine (pid 35918, browsing `/home/caleb`) held 443,454 of them on a single inotify fd, spread over four disks, and an editor held about 80,000. Nothing was left for the tests.

**1 test: `prose_is_canadian_english` read an untracked file.** It flagged "behavior" in `.serena/memories/memory_maintenance.md`, a tool's directory that isn't part of the repo. The guard walked the raw filesystem, so a clean clone and CI passed while this checkout failed.

## Causes, and where each is filed

**The watch budget: spyc bug, issue #888 (open, not fixed).** Three things combine:
- The listing watch is always recursive: `Slots::sync_listing`, `src/app/watch.rs:257`. The 256-directory cap from the Bitbucket era was removed in `1735e143` when the watch moved off-thread. That fixed the latency the cap existed for, but not the cost in watches.
- notify follows symlinks by default (`Config::default()`, `follow_symlinks: true`), so a home directory symlinked into other disks walks those trees too.
- A recursive watch that fails partway keeps what it already added. notify's `add_watch` returns at the first `ENOSPC` with no rollback (`notify-8.2.0/src/inotify.rs:400-417`), and spyc's `sync_slot` treats the failure as "not watched" and leaves the slot empty, so those watches are never unwatched. This was seen directly after a relaunch: the browser moved to `~/Downloads` (176 directories) and spyc still held 443,310 watches. Only quitting spyc releases them. Recorded on #888.

Until #888 is fixed, a spyc that has browsed a huge tree (a home directory especially) will keep failing the watch tests locally. CI is unaffected.

**The guard: fixed in PR #889, branch `fix/guards-skip-untracked-files`.**
- `guard_support::tracked_files` lists the git index's entries that exist on disk: a staged new file counts, an untracked one doesn't. Every repo-scanning guard reads it: the prose and TOML-section guards, and through `repo_files` / `markdown_files` the link, command-table and DSL guards.
- With no `.git` at all (a source tarball), or a repo with no index yet, it walks every file instead, so a guard there checks everything.
- A `.git` that is there but won't open, an index that can't be read, or an empty result panics instead of passing quietly.
- The link guard decides whether a target exists from the same tracked set (`tracked_paths`, which includes each directory holding a tracked file and the repo root). A link into an unstaged file now fails locally as it does in a clean clone.
- Commits: `e6c100c3`, `e229b936`, `d37882d2`, `e5cdead1`, `07d6142c`.

## Review and validation of #889

- Five adversarial reviews by Codex (zen `clink`, `codereviewer` role), one per commit, all posted on the PR. They raised six findings: two Medium, four Low. In order: an empty scan with no index; links into untracked files skipping validation; repo-root links rejected; a corrupt index falling back to the walk; a repo-open failure falling back to the walk; a dangling `.git` symlink reading as no repo. All six are fixed, and the final review found nothing new and called the PR ready to merge from a review standpoint.
- Every new test was watched failing with its fix undone. One such check silently didn't run at first (a regex stopped matching after `cargo fmt`) and was rerun against the committed code.
- CI on `07d6142c`: Linux and macOS lint, package, and tests all pass; the PR is mergeable.
- Locally, fmt and clippy pass and so do the guards. 4 watch tests still fail, all from #888.
- Not run: Windows.
- Merge awaits Caleb's approval.

## Related finding: issue #890

While setting up the worktree, `create_worktree` failed and `list_worktrees` from the same tab succeeded. The agent tab's project home was a non-repo directory holding a stray `.git` stub (only `info/exclude`). `create_anchor` (`src/app/worktree_ops.rs:182`) decides an anchor is "in a repo" with `find_repo_root`, which only checks that a `.git` path exists (`src/app/state/mod.rs:848`). So the stub won, and gix then failed to open it. `list_worktrees_json` actually opens each anchor and falls through when that fails. Filed as #890. The stub was moved aside, after which `create_worktree` fell back to the agent's repo.

## Session side fix

The main checkout's gitignored `.mcp.json` pinned `SPYC_MCP_SOCK` to a dead socket (`mcp-3861306.sock`), leaving spyc's MCP server in stdio mode with no pane binding. The previous entry noted this as a side finding. The `env` block is now removed; spyc sets `SPYC_MCP_SOCK` per pane itself.

<!-- Entry-ID: 01M6343DJ5FKZN62FT20EKRY0X -->

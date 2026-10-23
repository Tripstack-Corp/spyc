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

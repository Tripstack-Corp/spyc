# spyc TODO

Operational checklist. Strategy, thesis, and rationale live in
`ROADMAP.md`. Items move to `ROADMAP.md`'s "Done (recent)" section
as they ship — not to a `Done` section here.

**Sizes:** `[S]` hours · `[M]` a day or two · `[L]` a week or more.

## Now — Foundations (v1.x)
- [ ] better document the code base and structure of the UI
  i.e. the design language of the UI and the names of the components
  and the philosophy so that as extensions continue to be made they
  fit a general design plan

### Safety and correctness

- [x] **[S] Install panic hook in `main()`.**
  Restores terminal (`restore_terminal()`) before unwinding. Writes
  backtrace to the debug log. Non-negotiable before any external
  release.
  *Done when:* `panic!()` from any code path exits cleanly without
  leaving the terminal in raw mode or alt screen.

- [x] **[S] Fix CI Rust version mismatch.**
  `bitbucket-pipelines.yml` uses `rust:1.80-slim`; `rust-toolchain.toml`
  pins `1.85`. Bump the image. Verify a clean-cache build passes.

- [x] **[M] Unicode width in list view.**
  *Done:* `unicode-width` crate, `display_width()`/`display_truncate()`
  helpers in `ui/mod.rs`. All UI width sites fixed: list_view, status
  bar (powerline + mono), help, pager, truncate_middle().

- [x] **[S] `cargo-audit` in CI quality gate.**
  Fail on advisories. RUSTSEC-2026-0009 (time 0.3.45) ignored — fix
  needs Rust 1.88, not exploitable in our dep chain.

- [x] **[S] `cargo-llvm-cov` in CI with ratcheting floor.**
  Baseline: 38% line coverage. Floor set at 35%. Runs in parallel
  with quality gate on main and PRs.

### Testing

Priority order below; each item is a meaningful unit of work on its
own. The handler-extraction refactor is the prerequisite for the
rest of the dispatch testing.

- [x] **[L] `app.rs` handler extraction refactor.**
  Split handlers from the live `&mut Tui` surface. Each becomes a
  pure-ish `(state, event) -> (state', PostAction)`. Start with
  `handle_prompt_key`.
  *Done:* `AppState` extracted with 87 tests. Phases 0–4 complete.

- [x] **[M] Keymap resolver test module.**
  77 tests covering count accumulation, all pending-seq states,
  ctrl codes, user keymap override, pending display strings.

- [x] **[M] State module tests.**
  picks (6), inventory (7), cursor (5), ignore (11), history (14),
  sessions (11). All core invariants pinned.

- [x] **[S] DSL → resolver round-trip tests.**
  5 tests: DSL parse → UserKeymap → Resolver → Action/BoundAction.
  Inline in `config/mod.rs` + contract tests in
  `tests/keymap_roundtrip.rs`.

- [x] **[M] `tests/` integration directory.**
  `tests/filesystem.rs` (6 tests, tempdir trees) and
  `tests/keymap_roundtrip.rs` (5 TOML grammar contract tests).

- [ ] **[M] Snapshot tests on widgets.**
  ~~Add `insta` dev-dep.~~ Done. Status bar snapshots (4) done.
  Remaining: `list_view`, `pager` (ANSI, hex, line numbers, search
  highlight), `line_edit` modes.

- [ ] **[L] One pty integration test.**
  `tests/pane_roundtrip.rs`. Spawn `cat` via `portable-pty`, write
  bytes, parse `vt100::Screen`, assert rendered output.
  `#[cfg(unix)]`. One test, not a suite.

- [ ] **[S] Property tests (narrow).**
  `proptest!` blocks for: shell-arg quoting round-trip
  (`shell/expand.rs`); limit-filter glob matching; resolver count
  invariants. One block per site.

### Release hygiene

- [x] **[S] `CHANGELOG.md` in Keep-a-Changelog format.**
  *Done:* Seeded with entries for v0.11.0 through v1.5.0.

- [x] **[S] `spyc --version --verbose`.**
  *Done:* `build.rs` embeds git SHA, build timestamp, rustc version.
  `--version --verbose` dumps version, git, built, rustc, TERM,
  COLORTERM, os/arch.

- [x] **[S] Panic backtraces in debug log.**
  Wire the panic hook to dump `RUST_BACKTRACE=full` to the debug log.
  *Done:* `main.rs` does `Backtrace::force_capture()` → `debug_log::log()`.

- [ ] **[S] `spyc --dump-default-config`.**
  Prints the full default `.spycrc.toml` with comments. Self-doc
  for the keymap DSL + user starting point.

### Hardening

- [ ] **[S] Startup health check on state directory.**
  Scan `~/.local/state/spyc/` on launch for unexpected types (e.g.
  flat file where a directory is expected, corrupt JSON, orphaned
  `.dat` without `.json`). Flash a warning and offer to fix.
  Defensive against version upgrades that change storage format.

- [ ] **[L] Background directory loading.**
  Async listing above a threshold (~10K entries). Cancellable
  progress indicator; synchronous path stays for the common case.
  *Done when:* opening `node_modules` or a Druid segment cache
  doesn't block the event loop.

## Next — v2.0 gate

Gates the public distribution push. Thesis items first (the reason
the tool exists), then distribution.

### Thesis — deepening the agent integration

- [x] **[M] Bidirectional path references — `gf` / `gF`.**
  *Done:* M13, v1.4.0. Path extraction with 35 tests, dual cwd
  resolution, scroll mode support.

- [x] **[L] Automatic context handoff.**
  *Done:* M14, v1.5.0. HTTP MCP server on background thread,
  `get_spyc_context` tool, `--mcp-config` injection at pane spawn.

- [ ] **[M] Session forking — `^W f`.**
  Duplicate pane tab with scrollback replayed. Old roadmap item,
  more valuable after the two items above land.

- [x] **[M] Conversation-aware session restore.**
  *Done:* Session save captures Claude session ID + name. Restore
  spawns `claude --resume <sessionId>`. Picker shows name + ID.

- [ ] **[M] Prompt templates in `.spycrc.toml`.**
  User-defined macros that send pre-composed prompts to the pane
  with picks/inventory substituted. Extends existing keymap DSL.

- [ ] **[S] Status bar agent segment.**
  Indicator when the pane is running Claude: session identity,
  token usage if surfaced by the CLI.

### Distribution

- [ ] **[L] Release automation.**
  Bitbucket Pipelines on `v[0-9]+.*` tag. Cross-compile matrix
  (Linux x86_64/aarch64 musl, macOS universal). Upload artifacts.
  Generate release notes from `CHANGELOG.md`. Bump Homebrew
  formula. Publish to crates.io. One tag push = full release.

- [ ] **[M] macOS code signing + notarization.**
  TripStack Developer ID cert. `codesign --deep --sign`,
  `xcrun notarytool submit`, stapled.

- [ ] **[S] Linux signing with minisign.**
  Public key in repo and release notes. Signature alongside every
  binary.

- [ ] **[M] SBOM generation at release.**
  `cargo-auditable` embeds build metadata; `cargo-sbom` emits
  SPDX/CycloneDX alongside artifacts.

- [ ] **[M] Reproducible build verification.**
  `SOURCE_DATE_EPOCH` honored. Second CI job rebuilds from tag,
  diffs against released artifact, fails if non-reproducible.

- [ ] **[S] `cargo publish` to crates.io.**
  Binary-only crate. Acceptable for this tier.

- [ ] **[M] Homebrew tap — `tripstack/homebrew-spyc`.**
  Formula auto-bumped by the release pipeline.

- [ ] **[S] AUR package — `spyc-bin`.**
  Submit once. Updates driven by the release pipeline.

- [ ] **[S] GitHub mirror.**
  Read-only `github.com/tripstack/spyc`, synced on every push to
  Bitbucket `main`.

- [ ] **[M] Docs site via `mdbook`.**
  Getting started, keymap reference, `.spycrc.toml` DSL, agent
  workflow guide. Auto-built to Pages on tag.

- [ ] **[M] README rewrite.**
  First paragraph sells the thesis. Install above features.
  One ≤ 90s asciinema embedded. Link to docs site.

- [ ] **[S] `SECURITY.md`.**
  Vulnerability disclosure process. Clear with TripStack
  legal/security before publishing externally.

- [ ] **[S] `CODE_OF_CONDUCT.md`.**
  Contributor Covenant. Link only.

- [ ] **[S] PR and issue templates.**
  `.bitbucket/` and mirrored `.github/`. Bug report, feature
  request, PR checklist.

- [ ] **[S] Shell completions.**
  `spyc --generate-completion {bash,zsh,fish}`. `clap` derive
  makes this trivial; ship in release artifacts.

## Later — post-v2.0

Graduate into Now/Next sections when picked up. Sizes TBD until
scoped.

- [ ] Drag and drop from desktop (OSC 52 / path paste).
- [ ] Page scroll overlap in pager.
- [ ] Auto-scroll reading mode.
- [ ] Jump-back in pager (`''`).
- [ ] Macro recording (`qa` … `q` … `@a`).
- [ ] Startup/exit command flags (`-c`, `-F`).
- [ ] Stdout on exit for shell-pipeline composability.
- [ ] Conditional status bar expandos.
- [ ] Per-file tags/metadata.
- [ ] Autocommands, scoped to agent workflow.

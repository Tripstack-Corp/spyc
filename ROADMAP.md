# spyc roadmap

## Thesis

spyc is a vi-keyboard-driven file commander that pairs with an AI coding
agent in a single terminal window. The target user is a developer who
already thinks in vi motions and wants Claude Code living in the same
workspace — not one window over, not in a browser tab, in the same
session, sharing context.

That positioning is the reason the tool exists. Every other feature —
picks, inventory, pager, status bar, sessions — is supporting
infrastructure that makes the split-pane workflow fast and comfortable.
The roadmap is organized accordingly: the pane-and-agent integration is
the defining work track, not the trailing milestone.

## Working tracks

Work proceeds along three parallel tracks. They're not strictly
sequential; distribution work can land while thesis work is still in
flight, and foundations work continues throughout.

- **Foundations** — testing, hardening, build hygiene. The minimum to
  not embarrass ourselves and to make every other change safer.
- **Thesis** — deepening the agent integration until the split-pane
  workflow is measurably better than `tmux` + `claude` for the target
  audience. This is where the tool earns its reason for being.
- **Distribution** — release automation, signing, packaging, docs.
  Turns a repo into a tool people can install, trust, and find.

Each track has its own priority ordering below. Specific items migrate
from these lists into `Done (recent)` as they ship.

## Foundations

Most of the critical foundations work has shipped. Remaining items are
listed in priority order; the testing infrastructure and CI pipeline
are now solid enough to support thesis feature development.

### Done

- ~~Panic hook that restores the terminal~~ — shipped. Restores raw
  mode + alt screen on panic, writes backtrace to debug log.
- ~~CI fixes~~ — `rust:1.85-slim` matches MSRV, `cargo-audit` in
  pipeline (RUSTSEC-2026-0009 ignored — needs Rust 1.88),
  `cargo-llvm-cov` with 35% ratcheting floor.
- ~~Testing strategy execution~~ — 311 tests (up from 74). Keymap
  resolver (77 tests), state modules (picks/inventory/cursor/ignore/
  history/sessions), DSL→resolver round-trips, `tests/` integration
  directory, snapshot tests via `insta` + `TestBackend`, handler
  extraction (Phases 0–4 complete: `AppState` with domain logic
  cleanly separated from terminal state).
- ~~71 clippy errors fixed~~ — clean `cargo clippy -D warnings` build.

### Remaining

- **Unicode width in the list view.** `chars().count()` is used as a
  width proxy in places; CJK filenames, flags, and family emoji will
  misalign columns. Bring in `unicode-width` and route width
  calculations through it.
- **CHANGELOG.md** in Keep-a-Changelog format. Every user-visible
  change gets an entry; the release pipeline reads from it for release
  notes.
- **`spyc --version --verbose`** dumps version, git SHA (via
  `build.rs`), build timestamp, rustc version, terminal detection,
  and active feature flags. First line of every bug triage.
- **`spyc --dump-default-config`** — complete `.spycrc.toml` with
  comments. Self-documentation for the keymap DSL and a user starting
  point.
- **Handler extraction Phases 5–6** (deferred). The pager handler
  (~500 lines) is deeply coupled to `PagerView` widget state;
  extracting it cleanly needs a `PagerState` restructuring, best done
  when we're actively modifying the pager. `handle_key` thinning is
  lower ROI — mostly wiring. Both can land alongside thesis features
  that touch those handlers.
- **Expand snapshot tests.** `insta` + `TestBackend` infra is wired.
  Status bar snapshots done (4). Remaining: `list_view`, `pager`
  (ANSI, hex, line numbers, search highlight), `line_edit` modes.
  Incremental — add as widgets are touched.
- **One pty integration test.** Spawn `cat` via `portable-pty`, write
  bytes, parse `vt100::Screen`, assert rendered output. `#[cfg(unix)]`.
  One test, not a suite.
- **Property tests (narrow).** `proptest!` blocks for: shell-arg
  quoting round-trip, limit-filter glob matching, resolver count
  invariants. One block per site.
- **Background directory loading.** Large directories (100K+ entries)
  block the event loop. Async listing with a cancellable progress
  indicator. Scoped conservatively — the common case stays synchronous.

## Thesis — deepening the agent integration

The pty pane is already the core of the tool (M8–M12 done). The work
that remains is making the integration genuinely novel, not just "a
terminal inside a terminal." In priority order:

- **Bidirectional path references.** When Claude outputs
  `src/app.rs:1284` in the pane, spyc should recognize the path and
  support a vi-style `gf`-equivalent to jump the file list there (and
  optionally open the pager to that line). Claude produces path
  references constantly; making them navigable closes the most
  valuable workflow loop. Probably the single highest-leverage
  feature remaining.
- **Automatic context handoff.** Picks and inventory are already
  pipeable to the pane via `^W p` / `^W i` (M10). The next step is
  passive: spyc maintains a context file Claude can watch (e.g.,
  `.claude/spyc-context.md`) that reflects the current directory,
  picks, inventory, and active filter. When Claude is asked "what am
  I looking at," it already knows. Wire via a small MCP server if the
  file-watcher approach gets noisy.
- **Session forking** (already in old roadmap as `^W f`). Duplicate a
  pane tab with scrollback replayed, so a Claude conversation can
  branch without losing the prior line of inquiry. High-value for
  "let me try a different approach." Implementable with current
  plumbing.
- **Conversation-aware session restore.** `--resume` already restores
  cwd, tabs, and pane geometry. Extend to capture the agent session
  identity (Claude Code's `--resume` surface) so restoring a spyc
  session restores the conversation, not just a fresh pane pointed at
  the right directory.
- **Prompt templates in `.spycrc.toml`.** User-defined macros that
  send a pre-composed prompt to the pane with picks/inventory
  substituted in — e.g., `map "<space>cr" claude-template review`
  where `review` is defined in config. Turns spyc into a
  keyboard-driven Claude launcher for repeated workflows.
- **Status bar agent segment.** When the pane is running Claude, show
  a small indicator: session identity, maybe token usage if the CLI
  surface exposes it. Useful, not essential.
- **Autocommands** per the old roadmap, but scoped to the agent
  workflow — `autocmd "*.test.ts" "claude-template test-review"`
  etc. Defer until the template feature lands and the shape is
  clear.

## Distribution

Most of this is one-time setup work. Worth doing properly and then
forgetting about.

- **Release automation in Bitbucket Pipelines.** Tag push
  (`v[0-9]+.*`) triggers: cross-compile matrix (Linux x86_64/aarch64
  musl, macOS universal), build artifacts uploaded to a release
  bucket, release notes generated from CHANGELOG.md, Homebrew tap
  formula bumped, crates.io publish. Zero-manual-step release.
- **macOS code signing and notarization.** Developer ID certificate,
  `codesign --deep --sign`, `xcrun notarytool submit`, stapled.
  Without this, Gatekeeper blocks the binary on fresh macOS
  installations and the first user report will be "it says spyc is
  damaged."
- **Linux signing.** Minisign or cosign on release artifacts,
  public key committed to the repo and published in release notes.
  Cheaper than Sigstore/SLSA attestation and sufficient for this
  audience.
- **Reproducible build verification.** Lock the toolchain
  (`rust-toolchain.toml` already pins), `SOURCE_DATE_EPOCH` honored,
  `cargo-auditable` to embed build metadata. A second CI job rebuilds
  from the tag and diffs against the released artifact. Not strictly
  required but cheap insurance and a nice signal.
- **SBOM.** `cargo-sbom` or `cargo-auditable` generates SPDX/CycloneDX
  at release time. Uploaded alongside binaries.
- **Package registries.** `cargo publish` to crates.io (binary-only
  crate, acceptable). Homebrew tap at `tripstack/homebrew-spyc`.
  Arch via AUR `spyc-bin`. Skip nixpkgs, Debian, Fedora packaging
  unless volunteers emerge — not worth the maintenance tail for this
  tier.
- **GitHub mirror.** Read-only mirror at `github.com/tripstack/spyc`,
  synced from Bitbucket on every push. Bitbucket's public-repo
  discoverability is worse than GitHub's, and the target audience
  expects a GitHub URL. Mirror, don't migrate.
- **Docs site.** `mdbook` rendered to Bitbucket/GitHub Pages.
  Getting started, keymap reference, `.spycrc.toml` DSL reference,
  agent workflow guide. Auto-built from the `docs/` directory on
  release.
- **README rewrite.** Current README buries the thesis. First
  paragraph should sell the split-pane agent workflow. One asciinema
  cast embedded — 90 seconds, nothing more. Install instructions
  above feature list.
- **Repo hygiene.** `SECURITY.md` (how to report vulnerabilities),
  `CODE_OF_CONDUCT.md` (one of the standard ones, link only),
  PR template, issue templates for bug reports and feature
  requests. Low effort, sets the tone.
- **`spyc --generate-completion {bash,zsh,fish}`.** Shell
  completions for the (small) CLI surface. Trivial with `clap`
  derive, worth it for the polish signal.

## Non-goals

These are things someone will inevitably ask for. The answer is no, and
the roadmap committing to that saves a lot of drift.

- **Native Windows support.** WSL is the supported story. `portable-pty`
  technically works on Windows but debugging the failure modes is a tax
  we're not paying.
- **Plugin system.** A decade of maintenance debt for a feature 3% of
  users will touch. The `.spycrc` DSL and keymap extensibility are the
  customization surface.
- **Localization.** English only. The target audience reads English
  docs.
- **Telemetry.** Not even anonymized opt-in. The greybeard half of
  the audience will not forgive it and the vibe-coder half won't
  notice it's missing.
- **SLSA L3 / supply-chain theatre.** Minisign signatures + SBOM + a
  reproducible build job are proportionate. Full SLSA attestation
  infrastructure is not.
- **Mouse support beyond what already exists.** Old roadmap mentions
  it; deprioritize indefinitely. The tool is keyboard-first by
  thesis.

## Releases

Semver per `CONTRIBUTING.md`. Version bumps in `Cargo.toml` as part of
the PR that ships the change. The `CHANGELOG.md` entry lands in the
same commit.

- **v1.x** — Foundations track. Ship incrementally; no external
  announcement. Internal dogfooding feedback loop.
- **v2.0** — Public distribution launch. Gated on: thesis-track items
  #1 (bidirectional path refs) and #2 (automatic context handoff)
  shipped, all Foundations track items complete, Distribution track
  complete. External announcement: TripStack engineering blog post,
  optional Show HN. This is the "credibly public" moment.
- **v2.x onward** — Remaining thesis items, feature work from
  `Additional Ideas` section, community-driven contributions.

The v2.0 version bump is a signaling choice as much as a semver one.
The tool has been shipping 1.x for a while, but the repositioning +
public distribution justifies a major bump to mark the transition.

## Additional Ideas

Lower-priority items retained from the prior roadmap. Will graduate to
one of the tracks above when picked up.

- **Drag and drop** — files from the desktop into spyc via OSC 52 or
  path paste.
- **Page scroll overlap** in the pager — keep 2–3 lines of previous
  page visible (`_scroll_skip_page_fraction`).
- **Auto-scroll reading mode** — continuous scroll at configurable
  speed for hands-free reading.
- **Jump-back in pager** (`''`) — return to the pre-search/jump
  position, matching the file-list behavior.
- **Macro recording** (`qa` … `q` … `@a`) — vim-style action
  recording and replay.
- **Startup/exit command flags** — `spyc -c "sort mtime"` runs
  commands at launch, `-F` for exit hooks.
- **Stdout on exit** — emit picks/inventory paths on quit so spyc
  composes with shell pipelines (`spyc | xargs rm`).
- **Conditional status bar expandos** — `%?git?%branch?` shows a
  segment only when its condition holds. Requires a format-string
  parser; worth it only if the status bar gains more segments.
- **Per-file tags/metadata** — key-value pairs attached to files,
  usable in filters and autocommands.

## Done (recent)

- **Foundations overhaul** — 311 tests (from 74), 38% line coverage,
  clean `cargo clippy -D warnings`, panic hook, `cargo-audit` +
  `cargo-llvm-cov` in CI, `AppState` domain-layer extraction
  (Phases 0–4).
- **`:` command extensions** — `:cd`, `:sort` (name/size/mtime/ext),
  `:marks`, `:set key=value`, `:bprev`/`:bnext` buffer history.
- **Pager buffer history** — closed pagers saved to a back/forward stack
  (max 10). Navigate with `:bprev`/`:bnext` or `[b`/`]b` in pager.
- **`:` command line** — vim-style command prompt with `:limit`, `:!cmd`,
  `:!!`, `:;cmd`, `:q`. Vi line editor with history.
- **`=` limit filter** — temporary glob filtering (`=*.rs`, `=!` for
  picks only, `=` clears). Status bar indicator, auto-clears on chdir.
- **`!?` history editor** — vi-editable popup with `/search`, `n`/`N`
  match navigation, `:N` jump, `G`/`gg`, `Ctrl+D` delete, instant
  trigger from `!` prompt, deduped history on load.
- **Numeric prefix display** — typing `3j` shows "3" in the prompt area.
- **`:N` jump-to-line** in pager and history editor.
- **Pager repaint fix** — force full repaint on pager open when pane is
  active, preventing stale PTY cells from bleeding through.
- Syntax highlighting in pager via syntect (base16-eighties.dark theme,
  hundreds of languages, auto-detected from file extension)
- Streaming pager for `!` commands — output streams live with hourglass
  timer, stderr merged into stdout, auto-scroll to bottom
- Session save/restore (`--resume`) — auto-save on quit, picker UI with
  j/k navigation, human-readable timestamps, dedup by cwd+tabs
- Separate pane command history with move-to-end dedup; `j`/`k` in
  normal mode cycle history without leaving normal mode
- Git file status colors in the listing (modified, added, untracked,
  deleted, renamed, conflicted) — refreshes on chdir and fs events
- Cursor returns to previous directory on climb (`u`/`-`)
- h/l at column edges clamp instead of wrapping
- Terminal resize handler: pty tabs resize immediately on `SIGWINCH`
- Pager `v` opens buffer in `$EDITOR`, returns to pager on quit
- **Diff view in pager (M12)** — `g d` shows unstaged diff, `g D` shows
  staged diff. Runs `git diff --color=always` and pipes through the
  existing ANSI pager. Works on cursor file or picks selection.
- **Git worktree integration (M11)** — `W l` list/switch worktrees,
  `W n` create new worktree (prompt for branch), `W d` delete worktree.
  Status bar already shows branch per worktree. Pane tabs are independent.
- **Context piping (M10)** — `^W p` pipes file contents of selection,
  `^W i` pipes inventory contents to pane as bracketed paste with
  `[file: path]` headers. `^W s` remains for paths only.
- Help overlay uses the pager (scrollable, searchable)
- Pager multi-column layout with position indicator (Top/Bot/NN%)
- Focus indicators: dim list cursor when pane focused, blinking pane
  cursor when focused, static block when not
- Alt+Enter sends newline to pane (Claude CLI multi-line input)
- Vi line editor: operator+motion (`dw`, `cw`, `db`, `d$`, `dd`, `cc`)
- Backspace on empty no longer cancels vi-mode prompts
- Force full repaint on pager close (fixes ghost character artifacts)
- **Multi-tab pane (M9)** — multiple independent pty tabs with `^W n`
  new, `^W x` close, `^W 1`..`^W 9` switch, `^W [`/`^W ]` prev/next
- Tab rename (`^W r`), activity indicators (`+`) on background tabs
- Powerline-style status bar with git branch + dirty flag
- Pager full-width rendering, yank to clipboard
- ESC in vi-normal mode cancels prompt (new-tab flow fix)
- Removed mouse capture (coexists with terminal text selection)
- Bracketed paste forwarding to pane — multi-line paste delivered as
  a single block to Claude CLI instead of line-by-line
- Pager line wrapping — long lines wrap instead of clipping
- Pane scroll mode (`^W v`) — browse 10K-line scrollback without
  interrupting the child process; save with `s`
- One-shot repaint strategy (`needs_full_repaint` flag, `^L` manual
  redraw) replacing per-frame `terminal.clear()`
- Makefile: build, release, cross-compile, install, deploy
- Pager enhancements: line numbers, save output, page-back, `[V]` tag
- Vi-editable shell prompt with persistent history
- Navigation: `''` jump-back, backtick jump to start dir
- Shell modes: `!` captured, `;` foreground
- Hex-dump view for binary files
- Embedded pty pane (M8)
- `.spycrc.toml` config, keymap DSL, live reload

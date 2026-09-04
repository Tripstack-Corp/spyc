# Launch plan (2.0) — archived

> **Shipped (v2.0.0, tagged 2026-07-08) — archived as historical record.** The
> repo is public at `github.com/Tripstack-Corp/spyc`, signed binaries ship on
> every tag through brew / apt / crates.io / tarballs, and the Show HN
> happened: <https://news.ycombinator.com/item?id=49346040>. Lifted verbatim
> out of `ROADMAP.md` in the 2.2 refresh, where it had outlived every gate it
> described. Nothing here is future work — the living roadmap is
> [`ROADMAP.md`](../../ROADMAP.md).

## Done-criteria, one line each

All six closed. The tree, not the plan, is the evidence:

1. **Demo in the README.** `docs/assets/demo.gif` sits above the fold, with
   four more tour clips (pager, vsplit, review, Lua, agents) added in #323 and
   #355. It is a VHS tape, not a hand recording — which is how #326 was found.
2. **Install without a Rust toolchain.** `brew install
   Tripstack-Corp/tap/spyc`, a signed apt repo, verified release tarballs, and
   `cargo install spyc` for anyone who has the toolchain anyway.
   `.github/workflows/{release,homebrew,apt}.yml` run the whole chain off a
   `v*` tag.
3. **FEATURES.md and INSTALL.md read clean.** The pre-2.1 review's
   doc-contract cluster walked every link and version reference and closed the
   drift it found (#386, #403, #411).
4. **Templated issues.** `.github/ISSUE_TEMPLATE/{bug_report,feature_request}.yml`
   plus `config.yml` and a PR template.
5. **A recent release and a current CHANGELOG.** v2.1.1 tagged 2026-08-18;
   `CHANGELOG.md` is git-cliff-generated from the conventional-commit history.
6. **A clear 2.0 CHANGELOG entry.** `CHANGELOG.md` § [2.0.0] is a curated
   milestone entry — what changed since 1.x, grouped by theme, with the
   `v2.0.0-rc.*` notes carrying per-change detail.

## The two open decisions, resolved

- **License footer** — closed. `LICENSE` is at the repo root, `Cargo.toml`
  declares `license = "BSD-3-Clause"`, and the README carries both a badge and
  a License section.
- **Status statement wording** — closed with different words. The proposal was
  *"Public beta, daily-driver-ready. macOS and Linux."*; the README instead
  leads with "The file commander built for collaborating with your coding
  agents" over "Keyboard-driven · MCP-native · Rust · macOS and Linux". The
  phrase "public beta" appears nowhere, deliberately.

## What did not ship

Recorded because the plan below still reads as a specification:

- **`MIGRATION.md`** (ranger / lf / Yazi keybind tables) — never written. The
  two Yazi-review recommendations it was meant to unblock are still open, and
  the lf fold added a third that wants the same page.
- **`:tutor`** — never built.
- **Shell completions** (`--generate-completion`) — never built.
- **First-run hint flash** — never built; there is no `first_run_done` marker
  in the tree.
- **macOS Developer ID signing + notarization, Linux minisign signatures,
  SBOM, reproducible-build verification** — none of these. What shipped
  instead is GitHub build-provenance attestation, and `SECURITY.md` § "Known
  caveats" says so plainly: ad-hoc `codesign -s -` only, no SBOM, no
  reproducible builds.

---

The rest of this file is the plan as it stood in `ROADMAP.md`, unedited.

---

## Launch plan (2.0)

> **Execution manual:** this section holds the strategic gates and open
> decisions; the end-to-end release *mechanics* — release streams, CI
> workflows, signing/notarization, Homebrew, org setup — live in
> [`docs/RELEASE_ENGINEERING.md`](../RELEASE_ENGINEERING.md), the launch
> operating manual. Keep the two in sync when a gate moves.

Benchmarked against Yazi (github.com/sxyazi/yazi, ~39.9k stars) as the
gold-standard reputable TUI tool. The MCP / Claude-Code pairing
remains spyc's differentiator — Yazi has nothing like it; keep it
front and centre. Goal: a release that someone reading the repo cold
can trust enough to make their daily file manager. Not a promotion
blitz — just enough signal to feel "this is real, maintained, and
works for me."

### Open decisions

- [x] **Repo home: RESOLVED (2026-07-02) — full move to GitHub.**
  `github.com/Tripstack-Corp/spyc` is canonical; **all dev + CI move there**
  (not a mirror). The repo stays **private** until launch. `Cargo.toml
  repository =`, the clone URLs (README/INSTALL/CONTRIBUTING), and the CI moved
  in this pass: `.github/workflows/{ci,audit}.yml` port the retired
  `bitbucket-pipelines.yml` (archived under `docs/archive/`). Remaining
  GitHub-side setup (branch protection on `main`, the weekly-audit schedule,
  and the distribution workflows — release/snapshot/homebrew per
  RELEASE_ENGINEERING.md) is done on the repo before it goes public.
- [ ] **License footer.** Already BSD-3-Clause in `Cargo.toml`;
  confirm for public release and that LICENSE is at repo root.
- [ ] **Status statement wording.** Default proposal: *"Public beta,
  daily-driver-ready. macOS and Linux."*

### Required for 2.0

1. **Repo move/mirror execution** (per the decision above): public
   repo, history + tags pushed, `Cargo.toml` repository field,
   README/INSTALL link updates, branch protection on `main`.
2. **Demo capture at top of README.** 30–60s asciinema or MP4 of the
   full Claude pairing loop: launch → `F` fuzzy-find → `:grep` →
   `^\` to Claude → "what files am I picking?" answered via
   `get_spyc_context` → `gf` jump on a path Claude mentions → quit.
   Place as the first media element after the value prop.
3. **Release pipeline + binaries.** Tag push triggers cross-compile
   matrix — macOS arm64 + x86_64, Linux x86_64 + arm64 (musl,
   static) — with artifacts attached to Releases. Homebrew tap
   (`brew tap <org>/spyc && brew install spyc`) auto-bumped from the
   release workflow. crates.io publish (binary-only crate,
   acceptable). AUR `spyc-bin` deferred post-2.0 unless a volunteer
   emerges.

### Cheap wins — batch with the launch pass

- **README hygiene**: stale status line replaced with the agreed
  status statement; headline sells the Claude angle in one sentence;
  spot-check keybinding tables.
- **Repo scaffolding**: issue templates (bug: repro/version/OS/
  terminal; feature: what/why/would-you-use-it), PR template,
  CODE_OF_CONDUCT (Contributor Covenant, link only). SECURITY.md ✅
  exists.
- **`MIGRATION.md`**: three small keybind tables (ranger → spyc,
  lf → spyc, Yazi → spyc, ~10 binds each) plus one paragraph on what
  spyc has that they don't (the MCP integration). Unblocks the two
  remaining Yazi-review recommendations
  ([`docs/COMPETITIVE_REVIEW.md`](../COMPETITIVE_REVIEW.md) §1d).
- **Signing & supply chain**: macOS Developer ID signing +
  notarization (without it the first user report is "macOS says spyc
  is damaged"); Linux minisign signatures with the public key in the
  repo; SBOM via `cargo-sbom`/`cargo-auditable`; reproducible-build
  verification job (toolchain already pinned, `SOURCE_DATE_EPOCH`,
  rebuild-and-diff). Proportionate — no SLSA theatre (see Non-goals).
- **Shell completions**: `spyc --generate-completion {bash,zsh,fish}`
  via clap derive; ship in release artifacts.
- **First-run hint flash**: on first launch (no
  `state_root()/first_run_done` marker), flash that (1) `^a`/`^w` are
  reserved chord prefixes (rebindable) and (2) `?` opens help.
  ~30 lines; saves every tmux/shell-heavy user the same surprise.
- **`:tutor` (vimtutor-style)**: interactive walkthrough on a
  pre-baked scratch directory — motions, marks, picks, `=` filter,
  pager, `^a` family, MCP context, sessions. Each lesson sets a goal,
  watches for the action, advances. The one-command demo for a
  Show-HN reader. Tutor content tracks bindings — add to the AGENTS.md
  doc-sync checklist when it lands.

### Explicitly deferred (not 2.0)

- Dedicated docs site (mdbook/Starlight). The Markdown reads fine on
  GitHub; revisit if docs outgrow single files.
- Blog/marketing posts beyond one Show HN at 2.0. CHANGELOG is enough.
- Windows support (see Non-goals — WSL is the story).
- Discord/Matrix/forum. GitHub Discussions post-launch if traffic
  warrants; a chat channel is a maintenance commitment.
- Sponsorship buttons, until traction warrants.

### Done-criteria for the 2.0 launch

A user landing on the repo cold should be able to:

1. Watch a 30-second demo in the README and understand what spyc does
   and why it's different.
2. Install via Homebrew *or* a pre-built Release binary — no Rust
   toolchain required.
3. Read FEATURES.md and INSTALL.md without broken links or stale
   version numbers.
4. File a bug or feature request via templated issues.
5. See a recent release (within ~30 days) and a current CHANGELOG.
6. Read a clear 2.0 CHANGELOG entry: what changed since 1.x, what
   stability we promise going forward.

Sequencing: repo decision first (blocks everything) → README hygiene
→ demo capture → scaffolding → release pipeline + first 2.0 binaries
→ Homebrew → migration page. The 2.0 CHANGELOG entry is written last,
once we've daily-driven our own builds for a few days.

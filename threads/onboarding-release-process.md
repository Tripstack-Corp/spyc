# onboarding-release-process — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-release-process
Created: 2026-05-07T07:48:30.692537+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:48:30.692537+00:00
Role: scribe
Type: Note
Title: Onboarding: release and distribution flow

Spec: docs

Purpose: Capture the (intentionally narrow) release flow today and the in-flight distribution work, so a contributor knows what "ship a change" means right now and which surfaces will gain new contracts as public distribution lands.

Observed:
- **Today's release flow is local-only.** Engineers install spyc by SSH-cloning the Bitbucket repo and running `make install` (`README.md:60-66`, `SECURITY.md:60-66`). There is no prebuilt binary distribution, no release page, no Homebrew tap, no AUR package — `SECURITY.md:65-66` is explicit: "There is no prebuilt binary distribution. Engineers install spyc by cloning the repo and running `make install`."
- **SemVer is enforced via PR.** `CONTRIBUTING.md:118-126` states the version lives in `Cargo.toml` and must be bumped in the PR that ships a user-visible change. Patch for fixes, minor for features, major for stable / public-API commitments. `AGENTS.md:77` reinforces: "Always bump the version in `Cargo.toml` when shipping user-visible changes."
- **Changelog convention.** `CHANGELOG.md` follows Keep-a-Changelog format. The current entry stack lives under `## [Unreleased]` until released; entries land in the same commit as the code change per the doc-sync rule. `CHANGELOG.md:1-100` confirms recent entries (v1.41.24 MCP socket discovery fix, v1.41.23 substring matcher, v1.41.22 D-pager-overlay) are tracked under Unreleased; the maintainer's release flow promotes Unreleased → versioned section in the bump commit.
- **Build-time artifacts in the binary.** `build.rs:1-26` embeds `SPYC_GIT_SHA` (short HEAD SHA), `SPYC_BUILD_TIME` (UTC `date -u`), and `SPYC_RUSTC_VERSION`. Exposed via `spyc --version --verbose` (`src/main.rs:110-126`). This means **every release build is traceable to a commit even without external metadata**, which matters because the repo doesn't yet emit a separate SBOM.
- **macOS ad-hoc signing exists; Developer ID does not.** `Makefile:152-164` invokes `codesign -s - -v` on macOS as part of `make install`. `SECURITY.md:74-80` is candid about the meaning: "ad-hoc signing, not Developer ID signing. It's enough for the binary to keep entitlements across rebuilds (and to silence some Gatekeeper-on-translocation noise), but it does **not** prove the binary came from a specific person and would not survive notarization."
- **Distribution scaffolding already exists in the Makefile** for the future public-release flow:
  - `dist` — builds Linux x86_64-musl, Linux aarch64-musl, and macOS universal into `dist/` (`Makefile:117-129`).
  - `dist-checksums` — writes `dist/checksums-sha256.txt` (`Makefile:131-134`).
  - `dist-sign` — produces a detached GPG signature on the checksums file; `GPG_KEY=<id>` selects a key (`Makefile:140-146`). The maintainer's plan: "the signing key fingerprint will be published here when that happens" (`SECURITY.md:81-87`). Today, the GPG key is not committed — the recipe is pre-staged.
- **CI does not yet run release automation.** `bitbucket-pipelines.yml:56-77` defines only `branches.main` and `pull-requests:**`, both running `quality + coverage` in parallel. There is no tag-triggered (`v[0-9]+.*`) cross-compile + upload pipeline yet — that's tracked in `ROADMAP.md` Distribution / `LAUNCH_PREP.md`.
- **Remote deploy target exists for one engineer's VM.** `Makefile:182-185` `deploy-fika` builds Linux x86_64 musl and `scp`s to `drek@10.130.1.36:~/bin/spyc`. Treat as a personal convenience target, not a release artifact. (Maintainer's local nickname; sees `derek.marshall@tripstack.com` as the project's external contact, `SECURITY.md:7,115`.)
- **In-flight distribution scope** (sources: `ROADMAP.md` Distribution section + `LAUNCH_PREP.md`):
  - GitHub move (org account undecided — Etraveli vs Tripstack vs personal); blocks Cargo.toml `repository`, `.github/` workflows, Homebrew tap namespace (`LAUNCH_PREP.md:21-25`).
  - Release automation in Bitbucket Pipelines on `v[0-9]+.*` tag push: cross-compile matrix, artifact upload, release notes from `CHANGELOG.md`, Homebrew formula bump, crates.io publish (`ROADMAP.md:367-371`).
  - macOS Developer ID code signing + notarization (`ROADMAP.md:372-377`).
  - Linux signing via minisign or cosign with the public key in the repo (`ROADMAP.md:378-381`).
  - Reproducible build verification: `SOURCE_DATE_EPOCH` honored, `cargo-auditable` to embed metadata, second CI job rebuilds from tag and diffs (`ROADMAP.md:382-386`).
  - SBOM at release: `cargo-sbom` or `cargo-auditable` emits SPDX/CycloneDX (`ROADMAP.md:387-389`).
  - Package registries: `cargo publish` to crates.io, `tripstack/homebrew-spyc` tap, AUR `spyc-bin` (`ROADMAP.md:389-393`); deliberately skipping nixpkgs / Debian / Fedora.
  - GitHub mirror at `github.com/tripstack/spyc` for discoverability — "Mirror, don't migrate" (`ROADMAP.md:394-397`).
- **v2.0 is a signaling bump.** `ROADMAP.md:472-476` calls v2.0 "a signaling choice as much as a semver one. The tool has been shipping 1.x for a while, but the MCP positioning shift + public distribution justifies a major bump to mark the transition." Target: mid-to-late May 2026 (`ROADMAP.md:467-468`).

Inferred:
- The current "release" is a `git pull` + `make install` away — adopting a new version on a teammate's machine is a person-touches-the-machine flow. — confidence: high — basis: `SECURITY.md:60-66` and the absence of any tag-triggered pipeline. How to apply: any change with a security-sensitive footprint (e.g., MCP socket discovery) needs explicit pull-and-install instructions in the PR description and CHANGELOG entry until automation lands.
- The signing posture today is "theater-avoided"; that flips to "actually meaningful" only when prebuilt binaries get published. — confidence: high — basis: maintainer's own framing in `SECURITY.md:122-136` ("When to revisit this document"). How to apply: any PR that adds a release artifact (Homebrew tap, GitHub release, crates.io publish) MUST also land the signing posture in the same PR — the threat model assumes those flip together.
- The `dist-sign` recipe was added pre-emptively so the public-release commit doesn't have to invent it; treat the GPG key fingerprint as the missing piece. — confidence: medium — basis: `Makefile:140-146` is fully working modulo `GPG_KEY`; `SECURITY.md:84-87` says explicitly "scaffolding for the right thing."

Next query: `watercooler_search(query="release version distribution signing", thread_topic="onboarding-release-process", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-developer-experience` — `make install`, `dist`, `dist-checksums`, `dist-sign`, `deploy-fika` are the developer-side surfaces this entry generalizes.
- `onboarding-product-charter` — the v2.0 framing comes from the same maintainer-authored thesis.
- `onboarding-security` — supply-chain controls and the threat model that release work has to preserve.

Provenance:
- Files read: `Makefile:117-185` (dist + deploy + install), `SECURITY.md:60-87,122-136`, `bitbucket-pipelines.yml:56-77`, `CONTRIBUTING.md:118-126`, `AGENTS.md:77`, `README.md:60-66`, `ROADMAP.md:354-468`, `LAUNCH_PREP.md:1-40`, `CHANGELOG.md:1-100`, `build.rs:1-26`, `src/main.rs:110-126`.
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-developer-experience = 01KR0PFHHCNVJPNJSTPA3VW62J`, `onboarding-product-charter = 01KR0P18MCE1H57Q5ZTAGKAJNH`.

<!-- Entry-ID: 01KR0PHNA4XW7CWPQ2D93K24HC -->

---
Entry: Claude Code (caleb) 2026-06-09T05:19:32.159867+00:00
Role: scribe
Type: Note
Title: Onboarding refresh: release/versioning at #311 — now v1.56.0 (gix-drop release), Bitbucket squash-merge PR as the vehicle, three-remote topology, patch-corridor cadence

Spec: docs

Purpose: Refresh of the release/versioning flow from #37 (~v1.41.24, "Unreleased stack tracking v1.41.x") to #311 / v1.56.0. The release vehicle is still the Bitbucket squash-merge PR with a Cargo.toml version bump + CHANGELOG promotion; what moved is the version (15 minor versions on), the now-visible release-tag history, and the confirmed three-remote topology with the GitHub mirrors frozen.

Observed:
- **Now at v1.56.0** (`Cargo.toml:3` `version = "1.56.0"`), up from the ~v1.41.24 the #37 entry recorded. CHANGELOG head shows the cadence: `## [Unreleased]` (ratatui 0.30.1 bump, aislop baseline gate) → `## [1.56.0] - 2026-06-06` → `## [1.51.4] - 2026-05-29` → `## [1.50.0] - 2026-05-08` (`CHANGELOG.md:6,28,140,1852`). The intervening v1.50.x patch corridor (.0→.82) and v1.51.x are collapsed into squashed CHANGELOG sections.
- **v1.56.0 is the gix-drop release.** Its CHANGELOG entry leads with "spyc no longer runs the `git` binary at all" — repo discovery, status, diff, show, blame, worktree all run in-process via `gix`; "A guard test enforces zero `git`-subprocess usages in production code. (The `git` binary is still used to build fixtures when running spyc's own test suite.)" (`CHANGELOG.md:29-37`). VERIFIED accurate: the 8 remaining `Command::new("git")` sites in `src/` are all inside `#[cfg(test)]` modules — `run_git`/`porcelain` fixture helpers in `src/git/{discovery,blame,status,worktree,diff_model}.rs` and `src/app/state/tests/mod.rs`. Production is clean; the binary is a test-fixture dependency only. (#37 predates gix entirely; cross-ref `history-seg-gix-migration`.)
- **The release vehicle is the Bitbucket squash-merge PR**, not a tag pipeline. SemVer is enforced in-PR: the version lives in `Cargo.toml` and is bumped in the PR that ships a user-visible change — patch for fixes, minor for features, major for stable/public-API (`CONTRIBUTING.md:117-126`; `AGENTS.md` "always bump the version when shipping user-visible changes"). CHANGELOG follows Keep-a-Changelog (`CHANGELOG.md:1-4`); `Unreleased` → versioned section is promoted in the bump commit.
- **Release tags exist now.** Local tags: `v1.50.0` (#51) and `v1.51.4` (#170). v1.56.0 is the CHANGELOG head dated 2026-06-06 (the gix-drop, #292) — its tag, if cut, lives on the Bitbucket canonical remote (only two release tags are present in the local clone). The #37 entry could cite no tags at all.
- **Three-remote topology confirmed** (`git remote -v`): `bitbucket` = `git@bitbucket.org:tripstack/spyc.git` (canonical, fetch+push), `origin` = `git@github.com:calebjacksonhoward/spyc.git`, `tripstack-corp` = `git@github.com:Tripstack-Corp/spyc.git`. Bitbucket is the only live remote; both GitHub remotes are stale mirrors frozen at #87 / 2026-05-14 (cross-ref `history-three-repo-lineage` 01KRX2YRNMPARTPFK3CW3R50FG and the repoint plan `migration-github-origin-to-tripstack-corp` 01KRM5G544P02SNE96D6HWTEWX — origin was a temporary personal-GH bridge; tripstack-corp the intended permanent home; neither is the release surface today).
- **CI still runs no release automation.** `bitbucket-pipelines.yml:203-222` defines only `branches.main`, `pull-requests:'**'` (each quality+coverage), and the `custom: weekly-deps` schedule. No tag-triggered (`v[0-9]+.*`) cross-compile-and-upload pipeline — that, plus crates.io/Homebrew/AUR publish, macOS Developer-ID signing, SBOM, and the GitHub public mirror, remain the in-flight Distribution scope tracked in `ROADMAP.md` / `LAUNCH_PREP.md`.
- **Distribution scaffolding is pre-staged but unused for public release:** `make dist` (musl x86/arm + macOS universal → `dist/`), `make dist-checksums` (SHA-256), `make dist-sign` (detached GPG, `GPG_KEY=<id>`) — `Makefile:175-205`. macOS install is ad-hoc `codesign -s -`, not Developer-ID. No prebuilt-binary distribution today; install is still clone + `make install`.

Inferred:
- The release cadence is a **patch-corridor under one minor**: a short v1.41.x corridor (window-1, to .37), then a long v1.50.x corridor (.0→.82), then v1.51.4 and v1.56.0 — the same patch-under-one-minor shape at larger scale across the #38–#311 window. — confidence: high — basis: `insight-recurrence` window-2 Pattern 6, entry 01KTMTYBDZ99R8XBA44E6NTZRQ; corroborated by the collapsed CHANGELOG sections.
- Adopting a new version on a teammate's machine is still a person-touches-the-machine flow (`git pull` + `make install`); the GPG/signing posture only becomes meaningful when prebuilt binaries actually ship. — confidence: high — basis: no tag pipeline; ad-hoc codesign; dist-sign gated on an uncommitted `GPG_KEY`.

Next query: `watercooler_search(query="release version tag distribution gix-drop three-remote", thread_topic="onboarding-release-process", code_path=".")`

Related:
- `onboarding-developer-experience` — `make install`/`dist`/`dist-checksums`/`dist-sign` are the dev-side surfaces this generalizes.
- `onboarding-security` — supply-chain controls and the signing threat model release must preserve.
- the history corpus — `history-three-repo-lineage` (01KRX2YRNMPARTPFK3CW3R50FG) + `migration-github-origin-to-tripstack-corp` (01KRM5G544P02SNE96D6HWTEWX) for the remote topology; `insight-recurrence` Pattern 6 (01KTMTYBDZ99R8XBA44E6NTZRQ) for the patch-corridor cadence; `history-seg-gix-migration` for the v1.56.0 gix drop.

Provenance:
- Files read: `Cargo.toml:3`, `CHANGELOG.md:1-37,140,1852`, `CONTRIBUTING.md:117-126`, `Makefile:175-205`, `bitbucket-pipelines.yml:203-222`.
- Commands run: `git remote -v` (three remotes), `git tag` (v1.50.0, v1.51.4), `grep -rn 'Command::new("git")' src` (8 sites, all under `#[cfg(test)]` — verified production-clean).
- History entry_ids consulted: 01KRX2YRNMPARTPFK3CW3R50FG (three-repo lineage), 01KRM5G544P02SNE96D6HWTEWX (github→tripstack-corp repoint), 01KTMTYBDZ99R8XBA44E6NTZRQ (patch-corridor Pattern 6).
- Prior #37 entry refreshed: 01KR0PHNA4XW7CWPQ2D93K24HC.

<!-- Entry-ID: 01KTND4J8QW64BPE7NKWBDXG6E -->

# spyc — Release Engineering & Public Launch

> Planning + living-process doc. The **process** half (streams, versioning,
> cadence, security) is public and carries to the repo; the **one-time
> deployment checklist** (§12) is setup scaffolding for the public 2.0 launch.
> Modeled on FreeBSD's release-engineering process, right-sized for a small team
> and phased in.

## 1. Philosophy

FreeBSD's release model is the gold standard for *predictable* software: a
rolling development head, conservative stabilization branches, frozen release
branches that only take vetted fixes, signed artifacts, and published support
windows. spyc adopts the **shape** of that model — the value is the
predictability, not the bureaucracy — and phases it in:

- **Stage 1 (launch):** `main` (CURRENT) + tagged RELEASEs cut through a short
  BETA/RC freeze, a single supported line, GitHub Security Advisories for
  vulns. Enough structure to be trustworthy; not so much it stalls a small team.
- **Stage 2 (maturity):** introduce `stable/N` + `releng/N.M` branches and
  backports once two major lines need parallel support. Don't pay for it before.

Principles: every released artifact is **reproducible** (pinned toolchain,
`--locked`) and **signed**; every release has **notes** (git-cliff +
human-written highlights); support windows are **published**; security has a
**single front door**.

## 2. Release streams (the FreeBSD model, mapped)

> **What actually happens today: every release is tagged on `main`.** No
> `stable/*` or `releng/*` branch has ever existed — v2.0.0 through v2.1.1 were
> all cut from `main`, and `make release-tag` *refuses to run anywhere else*
> (`test "$(git rev-parse --abbrev-ref HEAD)" = "main"`). The multi-stream model
> below is the intended shape, not the current one; §2a says when it becomes
> necessary and what has to change to adopt it. It is recorded here rather than
> deleted because the reasoning still holds — but a document that describes a
> process the repo does not run is worse than no document, so read the table
> with the Status column.

| FreeBSD | spyc | Git ref | Audience | Stability | Status |
|---|---|---|---|---|---|
| `-CURRENT` | **CURRENT** | `main` | contributors, early adopters | rolling, may break | **live** |
| `-RELEASE` | **RELEASE** | tag `vN.M.P` | end users | frozen snapshot | **live** (tagged on `main`) |
| `-STABLE` | **STABLE** | `stable/N` (per major) | users wanting fixes without churn | behavior-stable within major N | *not implemented* |
| `releng/*` | **release branch** | `releng/N.M` | release engineering | frozen; security/errata only | *not implemented* |
| snapshots | **SNAPSHOT** | CI artifacts off `main` | testers | nightly/weekly, unsigned-prerelease | *not implemented* |

- **CURRENT (`main`)** — all development lands here, gated by CI.
- **RELEASE** — a `vN.M.P` tag on `main`, produced by `make release-prep` on a
  branch, merging that PR, then `make release-tag` on `main`. A patch release
  works the same way: land the fix on `main`, then tag. v2.1.1 was cut exactly
  so, one commit after v2.1.0.
- **STABLE / release branches** — the planned shape, below.

### 2a. When the branch model becomes necessary

Tagging patches off `main` works while `main` contains nothing that shouldn't
ship in a patch. That held for v2.1.1: `main` was v2.1.0 plus one packaging fix.
It stops holding the moment you need to patch `N.M` *after* `N.M+1` work has
landed — the fix and a pile of unrelated features would then be on the same
commit, and there is no way to tag one without the other.

That is the trigger. Adopting the model at that point means:

- fork `releng/N.M` from the release tag, cherry-pick the fix, tag `vN.M.P+1`
  there;
- relax `release-tag`'s `main`-only check to accept `releng/*` (the check exists
  so a tag can't be cut off a feature branch by accident, which is still worth
  keeping — it needs widening, not removing);
- `stable/N` only earns its keep with more than one live minor, so it can wait
  until then.

**MFC / backport rule** (applies once release branches exist): a fix flows
*toward* stability — land on `main`, then a labelled cherry-pick PR backports it
to `stable/N`, then to `releng/N.M`. Never the reverse.

## 3. Versioning

SemVer, with FreeBSD's patch-level mapping made explicit:

- `MAJOR.MINOR.PATCH` — `MAJOR.MINOR` is the release line (and would name its
  branch, `releng/2.0`, once §2a's branches exist), `PATCH` is FreeBSD's `-pK`
  errata/security level (`v2.0.0` → `v2.0.1` → …).
- **What `main` carries between releases: `N.M.0-CURRENT`** — the *next* minor,
  suffixed to mark it as the rolling CURRENT stream (`2.1.0-CURRENT` while 2.1
  is in development). It is **static** for the whole cycle: no PR bumps it, and
  the release PR is what strips the suffix to `N.M.0`. Three consequences:
  - A dev build is self-identifying: `spyc --version` prints
    `2.1.0-CURRENT (<sha>)`. The SHA gives exact build identity, which is what
    the former bump-every-PR rule provided.
  - Version-line merge conflicts stop happening, since nothing edits that line.
    The `spyc-semver` driver goes dormant on CURRENT (it parses plain triples
    and declines a `-CURRENT` line) and still serves release branches.
  - Nothing publishes a CURRENT version: only `v*` tags trigger `release.yml`,
    and CURRENT is never tagged. Were one ever packaged, `make deb`'s
    `DEB_VERSION` tilde-forms it to `2.1.0~CURRENT`, which dpkg sorts *below*
    the real `2.1.0` — the same ordering rule the `2.0.0~rc.N` debs rely on.
  - After a minor ships, a follow-up PR opens the next cycle by setting `main`
    to the following `-CURRENT`.
- **Prereleases:** `vN.M.0-beta.1`, `vN.M.0-rc.1` — published as GitHub
  *pre-releases* (the "Latest" badge stays on the last stable).
- **Breaking changes** bump MAJOR and (Stage 2) start a new `stable/N`.
- **Reproducibility:** the pinned `rust-toolchain.toml` + `--locked` mean a tag
  rebuilds bit-stable across runners.

> **Launch number: `2.0.0`.** spyc keeps its name and its version line — the
> public launch is a deliberate major bump from the current `1.9x` development
> line, not a reset to `1.0.0`. (The `1.0.0`-reset option only existed under the
> abandoned clean-slate rebrand; staying spyc, we continue the line.)

## 4. Branch & tag topology

> **The planned topology (§2a), not the current one.** Today the graph is a
> straight line: every tag from `v2.0.0` to `v2.1.1` sits on `main`, with no
> `stable/*` or `releng/*` branch in existence.

```mermaid
gitGraph
   commit id: "dev"
   commit id: "dev2"
   branch stable/2
   checkout stable/2
   commit id: "stabilize"
   branch releng/2.0
   checkout releng/2.0
   commit id: "freeze" tag: "v2.0.0-rc.1"
   commit id: "ship" tag: "v2.0.0"
   commit id: "errata" tag: "v2.0.1"
   checkout main
   commit id: "dev3"
   checkout stable/2
   commit id: "backport"
   branch releng/2.1
   checkout releng/2.1
   commit id: "freeze2" tag: "v2.1.0"
```

(`spyc`'s own pager renders this; so does GitHub.) Stage 1 collapses the middle:
tag RELEASEs directly off `main` until a second major needs `stable/`.

## 5. The release cycle

**Stage 1 (single line, what we launch with):**

1. Development accrues on `main`, which sits at `N.M.0-CURRENT` (§3); CI green
   on every PR.
2. **Cut RC:** tag `vN.M.0-rc.1` on `main`. `release.yml` builds + publishes a
   GitHub *pre-release* with full artifacts. Soak (e.g., 3–7 days / dogfood).
3. Fix blockers on `main`; re-tag `-rc.2` as needed.
4. **Release — two steps, because `main` only takes PRs.** A single
   bump-commit-and-tag would put the tag on a release-branch commit that the
   squash-merge then orphans, leaving it outside `main`'s history:
   1. On a `chore/release-N.M.P` branch: `make release-prep VERSION=N.M.P` —
      strips the `-CURRENT` suffix to the real version, prepends the git-cliff
      section, commits. Open it as a PR and merge it.
   2. On `main` once merged: `make release-tag VERSION=N.M.P` — verifies the
      merged commit really is that version, that the changelog has its section,
      and that `HEAD` is `origin/main`, then tags. Push the tag →
      `release.yml` publishes the RELEASE.
   Then open a follow-up PR setting `main` to the next `-CURRENT`.
5. **Patch:** same two steps with `VERSION=N.M.(P+1)`. (Stage 2: cherry-pick
   onto `releng/N.M` first.)

**Stage 2 (parallel majors):** add a freeze on a `releng/N.M` branch forked
from `stable/N`, run `-beta.X` → `-rc.X` → `vN.M.0` on that branch, and keep
`main` open for the next major. The Release Manager owns the freeze window.

Each tag matching `v*` triggers `release.yml` (§8); `-beta`/`-rc` tags are
auto-marked pre-release by pattern.

**RC retention:** `release.yml` keeps only the **3 most recent `-rc` pre-releases**,
deleting older ones (release + git tag + assets) on every publish. Only `-rc`
pre-releases are pruned — stable `vN.M.P` releases are never touched. Older RCs are
throwaway soak builds (~38 MB of assets each); the changelog and stable tags are the
durable record.

**RC retention in apt** mirrors it, but on a different rule, because a published
deb is an *installable artifact* rather than a download. `apt.yml` runs
`scripts/prune-apt-repo.sh` on every publish, before regenerating the index:

- A prerelease deb is dropped **once its final release is present**. dpkg sorts
  `2.0.0~rc.11` below `2.0.0`, so once the stable exists that deb can never be
  selected again — it is pure index weight.
- Otherwise the **3 newest prereleases of an in-flight version** are kept, per
  version line, so a soak in progress stays installable.
- **Stable versions are never pruned** — pinning an older release is legitimate.

The prune runs in the same step as the reindex on purpose: `Packages` /
`Release` / `InRelease` are GPG-signed over the file set, so deleting debs
without regenerating and re-signing would leave the signed index advertising
files that are gone, failing apt's hash check on every client. That is also why
the archive can't be edited by hand — there is nowhere to edit it (it exists
only as a Pages artifact, rebuilt per publish), and the signing key only exists
inside the `apt-publish` environment. To purge retroactively, dispatch `apt.yml`
with any recent tag; the prune runs on the way through.

The policy also decides what gets BUILT. `apt.yml` materializes one empty
placeholder per candidate version, runs this same script over them, and builds
only the survivors — the script is purely filename-driven, so it yields the same
answer against placeholders as against real debs, and the retention rule stays
in one file rather than being restated in the workflow.

Inspect the policy against a directory of debs without publishing:
`make apt-prune-check APT_REPO=<dir>`.

### 5a. Two crates now publish, in order

`spyc-vt-sys` must reach crates.io **before** spyc. This is not a preference:
`cargo package` refuses a dependency without a registry version — *"all
dependencies must have a version requirement specified when packaging"* — so
spyc cannot be published while its FFI crate is unpublished. Verified by
attempting it.

```
cargo publish -p spyc-vt-sys     # first
cargo publish -p spyc            # then, once the registry has it
```

The two package separately, which is what makes the vendored archives
affordable: `spyc-vt-sys` measures **3.91 MiB** as a `.crate` (five archives)
and spyc's own stays around 1.6 MiB, so each is independently well inside
crates.io's 10 MiB cap instead of sharing one budget. Both figures are from an
actual `cargo package`, not arithmetic — re-measure on a pin bump, since the
archives are most of the first number.

`spyc-vt-sys` is versioned independently of spyc and only moves when the pin or
the bindings move. A spyc release that changes neither republishes nothing.

## 6. Maintenance: Errata & Security (EN / SA)

FreeBSD splits post-release fixes into **Errata Notices** (critical non-security)
and **Security Advisories**. spyc's equivalents:

- **Security front door:** `SECURITY.md` (already in repo — add a *Supported
  Versions* table) pointing at **GitHub private vulnerability reporting**
  (Security tab → advisories). Triage → fix on `main` → backport to every
  supported `releng/*` → coordinated patch release `vN.M.(P+1)` → publish a
  **GHSA** with a CVE if warranted. Credit reporters.
- **Errata:** critical non-security regressions get the same backport-and-patch
  path, announced in the release notes under a dedicated "Errata" heading.
- **Signing:** release notes + `SHA256SUMS` are signed (§9) so an advisory's
  artifacts are verifiable.
- **Supply chain:** `audit.yml` owns it — the full `make deny` (advisories, bans,
  licenses, sources) weekly *and* on `workflow_dispatch`, against a freshly
  fetched RUSTSEC DB, so advisories surface between releases even with no commits.
  Deliberately **not** in `ci.yml`'s PR gate (the fetch made commits
  network-dependent and segfaulted a runner mid-check). **Dispatch it before
  cutting a release**, and after merging anything that moves `Cargo.lock`;
  optionally Dependabot for the Actions themselves.

## 7. Support & EOL policy

Publish this table in the README/SECURITY.md and keep it current:

| Stream | Supported? | Window |
|---|---|---|
| Latest RELEASE (`vN.M.x`) | ✅ full (features land in next minor; fixes as patches) | until 2 minors newer ships |
| Previous minor (`vN.(M-1).x`) | ✅ security + errata only | 3 months after the newer minor |
| `stable/N` (current major) | ✅ (Stage 2) | life of major N |
| Older majors | ❌ EOL | — |
| `nightly` / RC | ⚠️ testing only | never "supported" |

Right-sized for launch: **support the latest RELEASE; security-patch the
previous minor for a short tail.** Expand windows as the user base grows.

## 8. CI/CD pipelines (GitHub Actions)

The workflows under `.github/workflows/`. All pin the toolchain via
`rust-toolchain.toml` and run cargo with `--locked`. The local `make` targets
are the source of truth — Actions *call them* so local and CI never drift.

> **Dev-platform: RESOLVED (2026-07-02) — full move to GitHub.** All dev + CI run
> on GitHub Actions; `bitbucket-pipelines.yml` is retired (archived under
> `docs/archive/`). **`ci.yml`, `audit.yml`, and `release.yml` are implemented**
> (the last with keyless signing, no secrets). The **Homebrew tap is live** —
> `Tripstack-Corp/homebrew-tap` carries a `Formula/spyc.rb` that installs the
> signed release tarballs (macOS universal + Linux x86_64/aarch64) and supports
> `--HEAD` source builds. **`apt.yml` is implemented** — it builds `.deb`
> packages and publishes a signed apt repository to GitHub Pages (see below).
> `snapshot.yml` (nightly cadence) remains to build.

### `ci.yml` — quality gate (PR + push to `main`) — **IMPLEMENTED**
- Direct port of the retired `bitbucket-pipelines.yml`: `lint` (fmt + clippy) ∥
  `test` on PRs, plus a `coverage` job (`cargo llvm-cov --locked
  --all-targets --fail-under-lines 35`) on pushes to `main`. Supply-chain
  (cargo-deny) moved to `audit.yml` — see §6. Toolchain cached via
  `Swatinem/rust-cache`; `cargo-llvm-cov` is the same sha-pinned prebuilt binary
  as before; `CARGO_INCREMENTAL=0` throughout. Make it a required status check in
  branch protection. The required contexts are exactly `Lint (fmt, clippy)` and
  `Tests (cargo test --all-targets)` — a job's **`name` IS its context**, so
  renaming either one is a three-step operation: drop the old context from
  protection, merge the rename, add the new context. Renaming without that dance
  deadlocks every open PR on a context that will never report again.
- **Follow-ups:** (1) add a `macos-latest` matrix leg to catch OS-gated lints
  both ways (replaces needing `make lint-linux` + zig locally) — the initial
  port is Linux-only, matching the retired pipeline; (2) extend the trigger to
  `stable/*` / `releng/*` once Stage 2 branches exist. Both are cheap once the
  Linux gate is confirmed green on GitHub.

### `release.yml` — build, sign, publish (on tag `v*`) — **IMPLEMENTED**
- **Trigger:** `push: tags: ['v*']`. Detect `-alpha`/`-beta`/`-rc` → mark GitHub
  Release as *pre-release*.
- **Build matrix** (calls the existing Makefile targets):
  - `macos-latest` → `make release-macos-universal` (arm64 + x86_64 lipo).
  - `ubuntu-latest` → `make release-linux-x86` + `release-linux-arm` (musl
    static via `cargo-zigbuild`, already wired).
- **Package:** `make dist` collects them into `dist/`; `make dist-checksums`
  emits `SHA256SUMS`.
- **Sign (§9):** GitHub artifact attestations (SLSA provenance, keyless OIDC) +
  sign `SHA256SUMS` (cosign keyless, and/or `make dist-sign` GPG with `GPG_KEY`
  secret).
- **Notes:** `make changelog` (git-cliff) for the generated section + a hand-
  written highlights block (the 1Password/Slack "changelog for humans" style the
  backlog calls for).
- **Publish:** create the GitHub Release, attach `spyc-vN.M.P-<target>.tar.gz` ×4,
  `SHA256SUMS`, signatures. Then **dispatch** `homebrew.yml` + `apt.yml`: a
  GITHUB_TOKEN-created release does NOT fire their `on: release` triggers (GitHub
  blocks token events from cascading), so release.yml runs them via
  `gh workflow run` — the one event a GITHUB_TOKEN can trigger.
- **Permissions:** `contents: write`, `id-token: write` (attestations),
  `attestations: write`, `actions: write` (dispatch the channel bumps).
- **As built (deviations from the sketch above):** the `macos`/`linux` build
  jobs run per-runner and call the platform Makefile targets directly (rather
  than `make dist` / `make dist-checksums`, which build *all* platforms on one
  host); a final `publish` job downloads the artifacts, computes `SHA256SUMS`,
  and creates the release with `gh release create` (no third-party publish
  action). Three platform tarballs ship (macOS universal counts once). Signing
  is **keyless only** — build-provenance attestations + a cosign-signed
  `SHA256SUMS.cosign.bundle`; GPG (`make dist-sign`), the Homebrew tap, and the
  hand-written highlights block are fast-follows. Zig for the musl cross-links
  comes from `mlugg/setup-zig`; `cargo-zigbuild` + `git-cliff` from
  `taiki-e/install-action`.

### `snapshot.yml` — nightly CURRENT builds (schedule + manual)
- `schedule` (nightly) + `workflow_dispatch`. Build `main` with the release
  matrix, publish/refresh a single rolling `nightly` pre-release (delete-and-
  recreate, or a dated tag pruned to last N). Unsigned-acceptable; clearly
  labelled "testing only."

### `audit.yml` — supply-chain drift (schedule) — **IMPLEMENTED**
- Weekly `cargo deny check advisories` (fresh RUSTSEC DB) + `cargo outdated` +
  `cargo tree --duplicates` (Mon 06:00 UTC + manual dispatch). Ported from the
  retired Bitbucket `weekly-deps` pipeline. The old Bitbucket→Slack failure
  notification does *not* carry over — add a GitHub-issue/Slack step if wanted.

### `codeql.yml` — static analysis (push to `main` + schedule) — **IMPLEMENTED**
- `actions` + `rust` legs, both `build-mode: none` — CodeQL's Rust extractor
  reads source and never invokes cargo, so no build cache shortens it.
- **Deliberately off the PR path.** CodeQL is not a required context, so a
  per-PR run gated nothing while costing ~12 min a push (the `rust` leg measured
  601-737s; the `ci.yml` gate is ~70s). Merge to `main` still analyzes
  everything that lands; the Monday 05:00 UTC sweep re-runs newly published
  queries against unchanged code.
- Migrated off GitHub's **default setup**, whose triggers are fixed and cannot
  be narrowed. Default setup must stay **disabled** in repo settings — an
  advanced-setup upload is rejected while it is on. Re-enabling it in the UI
  silently restores the per-PR cost this workflow exists to avoid.
- `python` is not analyzed: default setup covered one dev helper,
  `scripts/aislop-baseline.py`, which ships in no artifact.

### `homebrew.yml` — tap bump (on release published) — **TAP LIVE**
- `Tripstack-Corp/homebrew-tap` is live with `Formula/spyc.rb` (pins the release
  version, its per-platform tarball URLs + SHA256s, and a `--HEAD` source build).
  On any publish (prereleases included — the tap tracks the current rc stream)
  this workflow recomputes the SHAs and pushes the formula bump. Auth is an
  org-owned GitHub App (contents:write on the tap repo, no personal PAT):
  `HOMEBREW_APP_ID` + `HOMEBREW_APP_KEY` in the `homebrew-tap` environment mint a
  short-lived token via `actions/create-github-app-token`.

### `apt.yml` — signed apt repo publish (on release published) — **LIVE**
- On any publish (prereleases included — apt tracks the rc stream): download the
  release's Linux tarballs, build
  `.deb`s (`make deb`), regenerate the apt index (`apt-ftparchive`), sign the
  `Release` file with the signing key, and deploy the archive to GitHub Pages as
  a **workflow artifact** — served at `https://tripstack-corp.github.io/spyc`,
  so users `apt install spyc` / `apt upgrade`. The job self-skips when the
  signing key is absent.
- **No `gh-pages` branch, by design.** It used to hold the archive, and because
  `git clone` fetches every branch that made a clone of the SOURCE repo 255 MiB
  against 14 MiB of project — every `.deb` ever published, including 22 already
  pruned from the index and unfetchable by any client. A user reported the
  checkout time. Pages now deploys from an artifact (`build_type: workflow`),
  which needs no branch and keeps the URL, so no `sources.list` had to change.
- **The archive is a pure function of the releases.** Nothing stores debs:
  every version in the index is rebuilt from its own release tarball on each
  publish. Attaching them as release assets was the obvious alternative and is
  impossible retroactively — this repo has immutable releases, so assets freeze
  at creation. A rebuilt deb may not be byte-identical to the one it replaces
  (`dpkg-deb` embeds mtimes); harmless, since the index is re-signed over
  exactly what is deployed, in the same deployment.
- **Signing key** — a dedicated RSA-4096 key signs `Release`; its public half is
  published as `KEY.gpg` (what users pin via `signed-by`). The private key +
  passphrase live in the **`apt-publish` protected Environment** (secrets
  `APT_GPG_PRIVATE_KEY` + `APT_GPG_PASSPHRASE`), scoped so only this job — on a
  `v*` tag or `main` — can read them.
- **Operator setup — DONE** (kept for reference / rebuild):
  1. Pages on `spyc` is set to **GitHub Actions** as its source
     (`build_type: workflow`), not a branch.
  2. The `apt-publish` Environment (deployment policy: `v*` tag + `main` branch)
     holds the two `APT_GPG_*` secrets.
  3. The signing key is backed up off-repo (owner's password manager) with a
     revocation certificate.
- **Prerelease ordering:** rc tags publish too. The deb version is tilde-formed
  (`make deb`'s `DEB_VERSION` does `-`→`~`, e.g. `2.0.0~rc.4`) because dpkg sorts
  `2.0.0-rc.4` ABOVE the final `2.0.0` — the `~` form sorts a prerelease BELOW
  it, so an rc deb never blocks the stable upgrade.

**Cross-cutting:** `concurrency` groups to cancel superseded runs (already wired
in `ci.yml`); the `apt-publish` environment holds `APT_GPG_PRIVATE_KEY` +
`APT_GPG_PASSPHRASE`, the `homebrew-tap` environment holds `HOMEBREW_APP_ID` +
`HOMEBREW_APP_KEY` (org GitHub App). The apt push uses the built-in
`GITHUB_TOKEN` and cosign uses OIDC — no personal tokens stored.

## 9. Artifacts, signing & distribution

**Target matrix** (already supported by `rust-toolchain.toml` + Makefile):

| Platform | Target triple | Build | Artifact |
|---|---|---|---|
| macOS (Apple Silicon + Intel) | `aarch64`/`x86_64-apple-darwin` | universal lipo | `spyc-vN.M.P-macos-universal.tar.gz` |
| Linux x86_64 | `x86_64-unknown-linux-musl` | static | `spyc-vN.M.P-linux-x86_64.tar.gz` |
| Linux aarch64 | `aarch64-unknown-linux-musl` | static | `spyc-vN.M.P-linux-aarch64.tar.gz` |
| Windows | — | via WSL (use the Linux build) | documented, not a native target |

**Checksums:** `SHA256SUMS` (`make dist-checksums`).

**Signing — recommend layering:**
1. **GitHub artifact attestations** (SLSA build provenance, keyless via OIDC) —
   the modern default; verifiable with `gh attestation verify`. Zero key
   management.
2. **Signed `SHA256SUMS`** — cosign keyless (Rekor transparency log) and/or the
   existing `make dist-sign` GPG path (`GPG_KEY`) for users who want a Web-of-
   Trust signature. Document `cosign verify-blob` / `gpg --verify` in INSTALL.

**Distribution channels:**
- **GitHub Releases** — primary; the binaries + checksums + signatures live here.
- **Homebrew tap** — `brew install Tripstack-Corp/tap/spyc` (**live**; works on
  macOS and Linux/Linuxbrew). `homebrew.yml` keeps the formula current.
- **apt repository** — `apt install spyc` on Debian/Ubuntu from the signed
  GitHub-Pages repo (`apt.yml`; amd64 + arm64).
- **`cargo-binstall`** — add the `[package.metadata.binstall]` hints to
  `Cargo.toml` so `cargo binstall spyc` pulls the GitHub artifact (no compile).
- **crates.io** — optional `cargo install spyc` (§13.5); reserve the name
  regardless.

## 10. GitHub org & repo presentation (Tripstack-Corp)

Two brands coexist and must stay distinct: **spyc is the product** (its mark is
the chili 🌶️, per `BRAND.md`); **Tripstack is the maintainer/publisher** (its
logo lives at the org level and in a "maintained by" line — not as the product
mark).

**Org — `github.com/Tripstack-Corp`:**
- **Profile README** via a `Tripstack-Corp/.github` repo (`profile/README.md`) —
  who Tripstack is, what it ships, links. Carries the **Tripstack logo** + brand
  description.
- **Org avatar** = Tripstack logo; org description + verified domain if available.
- Pin `spyc` once public.

**Repo — `Tripstack-Corp/spyc`:**
- **About:** description = BRAND.md's crate line ("A keyboard-driven, MCP-native
  terminal file commander that gives your coding agent live eyes on your working
  tree."); homepage; **topics:** `rust` `tui` `cli` `terminal` `file-manager`
  `mcp` `ai-agents` `claude` `developer-tools`.
- **Social preview** (1280×640) — the spyc chili on Charcoal with the wordmark,
  palette from BRAND.md.
- **README** — spyc logo at top, badges (CI, latest release, license, platform),
  the positioning line, the install one-liners, a "Maintained by Tripstack"
  footer with the Tripstack logo.
- **Community health files:**
  - `LICENSE` — BSD-3 (final form pending legal, §13.2).
  - `SECURITY.md` — **exists**; add the Supported Versions table + GitHub
    private-reporting link.
  - `CONTRIBUTING.md` — exists; update + add the release/backport workflow.
  - `CODE_OF_CONDUCT.md` — **add** (none today; Contributor Covenant).
  - `.github/ISSUE_TEMPLATE/` (bug + feature) + `PULL_REQUEST_TEMPLATE.md`.
  - `CODEOWNERS` — route reviews to the maintainers.
  - `FUNDING.yml` — optional.
- **Branch protection:** `main` (require `ci.yml`, ≥1 review, linear history);
  `stable/*` + `releng/*` (same + **no force-push**, restrict who can push).
- **Repo settings:** enable Discussions (optional), private vulnerability
  reporting, secret scanning + push protection, Actions with the minimal
  permissions above; default to **squash-merge** (matches the existing
  one-commit-per-shape convention) with a clean title.

## 11. Roles

- **Release Manager (FreeBSD `re@`):** owns the cadence, declares freezes, cuts
  RCs/RELEASEs, approves backports to a frozen `releng/*`. Holds the release
  signing key (or the OIDC config).
- **Security Contact (FreeBSD `so@`):** monitors private vuln reports, drives
  fix + coordinated disclosure + GHSA. Can be the same person at launch; name a
  backup.
- Document both in `SECURITY.md` / `CONTRIBUTING.md` (a handle/alias, not a
  personal name per our conventions).

## 12. One-time launch checklist (public 2.0)

spyc keeps its name and git history — the public launch is publishing the
existing repo, not seeding a clean slate. Ordered to stand the whole thing up:

1. **Decide the public-repo topology (§13.4):** the private
   `Tripstack-Corp/spyc` is canonical — decide whether to flip it public with
   full history intact or curate/squash a seed first (Bitbucket is retired, not
   a fallback record). Then create the supporting repos — `Tripstack-Corp/.github`
   (org profile), `Tripstack-Corp/homebrew-tap`.
2. Set org avatar (Tripstack logo) + profile README + descriptions.
3. **Resolve the remaining launch gates (§13):** license final form +
   maintainer/IP sign-off (§13.1–2, both launch blockers). Dev/CI platform
   (§13.3) is resolved — full GitHub. The homage-line framing (§13.6) is already
   implemented in the README (nominative lineage + disclaimer); confirm and close.
4. Add the remaining workflows — `.github/workflows/{release,snapshot,homebrew}.yml`
   (`ci.yml` + `audit.yml` already exist).
5. Add/refresh community-health files (LICENSE, SECURITY.md Supported-Versions
   table, CONTRIBUTING, add CODE_OF_CONDUCT, issue/PR templates, CODEOWNERS).
6. Repo About: description, homepage, topics; upload the social-preview card.
7. Configure secrets (`GPG_KEY`/`GPG_PASSPHRASE` if GPG; `HOMEBREW_APP_ID` +
   `HOMEBREW_APP_KEY` for the tap-bump App) + Actions permissions
   (`id-token: write` for attestations).
8. Branch protection on `main` (+ `stable/*`, `releng/*` for Stage 2); required
   checks; squash-merge default.
9. Enable Security: private vuln reporting, secret scanning, (optional)
   Dependabot for Actions.
10. **Dry run:** push a throwaway `v0.0.0-rc.1` tag → confirm `release.yml`
    builds all four artifacts, signs, and publishes a pre-release; verify with
    `gh attestation verify` + checksum.
11. Add `[package.metadata.binstall]` to `Cargo.toml`; publish the first Homebrew
    formula; (optional) reserve/publish crates.io.
12. Flip the repo public → cut `v2.0.0` (via the RC → RELEASE cycle, §5) →
    announce.

## 13. Open decisions

1. **License — public-release form:** confirm with legal what Tripstack can
   publish and under which license (currently **BSD-3-Clause** in `Cargo.toml` /
   `deny.toml`). Reconcile a *single* answer across `Cargo.toml` `license`, a
   root `LICENSE` file, `deny.toml`, and BRAND.md. **Launch blocker.**
2. **Maintainer / IP:** confirm Tripstack owns the work and signs off on a public
   OSS release under the company name. **Launch blocker.**
3. **CI / dev platform — RESOLVED (2026-07-02): full move to GitHub.** All dev +
   CI run on `github.com/Tripstack-Corp/spyc` (Actions + `gh`); the repo stays
   private until launch. `bitbucket-pipelines.yml` is retired (archived);
   `ci.yml` + `audit.yml` are ported and live in `.github/workflows/`. `bkt` →
   `gh`; PRs, branch protection, and required checks are GitHub-native.
4. **Public-repo topology:** flip the private, full-history `Tripstack-Corp/spyc`
   public with its history intact (transparent "built from scratch" trail) vs.
   curate/squash a seed first (drop internal planning/review docs + the owner's
   backlog from the public record). Bitbucket is retired, so it is no longer a
   provenance fallback. *(Recommend: carry history if nothing sensitive is in it;
   otherwise curate a seed.)*
5. **crates.io:** publish `spyc` (reserve the name now) or stay source/binary
   install only? *(Recommend: reserve now; publish fast-follow.)*
6. **Public homage-line framing:** the README's "spy + claude = spyc" origin
   reads, to a strict trademark eye, as an admission of derivation from SideFX's
   `spy` (risk assessed **Medium → Negligible**; no registered SideFX mark).
   Decide whether to keep it verbatim, soften to a plain nominative-fair-use
   lineage line (no hybrid-mark construction, add a "not affiliated with Side
   Effects Software or Anthropic" disclaimer), or omit the explicit derivation.
   *(Recommend: keep the spice/`spy-see` story, frame the lineage descriptively,
   add the disclaimer.)*
7. **Cadence:** feature-ready minors vs. time-based? *(Recommend: feature-ready
   at launch, revisit time-based once stable.)*
8. **Signing:** attestations only, + cosign, + GPG? *(Recommend: attestations +
   cosign-signed checksums; GPG optional.)*
9. **Install channels at launch:** Releases + Homebrew + binstall; add `curl|sh`?
   *(Recommend: Releases + Homebrew + binstall for v2.0; script + crates.io
   fast-follow.)*
10. **Native Windows** ever, or WSL-only? *(Recommend: WSL-only; revisit on
    demand.)*
11. **Nightly snapshots** from day one or after launch? *(Recommend: after
    launch.)*

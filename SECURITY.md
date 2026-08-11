# Security

This document describes spyc's actual security posture — what we do,
what we don't, and why. It exists so reviewers and future maintainers
can see the threat model without inferring it from CI config.

If you find something wrong, contact derek.marshall@tripstack.com.

## Threat model

spyc is a single-binary terminal file manager. It runs locally as the
invoking user and has no network code of its own. It is distributed
both as source and as **signed prebuilt binaries** (GitHub Releases,
a Homebrew tap, a signed apt repo, and crates.io — see INSTALL.md).
The realistic threats are:

- **Supply-chain compromise of a Rust dependency** — a transitive
  crate is yanked + republished with malicious code, or an unmaintained
  dep develops a CVE.
- **Release-pipeline compromise** — an attacker who can run code in a
  release workflow, or who obtains the apt signing key, can publish a
  malicious binary that installs as "spyc" on every consumer machine.
  This is the highest-consequence threat and the reason for the
  keyless-signing posture below.
- **Tampered local build** — someone modifies the source on a shared
  clone before `make install` runs.
- **MCP socket misuse by another local process** — the per-PID Unix
  socket exposes tool calls to whatever process can read
  `~/.local/state/spyc/mcp-<pid>.sock`. Filesystem permissions gate
  this; an attacker already running as your user can talk to any of
  your spyc instances.
- **MCP tool misuse by the connected agent** — distinct from the above
  and worth stating plainly, because it is the case people assume is
  covered. See "MCP is not a privilege boundary" below.

spyc has no remote attack surface, no privilege boundary inside the
binary, and handles no secrets of its own.

It does, however, **parse a great deal of untrusted input**: any file
you open in the pager, ANSI/escape output from arbitrary child
processes in the pty pane (vt100), git objects (gix), and images /
SVG / markdown / syntax-highlighting input. A malicious file or a
hostile child process is a realistic way to reach a parser bug. This
is what `fuzz/fuzz_targets/` exists for — see the fuzzing caveat.

### MCP is not a privilege boundary against the connected agent

The MCP read tools take an optional `root` argument to scope a query to
a worktree. That argument is **validated against a set of roots spyc
knows about**, so an agent cannot point a read tool at an arbitrary
directory. The rationale is recorded in ROADMAP.md's decisions log:
agent harnesses commonly auto-approve MCP tool calls while gating shell
execution behind per-command permission prompts, so an unvalidated
`root` would bypass a boundary the *user* believes exists.

That validation is a guardrail, not a sandbox. An agent with shell
access can read anything you can read, and spyc does not try to prevent
it. Do not treat the MCP tool surface as a containment mechanism for an
untrusted agent.

## Supply-chain controls (what we do)

- **`Cargo.lock` is committed.** Every build resolves the same set
  of versions. `cargo build` will not silently bump deps.
- **`--locked` everywhere.** `make test` / `make lint` / all release
  builds pass `--locked` to cargo, so a CI-time `Cargo.lock` drift
  fails loudly. The Makefile and the GitHub Actions CI
  (`.github/workflows/ci.yml`) both enforce this.
- **`cargo deny check`** runs on every CI build (advisories,
  licenses, sources, bans). Configuration is checked in at `deny.toml`
  with documented reasons for every advisory ignore — none are silent.
- **License allow-list.** Only the licenses present in our actual
  dep graph are allowed; the list is reviewed against
  `cargo deny list` when deps change. Adding a dep with a license
  outside that set fails CI; you read it, decide, and either add to
  the allow-list (with a reason) or pick a different dep. Several deps
  are multi-licensed and offer a copyleft option (`self_cell`,
  `r-efi`, the `Unlicense OR MIT` BurntSushi crates); cargo-deny
  selects an allowed alternative, so no copyleft obligation attaches.
- **Source allow-list.** Only crates from
  `https://github.com/rust-lang/crates.io-index` are accepted.
  No `git = "..."` deps, no patched forks.
- **MSRV pinned** via `rust-toolchain.toml`. A new stable release
  cannot tighten lints, change behavior, or drop features behind
  our back.
- **Pre-commit hook** (optional, install via `make install-hooks`)
  runs the same gate as CI before each commit so drift surfaces in
  seconds locally instead of ~10 min later in pipelines.

## Release pipeline, signing, and what you can verify

Pushing a `v*` tag runs `.github/workflows/release.yml`. Operator
mechanics live in `docs/RELEASE_ENGINEERING.md` §8–9; this section
covers only the security-relevant chain.

**Build.** macOS universal (`arm64` + `x86_64` lipo) on
`macos-latest`; static musl `x86_64` and `aarch64` on `ubuntu-latest`
via `cargo-zigbuild` with a pinned Zig. The workflows call the same
`make` targets used locally, so CI and local builds don't drift.
`SHA256SUMS` is generated over all three tarballs.

**Signing is keyless — no stored secret is involved.**

- **Build provenance** — `actions/attest-build-provenance` produces a
  SLSA attestation binding each tarball to the workflow, repo, and
  commit that produced it, via GitHub's OIDC identity.
- **cosign-signed checksums** — `cosign sign-blob` signs `SHA256SUMS`
  with an ephemeral OIDC-derived key, recorded in the Rekor
  transparency log, published as `SHA256SUMS.cosign.bundle`.
- **crates.io Trusted Publishing** — the publish job exchanges a
  GitHub OIDC token for a crates.io token scoped to this repo and
  workflow file, valid only for that step and auto-revoked afterwards.
  No registry token is stored.

**What a consumer can verify** (commands in INSTALL.md):

```sh
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify spyc-<tag>-<platform>.tar.gz --repo Tripstack-Corp/spyc
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '...' --certificate-oidc-issuer '...'
```

Attestation verification is the strong check: it proves the artifact
was built by this repo's release workflow, not merely that a hash
matches a file someone published alongside it.

**Actions permissions.** Every workflow defaults to
`contents: read`. Only the release publish job widens it —
`contents: write` (create the release), `id-token: write` (OIDC for
attestation and cosign), `attestations: write`, and `actions: write`
(to dispatch the homebrew and apt workflows, because a release created
by `GITHUB_TOKEN` does not fire `on: release`). The crates.io job holds
`id-token: write` and nothing else. `apt.yml` holds `contents: write`
solely to push the index to `gh-pages`.

**The apt signing key is the one real stored secret.** A dedicated
RSA-4096 key signs the apt `Release` file; its public half is published
as `KEY.gpg` and pinned by clients via `signed-by`. The private key and
passphrase live in the `apt-publish` **protected Environment**
(`APT_GPG_PRIVATE_KEY`, `APT_GPG_PASSPHRASE`), scoped by deployment
policy so only that job, on a `v*` tag or `main`, can read them. The
key is backed up off-repo in the owner's password manager together with
a revocation certificate. The apt push itself uses the built-in
org-owned `GITHUB_TOKEN`, not a personal or cross-repo token. The
Homebrew tap bump uses an org GitHub App credential in the separate
`homebrew-tap` Environment.

**On key compromise.** The keyless channels need no rotation — there is
no long-lived key to steal, and a malicious artifact would carry an
attestation naming the workflow that actually built it. For apt:
revoke using the stored revocation certificate, generate a new
RSA-4096 key, replace both `apt-publish` secrets, and republish
`KEY.gpg`. Every existing apt client must then re-import the new key,
because `signed-by` pins the old one — an unavoidable break, and the
reason the apt key is the most valuable secret in the project. This
procedure is stated here but has never been rehearsed.

## Known caveats (what we don't do)

- **No reproducible builds.** Two builds of the same source on two
  machines may differ in timestamps, paths, and rustc-version
  fingerprints. Bit-for-bit reproducibility is non-trivial for Rust
  binaries and we don't claim it. Build-provenance attestation is what
  we offer instead: it proves *where* a binary came from rather than
  letting you rebuild it yourself.
- **No SBOM published.** `cargo deny check` and `Cargo.lock` together
  give us a full audit trail, but we don't emit a CycloneDX or SPDX
  SBOM artifact. If a consumer needs one, generating it from
  `Cargo.lock` is a one-shot script away.
- **No commit signing requirement.** The repo does not require signed
  commits. A compromised dev account could push unsigned commits
  indistinguishable from real ones, bounded by branch protection
  (PR-only merge into `main`, required status checks, restricted write
  access). Note this is the weakest link in the release chain: signing
  proves an artifact came from the workflow, not that the *source* the
  workflow built was authored by who you think.
- **Fuzz targets run weekly, not per-PR.** `fuzz/fuzz_targets/` covers the
  DSL parser, path/percent expansion, highlighting, markdown rendering,
  word wrap, and the archive layer — member names and the container
  parsers themselves. `.github/workflows/fuzz.yml` runs every target on a
  schedule; `make fuzz` runs one on demand (nightly, deliberately out of
  `make check`). A regression therefore surfaces within the week rather
  than at the next release, but a PR can still merge without any target
  having seen its code.
- **macOS binaries are ad-hoc signed only.** `make install` invokes
  `codesign -s -`, which keeps entitlements stable across rebuilds and
  silences some Gatekeeper-on-translocation noise. It does **not**
  prove origin and would not survive notarization. A real Developer ID
  requires Apple Developer enrollment and is out of scope; the
  provenance attestation is the origin proof instead.
- **MCP socket permissions are filesystem-default.** Anyone running
  as your user on the same machine can read the per-PID socket and
  exercise the MCP tool surface. We rely on user-process isolation,
  not stricter ACLs.

## Reporting a vulnerability

Email derek.marshall@tripstack.com. Internal contact, no formal SLA;
expect a same-day response during business hours.

If the issue is in a dependency we use rather than spyc itself,
please **also** report it upstream — we'll coordinate on a fix and
update `deny.toml` as needed.

## When to revisit this document

Update this document when any of the following change:

- A new dependency is added with a license outside `deny.toml`'s
  allow-list (and the allow-list is widened).
- A new advisory is ignored in `deny.toml` (the reason should also
  be expanded here under "known caveats" if it's load-bearing).
- spyc gains a network attack surface (HTTP client, RPC server,
  remote config source).
- A distribution channel is added or removed, or the signing chain
  changes (new key, a channel that stores a credential, a move away
  from keyless).
- The MCP tool surface gains a tool that writes outside spyc's own
  process, or the `root` validation described above is relaxed.

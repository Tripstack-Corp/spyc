# onboarding-developer-experience — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-developer-experience
Created: 2026-05-07T07:47:21.234209+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:47:21.234209+00:00
Role: scribe
Type: Note
Title: Onboarding: developer experience and local build flow

Spec: docs

Purpose: Capture the local build / lint / test / cross-compile workflow so a new contributor can go from a fresh clone to a working development loop without piecing the answer together from three different files.

Observed:
- **Two task runners exist**: `Makefile` (canonical, what CI runs) and `Justfile` (lighter alternative). They are NOT in lockstep — `Justfile` covers build/run/test/clippy/fmt and the three release-static cross-compile recipes; `Makefile` adds `check` (the full quality gate), `deny`, `install`, `install-hooks`, `dist*`, `deploy-fika`, `doctor`, `uninstall`, `clean`. CI uses the Makefile (`bitbucket-pipelines.yml:43-44`).
- **Quickstart** (`README.md:60-66`):
  ```sh
  git clone https://bitbucket.org/tripstack/spyc.git
  cd spyc
  make install
  ```
  builds release + copies to `~/.local/bin` (no sudo). `Makefile:152-164` defines `install` to depend on `release`, `install -d $(PREFIX)/bin`, `install -m 755 ...`, and on macOS run `codesign -s -` (ad-hoc signing).
- **Prerequisites** (`README.md:53-58`, `INSTALL.md:51-57`): Rust 1.85+ (note: INSTALL.md says 1.80, which is stale — see `onboarding-risk-register`); Nerd Font recommended for the powerline status bar (toggle with `C` if absent); `claude` CLI optional. For cross-compile: `zig`, `cargo-zigbuild`, four `rustup target add` lines (`Makefile:3-7`).
- **Doctor target.** `make doctor` (`Makefile:189-220`) prints rustc/cargo/rustup/zig/cargo-zigbuild versions, lists installed cross-compile targets, computes missing targets vs the four required, prints a fix-line, and warns if Homebrew rust is detected (it shadows rustup). This is the right command for "is my dev env ready?"
- **Local quality gate.** `make check` = `fmt-check + lint + test + deny` (`Makefile:36`). The same target is invoked in CI (`bitbucket-pipelines.yml:43-44`). Running it locally before pushing reproduces CI's `quality` step exactly.
  - Clippy is configured pedantic + nursery + `-D warnings`; `Cargo.toml:81-144` lists every allowed lint with a documented reason.
  - `cargo-deny` covers what the older `cargo-audit` step covered, plus licenses + sources + bans (`SECURITY.md:42-43`).
  - `cargo test` is forced single-threaded somewhere in the test setup because of XDG-state contention (`bitbucket-pipelines.yml:43-44` notes the constraint).
- **Optional pre-commit hook.** `make install-hooks` (`Makefile:170-176`) installs `scripts/git-hooks/pre-commit` so `make check` runs locally on every commit (`SECURITY.md:54-56`). Bypassable with `git commit --no-verify`.
- **Cross-compile.** `Makefile` recipes:
  - `release` — current platform (no zigbuild).
  - `release-macos-arm` / `release-macos-x86` / `release-macos-universal` — uses `lipo` to fuse arm + x86_64 into a universal binary at `dist/spyc-macos-universal`.
  - `release-linux-x86` / `release-linux-arm` — uses `cargo-zigbuild` for static musl builds. `Makefile:101-103,109-111` `touch src/main.rs` before each zigbuild because "zigbuild cache is separate from cargo build and can go stale."
  - `dist` — builds all platforms and copies into `dist/`. `dist-checksums` writes SHA-256s; `dist-sign` produces a detached GPG signature on the checksums (set `GPG_KEY=<id>` to choose a key).
- **Remote deploy target** (`Makefile:182-185`): `deploy-fika` builds Linux x86_64 musl and `scp`s to `drek@10.130.1.36:~/bin/spyc`. This is one engineer's specific dev VM; not a release surface.
- **Build-time artifacts.** `build.rs:1-26` embeds `SPYC_GIT_SHA`, `SPYC_BUILD_TIME` (UTC), `SPYC_RUSTC_VERSION` into the binary; surfaced via `spyc --version --verbose` (`src/main.rs:110-126`). `cargo:rerun-if-changed=.git/HEAD` and `.git/refs/heads/` keep the SHA fresh on local commits.
- **CLI flags useful in development** (`src/main.rs:42-73`): `--debug` (writes `/tmp/spyc-debug-*.log`), `--key-trace` (writes `/tmp/spyc-key-trace-*.log`, also `SPYC_KEY_TRACE=1`), `--mcp` (run as MCP stdio proxy — what Claude Code spawns), `--print-config` (emits a fully-commented default `.spycrc.toml` template), `--verbose` (with `--version`).
- **Live config reload.** `~/.spycrc.toml` (user) and `<cwd>/.spycrc.toml` (project; project wins) are watched by the same `notify` thread that watches the working directory. Changes are picked up without restart; force a re-read with `^R` (`README.md:276-277`, `ARCHITECTURE.md:124-127`).

Inferred:
- The "right" first-run sequence for a new contributor is `make doctor` → `make` (debug build) → `make test` → `make check` → optional `make install-hooks`. — confidence: high — basis: every step is a `Makefile` target with a documented purpose; `doctor` exists specifically to flag missing prerequisites before the rest of the flow trips.
- `Justfile` and `Makefile` will *eventually* drift if both are maintained. — confidence: medium — basis: the Makefile already has 50% more targets than the Justfile (no parity for `check`, `deny`, `install`, `dist*`, `doctor`, `install-hooks`, `deploy-fika`). How to apply: when adding a new make target, decide explicitly whether it belongs in `Justfile` too, or remove `Justfile` entirely if the maintenance ratio crosses a threshold.

Next query: `watercooler_search(query="local build cross-compile dev environment", thread_topic="onboarding-developer-experience", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-test-surface` — `make check` is the local mirror of CI's quality step.
- `onboarding-risk-register` — flags `INSTALL.md` MSRV stale-ness that affects the prerequisites story here.
- `onboarding-release-process` — picks up where `make install` ends and frames the future public-release flow.

Provenance:
- Files read: `Makefile:1-235` (full), `Justfile:1-49`, `bitbucket-pipelines.yml:1-77`, `SECURITY.md:42-56`, `README.md:53-66,276-303`, `INSTALL.md:51-57`, `Cargo.toml:81-144`, `rust-toolchain.toml:1-5`, `build.rs:1-26`, `src/main.rs:42-73,110-126`, `ARCHITECTURE.md:114-127`.
- Commands run: `ls -1 scripts/git-hooks/` (`pre-commit`).
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-test-surface = 01KR0PBH6T1AK4VA0JSE75390F`, `onboarding-risk-register = 01KR0P9JC8Z3DF6FQ1GJPF3VKA`.

<!-- Entry-ID: 01KR0PFHHCNVJPNJSTPA3VW62J -->

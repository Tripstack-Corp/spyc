# spyc — build and distribution
#
# Prerequisites (one-time setup):
#   brew install zig
#   cargo install cargo-zigbuild
#   rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
#   rustup target add x86_64-apple-darwin aarch64-apple-darwin
#
# Quick reference:
#   make              — debug build (fast, for development)
#   make release      — optimized release for current platform
#   make dist         — all platforms → dist/
#   make check        — fmt + clippy + test (CI gate)
#   make install      — copy to ~/.local/bin (run `make release` first)
#   make install-debug — install symbolicated `spyc.debug` for sample/lldb/perf

BINARY   := spyc
VERSION  := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
DIST_DIR := dist

# Where cargo actually writes build output. `target/` by default, but a shared
# `CARGO_TARGET_DIR` / `[build] target-dir` redirects it — worth doing when a
# worktree-per-PR layout would otherwise carry a ~4 GB `target/` each. Ask cargo
# rather than assuming, or `make install` silently installs nothing. Recursive
# `=`, so only the rules that need a built binary pay the `cargo metadata` call.
TARGET_DIR = $(or $(CARGO_TARGET_DIR),$(shell cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])' 2>/dev/null),target)

# Rust flags shared across release builds.
RELEASE_FLAGS := --locked --release

# ---------- Development -----------------------------------------------------

.PHONY: build
build: ## Debug build (fast iteration)
	cargo build

.PHONY: run
run: ## Debug run
	cargo run

# ---------- Quality gate -----------------------------------------------------

.PHONY: check
check: fmt-check lint test ## Full quality gate (CI)

# `deny` is deliberately NOT in `check`. `check` is what the pre-commit hook
# runs, and cargo-deny re-fetches the RUSTSEC advisory DB over the network on
# every invocation — slow on every commit, offline-hostile, and the source of two
# concrete incidents: the advisory-DB update inherits the hook's `GIT_DIR` and
# hard-reset the developer's own checked-out branch (see the pre-commit hook and
# AGENTS.md), and it has segfaulted mid-run on CI's Linux runner, failing a PR
# that had nothing to do with dependencies. Supply chain is owned by
# `audit.yml` (weekly + `workflow_dispatch`), which runs this same target.
# Run it by hand any time with `make deny`.

# A pipeline's exit status is its LAST command's, so `make check | tail` reports
# the pager's success and a failed gate reads as green. No target can fix that —
# the pipe lives in the caller's shell. What a target CAN do is put the verdict
# in the output, where a tail will see it.
.PHONY: check-ci
check-ci: ## `check` ending in an unmissable verdict line — safe to pipe
	@if $(MAKE) --no-print-directory check; then \
		echo "=== GATE: PASS ==="; \
	else \
		status=$$?; \
		echo "=== GATE: FAIL (exit $$status) ==="; \
		exit $$status; \
	fi

# `--locked` on test/lint/build forbids implicit Cargo.lock changes —
# CI and dev builds use the committed lockfile or fail loudly.
.PHONY: test
test: ## Run all tests (--workspace: spyc-vt-sys is a member and must be gated too)
	cargo test --locked --workspace --all-targets

.PHONY: lint
lint: ## Clippy with pedantic + nursery
	cargo clippy --locked --workspace --all-targets -- -D warnings

# Clippy for the Linux target, runnable from macOS via zig as the C
# cross-compiler (cargo-zigbuild's `zig cc` wrapper rewrites the Rust
# target triple into zig's format so zstd-sys et al. build). This is the
# only way to lint `cfg(target_os = "linux")` code — e.g. clipboard.rs's
# wl-copy/xclip path — from a Mac: the host clippy compiles that code
# *out*, so OS-gated lints slip past `make check` and only fail in CI
# (which lints on Linux). Run this before pushing anything that touches
# platform-gated code. Needs the one-time setup at the top of this file.
LINUX_LINT_TARGET := x86_64-unknown-linux-musl

.PHONY: lint-linux
lint-linux: ## Clippy for the Linux target (catches OS-gated lints; needs zig + cargo-zigbuild)
	@command -v cargo-zigbuild >/dev/null 2>&1 || { \
		echo "cargo-zigbuild not found — install with: cargo install cargo-zigbuild"; \
		exit 1; \
	}
	CC_x86_64_unknown_linux_musl="cargo-zigbuild zig cc --" \
	CXX_x86_64_unknown_linux_musl="cargo-zigbuild zig c++ --" \
	CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="cargo-zigbuild zig cc --" \
	cargo clippy --locked --all-targets --target $(LINUX_LINT_TARGET) -- -D warnings

# actionlint parses every workflow and runs shellcheck over each `run:` block —
# the half that matters here, since the release-critical workflows are mostly
# bash and CI never executes them (apt.yml fires on release, not on a PR).
#
# The shellcheck probe below is load-bearing, not politeness: with no shellcheck
# on PATH actionlint does not warn, it silently drops the integration and exits
# 0. Measured on this tree — the one real finding vanished and the run went
# green. A gate that checks almost nothing while reporting success is the
# failure mode src/guard_support.rs exists to document.
.PHONY: lint-workflows
lint-workflows: ## Lint .github/workflows (actionlint + shellcheck over every `run:` block)
	@command -v actionlint >/dev/null 2>&1 || { \
		echo "actionlint not found — install with: brew install actionlint"; \
		exit 1; \
	}
	@command -v shellcheck >/dev/null 2>&1 || { \
		echo "shellcheck not found — actionlint would silently skip every run: block and exit 0."; \
		echo "  install with: brew install shellcheck  (CI pins 0.11.0 — findings differ by version)"; \
		exit 1; \
	}
	@actionlint --version | head -1 | sed 's/^/actionlint /'
	@shellcheck --version | sed -n 's/^version: /shellcheck /p'
	actionlint -no-color

.PHONY: fmt
fmt: ## Format code
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying
	cargo fmt --all -- --check

.PHONY: vendor-ghostty
vendor-ghostty: ## Rebuild spyc-vt-sys's vendored libghostty-vt archives at the pinned commit
	@# Out-of-band on purpose: the archives are committed so that `cargo install
	@# spyc` needs no Zig, no network and no git for this crate. Run this only on
	@# a deliberate pin bump, and follow it with the full harness re-run that
	@# `spyc-vt-sys/src/pin.rs` (BUMP_POLICY) describes.
	@command -v zig >/dev/null 2>&1 || { \
		echo "zig not found. The pin needs exactly zig $$(cat crates/spyc-vt-sys/REQUIRED_ZIG)"; \
		echo "  ghostty's requireZig compares major.minor for EQUALITY, not as a floor."; \
		exit 1; \
	}
	@have=$$(zig version); want=$$(cat crates/spyc-vt-sys/REQUIRED_ZIG); \
	if [ "$$have" != "$$want" ]; then \
		echo "zig $$have found, but the pin requires exactly $$want"; \
		echo "  a newer zig is REJECTED by ghostty's build, not tolerated"; \
		exit 1; \
	fi
	scripts/vendor-ghostty.sh

.PHONY: deny
deny: ## Supply-chain checks: advisories, licenses, sources, bans (cargo-deny)
	@command -v cargo-deny >/dev/null 2>&1 || { \
		echo "cargo-deny not found — install with: cargo install cargo-deny --locked"; \
		exit 1; \
	}
	cargo deny --all-features check

# Re-record the README demo GIFs. On-demand: each tape drives a real spyc
# through ttyd for ~20-30s, so this is minutes of wall clock, not a gate step.
#
# The tapes record the RELEASE binary from this tree (prepended to PATH), never
# whatever `spyc` is installed — a demo must show the code it ships beside.
# TAPE=<name> re-records one; bare `make demos` does the hero + all five tour
# loops. The hero (spyc.tape) records in THIS repo — its stand-in answers with
# real spyc paths — so it takes only the fixture's private HOME.
#
# Not recordable here, by design of the medium: an inline image (ttyd/xterm.js
# has no graphics protocol, so spyc correctly draws nothing) and the desktop
# notification (an OS popup outside the terminal). The in-terminal half of the
# "an agent needs you" cue — the red dot and the spice-heat border pulse — is
# in demo-agents.gif.
.PHONY: demos
demos: ## Re-record the README demo GIFs (needs vhs; TAPE=spyc|pager|vsplit|lua|review|agents for one)
	@command -v vhs >/dev/null 2>&1 || { \
		echo "vhs not found — install with: brew install vhs"; \
		exit 1; \
	}
	@test -x $(TARGET_DIR)/release/spyc || { \
		echo "no release binary — run: cargo build --release"; \
		exit 1; \
	}
	@for tape in $(or $(TAPE),spyc pager vsplit lua review agents); do \
		echo "── recording $$tape ──"; \
		PATH="$(abspath $(TARGET_DIR))/release:$$PATH" vhs docs/assets/demo/$$tape.tape || exit 1; \
	done

.PHONY: fuzz
fuzz: ## Coverage-guided fuzz (needs nightly + cargo-fuzz; on-demand, NOT in `check`). TARGET=archive_container|archive_name|dsl_parse|render_markdown|highlight|word_wrap|expand_path|expand_percent, FUZZ_SECS=N, FUZZ_TOOLCHAIN=nightly-YYYY-MM-DD, FUZZ_TRIPLE=<host triple>.
	@command -v cargo-fuzz >/dev/null 2>&1 || { \
		echo "cargo-fuzz not found — install with: cargo install cargo-fuzz"; \
		exit 1; \
	}
	@rustup toolchain list | grep -q nightly || { \
		echo "nightly toolchain not found — install with: rustup toolchain install nightly"; \
		exit 1; \
	}
        # Seed from the committed inputs before every run. The corpus itself is
        # gitignored (CI restores it from cache), so without this a fresh clone
        # — and every first CI run after a cache miss — would start from nothing
        # and have to rediscover shapes we already know matter.
	@if [ -d "fuzz/seeds/$(or $(TARGET),dsl_parse)" ]; then \
		mkdir -p "fuzz/corpus/$(or $(TARGET),dsl_parse)"; \
		cp -n fuzz/seeds/$(or $(TARGET),dsl_parse)/* \
			"fuzz/corpus/$(or $(TARGET),dsl_parse)/" 2>/dev/null || true; \
	fi
	cargo +$(or $(FUZZ_TOOLCHAIN),nightly) fuzz run \
		$(if $(FUZZ_TRIPLE),--target $(FUZZ_TRIPLE),) $(or $(TARGET),dsl_parse) \
		-- -max_total_time=$(or $(FUZZ_SECS),30)

# Advisory AI-slop / code-quality scan. Deliberately NOT part of `check`: its
# format/lint/security engines duplicate clippy+rustfmt (already in `check`),
# and its comment/complexity rules are tuned in `.aislop/` to respect spyc's
# deliberate choices (dense "why" docs, allowed-long MVU dispatch fns, the
# in-progress 800-LoC decomposition). Run it to triage genuine slop, not as a
# pass/fail gate.
#
# `make aislop` runs through scripts/aislop-baseline.py, which subtracts the
# accepted findings recorded in .aislop/baseline.json (per-rule, per-file
# counts) and reports only NET-NEW slop — aislop 0.10.2 has no native
# baseline, and its comment engine over-fires on spyc's style, so the raw
# scan is mostly false positives. After intentionally accepting new findings,
# refresh the snapshot with `make aislop-baseline`. Raw output is still
# `aislop scan .` / `aislop --json scan .` / `aislop ci .`.
.PHONY: aislop
aislop: ## Advisory AI-slop scan vs .aislop/baseline.json (net-new only)
	@command -v aislop >/dev/null 2>&1 || { \
		echo "aislop not found — install with: npm i -g aislop"; \
		exit 1; \
	}
	@python3 scripts/aislop-baseline.py check

.PHONY: aislop-baseline
aislop-baseline: ## Regenerate .aislop/baseline.json from the current scan
	@command -v aislop >/dev/null 2>&1 || { \
		echo "aislop not found — install with: npm i -g aislop"; \
		exit 1; \
	}
	@python3 scripts/aislop-baseline.py update

# ---------- Release builds ---------------------------------------------------

.PHONY: release
release: ## Optimized release for the current platform
	@echo "building $(BINARY) v$(VERSION) (release — final crate is the linker, may take a moment)…"
	cargo build $(RELEASE_FLAGS)
	@echo "→ $(TARGET_DIR)/release/$(BINARY)"
	@ls -lh $(TARGET_DIR)/release/$(BINARY)

.PHONY: release-debug
release-debug: ## Optimized build with debug symbols (for `sample`, `lldb`, `perf`)
	@echo "building $(BINARY) v$(VERSION) (release-debug — symbols included)…"
	cargo build --locked --profile release-debug
	@echo "→ $(TARGET_DIR)/release-debug/$(BINARY)"
	@ls -lh $(TARGET_DIR)/release-debug/$(BINARY)

# --- Changelog & release tagging (git-cliff) ---------------------------------
# CHANGELOG.md is git-cliff-generated from v1.57.0 onward (older entries are
# frozen hand-written history). `make changelog` PREVIEWS the pending section.
#
# Releasing is TWO steps because `main` only takes PRs — a single
# bump-commit-and-tag target would put the tag on a release-branch commit that
# the squash-merge then orphans, leaving the tag pointing at a commit outside
# `main`'s history:
#
#   1. `make release-prep VERSION=x.y.z` on a release branch — strips `main`'s
#      `-CURRENT` suffix down to the real version, prepends the changelog
#      section, commits. Open that as a PR and merge it.
#   2. `make release-tag VERSION=x.y.z` on `main` after the merge — verifies the
#      merged commit really is that version, then tags it.
#
# All LOCAL / release-time — none of this runs in `make check` or CI.

.PHONY: changelog
changelog: ## Preview the pending (unreleased) CHANGELOG section from commits since the last tag
	@command -v git-cliff >/dev/null 2>&1 || { echo "git-cliff MISSING — brew install git-cliff"; exit 1; }
	@git cliff --config cliff.toml --unreleased

.PHONY: release-prep
release-prep: ## Step 1 of 2, on a release branch: VERSION=x.y.z → set version, prepend changelog, commit (then PR it)
	@test "$(origin VERSION)" = "command line" || { echo "usage: make release-prep VERSION=x.y.z (VERSION defaults to the Cargo.toml value, so it must be passed explicitly)"; exit 1; }
	@echo "$(VERSION)" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "VERSION must be semver x.y.z (got '$(VERSION)') — a release drops any -CURRENT suffix"; exit 1; }
	@command -v git-cliff >/dev/null 2>&1 || { echo "git-cliff MISSING — brew install git-cliff"; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "working tree not clean — commit or stash first"; exit 1; }
	@test "$$(git rev-parse --abbrev-ref HEAD)" != "main" || { echo "refusing to prepare a release on main — main takes PRs only; branch first (e.g. chore/release-$(VERSION))"; exit 1; }
	@git rev-parse "v$(VERSION)" >/dev/null 2>&1 && { echo "tag v$(VERSION) already exists"; exit 1; } || true
	@echo "→ setting version to $(VERSION) and prepending its changelog section…"
	@tmp=$$(mktemp); sed 's/^version = ".*"/version = "$(VERSION)"/' Cargo.toml > $$tmp && mv $$tmp Cargo.toml
	cargo update -p $(BINARY)
	git cliff --config cliff.toml --unreleased --tag v$(VERSION) --prepend CHANGELOG.md
	git add Cargo.toml Cargo.lock CHANGELOG.md
	git commit -m "chore(release): v$(VERSION)"
	@echo "✓ prepared v$(VERSION) on $$(git rev-parse --abbrev-ref HEAD). Next:"
	@echo "    git push -u origin HEAD && gh pr create --title 'chore(release): v$(VERSION)'"
	@echo "    # after it merges, on main: make release-tag VERSION=$(VERSION)"

.PHONY: release-tag
release-tag: ## Step 2 of 2, on main after the release PR merged: VERSION=x.y.z → verify + tag (push yourself)
	@test "$(origin VERSION)" = "command line" || { echo "usage: make release-tag VERSION=x.y.z (VERSION defaults to the Cargo.toml value, so it must be passed explicitly)"; exit 1; }
	@echo "$(VERSION)" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "VERSION must be semver x.y.z (got '$(VERSION)')"; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "working tree not clean — commit or stash first"; exit 1; }
	@test "$$(git rev-parse --abbrev-ref HEAD)" = "main" || { echo "refusing to tag off main (on $$(git rev-parse --abbrev-ref HEAD)) — the tag must land on main's merged release commit"; exit 1; }
	@git rev-parse "v$(VERSION)" >/dev/null 2>&1 && { echo "tag v$(VERSION) already exists"; exit 1; } || true
	@# The release PR is what sets the version, so a mismatch here means it has
	@# not merged yet (or main is still on its -CURRENT suffix).
	@test "$(VERSION)" = "$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')" || { echo "Cargo.toml says '$$(grep '^version' Cargo.toml | head -1 | sed 's/.*\"\(.*\)\".*/\1/')', not '$(VERSION)' — run release-prep on a branch and merge that PR first"; exit 1; }
	@grep -q '^## \[$(VERSION)\]' CHANGELOG.md || { echo "CHANGELOG.md has no [$(VERSION)] section — merge the release-prep PR first"; exit 1; }
	@git fetch -q origin main && test "$$(git rev-parse HEAD)" = "$$(git rev-parse origin/main)" || { echo "HEAD is not origin/main — pull first so the tag lands on the merged commit"; exit 1; }
	git tag v$(VERSION)
	@echo "✓ tagged v$(VERSION) at $$(git rev-parse --short HEAD). Push it to publish:"
	@echo "    git push origin v$(VERSION)"
	@echo "  then open a PR setting main's version to the next minor + '-CURRENT'."

# --- macOS ---

.PHONY: release-macos-arm
release-macos-arm: ## macOS Apple Silicon (aarch64)
	cargo build $(RELEASE_FLAGS) --target aarch64-apple-darwin
	@echo "→ target/aarch64-apple-darwin/release/$(BINARY)"

.PHONY: release-macos-x86
release-macos-x86: ## macOS Intel (x86_64)
	cargo build $(RELEASE_FLAGS) --target x86_64-apple-darwin
	@echo "→ target/x86_64-apple-darwin/release/$(BINARY)"

.PHONY: release-macos-universal
release-macos-universal: release-macos-arm release-macos-x86 ## macOS Universal binary (arm64 + x86_64)
	@mkdir -p $(DIST_DIR)
	lipo -create \
		target/aarch64-apple-darwin/release/$(BINARY) \
		target/x86_64-apple-darwin/release/$(BINARY) \
		-output $(DIST_DIR)/$(BINARY)-macos-universal
	@echo "→ $(DIST_DIR)/$(BINARY)-macos-universal"
	@ls -lh $(DIST_DIR)/$(BINARY)-macos-universal
	@file $(DIST_DIR)/$(BINARY)-macos-universal

# --- Linux (static, musl) ---

# `ulimit -n 8192`: zig's linker opens an fd per object file (250+ for spyc),
# so the final link dies with `ProcessFdQuotaExceeded` under macOS's 256-fd
# default soft limit — which bites when a musl build is invoked from a
# context that didn't raise it (e.g. spyc's own `!` shell). Raise it on the
# same shell line as the build so it applies to that subshell.
.PHONY: release-linux-x86
release-linux-x86: ## Linux x86_64 (static, musl)
	@# Touch the main source so zigbuild always recompiles spyc itself
	@# (zigbuild cache is separate from cargo build and can go stale).
	@touch src/main.rs
	ulimit -n 8192; cargo zigbuild $(RELEASE_FLAGS) --target x86_64-unknown-linux-musl
	@echo "→ target/x86_64-unknown-linux-musl/release/$(BINARY)"
	@ls -lh target/x86_64-unknown-linux-musl/release/$(BINARY)

.PHONY: release-linux-arm
release-linux-arm: ## Linux aarch64 (static, musl)
	@touch src/main.rs
	ulimit -n 8192; cargo zigbuild $(RELEASE_FLAGS) --target aarch64-unknown-linux-musl
	@echo "→ target/aarch64-unknown-linux-musl/release/$(BINARY)"
	@ls -lh target/aarch64-unknown-linux-musl/release/$(BINARY)

# ---------- Distribution -----------------------------------------------------

.PHONY: dist
dist: release-macos-universal release-linux-x86 release-linux-arm ## Build all platforms → dist/
	@mkdir -p $(DIST_DIR)
	cp target/x86_64-unknown-linux-musl/release/$(BINARY) \
		$(DIST_DIR)/$(BINARY)-linux-x86_64
	cp target/aarch64-unknown-linux-musl/release/$(BINARY) \
		$(DIST_DIR)/$(BINARY)-linux-aarch64
	@echo ""
	@echo "=== dist/ ==="
	@ls -lh $(DIST_DIR)/
	@echo ""
	@echo "Verify static linking (Linux):"
	@file $(DIST_DIR)/$(BINARY)-linux-x86_64
	@file $(DIST_DIR)/$(BINARY)-linux-aarch64

.PHONY: dist-checksums
dist-checksums: dist ## Generate SHA-256 checksums
	cd $(DIST_DIR) && shasum -a 256 $(BINARY)-* > checksums-sha256.txt
	@cat $(DIST_DIR)/checksums-sha256.txt

# Detached GPG signature on the checksums file. Verifying users run:
#   gpg --verify checksums-sha256.txt.asc checksums-sha256.txt
# then `shasum -a 256 -c checksums-sha256.txt`. The maintainer's key
# fingerprint is published in SECURITY.md.
GPG_KEY ?=
.PHONY: dist-sign
dist-sign: dist-checksums ## GPG-sign the checksums file (set GPG_KEY=<id> to choose a key)
	@command -v gpg >/dev/null 2>&1 || { echo "gpg not found"; exit 1; }
	cd $(DIST_DIR) && rm -f checksums-sha256.txt.asc && \
		gpg --detach-sign --armor $(if $(GPG_KEY),--local-user $(GPG_KEY),) checksums-sha256.txt
	@echo "✓ signature written to $(DIST_DIR)/checksums-sha256.txt.asc"

# ---------- Debian packages --------------------------------------------------

# Wrap an already-built static-musl binary in a .deb. `dpkg-deb` is Linux-only
# (on macOS: `brew install dpkg`). Build the target binary first
# (`make release-linux-x86` / `release-linux-arm`) — these targets PACKAGE an
# existing binary rather than rebuild, so the release pipeline reuses the
# artifacts it already produced. Output → dist/spyc_<version>_<arch>.deb.
DEB_MAINTAINER ?= Derek Marshall <derek.marshall@tripstack.com>

# dpkg orders `2.0.0-rc.4` ABOVE the final `2.0.0` (a `-` starts the Debian
# revision, and having one outranks having none). The tilde form `2.0.0~rc.4`
# sorts a prerelease correctly BELOW `2.0.0`, so rc debs never block the stable
# upgrade. `-` → `~` (a no-op for a final `X.Y.Z`, which has no `-`).
DEB_VERSION := $(subst -,~,$(VERSION))

.PHONY: deb-x86
deb-x86: ## Package the built Linux x86_64 binary → dist/spyc_<version>_amd64.deb
	@$(MAKE) --no-print-directory _deb ARCH=amd64 \
		SRC=target/x86_64-unknown-linux-musl/release/$(BINARY)

.PHONY: deb-arm
deb-arm: ## Package the built Linux aarch64 binary → dist/spyc_<version>_arm64.deb
	@$(MAKE) --no-print-directory _deb ARCH=arm64 \
		SRC=target/aarch64-unknown-linux-musl/release/$(BINARY)

.PHONY: deb
deb: deb-x86 deb-arm ## Package both Linux .debs (binaries must already be built)

# apt.yml runs this against the gh-pages checkout on every publish, so the repo
# self-maintains. Exposed here to inspect the policy against a local checkout of
# gh-pages without publishing: `make apt-prune-check APT_REPO=../spyc-gh-pages`.
APT_REPO ?=
.PHONY: apt-prune-check
apt-prune-check: ## Dry-run the apt prerelease pruning (set APT_REPO=<gh-pages checkout>)
	@test -n "$(APT_REPO)" || { echo "usage: make apt-prune-check APT_REPO=<dir>"; exit 1; }
	@./scripts/prune-apt-repo.sh --dry-run "$(APT_REPO)"

# Internal: build one .deb from $(SRC) for $(ARCH). Fails clearly if the binary
# is missing — build it with the matching release-linux-* target first.
.PHONY: _deb
_deb:
	@command -v dpkg-deb >/dev/null 2>&1 || { echo "dpkg-deb not found (Linux, or 'brew install dpkg')"; exit 1; }
	@test -f "$(SRC)" || { echo "missing $(SRC) — run the matching 'make release-linux-*' first"; exit 1; }
	@rm -rf "$(DIST_DIR)/deb-$(ARCH)"
	@mkdir -p "$(DIST_DIR)/deb-$(ARCH)/DEBIAN" "$(DIST_DIR)/deb-$(ARCH)/usr/bin"
	@install -m 0755 "$(SRC)" "$(DIST_DIR)/deb-$(ARCH)/usr/bin/$(BINARY)"
	@printf 'Package: %s\nVersion: %s\nArchitecture: %s\nMaintainer: %s\nSection: utils\nPriority: optional\nHomepage: https://github.com/Tripstack-Corp/spyc\nDescription: Keyboard-driven, MCP-native terminal file commander\n' \
		"$(BINARY)" "$(DEB_VERSION)" "$(ARCH)" "$(DEB_MAINTAINER)" \
		> "$(DIST_DIR)/deb-$(ARCH)/DEBIAN/control"
	dpkg-deb --build --root-owner-group "$(DIST_DIR)/deb-$(ARCH)" \
		"$(DIST_DIR)/$(BINARY)_$(DEB_VERSION)_$(ARCH).deb"
	@rm -rf "$(DIST_DIR)/deb-$(ARCH)"
	@ls -lh "$(DIST_DIR)/$(BINARY)_$(DEB_VERSION)_$(ARCH).deb"

# ---------- Install ----------------------------------------------------------

PREFIX ?= $(HOME)/.local

.PHONY: install
install: release ## Install to ~/.local/bin (builds release first; override with PREFIX=/usr/local)
	install -d $(PREFIX)/bin
	install -m 755 $(TARGET_DIR)/release/$(BINARY) $(PREFIX)/bin/$(BINARY)
ifeq ($(shell uname),Darwin)
	codesign -s - -v $(PREFIX)/bin/$(BINARY)
endif
	@echo "✓ installed $(BINARY) v$(VERSION) → $(PREFIX)/bin/$(BINARY)"
	@case ":$$PATH:" in \
		*":$(PREFIX)/bin:"*) ;; \
		*) echo "  note: $(PREFIX)/bin is not on your PATH — add it to your shell rc:"; \
		   echo "        export PATH=\"$(PREFIX)/bin:\$$PATH\"" ;; \
	esac

.PHONY: install-debug
install-debug: release-debug ## Install symbolicated build as $(PREFIX)/bin/spyc.debug (for profiling)
	install -d $(PREFIX)/bin
	install -m 755 $(TARGET_DIR)/release-debug/$(BINARY) $(PREFIX)/bin/$(BINARY).debug
ifeq ($(shell uname),Darwin)
	codesign -s - -v $(PREFIX)/bin/$(BINARY).debug
endif
	@echo "✓ installed $(BINARY).debug v$(VERSION) → $(PREFIX)/bin/$(BINARY).debug"
	@echo "  Use this binary when running \`sample\` / \`lldb\` / \`perf\` —"
	@echo "  Rust symbols are kept so the profiler can resolve function names."

.PHONY: uninstall
uninstall: ## Remove from $(PREFIX)/bin
	rm -f $(PREFIX)/bin/$(BINARY)

.PHONY: uninstall-debug
uninstall-debug: ## Remove spyc.debug from $(PREFIX)/bin
	rm -f $(PREFIX)/bin/$(BINARY).debug

# ---------- Git hooks --------------------------------------------------------

# Ask git where the hooks live rather than assuming `.git/hooks`: from a linked
# worktree `.git` is a FILE, so the literal path doesn't exist and the install
# fails — in the layout this project actually works in. There is one hooks dir
# per repository (the common dir), which is also what
# `git::commit_hook_is_current` compares against.
HOOKS_DIR = $(shell git rev-parse --git-common-dir 2>/dev/null)/hooks

.PHONY: install-hooks
install-hooks: ## Install pre-commit hook (runs `make check` before each commit)
	@install -m 755 scripts/git-hooks/pre-commit $(HOOKS_DIR)/pre-commit
	@echo "✓ installed $(HOOKS_DIR)/pre-commit — runs 'make check' on each commit"
	@echo "  bypass with 'git commit --no-verify' (don't make a habit)"

# ---------- Doctor (preflight checks) ----------------------------------------

.PHONY: doctor
doctor: ## Check build prerequisites
	@echo "=== spyc doctor ==="
	@echo ""
	@printf "  %-24s" "rustup:" && (rustup --version 2>/dev/null || echo "MISSING — https://rustup.rs")
	@printf "  %-24s" "rustc:" && (rustc --version 2>/dev/null || echo "MISSING — install via rustup")
	@printf "  %-24s" "cargo:" && (cargo --version 2>/dev/null || echo "MISSING — install via rustup")
	@printf "  %-24s" "zig:" && (zig version 2>/dev/null || echo "MISSING — brew install zig")
	@printf "  %-24s" "cargo-zigbuild:" && (cargo zigbuild --help >/dev/null 2>&1 && echo "ok" || echo "MISSING — cargo install cargo-zigbuild")
	@printf "  %-24s" "git-cliff:" && (git-cliff --version 2>/dev/null || echo "MISSING — brew install git-cliff (only needed for changelog/releases)")
	@echo ""
	@echo "  Installed targets:"
	@rustup target list --installed 2>/dev/null | sed 's/^/    /' || echo "    (rustup not available)"
	@echo ""
	@NEED_TARGETS="x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin"; \
	INSTALLED=$$(rustup target list --installed 2>/dev/null); \
	MISSING=""; \
	for t in $$NEED_TARGETS; do \
		echo "$$INSTALLED" | grep -q "$$t" || MISSING="$$MISSING $$t"; \
	done; \
	if [ -n "$$MISSING" ]; then \
		echo "  Missing targets:$$MISSING"; \
		echo "  Fix: rustup target add$$MISSING"; \
	else \
		echo "  All cross-compile targets installed ✓"; \
	fi
	@echo ""
	@printf "  %-24s" "sysroot:" && rustc --print sysroot 2>/dev/null
	@echo ""
	@# Check for homebrew rust conflict
	@if [ -f /opt/homebrew/Cellar/rust/*/bin/rustc ] 2>/dev/null; then \
		echo "  ⚠  Homebrew rust detected — may shadow rustup. Run: brew uninstall rust"; \
	fi

# ---------- Clean ------------------------------------------------------------

.PHONY: clean
clean: ## Remove build artifacts
	cargo clean
	rm -rf $(DIST_DIR)

# ---------- Help -------------------------------------------------------------

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-24s\033[0m %s\n", $$1, $$2}'

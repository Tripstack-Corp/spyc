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
check: fmt-check lint test deny ## Full quality gate (CI)

# `--locked` on test/lint/build forbids implicit Cargo.lock changes —
# CI and dev builds use the committed lockfile or fail loudly.
.PHONY: test
test: ## Run all tests
	cargo test --locked --all-targets

.PHONY: lint
lint: ## Clippy with pedantic + nursery
	cargo clippy --locked --all-targets -- -D warnings

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

.PHONY: fmt
fmt: ## Format code
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying
	cargo fmt --all -- --check

.PHONY: deny
deny: ## Supply-chain checks: advisories, licenses, sources, bans (cargo-deny)
	@command -v cargo-deny >/dev/null 2>&1 || { \
		echo "cargo-deny not found — install with: cargo install cargo-deny --locked"; \
		exit 1; \
	}
	cargo deny --all-features check

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
	@echo "→ target/release/$(BINARY)"
	@ls -lh target/release/$(BINARY)

.PHONY: release-debug
release-debug: ## Optimized build with debug symbols (for `sample`, `lldb`, `perf`)
	@echo "building $(BINARY) v$(VERSION) (release-debug — symbols included)…"
	cargo build --locked --profile release-debug
	@echo "→ target/release-debug/$(BINARY)"
	@ls -lh target/release-debug/$(BINARY)

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

.PHONY: release-linux-x86
release-linux-x86: ## Linux x86_64 (static, musl)
	@# Touch the main source so zigbuild always recompiles spyc itself
	@# (zigbuild cache is separate from cargo build and can go stale).
	@touch src/main.rs
	cargo zigbuild $(RELEASE_FLAGS) --target x86_64-unknown-linux-musl
	@echo "→ target/x86_64-unknown-linux-musl/release/$(BINARY)"
	@ls -lh target/x86_64-unknown-linux-musl/release/$(BINARY)

.PHONY: release-linux-arm
release-linux-arm: ## Linux aarch64 (static, musl)
	@touch src/main.rs
	cargo zigbuild $(RELEASE_FLAGS) --target aarch64-unknown-linux-musl
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

# ---------- Install ----------------------------------------------------------

PREFIX ?= $(HOME)/.local

.PHONY: install
install: release ## Install to ~/.local/bin (builds release first; override with PREFIX=/usr/local)
	install -d $(PREFIX)/bin
	install -m 755 target/release/$(BINARY) $(PREFIX)/bin/$(BINARY)
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
	install -m 755 target/release-debug/$(BINARY) $(PREFIX)/bin/$(BINARY).debug
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

.PHONY: install-hooks
install-hooks: ## Install pre-commit hook (runs `make check` before each commit)
	@install -m 755 scripts/git-hooks/pre-commit .git/hooks/pre-commit
	@echo "✓ installed .git/hooks/pre-commit — runs 'make check' on each commit"
	@echo "  bypass with 'git commit --no-verify' (don't make a habit)"

# --- Remote deploy ---

FIKA_HOST := drek@10.130.1.36

.PHONY: deploy-fika
deploy-fika: release-linux-x86 ## Build Linux x86_64 and scp to fika-vm
	scp target/x86_64-unknown-linux-musl/release/$(BINARY) $(FIKA_HOST):~/bin/$(BINARY)
	@echo "deployed: $(FIKA_HOST):~/bin/$(BINARY)"

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

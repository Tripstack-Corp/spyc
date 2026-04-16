# cspy — build and distribution
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
#   make install      — install to /usr/local/bin

BINARY   := cspy
VERSION  := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
DIST_DIR := dist

# Rust flags shared across release builds.
RELEASE_FLAGS := --release

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

.PHONY: test
test: ## Run all tests
	cargo test --all-targets

.PHONY: lint
lint: ## Clippy with pedantic + nursery
	cargo clippy --all-targets -- -D warnings

.PHONY: fmt
fmt: ## Format code
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying
	cargo fmt --all -- --check

# ---------- Release builds ---------------------------------------------------

.PHONY: release
release: ## Optimized release for the current platform
	cargo build $(RELEASE_FLAGS)
	@echo "→ target/release/$(BINARY)"
	@ls -lh target/release/$(BINARY)

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
	cargo zigbuild $(RELEASE_FLAGS) --target x86_64-unknown-linux-musl
	@echo "→ target/x86_64-unknown-linux-musl/release/$(BINARY)"
	@ls -lh target/x86_64-unknown-linux-musl/release/$(BINARY)

.PHONY: release-linux-arm
release-linux-arm: ## Linux aarch64 (static, musl)
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

# ---------- Install ----------------------------------------------------------

PREFIX ?= $(HOME)

.PHONY: install
install: release ## Install to ~/bin
	install -d $(PREFIX)/bin
	install -m 755 target/release/$(BINARY) $(PREFIX)/bin/$(BINARY)
	@echo "installed: $(PREFIX)/bin/$(BINARY)"

.PHONY: uninstall
uninstall: ## Remove from ~/bin
	rm -f $(PREFIX)/bin/$(BINARY)

# --- Remote deploy ---

FIKA_HOST := drek@10.130.1.36

.PHONY: deploy-fika
deploy-fika: release-linux-x86 ## Build Linux x86_64 and scp to fika-vm
	scp target/x86_64-unknown-linux-musl/release/$(BINARY) $(FIKA_HOST):~/bin/$(BINARY)
	@echo "deployed: $(FIKA_HOST):~/bin/$(BINARY)"

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

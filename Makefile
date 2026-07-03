# ═══════════════════════════════════════════════════════════
# QVOD — Cross-Platform Build & Package Makefile
# Targets: linux, windows, macos, all, package, clean
# ═══════════════════════════════════════════════════════════

PKG_NAME   := qvs
VERSION    := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*= "//;s/"//')
GIT_HASH   := $(shell git log --pretty=format:'%h' -n 1 2>/dev/null || echo "unknown")
ARCH       := $(shell uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')

# ── Binaries ──────────────────────────────────────────────
BINS       := qvs-server qvs-cli qvs-gui

# ── Target directories ────────────────────────────────────
LINUX_DIR  := target/x86_64-unknown-linux-musl/release
WIN_DIR    := target/x86_64-pc-windows-gnu/release
MAC_DIR    := target/x86_64-apple-darwin/release

# ── Output directories ────────────────────────────────────
DIST_DIR   := dist
DOCS_DIR   := docs
AGENTS_DIR := agents

# ── Find the host OS ──────────────────────────────────────
UNAME_S    := $(shell uname -s 2>/dev/null || echo "Unknown")
UNAME_M    := $(shell uname -m 2>/dev/null || echo "Unknown")

# ── Colors (disabled by default, enable with `make COLOR=1`) ──
ifeq ($(COLOR),1)
  RESET  := \033[0m
  BOLD   := \033[1m
  GREEN  := \033[32m
  YELLOW := \033[33m
  CYAN   := \033[36m
  RED    := \033[31m
else
  RESET  :=
  BOLD   :=
  GREEN  :=
  YELLOW :=
  CYAN   :=
  RED    :=
endif

.PHONY: all distclean help list-targets \
        linux linux-musl linux-gnu \
        windows windows-gnu \
        macos macos-darwin \
        check-env \
        package-linux package-windows package-macos \
        install-mingw install-osxcross

# ═══════════════════════════════════════════════════════════
# Default target: build for current host
# ═══════════════════════════════════════════════════════════
all: check-env
	@echo "=== QVOD Build System v$(VERSION) ($(GIT_HASH)) ==="
	@echo "Host: $(UNAME_S) $(UNAME_M)"
	@echo ""
	@echo "Available targets:"
	@echo "  make linux        Build fully static Linux binaries (musl)"
	@echo "  make windows      Cross-compile for Windows (needs MinGW)"
	@echo "  make macos        Cross-compile for macOS (needs osxcross)"
	@echo "  make package      Build + package for current host"
	@echo "  make all-platforms  Build all available targets"
	@echo ""
	@echo "Run 'make list-targets' to see available Rust targets."

# ═══════════════════════════════════════════════════════════
# Environment check
# ═══════════════════════════════════════════════════════════
check-env:
	@which rustc >/dev/null 2>&1 || { echo "Error: rustc not found. Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; exit 1; }

list-targets:
	@echo "Installed Rust targets:"
	@rustup target list --installed
	@echo ""
	@echo "Available for install:"
	@echo "  windows:  rustup target add x86_64-pc-windows-gnu"
	@echo "  macos:    rustup target add x86_64-apple-darwin"

# ═══════════════════════════════════════════════════════════
# Linux — fully static with musl
# ═══════════════════════════════════════════════════════════
linux: check-env
	@echo "==> Building for Linux (musl, fully static)"
	@rustup target list --installed | grep -q x86_64-unknown-linux-musl || \
		(echo "Target not installed. Installing..." && \
		 rustup target add x86_64-unknown-linux-musl)
	RUSTFLAGS="-C target-feature=+crt-static" \
		cargo build --release --target x86_64-unknown-linux-musl \
		$(foreach bin,$(BINS),-p $(bin))
	@mkdir -p $(DIST_DIR)/linux/bin $(DIST_DIR)/linux/docs
	@for bin in $(BINS); do \
		cp $(LINUX_DIR)/$$bin $(DIST_DIR)/linux/bin/; \
	done
	@cp README.md $(DIST_DIR)/linux/docs/
	@cp DEPLOY.md $(DIST_DIR)/linux/docs/
	@$(MAKE) verify-static
	@echo "Linux build complete: $(DIST_DIR)/linux/"
	@ls -lh $(DIST_DIR)/linux/bin/

verify-static:
	@for f in $(DIST_DIR)/linux/bin/*; do \
		if file $$f 2>/dev/null | grep -q "static"; then \
			echo "  * $$(basename $$f): fully static"; \
		else \
			echo "  ! $$(basename $$f): dynamic linking detected"; \
		fi \
	done

# ═══════════════════════════════════════════════════════════
# Windows — cross-compile with MinGW
# ═══════════════════════════════════════════════════════════
windows: check-env
	@echo "==> Building for Windows (cross-compile)"
	@rustup target list --installed | grep -q x86_64-pc-windows-gnu || \
		(echo "Target not installed. Installing..." && \
		 rustup target add x86_64-pc-windows-gnu)
	@which x86_64-w64-mingw32-gcc >/dev/null 2>&1 || \
		(echo "Error: MinGW not found. Install:" && \
		 echo "  Ubuntu/Debian: sudo apt install mingw-w64" && \
		 echo "  Fedora/RHEL:   sudo dnf install mingw64-gcc" && \
		 echo "  Arch:          sudo pacman -S mingw-w64-gcc" && \
		 exit 1)
	cargo build --release --target x86_64-pc-windows-gnu \
		$(foreach bin,$(BINS),-p $(bin))
	@mkdir -p $(DIST_DIR)/windows/bin $(DIST_DIR)/windows/docs
	@for bin in $(BINS); do \
		cp $(WIN_DIR)/$$bin.exe $(DIST_DIR)/windows/bin/; \
	done
	@cp README.md $(DIST_DIR)/windows/docs/
	@cp DEPLOY.md $(DIST_DIR)/windows/docs/
	@# Create Windows batch launchers
	@echo '@echo off' > $(DIST_DIR)/windows/run-player.bat
	@echo 'start "" "%~dp0bin\qvs-gui.exe"' >> $(DIST_DIR)/windows/run-player.bat
	@echo '@echo off' > $(DIST_DIR)/windows/run-server.bat
	@echo '"%~dp0bin\qvs-server.exe" --port 8621' >> $(DIST_DIR)/windows/run-server.bat
	@echo 'pause' >> $(DIST_DIR)/windows/run-server.bat
	@echo "Windows build complete: $(DIST_DIR)/windows/"
	@ls -lh $(DIST_DIR)/windows/bin/

# ═══════════════════════════════════════════════════════════
# macOS — cross-compile with osxcross
# ═══════════════════════════════════════════════════════════
macos: check-env
	@echo "==> Building for macOS (cross-compile)"
	@rustup target list --installed | grep -q x86_64-apple-darwin || \
		(echo "Target not installed. Installing..." && \
		 rustup target add x86_64-apple-darwin)
	@which o64-clang >/dev/null 2>&1 || \
		(echo "Error: osxcross not found." && \
		 echo "  Install osxcross: https://github.com/tpoechtrager/osxcross" && \
		 echo "  Or build natively on macOS: make macos-native" && \
		 exit 1)
	CC=o64-clang CXX=o64-clang++ \
		cargo build --release --target x86_64-apple-darwin \
		$(foreach bin,$(BINS),-p $(bin))
	@mkdir -p $(DIST_DIR)/macos/bin $(DIST_DIR)/macos/docs
	@for bin in $(BINS); do \
		cp $(MAC_DIR)/$$bin $(DIST_DIR)/macos/bin/; \
	done
	@cp README.md $(DIST_DIR)/macos/docs/
	@cp DEPLOY.md $(DIST_DIR)/macos/docs/
	@echo "macOS build complete: $(DIST_DIR)/macos/"
	@ls -lh $(DIST_DIR)/macos/bin/

# Native macOS build (run on macOS directly)
macos-native: check-env
	@echo "==> Building for macOS (native)"
	cargo build --release $(foreach bin,$(BINS),-p $(bin))
	@mkdir -p $(DIST_DIR)/macos/bin $(DIST_DIR)/macos/docs
	@for bin in $(BINS); do \
		cp target/release/$$bin $(DIST_DIR)/macos/bin/; \
	done
	@cp README.md $(DIST_DIR)/macos/docs/
	@cp DEPLOY.md $(DIST_DIR)/macos/docs/
	@echo "macOS native build complete: $(DIST_DIR)/macos/"

# ═══════════════════════════════════════════════════════════
# All platforms
# ═══════════════════════════════════════════════════════════
all-platforms: check-env
	@echo "==> Building for all available platforms"
	@echo ""
	@$(MAKE) linux  || echo "! Linux build skipped"
	@echo ""
	-@$(MAKE) windows || echo "! Windows build skipped"
	@echo ""
	-@$(MAKE) macos || echo "! macOS build skipped"
	@echo ""
	@echo "All builds completed"
	@ls -la $(DIST_DIR)/*/bin/ 2>/dev/null || echo "No builds produced"

# ═══════════════════════════════════════════════════════════
# Package — create distributable archives
# ═══════════════════════════════════════════════════════════
package: linux
	@echo "==> Packaging"
	@mkdir -p $(DIST_DIR)/packages
	@cd $(DIST_DIR) && tar czf packages/$(PKG_NAME)-$(VERSION)-linux-$(ARCH).tar.gz linux/
	@echo "  * packages/$(PKG_NAME)-$(VERSION)-linux-$(ARCH).tar.gz"
	@if [ -d "$(DIST_DIR)/windows" ]; then \
		cd $(DIST_DIR) && zip -qr packages/$(PKG_NAME)-$(VERSION)-windows-amd64.zip windows/; \
		echo "  * packages/$(PKG_NAME)-$(VERSION)-windows-amd64.zip"; \
	fi
	@if [ -d "$(DIST_DIR)/macos" ]; then \
		cd $(DIST_DIR) && tar czf packages/$(PKG_NAME)-$(VERSION)-macos-amd64.tar.gz macos/; \
		echo "  * packages/$(PKG_NAME)-$(VERSION)-macos-amd64.tar.gz"; \
	fi
	@echo "All packages in $(DIST_DIR)/packages/"
	@ls -lh $(DIST_DIR)/packages/

# ═══════════════════════════════════════════════════════════
# Build and test on current host
# ═══════════════════════════════════════════════════════════
build:
	cargo build --workspace

release:
	cargo build --release --workspace

test:
	cargo test --workspace

clippy:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --check

check: test clippy fmt
	@echo "All checks passed"

# ═══════════════════════════════════════════════════════════
# Install cross-compilation toolchains
# ═══════════════════════════════════════════════════════════
install-mingw:
	@echo "==> Installing MinGW cross-compiler"
	@if command -v apt-get >/dev/null 2>&1; then \
		sudo apt-get install -y mingw-w64; \
	elif command -v dnf >/dev/null 2>&1; then \
		sudo dnf install -y mingw64-gcc; \
	elif command -v pacman >/dev/null 2>&1; then \
		sudo pacman -S --noconfirm mingw-w64-gcc; \
	else \
		echo "Unsupported package manager. Install mingw-w64 manually."; \
		exit 1; \
	fi
	rustup target add x86_64-pc-windows-gnu
	@echo "MinGW installed. Run 'make windows' to cross-compile."

install-osxcross:
	@echo "==> Installing osxcross (macOS cross-compiler)"
	@echo "  Clone: git clone https://github.com/tpoechtrager/osxcross.git"
	@echo "  Build: cd osxcross && ./build.sh"
	@echo "  Requires: macOS Xcode SDK (download separately)"
	@rustup target add x86_64-apple-darwin

# ═══════════════════════════════════════════════════════════
# Clean
# ═══════════════════════════════════════════════════════════
clean:
	cargo clean
	rm -rf $(DIST_DIR)

distclean: clean
	rm -rf $(DIST_DIR)/packages
	rm -f Cargo.lock

# ═══════════════════════════════════════════════════════════
# Help
# ═══════════════════════════════════════════════════════════
help:
	@echo "══════════════════════════════════════════════"
	@echo "  QVOD Build System"
	@echo "══════════════════════════════════════════════"
	@echo ""
	@echo "  Build targets:"
	@echo "    make linux          Linux (musl, fully static)"
	@echo "    make windows        Windows (cross, needs MinGW)"
	@echo "    make macos          macOS (cross, needs osxcross)"
	@echo "    make all-platforms  Build for all available"
	@echo ""
	@echo "  Package targets:"
	@echo "    make package        Build Linux + package archives"
	@echo ""
	@echo "  Development:"
	@echo "    make build          cargo build --workspace"
	@echo "    make release        cargo build --release"
	@echo "    make test           cargo test --workspace"
	@echo "    make clippy         cargo clippy"
	@echo "    make fmt            cargo fmt --check"
	@echo "    make check          test + clippy + fmt"
	@echo ""
	@echo "  Toolchain setup:"
	@echo "    make install-mingw    Install MinGW cross-compiler"
	@echo "    make install-osxcross  Install osxcross"
	@echo "    make list-targets     List Rust targets"
	@echo ""
	@echo "  Cleanup:"
	@echo "    make clean         cargo clean"
	@echo "    make distclean     clean + remove packages"
	@echo ""

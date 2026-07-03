# ═══════════════════════════════════════════════════════════
# QVOD — Cross-Platform Build & Package Makefile
#
# Package structure:
#   server:        qvs-server + qvs-cli (Linux only)
#   gui-linux:     qvs-gui (Linux)
#   gui-windows:   qvs-gui.exe (Windows)
#   gui-macos:     qvs-gui (macOS)
#
# Usage:
#   make server                          # Build server package
#   make gui-linux   SERVER_URL=http://...  # GUI with server URL
#   make gui-windows SERVER_URL=http://...  # Windows GUI
#   make package-all SERVER_URL=http://...  # Everything
# ═══════════════════════════════════════════════════════════

PKG_NAME   := qvs
VERSION    := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*= "//;s/"//')
GIT_HASH   := $(shell git log --pretty=format:'%h' -n 1 2>/dev/null || echo "unknown")
ARCH       := $(shell uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')
SERVER_URL ?= ""   # e.g. http://192.168.1.100:8621

# ── Target directories ────────────────────────────────────
LINUX_DIR  := target/x86_64-unknown-linux-musl/release
WIN_DIR    := target/x86_64-pc-windows-gnu/release
MAC_DIR    := target/x86_64-apple-darwin/release
DIST_DIR   := dist

# ── Find the host OS ──────────────────────────────────────
UNAME_S    := $(shell uname -s 2>/dev/null || echo "Unknown")
UNAME_M    := $(shell uname -m 2>/dev/null || echo "Unknown")

.PHONY: all server gui-linux gui-windows gui-macos package-all \
        check-env list-targets install-mingw clean distclean help

all: check-env
	@echo "=== QVOD Build System v$(VERSION) ($(GIT_HASH)) ==="
	@echo "Host: $(UNAME_S) $(UNAME_M)"
	@echo ""
	@echo "Package targets:"
	@echo "  make server              Server: qvs-server + qvs-cli (Linux)"
	@echo "  make gui-linux           GUI player (Linux)"
	@echo "  make gui-windows         GUI player (Windows, needs MinGW)"
	@echo "  make gui-macos           GUI player (macOS, needs osxcross)"
	@echo "  make package-all SERVER_URL=http://...   All packages"
	@echo ""
	@echo "Server URL can be baked into GUI:"
	@echo "  make gui-linux SERVER_URL=http://192.168.1.100:8621"
	@echo ""

check-env:
	@which rustc >/dev/null 2>&1 || { echo "rustc not found. Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; exit 1; }

list-targets:
	@rustup target list --installed
	@echo ""
	@echo "Available:"
	@echo "  windows:  rustup target add x86_64-pc-windows-gnu"
	@echo "  macos:    rustup target add x86_64-apple-darwin"

# ============================================================
# Server — Linux only (qvs-server + qvs-cli)
# ============================================================
server: check-env
	@echo "==> Building server binaries"
	cargo build --release -p qvs-server -p qvs-cli 2>&1 | tail -3
	@echo "==> Packaging server"
	@mkdir -p $(DIST_DIR)/packages
	$(eval DIR := $(DIST_DIR)/$(PKG_NAME)-server-$(VERSION)-linux-$(ARCH))
	@rm -rf $(DIR)
	@mkdir -p $(DIR)/bin $(DIR)/config $(DIR)/systemd $(DIR)/scripts
	cp target/release/qvs-server $(DIR)/bin/
	cp target/release/qvs-cli   $(DIR)/bin/
	@echo '[network]' > $(DIR)/config/config.toml
	@echo 'listen_port = 8621' >> $(DIR)/config/config.toml
	@echo 'max_connections = 200' >> $(DIR)/config/config.toml
	@echo '[buffer]' >> $(DIR)/config/config.toml
	@echo 'capacity_mb = 256' >> $(DIR)/config/config.toml
	@echo '[cache]' >> $(DIR)/config/config.toml
	@echo 'max_size_gb = 20' >> $(DIR)/config/config.toml
	cd $(DIST_DIR) && tar czf packages/$(PKG_NAME)-server-$(VERSION)-linux-$(ARCH).tar.gz $(PKG_NAME)-server-$(VERSION)-linux-$(ARCH)/
	@$(RM) -r $(DIR)
	@ls -lh $(DIST_DIR)/packages/$(PKG_NAME)-server-$(VERSION)-linux-$(ARCH).tar.gz

# ============================================================
# GUI — Linux
# ============================================================
gui-linux: check-env
	@echo "==> Building Linux GUI"$(if $(SERVER_URL)," with server URL: $(SERVER_URL)")
	QVS_SERVER_URL="$(SERVER_URL)" cargo build --release -p qvs-gui 2>&1 | tail -3
	@echo "==> Packaging"
	@mkdir -p $(DIST_DIR)/packages
	$(eval SUFFIX := $(if $(SERVER_URL),gui-linux-$(ARCH)-server,gui-linux-$(ARCH)))
	$(eval DIR := $(DIST_DIR)/$(PKG_NAME)-$(SUFFIX)-$(VERSION))
	@rm -rf $(DIR)
	@mkdir -p $(DIR)/bin $(DIR)/scripts $(DIR)/share/applications $(DIR)/share/icons/hicolor/scalable/apps
	cp target/release/qvs-gui $(DIR)/bin/
	ln -sf qvs-gui $(DIR)/bin/qvs
	cp assets/qvs.desktop $(DIR)/share/applications/
	cp assets/qvs-qvod-handler.desktop $(DIR)/share/applications/
	cp assets/icon.svg $(DIR)/share/icons/hicolor/scalable/apps/
	@echo '#!/usr/bin/env bash' > $(DIR)/scripts/start-player.sh
	@echo 'exec "$$(dirname "$$0")/../bin/qvs-gui"' $(if $(SERVER_URL),'--server-url "$(SERVER_URL)"',) '"$$@"' >> $(DIR)/scripts/start-player.sh
	@chmod +x $(DIR)/scripts/start-player.sh
	cd $(DIST_DIR) && tar czf packages/$(PKG_NAME)-$(SUFFIX)-$(VERSION).tar.gz $(PKG_NAME)-$(SUFFIX)-$(VERSION)/
	@$(RM) -r $(DIR)
	@ls -lh $(DIST_DIR)/packages/$(PKG_NAME)-$(SUFFIX)-$(VERSION).tar.gz

# ============================================================
# GUI — Windows (cross-compile)
# ============================================================
gui-windows: check-env
	@rustup target list --installed | grep -q x86_64-pc-windows-gnu || rustup target add x86_64-pc-windows-gnu
	@which x86_64-w64-mingw32-gcc >/dev/null 2>&1 || { echo "MinGW required: sudo apt install mingw-w64"; exit 1; }
	@echo "==> Building Windows GUI"$(if $(SERVER_URL)," with server URL: $(SERVER_URL)")
	QVS_SERVER_URL="$(SERVER_URL)" cargo build --release --target x86_64-pc-windows-gnu -p qvs-gui 2>&1 | tail -3
	@echo "==> Packaging"
	@mkdir -p $(DIST_DIR)/packages
	$(eval SUFFIX := $(if $(SERVER_URL),gui-windows-x86_64-server,gui-windows-x86_64))
	$(eval DIR := $(DIST_DIR)/$(PKG_NAME)-$(SUFFIX)-$(VERSION))
	@rm -rf $(DIR)
	@mkdir -p $(DIR)/bin $(DIR)/scripts
	cp target/x86_64-pc-windows-gnu/release/qvs-gui.exe $(DIR)/bin/
	cp qvod.png $(DIR)/bin/qvs.png
	cp assets/qvod.ico $(DIR)/bin/qvs.ico
	@echo '@echo off' > $(DIR)/scripts/start-player.bat
	@echo 'start "" "%~dp0..\bin\qvs-gui.exe"' $(if $(SERVER_URL),'--server-url "$(SERVER_URL)"',) >> $(DIR)/scripts/start-player.bat
	cd $(DIST_DIR) && zip -qr packages/$(PKG_NAME)-$(SUFFIX)-$(VERSION).zip $(PKG_NAME)-$(SUFFIX)-$(VERSION)/
	@$(RM) -r $(DIR)
	@ls -lh $(DIST_DIR)/packages/$(PKG_NAME)-$(SUFFIX)-$(VERSION).zip

# ============================================================
# GUI — macOS (cross-compile)
# ============================================================
gui-macos: check-env
	@rustup target list --installed | grep -q x86_64-apple-darwin || rustup target add x86_64-apple-darwin
	@echo "==> Building macOS GUI"$(if $(SERVER_URL)," with server URL: $(SERVER_URL)")
	QVS_SERVER_URL="$(SERVER_URL)" cargo build --release --target x86_64-apple-darwin -p qvs-gui 2>&1 | tail -3
	@echo "==> Packaging"
	@mkdir -p $(DIST_DIR)/packages
	$(eval SUFFIX := $(if $(SERVER_URL),gui-macos-x86_64-server,gui-macos-x86_64))
	$(eval DIR := $(DIST_DIR)/$(PKG_NAME)-$(SUFFIX)-$(VERSION))
	@rm -rf $(DIR)
	@mkdir -p $(DIR)/bin
	cp target/x86_64-apple-darwin/release/qvs-gui $(DIR)/bin/
	cd $(DIST_DIR) && tar czf packages/$(PKG_NAME)-$(SUFFIX)-$(VERSION).tar.gz $(PKG_NAME)-$(SUFFIX)-$(VERSION)/
	@$(RM) -r $(DIR)
	@ls -lh $(DIST_DIR)/packages/$(PKG_NAME)-$(SUFFIX)-$(VERSION).tar.gz

# ============================================================
# Package all
# ============================================================
package-all: server gui-linux gui-windows
	@echo "==> All packages:"
	@ls -lh $(DIST_DIR)/packages/

# ============================================================
# Development
# ============================================================
build: check-env
	cargo build --workspace

release: check-env
	cargo build --release --workspace

test: check-env
	cargo test --workspace

clippy: check-env
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --check

check: test clippy fmt
	@echo "All checks passed"

install-mingw:
	@if command -v apt-get >/dev/null 2>&1; then sudo apt-get install -y mingw-w64; \
	elif command -v dnf >/dev/null 2>&1; then sudo dnf install -y mingw64-gcc; \
	elif command -v pacman >/dev/null 2>&1; then sudo pacman -S --noconfirm mingw-w64-gcc; \
	else echo "Unsupported package manager"; exit 1; fi
	rustup target add x86_64-pc-windows-gnu

# ============================================================
# Clean
# ============================================================
clean:
	cargo clean
	rm -rf $(DIST_DIR)

distclean: clean
	rm -f Cargo.lock

help:
	@echo "═══════════════════════════════════════════"
	@echo "  QVOD Build System"
	@echo "═══════════════════════════════════════════"
	@echo ""
	@echo "  Package targets:"
	@echo "    make server                  Server (qvs-server+qvs-cli)"
	@echo "    make gui-linux               Linux GUI"
	@echo "    make gui-windows             Windows GUI (needs MinGW)"
	@echo "    make gui-macos               macOS GUI (needs osxcross)"
	@echo "    make package-all             All packages"
	@echo ""
	@echo "  With server URL (baked in):"
	@echo "    make gui-linux   SERVER_URL=http://192.168.1.100:8621"
	@echo "    make gui-windows SERVER_URL=http://192.168.1.100:8621"
	@echo ""
	@echo "  Development:"
	@echo "    make build       cargo build"
	@echo "    make release     cargo build --release"
	@echo "    make test        cargo test"
	@echo "    make clippy      cargo clippy"
	@echo "    make check       test + clippy + fmt"
	@echo "    make clean       cargo clean"
	@echo ""

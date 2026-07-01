# Deployment Specification

## Overview

This document covers everything required to build, install, configure, and deploy the QVOD P2SP streaming player across Linux, macOS, and Windows. It includes build instructions, installation packages, configuration file format, protocol handler registration, first-run flow, troubleshooting, and operational guidance.

**Target Runtime Environments:**

| Platform | Minimum OS | Architecture |
|----------|-----------|-------------|
| Linux | Ubuntu 20.04+, Fedora 36+, Debian 11+, Arch (rolling) | x86_64, aarch64 |
| macOS | macOS 11 (Big Sur)+ | x86_64, arm64 (Apple Silicon) |
| Windows | Windows 10 1809+, Windows Server 2019+ | x86_64, aarch64 |

---

## 1. System Requirements

### 1.1 Minimum Requirements

| Component | Minimum | Recommended |
|-----------|---------|------------|
| CPU | 2 cores, 1.5 GHz | 4+ cores, 2.5 GHz |
| RAM | 256 MB | 1 GB |
| Disk | 200 MB (application) + 1 GB cache | 10 GB+ cache |
| Network | Broadband (5 Mbps) | 50+ Mbps |
| GPU | Any (software decode fallback) | Hardware decode capable |

### 1.2 Required Dependencies

#### Rust Toolchain

The project requires Rust 2024 edition. Install via rustup:

```bash
# Install rustup (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Ensure the correct toolchain
rustup toolchain install stable
rustup default stable
```

#### FFmpeg Libraries (runtime required for media playback)

**Linux (Ubuntu/Debian):**
```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libswscale-dev \
    libavfilter-dev \
    libavdevice-dev \
    libswresample-dev \
    libclang-dev \
    libgtk-3-dev \
    libxdo-dev \
    libasound2-dev \
    libpulse-dev
```

**Linux (Fedora/RHEL):**
```bash
sudo dnf install -y \
    gcc \
    pkg-config \
    openssl-devel \
    ffmpeg-devel \
    ffmpeg-libs \
    clang-devel \
    gtk3-devel \
    libxdo-devel \
    alsa-lib-devel \
    pulseaudio-libs-devel
```

**macOS (Homebrew):**
```bash
brew install ffmpeg pkg-config
```

**Windows:**
```powershell
# Option 1: Download FFmpeg shared builds from https://www.gyan.dev/ffmpeg/builds/
# Add bin/ directory to PATH

# Option 2: Using vcpkg
vcpkg install ffmpeg:x64-windows
```

#### Build-time Dependencies

```bash
# All platforms
cargo install cargo-audit    # Security auditing (optional)
cargo install cargo-llvm-cov # Coverage reporting (optional)
```

---

## 2. Build Commands

### 2.1 Standard Build

```bash
# Clone the repository
git clone https://github.com/example/qvs.git
cd qvs

# Build everything (debug)
cargo build

# Build with optimizations (release)
cargo build --release

# Build a specific crate
cargo build --package qvs-core
cargo build --package qvs-gui --release
```

### 2.2 Feature Flags

```toml
# Cargo.toml features
[features]
default = ["gui"]
gui = ["qvs-gui"]
cli = ["qvs-cli"]
prometheus = ["qvs-stream/prometheus", "qvs-local-server/prometheus"]
static-ffmpeg = ["qvs-media/static"]  # Statically link FFmpeg
minimal = []                          # No GUI, no media, engine only
```

```bash
# Build with Prometheus support
cargo build --release --features prometheus

# Build minimal (headless engine only, no GUI/media dependencies)
cargo build --release --no-default-features --features cli

# Build with statically linked FFmpeg
cargo build --release --features static-ffmpeg
```

### 2.3 Cross-Compilation

#### Linux → aarch64 (cross-compile on x86_64)

```bash
# Install cross-compilation toolchain
sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu

# Install aarch64 FFmpeg libraries
sudo apt-get install -y libavcodec-dev:arm64 libavformat-dev:arm64 \
    libavutil-dev:arm64 libswscale-dev:arm64

# Build
cargo build --release --target aarch64-unknown-linux-gnu
```

#### macOS → Apple Silicon (build on Apple Silicon natively)

```bash
# For x86_64 target on Apple Silicon
cargo build --release --target x86_64-apple-darwin
```

#### Using cross-rs (Docker-based cross-compilation)

```bash
# Install cross
cargo install cross

# Build for aarch64 Linux
cross build --release --target aarch64-unknown-linux-gnu

# Build for x86_64 Windows
cross build --release --target x86_64-pc-windows-gnu

# Build for arm64 macOS
cross build --release --target aarch64-apple-darwin
```

### 2.4 Build Artifacts

```bash
# Output locations
target/release/qvs            # GUI player (Linux/macOS)
target/release/qvs.exe        # GUI player (Windows)
target/release/qvs-cli        # CLI-only binary
target/release/qvs-cli.exe    # CLI-only binary (Windows)
target/release/libqvs_core.rlib   # Static library
target/release/libqvs_core.so     # Shared library (Linux)
target/release/libqvs_core.dylib  # Shared library (macOS)
target/release/qvs_core.dll       # Shared library (Windows)
```

### 2.5 Verification

```bash
# Verify the build
cargo build --release
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check

# Show binary info
file target/release/qvs
ldd target/release/qvs          # Linux: verify linked libraries
otool -L target/release/qvs     # macOS: verify linked libraries
```

---

## 3. Installation

### 3.1 Linux

#### AppImage

```yaml
# qvs.appimage.yml (AppImage configuration)
app: qvs
linux:
  target: x86_64-unknown-linux-gnu
  icon: assets/icon.png
  categories: AudioVideo;Player;
  desktop:
    Name: QVOD Player
    Comment: P2SP Streaming Media Player
    Exec: qvs %U
    Terminal: false
    Type: Application
    MimeType: x-scheme-handler/qvod;
```

```bash
# Build AppImage (requires linuxdeploy)
cargo build --release
linuxdeploy --appdir AppDir --executable target/release/qvs --desktop-file qvs.desktop \
    --icon-file assets/icon.png --output appimage
# Produces: QVOD_Player-x86_64.AppImage
```

#### .deb Package

```bash
# Build .deb using cargo-deb
cargo install cargo-deb
cargo deb --package qvs-gui

# Manual .deb structure
mkdir -p debian/usr/bin
mkdir -p debian/usr/share/applications
mkdir -p debian/usr/share/icons/hicolor/256x256/apps
mkdir -p debian/DEBIAN

cp target/release/qvs debian/usr/bin/
cat > debian/usr/share/applications/qvs.desktop << 'EOF'
[Desktop Entry]
Name=QVOD Player
Comment=P2SP Streaming Media Player
Exec=qvs %U
Icon=qvs
Terminal=false
Type=Application
Categories=AudioVideo;Player;
MimeType=x-scheme-handler/qvod;
EOF

cp assets/icon.png debian/usr/share/icons/hicolor/256x256/apps/qvs.png

cat > debian/DEBIAN/control << 'EOF'
Package: qvs-player
Version: 0.1.0
Section: video
Priority: optional
Architecture: amd64
Depends: libavcodec59 (>= 7:6.0), libavformat59 (>= 7:6.0), libavutil57 (>= 7:6.0),
         libswscale6 (>= 7:6.0), libc6 (>= 2.35), libssl3 (>= 3.0),
         libasound2 (>= 1.2), libpulse0 (>= 15.0)
Maintainer: QVOD Team <team@qvod.example.com>
Description: QVOD P2SP Streaming Media Player
 A cross-platform P2SP streaming system with support for qvod://
 protocol, DHT peer discovery, and media playback.
Homepage: https://qvod.example.com
EOF

dpkg-deb --build debian qvs-player_0.1.0_amd64.deb
```

#### .rpm Package

```bash
# Build .rpm using cargo-rpm
cargo install cargo-rpm
cargo rpm --package qvs-gui

# Or use rpmbuild
cat > qvs.spec << 'EOF'
Name: qvs-player
Version: 0.1.0
Release: 1%{?dist}
Summary: QVOD P2SP Streaming Media Player
License: MIT
URL: https://qvod.example.com
Source0: qvs-%{version}.tar.gz
BuildRequires: rust >= 1.80, cargo, pkgconfig, openssl-devel, ffmpeg-devel
Requires: ffmpeg-libs >= 6.0, openssl >= 3.0, alsa-lib, pulseaudio-libs

%description
QVOD P2SP Streaming Media Player - a cross-platform P2SP streaming system.

%build
cargo build --release

%install
mkdir -p %{buildroot}%{_bindir}
install -m 755 target/release/qvs %{buildroot}%{_bindir}/qvs
mkdir -p %{buildroot}%{_datadir}/applications
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/256x256/apps
install -m 644 assets/qvs.desktop %{buildroot}%{_datadir}/applications/
install -m 644 assets/icon.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/qvs.png

%files
%{_bindir}/qvs
%{_datadir}/applications/qvs.desktop
%{_datadir}/icons/hicolor/256x256/apps/qvs.png

%changelog
* Mon Jul 1 2026 QVOD Team - 0.1.0
- Initial release
EOF

rpmbuild -ba qvs.spec
```

### 3.2 macOS

#### .dmg Package

```bash
# Build .dmg using cargo-bundle
cargo install cargo-bundle

# Add to Cargo.toml
# [package.metadata.bundle]
# name = "QVOD Player"
# identifier = "com.qvod.player"
# icon = ["assets/icon.icns"]
# bundle-category = "public.app-category.video"

cargo bundle --package qvs-gui --release
# Produces: target/release/bundle/osx/QVOD Player.app
```

```bash
# Create DMG manually
mkdir -p dmg
cp -r "target/release/bundle/osx/QVOD Player.app" dmg/
ln -s /Applications dmg/Applications

hdiutil create -volname "QVOD Player" -srcfolder dmg -ov \
    -format UDZO "QVOD_Player-0.1.0.dmg"

# Notarize (for macOS Gatekeeper)
xcrun notarytool submit "QVOD_Player-0.1.0.dmg" \
    --apple-id user@example.com \
    --team-id TEAMID \
    --password @keychain:AC_PASSWORD \
    --wait
xcrun stapler staple "QVOD_Player-0.1.0.dmg"
```

#### Homebrew Tap

```ruby
# qvs.rb (Homebrew formula)
class Qvs < Formula
  desc "QVOD P2SP Streaming Media Player"
  homepage "https://qvod.example.com"
  url "https://github.com/example/qvs/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "abc123..."
  license "MIT"

  depends_on "rust" => :build
  depends_on "ffmpeg"
  depends_on "pkg-config"

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    system "#{bin}/qvs", "--version"
  end
end
```

```bash
# Install via Homebrew
brew tap example/qvs
brew install qvs
```

### 3.3 Windows

#### MSI Installer

```bash
# Using wix (Windows)
cargo install cargo-wix
cargo wix --package qvs-gui --release

# Using nsis
makensis installer.nsi
```

```nsis
; installer.nsi (NSIS script)
!include "MUI2.nsh"
!include "FileFunc.nsh"

Name "QVOD Player"
OutFile "QVOD_Player-0.1.0-x86_64.exe"
InstallDir "$PROGRAMFILES64\QVOD Player"
RequestExecutionLevel admin

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

Section "Install"
  SetOutPath "$INSTDIR"
  File "target/release/qvs.exe"
  File "target/release/*.dll"
  
  ; Install FFmpeg DLLs
  File /r "ffmpeg_bin\*.dll"
  
  ; Create shortcuts
  CreateShortCut "$SMPROGRAMS\QVOD Player.lnk" "$INSTDIR\qvs.exe"
  CreateShortCut "$DESKTOP\QVOD Player.lnk" "$INSTDIR\qvs.exe"
  
  ; Register qvod:// protocol handler
  WriteRegStr HKCR "qvod" "" "URL:QVOD Protocol"
  WriteRegStr HKCR "qvod" "URL Protocol" ""
  WriteRegStr HKCR "qvod\shell\open\command" "" '"$INSTDIR\qvs.exe" "%1"'
  
  ; Add to PATH (optional)
  WriteRegStr HKLM "SYSTEM\CurrentControlSet\Control\Session Manager\Environment" \
      "PATH" "$INSTDIR;$PATH"
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\QVOD Player.lnk"
  Delete "$DESKTOP\QVOD Player.lnk"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCR "qvod"
SectionEnd
```

---

## 4. Configuration File

The configuration file uses TOML format. Default location:

| Platform | Path |
|----------|------|
| Linux | `~/.config/qvs/config.toml` |
| macOS | `~/Library/Application Support/com.qvod.player/config.toml` |
| Windows | `%APPDATA%\QVOD Player\config.toml` |

### 4.1 Full Configuration Reference

```toml
# ============================================================================
# QVOD Player Configuration File
# ============================================================================

# ---- General Settings ----

[general]
# Application language (zh-CN, en-US)
language = "en-US"

# Check for updates on startup
check_updates = true

# Enable crash reporting
crash_reporting = true

# Log level: trace, debug, info, warn, error
log_level = "info"

# Log file path (empty = stdout only)
log_file = "~/.local/share/qvs/logs/qvs.log"


# ---- Playback Settings ----

[playback]
# Default volume (0.0 = mute, 1.0 = max)
default_volume = 0.8

# Remember playback position per file and auto-resume
remember_position = true

# Enable gapless playback between tracks
gapless_playback = false

# Default playback speed multiplier
default_speed = 1.0

# Audio device name (empty = default)
audio_device = ""

# Audio output sample rate (0 = auto-detect)
sample_rate = 48000

# Audio output channels (0 = auto-detect)
channels = 2


# ---- Network Settings ----

[network]
# Listen port for local HTTP server (0 = auto-assign)
listen_port = 8621

# UDP port for DHT and peer communication (0 = auto-assign)
udp_port = 8622

# Maximum number of concurrent peer connections
max_connections = 50

# Maximum number of upload slots (peers we unchoke)
max_upload_slots = 5

# Enable DHT peer discovery
enable_dht = true

# Enable HTTP tracker
enable_tracker = true

# Enable HTTP source fallback (for direct HTTP links)
enable_http_fallback = true

# Download rate limit in bytes/sec (0 = unlimited)
download_rate_limit = 0

# Upload rate limit in bytes/sec (0 = unlimited)
upload_rate_limit = 0

# Peer connection timeout in seconds
peer_timeout_secs = 120

# Handshake timeout in seconds
handshake_timeout_secs = 30

# Read/write timeout in seconds
read_write_timeout_secs = 30

# Enable NAT-PMP / UPnP for port forwarding
enable_port_forwarding = true

# Enable TCP_NODELAY on peer connections
tcp_no_delay = true

# Number of ports to try when auto-assigning
port_retry_count = 10


# ---- Buffer Settings ----

[buffer]
# Buffer capacity in megabytes
capacity_mb = 64

# Low watermark (percentage of capacity, triggers buffering)
watermark_low = 0.1

# High watermark (percentage of capacity, stops aggressive download)
watermark_high = 0.8

# Minimum playable duration in seconds before starting playback
min_playable_secs = 1

# Enable adaptive buffer sizing based on network speed
adaptive = true

# Maximum buffer size when adaptive mode is active (MB)
adaptive_max_mb = 256

# Minimum buffer size when adaptive mode is active (MB)
adaptive_min_mb = 8

# Piece length in bytes (256KB default)
piece_length = 262144

# Block length in bytes (16KB default)
block_length = 16384

# Maximum outstanding requests per peer
max_pipeline_depth = 5


# ---- Cache Settings ----

[cache]
# Cache directory path
# Linux:   ~/.local/share/qvod/cache/
# macOS:   ~/Library/Caches/com.qvod.player/
# Windows: %LOCALAPPDATA%\QVOD Player\cache\
directory = ""

# Maximum cache size in gigabytes
max_size_gb = 4

# Auto-cleanup when usage exceeds this percentage
auto_cleanup = true
cleanup_threshold_pct = 90

# Target usage percentage after cleanup
cleanup_target_pct = 70

# Cache file format (sparse = efficient for large files; plain = simple)
# Sparse files reduce disk usage for partially downloaded content.
file_format = "sparse"

# Enable read-ahead cache: reads subsequent blocks on cache hit
read_ahead = true
read_ahead_blocks = 4


# ---- Tracker Settings ----

[tracker]
# Tracker announce URLs (list)
urls = [
    "http://tracker.qvod.example.com/announce",
    "udp://tracker.qvod.example.com:6969/announce",
]

# Interval in seconds between regular announces
announce_interval_secs = 1800

# Minimum announce interval in seconds
min_interval_secs = 900

# Number of peers to request from tracker
num_want = 50

# Prefer compact peer responses (6 bytes per peer)
compact = true

# Request scrape info on announce
scrape = true


# ---- DHT Settings ----

[dht]
# DHT seed nodes (host:port format)
seed_nodes = [
    "dht.qvod.example.com:8622",
    "dht2.qvod.example.com:8622",
    "router.bittorrent.com:6881",    # Public DHT bootstrap
    "dht.transmissionbt.com:6881",   # Public DHT bootstrap
]

# Kademlia K (bucket size)
k = 8

# Kademlia alpha (concurrent queries)
alpha = 3

# Bucket refresh interval in seconds
refresh_interval_secs = 900

# Peer timeout in seconds (remove from routing table)
peer_timeout_secs = 1800

# Maximum peers to store per info_hash
max_peers_per_hash = 50

# Token secret rotation interval in seconds
token_secret_rotation_secs = 600


# ---- HTTP Fallback Settings ----

[http_fallback]
# Enable HTTP source fallback
enabled = true

# HTTP sources for fallback (queried when P2P peers are insufficient)
# These are indexed by info_hash. Configured via external manifest.
# sources = ["https://cdn.example.com/{info_hash}/"]

# Fallback timeout in milliseconds
timeout_ms = 5000

# Maximum retries per source
max_retries = 3

# List of user-agent strings to rotate
user_agents = [
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
    "VLC/3.0.20 LibVLC/3.0.20",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
]


# ---- Proxy Settings ----

[proxy]
# Enable SOCKS5/HTTP proxy
enabled = false

# Proxy type: "socks5", "http", "https"
proxy_type = "socks5"

# Proxy host
host = "127.0.0.1"

# Proxy port
port = 1080

# Proxy username (optional)
username = ""

# Proxy password (optional)
password = ""

# Proxy DNS through the proxy
proxy_dns = true


# ---- UI Settings ----

[ui]
# Default window width in pixels
window_width = 1280

# Default window height in pixels
window_height = 720

# Start in fullscreen mode
fullscreen = false

# Remember and restore window position
remember_position = true

# Theme: "dark", "light", "system"
theme = "dark"

# Show network status panel by default
show_network_panel = true

# Auto-hide playback controls after idle
auto_hide_controls = true

# Controls idle timeout in seconds
controls_idle_timeout_secs = 3.0

# Font size multiplier (0.8 - 1.5)
font_scale = 1.0

# Show buffering indicator overlay
show_buffering_indicator = true

# Show FPS counter (debug)
show_fps = false


# ---- Advanced / Performance ----

[advanced]
# Number of tokio worker threads (0 = auto-detect)
worker_threads = 0

# UDP socket receive buffer size in bytes (0 = OS default)
udp_recv_buffer_size = 1048576

# UDP socket send buffer size in bytes (0 = OS default)
udp_send_buffer_size = 524288

# TCP socket receive buffer size in bytes (0 = OS default)
tcp_recv_buffer_size = 262144

# TCP socket send buffer size in bytes (0 = OS default)
tcp_send_buffer_size = 262144

# Enable SO_REUSEPORT on UDP sockets (Linux)
reuse_port = false

# Priority of network threads: "realtime", "high", "normal", "low"
thread_priority = "normal"

# Enable detailed connection logging
debug_connections = false

# Enable packet tracing (very verbose)
debug_packets = false
```

### 4.2 Minimal Configuration

```toml
# ~/.config/qvs/config.toml — minimal working config
[network]
listen_port = 8621
udp_port = 8622
max_connections = 50

[buffer]
capacity_mb = 64

[cache]
max_size_gb = 4

[tracker]
urls = ["http://tracker.qvod.example.com/announce"]

[dht]
seed_nodes = ["dht.qvod.example.com:8622"]

[ui]
theme = "dark"
```

### 4.3 Configuration Loading

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub playback: PlaybackConfig,

    #[serde(default)]
    pub network: NetworkConfig,

    #[serde(default)]
    pub buffer: BufferConfig,

    #[serde(default)]
    pub cache: CacheConfig,

    #[serde(default)]
    pub tracker: TrackerConfig,

    #[serde(default)]
    pub dht: DhtConfig,

    #[serde(default)]
    pub http_fallback: HttpFallbackConfig,

    #[serde(default)]
    pub proxy: ProxyConfig,

    #[serde(default)]
    pub ui: UiConfig,

    #[serde(default)]
    pub advanced: AdvancedConfig,
}

impl AppConfig {
    /// Load configuration from default locations
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = Self::config_path()?;

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            let config = AppConfig::default();
            // Write default config on first run
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let content = toml::to_string_pretty(&config)?;
            std::fs::write(&config_path, content)?;
            Ok(config)
        }
    }

    fn config_path() -> Result<PathBuf, ConfigError> {
        let base = if cfg!(target_os = "linux") {
            dirs::config_dir()
                .ok_or(ConfigError::NoConfigDir)?
                .join("qvs")
        } else if cfg!(target_os = "macos") {
            dirs::config_dir()
                .ok_or(ConfigError::NoConfigDir)?
                .join("com.qvod.player")
        } else if cfg!(target_os = "windows") {
            dirs::config_dir()
                .ok_or(ConfigError::NoConfigDir)?
                .join("QVOD Player")
        } else {
            PathBuf::from(".qvs")
        };
        Ok(base.join("config.toml"))
    }

    /// Override from environment variables
    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("QVS_LISTEN_PORT") {
            self.network.listen_port = val.parse().unwrap_or(self.network.listen_port);
        }
        if let Ok(val) = std::env::var("QVS_MAX_CONNECTIONS") {
            self.network.max_connections = val.parse().unwrap_or(self.network.max_connections);
        }
        if let Ok(val) = std::env::var("QVS_CACHE_DIR") {
            self.cache.directory = val.into();
        }
        if let Ok(val) = std::env::var("QVS_LOG_LEVEL") {
            self.general.log_level = val;
        }
        if let Ok(val) = std::env::var("QVS_TRACKER_URLS") {
            self.tracker.urls = val.split(',').map(String::from).collect();
        }
        if let Ok(val) = std::env::var("QVS_DHT_SEED_NODES") {
            self.dht.seed_nodes = val.split(',').map(String::from).collect();
        }
        if let Ok(val) = std::env::var("QVS_PROXY_ENABLED") {
            self.proxy.enabled = val == "1" || val.to_lowercase() == "true";
        }
        if let Ok(val) = std::env::var("QVS_PROXY_HOST") {
            self.proxy.host = val;
        }
        if let Ok(val) = std::env::var("QVS_PROXY_PORT") {
            self.proxy.port = val.parse().unwrap_or(self.proxy.port);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("No config directory found")]
    NoConfigDir,
}
```

---

## 5. Protocol Handler Registration

### 5.1 Linux (qvod://)

```bash
# Desktop entry (via .desktop file)
cat > ~/.local/share/applications/qvs-qvod-handler.desktop << 'EOF'
[Desktop Entry]
Type=Application
Name=QVOD Player (qvod:// handler)
Exec=qvs %u
NoDisplay=true
MimeType=x-scheme-handler/qvod;
EOF

# Register the handler
xdg-mime default qvs-qvod-handler.desktop x-scheme-handler/qvod

# Verify
xdg-open "qvod://A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9|movie.mp4|734003200|mp4|"
```

### 5.2 macOS (qvod://)

```xml
<!-- Info.plist (embedded in .app bundle) -->
<key>CFBundleURLTypes</key>
<array>
    <dict>
        <key>CFBundleURLName</key>
        <string>com.qvod.player</string>
        <key>CFBundleURLSchemes</key>
        <array>
            <string>qvod</string>
        </array>
    </dict>
</array>
```

```bash
# Register after installation
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
    -f /Applications/QVOD\ Player.app
```

### 5.3 Windows (qvod://)

```powershell
# PowerShell script to register protocol handler (run as admin)
$exePath = "$env:ProgramFiles\QVOD Player\qvs.exe"

New-Item -Path "HKCR:\qvod" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\qvod" -Name "(Default)" -Value "URL:QVOD Protocol"
Set-ItemProperty -Path "HKCR:\qvod" -Name "URL Protocol" -Value ""

New-Item -Path "HKCR:\qvod\shell\open\command" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\qvod\shell\open\command" -Name "(Default)" -Value "`"$exePath`" `"%1`""

# Verify
Start-Process "qvod://A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9|movie.mp4|734003200|mp4|"
```

---

## 6. First-Run Setup and Bootstrap

### 6.1 First-Run Flow

```
┌────────────────────────────────────────────────────┐
│                  First Run Flow                     │
├────────────────────────────────────────────────────┤
│                                                     │
│  1. Application launched                             │
│     ├─ Check for config.toml                         │
│     │   ├─ Found → load                              │
│     │   └─ Not found → create default, write it      │
│     │                                                │
│  2. Create cache directories                          │
│     ├─ $CACHE_DIR/qdata/                              │
│     ├─ $CACHE_DIR/qmv/                                │
│     └─ $CACHE_DIR/hls/ (temporary HLS segments)       │
│                                                        │
│  3. Check FFmpeg availability                          │
│     ├─ Available → enable media decoding               │
│     └─ Not found → show warning, disable decode        │
│                                                        │
│  4. Attempt to bind ports                              │
│     ├─ listen_port available → bind                    │
│     ├─ listen_port busy → try next port (up to N)      │
│     └─ All ports busy → show error, use random         │
│                                                        │
│  5. Start network services                              │
│     ├─ Local HTTP server (axum)                        │
│     ├─ DHT node (UDP socket)                           │
│     └─ Connection pool (initialized, empty)            │
│                                                        │
│  6. DHT Bootstrap (background)                         │
│     ├─ Connect to seed nodes                           │
│     ├─ Iterative FIND_NODE to fill routing table       │
│     └─ Schedule periodic refresh                       │
│                                                        │
│  7. Configure logging                                  │
│     ├─ Initialize tracing subscriber                   │
│     └─ Set log level from config                       │
│                                                        │
│  8. GUI initialization                                 │
│     ├─ Create egui window                              │
│     ├─ Load theme                                      │
│     └─ Show main interface                             │
│                                                        │
│  9. Show welcome dialog (first-run only)               │
│     ├─ "Welcome to QVOD Player!"                        │
│     ├─ Brief intro to qvod:// links                    │
│     └─ Option to open test stream                      │
│                                                        │
└────────────────────────────────────────────────────┘
```

### 6.2 Bootstrap Implementation

```rust
pub struct FirstRunSetup;

impl FirstRunSetup {
    pub async fn run(config: &AppConfig) -> Result<SetupResult, SetupError> {
        let mut warnings = Vec::new();

        // Step 1: Create directories
        let dirs = [
            config.cache.effective_directory().join("qdata"),
            config.cache.effective_directory().join("qmv"),
            config.cache.effective_directory().join("hls"),
        ];
        for dir in &dirs {
            tokio::fs::create_dir_all(dir).await.map_err(|e| {
                warnings.push(SetupWarning::CacheDir { path: dir.clone(), error: e.to_string() });
            }).ok();
        }

        // Step 2: Check FFmpeg
        if !check_ffmpeg().await {
            warnings.push(SetupWarning::NoFfmpeg);
        }

        // Step 3: Port availability check
        let listen_port = check_port_available(config.network.listen_port, config.network.port_retry_count).await;
        let udp_port = check_port_available(config.network.udp_port, config.network.port_retry_count).await;

        if listen_port != config.network.listen_port || udp_port != config.network.udp_port {
            warnings.push(SetupWarning::PortChanged {
                old_listen: config.network.listen_port,
                new_listen: listen_port,
                old_udp: config.network.udp_port,
                new_udp: udp_port,
            });
        }

        // Step 4: Apply effective ports
        // (modify a runtime config overlay)

        Ok(SetupResult {
            effective_listen_port: listen_port,
            effective_udp_port: udp_port,
            warnings,
        })
    }

    pub async fn show_welcome_dialog(config: &AppConfig) {
        // Show egui dialog on first run
        // Detected by absence of a "first_run_completed" marker file
        let marker = config.effective_data_dir().join(".first_run");
        if marker.exists() {
            return;
        }

        // ... show welcome dialog ...

        // Mark first run as completed
        std::fs::write(&marker, "").ok();
    }
}

async fn check_ffmpeg() -> bool {
    ffmpeg_next::init().is_ok()
}

async fn check_port_available(preferred: u16, retry_count: u32) -> u16 {
    for port in preferred..(preferred + retry_count) {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => {
                drop(listener);
                return port;
            }
            Err(_) => continue,
        }
    }
    0 // fallback: OS-assigned
}
```

---

## 7. Cache Migration and Cleanup

### 7.1 Cache Directory Structure

```
~/.local/share/qvs/cache/          # Linux
├── qdata/
│   ├── A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9.qdata
│   ├── F0E1D2C3B4A5968778695A4B3C2D1E0F1A2B3C4.qdata
│   └── ...
├── qmv/
│   ├── A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9.qmv
│   ├── F0E1D2C3B4A5968778695A4B3C2D1E0F1A2B3C4.qmv
│   └── ...
└── hls/                           # Temporary HLS segments (cleared on exit)
    ├── A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9/
    │   ├── index.m3u8
    │   ├── segment_0001.ts
    │   └── ...
    └── ...
```

### 7.2 Cache Migration

```rust
pub struct CacheMigration;

impl CacheMigration {
    /// Migrate from old cache format (if any)
    pub async fn migrate(old_dir: &Path, new_dir: &Path) -> Result<MigrationReport, MigrationError> {
        let mut report = MigrationReport {
            files_moved: 0,
            files_skipped: 0,
            errors: Vec::new(),
        };

        // Migrate qdata files
        let old_qdata = old_dir.join("qdata");
        let new_qdata = new_dir.join("qdata");
        if old_qdata.exists() {
            let mut dir = tokio::fs::read_dir(&old_qdata).await?;
            while let Some(entry) = dir.next_entry().await? {
                let name = entry.file_name();
                let src = entry.path();
                let dst = new_qdata.join(&name);

                if dst.exists() {
                    // If destination exists, skip (keep newer one)
                    report.files_skipped += 1;
                    continue;
                }

                tokio::fs::rename(&src, &dst).await.map_err(|e| {
                    report.errors.push((name.to_string_lossy().to_string(), e.to_string()));
                }).ok();
                report.files_moved += 1;
            }
        }

        // Migrate qmv files (metadata)
        let old_qmv = old_dir.join("qmv");
        let new_qmv = new_dir.join("qmv");
        if old_qmv.exists() {
            let mut dir = tokio::fs::read_dir(&old_qmv).await?;
            while let Some(entry) = dir.next_entry().await? {
                let name = entry.file_name();
                let src = entry.path();
                let dst = new_qmv.join(&name);

                if dst.exists() {
                    report.files_skipped += 1;
                    continue;
                }

                tokio::fs::rename(&src, &dst).await.map_err(|e| {
                    report.errors.push((name.to_string_lossy().to_string(), e.to_string()));
                }).ok();
                report.files_moved += 1;
            }
        }

        Ok(report)
    }
}

pub struct MigrationReport {
    pub files_moved: u64,
    pub files_skipped: u64,
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("IO error during migration: {0}")]
    Io(#[from] std::io::Error),
}
```

### 7.3 Cache Cleanup (LRU)

```rust
impl CacheManager {
    /// Clean cache to stay within limits
    pub async fn cleanup(&self) -> Result<CleanupReport> {
        let max_bytes = (self.max_cache_size_gb as u64) * 1024 * 1024 * 1024;
        let threshold_bytes = (max_bytes as f64 * self.cleanup_threshold_pct / 100.0) as u64;
        let target_bytes = (max_bytes as f64 * self.cleanup_target_pct / 100.0) as u64;

        let current = self.calculate_cache_size().await?;
        if current < threshold_bytes {
            return Ok(CleanupReport {
                bytes_freed: 0,
                files_removed: 0,
                current_size: current,
            });
        }

        // Gather entries sorted by last access time (oldest first)
        let mut entries = self.list_cache_entries().await?;
        entries.sort_by_key(|e| e.last_access);

        let mut bytes_to_free = current.saturating_sub(target_bytes);
        let mut files_removed = 0u64;

        for entry in &entries {
            if bytes_to_free == 0 {
                break;
            }

            let file_size = entry.file_size;
            let path = self.qdata_path(&entry.info_hash);
            if tokio::fs::remove_file(&path).await.is_ok() {
                // Also remove metadata
                let mv_path = self.qmv_path(&entry.info_hash);
                tokio::fs::remove_file(&mv_path).await.ok();

                bytes_to_free = bytes_to_free.saturating_sub(file_size);
                files_removed += 1;
            }
        }

        Ok(CleanupReport {
            bytes_freed: current - target_bytes,
            files_removed,
            current_size: self.calculate_cache_size().await?,
        })
    }
}

pub struct CleanupReport {
    pub bytes_freed: u64,
    pub files_removed: u64,
    pub current_size: u64,
}
```

---

## 8. Logging Configuration

### 8.1 Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `QVS_LOG` | `info` | Log level filter (trace/debug/info/warn/error) |
| `QVS_LOG_FILE` | (stdout) | Path to log file |
| `QVS_LOG_FORMAT` | `full` | Log format: `full`, `compact`, `json` |
| `QVS_LOG_COLOR` | `auto` | Color output: `auto`, `always`, `never` |
| `RUST_LOG` | (QVS_LOG) | Standard env_logger override |

### 8.2 Log Levels per Module

```
# Default log level: info
# More verbose for specific modules:
QVS_LOG=info,qvs_transport=debug,qvs_dht=trace,qvs_media=warn
```

### 8.3 Log Rotation

```rust
pub struct LogRotator {
    file: std::fs::File,
    path: PathBuf,
    max_size: u64,          // Max bytes before rotation
    max_files: u32,         // Max rotated files to keep
    current_size: u64,
}

impl LogRotator {
    pub fn new(path: PathBuf, max_size: u64, max_files: u32) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let current_size = file.metadata()?.len();
        Ok(Self { file, path, max_size, max_files, current_size })
    }

    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        self.file.write_all(buf)?;
        self.current_size += buf.len() as u64;

        if self.current_size >= self.max_size {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        // Close current file
        self.file.flush()?;

        // Shift rotated files: .3 → .4, .2 → .3, .1 → .2
        for i in (1..self.max_files).rev() {
            let src = self.path.with_extension(format!("log.{}", i));
            let dst = self.path.with_extension(format!("log.{}", i + 1));
            if src.exists() {
                std::fs::rename(&src, &dst)?;
            }
        }

        // Rename current → .1
        let rotated = self.path.with_extension("log.1");
        std::fs::rename(&self.path, &rotated)?;

        // Open new file
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.current_size = 0;

        Ok(())
    }
}
```

### 8.4 JSON Log Format (for production/ELK)

```json
{
  "timestamp": "2026-07-01T12:34:56.789Z",
  "level": "INFO",
  "module": "qvs_transport::pool",
  "file": "crates/qvs-transport/src/pool.rs:142",
  "thread": "tokio-runtime-worker-3",
  "message": "Added peer to connection pool",
  "fields": {
    "peer_id": "A1B2C3D4",
    "addr": "192.168.1.5:8621",
    "total_peers": 12
  },
  "span": {
    "info_hash": "A1B2C3D4E5F6G7H8I9J0",
    "connection_id": "tx-42"
  }
}
```

---

## 9. Troubleshooting Guide

### 9.1 Common Issues

| Symptom | Likely Cause | Solution |
|---------|-------------|----------|
| `qvod://` links don't open the player | Protocol handler not registered | Run `xdg-mime default qvs-qvod-handler.desktop x-scheme-handler/qvod` (Linux) or reinstall (Windows/macOS) |
| "Failed to bind port" on startup | Port conflict with another application | Change `listen_port` / `udp_port` in config, or let auto-assignment handle it |
| "No peers found" | Tracker unreachable, DHT not bootstrapped | Check network connectivity, wait 30s for DHT bootstrap, enable HTTP fallback |
| "FFmpeg not found" error | FFmpeg libraries not installed | Install ffmpeg via package manager (see Section 1.2) |
| Playback starts but stutters | Slow network, insufficient buffer | Increase `buffer.capacity_mb`, reduce `max_connections` if CPU-bound |
| High memory usage | Large buffer, many peer connections | Reduce `buffer.capacity_mb`, reduce `max_connections` |
| Cache not being written | Permission denied on cache directory | Check `cache.directory` permissions, verify disk space |
| UDP packets not arriving | Firewall blocking UDP | Allow UDP on configured port range, disable firewall temporarily for testing |
| NAT traversal failed | Symmetric NAT, UPnP disabled | Enable UPnP on router, or configure port forwarding manually |
| GUI doesn't launch (Linux) | Missing X11/Wayland display, missing GTK | Install `libgtk-3-dev` / `libxdo-dev`, ensure DISPLAY/WAYLAND_DISPLAY is set |
| CLI reports "Connection refused" | Engine not running | Start GUI first (it runs the engine), or use `qvs play --headless` to start engine-only mode |
| Downloads seem stuck at 99% | Last pieces are rare | Enable `enable_http_fallback` or wait for more peers to join |
| Video plays but no audio | Missing audio codec, wrong audio device | Install full FFmpeg (not minimal), check `playback.audio_device` |
| High CPU usage | Software decoding for unsupported codec | Enable hardware acceleration in FFmpeg, or use a less demanding codec |

### 9.2 Debugging Commands

```bash
# Check installed version
qvs --version

# Verbose logging
QVS_LOG=debug qvs play qvod://...

# Trace-level logging (very verbose)
QVS_LOG=trace,qvs_transport=debug,qvs_dht=debug qvs play qvod://...

# Check DHT status
qvs status

# List active connections
qvs list

# Check cache size and entries
qvs cache info

# Force cache cleanup
qvs cache clean --target-gb 2

# Test port bindings
qvs --test-ports

# Generate diagnostic report
qvs diag > qvs-diag.txt

# Run in headless mode with logging to file
qvs play qvod://... --headless 2>&1 | tee qvs-session.log
```

### 9.3 Diagnostic Report

```rust
// qvs diag command
pub fn generate_diagnostic_report(
    metrics: &MetricsSnapshot,
    config: &AppConfig,
    engine: &QvodEngine,
) -> String {
    let mut report = String::new();
    report.push_str("=== QVOD Player Diagnostic Report ===\n");
    report.push_str(&format!("Version: {}\n", env!("CARGO_PKG_VERSION")));
    report.push_str(&format!("Build: {} ({})\n",
        env!("BUILD_DATE").unwrap_or("unknown"),
        env!("BUILD_PROFILE").unwrap_or("unknown")));
    report.push_str(&format!("Timestamp: {}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));

    report.push_str("--- System ---\n");
    report.push_str(&format!("OS: {}\n", std::env::consts::OS));
    report.push_str(&format!("Arch: {}\n", std::env::consts::ARCH));
    report.push_str(&format!("CPU Cores: {}\n", num_cpus::get()));
    report.push_str(&format!("Memory: {} MB\n",
        sys_info::mem_info().map(|m| m.total / 1024).unwrap_or(0)));
    report.push_str(&format!("FFmpeg: {}\n",
        if ffmpeg_next::init().is_ok() { "available" } else { "NOT FOUND" }));
    report.push_str("\n");

    report.push_str("--- Network ---\n");
    report.push_str(&format!("Engine state: {:?}\n", metrics.engine_state));
    report.push_str(&format!("Listen port: {}\n", config.network.listen_port));
    report.push_str(&format!("UDP port: {}\n", config.network.udp_port));
    report.push_str(&format!("Peers: {}/{} connected/total\n",
        metrics.peers_connected, metrics.peers_total));
    report.push_str(&format!("Download speed: {}/s\n",
        format_bytes(metrics.download_speed)));
    report.push_str(&format!("Upload speed: {}/s\n",
        format_bytes(metrics.upload_speed)));
    report.push_str(&format!("Loss rate: {:.2}%\n", metrics.loss_rate * 100.0));
    report.push_str("\n");

    report.push_str("--- Buffer ---\n");
    report.push_str(&format!("Fill: {:.1}% ({} / {})\n",
        metrics.buffer_fill_pct * 100.0,
        format_bytes(metrics.buffer_fill_bytes as f64),
        format_bytes(metrics.buffer_capacity_bytes as f64)));
    report.push_str(&format!("Playable: {}s\n", metrics.buffer_playable.as_secs()));
    report.push_str("\n");

    report.push_str("--- Cache ---\n");
    report.push_str(&format!("Directory: {}\n",
        config.cache.effective_directory().display()));
    report.push_str(&format!("Max size: {} GB\n", config.cache.max_size_gb));
    report.push_str(&format!("Hit rate: {:.1}%\n", metrics.cache_hit_rate * 100.0));
    report.push_str("\n");

    report.push_str("--- Errors ---\n");
    if metrics.total_errors > 0 {
        report.push_str(&format!("Network: {}\n", metrics.errors_network));
        report.push_str(&format!("Protocol: {}\n", metrics.errors_protocol));
        report.push_str(&format!("Hash fail: {}\n", metrics.errors_hash_fail));
        report.push_str(&format!("Timeout: {}\n", metrics.errors_timeout));
    } else {
        report.push_str("No errors recorded.\n");
    }
    report.push_str("\n");

    report.push_str("--- Active Trackers ---\n");
    for url in &config.tracker.urls {
        report.push_str(&format!("  {}\n", url));
    }
    report.push_str("\n");

    report.push_str("--- DHT Seed Nodes ---\n");
    for node in &config.dht.seed_nodes {
        report.push_str(&format!("  {}\n", node));
    }
    report.push_str("\n");

    report.push_str("--- Peer Details ---\n");
    for peer in &metrics.peers {
        report.push_str(&format!(
            "  {} | {} | speed: {}/s down | rtt: {}ms | progress: {:.1}% | quality: {:.2}\n",
            hex::encode(&peer.peer_id[..8]),
            peer.addr,
            format_bytes(peer.download_speed),
            peer.rtt.as_millis(),
            peer.progress * 100.0,
            peer.quality_score,
        ));
    }

    report
}
```

### 9.4 Log File Locations

| Platform | Log Path |
|----------|----------|
| Linux | `~/.local/share/qvs/logs/qvs.log` |
| macOS | `~/Library/Logs/com.qvod.player/qvs.log` |
| Windows | `%LOCALAPPDATA%\QVOD Player\logs\qvs.log` |

---

## 10. Systemd Service (Headless Server Mode)

For running QVOD as a headless streaming server (CLI mode without GUI):

```ini
# /etc/systemd/system/qvs.service
[Unit]
Description=QVOD P2SP Streaming Engine
After=network.target network-online.target
Wants=network-online.target

[Service]
Type=simple
User=qvs
Group=qvs
ExecStart=/usr/bin/qvs-cli daemon --config /etc/qvs/config.toml
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5
LimitNOFILE=65536
MemoryMax=1G

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
ReadWritePaths=/var/lib/qvs

[Install]
WantedBy=multi-user.target
```

```bash
# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable qvs
sudo systemctl start qvs

# View logs
sudo journalctl -u qvs -f

# Restart with new config
sudo systemctl restart qvs

# Check status
sudo systemctl status qvs
```

---

## Summary

| Package/Task | Command |
|-------------|---------|
| Build release | `cargo build --release` |
| Run tests | `cargo test --workspace` |
| Lint | `cargo clippy --workspace -- -D warnings` |
| Cross-compile aarch64 | `cross build --release --target aarch64-unknown-linux-gnu` |
| Create .deb | `cargo deb --package qvs-gui` |
| Create .rpm | `cargo rpm --package qvs-gui` |
| Create .dmg | `cargo bundle --package qvs-gui --release` |
| Create MSI | `cargo wix --package qvs-gui --release` |
| Create AppImage | `linuxdeploy --appdir AppDir ...` |
| Register qvod:// handler | `xdg-mime default qvs-qvod-handler.desktop x-scheme-handler/qvod` |
| Run headless server | `qvs-cli daemon --config /etc/qvs/config.toml` |
| Generate diagnostics | `qvs diag > report.txt` |
| View logs | `journalctl -u qvs -f` (systemd) or `tail -f ~/.local/share/qvs/logs/qvs.log` |

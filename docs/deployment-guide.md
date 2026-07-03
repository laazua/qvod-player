# QVOD Player 部署指南

> **版本**: 0.1.0 | **更新**: 2026-07-03  
> **兼容**: Linux (x86_64, aarch64) · macOS (x86_64, arm64) · Windows (x86_64)

---

## 目录

1. [系统要求](#1-系统要求)
2. [构建](#2-构建)
3. [打包](#3-打包)
4. [安装](#4-安装)
5. [配置](#5-配置)
6. [部署场景](#6-部署场景)
7. [协议处理器注册](#7-协议处理器注册)
8. [性能调优](#8-性能调优)
9. [故障排除](#9-故障排除)

---

## 1. 系统要求

### 1.1 硬件要求

| 组件 | 最低 | 推荐 |
|------|------|------|
| CPU | 2 核, 1.5 GHz | 4+ 核, 2.5 GHz |
| 内存 | 256 MB | 1 GB+ |
| 磁盘 | 200 MB (程序) + 1 GB (缓存) | 10 GB+ (缓存) |
| 网络 | 宽带 (5 Mbps) | 50+ Mbps |
| GPU | 任意 (支持软件解码回退) | 支持硬件解码 |

### 1.2 依赖安装

#### Linux

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential pkg-config \
    libssl-dev libclang-dev \
    libavcodec-dev libavformat-dev libavutil-dev \
    libswscale-dev libswresample-dev \
    libgtk-3-dev libxdo-dev \
    libasound2-dev libpulse-dev
```

**Fedora/RHEL/Rocky Linux:**
```bash
sudo dnf install -y \
    gcc pkg-config \
    openssl-devel clang-devel \
    ffmpeg-devel ffmpeg-libs \
    gtk3-devel libxdo-devel \
    alsa-lib-devel pulseaudio-libs-devel
```

**Arch Linux:**
```bash
sudo pacman -S --needed \
    base-devel pkg-config \
    openssl clang \
    ffmpeg gtk3 libxdo \
    alsa-lib pulseaudio
```

#### macOS

```bash
# 安装 Homebrew (如未安装)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# 安装依赖
brew install ffmpeg pkg-config
```

#### Windows

**选项 A: vcpkg (推荐)**
```powershell
git clone https://github.com/Microsoft/vcpkg.git
cd vcpkg
.\bootstrap-vcpkg.bat
.\vcpkg install ffmpeg:x64-windows
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
$env:PATH = "$env:VCPKG_ROOT\installed\x64-windows\bin;$env:PATH"
```

**选项 B: 预编译 FFmpeg DLL**
1. 从 [gyan.dev FFmpeg Builds](https://www.gyan.dev/ffmpeg/builds/) 下载 `ffmpeg-release-full.7z`
2. 解压到 `C:\ffmpeg`
3. 将 `C:\ffmpeg\bin` 添加到系统 PATH

**选项 C: 跳过 FFmpeg（GUI 仍可启动，仅视频解码不可用）**

---

## 2. 构建

### 2.1 快速开始

```bash
# 1. 安装 Rust 工具链
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. 克隆仓库
git clone https://github.com/example/qvs.git
cd qvs

# 3. 完整构建（Release 模式）
cargo build --release --workspace

# 4. 运行测试
cargo test --workspace

# 5. Lint 检查
cargo clippy --workspace -- -D warnings
```

### 2.2 构建产物

构建完成后，生成以下二进制文件：

| 二进制 | 路径 | 大小 | 说明 |
|--------|------|------|------|
| `qvs-gui` | `target/release/qvs-gui` | ~17 MB | GUI 播放器（主程序） |
| `qvs-server` | `target/release/qvs-server` | ~9 MB | Headless 服务器 |
| `qvs-cli` | `target/release/qvs-cli` | ~8 MB | 命令行客户端 |

### 2.3 按需构建

```bash
# 仅构建 GUI 播放器（含媒体解码）
cargo build --release -p qvs-gui

# 仅构建 Headless 服务器
cargo build --release -p qvs-server

# 仅构建 CLI 客户端
cargo build --release -p qvs-cli

# 无 FFmpeg 构建（跳过 media crate）
cargo build --release -p qvs-gui -p qvs-server -p qvs-cli
```

### 2.4 交叉编译

#### Linux → aarch64

```bash
# 安装交叉编译工具链
sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu

# 添加 Rust 目标
rustup target add aarch64-unknown-linux-gnu

# 构建
cargo build --release --target aarch64-unknown-linux-gnu
```

#### Linux → Windows (MinGW)

```bash
# 安装 MinGW
sudo apt-get install -y mingw-w64

# 添加 Rust 目标
rustup target add x86_64-pc-windows-gnu

# 构建
cargo build --release --target x86_64-pc-windows-gnu
```

#### 使用 cross-rs（Docker 方式）

```bash
# 安装 cross
cargo install cross

# 交叉编译到各平台
cross build --release --target aarch64-unknown-linux-gnu
cross build --release --target x86_64-pc-windows-gnu
cross build --release --target aarch64-apple-darwin
```

### 2.5 Makefile 构建

项目提供 Makefile 自动化构建：

```bash
# 构建当前平台
make release

# Linux 静态构建 (musl)
make linux

# 跨平台构建（需要对应工具链）
make windows    # 需要 MinGW
make macos      # 需要 osxcross

# 全平台构建
make all-platforms

# 运行检查
make check      # test + clippy + fmt
```

---

## 3. 打包

### 3.1 快速打包

```bash
# 使用打包脚本（自动构建 + 测试 + 打包）
bash scripts/package.sh

# 仅打包（需已构建）
bash scripts/package.sh --package-only

# 仅构建
bash scripts/package.sh --build-only
```

### 3.2 产物清单

打包脚本生成以下内容：

```
dist/
├── packages/
│   ├── qvs-0.1.0-linux-amd64.tar.gz      # 主分发归档 (~13 MB)
│   └── qvs-0.1.0-linux-amd64.sha256      # SHA256 校验和
│
├── qvs-0.1.0-linux-amd64/                # 展开的打包目录
│   ├── bin/
│   │   ├── qvs-gui                       # GUI 播放器
│   │   ├── qvs-server                    # Headless 服务器
│   │   ├── qvs-cli                       # CLI 客户端
│   │   └── qvs → qvs-gui                 # 兼容符号链接
│   ├── config/
│   │   └── config.toml                   # 默认配置文件
│   ├── docs/
│   │   ├── README.md
│   │   ├── DEPLOY.md
│   │   ├── AGENTS.md
│   │   └── agents/                       # 全部技术文档
│   ├── share/
│   │   ├── applications/
│   │   │   ├── qvs.desktop              # 桌面入口
│   │   │   └── qvs-qvod-handler.desktop # qvod:// 协议处理
│   │   └── icons/hicolor/
│   │       └── scalable/apps/qvs.svg    # 图标
│   ├── systemd/
│   │   └── qvs-server.service           # Systemd 服务单元
│   ├── scripts/
│   │   ├── install.sh                   # 一键安装脚本
│   │   └── uninstall.sh                 # 卸载脚本
│   └── RELEASE                          # 版本信息
│
└── qvs.AppDir/                           # AppImage 结构
    ├── AppRun
    └── usr/bin/
        ├── qvs-gui
        ├── qvs-server
        └── qvs-cli
```

### 3.3 Makefile 打包

```bash
# 使用 Makefile 打包
make package    # 构建 Linux + 创建 tar.gz

# 产物在 dist/packages/ 目录
ls -lh dist/packages/
```

### 3.4 平台专属打包

#### AppImage (Linux)

```bash
# 安装 linuxdeploy
wget https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
chmod +x linuxdeploy-x86_64.AppImage

# 生成 AppImage
linuxdeploy-x86_64.AppImage \
    --appdir dist/qvs.AppDir \
    --output appimage
# 产生: QVOD_Player-0.1.0-x86_64.AppImage
```

#### .deb 包 (Debian/Ubuntu)

```bash
# 使用 cargo-deb
cargo install cargo-deb
cargo deb -p qvs-gui

# 或手动构建
mkdir -p debian/usr/bin debian/DEBIAN debian/usr/share/applications
cp target/release/qvs-gui debian/usr/bin/
cp target/release/qvs-cli debian/usr/bin/
cp target/release/qvs-server debian/usr/bin/
cp assets/qvs.desktop debian/usr/share/applications/

cat > debian/DEBIAN/control << 'EOF'
Package: qvs-player
Version: 0.1.0
Section: video
Priority: optional
Architecture: amd64
Depends: libavcodec59, libavformat59, libavutil57, libswscale6,
         libc6 (>= 2.35), libssl3, libasound2, libpulse0
Maintainer: QVOD Team <team@qvod.example.com>
Description: QVOD P2SP 流媒体播放器
 跨平台 P2SP 流媒体系统，支持 qvod:// 协议、DHT 节点发现和媒体播放。
Homepage: https://qvod.example.com
EOF

dpkg-deb --build debian qvs-player_0.1.0_amd64.deb
```

#### .rpm 包 (Fedora/RHEL)

```bash
# 使用 cargo-rpm
cargo install cargo-rpm
cargo rpm -p qvs-gui

# 或使用 rpmbuild
rpmbuild -ba qvs.spec
```

#### .dmg 包 (macOS)

```bash
# 使用 cargo-bundle
cargo install cargo-bundle
cargo bundle -p qvs-gui --release

# 创建 DMG
mkdir -p dmg
cp -r "target/release/bundle/osx/QVOD Player.app" dmg/
ln -s /Applications dmg/Applications
hdiutil create -volname "QVOD Player" -srcfolder dmg \
    -format UDZO "QVOD_Player-0.1.0.dmg"
```

#### MSI 安装包 (Windows)

```powershell
# 使用 cargo-wix
cargo install cargo-wix
cargo wix -p qvs-gui --release

# 或使用 NSIS
makensis installer.nsi
```

---

## 4. 安装

### 4.1 Linux 安装

#### 一键安装（推荐）

```bash
# 解压
tar xzf qvs-0.1.0-linux-amd64.tar.gz
cd qvs-0.1.0-linux-amd64

# 一键安装到 /usr/local
sudo bash scripts/install.sh

# 或安装到自定义目录
bash scripts/install.sh --prefix /opt/qvs
```

#### 手动安装

```bash
# 复制二进制
sudo cp bin/qvs-gui /usr/local/bin/
sudo cp bin/qvs-server /usr/local/bin/
sudo cp bin/qvs-cli /usr/local/bin/

# 复制桌面集成
sudo cp share/applications/qvs.desktop /usr/share/applications/
sudo cp share/applications/qvs-qvod-handler.desktop /usr/share/applications/
sudo cp share/icons/hicolor/scalable/apps/qvs.svg /usr/share/icons/hicolor/scalable/apps/

# 注册 qvod:// 协议
xdg-mime default qvs-qvod-handler.desktop x-scheme-handler/qvod

# 更新图标缓存
gtk-update-icon-cache -f -t /usr/share/icons/hicolor/
```

#### 卸载

```bash
# 使用卸载脚本
sudo bash scripts/uninstall.sh

# 或手动卸载
sudo rm -f /usr/local/bin/qvs-gui /usr/local/bin/qvs-server /usr/local/bin/qvs-cli
sudo rm -f /usr/share/applications/qvs.desktop
sudo rm -f /usr/share/applications/qvs-qvod-handler.desktop
sudo rm -f /usr/share/icons/hicolor/scalable/apps/qvs.svg
sudo rm -rf /usr/share/doc/qvs
```

### 4.2 macOS 安装

```bash
# 打开 DMG 文件
open QVOD_Player-0.1.0.dmg

# 将 QVOD Player.app 拖入 Applications 文件夹
cp -r "QVOD Player.app" /Applications/

# 注册 qvod:// 协议
/System/Library/Frameworks/CoreServices.framework/Frameworks/\
    LaunchServices.framework/Support/lsregister \
    -f /Applications/QVOD\ Player.app
```

### 4.3 Windows 安装

```powershell
# 运行 MSI 安装包
msiexec /i QVOD_Player-0.1.0-x86_64.msi

# 或运行 NSIS 安装包
.\QVOD_Player-0.1.0-x86_64.exe

# 安装程序会自动：
# 1. 复制文件到 %ProgramFiles%\QVOD Player\
# 2. 创建开始菜单和桌面快捷方式
# 3. 注册 qvod:// 协议处理器
```

---

## 5. 配置

### 5.1 配置文件路径

| 平台 | 路径 |
|------|------|
| Linux | `~/.config/qvs/config.toml` |
| macOS | `~/Library/Application Support/com.qvod.player/config.toml` |
| Windows | `%APPDATA%\QVOD Player\config.toml` |

### 5.2 完整配置参考

```toml
# ============================================================================
# QVOD Player 配置文件
# ============================================================================

[general]
language = "zh-CN"            # 界面语言: zh-CN, en-US
log_level = "info"            # 日志级别: trace, debug, info, warn, error

[playback]
default_volume = 0.8          # 默认音量 (0.0~1.0)
default_speed = 1.0           # 默认播放速度
remember_position = true      # 记忆播放位置

[network]
listen_port = 8621            # HTTP 服务端口 (0=自动分配)
udp_port = 8622               # UDP 通信端口 (0=自动分配)
max_connections = 50          # 最大 P2P 连接数
enable_dht = true             # 启用 DHT 节点发现
enable_tracker = true         # 启用 HTTP Tracker
enable_http_fallback = true   # 启用 HTTP 源回退
download_rate_limit = 0       # 下载限速 (字节/秒, 0=不限)
upload_rate_limit = 0         # 上传限速 (字节/秒, 0=不限)
enable_port_forwarding = true # 启用 UPnP 端口映射

[buffer]
capacity_mb = 64              # 缓冲大小 (MB)
watermark_low = 0.1           # 低位水位 (触发缓冲)
watermark_high = 0.8          # 高位水位 (停止激进下载)
min_playable_secs = 1         # 最小可播放时长 (秒)
adaptive = true               # 自适应缓冲模式

[cache]
max_size_gb = 4               # 缓存上限 (GB)
auto_cleanup = true           # 自动清理缓存
directory = ""                # 缓存目录 (空=默认路径)

[tracker]
urls = [
    "http://tracker.example.com:6969/announce",
    "udp://tracker.opentrackr.org:1337/announce",
]
announce_interval_secs = 1800 # 通告间隔 (秒)
num_want = 50                 # 请求的 peer 数量

[dht]
seed_nodes = [
    "router.bittorrent.com:6881",
    "dht.transmissionbt.com:6881",
]
k = 8                         # Kademlia K (bucket 大小)
alpha = 3                     # Kademlia α (并发查询数)

[ui]
theme = "dark"                # 主题: dark, light, system
fullscreen = false            # 默认全屏
show_network_panel = true     # 显示网络状态面板
auto_hide_controls = true     # 自动隐藏控制栏
font_scale = 1.0              # 字体缩放 (0.8~1.5)
```

### 5.3 环境变量覆盖

配置项可通过环境变量覆盖：

| 变量 | 覆盖配置 | 示例 |
|------|----------|------|
| `QVS_LISTEN_PORT` | `network.listen_port` | `8621` |
| `QVS_MAX_CONNECTIONS` | `network.max_connections` | `100` |
| `QVS_CACHE_DIR` | `cache.directory` | `/mnt/cache/qvs` |
| `QVS_LOG_LEVEL` | `general.log_level` | `debug` |
| `QVS_TRACKER_URLS` | `tracker.urls` | `url1,url2` |
| `QVS_DHT_SEED_NODES` | `dht.seed_nodes` | `node1:6881,node2:6881` |

---

## 6. 部署场景

### 6.1 桌面播放器（默认场景）

```bash
# 启动 GUI 播放器
qvs-gui

# 从命令行播放
qvs-gui "qvod://A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9|movie.mp4|734003200|mp4|"

# 后台播放（隐藏主窗口）
qvs-gui --headless "qvod://<hash>|<name>|<size>|<format>|"
```

### 6.2 Headless 流媒体服务器

适合部署在服务器上，为局域网设备提供流媒体服务。

```bash
# 启动服务器
qvs-server --port 8621

# 使用 systemd 管理
sudo cp systemd/qvs-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable qvs-server
sudo systemctl start qvs-server

# 查看状态
sudo systemctl status qvs-server

# 查看日志
sudo journalctl -u qvs-server -f
```

**API 接口：**

| 方法 | 路径 | 参数 | 说明 |
|------|------|------|------|
| GET | `/play` | `hash=xxx` | 流式播放（Chunked 响应） |
| GET | `/play` | `hash=xxx&offset=N` | Range 请求（Seek） |
| GET | `/segment` | `hash=xxx&index=N` | HLS 切片 |
| GET | `/status` | `hash=xxx` | 流状态 JSON |
| POST | `/control` | `action=pause` | 暂停 |
| POST | `/control` | `action=resume` | 恢复 |
| POST | `/control` | `action=stop&hash=xxx` | 停止 |
| POST | `/control` | `action=seek&value=5000` | 跳转到 5000ms |

**播放器端使用：**

```bash
# VLC / PotPlayer 等播放器打开
http://server-ip:8621/play?hash=A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9

# 或使用 ffplay
ffplay http://server-ip:8621/play?hash=...

# 支持 Range 请求，拖拽定位
```

### 6.3 CLI 客户端

```bash
# 播放
qvs-cli play "qvod://<hash>|<name>|<size>|<format>|"

# 查看所有活跃流
qvs-cli status

# 列出流信息
qvs-cli list

# 缓存管理
qvs-cli cache --info          # 查看缓存状态
qvs-cli cache --clean         # 清理缓存
qvs-cli cache --size 2048     # 设置缓存上限 2GB

# 诊断
qvs-cli diag > report.txt     # 生成诊断报告
```

### 6.4 Docker 部署

```dockerfile
# Dockerfile
FROM rust:1.75-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev libclang-dev \
    libavcodec-dev libavformat-dev libavutil-dev \
    libswscale-dev libswresample-dev

WORKDIR /app
COPY . .
RUN cargo build --release -p qvs-server -p qvs-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    libavcodec59 libavformat59 libavutil57 \
    libswscale6 libswresample4 libssl3 \
    ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/qvs-server /usr/local/bin/
COPY --from=builder /app/target/release/qvs-cli /usr/local/bin/

EXPOSE 8621 8622/udp
ENTRYPOINT ["qvs-server"]
CMD ["--port", "8621"]
```

```bash
# 构建 Docker 镜像
docker build -t qvs-server:0.1.0 .

# 运行容器
docker run -d \
    --name qvs-server \
    -p 8621:8621 \
    -p 8622:8622/udp \
    -v qvs-cache:/root/.local/share/qvs \
    -v qvs-config:/root/.config/qvs \
    qvs-server:0.1.0 --port 8621
```

---

## 7. 协议处理器注册

### 7.1 Linux

```bash
# 注册 qvod:// 协议
xdg-mime default qvs-qvod-handler.desktop x-scheme-handler/qvod

# 验证
xdg-open "qvod://A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9|test.mp4|1024000|mp4|"
```

### 7.2 macOS

自动在 `.app` 包中注册，手动刷新：

```bash
/System/Library/Frameworks/CoreServices.framework/Frameworks/\
    LaunchServices.framework/Support/lsregister \
    -f /Applications/QVOD\ Player.app
```

### 7.3 Windows

```powershell
# 管理员 PowerShell
$exePath = "$env:ProgramFiles\QVOD Player\qvs-gui.exe"

New-Item -Path "HKCR:\qvod" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\qvod" -Name "(Default)" -Value "URL:QVOD Protocol"
Set-ItemProperty -Path "HKCR:\qvod" -Name "URL Protocol" -Value ""

New-Item -Path "HKCR:\qvod\shell\open\command" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\qvod\shell\open\command" \
    -Name "(Default)" -Value "`"$exePath`" `"%1`""

# 验证
Start-Process "qvod://A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9|test.mp4|1024000|mp4|"
```

---

## 8. 性能调优

### 8.1 常见场景调优

| 场景 | 缓存大小 | 连接数 | 缓冲策略 | HTTP 回退 |
|------|---------|--------|---------|----------|
| 桌面播放器 | 64 MB | 50 | 自适应 | 开启 |
| 服务器 (100 Mbps) | 256 MB | 200 | 高水位 | 开启 |
| 服务器 (1 Gbps) | 512 MB | 500 | 高水位 | 开启 |
| 低配设备 (2 GB RAM) | 32 MB | 20 | 低水位 | 开启 |
| 移动端代理 | 16 MB | 10 | 低水位 | 关闭 |

### 8.2 关键参数

```toml
# 高带宽服务器配置
[network]
max_connections = 200
enable_http_fallback = true

[buffer]
capacity_mb = 256
watermark_low = 0.05
watermark_high = 0.6
adaptive = true
adaptive_max_mb = 512

[cache]
max_size_gb = 20

[advanced]
worker_threads = 4
udp_recv_buffer_size = 2097152
tcp_recv_buffer_size = 524288
```

### 8.3 系统调优

#### Linux 内核参数

```bash
# /etc/sysctl.d/99-qvs.conf

# 增加网络缓冲区
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216

# 增加 UDP 缓冲区
net.ipv4.udp_rmem_min = 131072
net.ipv4.udp_wmem_min = 131072

# 启用端口复用
net.ipv4.tcp_tw_reuse = 1

# 增加连接跟踪
net.netfilter.nf_conntrack_max = 524288

# 生效
sudo sysctl -p /etc/sysctl.d/99-qvs.conf
```

#### 文件描述符限制

```bash
# /etc/security/limits.d/99-qvs.conf
qvs   soft   nofile   65536
qvs   hard   nofile   131072
```

---

## 9. 故障排除

### 常见问题

**Q: 编译报错 "ffmpeg-next not found"**

A: 系统缺少 FFmpeg 开发库。参见 [1.2 依赖安装](#12-依赖安装)。或跳过 `qvs-media` 构建：

```bash
cargo build --release -p qvs-gui -p qvs-server -p qvs-cli
```

**Q: `qvod://` 链接无法打开播放器**

A: 协议处理器未注册。参见 [第 7 章 协议处理器注册](#7-协议处理器注册)。

**Q: "Failed to bind port" 启动失败**

A: 端口被占用。修改配置或使用端口偏移：

```bash
qvs-server --port 18621
```

**Q: 播放卡顿/缓冲慢**

A: 网络带宽不足或配置不合理：
- 增大 `buffer.capacity_mb`
- 确保 `enable_http_fallback = true`
- 检查 `max_connections` 是否过小
- 等待 30-60 秒让 DHT 完成引导

**Q: GUI 无法启动 (Linux)**

A: 缺少依赖或显示环境：
```bash
# 检查 OpenGL
glxinfo | grep "OpenGL version"

# 确保显示服务运行
echo $DISPLAY         # X11
echo $WAYLAND_DISPLAY # Wayland

# 安装依赖
sudo apt install libgtk-3-dev libxdo-dev
```

**Q: DHT 找不到节点**

A: 首次启动需要 30-60 秒完成 bootstrap：
```bash
# 检查 DHT 状态
qvs-cli status

# 检查种子节点可达性
nc -zu router.bittorrent.com 6881
```

### 调试命令

```bash
# 详细日志
QVS_LOG=debug qvs-gui

# 跟踪日志 (极详细)
QVS_LOG=trace qvs-gui

# 指定模块详细日志
QVS_LOG=info,qvs_transport=debug,qvs_dht=trace qvs-server

# 诊断报告
qvs-cli diag > qvs-diag.txt

# 检查二进制链接库
ldd $(which qvs-server)
```

---

## 附录 A: 构建验证清单

```bash
# 完整验证流程
cargo build --release --workspace && \
cargo test --workspace && \
cargo clippy --workspace -- -D warnings && \
cargo fmt --check
```

## 附录 B: 目录结构

```
~/.config/qvs/              # 配置目录
├── config.toml             # 主配置文件

~/.local/share/qvs/         # 数据目录 (Linux)
├── cache/
│   ├── qdata/              # 缓存数据文件 (稀疏文件)
│   │   └── {hash}.qdata
│   ├── qmv/                # 元数据文件
│   │   └── {hash}.qmv
│   └── hls/                # 临时 HLS 切片
│       └── {hash}/
│           ├── index.m3u8
│           └── segment_*.ts
├── logs/
│   └── qvs.log             # 日志文件
└── .first_run              # 首次运行标记

~/Library/Application Support/com.qvod.player/   # macOS 配置
~/Library/Caches/com.qvod.player/                # macOS 缓存
~/Library/Logs/com.qvod.player/                  # macOS 日志

%APPDATA%\QVOD Player\                           # Windows 配置
%LOCALAPPDATA%\QVOD Player\cache\                # Windows 缓存
%LOCALAPPDATA%\QVOD Player\logs\                 # Windows 日志
```

## 附录 C: 版本号约定

采用语义化版本 [SemVer 2.0](https://semver.org/)：

- **主版本号**: 不兼容的 API 变更
- **次版本号**: 向下兼容的功能新增
- **修订号**: 向下兼容的问题修复

当前版本: `0.1.0`（开发阶段）

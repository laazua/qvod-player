# QVOD 部署文档

## 系统架构

```
用户输入 qvod:// URI
  │
  ▼
QvodEngine::play(uri)          ← 核心引擎，集成所有子系统
  ├─ URI 解析 → InfoHash
  ├─ 本地缓存查询 (CacheManager)
  ├─ 并行 Peer 发现
  │   ├─ HTTP Tracker (指数退避重试 + 随机负载均衡)
  │   └─ Kademlia DHT (迭代递归查询, α=3)
  ├─ TCP 连接最优 Peers
  ├─ ut_metadata 扩展获取 FileMeta
  ├─ RingBuffer + PieceScheduler 初始化
  ├─ P2SP 后台下载循环 (Critical/High/Normal/Idle)
  │   ├─ Critical → P2P + HTTP 并行
  │   ├─ High → P2P 优先, 3s 超时后 HTTP 回退
  │   ├─ Normal → 仅 P2P, rarest-first
  │   └─ Idle → 后台预取
  ├─ 关键帧优先, 稀有度加权, 自适应水位
  └─ 数据流入 RingBuffer → HTTP ChunkedStream / GUI 渲染
```

---

## 1. 系统要求

### 构建时

| 组件 | 版本 | 用途 |
|------|------|------|
| Rust | 1.70+ (2021 edition) | 编译整个项目 |
| FFmpeg dev | 7.0+ | qvs-media 解码 (libavcodec/libavformat/libavutil) |
| pkg-config | 任意 | 查找 FFmpeg 库路径 |

### 运行时 (最小)

| 组件 | 版本 | 用途 |
|------|------|------|
| FFmpeg shared libs | 7.0+ | 仅播放器/解码需要 |
| OpenGL 3.3+ | — | egui GUI 渲染 |
| 网络 | — | P2P 节点发现和传输 |

---

## 2. 构建

### 2.1 完整构建（推荐）

```bash
# 构建所有 crate
cargo build --release --workspace

# 运行所有测试
cargo test --workspace

# Lint 检查
cargo clippy --workspace -- -D warnings

# 格式化检查
cargo fmt --check
```

### 2.2 按需构建

```bash
# 仅 headless 服务器
cargo build --release -p qvs-server

# 仅 CLI 客户端
cargo build --release -p qvs-cli

# 仅 GUI 播放器
cargo build --release -p qvs-gui
```

### 2.3 无 FFmpeg 构建

如果系统未安装 FFmpeg 开发库，qvs-media 将编译为 stub（返回明确的错误信息），不影响其他模块：

```bash
cargo build --release -p qvs-server -p qvs-cli -p qvs-gui
```

---

## 3. 运行

### 3.1 Headless 服务器

```bash
./target/release/qvs-server --port 8621
```

服务器启动后会：
1. 绑定 HTTP 端口提供服务
2. 启动 DHT 节点（UDP）进行 peer 发现
3. 初始化 Tracker 客户端

**API 端点：**

| 方法 | 路径 | 参数 | 说明 |
|------|------|------|------|
| GET | `/play` | `hash=xxx` | 流式播放（chunked 响应） |
| GET | `/play` | `hash=xxx&offset=N` | Range 请求（seek） |
| GET | `/segment` | `hash=xxx&index=N` | HLS 伪分片 |
| POST | `/control` | `action=pause` | 暂停播放 |
| POST | `/control` | `action=resume` | 恢复播放 |
| POST | `/control` | `action=stop&hash=xxx` | 停止播放 |
| POST | `/control` | `action=seek&value=5000` | 跳转到 5000ms |
| POST | `/control` | `action=status` | 查看所有流状态 |
| GET | `/status` | `hash=xxx` | JSON 状态 |

### 3.2 CLI 客户端

```bash
# 播放 URI
./target/release/qvs-cli play "qvod://<40hex>|<filename>|<size>|<format>|"

# 查看状态
./target/release/qvs-cli status

# 列出活跃流
./target/release/qvs-cli list

# 清理缓存
./target/release/qvs-cli cache --clean

# 设置缓存大小限制
./target/release/qvs-cli cache --size 2048
```

### 3.3 GUI 播放器

```bash
./target/release/qvs
```

启动后界面包含：
- **Player** — 视频显示区域（无 ffmpeg 时显示占位符）
- **Playlist** — 播放列表管理
- **Settings** — 全部可配置项
- **Status** — 实时网络状态面板

**键盘快捷键：**

| 按键 | 功能 |
|------|------|
| `Space` | 播放/暂停 |
| `←` | 后退 10 秒 |
| `→` | 快进 10 秒 |
| `Esc` | 退出全屏 |

---

## 4. Windows 部署

### 4.1 安装 Rust

```powershell
# 1. 下载并运行 rustup
# https://rustup.rs/
# 或者用 winget:
winget install Rustlang.Rustup

# 2. 验证安装
rustc --version
cargo --version

# 3. 确保 windows 目标已安装
rustup target list --installed
# 应包含 x86_64-pc-windows-msvc
```

### 4.2 安装 FFmpeg

**选项 A：使用 vcpkg（推荐）**

```powershell
# 1. 安装 vcpkg
git clone https://github.com/Microsoft/vcpkg.git
cd vcpkg
.\bootstrap-vcpkg.bat

# 2. 安装 FFmpeg
.\vcpkg install ffmpeg:x64-windows

# 3. 设置环境变量
$env:VCPKG_ROOT = "C:\path\to\vcpkg"
$env:PATH = "$env:VCPKG_ROOT\installed\x64-windows\bin;$env:PATH"
```

**选项 B：下载预编译 DLL**

1. 从 https://github.com/BtbN/FFmpeg-Builds/releases 下载 `ffmpeg-master-latest-win64-gpl.zip`
2. 解压到 `C:\ffmpeg`
3. 将 `C:\ffmpeg\bin` 添加到系统 PATH

**选项 C：跳过 FFmpeg**

GUI 仍可启动，仅视频解码功能不可用（显示 "FFmpeg not available" 提示）。

### 4.3 构建

```powershell
# PowerShell
cd qvs

# 完整构建（需要 FFmpeg dev）
$env:PKG_CONFIG_PATH = "C:\path\to\vcpkg\installed\x64-windows\lib\pkgconfig"
cargo build --release --workspace

# 或仅构建可执行部分（无需 FFmpeg）
cargo build --release -p qvs-gui -p qvs-server -p qvs-cli
```

### 4.4 运行 GUI 播放器

```powershell
# 直接双击
.\target\release\qvs.exe

# 或命令行
.\target\release\qvs.exe play "qvod://<hash>|<name>|<size>|<format>|"
```

### 4.5 运行服务器（Windows 服务模式）

```powershell
# 前台运行
.\target\release\qvs-server.exe --port 8621

# 后台运行（新建窗口）
Start-Process -WindowStyle Hidden -FilePath ".\target\release\qvs-server.exe" -ArgumentList "--port 8621"
```

---

## 5. macOS 部署

```bash
# 1. 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. 安装 FFmpeg
brew install ffmpeg pkg-config

# 3. 构建并运行
cargo build --release --workspace
./target/release/qvs
```

---

## 6. Linux 部署

### 6.1 Ubuntu/Debian

```bash
# 1. 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. 安装 FFmpeg
sudo apt install libavcodec-dev libavformat-dev libavutil-dev \
                 libswresample-dev libswscale-dev pkg-config

# 3. 构建并运行
cargo build --release --workspace
./target/release/qvs-server --port 8621
```

### 6.2 Rocky Linux / RHEL

```bash
# 1. 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. 安装 FFmpeg
sudo dnf install ffmpeg-devel pkgconf-pkg-config

# 3. 构建
cargo build --release --workspace
```

---

## 7. 配置

配置文件为 TOML 格式，默认路径 `~/.config/qvs/config.toml`：

```toml
listen_port = 8621
udp_port = 8621
max_connections = 50
buffer_capacity_mb = 64
cache_dir = "/tmp/qvs-cache"
http_fallback = true
dht_enabled = true
tracker_enabled = true
cache_enabled = true
max_peers_per_stream = 50
download_timeout_secs = 30

tracker_urls = [
    "http://tracker.example.com:6969/announce",
]

dht_seed_nodes = [
    "router.bittorrent.com:6881",
    "dht.transmissionbt.com:6881",
]
```

---

## 8. 生成产物清单

| 二进制 | 路径 | 说明 |
|--------|------|------|
| `qvs` | `target/release/qvs.exe` (Windows) / `target/release/qvs` | GUI 播放器 |
| `qvs-server` | `target/release/qvs-server.exe` / `target/release/qvs-server` | Headless 服务器 |
| `qvs-cli` | `target/release/qvs-cli.exe` / `target/release/qvs-cli` | CLI 客户端 |

---

## 9. 常见问题

### Q: 编译报错 "ffmpeg-next not found"
A: 系统缺少 FFmpeg 开发库。安装对应包（见第 6 节），或跳过 `qvs-media` 构建：
```bash
cargo build --release -p qvs-gui -p qvs-server -p qvs-cli
```

### Q: GUI 窗口无法打开 / OpenGL 错误
A: 确保系统支持 OpenGL 3.3+。Windows 用户需安装显卡驱动。Linux 用户：
```bash
# 检查 OpenGL 版本
glxinfo | grep "OpenGL version"
```

### Q: 服务器启动报 "port unavailable"
A: 端口被占用，使用其他端口：
```bash
./target/release/qvs-server --port 18621
```

### Q: 播放时一直显示 "No Video"
A: 当前环境可能没有 FFmpeg 运行时库。确认 FFmpeg 已安装：
```bash
ffmpeg -version
# Windows: where ffmpeg
```

### Q: DHT 无法找到节点
A: 确认种子节点可达。默认使用公共 DHT 种子节点，首次启动可能需要 30-60 秒完成 bootstrap：
```json
POST /control { "action": "status" }
# 检查 DHT table size
```

---

## 10. 性能调优

| 参数 | 默认值 | 推荐范围 | 说明 |
|------|--------|----------|------|
| `buffer_capacity_mb` | 64 | 32-256 | 内存缓冲大小，越大越流畅但吃内存 |
| `max_connections` | 50 | 20-200 | 最大并发 P2P 连接数 |
| `http_fallback` | true | — | 当 P2P 速度不足时启用 HTTP 源回退 |
| `download_timeout_secs` | 30 | 10-60 | 单块下载超时时间 |
| `max_peers_per_stream` | 50 | 20-100 | 每个流最大维持的 peer 数 |

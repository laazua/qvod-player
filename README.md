# QVOD (快播) — Rust 跨平台 P2SP 流媒体点播系统

快播 (QvodPlayer) 的完整 Rust 跨平台复刻，基于 **P2SP (Peer to Server & Peer)** + **UDP 混合加速** 架构。

## 技术特点

- **P2SP 混合加速** — 同时从 P2P 节点和 HTTP 源获取数据
- **TCP+UDP 双通道** — TCP 保证关键帧可靠性，UDP 提升非关键帧传输效率
- **关键帧优先调度** — I 帧优先下载，实现秒开播放
- **任意拖拽定位** — 直接跳转目标位置，无需顺序下载
- **伪 HLS 动态适配** — 自动将视频流转换为 HLS 分片，支持移动端
- **NAT 穿透** — UDP 打洞 + 中继后备，内网节点互联率 90%+
- **本地 Web 网关** — 内嵌 HTTP Server 桥接浏览器与 P2P 引擎

## 项目结构

```
qvs/
├── crates/
│   ├── qvs-core/           # 基础类型、trait、错误类型
│   ├── qvs-dht/            # Kademlia DHT 网络（路由分裂、迭代查询）
│   ├── qvs-tracker/        # HTTP Tracker 客户端（重试+负载均衡）
│   ├── qvs-transport/      # P2SP TCP+UDP 传输层（11种消息、拥塞控制）
│   ├── qvs-stream/         # 流媒体引擎（集成所有子系统）
│   ├── qvs-local-server/   # 本地 HTTP 网关（流式/分片/控制）
│   ├── qvs-media/          # 音视频解码 (FFmpeg stub)
│   ├── qvs-gui/            # GUI 播放器 (egui 5面板+主题)
│   └── qvs-format/         # URI、Bencode、缓存管理器
├── agents/                 # AI 辅助开发文档
├── docs/                   # 技术参考文档
└── tests/                  # 集成测试
```

## 快速开始

```bash
# 构建
cargo build --release

# 启动服务器
./target/release/qvs-server --port 8621

# 或启动 GUI 播放器
./target/release/qvs
```

更多部署方式详见 [DEPLOY.md](DEPLOY.md)。

## 构建状态

所有 11 个 crate 均已完成，~300 个测试通过，cargo clippy + cargo fmt 干净。

| 二进制 | 路径 | 说明 |
|--------|------|------|
| `qvs` | `target/release/qvs` | GUI 播放器 |
| `qvs-server` | `target/release/qvs-server` | Headless 服务器 |
| `qvs-cli` | `target/release/qvs-cli` | CLI 客户端 |

## 系统要求

- Rust 1.70+ (2021 edition)
- FFmpeg 库（可选，仅播放器解码需要）
- Linux / macOS / Windows

## 许可

本项目仅供学习研究。

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
│   ├── qvs-dht/            # Kademlia DHT 网络
│   ├── qvs-tracker/        # HTTP Tracker 客户端
│   ├── qvs-transport/      # P2SP TCP+UDP 传输层
│   ├── qvs-stream/         # 流媒体引擎
│   ├── qvs-local-server/   # 本地 HTTP 网关
│   ├── qvs-media/          # 音视频解码 (FFmpeg)
│   ├── qvs-gui/            # GUI 播放器 (egui)
│   └── qvs-format/         # URI、Bencode、文件格式
├── agents/                 # AI 辅助开发文档
├── docs/                   # 技术参考文档
└── tests/                  # 集成测试
```

## 构建

```bash
cargo build --release
cargo test --workspace
```

## 系统要求

- Rust 2024 edition
- FFmpeg 库 (libavcodec, libavformat, libavutil)
- Linux / macOS / Windows

## 许可

本项目仅供学习研究。

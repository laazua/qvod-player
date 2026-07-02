# QVOD (快播) 系统 — AI 协作入口

## 项目概述

本项目的目标是使用 **Rust** 跨平台复刻快播 (QvodPlayer) 的完整 P2SP 流媒体点播系统。

快播采用 **P2SP (Peer to Server & Peer)** + **UDP 混合加速** 架构，核心特点：
- 中心 Tracker 索引 + DHT 辅助节点发现
- P2P 节点互传 + HTTP 源服务器后备
- 本地 Web Server 桥接浏览器与 P2P 引擎
- 关键帧优先、边下边播、任意拖拽定位
- 伪 HLS 动态适配支持移动端
- 混合 TCP/UDP 传输：TCP 保证关键帧可靠性，UDP 提升非关键帧传输效率
- 自定义拥塞控制算法适配流媒体场景

## 项目结构

```
qvs/
├── Cargo.toml                        # workspace 根
├── AGENTS.md                         # ← 本文件 (AI 协作入口)
├── CLAUDE.md                         # 项目规范 & 编码约定
│
├── agents/                           # AI 辅助文档
│   ├── architecture.md               # 5 层架构详细设计
│   ├── coding.md                     # Rust 编码规范
│   └── security.md                   # 安全规范
│
├── crates/
│   ├── qvs-core/                     # 基础类型、trait 定义、错误类型
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                # 模块导出
│   │       ├── types.rs              # InfoHash, NodeId, PeerInfo, Bitfield, SocketAddr
│   │       ├── error.rs              # QvodError 统一错误类型 (thiserror)
│   │       ├── traits.rs             # DhtEngine, Transport, CacheBackend trait
│   │       ├── constants.rs          # PIECE_LENGTH, BLOCK_LENGTH, 协议常量
│   │       └── util.rs               # 辅助函数 (时间戳, 随机数, hex 编码)
│   │
│   ├── qvs-format/                   # .qvs/URI/缓存 跨层工具
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── uri.rs                # qvod:// URI 解析与构造
│   │       ├── qvs_file.rs           # .qvs 种子文件读写 (Bencode)
│   │       ├── cache.rs              # 缓存管理器 (qdata/qmv 读写, LRU 清理)
│   │       ├── bencode.rs            # Bencode 编解码器
│   │       ├── bitfield.rs           # Bitfield 数据结构 (piece 完成状态)
│   │       └── keyframe.rs           # 关键帧索引结构
│   │
│   ├── qvs-dht/                      # Kademlia DHT 网络 (Layer 2)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── node.rs               # DHT 节点 (维护 NodeId, 处理 RPC)
│   │       ├── routing.rs            # 路由表 (Kademlia k-bucket)
│   │       ├── rpc.rs                # UDP RPC 消息编解码 (消息头 + 负载)
│   │       ├── bootstrap.rs          # 启动引导 (种子节点 → 路由表填充)
│   │       ├── krpc.rs               # Kademlia RPC 逻辑 (FIND_NODE, FIND_PEERS, ANNOUNCE)
│   │       ├── token.rs              # announce token 管理
│   │       └── stats.rs              # DHT 网络统计信息
│   │
│   ├── qvs-tracker/                  # HTTP Tracker 客户端 (Layer 2)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── client.rs             # HTTP Tracker announce/scrape 请求
│   │       ├── protocol.rs           # Tracker 协议参数编码 & Bencode 响应解析
│   │       ├── scraper.rs            # Scrape 接口 (查询 swarm 状态)
│   │       └── peer_list.rs          # Peer 列表解析 (compact 和非 compact)
│   │
│   ├── qvs-local-server/             # 本地 Web 服务 (Layer 1)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── server.rs             # HTTP 服务器启动/停止 (axum)
│   │       ├── handler.rs            # 路由分发: /play, /status, /segment
│   │       ├── stream.rs             # Chunked 响应流 (tokio::sync::mpsc)
│   │       ├── config.rs             # 端口配置 & 自动选择 (端口冲突回退)
│   │       ├── range.rs              # HTTP Range 请求处理 (seek 支持)
│   │       └── middleware.rs         # CORS, 日志, 限速中间件
│   │
│   ├── qvs-transport/                # P2SP 传输层: TCP+UDP (Layer 3)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── handshake.rs          # TCP 握手协议 (68 字节, 扩展位协商)
│   │       ├── message.rs            # 消息编解码 (length_prefix + msg_id + payload)
│   │       ├── tcp_stream.rs         # TCP 传输 (关键帧, 可靠传输)
│   │       ├── udp_stream.rs         # UDP 传输 (非关键帧, 数据/ACK/NACK)
│   │       ├── congestion.rs         # UDP 拥塞控制 (类 TCP Reno + 流媒体优化)
│   │       ├── pool.rs               # 连接池管理 (上限 50, 超时清理, 保活)
│   │       ├── scheduler.rs          # Piece 优先级调度器 (Critical/High/Normal/Low)
│   │       ├── p2sp.rs               # P2SP 混合下载决策 (P2P + HTTP 源选择)
│   │       ├── peer_wire.rs          # Peer Wire 协议实现 (choke/unchoke/request/piece)
│   │       ├── nat.rs                # NAT 穿透 (UDP 打洞 + TURN 中继后备)
│   │       └── stats.rs              # 连接统计 (速度, RTT, 丢包率)
│   │
│   ├── qvs-stream/                   # 流媒体引擎: 缓冲/调度/HLS (Layer 4)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── engine.rs             # QvodEngine 主引擎 (play/pause/stop/seek)
│   │       ├── buffer.rs             # RingBuffer 环形缓冲区 (64MB 默认, 水位自适应)
│   │       ├── metadata.rs           # FileMeta 解析 & 扩展协议 ut_metadata
│   │       ├── seek.rs               # SeekEngine 随机定位 (关键帧跳转)
│   │       ├── hls.rs                # 伪 HLS 适配器 (M3U8 生成, TS 包装)
│   │       ├── adaptive.rs           # AdaptiveBuffer 自适应缓冲策略
│   │       ├── playback.rs           # MediaStream 管理 (速率监控, EOS 检测)
│   │       └── config.rs             # EngineConfig 配置
│   │
│   ├── qvs-media/                    # 媒体解码/渲染 (Layer 5)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── demuxer.rs            # 解复用器 (ffmpeg-next, 支持 rmvb/avi/mkv/mp4)
│   │       ├── decoder.rs            # 视频/音频解码 (H.264, RV40, AAC, COOK)
│   │       ├── renderer.rs           # 渲染输出 (egui 集成, OpenGL 纹理)
│   │       ├── format.rs             # 格式探测 (魔数 + 扩展名)
│   │       ├── resampler.rs          # 音频重采样 (ffmpeg swr)
│   │       └── sync.rs               # 音视频同步 (基于 PTS/DTS)
│   │
│   └── qvs-gui/                      # GUI 播放器 (Layer 5)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs               # 入口 & CLI 参数解析 (clap)
│           ├── app.rs                # 应用主窗口 (egui::App)
│           ├── player.rs             # 播放器面板 (视频渲染, 控制叠加)
│           ├── controls.rs           # 播放控制 (播放/暂停/进度条/音量)
│           ├── playlist.rs           # 播放列表 & 历史记录
│           ├── settings.rs           # 设置页面 (缓存路径, 端口, 最大连接数)
│           ├── status.rs             # 网络状态面板 (peer 数, 速度, 缓冲进度)
│           ├── overlay.rs            # OSD 叠加层 (缓冲提示, 错误提示)
│           └── theme.rs              # 主题 & UI 样式常量
│
├── tests/                            # 全局集成测试
│   ├── integration/                  # 跨 crate 集成测试
│   │   ├── p2p_transfer.rs           # 双节点 P2P 传输测试
│   │   ├── full_stream.rs            # 完整播放流测试 (mock Tracker + DHT)
│   │   └── nat_traversal.rs          # NAT 穿透集成测试
│   └── fixtures/                     # 测试数据
│       ├── sample.qvs                # 测试用 .qvs 种子文件
│       ├── test_video.mp4            # 短视频测试文件 (< 1MB)
│       └── dht_bootstrap.txt         # DHT 种子节点列表
│
├── docs/
│   ├── superpowers/
│   │   └── specs/
│   │       └── 2025-07-01-qvod-system-design.md  # 完整技术规格书
│   └── architecture/                 # 架构图 (Mermaid/PlantUML)
│       ├── layers.puml               # 层架构图
│       ├── data-flow.puml            # 数据流图
│       └── module-deps.puml          # 模块依赖图
│
├── scripts/                          # 开发脚本
│   ├── setup.sh                      # 开发环境初始化
│   ├── test-all.sh                   # 运行所有测试
│   ├── lint-all.sh                   # 运行所有 lint
│   └── coverage.sh                   # 生成测试覆盖率报告
│
├── .github/
│   └── workflows/
│       ├── ci.yml                    # CI: build + test + clippy
│       └── audit.yml                 # 安全审计 (cargo audit)
│
├── rust-toolchain.toml               # Rust 工具链配置
├── .gitignore
├── .env.example                      # 环境变量模板
└── README.md                         # 项目简介
```

## AI 开发流程

### 1. 先读规格书

所有技术细节、协议格式、接口定义、数据结构均在以下文档中：

核心文档（依次阅读）：
1. `agents/architecture.md` — 系统 5 层架构总览与数据流
2. `agents/coding.md` — Rust 编码规范与最佳实践
3. `agents/security.md` — 安全规范与威胁模型
4. `agents/protocol.md` — 字节级 wire protocol 定义
5. `agents/tracker.md` — Tracker 协议与实现
6. `agents/peer.md` — Peer 连接管理
7. `agents/metadata.md` — 元数据格式与交换
8. `agents/scheduler.md` — Piece 调度算法
9. `agents/download.md` — P2SP 下载引擎
10. `agents/cache.md` — 缓存系统
11. `agents/storage.md` — 存储与文件格式
12. `agents/gateway.md` — 本地 HTTP 网关
13. `agents/player.md` — 播放器模块
14. `agents/monitoring.md` — 监控与统计
15. `agents/deployment.md` — 构建与部署

技术参考（用于实现时查阅）：
- `docs/protocol/` — 协议格式详解 & 十六进制样例
- `docs/tracker/` — Tracker API 与错误码
- `docs/scheduler/` — 调度算法公式与伪代码
- `docs/storage/` — .qvs/.qdata/.qmv 格式
- `docs/cache/` — 缓存策略与一致性
- `docs/streaming/` — 流媒体引擎详解
- `docs/api/` — HTTP API 与 Rust trait 定义
- `docs/database/` — SQLite 表结构

### 2. 实现顺序

建议按以下顺序逐 crate 实现（自底向上）：

| 顺序 | Crate | 依赖 | 预估工作量 | 核心产出 |
|------|-------|------|-----------|---------|
| 1 | `qvs-core` | 无 | 中等 | InfoHash, PeerInfo, Bitfield, QvodError, DhtEngine trait, Transport trait, 协议常量 |
| 2 | `qvs-format` | qvs-core | 中等 | qvod:// URI 解析/构造, .qvs 种子文件 Bencode 读写, 缓存管理器, 关键帧索引 |
| 3 | `qvs-dht` | qvs-core | 大 | Kademlia 路由表, UDP RPC, FIND_NODE/FIND_PEERS/ANNOUNCE, bootstrap, token 管理 |
| 4 | `qvs-tracker` | qvs-core | 小 | HTTP Tracker announce/scrape, Bencode 响应解析, compact peer 列表 |
| 5 | `qvs-local-server` | qvs-stream | 中等 | axum HTTP 服务, /play /status /segment 路由, Range 请求, Chunked 流式响应 |
| 6 | `qvs-transport` | qvs-core, qvs-format | 大 | TCP 握手/消息协议, UDP 数据通道, 拥塞控制, 连接池, Piece 调度, P2SP 决策, NAT 穿透 |
| 7 | `qvs-stream` | qvs-core, qvs-transport, qvs-format | 大 | QvodEngine 主循环, RingBuffer, SeekEngine, HLS 适配器, 自适应缓冲, Metadata 获取 |
| 8 | `qvs-media` | qvs-core | 中等 | ffmpeg-next 解复用/解码, 格式探测, 音频重采样, 音视频同步 |
| 9 | `qvs-gui` | qvs-stream, qvs-media | 中等 | egui 播放器窗口, 控制组件, 播放列表, 设置页面, 网络状态面板 |

### 3. 每步验收标准

每个 crate 实现后必须通过以下检查：

```bash
# 1. 编译检查
cargo build --package qvs-{crate_name}

# 2. 单元测试
cargo test --package qvs-{crate_name}

# 3. Clippy lint
cargo clippy --package qvs-{crate_name} -- -D warnings

# 4. 文档测试
cargo test --doc --package qvs-{crate_name}

# 5. 格式化检查
cargo fmt --check --package qvs-{crate_name}
```

### 4. 详细实现步骤

#### 步骤 1: qvs-core (基础类型与 Trait)

```
实现内容:
  1. src/types.rs:
     - InfoHash (newtype [u8; 20], Display hex, FromStr, Serialize/Deserialize)
     - NodeId (newtype [u8; 20], XOR distance 计算, Display hex)
     - PeerInfo (peer_id, addr: SocketAddr, is_firewalled, bw_up, bw_down, location)
     - Bitfield (bit 数组, 支持 get/set/set_all/count/iter, serialize/deserialize)
     - PiecePriority enum (Critical/High/Normal/Low, Ord/PartialOrd)
     - PieceInfo (index, hash, priority, length)
     - BlockRequest (piece_index, begin, length)
     - ConnectionStats (speed_down, speed_up, rtt, loss_rate, total_downloaded)
     - AnnounceEvent enum (Started/Completed/Stopped/Empty)
     - SwarmStatus (complete, incomplete, downloaded)
     - KBucketEntry (node_id, addr, last_seen, latency, is_firewalled)
     - FrameType enum (I/P/B)
     - KeyFrameEntry (timestamp_ms, file_offset, frame_size, frame_type)
     - KeyFrameIndex (entries: Vec<KeyFrameEntry>)
     - FileMeta (info_hash, filename, file_size, piece_length, pieces, keyframe_index,
       duration_ms, video_codec, audio_codec, width, height, bitrate)
     - MediaStream (metadata + buffer reader endpoint)

  2. src/error.rs:
     - QvodError enum with thiserror:
       * Network(io::Error)
       * Protocol(String)
       * MetadataParse
       * DhtTimeout
       * DhtRoutingFailed
       * TrackerTimeout
       * TrackerProtocol(String)
       * ResourceNotFound(InfoHash)
       * NoPeers
       * NatFailed
       * CacheFull
       * CacheCorrupted(String)
       * UnsupportedFormat(String)
       * Decode(String)
       * InvalidUri(String)
       * Bencode(String)
       * PieceVerificationFailed { index: u32, expected: [u8; 20], got: [u8; 20] }
       * ConnectionLimitReached
       * Timeout(String)
       * Cancelled
     - 实现 Into<io::Error> 和 Into<String> 以便各层转换

  3. src/traits.rs:
     - DhtEngine: bootstrap, find_peers, announce, local_id, stats
     - Transport: connect, disconnect, send_request, send_piece, stats
     - CacheBackend: find, read, write, completion, cleanup
     - MetadataResolver: resolve_metadata (info_hash → FileMeta)

  4. src/constants.rs:
     - PIECE_LENGTH: u64 = 262144 (256KB)
     - BLOCK_LENGTH: u64 = 16384 (16KB)
     - MAX_BLOCKS_PER_PIECE: u32 = 16
     - PROTOCOL_MAGIC: [u8; 4] = [0x51, 0x56, 0x44, 0x54] ("QVDT")
     - HANDSHAKE_PROTOCOL: &str = "Qvod P2SP Protocol"
     - DEFAULT_PORT: u16 = 8621
     - MAX_PEER_CONNECTIONS: u32 = 50
     - DEFAULT_BUFFER_MB: u32 = 64
     - DHT_K: u8 = 8 (bucket 容量)
     - DHT_ALPHA: u8 = 3 (并发度)
     - DHT_REFRESH_INTERVAL: u64 = 900 (秒)
     - DHT_PEER_TIMEOUT: u64 = 1800 (秒)

  5. src/util.rs:
     - generate_peer_id() → [u8; 20] (随机 peer_id)
     - generate_node_id() → [u8; 20] (DHT 节点 ID)
     - xor_distance(a, b) → [u8; 20]
     - hex_encode(data) → String
     - hex_decode(s) → Result<Vec<u8>>
     - current_time_millis() → u64

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 2: qvs-format (格式工具)

```
实现内容:
  1. src/uri.rs:
     - QvodUri 结构体 (info_hash, filename, filesize, format)
     - QvodUri::from_str() — 解析 qvod:// 格式
     - QvodUri::to_string() — 序列化为 qvod:// 字符串
     - 验证: info_hash 必须为 40 字符 hex, filesize 必须为数字, 末尾必须 |
     - 错误: InvalidUri 明确错误位置

  2. src/bencode.rs:
     - BencodeValue enum: Int(i64), Str(Vec<u8>), List(Vec<BencodeValue>), Dict(BTreeMap)
     - BencodeValue::encode() → Vec<u8>
     - BencodeValue::decode(bytes) → Result<(BencodeValue, &[u8])>
     - 便捷函数: decode_int, decode_str, decode_list, decode_dict
     - 支持对 i64 编码 (i 数字 e)
     - 支持字符串长度前缀编码
     - 支持嵌套 dict 和 list
     - 往返测试: encode(decode(x)) == x

  3. src/qvs_file.rs:
     - QvsFile 结构体 (info_hash, filename, file_size, piece_length, pieces, trackers, etc)
     - QvsFile::encode() → Vec<u8> (Bencode)
     - QvsFile::decode(data) → Result<Self>
     - 支持 keyframe_index 可选字段

  4. src/cache.rs:
     - CacheConfig (cache_dir, max_size, max_files)
     - CacheEntry (info_hash, file_size, downloaded, bitfield, last_access, created_at)
     - CacheManager 实现:
       * find(info_hash) → Option<CacheEntry>
       * read(info_hash, offset, length) → Result<Vec<u8>>
       * write(info_hash, offset, data) → Result<()>
       * completion(info_hash) → f64 (0.0 ~ 1.0)
       * cleanup() → Result<()> (LRU 淘汰, 直到低于 max_size 的 80%)
       * delete(info_hash) → Result<()>
       * list() → Vec<CacheEntry> (所有缓存条目)
     - 文件格式: {cache_dir}/qdata/{hash_hex}.qdata (稀疏文件)
     - 元数据文件: {cache_dir}/qmv/{hash_hex}.qmv (Bencode 编码的 FileMeta)
     - 使用 tokio::fs 异步文件操作
     - 线程安全: Arc<Mutex<CacheManager>>

  5. src/bitfield.rs:
     - Bitfield 结构体 (内部 bytes: Vec<u8>)
     - Bitfield::new(num_pieces) → Self
     - Bitfield::from_bytes(bytes) → Self
     - Bitfield::has(index) → bool
     - Bitfield::set(index, value)
     - Bitfield::set_all(value)
     - Bitfield::count() → u32 (已设置 bit 数)
     - Bitfield::completion() → f64
     - Bitfield::is_empty() → bool
     - Bitfield::to_bytes() → &[u8]
     - Bitfield::iter() → impl Iterator<Item=bool>

  6. src/keyframe.rs:
     - KeyFrameEntry, KeyFrameIndex, FrameType (从 qvs-core 引入)
     - KeyFrameIndex::find_nearest_i_frame(timestamp) → Option<&KeyFrameEntry>
     - KeyFrameIndex::find_all_i_frames() → Vec<&KeyFrameEntry>
     - KeyFrameIndex::segment_at(segment_index) → (offset, length)

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 3: qvs-dht (DHT 网络)

```
实现内容:
  1. src/node.rs:
     - DhtConfig (listen_port, k, alpha, refresh_interval, peer_timeout, seed_nodes)
     - DhtStats (total_peers_found, messages_sent, messages_received, routing_table_size)
     - DhtNode 主结构体 (routing_table, token_manager, config, stats, socket)
     - DhtNode::new(config) → Self
     - DhtNode::start(listener: mpsc::Receiver) → task JoinHandle
     - DhtNode::stop()
     - 实现 DhtEngine trait

  2. src/routing.rs:
     - KBucket (entries: VecDeque<KBucketEntry>, max K=8)
     - KBucket::insert(entry) → 替换策略 (最近活跃优先保留)
     - KBucket::find_closest(target, count) → Vec<KBucketEntry>
     - KBucket::remove(node_id)
     - KBucket::refresh_needed() → bool (上次刷新超过 15 分钟)
     - RoutingTable (buckets: [KBucket; 160], local_id)
     - RoutingTable::new(local_id) → Self
     - RoutingTable::insert(entry) → 分裂规则 (非满 bucket 直接插入, 满则判断是否需要分裂)
     - RoutingTable::find_closest(target, count) → Vec<KBucketEntry>
     - RoutingTable::refresh_list() → Vec<usize> (需要刷新的 bucket 索引)
     - RoutingTable::size() → usize

  3. src/rpc.rs:
     - MessageHeader (magic: [u8;4], msg_type: u8, txn_id: u16, ver: u8)
     - MessageType enum: Ping(0x00), FindNode(0x01), FindPeers(0x02), Announce(0x03)
     - DhtMessage enum (header + 各类型负载)
     - DhtMessage::encode() → Vec<u8>
     - DhtMessage::decode(bytes) → Result<Self>
     - 验证: magic 必须匹配, 长度校验, 版本兼容校验
     - 严格限制单包 1400 字节

  4. src/bootstrap.rs:
     - bootstrap(engine, seed_nodes) → Result<()>
     - 阶段 1: 向所有种子节点发送 PING, 收集响应
     - 阶段 2: 向种子节点发送 FIND_NODE(local_id)
     - 阶段 3: 迭代查询, 每次取距离最近的 α 个节点
     - 阶段 4: 路由表非空时结束
     - 超时: 每轮 5 秒, 最多 3 轮
     - 每隔 15 分钟刷新空闲 bucket

  5. src/krpc.rs:
     - KademliaRpc 实现 (FIND_NODE, FIND_PEERS, ANNOUNCE)
     - handle_ping(request) → Response
     - handle_find_node(request) → Response (返回距离目标最近的 K 个节点)
     - handle_find_peers(request) → Response (返回 peers 或 nodes)
     - handle_announce(request) → Response (存储 peer 信息, 验证 token)
     - find_peers 迭代查询: 每次取 α 个最近节点并行查询
     - 去重: 同一 info_hash 最多保留 50 个 peer

  6. src/token.rs:
     - TokenManager
     - generate_token(addr) → [u8; 4] (基于 IP + secret 的 HMAC)
     - verify_token(addr, token) → bool
     - rotate_secret() (每 10 分钟轮换, 保留上一个 5 分钟防止竞争)

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 4: qvs-tracker (Tracker 客户端)

```
实现内容:
  1. src/client.rs:
     - TrackerConfig (tracker_urls, peer_id, port, compact: bool)
     - TrackerClient 结构体
     - announce(info_hash, event, uploaded, downloaded, left) → Result<Vec<PeerInfo>>
     - scrape(info_hashes) → Result<SwarmStatus>
     - HTTP 请求: reqwest GET
     - 超时: 连接 10s, 响应 30s
     - 重试: 3 次, 指数退避
     - 多 tracker 负载均衡: 随机选择, 失败自动切换

  2. src/protocol.rs:
     - AnnounceParams (info_hash, peer_id, port, uploaded, downloaded, left, event, compact)
     - AnnounceParams::to_query() → String (URL 查询参数)
     - AnnounceResponse 解析:
       * interval, min_interval, complete, incomplete, downloaded
       * peers: compact (6 字节/peer: IP4 + port) 或 dict 格式
     - 验证响应 Bencode 结构完整性

  3. src/scraper.rs:
     - scrape 请求构造: GET /scrape?info_hash=hex1&info_hash=hex2&...
     - 解析 Bencode 响应
     - 返回每个 info_hash 的 complete/incomplete/downloaded

  4. src/peer_list.rs:
     - parse_compact_peers(data) → Vec<PeerInfo> (每 6 字节: 4 字节 IP + 2 字节 port)
     - parse_dict_peers(list) → Vec<PeerInfo> (Bencode dict 列表)

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 5: qvs-transport (P2SP 传输层)

```
实现内容:
  1. src/handshake.rs:
     - Handshake 结构体 (pstrlen, pstr, reserved, info_hash, peer_id)
     - Handshake::encode() → [u8; 68]
     - Handshake::decode(bytes) → Result<Self>
     - Handshake::verify() → 验证 pstr 是否为 "Qvod P2SP Protocol"
     - 扩展位解析: reserved[5] & 0x10 表示支持 ut_metadata

  2. src/message.rs:
     - MsgId enum (choke/unchoke/interested/not_interested/have/bitfield/request/piece/...)
     - PeerMessage 结构体 (length_prefix, msg_id, payload)
     - PeerMessage::encode() → Vec<u8>
     - PeerMessage::decode(bytes) → Result<Self>
     - 各消息类型的 payload 编解码:
       * have: piece_index (u32 big-endian)
       * bitfield: bitfield bytes
       * request: index(u32) + begin(u32) + length(u32)
       * piece: index(u32) + begin(u32) + block data
       * cancel: index(u32) + begin(u32) + length(u32)
       * port: dht_port (u16)
       * suggest_piece: piece_index (u32)
     - keep-alive: length_prefix = 0 的消息

  3. src/tcp_stream.rs:
     - TcpStreamManager
     - connect(addr) → Result<()>
     - send_handshake(info_hash, peer_id) → Result<()>
     - receive_handshake() → Result<(info_hash, peer_id, reserved)>
     - send_message(msg) → Result<()>
     - read_message() → Result<PeerMessage>
     - 使用 tokio::net::TcpStream, 非阻塞 I/O
     - 读写超时: 30 秒

  4. src/udp_stream.rs:
     - UdpPacket (msg_type, seq, piece_index, block_offset, payload)
     - UdpPacket::encode() → Vec<u8>
     - UdpPacket::decode(bytes) → Result<Self>
     - UdpTransport (socket, send_queue, pending_acks, congestion_ctrl)
     - send_data(packet) → Result<()> (受拥塞窗口控制)
     - receive_ack(seq) → 更新拥塞状态
     - retransmit_timeout() → 重发未确认的包
     - 最大包大小: 1400 字节

  5. src/congestion.rs:
     - UdpCongestionControl
     - 状态: SlowStart / CongestionAvoidance / FastRecovery
     - on_ack(seq, rtt): SlowStart → cwnd += 1, 直到 ssthresh
     - on_loss(): cwnd /= 2, ssthresh = cwnd
     - can_send(): 当前飞行中包数 < cwnd
     - wait_time(): 基于 RTT 的速率整形
     - 流媒体优化: 当 loss_rate > 10% 时切换为仅 TCP 模式

  6. src/pool.rs:
     - ConnectionPool
     - max_connections: u32 (默认 50)
     - add_peer(peer_info) → Result<()>
     - remove_peer(peer_id)
     - get_peer(peer_id) → Option<&PeerConnection>
     - select_upload_peers(count) → Vec<&PeerConnection>
     - select_download_peers(count, priority) → Vec<&PeerConnection>
     - cleanup_idle() → 清理超时连接 (5 分钟无活动)
     - maintain_connections() → 保活 ping
     - stats() → PoolStats

  7. src/scheduler.rs:
     - PieceScheduler (playhead, metadata, priority_map)
     - calculate_priority(piece) → PiecePriority
     - next_request(peers_bitfields) → Option<BlockRequest>
     - set_seek_target(piece_index)
     - select_peer_for_piece(piece, peers) → 选择最优 peer
     - rarest_first(piece, bitfields) → 稀有度优先 (避免冗余)

  8. src/p2sp.rs:
     - P2spDownloader (p2p_engine, http_sources)
     - select_source(piece, priority) → Source (Parallel/P2PWithHttpFallback/P2POnly/P2PIdle)
     - download_critical(piece) → 并行从 P2P + HTTP 下载, 取先完成的
     - download_high(piece) → P2P 优先, HTTP 3 秒超时后备
     - download_normal(piece) → 仅 P2P
     - download_idle(piece) → 低优先级后台填充

  9. src/peer_wire.rs:
     - PeerWireProtocol (handle_choke, handle_unchoke, handle_interested, etc)
     - state machine: ConnectionState 转换
     - 对等协议交互: interested → unchoke → request → piece
     - 端序处理: 所有多字节字段 big-endian

  10. src/nat.rs:
      - NatType enum: None, FullCone, RestrictedCone, PortRestrictedCone, Symmetric
      - detect_nat_type() → NatType (使用 STUN 风格探测)
      - udp_hole_punching(addr) → Result<()>
      - relay_fallback(relay_addr) → 建立中继连接
      - UPnP port mapping (可选, 使用 igd-next crate)

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 6: qvs-stream (流媒体引擎)

```
实现内容:
  1. src/engine.rs:
     - QvodEngine 主结构体 (local_server, tracker, dht, transport, buffer, scheduler, metadata)
     - QvodEngine::new(config) → Self
     - QvodEngine::play(uri) → Result<MediaStream>
     - QvodEngine::pause()
     - QvodEngine::resume()
     - QvodEngine::stop(info_hash)
     - QvodEngine::seek(timestamp_ms) → Result<()>
     - QvodEngine::status(info_hash) → StreamStatus
     - play() 内部流程:
       a. 解析 URI → info_hash
       b. 检查缓存
       c. 获取 peer 列表 (Tracker + DHT 并行)
       d. 连接最优 peers
       e. 获取 Metadata (扩展协议)
       f. 初始化 RingBuffer + PieceScheduler
       g. 开始调度下载
       h. 返回 MediaStream
     - 主事件循环: 处理缓冲事件、网络事件、用户事件

  2. src/buffer.rs:
     - RingBuffer (capacity, data, play_cursor, write_cursor, filled_ranges)
     - write(offset, data) → Result<()>
     - read(offset, length) → Result<&[u8]>
     - is_playable() → bool (头部 >= 1 秒数据且包含 I 帧)
     - buffered_duration() → Duration
     - adapt_watermarks(speed) → 动态调整水位
     - filled_percentage() → f64
     - clear()
     - 线程安全: Arc<RwLock<RingBuffer>>

  3. src/metadata.rs:
     - MetadataResolver (从 peer 获取 ut_metadata)
     - request_metadata(peer_conn) → Result<FileMeta>
     - parse_metadata(raw_bencode) → Result<FileMeta>
     - 缓存 metadata 到 qmv 文件

  4. src/seek.rs:
     - SeekEngine (metadata, buffer, scheduler)
     - seek_to(timestamp_ms) → Result<()>
     - find_nearest_keyframe(timestamp) → KeyFrameEntry
     - reschedule_priorities(target_piece)
     - reset_play_cursor(offset)

  5. src/hls.rs:
     - HlsAdapter (metadata, segment_duration, output_dir)
     - generate_m3u8() → String (M3U8 播放列表)
     - wrap_as_ts(data, offset) → Vec<u8>
     - segment_info(index) → (offset, length, duration)
     - M3U8: #EXTM3U, #EXT-X-VERSION:3, #EXT-X-TARGETDURATION
     - 每个 segment 在 I 帧边界切割

  6. src/adaptive.rs:
     - AdaptiveBuffer (stats, state)
     - tick() → BufferCommand
     - BufferCommand: PauseAndBuffer, ThrottleUpload, Normal, IncreaseHttpRatio
     - 速度测量: 滑动窗口 10 秒
     - RTT 测量: 平均 RTT 100 秒窗口
     - 缓冲不足: playable < 2s && speed < 100KB/s → 暂停播放

  7. src/playback.rs:
     - MediaStream 实现 (提供 Read 接口给播放器)
     - StreamStats (position, duration, speed, buffered, peers, state)
     - EOS detection: 所有 piece 下载完成

  8. src/config.rs:
     - EngineConfig 结构体 (listen_port, udp_port, max_connections, buffer_capacity_mb,
       cache_dir, tracker_urls, dht_seed_nodes, http_fallback)
     - EngineConfig::default() → Self
     - EngineConfig::load(path) → Result<Self> (从 TOML 文件加载)

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 7: qvs-local-server (本地 Web 服务)

```
实现内容:
  1. src/server.rs:
     - LocalServer (axum Router + tokio task handle)
     - start(config) → Result<Self> (绑定端口, 启动 HTTP 服务)
     - stop() (优雅关闭, 等待当前请求完成)
     - port() → u16 (实际绑定的端口)

  2. src/handler.rs:
     - GET /play?hash=&name=&size= → 流式响应
     - GET /play?hash=&offset= → Range 请求 (seek)
     - GET /status?hash= → JSON 状态
     - GET /segment?offset=0&length=0 → HLS 伪分片
     - POST /control?action=pause|resume|stop

  3. src/stream.rs:
     - ChunkedStream (tokio::sync::mpsc::Receiver)
     - 实现 axum::body::HttpBody 或 IntoResponse
     - 流量控制: 背压 (当通道满时暂停写入)

  4. src/config.rs:
     - LocalServerConfig (preferred_port: u16, max_retry: u8)
     - port_available(port) → bool (检查端口可用性)
     - find_available_port(preferred, max_retry) → u16

  5. src/range.rs:
     - RangeHeader 解析 (支持 bytes=start-end, bytes=start-, bytes=-suffix)
     - RangeResult (start, end, total_length)
     - Content-Range 响应头构造

  6. src/middleware.rs:
     - CORS 头 (允许所有来源, 用于 Web 播放器)
     - 请求日志 (方法, 路径, 状态码, 耗时)
     - 速率限制 (每 IP 每秒最多 100 请求)

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 8: qvs-media (媒体解码)

```
实现内容:
  1. src/demuxer.rs:
     - Demuxer trait (open, read_frame, seek, duration, info)
     - FfmpegDemuxer (基于 ffmpeg-next)
     - open(path) → Result<Self> (打开媒体文件)
     - read_frame() → Result<MediaFrame>
     - seek(timestamp) → Result<()>
     - info() → MediaInfo (codec, resolution, bitrate, duration)

  2. src/decoder.rs:
     - VideoDecoder (AVCodecContext, AVFrame)
     - decode_video(packet) → Result<AVFrame>
     - AudioDecoder
     - decode_audio(packet) → Result<AVFrame>
     - 硬件加速检测 (VAAPI, VideoToolbox, DXVA)

  3. src/renderer.rs:
     - VideoRenderer (egui texture, OpenGL)
     - render_frame(frame) → egui::TextureId
     - AudioRenderer (cpal 或 rodio)
     - play_audio(samples) → Result<()>

  4. src/format.rs:
     - probe_format(path) → Result<MediaFormat>
     - MediaFormat enum (Rmvb, Avi, Mkv, Mp4, Wmv, Flv, etc)
     - 基于文件魔数 (magic bytes) 探测
     - 支持格式列表: rmvb, avi, mkv, mp4, wmv, flv, mov, ts, webm

  5. src/resampler.rs:
     - AudioResampler (ffmpeg swr)
     - resample(input_frame, target_sample_rate, target_channels) → Result<Vec<f32>>

  6. src/sync.rs:
     - AudioVideoSync (基于 PTS/DTS)
     - sync_strategy: AudioMaster (默认), VideoMaster, ExternalMaster
     - 计算音视频时间差, 丢帧/插帧策略
     - 允许误差: audio 领先 video < 20ms 不调整

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

#### 步骤 9: qvs-gui (GUI 播放器)

```
实现内容:
  1. src/main.rs:
     - CLI 参数解析 (clap):
       * qvs play <uri>
       * qvs status
       * qvs list
       * qvs cache (--clean, --size)
       * qvs settings
     - 启动 egui 窗口

  2. src/app.rs:
     - QvodApp 实现 egui::App
     - update(ctx, frame) → 渲染各面板
     - 状态管理: PlayerState enum
     - 事件处理: 键盘快捷键 (Space=暂停, 左右箭头=快进快退)

  3. src/player.rs:
     - VideoPanel (渲染视频帧到 egui 纹理)
     - 视频显示: Image/Texture 控件
     - 缓冲进度指示器 (圆形进度条或波形)
     - 错误覆盖层

  4. src/controls.rs:
     - 播放/暂停按钮 (Space 快捷键)
     - 进度条 (可拖拽, 显示缓冲范围)
     - 音量控制 (滑块 + 静音按钮)
     - 时间显示 (当前时间 / 总时长)
     - 快进/快退 (左右箭头, 每次 10 秒)

  5. src/playlist.rs:
     - 播放列表 (Vec<PlaylistEntry>)
     - 添加/删除/清空
     - 拖拽排序
     - 历史记录 (最近 100 条, 持久化到 JSON)
     - 右键菜单 (复制链接, 删除, 属性)

  6. src/settings.rs:
     - 缓存目录选择
     - 缓存大小限制滑块 (1GB ~ 100GB)
     - 本地服务器端口输入
     - 最大连接数滑块 (10 ~ 200)
     - HTTP 后备开关
     - Tracker 地址列表编辑
     - DHT 种子节点编辑
     - 语言选择 (zh-CN, en-US)
     - 主题选择 (深色/浅色/跟随系统)
     - 设置持久化: TOML 文件

  7. src/status.rs:
     - 网络状态面板:
       * 当前速度 (下行/上行)
       * 已连接 peer 数
       * 缓冲进度 (%)
       * 下载进度 (%)
       * DHT 路由表大小
       * 活跃连接列表
     - 实时更新 (每秒刷新)

  8. src/overlay.rs:
     - 缓冲提示: "缓冲中..." + 进度条
     - 错误提示: 红色覆盖 + 错误详情
     - 信息覆盖: 显示当前分辨率/码率/编码格式
     - 淡入淡出动画

  9. src/theme.rs:
     - 颜色常量 (背景, 前景, 强调色, 成功/警告/错误色)
     - 字体设置
     - 控件样式 (圆角, 边距, 阴影)
     - 深色/浅色主题切换

验收: cargo build + cargo test + cargo clippy + cargo fmt
```

### 5. 通信约定

- 实现完整模块后，更新 AGENTS.md 中对应 crate 的状态为 `✅ 已完成`
- 遇到无法决策的设计问题，回退到 spec 文档按逻辑推导
- 接口变更需同时更新 spec 文档和 AGENTS.md
- 每个 pub 函数必须有文档注释 (`///`)
- 所有错误必须返回 QvodError (或其子集)
- 异步函数使用 async/await + tokio runtime

### 6. 代码审查标准

在将 crate 标记为完成前，必须检查：
- [ ] 所有公共 API 有文档注释
- [ ] 单元测试覆盖率 > 80%
- [ ] 协议编解码有往返测试
- [ ] clippy 无警告
- [ ] cargo fmt 通过
- [ ] 无 unsafe 代码 (除非批准)
- [ ] 所有 Result 返回 QvodError 类型
- [ ] 网络模块使用 mock 测试
- [ ] 无硬编码常量 (使用 constants.rs)
- [ ] 日志覆盖关键路径

---

## Crate 实现状态

| Crate | 状态 | 依赖 | 优先级 | 备注 |
|-------|------|------|--------|------|
| qvs-core | ✅ 已完成 | 无 | P0 | 基础类型与 trait，必须先完成 |
| qvs-format | ✅ 已完成 | qvs-core | P0 | URI、Bencode、缓存。CacheManager 已修复：稀疏写入、LRU 清理、bitfield 追踪 |
| qvs-dht | ✅ 已完成 | qvs-core | P1 | Kademlia DHT：路由表分裂、迭代 find_peers、桶刷新、23 个测试 |
| qvs-tracker | ✅ 已完成 | qvs-core | P1 | HTTP Tracker：指数退避重试、多 tracker 负载均衡、超时控制 |
| qvs-transport | ✅ 已完成 | qvs-core, qvs-format | P1 | 核心传输层：完整消息解析 (11 种消息)、NAT 穿越 (STUN)、P2SP 下载 (4 级优先级)、拥塞控制 |
| qvs-stream | ✅ 已完成 | qvs-core, qvs-transport, qvs-format | P1 | 核心引擎：QvodEngine 集成 tracker+DHT+transport+cache+seeker，async play/pause/seek/stop |
| qvs-local-server | ✅ 已完成 | qvs-stream | P2 | HTTP 流式服务器：/play 流媒体、/segment 切片、POST /control、IP 速率限制 (100 req/s)、优雅关闭 |
| qvs-media | ✅ 已完成 | qvs-core | P2 | 媒体层 (stubs：dev libs 未安装时优雅返回错误) |
| qvs-gui | ✅ 已完成 | qvs-stream, qvs-media | P2 | egui 播放器：播放器面板、控制栏、播放列表、设置页、状态面板、覆盖层、深色/浅色主题 |
| qvs-server | ✅ 已完成 | qvs-stream, qvs-local-server | P1 | Headless 守护进程：8 个测试 |
| qvs-cli | ✅ 已完成 | qvs-stream | P1 | CLI 客户端：play/status/list/cache 子命令，90 个测试 |

## 构建验证

```bash
# 完整构建
cargo build --workspace

# 运行所有测试
cargo test --workspace

# 所有 clippy
cargo clippy --workspace -- -D warnings

# 格式化
cargo fmt

# 文档构建
cargo doc --no-deps

# 测试覆盖率 (需要 cargo-llvm-cov)
cargo llvm-cov --all-features --workspace --html

# 安全审计 (需要 cargo-audit)
cargo audit
```

## 参考资源

- 完整技术规格: `docs/superpowers/specs/2025-07-01-qvod-system-design.md`
- 架构设计: `agents/architecture.md`
- 编码规范: `agents/coding.md`
- 安全规范: `agents/security.md`
- 协议定义: `agents/protocol.md` | `docs/protocol/`
- Tracker 协议: `agents/tracker.md` | `docs/tracker/`
- Peer 管理: `agents/peer.md`
- 元数据格式: `agents/metadata.md`
- 调度算法: `agents/scheduler.md` | `docs/scheduler/`
- 下载引擎: `agents/download.md`
- 缓存系统: `agents/cache.md` | `docs/cache/`
- 存储格式: `agents/storage.md` | `docs/storage/`
- 本地网关: `agents/gateway.md`
- 流媒体引擎: `docs/streaming/`
- 播放器: `agents/player.md`
- 监控统计: `agents/monitoring.md`
- 构建部署: `agents/deployment.md`
- API 参考: `docs/api/`
- 数据库: `docs/database/`
- Qvod 原始架构: P2SP + UDP 混合加速, 基于 Modified BitTorrent 协议做流媒体优化
- 目标语言: Rust (跨平台: Linux / macOS / Windows)
- Rust 版本: 2024 edition
- GUI 框架: egui
- 音视频解码: ffmpeg-next (FFmpeg 绑定)
- 异步运行时: tokio (multi-thread)
- HTTP 框架: axum
- HTTP 客户端: reqwest

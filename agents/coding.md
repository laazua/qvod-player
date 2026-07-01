# QVOD Rust 编码规范

## 1. Rust 版本与工具链

### 1.1 版本要求

```toml
# rust-toolchain.toml
[toolchain]
channel = "stable"
components = ["rustc", "cargo", "clippy", "rustfmt", "rust-analyzer"]
```

- 使用 Rust 2024 edition
- 最低支持 Rust 1.80+
- 使用 `cargo clippy` 作为 lint 标准
- 使用 `rustfmt` 作为格式化标准，对齐方式使用 `ChainIndent`

### 1.2 工作区配置

```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "2"
members = [
    "crates/qvs-core",
    "crates/qvs-format",
    "crates/qvs-dht",
    "crates/qvs-tracker",
    "crates/qvs-local-server",
    "crates/qvs-transport",
    "crates/qvs-stream",
    "crates/qvs-media",
    "crates/qvs-gui",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
authors = ["QVOD Team"]
license = "MIT"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
futures = "0.3"
thiserror = "2"
tracing = "0.1"
serde = { version = "1", features = ["derive"] }
serde_bytes = "0.11"
sha1 = "0.10"
rand = "0.8"
```

### 1.3 crate 命名与目录规范

| 元素 | 规范 | 示例 |
|------|------|------|
| crate 名称 | `qvs-{模块名}` | `qvs-core`, `qvs-transport` |
| 目录名 | crate 名称一致 | `crates/qvs-core/` |
| 主类型 | `lib.rs` 中 pub 导出 | `pub use types::InfoHash;` |
| 测试文件 | 内联 `#[cfg(test)] mod tests` | 不单独创建 test 文件 |
| 集成测试 | `tests/` 目录 | `tests/integration/p2p_transfer.rs` |

## 2. 模块组织

### 2.1 文件命名

```rust
// lib.rs - 只做模块重新导出
mod types;
mod error;
mod traits;
mod constants;
mod util;

pub use types::*;
pub use error::*;
pub use traits::*;
pub use constants::*;
pub use util::*;
```

### 2.2 模块职责

- 一个 crate 一个职责，避免循环依赖
- `qvs-core` 是所有 crate 的唯一基底依赖
- trait 定义放在 `qvs-core` 或使用该 trait 的 crate 中
- 禁止非安全代码 (`unsafe`) 除非经过特别批准 (需要在 `CLAUDE.md` 中记录)
- 跨 crate 数据传递使用 `Arc<Mutex<T>>` 或通道 (`tokio::sync::mpsc`)
- 纯数据结体使用 `#[derive(Clone, Debug, PartialEq)]`
- 网络相关的结构体需要 `Send + Sync`

### 2.3 文件大小限制

- 单个 `.rs` 文件不超过 800 行
- 超过时拆分为子模块
- `lib.rs` 不超过 50 行（只做导出）

## 3. 命名规范

### 3.1 通用规则

```
类型/结构/枚举: PascalCase        → FileMeta, PiecePriority, InfoHash
方法/函数:     snake_case         → generate_peer_id(), is_playable()
常量:          SCREAMING_SNAKE    → PIECE_LENGTH, MAX_PEER_CONNECTIONS
特征:          首字母 I 前缀或 PascalCase → DhtEngine, Transport,
                  CacheBackend
枚举变体:      PascalCase          → Critical, High, Normal, Low
私有辅助:      _ 前缀               → _validate(), _do_split()
```

### 3.2 命名原则

```rust
// 好的命名: 自文档化
pub fn find_nearest_i_frame(&self, timestamp: Duration) -> Option<&KeyFrameEntry>;

// 好的命名: 动词 + 名词
pub fn set_seek_target(&mut self, piece_index: u32);

// 避免缩写: 除非是行业标准
// 好: info_hash, peer_id, rtt
// 避免: calc_prio, upd_buf, cfg_mgr

// 布尔方法: is_ / has_ / can_ / should_
pub fn is_playable(&self) -> bool;
pub fn has_piece(&self, index: u32) -> bool;
pub fn can_send(&self) -> bool;
pub fn should_retry(&self) -> bool;
```

## 4. 编码风格

### 4.1 格式化

使用 rustfmt 默认配置，添加以下覆盖：

```rust
// rustfmt 配置: 在 rust-toolchain.toml 或 .rustfmt.toml
// 使用 ChainIndent 风格
fn long_function_name(
    param1: VeryLongTypeName,
    param2: AnotherLongTypeName,
    param3: YetAnotherType,
) -> Result<SomeReturnType> {
    // ...
}
```

### 4.2 导入顺序

```rust
// 顺序: 标准库 → 外部依赖 → workspace 内部依赖 → 当前 crate

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};

use qvs_core::types::InfoHash;
use qvs_core::error::QvodError;

use crate::types::PeerConnection;
```

### 4.3 错误处理模式

```rust
use std::result::Result as StdResult;

/// crate 级别的 Result 别名
pub type Result<T> = StdResult<T, QvodError>;

/// 使用 thiserror 定义错误
#[derive(Debug, thiserror::Error)]
pub enum QvodError {
    #[error("网络错误: {0}")]
    Network(#[from] std::io::Error),

    #[error("协议错误: {0}")]
    Protocol(String),

    #[error("无效 URI: {0}")]
    InvalidUri(String),
}

/// 错误转换模式
impl From<&str> for QvodError {
    fn from(msg: &str) -> Self {
        QvodError::Protocol(msg.to_string())
    }
}

/// 不要吞掉错误上下文
// 不好的做法:
let _ = do_something();  // 忽略错误
do_something().unwrap(); // 可能 panic

// 好的做法:
do_something().map_err(|e| QvodError::Protocol(format!("操作失败: {e}")))?;

// 或使用 context (如果有)
do_something().context("执行操作失败")?;
```

### 4.4 文档注释

```rust
/// 每个公共 API 必须有文档注释
///
/// 第一行是简短描述，空一行后是详细说明。
/// 文档注释必须包含:
///   - 参数说明 (如果函数有参数)
///   - 返回值说明
///   - Panic 条件 (如果可能)
///   - 示例 (如果是公共 API)
///
/// # Arguments
///
/// * `timestamp_ms` - 目标时间戳（毫秒），相对于视频开始
///
/// # Returns
///
/// * `Ok(())` - 定位成功
/// * `Err(QvodError::NoKeyFrame)` - 该时间附近无关键帧
///
/// # Examples
///
/// ```no_run
/// # use qvs_core::*;
/// let mut engine = SeekEngine::new(metadata, buffer, scheduler);
/// engine.seek_to(5000).expect("seek failed");
/// ```
pub fn seek_to(&mut self, timestamp_ms: u64) -> Result<()> {
    // ...
}
```

### 4.5 匹配与模式

```rust
// 好的: 全面覆盖所有分支
match priority {
    PiecePriority::Critical => { /* ... */ }
    PiecePriority::High => { /* ... */ }
    PiecePriority::Normal => { /* ... */ }
    PiecePriority::Low => { /* ... */ }
}

// 不要用 if-else 链代替 match
// 不要对 Option 使用 unwrap() - 使用 expect() 并说明原因
let value = optional_value.expect("metadata must be resolved before scheduling");

// 使用 if let 简化单分支匹配
if let Some(peer) = pool.get_peer(peer_id) {
    peer.send_request(request);
}
```

## 5. 异步编程规范

### 5.1 异步运行时

- 使用 `tokio` 多线程运行时
- 默认 `#[tokio::main(flavor = "multi_thread", worker_threads = 4)]`
- 网络 I/O 使用 tokio 的异步 `TcpStream` 和 `UdpSocket`
- 文件 I/O 使用 `tokio::fs`
- 同步操作 (如 SHA-1 哈希) 使用 `tokio::task::spawn_blocking`

```rust
// 好的: 异步函数
pub async fn play(&mut self, uri: &QvodUri) -> Result<MediaStream> {
    // ...
}

// 好的: CPU 密集型任务使用 spawn_blocking
pub fn verify_piece(data: &[u8], expected_hash: &[u8; 20]) -> bool {
    let hash = sha1::Sha1::from(data).digest().bytes();
    &hash == expected_hash
}

// 调用时:
let data = buffer.read(offset, length)?;
let is_valid = tokio::task::spawn_blocking(move || {
    verify_piece(&data, &expected_hash)
}).await?;
```

### 5.2 异步 Trait

```rust
// 使用 async-trait 或 tokio 内置模式
// 注意: Rust 2024 edition 支持 async fn in trait

#[async_trait]
pub trait DhtEngine: Send + Sync {
    async fn bootstrap(&self, seed_nodes: &[SocketAddr]) -> Result<()>;

    async fn find_peers(
        &self,
        info_hash: &InfoHash,
    ) -> Result<mpsc::Receiver<PeerInfo>>;

    async fn announce(&self, info_hash: &InfoHash, port: u16) -> Result<()>;

    fn local_id(&self) -> &NodeId;

    fn stats(&self) -> DhtStats;
}
```

### 5.3 通道选择

```rust
// tokio::sync::mpsc: 多生产者单消费者
// 用于: Engine → HTTP Response 流
let (tx, rx) = mpsc::channel::<Vec<u8>>(64);  // 64 个 slot 背压

// tokio::sync::watch: 一生产者多消费者
// 用于: 状态更新 (stream status)
let (tx, rx) = watch::channel(StreamStatus::default());

// tokio::sync::oneshot: 一次性
// 用于: 请求-响应
let (tx, rx) = oneshot::channel::<PeerInfo>();

// 主循环使用 tokio::select!
loop {
    tokio::select! {
        Some(msg) = rx.recv() => handle(msg),
        _ = interval.tick() => do_periodic(),
        cmd = cmd_rx.recv() => handle_command(cmd),
        else => break,
    }
}
```

### 5.4 超时处理

```rust
// 所有网络操作必须设置超时
use tokio::time::{timeout, Duration};

// 连接超时: 10 秒
let stream = timeout(Duration::from_secs(10), TcpStream::connect(addr))
    .await
    .map_err(|_| QvodError::Timeout("连接超时".into()))??;

// 读写超时: 30 秒
let msg = timeout(Duration::from_secs(30), read_message(&mut stream))
    .await
    .map_err(|_| QvodError::Timeout("读取消息超时".into()))??;

// DHT 查询超时: 5 秒
let result = timeout(Duration::from_secs(5), find_node(target))
    .await
    .unwrap_or(Err(QvodError::DhtTimeout))?;
```

## 6. 类型与泛型

### 6.1 newtype 模式

```rust
// 使用 newtype 增加类型安全
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InfoHash(pub [u8; 20]);

impl InfoHash {
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s)
            .map_err(|_| QvodError::InvalidUri("info_hash 不是合法 hex".into()))?;
        if bytes.len() != 20 {
            return Err(QvodError::InvalidUri("info_hash 长度必须为 20 字节".into()));
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes);
        Ok(InfoHash(arr))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl Display for InfoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}
```

### 6.2 泛型约束

```rust
// 使用泛型时要明确约束
pub struct CacheManager<B: CacheBackend> {
    backend: B,
    config: CacheConfig,
}

impl<B: CacheBackend> CacheManager<B> {
    pub fn find(&self, info_hash: &InfoHash) -> Option<CacheEntry> {
        self.backend.find(info_hash)
    }
}

// 只接受 trait object 时使用 Box<dyn Trait>
pub struct QvodEngine {
    dht: Arc<dyn DhtEngine>,
    transport: Arc<dyn Transport>,
    cache: Arc<dyn CacheBackend>,
}
```

### 6.3 枚举设计

```rust
/// 使用 #[non_exhaustive] 允许未来扩展
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PiecePriority {
    Critical,  // 立即播放
    High,      // 30秒内
    Normal,    // 30-120秒
    Low,       // 已播放区域
}

/// 有数据的枚举使用命名域
pub struct Piece {
    pub index: u32,
    pub hash: [u8; 20],
    pub priority: PiecePriority,
    pub length: u64,
}

/// 避免从枚举转换的 unwrap
#[derive(Debug)]
pub enum Source {
    Parallel { p2p: bool, http: bool },
    P2pWithHttpFallback { timeout: Duration },
    P2pOnly,
    P2pIdle,
}
```

## 7. 内存与性能

### 7.1 避免不必要的分配

```rust
// 不好的: 频繁分配 Vec
pub fn get_peers(&self) -> Vec<PeerInfo> {
    self.peers.values().cloned().collect()
}

// 好的: 返回迭代器或引用 slice
pub fn peers(&self) -> impl Iterator<Item = &PeerInfo> {
    self.peers.values()
}

// 好的: 使用 Cow 避免拷贝
pub fn get_name(&self) -> Cow<'_, str> {
    if let Some(ref name) = self.name {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("Resource {}", self.info_hash))
    }
}
```

### 7.2 零拷贝缓冲区

```rust
// RingBuffer 使用 Vec<u8> 预分配
// 避免频繁 resize
pub struct RingBuffer {
    data: Vec<u8>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity_mb: u32) -> Self {
        let capacity = (capacity_mb as usize) * 1024 * 1024;
        // 预分配整个缓冲区，避免运行时的 reallocation
        let mut data = Vec::with_capacity(capacity);
        // 安全: 我们手动管理这个内存
        unsafe { data.set_len(capacity); }
        Self { data, capacity }
    }
}

// 使用 bytes::Bytes 避免数据拷贝
use bytes::Bytes;

pub async fn read_chunk(&self, offset: u64, length: usize) -> Result<Bytes> {
    let data = self.cache.read(&self.info_hash, offset, length as u64)?;
    Ok(Bytes::from(data))  // 零拷贝共享
}
```

### 7.3 Arc 与引用计数

```rust
// 共享只读数据使用 Arc
let metadata = Arc::new(file_meta);
let engine_scheduler = PieceScheduler::new(metadata.clone());
let engine_seeker = SeekEngine::new(metadata.clone());

// 共享可变数据使用 Arc<Mutex<T>> 或 Arc<RwLock<T>>
let buffer = Arc::new(RwLock::new(RingBuffer::new(64)));
let buffer_for_engine = buffer.clone();
let buffer_for_server = buffer.clone();

// 对于写少读多的场景, 使用 RwLock
let state = buffer.read().expect("lock poisoned");
let speed = state.buffered_duration();

// 对于频繁写入的场景, 使用 Mutex
let mut scheduler = self.scheduler.lock().expect("lock poisoned");
scheduler.set_seek_target(piece_index);
```

### 7.4 内存池

```rust
/// 为 UDP 包预分配缓冲区池
pub struct PacketPool {
    pool: Vec<Vec<u8>>,
    packet_size: usize,
}

impl PacketPool {
    pub fn new(pool_size: usize, packet_size: usize) -> Self {
        let mut pool = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            pool.push(vec![0u8; packet_size]);
        }
        Self { pool, packet_size }
    }

    pub fn acquire(&mut self) -> Option<Vec<u8>> {
        self.pool.pop()
    }

    pub fn release(&mut self, mut buf: Vec<u8>) {
        buf.clear();
        if self.pool.len() < self.pool.capacity() {
            self.pool.push(buf);
        }
    }
}

// 在 UDP 接收循环中使用
loop {
    let mut buf = packet_pool.acquire().unwrap_or_else(|| vec![0u8; 1500]);
    let (len, addr) = socket.recv_from(&mut buf).await?;
    buf.truncate(len);
    process_packet(buf, addr);
    // buf 在 process_packet 结束后自动 drop 或被回收
}
```

### 7.5 性能关键路径

```rust
// hot path: piece 数据处理
// - 避免在 hot path 使用 format! 或 String 拼接
// - 避免在 hot path 使用 Deref trait (如 Box, Arc 解引用)
// - 使用 `#[inline]` 标注频繁调用的小函数
// - 使用 const 或 static 避免重复初始化

#[inline]
pub fn is_interesting(&self, peer_bitfield: &Bitfield) -> bool {
    // 检查 peer 是否有我们没有的 piece
    self.bitfield.count() < peer_bitfield.count()
        && self.bitfield.0.iter().zip(peer_bitfield.0.iter())
            .any(|(a, b)| !a & b)
}

// 使用 const 而不是 static
pub const PIECE_LENGTH: u64 = 262144;

// 对性能敏感的循环避免使用 iterator 的 collect
// 不好的:
let data: Vec<u8> = (0..length).map(|i| buffer[offset + i]).collect();

// 好的:
let data = buffer[offset..offset + length].to_vec();

// 避免不必要的 clone
// 不好的:
let metadata = self.metadata.clone()?;

// 好的:
let metadata = self.metadata.as_ref().ok_or(QvodError::MetadataParse)?;
```

## 8. 测试要求

### 8.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // 测试函数命名: test_{模块}_{场景}_{预期结果}
    #[test]
    fn test_bitfield_set_and_check() {
        let mut bf = Bitfield::new(100);
        assert!(!bf.has(50));

        bf.set(50, true);
        assert!(bf.has(50));
        assert_eq!(bf.count(), 1);

        bf.set(50, false);
        assert!(!bf.has(50));
    }

    // 协议编解码必须做往返测试
    #[test]
    fn test_bencode_roundtrip_integer() {
        let original = BencodeValue::Int(42);
        let encoded = original.encode();
        let (decoded, _rest) = BencodeValue::decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_handshake_encode_decode() {
        let orig = Handshake {
            pstrlen: 19,
            pstr: *b"Qvod P2SP Protocol",
            reserved: [0u8; 8],
            info_hash: [0xAB; 20],
            peer_id: [0xCD; 20],
        };
        let bytes = orig.encode();
        assert_eq!(bytes.len(), 68);
        let decoded = Handshake::decode(&bytes).unwrap();
        assert_eq!(orig.pstrlen, decoded.pstrlen);
        assert_eq!(orig.info_hash, decoded.info_hash);
        assert_eq!(orig.peer_id, decoded.peer_id);
    }
}
```

### 8.2 异步测试

```rust
#[tokio::test]
async fn test_tracker_announce() {
    // 使用 mock server
    let mut mock_server = mockito::Server::new_async().await;

    // 设置 mock 响应
    let mock = mock_server
        .mock("GET", "/announce")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body(bencode_response())
        .create();

    let client = TrackerClient::new(vec![mock_server.url()]);
    let peers = client
        .announce(&InfoHash::from_hex("AA".repeat(20).as_str()).unwrap(),
            AnnounceEvent::Started, 8621)
        .await
        .unwrap();

    assert!(!peers.is_empty());
    mock.assert();
}

#[tokio::test]
async fn test_dht_routing_table_operations() {
    let local_id = NodeId::random();
    let mut table = RoutingTable::new(local_id);

    // 插入 10 个节点
    for i in 0..10u8 {
        let node_id = NodeId::from([i; 20]);
        let entry = KBucketEntry {
            node_id,
            addr: "127.0.0.1:9000".parse().unwrap(),
            last_seen: Instant::now(),
            latency: Duration::from_millis(10),
            is_firewalled: false,
        };
        table.insert(entry);
    }

    // 查询最近节点
    let target = NodeId::from([5; 20]);
    let closest = table.find_closest(&target, 8);
    assert!(closest.len() <= 8);
}
```

### 8.3 集成测试

```rust
// tests/integration/p2p_transfer.rs

#[tokio::test]
async fn test_two_peer_transfer() {
    // 启动两个本地节点
    let peer_a = TestPeer::new("127.0.0.1:0").await;
    let peer_b = TestPeer::new("127.0.0.1:0").await;

    // peer_a 有完整数据, peer_b 为空
    let data = vec![0xAB; 262144];  // 1 piece
    peer_a.add_data(data.clone());

    // peer_b 从 peer_a 请求 piece
    peer_b.request_piece(peer_a.addr(), 0).await.unwrap();

    // 验证 peer_b 的数据
    assert_eq!(peer_b.get_piece(0), Some(&data[..]));
}

// tests/integration/full_stream.rs

#[tokio::test]
async fn test_full_playback_flow() {
    // 启动 mock Tracker
    let tracker = MockTracker::start().await;

    // 启动 DHT (本地回环模式)
    let dht = DhtNode::new(DhtConfig::local_test()).await;

    // 创建 Engine
    let engine = QvodEngine::new(EngineConfig {
        tracker_urls: vec![tracker.url()],
        dht_seed_nodes: vec![],
        http_fallback: false,
        ..Default::default()
    });

    // 使用测试 URI
    let uri = QvodUri::from_str("qvod://AA...|test.bin|1048576|mp4|").unwrap();
    let stream = engine.play(&uri).await.unwrap();

    // 读取前 64KB 数据
    let mut buf = vec![0u8; 65536];
    stream.read(&mut buf).await.unwrap();
    assert!(!buf.iter().all(|&b| b == 0));
}
```

### 8.4 Mock 策略

```rust
// 网络模块使用 mock 测试，不依赖真实网络环境
// 创建 Trait 的 mock 实现

pub struct MockDhtEngine {
    peers: Vec<PeerInfo>,
    local_id: NodeId,
}

impl DhtEngine for MockDhtEngine {
    async fn bootstrap(&self, _seed_nodes: &[SocketAddr]) -> Result<()> {
        Ok(())
    }

    async fn find_peers(&self, _info_hash: &InfoHash) -> Result<mpsc::Receiver<PeerInfo>> {
        let (tx, rx) = mpsc::channel(10);
        for peer in &self.peers {
            tx.send(peer.clone()).await.ok();
        }
        Ok(rx)
    }

    async fn announce(&self, _info_hash: &InfoHash, _port: u16) -> Result<()> {
        Ok(())
    }

    fn local_id(&self) -> &NodeId {
        &self.local_id
    }

    fn stats(&self) -> DhtStats {
        DhtStats::default()
    }
}

// 在测试中使用 mock
#[tokio::test]
async fn test_engine_with_mock_dht() {
    let dht = Arc::new(MockDhtEngine {
        peers: vec![
            PeerInfo {
                peer_id: [1u8; 20],
                addr: "127.0.0.1:8621".parse().unwrap(),
                is_firewalled: false,
                bw_up: 1024,
                bw_down: 2048,
                location: None,
            },
        ],
        local_id: NodeId::random(),
    });

    let engine = QvodEngine::with_dht(dht);
    // 继续测试...
}
```

## 9. 日志规范

### 9.1 日志级别

```rust
use tracing::{error, warn, info, debug, trace};

// ERROR: 不可恢复的错误，需要人工干预
error!(%info_hash, "所有 peer 都不可用");

// WARN: 可恢复的异常，系统自动降级
warn!(%peer_id, "peer 连接超时，重试其他节点");

// INFO: 重要的生命周期事件
info!("QvodEngine 启动完成，监听端口: {}", port);

// DEBUG: 调试信息，开发时有用
debug!(%info_hash, "tracker announce 返回 {} 个 peer", count);

// TRACE: 详细内部逻辑，仅在深度调试时使用
trace!(piece_index, "调度器产出一个 Critical 请求");
```

### 9.2 结构化日志

```rust
// 使用结构化字段，便于日志分析
info!(
    info_hash = %hash,
    peers = %count,
    speed_kbps = %speed,
    "下载状态更新"
);

// 记录持续时间
let start = Instant::now();
// ... 操作 ...
info!(
    elapsed_ms = %start.elapsed().as_millis(),
    "元数据获取完成"
);

// 不要记录敏感数据 (如 peer_id 完整值)
// 好的: info!(peer_id_short = %&peer_id[..8], "peer 连接成功");
// 不好: info!("peer_id: {:?}", peer_id);
```

## 10. 跨平台注意事项

### 10.1 平台差异

```rust
// 路径处理: 使用 PathBuf 而不是字符串拼接
// 好的:
let cache_path = cache_dir.join("qdata").join(format!("{}.qdata", info_hash.to_hex()));

// 不好的:
let cache_path = format!("{}/qdata/{}.qdata", cache_dir, info_hash.to_hex());

// 网络: 使用 SocketAddr 处理 IPv4/IPv6
// 好的:
let addr: SocketAddr = format!("[::1]:{}", port).parse()?;

// 文件锁: 不同平台使用不同机制
#[cfg(unix)]
fn lock_file(file: &File) -> Result<()> {
    // 使用 flock
}

#[cfg(windows)]
fn lock_file(file: &File) -> Result<()> {
    // 使用 LockFileEx
}
```

### 10.2 条件编译

```rust
// 平台特定功能
#[cfg(target_os = "linux")]
pub fn get_default_cache_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".local/share/qvs")
}

#[cfg(target_os = "macos")]
pub fn get_default_cache_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Caches/com.qvs.player")
}

#[cfg(target_os = "windows")]
pub fn get_default_cache_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("QVS\\Cache")
}
```

### 10.3 原生 API 绑定

```rust
// 仅在目标平台可用时编译
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_nat {
    pub fn upnp_map_port(port: u16) -> Result<()> {
        // 使用 igd-next crate
    }
}

#[cfg(target_os = "windows")]
mod windows_nat {
    pub fn upnp_map_port(port: u16) -> Result<()> {
        // 使用 COM 接口
    }
}
```

## 11. 代码审查检查清单

在将 crate 标记为完成前，必须逐项检查：

### 11.1 API 设计
- [ ] 所有公共 API 有文档注释 (`///`)
- [ ] 函数签名是否合理（参数类型、返回类型）
- [ ] 是否过度暴露内部实现（pub 是否必要）
- [ ] API 是否一致（命名、参数顺序）
- [ ] 是否存在不必要的泛型抽象

### 11.2 错误处理
- [ ] 所有 Result 返回 QvodError 类型
- [ ] 没有 unwrap() 或 expect()（测试代码除外）
- [ ] 错误消息对用户友好且有上下文
- [ ] 降级路径已考虑

### 11.3 安全性
- [ ] 无 unsafe 代码（除非批准）
- [ ] 所有网络输入经过验证
- [ ] 缓冲区没有溢出风险
- [ ] 路径遍历防护

### 11.4 性能
- [ ] 无明显的重复分配
- [ ] hot path 无日志输出
- [ ] 大缓冲区预分配
- [ ] 锁范围最小化

### 11.5 测试
- [ ] 单元测试覆盖率 > 80%
- [ ] 协议编解码有往返测试
- [ ] 网络模块使用 mock 测试
- [ ] 边界条件覆盖（空、满、超时）

### 11.6 代码质量
- [ ] clippy 无警告 (`cargo clippy -- -D warnings`)
- [ ] cargo fmt 通过 (`cargo fmt --check`)
- [ ] 无硬编码常量（使用 constants.rs）
- [ ] 日志覆盖关键路径
- [ ] 模块拆分合理，单个文件 < 800 行
- [ ] 无循环依赖

## 12. 常用代码模板

### 12.1 Cargo.toml 模板

```toml
[package]
name = "qvs-transport"
version.workspace = true
edition.workspace = true

[lints]
workspace = true

[dependencies]
qvs-core = { path = "../qvs-core" }
qvs-format = { path = "../qvs-format" }
tokio = { workspace = true, features = ["net", "io-util", "time", "sync"] }
bytes = "1"
sha1 = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["rt", "macros"] }
mockito = "1"
criterion = { version = "0.5", features = ["async_futures"] }
```

### 12.2 lib.rs 模板

```rust
//! # qvs-transport
//!
//! P2SP 传输层实现，提供 TCP 和 UDP 混合传输能力。
//! 关键帧使用 TCP 保证可靠性，非关键帧使用 UDP 提高效率。

mod handshake;
mod message;
mod tcp_stream;
mod udp_stream;
mod congestion;
mod pool;
mod scheduler;
mod p2sp;
mod peer_wire;
mod nat;
mod stats;

pub use handshake::*;
pub use message::*;
pub use tcp_stream::*;
pub use udp_stream::*;
pub use congestion::*;
pub use pool::*;
pub use scheduler::*;
pub use p2sp::*;
pub use peer_wire::*;
pub use nat::*;
pub use stats::*;

use tracing::info;

/// 初始化传输层，启动后台任务
pub fn init() {
    info!("qvs-transport initialized");
}
```

### 12.3 主事件循环模板

```rust
/// Engine 主事件循环
pub async fn run(self: Arc<Self>) {
    // 定时器
    let mut tracker_timer = tokio::time::interval(Duration::from_secs(30));
    let mut dht_timer = tokio::time::interval(Duration::from_secs(900));
    let mut maintain_timer = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            // 网络事件
            event = self.transport.next_event() => {
                if let Err(e) = self.handle_event(event).await {
                    warn!(error = %e, "处理网络事件失败");
                }
            }
            // Tracker announce
            _ = tracker_timer.tick() => {
                if let Err(e) = self.tracker_announce().await {
                    warn!(error = %e, "tracker announce 失败");
                }
            }
            // DHT 刷新
            _ = dht_timer.tick() => {
                self.dht.refresh_routing_table();
            }
            // 连接池维护
            _ = maintain_timer.tick() => {
                self.transport.maintain_connections();
                if let Err(e) = self.cache.cleanup() {
                    warn!(error = %e, "缓存清理失败");
                }
            }
            // 用户命令
            Some(cmd) = self.cmd_rx.recv() => {
                match cmd {
                    Command::Pause => self.pause(),
                    Command::Resume => self.resume(),
                    Command::Stop(hash) => self.stop(hash),
                    Command::Seek(ts) => self.seek_to(ts).await?,
                }
            }
            // 优雅关闭
            _ = self.shutdown_rx.changed() => {
                info!("Engine 收到关闭信号，开始清理");
                break;
            }
        }
    }
    info!("Engine 主循环退出");
}
```

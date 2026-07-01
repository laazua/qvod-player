# QVOD 系统架构设计

## 五层架构总览

```
 ┌─────────────────────────────────────────────────────────────────────┐
 │  Layer 5: 应用层 (qvs-gui + qvs-media)                              │
 │  ┌────────────────────────┐  ┌──────────────────────────────────┐   │
 │  │ GUI 播放器             │  │ 媒体解码                        │   │
 │  │ - egui 窗口            │  │ - ffmpeg-next 解复用             │   │
 │  │ - 播放控制             │  │ - H.264/RV40/AAC/COOK 解码      │   │
 │  │ - 播放列表/历史        │  │ - OpenGL 渲染                   │   │
 │  │ - 网络状态面板         │  │ - 音视频同步                    │   │
 │  └──────────┬─────────────┘  └────────────┬─────────────────────┘   │
 ├─────────────┼─────────────────────────────┼─────────────────────────┤
 │  Layer 4: 流媒体引擎 (qvs-stream)                                    │
 │  ┌────────────────────────────────────────────────────────────────┐  │
 │  │ QvodEngine                                                     │  │
 │  │                                                                  │  │
 │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │  │
 │  │  │RingBuffer │  │Scheduler │  │SeekEngine│  │HlsAdapter      │  │  │
 │  │  │ 64MB环形  │  │ 优先级   │  │ I帧跳转   │  │ M3U8 + TS      │  │  │
 │  │  │ 水位自适应│  │ 稀有度   │  │ 即时定位  │  │ 移动端适配     │  │  │
 │  │  └─────┬─────┘  └────┬─────┘  └────┬─────┘  └───────┬────────┘  │  │
 │  │        │              │             │                │           │  │
 │  │  ┌─────┴──────────────┴─────────────┴────────────────┴──────┐   │  │
 │  │  │                 AdaptiveBuffer                            │   │  │
 │  │  │        自适应策略: 暂停/限速/正常/增加HTTP                 │   │  │
 │  │  └───────────────────────────────────────────────────────────┘   │  │
 │  └────────────────────────────────────────────────────────────────┘  │
 ├─────────────────────────────────────────────────────────────────────┤
 │  Layer 3: P2SP 传输层 (qvs-transport)                                │
 │  ┌────────────────────────────────────────────────────────────────┐  │
 │  │                   P2spDownloader                                │  │
 │  │  ┌───────────────────┐  ┌────────────────────────────────┐     │  │
 │  │  │    TCP Channel    │  │        UDP Channel             │     │  │
 │  │  │   (I帧/关键数据)  │  │    (P帧/B帧/非关键数据)        │     │  │
 │  │  ├───────────────────┤  ├────────────────────────────────┤     │  │
 │  │  │ • 握手 68字节     │  │ • 数据/ACK/NACK 消息          │     │  │
 │  │  │ • Peer Wire 协议  │  │ • 自定义拥塞控制 (Reno+)      │     │  │
 │  │  │ • 可靠传输        │  │ • 最大 1400 字节/包           │     │  │
 │  │  └────────┬──────────┘  └────────────┬───────────────────┘     │  │
 │  │           │                          │                          │  │
 │  │  ┌────────┴──────────────────────────┴──────────────────────┐  │  │
 │  │  │                 ConnectionPool (最大 50)                  │  │  │
 │  │  │     连接管理 / 保活 / 超时清理 / 带宽统计                  │  │  │
 │  │  └───────────────────────────────────────────────────────────┘  │  │
 │  │                                                                  │  │
 │  │  ┌──────────────────┐  ┌────────────────────────────────────┐   │  │
 │  │  │  NAT Traversal   │  │  HTTP Fallback Source              │   │  │
 │  │  │  UDP打洞 + TURN   │  │  源服务器后备下载                  │   │  │
 │  │  └──────────────────┘  └────────────────────────────────────┘   │  │
 │  └────────────────────────────────────────────────────────────────┘  │
 ├─────────────────────────────────────────────────────────────────────┤
 │  Layer 2: 覆盖网络 (qvs-dht + qvs-tracker)                           │
 │  ┌──────────────────────────┐  ┌────────────────────────────────┐    │
 │  │      DHT Kademlia        │  │      HTTP Tracker              │   │
 │  │  ┌────────────────────┐  │  │  ┌────────────────────────┐    │   │
 │  │  │ Routing Table      │  │  │  │ announce (Started/     │    │   │
 │  │  │ 160 k-bucket × K=8 │  │  │  │  Completed/Stopped)    │    │   │
 │  │  │ XOR 距离维护       │  │  │  │ scrape (swarm 状态)    │    │   │
 │  │  └────────┬───────────┘  │  │  └───────────┬────────────┘    │   │
 │  │           │               │  │               │                │   │
 │  │  ┌────────┴───────────┐  │  │  ┌────────────┴──────────┐     │   │
 │  │  │ UDP RPC            │  │  │  │ Bencode 响应解析       │     │   │
 │  │  │ FIND_NODE          │  │  │  │ compact peer 解码      │     │   │
 │  │  │ FIND_PEERS         │  │  │  │ 多 tracker 负载均衡    │     │   │
 │  │  │ ANNOUNCE           │  │  │  └───────────────────────┘     │   │
 │  │  └────────┬───────────┘  │  └────────────────────────────────┘   │
 │  │           │               │                                       │
 │  │  ┌────────┴───────────┐  │                                       │
 │  │  │ Bootstrap          │  │                                       │
 │  │  │ 种子节点 → 迭代填充 │  │                                       │
 │  │  │ 每900s 刷新空闲bucket│  │                                       │
 │  │  └────────────────────┘  │                                       │
 │  └──────────────────────────┘                                       │
 ├─────────────────────────────────────────────────────────────────────┤
 │  Layer 1: 本地 HTTP 网关 (qvs-local-server)                          │
 │                                                                      │
 │  ┌──────────────────────────────────────────────────────────────┐   │
 │  │               qvs-local-server (axum)                        │   │
 │  │  端口: 8621 (自动回退)                                        │   │
 │  │                                                              │   │
 │  │  GET /play?hash=&name=&size=    → HTTP Chunked 流            │   │
 │  │  GET /play?hash=&offset=        → HTTP 206 Partial           │   │
 │  │  GET /status?hash=              → JSON 状态                  │   │
 │  │  GET /segment?offset=&length=   → HLS 伪分片                 │   │
 │  │  POST /control?action=          → 控制指令                   │   │
 │  │                                                              │   │
 │  │  ┌─────────┐ ┌──────────┐ ┌────────┐ ┌─────────────┐       │   │
 │  │  │ 路由分发 │ │ Range处理 │ │CORS/日志│ │ 速率限制     │       │   │
 │  │  └────┬────┘ └────┬─────┘ └───┬────┘ └──────┬──────┘       │   │
 │  │       │            │           │              │              │   │
 │  │  ┌────┴────────────┴───────────┴──────────────┴──────┐      │   │
 │  │  │           tokio::sync::mpsc 流式通道              │      │   │
 │  │  │        QvodEngine ←→ HTTP Response Body           │      │   │
 │  │  └───────────────────────────────────────────────────┘      │   │
 │  └──────────────────────────────────────────────────────────────┘   │
 │                                                                      │
 │  浏览器 → qvod:// 链接 → OS协议处理器 → localhost:8621/play → 播放   │
 └─────────────────────────────────────────────────────────────────────┘
```

## Layer 1: 本地 HTTP 网关 (qvs-local-server)

### 职责

Layer 1 是用户可见的最外层接口。快播安装后在本机启动一个 HTTP Server（端口 8621，被占用时自动回退），作为浏览器与 P2P 引擎之间的桥梁。当用户点击 `qvod://` 链接时，浏览器将请求交给本地 HTTP Server，由它调度 P2P 引擎下载数据，再通过 HTTP Chunked Transfer 实时推送给播放器。

### 架构决策

| 决策 | 选择 | 理由 |
|------|------|------|
| HTTP 框架 | axum | 异步、类型安全、生态完善 |
| 流式传输 | tokio::sync::mpsc | 背压支持、通道缓冲防止内存溢出 |
| Range 处理 | 原生解析 | 支持标准 HTTP Range 头，兼容所有播放器 |
| 端口选择 | 自动回退 | 8621 → +1 递增，最多重试 10 次 |

### 数据流: 播放请求

```
浏览器: 用户点击 qvod://A1B2...|movie.mp4|734003200|rmvb|
    │
    ▼
OS: qvod:// 协议处理器 → 启动 qvs 可执行文件
    │
    ▼
qvs 可执行文件 → 解析 URI → 调用 LocalServer API:
    GET http://localhost:8621/play?hash=A1B2C3...&name=movie.mp4&size=734003200
    │
    ▼
handler.rs: 解析查询参数
    ├── info_hash: [u8; 20]  (40 字符 hex → 20 字节)
    ├── filename: "movie.mp4"
    └── filesize: 734003200
    │
    ▼
handler.rs: 调用 QvodEngine::play(info_hash)
    │
    ▼
QvodEngine: 异步启动 P2P 下载流程
    │
    ▼
stream.rs: 创建 mpsc::channel::<Vec<u8>>(64)  (64 个 slot 的背压缓冲)
    │
    ▼
handler.rs: 从 mpsc::Receiver 读取数据块
    │  loop:
    │    recv().await → Some(data) → 写入 HTTP Response Body (chunked)
    │    recv().await → None       → 关闭连接 (流结束)
    │
    ▼
浏览器播放器: 接收 HTTP Chunked 流 → 开始播放
```

### Layer 1 错误处理

| 场景 | 行为 |
|------|------|
| 端口被占用 | 尝试 8622, 8623 ... 最多 10 次 |
| 所有端口被占用 | 返回错误，提示用户关闭冲突程序 |
| URI 参数缺失 | 返回 400 Bad Request |
| 资源不存在 | 返回 404 Not Found |
| P2P 引擎未就绪 | 返回 503 Service Unavailable |
| 请求超时 | 返回 408 Request Timeout |
| 速率超过限制 | 返回 429 Too Many Requests |

---

## Layer 2: 覆盖网络 (qvs-dht + qvs-tracker)

### 职责

维护网络中所有活跃节点的地址列表，为客户端提供最优播放节点。快播采用中心 Tracker + DHT 辅助的混合方案。两者并行工作，互为备份。

### 2.1 HTTP Tracker (qvs-tracker)

Tracker 是中心化的 HTTP 服务，维护每个 `info_hash` 对应的 peer 列表。客户端启动时向 Tracker 注册，播放时从 Tracker 获取节点列表。

#### 协议交互

```
客户端                                Tracker 服务器
  │                                       │
  │── GET /announce?info_hash=...       ──│
  │   &peer_id=...                       │
  │   &port=8621                         │
  │   &uploaded=0                        │
  │   &downloaded=0                      │
  │   &left=734003200                    │
  │   &event=started                     │
  │   &compact=1                         │
  │                                       │
  │── Bencode 响应:                     ──│
  │   {                                  │
  │     "interval": 1800,               │
  │     "complete": 42,                  │
  │     "incomplete": 17,                │
  │     "peers": <compact 6-byte entries>│
  │   }                                  │
  │                                       │
  │   解析 peer 列表, 排序, 连接          │
  │                                       │
  │── [每 30 分钟]                       │
  │   GET /announce?event=empty         ──│
```

#### 多 Tracker 策略

1. 在 `EngineConfig` 中配置多个 Tracker URL（优先级有序）
2. 每次 announce 时，优先使用最近成功的 Tracker
3. 如果超时或失败，按优先级切换到下一个 Tracker
4. 所有 Tracker 都失败时，完全依赖 DHT
5. 后台每 5 分钟尝试一个失败的 Tracker，看是否恢复

### 2.2 DHT Kademlia (qvs-dht)

DHT 实现用于在 Tracker 不可用时辅助节点发现，并降低对中心服务器的依赖。

#### Kademlia 参数

| 参数 | 值 | 说明 |
|------|-----|------|
| 地址空间 | 160 bit | SHA-1 哈希空间 |
| K (bucket 容量) | 8 | 每个 k-bucket 最多 8 个 entry |
| α (并行度) | 3 | α 个节点同时查询 |
| Refresh 间隔 | 900 秒 | 空闲 bucket 刷新 |
| Peer 过期 | 1800 秒 | 超时从未响应的节点 |
| 最大包大小 | 1400 字节 | 避免 IP 分片 |
| 秘密轮换间隔 | 600 秒 | announce token 安全 |

#### 路由表结构

```
RoutingTable
├── local_id: [u8; 20]  (随机生成)
└── buckets[0..160]
    └── 每个是 KBucket { entries: VecDeque<KBucketEntry> }
        └── 每个 KBucketEntry { node_id, addr, last_seen, latency, is_firewalled }

分裂规则:
  1. 插入新节点时，计算其 bucket 索引 = 160 - leading_zeros(xor(local_id, node_id))
  2. 如果 bucket 未满 (entries < K): 直接插入
  3. 如果 bucket 已满:
     a. 检查 entries 中是否有过期节点 (last_seen > 15min)
     b. 有 → 替换最旧的过期节点
     c. 没有 → ping 最久未联系的节点:
          - 有响应 → 丢弃新节点
          - 无响应 → 移除该节点, 插入新节点
  4. 如果 bucket 索引 < 160 且本 bucket 需要分裂:
     a. 将 bucket 分裂为两个
     b. 重新分配 entries

刷新策略:
  1. 每隔 REFRESH_INTERVAL (900s) 检查所有 bucket
  2. 如果 bucket 的 last_refreshed > REFRESH_INTERVAL:
     - 从该 bucket 中选一个随机 ID
     - 对距离最近的节点发送 FIND_NODE
  3. 刚收到查询的 bucket 重置 last_refreshed
```

#### DHT RPC 消息格式

```
所有消息以 UDP 发送，单包最大 1400 字节。

通用消息头 (8 字节):
  偏移  长度  字段       说明
  0     4    magic      固定 [0x51, 0x56, 0x44, 0x54] ("QVDT")
  4     1    msg_type   0x00=PING, 0x01=FIND_NODE, 0x02=FIND_PEERS, 0x03=ANNOUNCE
  5     2    txn_id     事务 ID (big-endian)，用于匹配请求和响应
  7     1    ver        协议版本 (当前为 0x01)

PING:
  请求: header + node_id(20)
  响应: header + node_id(20)

FIND_NODE:
  请求: header + node_id(20) + target(20)
  响应: header + node_id(20) + nodes(n*26)
  其中 nodes 为: [node_id(20) + ip(4) + port(2)] * n

FIND_PEERS:
  请求: header + node_id(20) + info_hash(20)
  响应 (有 peers):
    header + node_id(20) + 0x00(peers_tag) + peer_count(u16) + peers(n*6)
    其中 peers 为: [ip(4) + port(2)] * n
  响应 (无 peers):
    header + node_id(20) + 0x01(nodes_tag) + node_count(u16) + nodes(n*26)

ANNOUNCE:
  请求: header + node_id(20) + info_hash(20) + token(4) + port(2)
  响应: header + node_id(20) + 0x00(ok_tag)
```

#### Token 管理

```rust
pub struct TokenManager {
    secret_a: [u8; 16],   // 当前秘密
    secret_b: [u8; 16],   // 上一个秘密 (5 分钟窗口)
    last_rotation: Instant,
}

impl TokenManager {
    /// 为指定地址生成 token
    /// token = SHA-1(ip + ":" + port + ":" + secret)[0..4]
    pub fn generate_token(&self, addr: &SocketAddr) -> [u8; 4] {
        let mut hasher = Sha1::new();
        hasher.update(addr.ip().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(addr.port().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(&self.secret_a);
        let result = hasher.finalize();
        let mut token = [0u8; 4];
        token.copy_from_slice(&result[..4]);
        token
    }

    /// 验证 token，同时检查当前秘密和上一个秘密
    pub fn verify_token(&self, addr: &SocketAddr, token: &[u8; 4]) -> bool {
        let current = self.generate_token(addr);
        if &current == token { return true; }
        // 检查上一个秘密 (用于时钟偏移)
        let previous = self.generate_with_secret(addr, &self.secret_b);
        &previous == token
    }

    /// 每 10 分钟轮换秘密
    pub fn rotate_secret(&mut self) {
        self.secret_b = self.secret_a;
        self.secret_a = rand::random::<[u8; 16]>();
        self.last_rotation = Instant::now();
    }
}
```

### 2.3 节点评分与选择

```rust
pub struct NodeScorer;

impl NodeScorer {
    /// 综合评分，分值越高越优先连接
    pub fn score(peer: &PeerInfo, local: &NodeContext) -> f64 {
        // 带宽评分 (归一化到 0-1)
        let bw = (peer.bw_up.min(peer.bw_down) as f64).max(1.0);
        let bw_score = (bw / 1024.0).min(1.0);

        // 延迟惩罚
        let latency_penalty = match peer.latency {
            l if l > Duration::from_millis(1000) => 0.3,
            l if l > Duration::from_millis(500) => 0.5,
            l if l > Duration::from_millis(200) => 0.8,
            _ => 1.0,
        };

        // 地理亲和性奖励 (同城市/地区)
        let geo_bonus = if peer.location == local.location {
            1.2
        } else if peer.location.as_ref().map_or(false, |pl| {
            local.location.as_ref().map_or(false, |ll| pl[..2] == ll[..2])
        }) {
            1.1  // 同一国家
        } else {
            1.0
        };

        // 防火墙惩罚
        let firewall_penalty = if peer.is_firewalled { 0.3 } else { 1.0 };

        // 校验和被惩罚 (之前从该 peer 收到过错误数据)
        let integrity_penalty = if local.failed_peers.contains(&peer.peer_id) {
            0.1
        } else {
            1.0
        };

        bw_score * latency_penalty * geo_bonus * firewall_penalty * integrity_penalty
    }
}
```

---

## Layer 3: P2SP 传输层 (qvs-transport)

### 职责

Layer 3 是快播的核心创新层。负责节点间实际的数据传输，同时从 P2P 节点和 HTTP 源服务器获取数据。关键帧用 TCP 保证可靠性，非关键帧用 UDP 提高效率。

### 3.1 P2SP 混合下载策略

```
Piece 优先级 → 选择下载策略

                   Piece Priority
                        │
          ┌─────────────┼──────────────┐
          │             │              │
      Critical        High         Normal/Low
          │             │              │
    ┌─────┴─────┐  ┌───┴───┐      ┌───┴───┐
    │           │  │       │      │       │
   P2P + HTTP  P2P优先 仅 P2P   仅 P2P
   并行下载    HTTP     (正常)   (空闲时)
   (取先完成)  3秒后备
```

### 3.2 TCP 通道详细协议

#### 握手 (68 字节)

```
Byte 0:     pstrlen = 19 (u8)
Byte 1-19:  pstr = "Qvod P2SP Protocol" (19 bytes)
Byte 20-27: reserved = 0 (8 bytes)
  Bit 20 (reserved[5] & 0x10): ut_metadata 支持
  Bit 21 (reserved[5] & 0x20): DHT 支持
  Bit 22 (reserved[5] & 0x40): FAST 扩展
Byte 28-47: info_hash (20 bytes)
Byte 48-67: peer_id (20 bytes)
```

#### Peer Wire 消息

```
keep-alive:  <len=0x00000000>
             消息头 4 字节，无消息 ID 和负载

标准消息:  <len=0x00000001+payload_len> <id=1 byte> <payload>
             例如 request 消息: len=13, id=0x06, payload=12 bytes

消息类型:
  0x00: choke               (无 payload)
  0x01: unchoke             (无 payload)
  0x02: interested          (无 payload)
  0x03: not_interested      (无 payload)
  0x04: have                (payload: piece_index u32 BE)
  0x05: bitfield            (payload: bitfield bytes)
  0x06: request             (payload: index u32, begin u32, length u32)  = 12 bytes
  0x07: piece               (payload: index u32, begin u32, block data) = 8 + block_len
  0x08: cancel              (payload: index u32, begin u32, length u32)  = 12 bytes
  0x09: port                (payload: dht_port u16 BE)
  0x0A: suggest_piece       (payload: piece_index u32 BE)              [扩展]
  0x0B: reject_request      (payload: index u32, begin u32, length u32) [扩展]
  0x0C: have_all            (无 payload)                                [扩展]
  0x0D: have_none           (无 payload)                                [扩展]
  0x0E: extended           (payload: ext_msg_id u8, ...)               [扩展]
```

#### 扩展协议 ut_metadata

```
extended 消息的 payload:
  - ext_msg_id: u8 (0x00 = handshake, 0x01 = ut_metadata)

extended handshake:
  {
    "m": {"ut_metadata": 3},
    "metadata_size": 12345,
    "p": {"q": "QVOD"}
  }

ut_metadata request:
  {
    "msg_type": 0,   // request
    "piece": 0       // metadata piece index
  }

ut_metadata response:
  {
    "msg_type": 1,   // response
    "piece": 0,      // metadata piece index
    "total_size": 12345
  }
  payload: binary metadata data

ut_metadata reject:
  {
    "msg_type": 2,   // reject
    "piece": 0
  }
```

### 3.3 UDP 通道详细协议

#### 包格式

```
Byte 0:     msg_type (u8)
  └─ 0x01: DATA
  └─ 0x02: ACK
  └─ 0x03: NACK
  └─ 0x04: PING
  └─ 0x05: PONG
Byte 1-4:   sequence_number (u32 BE)
Byte 5-8:   piece_index (u32 BE)
Byte 9-12:  block_offset (u32 BE)
Byte 13-14: data_length (u16 BE)   [DATA 消息有效]
Byte 15+:   payload (DATA 消息: block 数据, ACK/NACK: 空或 ack_bitmask)

最大包大小: 1400 字节 (避免 IP 分片)
其中头部 15 字节，最大 payload 1385 字节
```

#### 拥塞控制算法

```rust
pub struct UdpCongestionControl {
    // 状态
    state: CongestionState,    // SlowStart | CongestionAvoidance | FastRecovery
    cwnd: u32,                 // 拥塞窗口 (以包为单位)
    ssthresh: u32,             // 慢启动阈值
    // 统计
    rtt_estimate: Duration,    // 平滑 RTT 估计
    rtt_deviation: Duration,   // RTT 偏差
    loss_rate: f64,            // 滑动窗口丢包率 (最近 100 个包)
    // 发送跟踪
    packets_in_flight: u32,    // 已发送未确认的包数
    last_ack_seq: u32,         // 最后收到的连续 ACK 序列号
    duplicate_acks: u32,       // 重复 ACK 计数
    // 超时
    rto: Duration,             // 重传超时
    // 流媒体优化
    consecutive_losses: u32,   // 连续丢包计数
    mode: TransportMode,       // Normal | TcpOnly (loss > 10% 时切换)
}

impl UdpCongestionControl {
    // TCP Reno 拥塞控制 + 流媒体优化

    // 收到新 ACK 时调用
    pub fn on_ack(&mut self, seq: u32, rtt: Duration) {
        // 更新 RTT 估计 (RFC 6298)
        self.rtt_deviation = self.rtt_deviation * 3 / 4
            + (self.rtt_estimate - rtt).abs() / 4;
        self.rtt_estimate = self.rtt_estimate * 7 / 8 + rtt * 7 / 8;
        self.rto = self.rtt_estimate + 4 * self.rtt_deviation;

        match self.state {
            SlowStart => {
                self.cwnd += 1;  // 每个 ACK 增加 1 个包
                if self.cwnd >= self.ssthresh {
                    self.state = CongestionAvoidance;
                }
            }
            CongestionAvoidance => {
                // 每个 RTT 增加 1 个包
                self.cwnd += 1 / self.cwnd;  // 更准确的实现
            }
            FastRecovery => {
                self.cwnd = self.ssthresh;
                self.state = CongestionAvoidance;
            }
        }

        // 更新 in_flight
        self.packets_in_flight = self.packets_in_flight.saturating_sub(1);
        self.consecutive_losses = 0;
        self.loss_rate *= 0.95;  // 指数衰减
    }

    // 检测到丢包时调用
    pub fn on_loss(&mut self) {
        self.consecutive_losses += 1;
        self.ssthresh = (self.cwnd / 2).max(2);
        self.cwnd = self.ssthresh;
        self.state = CongestionAvoidance;
        // 流媒体优化: 连续丢包超过阈值时降级为 TCP only
        if self.consecutive_losses >= 3 {
            self.mode = TransportMode::TcpOnly;
        }
        self.loss_rate = self.loss_rate * 0.95 + 0.05;
    }

    pub fn should_send(&self) -> bool {
        self.packets_in_flight < self.cwnd
    }

    pub fn can_use_udp(&self) -> bool {
        self.mode != TransportMode::TcpOnly && self.loss_rate < 0.1
    }
}
```

### 3.4 NAT 穿透

#### NAT 类型检测

```
1. 向 STUN 服务器发送 Binding Request
2. 比较 SocketAddr (IP:Port):
   - 请求地址 == 响应映射地址 → 无 NAT
   - 请求地址 != 响应映射地址:
     - 更换端口再次发送:
       - 映射地址端口未变 → Full Cone NAT
       - 映射地址端口改变 → 向另一服务器发送:
         - 映射地址未变 → Restricted Cone
         - 映射地址改变 → Port Restricted 或 Symmetric
3. 结果缓存: 首次检测后缓存，后续复用
```

#### UDP 打洞流程

```
Peer A (192.168.1.2:5000)          Peer B (10.0.0.2:6000)
       │                                  │
       │ 1. A 向 B 的公共地址发送 UDP 包     │
       │    (源: A:NAT_A_port, 目的: B:6000) │
       │ ─────────────────────────────────► │
       │                                  │
       │ 2. B 向 A 的公共地址发送 UDP 包     │
       │    (源: B:6000, 目的: A:NAT_A_port) │
       │ ◄───────────────────────────────── │
       │                                  │
       │ 3. 双向 UDP 通道建立               │
       │ ◄═══════════════════════════════► │
       │                                  │
       │ 如果打洞失败:                      │
       │ 4. A 连接 TURN 中继服务器           │
       │    通过中继转发数据                  │
```

---

## Layer 4: 流媒体引擎 (qvs-stream)

### 职责

Layer 4 是快播的核心协调层，负责整合所有下层服务，提供流畅的播放体验。核心组件包括：引擎主循环、环形缓冲区、关键帧调度器、随机定位、自适应缓冲、伪 HLS。

### 4.1 引擎主循环

```rust
// QvodEngine 主循环伪代码
async fn engine_loop(self: Arc<Self>) {
    loop {
        select! {
            // 网络事件
            peer_event = self.transport.next_event() => {
                self.handle_peer_event(peer_event);
            }
            // 缓冲水位检查
            _ = self.buffer_timer.tick() => {
                let cmd = self.adaptive.tick();
                self.execute_buffer_command(cmd);
            }
            // 调度器产出的请求
            request = self.scheduler.next_request() => {
                let source = self.p2sp.select_source(&request);
                self.p2sp.dispatch(request, source);
            }
            // 用户控制命令
            cmd = self.command_rx.recv() => {
                self.handle_command(cmd);
            }
            // 周期 Tracker announce
            _ = self.tracker_timer.tick() => {
                self.tracker_announce_periodic();
            }
            // DHT refresh
            _ = self.dht_refresh_timer.tick() => {
                self.dht.refresh_routing_table();
            }
            // 连接池维护
            _ = self.pool_maintain_timer.tick() => {
                self.transport.maintain_connections();
            }
        }
    }
}
```

### 4.2 环形缓冲区 (RingBuffer)

```
缓冲区布局:

┌─────────────────────────────────────────────────────────────────────┐
│  RingBuffer (64MB = 65536 KB)                                      │
│                                                                     │
│  ┌──────┬──────┬──────┬──────┬──────┬──────┬──────┬──────┬──────┐  │
│  │P0    │P1    │P2    │P3    │P4    │P5    │P6    │ ...  │PN    │  │
│  │██████│██████│░░░░░░│░░░░░░│██████│██████│██████│      │░░░░░░│  │
│  └──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┘  │
│   ↑play                 ↑write            ↑watermark_high           │
│                                                                     │
│  filled_ranges: [0..2P, 4P..7P, NP..]                              │
│  is_playable: play 位置有 >= 1 秒连续数据且包含 I 帧                │
└─────────────────────────────────────────────────────────────────────┘

水位自适应:
  speed > 1 MB/s   → high_watermark = 10 MB
  speed > 200 KB/s → high_watermark = 30 MB
  speed < 200 KB/s → high_watermark = 60 MB
  is_playable=false → 暂停播放, 全力缓冲
```

### 4.3 关键帧优先调度

```
时间轴:  |---I---|---P---|---B---|---P---|---I---|---P---|---B---|---P---|---I---|
          ↑                                                               ↑
       播放位置                                                          关键帧

关键帧索引:
  [0]  I-frame  @ 0.0s     offset=0       frame_size=25600
  [1]  P-frame  @ 0.1s     offset=25600    frame_size=12800
  [2]  B-frame  @ 0.15s    offset=38400    frame_size=6400
  [3]  P-frame  @ 0.2s     offset=44800    frame_size=12800
  [4]  I-frame  @ 5.0s     offset=204800   frame_size=25600
  ...

调度决策:
  Critical:  播放头所在 Piece, 以及包含 I 帧的 Piece
  High:      播放头往后 30 秒范围内的 Piece
  Normal:    30 秒 ~ 120 秒范围的 Piece
  Low:       已播放区域 (用于上传贡献)

稀有度优先:
  对于同一优先级的多个 piece, 优先下载拥有者最少的 (rared first)
  避免冗余: 同一 piece 最多从 2 个 peer 同时请求
```

### 4.4 自适应缓冲策略

```
AdaptiveBuffer 状态机:

                  ┌──────────────┐
                  │   Normal     │ ◄──────────── 网速 > 500KB/s && RTT < 100ms
                  └──────┬───────┘
                         │
            ┌────────────┼────────────┐
            │            │            │
            ▼            ▼            ▼
    ┌────────────┐ ┌──────────┐ ┌──────────────┐
    │PauseAndBuf  │ │Throttle  │ │IncHttpRatio  │
    │fer          │ │Upload    │ │              │
    │playable<2s  │ │buffered  │ │网速不足时    │
    │speed<100KB/s│ │>60s      │ │增加 HTTP 源  │
    └────────────┘ └──────────┘ └──────────────┘

  PauseAndBuffer:
    - 暂停播放器输出
    - 设置 scheduler 为全力缓冲模式
    - 增加 HTTP 源比例到 100%
    - 直到 playable >= 5s 才恢复播放

  ThrottleUpload:
    - 降低上传带宽限制 (减少对下载的影响)
    - 降低连接池大小 (断开低效连接)
    - 当 buffered < 45s 时恢复到 Normal

  Normal:
    - 默认策略
    - P2P 为主, HTTP 为辅

  IncreaseHttpRatio:
    - 对 High 及以上优先级改为 Parallel 模式
    - 降低 rarest-first 权重, 增加进度优先权重
```

### 4.5 伪 HLS 适配

```
伪 HLS 适配器将 P2P 下载的视频流实时包装为 Apple HLS 兼容格式。

工作流程:

  1. 播放开始时:
     - 解析 FileMeta 中的 keyframe_index
     - 以 I 帧为边界划分 segment
     - 生成 M3U8 播放列表

  2. 播放器请求:
     - GET /play?hash=... → M3U8 播放列表
     - GET /segment?offset=X&length=Y → 某段 TS 数据

  3. TS 包装:
     - 从 RingBuffer 读取 offset 位置的原始数据
     - 包装为 MPEG-TS 格式:
       * PAT (Program Association Table)
       * PMT (Program Map Table)
       * PES (Packetized Elementary Stream) for video
       * PES for audio (if available)
     - 返回给播放器

  M3U8 格式:
    #EXTM3U
    #EXT-X-VERSION:3
    #EXT-X-TARGETDURATION:10
    #EXTINF:10.000,
    /segment?offset=0
    #EXTINF:10.000,
    /segment?offset=204800
    #EXTINF:10.000,
    /segment?offset=409600
    #EXT-X-ENDLIST
```

---

## Layer 5: 应用层 (qvs-media + qvs-gui)

### 职责

Layer 5 是用户直接交互的层面。提供完整的播放器界面、媒体解码渲染、命令行控制。

### 5.1 媒体解码管道

```
.qdata 文件 / RingBuffer
    │
    ▼
Demuxer (ffmpeg-next)
    │  - 从缓存或 RingBuffer 读取原始数据
    │  - 解复用: 分离视频流和音频流
    │  - 输出: AVPacket (压缩帧)
    │
    ├──► Video Packet ──► VideoDecoder ──► AVFrame (YUV) ──► Renderer (egui/OpenGL)
    │       H.264/RV40        VAAPI/CUDA          纹理上传       屏幕输出
    │
    └──► Audio Packet ──► AudioDecoder ──► Resampler ──► Audio Renderer
            AAC/COOK          f32 PCM        swr           cpal/rodio
                                      │
                                      ▼
                               AudioVideoSync
                              基于 PTS/DTS 同步
                              AudioMaster 策略
                              误差 < 20ms 不调整
```

### 5.2 GUI 组件树

```
QvodApp (egui::App)
│
├── TopPanel
│   ├── URL 输入框 (qvod:// 或 http://)
│   ├── 播放列表按钮
│   ├── 设置按钮
│   └── 缓存管理按钮
│
├── CentralPanel
│   ├── VideoPanel (视频渲染区域)
│   │   ├── Video Texture (egui::TextureId)
│   │   ├── Buffer Progress Overlay (圆形进度)
│   │   ├── Error Overlay (红色遮罩)
│   │   └── Info Overlay (码率/分辨率/编码)
│   │
│   ├── Controls
│   │   ├── 播放/暂停按钮 (Space 快捷键)
│   │   ├── 进度条 (可拖拽, 显示缓冲范围)
│   │   ├── 时间显示 (当前/总时长)
│   │   ├── 音量滑块 + 静音按钮
│   │   └── 快进/快退 (←/→ 10秒)
│   │
│   └── StatusPanel
│       ├── 下载速度 (实时图表)
│       ├── 已连接 Peers (数量 + 列表)
│       ├── 缓冲进度 (百分比 + 进度条)
│       ├── 下载进度 (百分比)
│       └── DHT 路由表大小
│
├── SidePanel (可选)
│   ├── 播放列表
│   └── 历史记录
│
└── Settings Window (弹出)
    ├── 常规 (缓存路径/大小, 端口)
    ├── 网络 (最大连接, HTTP后备, Tracker编辑)
    ├── 外观 (主题, 语言)
    └── 关于
```

---

## 跨层数据流

### 启动播放完整流程

```
用户点击 qvod://A1B2...|movie.mp4|734003200|rmvb|
  │
  ▼
[Layer 1] LocalServer: 解析 URI
  ├── info_hash = [0xA1, 0xB2, ...] (20 bytes)
  ├── filename = "movie.mp4"
  └── filesize = 734003200
  │
  ▼
[Layer 4] QvodEngine::play(info_hash)
  │
  ├── 1. [Cross] CacheManager::find(info_hash)
  │     └── 缓存命中 → 直接返回 MediaStream → 播放
  │
  ├── 2. [Layer 2] TrackerClient::announce + DhtEngine::find_peers (并行)
  │     ├── Tracker 返回 peer 列表
  │     ├── DHT 返回 peer 列表
  │     └── 合并、去重、评分排序
  │
  ├── 3. [Layer 3] ConnectionPool::add_peer (Top 20 peers)
  │     └── 逐个建立 TCP 连接 → 握手 → 获取 bitfield
  │
  ├── 4. [Layer 3] 扩展协议获取 Metadata
  │     └── ut_metadata → FileMeta (含 keyframe_index)
  │
  ├── 5. [Layer 4] 初始化 RingBuffer (64MB) + PieceScheduler
  │     └── 计算每个 piece 的优先级 (Critical/High/Normal/Low)
  │
  ├── 6. [Layer 3] P2spDownloader 开始并行下载
  │     ├── Critical pieces: TCP + HTTP 并行 (取先完成)
  │     ├── High pieces: TCP 优先, HTTP 3 秒后备
  │     └── Normal pieces: 仅 TCP
  │
  ├── 7. [Layer 4] RingBuffer 填充 → is_playable() == true
  │     └── 通知 LocalServer 可以开始输出流
  │
  ├── 8. [Layer 1] LocalServer 返回 HTTP Response
  │     └── 浏览器播放器开始接收数据
  │
  └── 9. [Layer 4] 后台事件循环继续下载
        ├── AdaptiveBuffer 监控速度/缓冲
        ├── 定期 Tracker announce
        ├── DHT 路由表刷新
        └── 连接池维护 (保活/清理)
```

### 拖拽定位流程

```
用户拖拽进度条到 1:23:45 (timestamp_ms = 5025000)
  │
  ▼
[Layer 4] SeekEngine::seek_to(5025000)
  │
  ├── 1. keyframe_index.find_nearest_i_frame(5025000)
  │     └── 找到: I-frame @ 5023000ms, offset=251658240, frame_size=25600
  │
  ├── 2. 计算目标 Piece index = 251658240 / 262144 = 960
  │
  ├── 3. PieceScheduler::set_seek_target(960)
  │     └── Piece 960 → Critical
  │     └── Piece 961..1024 → High
  │     └── 其他 → Low
  │
  ├── 4. RingBuffer::play_cursor = 251658240
  │     └── 清空已缓冲但跳过的数据?
  │     └── 如果 offset 不在 buffer 中, 保留已缓冲区域
  │
  ├── 5. 通知 P2spDownloader 紧急下载目标位置
  │     ├── HTTP 源: 立即请求 251658240 偏移 (最快到达)
  │     └── P2P: 异步请求 Piece 960
  │
  ├── 6. 等待 piece 960 完成
  │     └── 播放器解码 I 帧 → 继续播放
  │
  └── 7. AdaptiveBuffer 检测跳转后的网速变化
        └── 如果缓冲不足 → 进入 PauseAndBuffer 模式
```

### 降级与容错路径

```
正常流程:
  Tracker(OK) + DHT(OK) + P2P(OK) + HTTP(OK)
  → 混合下载, 最大化速度

降级路径 1: Tracker 不可用
  Tracker(FAIL) + DHT(OK) + P2P(OK) + HTTP(OK)
  → 仅依赖 DHT 发现 peer
  → 每 30 秒重试 Tracker

降级路径 2: DHT 不可用
  Tracker(OK) + DHT(FAIL) + P2P(OK) + HTTP(OK)
  → 仅依赖 Tracker 发现 peer
  → 每 60 秒重试 bootstrap

降级路径 3: 两者都不可用
  Tracker(FAIL) + DHT(FAIL) + P2P(FAIL) + HTTP(OK)
  → 纯 HTTP 下载 (CDN 回源)
  → 无 P2P 加速, 但可播放

降级路径 4: 无 peer 但有 HTTP 源
  Tracker(FAIL) + DHT(FAIL) + P2P(FAIL) + HTTP(OK)
  → 纯 HTTP 流媒体播放
  → 不支持拖拽 (HTTP Range 受限于源服务器)

降级路径 5: 全部不可用
  → 返回 QvodError::NoPeers → 用户提示 "无法连接"

连接退化:
  高延迟 peer (> 1000ms RTT)       → 降低优先级
  高丢包 peer (> 10% loss)         → 断开连接
  速度不足 peer (< 10KB/s)          → 断开连接
  错误数据 (SHA-1 校验失败)         → 断开并拉黑
  TCP 连接超时                      → 标记为 firewalled, 尝试 UDP
```

---

## 组件依赖图

```
qvs-gui (播放器)
  ├── qvs-stream (流媒体引擎)
  │     ├── qvs-core (基础类型)
  │     ├── qvs-transport (P2SP 传输)
  │     │     ├── qvs-core
  │     │     └── qvs-format (Bencode, Bitfield)
  │     └── qvs-format
  │
  ├── qvs-media (媒体解码)
  │     └── qvs-core
  │
  └── qvs-core

qvs-local-server (HTTP 网关)
  └── qvs-stream

qvs-stream 内部依赖:
  qvs-stream
  ├── qvs-transport
  ├── qvs-format (URI, Bencode, Cache, Bitfield, KeyFrame)
  └── qvs-core (InfoHash, FileMeta, Error, Traits)

qvs-transport 内部依赖:
  qvs-transport
  ├── qvs-format (Bitfield)
  └── qvs-core (InfoHash, PeerInfo, Error, Traits)

qvs-dht 依赖:
  qvs-dht
  └── qvs-core (InfoHash, NodeId, Error, Constants)

qvs-tracker 依赖:
  qvs-tracker
  └── qvs-core (InfoHash, PeerInfo, Error)

qvs-format 依赖:
  qvs-format
  └── qvs-core (InfoHash, NodeId, FileMeta, Error)

qvs-core 依赖:
  qvs-core
  └── 无 (标准库 + thiserror + serde + sha1)
```

---

## 错误处理全局策略

所有模块错误统一向上传播，Engine 层统一处理重试和降级：

```rust
#[derive(Debug, thiserror::Error)]
pub enum QvodError {
    // 网络层错误
    #[error("网络错误: {0}")]
    Network(#[from] std::io::Error),

    // 协议错误
    #[error("协议错误: {0}")]
    Protocol(String),

    // DHT 错误
    #[error("DHT 超时")]
    DhtTimeout,
    #[error("DHT 路由失败: {0}")]
    DhtRoutingFailed(String),

    // Tracker 错误
    #[error("Tracker 连接超时")]
    TrackerTimeout,
    #[error("Tracker 协议错误: {0}")]
    TrackerProtocol(String),

    // 资源错误
    #[error("资源不存在: {0}")]
    ResourceNotFound(InfoHash),
    #[error("没有可用的 peer")]
    NoPeers,

    // NAT 错误
    #[error("NAT 穿透失败")]
    NatFailed,

    // 缓存错误
    #[error("缓存空间不足")]
    CacheFull,
    #[error("缓存损坏: {0}")]
    CacheCorrupted(String),

    // 媒体错误
    #[error("不支持的格式: {0}")]
    UnsupportedFormat(String),
    #[error("解码错误: {0}")]
    Decode(String),

    // 格式错误
    #[error("无效 URI: {0}")]
    InvalidUri(String),
    #[error("Bencode 错误: {0}")]
    Bencode(String),

    // 数据校验错误
    #[error("Piece 校验失败 index={index}")]
    PieceVerificationFailed { index: u32, expected: [u8; 20], got: [u8; 20] },

    // 连接错误
    #[error("达到最大连接数")]
    ConnectionLimitReached,

    // 超时/取消
    #[error("超时: {0}")]
    Timeout(String),
    #[error("操作已取消")]
    Cancelled,
}
```

### 按层错误处理矩阵

| Layer | 错误类型 | 处理方式 |
|-------|----------|----------|
| L1 (HTTP) | 参数错误 | 返回 4xx 给客户端 |
| L1 (HTTP) | 流中断 | 关闭 HTTP 连接，清理资源 |
| L2 (DHT) | 超时 | 移除节点，重试其他节点 |
| L2 (DHT) | 路由失败 | 重新 bootstrap |
| L2 (Tracker) | 超时 | 切换 Tracker，3 次后跳过 |
| L2 (Tracker) | 协议错误 | 返回空 peer 列表 |
| L3 (TCP) | 连接失败 | 标记节点不可达，尝试 UDP |
| L3 (TCP) | 握手失败 | 断开连接，拉黑该节点 |
| L3 (UDP) | 丢包 | 重传，拥塞控制调整 |
| L3 (UDP) | 拥塞 | 降级为 TCP only |
| L3 (P2SP) | Piece 校验失败 | 重新下载，标记对端不可信 |
| L3 (NAT) | 打洞失败 | 尝试中继，降级为仅传出 |
| L4 (Buffer) | 缓冲不足 | 暂停播放，增加 HTTP 源 |
| L4 (Seek) | KeyFrame 未下载 | 等待下载完成 |
| L4 (Metadata) | 获取失败 | 尝试其他 peer |
| L5 (Media) | 格式不支持 | 提示用户 |
| L5 (Media) | 解码错误 | 跳过损坏帧 |
| L5 (GUI) | 窗口关闭 | 清理所有连接 |

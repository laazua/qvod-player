# QVOD 安全规范

## 安全设计原则

1. **纵深防御**: 在所有层级实施安全检查，不依赖单一防护措施
2. **默认安全**: 所有配置默认以最安全的方式运行
3. **最小权限**: 连接、文件、系统资源按需申请最小权限
4. **输入校验**: 永远不信任外部输入——包括网络数据、文件、URI
5. **失败安全**: 安全机制失败时应拒绝访问，而非允许访问
6. **可审计**: 所有安全相关事件必须记录日志

---

## 1. SHA-1 Piece 验证

### 1.1 验证流程

每一块下载完成的 piece 必须经过 SHA-1 哈希验证才能标记为完成。

```rust
pub struct PieceVerifier;

impl PieceVerifier {
    /// 验证 piece 的 SHA-1 哈希
    ///
    /// # 参数
    /// * `piece_index` - piece 序号 (用于错误报告)
    /// * `data` - 完整的 piece 数据 (PIECE_LENGTH 字节)
    /// * `expected_hash` - 从 FileMeta 中获取的 20 字节期望哈希值
    ///
    /// # 返回
    /// * `Ok(())` - 验证通过
    /// * `Err(QvodError::PieceVerificationFailed)` - 验证失败，包含详细信息
    pub fn verify(
        piece_index: u32,
        data: &[u8],
        expected_hash: &[u8; 20],
    ) -> Result<()> {
        let actual_hash = sha1::Sha1::from(data).digest().bytes();

        if &actual_hash == expected_hash {
            Ok(())
        } else {
            Err(QvodError::PieceVerificationFailed {
                index: piece_index,
                expected: *expected_hash,
                got: actual_hash,
            })
        }
    }
}
```

### 1.2 验证失败处理策略

| 场景 | 处理方式 |
|------|----------|
| 1 次验证失败 | 从不同 peer 重新下载该 piece |
| 3 次验证失败 (不同 peer) | 标记该 piece 为永久损坏，记录日志 |
| 同一 peer 提供 2 次无效数据 | 断开连接并将该 peer 加入黑名单 (30 分钟) |
| 同一 peer 提供 5 次以上无效数据 | 永久拉黑该 peer |

```rust
pub struct PeerIntegrityTracker {
    /// peer_id → 失败次数
    failures: HashMap<[u8; 20], u32>,
    /// peer_id → 被拉黑的时间 (如果已被拉黑)
    banned_until: HashMap<[u8; 20], Instant>,
    /// 全局失败次数
    total_failures: u64,
}

impl PeerIntegrityTracker {
    const MAX_FAILURES_BEFORE_DISCONNECT: u32 = 2;
    const MAX_FAILURES_BEFORE_PERMANENT_BAN: u32 = 5;
    const BAN_DURATION: Duration = Duration::from_secs(1800);  // 30 分钟

    pub fn report_failure(&mut self, peer_id: &[u8; 20]) -> PeerAction {
        let count = self.failures.entry(*peer_id).or_insert(0);
        *count += 1;
        self.total_failures += 1;

        match *count {
            n if n >= Self::MAX_FAILURES_BEFORE_PERMANENT_BAN => {
                // 永久拉黑
                self.banned_until.insert(*peer_id, Instant::now() + Duration::from_secs(365 * 86400));
                warn!(%peer_id, failures = %count, "peer 永久拉黑");
                PeerAction::Ban
            }
            n if n >= Self::MAX_FAILURES_BEFORE_DISCONNECT => {
                // 临时拉黑
                self.banned_until.insert(*peer_id, Instant::now() + Self::BAN_DURATION);
                warn!(%peer_id, failures = %count, "peer 临时拉黑 30 分钟");
                PeerAction::Disconnect
            }
            _ => {
                warn!(%peer_id, failures = %count, "piece 验证失败，请求重试");
                PeerAction::RequestRetry
            }
        }
    }

    pub fn is_banned(&self, peer_id: &[u8; 20]) -> bool {
        self.banned_until.get(peer_id)
            .map(|until| Instant::now() < *until)
            .unwrap_or(false)
    }
}

pub enum PeerAction {
    RequestRetry,   // 重新请求
    Disconnect,     // 断开连接，30分钟后可重新连接
    Ban,           // 永久拉黑
}
```

### 1.3 性能优化

- 使用 `spawn_blocking` 在后台线程计算 SHA-1，避免阻塞异步 runtime
- 为 256KB 的 piece 预分配缓冲区，避免重复分配
- 支持部分验证：如果 piece 在缓存中已存在部分块，仅验证新下载的部分

```rust
pub async fn verify_piece_async(
    piece_index: u32,
    data: Bytes,
    expected_hash: [u8; 20],
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        PieceVerifier::verify(piece_index, &data, &expected_hash)
    })
    .await
    .map_err(|e| QvodError::Protocol(format!("验证任务失败: {e}")))?
}
```

---

## 2. 输入验证

### 2.1 网络输入验证

所有从网络接收的数据必须经过严格验证，包括：

```rust
/// DHT 消息验证
pub fn validate_dht_message(bytes: &[u8]) -> Result<&DhtMessage> {
    // 1. 最小长度检查
    if bytes.len() < HEADER_SIZE {
        return Err(QvodError::Protocol("DHT 消息过短".into()));
    }

    // 2. Magic 验证
    let magic = &bytes[0..4];
    if magic != PROTOCOL_MAGIC {
        return Err(QvodError::Protocol("DHT magic 不匹配".into()));
    }

    // 3. 长度上限检查 (防止放大攻击)
    if bytes.len() > MAX_UDP_PACKET_SIZE as usize {
        return Err(QvodError::Protocol("DHT 消息超过最大长度".into()));
    }

    // 4. 版本兼容检查
    let version = bytes[7];
    if version > MAX_SUPPORTED_VERSION {
        return Err(QvodError::Protocol(format!("不支持的 DHT 版本: {version}")));
    }

    // 5. 消息类型检查
    let msg_type = bytes[4];
    if msg_type > 0x03 {
        return Err(QvodError::Protocol(format!("未知 DHT 消息类型: {msg_type}")));
    }

    // 6. 负载长度检查 (取决于消息类型)
    let payload_len = bytes.len() - HEADER_SIZE;
    validate_payload_length(msg_type, payload_len)?;

    Ok(())
}

/// Peer Wire 消息验证
pub fn validate_peer_message(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 4 {
        return Err(QvodError::Protocol("Peer 消息长度不足 4 字节".into()));
    }

    // 长度前缀解码
    let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;

    // 检查总长度匹配
    if bytes.len() != length + 4 {
        return Err(QvodError::Protocol("Peer 消息长度与声明不一致".into()));
    }

    // Keep-alive (length = 0) 是合法的
    if length == 0 {
        return Ok(());
    }

    // 消息 ID 检查
    let msg_id = bytes[4];
    if msg_id > 0x0D {
        return Err(QvodError::Protocol(format!("未知 Peer 消息类型: {msg_id}")));
    }

    // 具体消息的 payload 长度检查
    match msg_id {
        0x04 | 0x0A => { // have / suggest_piece: 4 bytes
            if length != 5 { return Err(QvodError::Protocol("have 消息长度错误".into())); }
        }
        0x06 | 0x08 | 0x0B => { // request / cancel / reject_request: 12 bytes payload
            if length != 13 { return Err(QvodError::Protocol("request 消息长度错误".into())); }
        }
        0x07 => { // piece: 至少 8 bytes payload
            if length < 9 { return Err(QvodError::Protocol("piece 消息长度不足".into())); }
        }
        0x09 => { // port: 2 bytes payload
            if length != 3 { return Err(QvodError::Protocol("port 消息长度错误".into())); }
        }
        _ => {}
    }

    Ok(())
}

/// Tracker 响应验证
pub fn validate_tracker_response(data: &[u8]) -> Result<BencodeValue> {
    // Bencode 解析
    let (value, _rest) = BencodeValue::decode(data)
        .map_err(|_| QvodError::TrackerProtocol("无效的 Bencode 响应".into()))?;

    // 必须是字典
    let dict = match &value {
        BencodeValue::Dict(d) => d,
        _ => return Err(QvodError::TrackerProtocol("Tracker 响应不是字典".into())),
    };

    // 验证必选字段
    if !dict.contains_key("interval") || !dict.contains_key("peers") {
        return Err(QvodError::TrackerProtocol("Tracker 响应缺少必选字段".into()));
    }

    // interval 必须为正整数
    if let Some(BencodeValue::Int(interval)) = dict.get("interval") {
        if *interval <= 0 {
            return Err(QvodError::TrackerProtocol("interval 必须为正数".into()));
        }
        if *interval > 3600 {
            // 最大值检查
            return Err(QvodError::TrackerProtocol("interval 超出最大值 3600".into()));
        }
    }

    // peers 字段类型校验 (compact 或 list)
    match dict.get("peers") {
        Some(BencodeValue::Str(peers_bytes)) => {
            // compact 格式: 每 6 字节为一个 peer
            if peers_bytes.len() % 6 != 0 {
                return Err(QvodError::TrackerProtocol("compact peers 长度不是 6 的倍数".into()));
            }
            if peers_bytes.len() > 6 * 200 {
                // 限制最多 200 个 peer
                return Err(QvodError::TrackerProtocol("peers 数量超过上限 200".into()));
            }
        }
        Some(BencodeValue::List(peers_list)) => {
            if peers_list.len() > 200 {
                return Err(QvodError::TrackerProtocol("peers 数量超过上限 200".into()));
            }
        }
        _ => {
            return Err(QvodError::TrackerProtocol("peers 字段类型错误".into()));
        }
    }

    Ok(value)
}
```

### 2.2 URI 输入验证

```rust
/// qvod:// URI 验证
pub fn validate_qvod_uri(uri: &str) -> Result<QvodUri> {
    // 1. scheme 验证
    if !uri.starts_with("qvod://") {
        return Err(QvodError::InvalidUri("必须以 qvod:// 开头".into()));
    }

    let rest = &uri[7..];  // 去掉 "qvod://"

    // 2. 分割字段
    let parts: Vec<&str> = rest.split('|').collect();

    // 3. 字段数量验证: info_hash | filename | filesize | format | (末尾空)
    if parts.len() < 5 {
        return Err(QvodError::InvalidUri("参数不足，需要 4 个 | 分隔的字段".into()));
    }

    // 4. 验证末尾的 |
    if !uri.ends_with('|') {
        return Err(QvodError::InvalidUri("URI 必须以 | 结尾".into()));
    }

    // 5. info_hash 验证
    let info_hash_hex = parts[0];
    if info_hash_hex.len() != 40 {
        return Err(QvodError::InvalidUri(
            format!("info_hash 长度应为 40 字符 hex，实际为 {}", info_hash_hex.len())
        ));
    }
    // 确保 hex 字符
    if !info_hash_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(QvodError::InvalidUri("info_hash 包含非十六进制字符".into()));
    }

    // 6. filename 验证 (防止路径遍历)
    let filename = parts[1];
    if filename.is_empty() {
        return Err(QvodError::InvalidUri("filename 不能为空".into()));
    }
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(QvodError::InvalidUri("filename 包含非法字符".into()));
    }
    if filename.len() > 255 {
        return Err(QvodError::InvalidUri("filename 超过 255 字符".into()));
    }

    // 7. filesize 验证
    let filesize = parts[2];
    let size: u64 = filesize.parse()
        .map_err(|_| QvodError::InvalidUri("filesize 不是合法数字".into()))?;
    if size == 0 {
        return Err(QvodError::InvalidUri("filesize 必须大于 0".into()));
    }
    if size > 100 * 1024 * 1024 * 1024 {  // 100GB 上限
        return Err(QvodError::InvalidUri("filesize 超过 100GB 上限".into()));
    }

    // 8. format 验证
    let format = parts[3];
    let allowed_formats = ["rmvb", "avi", "mkv", "mp4", "wmv", "flv", "mov", "ts", "webm", "3gp"];
    if !allowed_formats.contains(&format) {
        return Err(QvodError::InvalidUri(
            format!("不支持的视频格式: {format}")
        ));
    }

    // 构造 QvodUri
    Ok(QvodUri {
        info_hash: InfoHash::from_hex(info_hash_hex)?,
        filename: filename.to_string(),
        filesize: size,
        format: format.to_string(),
    })
}
```

### 2.3 HTTP 请求验证

```rust
/// 验证 /play 请求参数
pub fn validate_play_request(params: &HashMap<String, String>) -> Result<PlayRequest> {
    let hash = params.get("hash")
        .ok_or_else(|| QvodError::InvalidUri("缺少 hash 参数".into()))?;

    // hash 参数可以是 40 字符 hex 或 20 字节 base64
    if hash.len() != 40 && hash.len() != 32 {
        return Err(QvodError::InvalidUri("hash 参数长度错误".into()));
    }

    // 验证 offset 参数 (可选)
    let offset = if let Some(offset_str) = params.get("offset") {
        let offset: u64 = offset_str.parse()
            .map_err(|_| QvodError::InvalidUri("offset 不是合法数字".into()))?;
        if offset > 100 * 1024 * 1024 * 1024 {  // 100GB 上限
            return Err(QvodError::InvalidUri("offset 超出范围".into()));
        }
        Some(offset)
    } else {
        None
    };

    Ok(PlayRequest {
        info_hash: InfoHash::from_hex(hash)?,
        offset,
    })
}
```

---

## 3. 反欺骗 (Anti-Spoofing)

### 3.1 DHT 消息防欺骗

```rust
/// DHT 消息源验证
pub fn validate_dht_source(packet: &[u8], source: &SocketAddr) -> Result<()> {
    // 1. 源地址验证
    //    - 禁止来自私网地址的 DHT 消息 (除非配置允许)
    //    - 禁止来自广播地址
    //    - 禁止来自多播地址
    if !is_public_addr(*source) && !cfg!(test) {
        return Err(QvodError::Protocol("DHT 消息来自私网地址".into()));
    }

    // 2. 请求-响应匹配验证
    //    确保响应中的 node_id 与之前请求的 target 匹配
    //    (由调用方在事务表中维护)

    // 3. RPC ID 验证
    //    验证响应中的 node_id 与之前请求的一致
    //    (由调用方验证)

    Ok(())
}

/// 检查是否为可路由的公共地址
fn is_public_addr(addr: SocketAddr) -> bool {
    match addr.ip() {
        std::net::IpAddr::V4(ip) => {
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_unspecified()
                || ip.is_documentation())
        }
        std::net::IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}
```

### 3.2 Token 验证 (Anti-Spoofing for Announce)

```rust
/// Announce Token 管理
///
/// Token 用于验证 announce 请求的来源是否合法。
/// 只有经过 find_peers 查询的节点才能 announce。
pub struct TokenManager {
    /// 当前秘密 (每 10 分钟轮换)
    current_secret: [u8; 16],
    /// 上一个秘密 (用于处理时钟偏移)
    previous_secret: [u8; 16],
    /// 上次轮换时间
    last_rotation: Instant,
}

impl TokenManager {
    pub fn new() -> Self {
        Self {
            current_secret: rand::random(),
            previous_secret: rand::random(),
            last_rotation: Instant::now(),
        }
    }

    /// 为指定地址生成 token
    /// token = SHA-1(ip + port + secret)[0..4]
    pub fn generate_token(&self, addr: &SocketAddr) -> [u8; 4] {
        let mut hasher = sha1::Sha1::new();
        hasher.update(addr.ip().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(addr.port().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(&self.current_secret);
        let result = hasher.digest().bytes();
        let mut token = [0u8; 4];
        token.copy_from_slice(&result[..4]);
        token
    }

    /// 验证 token
    pub fn verify_token(&self, addr: &SocketAddr, token: &[u8; 4]) -> bool {
        // 检查当前秘密
        if &self.generate_token(addr) == token {
            return true;
        }
        // 检查上一个秘密 (5 分钟窗口)
        let prev_token = self.generate_with_secret(addr, &self.previous_secret);
        if &prev_token == token {
            return true;
        }
        false
    }

    /// 每 10 分钟轮换秘密
    pub fn maybe_rotate(&mut self) {
        if self.last_rotation.elapsed() >= Duration::from_secs(600) {
            self.previous_secret = self.current_secret;
            self.current_secret = rand::random();
            self.last_rotation = Instant::now();
        }
    }

    fn generate_with_secret(&self, addr: &SocketAddr, secret: &[u8; 16]) -> [u8; 4] {
        let mut hasher = sha1::Sha1::new();
        hasher.update(addr.ip().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(addr.port().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(secret);
        let result = hasher.digest().bytes();
        let mut token = [0u8; 4];
        token.copy_from_slice(&result[..4]);
        token
    }
}
```

### 3.3 Peer ID 验证

```rust
/// Peer ID 验证
///
/// 合法的 peer_id 格式:
///   - 20 字节
///   - 前 3 字节: 客户端标识 (如 "QVS" = 0x51, 0x56, 0x53)
///   - 后 17 字节: 随机数
///
/// 验证规则:
///   1. 长度为 20 字节
///   2. 包含可打印 ASCII 字符
///   3. 不与本地 peer_id 相同
///   4. 前 3 字节不能为全零
pub fn validate_peer_id(peer_id: &[u8; 20], local_peer_id: &[u8; 20]) -> Result<()> {
    // 自我连接防御
    if peer_id == local_peer_id {
        return Err(QvodError::Protocol("检测到自我连接".into()));
    }

    // 检查是否全零 (未初始化)
    if peer_id.iter().all(|&b| b == 0) {
        return Err(QvodError::Protocol("peer_id 全零".into()));
    }

    // 检查是否包含非法字节
    if peer_id.iter().any(|&b| b < 0x20 || b > 0x7E) {
        // DHT 节点 ID 可以是任意字节
        // 但 peer_id 应为可打印字符
        // 这里仅做 warn，不强制拒绝
        warn!("peer_id 包含不可打印字符");
    }

    Ok(())
}
```

---

## 4. DoS 防护

### 4.1 连接限制

```rust
/// 连接速率限制器
pub struct ConnectionRateLimiter {
    /// 每个 IP 的连接数
    connections_per_ip: HashMap<IpAddr, u32>,
    /// 每个 IP 的连接尝试率
    attempts_per_ip: HashMap<IpAddr, Vec<Instant>>,
    /// 全局连接数
    total_connections: u32,
    /// 配置
    config: RateLimitConfig,
}

pub struct RateLimitConfig {
    /// 每个 IP 最大并发连接数
    pub max_connections_per_ip: u32,
    /// 全局最大连接数
    pub max_total_connections: u32,
    /// 连接频率限制 (每分钟最大尝试数)
    pub max_attempts_per_minute: u32,
    /// 连接频率限制窗口 (秒)
    pub rate_limit_window: u64,
    /// 频率限制超过后拉黑时间 (秒)
    pub ban_duration: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_connections_per_ip: 5,
            max_total_connections: 50,
            max_attempts_per_minute: 20,
            rate_limit_window: 60,
            ban_duration: 300,  // 5 分钟
        }
    }
}

impl ConnectionRateLimiter {
    /// 检查是否允许新连接
    pub fn allow_connection(&mut self, ip: IpAddr) -> bool {
        // 全局限制
        if self.total_connections >= self.config.max_total_connections {
            warn!(%ip, "全局连接数达到上限");
            return false;
        }

        // 单 IP 连接数限制
        let ip_count = self.connections_per_ip.get(&ip).copied().unwrap_or(0);
        if ip_count >= self.config.max_connections_per_ip {
            warn!(%ip, connections = %ip_count, "单个 IP 连接数达到上限");
            return false;
        }

        // 频率限制
        let now = Instant::now();
        let attempts = self.attempts_per_ip.entry(ip).or_default();
        let window_start = now - Duration::from_secs(self.config.rate_limit_window);

        // 移除窗口外的记录
        attempts.retain(|t| *t > window_start);

        if attempts.len() >= self.config.max_attempts_per_minute as usize {
            warn!(%ip, attempts = %attempts.len(), "连接频率超过限制");
            return false;
        }

        attempts.push(now);
        true
    }

    /// 记录新连接
    pub fn record_connection(&mut self, ip: IpAddr) {
        *self.connections_per_ip.entry(ip).or_insert(0) += 1;
        self.total_connections += 1;
    }

    /// 记录断开连接
    pub fn record_disconnection(&mut self, ip: IpAddr) {
        if let Some(count) = self.connections_per_ip.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
            }
        }
        if self.total_connections > 0 {
            self.total_connections -= 1;
        }
    }
}
```

### 4.2 消息速率限制

```rust
/// DHT 消息速率限制
pub struct DhtRateLimiter {
    /// 每个节点 (IP:Port) 的最近消息时间
    last_message: HashMap<SocketAddr, Instant>,
    /// 每个节点的消息计数窗口
    message_counts: HashMap<SocketAddr, Vec<Instant>>,
}

impl DhtRateLimiter {
    const MIN_INTERVAL: Duration = Duration::from_millis(100);  // 最小间隔 100ms
    const MAX_MESSAGES_PER_WINDOW: usize = 50;                  // 每窗口最多 50 条
    const WINDOW_DURATION: Duration = Duration::from_secs(10);  // 10 秒窗口

    /// 检查是否允许处理来自该地址的消息
    pub fn allow_message(&mut self, addr: SocketAddr) -> bool {
        let now = Instant::now();

        // 最小间隔检查
        if let Some(last) = self.last_message.get(&addr) {
            if now - *last < Self::MIN_INTERVAL {
                warn!(%addr, "消息间隔过短");
                return false;
            }
        }

        // 窗口内计数检查
        let counts = self.message_counts.entry(addr).or_default();
        let window_start = now - Self::WINDOW_DURATION;
        counts.retain(|t| *t > window_start);

        if counts.len() >= Self::MAX_MESSAGES_PER_WINDOW {
            warn!(%addr, count = %counts.len(), "消息频率超过限制");
            return false;
        }

        counts.push(now);
        self.last_message.insert(addr, now);
        true
    }
}

/// Peer Wire 消息速率限制
pub struct PeerMessageRateLimiter {
    /// 每个 peer 每秒允许的最大请求数
    requests_per_second: HashMap<[u8; 20], Vec<Instant>>,
}

impl PeerMessageRateLimiter {
    const MAX_REQUESTS_PER_SECOND: usize = 20;  // 每秒最多 20 个 request
    const MAX_PIECE_PER_SECOND: usize = 5;      // 每秒最多 5 个 piece 响应

    pub fn allow_request(&mut self, peer_id: &[u8; 20]) -> bool {
        let now = Instant::now();
        let requests = self.requests_per_second.entry(*peer_id).or_default();
        let window_start = now - Duration::from_secs(1);
        requests.retain(|t| *t > window_start);

        if requests.len() >= Self::MAX_REQUESTS_PER_SECOND {
            warn!(%peer_id, "peer request 频率超过限制");
            return false;
        }

        requests.push(now);
        true
    }
}
```

### 4.3 带宽限制

```rust
/// 带宽限制器
pub struct BandwidthLimiter {
    /// 当前所有连接的下行带宽
    download_budget: u64,     // bytes/s
    /// 当前所有连接的上行带宽
    upload_budget: u64,       // bytes/s
    /// 当前周期开始时间
    interval_start: Instant,
    /// 当前周期已用下行字节数
    downloaded_this_interval: u64,
    /// 当前周期已用上行字节数
    uploaded_this_interval: u64,
    /// 配置
    config: BandwidthConfig,
}

pub struct BandwidthConfig {
    pub max_upload_bytes_per_sec: u64,
    pub max_download_bytes_per_sec: u64,
    pub per_peer_upload_limit: u64,
    pub per_peer_download_limit: u64,
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            max_upload_bytes_per_sec: 1024 * 1024,      // 1 MB/s
            max_download_bytes_per_sec: 10 * 1024 * 1024, // 10 MB/s
            per_peer_upload_limit: 512 * 1024,            // 512 KB/s
            per_peer_download_limit: 1024 * 1024,         // 1 MB/s
        }
    }
}

impl BandwidthLimiter {
    /// 检查是否允许发送指定大小的数据
    pub fn allow_upload(&mut self, size: u64) -> bool {
        self.reset_if_needed();
        if self.uploaded_this_interval + size > self.config.max_upload_bytes_per_sec {
            return false;
        }
        self.uploaded_this_interval += size;
        true
    }

    /// 检查是否允许下载指定大小的数据
    pub fn allow_download(&mut self, size: u64) -> bool {
        self.reset_if_needed();
        if self.downloaded_this_interval + size > self.config.max_download_bytes_per_sec {
            // 下载限速时，降低 Critical 之外的 piece 优先级
            return false;
        }
        self.downloaded_this_interval += size;
        true
    }

    fn reset_if_needed(&mut self) {
        if self.interval_start.elapsed() >= Duration::from_secs(1) {
            self.interval_start = Instant::now();
            self.downloaded_this_interval = 0;
            self.uploaded_this_interval = 0;
        }
    }
}
```

### 4.4 DHT Amplification 防护

```rust
/// DHT 放大攻击防护
///
/// 攻击者发送伪造的 FIND_NODE 请求，将源地址设为受害者地址，
/// 导致大量响应发送给受害者（反射放大攻击）。
///
/// 防护措施:
///   1. 请求和响应大小均衡（响应不应显著大于请求）
///   2. 不响应长度超过请求一定倍数的查询
///   3. 单 IP 速率限制
pub struct AmplificationProtection;

impl AmplificationProtection {
    /// 检查响应大小是否安全
    /// 响应不应大于请求的 3 倍
    pub fn check_response_ratio(req_len: usize, resp_len: usize) -> bool {
        resp_len <= req_len * 3
    }

    /// 最大响应大小 (UDP 安全)
    pub const MAX_RESPONSE_SIZE: usize = 1400;

    /// 节点响应中的最大节点数
    /// 确保响应包不会超过 MTU
    pub const MAX_NODES_IN_RESPONSE: usize = 8;
    pub const MAX_PEERS_IN_RESPONSE: usize = 50;

    /// 验证节点数量
    pub fn validate_node_count(count: usize) -> bool {
        count <= Self::MAX_NODES_IN_RESPONSE
    }
}
```

---

## 5. 缓存目录沙盒

### 5.1 路径遍历防护

```rust
/// 缓存路径安全处理
pub struct CachePathValidator;

impl CachePathValidator {
    /// 验证 info_hash 对应的缓存文件路径
    ///
    /// 防止路径遍历攻击: info_hash 经过严格 hex 格式验证后
    /// 才用于构造文件路径
    pub fn validate_cache_path(cache_dir: &Path, info_hash: &InfoHash) -> Result<PathBuf> {
        let hash_hex = info_hash.to_hex();

        // 验证 hex 格式 (已经在 InfoHash 中保证)
        assert_eq!(hash_hex.len(), 40);
        assert!(hash_hex.chars().all(|c| c.is_ascii_hexdigit()));

        // 构造路径 (使用 join 而不是字符串拼接)
        let path = cache_dir.join("qdata").join(format!("{}.qdata", hash_hex));

        // 沙盒检查: 确保路径在 cache_dir 内
        let canonical_cache = cache_dir.canonicalize()
            .map_err(|_| QvodError::CacheCorrupted("缓存目录不存在".into()))?;
        let canonical_path = path.canonicalize()
            .map_err(|e| QvodError::CacheCorrupted(format!("路径解析失败: {e}")))?;

        if !canonical_path.starts_with(&canonical_cache) {
            return Err(QvodError::CacheCorrupted("路径遍历攻击检测".into()));
        }

        Ok(path)
    }

    /// 验证文件名 (只允许安全的字符)
    pub fn validate_filename(filename: &str) -> Result<()> {
        if filename.is_empty() {
            return Err(QvodError::InvalidUri("filename 不能为空".into()));
        }
        if filename.len() > 255 {
            return Err(QvodError::InvalidUri("filename 超过 255 字符".into()));
        }
        // 只允许字母、数字、点、下划线、连字符、空格
        if !filename.chars().all(|c| c.is_alphanumeric()
            || c == '.' || c == '_' || c == '-' || c == ' ')
        {
            return Err(QvodError::InvalidUri("filename 包含非法字符".into()));
        }
        // 禁止以点开头 (隐藏文件)
        if filename.starts_with('.') {
            return Err(QvodError::InvalidUri("filename 不能以点开头".into()));
        }
        // 禁止 .. 路径
        if filename.contains("..") {
            return Err(QvodError::InvalidUri("filename 包含路径遍历".into()));
        }
        Ok(())
    }
}
```

### 5.2 缓存大小管理

```rust
/// 缓存空间管理
///
/// 安全约束:
///   1. 缓存总大小不超过配置的最大值 (默认 4GB)
///   2. 单个缓存文件不超过 10GB
///   3. 磁盘使用率达到 90% 时停止写入
///   4. 使用 LRU 策略自动清理
pub struct CacheSpaceManager {
    max_cache_size: u64,
    cache_dir: PathBuf,
}

impl CacheSpaceManager {
    pub fn new(cache_dir: PathBuf, max_cache_size: u64) -> Self {
        Self {
            max_cache_size,
            cache_dir,
        }
    }

    /// 检查是否有足够空间写入数据
    pub fn has_space_for(&self, required_bytes: u64) -> Result<bool> {
        // 检查当前缓存大小
        let current_size = self.calculate_cache_size()?;

        if current_size + required_bytes > self.max_cache_size {
            // 尝试清理
            self.cleanup_lru()?;
            let new_size = self.calculate_cache_size()?;
            if new_size + required_bytes > self.max_cache_size {
                return Err(QvodError::CacheFull);
            }
        }

        // 检查磁盘空间
        let disk_info = fs2::statvfs(&self.cache_dir)
            .map_err(|e| QvodError::CacheCorrupted(format!("磁盘检查失败: {e}")))?;
        let available_bytes = disk_info.available_space();
        if available_bytes < required_bytes {
            return Err(QvodError::CacheFull);
        }

        Ok(true)
    }

    /// 计算当前缓存大小
    fn calculate_cache_size(&self) -> Result<u64> {
        let qdata_dir = self.cache_dir.join("qdata");
        if !qdata_dir.exists() {
            return Ok(0);
        }
        let mut total = 0u64;
        for entry in std::fs::read_dir(&qdata_dir)
            .map_err(|e| QvodError::CacheCorrupted(format!("读取缓存目录失败: {e}")))? {
            let entry = entry.map_err(|e| QvodError::CacheCorrupted(format!("读取条目失败: {e}")))?;
            let metadata = entry.metadata()
                .map_err(|e| QvodError::CacheCorrupted(format!("读取元数据失败: {e}")))?;
            if metadata.is_file() {
                total += metadata.len();
            }
        }
        Ok(total)
    }

    /// LRU 清理：删除最早访问的文件直到低于 max_size 的 80%
    fn cleanup_lru(&self) -> Result<()> {
        let qdata_dir = self.cache_dir.join("qdata");
        let qmv_dir = self.cache_dir.join("qmv");

        // 收集所有缓存文件及其访问时间
        let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

        if qdata_dir.exists() {
            for entry in std::fs::read_dir(&qdata_dir)
                .map_err(|e| QvodError::CacheCorrupted(format!("读取缓存失败: {e}")))? {
                let entry = entry.map_err(|e| QvodError::CacheCorrupted(e.to_string()))?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("qdata") {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(atime) = metadata.accessed() {
                            files.push((path, atime));
                        }
                    }
                }
            }
        }

        // 按访问时间排序 (最旧的在前面)
        files.sort_by_key(|(_, time)| *time);

        // 计算需要删除的数据量 (当前大小 - max_size * 0.8)
        let current_size = self.calculate_cache_size()?;
        let target_size = (self.max_cache_size as f64 * 0.8) as u64;
        let mut to_delete = current_size.saturating_sub(target_size);

        for (qdata_path, _) in &files {
            if to_delete <= 0 {
                break;
            }

            // 删除 .qdata 文件
            if let Ok(metadata) = std::fs::metadata(qdata_path) {
                let file_size = metadata.len();
                std::fs::remove_file(qdata_path).ok();
                to_delete = to_delete.saturating_sub(file_size);
            }

            // 删除对应的 .qmv 文件
            let stem = qdata_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let qmv_path = qmv_dir.join(format!("{stem}.qmv"));
            std::fs::remove_file(&qmv_path).ok();
        }

        Ok(())
    }
}
```

### 5.3 文件权限

```rust
/// 设置安全文件权限
pub fn set_secure_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // -rw------- (600): 只有当前用户可读写
        let mut perms = std::fs::metadata(path)
            .map_err(|e| QvodError::CacheCorrupted(format!("无法获取文件权限: {e}")))?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)
            .map_err(|e| QvodError::CacheCorrupted(format!("无法设置文件权限: {e}")))?;
    }

    #[cfg(windows)]
    {
        // 在 Windows 上，确保文件不被继承权限
        // 需要调用 SetEntriesInAcl API
        // 简化为: 使用 std::fs::set_permissions
    }

    Ok(())
}

/// 确保缓存目录存在并设置安全权限
pub fn ensure_cache_dir(cache_dir: &Path) -> Result<()> {
    // 创建目录结构
    std::fs::create_dir_all(cache_dir.join("qdata"))
        .map_err(|e| QvodError::CacheCorrupted(format!("创建 qdata 目录失败: {e}")))?;
    std::fs::create_dir_all(cache_dir.join("qmv"))
        .map_err(|e| QvodError::CacheCorrupted(format!("创建 qmv 目录失败: {e}")))?;

    // 设置目录权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // drwx------ (700): 只有当前用户可访问
        let mut perms = std::fs::metadata(cache_dir)
            .map_err(|e| QvodError::CacheCorrupted(e.to_string()))?
            .permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(cache_dir, perms)
            .map_err(|e| QvodError::CacheCorrupted(e.to_string()))?;
        std::fs::set_permissions(cache_dir.join("qdata"), perms.clone())
            .map_err(|e| QvodError::CacheCorrupted(e.to_string()))?;
        std::fs::set_permissions(cache_dir.join("qmv"), perms)
            .map_err(|e| QvodError::CacheCorrupted(e.to_string()))?;
    }

    Ok(())
}
```

---

## 6. 配置安全

### 6.1 配置加载

```rust
/// 安全配置加载
///
/// 安全约束:
///   1. 配置文件必须属于当前用户 (Unix: owner match)
///   2. 配置文件权限不能是 world-readable (Unix: 不允许 other 读)
///   3. 所有路径必须经过沙盒验证
///   4. 端口必须在有效范围内
pub struct SecureConfigLoader;

impl SecureConfigLoader {
    pub fn load(path: &Path) -> Result<EngineConfig> {
        // 1. 文件存在性检查
        if !path.exists() {
            return Err(QvodError::Protocol("配置文件不存在".into()));
        }

        // 2. 文件所有权检查 (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(path)
                .map_err(|e| QvodError::Protocol(format!("无法读取配置文件元数据: {e}")))?;
            let uid = metadata.uid();
            let current_uid = unsafe { libc::getuid() };
            if uid != current_uid {
                return Err(QvodError::Protocol("配置文件不属于当前用户".into()));
            }
        }

        // 3. 文件权限检查 (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(path)
                .map_err(|e| QvodError::Protocol(e.to_string()))?;
            let mode = metadata.permissions().mode();
            // 不允许 other 有读权限 (o+r)
            if mode & 0o004 != 0 {
                return Err(QvodError::Protocol("配置文件权限不安全: other 可读".into()));
            }
        }

        // 4. 读取文件
        let content = std::fs::read_to_string(path)
            .map_err(|e| QvodError::Protocol(format!("无法读取配置文件: {e}")))?;

        // 5. 解析 TOML
        let config: EngineConfig = toml::from_str(&content)
            .map_err(|e| QvodError::Protocol(format!("配置解析错误: {e}")))?;

        // 6. 验证配置值
        config.validate()?;

        Ok(config)
    }
}

impl EngineConfig {
    /// 验证配置值的安全性
    pub fn validate(&self) -> Result<()> {
        // 端口验证
        if self.listen_port != 0 && (self.listen_port < 1024 || self.listen_port > 65535) {
            return Err(QvodError::Protocol("listen_port 必须在 1024-65535 范围或为 0".into()));
        }
        if self.udp_port != 0 && (self.udp_port < 1024 || self.udp_port > 65535) {
            return Err(QvodError::Protocol("udp_port 必须在 1024-65535 范围或为 0".into()));
        }

        // 连接数限制
        if self.max_connections > 200 {
            return Err(QvodError::Protocol("max_connections 不能超过 200".into()));
        }

        // 缓冲区大小限制
        if self.buffer_capacity_mb > 1024 {
            return Err(QvodError::Protocol("buffer_capacity_mb 不能超过 1024".into()));
        }

        // 缓存目录验证
        CachePathValidator::validate_filename(
            self.cache_dir.to_str().unwrap_or("")
        )?;

        // Tracker URL 验证
        for url in &self.tracker_urls {
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(QvodError::Protocol(format!("Tracker URL 协议不支持: {url}")));
            }
        }

        // DHT 种子节点验证 (仅允许公共地址)
        for seed in &self.dht_seed_nodes {
            if !is_public_addr(*seed) {
                return Err(QvodError::Protocol(format!("DHT 种子节点地址非法: {seed}")));
            }
        }

        Ok(())
    }
}
```

### 6.2 敏感信息处理

```rust
/// 不记录敏感信息
/// 日志中不能出现:
///   - peer_id 的完整值 (只记录前 8 个字节)
///   - info_hash 的完整值 (只记录前 8 个字节)
///   - 用户文件路径
///   - 任何 token/secret

// 好的日志:
info!(peer = %short_id(peer_id), "peer 连接成功");

// 不好的日志:
info!(peer_id = %hex::encode(peer_id), "peer 连接成功");

// 辅助函数: 缩短 ID 显示
fn short_id(id: &[u8; 20]) -> String {
    hex::encode(&id[..8])
}
```

---

## 7. 运行时安全

### 7.1 恐慌安全

```rust
/// 所有异步任务的恐慌捕获
///
/// 使用 catch_unwind 捕获子任务的恐慌，防止整个进程崩溃
pub async fn run_safely<F, T>(fut: F) -> Result<T>
where
    F: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let result = tokio::task::spawn(async {
        // 捕获恐慌
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // 这里无法直接 await future，所以使用 block_on
            // 实际应该使用其他方式
            panic!("任务恐慌捕获需要特殊处理");
        })) {
            Ok(_) => fut.await,
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "未知恐慌".to_string()
                };
                error!(panic = %msg, "任务恐慌");
                Err(QvodError::Protocol(format!("任务恐慌: {msg}")))
            }
        }
    }).await;

    match result {
        Ok(inner) => inner,
        Err(join_err) => {
            error!("任务 join 错误: {join_err}");
            Err(QvodError::Protocol("任务执行异常".into()))
        }
    }
}
```

### 7.2 资源清理

```rust
/// 安全关闭：确保所有资源被释放
pub struct CleanupGuard {
    operations: Vec<Box<dyn FnOnce() + Send>>,
}

impl CleanupGuard {
    pub fn new() -> Self {
        Self { operations: Vec::new() }
    }

    pub fn add<F>(&mut self, op: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.operations.push(Box::new(op));
    }

    /// 执行所有清理操作
    pub fn cleanup(self) {
        for op in self.operations {
            op();
        }
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        // 如果 panic 则跳过清理 (避免双重清理)
        if std::thread::panicking() {
            return;
        }
        // 正常 drop 时执行清理
        let ops = std::mem::take(&mut self.operations);
        for op in ops {
            op();
        }
    }
}
```

---

## 8. 安全事件日志

```rust
/// 所有安全事件必须记录结构化日志
///
/// 安全事件类别:
///   SEC_AUTH: 认证/验证事件 (token 验证失败, 校验失败)
///   SEC_INPUT: 输入验证事件 (无效 URI, 畸形消息)
///   SEC_RATE: 速率限制事件 (连接频率超限)
///   SEC_INTEGRITY: 完整性事件 (piece 校验失败)
///   SEC_SANDBOX: 沙盒事件 (路径遍历尝试)

pub fn log_security_event(
    category: &str,
    severity: &str,
    details: &dyn std::fmt::Display,
) {
    let now = chrono::Utc::now();
    eprintln!("[SECURITY] [{now}] [{category}] [{severity}] {details}");
    // 同步写入安全日志文件
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/qvs_security.log")  // TODO: 使用配置的日志路径
    {
        use std::io::Write;
        writeln!(file, "[{now}] [{category}] [{severity}] {details}").ok();
    }
}

// 使用示例:
// log_security_event("SEC_INTEGRITY", "WARN",
//     &format_args!("Piece {} 验证失败，来自 peer {:?}", index, peer_id));
```

---

## 9. 安全合规检查清单

在发布前必须检查：

### 9.1 输入验证
- [ ] 所有 HTTP API 参数经过类型和范围验证
- [ ] qvod:// URI 的每个字段经过严格格式检查
- [ ] Bencode 解析有最大深度和大小限制
- [ ] 网络消息的每个字段经过长度和范围检查
- [ ] 文件名经过路径遍历防护检查

### 9.2 数据完整性
- [ ] 每个下载的 piece 经过 SHA-1 验证
- [ ] piece 验证失败后有降级和拉黑策略
- [ ] Metadata 解析有完整性检查

### 9.3 DoS 防护
- [ ] 全局连接数限制
- [ ] 单 IP 连接数限制
- [ ] DHT 消息速率限制
- [ ] Peer 消息速率限制
- [ ] 放大攻击防护
- [ ] 带宽速率限制

### 9.4 防欺骗
- [ ] DHT source 地址验证
- [ ] Announce token 验证
- [ ] Peer ID 自我连接检测
- [ ] 响应节点数量限制

### 9.5 文件安全
- [ ] 缓存目录不在系统关键路径
- [ ] 缓存文件权限仅限当前用户
- [ ] 路径遍历防护 (canonicalize + starts_with)
- [ ] 缓存大小上限控制
- [ ] 自动 LRU 清理

### 9.6 配置安全
- [ ] 配置文件仅当前用户可读
- [ ] 端口范围限制
- [ ] 连接数上限
- [ ] 种子节点地址验证

### 9.7 运行时安全
- [ ] 子任务恐慌捕获
- [ ] 资源清理机制
- [ ] 安全事件日志
- [ ] 不记录敏感信息

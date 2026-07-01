# QVOD 项目规范与编码约定

## 项目组织

### 目录结构
```
qvs/
├── crates/          # Rust workspace crates
│   ├── qvs-core/    # 基础类型与 trait
│   ├── qvs-dht/     # DHT 网络
│   ├── qvs-tracker/ # Tracker 客户端
│   ├── qvs-transport/ # P2SP 传输层
│   ├── qvs-stream/  # 流媒体引擎
│   ├── qvs-local-server/ # 本地 Web 服务
│   ├── qvs-media/   # 媒体解码/渲染
│   ├── qvs-gui/     # GUI 播放器
│   └── qvs-format/  # 格式工具
├── server/          # 服务器端代码
├── client/          # 客户端代码
├── web/             # Web 界面
├── agents/          # AI 辅助文档
├── docs/            # 技术参考文档
└── tests/           # 集成测试
```

### 命名约定

- Crate 名: `qvs-{组件名}`, 如 `qvs-dht`, `qvs-stream`
- 模块名: 蛇形 (snake_case), 如 `peer_wire`, `tcp_stream`
- 类型名: 大驼峰 (PascalCase), 如 `RingBuffer`, `QvodEngine`
- 函数/方法: 蛇形 (snake_case), 如 `find_nearest_keyframe`
- 常量: 全大写下划线 (SCREAMING_SNAKE_CASE), 如 `MAX_PEER_CONNECTIONS`
- 枚举变体: 大驼峰 (PascalCase), 如 `PiecePriority::Critical`
- trait 名: 大驼峰, 如 `DhtEngine`, `CacheBackend`
- 错误变体: 大驼峰, 如 `QvodError::NoPeers`
- 类型参数: 简短大驼峰, 如 `T`, `E`, `Item`

## Rust 编码规范

### 必须遵守

- Rust 2024 edition
- 所有公共 API 必须包含文档注释 (`///` 或 `//!`)
- 禁止 `unsafe` 代码，除非在 `qvs-media` 的 FFI 边界且经过专门批准
- 所有错误必须返回 `QvodError` (或其变体)
- 使用 `thiserror` 派生宏定义错误类型
- 使用 `tracing` crate 做结构化日志，禁止使用 `println!` / `eprintln!`
- 所有异步函数使用 `async`/`await` + tokio runtime
- 主类型实现 `Send + Sync` 以支持跨线程安全

### 模块组织

- 每个 crate 一个 lib.rs 作为模块导出入口
- 子模块文件命名与结构中 `src/` 下的文件一一对应
- 一个文件不超过 800 行，超过则拆分子模块
- 公开类型统一在 lib.rs 中 `pub use` 重新导出

### 错误处理

```rust
// ✓ 正确：使用 thiserror
#[derive(Debug, thiserror::Error)]
pub enum QvodError {
    #[error("网络错误: {0}")]
    Network(#[from] std::io::Error),

    #[error("协议错误: {0}")]
    Protocol(String),
}

// ✓ 正确：错误向上传播
pub fn do_something() -> Result<(), QvodError> {
    let data = read_file()?;  // io::Error 自动转为 Network
    validate(&data).map_err(|e| QvodError::Protocol(e.to_string()))
}

// ✗ 错误：使用 String 作为错误
pub fn bad() -> Result<(), String> { ... }

// ✗ 错误：使用 unwrap/expect
let x = risky_call().unwrap();
```

### 日志规范

```rust
// 级别使用规则:
//   error! - 不可恢复的错误（连接失败、协议错误）
//   warn!  - 可恢复的异常（某 peer 超时、重试）
//   info!  - 重要状态变更（启动、停止、播放开始）
//   debug! - 调试信息（消息收发、调度决策）
//   trace! - 详细跟踪（数据包内容）

// ✓ 正确
fn handle_message(msg: &PeerMessage) {
    debug!(?msg.peer_id, msg_type = ?msg.msg_id, "收到消息");
}

// ✓ 正确：带 span
#[tracing::instrument(skip(stream))]
async fn read_packet(stream: &mut TcpStream) -> Result<Packet> { ... }
```

### 异步模式

```rust
// ✓ 正确：使用 tokio
use tokio::net::TcpStream;
use tokio::sync::mpsc;

// ✓ 正确：异步 trait
#[async_trait]
pub trait DhtEngine: Send + Sync {
    async fn find_peers(&self, info_hash: &InfoHash) -> Result<Vec<PeerInfo>>;
}

// ✗ 错误：阻塞调用
pub fn find_peers(&self, hash: &InfoHash) -> Result<Vec<PeerInfo>> {
    let stream = TcpStream::connect(addr)?;  // 阻塞！
    ...
}
```

### 跨 crate 依赖

- `qvs-core` 是唯一无依赖的根 crate
- 其他 crate 只能依赖 `qvs-core` 和比其"下层"的 crate
- 禁止循环依赖
- 依赖图：`qvs-core ← qvs-format ← {qvs-dht, qvs-tracker} ← qvs-transport ← qvs-stream ← {qvs-local-server, qvs-gui}`

### 线程安全

- 共享状态使用 `Arc<Mutex<T>>` 或 `Arc<RwLock<T>>`
- 事件驱动通信使用 `tokio::sync::mpsc` / `tokio::sync::broadcast`
- 无共享状态的数据传递使用通道 (channel)
- 避免使用 `std::sync::Mutex`（除非在非异步上下文）

## 测试规范

### 单元测试

```rust
// ✓ 正确：每个模块有测试模块
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(PiecePriority::Critical > PiecePriority::High);
    }

    #[test]
    fn test_bitfield_set_and_get() {
        let mut bf = Bitfield::new(100);
        bf.set(50, true);
        assert!(bf.has(50));
    }
}
```

### 测试要求

- 每个 crate 单元测试覆盖率 > 80%
- 协议编解码必须做往返测试 (roundtrip)
- Bencode 编解码必须测试嵌套结构
- 网络模块使用 mock 测试，不依赖真实网络
- 集成测试使用本地回环地址 (127.0.0.1)
- DHT 路由表测试验证分裂规则
- RingBuffer 测试验证边界条件（空、满、环绕）

### Mock 策略

```rust
// ✓ 正确：使用 trait 进行 mock 测试
#[cfg(test)]
mock! {
    pub DhtEngineMock;
    impl DhtEngine for DhtEngineMock { ... }
}

// ✓ 正确：或在测试中手动实现 trait
struct MockTracker;
impl TrackerClient for MockTracker { ... }
```

## Git 规范

### 分支策略
- `main` — 稳定分支，保护模式
- `feat/{crate-name}` — 功能分支
- `fix/{description}` — 修复分支

### 提交信息格式
```
<type>(<crate>): <简短描述>

<详细描述（可选）>
```

### 类型
- `feat` — 新功能
- `fix` — 修复
- `refactor` — 重构（不改变外部行为）
- `docs` — 文档
- `test` — 测试
- `chore` — 杂项（构建、CI、依赖）

### 提交前检查
- [ ] `cargo build` 通过
- [ ] `cargo test` 通过
- [ ] `cargo clippy -- -D warnings` 无警告
- [ ] `cargo fmt` 格式化通过
- [ ] 涉及协议变更时更新了文档
- [ ] 无硬编码常量值（使用 `constants.rs`）
- [ ] 日志覆盖了新增的关键路径

## 代码审查标准

### 强制检查项
- [ ] 所有公共 API 有文档注释
- [ ] 单元测试覆盖率 > 80%
- [ ] 协议编解码有往返测试
- [ ] clippy 无警告
- [ ] cargo fmt 通过
- [ ] 无 unsafe 代码（除非批准）
- [ ] 所有 Result 返回 QvodError 类型
- [ ] 网络模块使用 mock 测试
- [ ] 无硬编码常量
- [ ] 日志覆盖关键路径
- [ ] 异步代码正确处理错误（不使用 unwrap）

## 跨平台注意事项

### 路径处理
- 使用 `std::path::PathBuf`，不拼接字符串
- 缓存目录: Linux `~/.local/share/qvs/`, macOS `~/Library/Caches/qvs/`, Windows `%APPDATA%/qvs/`
- 配置目录: Linux `~/.config/qvs/`, macOS `~/Library/Preferences/qvs/`, Windows `%APPDATA%/qvs/`

### 网络
- 使用 `SocketAddr` 而非组合 IP + Port 字符串
- IPv6 兼容（使用 `std::net::ToSocketAddrs`）
- Windows 注意 `WinSock` 初始化（tokio 自动处理）

### 稀疏文件
- Linux: `fallocate()` with `FALLOC_FL_KEEP_SIZE`
- macOS: `ftruncate()` / `lseek` + write
- Windows: `SetFileValidData()`

### 协议处理器注册
- Linux: `xdg-mime` + `xdg-open`
- macOS: `CFBundleURLTypes` in Info.plist
- Windows: Registry `HKEY_CLASSES_ROOT\qvod`

## 构建验证命令

```bash
# 完整构建
cargo build --workspace

# 运行所有测试
cargo test --workspace

# Clippy 检查
cargo clippy --workspace -- -D warnings

# 格式化
cargo fmt --check

# 文档构建（不构建依赖）
cargo doc --no-deps

# 测试覆盖率
cargo llvm-cov --all-features --workspace --html

# 安全审计
cargo audit
```

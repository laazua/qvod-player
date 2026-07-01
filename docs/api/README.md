# QVOD API Reference

## 1. Overview

QVOD exposes three API surfaces:

| Layer | API | Consumers |
|-------|-----|-----------|
| Layer 1 | HTTP REST API (local server) | Web browsers, mobile HLS players |
| Layer 2-4 | Internal Rust traits & structs | Crate-to-crate communication |
| Rust | Library entry points | GUI, CLI, programmatic usage |

---

## 2. Local HTTP Server API (Layer 1)

Base URL: `http://127.0.0.1:{port}/` (default port: 8621)

### 2.1 Play Endpoint

Serves stream data as HTTP chunked transfer encoding.

```
GET /play?hash={info_hash_hex}&name={filename_urlencoded}&size={filesize}
```

**Parameters:**

| Parameter | Required | Type | Description |
|-----------|----------|------|-------------|
| `hash` | Yes | hex string (40) | info_hash of the content |
| `name` | No | URL-encoded string | Display filename |
| `size` | No | integer | Total file size in bytes |
| `fmt` | No | string | Format hint (rmvb, avi, mp4, etc.) |
| `offset` | No | integer | Byte offset for range seek (for later requests) |

**Responses:**

```
200 OK
Content-Type: video/octet-stream
Transfer-Encoding: chunked
Cache-Control: no-cache
Connection: keep-alive
X-QVOD-Peers: 12
X-QVOD-Speed: 1245678
X-QVOD-Buffered: 45.2
X-QVOD-Format: rmvb
X-QVOD-FileSize: 734003200
X-QVOD-Duration: 4567
```

```
206 Partial Content (when offset is specified)
Content-Range: bytes {start}-{end}/{total}
Content-Type: video/octet-stream
```

```
404 Not Found — resource not found (invalid info_hash)
503 Service Unavailable — P2P engine not ready
416 Range Not Satisfiable — invalid offset
```

### 2.2 Status Endpoint

Returns current download/playback status as JSON.

```
GET /status?hash={info_hash_hex}
```

**Response (200 OK):**

```json
{
    "hash": "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9",
    "name": "movie.rmvb",
    "file_size": 734003200,
    "downloaded": 52428800,
    "completion": 0.0714,
    "peers": 12,
    "seeders": 3,
    "leechers": 9,
    "speed_down": 1245678,
    "speed_up": 234567,
    "buffered_bytes": 31457280,
    "buffered_duration_sec": 31.5,
    "state": "playing",
    "position_sec": 45.2,
    "duration_sec": 4567.0,
    "bitrate": 1234567,
    "video_codec": "RV40",
    "audio_codec": "COOK",
    "resolution": "1280x720",
    "rtt_ms": 45,
    "loss_rate": 0.02,
    "dht_nodes": 128
}
```

### 2.3 Segment Endpoint

Used for pseudo-HLS playback.

```
GET /segment?hash={info_hash_hex}&offset={u64}&length={u64}
```

**Parameters:**

| Parameter | Required | Type | Description |
|-----------|----------|------|-------------|
| `hash` | Yes | hex string (40) | info_hash |
| `offset` | Yes | integer | Byte offset for segment start |
| `length` | Yes | integer | Segment length in bytes |

**Responses:**

```
200 OK
Content-Type: video/MP2T
Content-Length: {length}
Cache-Control: public, max-age=3600
```

```
206 Partial Content (if offset+length covers partially available data)
416 Range Not Satisfiable
503 Service Unavailable
```

### 2.4 M3U8 Playlist Endpoint

Returns a dynamically generated HLS playlist for mobile browsers.

```
GET /playlist.m3u8?hash={info_hash_hex}
```

**Response (200 OK):**

```
Content-Type: application/vnd.apple.mpegurl

#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:VOD
#EXTINF:10.000000,
/segment?hash=a1b2...&offset=0&length=262144
#EXTINF:8.500000,
/segment?hash=a1b2...&offset=262144&length=262144
...
#EXT-X-ENDLIST
```

### 2.5 Control Endpoint

```
POST /control?hash={info_hash_hex}&action={action}
```

**Actions:**

| Action | Description |
|--------|-------------|
| `pause` | Pause download (not playback) |
| `resume` | Resume download |
| `stop` | Stop and disconnect all peers |
| `prioritize` | Increase priority of this stream |

**Response (200 OK):**
```json
{ "status": "ok", "action": "pause" }
```

### 2.6 Server Info Endpoint

```
GET /
```

**Response:**
```json
{
    "name": "QVOD Stream Server",
    "version": "0.1.0",
    "port": 8621,
    "uptime_sec": 3600,
    "active_streams": 2,
    "total_peers": 34,
    "cache_size": 2147483648,
    "cache_max": 4294967296,
    "dht_nodes": 128
}
```

---

## 3. Internal Rust API Surfaces

### 3.1 qvs-core Types

```rust
// === Core Types ===

/// 20-byte SHA-1 info hash
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct InfoHash(pub [u8; 20]);

/// 20-byte Kademlia node ID
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub [u8; 20]);

/// Peer connection information
#[derive(Clone)]
pub struct PeerInfo {
    pub peer_id: [u8; 20],
    pub addr: SocketAddr,
    pub is_firewalled: bool,
    pub bw_up: u32,
    pub bw_down: u32,
    pub location: Option<String>,
}

/// Piece completion bitfield
#[derive(Clone)]
pub struct Bitfield { /* ... */ }

/// Piece priority for scheduling
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PiecePriority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

/// A single piece info
pub struct PieceInfo {
    pub index: u32,
    pub hash: [u8; 20],
    pub length: u64,
    pub priority: PiecePriority,
}

/// Block request to a peer
pub struct BlockRequest {
    pub piece_index: u32,
    pub begin: u32,
    pub length: u32,
}

/// Key frame entry for seek/scheduling
pub struct KeyFrameEntry {
    pub timestamp_ms: u64,
    pub file_offset: u64,
    pub frame_size: u32,
    pub frame_type: FrameType,
}

pub enum FrameType { I = 0, P = 1, B = 2 }

/// File metadata (obtained via ut_metadata or .qvs)
pub struct FileMeta {
    pub info_hash: InfoHash,
    pub filename: String,
    pub file_size: u64,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub keyframe_index: KeyFrameIndex,
    pub duration_ms: u64,
    pub video_codec: String,
    pub audio_codec: String,
    pub width: u32,
    pub height: u32,
    pub bitrate: u32,
}

/// Media stream returned to local-server or GUI
pub struct MediaStream {
    pub info_hash: InfoHash,
    pub metadata: Arc<FileMeta>,
    pub buffer: Arc<RwLock<RingBuffer>>,
    pub state: StreamState,
}

pub enum StreamState {
    Idle,
    Buffering,
    Playing,
    Paused,
    Seeking,
    Error(String),
}
```

### 3.2 Trait Definitions

**DhtEngine:**

```rust
#[async_trait]
pub trait DhtEngine: Send + Sync {
    /// Bootstrap the DHT node, connecting to seed nodes
    async fn bootstrap(&self, seed_nodes: &[SocketAddr]) -> Result<()>;

    /// Find peers for a given info_hash. Returns a receiver for streaming results
    async fn find_peers(&self, info_hash: &InfoHash) -> Result<Receiver<PeerInfo>>;

    /// Announce that this node has data for an info_hash
    async fn announce(&self, info_hash: &InfoHash, port: u16) -> Result<()>;

    /// Get local node ID
    fn local_id(&self) -> &NodeId;

    /// Get DHT statistics
    fn stats(&self) -> DhtStats;
}

#[derive(Default)]
pub struct DhtStats {
    pub total_peers_found: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub routing_table_size: usize,
    pub node_id: Option<NodeId>,
}
```

**Transport:**

```rust
#[async_trait]
pub trait Transport: Send + Sync {
    /// Connect to a peer
    async fn connect(&self, peer: &PeerInfo) -> Result<PeerConnectionHandle>;

    /// Disconnect from a peer
    async fn disconnect(&self, peer_id: &[u8; 20]) -> Result<()>;

    /// Send a block request to a peer
    async fn send_request(&self, peer_id: &[u8; 20], request: BlockRequest) -> Result<()>;

    /// Send a piece (block data) to a peer
    async fn send_piece(&self, peer_id: &[u8; 20], piece_index: u32, begin: u32, data: &[u8]) -> Result<()>;

    /// Get transport statistics
    fn stats(&self) -> TransportStats;
}

pub struct TransportStats {
    pub active_connections: u32,
    pub pending_requests: u32,
    pub speed_down_bps: u64,
    pub speed_up_bps: u64,
    pub avg_rtt_ms: f64,
    pub loss_rate: f64,
}
```

**CacheBackend:**

```rust
#[async_trait]
pub trait CacheBackend: Send + Sync {
    async fn find(&self, info_hash: &InfoHash) -> Result<Option<CacheEntry>>;
    async fn read(&self, info_hash: &InfoHash, offset: u64, length: u64) -> Result<Vec<u8>>;
    async fn write(&self, info_hash: &InfoHash, offset: u64, data: &[u8]) -> Result<()>;
    async fn completion(&self, info_hash: &InfoHash) -> Result<f64>;
    async fn cleanup(&self) -> Result<CleanupReport>;
    fn total_size(&self) -> Result<u64>;
}

pub struct CacheEntry {
    pub info_hash: InfoHash,
    pub file_size: u64,
    pub downloaded: u64,
    pub bitfield: Bitfield,
    pub last_access: Instant,
}
```

**MetadataResolver:**

```rust
#[async_trait]
pub trait MetadataResolver: Send + Sync {
    /// Resolve metadata for an info_hash from peers
    async fn resolve_metadata(&self, info_hash: &InfoHash) -> Result<FileMeta>;

    /// Resolve metadata from cache
    fn resolve_from_cache(&self, info_hash: &InfoHash) -> Result<Option<FileMeta>>;

    /// Cache metadata for later use
    async fn cache_metadata(&self, info_hash: &InfoHash, meta: &FileMeta) -> Result<()>;
}
```

### 3.3 qvs-stream Public API

```rust
/// Main streaming engine
pub struct QvodEngine {
    // Private fields
}

impl QvodEngine {
    /// Create a new engine with given configuration
    pub fn new(config: EngineConfig) -> Self;

    /// Start all services (local server, DHT, etc.)
    pub async fn start(&self) -> Result<()>;

    /// Begin playback of a qvod:// URI
    pub async fn play(&mut self, uri: &QvodUri) -> Result<MediaStream>;

    /// Pause download for a stream
    pub async fn pause(&self, info_hash: &InfoHash) -> Result<()>;

    /// Resume download
    pub async fn resume(&self, info_hash: &InfoHash) -> Result<()>;

    /// Stop and clean up a stream
    pub async fn stop(&self, info_hash: &InfoHash) -> Result<()>;

    /// Seek to a timestamp (ms) in an active stream
    pub async fn seek(&self, info_hash: &InfoHash, timestamp_ms: u64) -> Result<()>;

    /// Get current stream status
    pub fn status(&self, info_hash: &InfoHash) -> Result<StreamStatus>;

    /// Shutdown the engine and all services
    pub async fn shutdown(self) -> Result<()>;
}

pub struct StreamStatus {
    pub state: StreamState,
    pub position_sec: f64,
    pub duration_sec: f64,
    pub buffered_sec: f64,
    pub completion: f64,
    pub peers: u32,
    pub speed_down_bps: u64,
    pub speed_up_bps: u64,
}

#[derive(Clone)]
pub struct EngineConfig {
    pub listen_port: u16,
    pub udp_port: u16,
    pub max_connections: u32,
    pub buffer_capacity_mb: u32,
    pub cache_dir: PathBuf,
    pub tracker_urls: Vec<String>,
    pub dht_seed_nodes: Vec<SocketAddr>,
    pub http_fallback: bool,
    pub http_sources: Vec<String>,
}
```

### 3.4 qvs-local-server Public API

```rust
pub struct LocalServer {
    // Private fields
}

impl LocalServer {
    /// Start the HTTP server on an available port
    pub async fn start(config: LocalServerConfig) -> Result<Self>;

    /// Start with an existing engine reference
    pub async fn start_with_engine(config: LocalServerConfig, engine: Arc<QvodEngine>) -> Result<Self>;

    /// Stop the HTTP server gracefully
    pub async fn stop(self);

    /// Get the actual bound port
    pub fn port(&self) -> u16;

    /// Get server statistics
    pub fn stats(&self) -> ServerStats;
}

pub struct LocalServerConfig {
    pub preferred_port: u16,
    pub max_retry: u8,
}

pub struct ServerStats {
    pub uptime_sec: u64,
    pub active_streams: u32,
    pub total_requests: u64,
    pub bytes_served: u64,
}
```

### 3.5 qvs-transport Public API

```rust
pub struct ConnectionPool {
    // Private fields
}

impl ConnectionPool {
    pub fn new(config: TransportConfig) -> Self;
    pub async fn add_peer(&self, peer: PeerInfo) -> Result<PeerConnectionHandle>;
    pub async fn remove_peer(&self, peer_id: &[u8; 20]) -> Result<()>;
    pub fn select_upload_peers(&self, count: u32) -> Vec<PeerConnectionHandle>;
    pub fn select_download_peers(&self, count: u32, priority: PiecePriority) -> Vec<PeerConnectionHandle>;
    pub fn stats(&self) -> PoolStats;
    pub async fn cleanup_idle(&self);
}

pub struct PeerConnectionHandle {
    pub peer_id: [u8; 20],
    pub addr: SocketAddr,
    pub state: ConnectionState,
    // Send channel for messages
    tx: mpsc::Sender<PeerMessage>,
}

pub enum ConnectionState {
    Disconnected,
    Connecting,
    Handshaking,
    Established,
    Disconnecting,
}

pub struct PoolStats {
    pub total_connections: u32,
    pub active_connections: u32,
    pub pending_requests: u32,
    pub speed_down_bps: u64,
    pub speed_up_bps: u64,
}

pub struct P2spDownloader {
    // Private fields
}

impl P2spDownloader {
    pub fn new(pool: Arc<ConnectionPool>, http_sources: Vec<String>) -> Self;
    pub fn select_source(&self, piece: &PieceInfo, priority: PiecePriority) -> Source;
    pub async fn download_critical(&self, piece: &PieceInfo) -> Result<Vec<u8>>;
    pub async fn download_high(&self, piece: &PieceInfo) -> Result<Vec<u8>>;
    pub async fn download_normal(&self, piece: &PieceInfo) -> Result<Vec<u8>>;
    pub async fn download_low(&self, piece: &PieceInfo) -> Result<Vec<u8>>;
}

pub enum Source {
    Parallel { p2p: bool, http: bool },
    P2pWithHttpFallback { timeout: Duration },
    P2pOnly,
    P2pIdle,
}
```

### 3.6 qvs-format Public API

```rust
// === URI ===
impl QvodUri {
    pub fn from_str(s: &str) -> Result<Self>;
    pub fn to_string(&self) -> String;
    pub fn info_hash(&self) -> &InfoHash;
    pub fn filename(&self) -> &str;
    pub fn file_size(&self) -> u64;
    pub fn format(&self) -> Option<&str>;
}

// === Bencode ===
pub enum BencodeValue {
    Int(i64),
    Str(Vec<u8>),
    List(Vec<BencodeValue>),
    Dict(BTreeMap<String, BencodeValue>),
}

impl BencodeValue {
    pub fn encode(&self) -> Vec<u8>;
    pub fn decode(data: &[u8]) -> Result<(Self, &[u8])>;
    pub fn as_int(&self) -> Option<i64>;
    pub fn as_str(&self) -> Option<&str>;
    pub fn as_bytes(&self) -> Option<&[u8]>;
    pub fn as_list(&self) -> Option<&[BencodeValue]>;
    pub fn as_dict(&self) -> Option<&BTreeMap<String, BencodeValue>>;
    pub fn into_dict(self) -> Option<BTreeMap<String, BencodeValue>>;
}

// === .qvs File ===
impl QvsFile {
    pub fn encode(&self) -> Result<Vec<u8>, BencodeError>;
    pub fn decode(data: &[u8]) -> Result<Self, BencodeError>;
}

// === Cache ===
impl CacheManager {
    pub fn new(config: CacheConfig) -> Self;
    pub fn find(&self, info_hash: &InfoHash) -> Result<Option<CacheEntry>>;
    pub async fn read(&self, info_hash: &InfoHash, offset: u64, length: u64) -> Result<Vec<u8>>;
    pub async fn write(&self, info_hash: &InfoHash, offset: u64, data: &[u8]) -> Result<()>;
    pub async fn write_piece(&self, info_hash: &InfoHash, index: u32, data: &[u8]) -> Result<()>;
    pub fn completion(&self, info_hash: &InfoHash) -> Result<f64>;
    pub async fn cleanup(&self) -> Result<CleanupReport>;
    pub async fn delete(&self, info_hash: &InfoHash) -> Result<()>;
    pub fn list(&self) -> Result<Vec<CacheEntry>>;
    pub fn total_size(&self) -> Result<u64>;
    pub fn max_size(&self) -> u64;
    pub fn set_max_size(&self, max_bytes: u64);
}
```

### 3.7 qvs-dht Public API

```rust
impl DhtNode {
    pub fn new(config: DhtConfig) -> Self;
    pub async fn start(&self) -> Result<()>;
    pub async fn stop(&self);
    pub fn local_id(&self) -> &NodeId;
    pub fn routing_table_snapshot(&self) -> RoutingTableSnapshot;
}

impl RoutingTable {
    pub fn new(local_id: NodeId) -> Self;
    pub fn insert(&mut self, entry: KBucketEntry) -> InsertResult;
    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<KBucketEntry>;
    pub fn refresh_list(&self) -> Vec<usize>;
    pub fn size(&self) -> usize;
}

pub struct RoutingTableSnapshot {
    pub bucket_count: usize,
    pub total_nodes: usize,
    pub buckets: Vec<BucketInfo>,
}
```

---

## 4. Example API Usage Patterns

### 4.1 Basic Playback (CLI style)

```rust
use qvs_core::*;
use qvs_stream::*;
use qvs_format::*;

async fn play_video(uri_str: &str) -> Result<()> {
    // 1. Parse URI
    let uri = QvodUri::from_str(uri_str)?;
    println!("Playing: {} ({})", uri.filename(), uri.info_hash());

    // 2. Create engine
    let config = EngineConfig::default();
    let mut engine = QvodEngine::new(config);
    engine.start().await?;

    // 3. Start playback
    let mut stream = engine.play(&uri).await?;

    // 4. Stream data to whatever needs it
    let buf = stream.buffer.read(0, 8192)?;

    // 5. Wait for completion or user stop
    engine.stop(uri.info_hash()).await?;

    Ok(())
}
```

### 4.2 Local Server Integration

```rust
use qvs_local_server::*;
use qvs_stream::*;

async fn serve() -> Result<()> {
    let engine = Arc::new(QvodEngine::new(EngineConfig::default()));
    engine.start().await?;

    let server = LocalServer::start_with_engine(
        LocalServerConfig { preferred_port: 8621, max_retry: 5 },
        engine,
    ).await?;

    println!("QVOD server running on port {}", server.port());
    // Block until Ctrl+C
    tokio::signal::ctrl_c().await?;
    server.stop().await;
    Ok(())
}
```

### 4.3 Custom Cache Backend

```rust
use qvs_core::CacheBackend;

struct MyCustomCache {
    inner: Arc<MyStorage>,
}

#[async_trait]
impl CacheBackend for MyCustomCache {
    async fn find(&self, info_hash: &InfoHash) -> Result<Option<CacheEntry>> {
        self.inner.lookup(info_hash)
    }

    async fn read(&self, info_hash: &InfoHash, offset: u64, length: u64) -> Result<Vec<u8>> {
        self.inner.read_range(info_hash, offset, length)
    }

    async fn write(&self, info_hash: &InfoHash, offset: u64, data: &[u8]) -> Result<()> {
        self.inner.write_range(info_hash, offset, data)
    }

    async fn completion(&self, info_hash: &InfoHash) -> Result<f64> {
        self.inner.get_completion(info_hash)
    }

    async fn cleanup(&self) -> Result<CleanupReport> {
        self.inner.evict_lru()
    }

    fn total_size(&self) -> Result<u64> {
        self.inner.total_bytes()
    }
}
```

### 4.4 Peer Discovery with DHT

```rust
use qvs_dht::*;
use qvs_core::*;

async fn discover_peers(info_hash: &InfoHash) -> Result<Vec<PeerInfo>> {
    let config = DhtConfig {
        listen_port: 8621,
        seed_nodes: vec![
            "192.168.1.100:8621".parse()?,
        ],
        ..Default::default()
    };

    let dht = DhtNode::new(config);
    dht.start().await?;

    let mut peers_rx = dht.find_peers(info_hash).await?;
    let mut peers = Vec::new();

    while let Some(peer) = peers_rx.recv().await {
        peers.push(peer);
    }

    Ok(peers)
}
```

---

## 5. Stats JSON Schema

### 5.1 Stream Status Response

```json
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "title": "QVOD Stream Status",
    "type": "object",
    "properties": {
        "hash":              { "type": "string", "pattern": "^[0-9a-f]{40}$" },
        "name":              { "type": "string" },
        "file_size":         { "type": "integer", "minimum": 0 },
        "downloaded":        { "type": "integer", "minimum": 0 },
        "completion":        { "type": "number", "minimum": 0, "maximum": 1 },
        "peers":             { "type": "integer", "minimum": 0 },
        "seeders":           { "type": "integer", "minimum": 0 },
        "leechers":          { "type": "integer", "minimum": 0 },
        "speed_down":        { "type": "integer", "minimum": 0 },
        "speed_up":          { "type": "integer", "minimum": 0 },
        "buffered_bytes":    { "type": "integer", "minimum": 0 },
        "buffered_duration_sec": { "type": "number", "minimum": 0 },
        "state":             { "type": "string", "enum": ["idle", "buffering", "playing", "paused", "seeking", "error"] },
        "position_sec":      { "type": "number", "minimum": 0 },
        "duration_sec":      { "type": "number", "minimum": 0 },
        "bitrate":           { "type": "integer", "minimum": 0 },
        "video_codec":       { "type": "string" },
        "audio_codec":       { "type": "string" },
        "resolution":        { "type": "string", "pattern": "^\\d+x\\d+$" },
        "rtt_ms":            { "type": "number", "minimum": 0 },
        "loss_rate":         { "type": "number", "minimum": 0, "maximum": 1 },
        "dht_nodes":         { "type": "integer", "minimum": 0 }
    },
    "required": ["hash", "state"]
}
```

### 5.2 Server Info Response

```json
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "title": "QVOD Server Info",
    "type": "object",
    "properties": {
        "name":           { "type": "string" },
        "version":        { "type": "string" },
        "port":           { "type": "integer", "minimum": 1, "maximum": 65535 },
        "uptime_sec":     { "type": "integer", "minimum": 0 },
        "active_streams": { "type": "integer", "minimum": 0 },
        "total_peers":    { "type": "integer", "minimum": 0 },
        "cache_size":     { "type": "integer", "minimum": 0 },
        "cache_max":      { "type": "integer", "minimum": 0 },
        "dht_nodes":      { "type": "integer", "minimum": 0 }
    },
    "required": ["name", "version", "port"]
}
```

---

## 6. Error Type

```rust
#[derive(Debug, thiserror::Error)]
pub enum QvodError {
    #[error("network error: {0}")]
    Network(#[from] io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("metadata parse failed")]
    MetadataParse,
    #[error("DHT timeout")]
    DhtTimeout,
    #[error("DHT routing failed")]
    DhtRoutingFailed,
    #[error("tracker timeout")]
    TrackerTimeout,
    #[error("tracker protocol error: {0}")]
    TrackerProtocol(String),
    #[error("resource not found: {0}")]
    ResourceNotFound(InfoHash),
    #[error("no peers available")]
    NoPeers,
    #[error("NAT traversal failed")]
    NatFailed,
    #[error("cache full")]
    CacheFull,
    #[error("cache corrupted: {0}")]
    CacheCorrupted(String),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("invalid URI: {0}")]
    InvalidUri(String),
    #[error("Bencode error: {0}")]
    Bencode(String),
    #[error("piece verification failed at {index}: expected {expected}, got {got}")]
    PieceVerificationFailed { index: u32, expected: [u8; 20], got: [u8; 20] },
    #[error("connection limit reached")]
    ConnectionLimitReached,
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("cancelled")]
    Cancelled,
}
```

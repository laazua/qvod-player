# Tracker Module Specification

## Overview

The Tracker module provides centralized peer discovery for the QVOD P2SP network. Unlike pure P2P systems that rely solely on DHT, QVOD uses a **hybrid approach**: a central HTTP Tracker as the primary discovery mechanism with DHT as fallback. The Tracker maintains an index of all active peers for each resource identified by `info_hash`, and responds to client announcements with peer lists.

The module is split into two components:
- **TrackerClient** — embedded in every QVOD client, communicates with remote tracker servers
- **TrackerServer** — optional standalone server for hosting your own tracker

---

## 1. HTTP Tracker Protocol

### 1.1 Announce Request

Clients announce their presence to the tracker via HTTP GET requests:

```
GET /announce?info_hash=<hex>&peer_id=<hex>&port=<u16>
    &uploaded=<u64>&downloaded=<u64>&left=<u64>
    &event=<started|completed|stopped|empty>&compact=1
    &numwant=<u32>&key=<hex>&corrupt=<u32>
```

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `info_hash` | hex string (40 chars) | Yes | 20-byte SHA-1 hash identifying the resource |
| `peer_id` | hex string (40 chars) | Yes | 20-byte client ID, unique per client instance |
| `port` | uint16 | Yes | TCP port the client is listening on for peer connections |
| `uploaded` | uint64 | Yes | Total bytes uploaded so far |
| `downloaded` | uint64 | Yes | Total bytes downloaded so far |
| `left` | uint64 | Yes | Bytes remaining to download (0 if completed) |
| `event` | enum string | No | `started`, `completed`, `stopped`, or empty for periodic |
| `compact` | uint8 | No | Set to `1` to request compact peer response format |
| `numwant` | uint32 | No | Maximum number of peers wanted (default: 50, max: 200) |
| `key` | hex string (8 chars) | No | Unique key for client identification behind NAT |
| `corrupt` | uint32 | No | Number of corrupt pieces reported by client |

### 1.2 Announce Response

The tracker responds with a Bencode-encoded dictionary:

```python
d
8:interval i1800e
12:min_interval i900e
8:complete i42e
10:incomplete i17e
8:downloaded i156e
5:peers l
  d
   2:ip 7:1.2.3.4
   4:port i8621e
   7:peer_id 20:<binary>
   12:is_firewalled i1e
   7:bw_up i5120e
   9:bw_down i10240e
  e
  ...
e
5:flags i1e
e
```

#### Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `interval` | integer | Seconds to wait before next regular announce (default: 1800) |
| `min_interval` | integer | Minimum seconds between announces (default: 900) |
| `complete` | integer | Number of peers with complete file (seeders) |
| `incomplete` | integer | Number of peers still downloading (leechers) |
| `downloaded` | integer | Total number of times this resource has been downloaded |
| `peers` | list or string | Peer list (dictionary model if compact=0, binary if compact=1) |
| `flags` | integer | Bit flags for tracker capabilities (bit 0: TLS supported) |

#### Compact Peer Format

When `compact=1` is specified in the request, the `peers` field is a binary string:

```
b"peers" 6:<binary data>

Each peer entry = 6 bytes:
  IP address:     4 bytes (network byte order)
  Port:           2 bytes (network byte order)

Example:
  0xC0 0xA8 0x01 0x01 0x21 0xAB  = 192.168.1.1:8619
```

Compact peer format returns only IP:port pairs, no peer_id or metadata. For extended peer info, use the dictionary format or make a separate scrape request.

### 1.3 Failure Response

```python
d
14:failure reason 25:Resource not found or banned
e
```

| Field | Type | Description |
|-------|------|-------------|
| `failure reason` | string | Human-readable error message |

If the tracker returns a `failure reason`, the client must treat it as a permanent error for that tracker and stop announcing.

### 1.4 Warning Response

```python
d
14:warning message 42:Your IP is rate limited, reducing peers
5:peers l e
e
```

| Field | Type | Description |
|-------|------|-------------|
| `warning message` | string | Non-fatal warning (client should log but continue) |

---

## 2. TrackerClient Rust Struct Design

```rust
/// HTTP Tracker client for peer discovery
pub struct TrackerClient {
    /// Ordered list of tracker URLs, tried in sequence
    tracker_urls: Vec<String>,
    /// Index of currently active tracker
    current_tracker: usize,
    /// 20-byte peer ID unique to this client instance
    peer_id: [u8; 20],
    /// Key for NAT-identified clients (random 4-byte hex)
    key: String,
    /// HTTP client with connection pooling
    http_client: reqwest::Client,
    /// Configuration
    config: TrackerConfig,
}

#[derive(Clone)]
pub struct TrackerConfig {
    /// Connection timeout per tracker (default: 15s)
    pub connect_timeout: Duration,
    /// Read timeout for responses (default: 30s)
    pub read_timeout: Duration,
    /// Max peers to request per announce (default: 50)
    pub num_want: u32,
    /// Whether to use compact peer format (default: true)
    pub compact: bool,
    /// Max retries per tracker before marking it dead (default: 3)
    pub max_retries_per_tracker: u32,
    /// Backoff multiplier on failure (default: 2.0)
    pub backoff_factor: f64,
    /// Whether to parallelize requests to all trackers
    pub parallel_announce: bool,
}

impl TrackerClient {
    /// Create a new TrackerClient with the given tracker URLs.
    /// Generates a random 20-byte peer_id and 8-char hex key.
    pub fn new(tracker_urls: Vec<String>) -> Self;

    /// Announce to the active tracker.
    /// Returns the list of peers, swarm stats, and the suggested interval.
    pub fn announce(
        &self,
        info_hash: &InfoHash,
        event: AnnounceEvent,
        port: u16,
        uploaded: u64,
        downloaded: u64,
        left: u64,
    ) -> Result<AnnounceResponse>;

    /// Announce to multiple trackers in parallel.
    /// Merges peer lists, deduplicating by IP:port.
    /// Returns the union of all responses.
    pub fn announce_all(
        &self,
        info_hash: &InfoHash,
        event: AnnounceEvent,
        port: u16,
        stats: DownloadStats,
    ) -> Result<Vec<PeerInfo>>;

    /// Scrape swarm status for one or more info_hashes.
    pub fn scrape(&self, info_hashes: &[InfoHash]) -> Result<Vec<SwarmStatus>>;

    /// Get scrape URL from an announce URL by replacing "announce" with "scrape".
    fn derive_scrape_url(&self, announce_url: &str) -> Option<String>;

    /// Rotate to the next tracker URL in the list.
    pub fn rotate_tracker(&mut self);

    /// Mark the current tracker as failed and rotate.
    pub fn report_failure(&mut self);

    /// Reset failure count for all trackers.
    pub fn reset(&mut self);

    /// Current active tracker URL.
    pub fn current_url(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct AnnounceResponse {
    /// Suggested interval before next announce
    pub interval: Duration,
    /// Minimum interval between announces
    pub min_interval: Duration,
    /// Peer list received from tracker
    pub peers: Vec<PeerInfo>,
    /// Number of seeders (complete peers)
    pub complete: u32,
    /// Number of leechers (incomplete peers)
    pub incomplete: u32,
    /// Total download count for this resource
    pub downloaded: u32,
    /// Which tracker index this response came from
    pub tracker_index: usize,
    /// Warning message from tracker (if any)
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: [u8; 20],
    pub addr: SocketAddr,
    pub is_firewalled: bool,
    pub bw_up: u32,
    pub bw_down: u32,
    pub location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DownloadStats {
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub corrupt: u32,
}
```

### Peer ID Generation

```rust
impl TrackerClient {
    pub fn generate_peer_id() -> [u8; 20] {
        let mut id = [0u8; 20];
        // Prefix: "-QV0001-" = QVOD v0.0.1
        id[..8].copy_from_slice(b"-QV0001-");
        // Suffix: 12 random bytes
        thread_rng().fill_bytes(&mut id[8..]);
        id
    }
}
```

Peer ID convention: `-QVxxxx-` followed by 12 random bytes, where `xxxx` is the version number. This allows tracker operators to identify client versions in logs.

---

## 3. Announce Events

```rust
pub enum AnnounceEvent {
    /// Client has started downloading the resource.
    /// Sent once when a new download begins.
    Started,
    /// Client has completed downloading the resource.
    /// Sent once when all data has been received.
    Completed,
    /// Client has stopped downloading (user action or shutdown).
    /// Sent once to remove the peer from the swarm.
    Stopped,
    /// Periodic keep-alive update.
    /// No event parameter in the URL; omits the event field entirely.
    Empty,
}
```

### Event Behavior Matrix

| Event | Tracker Action | Client Action After |
|-------|---------------|-------------------|
| `started` | Add peer to swarm, increment `incomplete` count | Begin periodic announces |
| `completed` | Mark peer as seeder, decrement `incomplete`, increment `complete` | Continue uploading (seeding) |
| `stopped` | Remove peer from swarm, decrement appropriate count | Close connections, clean up |
| `empty` | Refresh peer's timeout in swarm | Schedule next announce at `interval` |

### State Machine

```
                    ┌─────────────┐
                    │   IDLE      │
                    └──────┬──────┘
                           │ announce(started)
                           ▼
                    ┌─────────────┐
              ┌─────┤ DOWNLOADING ├─────┐
              │     └─────────────┘     │
              │ announce(completed)     │ announce(stopped)
              ▼                         ▼
       ┌───────────┐            ┌────────────┐
       │  SEEDING  │            │   STOPPED  │
       └───────────┘            └────────────┘
              │
              │ announce(stopped)
              ▼
       ┌────────────┐
       │  STOPPED   │
       └────────────┘
```

The `Empty` event is sent as a periodic update from any non-stopped state.

---

## 4. Scrape Functionality

The scrape endpoint provides a lightweight way to query swarm statistics without joining the swarm.

### Scrape Request

```
GET /scrape?info_hash=<hex1>&info_hash=<hex2>...
```

Multiple `info_hash` parameters may be specified in a single request. If no `info_hash` is provided, statistics for all known resources are returned (subject to rate limiting).

### Scrape Response

```python
d
5:files d
  40:<info_hash_hex> d
    8:complete i42e
    10:incomplete i17e
    8:downloaded i156e
    10:downloaders i12e
    7:name 9:movie.mp4
  e
  40:<another_hash_hex> d
    8:complete i5e
    10:incomplete i3e
    8:downloaded i34e
  e
e
```

| Field | Type | Description |
|-------|------|-------------|
| `complete` | integer | Number of seeders (peers with all pieces) |
| `incomplete` | integer | Number of leechers (peers still downloading) |
| `downloaded` | integer | Total number of downloads ever completed |
| `downloaders` | integer | Number of active downloaders right now (optional) |
| `name` | string | Human-readable resource name if known (optional) |

### Rust Implementation

```rust
impl TrackerClient {
    pub fn scrape(&self, info_hashes: &[InfoHash]) -> Result<Vec<SwarmStatus>> {
        let url = self.derive_scrape_url(self.current_url())
            .ok_or(Error::NoScrapeEndpoint)?;

        let mut params = Vec::new();
        for hash in info_hashes {
            params.push(("info_hash", hex::encode(hash)));
        }

        let response = self.http_client
            .get(&url)
            .query(&params)
            .timeout(self.config.read_timeout)
            .send()?
            .bytes()?;

        let decoded = Bencode::decode(&response)?;
        Self::parse_scrape_response(&decoded, info_hashes)
    }

    fn parse_scrape_response(
        bencode: &BencodeValue,
        requested: &[InfoHash],
    ) -> Result<Vec<SwarmStatus>> {
        let files = bencode.dict_get("files")
            .and_then(|f| f.as_dict())
            .ok_or(Error::InvalidScrapeResponse)?;

        let mut results = Vec::new();
        for hash in requested {
            let hex = hex::encode(hash);
            if let Some(file) = files.get(&hex) {
                results.push(SwarmStatus {
                    complete: file.dict_get_as_int("complete").unwrap_or(0) as u32,
                    incomplete: file.dict_get_as_int("incomplete").unwrap_or(0) as u32,
                    downloaded: file.dict_get_as_int("downloaded").unwrap_or(0) as u32,
                });
            }
        }
        Ok(results)
    }
}

#[derive(Debug, Clone)]
pub struct SwarmStatus {
    pub complete: u32,
    pub incomplete: u32,
    pub downloaded: u32,
}
```

---

## 5. Peer List Management

### Compact vs. Dictionary Format

| Feature | Compact (binary) | Dictionary |
|---------|-----------------|------------|
| Size per peer | 6 bytes | ~50-80 bytes |
| IP:port only | Yes | No |
| Includes peer_id | No | Yes |
| Includes metadata | No | Yes (bw, location, firewall) |
| Bandwidth overhead | Minimal | Moderate |

The client should request compact format for initial announces to minimize bandwidth. After connecting to peers, extended info can be exchanged directly via the wire protocol.

### Deduplication

The client must deduplicate peer lists from multiple trackers and from DHT results:

```rust
fn deduplicate_peers(peers: Vec<PeerInfo>) -> Vec<PeerInfo> {
    let mut seen = HashSet::new();
    peers.into_iter()
        .filter(|p| seen.insert(p.addr))
        .collect()
}
```

### Peer Filtering

Before connecting, the client should filter peers:

```rust
impl TrackerClient {
    fn filter_peers(peers: Vec<PeerInfo>, config: &TrackerConfig) -> Vec<PeerInfo> {
        peers.into_iter()
            .filter(|p| {
                // Remove loopback (we can't connect to ourselves)
                !p.addr.ip().is_loopback() &&
                // Remove unspecified
                !p.addr.ip().is_unspecified() &&
                // Remove obviously invalid ports
                p.addr.port() > 1024 &&
                // Apply geo-filter if configured
                config.allowed_regions.as_ref()
                    .map_or(true, |regions| {
                        p.location.as_ref().map_or(true, |loc| regions.contains(loc))
                    })
            })
            .collect()
    }
}
```

---

## 6. Interval and Retry Logic

### Announce Scheduling

```rust
pub struct AnnounceScheduler {
    /// Base interval from tracker
    base_interval: Duration,
    /// Minimum interval from tracker
    min_interval: Duration,
    /// Current computed interval
    current_interval: Duration,
    /// Time of last successful announce
    last_announce: Instant,
    /// Consecutive failures
    fail_count: u32,
    /// Whether we're in backoff mode
    backing_off: bool,
}

impl AnnounceScheduler {
    pub fn new(interval: Duration, min_interval: Duration) -> Self;

    /// Returns how long to wait before next announce
    pub fn next_announce_in(&self) -> Duration;

    /// Call on successful announce — resets backoff
    pub fn on_success(&mut self, interval: Duration, min_interval: Duration) {
        self.base_interval = interval;
        self.min_interval = min_interval;
        self.current_interval = interval;
        self.fail_count = 0;
        self.backing_off = false;
        self.last_announce = Instant::now();
    }

    /// Call on announce failure — applies exponential backoff
    pub fn on_failure(&mut self, backoff_factor: f64) {
        self.fail_count += 1;
        self.backing_off = true;
        let backoff = Duration::from_secs(
            (self.min_interval.as_secs() as f64
                * backoff_factor.powi(self.fail_count as i32))
                .min(3600.0) as u64 // cap at 1 hour
        );
        self.current_interval = backoff.max(self.min_interval);
    }

    /// Whether the client is currently backing off
    pub fn is_backing_off(&self) -> bool;

    /// Reset to initial state
    pub fn reset(&mut self);
}
```

### Retry State Machine

```
Success:
  fail_count = 0
  interval = base_interval
  ─────────────────────────────► Next announce at interval

Failure #1:
  fail_count = 1
  interval = min_interval * 2^1
  ─────────────────────────────► Retry after backoff

Failure #2:
  fail_count = 2
  interval = min_interval * 2^2
  ─────────────────────────────► Retry after backoff

Failure #3:
  fail_count = 3
  Mark tracker as dead, rotate to next tracker
  ─────────────────────────────► Try next tracker

All trackers dead:
  Reset all failure counters after 5 minutes
  ─────────────────────────────► Full retry cycle
```

### Tracker Rotation

```rust
impl TrackerClient {
    /// Rotate to the next healthy tracker.
    /// If all trackers are exhausted, reset and try again after cooldown.
    pub fn select_tracker(&mut self) -> Option<&str> {
        let start = self.current_tracker;
        for i in 0..self.tracker_urls.len() {
            let idx = (start + i) % self.tracker_urls.len();
            if !self.dead_trackers.contains(&idx) {
                self.current_tracker = idx;
                return Some(&self.tracker_urls[idx]);
            }
        }
        // All trackers are dead — clear dead list after cooldown
        if self.all_dead_since.elapsed() > Duration::from_secs(300) {
            self.dead_trackers.clear();
            self.current_tracker = 0;
            return self.tracker_urls.first();
        }
        None
    }
}
```

### Failure Scenarios

| Scenario | Behavior |
|----------|----------|
| Connection timeout | Immediate retry with backoff (max 3 attempts) |
| HTTP 5xx | Backoff + retry, mark tracker dead after 3 consecutive |
| HTTP 4xx (except 404) | Backoff + retry (server may be overloaded) |
| HTTP 404 | Permanent failure for that tracker URL |
| `failure reason` in response | Permanent failure, display error to user |
| Malformed response | Backoff + retry (transient parse error) |
| Network error (DNS/refused) | Immediate rotation to next tracker |

---

## 7. Multiple Tracker Support

### Announcing to Multiple Trackers

```rust
impl TrackerClient {
    /// Announce to all trackers simultaneously, merge results.
    pub fn announce_all(
        &self,
        info_hash: &InfoHash,
        event: AnnounceEvent,
        port: u16,
        stats: DownloadStats,
    ) -> Result<Vec<PeerInfo>> {
        let mut handles = Vec::new();
        for url in &self.tracker_urls {
            let client = self.clone_client();
            let hash = *info_hash;
            let url = url.clone();
            let ev = event.clone();
            let st = stats.clone();

            handles.push(tokio::spawn(async move {
                client.announce_to(&url, &hash, ev, port, st).await
            }));
        }

        let mut all_peers = Vec::new();
        let mut best_interval = None;
        for handle in handles {
            if let Ok(Ok(response)) = handle.await {
                all_peers.extend(response.peers);
                if best_interval.is_none() {
                    best_interval = Some(response.interval);
                } else {
                    // Use the shortest interval
                    best_interval = Some(best_interval.unwrap().min(response.interval));
                }
            }
        }

        Ok(deduplicate_peers(all_peers))
    }
}
```

### Tracker Tier Strategy

Trackers can be organized into tiers for structured fallback:

```rust
pub struct TrackerTierList {
    /// Tiers of tracker URLs, tried in order
    tiers: Vec<Vec<String>>,
    /// Current active tier
    active_tier: usize,
}

impl TrackerTierList {
    /// Try all trackers in the current tier in parallel.
    /// If none respond, advance to the next tier.
    pub fn try_tier(&mut self) -> Result<Vec<PeerInfo>> {
        let results = self.parallel_announce(&self.tiers[self.active_tier])?;
        if results.is_empty() && self.active_tier + 1 < self.tiers.len() {
            self.active_tier += 1;
            self.try_tier() // retry next tier
        } else {
            Ok(results)
        }
    }
}
```

Tier 1: Dedicated high-performance trackers (low latency)
Tier 2: Community/backup trackers (moderate latency)
Tier 3: Web-scale trackers (high latency, always available)

---

## 8. Tracker Server Implementation

### Server Architecture

```rust
/// Standalone QVOD Tracker server
pub struct TrackerServer {
    /// Listen address
    addr: SocketAddr,
    /// Router mapping paths to handlers
    router: axum::Router,
    /// Shared database pool
    db: Arc<DatabasePool>,
    /// Configuration
    config: ServerConfig,
}

pub struct ServerConfig {
    /// Listen address (default: 0.0.0.0:6969)
    pub listen_addr: SocketAddr,
    /// Max peers per resource (default: 1000)
    pub max_peers_per_resource: u32,
    /// Peer timeout (seconds without announce before removal, default: 3600)
    pub peer_timeout: u64,
    /// Default announce interval (seconds, default: 1800)
    pub default_interval: u32,
    /// Minimum announce interval (seconds, default: 900)
    pub min_interval: u32,
    /// Max info_hashes per scrape request (default: 100)
    pub max_scrape_hashes: u32,
    /// Rate limit configuration
    pub rate_limit: RateLimitConfig,
    /// Database connection string
    pub database_url: String,
    /// Whether to persist state to database
    pub persistence: bool,
}
```

### Server Endpoints

```rust
impl TrackerServer {
    pub fn new(config: ServerConfig) -> Self;

    /// Start the server.
    pub async fn start(&self) -> Result<()>;

    /// GET /announce — handle client announce
    async fn handle_announce(
        Query(params): Query<AnnounceParams>,
        Extension(db): Extension<Arc<DatabasePool>>,
        Extension(config): Extension<Arc<ServerConfig>>,
        Extension(rate_limiter): Extension<Arc<RateLimiter>>,
        addr: ConnectInfo<SocketAddr>,
    ) -> Result<Response, StatusCode> {
        // 1. Rate limit check by IP
        if rate_limiter.is_rate_limited(addr.ip()) {
            return Ok(tracker_response(Some("Rate limited"), None, None, None));
        }

        // 2. Validate info_hash length (must be 20 bytes)
        let info_hash = hex::decode(&params.info_hash)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if info_hash.len() != 20 {
            return Err(StatusCode::BAD_REQUEST);
        }

        // 3. Parse and validate event
        let event = parse_event(&params.event);

        // 4. Update peer state in database
        match event {
            AnnounceEvent::Started => {
                db.add_peer(&info_hash, &params.peer_id, addr.ip(),
                            params.port, params.uploaded, params.downloaded,
                            params.left).await;
            }
            AnnounceEvent::Completed => {
                db.mark_completed(&info_hash, &params.peer_id).await;
            }
            AnnounceEvent::Stopped => {
                db.remove_peer(&info_hash, &params.peer_id).await;
            }
            AnnounceEvent::Empty => {
                db.refresh_peer(&info_hash, &params.peer_id).await;
            }
        }

        // 5. Query peer list
        let peers = db.get_peers(&info_hash, config.max_peers_per_resource,
                                &params.peer_id).await;

        // 6. Build response
        let (complete, incomplete, downloaded) = db.get_stats(&info_hash).await;

        // 7. Clean expired peers
        db.cleanup_expired(config.peer_timeout).await;

        // 8. Encode and return
        Ok(encode_announce_response(
            config.default_interval,
            config.min_interval,
            complete, incomplete, downloaded,
            peers, params.compact,
        ))
    }

    /// GET /scrape — handle scrape request
    async fn handle_scrape(
        Query(params): Query<ScrapeParams>,
        Extension(db): Extension<Arc<DatabasePool>>,
        Extension(config): Extension<Arc<ServerConfig>>,
    ) -> Result<Response, StatusCode>;
}
```

### Response Encoding

```rust
fn encode_announce_response(
    interval: u32,
    min_interval: u32,
    complete: u32,
    incomplete: u32,
    downloaded: u32,
    peers: Vec<PeerRow>,
    compact: bool,
) -> Response {
    let mut dict = BencodeDict::new();
    dict.insert_int("interval", interval as i64);
    dict.insert_int("min_interval", min_interval as i64);
    dict.insert_int("complete", complete as i64);
    dict.insert_int("incomplete", incomplete as i64);
    dict.insert_int("downloaded", downloaded as i64);

    if compact {
        let mut compact_data = Vec::with_capacity(peers.len() * 6);
        for peer in &peers {
            compact_data.extend_from_slice(&peer.ip.octets());
            compact_data.extend_from_slice(&peer.port.to_be_bytes());
        }
        dict.insert_bytes("peers", compact_data);
    } else {
        let peer_list = BencodeList::new();
        for peer in &peers {
            let mut p = BencodeDict::new();
            p.insert_str("ip", peer.ip.to_string());
            p.insert_int("port", peer.port as i64);
            p.insert_bytes("peer_id", peer.peer_id.to_vec());
            peer_list.push(BencodeValue::Dict(p));
        }
        dict.insert_list("peers", peer_list);
    }

    let response_data = dict.encode();
    Response::builder()
        .header("Content-Type", "text/plain")
        .header("Content-Length", response_data.len().to_string())
        .body(Body::from(response_data))
        .unwrap()
}
```

---

## 9. Database Schema

### SQLite Schema

The tracker uses SQLite for persistence with the following schema:

```sql
-- Resources table: tracks each info_hash
CREATE TABLE IF NOT EXISTS resources (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    info_hash       BLOB NOT NULL UNIQUE,       -- 20 bytes
    name            TEXT,                        -- Optional friendly name
    downloaded      INTEGER NOT NULL DEFAULT 0, -- Total download count
    created_at      INTEGER NOT NULL,            -- Unix timestamp
    updated_at      INTEGER NOT NULL,
    is_banned       INTEGER NOT NULL DEFAULT 0,  -- Soft ban flag
    metadata        BLOB                         -- Reserved for future use
);

CREATE INDEX idx_resources_hash ON resources(info_hash);

-- Peers table: tracks active peers per resource
CREATE TABLE IF NOT EXISTS peers (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    resource_id     INTEGER NOT NULL REFERENCES resources(id),
    peer_id         BLOB NOT NULL,               -- 20 bytes
    ip_address      TEXT NOT NULL,                -- IP as string
    port            INTEGER NOT NULL,
    uploaded        INTEGER NOT NULL DEFAULT 0,
    downloaded      INTEGER NOT NULL DEFAULT 0,
    left_bytes      INTEGER NOT NULL DEFAULT 0,
    is_seeder       INTEGER NOT NULL DEFAULT 0,  -- left=0 means seeder
    is_firewalled   INTEGER NOT NULL DEFAULT 0,
    bw_up           INTEGER NOT NULL DEFAULT 0,
    bw_down         INTEGER NOT NULL DEFAULT 0,
    location        TEXT,                         -- Geo-IP derived
    client_version  TEXT,
    first_seen      INTEGER NOT NULL,             -- Unix timestamp
    last_announce   INTEGER NOT NULL,             -- Unix timestamp

    UNIQUE(resource_id, peer_id)
);

CREATE INDEX idx_peers_resource ON peers(resource_id);
CREATE INDEX idx_peers_last_announce ON peers(last_announce);
CREATE INDEX idx_peers_ip ON peers(ip_address);

-- Stats table: aggregated statistics
CREATE TABLE IF NOT EXISTS stats (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    resource_id     INTEGER NOT NULL REFERENCES resources(id),
    timestamp       INTEGER NOT NULL,             -- Unix timestamp (hourly bucket)
    seeders         INTEGER NOT NULL DEFAULT 0,
    leechers        INTEGER NOT NULL DEFAULT 0,
    downloads       INTEGER NOT NULL DEFAULT 0,
    total_upload    INTEGER NOT NULL DEFAULT 0,
    total_download  INTEGER NOT NULL DEFAULT 0,

    UNIQUE(resource_id, timestamp)
);

CREATE INDEX idx_stats_resource_time ON stats(resource_id, timestamp);

-- IP blacklist for anti-abuse
CREATE TABLE IF NOT EXISTS blacklist (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ip_address      TEXT NOT NULL UNIQUE,
    reason          TEXT,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER                 -- NULL = permanent
);
```

### Rust Database Access Layer

```rust
pub struct DatabasePool {
    pool: sqlx::SqlitePool,
}

impl DatabasePool {
    pub async fn new(database_url: &str) -> Result<Self>;

    pub async fn add_peer(
        &self,
        info_hash: &[u8],
        peer_id: &[u8],
        ip: IpAddr,
        port: u16,
        uploaded: u64,
        downloaded: u64,
        left: u64,
    ) -> Result<()> {
        // Upsert: insert or update existing peer
        sqlx::query(
            r#"
            INSERT INTO peers (resource_id, peer_id, ip_address, port,
                               uploaded, downloaded, left_bytes, is_seeder,
                               first_seen, last_announce)
            VALUES (
                (SELECT id FROM resources WHERE info_hash = ?1),
                ?2, ?3, ?4, ?5, ?6, ?7,
                CASE WHEN ?7 = 0 THEN 1 ELSE 0 END,
                unixepoch(), unixepoch()
            )
            ON CONFLICT(resource_id, peer_id) DO UPDATE SET
                uploaded = ?5, downloaded = ?6, left_bytes = ?7,
                is_seeder = CASE WHEN ?7 = 0 THEN 1 ELSE 0 END,
                last_announce = unixepoch()
            "#,
        )
        .bind(info_hash)
        .bind(peer_id)
        .bind(ip.to_string())
        .bind(port as i64)
        .bind(uploaded as i64)
        .bind(downloaded as i64)
        .bind(left as i64)
        .execute(&self.pool)
        .await?;

        // Ensure resource exists
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO resources (info_hash, created_at, updated_at)
            VALUES (?1, unixepoch(), unixepoch())
            "#,
        )
        .bind(info_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn remove_peer(&self, info_hash: &[u8], peer_id: &[u8]) -> Result<()>;

    pub async fn refresh_peer(&self, info_hash: &[u8], peer_id: &[u8]) -> Result<()>;

    pub async fn mark_completed(&self, info_hash: &[u8], peer_id: &[u8]) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE peers SET is_seeder = 1, left_bytes = 0, last_announce = unixepoch()
            WHERE resource_id = (SELECT id FROM resources WHERE info_hash = ?1)
            AND peer_id = ?2
            "#,
        )
        .bind(info_hash)
        .bind(peer_id)
        .execute(&self.pool)
        .await?;

        // Increment download counter
        sqlx::query(
            r#"UPDATE resources SET downloaded = downloaded + 1 WHERE info_hash = ?1"#,
        )
        .bind(info_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_peers(
        &self,
        info_hash: &[u8],
        limit: u32,
        exclude_peer_id: &[u8],
    ) -> Result<Vec<PeerRow>> {
        let rows = sqlx::query_as::<_, PeerRow>(
            r#"
            SELECT p.peer_id, p.ip_address, p.port, p.is_firewalled,
                   p.bw_up, p.bw_down, p.location, p.is_seeder
            FROM peers p
            JOIN resources r ON p.resource_id = r.id
            WHERE r.info_hash = ?1
              AND p.peer_id != ?2
              AND p.last_announce > unixepoch() - 3600
            ORDER BY p.is_seeder DESC, p.bw_down DESC
            LIMIT ?3
            "#,
        )
        .bind(info_hash)
        .bind(exclude_peer_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn get_stats(&self, info_hash: &[u8]) -> Result<(u32, u32, u32)> {
        let row = sqlx::query(
            r#"
            SELECT
                (SELECT COUNT(*) FROM peers p JOIN resources r
                 ON p.resource_id = r.id WHERE r.info_hash = ?1 AND p.is_seeder = 1
                 AND p.last_announce > unixepoch() - 3600) as complete,
                (SELECT COUNT(*) FROM peers p JOIN resources r
                 ON p.resource_id = r.id WHERE r.info_hash = ?1 AND p.is_seeder = 0
                 AND p.last_announce > unixepoch() - 3600) as incomplete,
                COALESCE((SELECT downloaded FROM resources WHERE info_hash = ?1), 0) as downloaded
            "#,
        )
        .bind(info_hash)
        .fetch_one(&self.pool)
        .await?;

        let complete: i64 = row.get(0);
        let incomplete: i64 = row.get(1);
        let downloaded: i64 = row.get(2);
        Ok((complete as u32, incomplete as u32, downloaded as u32))
    }

    pub async fn cleanup_expired(&self, timeout_secs: u64) -> Result<u64> {
        let result = sqlx::query(
            r#"DELETE FROM peers WHERE last_announce < unixepoch() - ?1"#,
        )
        .bind(timeout_secs as i64)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn is_blacklisted(&self, ip: IpAddr) -> Result<bool>;

    pub async fn add_to_blacklist(&self, ip: IpAddr, reason: &str) -> Result<()>;
}
```

---

## 10. Rate Limiting and Anti-Abuse

### Rate Limiter Design

```rust
pub struct RateLimiter {
    /// Per-IP request counter (IP → window count)
    ip_counters: HashMap<IpAddr, SlidingWindowCounter>,
    /// Per-info_hash request counter
    hash_counters: HashMap<InfoHash, SlidingWindowCounter>,
    /// Configuration
    config: RateLimitConfig,
}

pub struct RateLimitConfig {
    /// Max announces per IP per window (default: 100)
    pub max_announces_per_ip: u32,
    /// Max announces per info_hash per window (default: 500)
    pub max_announces_per_hash: u32,
    /// Window duration in seconds (default: 60)
    pub window_seconds: u64,
    /// Max scrape requests per IP per window (default: 30)
    pub max_scrapes_per_ip: u32,
    /// Max concurrent peers per IP (default: 100)
    pub max_concurrent_peers_per_ip: u32,
    /// Whether to enable GeoIP blocking
    pub geoip_block_enabled: bool,
    /// List of banned country codes
    pub banned_countries: Vec<String>,
}

struct SlidingWindowCounter {
    /// Timestamps of requests within current window
    timestamps: VecDeque<Instant>,
}

impl SlidingWindowCounter {
    fn increment(&mut self, now: Instant, window: Duration) -> u32 {
        // Remove expired entries
        while let Some(&t) = self.timestamps.front() {
            if now.duration_since(t) > window {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
        self.timestamps.push_back(now);
        self.timestamps.len() as u32
    }
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self;

    /// Check if an IP is rate limited.
    /// Returns Some(warning_message) if limited, None if allowed.
    pub fn check_announce(&mut self, ip: IpAddr) -> Option<String> {
        let window = Duration::from_secs(self.config.window_seconds);
        let count = self.ip_counters
            .entry(ip)
            .or_insert_with(|| SlidingWindowCounter::new())
            .increment(Instant::now(), window);

        if count > self.config.max_announces_per_ip {
            return Some(format!("Rate limited: {} announces in {}s",
                                count, self.config.window_seconds));
        }
        None
    }

    pub fn check_scrape(&mut self, ip: IpAddr) -> bool;
}
```

### Anti-Abuse Strategies

#### 1. IP-based Rate Limiting

Track announces per IP address using a sliding window. Clients exceeding the limit receive a `warning message` in the response with reduced peer counts.

```rust
async fn enforce_rate_limit(
    rate_limiter: &mut RateLimiter,
    ip: IpAddr,
    db: &DatabasePool,
) -> Result<RateLimitResult, Error> {
    // Check blacklist first
    if db.is_blacklisted(ip).await? {
        return Ok(RateLimitResult::Blocked);
    }

    match rate_limiter.check_announce(ip) {
        None => Ok(RateLimitResult::Allowed),
        Some(warning) => {
            tracing::warn!("Rate limit triggered for {}: {}", ip, warning);
            Ok(RateLimitResult::Limited(warning))
        }
    }
}
```

#### 2. Peer Validation

```rust
/// Validate announce parameters for abuse patterns
fn validate_announce(params: &AnnounceParams) -> Result<(), AbuseFlag> {
    // Detect fake peer_ids (all zeros, all same byte)
    if params.peer_id.iter().all(|&b| b == 0) || params.peer_id.iter().all(|&b| b == 0xFF) {
        return Err(AbuseFlag::FakePeerId);
    }

    // Validate port range
    if params.port < 1024 || params.port > 65535 {
        return Err(AbuseFlag::InvalidPort);
    }

    // Check for impossibly high upload/download rates
    if params.uploaded > 1_000_000_000_000 {
        return Err(AbuseFlag::ImplausibleStats);
    }

    // info_hash must be exactly 20 bytes
    if params.info_hash.len() != 20 {
        return Err(AbuseFlag::InvalidHash);
    }

    Ok(())
}
```

#### 3. Info_hash Abuse Detection

```rust
/// Detect flashcrowd attacks on a specific resource
fn detect_flashcrowd_attack(
    db: &DatabasePool,
    info_hash: &InfoHash,
    threshold: u32, // default: 1000 announces/min
) -> bool {
    let recent_count = db.count_recent_announces(info_hash, Duration::from_secs(60));
    recent_count > threshold
}
```

#### 4. Cleanup Tasks

```rust
impl TrackerServer {
    /// Periodic cleanup of stale peers
    async fn cleanup_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            let removed = self.db.cleanup_expired(self.config.peer_timeout).await;
            if let Ok(count) = removed {
                if count > 0 {
                    tracing::info!("Cleaned up {} expired peers", count);
                }
            }
        }
    }

    /// Periodic stats aggregation (hourly buckets)
    async fn stats_aggregation_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            self.db.aggregate_hourly_stats().await;
        }
    }
}
```

#### 5. Blacklist Management

```rust
impl DatabasePool {
    /// Auto-blacklist IPs with suspicious behavior
    pub async fn auto_blacklist(&self, ip: IpAddr, reason: &str) -> Result<()> {
        let recent_announces = self.count_ip_announces(ip, Duration::from_secs(300)).await?;

        if recent_announces > 1000 {
            tracing::warn!("Auto-blacklisting {}: {} announces in 5min", ip, recent_announces);
            self.add_to_blacklist(ip, &format!("Auto: {} ({} announces/5min)", reason, recent_announces)).await?;
        }
        Ok(())
    }
}
```

---

## 11. Configuration Example

```toml
# tracker-config.toml
[server]
listen_addr = "0.0.0.0:6969"
max_peers_per_resource = 2000
peer_timeout = 3600
default_interval = 1800
min_interval = 900

[database]
url = "sqlite://data/tracker.db"
persistence = true

[rate_limit]
max_announces_per_ip = 100
max_announces_per_hash = 500
window_seconds = 60
max_scrapes_per_ip = 30
max_concurrent_peers_per_ip = 200
geoip_block_enabled = false

[logging]
level = "info"
```

---

## 12. Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Bencode decode error: {0}")]
    Bencode(String),

    #[error("Tracker returned failure: {0}")]
    FailureResponse(String),

    #[error("No tracker URL configured")]
    NoTrackers,

    #[error("All trackers failed")]
    AllTrackersFailed,

    #[error("Invalid announce URL")]
    InvalidUrl,

    #[error("No scrape endpoint available")]
    NoScrapeEndpoint,

    #[error("Timeout connecting to tracker")]
    Timeout,

    #[error("Rate limited by tracker")]
    RateLimited,
}
```

---

## Summary

The Tracker module provides the centralized peer discovery backbone of the QVOD P2SP network. Key design decisions:

1. **HTTP GET + Bencode**: Simple, stateless, easy to debug. Matches BitTorrent tracker convention.
2. **Compact peer format**: Minimizes bandwidth for peer lists (6 bytes/peer vs ~50 bytes).
3. **Exponential backoff + rotation**: Graceful degradation when trackers fail.
4. **SQLite persistence**: Lightweight embedded database suitable for moderate-scale trackers.
5. **Sliding window rate limiting**: Prevents abuse without penalizing legitimate users.
6. **Dual announce/scrape endpoints**: Separate heavy-weight (announce with peer registration) from light-weight (scrape with stats only).
7. **Tracker tiers**: Structured fallback from dedicated to community to web-scale trackers.

# QVOD Tracker Protocol Reference

## 1. Overview

QVOD uses a standard BitTorrent-compatible HTTP Tracker with extended fields for streaming metadata. The tracker is a centralized HTTP service that maintains peer lists per `info_hash`. It does not transfer video data — only coordinates peer discovery.

---

## 2. Announce Protocol

### 2.1 HTTP GET Request

```
GET /announce?info_hash={hex}&peer_id={hex}&port={u16}
    &uploaded={u64}&downloaded={u64}&left={u64}
    &event={started|completed|stopped|empty}
    &compact=1
    &numwant={u32}
    &key={hex}
    &trackerid={string}
Host: tracker.example.com:6969
User-Agent: QVOD-Client/0.1.0
```

#### Required Parameters

| Parameter | Type | Size | Description |
|-----------|------|------|-------------|
| `info_hash` | hex string | 20 bytes | SHA-1 hash of the metadata (hex-encoded = 40 chars) |
| `peer_id` | hex string | 20 bytes | Client peer identifier (hex-encoded = 40 chars) |
| `port` | integer | 1-65535 | TCP listening port for peer connections |
| `uploaded` | integer | 0-2^64 | Total bytes uploaded (cumulative) |
| `downloaded` | integer | 0-2^64 | Total bytes downloaded (cumulative) |
| `left` | integer | 0-2^64 | Bytes remaining to download |
| `event` | string | — | `started`, `completed`, `stopped`, or empty |
| `compact` | integer | — | `1` to request compact peer format |

#### Optional Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `numwant` | integer | Max peers to return (default: 50) |
| `key` | hex string | Unique identifier for client (for IP validation) |
| `trackerid` | string | Previous tracker ID for multi-tracker sessions |
| `ip` | string | Override IP address (if auto-detected incorrectly) |
| `bw_up` | integer | Upload bandwidth in KB/s (QVOD extension) |
| `bw_down` | integer | Download bandwidth in KB/s (QVOD extension) |
| `location` | string | Geographic location hint (QVOD extension) |
| `format` | string | Video format hint (QVOD extension) |

### 2.2 Bencode Response

**Success Response:**

```bencode
d
8:interval i1800e
12:min_interval i900e
8:complete i42e
10:incomplete i17e
10:downloaded i156e
5:peers l
  d
    2:ip 6:1.2.3.4
    4:port i8621e
    7:peer_id 20:abcdef0123456789abcd
  e
  d
    2:ip 7:10.0.0.5
    4:port i6881e
    7:peer_id 20:1234567890abcdef1234
  e
e
e
```

**Success Response (compact=1):**

```bencode
d
8:interval i1800e
12:min_interval i900e
8:complete i42e
10:incomplete i17e
10:downloaded i156e
5:peers 12:<6-byte entries × 2>
e
```

**Failure Response:**

```bencode
d
14:failure reason 25:access denied, not allowede
e
```

#### Response Fields

| Key | Type | Description |
|-----|------|-------------|
| `interval` | integer | Minimum seconds between announce requests |
| `min_interval` | integer | Minimum seconds allowed between announces |
| `complete` | integer | Number of seeders (peers with entire file) |
| `incomplete` | integer | Number of leechers (peers downloading) |
| `downloaded` | integer | Total completed downloads of this info_hash |
| `peers` | list/bytes | Peer list (dict format or compact bytes) |
| `peers6` | list/bytes | IPv6 peer list |
| `failure reason` | string | Error description (if present) |
| `warning message` | string | Warning description (non-fatal) |
| `tracker id` | string | Tracker identification string |

### 2.3 Compact Peer Format

When `compact=1`, the `peers` field is a byte string where each peer occupies exactly 6 bytes:

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `ip` | IPv4 address (network byte order) |
| 4 | 2 | `port` | Port (big-endian) |

**Example: 3 peers in compact format (18 bytes):**

```
01 02 03 04 21 AA   → 1.2.3.4:8618
0A 00 00 05 1A E1   → 10.0.0.5:6881
C0 A8 00 01 21 AE   → 192.168.0.1:8622
```

```rust
pub struct CompactPeer {
    pub ip: [u8; 4],     // IPv4 bytes
    pub port: u16,       // big-endian
}

impl CompactPeer {
    pub fn encode(&self) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0..4].copy_from_slice(&self.ip);
        buf[4..6].copy_from_slice(&self.port.to_be_bytes());
        buf
    }

    pub fn decode(bytes: &[u8; 6]) -> Self {
        let mut ip = [0u8; 4];
        ip.copy_from_slice(&bytes[0..4]);
        Self {
            ip,
            port: u16::from_be_bytes([bytes[4], bytes[5]]),
        }
    }

    pub fn to_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(self.ip)), self.port)
    }
}
```

### 2.4 Peer Dictionary Format (Non-Compact)

When `compact=0` or omitted, each peer is a Bencode dictionary:

```bencode
d
2:ip 9:192.168.1.1
4:port i8621e
7:peer_id 20:abcdef0123456789abcd
e
```

If the tracker supports QVOD extensions:

```bencode
d
2:ip 9:192.168.1.1
4:port i8621e
7:peer_id 20:abcdef0123456789abcd
5:bw_up i10240e
7:bw_down i20480e
8:location 5:beijing
4:fire i0e
e
```

---

## 3. Scrape Protocol

### 3.1 HTTP GET Request

```
GET /scrape?info_hash={hex}&info_hash={hex2}&...
Host: tracker.example.com:6969
```

Multiple `info_hash` parameters can be concatenated. Each hash is 20 bytes hex-encoded.

### 3.2 Bencode Response

```bencode
d
5:files d
  20:<20-byte info_hash> d
    8:complete i42e
    10:incomplete i17e
    10:downloaded i156e
    2:name 9:movie.mp4e
  e
  20:<20-byte info_hash_2> d
    8:complete i10e
    10:incomplete i5e
    10:downloaded i55e
  e
e
e
```

#### Scrape Response Fields

| Key | Type | Description |
|-----|------|-------------|
| `files` | dictionary | Maps info_hash to its swarm stats |
| `complete` | integer | Number of seeders |
| `incomplete` | integer | Number of leechers |
| `downloaded` | integer | Total completed downloads |
| `name` | string | Optional torrent/stream name |

```rust
pub struct ScrapeResponse {
    pub files: HashMap<[u8; 20], SwarmInfo>,
}

pub struct SwarmInfo {
    pub complete: u32,
    pub incomplete: u32,
    pub downloaded: u32,
    pub name: Option<String>,
}
```

---

## 4. Rust Tracker Client Implementation

### 4.1 Data Structures

```rust
pub struct TrackerClient {
    pub tracker_urls: Vec<String>,
    pub peer_id: [u8; 20],
    pub http_client: reqwest::Client,
    pub config: TrackerConfig,
}

pub struct TrackerConfig {
    pub timeout_connect: Duration,    // default: 10s
    pub timeout_response: Duration,   // default: 30s
    pub max_retries: u32,             // default: 3
    pub retry_backoff: Duration,      // default: 2s (exponential)
    pub compact: bool,                // default: true
    pub numwant: u32,                 // default: 50
}

pub struct AnnounceParams<'a> {
    pub info_hash: &'a InfoHash,
    pub peer_id: &'a [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub event: AnnounceEvent,
    pub compact: bool,
    pub numwant: u32,
    pub bw_up: Option<u32>,
    pub bw_down: Option<u32>,
    pub location: Option<&'a str>,
}

pub enum AnnounceEvent {
    Started,      // First announcement
    Completed,    // Download finished
    Stopped,      // Download stopped
    Empty,        // Periodic update
}

impl AnnounceEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Empty => "",
        }
    }
}
```

### 4.2 Announce Logic

```rust
impl TrackerClient {
    pub async fn announce(
        &self,
        info_hash: &InfoHash,
        event: AnnounceEvent,
        port: u16,
        stats: TransferStats,
    ) -> Result<AnnounceResponse, TrackerError> {
        let params = AnnounceParams {
            info_hash,
            peer_id: &self.peer_id,
            port,
            uploaded: stats.uploaded,
            downloaded: stats.downloaded,
            left: stats.left,
            event,
            compact: self.config.compact,
            numwant: self.config.numwant,
            bw_up: stats.bw_up,
            bw_down: stats.bw_down,
            location: stats.location.as_deref(),
        };

        // Try trackers in random order, fail over
        let mut shuffled = self.tracker_urls.clone();
        shuffled.shuffle(&mut rand::thread_rng());

        for url in &shuffled {
            match self.announce_single(url, &params).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    tracing::warn!("Tracker {url} failed: {e}, trying next");
                    continue;
                }
            }
        }

        Err(TrackerError::AllTrackersFailed)
    }

    async fn announce_single(
        &self,
        tracker_url: &str,
        params: &AnnounceParams,
    ) -> Result<AnnounceResponse, TrackerError> {
        let mut query = vec![
            ("info_hash", hex::encode(params.info_hash)),
            ("peer_id", hex::encode(params.peer_id)),
            ("port", params.port.to_string()),
            ("uploaded", params.uploaded.to_string()),
            ("downloaded", params.downloaded.to_string()),
            ("left", params.left.to_string()),
            ("event", params.event.as_str().to_string()),
            ("compact", if params.compact { "1" } else { "0" }),
            ("numwant", params.numwant.to_string()),
        ];

        // Add optional QVOD extensions
        if let Some(bw_up) = params.bw_up {
            query.push(("bw_up", bw_up.to_string()));
        }
        if let Some(bw_down) = params.bw_down {
            query.push(("bw_down", bw_down.to_string()));
        }
        if let Some(loc) = params.location {
            query.push(("location", loc.to_string()));
        }

        let url = format!("{tracker_url}/announce");
        let response = self.http_client
            .get(&url)
            .query(&query)
            .timeout(self.config.timeout_response)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    TrackerError::Timeout
                } else {
                    TrackerError::HttpRequest(e.to_string())
                }
            })?;

        if !response.status().is_success() {
            return Err(TrackerError::HttpStatus(response.status().as_u16()));
        }

        let body = response.bytes().await
            .map_err(|e| TrackerError::HttpRequest(e.to_string()))?;

        let response = self.parse_response(&body)?;

        // Check for failure reason
        if let Some(reason) = response.failure_reason {
            return Err(TrackerError::TrackerRejected(reason));
        }

        Ok(response)
    }
}
```

### 4.3 Response Parsing

```rust
impl TrackerClient {
    fn parse_response(&self, data: &[u8]) -> Result<AnnounceResponse, TrackerError> {
        let (value, _rest) = BencodeValue::decode(data)
            .map_err(|e| TrackerError::BencodeParse(e.to_string()))?;

        let dict = value.into_dict()
            .ok_or(TrackerError::BencodeParse("expected dict".into()))?;

        let interval = dict.get("interval")
            .and_then(|v| v.as_int())
            .unwrap_or(1800) as u64;

        let min_interval = dict.get("min_interval")
            .and_then(|v| v.as_int())
            .unwrap_or(900) as u64;

        let complete = dict.get("complete")
            .and_then(|v| v.as_int())
            .unwrap_or(0) as u32;

        let incomplete = dict.get("incomplete")
            .and_then(|v| v.as_int())
            .unwrap_or(0) as u32;

        let downloaded = dict.get("downloaded")
            .and_then(|v| v.as_int())
            .unwrap_or(0) as u32;

        let failure_reason = dict.get("failure reason")
            .and_then(|v| v.as_str())
            .map(String::from);

        let warning_message = dict.get("warning message")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Parse peers
        let peers = if self.config.compact {
            dict.get("peers")
                .and_then(|v| v.as_bytes())
                .map(|bytes| parse_compact_peers(bytes))
                .unwrap_or_default()
        } else {
            dict.get("peers")
                .and_then(|v| v.as_list())
                .map(|list| parse_dict_peers(list))
                .unwrap_or_default()
        };

        Ok(AnnounceResponse {
            interval,
            min_interval,
            complete,
            incomplete,
            downloaded,
            peers,
            failure_reason,
            warning_message,
        })
    }
}
```

### 4.4 Peer List Parsing

```rust
pub fn parse_compact_peers(data: &[u8]) -> Vec<PeerInfo> {
    data.chunks_exact(6)
        .map(|chunk| {
            let mut ip_bytes = [0u8; 4];
            ip_bytes.copy_from_slice(&chunk[0..4]);
            let port = u16::from_be_bytes([chunk[4], chunk[5]]);

            PeerInfo {
                peer_id: [0u8; 20],   // unknown in compact format
                addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip_bytes)), port),
                is_firewalled: false,
                bw_up: 0,
                bw_down: 0,
                location: None,
            }
        })
        .collect()
}

pub fn parse_dict_peers(list: &[BencodeValue]) -> Vec<PeerInfo> {
    list.iter().filter_map(|item| {
        let dict = item.as_dict()?;
        let ip_str = dict.get("ip")?.as_str()?;
        let port = dict.get("port")?.as_int()? as u16;

        // Parse peer_id if present
        let peer_id = dict.get("peer_id")
            .and_then(|v| v.as_bytes())
            .map(|b| {
                let mut id = [0u8; 20];
                let len = id.len().min(b.len());
                id[..len].copy_from_slice(&b[..len]);
                id
            })
            .unwrap_or([0u8; 20]);

        let ip: IpAddr = ip_str.parse().ok()?;

        Some(PeerInfo {
            peer_id,
            addr: SocketAddr::new(ip, port),
            is_firewalled: dict.get("fire").and_then(|v| v.as_int()).unwrap_or(0) != 0,
            bw_up: dict.get("bw_up").and_then(|v| v.as_int()).unwrap_or(0) as u32,
            bw_down: dict.get("bw_down").and_then(|v| v.as_int()).unwrap_or(0) as u32,
            location: dict.get("location").and_then(|v| v.as_str()).map(String::from),
        })
    }).collect()
}
```

---

## 5. Scrape Implementation

```rust
impl TrackerClient {
    pub async fn scrape(
        &self,
        info_hashes: &[InfoHash],
    ) -> Result<HashMap<InfoHash, SwarmStatus>, TrackerError> {
        for url in &self.tracker_urls {
            match self.scrape_single(url, info_hashes).await {
                Ok(resp) => return Ok(resp),
                Err(e) => tracing::warn!("Scrape failed on {url}: {e}"),
            }
        }
        Err(TrackerError::AllTrackersFailed)
    }

    async fn scrape_single(
        &self,
        tracker_url: &str,
        info_hashes: &[InfoHash],
    ) -> Result<HashMap<InfoHash, SwarmStatus>, TrackerError> {
        let params: Vec<(String, String)> = info_hashes.iter()
            .map(|h| ("info_hash".into(), hex::encode(h)))
            .collect();

        let url = format!("{tracker_url}/scrape");
        let response = self.http_client
            .get(&url)
            .query(&params)
            .timeout(self.config.timeout_response)
            .send()
            .await?;

        let body = response.bytes().await?;
        let (value, _) = BencodeValue::decode(&body)
            .map_err(|e| TrackerError::BencodeParse(e.to_string()))?;

        let dict = value.into_dict().unwrap();
        let files = dict.get("files")
            .and_then(|v| v.as_dict())
            .unwrap();

        let mut result = HashMap::new();
        for (key, value) in files {
            if key.len() != 20 {
                continue;
            }
            let mut info_hash = [0u8; 20];
            info_hash.copy_from_slice(key);
            let info = value.as_dict().unwrap();

            result.insert(info_hash, SwarmStatus {
                complete: info.get("complete").and_then(|v| v.as_int()).unwrap_or(0) as u32,
                incomplete: info.get("incomplete").and_then(|v| v.as_int()).unwrap_or(0) as u32,
                downloaded: info.get("downloaded").and_then(|v| v.as_int()).unwrap_or(0) as u32,
            });
        }

        Ok(result)
    }
}
```

---

## 6. Tracker Error Codes

| HTTP Status | Meaning | Client Action |
|-------------|---------|---------------|
| 200 | OK | Parse response |
| 400 | Bad Request | Check parameters; retry with fix |
| 404 | Not Found | Try next tracker |
| 500 | Server Error | Retry with backoff |
| 503 | Rate Limited | Wait `min_interval` before retry |

### Tracker Error Enum

```rust
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("all trackers failed")]
    AllTrackersFailed,

    #[error("connection timeout")]
    Timeout,

    #[error("HTTP request failed: {0}")]
    HttpRequest(String),

    #[error("HTTP status {0}")]
    HttpStatus(u16),

    #[error("tracker rejected request: {0}")]
    TrackerRejected(String),

    #[error("Bencode parse error: {0}")]
    BencodeParse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## 7. Complete Example: Request/Response

### Example 1: Starting a Download

**Request:**
```
GET /announce?info_hash=%A1%B2%C3%D4%E5%F6%07%08%09%0A%0B%0C%0D%0E%0F%10%11%12%13%14
    &peer_id=%2D%51%56%4F%44%2D%30%30%30%31%41%42%43%44%45%46%31%32%33%34
    &port=8621&uploaded=0&downloaded=0&left=734003200&event=started&compact=1&numwant=50
Host: tracker.qvod.example.com:6969
```

**Response:**
```bencode
d8:interval i1800e12:min_interval i900e8:complete i3e10:incomplete i12e10:downloaded i45e5:peers66:
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>
  <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes> <26 bytes>e
```

(66 bytes of compact peer data = 11 peers × 6 bytes)

**Decoded peers:**
```
Peer  1: 1.2.3.4:8621
Peer  2: 10.0.0.5:6881
Peer  3: 192.168.1.100:8621
Peer  4: 203.0.113.50:9000
Peer  5: 198.51.100.20:8621
Peer  6: 172.16.0.10:6881
Peer  7: 192.0.2.80:8621
Peer  8: 104.28.7.115:8621
Peer  9: 93.184.216.34:6881
Peer 10: 151.101.1.140:8621
Peer 11: 208.67.222.222:5353
```

### Example 2: Periodic Update

**Request:**
```
GET /announce?info_hash=%A1%B2...%14
    &peer_id=%2D%51...%34
    &port=8621&uploaded=52428800&downloaded=104857600&left=629145600
    &event=&compact=1&numwant=25
```

### Example 3: Completed Download

**Request:**
```
GET /announce?info_hash=%A1%B2...%14
    &peer_id=%2D%51...%34
    &port=8621&uploaded=734003200&downloaded=734003200&left=0
    &event=completed&compact=1&numwant=25
```

### Example 4: Scrape

**Request:**
```
GET /scrape?info_hash=%12%34%56%78%90%AB%CD%EF%12%34%56%78%90%AB%CD%EF%12%34%56%78
Host: tracker.qvod.example.com:6969
```

**Response:**
```bencode
d5:filesd20:124Vx<EF>4Vx<EF>4Vxd8:completei15e10:incompletei3e10:downloadedi89eee
```

---

## 8. Tracker Implementation Guidelines

### 8.1 Server-Side Considerations

| Aspect | Recommendation |
|--------|---------------|
| Max peers per info_hash | 200 |
| Stale peer timeout | 30 minutes (no announce) |
| NAT check | Verify connection IP matches reported IP |
| Rate limit | 1 announce / 30s per info_hash per IP |
| Database | SQLite or in-memory HashMap |

### 8.2 Client-Side Considerations

| Aspect | Recommendation |
|--------|---------------|
| Initial announce | Send `event=started` |
| Periodic announce | Every `interval` seconds |
| Min interval | Respect `min_interval`, never exceed |
| Event on stop | Send `event=stopped` before shutdown |
| Event on complete | Send `event=completed` when download finishes |
| Tracker failover | Try all trackers, random order |
| Retry backoff | Exponential: 2s, 4s, 8s (max 30s) |

# Local HTTP Gateway Specification

## Overview

The Local HTTP Gateway (crate `qvs-local-server`) is the bridge between the user's browser and the QVOD P2SP engine. When a user clicks a `qvod://` link, the OS protocol handler redirects the request to the local HTTP server, which translates it into P2SP engine commands and streams the resulting video data back to the browser via HTTP.

This is Layer 1 in the QVOD architecture — the outermost layer that users interact with directly.

---

## 1. Purpose and Architecture

### Why a Local HTTP Server?

QVOD uses a local HTTP gateway instead of a native browser plugin for several reasons:

1. **Cross-platform compatibility**: Works in any browser (Chrome, Firefox, Safari, Edge) without plugins.
2. **No browser-specific code**: HTML5 video player handles playback, the gateway just provides data.
3. **Protocol handler registration**: Most OSes support registering custom URL schemes (qvod://) with local applications.
4. **HLS pseudo-adaptation**: Generate M3U8 playlists on-the-fly for mobile/HTML5 playback.
5. **Separation of concerns**: The P2P engine runs independently of any UI.

### Architecture Diagram

```
Browser (HTML5 <video> tag)
    |
    | HTTP requests to http://127.0.0.1:8621
    v
+-------------------------------------------+
|           qvs-local-server                |
|  +---------+  +--------+  +------+       |
|  | Handler |  | Stream |  | HLS  |       |
|  | Router  |--|Manager |--|Adapt |       |
|  +----+----+  +----+---+  +------+       |
|       |           |                       |
|       v           v                       |
|  +------------------------------+         |
|  |       QvodEngine API         |         |
|  +------------------------------+         |
+-------------------------------------------+
         |
         v
+-------------------------------------------+
|         P2SP Transport Layer              |
|  (Tracker, DHT, P2P connections)          |
+-------------------------------------------+
```

---

## 2. HTTP Endpoints

### 2.1 GET /play

Primary streaming endpoint. Returns the video stream with HTTP Chunked Transfer Encoding.

#### Request

```
GET /play?hash={info_hash_hex}&name={filename_urlencoded}&size={filesize}&fmt={format}

Parameters:
  hash  (required): 40-character hex-encoded SHA-1 info_hash
  name  (optional): URL-encoded filename (for display purposes)
  size  (optional): Total file size in bytes
  fmt   (optional): Video format string (rmvb, mp4, avi, etc.)

Range Request:
  GET /play?hash={info_hash_hex}&offset={byte_offset}

  offset (optional): Byte offset for seek request
                     Triggers 206 Partial Content response
```

#### Success Response (200 OK)

```
HTTP/1.1 200 OK
Content-Type: video/octet-stream
Transfer-Encoding: chunked
Cache-Control: no-cache, no-store, must-revalidate
Pragma: no-cache
Connection: keep-alive
X-QVOD-Peers: 12
X-QVOD-Buffered: 45.2
X-QVOD-Speed: 1.8
```

The response body uses HTTP/1.1 chunked transfer encoding, where each chunk is a block of video data as it becomes available from the P2P engine.

#### Range Response (206 Partial Content)

```
HTTP/1.1 206 Partial Content
Content-Type: video/octet-stream
Content-Range: bytes 1234567-734003199/734003200
Transfer-Encoding: chunked
Cache-Control: no-cache
```

Used when the `offset` parameter is specified. The server starts streaming from the nearest keyframe preceding the requested offset.

#### Error Responses

```
HTTP/1.1 404 Not Found
Content-Type: application/json
{"error": "resource_not_found", "message": "Resource not found on network"}

HTTP/1.1 503 Service Unavailable
Content-Type: application/json
{"error": "engine_not_ready", "message": "P2P engine initializing, retry later"}

HTTP/1.1 416 Range Not Satisfiable
Content-Type: application/json
{"error": "invalid_range", "message": "Requested offset exceeds file size"}
```

### 2.2 GET /status

Returns real-time streaming statistics as JSON.

#### Request

```
GET /status?hash={info_hash_hex}

Parameters:
  hash (required): 40-character hex-encoded info_hash
```

#### Response

```json
HTTP/1.1 200 OK
Content-Type: application/json

{
  "info_hash": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9",
  "filename": "movie.mp4",
  "file_size": 734003200,
  "downloaded": 123456789,
  "progress_pct": 16.8,
  "state": "playing",
  "peers": {
    "connected": 12,
    "interested": 8,
    "choked": 3,
    "pending": 2
  },
  "speed": {
    "download_bytes_per_sec": 1843200,
    "upload_bytes_per_sec": 512000
  },
  "buffer": {
    "playable_pct": 45.2,
    "filled_ranges": [
      {"start": 0, "end": 331776000},
      {"start": 503316480, "end": 536870912}
    ],
    "watermark_high": 33554432,
    "watermark_low": 8388608
  },
  "playhead": {
    "position_ms": 45000,
    "duration_ms": 732000,
    "current_piece": 172,
    "current_keyframe_offset": 45088768
  },
  "engine": {
    "uptime_secs": 124,
    "memory_usage_mb": 156,
    "dht_nodes": 234,
    "tracker_status": "connected"
  },
  "cache": {
    "hit_ratio": 0.35,
    "bytes_cached": 88080384,
    "cache_path": "/home/user/.qvod/cache/A1B2C3D4.qdata"
  }
}
```

### 2.3 GET /segment

Pseudo-HLS segment endpoint. Returns an MPEG-TS segment for the HLS adapter.

#### Request

```
GET /segment?hash={info_hash_hex}&offset={byte_offset}&length={byte_length}

Parameters:
  hash   (required): info_hash
  offset (required): Starting byte offset in the file
  length (required): Length of segment in bytes (0 = auto-calculate from keyframe)
```

#### Response

```
HTTP/1.1 200 OK
Content-Type: video/MP2T
Content-Length: 524288
Cache-Control: max-age=3600
X-QVOD-Segment-Index: 5
X-QVOD-Keyframe-Offset: 45088768
```

Returns raw MPEG-TS data. The segment is extracted from the cache or P2P engine starting at the nearest keyframe to `offset`.

### 2.4 GET /m3u8

Generate a pseudo-HLS M3U8 playlist on-the-fly.

#### Request

```
GET /m3u8?hash={info_hash_hex}

Parameters:
  hash (required): info_hash
```

#### Response

```
HTTP/1.1 200 OK
Content-Type: application/vnd.apple.mpegurl
Cache-Control: no-cache

#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:10.000,
/segment?hash=A1B2C3D4...&offset=0&length=0
#EXTINF:10.000,
/segment?hash=A1B2C3D4...&offset=524288&length=0
#EXTINF:10.000,
/segment?hash=A1B2C3D4...&offset=1048576&length=0
#EXTINF:8.500,
/segment?hash=A1B2C3D4...&offset=1572864&length=0
#EXT-X-ENDLIST
```

### 2.5 POST /control

Control playback state.

#### Request

```
POST /control
Content-Type: application/json

{
  "hash": "A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9",
  "action": "pause" | "resume" | "stop" | "seek",
  "position_ms": 45000
}
```

#### Response

```
HTTP/1.1 200 OK
Content-Type: application/json

{
  "success": true,
  "state": "paused",
  "position_ms": 45000
}
```

---

## 3. Chunked Transfer Encoding for Streaming

### How It Works

HTTP/1.1 Chunked Transfer Encoding allows the server to send data incrementally without knowing the total content length in advance. This is perfect for P2P streaming where data arrives over time.

```
Chunked response format:

HTTP/1.1 200 OK
Content-Type: video/octet-stream
Transfer-Encoding: chunked

<chunk_size_hex>\r\n
<chunk_data>\r\n
<chunk_size_hex>\r\n
<chunk_data>\r\n
...
0\r\n
\r\n
```

### Implementation with Tokio Channels

```rust
use axum::{
    response::{IntoResponse, Response},
    body::Body,
};
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};

/// A streaming response that sends data as it becomes available
/// from the P2P engine through a tokio channel.
pub struct ChunkedStream {
    rx: mpsc::Receiver<StreamChunk>,
    state: Arc<Mutex<StreamState>>,
}

pub struct StreamChunk {
    pub data: Vec<u8>,
    pub is_last: bool,
    pub offset: u64,
}

pub enum StreamState {
    Buffering,
    Streaming { position: u64 },
    SeekPending { target_offset: u64 },
    Ended,
    Error(String),
}

impl IntoResponse for ChunkedStream {
    fn into_response(self) -> Response {
        let (mut tx, body) = Body::channel();

        tokio::spawn(async move {
            let mut rx = self.rx;
            while let Some(chunk) = rx.recv().await {
                if chunk.is_last {
                    break;
                }
                if tx.send(chunk.data.into()).await.is_err() {
                    break;
                }
            }
        });

        Response::builder()
            .header("Content-Type", "video/octet-stream")
            .header("Transfer-Encoding", "chunked")
            .header("Cache-Control", "no-cache")
            .body(body)
            .unwrap()
    }
}
```

### Backpressure Handling

```rust
pub struct StreamManager {
    active_streams: HashMap<InfoHash, StreamHandle>,
    channel_capacity: usize,
}

struct StreamHandle {
    tx: mpsc::Sender<StreamChunk>,
    state: Arc<Mutex<StreamState>>,
    started_at: Instant,
    bytes_sent: u64,
}

impl StreamManager {
    const DEFAULT_CHANNEL_CAPACITY: usize = 32;

    pub fn new() -> Self;

    /// Create a new stream for a resource.
    /// Returns the sender half for the P2P engine and the receiver half for HTTP.
    pub fn create_stream(
        &mut self,
        info_hash: InfoHash,
        initial_offset: u64,
    ) -> (mpsc::Sender<StreamChunk>, ChunkedStream) {
        let (tx, rx) = mpsc::channel(Self::DEFAULT_CHANNEL_CAPACITY);
        let state = Arc::new(Mutex::new(StreamState::Buffering));

        self.active_streams.insert(info_hash, StreamHandle {
            tx: tx.clone(),
            state: state.clone(),
            started_at: Instant::now(),
            bytes_sent: 0,
        });

        (tx, ChunkedStream { rx, state })
    }

    /// P2P engine calls this to push data into the stream.
    pub async fn push_data(
        &self,
        info_hash: &InfoHash,
        data: Vec<u8>,
        offset: u64,
    ) -> Result<()> {
        if let Some(handle) = self.active_streams.get(info_hash) {
            let chunk = StreamChunk {
                data,
                is_last: false,
                offset,
            };
            handle.tx.send(chunk).await
                .map_err(|_| Error::StreamClosed)?;
            handle.bytes_sent += data.len() as u64;
            Ok(())
        } else {
            Err(Error::StreamNotFound)
        }
    }

    /// Signal end of stream.
    pub async fn end_stream(&self, info_hash: &InfoHash) -> Result<()> {
        if let Some(handle) = self.active_streams.remove(info_hash) {
            let chunk = StreamChunk {
                data: vec![],
                is_last: true,
                offset: 0,
            };
            handle.tx.send(chunk).await.ok();
            *handle.state.lock() = StreamState::Ended;
            Ok(())
        } else {
            Err(Error::StreamNotFound)
        }
    }

    /// Seek the stream to a new position.
    pub async fn seek(&self, info_hash: &InfoHash, target_offset: u64) -> Result<()> {
        if let Some(handle) = self.active_streams.get(info_hash) {
            *handle.state.lock() = StreamState::SeekPending { target_offset };
            Ok(())
        } else {
            Err(Error::StreamNotFound)
        }
    }

    /// Adaptive backpressure: tells the P2P engine whether to slow down.
    pub fn backpressure_advice(&self, info_hash: &InfoHash) -> BackpressureLevel {
        if let Some(handle) = self.active_streams.get(info_hash) {
            let capacity = handle.tx.capacity();
            match capacity {
                0..=4 => BackpressureLevel::Stop,
                5..=10 => BackpressureLevel::Slow,
                11..=20 => BackpressureLevel::Normal,
                _ => BackpressureLevel::Fast,
            }
        } else {
            BackpressureLevel::Normal
        }
    }
}

pub enum BackpressureLevel {
    Stop,
    Slow,
    Normal,
    Fast,
}
```

---

## 4. Range Request Handling for Seek

### Request Parsing

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum RangeHeader {
    /// bytes=start-end
    FromTo { start: u64, end: u64 },
    /// bytes=start-
    FromOffset { start: u64 },
    /// bytes=-suffix (last N bytes)
    Suffix { last_n: u64 },
}

impl RangeHeader {
    pub fn parse(header: &str) -> Result<Self> {
        if !header.starts_with("bytes=") {
            return Err(RangeError::InvalidFormat);
        }

        let range = &header[6..];
        if let Some(dash_pos) = range.find('-') {
            let before = &range[..dash_pos];
            let after = &range[dash_pos + 1..];

            match (before.is_empty(), after.is_empty()) {
                (false, false) => {
                    let start: u64 = before.parse()
                        .map_err(|_| RangeError::InvalidNumber)?;
                    let end: u64 = after.parse()
                        .map_err(|_| RangeError::InvalidNumber)?;
                    Ok(RangeHeader::FromTo { start, end })
                }
                (false, true) => {
                    let start: u64 = before.parse()
                        .map_err(|_| RangeError::InvalidNumber)?;
                    Ok(RangeHeader::FromOffset { start })
                }
                (true, false) => {
                    let last_n: u64 = after.parse()
                        .map_err(|_| RangeError::InvalidNumber)?;
                    Ok(RangeHeader::Suffix { last_n })
                }
                (true, true) => Err(RangeError::EmptyRange),
            }
        } else {
            Err(RangeError::MissingDash)
        }
    }

    /// Resolve the range against the total file size.
    pub fn resolve(&self, total_size: u64) -> Result<(u64, u64)> {
        match self {
            RangeHeader::FromTo { start, end } => {
                if *start >= total_size {
                    return Err(RangeError::StartBeyondFile);
                }
                let end = (*end).min(total_size - 1);
                if *start > end {
                    return Err(RangeError::InvalidRange);
                }
                Ok((*start, end))
            }
            RangeHeader::FromOffset { start } => {
                if *start >= total_size {
                    return Err(RangeError::StartBeyondFile);
                }
                Ok((*start, total_size - 1))
            }
            RangeHeader::Suffix { last_n } => {
                let last_n = (*last_n).min(total_size);
                Ok((total_size - last_n, total_size - 1))
            }
        }
    }
}
```

### Seek Integration

When a Range request arrives (from a browser seek), the handler must:

1. Parse the Range header
2. Find the nearest keyframe before the requested offset
3. Seek the P2P engine to that keyframe position
4. Return 206 Partial Content with the Content-Range header
5. Start streaming from the keyframe offset

```rust
pub async fn handle_range_request(
    Query(params): Query<PlayParams>,
    headers: HeaderMap,
    Extension(engine): Extension<Arc<QvodEngine>>,
    Extension(stream_mgr): Extension<Arc<StreamManager>>,
) -> Result<Response, StatusCode> {
    let info_hash = hex::decode(&params.hash)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let info_hash = InfoHash::from_bytes(&info_hash);

    let metadata = engine.get_metadata(&info_hash)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let range_str = headers.get("range")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    let range = RangeHeader::parse(range_str)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let (start, end) = range.resolve(metadata.file_size)
        .map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;

    // Find nearest keyframe before the requested offset
    let keyframe_offset = metadata.keyframe_index
        .find_nearest_i_frame(start)
        .map(|kf| kf.file_offset)
        .unwrap_or(0);

    engine.seek(&info_hash, keyframe_offset)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (tx, stream) = stream_mgr.create_stream(info_hash, keyframe_offset);

    engine.attach_stream(&info_hash, tx, keyframe_offset)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let content_range = format!("bytes {}-{}/{}", start, end, metadata.file_size);

    Ok(Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("Content-Type", "video/octet-stream")
        .header("Content-Range", content_range)
        .header("Transfer-Encoding", "chunked")
        .header("Cache-Control", "no-cache")
        .body(stream.into_response().into_body())
        .unwrap())
}
```

---

## 5. qvod:// Protocol Handler Registration

### URI Format

```
qvod://{info_hash_hex}|{filename}|{filesize}|{format}|

Example:
qvod://A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9|movie.mp4|734003200|rmvb|
```

### Parsing Implementation

```rust
#[derive(Debug, Clone)]
pub struct QvodUri {
    pub info_hash: [u8; 20],
    pub filename: String,
    pub file_size: u64,
    pub format: String,
}

impl QvodUri {
    pub fn parse(uri: &str) -> Result<Self> {
        let rest = uri.strip_prefix("qvod://")
            .ok_or(UriError::InvalidScheme)?;

        let parts: Vec<&str> = rest.split('|').collect();
        if parts.len() < 4 {
            return Err(UriError::MissingFields);
        }

        let hash_hex = parts[0];
        if hash_hex.len() != 40 {
            return Err(UriError::InvalidHashLength);
        }
        let info_hash: [u8; 20] = hex::decode(hash_hex)
            .map_err(|_| UriError::InvalidHashHex)?
            .try_into()
            .map_err(|_| UriError::InvalidHashLength)?;

        let filename = parts[1].to_string();
        let file_size: u64 = parts[2].parse()
            .map_err(|_| UriError::InvalidFileSize)?;
        let format = parts[3].to_string();

        Ok(Self { info_hash, filename, file_size, format })
    }

    pub fn to_qvod_string(&self) -> String {
        format!(
            "qvod://{}|{}|{}|{}|",
            hex::encode(self.info_hash),
            self.filename,
            self.file_size,
            self.format,
        )
    }

    pub fn to_play_url(&self, server_port: u16) -> String {
        format!(
            "http://127.0.0.1:{}/play?hash={}&name={}&size={}&fmt={}",
            server_port,
            hex::encode(self.info_hash),
            urlencoding::encode(&self.filename),
            self.file_size,
            self.format,
        )
    }
}
```

### OS Protocol Registration

#### Linux (XDG)

```
~/.local/share/applications/qvod-handler.desktop:

[Desktop Entry]
Type=Application
Name=QVOD Stream
Exec=/usr/local/bin/qvs play %u
StartupNotify=false
MimeType=x-scheme-handler/qvod;
```

Registration:

```
xdg-mime default qvod-handler.desktop x-scheme-handler/qvod
```

#### macOS

```
<!-- Info.plist -->
<key>CFBundleURLTypes</key>
<array>
    <dict>
        <key>CFBundleURLName</key>
        <string>com.qvod.stream</string>
        <key>CFBundleURLSchemes</key>
        <array>
            <string>qvod</string>
        </array>
    </dict>
</array>
```

#### Windows

```
Windows Registry Editor Version 5.00

[HKEY_CLASSES_ROOT\qvod]
@="URL:QVOD Protocol"
"URL Protocol"=""

[HKEY_CLASSES_ROOT\qvod\shell]
[HKEY_CLASSES_ROOT\qvod\shell\open]
[HKEY_CLASSES_ROOT\qvod\shell\open\command]
@="\"C:\\Program Files\\QVOD\\qvs.exe\" play \"%1\""
```

### Rust Registration Helper

```rust
pub struct ProtocolHandler;

impl ProtocolHandler {
    pub fn register() -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let desktop_entry = r#"[Desktop Entry]
Type=Application
Name=QVOD P2SP Player
Exec=/usr/local/bin/qvs play %u
StartupNotify=false
MimeType=x-scheme-handler/qvod;"#;

            let path = dirs::home_dir()
                .ok_or(HandlerError::NoHomeDir)?
                .join(".local")
                .join("share")
                .join("applications")
                .join("qvod-handler.desktop");

            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&path, desktop_entry)?;

            std::process::Command::new("xdg-mime")
                .args(["default", "qvod-handler.desktop", "x-scheme-handler/qvod"])
                .status()?;
        }

        #[cfg(target_os = "macos")]
        {
            tracing::info!("macOS protocol handler must be registered in Info.plist");
        }

        #[cfg(target_os = "windows")]
        {
            let key = "HKEY_CLASSES_ROOT\\qvod";
            let cmd = format!(
                r#""{}" play "%1""#,
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.to_str().map(String::from))
                    .unwrap_or_else(|| "qvs.exe".to_string())
            );

            std::process::Command::new("reg")
                .args(["add", key, "/ve", "/t", "REG_SZ", "/d", "URL:QVOD Protocol", "/f"])
                .status()?;
            std::process::Command::new("reg")
                .args(["add", &format!("{}\\shell\\open\\command", key), "/ve", "/t", "REG_SZ", "/d", &cmd, "/f"])
                .status()?;
        }

        Ok(())
    }

    pub fn unregister() -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let path = dirs::home_dir()
                .ok_or(HandlerError::NoHomeDir)?
                .join(".local")
                .join("share")
                .join("applications")
                .join("qvod-handler.desktop");
            std::fs::remove_file(path).ok();
        }

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("reg")
                .args(["delete", "HKEY_CLASSES_ROOT\\qvod", "/f"])
                .status()?;
        }

        Ok(())
    }
}
```

---

## 6. Port Selection Strategy

```rust
pub struct PortSelector;

impl PortSelector {
    const PREFERRED_PORTS: &[u16] = &[8621, 8622, 8623, 8080, 80, 8888, 9000];

    pub fn find_available(preferred: Option<u16>, max_retry: u8) -> Result<u16> {
        if let Some(port) = preferred {
            if Self::is_port_available(port) {
                return Ok(port);
            }
        }

        for &port in Self::PREFERRED_PORTS {
            if Self::is_port_available(port) {
                return Ok(port);
            }
        }

        for _ in 0..max_retry {
            let port = 49152 + rand::thread_rng().gen_range(0..16384) as u16;
            if Self::is_port_available(port) {
                return Ok(port);
            }
        }

        Err(PortError::NoAvailablePort)
    }

    fn is_port_available(port: u16) -> bool {
        std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok()
    }
}
```

### Port Announcement

```rust
impl LocalServer {
    fn announce_port(&self, port: u16) -> Result<()> {
        let config_dir = dirs::config_dir()
            .ok_or(ServerError::NoConfigDir)?
            .join("qvod");

        std::fs::create_dir_all(&config_dir)?;
        std::fs::write(config_dir.join("port"), port.to_string())?;
        Ok(())
    }

    /// Get the actual port of the running server.
    /// This is called by the protocol handler to determine
    /// where to redirect qvod:// links.
    pub fn get_active_port() -> Result<u16> {
        let port_file = dirs::config_dir()
            .ok_or(ServerError::NoConfigDir)?
            .join("qvod")
            .join("port");

        let port_str = std::fs::read_to_string(&port_file)?;
        let port: u16 = port_str.trim().parse()
            .map_err(|_| ServerError::InvalidPortFile)?;

        Ok(port)
    }
}
```

---

## 7. Request Routing Logic

### Router Setup

```rust
use axum::{
    Router,
    routing::{get, post},
    Extension,
};

pub fn build_router(engine: Arc<QvodEngine>, stream_mgr: Arc<StreamManager>) -> Router {
    Router::new()
        .route("/play", get(handle_play))
        .route("/status", get(handle_status))
        .route("/segment", get(handle_segment))
        .route("/m3u8", get(handle_m3u8))
        .route("/control", post(handle_control))
        .layer(Extension(engine))
        .layer(Extension(stream_mgr))
}
```

### Play Handler with Request Routing

```rust
pub async fn handle_play(
    Query(params): Query<PlayParams>,
    headers: HeaderMap,
    Extension(engine): Extension<Arc<QvodEngine>>,
    Extension(stream_mgr): Extension<Arc<StreamManager>>,
) -> Result<Response, StatusCode> {
    // Check for Range header → seek request
    if headers.contains_key("range") || params.offset.is_some() {
        return handle_range_request(
            Query(params), headers, Extension(engine), Extension(stream_mgr)
        ).await;
    }

    // Normal play request
    let info_hash = InfoHash::from_hex(&params.hash)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Initiate P2P download
    let metadata = engine.play(&info_hash).await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Create streaming channel
    let (tx, stream) = stream_mgr.create_stream(info_hash, 0);

    // Connect P2P engine output to stream
    engine.attach_stream(&info_hash, tx, 0).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Return chunked streaming response
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "video/octet-stream")
        .header("Transfer-Encoding", "chunked")
        .header("Cache-Control", "no-cache");

    // Add QVOD diagnostic headers
    if let Some(status) = engine.get_status(&info_hash) {
        response = response
            .header("X-QVOD-Peers", status.peers_connected.to_string())
            .header("X-QVOD-Buffered", format!("{:.1}", status.buffer_pct))
            .header("X-QVOD-Speed", format!("{:.1}", status.download_speed_mbps));
    }

    Ok(response.body(stream.into_response().into_body()).unwrap())
}
```

---

## 8. Pseudo-HLS Adapter

### Overview

The pseudo-HLS adapter allows QVOD to serve video to devices that only support HLS (HTTP Live Streaming), such as iOS Safari and some smart TVs. It generates M3U8 playlists and MPEG-TS segments on-the-fly from the P2P engine, using the keyframe index to determine segment boundaries.

### M3U8 Playlist Generation

```rust
pub struct HlsAdapter {
    metadata: Arc<FileMeta>,
    segment_duration: Duration,
    base_url: String,
}

impl HlsAdapter {
    pub fn new(metadata: Arc<FileMeta>, base_url: String) -> Self {
        Self {
            metadata,
            segment_duration: Duration::from_secs(10),
            base_url,
        }
    }

    /// Generate a complete M3U8 playlist from keyframe index.
    /// Each segment starts at an I-frame boundary.
    pub fn generate_m3u8(&self) -> String {
        let mut m3u8 = String::new();
        m3u8.push_str("#EXTM3U\n");
        m3u8.push_str("#EXT-X-VERSION:3\n");
        m3u8.push_str(&format!(
            "#EXT-X-TARGETDURATION:{}\n",
            self.segment_duration.as_secs()
        ));
        m3u8.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
        m3u8.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");

        let mut last_timestamp = 0u64;
        let mut segment_index = 0;

        for entry in &self.metadata.keyframe_index.entries {
            if entry.frame_type != FrameType::I {
                continue;
            }

            let duration_secs = if last_timestamp == 0 {
                self.segment_duration.as_secs_f64()
            } else {
                (entry.timestamp_ms - last_timestamp) as f64 / 1000.0
            };

            m3u8.push_str(&format!(
                "#EXTINF:{:.3},\n",
                duration_secs.min(self.segment_duration.as_secs_f64() * 2)
            ));

            m3u8.push_str(&format!(
                "{}/segment?hash={}&offset={}&length={}\n",
                self.base_url,
                hex::encode(self.metadata.info_hash),
                entry.file_offset,
                self.estimate_segment_length(entry.file_offset, segment_index),
            ));

            last_timestamp = entry.timestamp_ms;
            segment_index += 1;
        }

        m3u8.push_str("#EXT-X-ENDLIST\n");
        m3u8
    }

    /// Estimate segment byte length from current offset to the next I-frame.
    fn estimate_segment_length(&self, offset: u64, current_idx: usize) -> u64 {
        let i_frames: Vec<&KeyFrameEntry> = self.metadata.keyframe_index.entries
            .iter()
            .filter(|e| e.frame_type == FrameType::I)
            .collect();

        if current_idx + 1 < i_frames.len() {
            i_frames[current_idx + 1].file_offset - offset
        } else {
            self.metadata.file_size - offset
        }
    }

    /// Wrap raw video data into an MPEG-TS segment.
    /// This is a simplified TS muxer that packages H.264/AAC
    /// data with minimal PSI/SI tables.
    pub fn wrap_as_ts(&self, data: &[u8], offset: u64, pts: u64) -> Vec<u8> {
        let mut ts_packet = Vec::with_capacity(data.len() + 192);

        // MPEG-TS sync byte
        ts_packet.push(0x47);

        // Transport error indicator, payload unit start indicator, etc.
        ts_packet.push(0x40); // payload_unit_start_indicator = 1

        // PID (e.g., 0x0100 for video)
        let pid = 0x0100u16;
        ts_packet.extend_from_slice(&pid.to_be_bytes());

        // Continuity counter, adaptation field control
        ts_packet.push(0x10); // adaptation_field_control = 01 (no adaptation, payload only)
        ts_packet.push(0x00); // continuity counter

        // PES packet header
        ts_packet.push(0x00);
        ts_packet.push(0x00);
        ts_packet.push(0x01);
        ts_packet.push(0xE0); // stream_id: video

        // PES packet length (0 = unbounded)
        ts_packet.extend_from_slice(&[0x00, 0x00]);

        // PES header flags
        ts_packet.push(0x80); // PTS present
        ts_packet.push(0x80); // header length

        // PTS (33-bit timestamp)
        let pts_val = pts as u64;
        let pts_encoded = [
            0x20 | ((pts_val >> 30) & 0x07) as u8,
            ((pts_val >> 22) & 0xFF) as u8,
            0x01 | ((pts_val >> 15) & 0x7E) as u8,
            ((pts_val >> 7) & 0xFF) as u8,
            0x01 | ((pts_val & 0x7F) as u8),
        ];
        ts_packet.extend_from_slice(&pts_encoded);

        // Raw data
        ts_packet.extend_from_slice(data);

        ts_packet
    }
}
```

### Segment Handler

```rust
pub async fn handle_segment(
    Query(params): Query<SegmentParams>,
    Extension(engine): Extension<Arc<QvodEngine>>,
) -> Result<Response, StatusCode> {
    let info_hash = InfoHash::from_hex(&params.hash)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let metadata = engine.get_metadata(&info_hash)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Find the keyframe nearest to the requested offset
    let keyframe = metadata.keyframe_index
        .find_nearest_i_frame(params.offset)
        .ok_or(StatusCode::BAD_REQUEST)?;

    // Calculate segment end (next I-frame or EOF)
    let end_offset = metadata.keyframe_index
        .find_next_i_frame(params.offset)
        .map(|kf| kf.file_offset)
        .unwrap_or(metadata.file_size);

    // Read data from engine (cache or P2P)
    let data = engine.read(&info_hash, keyframe.file_offset, end_offset - keyframe.file_offset)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    // Wrap as MPEG-TS
    let hls = HlsAdapter::new(metadata.clone(), String::new());
    let ts_data = hls.wrap_as_ts(&data, keyframe.file_offset, keyframe.timestamp_ms);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "video/MP2T")
        .header("Content-Length", ts_data.len().to_string())
        .header("Cache-Control", "max-age=3600")
        .body(Body::from(ts_data))
        .unwrap())
}
```

---

## 9. Server Lifecycle

### Server Start/Stop

```rust
pub struct LocalServer {
    engine: Arc<QvodEngine>,
    stream_mgr: Arc<StreamManager>,
    config: LocalServerConfig,
    shutdown_tx: Option<oneshot::Sender<()>>,
    bind_addr: Option<SocketAddr>,
}

pub struct LocalServerConfig {
    pub preferred_port: u16,
    pub max_retry: u8,
    pub host: String,
}

impl LocalServer {
    pub fn new(engine: Arc<QvodEngine>, config: LocalServerConfig) -> Self {
        Self {
            engine,
            stream_mgr: Arc::new(StreamManager::new()),
            config,
            shutdown_tx: None,
            bind_addr: None,
        }
    }

    /// Start the HTTP server.
    /// Returns the actual port bound.
    pub async fn start(&mut self) -> Result<u16> {
        let port = PortSelector::find_available(
            Some(self.config.preferred_port),
            self.config.max_retry,
        )?;

        let addr: SocketAddr = format!("{}:{}", self.config.host, port)
            .parse()
            .map_err(|_| ServerError::InvalidAddress)?;

        let app = build_router(self.engine.clone(), self.stream_mgr.clone());

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tracing::info!("Starting QVOD local server on {}", addr);

        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .unwrap();
        });

        self.shutdown_tx = Some(shutdown_tx);
        self.bind_addr = Some(addr);

        // Announce port for protocol handler
        self.announce_port(port)?;

        Ok(port)
    }

    /// Stop the server gracefully.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.bind_addr = None;
        Ok(())
    }

    /// Get the bound port.
    pub fn port(&self) -> Option<u16> {
        self.bind_addr.map(|a| a.port())
    }
}
```

---

## 10. CORS and Security Middleware

```rust
use axum::{
    http::{header, HeaderValue, Method},
    middleware,
    response::Response,
};

/// CORS middleware allowing cross-origin requests from browser-based players.
pub async fn cors_middleware(request: axum::http::Request<Body>, next: middleware::Next) -> Response {
    let mut response = next.run(request).await;

    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Range"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("Content-Range, X-QVOD-*"),
    );

    response
}

/// Rate limiting middleware to prevent abuse of the local server.
pub struct RateLimiter {
    requests: HashMap<IpAddr, SlidingWindowCounter>,
    max_per_second: u32,
}

impl RateLimiter {
    pub fn new(max_per_second: u32) -> Self {
        Self {
            requests: HashMap::new(),
            max_per_second,
        }
    }

    /// Check if request is allowed. Returns 429 if over limit.
    pub async fn check(&mut self, ip: IpAddr) -> Result<(), StatusCode> {
        let counter = self.requests.entry(ip).or_insert_with(|| {
            SlidingWindowCounter::new(Duration::from_secs(1))
        });
        if counter.increment() > self.max_per_second {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        Ok(())
    }
}

struct SlidingWindowCounter {
    window: Duration,
    timestamps: VecDeque<Instant>,
}

impl SlidingWindowCounter {
    fn new(window: Duration) -> Self {
        Self {
            window,
            timestamps: VecDeque::new(),
        }
    }

    fn increment(&mut self) -> u32 {
        let now = Instant::now();
        while let Some(&t) = self.timestamps.front() {
            if now.duration_since(t) > self.window {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
        self.timestamps.push_back(now);
        self.timestamps.len() as u32
    }
}
```

---

## 11. Configuration

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    /// Preferred HTTP listen port (default: 8621)
    pub port: u16,
    /// Max retries for port selection (default: 10)
    pub max_port_retry: u8,
    /// Listen address (default: "127.0.0.1")
    pub host: String,
    /// Enable pseudo-HLS support (default: true)
    pub enable_hls: bool,
    /// HLS segment duration in seconds (default: 10)
    pub hls_segment_duration: u32,
    /// CORS enabled (default: true)
    pub enable_cors: bool,
    /// Rate limit max requests per second per IP (default: 100)
    pub rate_limit_max: u32,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: 8621,
            max_port_retry: 10,
            host: "127.0.0.1".into(),
            enable_hls: true,
            hls_segment_duration: 10,
            enable_cors: true,
            rate_limit_max: 100,
        }
    }
}
```

### Config File Example

```toml
[gateway]
port = 8621
max_port_retry = 10
host = "127.0.0.1"
enable_hls = true
hls_segment_duration = 10
enable_cors = true
rate_limit_max = 100
```

---

## 12. Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("Port {0} not available")]
    PortUnavailable(u16),

    #[error("No available port found after {0} retries")]
    NoAvailablePort(u8),

    #[error("Failed to bind to {0}: {1}")]
    BindFailed(String, #[source] std::io::Error),

    #[error("Stream not found for resource")]
    StreamNotFound,

    #[error("Stream closed by client")]
    StreamClosed,

    #[error("Invalid range request: {0}")]
    InvalidRange(String),

    #[error("Protocol handler registration failed: {0}")]
    HandlerRegistration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## 13. Request Flow Summary

```
Browser                              Local HTTP Server                  P2P Engine
  |                                        |                              |
  |  1. User clicks qvod:// link          |                              |
  |───────────────────────────────────────>|                              |
  |                                        |                              |
  |  2. Parse URI (info_hash, name etc.)  |                              |
  |                                        |                              |
  |  3. Check local cache                 |                              |
  |                                       |                              |
  |  4. GET /play?hash=...                |                              |
  |                                        |  5. engine.play(info_hash)  |
  |                                       |─────────────────────────────>|
  |                                        |                              |
  |                                        |  6. Connect to tracker/DHT  |
  |                                        |  7. Get peer list           |
  |                                        |  8. Connect to peers        |
  |                                        |  9. Get metadata            |
  |                                        |  10. Initiate download      |
  |                                        |                              |
  |  11. HTTP 200 chunked response         |                              |
  |<───────────────────────────────────────|                              |
  |                                        |                              |
  |  12. Browser starts playing            |                              |
  |                                        |                              |
  |  ... ongoing streaming ...             |                              |
  |                                        |  13. push_data(chunks)      |
  |<────────────────── chunks ─────────────|<─────────────────────────────|
  |                                        |                              |
  |  14. User seeks to 45:00              |                              |
  |  GET /play?hash=...&offset=45088768   |                              |
  |───────────────────────────────────────>|                              |
  |                                        |                              |
  |  15. Find keyframe near offset         |                              |
  |  16. engine.seek(info_hash, offset)    |                              |
  |                                       |─────────────────────────────>|
  |                                        |  17. Scheduler reprioritizes|
  |                                        |  18. Buffer cursor moves    |
  |                                        |                              |
  |  19. HTTP 206 Partial Content          |                              |
  |<───────────────────────────────────────|                              |
  |                                        |                              |
  |  20. Playback resumes from keyframe    |                              |
```

---

## Summary

The Local HTTP Gateway is QVOD's user-facing layer, translating browser HTTP requests into P2SP engine commands. Key design decisions:

1. **Chunked Transfer Encoding**: Ideal for streaming where content length is unknown in advance. Works with any HTTP/1.1 client.

2. **Tokio mpsc channels**: Provides natural backpressure — when the HTTP client reads slowly, the channel fills up and signals the P2P engine to throttle.

3. **Keyframe-aligned seek**: Range requests map to the nearest I-frame, ensuring the decoder can immediately produce a valid frame.

4. **Pseudo-HLS**: Generated on-the-fly from the keyframe index. No pre-processing or storage needed. Makes QVOD content accessible to iOS/Safari users.

5. **Port auto-selection**: Graceful fallback from preferred port to random high ports, with file-based announcement for the protocol handler.

6. **OS-level protocol handler**: Registers `qvod://` as a system-wide URL scheme. Works on Linux, macOS, and Windows with platform-specific code.

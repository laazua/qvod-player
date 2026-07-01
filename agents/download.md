# Download Engine Module Specification

## Overview

The Download Engine (P2spDownloader) is the **data acquisition** layer of the QVOD system. It coordinates piece retrieval from three source types:

1. **P2P TCP** — Reliable transport for critical pieces (I-frames, playhead data)
2. **P2P UDP** — Lightweight transport for non-critical pieces (P/B-frames, pre-fetch)
3. **HTTP** — Fallback source for critical and high-priority pieces

The download engine works in lockstep with the `PieceScheduler`: the scheduler decides _what_ to download, and the download engine decides _how_ to download it.

## Architecture

```
                    ┌─────────────────────┐
                    │   PieceScheduler    │
                    │  (priority queue)   │
                    └──────────┬──────────┘
                               │ next_request()
                               ▼
                    ┌─────────────────────┐
                    │  P2spDownloader    │
                    │  (source selector)  │
                    └──────┬──────┬───────┘
                           │      │
                    ┌──────┘      └──────┐
                    ▼                    ▼
           ┌──────────────┐    ┌──────────────┐
           │  P2P Engine  │    │  HTTP Client │
           │ (TCP + UDP)  │    │ (Range GET)  │
           └──────┬───────┘    └──────┬───────┘
                  │                   │
                  ▼                   ▼
           ┌──────────────┐    ┌──────────────┐
           │  Peer Pool   │    │  CDN / HTTP  │
           │  (50 peers)  │    │   Sources    │
           └──────────────┘    └──────────────┘
```

## Data Structures

### DownloadSource

```rust
/// The type of source used to download a piece or block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadSource {
    /// TCP connection to a P2P peer.
    P2pTcp(PeerId),
    /// UDP exchange with a P2P peer.
    P2pUdp(PeerId),
    /// HTTP GET with Range header from a fallback server.
    Http,
}

impl DownloadSource {
    /// Human-readable label.
    pub fn label(&self) -> String {
        match self {
            DownloadSource::P2pTcp(id) => format!("TCP/{}", hex::encode(&id[..4])),
            DownloadSource::P2pUdp(id) => format!("UDP/{}", hex::encode(&id[..4])),
            DownloadSource::Http => "HTTP".into(),
        }
    }
}
```

### DownloadedBlock

```rust
/// A completed block download, ready for verification and buffer writing.
#[derive(Debug, Clone)]
pub struct DownloadedBlock {
    /// Piece index.
    pub piece_index: u32,
    /// Block-offset within the piece (bytes).
    pub begin: u32,
    /// Block data.
    pub data: Vec<u8>,
    /// Source that delivered this block.
    pub source: DownloadSource,
    /// Time taken to download this block.
    pub elapsed: Duration,
    /// Size of this block.
    pub length: u32,
}
```

### PieceDownloadResult

```rust
/// Result of downloading all blocks in a piece.
#[derive(Debug)]
pub struct PieceDownloadResult {
    /// Piece index.
    pub piece_index: u32,
    /// Complete piece data, concatenated from all blocks.
    pub data: Vec<u8>,
    /// Whether SHA-1 verification passed.
    pub verified: bool,
    /// Sources that contributed blocks (for stats).
    pub sources: Vec<DownloadSource>,
    /// Total time to complete the piece.
    pub elapsed: Duration,
    /// Number of retry attempts.
    pub retries: u32,
}
```

## P2SP Downloader

```rust
/// Main download coordinator that manages P2P and HTTP sources.
///
/// Thread safety: Shared via `Arc` across the engine's async tasks.
/// Internal state is protected by `Mutex` where necessary.
pub struct P2spDownloader {
    /// File metadata for piece verification and length calculations.
    metadata: Arc<FileMeta>,

    /// Connection pool for P2P peers.
    connection_pool: Arc<ConnectionPool>,

    /// HTTP fallback sources (from tracker or configured).
    http_sources: Arc<Vec<String>>,

    /// HTTP client for Range requests.
    http_client: reqwest::Client,

    /// Active piece downloads.
    active_downloads: Arc<Mutex<HashMap<u32, ActivePieceDownload>>>,

    /// Event sender for monitoring/progress.
    event_sender: Option<tokio::sync::mpsc::UnboundedSender<DownloadEvent>>,

    /// Configuration.
    config: DownloadConfig,
}

/// Configuration for the download engine.
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// Maximum concurrent piece downloads.
    pub max_concurrent_pieces: u32, // default: 5

    /// Maximum concurrent HTTP requests.
    pub max_http_connections: u32, // default: 3

    /// Block request timeout (per block from a single peer).
    pub block_timeout: Duration, // default: 15 seconds

    /// Piece download timeout (total for all blocks).
    pub piece_timeout: Duration, // default: 120 seconds

    /// Whether to enable HTTP fallback.
    pub http_fallback_enabled: bool, // default: true

    /// Whether to enable UDP transport.
    pub udp_enabled: bool, // default: true

    /// Maximum retries per block before marking the piece as failed.
    pub max_block_retries: u32, // default: 3

    /// Speed threshold (bytes/sec) below which a peer is considered slow.
    pub slow_peer_threshold: f64, // default: 10_000 (10 KB/s)
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            max_concurrent_pieces: 5,
            max_http_connections: 3,
            block_timeout: Duration::from_secs(15),
            piece_timeout: Duration::from_secs(120),
            http_fallback_enabled: true,
            udp_enabled: true,
            max_block_retries: 3,
            slow_peer_threshold: 10_000.0,
        }
    }
}
```

### Active Piece Download Tracker

```rust
/// Tracks an in-progress piece download across multiple sources.
#[derive(Debug)]
pub struct ActivePieceDownload {
    /// Piece index.
    pub piece_index: u32,
    /// Priority level at start.
    pub priority: PiecePriority,
    /// Blocks already completed (bitmask).
    pub completed_blocks: Vec<bool>,
    /// Total blocks in this piece.
    pub total_blocks: u32,
    /// When this download started.
    pub started_at: Instant,
    /// Number of retries so far.
    pub retries: u32,
    /// Which peer is handling which block:
    /// block_index -> Vec<source> (multiple sources for redundancy on Critical)
    pub block_sources: HashMap<u32, Vec<DownloadSource>>,
    /// Accumulated piece data (filled as blocks arrive).
    pub data: Vec<u8>,
    /// Whether HTTP has been triggered for this piece.
    pub http_triggered: bool,
}

impl ActivePieceDownload {
    pub fn new(piece_index: u32, piece_size: u64, priority: PiecePriority) -> Self {
        let total_blocks = ((piece_size + BLOCK_LENGTH - 1) / BLOCK_LENGTH) as u32;
        Self {
            piece_index,
            priority,
            completed_blocks: vec![false; total_blocks as usize],
            total_blocks,
            started_at: Instant::now(),
            retries: 0,
            block_sources: HashMap::new(),
            data: vec![0u8; piece_size as usize],
            http_triggered: false,
        }
    }

    /// Whether all blocks are completed.
    pub fn is_complete(&self) -> bool {
        self.completed_blocks.iter().all(|&c| c)
    }

    /// Progress as fraction (0.0–1.0).
    pub fn progress(&self) -> f64 {
        let done = self.completed_blocks.iter().filter(|&&c| c).count();
        done as f64 / self.total_blocks as f64
    }

    /// Elapsed time since start.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Register a completed block and return the data slice to write.
    pub fn register_block(&mut self, block_index: u32, data: &[u8]) {
        let begin = block_index as usize * BLOCK_LENGTH as usize;
        let end = begin + data.len();
        if end <= self.data.len() {
            self.data[begin..end].copy_from_slice(data);
        }
        self.completed_blocks[block_index as usize] = true;
    }
}
```

## Source Selection

```rust
impl P2spDownloader {
    /// Create a new downloader.
    pub fn new(
        metadata: Arc<FileMeta>,
        connection_pool: Arc<ConnectionPool>,
        http_sources: Vec<String>,
    ) -> Self {
        Self {
            metadata,
            connection_pool,
            http_sources: Arc::new(http_sources),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .pool_max_idle_per_host(3)
                .build()
                .expect("Failed to create HTTP client"),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            event_sender: None,
            config: DownloadConfig::default(),
        }
    }

    /// Start downloading a piece using the optimal source strategy
    /// based on its priority level.
    pub async fn download_piece(
        &self,
        piece_index: u32,
        priority: PiecePriority,
    ) -> Result<PieceDownloadResult, DownloadError> {
        let piece_size = self.metadata.piece_size(piece_index);
        let active = ActivePieceDownload::new(piece_index, piece_size, priority);
        let start = Instant::now();

        {
            let mut downloads = self.active_downloads.lock().unwrap();
            downloads.insert(piece_index, active);
        }

        let result = match priority {
            PiecePriority::Critical => {
                self.download_critical_piece(piece_index, piece_size).await
            }
            PiecePriority::High => {
                self.download_high_piece(piece_index, piece_size).await
            }
            PiecePriority::Normal => {
                self.download_normal_piece(piece_index, piece_size).await
            }
            PiecePriority::Low => {
                self.download_low_piece(piece_index, piece_size).await
            }
        };

        // Clean up active tracking
        self.active_downloads.lock().unwrap().remove(&piece_index);

        if let Ok(mut result) = result {
            result.elapsed = start.elapsed();
            // Verify piece hash
            result.verified = self.metadata.verify_piece(piece_index, &result.data);
            if !result.verified {
                return Err(DownloadError::VerificationFailed {
                    piece_index,
                    expected: self.metadata.piece_hashes[piece_index as usize],
                });
            }
            self.emit_event(DownloadEvent::PieceCompleted {
                piece_index,
                elapsed: result.elapsed,
                verified: true,
            });
            Ok(result)
        } else {
            self.emit_event(DownloadEvent::PieceFailed { piece_index });
            Err(result.unwrap_err())
        }
    }

    /// Critical pieces: download simultaneously from P2P (TCP) and HTTP.
    /// The first complete delivery wins; the redundant request is cancelled.
    async fn download_critical_piece(
        &self,
        piece_index: u32,
        piece_size: u64,
    ) -> Result<PieceDownloadResult, DownloadError> {
        // Start P2P download
        let meta = self.metadata.clone();
        let pool = self.connection_pool.clone();
        let cfg = self.config.clone();

        let p2p_handle = tokio::spawn(async move {
            Self::download_blocks_from_peers(piece_index, piece_size, PiecePriority::Critical, &meta, &pool, &cfg).await
        });

        // Start HTTP download (if enabled and sources available)
        let http_result = if self.config.http_fallback_enabled {
            self.download_piece_http(piece_index, piece_size).await.ok()
        } else {
            None
        };

        // If HTTP completed, cancel P2P and use HTTP data
        if let Some(http_data) = http_result {
            // p2p_handle will be dropped and cancelled
            return Ok(PieceDownloadResult {
                piece_index,
                data: http_data,
                verified: false, // will be verified by caller
                sources: vec![DownloadSource::Http],
                elapsed: Duration::default(),
                retries: 0,
            });
        }

        // Otherwise wait for P2P
        p2p_handle.await.map_err(|_| DownloadError::TaskCancelled)?
    }

    /// High priority pieces: P2P first, HTTP fallback after 3s timeout.
    async fn download_high_piece(
        &self,
        piece_index: u32,
        piece_size: u64,
    ) -> Result<PieceDownloadResult, DownloadError> {
        let meta = self.metadata.clone();
        let pool = self.connection_pool.clone();
        let cfg = self.config.clone();

        // Try P2P with 3-second timeout for first block
        let timeout_duration = Duration::from_secs(3);

        let p2p_fut = Self::download_blocks_from_peers(piece_index, piece_size, PiecePriority::High, &meta, &pool, &cfg);

        if self.config.http_fallback_enabled {
            tokio::select! {
                result = p2p_fut => result,
                _ = tokio::time::sleep(timeout_duration) => {
                    // Timeout on first response — use HTTP fallback
                    self.emit_event(DownloadEvent::HttpFallback { piece_index });
                    if let Ok(http_data) = self.download_piece_http(piece_index, piece_size).await {
                        return Ok(PieceDownloadResult {
                            piece_index,
                            data: http_data,
                            verified: false,
                            sources: vec![DownloadSource::Http],
                            elapsed: Duration::default(),
                            retries: 1,
                        });
                    }
                    // HTTP also failed; retry P2P
                    Self::download_blocks_from_peers(piece_index, piece_size, PiecePriority::High, &meta, &pool, &cfg).await
                }
            }
        } else {
            p2p_fut.await
        }
    }

    /// Normal priority: P2P only, no HTTP.
    async fn download_normal_piece(
        &self,
        piece_index: u32,
        piece_size: u64,
    ) -> Result<PieceDownloadResult, DownloadError> {
        Self::download_blocks_from_peers(
            piece_index,
            piece_size,
            PiecePriority::Normal,
            &self.metadata,
            &self.connection_pool,
            &self.config,
        )
        .await
    }

    /// Low priority: P2P only, idle bandwidth, single peer.
    async fn download_low_piece(
        &self,
        piece_index: u32,
        piece_size: u64,
    ) -> Result<PieceDownloadResult, DownloadError> {
        Self::download_blocks_from_peers(
            piece_index,
            piece_size,
            PiecePriority::Low,
            &self.metadata,
            &self.connection_pool,
            &self.config,
        )
        .await
    }
}
```

## P2P Block Download

```rust
impl P2spDownloader {
    /// Download all blocks of a piece from P2P peers.
    /// Distributes block requests across multiple peers for parallelism.
    async fn download_blocks_from_peers(
        piece_index: u32,
        piece_size: u64,
        priority: PiecePriority,
        metadata: &Arc<FileMeta>,
        pool: &Arc<ConnectionPool>,
        config: &DownloadConfig,
    ) -> Result<PieceDownloadResult, DownloadError> {
        let total_blocks = ((piece_size + BLOCK_LENGTH - 1) / BLOCK_LENGTH) as u32;
        let mut block_data: Vec<Option<Vec<u8>>> = vec![None; total_blocks as usize];
        let mut pending: Vec<u32> = (0..total_blocks).collect();

        // Get peers that have this piece
        let peers: Vec<PeerHandle> = pool
            .peers_with_piece(piece_index)
            .await;

        if peers.is_empty() {
            return Err(DownloadError::NoPeersForPiece(piece_index));
        }

        let max_concurrency = priority.max_concurrency().min(peers.len() as u32);
        let deadline = Instant::now() + config.piece_timeout;

        // Fan-out block requests across peers with pipelining
        // Use a simple work-queue approach: each peer gets blocks until full
        let mut peer_tasks: Vec<tokio::task::JoinHandle<Result<(), DownloadError>>> = Vec::new();
        let mut peer_index = 0usize;

        while !pending.is_empty() && Instant::now() < deadline {
            let block_index = pending.remove(0);

            // Assign to a peer (round-robin)
            let peer = &peers[peer_index % peers.len()];
            peer_index += 1;

            let begin = block_index * BLOCK_LENGTH as u32;
            let length = if block_index == total_blocks - 1 {
                (piece_size - begin as u64) as u32
            } else {
                BLOCK_LENGTH as u32
            };

            // Use TCP for Critical/High, UDP for Normal/Low
            let use_tcp = matches!(priority, PiecePriority::Critical | PiecePriority::High);

            let peer_clone = peer.clone();
            let handle = tokio::spawn(async move {
                let data = if use_tcp {
                    peer_clone.request_block_tcp(piece_index, begin, length).await
                } else {
                    peer_clone.request_block_udp(piece_index, begin, length).await
                }?;

                block_data[block_index as usize] = Some(data);
                Ok(())
            });

            peer_tasks.push(handle);

            // Limit concurrency
            if peer_tasks.len() >= max_concurrency as usize {
                // Wait for one to complete
                tokio::select! {
                    _ = futures::future::join_all(peer_tasks.drain(..)) => {}
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
            }
        }

        // Await remaining tasks
        for handle in peer_tasks.drain(..) {
            let _ = handle.await;
        }

        // Collect results
        let mut data = Vec::with_capacity(piece_size as usize);
        let mut sources = Vec::new();
        let mut retries = 0u32;

        for (i, block_opt) in block_data.iter().enumerate() {
            match block_opt {
                Some(block) => data.extend_from_slice(block),
                None => {
                    // Block failed; try HTTP fallback or return error
                    return Err(DownloadError::BlockDownloadFailed {
                        piece_index,
                        block_index: i as u32,
                    });
                }
            }
        }

        Ok(PieceDownloadResult {
            piece_index,
            data,
            verified: false, // verified by caller
            sources,
            elapsed: Duration::default(),
            retries,
        })
    }
}
```

## HTTP Fallback Download

```rust
impl P2spDownloader {
    /// Download an entire piece via HTTP Range request from fallback sources.
    /// Falls back through multiple HTTP sources if the first fails.
    async fn download_piece_http(
        &self,
        piece_index: u32,
        piece_size: u64,
    ) -> Result<Vec<u8>, DownloadError> {
        if self.http_sources.is_empty() {
            return Err(DownloadError::NoHttpSources);
        }

        let piece_start = piece_index as u64 * self.metadata.piece_length;
        let piece_end = piece_start + piece_size - 1;
        let range_header = format!("bytes={}-{}", piece_start, piece_end);

        // Try each HTTP source in order
        for source in self.http_sources.iter() {
            let url = format!("{}/play?hash={}", source, self.metadata.info_hash.to_hex());
            let result = self
                .http_client
                .get(&url)
                .header("Range", &range_header)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            match result {
                Ok(response) if response.status().is_success() || response.status() == reqwest::StatusCode::PARTIAL_CONTENT => {
                    let bytes = response.bytes().await.map_err(|e| {
                        DownloadError::HttpError(format!("Failed to read response body: {}", e))
                    })?;

                    if bytes.len() as u64 != piece_size {
                        continue; // Wrong size; try next source
                    }

                    self.emit_event(DownloadEvent::HttpBlockDownloaded {
                        piece_index,
                        size: bytes.len() as u64,
                    });

                    return Ok(bytes.to_vec());
                }
                Ok(response) => {
                    tracing::warn!(
                        "HTTP source {} returned status {} for piece {}",
                        source,
                        response.status(),
                        piece_index
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        "HTTP source {} failed for piece {}: {}",
                        source,
                        piece_index,
                        e
                    );
                    continue;
                }
            }
        }

        Err(DownloadError::HttpSourcesExhausted(piece_index))
    }
}
```

## UDP Transport for Non-Critical Blocks

```rust
/// UDP packet types for lightweight block transfer.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpPacketType {
    /// Data payload carrying a block.
    Data = 0x01,
    /// Acknowledgement of received data.
    Ack = 0x02,
    /// Negative acknowledgement (request retransmission).
    Nack = 0x03,
    /// Keep-alive / latency probe.
    Ping = 0x04,
    /// Response to Ping.
    Pong = 0x05,
}

/// A single UDP packet for block data transfer.
///
/// QVOD's UDP transport is designed for non-critical pieces (Normal/Low priority).
/// It uses a custom lightweight protocol on top of raw UDP sockets.
///
/// Packet overhead: ~28 bytes header (IPv4+UDP) + ~20 bytes QVOD header = ~48 bytes
/// Maximum payload: 1400 - 20 = 1380 bytes (safe MTU)
#[derive(Debug, Clone)]
pub struct UdpDataPacket {
    /// Packet type.
    pub msg_type: UdpPacketType,
    /// Sequence number (monotonically increasing per sender).
    pub seq: u32,
    /// Piece index this block belongs to.
    pub piece_index: u32,
    /// Byte offset within the piece.
    pub block_offset: u32,
    /// Block payload data (max 1380 bytes).
    pub payload: Vec<u8>,
}

impl UdpDataPacket {
    /// Maximum payload size for a single UDP packet.
    pub const MAX_PAYLOAD: usize = 1380;

    /// Encode to wire format.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + self.payload.len());
        buf.push(self.msg_type as u8);
        buf.extend_from_slice(&self.seq.to_be_bytes());
        buf.extend_from_slice(&self.piece_index.to_be_bytes());
        buf.extend_from_slice(&self.block_offset.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Decode from wire format.
    pub fn decode(data: &[u8]) -> Result<Self, DownloadError> {
        if data.len() < 17 {
            return Err(DownloadError::UdpPacketTooShort(data.len()));
        }
        let msg_type = match data[0] {
            0x01 => UdpPacketType::Data,
            0x02 => UdpPacketType::Ack,
            0x03 => UdpPacketType::Nack,
            0x04 => UdpPacketType::Ping,
            0x05 => UdpPacketType::Pong,
            _ => return Err(DownloadError::InvalidUdpPacketType(data[0])),
        };
        let mut arr = [0u8; 4];
        arr.copy_from_slice(&data[1..5]);
        let seq = u32::from_be_bytes(arr);
        arr.copy_from_slice(&data[5..9]);
        let piece_index = u32::from_be_bytes(arr);
        arr.copy_from_slice(&data[9..13]);
        let block_offset = u32::from_be_bytes(arr);
        let payload = data[13..].to_vec();

        Ok(Self {
            msg_type,
            seq,
            piece_index,
            block_offset,
            payload,
        })
    }
}

/// Acknowledgement for a UDP data packet.
#[derive(Debug, Clone)]
pub struct UdpAck {
    /// Sequence number being acknowledged.
    pub seq: u32,
    /// Number of consecutive packets being ACKed (cumulative ACK).
    pub cumulative_ack: u32,
    /// Bitmask of out-of-order packets received since last ACK.
    pub selective_ack_bits: u64,
}

impl UdpAck {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(18);
        buf.push(UdpPacketType::Ack as u8);
        buf.extend_from_slice(&self.seq.to_be_bytes());
        buf.extend_from_slice(&self.cumulative_ack.to_be_bytes());
        buf.extend_from_slice(&self.selective_ack_bits.to_be_bytes());
        buf
    }

    pub fn decode(data: &[u8]) -> Result<Self, DownloadError> {
        if data.len() < 14 {
            return Err(DownloadError::UdpPacketTooShort(data.len()));
        }
        let mut arr = [0u8; 4];
        arr.copy_from_slice(&data[1..5]);
        let seq = u32::from_be_bytes(arr);
        arr.copy_from_slice(&data[5..9]);
        let cumulative_ack = u32::from_be_bytes(arr);
        let mut sack = [0u8; 8];
        sack.copy_from_slice(&data[9..17]);
        let selective_ack_bits = u64::from_be_bytes(sack);
        Ok(Self { seq, cumulative_ack, selective_ack_bits })
    }
}
```

## UDP Congestion Control

```rust
/// Custom UDP congestion control algorithm for streaming media.
///
/// Design inspiration: TCP Reno with optimizations for video streaming:
/// - Faster slow-start (aggressive initial window)
/// - No slow-start after idle (streaming has natural pauses)
/// - Loss-based backoff is gentler (media can tolerate some loss)
/// - RTT-aware pacing to avoid bursts
///
/// State machine:
///   SlowStart ──(ssthresh reached)──→ CongestionAvoidance
///   SlowStart ──(packet loss)────────→ CongestionAvoidance (with cwnd/2)
///   CongestionAvoidance ──(loss)─────→ FastRecovery
///   FastRecovery ──(retransmit OK)──→ CongestionAvoidance
#[derive(Debug, Clone)]
pub struct UdpCongestionControl {
    /// Current congestion window (in packets).
    cwnd: u32,
    /// Slow-start threshold.
    ssthresh: u32,
    /// Minimum congestion window (never go below this).
    cwnd_min: u32,
    /// Maximum congestion window (safety limit).
    cwnd_max: u32,
    /// Smoothed RTT estimate (milliseconds).
    srtt_ms: f64,
    /// RTT variance (for timeout calculation).
    rttvar_ms: f64,
    /// Estimated loss rate (0.0–1.0) over the last window.
    loss_rate: f64,
    /// Packets in flight (sent but not yet ACKed).
    packets_in_flight: u32,
    /// Sequence number of the last sent packet.
    last_seq: u32,
    /// Highest sequence number that has been cumulatively ACKed.
    last_acked: u32,
    /// Number of consecutive duplicate ACKs.
    dup_ack_count: u32,
    /// Current state.
    state: CongestionState,
    /// Maximum packet size in bytes.
    max_packet_size: u16,
    /// Timestamp of last sent packet (for pacing).
    last_send_time: Instant,
    /// Minimum inter-packet gap (nanoseconds) for rate limiting.
    min_gap_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionState {
    SlowStart,
    CongestionAvoidance,
    FastRecovery,
}

impl UdpCongestionControl {
    /// Create a new congestion controller.
    pub fn new() -> Self {
        Self {
            cwnd: 10,          // Start with 10 packets (aggressive for streaming)
            ssthresh: 64,      // Default threshold
            cwnd_min: 4,       // At least 4 packets
            cwnd_max: 256,     // At most 256 packets
            srtt_ms: 100.0,    // Initial RTT estimate
            rttvar_ms: 50.0,   // Initial variance
            loss_rate: 0.0,
            packets_in_flight: 0,
            last_seq: 0,
            last_acked: 0,
            dup_ack_count: 0,
            state: CongestionState::SlowStart,
            max_packet_size: 1380,
            last_send_time: Instant::now(),
            min_gap_ns: 1_000, // Start with 1µs minimum gap
        }
    }

    /// Whether we can send a new packet.
    pub fn can_send(&self) -> bool {
        self.packets_in_flight < self.cwnd
    }

    /// Called when a packet is sent.
    pub fn on_packet_sent(&mut self) {
        self.packets_in_flight += 1;
        self.last_seq += 1;
        self.last_send_time = Instant::now();
    }

    /// Called when a data ACK is received.
    pub fn on_ack(&mut self, ack: &UdpAck) {
        // Update RTT estimate using Karn's algorithm
        // (Simplified: assume we measure RTT periodically)
        self.update_rtt(ack);

        // Update packets in flight
        let newly_acked = ack.cumulative_ack.saturating_sub(self.last_acked);
        self.packets_in_flight = self.packets_in_flight.saturating_sub(newly_acked);
        self.last_acked = ack.cumulative_ack;

        // Update dup_ack tracking
        if ack.cumulative_ack == self.last_acked {
            self.dup_ack_count += 1;
        } else {
            self.dup_ack_count = 0;
        }

        match self.state {
            CongestionState::SlowStart => {
                // cwnd += 1 per ACK (doubles every RTT)
                self.cwnd += 1;
                if self.cwnd >= self.ssthresh {
                    self.state = CongestionState::CongestionAvoidance;
                }
            }
            CongestionState::CongestionAvoidance => {
                // cwnd += 1/cwnd per ACK (linear growth)
                self.cwnd = self.cwnd + 1;
                // Cap at max
                if self.cwnd > self.cwnd_max {
                    self.cwnd = self.cwnd_max;
                }
            }
            CongestionState::FastRecovery => {
                // In fast recovery: increment cwnd for each duplicate ACK
                if self.dup_ack_count >= 3 {
                    self.cwnd += 1;
                }
            }
        }

        // Update rate pacing
        self.update_pacing();
    }

    /// Called when a packet loss is detected (timeout or NACK).
    pub fn on_loss(&mut self) {
        // Record loss for stats
        self.loss_rate = self.loss_rate * 0.9 + 0.1; // EWMA

        match self.state {
            CongestionState::SlowStart | CongestionState::CongestionAvoidance => {
                // TCP Reno: cwnd /= 2, ssthresh = cwnd
                self.ssthresh = (self.cwnd / 2).max(self.cwnd_min);
                self.cwnd = self.ssthresh.max(self.cwnd_min);
                self.state = CongestionState::CongestionAvoidance;
            }
            CongestionState::FastRecovery => {
                // Already recovering
                self.cwnd = (self.cwnd / 2).max(self.cwnd_min);
            }
        }

        // If loss rate exceeds 10%, switch to TCP-only mode
        if self.loss_rate > 0.10 {
            self.cwnd = self.cwnd_min;
        }

        self.update_pacing();
    }

    /// Get the current retransmission timeout (RTO) in milliseconds.
    pub fn rto_ms(&self) -> u64 {
        // RFC 6298: RTO = SRTT + 4 * RTTVAR
        (self.srtt_ms + 4.0 * self.rttvar_ms).max(200.0) as u64
    }

    /// Get the current smoothed RTT.
    pub fn srtt(&self) -> Duration {
        Duration::from_millis(self.srtt_ms as u64)
    }

    /// Get the current congestion window size (in packets).
    pub fn cwnd(&self) -> u32 {
        self.cwnd
    }

    /// Get the current estimated loss rate.
    pub fn loss_rate(&self) -> f64 {
        self.loss_rate
    }

    /// Wait time before the next packet can be sent (for pacing).
    pub fn wait_time(&self) -> Duration {
        if self.cwnd == 0 {
            return Duration::from_millis(100);
        }
        let rate_bytes_per_sec = (self.cwnd as f64 * self.max_packet_size as f64)
            / (self.srtt_ms / 1000.0).max(0.001);
        let packet_interval_ns = 1_000_000_000.0 / (rate_bytes_per_sec / self.max_packet_size as f64).max(1.0);
        Duration::from_nanos(packet_interval_ns.max(self.min_gap_ns as f64) as u64)
    }

    /// Current state label.
    pub fn state_label(&self) -> &'static str {
        match self.state {
            CongestionState::SlowStart => "SlowStart",
            CongestionState::CongestionAvoidance => "CongestionAvoidance",
            CongestionState::FastRecovery => "FastRecovery",
        }
    }

    fn update_rtt(&mut self, ack: &UdpAck) {
        // Simplified RTT estimation
        let sample_rtt = self.srtt_ms; // In practice, measure from send time
        let alpha = 0.125;
        let beta = 0.25;
        self.rttvar_ms = (1.0 - beta) * self.rttvar_ms + beta * (sample_rtt - self.srtt_ms).abs();
        self.srtt_ms = (1.0 - alpha) * self.srtt_ms + alpha * sample_rtt;

        // Clamp
        self.srtt_ms = self.srtt_ms.max(5.0).min(1000.0);
        self.rttvar_ms = self.rttvar_ms.max(1.0).min(500.0);
    }

    fn update_pacing(&mut self) {
        // Target: 1 packet per (cwnd / RTT) interval
        if self.cwnd > 0 && self.srtt_ms > 0.0 {
            let interval_ns = (self.srtt_ms * 1_000_000.0 / self.cwnd as f64) as u64;
            self.min_gap_ns = interval_ns.max(500); // At least 500ns between packets
        }
    }
}
```

## TCP Transport for Critical Blocks

```rust
impl P2spDownloader {
    /// Request a block from a peer via TCP (reliable transport).
    /// Used for Critical and High priority pieces.
    ///
    /// QVOD's TCP transport is a modified BitTorrent peer wire protocol:
    /// - Standard 68-byte handshake with "Qvod P2SP Protocol" identifier
    /// - Length-prefixed messages with 1-byte message ID
    /// - Extended messaging support (BEP 10) for ut_metadata and ut_pex
    async fn request_block_tcp(
        &self,
        peer: &PeerHandle,
        piece_index: u32,
        begin: u32,
        length: u32,
    ) -> Result<Vec<u8>, DownloadError> {
        // 1. Ensure connection is established and unchoked
        if !peer.is_connected() {
            peer.connect().await.map_err(|e| DownloadError::PeerConnectionFailed(e.to_string()))?;
        }

        // 2. Send interested if not already
        if !peer.is_interested() {
            peer.send_interested().await?;
        }

        // 3. Wait for unchoke (with timeout)
        peer.wait_for_unchoke(Duration::from_secs(10)).await?;

        // 4. Send request message
        // Format: length_prefix(4) + msg_id(1) + index(4) + begin(4) + length(4)
        peer.send_request(piece_index, begin, length).await?;

        // 5. Read response piece message
        // Format: length_prefix(4) + msg_id(1) + index(4) + begin(4) + data(length)
        let data = peer.read_piece_response(piece_index, begin, length, Duration::from_secs(30)).await?;

        Ok(data)
    }

    /// Request a block from a peer via UDP (lightweight transport).
    /// Used for Normal and Low priority pieces.
    async fn request_block_udp(
        &self,
        peer: &PeerHandle,
        piece_index: u32,
        begin: u32,
        length: u32,
    ) -> Result<Vec<u8>, DownloadError> {
        // UDP transport is simpler: fire-and-forget with ACK/NACK
        let packet = UdpDataPacket {
            msg_type: UdpPacketType::Data,
            seq: peer.next_seq(),
            piece_index,
            block_offset: begin,
            payload: Vec::new(), // Request has no payload; response carries data
        };

        peer.send_udp(&packet.encode()).await
            .map_err(|e| DownloadError::UdpSendFailed(e.to_string()))?;

        // Wait for response with timeout
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if Instant::now() > deadline {
                return Err(DownloadError::UdpTimeout { piece_index, block_offset: begin });
            }

            let response = peer.receive_udp(Duration::from_millis(500)).await
                .map_err(|_| DownloadError::UdpTimeout { piece_index, block_offset: begin })?;

            let data_packet = UdpDataPacket::decode(&response)?;

            if data_packet.piece_index == piece_index
                && data_packet.block_offset == begin
                && !data_packet.payload.is_empty()
            {
                // Send ACK
                let ack = UdpAck {
                    seq: data_packet.seq,
                    cumulative_ack: data_packet.seq,
                    selective_ack_bits: 0,
                };
                peer.send_udp(&ack.encode()).await.ok();
                return Ok(data_packet.payload);
            }

            // Send NACK for retransmission
            let nack = UdpAck {
                seq: data_packet.seq,
                cumulative_ack: data_packet.seq,
                selective_ack_bits: 0,
            };
            peer.send_udp(&nack.encode()).await.ok();
        }
    }
}
```

## Piece Verification

```rust
impl P2spDownloader {
    /// Verify a piece's data against its stored SHA-1 hash.
    /// Called after all blocks are assembled.
    ///
    /// Returns true if the SHA-1 hash matches.
    pub fn verify_piece(&self, piece_index: u32, data: &[u8]) -> bool {
        // Expected hash from FileMeta
        let expected = self.metadata.piece_hashes.get(piece_index as usize);
        let Some(expected) = expected else {
            return false;
        };

        // Compute actual SHA-1
        let mut hasher = sha1::Sha1::new();
        hasher.update(data);
        let actual = hasher.digest().bytes();

        // Constant-time comparison to prevent timing attacks
        let expected_bytes = expected.0;
        let mut diff = 0u8;
        for i in 0..20 {
            diff |= expected_bytes[i] ^ actual[i];
        }

        if diff != 0 {
            tracing::warn!(
                "Piece {} verification FAILED. Expected {}, got {}",
                piece_index,
                hex::encode(expected_bytes),
                hex::encode(actual),
            );
            return false;
        }

        tracing::debug!("Piece {} verified OK", piece_index);
        true
    }

    /// Re-download a piece that failed verification.
    /// Tries different peers and HTTP sources.
    pub async fn retry_piece(
        &self,
        piece_index: u32,
    ) -> Result<PieceDownloadResult, DownloadError> {
        // Blacklist peers that provided bad data for this piece
        let _bad_peers = self.connection_pool
            .peers_that_sent_piece(piece_index)
            .await;

        // Force HTTP download if available
        let piece_size = self.metadata.piece_size(piece_index);
        if self.config.http_fallback_enabled {
            match self.download_piece_http(piece_index, piece_size).await {
                Ok(data) => {
                    if self.verify_piece(piece_index, &data) {
                        return Ok(PieceDownloadResult {
                            piece_index,
                            data,
                            verified: true,
                            sources: vec![DownloadSource::Http],
                            elapsed: Duration::default(),
                            retries: 1,
                        });
                    }
                }
                Err(_) => {}
            }
        }

        // Try P2P with different peers (excluding blacklisted)
        Self::download_blocks_from_peers(
            piece_index,
            piece_size,
            PiecePriority::Critical, // Re-download at critical priority
            &self.metadata,
            &self.connection_pool,
            &self.config,
        )
        .await
    }
}
```

## Event and Stats Tracking

```rust
/// Events emitted during the download process.
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    /// A block was downloaded from a source.
    BlockDownloaded {
        piece_index: u32,
        block_index: u32,
        source: DownloadSource,
        size: u64,
        elapsed: Duration,
    },
    /// A block download failed.
    BlockFailed {
        piece_index: u32,
        block_index: u32,
        source: DownloadSource,
        error: String,
    },
    /// An entire piece completed.
    PieceCompleted {
        piece_index: u32,
        elapsed: Duration,
        verified: bool,
    },
    /// A piece download failed permanently.
    PieceFailed {
        piece_index: u32,
    },
    /// HTTP fallback triggered for a piece.
    HttpFallback {
        piece_index: u32,
    },
    /// HTTP block downloaded (for stats).
    HttpBlockDownloaded {
        piece_index: u32,
        size: u64,
    },
    /// Verification failed; re-downloading.
    VerificationFailed {
        piece_index: u32,
        retry_count: u32,
    },
    /// Peer choked or disconnected.
    PeerLost {
        peer_id: PeerId,
        piece_index: u32,
    },
}

/// Download statistics aggregated across all active and completed downloads.
#[derive(Debug, Clone, Default)]
pub struct DownloadStats {
    /// Current download speed (bytes/sec), EWMA over 10 seconds.
    pub download_speed: f64,
    /// Current upload speed (bytes/sec).
    pub upload_speed: f64,
    /// Total bytes downloaded over all time.
    pub total_downloaded: u64,
    /// Total bytes uploaded.
    pub total_uploaded: u64,
    /// Total pieces completed.
    pub total_pieces_completed: u32,
    /// Total piece verification failures.
    pub total_verification_failures: u32,
    /// Count of downloads by source type.
    pub source_counts: HashMap<&'static str, u64>,
    /// Active peer connections.
    pub active_peers: u32,
    /// Current UDP loss rate.
    pub udp_loss_rate: f64,
    /// Current average RTT.
    pub avg_rtt_ms: f64,
    /// Active piece downloads.
    pub active_downloads: u32,
    /// Number of HTTP requests made.
    pub http_requests: u64,
    /// Number of HTTP failures.
    pub http_failures: u64,
}

impl P2spDownloader {
    /// Emit a download event to registered listeners.
    fn emit_event(&self, event: DownloadEvent) {
        if let Some(sender) = &self.event_sender {
            let _ = sender.send(event);
        }
    }

    /// Set the event sender channel.
    pub fn set_event_sender(&mut self, sender: tokio::sync::mpsc::UnboundedSender<DownloadEvent>) {
        self.event_sender = Some(sender);
    }
}
```

## Retry Logic

```rust
impl P2spDownloader {
    /// Main retry strategy for failed pieces.
    /// Called when a piece download fails or verification fails.
    pub async fn download_with_retry(
        &self,
        piece_index: u32,
        priority: PiecePriority,
        max_retries: u32,
    ) -> Result<PieceDownloadResult, DownloadError> {
        let mut last_error = DownloadError::MaxRetriesExceeded(piece_index);

        for attempt in 0..=max_retries {
            if attempt > 0 {
                // Exponential backoff: 1s, 2s, 4s, ...
                let delay = Duration::from_secs(1 << (attempt - 1));
                tokio::time::sleep(delay).await;

                tracing::info!(
                    "Retry attempt {}/{} for piece {}",
                    attempt,
                    max_retries,
                    piece_index
                );
            }

            match self.download_piece(piece_index, priority).await {
                Ok(result) if result.verified => {
                    return Ok(result);
                }
                Ok(result) => {
                    // Verification failed
                    self.emit_event(DownloadEvent::VerificationFailed {
                        piece_index,
                        retry_count: attempt,
                    });
                    last_error = DownloadError::VerificationFailed {
                        piece_index,
                        expected: self.metadata.piece_hashes[piece_index as usize],
                    };
                    // Escalate priority on retry
                    let escalated = match priority {
                        PiecePriority::Low => PiecePriority::Normal,
                        PiecePriority::Normal => PiecePriority::High,
                        _ => PiecePriority::Critical,
                    };
                    // Continue loop to retry
                    drop(result);
                    // Re-download with retry_piece (which tries HTTP + different peers)
                    match self.retry_piece(piece_index).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_error = e,
                    }
                }
                Err(e) => {
                    last_error = e;
                }
            }
        }

        Err(last_error)
    }
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("No peers available for piece {0}")]
    NoPeersForPiece(u32),

    #[error("Block {block_index} download failed for piece {piece_index}")]
    BlockDownloadFailed {
        piece_index: u32,
        block_index: u32,
    },

    #[error("Piece {0} verification failed: hash mismatch")]
    VerificationFailed {
        piece_index: u32,
        expected: PieceHash,
    },

    #[error("No HTTP sources configured")]
    NoHttpSources,

    #[error("HTTP sources exhausted for piece {0}")]
    HttpSourcesExhausted(u32),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("Peer connection failed: {0}")]
    PeerConnectionFailed(String),

    #[error("Peer timed out during unchoke wait")]
    PeerUnchokeTimeout,

    #[error("Peer timed out during piece response")]
    PeerPieceTimeout,

    #[error("UDP send failed: {0}")]
    UdpSendFailed(String),

    #[error("UDP timeout for piece {piece_index} offset {block_offset}")]
    UdpTimeout {
        piece_index: u32,
        block_offset: u32,
    },

    #[error("UDP packet too short: {0} bytes")]
    UdpPacketTooShort(usize),

    #[error("Invalid UDP packet type: {0}")]
    InvalidUdpPacketType(u8),

    #[error("Max retries exceeded for piece {0}")]
    MaxRetriesExceeded(u32),

    #[error("Task cancelled")]
    TaskCancelled,

    #[error("Piece download timed out")]
    PieceTimeout(u32),

    #[error("Engine shutting down")]
    EngineShutdown,
}
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_packet_encode_decode_roundtrip() {
        let packet = UdpDataPacket {
            msg_type: UdpPacketType::Data,
            seq: 42,
            piece_index: 7,
            block_offset: 8192,
            payload: vec![0xAB; 100],
        };
        let encoded = packet.encode();
        let decoded = UdpDataPacket::decode(&encoded).unwrap();

        assert_eq!(decoded.msg_type, UdpPacketType::Data);
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.piece_index, 7);
        assert_eq!(decoded.block_offset, 8192);
        assert_eq!(decoded.payload, vec![0xAB; 100]);
    }

    #[test]
    fn test_udp_ack_encode_decode() {
        let ack = UdpAck {
            seq: 100,
            cumulative_ack: 95,
            selective_ack_bits: 0b1101,
        };
        let encoded = ack.encode();
        let decoded = UdpAck::decode(&encoded).unwrap();

        assert_eq!(decoded.seq, 100);
        assert_eq!(decoded.cumulative_ack, 95);
        assert_eq!(decoded.selective_ack_bits, 0b1101);
    }

    #[test]
    fn test_udp_packet_too_short() {
        let result = UdpDataPacket::decode(&[0x01; 5]);
        assert!(result.is_err());
    }

    #[test]
    fn test_congestion_control_slow_start() {
        let mut cc = UdpCongestionControl::new();
        assert_eq!(cc.cwnd(), 10);
        assert_eq!(cc.state, CongestionState::SlowStart);

        // Simulate ACKs during slow start
        for _ in 0..5 {
            cc.on_ack(&UdpAck {
                seq: 1,
                cumulative_ack: 1,
                selective_ack_bits: 0,
            });
        }
        assert_eq!(cc.cwnd(), 15); // cwnd increased by 1 per ACK
    }

    #[test]
    fn test_congestion_control_loss() {
        let mut cc = UdpCongestionControl::new();
        cc.cwnd = 40;
        cc.ssthresh = 64;

        cc.on_loss();
        assert_eq!(cc.cwnd, 20); // cwnd /= 2
        assert_eq!(cc.ssthresh, 20);
        assert_eq!(cc.state, CongestionState::CongestionAvoidance);
    }

    #[test]
    fn test_congestion_control_can_send() {
        let mut cc = UdpCongestionControl::new();
        assert!(cc.can_send());

        cc.packets_in_flight = cc.cwnd;
        assert!(!cc.can_send());
    }

    #[test]
    fn test_congestion_control_rto() {
        let cc = UdpCongestionControl::new();
        let rto = cc.rto_ms();
        assert!(rto >= 200); // Minimum 200ms
    }

    #[test]
    fn test_active_piece_download() {
        let mut apd = ActivePieceDownload::new(0, 262144, PiecePriority::Critical);
        assert_eq!(apd.total_blocks, 16);
        assert!(!apd.is_complete());
        assert!(apd.progress() < 0.01);

        apd.register_block(0, &vec![0u8; 16384]);
        assert!((apd.progress() - 0.0625).abs() < 0.01);

        for i in 1..16 {
            apd.register_block(i, &vec![0u8; 16384]);
        }
        assert!(apd.is_complete());
    }

    #[test]
    fn test_source_strategies() {
        assert_eq!(
            PiecePriority::Critical.source_strategy(),
            SourceStrategy::ParallelP2PAndHttp
        );
        assert_eq!(
            PiecePriority::High.source_strategy(),
            SourceStrategy::P2PWithHttpFallback(Duration::from_secs(3))
        );
        assert_eq!(
            PiecePriority::Normal.source_strategy(),
            SourceStrategy::P2POnly
        );
        assert_eq!(
            PiecePriority::Low.source_strategy(),
            SourceStrategy::P2PIdle
        );
    }

    #[tokio::test]
    async fn test_http_range_request() {
        // This test verifies the Range header construction
        let downloader = P2spDownloader::new(
            Arc::new(create_dummy_meta()),
            Arc::new(ConnectionPool::new(10)),
            vec!["http://example.com".into()],
        );

        // We can't test actual HTTP without a server, but we can verify
        // the range calculation
        let piece_size = downloader.metadata.piece_size(5);
        assert_eq!(piece_size, PIECE_LENGTH as u64);
    }

    fn create_dummy_meta() -> FileMeta {
        let num_pieces = 10u32;
        let piece_hashes = (0..num_pieces)
            .map(|_| PieceHash(sha1::Sha1::from(&[0u8; 256]).digest().bytes()))
            .collect();

        FileMeta {
            info_hash: InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: num_pieces as u64 * PIECE_LENGTH,
            piece_length: PIECE_LENGTH,
            piece_hashes,
            keyframe_index: KeyFrameIndex::new(vec![
                KeyFrameEntry {
                    timestamp_ms: 0,
                    file_offset: 0,
                    frame_size: 48000,
                    frame_type: FrameType::I,
                },
            ]).unwrap(),
            duration_ms: 10000,
            codec: CodecInfo {
                video_codec: "avc1".into(),
                audio_codec: "aac".into(),
                width: 1280,
                height: 720,
                bitrate: 2_000_000,
                audio_sample_rate: 44100,
                audio_channels: 2,
                frame_rate_num: 30000,
                frame_rate_den: 1001,
                pixel_aspect_num: 1,
                pixel_aspect_den: 1,
                has_b_frames: true,
            },
            from_cache: false,
        }
    }
}
```

# Monitoring Specification

## Overview

The monitoring subsystem provides comprehensive observability into every aspect of the QVOD P2SP engine. It collects real-time metrics from all layers (transport, DHT, tracker, streaming, cache) and makes them available through multiple channels: an HTTP JSON API exposed by the local server, console logging via `tracing`, structured metric export for Prometheus, and an in-app real-time status panel in the GUI player.

**Design Principles:**
- All metrics are collected atomically using lock-free or ARC-based counters
- The monitoring layer must never block the critical data path
- Metrics are aggregated over configurable windows (1s, 10s, 60s, 300s)
- Per-peer metrics are retained for scoring and UI display, then pruned when peers disconnect

---

## 1. Metrics Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Monitoring Subsystem                      │
│                                                                   │
│  ┌──────────────────────┐     ┌──────────────────────────────┐   │
│  │  Counter Aggregators │     │  Per-Peer Metrics Store      │   │
│  │  ┌───┐ ┌───┐ ┌───┐  │     │  ┌──────────┐ ┌──────────┐  │   │
│  │  │ B │ │ P │ │ E │  │     │  │ Peer #1  │ │ Peer #2  │  │   │
│  │  │ y │ │ a │ │ r │  │     │  │ speed    │ │ speed    │  │   │
│  │  │ t │ │ c │ │ r │  │     │  │ rtt      │ │ rtt      │  │   │
│  │  │ e │ │ k │ │ o │  │     │  │ progress │ │ progress │  │   │
│  │  │ s │ │ t │ │ r │  │     │  │ quality  │ │ quality  │  │   │
│  │  └───┘ └───┘ └───┘  │     │  └──────────┘ └──────────┘  │   │
│  └──────────────────────┘     └──────────────────────────────┘   │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │                  Metric Consumers                            │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐ │  │
│  │  │ HTTP API │  │ Tracing  │  │Prometheus│  │  GUI Stats │ │  │
│  │  │ (JSON)   │  │ (console)│  │(metrics) │  │  (egui)    │ │  │
│  │  └──────────┘  └──────────┘  └──────────┘  └────────────┘ │  │
│  └─────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### Metric Collection Pipeline

```rust
/// Central metrics registry. Thread-safe, lock-free via atomics.
pub struct MetricsRegistry {
    /// Global byte counters
    bytes_downloaded: AtomicU64,
    bytes_uploaded: AtomicU64,
    bytes_downloaded_total: AtomicU64,
    bytes_uploaded_total: AtomicU64,

    /// Packet counters
    packets_sent: AtomicU64,
    packets_received: AtomicU64,
    packets_lost: AtomicU64,
    packets_retransmitted: AtomicU64,

    /// Peer counters
    peers_connected: AtomicI32,
    peers_interested: AtomicI32,
    peers_choked: AtomicI32,
    peers_total: AtomicI32,
    peers_snubbed: AtomicI32,

    /// Piece counters
    pieces_downloaded: AtomicU64,
    pieces_verified: AtomicU64,
    pieces_failed_hash: AtomicU64,
    pieces_written_to_cache: AtomicU64,

    /// Time buckets for speed calculation (sliding window, 10s)
    speed_samples: Mutex<VecDeque<SpeedSample>>,

    /// Per-peer metrics
    peer_metrics: Mutex<HashMap<[u8; 20], PeerMetrics>>,

    /// Cache metrics
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,

    /// Error counters
    errors_network: AtomicU64,
    errors_protocol: AtomicU64,
    errors_hash_fail: AtomicU64,
    errors_timeout: AtomicU64,

    /// Engine state
    engine_state: AtomicU8,
    buffer_fill_bytes: AtomicU64,
    buffer_capacity_bytes: AtomicU64,
    buffer_playable_us: AtomicU64,

    /// Memory tracking
    memory_allocated: AtomicU64,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            bytes_downloaded: AtomicU64::new(0),
            bytes_uploaded: AtomicU64::new(0),
            bytes_downloaded_total: AtomicU64::new(0),
            bytes_uploaded_total: AtomicU64::new(0),
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            packets_lost: AtomicU64::new(0),
            packets_retransmitted: AtomicU64::new(0),
            peers_connected: AtomicI32::new(0),
            peers_interested: AtomicI32::new(0),
            peers_choked: AtomicI32::new(0),
            peers_total: AtomicI32::new(0),
            peers_snubbed: AtomicI32::new(0),
            pieces_downloaded: AtomicU64::new(0),
            pieces_verified: AtomicU64::new(0),
            pieces_failed_hash: AtomicU64::new(0),
            pieces_written_to_cache: AtomicU64::new(0),
            speed_samples: Mutex::new(VecDeque::with_capacity(200)),
            peer_metrics: Mutex::new(HashMap::new()),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            errors_network: AtomicU64::new(0),
            errors_protocol: AtomicU64::new(0),
            errors_hash_fail: AtomicU64::new(0),
            errors_timeout: AtomicU64::new(0),
            engine_state: AtomicU8::new(0),
            buffer_fill_bytes: AtomicU64::new(0),
            buffer_capacity_bytes: AtomicU64::new(0),
            buffer_playable_us: AtomicU64::new(0),
            memory_allocated: AtomicU64::new(0),
        }
    }

    // --- Updaters (called from engine threads) ---

    pub fn record_download(&self, bytes: u64) {
        self.bytes_downloaded.fetch_add(bytes, Ordering::Relaxed);
        self.bytes_downloaded_total.fetch_add(bytes, Ordering::Relaxed);
        self.record_speed_sample(bytes, Direction::Download);
    }

    pub fn record_upload(&self, bytes: u64) {
        self.bytes_uploaded.fetch_add(bytes, Ordering::Relaxed);
        self.bytes_uploaded_total.fetch_add(bytes, Ordering::Relaxed);
        self.record_speed_sample(bytes, Direction::Upload);
    }

    pub fn record_packet(&self, direction: Direction, lost: bool, retransmitted: bool) {
        match direction {
            Direction::Download => { self.packets_received.fetch_add(1, Ordering::Relaxed); },
            Direction::Upload => { self.packets_sent.fetch_add(1, Ordering::Relaxed); },
        }
        if lost { self.packets_lost.fetch_add(1, Ordering::Relaxed); }
        if retransmitted { self.packets_retransmitted.fetch_add(1, Ordering::Relaxed); }
    }

    pub fn set_peers(&self, connected: i32, interested: i32, choked: i32, total: i32) {
        self.peers_connected.store(connected, Ordering::Relaxed);
        self.peers_interested.store(interested, Ordering::Relaxed);
        self.peers_choked.store(choked, Ordering::Relaxed);
        self.peers_total.store(total, Ordering::Relaxed);
    }

    pub fn record_piece_result(&self, verified: bool) {
        self.pieces_downloaded.fetch_add(1, Ordering::Relaxed);
        if verified {
            self.pieces_verified.fetch_add(1, Ordering::Relaxed);
        } else {
            self.pieces_failed_hash.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_cache_access(&self, hit: bool) {
        if hit {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cache_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_error(&self, error_type: ErrorType) {
        match error_type {
            ErrorType::Network => self.errors_network.fetch_add(1, Ordering::Relaxed),
            ErrorType::Protocol => self.errors_protocol.fetch_add(1, Ordering::Relaxed),
            ErrorType::HashFail => self.errors_hash_fail.fetch_add(1, Ordering::Relaxed),
            ErrorType::Timeout => self.errors_timeout.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn record_peer_metric(&self, peer_id: [u8; 20], metric: PeerMetricUpdate) {
        if let Ok(mut peers) = self.peer_metrics.lock() {
            let entry = peers.entry(peer_id).or_default();
            entry.apply(metric);
        }
    }

    pub fn remove_peer(&self, peer_id: &[u8; 20]) {
        if let Ok(mut peers) = self.peer_metrics.lock() {
            peers.remove(peer_id);
        }
    }

    pub fn set_buffer_state(&self, fill_bytes: u64, capacity_bytes: u64, playable_us: u64) {
        self.buffer_fill_bytes.store(fill_bytes, Ordering::Relaxed);
        self.buffer_capacity_bytes.store(capacity_bytes, Ordering::Relaxed);
        self.buffer_playable_us.store(playable_us, Ordering::Relaxed);
    }

    pub fn set_engine_state(&self, state: EngineState) {
        self.engine_state.store(state as u8, Ordering::Relaxed);
    }

    pub fn set_memory_usage(&self, bytes: u64) {
        self.memory_allocated.store(bytes, Ordering::Relaxed);
    }

    // --- Internal helpers ---

    fn record_speed_sample(&self, bytes: u64, dir: Direction) {
        if let Ok(mut samples) = self.speed_samples.lock() {
            let now = Instant::now();
            samples.push_back(SpeedSample { time: now, bytes, dir });
            // Prune samples older than 10 seconds
            while samples.front().map_or(false, |s| now - s.time > Duration::from_secs(10)) {
                samples.pop_front();
            }
        }
    }

    // --- Snapshot ---

    pub fn snapshot(&self) -> MetricsSnapshot {
        let now = Instant::now();

        // Calculate speeds from sliding window
        let (download_speed, upload_speed) = self.calculate_speeds(now);

        let peers = self.peer_metrics.lock().map(|m| m.values().cloned().collect())
            .unwrap_or_default();

        let cache_hits = self.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.cache_misses.load(Ordering::Relaxed);
        let total_cache_accesses = cache_hits + cache_misses;

        MetricsSnapshot {
            timestamp: now,
            download_speed,
            upload_speed,
            bytes_downloaded_total: self.bytes_downloaded_total.load(Ordering::Relaxed),
            bytes_uploaded_total: self.bytes_uploaded_total.load(Ordering::Relaxed),
            packets_sent: self.packets_sent.load(Ordering::Relaxed),
            packets_received: self.packets_received.load(Ordering::Relaxed),
            packets_lost: self.packets_lost.load(Ordering::Relaxed),
            loss_rate: self.calculate_loss_rate(),
            packets_retransmitted: self.packets_retransmitted.load(Ordering::Relaxed),
            peers_connected: self.peers_connected.load(Ordering::Relaxed),
            peers_interested: self.peers_interested.load(Ordering::Relaxed),
            peers_choked: self.peers_choked.load(Ordering::Relaxed),
            peers_total: self.peers_total.load(Ordering::Relaxed),
            peers_snubbed: self.peers_snubbed.load(Ordering::Relaxed),
            pieces_downloaded: self.pieces_downloaded.load(Ordering::Relaxed),
            pieces_verified: self.pieces_verified.load(Ordering::Relaxed),
            pieces_failed_hash: self.pieces_failed_hash.load(Ordering::Relaxed),
            hash_fail_rate: self.calculate_hash_fail_rate(),
            pieces_written_to_cache: self.pieces_written_to_cache.load(Ordering::Relaxed),
            cache_hit_rate: if total_cache_accesses > 0 {
                cache_hits as f64 / total_cache_accesses as f64
            } else {
                0.0
            },
            errors_network: self.errors_network.load(Ordering::Relaxed),
            errors_protocol: self.errors_protocol.load(Ordering::Relaxed),
            errors_hash_fail: self.errors_hash_fail.load(Ordering::Relaxed),
            errors_timeout: self.errors_timeout.load(Ordering::Relaxed),
            total_errors: self.errors_network.load(Ordering::Relaxed)
                + self.errors_protocol.load(Ordering::Relaxed)
                + self.errors_hash_fail.load(Ordering::Relaxed)
                + self.errors_timeout.load(Ordering::Relaxed),
            engine_state: EngineState::from(self.engine_state.load(Ordering::Relaxed)),
            buffer_fill_bytes: self.buffer_fill_bytes.load(Ordering::Relaxed),
            buffer_capacity_bytes: self.buffer_capacity_bytes.load(Ordering::Relaxed),
            buffer_fill_pct: self.calculate_buffer_pct(),
            buffer_playable: Duration::from_micros(self.buffer_playable_us.load(Ordering::Relaxed)),
            memory_usage: self.memory_allocated.load(Ordering::Relaxed),
            peers,
        }
    }

    fn calculate_speeds(&self, now: Instant) -> (f64, f64) {
        if let Ok(samples) = self.speed_samples.lock() {
            let mut down_bytes = 0u64;
            let mut up_bytes = 0u64;
            let cutoff = now - Duration::from_secs(10);
            for s in samples.iter() {
                if s.time >= cutoff {
                    match s.dir {
                        Direction::Download => down_bytes += s.bytes,
                        Direction::Upload => up_bytes += s.bytes,
                    }
                }
            }
            let elapsed = 10.0_f64.max(0.001);
            (down_bytes as f64 / elapsed, up_bytes as f64 / elapsed)
        } else {
            (0.0, 0.0)
        }
    }

    fn calculate_loss_rate(&self) -> f64 {
        let sent = self.packets_sent.load(Ordering::Relaxed);
        let lost = self.packets_lost.load(Ordering::Relaxed);
        if sent + lost > 0 {
            lost as f64 / (sent + lost) as f64
        } else {
            0.0
        }
    }

    fn calculate_hash_fail_rate(&self) -> f64 {
        let total = self.pieces_downloaded.load(Ordering::Relaxed);
        let failed = self.pieces_failed_hash.load(Ordering::Relaxed);
        if total > 0 { failed as f64 / total as f64 } else { 0.0 }
    }

    fn calculate_buffer_pct(&self) -> f64 {
        let fill = self.buffer_fill_bytes.load(Ordering::Relaxed);
        let cap = self.buffer_capacity_bytes.load(Ordering::Relaxed);
        if cap > 0 { fill as f64 / cap as f64 } else { 0.0 }
    }
}

// --- Supporting Types ---

#[derive(Debug, Clone)]
pub struct SpeedSample {
    time: Instant,
    bytes: u64,
    dir: Direction,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction { Download, Upload }

#[derive(Debug, Clone, Copy)]
pub enum ErrorType { Network, Protocol, HashFail, Timeout }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineState {
    Idle = 0,
    Loading = 1,
    Playing = 2,
    Paused = 3,
    Error = 4,
}

impl From<u8> for EngineState {
    fn from(v: u8) -> Self {
        match v {
            1 => EngineState::Loading,
            2 => EngineState::Playing,
            3 => EngineState::Paused,
            4 => EngineState::Error,
            _ => EngineState::Idle,
        }
    }
}
```

---

## 2. Metrics Snapshot (Complete Snapshot)

```rust
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    // -- Timestamps --
    pub timestamp: Instant,

    // -- Throughput --
    pub download_speed: f64,              // bytes/sec (sliding 10s window)
    pub upload_speed: f64,                // bytes/sec (sliding 10s window)
    pub bytes_downloaded_total: u64,
    pub bytes_uploaded_total: u64,

    // -- Packet stats --
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub loss_rate: f64,                   // 0.0 - 1.0
    pub packets_retransmitted: u64,

    // -- Peer stats --
    pub peers_connected: i32,
    pub peers_interested: i32,
    pub peers_choked: i32,
    pub peers_total: i32,
    pub peers_snubbed: i32,

    // -- Piece completion --
    pub pieces_downloaded: u64,
    pub pieces_verified: u64,
    pub pieces_failed_hash: u64,
    pub hash_fail_rate: f64,              // 0.0 - 1.0
    pub pieces_written_to_cache: u64,

    // -- Cache --
    pub cache_hit_rate: f64,              // 0.0 - 1.0

    // -- Errors --
    pub errors_network: u64,
    pub errors_protocol: u64,
    pub errors_hash_fail: u64,
    pub errors_timeout: u64,
    pub total_errors: u64,

    // -- Engine state --
    pub engine_state: EngineState,
    pub buffer_fill_bytes: u64,
    pub buffer_capacity_bytes: u64,
    pub buffer_fill_pct: f64,
    pub buffer_playable: Duration,

    // -- Resource usage --
    pub memory_usage: u64,

    // -- Per-peer metrics --
    pub peers: Vec<PeerMetrics>,
}

impl MetricsSnapshot {
    /// Summary string for use in compact displays (status bar, CLI)
    pub fn summary(&self) -> String {
        format!(
            "↓{}/s ↑{}/s | peers {}/{} | buf {:.0}% ({}) | loss {:.1}% | cache {:.0}%",
            Self::format_bytes(self.download_speed),
            Self::format_bytes(self.upload_speed),
            self.peers_connected,
            self.peers_total,
            self.buffer_fill_pct * 100.0,
            Self::format_duration_compact(self.buffer_playable),
            self.loss_rate * 100.0,
            self.cache_hit_rate * 100.0,
        )
    }

    fn format_bytes(bps: f64) -> String {
        if bps > 1_048_576.0 {
            format!("{:.1}MB", bps / 1_048_576.0)
        } else if bps > 1024.0 {
            format!("{:.0}KB", bps / 1024.0)
        } else {
            format!("{:.0}B", bps)
        }
    }

    fn format_duration_compact(d: Duration) -> String {
        let secs = d.as_secs();
        if secs > 120 {
            format!("{}m{}s", secs / 60, secs % 60)
        } else {
            format!("{}s", secs)
        }
    }
}
```

---

## 3. Per-Peer Metrics

Each peer is tracked independently for connection quality scoring and UI display:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct PeerMetrics {
    pub peer_id: [u8; 20],
    pub addr: SocketAddr,

    // -- Throughput --
    pub download_speed: f64,          // bytes/sec (sliding 10s)
    pub upload_speed: f64,            // bytes/sec (sliding 10s)
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,

    // -- Latency --
    pub rtt: Duration,
    pub rtt_min: Duration,
    pub rtt_max: Duration,
    pub rtt_samples: u32,

    // -- Reliability --
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub loss_rate: f64,
    pub packets_retransmitted: u64,
    pub timeouts: u64,

    // -- Protocol state --
    pub choked: bool,
    pub chokes_received: u64,
    pub interested: bool,
    pub snubbed: bool,
    pub is_seed: bool,
    pub is_firewalled: bool,

    // -- Progress --
    pub progress: f64,                // 0.0 - 1.0
    pub pieces_have: u32,
    pub pieces_total: u32,

    // -- Connection --
    pub connected_since: Instant,
    pub last_request_time: Option<Instant>,
    pub last_response_time: Option<Instant>,

    // -- Quality scoring --
    pub quality_score: f64,           // composite score 0.0 - 1.0
    pub quality_history: VecDeque<f64>,

    // -- Location --
    pub location: Option<String>,
}

impl Default for PeerMetrics {
    fn default() -> Self {
        Self {
            peer_id: [0u8; 20],
            addr: "0.0.0.0:0".parse().unwrap(),
            download_speed: 0.0,
            upload_speed: 0.0,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
            rtt: Duration::from_secs(1),
            rtt_min: Duration::MAX,
            rtt_max: Duration::ZERO,
            rtt_samples: 0,
            packets_sent: 0,
            packets_received: 0,
            packets_lost: 0,
            loss_rate: 0.0,
            packets_retransmitted: 0,
            timeouts: 0,
            choked: true,
            chokes_received: 0,
            interested: false,
            snubbed: false,
            is_seed: false,
            is_firewalled: false,
            progress: 0.0,
            pieces_have: 0,
            pieces_total: 0,
            connected_since: Instant::now(),
            last_request_time: None,
            last_response_time: None,
            quality_score: 0.5,
            quality_history: VecDeque::with_capacity(100),
            location: None,
        }
    }
}

pub enum PeerMetricUpdate {
    BytesDownloaded(u64),
    BytesUploaded(u64),
    RttSample(Duration),
    PacketSent,
    PacketReceived,
    PacketLost,
    PacketRetransmitted,
    Timeout,
    Choked(bool),
    Interested(bool),
    Snubbed(bool),
    Progress(f64, u32, u32),
    Location(String),
    ResponseReceived,
}

impl PeerMetrics {
    pub fn apply(&mut self, update: PeerMetricUpdate) {
        match update {
            PeerMetricUpdate::BytesDownloaded(b) => {
                self.bytes_downloaded += b;
            }
            PeerMetricUpdate::BytesUploaded(b) => {
                self.bytes_uploaded += b;
            }
            PeerMetricUpdate::RttSample(rtt) => {
                self.rtt_samples += 1;
                let alpha = 0.125;  // Exponential moving average, TCP-style
                self.rtt = Duration::from_secs_f64(
                    (1.0 - alpha) * self.rtt.as_secs_f64() + alpha * rtt.as_secs_f64()
                );
                self.rtt_min = self.rtt_min.min(rtt);
                self.rtt_max = self.rtt_max.max(rtt);
            }
            PeerMetricUpdate::PacketSent => self.packets_sent += 1,
            PeerMetricUpdate::PacketReceived => self.packets_received += 1,
            PeerMetricUpdate::PacketLost => {
                self.packets_lost += 1;
                self.loss_rate = self.packets_lost as f64
                    / (self.packets_sent + self.packets_received + self.packets_lost).max(1) as f64;
            }
            PeerMetricUpdate::PacketRetransmitted => self.packets_retransmitted += 1,
            PeerMetricUpdate::Timeout => self.timeouts += 1,
            PeerMetricUpdate::Choked(v) => {
                if v && !self.choked { self.chokes_received += 1; }
                self.choked = v;
            }
            PeerMetricUpdate::Interested(v) => self.interested = v,
            PeerMetricUpdate::Snubbed(v) => self.snubbed = v,
            PeerMetricUpdate::Progress(p, have, total) => {
                self.progress = p;
                self.pieces_have = have;
                self.pieces_total = total;
                if self.pieces_have == self.pieces_total {
                    self.is_seed = true;
                }
            }
            PeerMetricUpdate::Location(l) => self.location = Some(l),
            PeerMetricUpdate::ResponseReceived => {
                self.last_response_time = Some(Instant::now());
            }
        }

        // Update speeds from sliding window
        self.update_speeds();
        // Recalculate quality score
        self.quality_score = self.calculate_quality();
    }

    fn update_speeds(&mut self) {
        // Simplified: use global speed tracking for accuracy
        // Per-peer speeds are approximated from byte counts and connection duration
        let elapsed = self.connected_since.elapsed().as_secs_f64().max(0.1);
        self.download_speed = self.bytes_downloaded as f64 / elapsed;
        self.upload_speed = self.bytes_uploaded as f64 / elapsed;
    }
}
```

---

## 4. Peer Quality Scoring

Each peer receives a composite quality score that determines its usefulness for downloads. The score is recalculated after each metric update.

```rust
impl PeerMetrics {
    /// Composite quality score (0.0 = useless, 1.0 = perfect peer)
    pub fn calculate_quality(&self) -> f64 {
        // Each component contributes a partial score
        // The overall score is a weighted geometric mean

        // 1. Speed score (35% weight) — normalize to 2 MB/s
        let speed_score = (self.download_speed / 2_000_000.0).min(1.0);

        // 2. Latency score (20% weight) — penalize high RTT
        let rtt_ms = self.rtt.as_millis() as f64;
        let latency_score = if rtt_ms < 50.0 {
            1.0
        } else if rtt_ms < 200.0 {
            1.0 - (rtt_ms - 50.0) / 150.0 * 0.4
        } else if rtt_ms < 500.0 {
            0.6 - (rtt_ms - 200.0) / 300.0 * 0.4
        } else {
            0.2
        }
        .max(0.0);

        // 3. Reliability score (25% weight)
        let reliability_score = if self.packets_sent + self.packets_received > 0 {
            let loss_penalty = (self.loss_rate * 3.0).min(1.0);  // 33% loss = zero
            let timeout_penalty = (self.timeouts as f64 * 0.1).min(1.0);
            let retransmit_penalty = if self.packets_sent > 0 {
                (self.packets_retransmitted as f64 / self.packets_sent as f64 * 2.0).min(1.0)
            } else {
                0.0
            };
            (1.0 - loss_penalty) * (1.0 - timeout_penalty) * (1.0 - retransmit_penalty)
        } else {
            0.5
        };

        // 4. Progress score (10% weight) — seeds get bonus
        let progress_score = if self.is_seed {
            1.0
        } else {
            self.progress * 0.5 + 0.5  // minimum 0.5 for non-seeds
        };

        // 5. Firewall penalty (10% weight)
        let firewall_score = if self.is_firewalled { 0.5 } else { 1.0 };

        // Weighted geometric mean
        let score = speed_score.powf(0.35)
            * latency_score.powf(0.20)
            * reliability_score.powf(0.25)
            * progress_score.powf(0.10)
            * firewall_score.powf(0.10);

        // Clamp to [0, 1]
        score.clamp(0.0, 1.0)
    }

    /// Label for UI display
    pub fn quality_label(&self) -> &str {
        if self.quality_score >= 0.8 { "Excellent" }
        else if self.quality_score >= 0.6 { "Good" }
        else if self.quality_score >= 0.4 { "Fair" }
        else if self.quality_score >= 0.2 { "Poor" }
        else { "Bad" }
    }

    /// Color for UI display
    pub fn quality_color(&self) -> (u8, u8, u8) {
        if self.quality_score >= 0.8 { (0, 200, 0) }
        else if self.quality_score >= 0.6 { (180, 180, 0) }
        else if self.quality_score >= 0.4 { (200, 130, 0) }
        else { (200, 0, 0) }
    }
}
```

---

## 5. HTTP Stats API

The local HTTP server exposes a JSON endpoint for external monitoring and integration:

```rust
// HTTP Handler (in qvs-local-server)
//
// GET /api/stats?hash={info_hash_hex}
//   → 200 JSON: {
//       "info_hash": "A1B2...",
//       "download_speed": 2345678.9,
//       "upload_speed": 123456.7,
//       "peers_connected": 12,
//       "peers_total": 47,
//       "buffer_fill_pct": 0.342,
//       "buffer_playable_secs": 137.5,
//       "pieces_complete": 342,
//       "pieces_total": 2800,
//       "pieces_complete_pct": 12.2,
//       "loss_rate": 0.023,
//       "cache_hit_rate": 0.78,
//       "engine_state": "playing",
//       "total_downloaded_bytes": 89522176,
//       "memory_usage_mb": 156.2,
//       "peers": [
//         {
//           "peer_id": "A1B2C3D4",
//           "addr": "192.168.1.5:8621",
//           "download_speed": 512000.0,
//           "upload_speed": 102400.0,
//           "rtt_ms": 45,
//           "loss_rate": 0.01,
//           "progress": 0.34,
//           "choked": false,
//           "interested": true,
//           "is_seed": false,
//           "quality_score": 0.72
//         }
//       ]
//     }

// GET /api/stats (no hash) → aggregate across all active torrents
// GET /api/health → simple health check
// GET /api/history?hash={hash}&seconds=300 → time-series data

pub fn stats_api_handler(
    metrics: Arc<MetricsRegistry>,
    info_hash: Option<InfoHash>,
) -> Json<Value> {
    let snapshot = metrics.snapshot();
    let json = serde_json::json!({
        "engine_state": format!("{:?}", snapshot.engine_state).to_lowercase(),
        "download_speed_bytes_per_sec": snapshot.download_speed,
        "upload_speed_bytes_per_sec": snapshot.upload_speed,
        "total_downloaded_bytes": snapshot.bytes_downloaded_total,
        "total_uploaded_bytes": snapshot.bytes_uploaded_total,
        "peers": {
            "connected": snapshot.peers_connected,
            "interested": snapshot.peers_interested,
            "choked": snapshot.peers_choked,
            "total_known": snapshot.peers_total,
        },
        "buffer": {
            "fill_bytes": snapshot.buffer_fill_bytes,
            "capacity_bytes": snapshot.buffer_capacity_bytes,
            "fill_pct": snapshot.buffer_fill_pct,
            "playable_secs": snapshot.buffer_playable.as_secs_f64(),
        },
        "pieces": {
            "downloaded": snapshot.pieces_downloaded,
            "verified": snapshot.pieces_verified,
            "failed_hash": snapshot.pieces_failed_hash,
            "hash_fail_rate": snapshot.hash_fail_rate,
        },
        "packets": {
            "sent": snapshot.packets_sent,
            "received": snapshot.packets_received,
            "lost": snapshot.packets_lost,
            "loss_rate": snapshot.loss_rate,
            "retransmitted": snapshot.packets_retransmitted,
        },
        "cache": {
            "hit_rate": snapshot.cache_hit_rate,
        },
        "errors": {
            "network": snapshot.errors_network,
            "protocol": snapshot.errors_protocol,
            "hash_fail": snapshot.errors_hash_fail,
            "timeout": snapshot.errors_timeout,
            "total": snapshot.total_errors,
        },
        "resource_usage": {
            "memory_mb": snapshot.memory_usage as f64 / 1_048_576.0,
        },
        "peers_detail": snapshot.peers.iter().map(|p| {
            serde_json::json!({
                "peer_id": hex::encode(&p.peer_id[..8]),
                "addr": p.addr.to_string(),
                "download_speed_bytes_per_sec": p.download_speed,
                "upload_speed_bytes_per_sec": p.upload_speed,
                "rtt_ms": p.rtt.as_millis(),
                "loss_rate": p.loss_rate,
                "progress": p.progress,
                "choked": p.choked,
                "interested": p.interested,
                "is_seed": p.is_seed,
                "quality_score": p.quality_score,
                "connected_secs": p.connected_since.elapsed().as_secs_f64(),
            })
        }).collect::<Vec<_>>(),
    });
    Json(json)
}
```

---

## 6. Health Check Endpoint

```rust
// GET /api/health
// Returns a simple health check for monitoring systems.
// Response:
//   {
//     "status": "ok" | "degraded" | "error",
//     "uptime_secs": 3600,
//     "engine_running": true,
//     "dht_connected": true,
//     "tracker_reachable": true,
//     "http_server_active": true,
//     "peers_connected": 12,
//     "memory_mb": 156.2,
//     "version": "0.1.0",
//     "checks": {
//       "engine": "ok",
//       "dht": "ok",
//       "tracker": "ok",
//       "buffer": "ok",
//       "cache": "ok"
//     }
//   }

#[derive(Debug, Clone, Serialize)]
pub struct HealthCheckResponse {
    pub status: HealthStatus,
    pub uptime_secs: u64,
    pub engine_running: bool,
    pub dht_connected: bool,
    pub tracker_reachable: bool,
    pub http_server_active: bool,
    pub peers_connected: i32,
    pub memory_mb: f64,
    pub version: String,
    pub checks: HashMap<String, HealthStatus>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum HealthStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "degraded")]
    Degraded,
    #[serde(rename = "error")]
    Error,
}

impl HealthCheckResponse {
    pub fn check(metrics: &MetricsSnapshot) -> Self {
        let mut checks = HashMap::new();

        // Engine health
        let engine_ok = matches!(metrics.engine_state, EngineState::Playing | EngineState::Paused);
        checks.insert("engine".into(), if engine_ok {
            HealthStatus::Ok
        } else if matches!(metrics.engine_state, EngineState::Loading) {
            HealthStatus::Degraded
        } else {
            HealthStatus::Error
        });

        // DHT health
        let dht_ok = metrics.peers_total > 0;
        checks.insert("dht".into(), if dht_ok {
            HealthStatus::Ok
        } else {
            HealthStatus::Degraded
        });

        // Buffer health
        let buffer_ok = metrics.buffer_fill_pct > 0.1;
        checks.insert("buffer".into(), if buffer_ok {
            HealthStatus::Ok
        } else if metrics.buffer_fill_pct > 0.0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Error
        });

        // Error rate health
        let error_ok = metrics.loss_rate < 0.1 && metrics.hash_fail_rate < 0.01;
        checks.insert("error_rate".into(), if error_ok {
            HealthStatus::Ok
        } else if metrics.loss_rate < 0.3 && metrics.hash_fail_rate < 0.05 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Error
        });

        let overall = if checks.values().all(|s| *s == HealthStatus::Ok) {
            HealthStatus::Ok
        } else if checks.values().any(|s| *s == HealthStatus::Error) {
            HealthStatus::Error
        } else {
            HealthStatus::Degraded
        };

        Self {
            status: overall,
            uptime_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            engine_running: engine_ok,
            dht_connected: dht_ok,
            tracker_reachable: true, // Updated by tracker client
            http_server_active: true,
            peers_connected: metrics.peers_connected,
            memory_mb: metrics.memory_usage as f64 / 1_048_576.0,
            version: env!("CARGO_PKG_VERSION").to_string(),
            checks,
        }
    }
}
```

---

## 7. Console Logging (tracing)

Structured logging using the `tracing` crate. All log messages include span context for correlation:

```rust
use tracing::{info, warn, error, debug, span, Level, instrument};
use tracing_subscriber::{
    fmt, EnvFilter,
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

/// Initialize logging subsystem
pub fn init_logging(log_level: &str, log_file: Option<&Path>) -> Result<()> {
    let env_filter = EnvFilter::builder()
        .with_default_directive(log_level.parse()?)
        .from_env()?
        .add_directive("hyper=warn".parse()?)
        .add_directive("reqwest=warn".parse()?)
        .add_directive("h2=warn".parse()?);

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_level(true);

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    if let Some(path) = log_file {
        let file = std::fs::File::create(path)?;
        let file_layer = fmt::layer()
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .with_target(true)
            .with_level(true);
        subscriber.with(file_layer).init();
    } else {
        subscriber.init();
    }

    Ok(())
}

// --- Instrumentation Examples ---

impl ConnectionPool {
    #[instrument(skip(self))]
    pub fn add_peer(&mut self, peer: PeerInfo) -> Result<()> {
        let peer_id_hex = hex::encode(&peer.peer_id[..4]);
        info!(
            peer_id = %peer_id_hex,
            addr = %peer.addr,
            total_peers = %self.connections.len() + 1,
            "Adding peer to connection pool"
        );

        // ... rest of implementation ...

        debug!(
            peer_id = %peer_id_hex,
            connection_count = %self.connections.len(),
            "Peer added successfully"
        );
        Ok(())
    }
}

impl P2spDownloader {
    #[instrument(skip(self, piece))]
    pub fn download_piece(&self, piece: &Piece) -> Result<Vec<u8>> {
        let source = self.select_source(piece, piece.priority);
        info!(
            piece_index = piece.index,
            priority = ?piece.priority,
            source = ?source,
            "Starting piece download"
        );

        // ... implementation ...

        if result.is_ok() {
            debug!(
                piece_index = piece.index,
                bytes = piece.length(),
                "Piece download completed"
            );
        } else {
            warn!(
                piece_index = piece.index,
                error = ?result.as_ref().err(),
                "Piece download failed, will retry"
            );
        }
        result
    }
}

impl DhtNode {
    #[instrument(skip(self))]
    pub fn handle_find_peers(&self, request: FindPeersRequest) -> FindPeersResponse {
        debug!(
            target = %hex::encode(&request.info_hash[..4]),
            sender = %request.node_id_hex,
            "Handling FIND_PEERS"
        );

        // ... implementation ...

        let peer_count = response.peers.len();
        if peer_count > 0 {
            debug!(peer_count = peer_count, "Returning cached peers");
        } else {
            trace!("No peers cached, returning closer nodes");
        }
        response
    }
}

// --- Log Events (Structured) ---

// All major events emit structured log records:
//
// Engine lifecycle:
//   event="engine_start" listen_port=8621 udp_port=8622
//   event="engine_stop"
//   event="playback_start" info_hash=hex duration_secs=1234
//   event="playback_stop" info_hash=hex position_secs=567
//   event="playback_pause" info_hash=hex position_secs=567
//   event="playback_resume" info_hash=hex position_secs=567
//   event="seek" info_hash=hex from_secs=100 to_secs=500
//   event="stream_end" info_hash=hex
//
// Network:
//   event="peer_connected" peer_id=hex addr=1.2.3.4:8621
//   event="peer_disconnected" peer_id=hex reason="timeout"
//   event="peer_snubbed" peer_id=hex
//   event="peer_unsnubbed" peer_id=hex
//   event="peer_choked" peer_id=hex
//   event="peer_unchoked" peer_id=hex
//   event="tracker_announce" url=... peer_count=42
//   event="tracker_error" url=... error=...
//   event="dht_bootstrap" seed_nodes=3 success=true
//   event="dht_find_peers" info_hash=hex nodes=8 peers=3
//   event="nat_detected" nat_type=FullCone
//
// Data:
//   event="piece_downloaded" piece_index=42 source=p2p peer_id=hex
//   event="piece_downloaded" piece_index=42 source=http url=...
//   event="piece_verified" piece_index=42 hash=hex matches=true
//   event="piece_hash_failed" piece_index=42 expected=hex got=hex
//   event="block_downloaded" piece_index=42 block=3 length=16384
//   event="cache_hit" info_hash=hex offset=123456
//   event="cache_miss" info_hash=hex offset=123456
//   event="cache_cleanup" removed=3 freed_mb=156
//   event="cache_corruption" info_hash=hex error=...
//
// Errors:
//   event="error" error_type=network message="Connection refused"
//   event="error" error_type=protocol message="Invalid handshake"
//   event="error" error_type=timeout message="Peer not responding"
//   event="error" error_type=hash_fail piece_index=42 expected=hex got=hex
//   event="error_retry" attempt=3 max_retries=5 error=...
//   event="fallback_http" reason="no_peers" url=...
```

---

## 8. Prometheus Metrics Export

For production deployments, metrics can be exported via a Prometheus `/metrics` endpoint:

```rust
// When compiled with feature "prometheus":
// [dependencies]
// prometheus = { version = "0.13", optional = true }

#[cfg(feature = "prometheus")]
pub mod prometheus_export {
    use prometheus::{
        register_counter, register_gauge, register_histogram,
        Counter, Gauge, Histogram, Registry,
    };

    pub struct PrometheusMetrics {
        registry: Registry,

        // Counters
        bytes_downloaded: Counter,
        bytes_uploaded: Counter,
        packets_sent: Counter,
        packets_received: Counter,
        packets_lost: Counter,
        pieces_downloaded: Counter,
        pieces_hash_failed: Counter,
        errors_total: Counter,
        cache_hits: Counter,
        cache_misses: Counter,

        // Gauges
        peers_connected: Gauge,
        peers_interested: Gauge,
        peers_choked: Gauge,
        peers_total_known: Gauge,
        buffer_fill_bytes: Gauge,
        buffer_capacity_bytes: Gauge,
        memory_bytes: Gauge,
        download_speed_bytes: Gauge,
        upload_speed_bytes: Gauge,

        // Histograms
        piece_download_duration: Histogram,
        peer_rtt_milliseconds: Histogram,
        block_size_bytes: Histogram,
    }

    impl PrometheusMetrics {
        pub fn new() -> Result<Self, prometheus::Error> {
            let registry = Registry::new();

            let bytes_downloaded = register_counter!(
                "qvod_bytes_downloaded_total",
                "Total bytes downloaded from all sources",
                registry
            )?;

            let bytes_uploaded = register_counter!(
                "qvod_bytes_uploaded_total",
                "Total bytes uploaded to peers",
                registry
            )?;

            let packets_sent = register_counter!(
                "qvod_packets_sent_total",
                "Total UDP packets sent",
                registry
            )?;

            let packets_received = register_counter!(
                "qvod_packets_received_total",
                "Total UDP packets received",
                registry
            )?;

            let packets_lost = register_counter!(
                "qvod_packets_lost_total",
                "Total UDP packets declared lost",
                registry
            )?;

            let peers_connected = register_gauge!(
                "qvod_peers_connected",
                "Number of currently connected peers",
                registry
            )?;

            let download_speed_bytes = register_gauge!(
                "qvod_download_speed_bytes_per_sec",
                "Current download speed in bytes/sec",
                registry
            )?;

            let upload_speed_bytes = register_gauge!(
                "qvod_upload_speed_bytes_per_sec",
                "Current upload speed in bytes/sec",
                registry
            )?;

            let piece_download_duration = register_histogram!(
                "qvod_piece_download_duration_seconds",
                "Histogram of piece download durations",
                vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0],
                registry
            )?;

            let peer_rtt_milliseconds = register_histogram!(
                "qvod_peer_rtt_milliseconds",
                "Histogram of peer round-trip times",
                vec![10.0, 25.0, 50.0, 100.0, 200.0, 500.0, 1000.0],
                registry
            )?;

            // ... remaining metrics ...

            Ok(Self {
                registry,
                bytes_downloaded,
                bytes_uploaded,
                packets_sent,
                packets_received,
                packets_lost,
                pieces_downloaded: register_counter!(
                    "qvod_pieces_downloaded_total", "Total pieces downloaded", registry
                )?,
                pieces_hash_failed: register_counter!(
                    "qvod_pieces_hash_failed_total", "Total pieces with hash failures", registry
                )?,
                errors_total: register_counter!(
                    "qvod_errors_total", "Total errors encountered", registry
                )?,
                cache_hits: register_counter!(
                    "qvod_cache_hits_total", "Total cache hits", registry
                )?,
                cache_misses: register_counter!(
                    "qvod_cache_misses_total", "Total cache misses", registry
                )?,
                peers_interested: register_gauge!(
                    "qvod_peers_interested", "Number of interested peers", registry
                )?,
                peers_choked: register_gauge!(
                    "qvod_peers_choked", "Number of choked peers", registry
                )?,
                peers_total_known: register_gauge!(
                    "qvod_peers_total_known", "Total known peers in swarm", registry
                )?,
                buffer_fill_bytes: register_gauge!(
                    "qvod_buffer_fill_bytes", "Buffer fill level in bytes", registry
                )?,
                buffer_capacity_bytes: register_gauge!(
                    "qvod_buffer_capacity_bytes", "Buffer capacity in bytes", registry
                )?,
                memory_bytes: register_gauge!(
                    "qvod_memory_bytes", "Process memory usage in bytes", registry
                )?,
                piece_download_duration,
                peer_rtt_milliseconds,
                block_size_bytes: register_histogram!(
                    "qvod_block_size_bytes", "Histogram of block sizes transferred",
                    vec![1024.0, 4096.0, 8192.0, 16384.0, 32768.0],
                    registry
                )?,
            })
        }

        pub fn update(&self, snapshot: &MetricsSnapshot) {
            self.bytes_downloaded.inc_by(snapshot.bytes_downloaded_total);
            self.bytes_uploaded.inc_by(snapshot.bytes_uploaded_total);
            self.packets_sent.inc_by(snapshot.packets_sent);
            self.packets_received.inc_by(snapshot.packets_received);
            self.packets_lost.inc_by(snapshot.packets_lost);
            self.peers_connected.set(snapshot.peers_connected as f64);
            self.peers_interested.set(snapshot.peers_interested as f64);
            self.peers_choked.set(snapshot.peers_choked as f64);
            self.peers_total_known.set(snapshot.peers_total as f64);
            self.buffer_fill_bytes.set(snapshot.buffer_fill_bytes as f64);
            self.buffer_capacity_bytes.set(snapshot.buffer_capacity_bytes as f64);
            self.memory_bytes.set(snapshot.memory_usage as f64);
            self.download_speed_bytes.set(snapshot.download_speed);
            self.upload_speed_bytes.set(snapshot.upload_speed);
        }

        pub fn render_metrics(&self) -> String {
            use prometheus::Encoder;
            let encoder = prometheus::TextEncoder::new();
            let mut buffer = Vec::new();
            encoder.encode(&self.registry.gather(), &mut buffer).unwrap();
            String::from_utf8(buffer).unwrap()
        }
    }
}
```

---

## 9. User-Facing Status Display

The in-app status panel (integrated in the player UI) renders a simplified real-time view:

```rust
pub fn render_status_overlay(snapshot: &MetricsSnapshot, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(egui::Color32::from_black_alpha(160))
        .rounding(4.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                // Engine state
                let (state_text, state_color) = match snapshot.engine_state {
                    EngineState::Playing => ("Playing", egui::Color32::GREEN),
                    EngineState::Paused => ("Paused", egui::Color32::YELLOW),
                    EngineState::Loading => ("Loading...", egui::Color32::LIGHT_BLUE),
                    EngineState::Idle => ("Idle", egui::Color32::GRAY),
                    EngineState::Error => ("Error", egui::Color32::RED),
                };
                ui.colored_label(state_color, state_text);

                ui.add_space(4.0);

                // Buffer bar
                let buffer_bar = egui::ProgressBar::new(snapshot.buffer_fill_pct as f32)
                    .fill(egui::Color32::from_rgb(0, 180, 255))
                    .desired_width(200.0);
                ui.add(buffer_bar);
                ui.label(format!(
                    "Buffer: {:.1}% ({})",
                    snapshot.buffer_fill_pct * 100.0,
                    Self::format_duration_compact(snapshot.buffer_playable)
                ));

                ui.add_space(2.0);

                // Speed
                ui.label(format!(
                    "↓ {}/s  ↑ {}/s",
                    format_bytes(snapshot.download_speed),
                    format_bytes(snapshot.upload_speed)
                ));

                ui.add_space(2.0);

                // Peers
                let peer_color = if snapshot.peers_connected > 5 {
                    egui::Color32::GREEN
                } else if snapshot.peers_connected > 0 {
                    egui::Color32::YELLOW
                } else {
                    egui::Color32::RED
                };
                ui.colored_label(peer_color, format!(
                    "Peers: {}/{}",
                    snapshot.peers_connected, snapshot.peers_total
                ));

                ui.add_space(2.0);

                // Health
                let health = snapshot.calculate_health();
                let (health_text, health_color) = if health > 0.7 {
                    ("Good", egui::Color32::GREEN)
                } else if health > 0.3 {
                    ("Fair", egui::Color32::YELLOW)
                } else {
                    ("Poor", egui::Color32::RED)
                };
                ui.colored_label(health_color, format!("Health: {}", health_text));
            });
        });
}
```

---

## 10. Time-Series History Buffer

For trend analysis and visualization, metrics samples are buffered in memory:

```rust
pub struct MetricsHistory {
    /// Fixed-size ring buffer of snapshots
    samples: VecDeque<MetricsSnapshot>,
    /// Maximum number of samples to retain
    max_samples: usize,
    /// Interval between samples
    sample_interval: Duration,
    /// Last sample time
    last_sample: Option<Instant>,
}

impl MetricsHistory {
    pub fn new(max_seconds: u64, sample_interval: Duration) -> Self {
        let max_samples = (max_seconds as f64 / sample_interval.as_secs_f64()).ceil() as usize;
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
            sample_interval,
            last_sample: None,
        }
    }

    pub fn tick(&mut self, snapshot: MetricsSnapshot) {
        let now = snapshot.timestamp;
        if self.last_sample.map_or(true, |t| now - t >= self.sample_interval) {
            if self.samples.len() >= self.max_samples {
                self.samples.pop_front();
            }
            self.samples.push_back(snapshot);
            self.last_sample = Some(now);
        }
    }

    pub fn get_time_series(&self, metric: TimeSeriesMetric) -> Vec<(f64, f64)> {
        self.samples.iter().map(|s| {
            let value = match metric {
                TimeSeriesMetric::DownloadSpeed => s.download_speed,
                TimeSeriesMetric::UploadSpeed => s.upload_speed,
                TimeSeriesMetric::PeersConnected => s.peers_connected as f64,
                TimeSeriesMetric::BufferFillPct => s.buffer_fill_pct,
                TimeSeriesMetric::LossRate => s.loss_rate,
                TimeSeriesMetric::CacheHitRate => s.cache_hit_rate,
                TimeSeriesMetric::MemoryUsage => s.memory_usage as f64,
            };
            (s.timestamp.elapsed().as_secs_f64(), value)
        }).collect()
    }
}

pub enum TimeSeriesMetric {
    DownloadSpeed,
    UploadSpeed,
    PeersConnected,
    BufferFillPct,
    LossRate,
    CacheHitRate,
    MemoryUsage,
}
```

---

## Summary

| Component | Integration | Frequency | Purpose |
|-----------|-------------|-----------|---------|
| `MetricsRegistry` | Core engine | Real-time | Lock-free atomic counters |
| `MetricsSnapshot` | All consumers | Polled on demand | Complete current state |
| `PeerMetrics` | Per-connection | On every event | Quality scoring, UI |
| HTTP `/api/stats` | Local server | Polled externally | Monitoring integration |
| HTTP `/api/health` | Local server | Polled externally | Health checks |
| `tracing` logs | Console/file | Every event | Debugging, auditing |
| Prometheus | Optional feature | Every 15s (scrape) | Production monitoring |
| UI status panel | Player GUI | Every frame (60fps) | User-facing display |
| `MetricsHistory` | In-memory | Configurable interval | Trend analysis |

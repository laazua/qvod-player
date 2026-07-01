# Piece Scheduling Module Specification

## Overview

The Piece Scheduler is the **intelligence core** of the QVOD streaming engine. It determines _which_ data to download _when_ from _which peer_. Unlike BitTorrent's sequential or rarest-first scheduler, QVOD's scheduler is **keyframe-driven** and **deadline-aware**: it prioritizes pieces needed for immediate playback, enabling instant start and arbitrary seeking.

### Core Principles

1. **Metadata before data** — The scheduler only activates after `FileMeta` (including `KeyFrameIndex`) is fully available.
2. **Non-sequential, sparse** — Pieces are not downloaded in file order. The scheduler jumps to keyframe positions first.
3. **Deadline-driven** — Each piece has a deadline (when it must be available for uninterrupted playback). Missed deadlines cause rebuffering.
4. **Priority tiers** — Four tiers (Critical, High, Normal, Low) with different source selection strategies.
5. **Dynamic re-prioritization** — Every seek event completely recalculates all priorities.

## Constants

```rust
/// Size of a single piece. Every piece (except the last) is exactly this size.
pub const PIECE_LENGTH: u64 = 256 * 1024; // 256 KB

/// Size of a single block. Pieces are requested as blocks from peers.
pub const BLOCK_LENGTH: u64 = 16 * 1024; // 16 KB

/// Number of blocks per piece (except the last piece may have fewer).
pub const BLOCKS_PER_PIECE: u32 = 16;

/// Maximum number of outstanding (in-flight) requests per peer.
pub const MAX_PIPELINE_PER_PEER: u32 = 5;

/// Global maximum number of pending block requests.
pub const MAX_GLOBAL_PENDING: u32 = 100;

/// Timeout before re-requesting a block from a different peer (milliseconds).
pub const BLOCK_REQUEST_TIMEOUT_MS: u64 = 15_000; // 15 seconds

/// How often the scheduler recalculates piece priorities (milliseconds).
pub const SCHEDULER_TICK_MS: u64 = 500; // 500 ms

/// Buffer duration thresholds (in milliseconds) for priority calculation.
pub const HIGH_PRIORITY_WINDOW_MS: u64 = 30_000;  // 30 seconds ahead
pub const NORMAL_PRIORITY_WINDOW_MS: u64 = 120_000; // 120 seconds ahead
pub const CRITICAL_BACKWARD_WINDOW_MS: u64 = 2_000; // 2 seconds behind playhead
```

## Core Data Structures

### PiecePriority

```rust
/// Priority level for a piece. Higher priority pieces are requested first.
/// The ordering ensures that playback-critical data is always preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PiecePriority {
    /// Piece required for immediate playback (currently at playhead).
    /// Downloaded via parallel P2P + HTTP with redundancy.
    /// Must be available within < 1 second.
    Critical = 0,

    /// Piece needed for the next 30 seconds of playback.
    /// Downloaded via P2P with HTTP fallback after 3s timeout.
    High = 1,

    /// Piece needed for the next 30-120 seconds of playback.
    /// Downloaded via P2P only, no HTTP urgency.
    Normal = 2,

    /// Piece already played or far in the future. Downloaded only
    /// when bandwidth is idle, primarily for upload contribution.
    Low = 3,
}

impl PiecePriority {
    /// Returns the source selection strategy for this priority.
    pub fn source_strategy(&self) -> SourceStrategy {
        match self {
            PiecePriority::Critical => SourceStrategy::ParallelP2PAndHttp,
            PiecePriority::High => SourceStrategy::P2PWithHttpFallback(Duration::from_secs(3)),
            PiecePriority::Normal => SourceStrategy::P2POnly,
            PiecePriority::Low => SourceStrategy::P2PIdle,
        }
    }

    /// Maximum number of concurrent block requests for pieces at this priority.
    pub fn max_concurrency(&self) -> u32 {
        match self {
            PiecePriority::Critical => 8,
            PiecePriority::High => 5,
            PiecePriority::Normal => 3,
            PiecePriority::Low => 1,
        }
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            PiecePriority::Critical => "CRITICAL",
            PiecePriority::High => "HIGH",
            PiecePriority::Normal => "NORMAL",
            PiecePriority::Low => "LOW",
        }
    }
}
```

### BlockRequest

```rust
/// A request for a single block (16 KB) of a piece.
/// Blocks are the atomic unit of data transfer in QVOD.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockRequest {
    /// Index of the piece containing this block.
    pub piece_index: u32,

    /// Byte offset within the piece where this block begins.
    pub begin: u32,

    /// Length of this block in bytes (typically 16 KB, last block may be shorter).
    pub length: u32,

    /// Priority of the parent piece at the time this request was created.
    pub priority: PiecePriority,

    /// When this request was created (for timeout tracking).
    pub created_at: Instant,

    /// Which peer this request was sent to (None if not yet assigned).
    pub assigned_peer: Option<PeerId>,
}

impl BlockRequest {
    /// Create a new block request.
    pub fn new(piece_index: u32, begin: u32, length: u32, priority: PiecePriority) -> Self {
        Self {
            piece_index,
            begin,
            length,
            priority,
            created_at: Instant::now(),
            assigned_peer: None,
        }
    }

    /// Check if this request has timed out.
    pub fn is_timed_out(&self, timeout: Duration) -> bool {
        self.created_at.elapsed() > timeout
    }

    /// The end offset (exclusive) of this block within the piece.
    pub fn end(&self) -> u32 {
        self.begin + self.length
    }
}
```

### PieceState

```rust
/// Tracks the download state of a single piece.
#[derive(Debug, Clone)]
pub struct PieceState {
    /// Index of this piece.
    pub index: u32,

    /// Current priority level.
    pub priority: PiecePriority,

    /// Number of bytes downloaded.
    pub downloaded: u64,

    /// Bitmask of completed blocks (bit i set = block i complete).
    pub block_bitmask: u64, // up to 64 blocks, we use 16

    /// Total blocks in this piece (usually 16, last piece may have fewer).
    pub total_blocks: u32,

    /// Number of failed download attempts for this piece.
    pub failures: u32,

    /// Whether this piece's data has been verified against SHA-1 hash.
    pub verified: bool,

    /// Whether this piece has been written to the ring buffer.
    pub written_to_buffer: bool,

    /// Human-readable state summary.
    pub fn completion(&self) -> f64 {
        self.downloaded as f64 / (self.total_blocks as u64 * BLOCK_LENGTH) as f64
    }

    /// Whether all blocks are completed.
    pub fn is_complete(&self) -> bool {
        let mask = if self.total_blocks < 64 {
            (1u64 << self.total_blocks) - 1
        } else {
            !0u64
        };
        self.block_bitmask == mask
    }

    /// Mark a block as completed.
    pub fn mark_block_complete(&mut self, block_index: u32) {
        self.block_bitmask |= 1u64 << block_index;
        self.downloaded += BLOCK_LENGTH.min(
            // last piece block may be smaller
            (self.index as u64 + 1) * PIECE_LENGTH
        );
    }
}
```

### DownloadStats

```rust
/// Aggregate statistics about the download process.
#[derive(Debug, Clone, Default)]
pub struct DownloadStats {
    /// Current download speed in bytes/sec (sliding window over 10 seconds).
    pub download_speed: f64,

    /// Current upload speed in bytes/sec.
    pub upload_speed: f64,

    /// Total bytes downloaded.
    pub total_downloaded: u64,

    /// Total bytes uploaded.
    pub total_uploaded: u64,

    /// Number of peers currently connected.
    pub active_peers: u32,

    /// Number of pieces completed and verified.
    pub pieces_completed: u32,

    /// Number of pieces in Critical priority.
    pub critical_pieces: u32,

    /// Number of pieces in High priority.
    pub high_pieces: u32,

    /// Number of pieces in Normal priority.
    pub normal_pieces: u32,

    /// Number of pieces in Low priority.
    pub low_pieces: u32,

    /// Number of piece verification failures.
    pub verification_failures: u32,

    /// Current buffer fill level in milliseconds of playable content.
    pub buffered_ms: u64,
}
```

## PieceScheduler

### Main Structure

```rust
/// The central piece scheduling engine.
/// Drives _what_ to download, _when_, and from _which_ source.
///
/// Thread safety: The scheduler is wrapped in `Arc<Mutex<>>` and shared
/// between the download engine (writer) and the playback engine (reader).
pub struct PieceScheduler {
    /// File metadata including keyframe index.
    metadata: Arc<FileMeta>,

    /// State for every piece in the file.
    pieces: Vec<PieceState>,

    /// Current playback head position (byte offset in the file).
    playhead: u64,

    /// The piece index currently being played (playhead / piece_length).
    current_piece: u32,

    /// Deadline-aware priority levels for each piece, recalculated on seek/tick.
    priorities: Vec<PiecePriority>,

    /// Queue of pending block requests, sorted by priority then deadline.
    pending_queue: BinaryHeap<PendingBlock>,

    /// Requests currently in-flight (assigned to peers but not yet completed).
    in_flight: HashMap<u64, BlockRequest>, // key: (piece_index << 32) | block_index

    /// Timestamp of last priority recalculation.
    last_recalc: Instant,

    /// Whether a seek is in progress (temporarily boosts target pieces).
    seeking: bool,

    /// Target piece index during seek (becomes Critical until downloaded).
    seek_target: Option<u32>,

    /// Download statistics.
    stats: DownloadStats,

    /// Configuration.
    config: SchedulerConfig,
}

/// Configuration parameters for the scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Number of blocks above the playhead to pre-fetch as Critical.
    pub critical_ahead_blocks: u32,

    /// Milliseconds of content to keep as Critical buffer ahead.
    pub critical_buffer_ms: u64,

    /// Milliseconds of content to keep as High priority.
    pub high_buffer_ms: u64,

    /// Milliseconds of content to keep as Normal priority.
    pub normal_buffer_ms: u64,

    /// Whether to enable rarest-first within same priority tier.
    pub rarest_first: bool,

    /// Maximum blocks in-flight per peer.
    pub max_pipeline: u32,

    /// Maximum global in-flight blocks.
    pub max_global_pending: u32,

    /// Block request timeout before re-requesting.
    pub block_timeout: Duration,

    /// Whether to use HTTP fallback for High priority pieces.
    pub http_fallback: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            critical_ahead_blocks: 10,
            critical_buffer_ms: 2_000,
            high_buffer_ms: 30_000,
            normal_buffer_ms: 120_000,
            rarest_first: true,
            max_pipeline: MAX_PIPELINE_PER_PEER,
            max_global_pending: MAX_GLOBAL_PENDING,
            block_timeout: Duration::from_millis(BLOCK_REQUEST_TIMEOUT_MS),
            http_fallback: true,
        }
    }
}
```

### Priority Calculation Engine

The heart of the scheduler: calculating which pieces are needed and how urgently.

```rust
impl PieceScheduler {
    /// Create a new scheduler from file metadata.
    pub fn new(metadata: Arc<FileMeta>) -> Self {
        let num_pieces = metadata.num_pieces();
        let pieces: Vec<PieceState> = (0..num_pieces)
            .map(|i| {
                let total_blocks = ((metadata.piece_size(i) + BLOCK_LENGTH - 1) / BLOCK_LENGTH) as u32;
                PieceState {
                    index: i,
                    priority: PiecePriority::Low,
                    downloaded: 0,
                    block_bitmask: 0,
                    total_blocks,
                    failures: 0,
                    verified: false,
                    written_to_buffer: false,
                }
            })
            .collect();

        let mut s = Self {
            metadata,
            pieces,
            playhead: 0,
            current_piece: 0,
            priorities: Vec::new(),
            pending_queue: BinaryHeap::new(),
            in_flight: HashMap::new(),
            last_recalc: Instant::now(),
            seeking: false,
            seek_target: None,
            stats: DownloadStats::default(),
            config: SchedulerConfig::default(),
        };
        s.recalculate_all_priorities();
        s
    }

    /// Recalculate priority for every piece in the file.
    /// Called on initialization and on every seek.
    pub fn recalculate_all_priorities(&mut self) {
        let num_pieces = self.metadata.num_pieces() as usize;
        self.priorities.resize(num_pieces, PiecePriority::Low);

        let playhead_offset = self.playhead;
        let playhead_piece = (playhead_offset / self.metadata.piece_length) as u32;

        // Determine the byte range for each priority tier
        let critical_end = self.offset_at_time_delta(self.config.critical_buffer_ms);
        let high_end = self.offset_at_time_delta(self.config.high_buffer_ms);
        let normal_end = self.offset_at_time_delta(self.config.normal_buffer_ms);

        // Also find keyframe pieces (they are always at least High priority)
        let keyframe_pieces: std::collections::HashSet<u32> = self
            .metadata
            .keyframe_index
            .entries
            .iter()
            .filter(|e| e.frame_type == FrameType::I)
            .map(|e| e.piece_index(self.metadata.piece_length))
            .collect();

        // Calculate priority for each piece
        let mut new_priorities = Vec::with_capacity(num_pieces);
        for i in 0..num_pieces {
            let piece_start = (i as u64) * self.metadata.piece_length;
            let piece_end = self.metadata.piece_byte_range(i as u32).end;

            let priority = if self.seek_target == Some(i as u32) {
                // Seek target is always Critical
                PiecePriority::Critical
            } else if piece_start <= playhead_offset && piece_end > playhead_offset {
                // Current playhead piece = Critical
                PiecePriority::Critical
            } else if piece_start >= playhead_offset && piece_start < critical_end {
                // Within critical buffer ahead
                PiecePriority::Critical
            } else if piece_start >= playhead_offset && piece_start < high_end {
                // Within high buffer ahead
                PiecePriority::High
            } else if piece_start >= playhead_offset && piece_start < normal_end {
                // Within normal buffer ahead
                PiecePriority::Normal
            } else if piece_end < playhead_offset
                && (playhead_offset - piece_end) < (self.config.critical_buffer_ms / 1000 * 100_000)
            {
                // Within critical buffer behind (for backward seek stability)
                PiecePriority::Critical
            } else if keyframe_pieces.contains(&(i as u32)) {
                // Keyframes outside the window still get High priority
                PiecePriority::High
            } else {
                PiecePriority::Low
            };

            new_priorities.push(priority);
        }

        self.priorities = new_priorities;
        self.last_recalc = Instant::now();
    }

    /// Convert a time delta (in ms) from the playhead to a byte offset.
    fn offset_at_time_delta(&self, delta_ms: u64) -> u64 {
        if self.metadata.duration_ms == 0 {
            return self.playhead + delta_ms * 100_000; // fallback: assume ~100 KB/s
        }
        let progress = self.playhead as f64 / self.metadata.file_size as f64;
        let remaining_fraction = 1.0 - progress;
        let remaining_ms = (self.metadata.duration_ms as f64 * remaining_fraction) as u64;

        if remaining_ms == 0 {
            return self.metadata.file_size;
        }

        let fraction = (delta_ms as f64) / (remaining_ms as f64).min(delta_ms as f64);
        let delta_bytes = ((self.metadata.file_size - self.playhead) as f64 * fraction.min(1.0)) as u64;
        self.playhead + delta_bytes
    }
}
```

### Request Selection

```rust
impl PieceScheduler {
    /// Get the next block request to send, considering priority and rarity.
    ///
    /// Returns `None` if there are no outstanding pieces or all requests are in-flight.
    pub fn next_request(&mut self, peer_bitfield: &Bitfield) -> Option<BlockRequest> {
        // 1. Check in-flight limits
        if self.in_flight.len() >= self.config.max_global_pending as usize {
            return None;
        }

        // 2. Collect candidate pieces by priority (highest first)
        let mut candidates: Vec<(PiecePriority, u32)> = (0..self.pieces.len() as u32)
            .filter(|&i| {
                !self.pieces[i as usize].is_complete()
                    && !self.pieces[i as usize].verified
                    && peer_bitfield.has(i) // peer has this piece
            })
            .map(|i| (self.priorities[i as usize], i))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // 3. Sort by priority (Critical first), then by deadline, then by rarity
        if self.config.rarest_first {
            // Within the same priority, prefer rarest pieces first
            candidates.sort_by(|a, b| {
                a.0.cmp(&b.0).then_with(|| {
                    // Count how many connected peers have this piece
                    let rarity_a = self.count_peers_with_piece(a.1);
                    let rarity_b = self.count_peers_with_piece(b.1);
                    rarity_a.cmp(&rarity_b)
                })
            });
        } else {
            candidates.sort_by_key(|(pri, idx)| (*pri, *idx));
        }

        // 4. Find the first candidate with an available block
        for (priority, piece_index) in candidates {
            if let Some(block) = self.next_block_for_piece(piece_index, priority) {
                return Some(block);
            }
        }

        None
    }

    /// Find the next unrequested block within a specific piece.
    fn next_block_for_piece(&self, piece_index: u32, priority: PiecePriority) -> Option<BlockRequest> {
        let state = &self.pieces[piece_index as usize];
        if state.is_complete() {
            return None;
        }

        // Find the first block that is not yet completed and not in-flight
        for block_index in 0..state.total_blocks {
            if (state.block_bitmask >> block_index) & 1 == 1 {
                continue; // already completed
            }

            let key = make_block_key(piece_index, block_index);
            if self.in_flight.contains_key(&key) {
                continue; // already requested
            }

            let begin = block_index * BLOCK_LENGTH as u32;
            let length = if block_index == state.total_blocks - 1 {
                // Last block may be shorter
                (self.metadata.piece_size(piece_index) - begin as u64) as u32
            } else {
                BLOCK_LENGTH as u32
            };

            return Some(BlockRequest::new(piece_index, begin, length, priority));
        }

        None
    }

    /// Count how many connected peers have a given piece (for rarest-first).
    fn count_peers_with_piece(&self, piece_index: u32) -> usize {
        // This would query the connection pool for peer bitfields.
        // For now, we return a placeholder; the actual implementation
        // is injected via a trait or callback.
        0
    }
}

/// Create a unique 64-bit key for tracking in-flight block requests.
fn make_block_key(piece_index: u32, block_index: u32) -> u64 {
    (piece_index as u64) << 32 | block_index as u64
}
```

### Seek Handling

```rust
impl PieceScheduler {
    /// Handle a seek event. Resets priorities around the new target position.
    ///
    /// Called by `SeekEngine::seek_to()` when the user drags the progress bar
    /// or issues a seek command.
    pub fn on_seek(&mut self, timestamp_ms: u64) {
        // 1. Find the nearest I-frame at or before the target timestamp
        let target_entry = self
            .metadata
            .keyframe_index
            .nearest_iframe(timestamp_ms)
            .unwrap_or_else(|| {
                // Fallback: first entry in the entire index
                self.metadata.keyframe_index.entries.first().unwrap()
            });

        // 2. Calculate the target piece index
        let target_piece = target_entry.piece_index(self.metadata.piece_length);

        // 3. Update seek state
        self.seeking = true;
        self.seek_target = Some(target_piece);
        self.playhead = target_entry.file_offset;
        self.current_piece = target_piece;

        // 4. Flush pending queue (old priorities are irrelevant)
        self.pending_queue.clear();

        // 5. Recalculate all priorities based on new playhead
        self.recalculate_all_priorities();

        // 6. The target piece is now Critical; the scheduler will
        //    prioritize it on the next tick()
    }

    /// Finalize a seek operation once the target piece is complete.
    pub fn finalize_seek(&mut self) {
        if let Some(target) = self.seek_target {
            if self.pieces[target as usize].is_complete() {
                self.seeking = false;
                self.seek_target = None;
            }
        }
    }

    /// Get the current playhead position in bytes.
    pub fn playhead_offset(&self) -> u64 {
        self.playhead
    }

    /// Get the current playhead position in milliseconds.
    pub fn playhead_ms(&self) -> u64 {
        self.metadata
            .keyframe_index
            .entries
            .iter()
            .rev()
            .find(|e| e.file_offset <= self.playhead)
            .map(|e| e.timestamp_ms)
            .unwrap_or(0)
    }
}
```

### Deadline-Aware Scheduling

The scheduler estimates when each piece is needed for uninterrupted playback:

```rust
impl PieceScheduler {
    /// Calculate the deadline for a piece — the maximum time (from now)
    /// by which this piece must be completely downloaded.
    ///
    /// Returns `None` for Low priority pieces (no deadline).
    pub fn piece_deadline(&self, piece_index: u32) -> Option<Duration> {
        let priority = self.priorities[piece_index as usize];

        match priority {
            PiecePriority::Critical => {
                // Critical pieces must arrive essentially immediately.
                // We estimate based on how far ahead the piece is from playhead.
                let piece_start = piece_index as u64 * self.metadata.piece_length;
                let distance = if piece_start >= self.playhead {
                    piece_start - self.playhead
                } else {
                    self.playhead - piece_start
                };

                // Critical pieces within ~1 second of playback
                let estimated_time_ms = (distance as f64 / 100_000.0) * 1000.0;
                Some(Duration::from_millis(estimated_time_ms as u64).max(Duration::from_millis(200)))
            }
            PiecePriority::High => {
                // High priority pieces have ~30 seconds of slack
                let piece_start = piece_index as u64 * self.metadata.piece_length;
                let distance = piece_start.saturating_sub(self.playhead);
                let slack_ms = 30_000u64;
                let needed_in_ms = (distance as f64 / 100_000.0 * 1000.0) as u64;
                Some(Duration::from_millis(needed_in_ms + slack_ms))
            }
            PiecePriority::Normal => {
                // Normal priority pieces have ~2 minutes of slack
                let piece_start = piece_index as u64 * self.metadata.piece_length;
                let distance = piece_start.saturating_sub(self.playhead);
                let slack_ms = 120_000u64;
                let needed_in_ms = (distance as f64 / 100_000.0 * 1000.0) as u64;
                Some(Duration::from_millis(needed_in_ms + slack_ms))
            }
            PiecePriority::Low => None, // No deadline
        }
    }

    /// Periodic tick called by the main engine loop.
    /// Recalculates priorities and cleans up timed-out requests.
    pub fn tick(&mut self) {
        let now = Instant::now();

        // 1. Periodically recalculate priorities (every 500ms)
        if now.duration_since(self.last_recalc) > Duration::from_millis(SCHEDULER_TICK_MS) {
            self.recalculate_all_priorities();
        }

        // 2. Re-request timed-out blocks
        let timed_out: Vec<u64> = self
            .in_flight
            .iter()
            .filter(|(_, req)| req.is_timed_out(self.config.block_timeout))
            .map(|(key, _)| *key)
            .collect();

        for key in timed_out {
            if let Some(mut req) = self.in_flight.remove(&key) {
                // Reset and re-queue
                req.assigned_peer = None;
                req.created_at = now;
                // Push back into the pending queue (or let next_request() pick it up)
                // The piece failure counter is incremented for peer selection
                self.pieces[req.piece_index as usize].failures += 1;
            }
        }

        // 3. Check if seek target is complete
        if self.seeking {
            self.finalize_seek();
        }
    }
}
```

### Request Pipeline Management

```rust
impl PieceScheduler {
    /// Register a completed block download.
    /// Called by the download engine when a block arrives from a peer or HTTP source.
    ///
    /// Returns the piece index if the piece is now complete and ready for verification.
    pub fn on_block_completed(
        &mut self,
        piece_index: u32,
        block_index: u32,
        peer_id: Option<PeerId>,
    ) -> Option<u32> {
        let key = make_block_key(piece_index, block_index);

        // Remove from in-flight tracking
        self.in_flight.remove(&key);

        // Update piece state
        let piece = &mut self.pieces[piece_index as usize];
        piece.mark_block_complete(block_index);

        // Update stats
        self.stats.total_downloaded += BLOCK_LENGTH;

        // Check if piece is fully downloaded
        if piece.is_complete() {
            self.stats.pieces_completed += 1;
            Some(piece_index)
        } else {
            None
        }
    }

    /// Register a failed block download.
    pub fn on_block_failed(&mut self, piece_index: u32, block_index: u32) {
        let key = make_block_key(piece_index, block_index);
        self.in_flight.remove(&key);

        let piece = &mut self.pieces[piece_index as usize];
        piece.failures += 1;
    }

    /// Assign an in-flight request to a specific peer.
    pub fn assign_request_to_peer(
        &mut self,
        request: BlockRequest,
        peer_id: PeerId,
    ) {
        let key = make_block_key(request.piece_index, request.begin / BLOCK_LENGTH as u32);
        let mut req = request;
        req.assigned_peer = Some(peer_id);
        self.in_flight.insert(key, req);
    }

    /// Check concurrency limits for a specific peer.
    pub fn peer_requests_in_flight(&self, peer_id: &PeerId) -> usize {
        self.in_flight
            .values()
            .filter(|r| r.assigned_peer == Some(*peer_id))
            .count()
    }

    /// Whether a peer can accept another request.
    pub fn can_send_to_peer(&self, peer_id: &PeerId) -> bool {
        self.peer_requests_in_flight(peer_id) < self.config.max_pipeline as usize
    }
}
```

### HTTP Fallback Integration

```rust
impl PieceScheduler {
    /// Determine if HTTP fallback should be triggered for a pending piece.
    ///
    /// For Critical pieces: HTTP is used in parallel with P2P immediately.
    /// For High pieces: HTTP is used if no P2P data arrives within 3 seconds.
    /// For Normal/Low pieces: HTTP is never triggered.
    pub fn needs_http_fallback(&self, piece_index: u32) -> bool {
        let priority = self.priorities[piece_index as usize];
        match priority {
            PiecePriority::Critical => {
                // Critical always uses HTTP in parallel
                true
            }
            PiecePriority::High => {
                if !self.config.http_fallback {
                    return false;
                }
                // High: use HTTP if no progress in 3 seconds
                let state = &self.pieces[piece_index as usize];
                state.failures > 0 || state.downloaded == 0
            }
            _ => false,
        }
    }

    /// Get the current download urgency for display/adaptive buffer logic.
    pub fn urgency_level(&self) -> UrgencyLevel {
        let critical_count = self.priorities.iter().filter(|p| **p == PiecePriority::Critical).count();
        let incomplete_critical = self
            .pieces
            .iter()
            .filter(|p| !p.is_complete() && self.priorities[p.index as usize] == PiecePriority::Critical)
            .count();

        if incomplete_critical > 3 {
            UrgencyLevel::Critical
        } else if incomplete_critical > 0 {
            UrgencyLevel::High
        } else if self.stats.buffered_ms < 5_000 {
            UrgencyLevel::Buffering
        } else {
            UrgencyLevel::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyLevel {
    /// Insufficient data for playback; may need to pause.
    Critical,
    /// Low buffer; increase download priority.
    High,
    /// Buffer filling; normal operation.
    Buffering,
    /// Stable playback.
    Normal,
}
```

### Source Selection

```rust
/// Which source(s) should be used to download a piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStrategy {
    /// Download simultaneously from P2P and HTTP.
    /// First one to complete wins; cancel the other.
    ParallelP2PAndHttp,

    /// Try P2P first. If no response within timeout, also try HTTP.
    P2PWithHttpFallback(Duration),

    /// Only use P2P sources.
    P2POnly,

    /// P2P only, but only when bandwidth is idle.
    P2PIdle,
}

impl PieceScheduler {
    /// Select the best peer from a list of candidates for a given piece.
    /// Prioritizes peers that:
    ///   1. Have the piece (bitfield check)
    ///   2. Are unchoked
    ///   3. Have the lowest latency
    ///   4. Have the highest bandwidth
    ///   5. Have the fewest outstanding requests
    pub fn select_peer_for_piece<'a>(
        &self,
        piece_index: u32,
        peers: &'a [PeerConnection],
    ) -> Option<&'a PeerConnection> {
        let candidates: Vec<&PeerConnection> = peers
            .iter()
            .filter(|p| {
                p.is_connected()
                    && !p.is_choked()
                    && p.bitfield().has(piece_index)
                    && self.can_send_to_peer(&p.peer_id())
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Score each candidate and pick the best
        candidates
            .into_iter()
            .max_by_key(|p| {
                let latency_score = (1000.0 / (p.latency().as_millis().max(1) as f64)) as u32;
                let bandwidth_score = (p.download_speed() / 1024.0) as u32; // KB/s
                let pipeline_score = 10 - self.peer_requests_in_flight(&p.peer_id()) as u32;
                latency_score + bandwidth_score + pipeline_score
            })
    }
}
```

### Peer Connection (simplified interface for scheduler)

```rust
/// Simplified peer connection interface used by the scheduler.
#[derive(Debug, Clone)]
pub struct PeerConnection {
    id: PeerId,
    addr: SocketAddr,
    connected: bool,
    choked: bool,
    bitfield: Bitfield,
    latency: Duration,
    download_speed: f64,
}

impl PeerConnection {
    pub fn peer_id(&self) -> PeerId;
    pub fn addr(&self) -> SocketAddr;
    pub fn is_connected(&self) -> bool;
    pub fn is_choked(&self) -> bool;
    pub fn bitfield(&self) -> &Bitfield;
    pub fn latency(&self) -> Duration;
    pub fn download_speed(&self) -> f64;
}

pub type PeerId = [u8; 20];
```

## Integration with Download Engine

```rust
/// Bridge between the PieceScheduler and the P2spDownloader.
///
/// The download engine calls `scheduler.next_request()` to get the next
/// block to fetch, and `scheduler.on_block_completed()` to signal completion.
pub struct DownloadCoordinator {
    scheduler: Arc<Mutex<PieceScheduler>>,
    downloader: Arc<P2spDownloader>,
    connection_pool: Arc<ConnectionPool>,
}

impl DownloadCoordinator {
    /// Main download loop — runs in a dedicated tokio task.
    pub async fn run(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(50));

        loop {
            interval.tick().await;

            let mut scheduler = self.scheduler.lock().unwrap();
            scheduler.tick();

            // Get all connected peers' bitfields
            let peers = self.connection_pool.active_peers();
            let peer_bitfields: Vec<(PeerId, Bitfield)> = peers
                .iter()
                .map(|p| (p.peer_id(), p.bitfield().clone()))
                .collect();

            // For each peer, find the best request to send
            for (peer_id, bitfield) in &peer_bitfields {
                if !scheduler.can_send_to_peer(peer_id) {
                    continue;
                }

                if let Some(request) = scheduler.next_request(bitfield) {
                    let piece_index = request.piece_index;

                    // Determine source strategy
                    if scheduler.needs_http_fallback(piece_index) {
                        // Dispatch to both P2P and HTTP
                        self.downloader.download_critical(piece_index).await;
                    } else {
                        // Dispatch to specific peer
                        scheduler.assign_request_to_peer(request, *peer_id);
                        self.downloader.request_from_peer(peer_id, piece_index).await;
                    }
                }
            }

            drop(scheduler);
        }
    }
}
```

## Notification/Event Hooks

```rust
/// Events emitted by the scheduler for monitoring and UI display.
#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    /// A block has been completed.
    BlockCompleted {
        piece_index: u32,
        block_index: u32,
        elapsed: Duration,
    },
    /// An entire piece is now complete.
    PieceCompleted {
        piece_index: u32,
        verified: bool,
    },
    /// All pieces are downloaded.
    DownloadComplete,
    /// A seek has been initiated.
    SeekInitiated {
        timestamp_ms: u64,
        target_piece: u32,
    },
    /// Priority levels changed (after recalc).
    PrioritiesChanged {
        critical: u32,
        high: u32,
        normal: u32,
        low: u32,
    },
    /// Block request timed out; will be re-queued.
    RequestTimeout {
        piece_index: u32,
        block_index: u32,
        peer_id: Option<PeerId>,
    },
}

impl PieceScheduler {
    /// Channel for emitting scheduler events to listeners (UI, logging).
    pub fn set_event_sender(&mut self, sender: tokio::sync::mpsc::UnboundedSender<SchedulerEvent>);
}
```

## Priority Calculation Examples

### Normal Playback from Start

```
Playhead: offset 0 (beginning of file)

Piece 0: Critical   (contains first I-frame, playhead is here)  
Piece 1: Critical   (within critical_buffer_ms = 2s ahead)
Piece 2: Critical   (within critical buffer)
Piece 3: High       (within 30s window)
Piece 4: High
...
Piece 20: Normal    (within 120s window)
...
Piece 80: Low       (beyond 120s window)
Piece 81..N: Low    (far ahead)
```

### After Seek to Middle

```
User seeks to timestamp 60s (middle of 2-minute video)
Nearest I-frame: piece 45

Piece 44: Critical   (just before seek target, for decoding context)
Piece 45: Critical   (contains the target I-frame, seek target)
Piece 46: Critical   (within critical buffer after seek point)
Piece 47: High       (within 30s after seek)
...
Piece 60: Normal     (within 120s after seek)
...
Piece 0..43: Low     (already played, available for upload)
Piece 80..N: Low     (far ahead)
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_metadata() -> Arc<FileMeta> {
        // Create a 10MB file with 40 pieces (256KB each)
        // With keyframes every 2 seconds (~50 pieces per keyframe)
        let num_pieces = 40u32;
        let piece_hashes: Vec<PieceHash> = (0..num_pieces)
            .map(|_| PieceHash(sha1::Sha1::from(&[0u8; 256]).digest().bytes()))
            .collect();

        let mut entries = Vec::new();
        // Insert I-frames at piece boundaries every ~2MB (8 pieces)
        for i in 0..5 {
            let piece_idx = i * 8;
            entries.push(KeyFrameEntry {
                timestamp_ms: i as u64 * 2000,
                file_offset: piece_idx as u64 * PIECE_LENGTH,
                frame_size: 48000,
                frame_type: FrameType::I,
            });
        }

        Arc::new(FileMeta {
            info_hash: InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: num_pieces as u64 * PIECE_LENGTH,
            piece_length: PIECE_LENGTH,
            piece_hashes,
            keyframe_index: KeyFrameIndex::new(entries).unwrap(),
            duration_ms: 10000,
            codec: CodecInfo {
                video_codec: "avc1".into(),
                audio_codec: "aac".into(),
                width: 1280,
                height: 720,
                bitrate: 2_000_000,
                ..Default::default()
            },
            from_cache: false,
        })
    }

    #[test]
    fn test_initial_priorities() {
        let meta = create_test_metadata();
        let scheduler = PieceScheduler::new(meta);

        // At start: pieces 0-2 should be Critical (within critical buffer)
        assert_eq!(scheduler.priorities[0], PiecePriority::Critical);
        assert_eq!(scheduler.priorities[1], PiecePriority::Critical);
        assert_eq!(scheduler.priorities[2], PiecePriority::Critical);

        // High: pieces within 30s at 100KB/s ≈ 3MB ≈ 12 pieces
        // Pieces 3-14 should be High
        for i in 3..15 {
            assert_eq!(
                scheduler.priorities[i],
                PiecePriority::High,
                "Piece {} expected High, got {:?}",
                i,
                scheduler.priorities[i]
            );
        }

        // Normal: pieces 15-... within 120s
        // Low: pieces beyond
    }

    #[test]
    fn test_seek_reprioritizes() {
        let meta = create_test_metadata();
        let mut scheduler = PieceScheduler::new(meta);

        // Seek to timestamp 5000ms (piece ~16)
        scheduler.on_seek(5000);

        // Target piece should be Critical
        let target_piece = scheduler.seek_target.unwrap();
        assert_eq!(
            scheduler.priorities[target_piece as usize],
            PiecePriority::Critical
        );
    }

    #[test]
    fn test_block_request_creation() {
        let req = BlockRequest::new(0, 0, BLOCK_LENGTH as u32, PiecePriority::Critical);
        assert_eq!(req.piece_index, 0);
        assert_eq!(req.begin, 0);
        assert_eq!(req.length, BLOCK_LENGTH as u32);
        assert_eq!(req.priority, PiecePriority::Critical);
        assert!(!req.is_timed_out(Duration::from_secs(1)));
    }

    #[test]
    fn test_piece_state() {
        let mut state = PieceState {
            index: 0,
            priority: PiecePriority::Critical,
            downloaded: 0,
            block_bitmask: 0,
            total_blocks: 16,
            failures: 0,
            verified: false,
            written_to_buffer: false,
        };

        assert!(!state.is_complete());
        assert_eq!(state.completion(), 0.0);

        for i in 0..16 {
            state.mark_block_complete(i);
        }

        assert!(state.is_complete());
        assert!((state.completion() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_make_block_key() {
        let key = make_block_key(5, 3);
        assert_eq!(key >> 32, 5);
        assert_eq!(key as u32, 3);
    }

    #[test]
    fn test_source_strategies() {
        assert_eq!(
            PiecePriority::Critical.source_strategy(),
            SourceStrategy::ParallelP2PAndHttp
        );
        assert_eq!(
            PiecePriority::Normal.source_strategy(),
            SourceStrategy::P2POnly
        );
    }

    #[test]
    fn test_priority_ordering() {
        assert!(PiecePriority::Critical < PiecePriority::High);
        assert!(PiecePriority::High < PiecePriority::Normal);
        assert!(PiecePriority::Normal < PiecePriority::Low);
    }

    #[test]
    fn test_keyframe_pieces_get_high_priority() {
        let meta = create_test_metadata();
        let scheduler = PieceScheduler::new(meta);

        // Keyframes at piece 0, 8, 16, 24, 32
        // Piece 16 is beyond the critical buffer but should be High (keyframe)
        assert_eq!(scheduler.priorities[16], PiecePriority::High);
        assert_eq!(scheduler.priorities[24], PiecePriority::High);
        assert_eq!(scheduler.priorities[32], PiecePriority::High);
    }
}
```

## Performance Considerations

### Block Request Lifecycle

```
Created → Added to pending_queue
    ↓
Assigned to peer → Added to in_flight map
    ↓
Peer responds → on_block_completed() → Check piece completeness
    ↓                              ↓
(Timeout) → on_block_failed() → Re-queued
    ↓
Piece complete → SHA-1 verification → Written to buffer → Ready for playback
```

### Concurrency Limits

| Limit | Value | Rationale |
|-------|-------|-----------|
| Max blocks per peer (pipeline) | 5 | Prevents overwhelming a single peer while maintaining throughput |
| Max global pending | 100 | Limits memory usage for pending requests |
| Max Critical concurrent | 8 | Critical pieces need maximum urgency |
| Max High concurrent | 5 | Balance between speed and fairness |
| Max Normal concurrent | 3 | Background download |
| Max Low concurrent | 1 | Idle filling |
| Block timeout | 15s | Long enough for slow peers, short enough to not stall |
| Recalculation interval | 500ms | Responsive to seek without being CPU-intensive |

# QVOD Piece Scheduling Algorithm Reference

## 1. Overview

QVOD's scheduler is the core intelligence that enables instant playback, smooth streaming, and efficient P2SP resource utilization. Unlike BitTorrent's strict rarest-first ordering, QVOD uses **priority-driven scheduling** where playback urgency and frame-type determine order, with rarest-first as a tiebreaker within priority levels.

---

## 2. Piece and Block Definitions

### 2.1 Unit Sizes

```rust
pub const PIECE_LENGTH: u64  = 262_144;    // 256 KB
pub const BLOCK_LENGTH: u64  = 16_384;     // 16 KB
pub const BLOCKS_PER_PIECE: u32 = 16;      // 256 KB / 16 KB
pub const MAX_BLOCK_SIZE: u32 = 16_384;     // protocol max
```

A file of size `S` is divided into `N = ceil(S / PIECE_LENGTH)` pieces.  
The last piece may be smaller than `PIECE_LENGTH`.  
Each piece is divided into `B = ceil(piece_size / BLOCK_LENGTH)` blocks.

### 2.2 Data Structures

```rust
pub struct PieceInfo {
    pub index: u32,
    pub length: u64,              // actual byte length (may be smaller for last piece)
    pub hash: [u8; 20],           // SHA-1 hash of piece data
    pub blocks: Vec<BlockInfo>,
}

pub struct BlockInfo {
    pub piece_index: u32,
    pub offset: u32,              // byte offset within piece
    pub length: u32,              // BLOCK_LENGTH (may be smaller for last block)
    pub downloaded: bool,
}

pub struct BlockRequest {
    pub piece_index: u32,
    pub begin: u32,               // byte offset within piece
    pub length: u32,
}
```

---

## 3. Priority System

### 3.1 Priority Levels

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PiecePriority {
    Critical = 0,    // Immediate playback need — download NOW
    High     = 1,    // Will play within ~30 seconds
    Normal   = 2,    // Future data, ~30-120 seconds ahead
    Low      = 3,    // Already played — for upload contribution only
}
```

Lower discriminant = higher priority (Critical = highest).

### 3.2 Priority Calculation Formula

For a piece at byte offset `P_off` in a file with metadata `M`:

```
def calculate_priority(piece_index, playhead_offset, metadata):
    piece_start = piece_index * PIECE_LENGTH
    piece_end   = min(piece_start + PIECE_LENGTH, metadata.file_size)

    # 1. Distance from playhead (bytes)
    distance = piece_start - playhead_offset   # may be negative (behind)

    # 2. Convert distance to playback time (seconds)
    avg_bitrate = metadata.bitrate  # bits/second
    distance_sec = (distance * 8) / avg_bitrate   # bytes → bits → seconds

    # 3. Check if piece contains a keyframe
    has_keyframe = any(
        entry.frame_type == I_FRAME
        and entry.file_offset >= piece_start
        and entry.file_offset < piece_end
        for entry in metadata.keyframe_index.entries
    )

    # 4. Calculate priority
    if distance < 0 and has_keyframe:
        # Keyframe behind playhead → needed if seeking backward or just started
        return CRITICAL
    elif distance_sec <= 0:
        # Current playback position piece
        return CRITICAL
    elif distance_sec <= 30:
        # Within next 30 seconds
        return HIGH
    elif distance_sec <= 120:
        # Within next 2 minutes
        return NORMAL
    else:
        # Beyond 2 minutes
        if has_uploaders_who_need(piece_index):
            return LOW
        else:
            return NORMAL  # progressive download
```

### 3.3 Keyframe Priority Boost

Keyframe-containing pieces receive a one-level priority boost if they are not already `Critical`:

```
def boosted_priority(piece_index, base_priority, metadata):
    if base_priority == CRITICAL:
        return CRITICAL  # already top

    if piece_contains_keyframe(piece_index, metadata):
        # Boost: LOW → NORMAL, NORMAL → HIGH, HIGH → CRITICAL
        return base_priority - 1  # ordinal shift

    return base_priority
```

---

## 4. Keyframe Index Lookup Algorithm

```rust
pub struct KeyFrameIndex {
    pub entries: Vec<KeyFrameEntry>,
}

pub struct KeyFrameEntry {
    pub timestamp_ms: u64,
    pub file_offset: u64,
    pub frame_size: u32,
    pub frame_type: FrameType,
}

pub enum FrameType {
    I = 0,  // Intra-frame (keyframe) — independently decodable
    P = 1,  // Predicted frame — depends on previous I/P
    B = 2,  // Bidirectional frame — depends on surrounding I/P
}
```

### 4.1 Find Nearest Keyframe

Binary search over the sorted keyframe index by timestamp:

```
def find_nearest_keyframe(timestamp_ms, keyframe_index):
    """
    Returns the keyframe entry closest to the given timestamp.
    If timestamp falls within a GOP, returns the preceding I-frame.
    """
    if keyframe_index.entries.is_empty():
        return None

    # Binary search for first entry with timestamp >= target
    lo, hi = 0, len(keyframe_index.entries)
    while lo < hi:
        mid = (lo + hi) / 2
        if keyframe_index.entries[mid].timestamp_ms < timestamp_ms:
            lo = mid + 1
        else:
            hi = mid

    # lo is the first entry >= timestamp, or len(entries) if past end
    if lo == 0:
        return keyframe_index.entries[0]
    if lo == len(keyframe_index.entries):
        return keyframe_index.entries[-1]

    # Return the closest I-frame (entries are sorted by timestamp)
    # If the found entry is an I-frame, use it; otherwise scan backward
    for i in (lo, lo - 1):
        if keyframe_index.entries[i].frame_type == I_FRAME:
            return keyframe_index.entries[i]

    # Fallback: scan backward for nearest I-frame
    for i in range(lo, -1, -1):
        if keyframe_index.entries[i].frame_type == I_FRAME:
            return keyframe_index.entries[i]

    return keyframe_index.entries[0]
```

### 4.2 Get All I-Frames

```
def get_all_iframes(keyframe_index):
    return [e for e in keyframe_index.entries if e.frame_type == I_FRAME]
```

### 4.3 Piece-to-Timestamp Mapping

```
def piece_to_timestamp(piece_index, metadata):
    """Returns the playback timestamp (ms) of a piece."""
    piece_start = piece_index * PIECE_LENGTH
    # Find the I-frame nearest to this offset
    closest = min(
        metadata.keyframe_index.entries,
        key=lambda e: abs(e.file_offset - piece_start)
    )
    # Interpolate based on bitrate
    if piece_start >= closest.file_offset:
        extra_bytes = piece_start - closest.file_offset
        extra_ms = (extra_bytes * 8 * 1000) / metadata.bitrate
        return closest.timestamp_ms + extra_ms
    else:
        missing_bytes = closest.file_offset - piece_start
        missing_ms = (missing_bytes * 8 * 1000) / metadata.bitrate
        return closest.timestamp_ms - missing_ms
```

---

## 5. Seek Re-Prioritization Algorithm

When the user seeks to a new position:

```rust
pub struct SeekEvent {
    pub target_timestamp_ms: u64,
    pub target_file_offset: u64,
}
```

### 5.1 Re-Prioritization on Seek

```
def on_seek(seek_event, scheduler):
    # 1. Find nearest I-frame
    keyframe = find_nearest_keyframe(
        seek_event.target_timestamp_ms,
        scheduler.metadata.keyframe_index
    )
    if keyframe is None:
        return  # no keyframe data, can't seek

    # 2. Determine target piece
    target_piece = keyframe.file_offset / PIECE_LENGTH

    # 3. Reset all piece priorities
    for each piece in scheduler.pieces:
        piece.priority = LOW  # reset to baseline

    # 4. Set target piece zone to CRITICAL
    target_piece_index = int(target_piece)
    set_piece_priority(target_piece_index, CRITICAL)

    # 5. Next ~30 seconds of pieces → HIGH
    seek_duration_sec = 30
    seek_ahead_bytes = (seek_duration_sec * scheduler.metadata.bitrate) / 8
    seek_end_offset = seek_event.target_file_offset + seek_ahead_bytes
    seek_end_piece = int(min(
        seek_end_offset / PIECE_LENGTH,
        scheduler.metadata.piece_count - 1
    ))
    for i in range(target_piece_index + 1, seek_end_piece + 1):
        set_piece_priority(i, HIGH)

    # 6. All I-frame pieces before target → CRITICAL too
    # (may be needed for backward decoding or if we overshot)
    for entry in scheduler.metadata.keyframe_index.entries:
        if entry.file_offset < keyframe.file_offset:
            p = int(entry.file_offset / PIECE_LENGTH)
            if get_piece_priority(p) > CRITICAL:
                set_piece_priority(p, HIGH)  # at least HIGH

    # 7. Reset buffer cursor
    scheduler.playhead_offset = keyframe.file_offset
    scheduler.seek_in_progress = true
```

### 5.2 Priority Distribution After Seek

```
After seek to 2:30:

        ┌────────────────────────────────────────────┐
        │  Timeline                                  │
        │  ──┬──┬──┬──┬──┬──┬──┬──┬──┬──┬──┬──┬───  │
        │   0  1  2  3  4  5  6  7  8  9  10 11      │
        │       ↑ seek (2:30)                        │
        │       │                                     │
        │  P0: LOW  (already played)                 │
        │  P1: LOW                                   │
        │  P2: CRITICAL (target I-frame)             │
        │  P3: HIGH  (within 30s)                    │
        │  P4: HIGH                                  │
        │  P5: HIGH                                  │
        │  P6: NORMAL (30s-120s)                     │
        │  P7: NORMAL                                │
        │  P8: NORMAL                                │
        │  P9: NORMAL                                │
        │  P10: LOW (beyond 2min)                    │
        │  P11: LOW                                  │
        └────────────────────────────────────────────┘
```

---

## 6. Deadline Calculation

Each piece has a deadline — the time by which it must be available for uninterrupted playback.

```
def deadline(piece_index, playhead_offset, metadata):
    """Returns the absolute deadline Instant for a piece."""
    piece_start = piece_index * PIECE_LENGTH
    distance = piece_start - playhead_offset

    if distance <= 0:
        # Already needed for playback
        return NOW

    bitrate = metadata.bitrate  # bits/sec
    duration_sec = (distance * 8) / bitrate
    # Apply 50% safety buffer
    safe_duration_sec = duration_sec * 0.5
    return NOW + Duration::from_secs_f64(safe_duration_sec)
```

### 6.1 Urgency Metric

```
def urgency(piece_index, scheduler):
    """Higher urgency = needs download sooner."""
    deadline = deadline(piece_index, scheduler.playhead_offset, scheduler.metadata)
    time_until_deadline = deadline - NOW

    if time_until_deadline <= 0:
        return INFINITY  # must download NOW

    return 1.0 / time_until_deadline.as_secs_f64()
```

---

## 7. Rarest-First Within Priority

For pieces at the same priority level, use rarest-first to improve swarm health.

### 7.1 Piece Rarity

```
def piece_rarity(piece_index, peer_bitfields):
    """Returns how many peers have this piece (0 = nobody has it)."""
    count = 0
    for bitfield in peer_bitfields:
        if bitfield.has(piece_index):
            count += 1
    return count
```

### 7.2 Combined Sort Key

```
def sort_key(piece_index, scheduler):
    priority = get_piece_priority(piece_index)
    rarity = piece_rarity(piece_index, scheduler.peer_bitfields)

    # Primary sort: priority
    # Secondary sort: rarity (ascending = rarest first)
    # Tertiary sort: distance from playhead (ascending)
    distance = abs(piece_index * PIECE_LENGTH - scheduler.playhead_offset)

    return (priority, rarity, distance)
```

### 7.3 Next Request Selection

```
def next_request(scheduler):
    """Returns the next (piece_index, block_offset) to request."""
    # Get all incomplete pieces, sorted by (priority, rarity, distance)
    candidates = []
    for piece in scheduler.incomplete_pieces():
        score = sort_key(piece.index, scheduler)
        candidates.append((score, piece))

    candidates.sort(key=lambda x: x[0])

    for _, piece in candidates:
        # Get the first missing block from this piece
        block = scheduler.first_missing_block(piece.index)
        if block is not None:
            return BlockRequest(
                piece_index=piece.index,
                begin=block.offset,
                length=BLOCK_LENGTH
            )

    return None  # nothing to request
```

---

## 8. Peer Selection for Piece Download

When multiple peers have the requested piece, select the best peer:

```
def select_peer_for_piece(piece_index, interested_peers):
    """
    Select the optimal peer to request a piece from.
    Higher score = better choice.
    """
    best_peer = None
    best_score = -INFINITY

    for peer in interested_peers:
        if not peer.bitfield.has(piece_index):
            continue
        if peer.choked:
            continue

        score = 0.0
        score += peer.stats.speed_down / 1024.0      # download speed bonus
        score -= peer.pending_requests.len() * 0.1    # request queue penalty
        score += 0.5 if peer.stats.rtt < 100ms else 0  # low latency bonus
        score -= peer.stats.loss_rate * 5.0           # loss penalty

        if score > best_score:
            best_score = score
            best_peer = peer

    return best_peer
```

---

## 9. Adaptive Pipelining

### 9.1 Request Pipeline Depth

The scheduler maintains a request pipeline — the number of outstanding (not-yet-received) block requests per peer.

```
def calculate_pipeline_depth(peer, priority):
    base = 0
    if priority == CRITICAL:
        base = 10   # aggressive pipelining for critical pieces
    elif priority == HIGH:
        base = 5
    elif priority == NORMAL:
        base = 3
    else:
        base = 1     # LOW priority: at most 1 pending request

    # Adjust for network conditions
    if peer.stats.loss_rate > 0.1:
        base = min(base, 3)   # reduce pipeline on lossy connections
    if peer.stats.rtt > Duration::from_millis(300):
        base = max(base, 8)   # longer pipelines for high-latency links

    return base
```

### 9.2 Endgame Mode

When fewer than 20 blocks remain for the entire file, enter endgame mode:

```
def endgame_request(piece_index, scheduler):
    """
    In endgame mode, request the same remaining block from MULTIPLE peers.
    Cancel duplicates upon first receipt.
    """
    remaining = scheduler.remaining_blocks()

    if remaining <= 20:
        # Request each remaining block from 3 different peers
        for block in scheduler.missing_blocks():
            peers_with_piece = [
                p for p in scheduler.interested_peers()
                if p.bitfield.has(block.piece_index)
            ]
            for peer in peers_with_piece[:3]:
                scheduler.send_request(peer, block)

        # Cancel duplicates as pieces arrive
        scheduler.on_piece_received = lambda block: cancel_duplicate_requests(block)
```

---

## 10. Main Scheduling Loop (Pseudo-Code)

```
thread SchedulingLoop {
    loop {
        // 1. Update scheduler state
        playhead = buffer.get_playhead_offset()
        scheduler.set_playhead(playhead)

        // 2. If seek pending, re-prioritize
        if scheduler.seek_in_progress:
            on_seek(scheduler.seek_event, scheduler)
            scheduler.seek_in_progress = false

        // 3. Recalculate priorities for all pieces
        for each piece in incomplete_pieces:
            base = calculate_priority(piece.index, playhead, metadata)
            boosted = boosted_priority(piece.index, base, metadata)
            set_piece_priority(piece.index, boosted)

        // 4. For each connected peer, send next request
        for each peer in connected_peers:
            if peer.is_choked or not peer.is_interested:
                continue

            pipeline = calculate_pipeline_depth(peer, get_highest_pending_priority())
            available_slots = pipeline - peer.pending_requests.len()

            for _ in range(available_slots):
                request = select_request_for_peer(peer)
                if request is None:
                    break

                send_request(peer, request)
                peer.pending_requests.push(request)

        // 5. Trigger HTTP fallback for CRITICAL pieces not serviced by P2P
        for each piece with priority CRITICAL:
            if piece.timed_out():
                http_downloader.enqueue(piece)

        // 6. Check for endgame mode
        if remaining_blocks() <= 20:
            enter_endgame_mode()

        // 7. Clean up timed-out requests
        for each peer:
            for each request in peer.pending_requests:
                if request.age > TIMEOUT:
                    peer.pending_requests.remove(request)
                    if request.retries < MAX_RETRIES:
                        re-enqueue request with retry+1
                    else:
                        mark peer as slow

        // 8. Sleep briefly to avoid busy-loop
        sleep(50ms)
    }
}
```

---

## 11. Constants and Configuration

```rust
pub struct SchedulerConfig {
    // Piece sizes
    pub piece_length: u64,              // 256 KB
    pub block_length: u64,              // 16 KB

    // Priority time windows
    pub critical_window_sec: f64,       // 0 (current position)
    pub high_window_sec: f64,           // 30 seconds
    pub normal_window_sec: f64,         // 120 seconds

    // Endgame
    pub endgame_threshold: u32,         // 20 blocks remaining

    // Pipelining
    pub critical_pipeline: u32,         // 10
    pub high_pipeline: u32,             // 5
    pub normal_pipeline: u32,           // 3
    pub low_pipeline: u32,              // 1

    // Timeouts
    pub request_timeout: Duration,      // 30 seconds
    pub peer_dead_timeout: Duration,    // 60 seconds (too many timeouts)
    pub max_retries_per_request: u32,   // 3

    // HTTP fallback
    pub critical_http_timeout: Duration, // 3 seconds (wait for P2P before HTTP)
    pub high_http_timeout: Duration,     // 10 seconds

    // Scoring
    pub speed_window: Duration,         // 10 seconds (for rate calculation)
    pub rtt_ema_alpha: f64,             // 0.2 (for smoothed RTT)

    // Endgame
    pub endgame_redundancy: u32,        // 3 (request each block from 3 peers)
}
```

---

## 12. Priority State Machine Per Piece

```
                    ┌─────────────┐
                    │  UNDEFINED  │
                    └──────┬──────┘
                           │
              calculated on scheduler init
                           │
                           ▼
                    ┌─────────────┐
                    │   NORMAL    │◄────────────────────┐
                    └──────┬──────┘                     │
                           │                            │
              ┌────────────┼────────────┐               │
              │            │            │               │
              ▼            ▼            ▼               │
        ┌─────────┐ ┌──────────┐ ┌──────────┐          │
        │ HIGH    │ │ CRITICAL │ │  LOW     │          │
        │ (boost) │ │ (needed  │ │ (played) │──────────┘
        └─────────┘ │  now)    │ └──────────┘  re-check
                    └──────────┘               on playhead
                         │                     advance
                    piece downloaded
                         │
                         ▼
                    ┌─────────────┐
                    │  COMPLETED  │
                    └─────────────┘
```

---

## 13. Rust Implementation Outline

```rust
pub struct PieceScheduler {
    pub metadata: Arc<FileMeta>,
    pub playhead_offset: u64,
    pub pieces: Vec<PieceState>,
    pub peer_bitfields: Arc<Mutex<HashMap<NodeId, Bitfield>>>,
    pub pending_requests: VecMap<BlockRequest>,
    pub seek_in_progress: bool,
    pub seek_event: Option<SeekEvent>,
    pub config: SchedulerConfig,
}

pub struct PieceState {
    pub index: u32,
    pub priority: PiecePriority,
    pub downloaded: bool,
    pub blocks: Vec<BlockState>,
}

pub struct BlockState {
    pub offset: u32,
    pub length: u32,
    pub downloaded: bool,
    pub requested: bool,
    pub requested_at: Option<Instant>,
    pub requested_from: Option<NodeId>,
    pub retries: u32,
}

impl PieceScheduler {
    pub fn new(metadata: Arc<FileMeta>, config: SchedulerConfig) -> Self;

    pub fn set_playhead(&mut self, offset: u64);

    pub fn set_seek_target(&mut self, piece_index: u32);

    pub fn next_request(&mut self) -> Option<BlockRequest>;

    pub fn on_block_received(&mut self, piece_index: u32, block_offset: u32);

    pub fn on_block_timeout(&mut self, request: &BlockRequest);

    pub fn on_have(&mut self, peer_id: &NodeId, piece_index: u32);

    pub fn on_bitfield(&mut self, peer_id: &NodeId, bitfield: Bitfield);

    pub fn on_completed(&mut self) -> bool;

    pub fn remaining_count(&self) -> u32;

    pub fn completion(&self) -> f64;
}
```

# QVOD Streaming Engine Reference

## 1. Overview

The streaming engine (Layer 4, `qvs-stream`) is the core coordinator that ties together P2SP transport, buffering, scheduling, and pseudo-HLS adaptation. It orchestrates the lifecycle of a playback session from URI to pixels.

---

## 2. Ring Buffer Design

### 2.1 Architecture

The ring buffer is a fixed-capacity circular memory region that holds recently downloaded media data. The play cursor reads from the "past" edge; write cursor adds data at the "future" edge.

```
                    write_cursor (advances as data arrives)
                         │
                         ▼
    ┌────┬────┬────┬────┬────┬────┬────┬────┬────┬────┐
    │ P2 │ P3 │ P4 │ P5 │ P6 │ P7 │    │    │    │ P1 │
    └────┴────┴────┴────┴────┴────┴────┴────┴────┴────┘
     ▲                                       ▲
     │                                       │
     play_cursor                         last written
     (read position)                     (newest data)
```

### 2.2 Data Structure

```rust
pub struct RingBuffer {
    // Fixed allocation
    data: Vec<u8>,
    capacity: usize,

    // Cursors
    play_cursor: AtomicUsize,     // playback read position
    write_cursor: AtomicUsize,    // next write position

    // Filled regions tracking (for sparse availability checks)
    filled_ranges: Vec<Range<usize>>,

    // Watermarks
    watermark_low: usize,         // bytes — trigger more buffering
    watermark_high: usize,        // bytes — pause buffering

    // State
    eof: AtomicBool,

    // Metrics
    bytes_written: AtomicU64,
    bytes_read: AtomicU64,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: vec![0u8; capacity],
            capacity,
            play_cursor: AtomicUsize::new(0),
            write_cursor: AtomicUsize::new(0),
            filled_ranges: Vec::new(),
            watermark_low: 5 * 1024 * 1024,      // 5 MB
            watermark_high: 30 * 1024 * 1024,     // 30 MB
            eof: AtomicBool::new(false),
            bytes_written: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
        }
    }
}
```

### 2.3 Reading and Writing

```rust
impl RingBuffer {
    /// Write data at a specific file offset.
    /// File offset → buffer position:
    ///   buf_pos = file_offset % capacity
    pub fn write(&mut self, file_offset: u64, data: &[u8]) -> Result<()> {
        let len = data.len();
        let start = (file_offset as usize) % self.capacity;

        // Handle wraparound
        if start + len > self.capacity {
            let first_part = self.capacity - start;
            self.data[start..].copy_from_slice(&data[..first_part]);
            self.data[..(len - first_part)].copy_from_slice(&data[first_part..]);
        } else {
            self.data[start..start + len].copy_from_slice(data);
        }

        // Update write cursor
        self.write_cursor.store(
            (file_offset as usize + len) % self.capacity,
            Ordering::Release
        );

        // Track filled region (in file offset space, not buffer space)
        self.merge_filled_range(file_offset as usize, file_offset as usize + len);

        self.bytes_written.fetch_add(len as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Read data from a specific file offset.
    /// Returns data from buffer if available within capacity window.
    pub fn read(&mut self, file_offset: u64, length: usize) -> Result<Vec<u8>> {
        // Check if requested range is within what we have buffered
        let end = file_offset as usize + length;
        let buf_low = self.play_cursor.load(Ordering::Acquire);
        let buf_high = buf_low + self.capacity;

        if (file_offset as usize) < buf_low || end > buf_high {
            return Err(BufferError::RangeNotAvailable {
                offset: file_offset,
                length,
                buffer_start: buf_low as u64,
                buffer_end: buf_high as u64,
            });
        }

        let start = (file_offset as usize) % self.capacity;
        let mut result = vec![0u8; length];

        // Handle wraparound
        if start + length > self.capacity {
            let first_part = self.capacity - start;
            result[..first_part].copy_from_slice(&self.data[start..]);
            result[first_part..].copy_from_slice(&self.data[..(length - first_part)]);
        } else {
            result.copy_from_slice(&self.data[start..start + length]);
        }

        self.bytes_read.fetch_add(length as u64, Ordering::Relaxed);

        Ok(result)
    }

    /// Merge a newly filled byte range [start, end) into the range tracker.
    fn merge_filled_range(&mut self, start: usize, end: usize) {
        let new_range = start..end;
        let mut merged = Vec::new();
        let mut inserted = false;

        for r in self.filled_ranges.drain(..) {
            if r.end < new_range.start {
                merged.push(r);
            } else if r.start > new_range.end {
                if !inserted {
                    merged.push(new_range.clone());
                    inserted = true;
                }
                merged.push(r);
            } else {
                // Overlapping: merge
                let merged_start = r.start.min(new_range.start);
                let merged_end = r.end.max(new_range.end);
                let merged_range = merged_start..merged_end;
                if !inserted {
                    merged.push(merged_range);
                    inserted = true;
                } else {
                    // Merge with last inserted
                    let last = merged.last_mut().unwrap();
                    last.start = last.start.min(merged_range.start);
                    last.end = last.end.max(merged_range.end);
                }
            }
        }

        if !inserted {
            merged.push(new_range);
        }

        self.filled_ranges = merged;
    }
}
```

### 2.4 Cursors Explained

| Cursor | Semantic | Advances when | Controlled by |
|--------|----------|---------------|---------------|
| `play_cursor` | File offset of audio/video decode head | Playback consumes data | Playback thread |
| `write_cursor` | File offset of latest written byte | New data arrives | Download threads |

**Key invariant**: `write_cursor - play_cursor ≤ capacity`  
(assuming no wraparound; in circular arithmetic, the unwrapped difference must be maintained)

**Play cursor advance logic**:
```
every 10ms:
    bytes_to_advance = (bitrate / 8) * 0.01   # 10ms of audio at stream bitrate
    play_cursor += bytes_to_advance
    play_cursor = play_cursor.clamp(0, file_size)
```

---

## 3. Watermarks and Adaptation

### 3.1 Watermark Levels

```
Buffer Full (100%)
    │
    │  ┌───── watermark_high (pause buffering above this)
    │  │
    │  │    ┌─ watermark_low (resume buffering below this)
    │  │    │
    ▼  ▼    ▼
0%  ●──────●──────────────► time
    Playback can start
    (is_playable)
```

### 3.2 Dynamic Watermark Adaptation

Watermarks are adjusted based on measured network speed to balance latency vs. stall probability.

```rust
impl RingBuffer {
    pub fn adapt_watermarks(&mut self, speed_bps: f64) {
        // Target: buffer enough data for T seconds at current speed
        // Fast network: buffer less (lower latency)
        // Slow network: buffer more (fewer stalls)
        let target_seconds = if speed_bps > 10_000_000.0 {
            // 10 Mbps+: buffer 5 seconds
            5.0
        } else if speed_bps > 2_000_000.0 {
            // 2-10 Mbps: buffer 15 seconds
            15.0
        } else if speed_bps > 500_000.0 {
            // 500 Kbps - 2 Mbps: buffer 30 seconds
            30.0
        } else if speed_bps > 200_000.0 {
            // 200-500 Kbps: buffer 60 seconds
            60.0
        } else {
            // Below 200 Kbps: buffer 120 seconds
            120.0
        };

        let target_bytes = (target_seconds * speed_bps / 8.0) as usize;

        self.watermark_low = target_bytes / 3;          // resume at 1/3 target
        self.watermark_high = target_bytes;              // stop at target

        // Clamp to ring buffer capacity
        self.watermark_high = self.watermark_high.min(self.capacity);
        self.watermark_low = self.watermark_low.min(self.watermark_high / 2);

        tracing::debug!(
            "Adapted watermarks: low={}, high={} (speed={} bps)",
            self.watermark_low, self.watermark_high, speed_bps
        );
    }
}
```

### 3.3 AdaptiveBuffer State Machine

```rust
pub enum BufferCommand {
    PauseAndBuffer,        // Not enough data; pause playback
    ThrottleUpload,        // Buffer full; reduce upload bandwidth
    Normal,                // Normal operation
    IncreaseHttpRatio,     // Network poor; fall back to HTTP more
    Aggressive,            // Critical; download from all sources
}

pub struct AdaptiveBuffer {
    stats: DownloadStats,
    state: AdaptiveState,
    next_check: Instant,
}

enum AdaptiveState {
    Buffering,      // Initial fill
    Steady,         // Normal playback
    Starving,       // Buffer underrun
    Full,           // Buffer saturated
}

impl AdaptiveBuffer {
    pub fn new() -> Self;

    /// Called every ~100ms by the engine
    pub fn tick(&mut self, buffer: &RingBuffer, speed_bps: f64, rtt: Duration) -> BufferCommand {
        let buffered_bytes = self.buffered_bytes(buffer);
        let playable_duration = self.estimate_playable_duration(buffered_bytes, speed_bps);
        let downloaded_percent = buffer.completion();

        match self.state {
            AdaptiveState::Buffering => {
                // Initial startup: wait until playable
                if playable_duration > Duration::from_secs(5) {
                    self.state = AdaptiveState::Steady;
                    BufferCommand::Normal
                } else if speed_bps < 50_000.0 {
                    // Very slow: increase HTTP ratio
                    BufferCommand::IncreaseHttpRatio
                } else {
                    BufferCommand::PauseAndBuffer
                }
            }

            AdaptiveState::Steady => {
                if buffered_bytes > buffer.watermark_high {
                    self.state = AdaptiveState::Full;
                    BufferCommand::ThrottleUpload
                } else if playable_duration < Duration::from_secs(2) {
                    self.state = AdaptiveState::Starving;
                    BufferCommand::PauseAndBuffer
                } else if playable_duration < Duration::from_secs(5) {
                    // Running low
                    BufferCommand::IncreaseHttpRatio
                } else if speed_bps > 500_000.0 && rtt < Duration::from_millis(100) {
                    BufferCommand::Normal
                } else {
                    BufferCommand::IncreaseHttpRatio
                }
            }

            AdaptiveState::Starving => {
                // After underrun: aggressively buffer
                if playable_duration > Duration::from_secs(10) {
                    self.state = AdaptiveState::Steady;
                    BufferCommand::Normal
                } else {
                    BufferCommand::Aggressive
                }
            }

            AdaptiveState::Full => {
                if buffered_bytes < buffer.watermark_low {
                    self.state = AdaptiveState::Steady;
                    BufferCommand::Normal
                } else {
                    BufferCommand::ThrottleUpload
                }
            }
        }
    }

    fn buffered_bytes(&self, buffer: &RingBuffer) -> usize {
        let play = buffer.play_cursor.load(Ordering::Acquire);
        let write = buffer.write_cursor.load(Ordering::Acquire);

        if write >= play {
            write - play
        } else {
            // Wraparound case
            (buffer.capacity - play) + write
        }
    }

    fn estimate_playable_duration(&self, buffered_bytes: usize, speed_bps: f64) -> Duration {
        // Conservative: assume 1 Mbps average bitrate for video
        const ESTIMATED_BITRATE: f64 = 1_000_000.0;  // 1 Mbps
        let seconds = (buffered_bytes as f64 * 8.0) / ESTIMATED_BITRATE;
        Duration::from_secs_f64(seconds)
    }
}
```

---

## 4. Playback Readiness Detection

### 4.1 is_playable Check

A stream is playable when:
1. The first keyframe (I-frame) is fully buffered
2. At least ~1 second of audio data is available

```rust
impl RingBuffer {
    pub fn is_playable(&self, metadata: &FileMeta) -> bool {
        // Find the first I-frame
        let first_iframe = metadata.keyframe_index.entries.iter()
            .find(|e| e.frame_type == FrameType::I);

        let Some(iframe) = first_iframe else {
            // No keyframe info: playable if we have any data
            return self.bytes_written.load(Ordering::Relaxed) > 0;
        };

        // Check if the I-frame's piece is fully in buffer
        let iframe_end = iframe.file_offset + iframe.frame_size as u64;
        for range in &self.filled_ranges {
            if range.start as u64 <= iframe.file_offset && range.end as u64 >= iframe_end {
                // I-frame is available
                // Additionally check ~1 second of subsequent data
                let one_sec_bytes = (metadata.bitrate / 8) as u64;
                let required_end = iframe_end + one_sec_bytes;
                for range2 in &self.filled_ranges {
                    if range2.start as u64 <= iframe_end && range2.end as u64 >= required_end {
                        return true;
                    }
                }
                return false;
            }
        }

        false
    }

    /// How far ahead from playhead we have buffered (in bytes)
    pub fn buffered_ahead(&self) -> usize {
        let play = self.play_cursor.load(Ordering::Acquire);
        let write = self.write_cursor.load(Ordering::Acquire);

        if write >= play {
            write - play
        } else {
            (self.capacity - play) + write
        }
    }

    /// Convert bytes to approximate duration at known bitrate
    pub fn buffered_duration(&self, bitrate: u32) -> Duration {
        let bytes = self.buffered_ahead();
        let secs = (bytes as f64 * 8.0) / bitrate as f64;
        Duration::from_secs_f64(secs)
    }
}
```

### 4.2 Playback Start Flow

```rust
impl QvodEngine {
    pub async fn play(&mut self, uri: &QvodUri) -> Result<MediaStream> {
        // 1. Resolve info_hash from URI
        let info_hash = uri.info_hash;

        // 2. Check cache
        if let Some(entry) = self.cache.find(&info_hash)? {
            if entry.completion() >= 1.0 {
                // Fully cached: serve directly
                let stream = MediaStream::from_cache(entry, uri, self.cache.clone());
                return Ok(stream);
            }
        }

        // 3. Discover peers (parallel Tracker + DHT)
        let (tracker_result, dht_receiver) = tokio::join!(
            self.discover_from_tracker(&info_hash),
            self.discover_from_dht(&info_hash),
        );

        let peers = self.merge_peer_lists(tracker_result, dht_receiver);

        // 4. Connect to top-scoring peers
        let connections = self.connect_to_peers(&peers, MAX_INITIAL_CONNECTIONS).await;

        // 5. Get metadata from connected peers
        let metadata = self.resolve_metadata(&info_hash, &connections).await?;

        // 6. Initialize scheduler and buffer
        let scheduler = PieceScheduler::new(metadata.clone(), self.config.scheduler.clone());
        let buffer = RingBuffer::new(self.config.buffer_capacity_mb * 1024 * 1024);
        buffer.adapt_watermarks(500_000.0); // initial estimate: 500 Kbps

        // 7. Start download engine
        let download_handle = self.start_download_loop(
            info_hash,
            metadata.clone(),
            scheduler,
            buffer.clone(),
            connections,
        );

        // 8. Wait until playable (with timeout)
        let start = Instant::now();
        while !buffer.is_playable(&metadata) {
            if start.elapsed() > Duration::from_secs(30) {
                return Err(StreamingError::PlaybackTimeout);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // 9. Return playable stream
        Ok(MediaStream {
            info_hash,
            metadata,
            buffer,
            download_handle: Some(download_handle),
            state: StreamState::Playing,
        })
    }
}
```

---

## 5. Pseudo-HLS M3U8 Generation

### 5.1 Why Pseudo-HLS

QVOD generates a virtual HLS playlist from the keyframe index, allowing mobile browsers and non-QVOD players to consume the stream through a standard HLS interface. Segments are generated on-the-fly from the buffer (or cache).

### 5.2 M3U8 Generation Algorithm

```rust
pub struct HlsAdapter {
    metadata: Arc<FileMeta>,
    segment_duration: Duration,  // default 10 seconds
}

impl HlsAdapter {
    /// Generate M3U8 playlist from keyframe index
    pub fn generate_m3u8(&self) -> String {
        let mut m3u8 = String::new();

        // Header
        m3u8.push_str("#EXTM3U\n");
        m3u8.push_str("#EXT-X-VERSION:3\n");
        m3u8.push_str(&format!(
            "#EXT-X-TARGETDURATION:{}\n",
            self.segment_duration.as_secs()
        ));
        m3u8.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
        m3u8.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");

        // Collect I-frames as segment boundaries
        let iframes: Vec<&KeyFrameEntry> = self.metadata.keyframe_index.entries.iter()
            .filter(|e| e.frame_type == FrameType::I)
            .collect();

        for (i, window) in iframes.windows(2).enumerate() {
            let current = window[0];
            let next = window[1];
            let segment_duration_sec = (next.timestamp_ms - current.timestamp_ms) as f64 / 1000.0;

            // Segment URL: points to local server
            m3u8.push_str(&format!(
                "#EXTINF:{:.6},\n",
                segment_duration_sec
            ));
            m3u8.push_str(&format!(
                "/segment?hash={}&offset={}&length={}\n",
                hex::encode(self.metadata.info_hash),
                current.file_offset,
                next.file_offset - current.file_offset,
            ));
        }

        // Last segment (if no next I-frame)
        if let Some(last) = iframes.last() {
            if last.file_offset + last.frame_size as u64 < self.metadata.file_size {
                let remaining = self.metadata.file_size - last.file_offset;
                let time_up_to_duration = (remaining as f64 * 8.0) / self.metadata.bitrate as f64;
                m3u8.push_str(&format!(
                    "#EXTINF:{:.6},\n",
                    time_up_to_duration
                ));
                m3u8.push_str(&format!(
                    "/segment?hash={}&offset={}&length={}\n",
                    hex::encode(self.metadata.info_hash),
                    last.file_offset,
                    remaining,
                ));
            }
        }

        // For live-like dynamic playlists (not VOD):
        // m3u8.push_str("#EXT-X-ENDLIST\n");
        // For VOD:
        m3u8.push_str("#EXT-X-ENDLIST\n");

        m3u8
    }
}
```

### 5.3 Example M3U8 Output

```
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:VOD
#EXTINF:10.000000,
/segment?hash=a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9&offset=0&length=262144
#EXTINF:8.500000,
/segment?hash=a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9&offset=262144&length=262144
#EXTINF:9.200000,
/segment?hash=a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9&offset=524288&length=262144
#EXTINF:10.000000,
/segment?hash=a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9&offset=786432&length=262144
#EXT-X-ENDLIST
```

### 5.4 TS Segment Wrapping

Each segment is wrapped in a minimal MPEG-TS container:

```rust
impl HlsAdapter {
    /// Wraps raw data into a pseudo-MPEG-TS segment for HLS playback.
    /// In QVOD's design, this is a pass-through or minimal container
    /// since the raw stream is typically already in a playable format
    /// (H.264/AAC in FLV or MP4 container).
    pub fn wrap_as_ts(&self, data: &[u8], offset: u64) -> Vec<u8> {
        // QVOD's pseudo-HLS can simply serve the raw data frame
        // because the underlying format (RMVB/AVI/MP4) may already contain
        // full frames. For strict HLS compliance, you would re-mux to TS.
        //
        // Strategy: for known H.264+AAC content, construct minimal
        // MPEG-TS packets with PAT/PMT + PES packets.
        //
        // For simplicity and low latency, return raw data with correct
        // Content-Type header. Many modern HLS players handle this.
        data.to_vec()
    }
}
```

---

## 6. Keyframe Index Construction

### 6.1 From Metadata Exchange

The keyframe index is obtained via the `ut_metadata` extension protocol or the `qvod_keyframe` extension:

```rust
pub struct KeyFrameIndexBuilder;

impl KeyFrameIndexBuilder {
    /// Build index from raw metadata Bencode (received via ut_metadata).
    pub fn from_bencode(dict: &BTreeMap<String, BencodeValue>) -> Result<KeyFrameIndex> {
        let entries_dict = dict.get("keyframe index")
            .and_then(|v| v.as_dict())
            .ok_or(StreamingError::NoKeyframeIndex)?;

        let entries_list = entries_dict.get("entries")
            .and_then(|v| v.as_list())
            .ok_or(StreamingError::NoKeyframeIndex)?;

        let entries: Result<Vec<KeyFrameEntry>> = entries_list.iter().map(|v| {
            let edict = v.as_dict().ok_or(StreamingError::InvalidKeyframeEntry)?;
            Ok(KeyFrameEntry {
                timestamp_ms: edict.get("ts").and_then(|v| v.as_int()).ok_or(StreamingError::InvalidKeyframeEntry)? as u64,
                file_offset: edict.get("off").and_then(|v| v.as_int()).ok_or(StreamingError::InvalidKeyframeEntry)? as u64,
                frame_size: edict.get("siz").and_then(|v| v.as_int()).ok_or(StreamingError::InvalidKeyframeEntry)? as u32,
                frame_type: match edict.get("type").and_then(|v| v.as_int()).ok_or(StreamingError::InvalidKeyframeEntry)? {
                    0 => FrameType::I,
                    1 => FrameType::P,
                    2 => FrameType::B,
                    _ => return Err(StreamingError::InvalidKeyframeEntry),
                },
            })
        }).collect();

        Ok(KeyFrameIndex { entries: entries? })
    }

    /// Build index by demuxing the first few pieces of the video.
    /// Fallback when metadata exchange is not available.
    pub fn from_demuxer<P: AsRef<Path>>(path: P) -> Result<KeyFrameIndex> {
        // Requires ffmpeg-next
        // Open file, scan for keyframes, build index
        // This is a fallback; the preferred source is metadata exchange
        unimplemented!("demuxer-based keyframe indexing")
    }
}
```

### 6.2 Index Validation

```rust
impl KeyFrameIndex {
    /// Validate that the index is well-formed
    pub fn validate(&self, file_size: u64) -> Result<()> {
        if self.entries.is_empty() {
            return Err(StreamingError::EmptyKeyframeIndex);
        }

        // Must be sorted by timestamp
        for window in self.entries.windows(2) {
            if window[0].timestamp_ms > window[1].timestamp_ms {
                return Err(StreamingError::UnsortedKeyframeIndex);
            }
        }

        // Entries must be within file bounds
        for entry in &self.entries {
            if entry.file_offset + entry.frame_size as u64 > file_size {
                return Err(StreamingError::KeyframeOutOfBounds);
            }
        }

        // First entry should be an I-frame
        if self.entries[0].frame_type != FrameType::I {
            // This is a warning, not a hard error — but problematic
            tracing::warn!("First keyframe entry is not an I-frame");
        }

        Ok(())
    }

    /// Build a segment table: (offset, length, duration_ms) for each segment
    pub fn to_segments(&self) -> Vec<(u64, u64, u64)> {
        let iframes: Vec<&KeyFrameEntry> = self.entries.iter()
            .filter(|e| e.frame_type == FrameType::I)
            .collect();

        iframes.windows(2).map(|w| {
            let dur = w[1].timestamp_ms - w[0].timestamp_ms;
            let len = w[1].file_offset - w[0].file_offset;
            (w[0].file_offset, len, dur)
        }).collect()
    }
}
```

---

## 7. Buffer Underrun Recovery

### 7.1 Underrun Detection

```rust
impl RingBuffer {
    /// Returns true if the playback head has caught up to or passed
    /// the write cursor (no more data to consume).
    pub fn is_underrun(&self) -> bool {
        let play = self.play_cursor.load(Ordering::Acquire);
        let write = self.write_cursor.load(Ordering::Acquire);

        if write >= play {
            // Normal: write ahead of play
            (write - play) < MIN_PLAYABLE_BYTES
        } else {
            // Wraparound: check if the gap between play→end + start→write is small
            let ahead = (self.capacity - play) + write;
            ahead < MIN_PLAYABLE_BYTES
        }
    }
}

pub const MIN_PLAYABLE_BYTES: usize = 256 * 1024;  // 256 KB minimum
```

### 7.2 Recovery Strategy

```rust
impl QvodEngine {
    pub fn on_underrun(&mut self) {
        tracing::warn!("Buffer underrun — entering recovery");

        // 1. Pause playback
        self.stream_state = StreamState::Buffering;

        // 2. Increase buffer target
        self.buffer.watermark_high = self.buffer.watermark_high.saturating_mul(2)
            .min(self.buffer.capacity);

        // 3. Prioritize playhead piece more aggressively
        let playhead = self.buffer.play_cursor.load(Ordering::Acquire);
        self.scheduler.set_seek_target(
            (playhead as u64 / PIECE_LENGTH) as u32
        );

        // 4. Increase HTTP fallback ratio for critical pieces
        self.p2sp.set_http_fallback_ratio(1.0); // 100% HTTP for critical

        // 5. Drop slowest peers
        self.connection_pool.drop_slowest(3);

        // 6. Wait for buffer to recover
        let recovery_target = self.buffer.watermark_low;
        loop {
            if self.buffer.buffered_ahead() >= recovery_target {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // 7. Resume playback
        self.stream_state = StreamState::Playing;
        tracing::info!("Buffer recovered, resuming playback");
    }
}
```

### 7.3 Underrun Prevention

```rust
impl AdaptiveBuffer {
    /// Called preemptively to prevent underrun before it happens
    pub fn preemptive_check(&self, buffer: &RingBuffer, speed_bps: f64) -> Option<BufferCommand> {
        let buffered = buffer.buffered_ahead();
        let bitrate = 1_000_000;  // 1 Mbps conservative estimate
        let buffered_seconds = (buffered as f64 * 8.0) / bitrate as f64;

        // Estimate time to consume buffer vs. time to fetch more
        let fetch_seconds = if speed_bps > 0.0 {
            (self.piece_size as f64 * 8.0) / speed_bps
        } else {
            f64::MAX
        };

        if buffered_seconds < fetch_seconds * 2.0 {
            // We'll run out before we can fetch the next piece
            Some(BufferCommand::Aggressive)
        } else {
            None
        }
    }
}
```

---

## 8. Streaming Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum StreamingError {
    #[error("range not available: offset={offset}, length={length}, buffer=[{buffer_start}, {buffer_end})")]
    RangeNotAvailable {
        offset: u64,
        length: usize,
        buffer_start: u64,
        buffer_end: u64,
    },

    #[error("playback timeout: stream not playable within timeout")]
    PlaybackTimeout,

    #[error("no keyframe index available")]
    NoKeyframeIndex,

    #[error("invalid keyframe entry")]
    InvalidKeyframeEntry,

    #[error("empty keyframe index")]
    EmptyKeyframeIndex,

    #[error("unsorted keyframe index")]
    UnsortedKeyframeIndex,

    #[error("keyframe out of file bounds")]
    KeyframeOutOfBounds,

    #[error("buffer capacity exceeded")]
    BufferCapacityExceeded,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("scheduler error: {0}")]
    Scheduler(String),
}
```

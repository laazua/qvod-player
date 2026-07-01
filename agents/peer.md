# Peer Connection Management Specification

## Overview

Peer connection management is the core of QVOD's P2SP transport layer. It handles the full lifecycle of connections between peers, from discovery and handshaking through data exchange to teardown. Unlike BitTorrent's tit-for-tat choking algorithm optimized for bulk file distribution, QVOD uses a **streaming-optimized** choking/unchoking strategy that prioritizes playback continuity over upload fairness.

---

## 1. Connection Lifecycle

### State Machine

```
                    ┌──────────────┐
                    │ DISCONNECTED │
                    └──────┬───────┘
                           │ connect()
                           ▼
                    ┌──────────────┐
              ┌─────┤  CONNECTING  │
              │     └──────┬───────┘
              │            │ TCP handshake complete
              │            ▼
              │     ┌──────────────┐
              │     │ HANDSHAKING  │
              │     └──────┬───────┘
              │            │ 68-byte handshake exchanged + verified
              │            ▼
              │     ┌──────────────┐
              │     │ ESTABLISHED  │
              │     └──────┬───────┘
              │            │ disconnect() / error / timeout
              │            ▼
              │     ┌──────────────┐
              └─────┤DISCONNECTING │
                    └──────┬───────┘
                           │ clean resources
                           ▼
                    ┌──────────────┐
                    │ DISCONNECTED │
                    └──────────────┘
```

### State Definitions

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// No connection exists
    Disconnected,
    /// TCP connection being established (SYN_SENT)
    Connecting,
    /// 68-byte handshake being exchanged
    Handshaking,
    /// Handshake complete; ready for data exchange
    Established,
    /// Graceful teardown in progress
    Disconnecting,
}

impl ConnectionState {
    pub fn is_active(&self) -> bool {
        matches!(self, ConnectionState::Established)
    }

    pub fn is_alive(&self) -> bool {
        !matches!(self, ConnectionState::Disconnected)
    }

    pub fn can_exchange_data(&self) -> bool {
        matches!(self, ConnectionState::Established)
    }
}
```

### Connection Lifecycle Implementation

```rust
pub struct PeerConnection {
    /// Unique peer identifier (20 bytes)
    pub peer_id: [u8; 20],
    /// Network address
    pub addr: SocketAddr,
    /// TCP stream for reliable data (key frames, control messages)
    pub tcp_stream: Option<TcpStream>,
    /// UDP socket for non-critical data (P/B frames)
    pub udp_socket: Option<UdpSocket>,
    /// Current connection state
    pub state: ConnectionState,
    /// Peer's piece availability bitfield
    pub bitfield: Bitfield,
    /// Whether we are interested in this peer's data
    pub am_interested: bool,
    /// Whether this peer is interested in our data
    pub peer_interested: bool,
    /// Whether this peer has choked us
    pub am_choked: bool,
    /// Whether we have choked this peer
    pub peer_choked: bool,
    /// Pending outgoing requests
    pub pending_requests: Vec<BlockRequest>,
    /// Pending incoming piece data
    pub pending_pieces: Vec<Piece>,
    /// Connection statistics
    pub stats: ConnectionStats,
    /// Last activity timestamp (for keep-alive)
    pub last_active: Instant,
    /// When connection was established
    pub connected_since: Option<Instant>,
}

impl PeerConnection {
    /// Create a new peer connection placeholder
    pub fn new(peer_id: [u8; 20], addr: SocketAddr) -> Self;

    /// Initiate outbound connection
    pub async fn connect(&mut self, info_hash: &[u8; 20], local_peer_id: &[u8; 20]) -> Result<()> {
        self.state = ConnectionState::Connecting;
        self.last_active = Instant::now();

        // 1. Establish TCP connection
        let stream = tokio::time::timeout(
            Duration::from_secs(10),
            TcpStream::connect(self.addr),
        )
        .await
        .map_err(|_| Error::Timeout("TCP connect".into()))??;

        self.state = ConnectionState::Handshaking;

        // 2. Perform handshake
        self.handshake(stream, info_hash, local_peer_id).await?;

        self.state = ConnectionState::Established;
        self.connected_since = Some(Instant::now());
        Ok(())
    }

    /// Disconnect gracefully
    pub async fn disconnect(&mut self) -> Result<()> {
        self.state = ConnectionState::Disconnecting;

        // Send "stopped" event logic if needed
        if let Some(stream) = &mut self.tcp_stream {
            // Send keep-alive (0-length message) as last goodbye
            let _ = stream.writable().await;
            // Graceful shutdown
            stream.shutdown().ok();
        }
        self.tcp_stream = None;
        self.udp_socket = None;
        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    /// Track activity
    pub fn mark_active(&mut self) {
        self.last_active = Instant::now();
    }

    /// Check if connection is idle beyond threshold
    pub fn is_idle(&self, timeout: Duration) -> bool {
        self.last_active.elapsed() > timeout
    }
}
```

---

## 2. Handshake Protocol

### Wire Format (68 bytes total)

```
Offset  Size  Field         Description
──────  ────  ─────────     ──────────────────────────────────────────
0       1     pstrlen       String length = 19 (0x13)
1       19    pstr          Protocol string = "Qvod P2SP Protocol"
20      8     reserved      Reserved bytes (all zeros)
                            Bit 5 of byte 5: supports ut_metadata extension
                            Bit 4 of byte 5: supports UDP data channel
28      20    info_hash     SHA-1 hash identifying the resource
48      20    peer_id       20-byte peer identifier
```

### Reserved Bytes Interpretation

```
Byte 0-4: All zeros (reserved for future protocol extensions)
Byte 5:
  Bit 0: supports DHT protocol
  Bit 1: supports peer exchange (PEX)
  Bit 2: supports FAST extension
  Bit 3: supports NAT traversal
  Bit 4: supports UDP data channel
  Bit 5: supports ut_metadata extension
  Bit 6-7: reserved
Byte 6-7: All zeros
```

### Handshake Implementation

```rust
#[derive(Debug, Clone)]
pub struct Handshake {
    pub pstrlen: u8,
    pub pstr: [u8; 19],
    pub reserved: [u8; 8],
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

impl Handshake {
    const PROTOCOL_STRING: &[u8; 19] = b"Qvod P2SP Protocol";

    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        let mut reserved = [0u8; 8];
        // Set ut_metadata support (byte 5, bit 5)
        reserved[5] |= 0x10;
        // Set UDP data channel support (byte 5, bit 4)
        reserved[5] |= 0x08;

        Self {
            pstrlen: 19,
            pstr: *Self::PROTOCOL_STRING,
            reserved,
            info_hash,
            peer_id,
        }
    }

    /// Encode handshake into 68-byte buffer
    pub fn encode(&self) -> [u8; 68] {
        let mut buf = [0u8; 68];
        buf[0] = self.pstrlen;
        buf[1..20].copy_from_slice(&self.pstr);
        buf[20..28].copy_from_slice(&self.reserved);
        buf[28..48].copy_from_slice(&self.info_hash);
        buf[48..68].copy_from_slice(&self.peer_id);
        buf
    }

    /// Decode handshake from 68-byte buffer
    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < 68 {
            return Err(Error::Protocol("Handshake too short".into()));
        }

        let pstrlen = buf[0];
        if pstrlen != 19 {
            return Err(Error::Protocol(format!(
                "Invalid pstrlen: {}", pstrlen
            )));
        }

        let mut pstr = [0u8; 19];
        pstr.copy_from_slice(&buf[1..20]);

        if &pstr != Self::PROTOCOL_STRING {
            return Err(Error::Protocol("Protocol mismatch".into()));
        }

        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&buf[20..28]);

        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&buf[28..48]);

        let mut peer_id = [0u8; 20];
        peer_id.copy_from_slice(&buf[48..68]);

        Ok(Self { pstrlen, pstr, reserved, info_hash, peer_id })
    }

    /// Check if peer supports ut_metadata extension
    pub fn supports_metadata(&self) -> bool {
        self.reserved[5] & 0x10 != 0
    }

    /// Check if peer supports UDP data channel
    pub fn supports_udp(&self) -> bool {
        self.reserved[5] & 0x08 != 0
    }

    /// Check if peer supports DHT
    pub fn supports_dht(&self) -> bool {
        self.reserved[5] & 0x01 != 0
    }

    /// Check if peer supports NAT traversal
    pub fn supports_nat_traversal(&self) -> bool {
        self.reserved[5] & 0x04 != 0
    }
}

/// Perform the full handshake exchange over a TCP stream.
pub async fn perform_handshake(
    stream: &mut TcpStream,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
) -> Result<(Handshake, bool)> {
    let is_initiator: bool;

    // Send our handshake
    let hs = Handshake::new(*info_hash, *peer_id);
    let encoded = hs.encode();
    stream.write_all(&encoded).await?;

    // Read peer's handshake
    let mut buf = [0u8; 68];
    stream.read_exact(&mut buf).await?;
    let peer_hs = Handshake::decode(&buf)?;

    // Verify info_hash matches
    if peer_hs.info_hash != *info_hash {
        return Err(Error::Protocol("InfoHash mismatch during handshake".into()));
    }

    // If our peer_id sorts lower, we are the "initiator" in dual scenarios
    is_initiator = *peer_id < peer_hs.peer_id;

    Ok((peer_hs, is_initiator))
}
```

---

## 3. Bitfield Management

### Bitfield Structure

```rust
#[derive(Debug, Clone)]
pub struct Bitfield {
    /// Raw bitfield bytes
    bytes: Vec<u8>,
    /// Total number of pieces
    num_pieces: u32,
}

impl Bitfield {
    /// Create a new bitfield for given number of pieces
    pub fn new(num_pieces: u32) -> Self {
        let byte_len = (num_pieces as usize + 7) / 8;
        Self {
            bytes: vec![0u8; byte_len],
            num_pieces,
        }
    }

    pub fn from_bytes(bytes: Vec<u8>, num_pieces: u32) -> Self {
        Self { bytes, num_pieces }
    }

    /// Check if piece at index is available
    pub fn has(&self, index: u32) -> bool {
        if index >= self.num_pieces {
            return false;
        }
        let byte_idx = (index / 8) as usize;
        let bit_idx = (index % 8) as u8;
        if byte_idx >= self.bytes.len() {
            return false;
        }
        self.bytes[byte_idx] & (1 << (7 - bit_idx)) != 0
    }

    /// Set piece availability
    pub fn set(&mut self, index: u32, value: bool) {
        if index >= self.num_pieces {
            return;
        }
        let byte_idx = (index / 8) as usize;
        let bit_idx = (index % 8) as u8;
        if value {
            self.bytes[byte_idx] |= 1 << (7 - bit_idx);
        } else {
            self.bytes[byte_idx] &= !(1 << (7 - bit_idx));
        }
    }

    /// Set all bits
    pub fn set_all(&mut self, value: bool) {
        if value {
            self.bytes.fill(0xFF);
            // Clear trailing bits beyond num_pieces
            let last_byte_bits = self.num_pieces % 8;
            if last_byte_bits != 0 {
                let last = self.bytes.last_mut().unwrap();
                *last &= !(0xFF << (8 - last_byte_bits));
            }
        } else {
            self.bytes.fill(0x00);
        }
    }

    /// Count available pieces
    pub fn count(&self) -> u32 {
        self.bytes.iter().map(|&b| b.count_ones()).sum()
    }

    /// Completion ratio (0.0 to 1.0)
    pub fn completion(&self) -> f64 {
        if self.num_pieces == 0 {
            return 1.0;
        }
        self.count() as f64 / self.num_pieces as f64
    }

    /// Check if all pieces are available
    pub fn is_complete(&self) -> bool {
        self.count() == self.num_pieces
    }

    /// Check if no pieces are available
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Raw bytes
    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// XOR with another bitfield (returns new bitfield of common pieces)
    pub fn intersection(&self, other: &Bitfield) -> Bitfield {
        let min_len = self.bytes.len().min(other.bytes.len());
        let mut result = Bitfield::new(self.num_pieces.min(other.num_pieces));
        for i in 0..min_len {
            result.bytes[i] = self.bytes[i] & other.bytes[i];
        }
        result
    }

    /// Get indices of all available pieces
    pub fn available_pieces(&self) -> Vec<u32> {
        let mut pieces = Vec::with_capacity(self.count() as usize);
        for i in 0..self.num_pieces {
            if self.has(i) {
                pieces.push(i);
            }
        }
        pieces
    }

    /// Iterator over piece availability
    pub fn iter(&self) -> BitfieldIter {
        BitfieldIter {
            bitfield: self,
            current: 0,
        }
    }
}

pub struct BitfieldIter<'a> {
    bitfield: &'a Bitfield,
    current: u32,
}

impl<'a> Iterator for BitfieldIter<'a> {
    type Item = bool;

    fn next(&mut self) -> Option<bool> {
        if self.current >= self.bitfield.num_pieces {
            return None;
        }
        let val = self.bitfield.has(self.current);
        self.current += 1;
        Some(val)
    }
}
```

### Bitfield Exchange on Connection

Upon handshake completion, the established peer immediately sends a `bitfield` message if it has any pieces, or `have_none` if it has none, or `have_all` if complete:

```rust
impl PeerConnection {
    /// Send bitfield to peer after handshake
    pub async fn send_bitfield(&mut self) -> Result<()> {
        let msg = if self.bitfield.is_complete() {
            PeerMessage::new(MsgId::HaveAll, vec![])
        } else if self.bitfield.is_empty() {
            PeerMessage::new(MsgId::HaveNone, vec![])
        } else {
            PeerMessage::new(MsgId::Bitfield, self.bitfield.to_bytes().to_vec())
        };
        self.send_message(msg).await?;
        Ok(())
    }

    /// Update local bitfield when a piece is received
    pub fn mark_piece_complete(&mut self, piece_index: u32) {
        self.bitfield.set(piece_index, true);
        self.stats.local_pieces_completed += 1;
    }

    /// Send a 'have' message to all connected peers
    pub async fn announce_have(&mut self, piece_index: u32) -> Result<()> {
        let payload = piece_index.to_be_bytes().to_vec();
        let msg = PeerMessage::new(MsgId::Have, payload);
        self.send_message(msg).await?;
        Ok(())
    }
}
```

---

## 4. Choking/Unchoking Algorithm

### Streaming-Optimized Strategy

QVOD uses a fundamentally different choking strategy from BitTorrent. BitTorrent's tit-for-tat rewards peers who upload to you — this optimizes for swarm health in bulk file distribution. QVOD optimizes for **streaming continuity**: the priority is getting the next piece needed for playback, not maintaining upload/download ratio fairness.

```rust
pub struct ChokingManager {
    /// Local peer's download state
    local_state: DownloadState,
    /// Map of peer_id → peer connection
    connections: HashMap<[u8; 20], PeerConnection>,
    /// Configuration
    config: ChokingConfig,
    /// Last choke calculation time
    last_calc: Instant,
}

#[derive(Clone)]
pub struct ChokingConfig {
    /// Maximum number of unchoked peers (default: 4 upload + 10 download)
    pub max_unchoked_upload: u32,
    pub max_unchoked_download: u32,
    /// How often to recalculate choking (default: 10 seconds)
    pub recalc_interval: Duration,
    /// Optimistic unchoke slot count (default: 1)
    pub optimistic_slots: u32,
    /// Whether to use tit-for-tat for upload slots (default: false)
    /// QVOD defaults to streaming mode, not tit-for-tat
    pub use_tit_for_tat: bool,
    /// Number of pieces ahead of playhead to consider "urgent" (default: 20)
    pub urgent_piece_window: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChokeReason {
    NotInterested,
    SlowUploader,
    HighLatency,
    Firewalled,
    OptimisticSlot,
    Default,
}
```

### Choking Algorithm (Streaming Mode)

```rust
impl ChokingManager {
    pub fn new(config: ChokingConfig) -> Self;

    /// Recalculate choking decisions.
    /// Called every `recalc_interval` seconds and on significant events.
    pub fn recalculate(&mut self) {
        let now = Instant::now();

        // ===== DOWNLOAD CHOKING =====
        // Who to unchoke for downloading (who we request FROM):
        // 1. Peers who have the piece we need right now
        // 2. Peers with highest download speed
        // 3. Peers with lowest latency

        let mut download_candidates: Vec<PeerId> = self.connections
            .iter()
            .filter(|(_, conn)| conn.state == ConnectionState::Established)
            .filter(|(_, conn)| conn.am_choked) // currently choked peers
            .map(|(id, conn)| (*id, self.score_download_peer(conn)))
            .collect();

        // Sort by download score descending
        download_candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Unchoke top N
        for (peer_id, _) in download_candidates.iter().take(self.config.max_unchoked_download as usize) {
            if let Some(conn) = self.connections.get_mut(peer_id) {
                if conn.am_choked {
                    conn.am_choked = false;
                    conn.send_message(PeerMessage::new(MsgId::Unchoke, vec![])).ok();
                }
            }
        }

        // ===== UPLOAD CHOKING =====
        // Who to unchoke for uploading (who we send TO):
        // In streaming mode, we prioritize peers who:
        // 1. Are interested in our data
        // 2. Have pieces we need (reciprocity)
        // 3. Have highest upload speed
        // 4. Have lowest RTT

        let mut upload_candidates: Vec<PeerId> = self.connections
            .iter()
            .filter(|(_, conn)| conn.state == ConnectionState::Established)
            .filter(|(_, conn)| conn.peer_interested) // they want our data
            .filter(|(_, conn)| conn.peer_choked) // we currently choke them
            .map(|(id, conn)| (*id, self.score_upload_peer(conn)))
            .collect();

        upload_candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Unchoke top N
        for (peer_id, _) in upload_candidates.iter().take(self.config.max_unchoked_upload as usize) {
            if let Some(conn) = self.connections.get_mut(peer_id) {
                if conn.peer_choked {
                    conn.peer_choked = false;
                    conn.send_message(PeerMessage::new(MsgId::Unchoke, vec![])).ok();
                }
            }
        }

        // ===== OPTIMISTIC UNCHOKE =====
        // Periodically unchoke a random peer to discover better connections
        self.optimistic_unchoke();

        // ===== CHOKE SLOW PEERS =====
        // Choke any unchoked peer that falls below threshold
        self.choke_slow_peers();
    }

    /// Score a peer for download (higher = more likely to be unchoked for downloading)
    pub fn score_download_peer(&self, conn: &PeerConnection) -> f64 {
        // Base score
        let mut score = 0.0;

        // Bonus: peer has pieces we urgently need
        let urgent_pieces = self.count_urgent_pieces_available(conn);
        score += urgent_pieces as f64 * 100.0;

        // Bonus: download speed (normalized to MB/s)
        let speed_mbps = conn.stats.download_speed as f64 / 1_048_576.0;
        score += speed_mbps * 50.0;

        // Penalty: high latency
        let latency_ms = conn.stats.rtt.as_millis() as f64;
        if latency_ms > 500.0 {
            score -= 100.0;
        } else if latency_ms > 200.0 {
            score -= 30.0;
        }

        // Bonus: low loss rate
        if conn.stats.loss_rate < 0.01 {
            score += 20.0;
        } else if conn.stats.loss_rate > 0.1 {
            score -= 50.0;
        }

        // Penalty: firewalled peer can only receive, not share
        if conn.stats.is_firewalled {
            score -= 50.0;
        }

        // Geo bonus: same region = lower latency
        if let Some(loc) = &conn.stats.location {
            if loc == "same_region" {
                score += 15.0;
            }
        }

        score.max(0.0)
    }

    /// Score a peer for upload (higher = more likely to be unchoked for uploading)
    pub fn score_upload_peer(&self, conn: &PeerConnection) -> f64 {
        let mut score = 20.0; // baseline

        // Reciprocity bonus: they have pieces we need
        let needed = self.count_needed_pieces(conn);
        score += needed as f64 * 50.0;

        // Speed bonus
        let up_speed = conn.stats.upload_speed as f64 / 1_048_576.0;
        score += up_speed * 30.0;

        // Interest bonus: they are interested
        if conn.peer_interested {
            score += 40.0;
        }

        score
    }

    /// Count how many pieces this peer has that we need urgently
    fn count_urgent_pieces_available(&self, conn: &PeerConnection) -> u32 {
        // Urgent = pieces within the next `urgent_piece_window` pieces from playhead
        // that we don't have yet but this peer does
        let local_bitfield = &self.local_state.bitfield;
        let peer_bitfield = &conn.bitfield;

        let start = self.local_state.playhead_piece;
        let end = start + self.config.urgent_piece_window;
        let mut count = 0;
        for i in start..end.min(peer_bitfield.num_pieces) {
            if !local_bitfield.has(i) && peer_bitfield.has(i) {
                count += 1;
            }
        }
        count
    }

    fn count_needed_pieces(&self, conn: &PeerConnection) -> u32 {
        let local = &self.local_state.bitfield;
        let peer = &conn.bitfield;
        (0..local.num_pieces.min(peer.num_pieces))
            .filter(|&i| !local.has(i) && peer.has(i))
            .count() as u32
    }

    /// Optimistic unchoke: randomly unchoke a peer to discover better connections
    fn optimistic_unchoke(&mut self) {
        let choked_peers: Vec<PeerId> = self.connections
            .iter()
            .filter(|(_, conn)| conn.state == ConnectionState::Established)
            .filter(|(_, conn)| conn.peer_choked && conn.peer_interested)
            .map(|(id, _)| *id)
            .collect();

        if choked_peers.is_empty() {
            return;
        }

        // Pick random peer for optimistic unchoke
        let mut rng = thread_rng();
        for _ in 0..self.config.optimistic_slots {
            if let Some(&peer_id) = choked_peers.choose(&mut rng) {
                if let Some(conn) = self.connections.get_mut(&peer_id) {
                    if conn.peer_choked {
                        conn.peer_choked = false;
                        conn.send_message(PeerMessage::new(MsgId::Unchoke, vec![])).ok();
                    }
                }
            }
        }
    }

    /// Choke peers that are performing poorly
    fn choke_slow_peers(&mut self) {
        let to_choke: Vec<PeerId> = self.connections
            .iter()
            .filter(|(_, conn)| conn.state == ConnectionState::Established)
            .filter(|(_, conn)| !conn.peer_choked) // currently unchoked
            .filter(|(_, conn)| {
                // Choke if:
                // - They have no pieces we need
                // - They are not interested in our data
                // - Their speed is below threshold for > 30 seconds
                let needed = self.count_needed_pieces(conn);
                let connected_secs = conn.connected_since
                    .map(|t| t.elapsed().as_secs())
                    .unwrap_or(0);

                (needed == 0 && connected_secs > 30) ||
                (!conn.peer_interested && connected_secs > 60) ||
                (conn.stats.download_speed < 1024 && connected_secs > 120)
            })
            .map(|(id, _)| *id)
            .collect();

        for peer_id in to_choke {
            if let Some(conn) = self.connections.get_mut(&peer_id) {
                conn.peer_choked = true;
                conn.send_message(PeerMessage::new(MsgId::Choke, vec![])).ok();
            }
        }
    }
}
```

### Comparison: QVOD Streaming vs BitTorrent Tit-for-Tat

| Aspect | BitTorrent | QVOD Streaming |
|--------|-----------|----------------|
| Primary goal | Upload fairness | Playback continuity |
| Unchoke priority | Best uploaders | Peers with needed pieces |
| Optimistic unchoke | 1 slot, 30s cycle | 1 slot, 10s cycle |
| Download choke | Not applicable (interested = unchoked) | Based on urgency + speed |
| Rarest first priority | Yes | No (playhead proximity first) |
| Snubbing detection | 60s no data | 15s no data (streaming is time-sensitive) |
| Free-riding tolerance | Very low | Moderate (as long as they have pieces we need) |

---

## 5. Interested State Management

```rust
impl PeerConnection {
    /// Check if we should be interested in this peer.
    /// We are interested if the peer has at least one piece we don't have.
    pub fn evaluate_interested(&mut self, local_bitfield: &Bitfield) {
        let should_be = (0..self.bitfield.num_pieces.min(local_bitfield.num_pieces))
            .any(|i| self.bitfield.has(i) && !local_bitfield.has(i));

        if should_be && !self.am_interested {
            self.am_interested = true;
            // Send interested message
            if let Some(stream) = &mut self.tcp_stream {
                let msg = PeerMessage::new(MsgId::Interested, vec![]);
                send_message(stream, &msg).ok();
            }
        } else if !should_be && self.am_interested {
            self.am_interested = false;
            // Send not_interested
            if let Some(stream) = &mut self.tcp_stream {
                let msg = PeerMessage::new(MsgId::NotInterested, vec![]);
                send_message(stream, &msg).ok();
            }
        }
    }

    /// Handle incoming interested/not_interested from peer
    pub fn handle_interested(&mut self, interested: bool) {
        self.peer_interested = interested;
        // If peer is interested and we have spare upload slots,
        // choking manager will decide to unchoke
    }
}
```

### Interested State Machine

```
Peer has no pieces we need
  ──────────────────────────► not_interested

Peer has ≥ 1 piece we need
  ──────────────────────────► interested

We receive a 'have' message for a needed piece
  ──────────────────────────► interested (if not already)

We download the last piece the peer had that we needed
  ──────────────────────────► not_interested

Peer unchokes us (we were interested, now can request)
  ──────────────────────────► start requesting pieces
```

The `interested` state is re-evaluated whenever:
- A `have` message is received
- A `bitfield` message is received  
- We complete a piece (our local bitfield changes)
- The choking manager recalculates

---

## 6. Connection Pool Design

```rust
pub struct ConnectionPool {
    /// Active connections keyed by peer_id
    connections: HashMap<[u8; 20], PeerConnection>,
    /// Pending outbound connection attempts
    pending_outbound: HashMap<[u8; 20], JoinHandle<Result<()>>>,
    /// Maximum number of concurrent connections
    max_connections: u32,
    /// Local peer info
    local_peer_id: [u8; 20],
    /// Configuration
    config: PoolConfig,
    /// Choking manager
    choking: ChokingManager,
}

#[derive(Clone)]
pub struct PoolConfig {
    /// Max concurrent connections (default: 50)
    pub max_connections: u32,
    /// Max connections per IP (default: 3, prevents single-IP saturation)
    pub max_per_ip: u32,
    /// Idle timeout before disconnect (default: 300s)
    pub idle_timeout: Duration,
    /// Keep-alive interval (default: 60s)
    pub keep_alive_interval: Duration,
    /// Handshake timeout (default: 15s)
    pub handshake_timeout: Duration,
    /// Request timeout (default: 30s)
    pub request_timeout: Duration,
    /// Max pending requests per peer (default: 5)
    pub max_pending_per_peer: u32,
    /// Download rate limit per peer (bytes/sec, default: 0 = unlimited)
    pub per_peer_rate_limit: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub total_connections: u32,
    pub established: u32,
    pub connecting: u32,
    pub handshaking: u32,
    pub disconnecting: u32,
    pub pending_outbound: u32,
    pub download_speed_total: u64,
    pub upload_speed_total: u64,
    pub total_downloaded: u64,
    pub total_uploaded: u64,
    pub avg_rtt: Duration,
}

impl ConnectionPool {
    pub fn new(local_peer_id: [u8; 20], config: PoolConfig) -> Self;

    /// Add a peer to the pool and initiate connection.
    /// Returns error if pool is full or peer already exists.
    pub async fn add_peer(&mut self, peer_info: PeerInfo, info_hash: &[u8; 20]) -> Result<()> {
        // Check pool capacity
        if self.connections.len() >= self.max_connections as usize {
            return Err(Error::ConnectionLimitReached);
        }

        // Check per-IP limit
        let ip_count = self.connections.values()
            .filter(|c| c.addr.ip() == peer_info.addr.ip())
            .count();
        if ip_count >= self.config.max_per_ip as usize {
            return Err(Error::PerIpLimitReached);
        }

        // Check if peer already exists
        let peer_id = peer_info.peer_id;
        if self.connections.contains_key(&peer_id) {
            return Err(Error::PeerAlreadyConnected);
        }

        // Create connection object
        let mut conn = PeerConnection::new(peer_id, peer_info.addr);

        // Initiate connection in background
        let handle = tokio::spawn(async move {
            conn.connect(info_hash, &self.local_peer_id).await
        });

        self.pending_outbound.insert(peer_id, handle);
        Ok(())
    }

    /// Remove a peer from the pool (called on disconnect or error)
    pub async fn remove_peer(&mut self, peer_id: &[u8; 20]) -> Result<()> {
        if let Some(mut conn) = self.connections.remove(peer_id) {
            conn.disconnect().await?;
        }
        self.pending_outbound.remove(peer_id);
        Ok(())
    }

    /// Get a peer by peer_id
    pub fn get(&self, peer_id: &[u8; 20]) -> Option<&PeerConnection> {
        self.connections.get(peer_id)
    }

    pub fn get_mut(&mut self, peer_id: &[u8; 20]) -> Option<&mut PeerConnection> {
        self.connections.get_mut(peer_id)
    }

    /// Select peers for downloading based on priority requirement
    pub fn select_download_peers(&self, count: u32, piece_index: u32) -> Vec<&PeerConnection> {
        let mut candidates: Vec<&PeerConnection> = self.connections
            .values()
            .filter(|c| c.state == ConnectionState::Established)
            .filter(|c| !c.am_choked) // peer has unchoked us
            .filter(|c| c.bitfield.has(piece_index)) // peer has this piece
            .collect();

        // Sort by download speed descending
        candidates.sort_by(|a, b| {
            b.stats.download_speed.cmp(&a.stats.download_speed)
        });

        candidates.truncate(count as usize);
        candidates
    }

    /// Select peers for uploading (who to serve data to)
    pub fn select_upload_peers(&self, count: u32) -> Vec<&PeerConnection> {
        self.connections
            .values()
            .filter(|c| c.state == ConnectionState::Established)
            .filter(|c| c.peer_interested)
            .filter(|c| !c.peer_choked) // we have unchoked them
            .take(count as usize)
            .collect()
    }

    /// Re-evaluate interested state for all connections
    pub fn refresh_interested(&mut self, local_bitfield: &Bitfield) {
        for conn in self.connections.values_mut() {
            conn.evaluate_interested(local_bitfield);
        }
    }

    /// Run periodic maintenance:
    /// - Cleanup idle connections
    /// - Send keep-alives
    /// - Check pending connection results
    pub async fn maintain(&mut self) -> Result<MaintenanceResult> {
        let mut result = MaintenanceResult::default();

        // 1. Process completed pending connections
        let mut completed = Vec::new();
        for (peer_id, handle) in &mut self.pending_outbound {
            if handle.is_finished() {
                completed.push(*peer_id);
            }
        }
        for peer_id in completed {
            if let Some(handle) = self.pending_outbound.remove(&peer_id) {
                match handle.await {
                    Ok(Ok(())) => {
                        // Connection successful — it was moved to connections
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Connection to peer failed: {}", e);
                        result.failed_connections += 1;
                    }
                    Err(e) => {
                        tracing::error!("Connection task panicked: {}", e);
                        result.failed_connections += 1;
                    }
                }
            }
        }

        // 2. Cleanup idle connections
        let idle: Vec<[u8; 20]> = self.connections
            .iter()
            .filter(|(_, conn)| conn.state == ConnectionState::Established)
            .filter(|(_, conn)| conn.is_idle(self.config.idle_timeout))
            .map(|(id, _)| *id)
            .collect();
        for peer_id in idle {
            self.remove_peer(&peer_id).await?;
            result.closed_idle += 1;
        }

        // 3. Send keep-alives
        let now = Instant::now();
        for conn in self.connections.values_mut() {
            if conn.state == ConnectionState::Established {
                if now.duration_since(conn.last_active) >= self.config.keep_alive_interval {
                    // Send keep-alive (0-length message)
                    let keepalive = PeerMessage::new(MsgId::KeepAlive, vec![]);
                    conn.send_message(keepalive).await.ok();
                    conn.mark_active();
                    result.keepalives_sent += 1;
                }
            }
        }

        // 4. Recalculate choking
        self.choking.recalculate();

        Ok(result)
    }

    /// Pool statistics
    pub fn stats(&self) -> PoolStats {
        let mut stats = PoolStats::default();
        for conn in self.connections.values() {
            match conn.state {
                ConnectionState::Established => stats.established += 1,
                ConnectionState::Connecting => stats.connecting += 1,
                ConnectionState::Handshaking => stats.handshaking += 1,
                ConnectionState::Disconnecting => stats.disconnecting += 1,
                ConnectionState::Disconnected => {}
            }
            stats.total_downloaded += conn.stats.bytes_downloaded;
            stats.total_uploaded += conn.stats.bytes_uploaded;
            stats.download_speed_total += conn.stats.download_speed;
            stats.upload_speed_total += conn.stats.upload_speed;
        }
        stats.total_connections = self.connections.len() as u32;
        stats.pending_outbound = self.pending_outbound.len() as u32;
        stats
    }
}

#[derive(Debug, Default)]
pub struct MaintenanceResult {
    pub closed_idle: u32,
    pub failed_connections: u32,
    pub keepalives_sent: u32,
}
```

---

## 7. Peer Scoring and Selection

### Scoring Algorithm

```rust
pub struct PeerScorer;

impl PeerScorer {
    /// Compute a composite score for peer selection.
    /// Higher is better. Used for initial connection prioritization.
    pub fn score(peer: &PeerInfo, local_context: &NodeContext) -> f64 {
        let mut score = 0.0;

        // 1. Bandwidth score (normalized to 0-100)
        let bw = peer.bw_down.max(peer.bw_up) as f64;
        let bw_score = (bw / 1_048_576.0).min(100.0) * 0.30;
        score += bw_score;

        // 2. Latency penalty (0-200ms: full, 200-500ms: decreasing, >500ms: low)
        let latency_ms = peer.latency.as_millis() as f64;
        let latency_score = if latency_ms < 200.0 {
            30.0
        } else if latency_ms < 500.0 {
            30.0 * (1.0 - (latency_ms - 200.0) / 300.0)
        } else {
            5.0
        };
        score += latency_score;

        // 3. Geo bonus (same region = lower latency, better peering)
        if let Some(ref loc) = peer.location {
            if loc == &local_context.region {
                score += 15.0;
            }
        }

        // 4. Firewall penalty (firewalled peers can't serve us via incoming)
        if peer.is_firewalled {
            score *= 0.5;
        }

        // 5. Seeder bonus (peers with full file are more useful)
        if peer.is_seeder {
            score += 20.0;
        }

        // 6. Peer_id diversity penalty (cluster detection)
        // Peers with similar peer_id prefixes are likely from same ISP/AS
        // and may share bottleneck links
        if peer.peer_id[..4] == local_context.local_peer_id[..4] {
            score *= 0.8;
        }

        score.max(0.0)
    }
}

pub struct NodeContext {
    pub region: String,
    pub local_peer_id: [u8; 20],
    pub local_addr: SocketAddr,
    pub is_firewalled: bool,
}
```

### Peer Selection Strategies

```rust
pub enum PeerSelectionStrategy {
    /// Pick highest-scored peers first (default)
    TopScore,
    /// Random selection among top 50%
    RandomAmongTop,
    /// Ensure diversity: pick from different /24 subnets
    DiversityFirst,
    /// Prefer peers with pieces we need right now
    UrgentOnly,
}

impl ConnectionPool {
    /// Select the best N peers for initial connection from a larger candidate list
    pub fn select_best_peers(
        candidates: Vec<PeerInfo>,
        count: usize,
        strategy: PeerSelectionStrategy,
        context: &NodeContext,
        local_bitfield: &Bitfield,
    ) -> Vec<PeerInfo> {
        match strategy {
            PeerSelectionStrategy::TopScore => {
                let mut scored: Vec<(f64, PeerInfo)> = candidates
                    .into_iter()
                    .map(|p| (PeerScorer::score(&p, context), p))
                    .collect();
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                scored.into_iter().take(count).map(|(_, p)| p).collect()
            }
            PeerSelectionStrategy::DiversityFirst => {
                // Group by /24 subnet, pick best from each
                let mut by_subnet: HashMap<u32, Vec<PeerInfo>> = HashMap::new();
                for peer in candidates {
                    if let IpAddr::V4(v4) = peer.addr.ip() {
                        let subnet = u32::from(v4) & 0xFFFFFF00;
                        by_subnet.entry(subnet).or_default().push(peer);
                    }
                }
                let mut selected = Vec::new();
                for peers in by_subnet.values_mut() {
                    peers.sort_by(|a, b| {
                        let sa = PeerScorer::score(a, context);
                        let sb = PeerScorer::score(b, context);
                        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    if let Some(best) = peers.first() {
                        selected.push(best.clone());
                    }
                }
                selected.truncate(count);
                selected
            }
            PeerSelectionStrategy::UrgentOnly => {
                candidates.into_iter()
                    .filter(|p| has_urgent_pieces(p, local_bitfield))
                    .collect()
            }
            PeerSelectionStrategy::RandomAmongTop => {
                let mut scored: Vec<(f64, PeerInfo)> = candidates
                    .into_iter()
                    .map(|p| (PeerScorer::score(&p, context), p))
                    .collect();
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                let cutoff = (scored.len() as f64 * 0.5) as usize;
                let mut rng = thread_rng();
                scored.truncate(cutoff);
                scored.shuffle(&mut rng);
                scored.into_iter().take(count).map(|(_, p)| p).collect()
            }
        }
    }
}

fn has_urgent_pieces(peer: &PeerInfo, local_bitfield: &Bitfield) -> bool {
    // Fast check: if we have no bitfield yet, consider all peers
    if local_bitfield.is_empty() {
        return true;
    }
    // Otherwise check if peer has pieces in the urgent window
    // This requires peer bitfield info, which we may not have before connecting
    // So this strategy is only useful after initial bitfield exchange
    true
}
```

---

## 8. Keep-Alive and Timeout Handling

### Keep-Alive Mechanism

```rust
impl PeerConnection {
    /// Send a keep-alive message (length prefix = 0)
    pub async fn send_keepalive(&mut self) -> Result<()> {
        let msg = PeerMessage::new(MsgId::KeepAlive, vec![]);
        self.send_message(msg).await?;
        self.mark_active();
        Ok(())
    }

    /// Check if keep-alive is due
    pub fn keepalive_due(&self, interval: Duration) -> bool {
        self.last_active.elapsed() >= interval
    }
}
```

### Timeout Configuration

| Timeout | Default | Description |
|---------|---------|-------------|
| TCP connect | 10s | Time to establish TCP connection |
| Handshake | 15s | Time to exchange 68-byte handshake |
| Idle (no messages) | 300s (5min) | Disconnect if completely idle |
| Keep-alive interval | 60s | Send keep-alive if no other messages |
| Request response | 30s | Time to wait for a piece response |
| Stale connection | 120s (2min) | Connection alive but no data transfer |
| Snubbing detection | 15s | Time before considering peer "snubbing" us |

### Connection Monitoring

```rust
impl ConnectionPool {
    /// Periodic health check of all connections
    pub async fn health_check(&mut self) -> Vec<HealthReport> {
        let mut reports = Vec::new();
        let now = Instant::now();

        let to_remove: Vec<[u8; 20]> = self.connections
            .iter()
            .filter_map(|(id, conn)| {
                let elapsed = now.duration_since(conn.last_active);

                if conn.state == ConnectionState::Disconnected {
                    Some((*id, "already disconnected"))
                } else if elapsed > self.config.idle_timeout {
                    Some((*id, "idle timeout"))
                } else if conn.state == ConnectionState::Handshaking
                    && elapsed > self.config.handshake_timeout {
                    Some((*id, "handshake timeout"))
                } else {
                    None
                }
            })
            .map(|(id, reason)| id)
            .collect();

        for peer_id in to_remove {
            if let Some(conn) = self.connections.get(&peer_id) {
                reports.push(HealthReport {
                    peer_id,
                    addr: conn.addr,
                    state: conn.state,
                    reason: "timeout".into(),
                    duration: conn.connected_since
                        .map(|t| t.elapsed())
                        .unwrap_or_default(),
                    bytes_transferred: conn.stats.bytes_downloaded + conn.stats.bytes_uploaded,
                });
            }
            self.remove_peer(&peer_id).await.ok();
        }

        reports
    }
}

#[derive(Debug, Clone)]
pub struct HealthReport {
    pub peer_id: [u8; 20],
    pub addr: SocketAddr,
    pub state: ConnectionState,
    pub reason: String,
    pub duration: Duration,
    pub bytes_transferred: u64,
}
```

---

## 9. Connection Limits and Rate Limiting

### Per-Peer Rate Limiting

```rust
pub struct RateLimiter {
    /// Per-peer token buckets
    buckets: HashMap<[u8; 20], TokenBucket>,
    /// Global rate limit
    global: TokenBucket,
    /// Configuration
    config: RateLimitConfig,
}

pub struct RateLimitConfig {
    /// Max bytes per second per peer (default: 0 = unlimited)
    pub per_peer_bytes_per_sec: u64,
    /// Max bytes per second globally (default: 0 = unlimited)
    pub global_bytes_per_sec: u64,
    /// Burst size (default: 64KB)
    pub burst_size: u64,
}

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(rate: u64, burst: u64) -> Self {
        Self {
            tokens: burst as f64,
            max_tokens: burst as f64,
            refill_rate: rate as f64,
            last_refill: Instant::now(),
        }
    }

    fn consume(&mut self, amount: u64) -> bool {
        self.refill();
        if self.tokens >= amount as f64 {
            self.tokens -= amount as f64;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = Instant::now();
    }
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self;

    /// Check if a peer should be allowed to send `size` bytes.
    /// Returns true if allowed, false if rate-limited.
    pub fn check_peer(&mut self, peer_id: &[u8; 20], size: u64) -> bool {
        // Check global limit first
        if !self.global.consume(size) {
            return false;
        }

        // Check per-peer limit
        let bucket = self.buckets
            .entry(*peer_id)
            .or_insert_with(|| TokenBucket::new(
                self.config.per_peer_bytes_per_sec,
                self.config.burst_size,
            ));

        bucket.consume(size)
    }

    /// Clean up stale buckets periodically
    pub fn cleanup(&mut self, peer_ids: &HashSet<[u8; 20]>) {
        self.buckets.retain(|id, _| peer_ids.contains(id));
    }
}
```

### Per-IP Connection Limit

```rust
impl ConnectionPool {
    /// Count connections per IP
    fn count_per_ip(&self, ip: IpAddr) -> u32 {
        self.connections
            .values()
            .filter(|c| c.addr.ip() == ip)
            .count() as u32
    }

    /// Check if we can add a peer from this IP
    fn check_per_ip_limit(&self, ip: IpAddr) -> bool {
        self.count_per_ip(ip) < self.config.max_per_ip
    }
}
```

---

## 10. Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Connection timed out: {0}")]
    Timeout(String),

    #[error("Connection reset by peer")]
    ConnectionReset,

    #[error("Peer sent invalid message: {0}")]
    InvalidMessage(String),

    #[error("Protocol violation: {0}")]
    ProtocolViolation(String),

    #[error("Connection limit reached (max {0})")]
    ConnectionLimitReached(u32),

    #[error("Per-IP connection limit reached for {0}")]
    PerIpLimitReached(IpAddr),

    #[error("Peer already connected")]
    PeerAlreadyConnected,

    #[error("Peer choked our request")]
    Choked,

    #[error("Request timed out for piece {0}")]
    RequestTimeout(u32),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## Summary

QVOD's peer connection management is designed for **streaming media delivery**, not bulk file distribution. Key differentiators from BitTorrent:

1. **Streaming-optimized choking**: Unchoke peers who have the data we need *right now*, not necessarily those who upload the fastest. The playhead position drives all choking decisions.

2. **Hybrid TCP/UDP**: Critical data (I-frames, metadata) uses reliable TCP; non-critical data (P/B-frames) uses faster UDP with custom congestion control.

3. **Aggressive timeouts**: Streaming is time-sensitive. 15s snubbing detection vs BitTorrent's 60s. 300s idle disconnect vs BitTorrent's 120s.

4. **Geo-aware peer selection**: Same-region peers get scoring bonuses because lower latency directly improves streaming quality.

5. **Per-IP limits**: Prevents any single IP from saturating the connection pool, improving swarm diversity and resilience.

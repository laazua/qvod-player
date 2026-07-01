# QVOD Wire Protocol Reference

## 1. Overview

QVOD uses a modified BitTorrent-compatible wire protocol with extensions for streaming. Two transports coexist:

| Transport | Role | Reliability |
|-----------|------|-------------|
| TCP | Handshake, control messages, I-frame (keyframe) data | Guaranteed |
| UDP | P/B-frame data transfer, DHT RPC | Best-effort |

All integer values are **big-endian** (network byte order) unless noted.

---

## 2. TCP Peer Wire Protocol

### 2.1 Connection Lifecycle

```
CLIENT                                    SERVER
  |                                         |
  |-------- TCP SYN ----------------------->|
  |<------- TCP SYN-ACK --------------------|
  |-------- Handshake (68 bytes) ---------->|
  |<------- Handshake (68 bytes) -----------|
  |<------- Bitfield -----------------------|
  |-------- Interested -------------------->|
  |<------- Unchoke ------------------------|
  |-------- Request (index, begin, len) --->|
  |<------- Piece (index, begin, data) -----|
  |-------- Request (index, begin, len) --->|
  |<------- Piece (index, begin, data) -----|
  ...                                       ...
  |-------- Keep-Alive -------------------->|
  ...                                       ...
  |-------- Cancel ------------------------>|
  |-------- TCP FIN ----------------------->|
```

### 2.2 Handshake Message (68 bytes)

Fixed-size, no length prefix. Sent immediately after TCP connection establishment.

| Offset | Size | Field | Value |
|--------|------|-------|-------|
| 0 | 1 | `pstrlen` | `0x13` (19) |
| 1 | 19 | `pstr` | `"Qvod P2SP Protocol"` |
| 20 | 8 | `reserved` | `0x00 * 8` (extension bits in BitTorrent; may carry flags) |
| 28 | 20 | `info_hash` | SHA-1 hash of metadata |
| 48 | 20 | `peer_id` | 20-byte client identifier |

**Hex dump example (handshake):**
```
13 51 76 6F 64 20 50 32 53 50 20 50 72 6F 74 6F  .Qvod P2SP Proto
63 6F 6C 00 00 00 00 00 00 00 00 A1 B2 C3 D4 E5  col.............
F6 G7 H8 I9 J0 K1 L2 M3 N4 O5 P6 Q7 R8 S9 T0 U1  ................
V2 W3 X4 Y5 Z6                                    ......
```

**Rust struct:**

```rust
#[repr(C, packed)]
pub struct Handshake {
    pub pstrlen: u8,                     // = 19
    pub pstr: [u8; 19],                  // "Qvod P2SP Protocol"
    pub reserved: [u8; 8],               // extension flags
    pub info_hash: [u8; 20],             // SHA-1 info_hash
    pub peer_id: [u8; 20],               // peer identifier
}

impl Handshake {
    pub const PROTOCOL: &'static [u8; 19] = b"Qvod P2SP Protocol";

    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            pstrlen: 19,
            pstr: *Self::PROTOCOL,
            reserved: [0u8; 8],
            info_hash,
            peer_id,
        }
    }

    pub fn encode(&self) -> [u8; 68] {
        let mut buf = [0u8; 68];
        buf[0] = self.pstrlen;
        buf[1..20].copy_from_slice(&self.pstr);
        buf[20..28].copy_from_slice(&self.reserved);
        buf[28..48].copy_from_slice(&self.info_hash);
        buf[48..68].copy_from_slice(&self.peer_id);
        buf
    }

    pub fn decode(buf: &[u8; 68]) -> Result<Self, ProtocolError> {
        if buf[0] != 19 || &buf[1..20] != Self::PROTOCOL {
            return Err(ProtocolError::InvalidHandshake);
        }
        let mut info_hash = [0u8; 20];
        let mut peer_id = [0u8; 20];
        info_hash.copy_from_slice(&buf[28..48]);
        peer_id.copy_from_slice(&buf[48..68]);
        Ok(Self {
            pstrlen: 19,
            pstr: *Self::PROTOCOL,
            reserved: {
                let mut r = [0u8; 8];
                r.copy_from_slice(&buf[20..28]);
                r
            },
            info_hash,
            peer_id,
        })
    }
}
```

### 2.3 Peer ID Convention

```
Format: [-QVOD-][version][random12]

Example: -QVOD-0001ABCDEF12345678
         ^^^^^ ^^^^^^^^^^^^^^^^^^^^
         |     |
         |     16 alphanumeric chars
         |
         6-char client identifier
```

| Byte range | Size | Field |
|-----------|------|-------|
| 0-5 | 6 | Client ID (`-QVOD-`) |
| 6-7 | 2 | Version major.minor (e.g. `01` = v0.1) |
| 8-19 | 12 | Random alphanumeric |

### 2.4 Reserved Bytes — Extension Bits

| Bit | Extension | Description |
|-----|-----------|-------------|
| 0 | `ut_metadata` | Metadata exchange (BEP 9) |
| 1 | `ut_pex` | Peer exchange (BEP 11) |
| 2 | `qvod_speed` | QVOD speed report extension |
| 3 | `qvod_keyframe` | Keyframe index exchange |
| 4-7 | Unused | Reserved for future |

### 2.5 Message Framing

All messages after handshake use a 4-byte length prefix:

```
+----------------+--------+--------------------------+
| length_prefix  | msg_id | payload                  |
| (4 bytes, BE)  | (1)    | (variable length)        |
+----------------+--------+--------------------------+
```

`length_prefix` = length of `msg_id + payload` (not inclusive of the prefix itself).  
A `length_prefix` of `0` indicates a keep-alive message (no `msg_id` or `payload`).

### 2.6 Message Types

| ID | Name | Payload | Direction |
|----|------|---------|-----------|
| `0x00` | `choke` | None | Both |
| `0x01` | `unchoke` | None | Both |
| `0x02` | `interested` | None | Both |
| `0x03` | `not_interested` | None | Both |
| `0x04` | `have` | `piece_index: u32` | Both |
| `0x05` | `bitfield` | `bitfield: [u8; N]` | Both |
| `0x06` | `request` | `index: u32, begin: u32, length: u32` | Both |
| `0x07` | `piece` | `index: u32, begin: u32, block: [u8]` | Both |
| `0x08` | `cancel` | `index: u32, begin: u32, length: u32` | Both |
| `0x09` | `port` | `dht_port: u16` | Both |
| `0x0A` | `suggest_piece` | `piece_index: u32` | Both |
| `0x0B` | `reject_request` | `index: u32, begin: u32, length: u32` | Both |
| `0x0C` | `have_all` | None | Both |
| `0x0D` | `have_none` | None | Both |
| `0x14` | `extended` | `ext_msg_id: u8, payload: [u8]` | Both |

#### Message 0x05: Bitfield

Payload is a variable-length bitfield where bit `i` (from MSB) indicates piece `i` availability.

```
Example: 3 pieces → 1 byte: 0b11100000
         ^^^
         pieces 0,1,2 available
```

For `N` pieces, bitfield size = `ceil(N / 8)` bytes.

```rust
pub struct Bitfield {
    pub bytes: Vec<u8>,
    pub piece_count: u32,
}

impl Bitfield {
    pub fn has(&self, index: u32) -> bool {
        let byte_idx = (index / 8) as usize;
        let bit_idx = 7 - (index % 8);
        byte_idx < self.bytes.len() && (self.bytes[byte_idx] & (1 << bit_idx)) != 0
    }

    pub fn set(&mut self, index: u32, value: bool) {
        let byte_idx = (index / 8) as usize;
        let bit_idx = 7 - (index % 8);
        if value {
            self.bytes[byte_idx] |= 1 << bit_idx;
        } else {
            self.bytes[byte_idx] &= !(1 << bit_idx);
        }
    }

    pub fn completion(&self) -> f64 {
        let total = self.piece_count;
        if total == 0 { return 0.0; }
        let done = (0..total).filter(|&i| self.has(i)).count();
        done as f64 / total as f64
    }
}
```

#### Message 0x06: Request (12-byte payload)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `index` | Piece index |
| 4 | 4 | `begin` | Byte offset within piece |
| 8 | 4 | `length` | Block length (typically 16 KB) |

```rust
#[repr(C, packed)]
pub struct RequestPayload {
    pub index: u32,   // piece index
    pub begin: u32,   // offset within piece
    pub length: u32,  // block size (max 16KB for streaming)
}
```

#### Message 0x07: Piece (variable payload)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `index` | Piece index |
| 4 | 4 | `begin` | Byte offset within piece |
| 8 | variable | `block` | Raw block data |

#### Message 0x09: Port (2-byte payload)

Advertises DHT listening port.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 2 | `dht_port: u16` |

#### Message 0x0A: Suggest Piece (QVOD extension)

Tells the peer which piece to request next — used for scheduling hints.

```rust
pub struct SuggestPiece {
    pub piece_index: u32,
}
```

#### Message 0x14: Extended Message

Used for protocol extensions (BEP 10). The first byte of payload is the extended message ID negotiated during handshake.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 | `ext_msg_id` |
| 1 | variable | `ext_payload` (typically Bencode dict) |

### 2.7 Keep-Alive

A message with `length_prefix = 0` (4 zero bytes). Sent every 120 seconds on idle connections.

```
Hex: 00 00 00 00
```

---

## 3. Extension Protocol (BEP 10)

### 3.1 Handshake Extension

During the initial handshake, if bit 0 of byte 5 in `reserved` is set (the `ut_metadata` bit), both peers send an `extended` message (msg_id=0x14) with `ext_msg_id=0` containing a Bencode dictionary:

```bencode
d
1:m d6:ut_metadatai3e6:ut_pexi1e10:qvod_speedi2ee
1:p {peer_id}
1:v qvs-0.1.0
e
```

`ext_msg_id=0` is reserved for the handshake extension. The payload maps extension names to local message IDs.

```rust
pub struct ExtensionHandshake {
    pub m: HashMap<String, u8>,        // extension name → local msg id
    pub p: Option<[u8; 20]>,           // peer id (optional)
    pub v: Option<String>,             // client version
    pub metadata_size: Option<usize>,   // metadata size for BEP 9
    pub qvod_speed: Option<u8>,        // QVOD speed reporting version
    pub qvod_keyframe: Option<u8>,     // QVOD keyframe exchange version
}
```

### 3.2 ut_metadata (BEP 9)

| Extended Msg ID | Request Type |
|-----------------|--------------|
| `msg_type=0` | `request` |
| `msg_type=1` | `data` |
| `msg_type=2` | `reject` |

**Request message:**

```bencode
d
8:msg_type i0e
4:piece i0e
e
```

**Data message (includes raw metadata after dictionary):**

```bencode
d
8:msg_type i1e
4:piece i0e
11:total_size i12345e
e
<raw metadata bytes>
```

```rust
pub struct MetadataRequest {
    pub msg_type: u8,
    pub piece: u32,
}

pub struct MetadataData {
    pub msg_type: u8,
    pub piece: u32,
    pub total_size: u64,
    pub data: Vec<u8>,
}
```

### 3.3 ut_pex (BEP 11)

Sent periodically to exchange known peer addresses.

```bencode
d
5:added <6-byte compact peer entries>
e
```

### 3.4 qvod_speed Extension (QVOD-specific)

Allows peers to exchange current download/upload speed for scoring.

```bencode
d
4:down i{download_speed}e
2:up i{upload_speed}e
e
```

### 3.5 qvod_keyframe Extension (QVOD-specific)

Requests keyframe index from peers before metadata is fully available.

```bencode
d
4:type i{0=request, 1=response}e
6:offset i{file_offset_for_lookup}e
5:count i{number_of_frames}e
e
```

**Response payload:**

```bencode
d
4:type i1e
7:entries l
  d3:tsi{timestamp_ms}e4:offi{file_offset}e5:sizi{frame_size}e4:typei{0=I,1=P,2=B}ee
  ...
e
e
```

---

## 4. UDP DHT RPC Protocol

### 4.1 Common Header (8 bytes)

All DHT RPC messages begin with:

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `magic` | `0x51 0x56 0x44 0x54` = "QVDT" |
| 4 | 1 | `msg_type` | RPC method |
| 5 | 2 | `txn_id` | Transaction ID (echoed in response) |
| 7 | 1 | `ver` | Protocol version (currently `0x01`) |

```rust
#[repr(C, packed)]
pub struct DhtHeader {
    pub magic: [u8; 4],     // [0x51, 0x56, 0x44, 0x54]
    pub msg_type: u8,
    pub txn_id: [u8; 2],    // big-endian
    pub ver: u8,
}

impl DhtHeader {
    pub const MAGIC: [u8; 4] = [0x51, 0x56, 0x44, 0x54];
    pub const VERSION: u8 = 0x01;
}
```

### 4.2 Message Types

| `msg_type` | Name | Description |
|-----------|------|-------------|
| `0x00` | `PING` | Node liveness check |
| `0x01` | `FIND_NODE` | Find nodes close to a target |
| `0x02` | `FIND_PEERS` | Find peers for an info_hash |
| `0x03` | `ANNOUNCE` | Announce as a peer for an info_hash |

### 4.3 PING (0x00)

**Request:**
```
header(8) + node_id(20)
Total: 28 bytes
```

**Response:**
```
header(8) + node_id(20)
Total: 28 bytes
```

**Hex dump (PING request):**
```
51 56 44 54 00 00 01 01  QVDT....
A1 B2 C3 D4 E5 F6 G7 H8  ........
I9 J0 K1 L2 M3 N4 O5 P6  ........
Q7 R8 S9 T0                ....
```

### 4.4 FIND_NODE (0x01)

**Request:**
```
header(8) + node_id(20) + target(20)
Total: 48 bytes
```

**Response:**
```
header(8) + node_id(20) + nodes(n * 26)
```

Each node entry in response is 26 bytes:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 20 | `node_id` |
| 20 | 4 | `ip` (binary IPv4) |
| 24 | 2 | `port` |

```rust
#[repr(C, packed)]
pub struct NodeEntry {
    pub node_id: [u8; 20],
    pub ip: [u8; 4],
    pub port: u16,
}

impl NodeEntry {
    pub fn encode(&self) -> [u8; 26] {
        let mut buf = [0u8; 26];
        buf[0..20].copy_from_slice(&self.node_id);
        buf[20..24].copy_from_slice(&self.ip);
        buf[24..26].copy_from_slice(&self.port.to_be_bytes());
        buf
    }
}
```

### 4.5 FIND_PEERS (0x02)

**Request:**
```
header(8) + node_id(20) + info_hash(20)
Total: 48 bytes
```

**Response (has peers):**
```
header(8) + node_id(20) + marker("peers") + peer_list
```

**Response (no peers — return close nodes):**
```
header(8) + node_id(20) + marker("nodes") + nodes(n*26)
```

Peer encoding in response (compact, 6 bytes per peer):

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | `ip` (binary IPv4) |
| 4 | 2 | `port` |

```rust
pub struct PeerEntry {
    pub ip: [u8; 4],
    pub port: u16,
}

impl PeerEntry {
    pub fn encode_compact(&self) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0..4].copy_from_slice(&self.ip);
        buf[4..6].copy_from_slice(&self.port.to_be_bytes());
        buf
    }

    pub fn decode_compact(buf: &[u8; 6]) -> Self {
        Self {
            ip: {
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&buf[0..4]);
                ip
            },
            port: u16::from_be_bytes([buf[4], buf[5]]),
        }
    }
}
```

### 4.6 ANNOUNCE (0x03)

**Request:**
```
header(8) + node_id(20) + info_hash(20) + port(2)
Total: 50 bytes
```

**Response:**
```
header(8) + node_id(20) + status(2)
```

Status values:
- `0x00 0x4F` = "OK"
- `0x00 0x45` = error

```rust
#[repr(C, packed)]
pub struct AnnounceRequest {
    pub header: DhtHeader,
    pub node_id: [u8; 20],
    pub info_hash: [u8; 20],
    pub port: u16,
}

impl AnnounceRequest {
    pub const MSG_TYPE: u8 = 0x03;
}
```

### 4.7 DHT Transaction Management

```rust
pub struct DhtTransaction {
    pub txn_id: u16,
    pub target: SocketAddr,
    pub method: DhtMethod,
    pub sent_at: Instant,
    pub retries: u8,
}

pub struct DhtTransactionTable {
    transactions: HashMap<u16, DhtTransaction>,
    next_txn_id: u16,
    timeout: Duration,       // default 5s
    max_retries: u8,         // default 3
}

impl DhtTransactionTable {
    pub fn new() -> Self;
    pub fn allocate(&mut self, target: SocketAddr, method: DhtMethod) -> u16;
    pub fn resolve(&mut self, txn_id: u16) -> Option<DhtTransaction>;
    pub fn expire(&mut self) -> Vec<DhtTransaction>;  // returns timed-out txns
}
```

---

## 5. UDP Data Transfer Protocol

Used for P/B-frame data (non-critical). A separate UDP socket from the DHT socket.

### 5.1 UDP Packet Header (16 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `magic` | `0x51 0x56 0x44 0x54` = "QVDT" |
| 4 | 1 | `msg_type` | See below |
| 5 | 4 | `seq` | Sequence number (for ACK/NACK) |
| 9 | 4 | `piece_index` | Which piece this packet belongs to |
| 13 | 2 | `block_offset` | Block offset within piece (in blocks; 0-15 for 256KB/16KB) |
| 15 | 1 | `flags` | Bit flags (see below) |

**`msg_type` values:**

| Value | Name |
|-------|------|
| `0x01` | `DATA` |
| `0x02` | `ACK` |
| `0x03` | `NACK` |
| `0x04` | `PING` (UDP control, not DHT) |
| `0x05` | `PONG` (UDP control) |

**`flags` bitfield:**

| Bit | Meaning |
|-----|---------|
| 0 | `MORE` — more blocks follow for same piece |
| 1 | `LAST` — last block of this piece |
| 2 | `PRIORITY` — high priority data |
| 3-7 | Reserved |

### 5.2 DATA Packet (0x01)

```
header(16) + payload(up to 1400 - 16 = 1384 bytes)
```

Maximum packet size: 1400 bytes (MTU-safe). Typical block: 1384 bytes payload + 16 header.

```rust
#[repr(C, packed)]
pub struct UdpDataHeader {
    pub magic: [u8; 4],
    pub msg_type: u8,          // 0x01
    pub seq: u32,
    pub piece_index: u32,
    pub block_offset: u16,     // in units of BLOCK_SIZE
    pub flags: u8,
}

impl UdpDataHeader {
    pub const MAGIC: [u8; 4] = [0x51, 0x56, 0x44, 0x54];
    pub const SIZE: usize = 16;
    pub const MAX_PAYLOAD: usize = 1384;
    pub const MTU_SAFE: usize = 1400;
}
```

### 5.3 ACK Packet (0x02)

Acknowledges receipt of one or more data packets.

```
header(16) + ack_bitmask(4)
```

`ack_bitmask` is a 32-bit bitmask where bit `i` acknowledges sequence number `(base_seq + i)`.
The `seq` field in the header is the base sequence number.

```rust
#[repr(C, packed)]
pub struct UdpAck {
    pub header: UdpDataHeader,  // msg_type = 0x02
    pub ack_bitmask: u32,       // cumulative ACK bitmask
}
```

### 5.4 NACK Packet (0x03)

Requests retransmission of specific blocks.

```
header(16) + nack_count(2) + nack_entries(nack_count * 6)
```

Each NACK entry:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | `seq` (original sequence number) |
| 4 | 2 | `block_offset` |

```rust
pub struct UdpNackEntry {
    pub seq: u32,
    pub block_offset: u16,
}
```

### 5.5 UDP Congestion Control

```rust
pub struct UdpCongestionState {
    pub cwnd: u32,                    // congestion window (packets)
    pub ssthresh: u32,                // slow start threshold
    pub rtt: Duration,                // smoothed RTT
    pub rtt_var: Duration,            // RTT variance
    pub loss_rate: f64,               // estimated loss rate
    pub sent_map: HashMap<u32, SentPacket>,
    pub next_seq: u32,
    pub outstanding: u32,             // packets in flight
}

pub struct SentPacket {
    pub seq: u32,
    pub sent_at: Instant,
    pub size: u16,
    pub retransmitted: bool,
}

impl UdpCongestionState {
    pub fn new() -> Self;

    pub fn on_ack(&mut self, seq: u32, now: Instant) {
        self.outstanding = self.outstanding.saturating_sub(1);
        if self.cwnd < self.ssthresh {
            // Slow start: +1 per ACK
            self.cwnd += 1;
        } else {
            // Congestion avoidance: +1 per RTT (approximately)
            self.cwnd = self.cwnd.saturating_add(1).max(self.cwnd / 2 + 1);
            // More precise: cwnd += 1/cwnd per ACK
        }
        // Update RTT estimation (Jacobson/Karels algorithm)
        if let Some(packet) = self.sent_map.get(&seq) {
            let rtt_sample = now - packet.sent_at;
            let err = rtt_sample.as_micros() as i64 - self.rtt.as_micros() as i64;
            self.rtt = Duration::from_micros(
                (self.rtt.as_micros() as i64 + err / 8) as u64
            );
            self.rtt_var = Duration::from_microos(
                (self.rtt_var.as_micros() as i64 + (err.abs() - self.rtt_var.as_micros() as i64) / 4) as u64
            );
        }
    }

    pub fn on_loss(&mut self) {
        self.ssthresh = self.cwnd / 2;
        self.cwnd = self.ssthresh.max(2);
        self.loss_rate = self.loss_rate * 0.9 + 0.1;
    }

    pub fn can_send(&self) -> bool {
        self.outstanding < self.cwnd
    }

    /// RTO = smoothed RTT + 4 * RTT variance
    pub fn retransmit_timeout(&self) -> Duration {
        self.rtt + self.rtt_var * 4
    }
}
```

### 5.6 UDP Flow Control

Byte-level data flow per peer using a sliding window:

```rust
pub struct UdpFlowWindow {
    pub piece_index: u32,
    pub total_blocks: u16,
    pub received: Bitfield,             // which blocks received
    pub pending_ack: Vec<u32>,          // sent seqs awaiting ACK
    pub window_start: u16,              // current window start (block index)
    pub window_size: u16,               // configured window (default 32 blocks)
}
```

---

## 6. Example Hex Dumps

### 6.1 Complete TCP Handshake

```
Client → Server (68 bytes):
13 51 76 6F 64 20 50 32 53 50 20 50 72 6F 74 6F  .Qvod P2SP Proto
63 6F 6C 00 00 00 00 00 00 00 00                  col........
A1 B2 C3 D4 E5 F6 07 08 09 0A 0B 0C 0D 0E 0F 10  ................
11 12 13 14                                        ....
2D 51 56 4F 44 2D 30 30 30 31 41 42 43 44 45 46  -QVOD-0001ABCDEF
31 32 33 34                                        1234

Server → Client (68 bytes):
13 51 76 6F 64 20 50 32 53 50 20 50 72 6F 74 6F  .Qvod P2SP Proto
63 6F 6C 00 00 00 00 00 00 00 00                  col........
A1 B2 C3 D4 E5 F6 07 08 09 0A 0B 0C 0D 0E 0F 10  ................
11 12 13 14                                        ....
2D 51 56 4F 44 2D 30 30 30 31 58 59 5A 5A 59 58  -QVOD-0001XYZZYX
57 56 55                                           WVU
```

### 6.2 Bitfield Message (3 pieces, all available)

```
Length prefix:  00 00 00 02    (2 bytes payload)
Message ID:     05
Bitfield data:  E0            (0b11100000)
```

### 6.3 Request Message

```
Length prefix:  00 00 00 0D    (13 bytes payload)
Message ID:     06
Index:          00 00 00 0A    (piece 10)
Begin:          00 00 00 00    (offset 0)
Length:         00 00 40 00    (16384 bytes = 16KB)
```

### 6.4 Piece Message (first block of piece 10)

```
Length prefix:  00 00 40 0D    (16397 bytes payload)
Message ID:     07
Index:          00 00 00 0A    (piece 10)
Begin:          00 00 00 00    (offset 0)
Data:           4D 61 73 74 65 72 ...   (16384 bytes of media data)
```

### 6.5 DHT PING Hex Dump

```
Request (28 bytes):
51 56 44 54 00 00 01 01  QVDT....
A1 B2 C3 D4 E5 F6 07 08  ........
09 0A 0B 0C 0D 0E 0F 10  ........
11 12 13 14                ....

Response (28 bytes):
51 56 44 54 00 00 01 01  QVDT....
2A 2B 2C 2D 2E 2F 30 31  *+,-./01
32 33 34 35 36 37 38 39  23456789
40 41 42 43                @ABC
```

### 6.6 UDP DATA Packet

```
Header (16 bytes):
51 56 44 54 01             QVDT.
00 00 00 1E                seq 30
00 00 00 0A                piece 10
00 01                      block offset 1
01                         flags (MORE)
Payload (1384 bytes of P-frame data):
...
```

---

## 7. Error Handling

### 7.1 Protocol Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid handshake: wrong protocol string or length")]
    InvalidHandshake,

    #[error("invalid message id: {0}")]
    InvalidMessageId(u8),

    #[error("message too short: expected {expected}, got {actual}")]
    MessageTooShort { expected: usize, actual: usize },

    #[error("message too long: {0} bytes exceeds maximum")]
    MessageTooLong(usize),

    #[error("checksum mismatch")]
    ChecksumMismatch,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

### 7.2 Peer Disconnection Reasons

| Reason | Code | Description |
|--------|------|-------------|
| Graceful shutdown | `0x00` | Normal connection close |
| Protocol violation | `0x01` | Invalid message sequence |
| Timeout | `0x02` | No activity for 120s |
| Duplicate connection | `0x03` | Already connected to this peer |
| Resource exhaustion | `0x04` | Too many connections |
| Unsupported version | `0x05` | Incompatible protocol version |

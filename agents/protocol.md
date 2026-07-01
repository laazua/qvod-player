# Wire Protocol Specification

## Overview

QVOD's wire protocol is a modified BitTorrent protocol optimized for streaming media. It retains the basic message framing (length-prefixed messages over TCP) while adding extensions for UDP data transport, keyframe-aware piece scheduling, and streaming-specific control messages.

Three distinct protocol layers are defined:
1. **Peer Wire Protocol** (TCP) — reliable control and critical data
2. **Extension Protocol** (TCP, on top of Peer Wire) — metadata exchange
3. **UDP Data Protocol** (UDP) — high-speed non-critical data transfer
4. **DHT RPC Protocol** (UDP) — distributed peer discovery

---

## 1. Handshake Message Format

### Wire Format (68 bytes)

The handshake is the first message exchanged on a TCP connection. It establishes protocol identity, resource identity, and feature negotiation.

```
Byte 0:     pstrlen (1 byte)
            Value: 0x13 (19)

Bytes 1-19: pstr (19 bytes)
            ASCII: "Qvod P2SP Protocol"

Bytes 20-27: reserved (8 bytes)
             Bitfield of supported extensions:

             Byte 20 (reserved[0]): Future use
             Byte 21 (reserved[1]): Future use
             Byte 22 (reserved[2]): Future use
             Byte 23 (reserved[3]): Future use
             Byte 24 (reserved[4]): Future use
             Byte 25 (reserved[5]):
               Bit 0 (0x01): DHT protocol support
               Bit 1 (0x02): Peer Exchange (PEX) support
               Bit 2 (0x04): FAST extension support
               Bit 3 (0x08): NAT traversal support
               Bit 4 (0x10): UDP data channel support
               Bit 5 (0x20): ut_metadata extension support
               Bits 6-7: Reserved
             Byte 26 (reserved[6]): Future use
             Byte 27 (reserved[7]): Future use

Bytes 28-47: info_hash (20 bytes)
             SHA-1 hash of the resource being requested

Bytes 48-67: peer_id (20 bytes)
             Unique client identifier
             Format: "-QVxxxx-" + 12 random bytes
```

### Handshake Exchange Sequence

```
Client                                     Server
  │                                          │
  ├── TCP SYN ──────────────────────────────►│
  │◄── TCP SYN+ACK ──────────────────────────┤
  │◄── TCP ACK ──────────────────────────────┤
  │                                          │
  │  (Connection established)                │
  │                                          │
  ├── Handshake (68 bytes) ─────────────────►│
  │                                          │
  │  (Server validates:                      │
  │   - pstrlen == 19                        │
  │   - pstr == "Qvod P2SP Protocol"         │
  │   - info_hash matches requested resource)│
  │                                          │
  │◄── Handshake (68 bytes) ─────────────────┤
  │                                          │
  │  (Client validates peer's handshake)     │
  │                                          │
  │◄── Bitfield / HaveAll / HaveNone ────────┤
  │                                          │
  │  (Optional extension handshake)          │
  │◄── Extended message ─────────────────────┤
  ├── Extended message ─────────────────────►│
  │                                          │
  │  (Ready for data exchange)               │
  │                                          │
```

---

## 2. Peer Wire Protocol Message Format

### Frame Format

All messages after the handshake use a length-prefixed framing format:

```
Offset  Size  Field            Description
──────  ────  ─────────        ─────────────────────────────────────
0       4     length_prefix    Message length (big-endian u32)
                               Does NOT include these 4 bytes
                               Value 0 = keep-alive message
4       1     message_id       Message type identifier
5       N     payload          Message-specific payload (see below)
```

Maximum message size: 128 KB (131072 bytes). This prevents memory exhaustion from oversized messages.

### Message Types

| ID | Name | Payload | Direction |
|----|------|---------|-----------|
| 0x00 | choke | None | Both |
| 0x01 | unchoke | None | Both |
| 0x02 | interested | None | Both |
| 0x03 | not_interested | None | Both |
| 0x04 | have | `piece_index: u32` | Both |
| 0x05 | bitfield | `bitfield: variable` | Both |
| 0x06 | request | `index: u32, begin: u32, length: u32` | Both |
| 0x07 | piece | `index: u32, begin: u32, block: variable` | Both |
| 0x08 | cancel | `index: u32, begin: u32, length: u32` | Both |
| 0x09 | port | `dht_port: u16` | Both |
| 0x0A | suggest_piece | `piece_index: u32` | Both (QVOD extension) |
| 0x0B | reject_request | `index: u32, begin: u32, length: u32` | Both |
| 0x0C | have_all | None | Both |
| 0x0D | have_none | None | Both |
| 0x14 | extended | `ext_msg_id: u8, payload: variable` | Both |

### Message Detail

#### 0x00: choke (4 bytes total)
```
Length: 0x00000001
Payload: (none)
```
Peer is choking us (we cannot request data). After receiving choke, all pending requests are voided.

#### 0x01: unchoke (4 bytes total)
```
Length: 0x00000001
Payload: (none)
```
Peer has unchoked us (we may now send requests). Indicates the peer has available upload bandwidth.

#### 0x02: interested (4 bytes total)
```
Length: 0x00000001
Payload: (none)
```
We are interested in data this peer has. Sent when we discover the peer has pieces we need.

#### 0x03: not_interested (4 bytes total)
```
Length: 0x00000001
Payload: (none)
```
We are no longer interested. Sent when we have all pieces the peer offers.

#### 0x04: have (9 bytes total)
```
Length: 0x00000005
Payload:
  Offset 0: piece_index (4 bytes, big-endian u32)
```
Notification that the peer now has a specific piece.

#### 0x05: bitfield (variable)
```
Length: var
Payload:
  bitfield bytes (ceil(num_pieces / 8) bytes)
```
Complete bitfield of pieces the peer has. Only sent once immediately after handshake.

#### request (13 bytes total)
```
Length: 0x00000009
Payload:
  Offset 0: index  (4 bytes, big-endian u32) — piece index
  Offset 4: begin  (4 bytes, big-endian u32) — offset within piece
  Offset 8: length (4 bytes, big-endian u32) — block length (typically 16KB)
```
Request a block of data. Must only be sent while unchoked.

#### piece (variable)
```
Length: 9 + block_size
Payload:
  Offset 0: index  (4 bytes, big-endian u32)
  Offset 4: begin  (4 bytes, big-endian u32)
  Offset 8: block  (variable) — raw block data
```
Response to a request message. Contains the requested block.

#### cancel (13 bytes total)
```
Length: 0x00000009
Payload:
  Offset 0: index  (4 bytes, big-endian u32)
  Offset 4: begin  (4 bytes, big-endian u32)
  Offset 8: length (4 bytes, big-endian u32)
```
Cancel a previously sent request. Used when:
- Seek operation changes the target piece
- Piece was already obtained from another peer

#### port (7 bytes total)
```
Length: 0x00000003
Payload:
  Offset 0: dht_port (2 bytes, big-endian u16) — UDP port for DHT
```
Announce the DHT port. Sent after handshake if DHT is supported. Allows peer to participate in DHT network.

#### suggest_piece (9 bytes total) — QVOD Extension
```
Length: 0x00000005
Payload:
  Offset 0: piece_index (4 bytes, big-endian u32)
```
QVOD-specific extension. A peer suggests we prioritize downloading a specific piece. Typically used when:
- The piece is rare in the swarm
- The piece contains a keyframe
- The sending peer has high bandwidth for this piece

#### reject_request (13 bytes total)
```
Length: 0x00000009
Payload:
  Offset 0: index  (4 bytes, big-endian u32)
  Offset 4: begin  (4 bytes, big-endian u32)
  Offset 8: length (4 bytes, big-endian u32)
```
Peer rejects our request. Reasons include:
- Piece is no longer available
- Peer is overloaded
- Invalid request parameters

#### have_all (4 bytes total)
```
Length: 0x00000001
Payload: (none)
```
Sent instead of bitfield when peer has every piece. More efficient than sending a full bitfield.

#### have_none (4 bytes total)
```
Length: 0x00000001
Payload: (none)
```
Sent instead of bitfield when peer has zero pieces. More efficient than sending an empty bitfield.

### Message Encoding/Decoding

```rust
#[derive(Debug, Clone)]
pub struct PeerMessage {
    pub msg_id: MsgId,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgId {
    Choke = 0x00,
    Unchoke = 0x01,
    Interested = 0x02,
    NotInterested = 0x03,
    Have = 0x04,
    Bitfield = 0x05,
    Request = 0x06,
    Piece = 0x07,
    Cancel = 0x08,
    Port = 0x09,
    SuggestPiece = 0x0A,
    RejectRequest = 0x0B,
    HaveAll = 0x0C,
    HaveNone = 0x0D,
    Extended = 0x14,
    KeepAlive = 0xFF, // length=0, no msg_id
}

impl MsgId {
    pub fn from_u8(id: u8) -> Option<Self> {
        match id {
            0x00 => Some(MsgId::Choke),
            0x01 => Some(MsgId::Unchoke),
            0x02 => Some(MsgId::Interested),
            0x03 => Some(MsgId::NotInterested),
            0x04 => Some(MsgId::Have),
            0x05 => Some(MsgId::Bitfield),
            0x06 => Some(MsgId::Request),
            0x07 => Some(MsgId::Piece),
            0x08 => Some(MsgId::Cancel),
            0x09 => Some(MsgId::Port),
            0x0A => Some(MsgId::SuggestPiece),
            0x0B => Some(MsgId::RejectRequest),
            0x0C => Some(MsgId::HaveAll),
            0x0D => Some(MsgId::HaveNone),
            0x14 => Some(MsgId::Extended),
            _ => None,
        }
    }
}

impl PeerMessage {
    pub fn new(msg_id: MsgId, payload: Vec<u8>) -> Self {
        Self { msg_id, payload }
    }

    /// Encode message to wire format (length prefix + id + payload)
    pub fn encode(&self) -> Vec<u8> {
        // Keep-alive is special: length = 0, no msg_id
        if self.msg_id == MsgId::KeepAlive {
            return vec![0u8; 4]; // length_prefix = 0
        }

        let payload_len = self.payload.len() as u32;
        let total_len = 1 + payload_len; // msg_id + payload
        let mut buf = Vec::with_capacity(4 + total_len as usize);

        // Length prefix (big-endian)
        buf.extend_from_slice(&total_len.to_be_bytes());

        // Message ID
        buf.push(self.msg_id as u8);

        // Payload
        buf.extend_from_slice(&self.payload);

        buf
    }

    /// Decode message from wire format
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(ProtocolError::MessageTooShort);
        }

        let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

        // Keep-alive
        if length == 0 {
            return Ok(Self::new(MsgId::KeepAlive, vec![]));
        }

        if data.len() < 5 {
            return Err(ProtocolError::MessageTooShort);
        }

        let msg_id = MsgId::from_u8(data[4])
            .ok_or(ProtocolError::UnknownMessageId(data[4]))?;

        let payload = if length > 1 {
            let payload_start = 5;
            let payload_end = payload_start + (length as usize - 1);
            if data.len() < payload_end {
                return Err(ProtocolError::MessageTruncated);
            }
            data[payload_start..payload_end].to_vec()
        } else {
            vec![]
        };

        Ok(Self::new(msg_id, payload))
    }

    /// Convenience constructors
    pub fn request(piece_index: u32, begin: u32, length: u32) -> Self {
        let mut payload = Vec::with_capacity(12);
        payload.extend_from_slice(&piece_index.to_be_bytes());
        payload.extend_from_slice(&begin.to_be_bytes());
        payload.extend_from_slice(&length.to_be_bytes());
        Self::new(MsgId::Request, payload)
    }

    pub fn piece(piece_index: u32, begin: u32, data: Vec<u8>) -> Self {
        let mut payload = Vec::with_capacity(8 + data.len());
        payload.extend_from_slice(&piece_index.to_be_bytes());
        payload.extend_from_slice(&begin.to_be_bytes());
        payload.extend_from_slice(&data);
        Self::new(MsgId::Piece, payload)
    }

    pub fn have(piece_index: u32) -> Self {
        Self::new(MsgId::Have, piece_index.to_be_bytes().to_vec())
    }

    pub fn suggest_piece(piece_index: u32) -> Self {
        Self::new(MsgId::SuggestPiece, piece_index.to_be_bytes().to_vec())
    }

    pub fn port(dht_port: u16) -> Self {
        Self::new(MsgId::Port, dht_port.to_be_bytes().to_vec())
    }
}
```

---

## 3. Extension Protocol (ut_metadata)

### Overview

The extension protocol allows peers to exchange metadata (FileMeta including keyframe index) without requiring a separate .qvs file. It's built on top of the standard peer wire protocol using message ID 0x14 (extended).

### Extended Message Format

```
Length-prefix (4 bytes): extended message length
Message ID (1 byte):    0x14
Extension ID (1 byte):  Identifies the extension type
Payload (variable):     Extension-specific data

Extension IDs:
  0x00: Handshake (negotiate supported extensions)
  0x01: ut_metadata request
  0x02: ut_metadata data
  0x03: ut_metadata reject
```

### Extension Handshake (msg_id = 0x14, ext_id = 0x00)

Sent by both peers after the standard bitfield exchange. Uses Bencode encoding for the payload:

```python
d
  1:m d
    12:ut_metadata i1e       # We support ut_metadata with ID 1
    3:qvod i2e               # We support QVOD extensions with ID 2
  e
  1:v 7:QVOD 0.1             # Client version
  1:p 10:2A3B4C5D6E          # Local peer capabilities
e
```

**Payload structure (Bencode):**
- `m` (dict): maps extension names to extension IDs
- `v` (string): client version identifier
- `p` (string): general-purpose capabilities bitmask (hex)

### ut_metadata Request (ext_id = 0x01)

```python
d
  1:msg_type i0e             # 0 = request
  1:piece i0e                # metadata piece index (metadata is split into 256KB pieces)
e
```

### ut_metadata Data (ext_id = 0x02)

```python
d
  1:msg_type i1e             # 1 = data
  1:piece i0e                # piece index
  1:total_size i123456e      # total metadata size (for reassembly)
e
<payload_bytes>             # raw metadata bytes (Bencode-encoded FileMeta)
```

### ut_metadata Reject (ext_id = 0x03)

```python
d
  1:msg_type i2e             # 2 = reject
  1:piece i0e                # piece index rejected (or -1 for all)
e
```

### Metadata Structure (Bencode)

The FileMeta is encoded as a Bencode dictionary within the ut_metadata data payload:

```python
d
  8:info_hash 40:A1B2C3D4E5F6G7H8I9J0K1L2M3N4O5P6Q7R8S9
  6:length i734003200e
  12:piece length i262144e
  6:pieces 200:<20*N binary SHA-1 hashes>
  13:keyframe index li12345ei67890e...e
  7:trackers l15:http://tracker1:6969/announce19:http://tracker2:6969/announcee
  10:name 9:movie.mp4
  4:codec 3:AVC
  5:width i1920e
  6:height i1080e
  5:fps i24e
  7:bitrate i2500000e
  11:duration ms i7320000e
  4:hash 3:SHA1
e
```

### Rust Implementation

```rust
pub struct ExtendedHandshake {
    pub extension_ids: HashMap<String, u8>,
    pub version: Option<String>,
    pub capabilities: Option<String>,
}

impl ExtendedHandshake {
    pub fn encode(&self) -> Vec<u8> {
        let mut dict = BencodeDict::new();

        let mut m = BencodeDict::new();
        for (name, id) in &self.extension_ids {
            m.insert_int(name, *id as i64);
        }
        dict.insert_dict("m", m);

        if let Some(ref v) = self.version {
            dict.insert_str("v", v);
        }
        if let Some(ref p) = self.capabilities {
            dict.insert_str("p", p);
        }

        dict.encode()
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let value = Bencode::decode(data)?;
        let dict = value.as_dict().ok_or(ProtocolError::BencodeExpected)?
            .dict_get("m")
            .and_then(|m| m.as_dict())
            .ok_or(ProtocolError::MissingExtensionMap)?;

        let mut extension_ids = HashMap::new();
        for (key, val) in dict {
            if let Some(id) = val.as_int() {
                extension_ids.insert(key.clone(), id as u8);
            }
        }

        let version = value.dict_get("v").and_then(|v| v.as_str()).map(String::from);
        let capabilities = value.dict_get("p").and_then(|p| p.as_str()).map(String::from);

        Ok(Self { extension_ids, version, capabilities })
    }
}
```

---

## 4. DHT RPC Protocol (UDP)

### Overview

The DHT protocol runs over UDP. All messages are single datagrams, maximum 1400 bytes (to avoid IP fragmentation). The protocol uses a request-response model with transaction IDs matching requests to responses.

### Message Header (8 bytes)

```
Offset  Size  Field         Description
──────  ────  ─────────     ──────────────────────────────────────
0       4     magic         Magic bytes: 0x51 0x56 0x44 0x54 ("QVDT")
4       1     msg_type      Message type identifier
5       2     txn_id        Transaction ID (big-endian u16)
7       1     version       Protocol version (current: 0x01)
```

### Message Types

| Type | Name | Request Payload | Response Payload |
|------|------|----------------|------------------|
| 0x00 | PING | `node_id (20 bytes)` | `node_id (20 bytes)` |
| 0x01 | FIND_NODE | `node_id (20) + target (20)` | `node_id (20) + nodes (variable)` |
| 0x02 | FIND_PEERS | `node_id (20) + info_hash (20)` | `node_id (20) + values type + data` |
| 0x03 | ANNOUNCE | `node_id (20) + info_hash (20) + port (2) + token (4)` | `node_id (20) + "OK"` |

### Detailed Message Layouts

#### 0x00: PING
```
Request (28 bytes):
  header (8) + node_id (20)

Response (28 bytes):
  header (8) + node_id (20)
```
Used for connectivity checks and routing table maintenance.

#### 0x01: FIND_NODE
```
Request (48 bytes):
  header (8) + node_id (20) + target_node_id (20)

Response (variable):
  header (8) + node_id (20) + nodes (K * 26 bytes)

  Each node entry (26 bytes):
    node_id:    20 bytes
    ip:         4 bytes (IPv4, network byte order)
    port:       2 bytes (big-endian u16)
```
Returns the K closest nodes to the target ID from the responding node's routing table.

#### 0x02: FIND_PEERS
```
Request (48 bytes):
  header (8) + node_id (20) + info_hash (20)

Response (if peers found):
  header (8) + node_id (20) + 0x00 + peer_list (n * 6 bytes)
  Each peer: ip (4 bytes) + port (2 bytes)

Response (if no peers):
  header (8) + node_id (20) + 0x01 + nodes (K * 26 bytes)
  (same node format as FIND_NODE)
```
Looks up peers associated with an info_hash. If the responding node has stored peers, returns them. Otherwise returns closer nodes.

#### 0x03: ANNOUNCE_PEER
```
Request (54 bytes):
  header (8) + node_id (20) + info_hash (20) + port (2) + token (4)

Response (28 bytes):
  header (8) + node_id (20) + "OK"
```
Announce that the requesting node is available for peer connections for a given info_hash. Requires a valid token (obtained from FIND_PEERS) to prevent poisoning.

### Token Management

```rust
pub struct TokenManager {
    /// Current secret (rotated every 10 minutes)
    current_secret: [u8; 8],
    /// Previous secret (accepted for 5-minute overlap)
    previous_secret: [u8; 8],
    /// Last rotation time
    last_rotation: Instant,
}

impl TokenManager {
    pub fn new() -> Self;

    /// Generate a token for a given IP address
    pub fn generate_token(&self, ip: IpAddr) -> [u8; 4] {
        let mut hmac = Hmac::<Sha1>::new_from_slice(&self.current_secret)
            .expect("HMAC init");
        hmac.update(&ip.octets());
        let result = hmac.finalize().into_bytes();

        let mut token = [0u8; 4];
        token.copy_from_slice(&result[..4]);
        token
    }

    /// Verify a token from a given IP
    pub fn verify_token(&self, ip: IpAddr, token: &[u8; 4]) -> bool {
        // Check with current secret
        let expected = self.generate_token(ip);
        if *token == expected {
            return true;
        }

        // Check with previous secret (if within overlap window)
        if self.last_rotation.elapsed() < Duration::from_secs(300) {
            let mut hmac = Hmac::<Sha1>::new_from_slice(&self.previous_secret)
                .expect("HMAC init");
            hmac.update(&ip.octets());
            let result = hmac.finalize().into_bytes();

            let mut expected_prev = [0u8; 4];
            expected_prev.copy_from_slice(&result[..4]);
            return *token == expected_prev;
        }

        false
    }

    /// Rotate secrets (called every 10 minutes)
    pub fn rotate(&mut self) {
        self.previous_secret = self.current_secret;
        thread_rng().fill_bytes(&mut self.current_secret);
        self.last_rotation = Instant::now();
    }
}
```

### DHT Message Encoding/Decoding

```rust
#[derive(Debug, Clone)]
pub struct DhtHeader {
    pub magic: [u8; 4],
    pub msg_type: DhtMsgType,
    pub txn_id: u16,
    pub version: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DhtMsgType {
    Ping = 0x00,
    FindNode = 0x01,
    FindPeers = 0x02,
    Announce = 0x03,
}

impl DhtHeader {
    const MAGIC: [u8; 4] = [0x51, 0x56, 0x44, 0x54]; // "QVDT"
    const CURRENT_VERSION: u8 = 0x01;

    pub fn new(msg_type: DhtMsgType, txn_id: u16) -> Self {
        Self {
            magic: Self::MAGIC,
            msg_type,
            txn_id,
            version: Self::CURRENT_VERSION,
        }
    }

    pub fn encode(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[..4].copy_from_slice(&self.magic);
        buf[4] = self.msg_type as u8;
        buf[5..7].copy_from_slice(&self.txn_id.to_be_bytes());
        buf[7] = self.version;
        buf
    }

    pub fn decode(data: &[u8; 8]) -> Result<Self> {
        if &data[..4] != &Self::MAGIC {
            return Err(DhtError::InvalidMagic);
        }
        let msg_type = match data[4] {
            0x00 => DhtMsgType::Ping,
            0x01 => DhtMsgType::FindNode,
            0x02 => DhtMsgType::FindPeers,
            0x03 => DhtMsgType::Announce,
            _ => return Err(DhtError::UnknownType(data[4])),
        };
        let txn_id = u16::from_be_bytes([data[5], data[6]]);
        let version = data[7];
        if version > Self::CURRENT_VERSION {
            return Err(DhtError::UnsupportedVersion(version));
        }
        Ok(Self { magic: Self::MAGIC, msg_type, txn_id, version })
    }
}

#[derive(Debug, Clone)]
pub enum DhtMessage {
    Ping {
        header: DhtHeader,
        node_id: [u8; 20],
    },
    PingResponse {
        header: DhtHeader,
        node_id: [u8; 20],
    },
    FindNode {
        header: DhtHeader,
        node_id: [u8; 20],
        target: [u8; 20],
    },
    FindNodeResponse {
        header: DhtHeader,
        node_id: [u8; 20],
        nodes: Vec<NodeInfo>,
    },
    FindPeers {
        header: DhtHeader,
        node_id: [u8; 20],
        info_hash: [u8; 20],
    },
    FindPeersResponse {
        header: DhtHeader,
        node_id: [u8; 20],
        values: PeerValues,
    },
    Announce {
        header: DhtHeader,
        node_id: [u8; 20],
        info_hash: [u8; 20],
        port: u16,
        token: [u8; 4],
    },
    AnnounceResponse {
        header: DhtHeader,
        node_id: [u8; 20],
    },
}

#[derive(Debug, Clone)]
pub enum PeerValues {
    Peers(Vec<SocketAddr>),
    Nodes(Vec<NodeInfo>),
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: [u8; 20],
    pub addr: SocketAddr,
}

impl DhtMessage {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            DhtMessage::Ping { header, node_id } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                buf
            }
            DhtMessage::PingResponse { header, node_id } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                buf
            }
            DhtMessage::FindNode { header, node_id, target } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                buf.extend_from_slice(target);
                buf
            }
            DhtMessage::FindNodeResponse { header, node_id, nodes } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                for node in nodes {
                    buf.extend_from_slice(&node.node_id);
                    match node.addr {
                        SocketAddr::V4(v4) => {
                            buf.extend_from_slice(&v4.ip().octets());
                            buf.extend_from_slice(&v4.port().to_be_bytes());
                        }
                        SocketAddr::V6(_) => {
                            // IPv6 not supported in DHT (use IPv4-mapped)
                            buf.extend_from_slice(&[0u8; 6]);
                        }
                    }
                }
                buf
            }
            DhtMessage::FindPeers { header, node_id, info_hash } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                buf.extend_from_slice(info_hash);
                buf
            }
            DhtMessage::FindPeersResponse { header, node_id, values } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                match values {
                    PeerValues::Peers(peers) => {
                        buf.push(0x00); // type: peers
                        for peer in peers {
                            match peer {
                                SocketAddr::V4(v4) => {
                                    buf.extend_from_slice(&v4.ip().octets());
                                    buf.extend_from_slice(&v4.port().to_be_bytes());
                                }
                                SocketAddr::V6(_) => {
                                    buf.extend_from_slice(&[0u8; 6]);
                                }
                            }
                        }
                    }
                    PeerValues::Nodes(nodes) => {
                        buf.push(0x01); // type: nodes
                        for node in nodes {
                            buf.extend_from_slice(&node.node_id);
                            match node.addr {
                                SocketAddr::V4(v4) => {
                                    buf.extend_from_slice(&v4.ip().octets());
                                    buf.extend_from_slice(&v4.port().to_be_bytes());
                                }
                                SocketAddr::V6(_) => {
                                    buf.extend_from_slice(&[0u8; 6]);
                                }
                            }
                        }
                    }
                }
                buf
            }
            DhtMessage::Announce { header, node_id, info_hash, port, token } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                buf.extend_from_slice(info_hash);
                buf.extend_from_slice(&port.to_be_bytes());
                buf.extend_from_slice(token);
                buf
            }
            DhtMessage::AnnounceResponse { header, node_id } => {
                let mut buf = header.encode().to_vec();
                buf.extend_from_slice(node_id);
                buf.extend_from_slice(b"OK");
                buf
            }
        }
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(DhtError::MessageTooShort);
        }

        let mut header_bytes = [0u8; 8];
        header_bytes.copy_from_slice(&data[..8]);
        let header = DhtHeader::decode(&header_bytes)?;

        match header.msg_type {
            DhtMsgType::Ping => {
                if data.len() < 28 { return Err(DhtError::MessageTooShort); }
                let mut node_id = [0u8; 20];
                node_id.copy_from_slice(&data[8..28]);
                Ok(DhtMessage::Ping { header, node_id })
            }
            DhtMsgType::FindNode => {
                if data.len() < 48 { return Err(DhtError::MessageTooShort); }
                let mut node_id = [0u8; 20];
                node_id.copy_from_slice(&data[8..28]);
                let mut target = [0u8; 20];
                target.copy_from_slice(&data[28..48]);
                Ok(DhtMessage::FindNode { header, node_id, target })
            }
            // ... etc for other message types
            _ => Err(DhtError::NotImplemented),
        }
    }
}
```

---

## 5. UDP Data Transfer Protocol

### Overview

QVOD uses UDP as a secondary data channel for non-critical pieces (P-frames, B-frames). TCP remains the primary channel for critical data (I-frames, control messages, metadata). The UDP protocol includes sequence numbering, ACK/NACK for reliability, and a custom congestion control algorithm.

### UDP Packet Format

```
Offset  Size  Field           Description
──────  ────  ─────────       ──────────────────────────────────────
0       2     magic           Magic: 0x5155 ("QU" = QVOD UDP)
2       1     msg_type        Message type
3       4     seq             Sequence number (big-endian u32)
7       4     ack             Cumulative ACK (big-endian u32)
11      4     ack_bits        Selective ACK bitmask (big-endian u32)
15      4     piece_index     Piece index (big-endian u32)
19      4     block_offset    Block offset within piece (big-endian u32)
23      2     payload_len     Payload length (big-endian u16)
25      N     payload         Payload data (max 1375 bytes)
```

Max total packet size: 1400 bytes (safe MTU to avoid fragmentation).

### Message Types

| Type | Name | Payload | Description |
|------|------|---------|-------------|
| 0x01 | DATA | `block: variable` | Data block transport |
| 0x02 | ACK | none | Acknowledgement |
| 0x03 | NACK | `seqs: variable` | Negative acknowledgement (list of lost seqs) |
| 0x04 | PING | `timestamp: u64` | Connectivity check |
| 0x05 | PONG | `timestamp: u64` | Ping response |

### DATA Packet (0x01)

```
Total: 25 + payload_len bytes
Magic:    2 bytes = 0x5155
Type:     1 byte  = 0x01
Seq:      4 bytes = sequence number (monotonically increasing per sender)
Ack:      4 bytes = last received seq from peer
AckBits:  4 bytes = bitmask of received seqs after ack (bit 0 = seq+1, bit 1 = seq+2, ...)
PI:       4 bytes = piece index
BO:       4 bytes = block offset within piece
PL:       2 bytes = payload length
Payload:  N bytes = block data (max 1375 bytes)
```

### ACK Packet (0x02)

```
Total: 25 bytes (no payload)
Magic:    2 bytes
Type:     1 byte  = 0x02
Seq:      4 bytes = sender's current sequence number (not used for data here)
Ack:      4 bytes = highest contiguous seq received
AckBits:  4 bytes = selective ACK bitmask
PI:       4 bytes = 0 (not applicable)
BO:       4 bytes = 0 (not applicable)
PL:       2 bytes = 0
Payload:  (empty)
```

### NACK Packet (0x03)

```
Total: 25 + lost_seqs bytes
Magic:    2 bytes
Type:     1 byte  = 0x03
Seq:      4 bytes = sender's seq
Ack:      4 bytes = last received seq
AckBits:  4 bytes = selective ACK bitmask
PI:       4 bytes = 0
BO:       4 bytes = 0
PL:       2 bytes = number of lost sequences
Payload:  lost_seqs (N * 4 bytes = sequence numbers of lost packets)
```

### Flow Control

```rust
pub struct UdpCongestionControl {
    /// Congestion window (packets)
    cwnd: u32,
    /// Slow start threshold
    ssthresh: u32,
    /// Current state
    state: CongestionState,
    /// Estimated RTT
    rtt_estimate: f64,
    /// Smoothed RTT (SRTT)
    srtt: f64,
    /// RTT variance
    rttvar: f64,
    /// Loss rate over sliding window
    loss_rate: f64,
    /// In-flight packets count
    in_flight: u32,
    /// Sequence number of next packet to send
    next_seq: u32,
    /// Last sequence acknowledged
    last_ack: u32,
    /// Sent packets awaiting ACK
    sent_packets: HashMap<u32, SentPacket>,
    /// Whether to use streaming-optimized mode
    streaming_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionState {
    SlowStart,
    CongestionAvoidance,
    FastRecovery,
}

#[derive(Debug, Clone)]
pub struct SentPacket {
    pub seq: u32,
    pub sent_at: Instant,
    pub size: u16,
    pub retransmitted: bool,
}

impl UdpCongestionControl {
    pub fn new(streaming_mode: bool) -> Self {
        Self {
            cwnd: 10, // initial window (lower than TCP for loss avoidance)
            ssthresh: u32::MAX,
            state: CongestionState::SlowStart,
            rtt_estimate: 100.0,
            srtt: 100.0,
            rttvar: 50.0,
            loss_rate: 0.0,
            in_flight: 0,
            next_seq: 1,
            last_ack: 0,
            sent_packets: HashMap::new(),
            streaming_mode,
        }
    }

    /// Whether we can send a new packet
    pub fn can_send(&self) -> bool {
        self.in_flight < self.cwnd
    }

    /// Get the next sequence number
    pub fn next_seq_num(&mut self) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    /// Register a sent packet
    pub fn on_packet_sent(&mut self, seq: u32, size: u16) {
        self.sent_packets.insert(seq, SentPacket {
            seq,
            sent_at: Instant::now(),
            size,
            retransmitted: false,
        });
        self.in_flight += 1;
    }

    /// Handle ACK
    pub fn on_ack(&mut self, ack: u32, ack_bits: u32, rtt_sample: Duration) {
        // Update RTT estimate (using Karn's algorithm: ignore retransmitted)
        if let Some(packet) = self.sent_packets.get(&ack) {
            if !packet.retransmitted {
                let rtt = rtt_sample.as_secs_f64() * 1000.0; // ms
                // Van Jacobson's RTT estimation
                self.rttvar = 0.75 * self.rttvar + 0.25 * (self.srtt - rtt).abs();
                self.srtt = 0.875 * self.srtt + 0.125 * rtt;
                self.rtt_estimate = self.srtt;
            }
        }

        // Remove acknowledged packets from in-flight
        let removed: Vec<u32> = self.sent_packets
            .keys()
            .filter(|&&seq| seq <= ack || (seq > ack && (ack_bits & (1 << (seq - ack - 1)) != 0)))
            .copied()
            .collect();

        for seq in &removed {
            if let Some(packet) = self.sent_packets.remove(seq) {
                self.in_flight = self.in_flight.saturating_sub(1);
            }
        }

        self.last_ack = ack;

        // Congestion state machine
        match self.state {
            CongestionState::SlowStart => {
                self.cwnd += 1;
                if self.cwnd >= self.ssthresh {
                    self.state = CongestionState::CongestionAvoidance;
                    tracing::debug!("UDP CC: entering congestion avoidance, cwnd={}", self.cwnd);
                }
            }
            CongestionState::CongestionAvoidance => {
                // Additive increase: cwnd += 1/cwnd per ACK
                self.cwnd = (self.cwnd as f64 + 1.0 / self.cwnd as f64) as u32;
            }
            CongestionState::FastRecovery => {
                self.cwnd += 1;
                // Exit fast recovery when all lost packets are retransmitted
                if self.sent_packets.is_empty() || removed.len() >= 3 {
                    self.state = CongestionState::CongestionAvoidance;
                    self.ssthresh = self.cwnd / 2;
                    tracing::debug!("UDP CC: exiting fast recovery, cwnd={}, ssthresh={}", self.cwnd, self.ssthresh);
                }
            }
        }

        // Streaming optimization: cap cwnd in streaming mode
        if self.streaming_mode {
            self.cwnd = self.cwnd.min(50);
        }
    }

    /// Handle loss (timeout or 3 duplicate ACKs)
    pub fn on_loss(&mut self) {
        if self.streaming_mode {
            // Streaming mode: more aggressive loss recovery
            self.ssthresh = (self.cwnd / 2).max(4);
            self.cwnd = 4; // reset to minimum for quick restart
        } else {
            // Standard mode: halve window
            self.ssthresh = (self.cwnd / 2).max(2);
            self.cwnd = self.ssthresh;
        }
        self.state = CongestionState::SlowStart;
        tracing::debug!("UDP CC: loss event, cwnd={}, ssthresh={}", self.cwnd, self.ssthresh);
    }

    /// Handle triple duplicate ACK (fast retransmit)
    pub fn on_dup_ack(&mut self, dup_ack_count: u32) {
        if dup_ack_count >= 3 && self.state != CongestionState::FastRecovery {
            self.ssthresh = (self.cwnd / 2).max(2);
            self.cwnd = self.ssthresh + 3;
            self.state = CongestionState::FastRecovery;
            tracing::debug!("UDP CC: fast retransmit, cwnd={}, ssthresh={}", self.cwnd, self.ssthresh);
        }
    }

    /// Check for retransmission timeout
    pub fn check_retransmit(&mut self) -> Vec<u32> {
        let timeout = Duration::from_millis(
            (self.rtt_estimate * 2.0).max(200.0) as u64
        );

        let to_retransmit: Vec<u32> = self.sent_packets
            .iter()
            .filter(|(_, pkt)| !pkt.retransmitted && pkt.sent_at.elapsed() > timeout)
            .map(|(seq, _)| *seq)
            .collect();

        for seq in &to_retransmit {
            if let Some(pkt) = self.sent_packets.get_mut(seq) {
                pkt.retransmitted = true;
                pkt.sent_at = Instant::now();
            }
        }

        if !to_retransmit.is_empty() {
            self.on_loss();
        }

        to_retransmit
    }

    /// Calculate wait time for rate shaping
    pub fn wait_time(&self) -> Duration {
        if self.cwnd == 0 {
            return Duration::from_millis(100);
        }
        // Spread out sends based on RTT and cwnd
        let interval = self.rtt_estimate / self.cwnd as f64;
        Duration::from_millis(interval.max(1.0) as u64)
    }

    pub fn stats(&self) -> UdpCcStats {
        UdpCcStats {
            cwnd: self.cwnd,
            ssthresh: self.ssthresh,
            state: self.state,
            rtt_estimate_ms: self.rtt_estimate,
            loss_rate: self.loss_rate,
            in_flight: self.in_flight,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UdpCcStats {
    pub cwnd: u32,
    pub ssthresh: u32,
    pub state: CongestionState,
    pub rtt_estimate_ms: f64,
    pub loss_rate: f64,
    pub in_flight: u32,
}
```

---

## 6. Bencode Encoding/Decoding

### Bencode Types

Bencode supports four data types:

| Type | Encoding | Example |
|------|----------|---------|
| Integer | `i<number>e` | `i42e` → 42 |
| String | `<length>:<data>` | `4:spam` → "spam" |
| List | `l<items>e` | `l4:spami42ee` → ["spam", 42] |
| Dictionary | `d<key-value-pairs>e` | `d3:bar4:spame` → {"bar": "spam"} |

### Rust Implementation

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum BencodeValue {
    Int(i64),
    Str(Vec<u8>),
    List(Vec<BencodeValue>),
    Dict(BTreeMap<String, BencodeValue>),
}

impl BencodeValue {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            BencodeValue::Int(i) => {
                format!("i{}e", i).into_bytes()
            }
            BencodeValue::Str(s) => {
                let mut buf = format!("{}:", s.len()).into_bytes();
                buf.extend_from_slice(s);
                buf
            }
            BencodeValue::List(items) => {
                let mut buf = vec![b'l'];
                for item in items {
                    buf.extend_from_slice(&item.encode());
                }
                buf.push(b'e');
                buf
            }
            BencodeValue::Dict(dict) => {
                let mut buf = vec![b'd'];
                // Keys must be sorted lexicographically per Bencode spec
                for (key, val) in dict {
                    let key_bytes = key.as_bytes();
                    let mut key_enc = format!("{}:", key_bytes.len()).into_bytes();
                    key_enc.extend_from_slice(key_bytes);
                    buf.extend_from_slice(&key_enc);
                    buf.extend_from_slice(&val.encode());
                }
                buf.push(b'e');
                buf
            }
        }
    }

    pub fn decode(data: &[u8]) -> Result<(Self, &[u8])> {
        if data.is_empty() {
            return Err(BencodeError::UnexpectedEof);
        }

        match data[0] {
            b'i' => {
                let end = data.iter().position(|&b| b == b'e')
                    .ok_or(BencodeError::UnterminatedInteger)?;
                let num_str = std::str::from_utf8(&data[1..end])
                    .map_err(|_| BencodeError::InvalidInteger)?;
                let num: i64 = num_str.parse()
                    .map_err(|_| BencodeError::InvalidInteger)?;
                Ok((BencodeValue::Int(num), &data[end + 1..]))
            }
            b'0'..=b'9' => {
                let colon = data.iter().position(|&b| b == b':')
                    .ok_or(BencodeError::MissingColon)?;
                let len_str = std::str::from_utf8(&data[..colon])
                    .map_err(|_| BencodeError::InvalidStringLength)?;
                let len: usize = len_str.parse()
                    .map_err(|_| BencodeError::InvalidStringLength)?;
                let start = colon + 1;
                let end = start + len;
                if end > data.len() {
                    return Err(BencodeError::StringTooShort);
                }
                Ok((BencodeValue::Str(data[start..end].to_vec()), &data[end..]))
            }
            b'l' => {
                let mut items = Vec::new();
                let mut rest = &data[1..];
                while !rest.is_empty() && rest[0] != b'e' {
                    let (item, remaining) = BencodeValue::decode(rest)?;
                    items.push(item);
                    rest = remaining;
                }
                if rest.is_empty() {
                    return Err(BencodeError::UnterminatedList);
                }
                Ok((BencodeValue::List(items), &rest[1..]))
            }
            b'd' => {
                let mut dict = BTreeMap::new();
                let mut rest = &data[1..];
                while !rest.is_empty() && rest[0] != b'e' {
                    // Key (must be a byte string)
                    let (key, remaining) = BencodeValue::decode(rest)?;
                    let key_bytes = match &key {
                        BencodeValue::Str(s) => s.clone(),
                        _ => return Err(BencodeError::DictKeyNotString),
                    };
                    // Value
                    let (val, remaining) = BencodeValue::decode(remaining)?;
                    dict.insert(
                        String::from_utf8(key_bytes)
                            .map_err(|_| BencodeError::DictKeyNotUtf8)?,
                        val,
                    );
                    rest = remaining;
                }
                if rest.is_empty() {
                    return Err(BencodeError::UnterminatedDict);
                }
                Ok((BencodeValue::Dict(dict), &rest[1..]))
            }
            _ => Err(BencodeError::UnexpectedByte(data[0])),
        }
    }

    // Convenience accessors
    pub fn as_int(&self) -> Option<i64> {
        match self { BencodeValue::Int(i) => Some(*i), _ => None }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            BencodeValue::Str(s) => std::str::from_utf8(s).ok(),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self { BencodeValue::Str(s) => Some(s), _ => None }
    }

    pub fn as_list(&self) -> Option<&Vec<BencodeValue>> {
        match self { BencodeValue::List(l) => Some(l), _ => None }
    }

    pub fn as_dict(&self) -> Option<&BTreeMap<String, BencodeValue>> {
        match self { BencodeValue::Dict(d) => Some(d), _ => None }
    }

    pub fn dict_get(&self, key: &str) -> Option<&BencodeValue> {
        self.as_dict()?.get(key)
    }

    pub fn dict_get_as_int(&self, key: &str) -> Option<i64> {
        self.dict_get(key)?.as_int()
    }

    pub fn dict_get_as_str(&self, key: &str) -> Option<&str> {
        self.dict_get(key)?.as_str()
    }

    pub fn dict_get_as_bytes(&self, key: &str) -> Option<&[u8]> {
        self.dict_get(key)?.as_bytes()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BencodeError {
    #[error("Unexpected end of data")]
    UnexpectedEof,
    #[error("Unterminated integer")]
    UnterminatedInteger,
    #[error("Invalid integer value")]
    InvalidInteger,
    #[error("Missing colon in string")]
    MissingColon,
    #[error("Invalid string length")]
    InvalidStringLength,
    #[error("String data too short")]
    StringTooShort,
    #[error("Unterminated list")]
    UnterminatedList,
    #[error("Unterminated dictionary")]
    UnterminatedDict,
    #[error("Dictionary key must be a string")]
    DictKeyNotString,
    #[error("Dictionary key is not valid UTF-8")]
    DictKeyNotUtf8,
    #[error("Unexpected byte: 0x{0:02x}")]
    UnexpectedByte(u8),
}
```

### Bencode Encoding Conventions (QVOD-specific)

1. **Dictionary keys** must be sorted lexicographically (byte order, not Unicode collation).
2. **Integer values** must not be padded with leading zeros. `i5e` is valid, `i05e` is not.
3. **Negative integers** are allowed: `i-3e`.
4. **Strings** are raw bytes; UTF-8 encoding is not enforced by the protocol but human-readable fields use UTF-8.
5. **Top-level value** for tracker responses and .qvs files is always a dictionary.
6. **Maximum nesting depth**: 10 levels to prevent stack overflow in decoders.

---

## 7. Protocol Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Handshake failed: {0}")]
    Handshake(String),

    #[error("Message too short ({0} bytes)")]
    MessageTooShort(usize),

    #[error("Message truncated")]
    MessageTruncated,

    #[error("Unknown message ID: 0x{0:02x}")]
    UnknownMessageId(u8),

    #[error("Invalid message payload: {0}")]
    InvalidPayload(String),

    #[error("Bencode error: {0}")]
    Bencode(#[from] BencodeError),

    #[error("DHT protocol error: {0}")]
    Dht(String),

    #[error("Extension protocol error: {0}")]
    Extension(String),

    #[error("UDP transport error: {0}")]
    Udp(String),
}
```

---

## Summary

QVOD's wire protocol implements a streaming-optimized variant of the BitTorrent protocol with four distinct layers:

| Layer | Transport | Purpose | Reliability |
|-------|-----------|---------|------------|
| Peer Wire | TCP | Control messages, critical data, metadata | Reliable (TCP) |
| Extension | TCP (tunneled) | Metadata exchange, future extensions | Reliable (TCP) |
| UDP Data | UDP | Non-critical piece data | Best-effort + ACK/NACK |
| DHT RPC | UDP | Distributed peer discovery | Best-effort |

Key protocol design decisions:
1. **Modified peer wire protocol** retains BitTorrent's proven message framing while adding streaming-specific messages (suggest_piece, reject_request).
2. **Hybrid TCP+UDP transport** is the core innovation: TCP guarantees delivery for critical pieces, UDP provides speed for non-critical data.
3. **Custom UDP congestion control** uses a TCP Reno-like algorithm with streaming optimizations (lower initial window, faster recovery, streaming mode cap).
4. **DHT protocol** uses the standard Kademlia model with compact message encoding (max 1400 bytes/packet).
5. **Extension protocol** follows BitTorrent's BEP-0010 for metadata exchange, allowing metadata retrieval without separate seed files.

# QVOD Storage Format Reference

## 1. Overview

QVOD uses three storage layers:

| Format | Extension | Purpose | Location |
|--------|-----------|---------|----------|
| `.qvs` | `.qvs` | Seed/metadata file (distributed) | User-shared |
| `.qdata` | `.qdata` | Sparse file with raw piece data | Cache directory |
| `.qmv` | `.qmv` | Bencode-serialized metadata | Cache directory |
| Config | `.toml` | Engine configuration | App data directory |

---

## 2. .qvs Seed File Format

The `.qvs` file is the QVOD equivalent of a `.torrent` file. It contains all metadata needed to locate and verify a stream.

### 2.1 Top-Level Structure

```bencode
d
8:info_hash 40:{info_hash_hex}
6:length i{file_size}e
12:piece length i{piece_length}e
6:pieces 20*N:{binary_sha1_hashes}
8:trackers l{...}e
13:creation date i{unixtime}e
7:comment {comment_string}
13:keyframe index {keyframe_index_bencode}
5:name {filename_string}
e
```

### 2.2 Field Reference

| Key | Type | Required | Description |
|-----|------|----------|-------------|
| `info_hash` | string (40 hex) | Yes | SHA-1 of info dict |
| `length` | integer | Yes | File size in bytes |
| `piece length` | integer | Yes | Piece size in bytes (default 262144) |
| `pieces` | string (binary) | Yes | Concatenated SHA-1 hashes (20 bytes each) |
| `name` | string | Yes | Filename |
| `trackers` | list of lists | No | Tracker URLs |
| `creation date` | integer | No | Unix timestamp |
| `comment` | string | No | Human-readable comment |
| `keyframe index` | dictionary | No (QVOD) | Keyframe position index |

### 2.3 Keyframe Index Encoding

```bencode
d
7:entries l
  d
    3:ts i{timestamp_ms}e
    4:off i{file_offset}e
    5:siz i{frame_size}e
    4:type i{0=I, 1=P, 2=B}e
  e
  ...
e
e
```

### 2.4 Complete .qvs Example

```bencode
d
8:info_hash 40:a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0
6:length i734003200e
12:piece length i262144e
6:pieces 5600:<280 × 20-byte SHA-1 hashes>
8:trackers l
  l
    42:http://tracker1.qvod.example.com:6969/announce
  e
  l
    42:http://tracker2.qvod.example.com:6969/announce
  e
e
13:creation date i1719878400e
7:comment 18:QVOD streaming seed
13:keyframe index d
  7:entries l
    d3:tsi0e4:offi0e5:sizi24576e4:typei0ee
    d3:tsi1000e4:offi262144e5:sizi18432e4:typei0ee
    d3:tsi2000e4:offi524288e5:sizi21504e4:typei0ee
    ...
  e
e
5:name 9:movie.rmvbe
```

### 2.5 Rust Data Structure

```rust
pub struct QvsFile {
    pub info_hash: InfoHash,
    pub filename: String,
    pub file_size: u64,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub keyframe_index: Option<KeyFrameIndex>,
    pub trackers: Vec<Vec<String>>,
    pub creation_date: Option<u64>,
    pub comment: Option<String>,
}

impl QvsFile {
    pub fn encode(&self) -> Result<Vec<u8>, BencodeError> {
        let mut dict = BTreeMap::new();

        // Required fields
        dict.insert("info_hash".into(), BencodeValue::Str(
            hex::encode(self.info_hash).into_bytes()
        ));
        dict.insert("length".into(), BencodeValue::Int(self.file_size as i64));
        dict.insert("piece length".into(), BencodeValue::Int(self.piece_length as i64));
        dict.insert("name".into(), BencodeValue::Str(self.filename.as_bytes().to_vec()));

        // Concatenated piece hashes
        let pieces_concat: Vec<u8> = self.pieces.iter().flat_map(|h| h.to_vec()).collect();
        dict.insert("pieces".into(), BencodeValue::Str(pieces_concat));

        // Optional: trackers
        if !self.trackers.is_empty() {
            let tracker_lists: Vec<BencodeValue> = self.trackers.iter().map(|tier| {
                let urls: Vec<BencodeValue> = tier.iter()
                    .map(|url| BencodeValue::Str(url.as_bytes().to_vec()))
                    .collect();
                BencodeValue::List(urls)
            }).collect();
            dict.insert("trackers".into(), BencodeValue::List(tracker_lists));
        }

        // Optional: keyframe index
        if let Some(kfi) = &self.keyframe_index {
            dict.insert("keyframe index".into(), kfi.encode_bencode());
        }

        // Optional metadata
        if let Some(date) = self.creation_date {
            dict.insert("creation date".into(), BencodeValue::Int(date as i64));
        }
        if let Some(comment) = &self.comment {
            dict.insert("comment".into(), BencodeValue::Str(comment.as_bytes().to_vec()));
        }

        BencodeValue::Dict(dict).encode()
    }

    pub fn decode(data: &[u8]) -> Result<Self, BencodeError> {
        let (value, _) = BencodeValue::decode(data)?;
        let dict = value.into_dict().ok_or(BencodeError::ExpectedDict)?;

        let info_hash_hex = dict.get_str("info_hash")?;
        let info_hash = hex_decode_to_array(info_hash_hex)?;

        let file_size = dict.get_int("length")? as u64;
        let piece_length = dict.get_int("piece length")? as u64;
        let filename = dict.get_str("name")?.to_string();

        // Parse piece hashes
        let pieces_raw = dict.get_bytes("pieces")?;
        let pieces: Vec<[u8; 20]> = pieces_raw
            .chunks_exact(20)
            .map(|chunk| {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(chunk);
                arr
            })
            .collect();

        // Parse optional fields
        let keyframe_index = dict.get("keyframe index")
            .and_then(|v| v.as_dict())
            .map(|d| KeyFrameIndex::decode_bencode(d))
            .transpose()?;

        let trackers = dict.get("trackers")
            .and_then(|v| v.as_list())
            .map(|list| {
                list.iter().filter_map(|v| {
                    v.as_list().map(|inner| {
                        inner.iter().filter_map(|u| u.as_str().map(String::from)).collect()
                    })
                }).collect()
            })
            .unwrap_or_default();

        let creation_date = dict.get("creation date").and_then(|v| v.as_int()).map(|i| i as u64);
        let comment = dict.get("comment").and_then(|v| v.as_str()).map(String::from);

        Ok(Self {
            info_hash,
            filename,
            file_size,
            piece_length,
            pieces,
            keyframe_index,
            trackers,
            creation_date,
            comment,
        })
    }
}
```

---

## 3. .qdata Sparse File Format

### 3.1 File Layout

The `.qdata` file stores raw piece data in a sparse (hole-punched) file:

```
File: {cache_dir}/qdata/{info_hash_hex}.qdata

Offset                           Content
──────────────────────────────────────────────────────
0                                ┌─────────────────┐
                                 │ Piece 0 data     │ = PIECE_LENGTH bytes
PIECE_LENGTH                     ├─────────────────┤
                                 │ (hole — empty)   │ = sparse, no disk allocation
2 * PIECE_LENGTH                 ├─────────────────┤
                                 │ Piece 2 data     │ = PIECE_LENGTH bytes
3 * PIECE_LENGTH                 ├─────────────────┤
                                 │ (hole — empty)   │
...                              │       ...        │
(N-1) * PIECE_LENGTH             ├─────────────────┤
                                 │ Piece N-1 data   │ = last piece (may be shorter)
N * PIECE_LENGTH                 └─────────────────┘
```

Holes in the sparse file consume zero disk space. Only written regions are allocated.

### 3.2 Creating a Sparse File

```rust
use std::fs::File;
use std::os::unix::fs::FileExtExt;

pub fn create_sparse_file(path: &Path, total_size: u64) -> Result<File, std::io::Error> {
    let file = File::create(path)?;
    // On Linux: set file size without allocating blocks
    file.set_len(total_size)?;
    Ok(file)
}
```

For platforms without sparse file API (e.g., Windows without `SetFileValidData`), QVOD falls back to explicit hole management via a companion allocation bitmap.

### 3.3 Reading and Writing

```rust
pub const PIECE_LENGTH: u64 = 262_144;  // 256 KB

pub struct QdataFile {
    file: File,
    info_hash: InfoHash,
    file_size: u64,
    piece_count: u32,
}

impl QdataFile {
    pub fn open(cache_dir: &Path, info_hash: &InfoHash, file_size: u64) -> Result<Self> {
        let path = cache_dir.join("qdata").join(format!("{}.qdata", hex::encode(info_hash)));
        let file = if path.exists() {
            OpenOptions::new().read(true).write(true).open(&path)?
        } else {
            create_sparse_file(&path, file_size)?
        };
        Ok(Self {
            file,
            info_hash: *info_hash,
            file_size,
            piece_count: ((file_size + PIECE_LENGTH - 1) / PIECE_LENGTH) as u32,
        })
    }

    pub fn read_piece(&self, index: u32) -> Result<Vec<u8>> {
        let offset = index as u64 * PIECE_LENGTH;
        let length = self.piece_length(index);
        let mut buf = vec![0u8; length as usize];
        self.file.read_exact_at(&mut buf, offset)?;
        Ok(buf)
    }

    pub fn write_piece(&mut self, index: u32, data: &[u8]) -> Result<()> {
        let offset = index as u64 * PIECE_LENGTH;
        let expected_len = self.piece_length(index);
        if data.len() as u64 != expected_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("piece {index} length mismatch: expected {expected_len}, got {}", data.len())
            ));
        }
        self.file.write_all_at(data, offset)?;
        Ok(())
    }

    pub fn read_range(&self, offset: u64, length: u64) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; length as usize];
        self.file.read_exact_at(&mut buf, offset)?;
        Ok(buf)
    }

    pub fn write_range(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.file.write_all_at(data, offset)?;
        Ok(())
    }

    pub fn piece_length(&self, index: u32) -> u64 {
        if index == self.piece_count - 1 {
            let remainder = self.file_size % PIECE_LENGTH;
            if remainder == 0 { PIECE_LENGTH } else { remainder }
        } else {
            PIECE_LENGTH
        }
    }

    pub fn completion(&self, bitfield: &Bitfield) -> f64 {
        bitfield.completion()
    }

    pub fn verify_piece(&self, index: u32, expected_hash: &[u8; 20]) -> Result<bool> {
        let data = self.read_piece(index)?;
        use sha1::{Sha1, Digest};
        let hash = Sha1::digest(&data);
        Ok(hash.as_slice() == expected_hash)
    }
}
```

### 3.4 Pre-Allocation Strategy

```rust
pub enum AllocationStrategy {
    /// No pre-allocation — files grow on first write per piece.
    /// Best for SSDs where fragmentation is less impactful.
    None,

    /// Fallocate with FALLOC_FL_KEEP_SIZE — reserves space but leaves holes.
    /// Btrfs/XFS/EXT4 only.
    FallocateKeepSize,

    /// Fallocate with FALLOC_FL_ZERO_RANGE — writes zeroes (slower but reliable).
    /// Guarantees contiguous allocation.
    FallocateZeroRange,

    /// Sparse file with explicit set_len — no allocation of holes.
    /// Default. Fastest for initial creation.
    Sparse,
}

impl QdataFile {
    pub fn preallocate(&self, strategy: AllocationStrategy) -> Result<()> {
        match strategy {
            AllocationStrategy::Sparse => {
                // Already sparse from create_sparse_file
                Ok(())
            }
            AllocationStrategy::FallocateKeepSize => {
                #[cfg(target_os = "linux")]
                {
                    use std::os::unix::fs::FileExt;
                    let fd = self.file.as_raw_fd();
                    let ret = unsafe {
                        libc::fallocate(fd, libc::FALLOC_FL_KEEP_SIZE, 0, self.file_size as libc::off_t)
                    };
                    if ret != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = strategy; // no-op on non-Linux
                }
                Ok(())
            }
            AllocationStrategy::FallocateZeroRange => {
                #[cfg(target_os = "linux")]
                {
                    use std::os::unix::fs::FileExt;
                    let fd = self.file.as_raw_fd();
                    let ret = unsafe {
                        libc::fallocate(fd, 0, 0, self.file_size as libc::off_t)
                    };
                    if ret != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = strategy;
                    return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "zero-range fallocate not supported"));
                }
                Ok(())
            }
            AllocationStrategy::None => Ok(()),
        }
    }
}
```

---

## 4. .qmv Metadata Serialization

The `.qmv` file stores the full `FileMeta` in Bencode format, cached locally after the first metadata exchange.

### 4.1 File Path

```
{cache_dir}/qmv/{info_hash_hex}.qmv
```

### 4.2 Serialization Format

```bencode
d
9:info_hash 40:{info_hash_hex}
8:filename {filename_string}
6:length i{file_size}e
12:piece length i{piece_length}e
6:pieces 20*N:{binary_hashes}
10:duration ms i{duration_ms}e
11:video codec {codec_string}
11:audio codec {codec_string}
5:width i{width}e
6:height i{height}e
7:bitrate i{bitrate}e
13:keyframe index d
  7:entries l{...}e
e
5:format {format_string}e
```

### 4.3 Rust Implementation

```rust
impl FileMeta {
    pub fn encode_bencode(&self) -> Vec<u8> {
        let mut dict = BTreeMap::new();

        dict.insert("info_hash".into(), BencodeValue::Str(
            hex::encode(self.info_hash).into_bytes()
        ));
        dict.insert("filename".into(), BencodeValue::Str(
            self.filename.as_bytes().to_vec()
        ));
        dict.insert("length".into(), BencodeValue::Int(self.file_size as i64));
        dict.insert("piece length".into(), BencodeValue::Int(self.piece_length as i64));

        // Concatenated SHA-1 hashes
        let pieces_concat: Vec<u8> = self.pieces.iter().flat_map(|h| h.to_vec()).collect();
        dict.insert("pieces".into(), BencodeValue::Str(pieces_concat));

        // Duration
        dict.insert("duration ms".into(), BencodeValue::Int(self.duration_ms as i64));

        // Codec info
        dict.insert("video codec".into(), BencodeValue::Str(
            self.video_codec.as_bytes().to_vec()
        ));
        dict.insert("audio codec".into(), BencodeValue::Str(
            self.audio_codec.as_bytes().to_vec()
        ));

        // Resolution
        dict.insert("width".into(), BencodeValue::Int(self.width as i64));
        dict.insert("height".into(), BencodeValue::Int(self.height as i64));

        // Bitrate
        dict.insert("bitrate".into(), BencodeValue::Int(self.bitrate as i64));

        // Keyframe index
        dict.insert("keyframe index".into(), BencodeValue::Dict({
            let mut kd = BTreeMap::new();
            let entries: Vec<BencodeValue> = self.keyframe_index.entries.iter().map(|entry| {
                BencodeValue::Dict({
                    let mut ed = BTreeMap::new();
                    ed.insert("ts".into(), BencodeValue::Int(entry.timestamp_ms as i64));
                    ed.insert("off".into(), BencodeValue::Int(entry.file_offset as i64));
                    ed.insert("siz".into(), BencodeValue::Int(entry.frame_size as i64));
                    ed.insert("type".into(), BencodeValue::Int(entry.frame_type as i64));
                    ed
                })
            }).collect();
            kd.insert("entries".into(), BencodeValue::List(entries));
            kd
        }));

        // Format hint
        if let Some(fmt) = &self.format {
            dict.insert("format".into(), BencodeValue::Str(fmt.as_bytes().to_vec()));
        }

        BencodeValue::Dict(dict).encode()
    }

    pub fn decode_bencode(data: &[u8]) -> Result<Self> {
        let (value, _) = BencodeValue::decode(data)?;
        let dict = value.into_dict().ok_or(StorageError::NotADict)?;

        // ... (similar to QvsFile::decode but with all fields)
    }
}
```

---

## 5. Cache Directory Tree

### 5.1 Full Directory Layout

```
{cache_dir}/
├── qdata/
│   ├── a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9.qdata     (raw piece data)
│   ├── f0e1d2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9.qdata
│   └── ...
├── qmv/
│   ├── a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9.qmv       (metadata)
│   ├── f0e1d2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9.qmv
│   └── ...
├── db/
│   ├── cache.db                                            (SQLite master index, optional)
│   └── downloads.db                                        (download history, optional)
└── config.toml                                             (engine config)
```

### 5.2 Cache Index Database Schema

Optional SQLite index for efficient lookup (see [`database/README.md`](../database/README.md) for full schema).

```sql
-- Cache master index
CREATE TABLE cache_entries (
    info_hash   BLOB PRIMARY KEY,  -- 20-byte SHA-1
    filename    TEXT NOT NULL,
    file_size   INTEGER NOT NULL,
    piece_count INTEGER NOT NULL,
    downloaded  INTEGER NOT NULL DEFAULT 0,
    last_access INTEGER NOT NULL,  -- unix timestamp
    created_at  INTEGER NOT NULL,
    bitfield    BLOB,              -- serialized bitfield
    metadata    BLOB               -- cached .qmv content (bencode)
);

CREATE INDEX idx_cache_last_access ON cache_entries(last_access);
CREATE INDEX idx_cache_downloaded ON cache_entries(downloaded);
```

---

## 6. Bitfield Serialization

### 6.1 In-Memory Representation

```rust
pub struct Bitfield {
    bytes: Vec<u8>,
    piece_count: u32,
}

impl Bitfield {
    pub fn new(piece_count: u32) -> Self {
        let len = ((piece_count + 7) / 8) as usize;
        Self {
            bytes: vec![0u8; len],
            piece_count,
        }
    }

    pub fn from_bytes(bytes: Vec<u8>, piece_count: u32) -> Self {
        Self { bytes, piece_count }
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn has(&self, index: u32) -> bool {
        let byte_idx = (index / 8) as usize;
        let bit_pos = 7 - (index % 8) as usize;
        if byte_idx >= self.bytes.len() {
            return false;
        }
        (self.bytes[byte_idx] & (1u8 << bit_pos)) != 0
    }

    pub fn set(&mut self, index: u32, value: bool) {
        let byte_idx = (index / 8) as usize;
        let bit_pos = 7 - (index % 8) as usize;
        if byte_idx >= self.bytes.len() {
            return;
        }
        if value {
            self.bytes[byte_idx] |= 1u8 << bit_pos;
        } else {
            self.bytes[byte_idx] &= !(1u8 << bit_pos);
        }
    }

    pub fn set_all(&mut self, value: bool) {
        let fill = if value { 0xFFu8 } else { 0x00u8 };
        for byte in &mut self.bytes {
            *byte = fill;
        }
        // Clear unused bits in last byte
        if value {
            let used_bits = self.piece_count % 8;
            if used_bits != 0 {
                let last_idx = self.bytes.len() - 1;
                self.bytes[last_idx] &= 0xFFu8 << (8 - used_bits);
            }
        }
    }

    pub fn count(&self) -> u32 {
        self.bytes.iter()
            .map(|&b| b.count_ones())
            .sum::<u32>()
            .min(self.piece_count)
    }

    pub fn completion(&self) -> f64 {
        if self.piece_count == 0 { return 0.0; }
        self.count() as f64 / self.piece_count as f64
    }
}
```

### 6.2 Serialized Wire Format

For network transfer (Message `0x05`)

```
[padding nibble]           [bitfield bytes]
┌─────────┬─────────┬─────┬─────────┬─────────┐
│ 0xE0    │ 0xF0    │ ... │ 0x80    │ 0x00    │
│ piece 0 │ piece 4 │     │ last    │ padding │
│ bits    │ bits    │     │ partial │         │
└─────────┴─────────┴─────┴─────────┴─────────┘

Piece 0-2 set: byte 0 = 0b11100000
```

---

## 7. Configuration File Format (TOML)

### 7.1 Schema

```toml
# QVOD Engine Configuration
# Location: {cache_dir}/config.toml

[network]
listen_port = 8621               # Local HTTP server port
udp_port = 8621                   # UDP listening port (can same as TCP for NAT)
max_connections = 50              # Maximum P2P connections
http_fallback = true              # Enable HTTP source fallback
http_sources = []                 # Custom HTTP source URLs

[tracker]
urls = [
    "http://tracker1.qvod.example.com:6969/announce",
    "http://tracker2.qvod.example.com:6969/announce",
    "udp://tracker3.qvod.example.com:6969",
]
timeout_secs = 30
retry_interval_secs = 2
max_retries = 3

[dht]
enabled = true
listen_port = 0                   # 0 = same as network.udp_port
seed_nodes = [
    "dht1.qvod.example.com:8621",
    "dht2.qvod.example.com:8621",
    "dht3.qvod.example.com:8621",
]
k = 8
alpha = 3
refresh_interval_secs = 900
peer_timeout_secs = 1800

[cache]
directory = "~/.cache/qvod"       # Cache directory
max_size_gb = 4                   # Max cache size in GB
allocation_strategy = "sparse"    # none | sparse | fallocate_keep_size | fallocate_zero_range
verify_on_read = true             # SHA-1 verify pieces on cache read
verify_on_write = true            # SHA-1 verify pieces on cache write

[buffer]
capacity_mb = 64                  # Ring buffer size
watermark_low_mb = 5              # Resume buffering below this
watermark_high_mb = 30            # Stop buffering above this

[scheduler]
piece_length = 262144             # 256 KB
block_length = 16384              # 16 KB
critical_window_secs = 0
high_window_secs = 30
normal_window_secs = 120
critical_pipeline = 10
high_pipeline = 5
normal_pipeline = 3
low_pipeline = 1
request_timeout_secs = 30
endgame_threshold = 20
endgame_redundancy = 3

[logging]
level = "info"                    # trace | debug | info | warn | error
file = ""                         # empty = stderr only
format = "json"                   # text | json
```

### 7.2 Rust Deserialization

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct EngineConfig {
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub tracker: TrackerConfig,
    #[serde(default)]
    pub dht: DhtConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub buffer: BufferConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl EngineConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(e))?;
        toml::from_str(&content)
            .map_err(|e| ConfigError::Parse(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Serialize(e.to_string()))?;
        std::fs::write(path, &content)
            .map_err(|e| ConfigError::Io(e))?;
        Ok(())
    }
}
```

---

## 8. Storage Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Bencode error: {0}")]
    Bencode(#[from] BencodeError),

    #[error("Piece verification failed at index {index}: expected {expected}, got {actual}")]
    PieceVerification {
        index: u32,
        expected: [u8; 20],
        actual: [u8; 20],
    },

    #[error("Cache entry not found: {0}")]
    CacheEntryNotFound(InfoHash),

    #[error("Cache full: need {need} bytes, max {max}")]
    CacheFull { need: u64, max: u64 },

    #[error("Invalid data length: expected {expected}, got {actual}")]
    InvalidLength { expected: u64, actual: u64 },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Unsupported platform for {0}")]
    UnsupportedPlatform(String),
}
```

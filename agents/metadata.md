# Metadata Module Specification

## Overview

The Metadata module is the **first data exchanged** in the QVOD playback pipeline. Before any video piece can be requested, the engine must obtain the `FileMeta` structure containing the file's piece hashes, keyframe index, codec information, and duration. This module handles:

- Parsing `FileMeta` from the `ut_metadata` BitTorrent extension protocol
- Deserializing the Bencode-encoded metadata dictionary
- Maintaining `KeyFrameIndex` for frame-accurate seeking
- Caching metadata to `.qmv` files for future reuse
- Serialization/deserialization round-trip guarantees

## Data Structures

### InfoHash

```rust
/// 20-byte SHA-1 hash used to identify a resource.
/// This is the primary key for all QVOD operations:
/// tracker lookups, DHT peer searches, cache indexing, and piece verification.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InfoHash(pub [u8; 20]);

impl InfoHash {
    /// Parse from a 40-character hex string (lowercase).
    pub fn from_hex(s: &str) -> Result<Self, MetadataError>;

    /// Format as a 40-character lowercase hex string.
    pub fn to_hex(&self) -> String;

    /// Parse from raw 20-byte binary.
    pub fn from_bytes(bytes: [u8; 20]) -> Self;

    /// XOR distance metric for DHT routing table operations.
    pub fn distance(&self, other: &InfoHash) -> [u8; 20];
}
```

### PieceHash

```rust
/// SHA-1 hash of a single piece (256 KB of file data).
/// Used to verify piece integrity after download.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PieceHash(pub [u8; 20]);

impl PieceHash {
    pub fn as_bytes(&self) -> &[u8; 20];
    pub fn from_bytes(bytes: [u8; 20]) -> Self;
}
```

### FrameType

```rust
/// Video frame type according to MPEG compression standard.
/// I-frames are fully self-contained (keyframes).
/// P-frames reference previous I/P frames.
/// B-frames reference both previous and future I/P frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameType {
    /// Intra-coded frame (keyframe). Fully self-contained.
    /// QVOD prioritises I-frames for seeking and instant playback.
    I,
    /// Predicted frame. References previous I or P frame.
    P,
    /// Bi-predictive frame. References surrounding I/P frames.
    B,
}

impl FrameType {
    /// Returns the relative decoding priority: I > P > B.
    pub fn priority(&self) -> u8 {
        match self {
            FrameType::I => 3,
            FrameType::P => 2,
            FrameType::B => 1,
        }
    }

    /// Human-readable description.
    pub fn description(&self) -> &'static str {
        match self {
            FrameType::I => "Keyframe (Intra-coded)",
            FrameType::P => "Predicted frame",
            FrameType::B => "Bi-predictive frame",
        }
    }
}
```

### KeyFrameEntry

```rust
/// A single entry in the keyframe index.
/// Maps a timestamp to a specific byte offset in the file,
/// enabling frame-accurate seeking without sequential scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyFrameEntry {
    /// Presentation timestamp in milliseconds from start of video.
    pub timestamp_ms: u64,

    /// Absolute byte offset in the video file where this frame begins.
    pub file_offset: u64,

    /// Size of this frame in bytes (compressed).
    pub frame_size: u32,

    /// Type of frame: I (keyframe), P (predicted), or B (bi-predictive).
    pub frame_type: FrameType,
}

impl KeyFrameEntry {
    /// The piece index that contains this frame's start.
    pub fn piece_index(&self, piece_length: u64) -> u32 {
        (self.file_offset / piece_length) as u32
    }

    /// The offset within the piece where this frame starts.
    pub fn piece_offset(&self, piece_length: u64) -> u32 {
        (self.file_offset % piece_length) as u32
    }

    /// The range of pieces this frame spans.
    pub fn piece_range(&self, piece_length: u64) -> std::ops::Range<u32> {
        let start = self.piece_index(piece_length);
        let end = ((self.file_offset + self.frame_size as u64 + piece_length - 1) / piece_length) as u32;
        start..end
    }
}
```

### KeyFrameIndex

```rust
/// Complete index of all keyframes and delta frames in the video.
/// This is the foundation for QVOD's non-sequential, sparse download strategy.
///
/// The index is sorted by `file_offset` ascending, which guarantees
/// monotonic timestamp progression for well-formed files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyFrameIndex {
    /// All frame entries, sorted by file_offset.
    pub entries: Vec<KeyFrameEntry>,

    /// Total number of I-frames (keyframes) in the index.
    pub keyframe_count: usize,

    /// Byte offset of the very first I-frame (used for instant start).
    pub first_iframe_offset: u64,
}

impl KeyFrameIndex {
    /// Create a new index from a sorted vector of entries.
    /// Returns an error if entries are not sorted by file_offset.
    pub fn new(mut entries: Vec<KeyFrameEntry>) -> Result<Self, MetadataError> {
        entries.sort_by_key(|e| e.file_offset);
        let keyframe_count = entries.iter().filter(|e| e.frame_type == FrameType::I).count();
        let first_iframe_offset = entries
            .iter()
            .find(|e| e.frame_type == FrameType::I)
            .map(|e| e.file_offset)
            .unwrap_or(0);
        Ok(Self { entries, keyframe_count, first_iframe_offset })
    }

    /// Find the nearest I-frame at or before the given timestamp.
    /// Used when seeking: we must land on an I-frame to begin decoding.
    pub fn nearest_iframe_before(&self, timestamp_ms: u64) -> Option<&KeyFrameEntry> {
        self.entries
            .iter()
            .filter(|e| e.frame_type == FrameType::I)
            .rev()
            .find(|e| e.timestamp_ms <= timestamp_ms)
            .or_else(|| self.entries.iter().find(|e| e.frame_type == FrameType::I))
    }

    /// Find the nearest I-frame at or after the given timestamp.
    pub fn nearest_iframe_after(&self, timestamp_ms: u64) -> Option<&KeyFrameEntry> {
        self.entries
            .iter()
            .filter(|e| e.frame_type == FrameType::I)
            .find(|e| e.timestamp_ms >= timestamp_ms)
    }

    /// Find the I-frame closest to the given timestamp (minimal absolute diff).
    pub fn nearest_iframe(&self, timestamp_ms: u64) -> Option<&KeyFrameEntry> {
        self.entries
            .iter()
            .filter(|e| e.frame_type == FrameType::I)
            .min_by_key(|e| (e.timestamp_ms as i64 - timestamp_ms as i64).abs())
    }

    /// Get all I-frame entries (keyframes only).
    pub fn keyframes(&self) -> impl Iterator<Item = &KeyFrameEntry> {
        self.entries.iter().filter(|e| e.frame_type == FrameType::I)
    }

    /// Get the index of the piece containing the first I-frame.
    pub fn first_keyframe_piece(&self, piece_length: u64) -> u32 {
        self.first_iframe_offset / piece_length as u64
    }

    /// Calculate the worst-case decoding delay if we start from
    /// a given offset. Returns milliseconds.
    pub fn decoding_delay(&self, from_offset: u64) -> u64 {
        self.entries
            .iter()
            .find(|e| e.file_offset >= from_offset)
            .map(|e| e.timestamp_ms)
            .unwrap_or(0)
    }

    /// Estimate the file offset for a given playback percentage (0.0–1.0).
    pub fn offset_at_progress(&self, progress: f64) -> u64 {
        if self.entries.is_empty() {
            return 0;
        }
        let total_duration = self.entries.last().unwrap().timestamp_ms;
        let target_ts = (total_duration as f64 * progress) as u64;
        self.nearest_iframe(target_ts)
            .map(|e| e.file_offset)
            .unwrap_or(0)
    }

    /// Total duration of the video in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.entries.last().map(|e| e.timestamp_ms).unwrap_or(0)
    }

    /// Number of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
```

### CodecInfo

```rust
/// Codec metadata extracted from the video file's header.
/// This information is parsed from the media container's codec tags
/// and stored alongside the FileMeta for display and decoder initialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecInfo {
    /// Video codec string (e.g., "avc1", "rv40", "vp9", "hevc").
    pub video_codec: String,

    /// Audio codec string (e.g., "mp4a", "aac", "cook", "mp3").
    pub audio_codec: String,

    /// Video width in pixels.
    pub width: u32,

    /// Video height in pixels.
    pub height: u32,

    /// Average video bitrate in bits per second. 0 if unknown.
    pub bitrate: u32,

    /// Audio sample rate in Hz (e.g., 44100, 48000).
    pub audio_sample_rate: u32,

    /// Number of audio channels (1 = mono, 2 = stereo, 6 = 5.1).
    pub audio_channels: u8,

    /// Video frame rate as a rational number (numerator/denominator).
    /// e.g., 30000/1001 for 29.97 fps.
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,

    /// Pixel aspect ratio (1/1 for square pixels).
    pub pixel_aspect_num: u32,
    pub pixel_aspect_den: u32,

    /// Whether the video contains B-frames (affects decoding order).
    pub has_b_frames: bool,
}

impl CodecInfo {
    /// Display resolution as "WxH".
    pub fn resolution_string(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    /// Display frame rate as a floating-point value.
    pub fn frame_rate_f64(&self) -> f64 {
        if self.frame_rate_den == 0 {
            0.0
        } else {
            self.frame_rate_num as f64 / self.frame_rate_den as f64
        }
    }

    /// Human-readable bitrate (e.g., "2.5 Mbps", "512 Kbps").
    pub fn bitrate_string(&self) -> String {
        if self.bitrate >= 1_000_000 {
            format!("{:.1} Mbps", self.bitrate as f64 / 1_000_000.0)
        } else if self.bitrate >= 1_000 {
            format!("{} Kbps", self.bitrate / 1_000)
        } else {
            format!("{} bps", self.bitrate)
        }
    }

    /// Check if the codec is supported by ffmpeg-next.
    pub fn is_supported(&self) -> bool {
        matches!(
            self.video_codec.as_str(),
            "avc1" | "h264" | "rv40" | "rv30" | "vp9" | "vp8" | "hevc" | "h265"
            | "mpeg4" | "wmv3" | "vc1" | "av1"
        )
    }
}
```

### FileMeta

```rust
/// Complete metadata describing a playable resource.
/// This is the central data structure exchanged via the `ut_metadata`
/// extension and persisted to `.qmv` cache files.
///
/// FileMeta is obtained BEFORE any video data is downloaded.
/// It drives the PieceScheduler, SeekEngine, and HlsAdapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    /// 20-byte SHA-1 info hash identifying this resource.
    pub info_hash: InfoHash,

    /// Original filename from the .qvs / qvod:// URI.
    pub filename: String,

    /// Total file size in bytes.
    pub file_size: u64,

    /// Size of each piece in bytes. Default: 262144 (256 KB).
    pub piece_length: u64,

    /// SHA-1 hash for each piece. Length = ceil(file_size / piece_length).
    pub piece_hashes: Vec<PieceHash>,

    /// Keyframe index for frame-accurate seeking and priority scheduling.
    pub keyframe_index: KeyFrameIndex,

    /// Total duration in milliseconds.
    pub duration_ms: u64,

    /// Codec metadata for decoder initialization.
    pub codec: CodecInfo,

    /// Whether this metadata came from local cache (true) or network (false).
    #[serde(default)]
    pub from_cache: bool,
}

impl FileMeta {
    /// Total number of pieces in the file.
    pub fn num_pieces(&self) -> u32 {
        self.piece_hashes.len() as u32
    }

    /// Calculate the byte range for a given piece index.
    pub fn piece_byte_range(&self, piece_index: u32) -> std::ops::Range<u64> {
        let start = piece_index as u64 * self.piece_length;
        let end = if piece_index as usize == self.piece_hashes.len() - 1 {
            self.file_size
        } else {
            start + self.piece_length
        };
        start..end
    }

    /// Actual length of a piece (last piece may be shorter).
    pub fn piece_size(&self, piece_index: u32) -> u64 {
        let range = self.piece_byte_range(piece_index);
        range.end - range.start
    }

    /// Verify a piece's data against its stored SHA-1 hash.
    pub fn verify_piece(&self, piece_index: u32, data: &[u8]) -> bool {
        if piece_index as usize >= self.piece_hashes.len() {
            return false;
        }
        let expected = &self.piece_hashes[piece_index as usize];
        let actual = sha1::Sha1::from(data).digest().bytes();
        expected.0 == actual
    }

    /// Find all piece indices that contain keyframes (I-frames).
    /// These pieces receive Critical priority during scheduling.
    pub fn keyframe_pieces(&self) -> Vec<u32> {
        let mut pieces: Vec<u32> = self
            .keyframe_index
            .entries
            .iter()
            .filter(|e| e.frame_type == FrameType::I)
            .map(|e| e.piece_index(self.piece_length))
            .collect();
        pieces.sort();
        pieces.dedup();
        pieces
    }

    /// Find the piece index containing a specific byte offset.
    pub fn piece_at_offset(&self, offset: u64) -> u32 {
        (offset / self.piece_length) as u32
    }

    /// Calculate playback progress as a fraction (0.0–1.0).
    pub fn progress_at_offset(&self, offset: u64) -> f64 {
        if self.file_size == 0 {
            return 0.0;
        }
        offset as f64 / self.file_size as f64
    }

    /// Estimate the file offset for a given timestamp (via keyframe index).
    pub fn offset_at_timestamp(&self, timestamp_ms: u64) -> u64 {
        self.keyframe_index
            .nearest_iframe(timestamp_ms)
            .map(|e| e.file_offset)
            .unwrap_or(0)
    }

    /// Bencode-encode this FileMeta into a byte vector.
    pub fn encode(&self) -> Vec<u8>;

    /// Decode a FileMeta from Bencode-encoded bytes.
    pub fn decode(data: &[u8]) -> Result<Self, MetadataError>;
}

// Implementation of encode/decode for FileMeta
impl FileMeta {
    pub fn encode(&self) -> Vec<u8> {
        use bencode::{BencodeValue, Dictionary};
        let mut d = Dictionary::new();
        d.insert("info_hash".into(), BencodeValue::String(self.info_hash.0.to_vec()));
        d.insert("filename".into(), BencodeValue::String(self.filename.as_bytes().to_vec()));
        d.insert("file_size".into(), BencodeValue::Integer(self.file_size as i64));
        d.insert("piece_length".into(), BencodeValue::Integer(self.piece_length as i64));

        // Flatten piece hashes
        let pieces_bytes: Vec<u8> = self.piece_hashes.iter().flat_map(|h| h.0).collect();
        d.insert("pieces".into(), BencodeValue::String(pieces_bytes));

        // Keyframe index
        let kf_entries: Vec<BencodeValue> = self.keyframe_index.entries.iter().map(|e| {
            let mut ed = Dictionary::new();
            ed.insert("timestamp_ms".into(), BencodeValue::Integer(e.timestamp_ms as i64));
            ed.insert("file_offset".into(), BencodeValue::Integer(e.file_offset as i64));
            ed.insert("frame_size".into(), BencodeValue::Integer(e.frame_size as i64));
            let ft = match e.frame_type {
                FrameType::I => "I",
                FrameType::P => "P",
                FrameType::B => "B",
            };
            ed.insert("frame_type".into(), BencodeValue::String(ft.as_bytes().to_vec()));
            BencodeValue::Dictionary(ed)
        }).collect();
        d.insert("keyframe_index".into(), BencodeValue::List(kf_entries));

        d.insert("duration_ms".into(), BencodeValue::Integer(self.duration_ms as i64));

        // Codec info sub-dictionary
        let mut cd = Dictionary::new();
        cd.insert("video_codec".into(), BencodeValue::String(self.codec.video_codec.as_bytes().to_vec()));
        cd.insert("audio_codec".into(), BencodeValue::String(self.codec.audio_codec.as_bytes().to_vec()));
        cd.insert("width".into(), BencodeValue::Integer(self.codec.width as i64));
        cd.insert("height".into(), BencodeValue::Integer(self.codec.height as i64));
        cd.insert("bitrate".into(), BencodeValue::Integer(self.codec.bitrate as i64));
        cd.insert("audio_sample_rate".into(), BencodeValue::Integer(self.codec.audio_sample_rate as i64));
        cd.insert("audio_channels".into(), BencodeValue::Integer(self.codec.audio_channels as i64));
        cd.insert("frame_rate_num".into(), BencodeValue::Integer(self.codec.frame_rate_num as i64));
        cd.insert("frame_rate_den".into(), BencodeValue::Integer(self.codec.frame_rate_den as i64));
        cd.insert("pixel_aspect_num".into(), BencodeValue::Integer(self.codec.pixel_aspect_num as i64));
        cd.insert("pixel_aspect_den".into(), BencodeValue::Integer(self.codec.pixel_aspect_den as i64));
        cd.insert("has_b_frames".into(), BencodeValue::Integer(if self.codec.has_b_frames { 1 } else { 0 }));
        d.insert("codec".into(), BencodeValue::Dictionary(cd));

        BencodeValue::Dictionary(d).encode()
    }

    pub fn decode(data: &[u8]) -> Result<Self, MetadataError> {
        use bencode::BencodeValue;
        let value = BencodeValue::decode(data).map_err(|_| MetadataError::BencodeParse)?;

        let dict = match &value {
            BencodeValue::Dictionary(d) => d,
            _ => return Err(MetadataError::BencodeParse),
        };

        let info_hash_bytes = dict.get_bytes("info_hash").ok_or(MetadataError::MissingField("info_hash"))?;
        let info_hash = if info_hash_bytes.len() == 40 {
            // Hex string
            InfoHash::from_hex(std::str::from_utf8(info_hash_bytes).map_err(|_| MetadataError::InvalidInfoHash)?)?
        } else if info_hash_bytes.len() == 20 {
            let mut arr = [0u8; 20];
            arr.copy_from_slice(info_hash_bytes);
            InfoHash(arr)
        } else {
            return Err(MetadataError::InvalidInfoHash);
        };

        let filename = String::from_utf8(dict.get_bytes("filename").ok_or(MetadataError::MissingField("filename"))?.to_vec())
            .map_err(|_| MetadataError::InvalidUtf8)?;
        let file_size = dict.get_int("file_size").ok_or(MetadataError::MissingField("file_size"))? as u64;
        let piece_length = dict.get_int("piece_length").ok_or(MetadataError::MissingField("piece_length"))? as u64;

        // Parse piece hashes (concatenated 20-byte values)
        let pieces_data = dict.get_bytes("pieces").ok_or(MetadataError::MissingField("pieces"))?;
        if pieces_data.len() % 20 != 0 {
            return Err(MetadataError::InvalidPieceHashes);
        }
        let piece_hashes: Vec<PieceHash> = pieces_data
            .chunks_exact(20)
            .map(|chunk| {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(chunk);
                PieceHash(arr)
            })
            .collect();

        // Parse keyframe index
        let kf_list = dict.get_list("keyframe_index").ok_or(MetadataError::MissingField("keyframe_index"))?;
        let mut entries = Vec::new();
        for item in kf_list {
            let ed = match item {
                BencodeValue::Dictionary(d) => d,
                _ => return Err(MetadataError::BencodeParse),
            };
            let timestamp_ms = ed.get_int("timestamp_ms").ok_or(MetadataError::MissingField("timestamp_ms"))? as u64;
            let file_offset = ed.get_int("file_offset").ok_or(MetadataError::MissingField("file_offset"))? as u64;
            let frame_size = ed.get_int("frame_size").ok_or(MetadataError::MissingField("frame_size"))? as u32;
            let ft_str = std::str::from_utf8(
                ed.get_bytes("frame_type").ok_or(MetadataError::MissingField("frame_type"))?
            ).map_err(|_| MetadataError::BencodeParse)?;
            let frame_type = match ft_str {
                "I" => FrameType::I,
                "P" => FrameType::P,
                "B" => FrameType::B,
                _ => return Err(MetadataError::InvalidFrameType),
            };
            entries.push(KeyFrameEntry { timestamp_ms, file_offset, frame_size, frame_type });
        }
        let keyframe_index = KeyFrameIndex::new(entries)?;

        let duration_ms = dict.get_int("duration_ms").ok_or(MetadataError::MissingField("duration_ms"))? as u64;

        // Parse codec info
        let codec_dict = dict.get_dict("codec").ok_or(MetadataError::MissingField("codec"))?;
        let video_codec = String::from_utf8(
            codec_dict.get_bytes("video_codec").ok_or(MetadataError::MissingField("video_codec"))?.to_vec()
        ).map_err(|_| MetadataError::InvalidUtf8)?;
        let audio_codec = String::from_utf8(
            codec_dict.get_bytes("audio_codec").ok_or(MetadataError::MissingField("audio_codec"))?.to_vec()
        ).map_err(|_| MetadataError::InvalidUtf8)?;
        let width = codec_dict.get_int("width").ok_or(MetadataError::MissingField("width"))? as u32;
        let height = codec_dict.get_int("height").ok_or(MetadataError::MissingField("height"))? as u32;
        let bitrate = codec_dict.get_int("bitrate").unwrap_or(0) as u32;
        let audio_sample_rate = codec_dict.get_int("audio_sample_rate").unwrap_or(0) as u32;
        let audio_channels = codec_dict.get_int("audio_channels").unwrap_or(0) as u8;
        let frame_rate_num = codec_dict.get_int("frame_rate_num").unwrap_or(0) as u32;
        let frame_rate_den = codec_dict.get_int("frame_rate_den").unwrap_or(1) as u32;
        let pixel_aspect_num = codec_dict.get_int("pixel_aspect_num").unwrap_or(1) as u32;
        let pixel_aspect_den = codec_dict.get_int("pixel_aspect_den").unwrap_or(1) as u32;
        let has_b_frames = codec_dict.get_int("has_b_frames").unwrap_or(0) != 0;

        let codec = CodecInfo {
            video_codec,
            audio_codec,
            width,
            height,
            bitrate,
            audio_sample_rate,
            audio_channels,
            frame_rate_num,
            frame_rate_den,
            pixel_aspect_num,
            pixel_aspect_den,
            has_b_frames,
        };

        Ok(Self {
            info_hash,
            filename,
            file_size,
            piece_length,
            piece_hashes,
            keyframe_index,
            duration_ms,
            codec,
            from_cache: false,
        })
    }
}

/// Count of blocks within a single piece.
pub const BLOCKS_PER_PIECE: u32 = 16;

/// Size of each block in bytes.
pub const BLOCK_LENGTH: u64 = 16 * 1024; // 16 KB

/// Size of each piece in bytes.
pub const PIECE_LENGTH: u64 = BLOCK_LENGTH * BLOCKS_PER_PIECE as u64; // 256 KB
```

## Metadata Exchange Protocol: ut_metadata

QVOD uses the `ut_metadata` BitTorrent extension (BEP 9) to exchange FileMeta between peers. This is a lightweight, two-message protocol layered on top of the standard peer wire protocol.

### Extension Handshake

Before metadata exchange, both peers must advertise support via the LTEP (LibTorrent Extension Protocol) handshake:

```
Handshake message (via standard peer wire, msg_id = 0x14):
{
    "m": {
        "ut_metadata": 2,    // metadata message IDs start at 2
        "ut_pex": 3          // peer exchange (optional)
    },
    "metadata_size": 12345   // total size of metadata in bytes
}
```

### Message Types

Once the extension is negotiated, metadata is requested and transferred using these messages:

```rust
/// ut_metadata message types as defined in BEP 9.
#[repr(u8)]
pub enum MetadataMessageType {
    /// Request a metadata piece from a peer.
    Request = 0,
    /// Deliver a metadata piece to a peer.
    Data = 1,
    /// Peer does not have the requested metadata piece.
    Reject = 2,
}

/// A single metadata message exchanged between peers.
pub struct MetadataMessage {
    /// Message type (request, data, or reject).
    pub msg_type: MetadataMessageType,
    /// Zero-based index of the metadata piece being requested/delivered.
    pub piece: u32,
    /// Total metadata size in bytes. Only present in Data messages.
    pub total_size: Option<u64>,
    /// The metadata piece data (only present in Data messages).
    pub data: Option<Vec<u8>>,
}

impl MetadataMessage {
    /// Serialize this message to bencode bytes for wire transmission.
    pub fn encode(&self) -> Vec<u8>;

    /// Deserialize from bencode bytes received on the wire.
    pub fn decode(data: &[u8]) -> Result<Self, MetadataError>;
}
```

### Metadata Download Procedure

The metadata download follows this exact sequence:

```
Phase 1: Connection Establishment
  1. Establish TCP connection to peer
  2. Send standard BitTorrent handshake (68 bytes)
  3. Receive peer handshake + bitfield
  4. Send extended handshake with "ut_metadata" support

Phase 2: Metadata Size Discovery
  1. Receive peer's extended handshake
  2. Extract "metadata_size" from handshake dictionary
  3. If metadata_size > 0 and matches expected:
       -> Proceed to Phase 3
  4. If metadata_size is 0 or missing:
       -> Try next peer

Phase 3: Metadata Piece Request
  1. Calculate number of metadata pieces:
     metadata_pieces = ceil(metadata_size / (16 * 1024))
  2. For each piece index 0..metadata_pieces:
     a. Send ut_metadata Request message
     b. Start timeout timer (default: 10 seconds per piece)
     c. On Data response:
        - Verify piece fits expected size
        - Append to metadata buffer
     d. On Reject or timeout:
        - Request from another peer
  3. After all pieces collected:
     -> Concatenate in order
     -> Parse as bencode -> FileMeta
     -> Verify info_hash matches expected

Phase 4: Post-Download
  1. Cache FileMeta to .qmv file
  2. Initialize PieceScheduler with KeyFrameIndex
  3. Begin data download
```

### Metadata Request Strategy

```rust
/// Configuration for metadata fetching from the peer swarm.
pub struct MetadataFetcher {
    /// Maximum number of peers to request metadata from concurrently.
    pub max_concurrent_requests: usize,       // default: 3
    /// Timeout per metadata piece request in seconds.
    pub piece_timeout: Duration,               // default: 10s
    /// Total timeout for metadata download completion.
    pub total_timeout: Duration,               // default: 60s
    /// Whether to accept metadata from cache without network fetch.
    pub allow_cached: bool,                    // default: true
}

impl MetadataFetcher {
    /// Start fetching metadata for the given info_hash.
    /// Returns a Future that resolves to FileMeta.
    pub async fn fetch(
        &self,
        info_hash: &InfoHash,
        peer_pool: &ConnectionPool,
    ) -> Result<FileMeta, MetadataError> {
        // 1. Check local cache first if allow_cached
        if self.allow_cached {
            if let Some(cached) = MetadataCache::load(info_hash) {
                return Ok(cached);
            }
        }

        // 2. Get connected peers that support ut_metadata
        let candidates = peer_pool
            .peers_with_extension("ut_metadata")
            .await;

        if candidates.is_empty() {
            return Err(MetadataError::NoMetadataPeers);
        }

        // 3. Request metadata pieces from multiple peers
        let metadata_bytes = self
            .download_metadata_pieces(&candidates)
            .await?;

        // 4. Parse & validate
        let meta = FileMeta::decode(&metadata_bytes)?;
        if meta.info_hash != *info_hash {
            return Err(MetadataError::InfoHashMismatch);
        }

        // 5. Cache to disk
        MetadataCache::save(&meta)?;

        Ok(meta)
    }
}
```

### Parallel Metadata Piece Request

```rust
impl MetadataFetcher {
    /// Download all metadata pieces from the peer set using
    /// a rarest-first strategy to maximize resilience.
    async fn download_metadata_pieces(
        &self,
        peers: &[PeerHandle],
    ) -> Result<Vec<u8>, MetadataError> {
        // Discover metadata_size from any connected peer
        let metadata_size = self.discover_metadata_size(peers).await?;
        let num_pieces = (metadata_size + 16383) / 16384; // ceil division

        let mut buffer: Vec<u8> = vec![0u8; metadata_size as usize];
        let mut have_piece: Vec<bool> = vec![false; num_pieces as usize];
        let mut piece_sources: Vec<Vec<usize>> = vec![Vec::new(); num_pieces as usize];

        // Track which peers have which pieces
        // Use a simple round-robin at first, then rarest-first
        let mut request_queue: Vec<u32> = (0..num_pieces).collect();
        let mut completed = 0usize;
        let deadline = Instant::now() + self.total_timeout;

        while completed < num_pieces as usize && Instant::now() < deadline {
            // Select up to max_concurrent_requests outstanding pieces
            let mut in_flight = 0u32;
            for &piece_idx in &request_queue.clone() {
                if in_flight >= self.max_concurrent_requests as u32 {
                    break;
                }
                if have_piece[piece_idx as usize] {
                    continue;
                }

                // Find a peer for this piece
                let peer_idx = piece_idx as usize % peers.len();
                let peer = &peers[peer_idx];

                match self.request_single_piece(peer, piece_idx, self.piece_timeout).await {
                    Ok(data) => {
                        let offset = piece_idx as usize * 16384;
                        let len = data.len().min(buffer.len() - offset);
                        buffer[offset..offset + len].copy_from_slice(&data[..len]);
                        have_piece[piece_idx as usize] = true;
                        completed += 1;
                        in_flight += 1;
                    }
                    Err(_) => {
                        // Try another peer for this piece
                        continue;
                    }
                }
            }

            // Brief yield to avoid busy-loop
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if completed < num_pieces as usize {
            return Err(MetadataError::Timeout);
        }

        Ok(buffer)
    }

    /// Request a single metadata piece from a specific peer.
    async fn request_single_piece(
        &self,
        peer: &PeerHandle,
        piece_index: u32,
        timeout: Duration,
    ) -> Result<Vec<u8>, MetadataError> {
        // Send ut_metadata request message
        let request_msg = MetadataMessage {
            msg_type: MetadataMessageType::Request,
            piece: piece_index,
            total_size: None,
            data: None,
        };
        peer.send_ext_message("ut_metadata", &request_msg.encode()).await
            .map_err(|_| MetadataError::SendFailed)?;

        // Wait for response (Data or Reject)
        let response = tokio::time::timeout(
            timeout,
            peer.wait_for_message("ut_metadata"),
        ).await.map_err(|_| MetadataError::Timeout)??;

        let msg = MetadataMessage::decode(&response)?;
        match msg.msg_type {
            MetadataMessageType::Data => {
                Ok(msg.data.unwrap_or_default())
            }
            MetadataMessageType::Reject => {
                Err(MetadataError::Rejected)
            }
            _ => Err(MetadataError::ProtocolError),
        }
    }

    /// Obtain total metadata size from the peer's extended handshake.
    async fn discover_metadata_size(&self, peers: &[PeerHandle]) -> Result<u64, MetadataError> {
        for peer in peers {
            if let Some(handshake) = peer.ext_handshake().await {
                if let Some(size) = handshake.get("metadata_size").and_then(|v| v.as_integer()) {
                    if size > 0 && size < 10_000_000 {
                        // Sanity check: metadata < 10 MB
                        return Ok(size as u64);
                    }
                }
            }
        }
        // Fall back to a reasonable default
        Err(MetadataError::UnknownMetadataSize)
    }
}
```

## Metadata Caching (.qmv Files)

Once fetched, FileMeta is cached on disk as a `.qmv` file to avoid re-downloading on subsequent plays. The cache format is a direct Bencode dump of the serialized FileMeta.

### Cache File Format

```
{cache_dir}/qmv/{info_hash_hex}.qmv
```

File contents: raw Bencode-encoded `FileMeta` dictionary.

### Cache Read/Write

```rust
/// Manages on-disk metadata caching.
pub struct MetadataCache {
    cache_dir: PathBuf,
}

impl MetadataCache {
    /// Create a new MetadataCache rooted at the given directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        let qmv_dir = cache_dir.join("qmv");
        std::fs::create_dir_all(&qmv_dir).ok();
        Self { cache_dir }
    }

    /// Path to the .qmv file for a given info_hash hex.
    fn qmv_path(&self, info_hash: &InfoHash) -> PathBuf {
        self.cache_dir.join("qmv").join(format!("{}.qmv", info_hash.to_hex()))
    }

    /// Save FileMeta to a .qmv cache file.
    /// Overwrites any existing file silently.
    pub fn save(&self, meta: &FileMeta) -> Result<(), MetadataError> {
        let path = self.qmv_path(&meta.info_hash);
        let encoded = meta.encode();
        std::fs::write(&path, &encoded)
            .map_err(|e| MetadataError::CacheIo(e.to_string()))?;
        Ok(())
    }

    /// Load FileMeta from a .qmv cache file.
    /// Returns None if the file does not exist or fails to parse.
    pub fn load(&self, info_hash: &InfoHash) -> Option<FileMeta> {
        let path = self.qmv_path(info_hash);
        let data = std::fs::read(&path).ok()?;
        let mut meta = FileMeta::decode(&data).ok()?;
        meta.from_cache = true;
        Some(meta)
    }

    /// Check if a cached .qmv file exists for the given hash.
    pub fn exists(&self, info_hash: &InfoHash) -> bool {
        self.qmv_path(info_hash).exists()
    }

    /// Delete the cached .qmv file.
    pub fn delete(&self, info_hash: &InfoHash) -> Result<(), MetadataError> {
        let path = self.qmv_path(info_hash);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| MetadataError::CacheIo(e.to_string()))?;
        }
        Ok(())
    }

    /// Return the total size of all .qmv files in the cache directory.
    pub fn total_size(&self) -> u64 {
        let qmv_dir = self.cache_dir.join("qmv");
        if !qmv_dir.exists() {
            return 0;
        }
        std::fs::read_dir(&qmv_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }
}
```

## Metadata Download Priority

QVOD enforces a strict download order: **metadata MUST complete before any data piece is requested**. This is enforced by the `QvodEngine` play() method:

```rust
impl QvodEngine {
    pub async fn play(&mut self, uri: &QvodUri) -> Result<MediaStream, EngineError> {
        // Step 1: Parse URI
        let info_hash = uri.info_hash();

        // Step 2: Check local metadata cache
        let meta = if let Some(cached) = self.metadata_cache.load(&info_hash) {
            tracing::info!("Metadata loaded from cache for {}", info_hash.to_hex());
            cached
        } else {
            // Step 3: Connect to Tracker & DHT to find peers
            let peers = self.discover_peers(&info_hash).await?;

            // Step 4: Connect to top peers
            self.connection_pool.connect_peers(&peers).await?;

            // Step 5: Fetch metadata from peers (BLOCKING data download)
            tracing::info!("Fetching metadata from network for {}", info_hash.to_hex());
            let meta = self.metadata_fetcher.fetch(&info_hash, &self.connection_pool).await?;

            // Step 6: Cache it
            self.metadata_cache.save(&meta)?;
            meta
        };

        // Step 7: NOW we can initialize the scheduler and begin data download
        self.scheduler = PieceScheduler::new(meta.clone());
        self.buffer = RingBuffer::new(self.config.buffer_capacity_bytes);
        self.downloader = P2spDownloader::new(
            meta.clone(),
            self.connection_pool.clone(),
            self.http_sources.clone(),
        );

        // Step 8: Start streaming
        Ok(MediaStream::new(
            meta,
            self.buffer.clone(),
            self.scheduler.clone(),
        ))
    }
}
```

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("Failed to parse Bencode data")]
    BencodeParse,

    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[error("Invalid info hash")]
    InvalidInfoHash,

    #[error("Invalid piece hashes length (not multiple of 20)")]
    InvalidPieceHashes,

    #[error("Invalid frame type string")]
    InvalidFrameType,

    #[error("Invalid UTF-8 in metadata string")]
    InvalidUtf8,

    #[error("Info hash mismatch between URI and metadata")]
    InfoHashMismatch,

    #[error("No peers supporting ut_metadata available")]
    NoMetadataPeers,

    #[error("Metadata size unknown")]
    UnknownMetadataSize,

    #[error("Metadata download timed out")]
    Timeout,

    #[error("Peer rejected metadata request")]
    Rejected,

    #[error("Failed to send metadata request")]
    SendFailed,

    #[error("Protocol error during metadata exchange")]
    ProtocolError,

    #[error("Cache I/O error: {0}")]
    CacheIo(String),

    #[error("Metadata too large: {0} bytes exceeds maximum {1} bytes")]
    TooLarge(u64, u64),
}
```

## Serialization Format Summary

| Field | Bencode Type | Description |
|-------|-------------|-------------|
| `info_hash` | string (20 bytes binary) | Resource identifier |
| `filename` | string (UTF-8) | Original filename |
| `file_size` | integer | Total bytes |
| `piece_length` | integer | Bytes per piece (default 262144) |
| `pieces` | string (N*20 bytes) | Concatenated SHA-1 hashes |
| `keyframe_index` | list of dicts | Frame position index |
| `duration_ms` | integer | Playback duration |
| `codec` | dict | Codec metadata sub-dictionary |
| `codec.video_codec` | string | e.g., "avc1", "rv40" |
| `codec.audio_codec` | string | e.g., "aac", "cook" |
| `codec.width` | integer | Video width |
| `codec.height` | integer | Video height |
| `codec.bitrate` | integer | Bits per second |

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_meta() -> FileMeta {
        let mut entries = vec![
            KeyFrameEntry {
                timestamp_ms: 0,
                file_offset: 0,
                frame_size: 45000,
                frame_type: FrameType::I,
            },
            KeyFrameEntry {
                timestamp_ms: 40,
                file_offset: 45000,
                frame_size: 12000,
                frame_type: FrameType::P,
            },
            KeyFrameEntry {
                timestamp_ms: 80,
                file_offset: 57000,
                frame_size: 15000,
                frame_type: FrameType::B,
            },
            KeyFrameEntry {
                timestamp_ms: 5000,
                file_offset: 262144,
                frame_size: 48000,
                frame_type: FrameType::I,
            },
        ];
        let kf_idx = KeyFrameIndex::new(entries).unwrap();

        let codec = CodecInfo {
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
        };

        FileMeta {
            info_hash: InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: 1_000_000,
            piece_length: 262144,
            piece_hashes: vec![PieceHash([0u8; 20]); 4],
            keyframe_index: kf_idx,
            duration_ms: 60000,
            codec,
            from_cache: false,
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let meta = sample_meta();
        let encoded = meta.encode();
        let decoded = FileMeta::decode(&encoded).unwrap();

        assert_eq!(decoded.info_hash, meta.info_hash);
        assert_eq!(decoded.filename, meta.filename);
        assert_eq!(decoded.file_size, meta.file_size);
        assert_eq!(decoded.piece_length, meta.piece_length);
        assert_eq!(decoded.piece_hashes.len(), meta.piece_hashes.len());
        assert_eq!(decoded.keyframe_index.len(), meta.keyframe_index.len());
        assert_eq!(decoded.keyframe_index.keyframe_count, 2);
        assert_eq!(decoded.duration_ms, meta.duration_ms);
        assert_eq!(decoded.codec.video_codec, "avc1");
        assert_eq!(decoded.codec.width, 1280);
        assert_eq!(decoded.codec.frame_rate_num, 30000);
    }

    #[test]
    fn test_nearest_iframe() {
        let meta = sample_meta();
        let idx = &meta.keyframe_index;

        // Before first frame -> returns first I-frame
        let near = idx.nearest_iframe(0).unwrap();
        assert_eq!(near.timestamp_ms, 0);

        // Between I-frames -> returns closest
        let near = idx.nearest_iframe(2500).unwrap();
        // Both are 2500ms away, so min_by_key picks the first
        assert_eq!(near.timestamp_ms, 0);

        // After last I-frame -> returns last I-frame
        let near = idx.nearest_iframe(10000).unwrap();
        assert_eq!(near.timestamp_ms, 5000);
    }

    #[test]
    fn test_keyframe_pieces() {
        let meta = sample_meta();
        let pieces = meta.keyframe_pieces();
        assert_eq!(pieces, vec![0, 1]);
    }

    #[test]
    fn test_piece_byte_range() {
        let meta = sample_meta();
        let range = meta.piece_byte_range(0);
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 262144);

        let range = meta.piece_byte_range(3);
        assert_eq!(range.start, 262144 * 3);
        assert_eq!(range.end, 1_000_000); // last piece is shorter
    }

    #[test]
    fn test_metadata_message_encode_decode() {
        let msg = MetadataMessage {
            msg_type: MetadataMessageType::Data,
            piece: 0,
            total_size: Some(12345),
            data: Some(vec![1, 2, 3, 4, 5]),
        };
        let encoded = msg.encode();
        let decoded = MetadataMessage::decode(&encoded).unwrap();
        assert_eq!(decoded.msg_type as u8, MetadataMessageType::Data as u8);
        assert_eq!(decoded.piece, 0);
        assert_eq!(decoded.total_size, Some(12345));
        assert_eq!(decoded.data, Some(vec![1, 2, 3, 4, 5]));
    }

    #[test]
    fn test_codec_bitrate_string() {
        let codec = CodecInfo {
            bitrate: 2_500_000,
            ..sample_meta().codec
        };
        assert_eq!(codec.bitrate_string(), "2.5 Mbps");

        let codec = CodecInfo {
            bitrate: 512_000,
            ..sample_meta().codec
        };
        assert_eq!(codec.bitrate_string(), "512 Kbps");
    }

    #[test]
    fn test_cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MetadataCache::new(dir.path().to_path_buf());
        let meta = sample_meta();

        cache.save(&meta).unwrap();
        assert!(cache.exists(&meta.info_hash));

        let loaded = cache.load(&meta.info_hash).unwrap();
        assert!(loaded.from_cache);
        assert_eq!(loaded.filename, meta.filename);

        cache.delete(&meta.info_hash).unwrap();
        assert!(!cache.exists(&meta.info_hash));
    }
}
```

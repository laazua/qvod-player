use std::{fmt, net::SocketAddr, str::FromStr, time::Duration};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InfoHash(pub [u8; 20]);

impl InfoHash {
    #[must_use]
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for InfoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for InfoHash {
    type Err = crate::error::QvodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 40 {
            return Err(crate::error::QvodError::InvalidUri(
                "info_hash must be 40 hex characters".into(),
            ));
        }
        let mut bytes = [0u8; 20];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|e| crate::error::QvodError::InvalidUri(format!("hex decode: {e}")))?;
        }
        Ok(Self(bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub [u8; 20]);

impl NodeId {
    #[must_use]
    pub fn xor_distance(&self, other: &NodeId) -> [u8; 20] {
        let mut dist = [0u8; 20];
        for (i, d) in dist.iter_mut().enumerate() {
            *d = self.0[i] ^ other.0[i];
        }
        dist
    }

    #[must_use]
    pub fn leading_zeros(&self) -> u32 {
        let mut count = 0;
        for &byte in &self.0 {
            if byte == 0 {
                count += 8;
            } else {
                count += byte.leading_zeros();
                break;
            }
        }
        count
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: [u8; 20],
    pub addr: SocketAddr,
    pub is_firewalled: bool,
    pub bw_up: u32,
    pub bw_down: u32,
    pub location: Option<String>,
    pub latency: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bitfield {
    bytes: Vec<u8>,
    num_pieces: u32,
}

impl Bitfield {
    #[must_use]
    pub fn new(num_pieces: u32) -> Self {
        let byte_len = (num_pieces as usize).div_ceil(8);
        Self {
            bytes: vec![0u8; byte_len],
            num_pieces,
        }
    }

    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>, num_pieces: u32) -> Self {
        Self { bytes, num_pieces }
    }

    #[must_use]
    pub fn has(&self, index: u32) -> bool {
        if index >= self.num_pieces {
            return false;
        }
        let byte_index = (index / 8) as usize;
        let bit_offset = (index % 8) as u8;
        if byte_index >= self.bytes.len() {
            return false;
        }
        (self.bytes[byte_index] & (1 << (7 - bit_offset))) != 0
    }

    pub fn set(&mut self, index: u32, value: bool) {
        if index >= self.num_pieces {
            return;
        }
        let byte_index = (index / 8) as usize;
        let bit_offset = (index % 8) as u8;
        if byte_index >= self.bytes.len() {
            return;
        }
        if value {
            self.bytes[byte_index] |= 1 << (7 - bit_offset);
        } else {
            self.bytes[byte_index] &= !(1 << (7 - bit_offset));
        }
    }

    pub fn set_all(&mut self, value: bool) {
        let v = if value { 0xFF } else { 0x00 };
        for byte in &mut self.bytes {
            *byte = v;
        }
    }

    #[must_use]
    pub fn count(&self) -> u32 {
        self.bytes
            .iter()
            .map(|&b| b.count_ones())
            .sum::<u32>()
            .min(self.num_pieces)
    }

    #[must_use]
    pub fn completion(&self) -> f64 {
        if self.num_pieces == 0 {
            return 1.0;
        }
        f64::from(self.count()) / f64::from(self.num_pieces)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    #[must_use]
    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn num_pieces(&self) -> u32 {
        self.num_pieces
    }

    #[must_use]
    pub fn iter(&self) -> BitfieldIter<'_> {
        BitfieldIter {
            bitfield: self,
            index: 0,
        }
    }
}

impl<'a> IntoIterator for &'a Bitfield {
    type Item = bool;
    type IntoIter = BitfieldIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct BitfieldIter<'a> {
    bitfield: &'a Bitfield,
    index: u32,
}

impl Iterator for BitfieldIter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.bitfield.num_pieces {
            return None;
        }
        let val = self.bitfield.has(self.index);
        self.index += 1;
        Some(val)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PiecePriority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PieceInfo {
    pub index: u32,
    pub hash: [u8; 20],
    pub priority: PiecePriority,
    pub length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRequest {
    pub piece_index: u32,
    pub begin: u32,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStats {
    pub speed_down: f64,
    pub speed_up: f64,
    pub rtt: Duration,
    pub loss_rate: f64,
    pub total_downloaded: u64,
    pub total_uploaded: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AnnounceEvent {
    Started,
    Completed,
    Stopped,
    Empty,
}

impl AnnounceEvent {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Empty => "empty",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmStatus {
    pub complete: u32,
    pub incomplete: u32,
    pub downloaded: u32,
}

#[derive(Debug, Clone)]
pub struct KBucketEntry {
    pub node_id: NodeId,
    pub addr: SocketAddr,
    pub last_seen: std::time::Instant,
    pub latency: Duration,
    pub is_firewalled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameType {
    I,
    P,
    B,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFrameEntry {
    pub timestamp_ms: u64,
    pub file_offset: u64,
    pub frame_size: u64,
    pub frame_type: FrameType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFrameIndex {
    pub entries: Vec<KeyFrameEntry>,
}

impl KeyFrameIndex {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn find_nearest_i_frame(&self, timestamp_ms: u64) -> Option<&KeyFrameEntry> {
        let mut best: Option<&KeyFrameEntry> = None;
        for entry in &self.entries {
            if entry.frame_type != FrameType::I {
                continue;
            }
            if entry.timestamp_ms <= timestamp_ms {
                match best {
                    None => best = Some(entry),
                    Some(current) => {
                        if timestamp_ms - entry.timestamp_ms < timestamp_ms - current.timestamp_ms {
                            best = Some(entry);
                        }
                    }
                }
            }
        }
        best
    }

    #[must_use]
    pub fn find_all_i_frames(&self) -> Vec<&KeyFrameEntry> {
        self.entries
            .iter()
            .filter(|e| e.frame_type == FrameType::I)
            .collect()
    }

    #[must_use]
    pub fn segment_at(&self, segment_index: usize) -> Option<(u64, u64)> {
        let iframes: Vec<&KeyFrameEntry> = self.find_all_i_frames();
        if segment_index >= iframes.len() {
            return None;
        }
        let start = iframes[segment_index].file_offset;
        let end = if segment_index + 1 < iframes.len() {
            iframes[segment_index + 1].file_offset
        } else {
            start + iframes[segment_index].frame_size
        };
        Some((start, end - start))
    }
}

impl Default for KeyFrameIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub info_hash: InfoHash,
    pub filename: String,
    pub file_size: u64,
    pub piece_length: u64,
    pub pieces: Vec<[u8; 20]>,
    pub keyframe_index: Option<KeyFrameIndex>,
    pub duration_ms: u64,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: u32,
    pub height: u32,
    pub bitrate: u64,
}

#[derive(Debug, Clone)]
pub struct MediaStream {
    pub metadata: FileMeta,
}

impl MediaStream {
    #[must_use]
    pub fn new(metadata: FileMeta) -> Self {
        Self { metadata }
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn num_pieces(&self) -> u32 {
        let total = self.metadata.file_size + self.metadata.piece_length - 1;
        (total / self.metadata.piece_length) as u32
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TransportMode {
    Normal,
    TcpOnly,
}

# Storage Subsystem Module Specification

## Overview

The Storage subsystem handles **persistent data** for the QVOD system across five concerns:

1. **`.qvs` seed files** — Bencode-encoded resource descriptors (analogous to `.torrent`)
2. **`qvod://` URI parsing** — Extracting resource identifiers from protocol links
3. **Bencode serialization** — Encoding/decoding of structured data for wire and disk formats
4. **Configuration persistence** — Saving/loading engine settings from TOML files
5. **Download state persistence** — Saving progress for resume support

### Design Goals

- **Bencode compatibility** — Interoperable with BitTorrent's Bencode format for `.qvs` files
- **URI integrity** — Strict validation of `qvod://` URIs to prevent injection
- **Atomic writes** — Configuration and state files are written atomically (write to temp, rename)
- **Cross-platform paths** — All path handling uses `PathBuf` and OS-appropriate separators
- **Forward compatibility** — Unknown Bencode dictionary keys are preserved and re-encoded

## 1. QVS File Format

A `.qvs` file is the QVOD equivalent of a BitTorrent `.torrent` file. It describes a playable resource using Bencode encoding.

### Structure

```
d
  8:info_hash 40:{hex_string}
  6:length i{filesize}e
  12:piece length i{piece_length}e
  6:pieces 20*N:{binary_sha1_hashes}
  13:keyframe index l{...}e         (optional)
  8:trackers l{...}e               (optional)
  13:creation date i{unixtime}e    (optional)
  7:comment {string}               (optional)
  4:name {filename}                (optional)
  6:format {video_format}          (optional)
e
```

### QvsFile Data Structure

```rust
/// A parsed .qvs seed file describing a playable resource.
///
/// This is the file format used to distribute resource metadata
/// before the advent of qvod:// links. Modern QVOD primarily uses
/// qvod:// URIs, but .qvs files are still supported for backwards
/// compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QvsFile {
    /// 20-byte SHA-1 info hash identifying this resource.
    pub info_hash: InfoHash,

    /// Original filename (from qvs or URI).
    pub name: String,

    /// Total file size in bytes.
    pub file_size: u64,

    /// Size of each piece in bytes (default: 262144).
    pub piece_length: u64,

    /// SHA-1 hash for each piece, in file order.
    pub pieces: Vec<[u8; 20]>,

    /// Optional keyframe index for frame-accurate seeking.
    pub keyframe_index: Option<KeyFrameIndex>,

    /// List of HTTP tracker URLs for peer discovery.
    pub trackers: Vec<String>,

    /// Unix timestamp when this .qvs file was created.
    pub creation_date: u64,

    /// Optional human-readable comment.
    pub comment: Option<String>,

    /// Video format string (e.g., "rmvb", "mp4", "avi").
    pub format: Option<String>,

    /// Any extra keys not explicitly parsed (for forward compatibility).
    pub extra: BTreeMap<String, BencodeValue>,
}

impl QvsFile {
    /// Encode this QvsFile to Bencode bytes.
    pub fn encode(&self) -> Result<Vec<u8>, StorageError> {
        let mut dict = BTreeMap::new();

        dict.insert("info_hash".to_string(), BencodeValue::String(
            self.info_hash.to_hex().into_bytes()
        ));
        dict.insert("name".to_string(), BencodeValue::String(
            self.name.as_bytes().to_vec()
        ));
        dict.insert("length".to_string(), BencodeValue::Integer(self.file_size as i64));
        dict.insert("piece length".to_string(), BencodeValue::Integer(self.piece_length as i64));

        // Flatten piece hashes into a single byte string
        let pieces_concat: Vec<u8> = self.pieces.iter().flat_map(|h| h.to_vec()).collect();
        dict.insert("pieces".to_string(), BencodeValue::String(pieces_concat));

        // Optional keyframe index
        if let Some(ref kf) = self.keyframe_index {
            let entries: Vec<BencodeValue> = kf.entries.iter().map(|e| {
                let mut ed = BTreeMap::new();
                ed.insert("timestamp_ms".to_string(), BencodeValue::Integer(e.timestamp_ms as i64));
                ed.insert("file_offset".to_string(), BencodeValue::Integer(e.file_offset as i64));
                ed.insert("frame_size".to_string(), BencodeValue::Integer(e.frame_size as i64));
                let ft = match e.frame_type {
                    FrameType::I => "I",
                    FrameType::P => "P",
                    FrameType::B => "B",
                };
                ed.insert("frame_type".to_string(), BencodeValue::String(ft.as_bytes().to_vec()));
                BencodeValue::Dictionary(ed)
            }).collect();
            dict.insert("keyframe index".to_string(), BencodeValue::List(entries));
        }

        // Trackers
        if !self.trackers.is_empty() {
            let tracker_list: Vec<BencodeValue> = self.trackers.iter()
                .map(|t| BencodeValue::String(t.as_bytes().to_vec()))
                .collect();
            dict.insert("trackers".to_string(), BencodeValue::List(tracker_list));
        }

        if self.creation_date > 0 {
            dict.insert("creation date".to_string(), BencodeValue::Integer(self.creation_date as i64));
        }

        if let Some(ref comment) = self.comment {
            dict.insert("comment".to_string(), BencodeValue::String(comment.as_bytes().to_vec()));
        }

        if let Some(ref format) = self.format {
            dict.insert("format".to_string(), BencodeValue::String(format.as_bytes().to_vec()));
        }

        // Preserve extra keys
        for (k, v) in &self.extra {
            dict.insert(k.clone(), v.clone());
        }

        let root = BencodeValue::Dictionary(dict);
        Ok(root.encode())
    }

    /// Decode a QvsFile from Bencode-encoded bytes.
    pub fn decode(data: &[u8]) -> Result<Self, StorageError> {
        let value = BencodeValue::decode(data)
            .map_err(|e| StorageError::Bencode(format!("Failed to decode .qvs file: {}", e)))?;

        let dict = match &value {
            BencodeValue::Dictionary(d) => d,
            _ => return Err(StorageError::Bencode("Expected dictionary at root".into())),
        };

        // Helper to get bytes from a dict key
        let get_bytes = |key: &str| -> Option<&[u8]> {
            dict.get(key).and_then(|v| match v {
                BencodeValue::String(b) => Some(b.as_slice()),
                _ => None,
            })
        };

        let get_int = |key: &str| -> Option<i64> {
            dict.get(key).and_then(|v| match v {
                BencodeValue::Integer(i) => Some(*i),
                _ => None,
            })
        };

        // Parse info_hash (40-char hex string -> 20 bytes)
        let info_hash_bytes = get_bytes("info_hash")
            .ok_or_else(|| StorageError::MissingField("info_hash"))?;
        let info_hash_str = std::str::from_utf8(info_hash_bytes)
            .map_err(|_| StorageError::InvalidField("info_hash"))?;
        let info_hash = InfoHash::from_hex(info_hash_str)
            .map_err(|_| StorageError::InvalidField("info_hash"))?;

        let name = String::from_utf8(
            get_bytes("name").ok_or_else(|| StorageError::MissingField("name"))?.to_vec()
        ).map_err(|_| StorageError::InvalidField("name"))?;

        let file_size = get_int("length")
            .ok_or_else(|| StorageError::MissingField("length"))? as u64;
        let piece_length = get_int("piece length")
            .ok_or_else(|| StorageError::MissingField("piece length"))? as u64;

        // Parse pieces (concatenated 20-byte SHA-1 hashes)
        let pieces_bytes = get_bytes("pieces")
            .ok_or_else(|| StorageError::MissingField("pieces"))?;
        if pieces_bytes.len() % 20 != 0 {
            return Err(StorageError::InvalidField("pieces"));
        }
        let pieces: Vec<[u8; 20]> = pieces_bytes
            .chunks_exact(20)
            .map(|chunk| {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(chunk);
                arr
            })
            .collect();

        // Optional keyframe index
        let keyframe_index = dict.get("keyframe index").and_then(|v| {
            if let BencodeValue::List(list) = v {
                let mut entries = Vec::new();
                for item in list {
                    if let BencodeValue::Dictionary(ed) = item {
                        let get_entry_int = |key: &str| -> Option<i64> {
                            ed.get(key).and_then(|ev| match ev {
                                BencodeValue::Integer(i) => Some(*i),
                                _ => None,
                            })
                        };
                        let get_entry_str = |key: &str| -> Option<&[u8]> {
                            ed.get(key).and_then(|ev| match ev {
                                BencodeValue::String(s) => Some(s.as_slice()),
                                _ => None,
                            })
                        };

                        let timestamp_ms = get_entry_int("timestamp_ms")? as u64;
                        let file_offset = get_entry_int("file_offset")? as u64;
                        let frame_size = get_entry_int("frame_size")? as u32;
                        let ft_str = std::str::from_utf8(get_entry_str("frame_type")?).ok()?;
                        let frame_type = match ft_str {
                            "I" => FrameType::I,
                            "P" => FrameType::P,
                            "B" => FrameType::B,
                            _ => return None,
                        };
                        entries.push(KeyFrameEntry {
                            timestamp_ms,
                            file_offset,
                            frame_size,
                            frame_type,
                        });
                    }
                }
                KeyFrameIndex::new(entries).ok()
            } else {
                None
            }
        });

        // Optional trackers
        let trackers = dict.get("trackers").and_then(|v| {
            if let BencodeValue::List(list) = v {
                let urls: Vec<String> = list.iter()
                    .filter_map(|t| {
                        if let BencodeValue::String(b) = t {
                            String::from_utf8(b.clone()).ok()
                        } else {
                            None
                        }
                    })
                    .collect();
                Some(urls)
            } else {
                None
            }
        }).unwrap_or_default();

        let creation_date = get_int("creation date").unwrap_or(0) as u64;

        let comment = get_bytes("comment")
            .and_then(|b| String::from_utf8(b.to_vec()).ok());

        let format = get_bytes("format")
            .and_then(|b| String::from_utf8(b.to_vec()).ok());

        // Collect extra keys (everything not explicitly parsed)
        let known_keys: std::collections::HashSet<&str> = [
            "info_hash", "name", "length", "piece length", "pieces",
            "keyframe index", "trackers", "creation date", "comment", "format",
        ].into();
        let mut extra = BTreeMap::new();
        for (k, v) in dict.iter() {
            if !known_keys.contains(k.as_str()) {
                extra.insert(k.clone(), v.clone());
            }
        }

        Ok(Self {
            info_hash,
            name,
            file_size,
            piece_length,
            pieces,
            keyframe_index,
            trackers,
            creation_date,
            comment,
            format,
            extra,
        })
    }

    /// Save this .qvs file to disk atomically.
    pub fn save_to(&self, path: &Path) -> Result<(), StorageError> {
        let encoded = self.encode()?;
        let tmp_path = path.with_extension("qvs.tmp");
        std::fs::write(&tmp_path, &encoded)
            .map_err(|e| StorageError::Io(format!("Failed to write .qvs: {}", e)))?;
        std::fs::rename(&tmp_path, path)
            .map_err(|e| StorageError::Io(format!("Failed to rename .qvs: {}", e)))?;
        Ok(())
    }

    /// Load a .qvs file from disk.
    pub fn load_from(path: &Path) -> Result<Self, StorageError> {
        let data = std::fs::read(path)
            .map_err(|e| StorageError::Io(format!("Failed to read .qvs: {}", e)))?;
        Self::decode(&data)
    }

    /// Create a QvsFile from a FileMeta (for export).
    pub fn from_meta(meta: &FileMeta, trackers: Vec<String>) -> Self {
        Self {
            info_hash: meta.info_hash,
            name: meta.filename.clone(),
            file_size: meta.file_size,
            piece_length: meta.piece_length,
            pieces: meta.piece_hashes.iter().map(|h| h.0).collect(),
            keyframe_index: Some(meta.keyframe_index.clone()),
            trackers,
            creation_date: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            comment: None,
            format: Some(meta.codec.video_codec.clone()),
            extra: BTreeMap::new(),
        }
    }
}
```

## 2. qvod:// URI Parsing

### URI Format

```
qvod://{info_hash_hex}|{filename}|{filesize}|{format}|
```

### Examples

```
qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|movie.mp4|734003200|rmvb|

qvod://deadbeef0123456789abcdef0123456789abcdef|episode01.mkv|1572864000|mkv|

qvod://0000000000000000000000000000000000000000|test.avi|1048576|avi|
```

### URI Parser

```rust
/// A parsed qvod:// URI.
///
/// # Format
/// `qvod://{info_hash_hex}|{filename}|{filesize}|{format}|`
///
/// # Fields
/// - info_hash_hex: 40-character lowercase hex string (20 bytes SHA-1)
/// - filename:       URL-encoded filename
/// - filesize:       File size in bytes (decimal)
/// - format:         Video format extension (rmvb, avi, mkv, mp4, etc.)
///
/// The URI must end with a pipe character `|`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QvodUri {
    /// Parsed 20-byte info hash.
    info_hash: InfoHash,

    /// Original filename (URL-decoded).
    filename: String,

    /// File size in bytes.
    file_size: u64,

    /// Video format string.
    format: String,

    /// The original URI string.
    original: String,
}

impl QvodUri {
    /// Scheme prefix for qvod URIs.
    pub const SCHEME: &'static str = "qvod://";

    /// Parse a qvod:// URI string.
    ///
    /// # Validation
    /// - Must start with `qvod://`
    /// - info_hash must be exactly 40 lowercase hex characters
    /// - filesize must be a valid non-negative integer
    /// - Must end with `|`
    /// - Must have exactly 4 pipe-delimited fields after the scheme
    pub fn parse(input: &str) -> Result<Self, StorageError> {
        // Check scheme
        let remaining = input
            .strip_prefix(Self::SCHEME)
            .ok_or_else(|| StorageError::InvalidUri(
                "URI must start with qvod://".into()
            ))?;

        // Split by pipe
        let parts: Vec<&str> = remaining.split('|').collect();

        // Must have exactly 5 parts: [hash, filename, size, format, ""]
        // The trailing | produces an empty string at the end
        if parts.len() != 5 || !parts[4].is_empty() {
            return Err(StorageError::InvalidUri(
                format!("URI must have exactly 4 pipe-delimited fields, got {}", parts.len() - 1)
            ));
        }

        let hash_hex = parts[0];
        let filename_encoded = parts[1];
        let size_str = parts[2];
        let format_str = parts[3];

        // Validate info_hash: exactly 40 hex characters
        if hash_hex.len() != 40 {
            return Err(StorageError::InvalidUri(
                format!("info_hash must be 40 hex chars, got {}", hash_hex.len())
            ));
        }
        if !hash_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StorageError::InvalidUri(
                "info_hash must be hexadecimal".into()
            ));
        }
        let info_hash = InfoHash::from_hex(hash_hex)
            .map_err(|_| StorageError::InvalidUri("Failed to parse info_hash".into()))?;

        // Validate filesize
        let file_size = size_str.parse::<u64>()
            .map_err(|_| StorageError::InvalidUri(
                format!("Invalid file size: '{}'", size_str)
            ))?;

        // URL-decode filename
        let filename = urlencoding::decode(filename_encoded)
            .map_err(|_| StorageError::InvalidUri("Invalid URL encoding in filename".into()))?
            .into_owned();

        // Validate format
        if format_str.is_empty() {
            return Err(StorageError::InvalidUri("Format field must not be empty".into()));
        }
        let valid_formats = ["rmvb", "avi", "mkv", "mp4", "wmv", "flv", "mov", "ts", "webm", "3gp"];
        if !valid_formats.contains(&format_str) {
            // Not an error for unknown formats, just a warning
            tracing::warn!("Unknown video format: '{}'", format_str);
        }

        Ok(Self {
            info_hash,
            filename,
            file_size,
            format: format_str.to_string(),
            original: input.to_string(),
        })
    }

    /// Construct a qvod:// URI from components.
    pub fn build(info_hash: &InfoHash, filename: &str, file_size: u64, format: &str) -> Self {
        let encoded_filename = urlencoding::encode(filename);
        let uri_str = format!(
            "{}|{}|{}|{}|",
            Self::SCHEME,
            info_hash.to_hex(),
            encoded_filename,
            file_size,
            format,
        );

        // Strip the scheme prefix from the format args (it's already in SCHEME)
        let uri_str = format!(
            "qvod://{}|{}|{}|{}|",
            info_hash.to_hex(),
            encoded_filename,
            file_size,
            format,
        );

        Self {
            info_hash: *info_hash,
            filename: filename.to_string(),
            file_size,
            format: format.to_string(),
            original: uri_str,
        }
    }

    /// Get the info hash.
    pub fn info_hash(&self) -> &InfoHash {
        &self.info_hash
    }

    /// Get the filename.
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Get the file size.
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Get the format.
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Get the original URI string.
    pub fn as_str(&self) -> &str {
        &self.original
    }

    /// Serialize back to string.
    pub fn to_string(&self) -> String {
        self.original.clone()
    }
}

// Implement FromStr for ergonomic parsing
impl std::str::FromStr for QvodUri {
    type Err = StorageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

// Display trait for URI display
impl std::fmt::Display for QvodUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.original)
    }
}
```

## 3. Bencode Parser/Generator

```rust
/// A Bencode value, representing the four Bencode types:
/// - Integer: i<number>e
/// - String: <length>:<bytes>
/// - List: l<values>e
/// - Dictionary: d<key-value pairs>e
///
/// Bencode is used for .qvs files, .qmv metadata, tracker responses,
/// and the ut_metadata extension protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BencodeValue {
    /// Bencode integer (supports negative values).
    /// Range: arbitrary precision, but we use i64 for practical purposes.
    Integer(i64),

    /// Bencode byte string (can contain any bytes, not just UTF-8).
    /// Length prefix indicates the number of bytes.
    String(Vec<u8>),

    /// Bencode list (heterogeneous, ordered).
    List(Vec<BencodeValue>),

    /// Bencode dictionary (keys are byte strings, sorted lexicographically).
    Dictionary(BTreeMap<String, BencodeValue>),
}

impl BencodeValue {
    /// Encode this value to Bencode format.
    ///
    /// The encoding is deterministic: dictionary keys are sorted lexicographically,
    /// and integers are encoded in decimal without leading zeros.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.encode_to(&mut buf);
        buf
    }

    fn encode_to(&self, buf: &mut Vec<u8>) {
        match self {
            BencodeValue::Integer(i) => {
                buf.push(b'i');
                buf.extend_from_slice(i.to_string().as_bytes());
                buf.push(b'e');
            }
            BencodeValue::String(s) => {
                buf.extend_from_slice(s.len().to_string().as_bytes());
                buf.push(b':');
                buf.extend_from_slice(s);
            }
            BencodeValue::List(items) => {
                buf.push(b'l');
                for item in items {
                    item.encode_to(buf);
                }
                buf.push(b'e');
            }
            BencodeValue::Dictionary(dict) => {
                buf.push(b'd');
                // Keys must be sorted lexicographically (Bencode spec)
                for (key, value) in dict.iter() {
                    // Encode key as string
                    buf.extend_from_slice(key.len().to_string().as_bytes());
                    buf.push(b':');
                    buf.extend_from_slice(key.as_bytes());
                    // Encode value
                    value.encode_to(buf);
                }
                buf.push(b'e');
            }
        }
    }

    /// Decode a Bencode value from bytes.
    /// Returns the decoded value and any remaining unconsumed bytes.
    pub fn decode(data: &[u8]) -> Result<(Self, &[u8]), StorageError> {
        if data.is_empty() {
            return Err(StorageError::Bencode("Empty input".into()));
        }

        match data[0] {
            b'i' => Self::decode_integer(data),
            b'l' => Self::decode_list(data),
            b'd' => Self::decode_dict(data),
            b'0'..=b'9' => Self::decode_string(data),
            _ => Err(StorageError::Bencode(
                format!("Unexpected byte 0x{:02x}, expected i, l, d, or digit", data[0])
            )),
        }
    }

    fn decode_integer(data: &[u8]) -> Result<(Self, &[u8]), StorageError> {
        let end = data.iter().position(|&b| b == b'e')
            .ok_or_else(|| StorageError::Bencode("Unterminated integer".into()))?;

        let num_str = std::str::from_utf8(&data[1..end])
            .map_err(|_| StorageError::Bencode("Invalid UTF-8 in integer".into()))?;

        // Validate: no leading zeros except for the number zero itself
        if num_str.len() > 1 && num_str.starts_with('0') {
            return Err(StorageError::Bencode("Integer has leading zero".into()));
        }
        // Validate: negative zero is not allowed
        if num_str == "-0" {
            return Err(StorageError::Bencode("Negative zero not allowed".into()));
        }

        let value: i64 = num_str.parse()
            .map_err(|_| StorageError::Bencode(format!("Invalid integer: '{}'", num_str)))?;

        Ok((BencodeValue::Integer(value), &data[end + 1..]))
    }

    fn decode_string(data: &[u8]) -> Result<(Self, &[u8]), StorageError> {
        let colon = data.iter().position(|&b| b == b':')
            .ok_or_else(|| StorageError::Bencode("Unterminated string length".into()))?;

        let len_str = std::str::from_utf8(&data[..colon])
            .map_err(|_| StorageError::Bencode("Invalid UTF-8 in string length".into()))?;

        let len: usize = len_str.parse()
            .map_err(|_| StorageError::Bencode(format!("Invalid string length: '{}'", len_str)))?;

        // Validate: no leading zeros
        if len_str.len() > 1 && len_str.starts_with('0') {
            return Err(StorageError::Bencode("String length has leading zero".into()));
        }

        let start = colon + 1;
        if start + len > data.len() {
            return Err(StorageError::Bencode(
                format!("String length {} exceeds remaining data {} bytes", len, data.len() - start)
            ));
        }

        let bytes = data[start..start + len].to_vec();
        Ok((BencodeValue::String(bytes), &data[start + len..]))
    }

    fn decode_list(data: &[u8]) -> Result<(Self, &[u8]), StorageError> {
        let mut remaining = &data[1..]; // skip 'l'
        let mut items = Vec::new();

        while !remaining.is_empty() && remaining[0] != b'e' {
            let (item, rest) = Self::decode(remaining)?;
            items.push(item);
            remaining = rest;
        }

        if remaining.is_empty() {
            return Err(StorageError::Bencode("Unterminated list".into()));
        }

        Ok((BencodeValue::List(items), &remaining[1..])) // skip 'e'
    }

    fn decode_dict(data: &[u8]) -> Result<(Self, &[u8]), StorageError> {
        let mut remaining = &data[1..]; // skip 'd'
        let mut dict = BTreeMap::new();

        while !remaining.is_empty() && remaining[0] != b'e' {
            // Key must be a string
            let (key_value, rest) = Self::decode(remaining)?;
            let key_bytes = match &key_value {
                BencodeValue::String(b) => b.clone(),
                _ => return Err(StorageError::Bencode("Dictionary key must be a string".into())),
            };
            let key = String::from_utf8(key_bytes)
                .map_err(|_| StorageError::Bencode("Dictionary key is not valid UTF-8".into()))?;

            // Value
            let (value, rest) = Self::decode(rest)?;
            dict.insert(key, value);
            remaining = rest;
        }

        if remaining.is_empty() {
            return Err(StorageError::Bencode("Unterminated dictionary".into()));
        }

        Ok((BencodeValue::Dictionary(dict), &remaining[1..])) // skip 'e'
    }

    // Convenience accessors
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            BencodeValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&[u8]> {
        match self {
            BencodeValue::String(s) => Some(s.as_slice()),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[BencodeValue]> {
        match self {
            BencodeValue::List(l) => Some(l.as_slice()),
            _ => None,
        }
    }

    pub fn as_dict(&self) -> Option<&BTreeMap<String, BencodeValue>> {
        match self {
            BencodeValue::Dictionary(d) => Some(d),
            _ => None,
        }
    }
}

/// Convenience alias for Bencode dictionary type.
pub type BencodeDict = BTreeMap<String, BencodeValue>;

/// Extension trait for BencodeDict with convenient accessors.
pub trait DictExt {
    fn get_int(&self, key: &str) -> Option<i64>;
    fn get_str(&self, key: &str) -> Option<&[u8]>;
    fn get_list(&self, key: &str) -> Option<&[BencodeValue]>;
    fn get_dict(&self, key: &str) -> Option<&BTreeMap<String, BencodeValue>>;
    fn get_string_utf8(&self, key: &str) -> Option<String>;
}

impl DictExt for BTreeMap<String, BencodeValue> {
    fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_integer())
    }

    fn get_str(&self, key: &str) -> Option<&[u8]> {
        self.get(key).and_then(|v| v.as_string())
    }

    fn get_list(&self, key: &str) -> Option<&[BencodeValue]> {
        self.get(key).and_then(|v| v.as_list())
    }

    fn get_dict(&self, key: &str) -> Option<&BTreeMap<String, BencodeValue>> {
        self.get(key).and_then(|v| v.as_dict())
    }

    fn get_string_utf8(&self, key: &str) -> Option<String> {
        self.get_str(key)
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
    }
}
```

## 4. Persistent Configuration

```rust
/// QVOD engine configuration, persisted as TOML.
///
/// Configuration file location (by platform):
/// - Linux:   ~/.config/qvod/config.toml
/// - macOS:   ~/Library/Application Support/com.qvod/qvs/config.toml
/// - Windows: C:\Users\{user}\AppData\Roaming\Qvod\config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    // Network
    pub listen_port: u16,
    pub udp_port: u16,
    pub max_connections: u32,
    pub http_fallback: bool,

    // Cache
    pub cache_dir: PathBuf,
    pub cache_max_size_mb: u64,
    pub buffer_capacity_mb: u32,

    // Tracker / DHT
    pub tracker_urls: Vec<String>,
    pub dht_seed_nodes: Vec<String>,
    pub dht_port: u16,

    // Media
    pub preferred_video_codec: Option<String>,
    pub preferred_audio_codec: Option<String>,

    // UI
    pub language: String,
    pub theme: String, // "dark", "light", "system"
    pub window_width: u32,
    pub window_height: u32,

    // Advanced
    pub max_upload_rate_kbps: u32,
    pub max_download_rate_kbps: u32,
    pub enable_upnp: bool,
    pub enable_udp_transport: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            listen_port: 8621,
            udp_port: 8622,
            max_connections: 50,
            http_fallback: true,
            cache_dir: default_cache_dir(),
            cache_max_size_mb: 4096, // 4 GB
            buffer_capacity_mb: 64,
            tracker_urls: vec![
                "http://tracker.qvod.com:6969/announce".into(),
                "http://tracker.opentrackr.org:1337/announce".into(),
            ],
            dht_seed_nodes: vec![
                "router.bittorrent.com:6881".into(),
                "dht.transmissionbt.com:6881".into(),
            ],
            dht_port: 8623,
            preferred_video_codec: None,
            preferred_audio_codec: None,
            language: "zh-CN".into(),
            theme: "system".into(),
            window_width: 1024,
            window_height: 768,
            max_upload_rate_kbps: 0, // 0 = unlimited
            max_download_rate_kbps: 0,
            enable_upnp: true,
            enable_udp_transport: true,
        }
    }
}

impl EngineConfig {
    /// Path to the configuration file.
    pub fn config_path() -> Result<PathBuf, StorageError> {
        let base = dirs::config_dir()
            .ok_or_else(|| StorageError::Io("Cannot determine config directory".into()))?;
        Ok(base.join("qvod").join("config.toml"))
    }

    /// Load configuration from the default path.
    /// Returns default config if the file doesn't exist.
    pub fn load() -> Result<Self, StorageError> {
        let path = Self::config_path()?;
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| StorageError::Io(format!("Failed to read config: {}", e)))?;
            toml::from_str(&content)
                .map_err(|e| StorageError::Config(format!("Failed to parse config: {}", e)))
        } else {
            let config = Self::default();
            config.save()?; // Save defaults
            Ok(config)
        }
    }

    /// Save configuration to the default path atomically.
    pub fn save(&self) -> Result<(), StorageError> {
        let path = Self::config_path()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| StorageError::Io(format!("Failed to create config dir: {}", e)))?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| StorageError::Config(format!("Failed to serialize config: {}", e)))?;

        // Atomic write: write to temp, then rename
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, &content)
            .map_err(|e| StorageError::Io(format!("Failed to write config: {}", e)))?;
        std::fs::rename(&tmp_path, &path)
            .map_err(|e| StorageError::Io(format!("Failed to rename config: {}", e)))?;

        Ok(())
    }

    /// Validate the configuration values.
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.listen_port == 0 || self.listen_port > 65535 {
            return Err(StorageError::Config("Invalid listen_port".into()));
        }
        if self.udp_port == 0 || self.udp_port > 65535 {
            return Err(StorageError::Config("Invalid udp_port".into()));
        }
        if self.max_connections == 0 || self.max_connections > 200 {
            return Err(StorageError::Config("max_connections must be 1-200".into()));
        }
        if self.cache_max_size_mb < 100 {
            return Err(StorageError::Config("cache_max_size_mb must be >= 100".into()));
        }
        if self.buffer_capacity_mb < 16 || self.buffer_capacity_mb > 512 {
            return Err(StorageError::Config("buffer_capacity_mb must be 16-512".into()));
        }
        Ok(())
    }
}

/// Determine the platform-appropriate default cache directory.
fn default_cache_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("qvod")
            .join("cache")
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("com.qvod.qvs")
            .join("cache")
    }
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\Qvod"))
            .join("cache")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        PathBuf::from("/tmp/qvod/cache")
    }
}
```

## 5. Download State Persistence

```rust
/// Persistent download state for resume support.
///
/// When a download is interrupted (user closes app, crash, etc.),
/// this state allows the engine to resume from where it left off
/// without re-downloading verified pieces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    /// Info hash of the resource.
    pub info_hash: InfoHash,

    /// Piece length in bytes.
    pub piece_length: u64,

    /// Total pieces in the file.
    pub total_pieces: u32,

    /// Bitfield of completed pieces (hex-encoded for serialization).
    pub bitfield_hex: String,

    /// Timestamp when download started.
    pub started_at: u64,

    /// Timestamp of last update.
    pub updated_at: u64,

    /// Total bytes downloaded.
    pub bytes_downloaded: u64,

    /// Number of verification failures.
    pub verification_failures: u32,

    /// Status of the download.
    pub status: DownloadStatus,

    /// Per-peer statistics (anonymized).
    pub peer_stats: Vec<PeerDownloadStat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    /// Download is in progress.
    Downloading,
    /// All pieces verified and complete.
    Completed,
    /// Download was paused by user.
    Paused,
    /// Download failed with non-recoverable error.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDownloadStat {
    pub peer_id: String,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub pieces_received: u32,
    pub pieces_sent: u32,
    pub avg_speed_kbps: f64,
}

impl DownloadState {
    /// Create a new download state for an info hash.
    pub fn new(info_hash: InfoHash, total_pieces: u32, piece_length: u64) -> Self {
        let bitfield = Bitfield::new(total_pieces);
        Self {
            info_hash,
            piece_length,
            total_pieces,
            bitfield_hex: hex::encode(bitfield.to_bytes()),
            started_at: unix_timestamp_secs(),
            updated_at: unix_timestamp_secs(),
            bytes_downloaded: 0,
            verification_failures: 0,
            status: DownloadStatus::Downloading,
            peer_stats: Vec::new(),
        }
    }

    /// Update the bitfield with a newly completed piece.
    pub fn mark_piece_completed(&mut self, piece_index: u32) {
        let mut bitfield = Bitfield::from_bytes(
            &hex::decode(&self.bitfield_hex).unwrap_or_default()
        );
        bitfield.set(piece_index, true);
        self.bitfield_hex = hex::encode(bitfield.to_bytes());
        self.bytes_downloaded += self.piece_length;
        self.updated_at = unix_timestamp_secs();
    }

    /// Get the completion ratio.
    pub fn completion(&self) -> f64 {
        let total_bits = self.total_pieces as f64;
        if total_bits == 0.0 {
            return 0.0;
        }
        let bitfield = Bitfield::from_bytes(
            &hex::decode(&self.bitfield_hex).unwrap_or_default()
        );
        bitfield.count() as f64 / total_bits
    }

    /// Get the reconstructed Bitfield.
    pub fn bitfield(&self) -> Bitfield {
        Bitfield::from_bytes(
            &hex::decode(&self.bitfield_hex).unwrap_or_default()
        )
    }
}

/// Persistence manager for download states.
pub struct DownloadStateStore {
    /// Directory where state files are stored.
    state_dir: PathBuf,
}

impl DownloadStateStore {
    /// Create a new download state store.
    pub fn new(base_dir: &Path) -> Self {
        let state_dir = base_dir.join("state");
        std::fs::create_dir_all(&state_dir).ok();
        Self { state_dir }
    }

    /// Path to a download state file.
    fn state_path(&self, info_hash: &InfoHash) -> PathBuf {
        self.state_dir.join(format!("{}.state", info_hash.to_hex()))
    }

    /// Save download state for a resource.
    pub fn save(&self, state: &DownloadState) -> Result<(), StorageError> {
        let path = self.state_path(&state.info_hash);
        let content = serde_json::to_string_pretty(state)
            .map_err(|e| StorageError::Io(format!("Failed to serialize state: {}", e)))?;

        // Atomic write
        let tmp_path = path.with_extension("state.tmp");
        std::fs::write(&tmp_path, &content)
            .map_err(|e| StorageError::Io(format!("Failed to write state: {}", e)))?;
        std::fs::rename(&tmp_path, &path)
            .map_err(|e| StorageError::Io(format!("Failed to rename state: {}", e)))?;
        Ok(())
    }

    /// Load download state for a resource.
    pub fn load(&self, info_hash: &InfoHash) -> Result<DownloadState, StorageError> {
        let path = self.state_path(info_hash);
        if !path.exists() {
            return Err(StorageError::NotFound(info_hash.to_hex()));
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| StorageError::Io(format!("Failed to read state: {}", e)))?;
        serde_json::from_str(&content)
            .map_err(|e| StorageError::Io(format!("Failed to parse state: {}", e)))
    }

    /// Delete download state for a resource.
    pub fn delete(&self, info_hash: &InfoHash) -> Result<(), StorageError> {
        let path = self.state_path(info_hash);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| StorageError::Io(format!("Failed to delete state: {}", e)))?;
        }
        Ok(())
    }

    /// List all tracked download states.
    pub fn list(&self) -> Result<Vec<DownloadState>, StorageError> {
        let mut states = Vec::new();
        let mut dir = std::fs::read_dir(&self.state_dir)
            .map_err(|e| StorageError::Io(format!("Failed to list state dir: {}", e)))?;

        while let Some(entry) = dir.next().transpose()
            .map_err(|e| StorageError::Io(format!("Failed to read dir entry: {}", e)))?
        {
            let path = entry.path();
            if path.extension().map(|e| e == "state").unwrap_or(false) {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| StorageError::Io(format!("Failed to read state: {}", e)))?;
                if let Ok(state) = serde_json::from_str(&content) {
                    states.push(state);
                }
            }
        }

        Ok(states)
    }
}

/// Get current unix timestamp in seconds.
fn unix_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
```

## 6. Cross-Platform Path Handling

```rust
/// Platform-specific storage paths for QVOD.
pub struct StoragePaths;

impl StoragePaths {
    /// Root data directory for QVOD.
    pub fn data_dir() -> Result<PathBuf, StorageError> {
        let base = dirs::data_local_dir()
            .ok_or_else(|| StorageError::Io("Cannot determine data directory".into()))?;

        #[cfg(target_os = "linux")]
        { Ok(base.join("qvod")) }

        #[cfg(target_os = "macos")]
        { Ok(base.join("com.qvod.qvs")) }

        #[cfg(windows)]
        { Ok(base.join("Qvod")) }

        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { Ok(base.join("qvod")) }
    }

    /// Cache directory for downloaded data.
    pub fn cache_dir() -> Result<PathBuf, StorageError> {
        Ok(Self::data_dir()?.join("cache"))
    }

    /// Configuration directory.
    pub fn config_dir() -> Result<PathBuf, StorageError> {
        let base = dirs::config_dir()
            .ok_or_else(|| StorageError::Io("Cannot determine config directory".into()))?;

        #[cfg(target_os = "linux")]
        { Ok(base.join("qvod")) }

        #[cfg(target_os = "macos")]
        { Ok(base.join("com.qvod.qvs")) }

        #[cfg(windows)]
        { Ok(base.join("Qvod")) }

        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        { Ok(base.join("qvod")) }
    }

    /// Log file directory.
    pub fn log_dir() -> Result<PathBuf, StorageError> {
        Ok(Self::data_dir()?.join("logs"))
    }

    /// Temporary download directory.
    pub fn temp_dir() -> Result<PathBuf, StorageError> {
        Ok(Self::data_dir()?.join("tmp"))
    }

    /// Ensure all directories exist.
    pub fn ensure_all() -> Result<(), StorageError> {
        std::fs::create_dir_all(Self::data_dir()?)
            .map_err(|e| StorageError::Io(format!("Failed to create data dir: {}", e)))?;
        std::fs::create_dir_all(Self::cache_dir()?)
            .map_err(|e| StorageError::Io(format!("Failed to create cache dir: {}", e)))?;
        std::fs::create_dir_all(Self::config_dir()?)
            .map_err(|e| StorageError::Io(format!("Failed to create config dir: {}", e)))?;
        std::fs::create_dir_all(Self::log_dir()?)
            .map_err(|e| StorageError::Io(format!("Failed to create log dir: {}", e)))?;
        std::fs::create_dir_all(Self::temp_dir()?)
            .map_err(|e| StorageError::Io(format!("Failed to create temp dir: {}", e)))?;
        Ok(())
    }
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Bencode error: {0}")]
    Bencode(String),

    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[error("Invalid field: {0}")]
    InvalidField(&'static str),

    #[error("Invalid URI: {0}")]
    InvalidUri(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),
}
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ─── Bencode Tests ───

    #[test]
    fn test_bencode_integer_roundtrip() {
        let v = BencodeValue::Integer(42);
        let encoded = v.encode();
        let (decoded, rest) = BencodeValue::decode(&encoded).unwrap();
        assert!(rest.is_empty());
        assert_eq!(decoded.as_integer(), Some(42));
    }

    #[test]
    fn test_bencode_negative_integer() {
        let v = BencodeValue::Integer(-42);
        let encoded = v.encode();
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();
        assert_eq!(decoded.as_integer(), Some(-42));
    }

    #[test]
    fn test_bencode_string_roundtrip() {
        let v = BencodeValue::String(b"hello world".to_vec());
        let encoded = v.encode();
        let (decoded, rest) = BencodeValue::decode(&encoded).unwrap();
        assert!(rest.is_empty());
        assert_eq!(decoded.as_string(), Some(&b"hello world"[..]));
    }

    #[test]
    fn test_bencode_empty_string() {
        let v = BencodeValue::String(vec![]);
        let encoded = v.encode();
        assert_eq!(encoded, b"0:");
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();
        assert_eq!(decoded.as_string(), Some(&b""[..]));
    }

    #[test]
    fn test_bencode_list_roundtrip() {
        let v = BencodeValue::List(vec![
            BencodeValue::Integer(1),
            BencodeValue::String(b"two".to_vec()),
            BencodeValue::Integer(3),
        ]);
        let encoded = v.encode();
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();
        let list = decoded.as_list().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].as_integer(), Some(1));
        assert_eq!(list[1].as_string(), Some(&b"two"[..]));
        assert_eq!(list[2].as_integer(), Some(3));
    }

    #[test]
    fn test_bencode_dict_roundtrip() {
        let mut dict = BTreeMap::new();
        dict.insert("name".into(), BencodeValue::String(b"test".to_vec()));
        dict.insert("size".into(), BencodeValue::Integer(100));
        let v = BencodeValue::Dictionary(dict);

        let encoded = v.encode();
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();

        let d = decoded.as_dict().unwrap();
        assert_eq!(d.get_string_utf8("name"), Some("test".into()));
        assert_eq!(d.get_int("size"), Some(100));
    }

    #[test]
    fn test_bencode_nested_dict_list() {
        let mut inner = BTreeMap::new();
        inner.insert("key".into(), BencodeValue::String(b"val".to_vec()));
        let v = BencodeValue::List(vec![
            BencodeValue::Dictionary(inner),
        ]);
        let encoded = v.encode();
        let (decoded, _) = BencodeValue::decode(&encoded).unwrap();
        assert!(decoded.as_list().is_some());
    }

    #[test]
    fn test_bencode_invalid_leading_zero() {
        let result = BencodeValue::decode(b"i01e");
        assert!(result.is_err());
    }

    #[test]
    fn test_bencode_unterminated() {
        let result = BencodeValue::decode(b"i42");
        assert!(result.is_err());
    }

    #[test]
    fn test_bencode_dict_keys_sorted() {
        let mut dict = BTreeMap::new();
        dict.insert("z".into(), BencodeValue::Integer(1));
        dict.insert("a".into(), BencodeValue::Integer(2));
        dict.insert("m".into(), BencodeValue::Integer(3));
        let v = BencodeValue::Dictionary(dict);
        let encoded = v.encode();
        let encoded_str = String::from_utf8_lossy(&encoded);
        // Keys should appear in order: a, m, z
        assert!(encoded_str.contains("1:a"));
        assert!(encoded_str.contains("1:m"));
        assert!(encoded_str.contains("1:z"));
        // Verify ordering
        let a_pos = encoded_str.find("1:a").unwrap();
        let m_pos = encoded_str.find("1:m").unwrap();
        let z_pos = encoded_str.find("1:z").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    // ─── URI Tests ───

    #[test]
    fn test_uri_parse_valid() {
        let uri = QvodUri::parse("qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|movie.mp4|734003200|rmvb|").unwrap();
        assert_eq!(uri.info_hash().to_hex(), "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0");
        assert_eq!(uri.filename(), "movie.mp4");
        assert_eq!(uri.file_size(), 734003200);
        assert_eq!(uri.format(), "rmvb");
    }

    #[test]
    fn test_uri_parse_url_encoded_filename() {
        let uri = QvodUri::parse("qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|my%20movie.mp4|1024|mp4|").unwrap();
        assert_eq!(uri.filename(), "my movie.mp4");
    }

    #[test]
    fn test_uri_parse_invalid_scheme() {
        let result = QvodUri::parse("http://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_parse_invalid_hash_length() {
        let result = QvodUri::parse("qvod://short|file.mp4|100|mp4|");
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_parse_invalid_hash_chars() {
        let result = QvodUri::parse("qvod://zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz|file.mp4|100|mp4|");
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_parse_invalid_filesize() {
        let result = QvodUri::parse("qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|file.mp4|abc|mp4|");
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_parse_missing_trailing_pipe() {
        let result = QvodUri::parse("qvod://a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0|file.mp4|100|mp4");
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_build_roundtrip() {
        let info_hash = InfoHash([0xAB; 20]);
        let uri = QvodUri::build(&info_hash, "test.mp4", 1000, "mp4");
        let parsed = QvodUri::parse(uri.as_str()).unwrap();
        assert_eq!(parsed.info_hash(), &info_hash);
        assert_eq!(parsed.filename(), "test.mp4");
        assert_eq!(parsed.file_size(), 1000);
        assert_eq!(parsed.format(), "mp4");
    }

    #[test]
    fn test_uri_display() {
        let info_hash = InfoHash([0x00; 20]);
        let uri = QvodUri::build(&info_hash, "f.mkv", 500, "mkv");
        let s = uri.to_string();
        assert!(s.starts_with("qvod://"));
        assert!(s.ends_with("|"));
        assert!(s.contains("f.mkv"));
    }

    // ─── QVS File Tests ───

    fn sample_qvs() -> QvsFile {
        QvsFile {
            info_hash: InfoHash([0x01; 20]),
            name: "test.mp4".into(),
            file_size: 262144 * 4, // 4 pieces
            piece_length: 262144,
            pieces: vec![[0x02; 20]; 4],
            keyframe_index: None,
            trackers: vec!["http://tracker.example.com/announce".into()],
            creation_date: 1700000000,
            comment: Some("test comment".into()),
            format: Some("mp4".into()),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn test_qvs_roundtrip() {
        let qvs = sample_qvs();
        let encoded = qvs.encode().unwrap();
        let decoded = QvsFile::decode(&encoded).unwrap();

        assert_eq!(decoded.info_hash, qvs.info_hash);
        assert_eq!(decoded.name, qvs.name);
        assert_eq!(decoded.file_size, qvs.file_size);
        assert_eq!(decoded.piece_length, qvs.piece_length);
        assert_eq!(decoded.pieces.len(), qvs.pieces.len());
        assert_eq!(decoded.trackers, qvs.trackers);
        assert_eq!(decoded.creation_date, qvs.creation_date);
        assert_eq!(decoded.comment, qvs.comment);
        assert_eq!(decoded.format, qvs.format);
    }

    #[test]
    fn test_qvs_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.qvs");
        let qvs = sample_qvs();

        qvs.save_to(&path).unwrap();
        let loaded = QvsFile::load_from(&path).unwrap();
        assert_eq!(loaded.info_hash, qvs.info_hash);
        assert_eq!(loaded.name, qvs.name);

        // Clean up
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_qvs_from_meta() {
        let meta = create_dummy_meta();
        let trackers = vec!["http://t.com/announce".into()];
        let qvs = QvsFile::from_meta(&meta, trackers.clone());

        assert_eq!(qvs.info_hash, meta.info_hash);
        assert_eq!(qvs.name, meta.filename);
        assert_eq!(qvs.file_size, meta.file_size);
        assert_eq!(qvs.piece_length, meta.piece_length);
        assert_eq!(qvs.pieces.len(), meta.piece_hashes.len());
        assert_eq!(qvs.trackers, trackers);
        assert!(qvs.keyframe_index.is_some());
    }

    // ─── Config Tests ───

    #[test]
    fn test_config_default() {
        let config = EngineConfig::default();
        assert_eq!(config.listen_port, 8621);
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.cache_max_size_mb, 4096);
        assert!(config.http_fallback);
    }

    #[test]
    fn test_config_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let config = EngineConfig {
            cache_dir: dir.path().join("cache"),
            listen_port: 9000,
            max_connections: 100,
            ..Default::default()
        };

        // Write to temp path manually
        let content = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &content).unwrap();

        // Read back
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: EngineConfig = toml::from_str(&content).unwrap();

        assert_eq!(loaded.listen_port, 9000);
        assert_eq!(loaded.max_connections, 100);
        assert_eq!(loaded.cache_dir, config.cache_dir);
    }

    #[test]
    fn test_config_validate() {
        let mut config = EngineConfig::default();

        // Valid config
        assert!(config.validate().is_ok());

        // Invalid port
        config.listen_port = 0;
        assert!(config.validate().is_err());
        config.listen_port = 8621;

        // Invalid max_connections
        config.max_connections = 0;
        assert!(config.validate().is_err());
        config.max_connections = 50;

        // Invalid cache size
        config.cache_max_size_mb = 50;
        assert!(config.validate().is_err());
    }

    // ─── Download State Tests ───

    #[test]
    fn test_download_state() {
        let info_hash = InfoHash([0xAA; 20]);
        let mut state = DownloadState::new(info_hash, 100, 262144);

        assert_eq!(state.status, DownloadStatus::Downloading);
        assert!(state.completion() < 0.01);

        state.mark_piece_completed(0);
        assert!((state.completion() - 0.01).abs() < 0.01);
        assert_eq!(state.bytes_downloaded, 262144);

        state.status = DownloadStatus::Completed;
        assert_eq!(state.status, DownloadStatus::Completed);
    }

    #[test]
    fn test_download_state_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = DownloadStateStore::new(dir.path());

        let info_hash = InfoHash([0xBB; 20]);
        let state = DownloadState::new(info_hash, 10, 262144);

        store.save(&state).unwrap();
        let loaded = store.load(&info_hash).unwrap();
        assert_eq!(loaded.info_hash, info_hash);
        assert_eq!(loaded.total_pieces, 10);

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);

        store.delete(&info_hash).unwrap();
        assert!(store.load(&info_hash).is_err());
    }

    // ─── Storage Paths Tests ───

    #[test]
    fn test_storage_paths() {
        assert!(StoragePaths::data_dir().is_ok());
        assert!(StoragePaths::cache_dir().is_ok());
        assert!(StoragePaths::config_dir().is_ok());
        assert!(StoragePaths::log_dir().is_ok());
        assert!(StoragePaths::temp_dir().is_ok());
    }

    // ─── Helpers ───

    fn create_dummy_meta() -> FileMeta {
        let entries = vec![
            KeyFrameEntry {
                timestamp_ms: 0,
                file_offset: 0,
                frame_size: 48000,
                frame_type: FrameType::I,
            },
        ];
        FileMeta {
            info_hash: InfoHash([0xCC; 20]),
            filename: "dummy.mp4".into(),
            file_size: 262144 * 4,
            piece_length: 262144,
            piece_hashes: vec![PieceHash([0xDD; 20]); 4],
            keyframe_index: KeyFrameIndex::new(entries).unwrap(),
            duration_ms: 10000,
            codec: CodecInfo {
                video_codec: "avc1".into(),
                audio_codec: "aac".into(),
                width: 1920,
                height: 1080,
                bitrate: 5_000_000,
                ..Default::default()
            },
            from_cache: false,
        }
    }
}
```

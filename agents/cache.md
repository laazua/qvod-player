# Cache System Module Specification

## Overview

The Cache System provides **persistent local storage** for downloaded video data. QVOD uses a two-file strategy per resource:

1. **`.qdata`** — Sparse file containing raw piece data (the "data file")
2. **`.qmv`** — Bencode-encoded `FileMeta` (the "metadata file")

This design allows QVOD to resume partial downloads across sessions, re-use cached data for repeated plays of the same resource, and serve cached content directly without network access.

### Design Goals

- **Resume support** — If a download is interrupted, only the missing pieces need to be fetched
- **Instant replay** — Previously watched videos play from cache without any network activity
- **Sparse storage** — Only downloaded pieces occupy disk space; undownloaded areas use no blocks
- **LRU eviction** — When cache exceeds quota, least-recently-used resources are evicted
- **Thread safety** — Concurrent reads and writes from multiple tokio tasks are safe
- **Cross-platform** — Works on Linux (fallocate), macOS (fpunchhole), and Windows (SetFileValidData)

## Directory Structure

```
{cache_dir}/
├── qdata/                   # Raw video data files (sparse)
│   ├── A1B2C3D4E5...qdata  # 20-byte info_hash → hex filename
│   ├── F0E1D2C3F4...qdata
│   └── ...
├── qmv/                     # Metadata files (Bencode-encoded FileMeta)
│   ├── A1B2C3D4E5...qmv
│   ├── F0E1D2C3F4...qmv
│   └── ...
└── cache.db                 # (Optional) SQLite index for fast lookups
```

### Naming Convention

Both `.qdata` and `.qmv` files use the **lowercase hex encoding** of the 20-byte `InfoHash` as their filename:

```
info_hash = [0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0xA7, 0xB8, 0xC9, 0xD0,
             0xE1, 0xF2, 0xA3, 0xB4, 0xC5, 0xD6, 0xE7, 0xF8, 0xA9, 0xB0]

filename = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0.qdata"
```

## Data Structures

### CacheConfig

```rust
/// Configuration for the cache system.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Root directory for all cache data.
    pub cache_dir: PathBuf,

    /// Maximum cache size in bytes (default: 4 GB).
    pub max_size: u64,

    /// Minimum free disk space required (default: 1 GB).
    /// Cache cleanup runs when free space drops below this.
    pub min_free_space: u64,

    /// Maximum number of cache entries (default: 500).
    pub max_entries: usize,

    /// Whether to use sparse file allocation (default: true).
    pub sparse_files: bool,

    /// Whether to create an SQLite index for faster lookups (default: false).
    pub use_index: bool,

    /// I/O buffer size for read/write operations (default: 64 KB).
    pub io_buffer_size: usize,

    /// Flush interval for metadata writes (default: 5 seconds).
    pub flush_interval: Duration,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("/tmp/qvod_cache"),
            max_size: 4 * 1024 * 1024 * 1024, // 4 GB
            min_free_space: 1 * 1024 * 1024 * 1024, // 1 GB
            max_entries: 500,
            sparse_files: true,
            use_index: false,
            io_buffer_size: 65536, // 64 KB
            flush_interval: Duration::from_secs(5),
        }
    }
}

impl CacheConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), CacheError> {
        if self.max_size < 1024 * 1024 * 1024 {
            return Err(CacheError::ConfigError(
                "max_size must be at least 1 GB".into(),
            ));
        }
        if self.max_entries == 0 {
            return Err(CacheError::ConfigError(
                "max_entries must be > 0".into(),
            ));
        }
        Ok(())
    }

    /// Path to the qdata subdirectory.
    pub fn qdata_dir(&self) -> PathBuf {
        self.cache_dir.join("qdata")
    }

    /// Path to the qmv subdirectory.
    pub fn qmv_dir(&self) -> PathBuf {
        self.cache_dir.join("qmv")
    }

    /// Build the .qdata file path for a given info_hash.
    pub fn qdata_path(&self, info_hash: &InfoHash) -> PathBuf {
        self.qdata_dir().join(format!("{}.qdata", info_hash.to_hex()))
    }

    /// Build the .qmv file path for a given info_hash.
    pub fn qmv_path(&self, info_hash: &InfoHash) -> PathBuf {
        self.qmv_dir().join(format!("{}.qmv", info_hash.to_hex()))
    }
}
```

### CacheEntry

```rust
/// Metadata about a single cached resource.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Resource identifier.
    pub info_hash: InfoHash,

    /// Original filename.
    pub filename: String,

    /// Total file size in bytes.
    pub file_size: u64,

    /// Total bytes downloaded to cache.
    pub downloaded: u64,

    /// Bitfield tracking which pieces are fully cached.
    pub bitfield: Bitfield,

    /// Timestamp of last access (for LRU ordering).
    pub last_access: Instant,

    /// Timestamp of creation.
    pub created_at: Instant,

    /// Number of times this entry has been accessed.
    pub access_count: u64,

    /// Total bytes served from cache (for hit rate tracking).
    pub bytes_served: u64,
}

impl CacheEntry {
    /// Completion ratio (0.0–1.0).
    pub fn completion(&self) -> f64 {
        if self.file_size == 0 {
            return 0.0;
        }
        self.downloaded as f64 / self.file_size as f64
    }

    /// Whether the entire file is cached.
    pub fn is_complete(&self) -> bool {
        self.downloaded >= self.file_size
    }

    /// Whether a specific piece is cached.
    pub fn has_piece(&self, piece_index: u32) -> bool {
        self.bitfield.has(piece_index)
    }

    /// LRU score: lower = more eligible for eviction.
    pub fn lru_score(&self) -> u64 {
        let age_secs = self.last_access.elapsed().as_secs();
        // Prefer to keep files with higher access counts and more completion
        let completion_bonus = (self.completion() * 100.0) as u64;
        let access_bonus = self.access_count.min(100);
        // Older last_access = higher (worse) score
        age_secs.saturating_sub(completion_bonus + access_bonus)
    }
}
```

### CacheIndex

```rust
/// In-memory index of all cached resources.
/// Provides fast lookups without scanning the filesystem.
#[derive(Debug, Default)]
pub struct CacheIndex {
    /// Map from info_hash → CacheEntry.
    entries: HashMap<InfoHash, CacheEntry>,

    /// LRU ordering: sorted by last_access (oldest first).
    lru_order: Vec<InfoHash>,

    /// Total size of all cached data in bytes.
    total_size: u64,
}

impl CacheIndex {
    /// Insert or update an entry.
    pub fn insert(&mut self, entry: CacheEntry) {
        let size_delta = entry.downloaded as i64
            - self.entries.get(&entry.info_hash)
                .map(|e| e.downloaded as i64)
                .unwrap_or(0);

        self.total_size = (self.total_size as i64 + size_delta) as u64;
        self.entries.insert(entry.info_hash, entry);
        self.rebuild_lru();
    }

    /// Remove an entry from the index.
    pub fn remove(&mut self, info_hash: &InfoHash) -> Option<CacheEntry> {
        if let Some(entry) = self.entries.remove(info_hash) {
            self.total_size -= entry.downloaded;
            self.rebuild_lru();
            Some(entry)
        } else {
            None
        }
    }

    /// Get an entry by info_hash.
    pub fn get(&self, info_hash: &InfoHash) -> Option<&CacheEntry> {
        self.entries.get(info_hash)
    }

    /// Get a mutable reference to an entry (updates last_access).
    pub fn get_mut(&mut self, info_hash: &InfoHash) -> Option<&mut CacheEntry> {
        let entry = self.entries.get_mut(info_hash)?;
        entry.last_access = Instant::now();
        entry.access_count += 1;
        Some(entry)
    }

    /// Get entries sorted by LRU (oldest first).
    pub fn eviction_candidates(&self) -> Vec<&CacheEntry> {
        let mut candidates: Vec<&CacheEntry> = self.entries.values().collect();
        candidates.sort_by_key(|e| e.lru_score());
        candidates.reverse(); // highest score = most eligible for eviction
        candidates
    }

    /// Total size of all cached data.
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn rebuild_lru(&mut self) {
        let mut entries: Vec<(u64, InfoHash)> = self
            .entries
            .values()
            .map(|e| (e.lru_score(), e.info_hash))
            .collect();
        entries.sort_by_key(|(score, _)| *score);
        self.lru_order = entries.into_iter().map(|(_, h)| h).collect();
    }
}
```

## CacheManager

The `CacheManager` is the primary API for cache operations. It provides synchronous and asynchronous interfaces for reading, writing, and managing cached data.

```rust
/// Thread-safe cache manager for QVOD data.
///
/// All public methods are safe to call from multiple tokio tasks concurrently.
/// Internal state is protected by `RwLock` for concurrent reads and exclusive writes.
pub struct CacheManager {
    /// Configuration.
    config: CacheConfig,

    /// In-memory index (protected by RwLock for concurrent reads).
    index: RwLock<CacheIndex>,

    /// File I/O operations.
    io: Arc<CacheIo>,

    /// Whether the cache directories have been initialized.
    initialized: AtomicBool,
}

impl CacheManager {
    /// Create a new CacheManager with the given configuration.
    /// Does not initialize directories; call `init()` before use.
    pub fn new(config: CacheConfig) -> Self {
        config.validate().expect("Invalid cache config");
        Self {
            config,
            index: RwLock::new(CacheIndex::default()),
            io: Arc::new(CacheIo::new(&config)),
            initialized: AtomicBool::new(false),
        }
    }

    /// Initialize the cache: create directories and scan existing files.
    pub async fn init(&self) -> Result<(), CacheError> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }

        // Create cache directories
        tokio::fs::create_dir_all(self.config.qdata_dir()).await
            .map_err(|e| CacheError::IoError(format!("Failed to create qdata dir: {}", e)))?;

        tokio::fs::create_dir_all(self.config.qmv_dir()).await
            .map_err(|e| CacheError::IoError(format!("Failed to create qmv dir: {}", e)))?;

        // Scan existing .qmv files and build index
        self.scan_existing().await?;

        self.initialized.store(true, Ordering::Release);
        Ok(())
    }

    /// Scan the cache directory for existing .qmv and .qdata files,
    /// rebuilding the in-memory index.
    async fn scan_existing(&self) -> Result<(), CacheError> {
        let mut index = self.index.write().await;
        let qmv_dir = self.config.qmv_dir();

        let mut entries = tokio::fs::read_dir(&qmv_dir).await
            .map_err(|e| CacheError::IoError(format!("Failed to read qmv dir: {}", e)))?;

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| CacheError::IoError(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            if path.extension().map(|e| e == "qmv").unwrap_or(false) {
                let stem = path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string());

                if let Some(hex_hash) = stem {
                    if let Ok(info_hash) = InfoHash::from_hex(&hex_hash) {
                        // Try to parse .qmv to get full metadata
                        if let Ok(data) = tokio::fs::read(&path).await {
                            if let Ok(meta) = FileMeta::decode(&data) {
                                // Count downloaded bytes from .qdata file
                                let qdata_path = self.config.qdata_path(&info_hash);
                                let downloaded = Self::count_downloaded_pieces(
                                    &qdata_path, &meta
                                ).await.unwrap_or(0);

                                let num_pieces = meta.num_pieces();
                                let mut bitfield = Bitfield::new(num_pieces);
                                // Rebuild bitfield from .qdata file presence
                                for i in 0..num_pieces {
                                    let offset = i as u64 * meta.piece_length;
                                    let len = meta.piece_size(i);
                                    if Self::is_range_downloaded(&qdata_path, offset, len).await.unwrap_or(false) {
                                        bitfield.set(i, true);
                                    }
                                }

                                let entry = CacheEntry {
                                    info_hash,
                                    filename: meta.filename,
                                    file_size: meta.file_size,
                                    downloaded,
                                    bitfield,
                                    last_access: Instant::now(),
                                    created_at: Instant::now(),
                                    access_count: 0,
                                    bytes_served: 0,
                                };
                                index.insert(entry);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Count downloaded bytes from a .qdata sparse file.
    async fn count_downloaded_pieces(
        qdata_path: &Path,
        meta: &FileMeta,
    ) -> Result<u64, CacheError> {
        let metadata = tokio::fs::metadata(qdata_path).await.ok();
        match metadata {
            Some(m) if m.is_file() => {
                // For sparse files, the apparent size may not reflect actual usage.
                // We compute based on which pieces we can read.
                let mut total = 0u64;
                for i in 0..meta.num_pieces() {
                    let offset = i as u64 * meta.piece_length;
                    let len = meta.piece_size(i);
                    if Self::is_range_downloaded(qdata_path, offset, len).await.unwrap_or(false) {
                        total += len;
                    }
                }
                Ok(total)
            }
            _ => Ok(0),
        }
    }

    /// Check if a byte range is actually downloaded in a sparse file
    /// by attempting to read a small portion.
    async fn is_range_downloaded(
        qdata_path: &Path,
        offset: u64,
        length: u64,
    ) -> Result<bool, CacheError> {
        use tokio::io::AsyncSeekExt;
        let mut file = match tokio::fs::File::open(qdata_path).await {
            Ok(f) => f,
            Err(_) => return Ok(false),
        };

        // Try to read a single byte at the target offset
        file.seek(std::io::SeekFrom::Start(offset)).await
            .map_err(|e| CacheError::IoError(e.to_string()))?;

        let mut buf = vec![0u8; 1];
        match tokio::io::AsyncReadExt::read_exact(&mut file, &mut buf).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false), // Sparse hole
        }
    }
}
```

### Find / Read / Write

```rust
impl CacheManager {
    /// Check if a resource exists in the cache.
    pub async fn find(&self, info_hash: &InfoHash) -> Option<CacheEntry> {
        let index = self.index.read().await;
        index.get(info_hash).cloned()
    }

    /// Read data from the cache.
    ///
    /// Returns the requested byte range if it has been downloaded.
    /// Returns `CacheError::RangeNotCached` if the range is not available.
    pub async fn read(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, CacheError> {
        // Validate the request
        if length == 0 {
            return Ok(Vec::new());
        }

        let path = self.config.qdata_path(info_hash);

        // Check if the file exists
        if !path.exists() {
            return Err(CacheError::EntryNotFound(*info_hash));
        }

        // Update access time in index
        {
            let mut index = self.index.write().await;
            if let Some(entry) = index.get_mut(info_hash) {
                entry.bytes_served += length;
            }
        }

        // Read from the sparse file
        let data = self.io.read_range(&path, offset, length).await?;

        // Verify the read actually returned data (not a sparse hole)
        // For sparse files, reading a hole returns zeroes — we treat
        // this as "not cached" for practical purposes
        let all_zeros = data.iter().all(|&b| b == 0);
        if all_zeros {
            return Err(CacheError::RangeNotCached {
                info_hash: *info_hash,
                offset,
                length,
            });
        }

        Ok(data)
    }

    /// Write data to the cache.
    ///
    /// Writes the given data at the specified file offset in the sparse .qdata file.
    /// Updates the in-memory bitfield for the affected pieces.
    pub async fn write(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        data: &[u8],
    ) -> Result<(), CacheError> {
        if data.is_empty() {
            return Ok(());
        }

        let path = self.config.qdata_path(info_hash);

        // Ensure the qdata file exists (create as sparse if supported)
        self.io.ensure_file(&path).await?;

        // Write the data
        self.io.write_at(&path, offset, data).await?;

        // Update the in-memory index
        let mut index = self.index.write().await;
        if let Some(entry) = index.get_mut(info_hash) {
            entry.downloaded += data.len() as u64;

            // Update bitfield for affected pieces
            let piece_length = entry.file_size / entry.bitfield.len() as u64;
            let start_piece = (offset / piece_length) as u32;
            let end_piece = ((offset + data.len() as u64 - 1) / piece_length) as u32;

            for piece in start_piece..=end_piece {
                // Mark piece as complete if we've now downloaded all of it
                let piece_offset = piece as u64 * piece_length;
                let piece_size = if piece == entry.bitfield.len() as u32 - 1 {
                    entry.file_size - piece_offset
                } else {
                    piece_length
                };

                // We optimistically set the bit; full accuracy is verified
                // when checking piece completion
                entry.bitfield.set(piece, true);
            }
        }

        Ok(())
    }

    /// Write multiple pieces of data in a single operation.
    /// More efficient than multiple `write()` calls for sequential ranges.
    pub async fn write_batch(
        &self,
        info_hash: &InfoHash,
        writes: &[(u64, Vec<u8>)], // (offset, data) pairs
    ) -> Result<(), CacheError> {
        for (offset, data) in writes {
            self.write(info_hash, *offset, data).await?;
        }
        Ok(())
    }
}
```

### Completion and Cache State

```rust
impl CacheManager {
    /// Get the completion ratio (0.0–1.0) for a cached resource.
    pub async fn completion(&self, info_hash: &InfoHash) -> f64 {
        let index = self.index.read().await;
        match index.get(info_hash) {
            Some(entry) => entry.completion(),
            None => 0.0,
        }
    }

    /// Get the bitfield of completed pieces for a resource.
    pub async fn bitfield(&self, info_hash: &InfoHash) -> Option<Bitfield> {
        let index = self.index.read().await;
        index.get(info_hash).map(|e| e.bitfield.clone())
    }

    /// Get the total bytes downloaded for a resource.
    pub async fn downloaded_bytes(&self, info_hash: &InfoHash) -> u64 {
        let index = self.index.read().await;
        index.get(info_hash).map(|e| e.downloaded).unwrap_or(0)
    }

    /// Check if a resource is fully cached.
    pub async fn is_complete(&self, info_hash: &InfoHash) -> bool {
        let index = self.index.read().await;
        index.get(info_hash).map(|e| e.is_complete()).unwrap_or(false)
    }

    /// Check if a specific piece is cached.
    pub async fn has_piece(&self, info_hash: &InfoHash, piece_index: u32) -> bool {
        let index = self.index.read().await;
        index.get(info_hash)
            .map(|e| e.has_piece(piece_index))
            .unwrap_or(false)
    }

    /// Check if a byte range is available in the cache.
    pub async fn is_range_available(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> bool {
        let index = self.index.read().await;
        let Some(entry) = index.get(info_hash) else {
            return false;
        };

        let piece_length = entry.file_size / entry.bitfield.len() as u64;
        let start_piece = (offset / piece_length) as u32;
        let end_piece = ((offset + length - 1) / piece_length) as u32;

        for piece in start_piece..=end_piece {
            if !entry.bitfield.has(piece) {
                return false;
            }
        }
        true
    }
}
```

### Cleanup / LRU Eviction

```rust
impl CacheManager {
    /// Get the current total cache size.
    pub async fn total_size(&self) -> u64 {
        let index = self.index.read().await;
        index.total_size()
    }

    /// Get the number of cached entries.
    pub async fn entry_count(&self) -> usize {
        let index = self.index.read().await;
        index.len()
    }

    /// Get a list of all cached entries.
    pub async fn list_entries(&self) -> Vec<CacheEntry> {
        let index = self.index.read().await;
        index.eviction_candidates().into_iter().cloned().collect()
    }

    /// Run cache cleanup: evict entries until we're under the max size
    /// and above the min free space threshold.
    ///
    /// Eviction strategy (LRU-aware):
    ///   1. Sort entries by LRU score (oldest + least accessed first)
    ///   2. Remove entries until total_size <= max_size * 0.8
    ///   3. Also remove if free disk space < min_free_space
    pub async fn cleanup(&self) -> Result<(), CacheError> {
        self.ensure_initialized().await?;

        let mut evicted = 0u64;
        let mut freed_bytes = 0u64;

        loop {
            let (should_evict, total_size) = {
                let index = self.index.read().await;
                let free_space = self.free_disk_space().unwrap_or(u64::MAX);
                let over_limit = total_size > self.config.max_size;
                let low_disk = free_space < self.config.min_free_space;
                (over_limit || low_disk, index.total_size())
            };

            if !should_evict {
                break;
            }

            // Find the worst entry to evict
            let candidate = {
                let index = self.index.read().await;
                index.eviction_candidates().first().cloned().cloned()
            };

            match candidate {
                Some(entry) => {
                    // Never evict entries with > 90% completion (waste of bandwidth)
                    if entry.completion() > 0.90 && total_size <= self.config.max_size * 2 {
                        // Skip near-complete entries; evict less complete ones first
                        // unless we're really over capacity
                        break;
                    }

                    // Delete the entry
                    self.delete_entry(&entry.info_hash).await?;
                    evicted += 1;
                    freed_bytes += entry.downloaded;

                    tracing::info!(
                        "Evicted {} ({}): freed {} bytes",
                        entry.filename,
                        entry.info_hash.to_hex(),
                        entry.downloaded
                    );
                }
                None => break,
            }
        }

        if evicted > 0 {
            tracing::info!("Cache cleanup: evicted {} entries, freed {} bytes", evicted, freed_bytes);
        }

        Ok(())
    }

    /// Delete a single entry from the cache.
    async fn delete_entry(&self, info_hash: &InfoHash) -> Result<(), CacheError> {
        // Remove index entry
        {
            let mut index = self.index.write().await;
            index.remove(info_hash);
        }

        // Delete .qdata file
        let qdata_path = self.config.qdata_path(info_hash);
        if qdata_path.exists() {
            tokio::fs::remove_file(&qdata_path).await
                .map_err(|e| CacheError::IoError(format!("Failed to delete qdata: {}", e)))?;
        }

        // Delete .qmv file
        let qmv_path = self.config.qmv_path(info_hash);
        if qmv_path.exists() {
            tokio::fs::remove_file(&qmv_path).await
                .map_err(|e| CacheError::IoError(format!("Failed to delete qmv: {}", e)))?;
        }

        Ok(())
    }

    /// Delete a resource from the cache explicitly.
    pub async fn delete(&self, info_hash: &InfoHash) -> Result<(), CacheError> {
        self.delete_entry(info_hash).await
    }

    /// Delete all cache entries.
    pub async fn clear(&self) -> Result<(), CacheError> {
        let entries = {
            let index = self.index.read().await;
            index.eviction_candidates()
                .into_iter()
                .map(|e| e.info_hash)
                .collect::<Vec<_>>()
        };

        for hash in entries {
            self.delete_entry(&hash).await?;
        }

        Ok(())
    }

    /// Get available free disk space on the cache device.
    fn free_disk_space(&self) -> Result<u64, CacheError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let stat = nix::sys::statvfs::statvfs(&self.config.cache_dir)
                .map_err(|e| CacheError::IoError(format!("statvfs failed: {}", e)))?;
            Ok(stat.blocks_available() * stat.fragment_size())
        }
        #[cfg(not(unix))]
        {
            // On non-Unix, return a large number to skip disk space check
            Ok(u64::MAX)
        }
    }
}
```

### Metadata Integration

```rust
impl CacheManager {
    /// Save FileMeta to the .qmv cache file.
    pub async fn save_metadata(&self, meta: &FileMeta) -> Result<(), CacheError> {
        let path = self.config.qmv_path(&meta.info_hash);
        let encoded = meta.encode();

        // Write atomically: write to temp file, then rename
        let tmp_path = path.with_extension("qmv.tmp");
        tokio::fs::write(&tmp_path, &encoded).await
            .map_err(|e| CacheError::IoError(format!("Failed to write qmv: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| CacheError::IoError(format!("Failed to rename qmv: {}", e)))?;

        // Update index
        let mut index = self.index.write().await;
        let entry = CacheEntry {
            info_hash: meta.info_hash,
            filename: meta.filename.clone(),
            file_size: meta.file_size,
            downloaded: 0,
            bitfield: Bitfield::new(meta.num_pieces()),
            last_access: Instant::now(),
            created_at: Instant::now(),
            access_count: 0,
            bytes_served: 0,
        };
        index.insert(entry);

        Ok(())
    }

    /// Load FileMeta from the .qmv cache file.
    pub async fn load_metadata(&self, info_hash: &InfoHash) -> Result<FileMeta, CacheError> {
        let path = self.config.qmv_path(info_hash);
        let data = tokio::fs::read(&path).await
            .map_err(|_| CacheError::MetadataNotFound(*info_hash))?;

        let mut meta = FileMeta::decode(&data)
            .map_err(|e| CacheError::MetadataCorrupted {
                info_hash: *info_hash,
                details: e.to_string(),
            })?;

        meta.from_cache = true;
        Ok(meta)
    }

    /// Save metadata and create the .qdata sparse file in one operation.
    pub async fn init_cache_entry(&self, meta: &FileMeta) -> Result<(), CacheError> {
        // Save metadata first
        self.save_metadata(meta).await?;

        // Pre-allocate sparse file
        let path = self.config.qdata_path(&meta.info_hash);
        self.io.ensure_file(&path).await?;

        // If sparse files are enabled and supported, allocate space
        if self.config.sparse_files {
            self.io.allocate_sparse(&path, meta.file_size).await.ok();
        }

        Ok(())
    }
}
```

## I/O Layer

```rust
/// Low-level I/O operations for cache data files.
struct CacheIo {
    config: CacheConfig,
    /// Buffer pool to reduce allocations (optional).
    buffer_pool: Option<Vec<Vec<u8>>>,
}

impl CacheIo {
    fn new(config: &CacheConfig) -> Self {
        Self {
            config: config.clone(),
            buffer_pool: None,
        }
    }

    /// Ensure a file exists, creating it if necessary.
    async fn ensure_file(&self, path: &Path) -> Result<(), CacheError> {
        if path.exists() {
            return Ok(());
        }

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| CacheError::IoError(format!("Failed to create parent dir: {}", e)))?;
        }

        // Create empty file
        tokio::fs::File::create(path).await
            .map_err(|e| CacheError::IoError(format!("Failed to create file: {}", e)))?;

        Ok(())
    }

    /// Read a range of bytes from a file.
    async fn read_range(&self, path: &Path, offset: u64, length: u64) -> Result<Vec<u8>, CacheError> {
        use tokio::io::AsyncSeekExt;

        let mut file = tokio::fs::File::open(path).await
            .map_err(|e| CacheError::IoError(format!("Failed to open for read: {}", e)))?;

        file.seek(std::io::SeekFrom::Start(offset)).await
            .map_err(|e| CacheError::IoError(format!("Seek failed: {}", e)))?;

        let mut buf = vec![0u8; length as usize];
        let read_len = tokio::io::AsyncReadExt::read(&mut file, &mut buf).await
            .map_err(|e| CacheError::IoError(format!("Read failed: {}", e)))?;

        buf.truncate(read_len);
        Ok(buf)
    }

    /// Write data at a specific file offset.
    /// Uses `write_at` for atomic positional writes.
    async fn write_at(&self, path: &Path, offset: u64, data: &[u8]) -> Result<(), CacheError> {
        use tokio::io::AsyncSeekExt;

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(path).await
            .map_err(|e| CacheError::IoError(format!("Failed to open for write: {}", e)))?;

        file.seek(std::io::SeekFrom::Start(offset)).await
            .map_err(|e| CacheError::IoError(format!("Seek failed: {}", e)))?;

        tokio::io::AsyncWriteExt::write_all(&mut file, data).await
            .map_err(|e| CacheError::IoError(format!("Write failed: {}", e)))?;

        // Flush to ensure durability
        file.flush().await
            .map_err(|e| CacheError::IoError(format!("Flush failed: {}", e)))?;

        Ok(())
    }

    /// Allocate a sparse file of the given size.
    /// On Linux, uses `fallocate()` with `FALLOC_FL_KEEP_SIZE` or `fallocate(0)`.
    /// On macOS, uses `fstore()` or `ftruncate()`.
    /// On Windows, uses `SetFileValidData()`.
    async fn allocate_sparse(&self, path: &Path, size: u64) -> Result<(), CacheError> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            let file = tokio::fs::File::open(path).await
                .map_err(|e| CacheError::IoError(format!("Failed to open for allocation: {}", e)))?;
            let fd = file.as_raw_fd();

            // Try fallocate first (allocates but marks as unwritten)
            let ret = unsafe {
                libc::fallocate(fd, 0, 0, size as libc::off_t)
            };
            if ret != 0 {
                // Fall back to ftruncate for sparse file
                unsafe {
                    libc::ftruncate(fd, size as libc::off_t);
                }
            }

            // Don't close file; let it drop naturally
            Ok(())
        }

        #[cfg(target_os = "macos")]
        {
            use std::os::unix::io::AsRawFd;
            let file = tokio::fs::File::open(path).await
                .map_err(|e| CacheError::IoError(format!("Failed to open for allocation: {}", e)))?;
            let fd = file.as_raw_fd();

            // ftruncate creates a sparse file on macOS
            unsafe {
                libc::ftruncate(fd, size as libc::off_t);
            }
            Ok(())
        }

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            let file = tokio::fs::File::open(path).await
                .map_err(|e| CacheError::IoError(format!("Failed to open for allocation: {}", e)))?;
            let handle = file.as_raw_handle();

            // SetFileValidData on Windows
            let size_u64 = size as u64;
            unsafe {
                windows::Win32::Storage::FileSystem::SetFileValidData(
                    handle,
                    size_u64,
                );
            }
            Ok(())
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            // Fallback: just truncate the file (will be sparse-ish on some FS)
            let file = tokio::fs::File::create(path).await
                .map_err(|e| CacheError::IoError(format!("Failed to create for trunc: {}", e)))?;
            file.set_len(size).await
                .map_err(|e| CacheError::IoError(format!("Truncate failed: {}", e)))?;
            Ok(())
        }
    }
}
```

## Thread Safety and Concurrent Access

The `CacheManager` is safe to share across async tasks:

```rust
impl CacheManager {
    /// Create a shared reference for use across the engine.
    pub fn shared(config: CacheConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    /// Ensure the cache is initialized. Called by every public method.
    async fn ensure_initialized(&self) -> Result<(), CacheError> {
        if !self.initialized.load(Ordering::Acquire) {
            self.init().await?;
        }
        Ok(())
    }
}

// The CacheManager can be cloned via Arc
// type SharedCacheManager = Arc<CacheManager>;

/// Example: concurrent access from multiple download tasks
async fn example_concurrent_access(cache: &Arc<CacheManager>, info_hash: InfoHash) {
    let cache1 = cache.clone();
    let cache2 = cache.clone();

    let write_task = tokio::spawn(async move {
        cache1.write(&info_hash, 0, &[1, 2, 3]).await.unwrap();
    });

    let read_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let data = cache2.read(&info_hash, 0, 3).await.unwrap();
        assert_eq!(data, vec![1, 2, 3]);
    });

    write_task.await.unwrap();
    read_task.await.unwrap();
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Cache not initialized")]
    NotInitialized,

    #[error("Entry not found: {0}")]
    EntryNotFound(InfoHash),

    #[error("Metadata not found: {0}")]
    MetadataNotFound(InfoHash),

    #[error("Metadata corrupted for {info_hash}: {details}")]
    MetadataCorrupted {
        info_hash: InfoHash,
        details: String,
    },

    #[error("Range not cached: {info_hash} offset={offset} length={length}")]
    RangeNotCached {
        info_hash: InfoHash,
        offset: u64,
        length: u64,
    },

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Cache full: {current} bytes exceeds limit of {limit} bytes")]
    CacheFull {
        current: u64,
        limit: u64,
    },

    #[error("Insufficient disk space: {free} bytes free, need {required} bytes")]
    InsufficientDiskSpace {
        free: u64,
        required: u64,
    },

    #[error("Concurrent modification conflict for {0}")]
    ConcurrentModification(InfoHash),
}
```

## Integration with Playback

When the `QvodEngine::play()` method is called, it checks the cache first:

```rust
impl QvodEngine {
    pub async fn play(&self, uri: &QvodUri) -> Result<MediaStream, EngineError> {
        let info_hash = uri.info_hash();

        // Step 1: Check cache for existing data
        if let Some(entry) = self.cache_manager.find(&info_hash).await {
            tracing::info!("Cache HIT for {} (completed: {}%)",
                info_hash.to_hex(),
                entry.completion() * 100.0
            );

            if entry.is_complete() {
                // Fully cached — instant playback from local storage
                let meta = self.cache_manager.load_metadata(&info_hash).await?;
                return Ok(MediaStream::from_cache(meta, self.cache_manager.clone()));
            } else {
                // Partial cache — resume download from where we left off
                let meta = self.cache_manager.load_metadata(&info_hash).await?;
                // Seed the scheduler's bitfield with cached pieces
                self.scheduler = PieceScheduler::from_cache(meta.clone(), entry.bitfield);
                // ... continue with peer discovery for missing pieces
            }
        } else {
            // Cache MISS — full download from network
            // ... normal flow: metadata fetch → scheduler init → download
        }

        // ...
        todo!()
    }
}
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_config() -> CacheConfig {
        let dir = tempdir().unwrap();
        CacheConfig {
            cache_dir: dir.path().to_path_buf(),
            max_size: 100 * 1024 * 1024, // 100 MB for testing
            min_free_space: 0, // disable disk space check
            max_entries: 10,
            sparse_files: false, // disable sparse for test reliability
            use_index: false,
            io_buffer_size: 4096,
            flush_interval: Duration::from_secs(1),
        }
    }

    fn test_meta() -> FileMeta {
        let entries = vec![
            KeyFrameEntry {
                timestamp_ms: 0,
                file_offset: 0,
                frame_size: 48000,
                frame_type: FrameType::I,
            },
        ];
        FileMeta {
            info_hash: InfoHash([0xAB; 20]),
            filename: "test.mp4".into(),
            file_size: 65536, // 64 KB = 1 piece
            piece_length: 65536,
            piece_hashes: vec![PieceHash([0x00; 20])],
            keyframe_index: KeyFrameIndex::new(entries).unwrap(),
            duration_ms: 5000,
            codec: CodecInfo {
                video_codec: "avc1".into(),
                audio_codec: "aac".into(),
                width: 640,
                height: 480,
                bitrate: 500_000,
                ..Default::default()
            },
            from_cache: false,
        }
    }

    #[tokio::test]
    async fn test_init() {
        let config = test_config();
        let cache = CacheManager::new(config.clone());
        cache.init().await.unwrap();

        assert!(config.qdata_dir().exists());
        assert!(config.qmv_dir().exists());
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let config = test_config();
        let cache = CacheManager::new(config);
        cache.init().await.unwrap();

        let meta = test_meta();
        let info_hash = meta.info_hash;

        cache.save_metadata(&meta).await.unwrap();
        cache.write(&info_hash, 0, &[0xDE, 0xAD, 0xBE, 0xEF]).await.unwrap();

        let data = cache.read(&info_hash, 0, 4).await.unwrap();
        assert_eq!(data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[tokio::test]
    async fn test_find() {
        let config = test_config();
        let cache = CacheManager::new(config);
        cache.init().await.unwrap();

        let meta = test_meta();
        cache.save_metadata(&meta).await.unwrap();

        let entry = cache.find(&meta.info_hash).await;
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().filename, "test.mp4");
    }

    #[tokio::test]
    async fn test_completion() {
        let config = test_config();
        let cache = CacheManager::new(config);
        cache.init().await.unwrap();

        let meta = test_meta();
        cache.save_metadata(&meta).await.unwrap();

        let completion = cache.completion(&meta.info_hash).await;
        assert!(completion < 0.01); // barely started

        // Write the full piece
        let data = vec![0xFF; meta.file_size as usize];
        cache.write(&meta.info_hash, 0, &data).await.unwrap();

        let completion = cache.completion(&meta.info_hash).await;
        assert!((completion - 1.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_delete() {
        let config = test_config();
        let cache = CacheManager::new(config.clone());
        cache.init().await.unwrap();

        let meta = test_meta();
        cache.save_metadata(&meta).await.unwrap();
        cache.write(&meta.info_hash, 0, &[1, 2, 3]).await.unwrap();

        cache.delete(&meta.info_hash).await.unwrap();

        let entry = cache.find(&meta.info_hash).await;
        assert!(entry.is_none());

        // Files should be deleted
        assert!(!config.qdata_path(&meta.info_hash).exists());
        assert!(!config.qmv_path(&meta.info_hash).exists());
    }

    #[tokio::test]
    async fn test_cache_size_tracking() {
        let config = test_config();
        let cache = CacheManager::new(config);
        cache.init().await.unwrap();

        let meta = test_meta();
        cache.save_metadata(&meta).await.unwrap();

        assert_eq!(cache.total_size().await, 0);

        cache.write(&meta.info_hash, 0, &[0xAA; 1000]).await.unwrap();
        assert!(cache.total_size().await >= 1000);
    }

    #[tokio::test]
    async fn test_cleanup_eviction() {
        let config = CacheConfig {
            max_size: 1024 * 10, // 10 KB
            max_entries: 2,
            ..test_config()
        };
        let cache = CacheManager::new(config);
        cache.init().await.unwrap();

        // Insert two entries
        for i in 0..3u8 {
            let mut meta = test_meta();
            meta.info_hash = InfoHash([i; 20]);
            meta.filename = format!("test_{}.mp4", i);
            meta.file_size = 65536;
            cache.save_metadata(&meta).await.unwrap();
            cache.write(&meta.info_hash, 0, &vec![0xBB; 5000]).await.unwrap();
        }

        // Cleanup should evict the oldest entry
        cache.cleanup().await.unwrap();

        let count = cache.entry_count().await;
        assert!(count <= 2, "Expected at most 2 entries after cleanup, got {}", count);
    }

    #[tokio::test]
    async fn test_concurrent_read_write() {
        let config = test_config();
        let cache = Arc::new(CacheManager::new(config));
        cache.init().await.unwrap();

        let meta = test_meta();
        cache.save_metadata(&meta).await.unwrap();

        let info_hash = meta.info_hash;
        let mut tasks = Vec::new();

        // Spawn 10 concurrent writer tasks
        for i in 0..10u64 {
            let c = cache.clone();
            tasks.push(tokio::spawn(async move {
                let offset = i * 100;
                let data = vec![i as u8; 100];
                c.write(&info_hash, offset, &data).await.unwrap();
            }));
        }

        for task in tasks {
            task.await.unwrap();
        }

        // Verify all data is readable
        for i in 0..10u64 {
            let offset = i * 100;
            let data = cache.read(&info_hash, offset, 100).await.unwrap();
            assert_eq!(data, vec![i as u8; 100]);
        }
    }

    #[tokio::test]
    async fn test_range_not_cached() {
        let config = test_config();
        let cache = CacheManager::new(config);
        cache.init().await.unwrap();

        let meta = test_meta();
        cache.save_metadata(&meta).await.unwrap();

        // Write at offset 100
        cache.write(&meta.info_hash, 100, &[0xCC; 10]).await.unwrap();

        // Reading at offset 0 should fail (not cached)
        let result = cache.read(&meta.info_hash, 0, 10).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_metadata_roundtrip() {
        let config = test_config();
        let cache = CacheManager::new(config);
        cache.init().await.unwrap();

        let meta = test_meta();
        cache.save_metadata(&meta).await.unwrap();

        let loaded = cache.load_metadata(&meta.info_hash).await.unwrap();
        assert!(loaded.from_cache);
        assert_eq!(loaded.filename, meta.filename);
        assert_eq!(loaded.file_size, meta.file_size);
        assert_eq!(loaded.duration_ms, meta.duration_ms);
        assert_eq!(loaded.codec.video_codec, meta.codec.video_codec);
    }
}
```

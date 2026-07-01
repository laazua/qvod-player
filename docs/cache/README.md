# QVOD Caching Strategy Reference

## 1. Overview

QVOD employs a local disk cache to store downloaded pieces between sessions. The cache system supports:

- Sparse file storage (zero disk cost for undownloaded regions)
- LRU eviction when capacity threshold is exceeded
- Integrity verification (SHA-1 piece hashing)
- Concurrent read/write access via tokio async
- Piece-level completion tracking via bitfield

---

## 2. Architecture

```
┌───────────┐     ┌──────────────────┐     ┌──────────────────┐
│  Qvod     │────▶│  CacheManager    │────▶│  qdata/.qmv      │
│  Engine   │     │  (thread-safe)   │     │  file system     │
│           │     │                  │     │                  │
│           │◀────│  Arc<Mutex<Self>>│◀────│  sparse files    │
└───────────┘     └──────────────────┘     └──────────────────┘
                         │
                         ▼
                  ┌──────────────────┐
                  │  LRU Eviction    │
                  │  Policy Engine   │
                  └──────────────────┘
```

### 2.1 Core Interfaces

```rust
pub trait CacheBackend: Send + Sync {
    fn find(&self, info_hash: &InfoHash) -> Result<Option<CacheEntry>>;
    fn read(&self, info_hash: &InfoHash, offset: u64, length: u64) -> Result<Vec<u8>>;
    fn write(&self, info_hash: &InfoHash, offset: u64, data: &[u8]) -> Result<()>;
    fn write_piece(&self, info_hash: &InfoHash, index: u32, data: &[u8]) -> Result<()>;
    fn completion(&self, info_hash: &InfoHash) -> Result<f64>;
    fn cleanup(&self) -> Result<CleanupReport>;
    fn delete(&self, info_hash: &InfoHash) -> Result<()>;
    fn list(&self) -> Result<Vec<CacheEntry>>;
    fn total_size(&self) -> Result<u64>;
    fn max_size(&self) -> u64;
    fn set_max_size(&self, max_bytes: u64);
}
```

### 2.2 Thread Safety

```rust
#[derive(Clone)]
pub struct CacheManager {
    inner: Arc<Mutex<CacheInner>>,
}

struct CacheInner {
    config: CacheConfig,
    qdata_files: HashMap<InfoHash, QdataHandle>,
    metadata: HashMap<InfoHash, Arc<FileMeta>>,
    lru_list: VecDeque<InfoHash>,
    last_cleanup: Instant,
    total_on_disk: u64,  // approximate disk usage
}

struct QdataHandle {
    file: tokio::fs::File,
    bitfield: Bitfield,
    file_size: u64,
    last_access: Instant,
    created_at: Instant,
}
```

---

## 3. Cache Entry Lookup by InfoHash

### 3.1 Lookup Process

```
FIND(info_hash):
    │
    ├─ 1. Check in-memory cache (HashMap)
    │     ├─ Hit  → update LRU position, return CacheEntry
    │     └─ Miss → continue
    │
    ├─ 2. Check file system
    │     ├─ Does {cache_dir}/qdata/{hash}.qdata exist?
    │     │   ├─ No  → return None
    │     │   └─ Yes → continue
    │     │
    │     ├─ Does {cache_dir}/qmv/{hash}.qmv exist?
    │     │   ├─ No  → return None (corrupted cache state)
    │     │   └─ Yes → load metadata
    │     │
    │     ├─ Parse .qmv → FileMeta (with bitfield)
    │     ├─ Load into in-memory HashMap
    │     ├─ Add to LRU list
    │     └─ Return CacheEntry
```

### 3.2 Implementation

```rust
impl CacheManager {
    pub fn find(&self, info_hash: &InfoHash) -> Result<Option<CacheEntry>> {
        let mut inner = self.inner.lock().unwrap();

        // Check in-memory cache
        if let Some(handle) = inner.qdata_files.get(info_hash) {
            // Update LRU: move to front
            if let Some(pos) = inner.lru_list.iter().position(|h| h == info_hash) {
                let hash = inner.lru_list.remove(pos).unwrap();
                inner.lru_list.push_front(hash);
            }
            return Ok(Some(CacheEntry {
                info_hash: *info_hash,
                file_size: handle.file_size,
                downloaded: handle.bitfield.count() as u64 * inner.config.piece_length,
                bitfield: handle.bitfield.clone(),
                last_access: handle.last_access,
                created_at: handle.created_at,
            }));
        }

        // Check file system
        let qdata_path = inner.config.cache_dir
            .join("qdata")
            .join(format!("{}.qdata", hex::encode(info_hash)));

        let qmv_path = inner.config.cache_dir
            .join("qmv")
            .join(format!("{}.qmv", hex::encode(info_hash)));

        if !qdata_path.exists() || !qmv_path.exists() {
            return Ok(None);
        }

        // Load metadata
        let qmv_data = std::fs::read(&qmv_path)?;
        let file_meta = FileMeta::decode_bencode(&qmv_data)
            .map_err(|e| CacheError::Corrupted(format!("invalid .qmv: {e}")))?;

        let qdata_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&qdata_path)?;

        let file_size = qdata_file.metadata()?.len();
        let piece_count = ((file_size + PIECE_LENGTH - 1) / PIECE_LENGTH) as u32;

        // Reconstruct bitfield from disk: scan sparse file
        let bitfield = Self::scan_bitfield(&qdata_file, piece_count)?;

        let handle = QdataHandle {
            file: tokio::fs::File::from_std(qdata_file),
            bitfield: bitfield.clone(),
            file_size,
            last_access: Instant::now(),
            created_at: Instant::now(),
        };

        inner.qdata_files.insert(*info_hash, handle);
        inner.lru_list.push_front(*info_hash);

        Ok(Some(CacheEntry {
            info_hash: *info_hash,
            file_size,
            downloaded: bitfield.count() as u64 * PIECE_LENGTH,
            bitfield,
            last_access: Instant::now(),
            created_at: Instant::now(),
        }))
    }

    /// Scans sparse file for allocated blocks by attempting to read each piece
    fn scan_bitfield(file: &std::fs::File, piece_count: u32) -> Result<Bitfield> {
        let mut bf = Bitfield::new(piece_count);
        for i in 0..piece_count {
            let offset = i as u64 * PIECE_LENGTH;
            let mut buf = vec![0u8; PIECE_LENGTH as usize];
            match file.read_exact_at(&mut buf, offset) {
                Ok(_) => {
                    // Check if the piece is non-zero (sparse hole returns zeroes)
                    if buf.iter().any(|&b| b != 0) {
                        bf.set(i, true);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    // Short read = not fully written
                    let mut short_buf = vec![];
                    let read = file.read_to_end_at(&mut short_buf, offset).unwrap_or(0);
                    if read > 0 && short_buf.iter().any(|&b| b != 0) {
                        bf.set(i, true);
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(bf)
    }
}
```

---

## 4. Read/Write Strategy for Sparse Files

### 4.1 Write Path

```
WRITE(info_hash, offset, data):
    │
    ├─ 1. Validate: offset + data.len ≤ file_size
    │     └─ Fail → InvalidLength error
    │
    ├─ 2. Verify integrity (if config.verify_on_write)
    │     ├─ Compute SHA-1 of piece
    │     ├─ Compare against FileMeta.pieces[piece_index]
    │     └─ Fail → PieceVerificationFailed error
    │
    ├─ 3. Acquire lock for this info_hash
    │
    ├─ 4. Seek to offset in .qdata file
    │
    ├─ 5. Write data (async)
    │
    ├─ 6. Update bitfield: mark piece as complete
    │     (only if all blocks of the piece have been written)
    │
    ├─ 7. Update .qmv → write updated bitfield
    │
    ├─ 8. Update LRU: move to front
    │
    └─ 9. Update total_on_disk counter
```

### 4.2 Read Path

```
READ(info_hash, offset, length):
    │
    ├─ 1. Validate offset + length ≤ file_size
    │
    ├─ 2. Check bitfield: all required pieces present?
    │     ├─ No → Error: try again later or trigger download
    │     └─ Yes → continue
    │
    ├─ 3. Acquire lock for this info_hash
    │
    ├─ 4. Seek to offset in .qdata file
    │
    ├─ 5. Read `length` bytes
    │
    ├─ 6. Verify slice (if config.verify_on_read)
    │     ├─ Get piece index range covered by this read
    │     ├─ For each complete piece: SHA-1 verify
    │     └─ Fail → mark piece as incomplete, redownload
    │
    ├─ 7. Update LRU: move to front
    │
    └─ 8. Return data
```

### 4.3 Implementation

```rust
impl CacheManager {
    pub async fn write(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        data: &[u8],
    ) -> Result<(), CacheError> {
        let mut inner = self.inner.lock().unwrap();

        // Validate
        let handle = inner.qdata_files.get_mut(info_hash)
            .ok_or(CacheError::EntryNotFound(*info_hash))?;

        let end = offset + data.len() as u64;
        if end > handle.file_size {
            return Err(CacheError::InvalidLength {
                expected: handle.file_size,
                actual: end,
            });
        }

        // Write to file
        handle.file
            .write_all_at(data, offset)
            .await
            .map_err(CacheError::Io)?;

        // Update bitfield
        let piece_index = (offset / PIECE_LENGTH) as u32;
        let piece_start = piece_index as u64 * PIECE_LENGTH;
        let piece_data_len = if piece_index == handle.bitfield.piece_count() - 1 {
            let rem = handle.file_size % PIECE_LENGTH;
            if rem == 0 { PIECE_LENGTH } else { rem }
        } else {
            PIECE_LENGTH
        };

        // Check if all blocks of this piece are now written
        let write_end = offset + data.len() as u64;
        let piece_end = piece_start + piece_data_len;

        if write_end >= piece_end && !handle.bitfield.has(piece_index) {
            // Verify piece integrity
            if inner.config.verify_on_write {
                let piece_data = handle.file
                    .read_at(vec![0u8; piece_data_len as usize].as_mut_slice(), piece_start)
                    .await
                    .map_err(CacheError::Io)?;
                // SHA-1 verify
                let hash = sha1::Sha1::from(piece_data).digest().bytes();
                let expected = &inner.metadata.get(info_hash)
                    .ok_or(CacheError::EntryNotFound(*info_hash))?
                    .pieces[piece_index as usize];
                if hash != *expected {
                    return Err(CacheError::PieceVerification {
                        index: piece_index,
                        expected: *expected,
                        actual: hash,
                    });
                }
            }

            handle.bitfield.set(piece_index, true);
        }

        // Update LRU
        if let Some(pos) = inner.lru_list.iter().position(|h| h == info_hash) {
            let hash = inner.lru_list.remove(pos).unwrap();
            inner.lru_list.push_front(hash);
        }

        // Update disk usage
        inner.total_on_disk += data.len() as u64;

        // Flush metadata
        Self::flush_metadata(&inner, info_hash)?;

        Ok(())
    }

    pub async fn read(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, CacheError> {
        let mut inner = self.inner.lock().unwrap();

        let handle = inner.qdata_files.get_mut(info_hash)
            .ok_or(CacheError::EntryNotFound(*info_hash))?;

        // Verify all required pieces are complete
        let start_piece = (offset / PIECE_LENGTH) as u32;
        let end_piece = ((offset + length - 1) / PIECE_LENGTH) as u32;
        for i in start_piece..=end_piece {
            if !handle.bitfield.has(i) {
                return Err(CacheError::PieceMissing { index: i });
            }
        }

        // Read
        let mut buf = vec![0u8; length as usize];
        handle.file
            .read_exact_at(&mut buf, offset)
            .await
            .map_err(CacheError::Io)?;

        // Verify on read
        if inner.config.verify_on_read {
            for i in start_piece..=end_piece {
                if handle.bitfield.has(i) {
                    let piece_start = i as u64 * PIECE_LENGTH;
                    let piece_len = if i == handle.bitfield.piece_count() - 1 {
                        let rem = handle.file_size % PIECE_LENGTH;
                        if rem == 0 { PIECE_LENGTH } else { rem }
                    } else {
                        PIECE_LENGTH
                    };
                    let mut piece_buf = vec![0u8; piece_len as usize];
                    handle.file.read_exact_at(&mut piece_buf, piece_start).await?;
                    let hash = sha1::Sha1::from(&piece_buf).digest().bytes();
                    let expected = &inner.metadata.get(info_hash)
                        .ok_or(CacheError::EntryNotFound(*info_hash))?
                        .pieces[i as usize];
                    if hash != *expected {
                        handle.bitfield.set(i, false);
                        return Err(CacheError::PieceVerification {
                            index: i,
                            expected: *expected,
                            actual: hash,
                        });
                    }
                }
            }
        }

        // Update LRU
        if let Some(pos) = inner.lru_list.iter().position(|h| h == info_hash) {
            let hash = inner.lru_list.remove(pos).unwrap();
            inner.lru_list.push_front(hash);
        }

        Ok(buf)
    }
}
```

---

## 5. LRU Eviction Algorithm

### 5.1 Eviction Trigger

Cleanup runs when:
1. Total disk usage exceeds `max_size`
2. Explicit `cleanup()` call
3. Write fails due to disk space

### 5.2 Algorithm

```
CLEANUP():
    │
    ├─ 1. Scan qdata directory → measure total_on_disk
    │
    ├─ 2. If total_on_disk ≤ max_size * 0.8:
    │     └─ Return (no cleanup needed)
    │
    ├─ 3. Sort cache entries by last_access ASC (oldest first)
    │
    ├─ 4. Target = total_on_disk - (max_size * 0.8)
    │
    ├─ 5. For each entry (oldest first):
    │     ├─ If piece still has active downloaders → skip
    │     ├─ Delete .qdata and .qmv files
    │     ├─ Remove from in-memory cache
    │     ├─ freed += entry.file_size
    │     ├─ If freed ≥ target → break
    │
    └─ 6. Return CleanupReport { deleted_count, freed_bytes }
```

### 5.3 Implementation

```rust
impl CacheManager {
    pub fn cleanup(&self) -> Result<CleanupReport> {
        let mut inner = self.inner.lock().unwrap();
        let mut deleted = 0u32;
        let mut freed = 0u64;

        // Scan disk to get accurate total
        let total = self.measure_disk_usage(&inner.config.cache_dir)?;
        inner.total_on_disk = total;

        let max_bytes = inner.config.max_size;
        let target_usage = (max_bytes as f64 * 0.8) as u64;

        if total <= max_bytes {
            return Ok(CleanupReport { deleted_count: 0, freed_bytes: 0 });
        }

        let need_to_free = total - target_usage;
        tracing::info!("Cache cleanup: {total} bytes used, need to free {need_to_free}");

        // Collect all cache entries with last_access
        let mut entries: Vec<(InfoHash, CacheEntry)> = inner.qdata_files.iter()
            .map(|(hash, handle)| {
                (*hash, CacheEntry {
                    info_hash: *hash,
                    file_size: handle.file_size,
                    downloaded: handle.bitfield.count() as u64 * PIECE_LENGTH,
                    bitfield: handle.bitfield.clone(),
                    last_access: handle.last_access,
                    created_at: handle.created_at,
                })
            })
            .collect();

        // Sort by last_access ascending (oldest first)
        entries.sort_by_key(|(_, e)| e.last_access);

        for (hash, entry) in &entries {
            if freed >= need_to_free {
                break;
            }

            // Don't evict entries that are currently being downloaded
            if self.is_active_download(hash) {
                continue;
            }

            // Delete files
            let qdata_path = inner.config.cache_dir
                .join("qdata")
                .join(format!("{}.qdata", hex::encode(hash)));
            let qmv_path = inner.config.cache_dir
                .join("qmv")
                .join(format!("{}.qmv", hex::encode(hash)));

            let _ = std::fs::remove_file(&qdata_path);
            let _ = std::fs::remove_file(&qmv_path);

            // Remove from memory
            inner.qdata_files.remove(hash);
            inner.metadata.remove(hash);
            inner.lru_list.retain(|h| h != hash);

            freed += entry.file_size;
            deleted += 1;
            tracing::debug!("Evicted cache entry: {}", hex::encode(hash));
        }

        // Persist config with updated size
        inner.config.current_size = total.saturating_sub(freed);

        Ok(CleanupReport {
            deleted_count: deleted,
            freed_bytes: freed,
        })
    }
}
```

---

## 6. Cache Integrity Verification

### 6.1 Verification Strategy

| Scenario | Action |
|----------|--------|
| On cache lookup | Verify .qdata + .qmv both exist; load metadata |
| On piece write | SHA-1 verify piece against FileMeta.pieces |
| On piece read | SHA-1 verify (if `verify_on_read`) |
| Periodic scrub | Walk all cache entries; verify all pieces |
| Corruption detected | Mark piece as incomplete; redownload |

### 6.2 Periodic Scrub

```rust
impl CacheManager {
    pub async fn scrub(&self, info_hash: &InfoHash) -> Result<ScrubReport> {
        let inner = self.inner.lock().unwrap();
        let handle = inner.qdata_files.get(info_hash)
            .ok_or(CacheError::EntryNotFound(*info_hash))?;
        let meta = inner.metadata.get(info_hash)
            .ok_or(CacheError::EntryNotFound(*info_hash))?;

        let mut bad_pieces = Vec::new();
        let mut verified = 0u32;

        for i in 0..handle.bitfield.piece_count() {
            if !handle.bitfield.has(i) {
                continue;
            }

            let piece_len = if i == handle.bitfield.piece_count() - 1 {
                let rem = handle.file_size % PIECE_LENGTH;
                if rem == 0 { PIECE_LENGTH } else { rem }
            } else {
                PIECE_LENGTH
            };

            let offset = i as u64 * PIECE_LENGTH;
            let mut buf = vec![0u8; piece_len as usize];
            match handle.file.read_exact_at(&mut buf, offset).await {
                Ok(_) => {},
                Err(e) => {
                    bad_pieces.push((i, format!("read error: {e}")));
                    continue;
                }
            }

            let hash = sha1::Sha1::from(&buf).digest().bytes();
            let expected = &meta.pieces[i as usize];
            if hash != *expected {
                bad_pieces.push((i, format!("hash mismatch")));
            } else {
                verified += 1;
            }
        }

        // Mark bad pieces as incomplete
        for (idx, _) in &bad_pieces {
            handle.bitfield.set(*idx, false);
        }
        Self::flush_metadata(&inner, info_hash)?;

        Ok(ScrubReport {
            total_pieces: handle.bitfield.piece_count(),
            verified,
            corrupted: bad_pieces.len() as u32,
            bad_pieces: bad_pieces.into_iter().map(|(i, _)| i).collect(),
        })
    }
}

pub struct ScrubReport {
    pub total_pieces: u32,
    pub verified: u32,
    pub corrupted: u32,
    pub bad_pieces: Vec<u32>,
}
```

---

## 7. Concurrent Access Patterns

### 7.1 Locking Strategy

```
┌─────────────────────────────────────────────────┐
│                  CacheManager                    │
│                                                 │
│  ┌─────────────────────────────────────────┐    │
│  │           Mutex<CacheInner>             │    │
│  │                                         │    │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐ │    │
│  │  │ Qdata   │  │ Qdata   │  │ Qdata   │ │    │
│  │  │ File 1  │  │ File 2  │  │ File N  │ │    │
│  │  └─────────┘  └─────────┘  └─────────┘ │    │
│  │                                         │    │
│  │  LRU: [hash_3] [hash_1] [hash_5] ...   │    │
│  └─────────────────────────────────────────┘    │
└─────────────────────────────────────────────────┘
```

### 7.2 Concurrency Rules

1. **One lock per CacheManager**: All operations acquire the same `Mutex<CacheInner>`
2. **No nested locks**: Avoid holding CacheInner lock while acquiring external locks
3. **Async-aware blocking**: Use `std::sync::Mutex` (not tokio::sync::Mutex) for short-lived operations (file seeks, metadata updates); wrap async I/O in separate scope
4. **Read/Write ordering**: Writes are serialized per info_hash; reads may proceed concurrently on different handlers

### 7.3 Multiple Writer Coordination

```rust
impl CacheManager {
    /// Thread-safe piece write. Guards against concurrent writes
    /// to the same piece by different connections.
    pub async fn write_piece_exclusive(
        &self,
        info_hash: &InfoHash,
        index: u32,
        data: &[u8],
    ) -> Result<(), CacheError> {
        let piece_lock = self.get_piece_lock(info_hash, index);

        let _guard = piece_lock.lock().await;

        // Double-check: another writer may have completed this piece
        if self.is_piece_complete(info_hash, index)? {
            return Ok(());
        }

        let offset = index as u64 * PIECE_LENGTH;
        self.write(info_hash, offset, data).await
    }

    /// Per-piece lock table to allow concurrent writes to different pieces
    fn get_piece_lock(&self, info_hash: &InfoHash, index: u32) -> Arc<tokio::sync::Mutex<()>> {
        let mut inner = self.inner.lock().unwrap();
        let key = (*info_hash, index);
        inner.piece_locks
            .entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}
```

---

## 8. Pre-Allocation Strategy

### 8.1 Strategy Selection

| Strategy | Disk Space | Speed | Fragmentation | Linux | macOS | Windows |
|----------|-----------|-------|---------------|-------|-------|---------|
| `sparse` | Minimal | Fast | Low | ✓ | ✓ | ✓* |
| `fallocate_keep_size` | Reserved | Fast | Low | ✓ | ✗ | ✗ |
| `zero_range` | Full | Slow | None | ✓ | ✗ | ✗ |
| `none` | Minimal | Fast | High | ✓ | ✓ | ✓ |

*On Windows, sparse file support requires `FSCTL_SET_SPARSE` via `CreateFile`.

### 8.2 Implementation Decision

```rust
impl CacheManager {
    pub fn create_entry(&self, info_hash: &InfoHash, file_size: u64, meta: Arc<FileMeta>) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();

        let qdata_dir = inner.config.cache_dir.join("qdata");
        let qmv_dir = inner.config.cache_dir.join("qmv");

        std::fs::create_dir_all(&qdata_dir)?;
        std::fs::create_dir_all(&qmv_dir)?;

        let qdata_path = qdata_dir.join(format!("{}.qdata", hex::encode(info_hash)));
        let qmv_path = qmv_dir.join(format!("{}.qmv", hex::encode(info_hash)));

        // Create sparse .qdata file
        match inner.config.allocation_strategy {
            AllocationStrategy::Sparse => {
                let f = std::fs::File::create(&qdata_path)?;
                f.set_len(file_size)?;
            }
            AllocationStrategy::FallocateKeepSize => {
                #[cfg(target_os = "linux")]
                {
                    let f = std::fs::File::create(&qdata_path)?;
                    f.set_len(file_size)?;
                    let fd = f.as_raw_fd();
                    unsafe {
                        libc::fallocate(fd, libc::FALLOC_FL_KEEP_SIZE, 0, file_size as libc::off_t);
                    }
                }
                #[cfg(not(target_os = "linux"))]
                return Err(CacheError::UnsupportedPlatform("fallocate".into()));
            }
            AllocationStrategy::ZeroRange => {
                #[cfg(target_os = "linux")]
                {
                    let f = std::fs::File::create(&qdata_path)?;
                    f.set_len(file_size)?;
                    let fd = f.as_raw_fd();
                    unsafe {
                        libc::fallocate(fd, 0, 0, file_size as libc::off_t);
                    }
                }
                #[cfg(not(target_os = "linux"))]
                return Err(CacheError::UnsupportedPlatform("fallocate".into()));
            }
            AllocationStrategy::None => {
                // Create empty file; will grow on writes
                std::fs::File::create(&qdata_path)?;
            }
        }

        // Write .qmv metadata
        let qmv_data = meta.encode_bencode();
        std::fs::write(&qmv_path, &qmv_data)?;

        // Open qdata file handle
        let qdata_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&qdata_path)?;

        let piece_count = ((file_size + PIECE_LENGTH - 1) / PIECE_LENGTH) as u32;

        inner.qdata_files.insert(*info_hash, QdataHandle {
            file: tokio::fs::File::from_std(qdata_file),
            bitfield: Bitfield::new(piece_count),
            file_size,
            last_access: Instant::now(),
            created_at: Instant::now(),
        });
        inner.metadata.insert(*info_hash, meta);
        inner.lru_list.push_front(*info_hash);

        Ok(())
    }
}
```

---

## 9. Cache Entry Lifecycle

```
  ┌──────────────────┐
  │   NOT CACHED     │
  └────────┬─────────┘
           │
           ▼  play() → check cache → miss
  ┌──────────────────┐
  │  CREATING        │── create_entry() → allocate .qdata + .qmv
  └────────┬─────────┘
           │
           ▼  P2P download starts
  ┌──────────────────┐
  │  DOWNLOADING     │── write_piece_exclusive() for each piece
  │  (partial)       │── bitfield updated piece by piece
  └────────┬─────────┘
           │
           ▼  all pieces complete
  ┌──────────────────┐
  │  COMPLETE        │── .qmv bitfield = all 1s
  └────────┬─────────┘
           │
           ├──▶ on play → CacheEntry returned immediately
           │
           └──▶ on LRU eviction → files deleted
                    │
                    ▼
           ┌──────────────────┐
           │   EVICTED        │── return to NOT CACHED
           └──────────────────┘
```

---

## 10. Configuration

```rust
pub struct CacheConfig {
    pub cache_dir: PathBuf,              // ~/.cache/qvod
    pub max_size: u64,                   // 4 GB default
    pub allocation_strategy: AllocationStrategy,
    pub verify_on_read: bool,            // default: true
    pub verify_on_write: bool,           // default: true
    pub piece_length: u64,               // 262144
    pub scrub_interval: Duration,        // 24 hours
}

pub struct CleanupReport {
    pub deleted_count: u32,
    pub freed_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cache entry not found: {0}")]
    EntryNotFound(InfoHash),
    #[error("invalid data length: expected {expected}, got {actual}")]
    InvalidLength { expected: u64, actual: u64 },
    #[error("piece {index} verification failed")]
    PieceVerification { index: u32, expected: [u8; 20], actual: [u8; 20] },
    #[error("piece {index} is missing")]
    PieceMissing { index: u32 },
    #[error("cache corrupted: {0}")]
    Corrupted(String),
    #[error("unsupported platform for allocation strategy")]
    UnsupportedPlatform(String),
    #[error("cache full")]
    CacheFull,
}
```

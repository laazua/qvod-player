# QVOD Database Schema Reference

## 1. Overview

QVOD uses SQLite for persistent metadata storage. The database is optional — QVOD can function without it using filesystem-only cache management — but enables efficient queries for peer discovery, download history, and peer reputation.

### 1.1 Database Files

| File | Location | Purpose |
|------|----------|---------|
| `cache.db` | `{cache_dir}/db/cache.db` | Cache master index |
| `downloads.db` | `{cache_dir}/db/downloads.db` | Download history & peer reputation |
| `tracker.db` | `{cache_dir}/db/tracker.db` | Tracker response cache & peer lists |

### 1.2 Connection Management

```rust
pub struct DatabaseManager {
    cache_db: Option<Connection>,
    downloads_db: Option<Connection>,
    tracker_db: Option<Connection>,
}

impl DatabaseManager {
    pub fn open(cache_dir: &Path) -> Result<Self> {
        let db_dir = cache_dir.join("db");
        std::fs::create_dir_all(&db_dir)?;

        Ok(Self {
            cache_db: Self::open_or_none(db_dir.join("cache.db"))?,
            downloads_db: Self::open_or_none(db_dir.join("downloads.db"))?,
            tracker_db: Self::open_or_none(db_dir.join("tracker.db"))?,
        })
    }

    fn open_or_none(path: PathBuf) -> Result<Option<Connection>> {
        match Connection::open(&path) {
            Ok(conn) => {
                conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
                Ok(Some(conn))
            }
            Err(e) => {
                tracing::warn!("Failed to open database {path:?}: {e}");
                Ok(None)
            }
        }
    }
}
```

---

## 2. cache.db Schema

### 2.1 cache_entries

Master index of all cached resources.

```sql
CREATE TABLE IF NOT EXISTS cache_entries (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    info_hash     BLOB    NOT NULL UNIQUE,       -- 20-byte SHA-1
    filename      TEXT    NOT NULL,
    file_size     INTEGER NOT NULL,
    piece_length  INTEGER NOT NULL DEFAULT 262144,
    piece_count   INTEGER NOT NULL,
    downloaded    INTEGER NOT NULL DEFAULT 0,     -- bytes downloaded
    bitfield      BLOB,                            -- serialized bitfield
    last_access   INTEGER NOT NULL,                -- unix timestamp
    created_at    INTEGER NOT NULL,                 -- unix timestamp
    completed_at  INTEGER,                          -- when download finished (NULL=incomplete)
    metadata_b64  TEXT,                             -- base64-encoded .qmv content
    format        TEXT,                              -- video format hint
    duration_ms   INTEGER,                          -- duration in milliseconds
    bitrate       INTEGER                           -- bitrate in bps
);

CREATE INDEX IF NOT EXISTS idx_cache_info_hash ON cache_entries(info_hash);
CREATE INDEX IF NOT EXISTS idx_cache_last_access ON cache_entries(last_access);
CREATE INDEX IF NOT EXISTS idx_cache_completed ON cache_entries(completed_at);
CREATE INDEX IF NOT EXISTS idx_cache_format ON cache_entries(format);
```

### 2.2 cache_pieces

Piece-level tracking for partial downloads.

```sql
CREATE TABLE IF NOT EXISTS cache_pieces (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    cache_id      INTEGER NOT NULL REFERENCES cache_entries(id) ON DELETE CASCADE,
    piece_index   INTEGER NOT NULL,
    piece_hash    BLOB    NOT NULL,                -- 20-byte SHA-1 of piece
    size          INTEGER NOT NULL,
    downloaded    INTEGER NOT NULL DEFAULT 0,      -- bytes downloaded for this piece
    verified      INTEGER NOT NULL DEFAULT 0,      -- 0=unverified, 1=verified
    UNIQUE(cache_id, piece_index)
);

CREATE INDEX IF NOT EXISTS idx_cache_pieces_cache_id ON cache_pieces(cache_id);
```

### 2.3 cache_blocks

Block-level tracking (for resumed downloads).

```sql
CREATE TABLE IF NOT EXISTS cache_blocks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    piece_id      INTEGER NOT NULL REFERENCES cache_pieces(id) ON DELETE CASCADE,
    block_offset  INTEGER NOT NULL,                -- byte offset within piece
    block_size    INTEGER NOT NULL,                -- typically 16384
    completed     INTEGER NOT NULL DEFAULT 0,       -- 0=missing, 1=complete
    source_peer   BLOB,                             -- peer_id that provided this block
    downloaded_at INTEGER,                          -- unix timestamp
    UNIQUE(piece_id, block_offset)
);

CREATE INDEX IF NOT EXISTS idx_cache_blocks_piece_id ON cache_blocks(piece_id);
CREATE INDEX IF NOT EXISTS idx_cache_blocks_completed ON cache_blocks(completed);
```

### 2.4 Query Examples

**Find all incomplete cache entries, oldest first:**
```sql
SELECT info_hash, filename, file_size, downloaded,
       CAST(downloaded AS FLOAT) / file_size AS progress
FROM cache_entries
WHERE completed_at IS NULL
ORDER BY last_access ASC;
```

**Get bitfield for a specific info_hash:**
```sql
SELECT bitfield FROM cache_entries
WHERE info_hash = x'a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9';
```

**Calculate total cache usage:**
```sql
SELECT COUNT(*)        AS total_files,
       SUM(file_size)  AS total_bytes,
       SUM(downloaded) AS downloaded_bytes
FROM cache_entries;
```

**Find cache entries to evict (LRU, not currently downloading):**
```sql
SELECT id, info_hash, file_size, last_access
FROM cache_entries
WHERE completed_at IS NOT NULL
   OR completed_at IS NULL AND last_access < strftime('%s', 'now', '-7 days')
ORDER BY last_access ASC
LIMIT 10;
```

**Get piece availability for a cache entry:**
```sql
SELECT piece_index, size, downloaded, verified
FROM cache_pieces
WHERE cache_id = (SELECT id FROM cache_entries WHERE info_hash = x'...')
ORDER BY piece_index;
```

---

## 3. downloads.db Schema

### 3.1 download_history

Records of all download sessions.

```sql
CREATE TABLE IF NOT EXISTS download_history (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    info_hash     BLOB    NOT NULL,
    filename      TEXT    NOT NULL,
    file_size     INTEGER NOT NULL,
    format        TEXT,
    started_at    INTEGER NOT NULL,                -- unix timestamp
    completed_at  INTEGER,                          -- NULL if incomplete
    status        TEXT    NOT NULL DEFAULT 'active', -- active | paused | completed | failed | cancelled
    total_downloaded INTEGER NOT NULL DEFAULT 0,
    total_uploaded   INTEGER NOT NULL DEFAULT 0,
    peak_peers    INTEGER NOT NULL DEFAULT 0,
    peak_speed    INTEGER NOT NULL DEFAULT 0,       -- bytes/sec
    error_message TEXT,
    source_uri    TEXT                               -- original qvod:// URI
);

CREATE INDEX IF NOT EXISTS idx_history_info_hash ON download_history(info_hash);
CREATE INDEX IF NOT EXISTS idx_history_started_at ON download_history(started_at);
CREATE INDEX IF NOT EXISTS idx_history_status ON download_history(status);
```

### 3.2 download_peers

Peers encountered during downloads.

```sql
CREATE TABLE IF NOT EXISTS download_peers (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    download_id     INTEGER NOT NULL REFERENCES download_history(id) ON DELETE CASCADE,
    peer_id         BLOB    NOT NULL,               -- 20-byte peer ID
    peer_addr       TEXT    NOT NULL,                -- "IP:Port"
    first_seen      INTEGER NOT NULL,
    last_seen       INTEGER,
    bytes_uploaded  INTEGER NOT NULL DEFAULT 0,
    bytes_downloaded INTEGER NOT NULL DEFAULT 0,
    is_seeder       INTEGER NOT NULL DEFAULT 0,
    disconnect_reason TEXT,
    UNIQUE(download_id, peer_id)
);

CREATE INDEX IF NOT EXISTS idx_dl_peers_download_id ON download_peers(download_id);
CREATE INDEX IF NOT EXISTS idx_dl_peers_peer_id ON download_peers(peer_id);
```

### 3.3 peer_reputation

Cross-session peer reputation tracking.

```sql
CREATE TABLE IF NOT EXISTS peer_reputation (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    peer_id         BLOB    NOT NULL UNIQUE,        -- 20-byte peer ID
    first_seen      INTEGER NOT NULL,
    last_seen       INTEGER,
    total_sessions  INTEGER NOT NULL DEFAULT 0,
    total_uploaded  INTEGER NOT NULL DEFAULT 0,
    total_downloaded INTEGER NOT NULL DEFAULT 0,
    total_timeouts  INTEGER NOT NULL DEFAULT 0,
    total_failures  INTEGER NOT NULL DEFAULT 0,
    avg_speed_up    INTEGER,                         -- bytes/sec
    avg_speed_down  INTEGER,
    avg_rtt_ms      REAL,
    avg_loss_rate   REAL,
    reputation_score REAL DEFAULT 0.0,               -- computed score
    is_blacklisted  INTEGER NOT NULL DEFAULT 0,
    blacklist_reason TEXT,
    last_location   TEXT                              -- geographic hint
);

CREATE INDEX IF NOT EXISTS idx_reputation_peer_id ON peer_reputation(peer_id);
CREATE INDEX IF NOT EXISTS idx_reputation_score ON peer_reputation(reputation_score DESC);
CREATE INDEX IF NOT EXISTS idx_reputation_blacklisted ON peer_reputation(is_blacklisted)
    WHERE is_blacklisted = 1;
```

### 3.4 peer_reputation_events

Detailed event log for reputation calculation.

```sql
CREATE TABLE IF NOT EXISTS peer_reputation_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    peer_id         BLOB    NOT NULL REFERENCES peer_reputation(peer_id),
    event_type      TEXT    NOT NULL,                -- connect | disconnect | timeout | piece_ok | piece_bad | slow | banned
    occurred_at     INTEGER NOT NULL,
    duration_ms     INTEGER,                          -- connection duration (for disconnect events)
    bytes_transferred INTEGER,
    details         TEXT                               -- JSON-encoded event details
);

CREATE INDEX IF NOT EXISTS idx_reputation_events_peer ON peer_reputation_events(peer_id);
CREATE INDEX IF NOT EXISTS idx_reputation_events_type ON peer_reputation_events(event_type);
CREATE INDEX IF NOT EXISTS idx_reputation_events_time ON peer_reputation_events(occurred_at);
```

### 3.5 peer_reputation_aggregates

Materialized aggregate statistics for fast scoring.

```sql
CREATE TABLE IF NOT EXISTS peer_reputation_aggregates (
    peer_id           BLOB PRIMARY KEY REFERENCES peer_reputation(peer_id),
    connection_success_rate REAL,                    -- successful / total connections
    avg_uptime_seconds      REAL,
    upload_to_download_ratio REAL,                   -- bytes up / bytes down
    pieces_served           INTEGER,
    pieces_received         INTEGER,
    last_24h_speed_up       INTEGER,
    last_24h_speed_down     INTEGER,
    computed_at             INTEGER                  -- last computation timestamp
);
```

### 3.6 Query Examples

**Get download history for a specific info_hash:**
```sql
SELECT id, started_at, completed_at, status,
       CAST(total_downloaded AS FLOAT) / file_size AS progress,
       peak_speed
FROM download_history
WHERE info_hash = x'a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9'
ORDER BY started_at DESC;
```

**Top-rated peers in last 24 hours:**
```sql
SELECT p.peer_id, p.avg_speed_down, p.avg_rtt_ms, p.reputation_score,
       COALESCE(a.connection_success_rate, 0) AS reliability
FROM peer_reputation p
LEFT JOIN peer_reputation_aggregates a ON p.peer_id = a.peer_id
WHERE p.is_blacklisted = 0
  AND p.last_seen > strftime('%s', 'now', '-1 day')
ORDER BY p.avg_speed_down DESC, p.reputation_score DESC
LIMIT 20;
```

**Peers that provided the most data:**
```sql
SELECT peer_id, total_uploaded, total_downloaded,
       total_sessions, last_seen
FROM peer_reputation
ORDER BY total_uploaded DESC
LIMIT 50;
```

**Blacklisted peers:**
```sql
SELECT peer_id, blacklist_reason, total_failures, total_timeouts
FROM peer_reputation
WHERE is_blacklisted = 1
ORDER BY last_seen DESC;
```

**Download session summary:**
```sql
SELECT DATE(started_at, 'unixepoch') AS day,
       COUNT(*) AS downloads,
       SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed,
       SUM(file_size) / 1048576.0 AS total_gb
FROM download_history
WHERE started_at > strftime('%s', 'now', '-30 days')
GROUP BY DATE(started_at, 'unixepoch')
ORDER BY day DESC;
```

---

## 4. tracker.db Schema

### 4.1 tracker_announce_cache

Cached tracker announce responses to reduce redundant requests.

```sql
CREATE TABLE IF NOT EXISTS tracker_announce_cache (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    info_hash       BLOB    NOT NULL,
    tracker_url     TEXT    NOT NULL,
    cached_at       INTEGER NOT NULL,                -- unix timestamp
    expires_at      INTEGER NOT NULL,                -- cached_at + response.interval
    peer_count      INTEGER NOT NULL,
    seed_count      INTEGER NOT NULL,
    leech_count     INTEGER NOT NULL,
    response_bencode BLOB,                            -- raw response for re-parsing
    UNIQUE(info_hash, tracker_url)
);

CREATE INDEX IF NOT EXISTS idx_tracker_cache_info ON tracker_announce_cache(info_hash);
CREATE INDEX IF NOT EXISTS idx_tracker_cache_expires ON tracker_announce_cache(expires_at);
```

### 4.2 tracker_scrape_cache

Cached scrape responses.

```sql
CREATE TABLE IF NOT EXISTS tracker_scrape_cache (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    info_hash       BLOB    NOT NULL,
    tracker_url     TEXT    NOT NULL,
    cached_at       INTEGER NOT NULL,
    complete        INTEGER NOT NULL,
    incomplete      INTEGER NOT NULL,
    downloaded      INTEGER NOT NULL,
    UNIQUE(info_hash, tracker_url)
);

CREATE INDEX IF NOT EXISTS idx_scrape_cache_info ON tracker_scrape_cache(info_hash);
```

### 4.3 tracker_failures

Tracker reliability tracking.

```sql
CREATE TABLE IF NOT EXISTS tracker_failures (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    tracker_url     TEXT    NOT NULL,
    failed_at       INTEGER NOT NULL,
    error_code      INTEGER,                          -- HTTP status or network error code
    error_message   TEXT,
    UNIQUE(tracker_url, failed_at)
);

CREATE INDEX IF NOT EXISTS idx_tracker_failures_url ON tracker_failures(tracker_url);
CREATE INDEX IF NOT EXISTS idx_tracker_failures_time ON tracker_failures(failed_at);
```

### 4.4 Query Examples

**Get valid cached peers (not expired):**
```sql
SELECT info_hash, tracker_url, peer_count, seed_count, leech_count
FROM tracker_announce_cache
WHERE expires_at > strftime('%s', 'now')
ORDER BY peer_count DESC;
```

**Prune expired cache entries:**
```sql
DELETE FROM tracker_announce_cache
WHERE expires_at < strftime('%s', 'now', '-1 day');
```

---

## 5. Index Definitions

### 5.1 Complete Index List

```sql
-- cache.db indexes
CREATE INDEX IF NOT EXISTS idx_cache_info_hash      ON cache_entries(info_hash);
CREATE INDEX IF NOT EXISTS idx_cache_last_access     ON cache_entries(last_access);
CREATE INDEX IF NOT EXISTS idx_cache_completed       ON cache_entries(completed_at);
CREATE INDEX IF NOT EXISTS idx_cache_format          ON cache_entries(format);
CREATE INDEX IF NOT EXISTS idx_cache_pieces_cache_id ON cache_pieces(cache_id);
CREATE INDEX IF NOT EXISTS idx_cache_blocks_piece_id ON cache_blocks(piece_id);
CREATE INDEX IF NOT EXISTS idx_cache_blocks_completed ON cache_blocks(completed);

-- downloads.db indexes
CREATE INDEX IF NOT EXISTS idx_history_info_hash     ON download_history(info_hash);
CREATE INDEX IF NOT EXISTS idx_history_started_at    ON download_history(started_at);
CREATE INDEX IF NOT EXISTS idx_history_status        ON download_history(status);
CREATE INDEX IF NOT EXISTS idx_dl_peers_download_id  ON download_peers(download_id);
CREATE INDEX IF NOT EXISTS idx_dl_peers_peer_id      ON download_peers(peer_id);
CREATE INDEX IF NOT EXISTS idx_reputation_peer_id    ON peer_reputation(peer_id);
CREATE INDEX IF NOT EXISTS idx_reputation_score      ON peer_reputation(reputation_score DESC);
CREATE INDEX IF NOT EXISTS idx_reputation_blacklisted ON peer_reputation(is_blacklisted) WHERE is_blacklisted = 1;
CREATE INDEX IF NOT EXISTS idx_reputation_events_peer ON peer_reputation_events(peer_id);
CREATE INDEX IF NOT EXISTS idx_reputation_events_type ON peer_reputation_events(event_type);
CREATE INDEX IF NOT EXISTS idx_reputation_events_time ON peer_reputation_events(occurred_at);

-- tracker.db indexes
CREATE INDEX IF NOT EXISTS idx_tracker_cache_info    ON tracker_announce_cache(info_hash);
CREATE INDEX IF NOT EXISTS idx_tracker_cache_expires ON tracker_announce_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_scrape_cache_info     ON tracker_scrape_cache(info_hash);
CREATE INDEX IF NOT EXISTS idx_tracker_failures_url  ON tracker_failures(tracker_url);
CREATE INDEX IF NOT EXISTS idx_tracker_failures_time ON tracker_failures(failed_at);
```

---

## 6. Migration Strategy

### 6.1 Schema Versioning

```sql
CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  INTEGER NOT NULL,           -- unix timestamp
    description TEXT
);

-- Current version: 1
INSERT OR IGNORE INTO schema_version (version, applied_at, description)
VALUES (1, strftime('%s', 'now'), 'Initial schema');
```

### 6.2 Migration Procedure

```rust
pub struct Migration {
    pub version: u32,
    pub description: &'static str,
    pub statements: &'static [&'static str],
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial schema for cache.db",
        statements: &[
            "CREATE TABLE IF NOT EXISTS cache_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                info_hash BLOB NOT NULL UNIQUE,
                filename TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                piece_length INTEGER NOT NULL DEFAULT 262144,
                piece_count INTEGER NOT NULL,
                downloaded INTEGER NOT NULL DEFAULT 0,
                bitfield BLOB,
                last_access INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                completed_at INTEGER,
                metadata_b64 TEXT,
                format TEXT,
                duration_ms INTEGER,
                bitrate INTEGER
            )",
            "CREATE TABLE IF NOT EXISTS cache_pieces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                cache_id INTEGER NOT NULL REFERENCES cache_entries(id) ON DELETE CASCADE,
                piece_index INTEGER NOT NULL,
                piece_hash BLOB NOT NULL,
                size INTEGER NOT NULL,
                downloaded INTEGER NOT NULL DEFAULT 0,
                verified INTEGER NOT NULL DEFAULT 0,
                UNIQUE(cache_id, piece_index)
            )",
            "CREATE TABLE IF NOT EXISTS cache_blocks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                piece_id INTEGER NOT NULL REFERENCES cache_pieces(id) ON DELETE CASCADE,
                block_offset INTEGER NOT NULL,
                block_size INTEGER NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0,
                source_peer BLOB,
                downloaded_at INTEGER,
                UNIQUE(piece_id, block_offset)
            )",
            "CREATE INDEX IF NOT EXISTS idx_cache_info_hash ON cache_entries(info_hash)",
            "CREATE INDEX IF NOT EXISTS idx_cache_last_access ON cache_entries(last_access)",
            "CREATE INDEX IF NOT EXISTS idx_cache_completed ON cache_entries(completed_at)",
            "CREATE INDEX IF NOT EXISTS idx_cache_pieces_cache_id ON cache_pieces(cache_id)",
            "CREATE INDEX IF NOT EXISTS idx_cache_blocks_piece_id ON cache_blocks(piece_id)",
            "CREATE INDEX IF NOT EXISTS idx_cache_blocks_completed ON cache_blocks(completed)",
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL,
                description TEXT
            )",
            "INSERT OR IGNORE INTO schema_version (version, applied_at, description)
             VALUES (1, strftime('%s', 'now'), 'Initial schema')",
        ],
    },
    // Future migrations go here:
    // Migration {
    //     version: 2,
    //     description: "Add source_uri to download_history",
    //     statements: &[
    //         "ALTER TABLE download_history ADD COLUMN source_uri TEXT",
    //     ],
    // },
];

pub fn run_migrations(conn: &Connection, db_type: &str) -> Result<()> {
    let current_version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for migration in MIGRATIONS {
        if migration.version > current_version {
            tracing::info!("Running migration v{}: {}", migration.version, migration.description);
            for stmt in migration.statements {
                conn.execute_batch(stmt)?;
            }
        }
    }

    Ok(())
}
```

### 6.3 Migration History

| Version | Description | Date |
|---------|-------------|------|
| 1 | Initial schema (cache entries, pieces, blocks) | 2025-07-01 |
| 2 | Planned: add source_uri to download_history | — |

---

## 7. Database Maintenance

### 7.1 Periodic Maintenance

```sql
-- Vacuum to reclaim space (run weekly)
VACUUM;

-- Rebuild indexes (run after large deletions)
REINDEX;

-- Analyze query plans
ANALYZE;

-- WAL checkpoint (run on shutdown)
PRAGMA wal_checkpoint(TRUNCATE);
```

### 7.2 Cleanup Stale Data

```sql
-- Remove tracker cache entries older than 7 days
DELETE FROM tracker_announce_cache
WHERE cached_at < strftime('%s', 'now', '-7 days');

-- Remove old peer reputation events (keep 90 days)
DELETE FROM peer_reputation_events
WHERE occurred_at < strftime('%s', 'now', '-90 days');

-- Mark stale downloads as cancelled (no activity for 30 days)
UPDATE download_history
SET status = 'cancelled'
WHERE status = 'active'
  AND started_at < strftime('%s', 'now', '-30 days')
  AND completed_at IS NULL;

-- Reset poor reputation peers after 180 days
UPDATE peer_reputation
SET total_failures = 0, total_timeouts = 0,
    reputation_score = 0.0, is_blacklisted = 0
WHERE last_seen < strftime('%s', 'now', '-180 days');
```

### 7.3 Rust Maintenance API

```rust
pub struct DatabaseMaintenance;

impl DatabaseMaintenance {
    pub fn vacuum(conn: &Connection) -> Result<()> {
        conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    pub fn reindex(conn: &Connection) -> Result<()> {
        conn.execute_batch("REINDEX;")?;
        Ok(())
    }

    pub fn analyze(conn: &Connection) -> Result<()> {
        conn.execute_batch("ANALYZE;")?;
        Ok(())
    }

    pub fn wal_checkpoint(conn: &Connection) -> Result<()> {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
}
```

---

## 8. Reputation Score Calculation

### 8.1 Scoring Formula

```sql
-- Update reputation scores
UPDATE peer_reputation
SET reputation_score =
    -- Base score (0-100)
    50.0
    -- Speed bonus: up to +30 points for fast peers
    + CASE WHEN avg_speed_down IS NOT NULL
        THEN LEAST(avg_speed_down / 1048576.0 * 10, 30.0)
        ELSE 0.0
      END
    -- Reliability bonus: up to +20 points for low failure rates
    + CASE WHEN total_sessions > 0
        THEN (1.0 - CAST(total_failures + total_timeouts AS REAL)
              / CASE WHEN total_sessions > 0 THEN total_sessions ELSE 1 END) * 20.0
        ELSE 0.0
      END
    -- Session count bonus: up to +10 points for established peers
    + CASE
        WHEN total_sessions >= 100 THEN 10.0
        WHEN total_sessions >= 50  THEN 7.0
        WHEN total_sessions >= 10  THEN 4.0
        WHEN total_sessions >= 3   THEN 2.0
        ELSE 0.0
      END
    -- Recency bonus: up to +10 points for recently seen peers
    + CASE WHEN last_seen IS NOT NULL
        THEN CASE
            WHEN last_seen > strftime('%s', 'now', '-1 hour')  THEN 10.0
            WHEN last_seen > strftime('%s', 'now', '-1 day')   THEN 7.0
            WHEN last_seen > strftime('%s', 'now', '-7 days')  THEN 4.0
            ELSE 0.0
        END
        ELSE 0.0
      END
    -- Blacklist penalty: -100 for banned peers
    - CASE WHEN is_blacklisted = 1 THEN 100.0 ELSE 0.0 END
WHERE total_sessions > 0;
```

### 8.2 Auto-Blacklist Threshold

```sql
-- Automatically blacklist peers with >90% failure rate across 10+ sessions
UPDATE peer_reputation
SET is_blacklisted = 1,
    blacklist_reason = 'High failure rate (>90%)'
WHERE total_sessions >= 10
  AND (CAST(total_failures + total_timeouts AS REAL) / total_sessions) > 0.9
  AND is_blacklisted = 0;

-- Automatically blacklist peers that never provided data
UPDATE peer_reputation
SET is_blacklisted = 1,
    blacklist_reason = 'Zero upload across multiple sessions'
WHERE total_sessions >= 5
  AND total_uploaded = 0
  AND is_blacklisted = 0;
```

---

## 9. Performance Considerations

| Aspect | Recommendation |
|--------|---------------|
| Journal mode | WAL (Write-Ahead Logging) for concurrent read/write |
| Synchronous | NORMAL (balance between safety and speed) |
| Cache size | 64 MB (default; adjust based on available memory) |
| Page size | 4096 bytes (default) |
| Temp store | MEMORY for temp tables |
| Foreign keys | ON (enforce cascade deletes) |
| Busy timeout | 5000 ms (wait up to 5s for locked database) |

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -64000;          -- 64 MB
PRAGMA page_size = 4096;
PRAGMA temp_store = MEMORY;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

---

## 10. Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Migration error at version {version}: {message}")]
    Migration { version: u32, message: String },

    #[error("Database not available: {0}")]
    NotAvailable(String),

    #[error("Serialization error: {0}")]
    Serialize(String),

    #[error("Deserialization error: {0}")]
    Deserialize(String),
}
```

---

## 11. Initialization Script

Complete initialization for all three databases:

```rust
pub fn initialize_databases(cache_dir: &Path) -> Result<DatabaseManager> {
    let db_dir = cache_dir.join("db");
    std::fs::create_dir_all(&db_dir)?;

    // Open connections
    let cache_conn = Connection::open(db_dir.join("cache.db"))?;
    let downloads_conn = Connection::open(db_dir.join("downloads.db"))?;
    let tracker_conn = Connection::open(db_dir.join("tracker.db"))?;

    // Set pragmas
    for conn in [&cache_conn, &downloads_conn, &tracker_conn] {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-64000;
             PRAGMA temp_store=MEMORY;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;"
        )?;
    }

    // Run migrations
    run_migrations(&cache_conn, "cache")?;
    run_migrations(&downloads_conn, "downloads")?;
    run_migrations(&tracker_conn, "tracker")?;

    Ok(DatabaseManager {
        cache_db: Some(cache_conn),
        downloads_db: Some(downloads_conn),
        tracker_db: Some(tracker_conn),
    })
}
```

# QVOD Complete System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement plan task-by-task.

**Goal:** Complete all missing functionality to make a working QVOD-like P2SP streaming system.

**Architecture:** 5-layer architecture per AGENTS.md: Local HTTP Gateway (L1) → Overlay Network (L2) → P2SP Transport (L3) → Streaming Engine (L4) → Application (L5).

**Tech Stack:** Rust 2021, tokio async runtime, axum HTTP, egui GUI, reqwest HTTP client.

## Global Constraints

- Rust 2021 edition, stable toolchain
- All `Result` returns must use `QvodError` from `qvs-core`
- No `unsafe` code
- All public fns must have doc comments (`///`)
- cargo clippy --workspace -- -D warnings must pass
- cargo fmt --check must pass
- async fns use tokio runtime
- Network modules should support mock testing

---

### Phase 1: Core Infrastructure Fixes

### Task 1: Fix qvs-format CacheManager

**Files:**
- Modify: `crates/qvs-format/src/cache.rs`

- [ ] Fix `CacheManager::write` — remove `.truncate(true)`, use `append(false)` or just `create(true).write(true)` to support sparse writes at arbitrary offsets
- [ ] Add `bitfield` field to `CacheEntry` struct (use `qvs_core::Bitfield`)
- [ ] Fix `CacheManager::find` — make it return `Option<CacheEntry>` by adding a separate `find_meta` method returning `FileMeta`
- [ ] Fix `CacheManager::cleanup` — actually delete `.qdata` and `.qmv` files from disk
- [ ] Add tests for sparse write (write two non-adjacent blocks, verify both readable), cleanup file deletion, bitfield tracking
- [ ] Run `cargo test -p qvs-format && cargo clippy -p qvs-format -- -D warnings && cargo fmt`

### Task 2: Fix qvs-tracker Client

**Files:**
- Modify: `crates/qvs-tracker/src/client.rs`
- Modify: `crates/qvs-tracker/src/peer_list.rs`

- [ ] Add retry logic with exponential backoff (3 attempts, 1s/2s/4s delays) to `TrackerClient::announce` and `scrape`
- [ ] Add random multi-tracker selection: shuffle `tracker_urls` before iterating
- [ ] Fix dead code in `peer_list.rs` — either remove it or wire it into `protocol.rs` usage
- [ ] Add tests for retry behavior and tracker randomization
- [ ] Run `cargo test -p qvs-tracker && cargo clippy -p qvs-tracker -- -D warnings && cargo fmt`

---

### Phase 2: Complete DHT

### Task 3: Fix qvs-dht RoutingTable (bucket splitting)

**Files:**
- Modify: `crates/qvs-dht/src/routing.rs`

- [ ] Implement bucket splitting in `RoutingTable::insert`: when bucket is full and contains no stale entries, check if bucket index < 160, split into two buckets, redistribute entries
- [ ] Implement `KBucket::split(local_id, bucket_index)` → `(KBucket, KBucket)` that splits by the bit at `bucket_index`
- [ ] Add `RoutingTable::should_split(bucket_index)` helper
- [ ] Add tests for: bucket splits when full, entries redistributed correctly, split cascades across adjacent buckets
- [ ] Run `cargo test -p qvs-dht && cargo clippy -p qvs-dht -- -D warnings && cargo fmt`

### Task 4: Fix qvs-dht DhtNode lifecycle

**Files:**
- Modify: `crates/qvs-dht/src/node.rs`
- Modify: `crates/qvs-dht/src/bootstrap.rs`

- [ ] Add `DhtNode::stop()` method: sets a `stopped` flag, closes the socket
- [ ] Add `DhtNode::start()` to return `JoinHandle` for the event loop
- [ ] Add event loop that processes incoming UDP packets and dispatches to KademliaRpc
- [ ] Add periodic bucket refresh (calls `refresh_list()` every 900s using tokio interval)
- [ ] Fix bootstrap: add Phase 2 (FIND_NODE to seed nodes), Phase 3 (parallel α=3), Phase 4 (stop when routing table non-empty)
- [ ] Add DhtNode integration test: create node, ping, verify routing table populated
- [ ] Run `cargo test -p qvs-dht && cargo clippy -p qvs-dht -- -D warnings && cargo fmt`

### Task 5: Implement iterative find_peers in qvs-dht

**Files:**
- Modify: `crates/qvs-dht/src/node.rs`
- Modify: `crates/qvs-dht/src/krpc.rs`

- [ ] Implement iterative DHT walking for `find_peers`: start with K closest nodes from routing table, send FIND_PEERS to α=3 closest in parallel, collect responses, add new nodes not yet queried, repeat until found peers or no new nodes
- [ ] Add `KademliaRpc::find_peers_iterative(info_hash, routing_table, socket)` that implements the full Kademlia iterative lookup
- [ ] Dedup results: max 50 peers per info_hash
- [ ] Add tests with mock DHT responses
- [ ] Run `cargo test -p qvs-dht && cargo clippy -p qvs-dht -- -D warnings && cargo fmt`

---

### Phase 3: Complete Transport Layer

### Task 6: Fix qvs-transport message parsers

**Files:**
- Modify: `crates/qvs-transport/src/message.rs`

- [ ] Add `PeerMessage::parse_bitfield(msg) -> Option<Bitfield>`
- [ ] Add `PeerMessage::parse_cancel(msg) -> Option<(u32, u32, u32)>`
- [ ] Add `PeerMessage::parse_port(msg) -> Option<u16>`
- [ ] Add `PeerMessage::parse_suggest_piece(msg) -> Option<u32>`
- [ ] Add `PeerMessage::suggest_piece(piece_index) -> PeerMessage` constructor
- [ ] Add round-trip tests for all new parsers
- [ ] Run `cargo test -p qvs-transport && cargo clippy -p qvs-transport -- -D warnings && cargo fmt`

### Task 7: Implement NAT traversal in qvs-transport

**Files:**
- Modify: `crates/qvs-transport/src/nat.rs`

- [ ] Implement STUN-style `detect_nat_type()`: send UDP binding request to stun_server, compare source addr vs mapped addr, vary port to distinguish NAT types
- [ ] Implement `udp_hole_punching(addr)`: send packets from local socket to remote addr to create NAT mapping
- [ ] Implement relay_fallback(relay_addr): establish basic TCP relay connection
- [ ] Add UPnP port mapping comment/documentation (actual UPnP requires igd-next dep which may not be available)
- [ ] Add tests for NAT type detection logic
- [ ] Run `cargo test -p qvs-transport && cargo clippy -p qvs-transport -- -D warnings && cargo fmt`

### Task 8: Complete P2SP download methods

**Files:**
- Modify: `crates/qvs-transport/src/p2sp.rs`

- [ ] Implement `P2spDownloader::download_critical(piece)`: create tokio::select between P2P request and HTTP fallback, return whichever completes first
- [ ] Implement `P2spDownloader::download_high(piece)`: start P2P request, after 3s timeout start HTTP fallback, return first completion
- [ ] Implement `P2spDownloader::download_normal(piece)`: P2P only request
- [ ] Implement `P2spDownloader::download_idle(piece)`: background P2P with low priority
- [ ] Fix `select_source` to accept `piece` parameter
- [ ] Add integration tests with mock transports
- [ ] Run `cargo test -p qvs-transport && cargo clippy -p qvs-transport -- -D warnings && cargo fmt`

---

### Phase 4: Complete Stream Engine

### Task 9: Integrate QvodEngine — add tracker, dht, transport, scheduler

**Files:**
- Modify: `crates/qvs-stream/src/engine.rs`
- Modify: `crates/qvs-stream/src/seek.rs`
- Modify: `crates/qvs-stream/src/metadata.rs`

- [ ] Add fields to QvodEngine: `tracker_client: Option<TrackerClient>`, `dht_node: Option<DhtNode>`, `transport: Option<PoolManager>`, `scheduler: PieceScheduler`
- [ ] Implement proper `play()` flow:
  a. Parse URI → info_hash
  b. Check cache (via CacheManager)
  c. Get peer list: parallel Tracker announce + DHT find_peers
  d. Connect to top peers via transport
  e. Get metadata via ut_metadata (or fallback to empty_meta)
  f. Initialize RingBuffer, SeekEngine, PieceScheduler with metadata
  g. Start background download loop (spawn task)
  h. Return MediaStream with metadata + position tracking
- [ ] Implement background download loop: scheduler → priority → select_source → download → write to buffer
- [ ] Implement `seek(timestamp_ms)`: use SeekEngine to find nearest I-frame, reset play cursor, reschedule priorities
- [ ] Implement `stop(info_hash)`: remove active stream, disconnect peers
- [ ] Implement `status(info_hash) -> StreamStatus`: return current state from active stream
- [ ] Fix `SeekEngine::seek_to(timestamp_ms)` to actually work (currently missing)
- [ ] Fix `MetadataResolver::resolve_metadata` to attempt actual ut_metadata exchange
- [ ] Add integration test: play URI with mock peers, verify buffer gets data
- [ ] Run `cargo test -p qvs-stream && cargo clippy -p qvs-stream -- -D warnings && cargo fmt`

### Task 10: Fix RingBuffer and playback

**Files:**
- Modify: `crates/qvs-stream/src/buffer.rs`
- Modify: `crates/qvs-stream/src/playback.rs`
- Modify: `crates/qvs-stream/src/adaptive.rs`

- [ ] Fix `RingBuffer::write` return type: `Result<(), QvodError>`
- [ ] Fix `RingBuffer::read` return type: `Result<Vec<u8>, QvodError>`
- [ ] Implement `RingBuffer::adapt_watermarks(speed_bps)` — adjust high/low watermarks based on measured speed
- [ ] Make RingBuffer thread-safe: wrap internal state in `Arc<RwLock<>>`
- [ ] Implement `MediaStream::read(offset, length) -> Result<Vec<u8>>` for player consumption
- [ ] Implement EOS detection in MediaStream: track completed pieces vs total pieces
- [ ] Fix adaptive buffer to use time-windowed speed/RTT measurements (10s and 100s windows)
- [ ] Add tests for watermarks, threaded access, EOS detection
- [ ] Run `cargo test -p qvs-stream && cargo clippy -p qvs-stream -- -D warnings && cargo fmt`

---

### Phase 5: Complete Local HTTP Server

### Task 11: Make local-server actually stream media

**Files:**
- Modify: `crates/qvs-local-server/src/handler.rs`
- Modify: `crates/qvs-local-server/src/stream.rs`
- Modify: `crates/qvs-local-server/src/middleware.rs`
- Modify: `crates/qvs-local-server/src/server.rs`

- [ ] Implement `handle_play` to create a streaming response: call engine.play(), then stream buffer data via `ChunkedStream` (mpsc channel)
- [ ] Implement `handle_segment`: read offset/length from RingBuffer, wrap in TS if needed, return binary data
- [ ] Change `/control` route to POST (axum `post(handle_control)`)
- [ ] Implement `handle_control` for pause/resume/stop: lock engine, call appropriate method on active stream
- [ ] Add rate limiting middleware: per-IP counter with 100 req/s limit, return 429 when exceeded
- [ ] Fix server error propagation: new() should return Result, not silently log
- [ ] Add tests for streaming response, segment endpoint, rate limiting
- [ ] Run `cargo test -p qvs-local-server && cargo clippy -p qvs-local-server -- -D warnings && cargo fmt`

---

### Phase 6: Complete GUI

### Task 12: Render all GUI panels

**Files:**
- Modify: `crates/qvs-gui/src/app.rs`
- Modify: `crates/qvs-gui/src/player.rs`
- Modify: `crates/qvs-gui/src/controls.rs`
- Modify: `crates/qvs-gui/src/playlist.rs`
- Modify: `crates/qvs-gui/src/settings.rs`
- Modify: `crates/qvs-gui/src/status.rs`
- Modify: `crates/qvs-gui/src/overlay.rs`
- Modify: `crates/qvs-gui/src/theme.rs`
- Modify: `crates/qvs-gui/src/main.rs`

- [ ] Implement `app.rs::update()` to render:
  - Top bar with navigation (Player/Playlist/Settings/Status tabs)
  - CentralPanel with active page content
  - Keyboard shortcut handling (Space, arrows, Escape)
- [ ] Implement `player::render(ui)`: show video placeholder (colored rect), buffer progress bar, error overlay
- [ ] Implement `controls::render(ui)`: play/pause button, draggable progress bar, volume slider + mute, time display, seek buttons
- [ ] Implement `playlist::render(ui)`: scrollable list with play/remove buttons, right-click context menu, drag reorder
- [ ] Implement `settings::render(ui)`: tabbed settings with cache path/dir, port input, max connections slider, HTTP fallback toggle, tracker/DHT editors, language selector, theme selector
- [ ] Implement `status::render(ui)`: live speed up/down display, peer count, buffer %, download %, DHT size, connection list
- [ ] Implement `overlay::render(ui)` on top of player: buffering spinner, error red overlay with text, info overlay, fade animation
- [ ] Fix `theme.rs`: add BG/FG/SURFACE color constants, font size setting, widget style struct, system theme detection (check `ctx.system_theme()`)
- [ ] Fix `main.rs` CLI: implement all subcommands (status, list, cache) to actually work with engine
- [ ] Add snapshot/rendering tests where possible
- [ ] Run `cargo test -p qvs-gui && cargo clippy -p qvs-gui -- -D warnings && cargo fmt`

---

### Phase 7: Wire Binaries

### Task 13: Complete server and client binaries

**Files:**
- Modify: `server/src/main.rs`
- Modify: `client/src/main.rs`

- [ ] `qvs-server`: after starting HTTP server, also start DHT node (bootstrap + background refresh), initialize TrackerClient, pass all to QvodEngine
- [ ] `qvs-server`: add graceful shutdown that stops DHT, Tracker, Engine, then HTTP server
- [ ] `qvs-cli play`: after engine.play(), poll stream status periodically and print progress
- [ ] `qvs-cli status`: display engine status with active streams count
- [ ] `qvs-cli cache`: implement cache listing and cleanup using CacheManager
- [ ] Run full workspace verification

### Task 14: Final workspace verification

- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] Update AGENTS.md completion status

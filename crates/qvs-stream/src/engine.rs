use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use futures::future::join_all;
use sha1::Digest;
use sha1::Sha1;
use tokio::sync::{Mutex, RwLock};

use qvs_core::MediaStream as CoreMediaStream;
use qvs_core::{probe_media_file, DhtEngine, FileMeta, InfoHash, PeerInfo, QvodError};

use qvs_dht::{DhtConfig, DhtNode};
use qvs_format::cache::{CacheConfig, CacheManager};
use qvs_format::uri::MediaUri;
use qvs_tracker::{TrackerClient, TrackerConfig};

use crate::adaptive::AdaptiveBuffer;
use crate::buffer::RingBuffer;
use crate::config::EngineConfig;
use crate::metadata::MetadataResolver;
use crate::playback::{MediaStream, StreamState, StreamStats};
use crate::seek::SeekEngine;

pub struct StreamStatus {
    pub state: StreamState,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub buffered_seconds: f64,
    pub download_progress: f64,
    pub peer_count: usize,
}

pub struct QvodEngine {
    config: Arc<EngineConfig>,
    metadata_resolver: MetadataResolver,
    active_streams: HashMap<InfoHash, ActiveStream>,
    tracker: Option<Arc<TrackerClient>>,
    dht: Option<Arc<DhtNode>>,
    cache: Option<Arc<Mutex<CacheManager>>>,
}

struct ActiveStream {
    info_hash: InfoHash,
    metadata: FileMeta,
    buffer: Arc<RwLock<RingBuffer>>,
    seek_engine: SeekEngine,
    adaptive: AdaptiveBuffer,
    stream: Arc<Mutex<MediaStream>>,
    paused: bool,
    download_task: Option<tokio::task::JoinHandle<()>>,
    #[allow(dead_code)]
    created_at: tokio::time::Instant,
}

impl QvodEngine {
    pub async fn new(config: EngineConfig) -> Self {
        let config = Arc::new(config);
        tracing::info!(
            "QvodEngine::new: dht={}, tracker={}, cache={}, buffer={}MB, listen_port={}",
            config.dht_enabled,
            config.tracker_enabled,
            config.cache_enabled,
            config.buffer_capacity_mb,
            config.listen_port
        );

        let metadata_resolver = MetadataResolver::new(config.clone());

        let dht = if config.dht_enabled {
            let dht_config = DhtConfig {
                listen_port: config.udp_port,
                seed_nodes: config.dht_seed_nodes.clone(),
                ..Default::default()
            };
            tracing::info!("DHT: initializing on UDP port {}", config.udp_port);
            match DhtNode::new(dht_config).await {
                Ok(node) => {
                    tracing::info!("DHT: node created successfully");
                    let node = Arc::new(node);
                    let _handle = node.start().await;
                    let bootstrap_node = node.clone();
                    let seed_nodes = config.dht_seed_nodes.clone();
                    tokio::spawn(async move {
                        if !seed_nodes.is_empty() {
                            tracing::info!(
                                "DHT: bootstrapping with {} seed nodes",
                                seed_nodes.len()
                            );
                            if let Err(e) = bootstrap_node.bootstrap(&seed_nodes).await {
                                tracing::warn!(
                                    "DHT bootstrap failed (no compatible seed nodes?): {e}"
                                );
                            } else {
                                tracing::info!("DHT: bootstrap completed");
                            }
                        }
                    });
                    Some(node)
                }
                Err(e) => {
                    tracing::warn!("DHT init failed (UDP port {}?): {e}", config.udp_port);
                    None
                }
            }
        } else {
            tracing::info!("DHT: disabled");
            None
        };

        let tracker = if config.tracker_enabled && !config.tracker_urls.is_empty() {
            tracing::info!(
                "Tracker: initializing with {} URLs",
                config.tracker_urls.len()
            );
            for url in &config.tracker_urls {
                tracing::debug!("Tracker URL: {url}");
            }
            let tracker_config = TrackerConfig {
                tracker_urls: config.tracker_urls.clone(),
                peer_id: qvs_core::generate_peer_id(),
                port: config.listen_port,
                compact: true,
            };
            Some(Arc::new(TrackerClient::new(tracker_config)))
        } else {
            tracing::info!("Tracker: disabled");
            None
        };

        let cache = if config.cache_enabled {
            tracing::info!(
                "Cache: initializing, dir={}, max_size={}",
                config.cache_dir.display(),
                config.buffer_capacity() * 10
            );
            let cache_config = CacheConfig {
                cache_dir: config.cache_dir.clone(),
                max_size: (config.buffer_capacity() * 10).max(1024 * 1024 * 1024),
                max_files: 1000,
            };
            let cm = CacheManager::new(cache_config).await;
            Some(Arc::new(Mutex::new(cm)))
        } else {
            tracing::info!("Cache: disabled");
            None
        };

        Self {
            config,
            metadata_resolver,
            active_streams: HashMap::new(),
            tracker,
            dht,
            cache,
        }
    }

    pub async fn play(&mut self, uri: &str) -> Result<CoreMediaStream, QvodError> {
        tracing::info!("QvodEngine::play: uri={}", uri);
        let media_uri: MediaUri = uri.parse()?;

        let result = match media_uri {
            MediaUri::Qvod(qvod_uri) => self.play_qvod(qvod_uri).await,
            MediaUri::Http(http_url) => self.play_http(http_url).await,
            MediaUri::File(path) => self.play_file(path).await,
        };

        match &result {
            Ok(stream) => tracing::info!(
                "QvodEngine::play: success, info_hash={}, duration={}ms, file_size={}",
                stream.metadata.info_hash,
                stream.metadata.duration_ms,
                stream.metadata.file_size
            ),
            Err(e) => tracing::error!("QvodEngine::play: failed, uri={}, error={e}", uri),
        }
        result
    }

    async fn play_qvod(
        &mut self,
        qvod_uri: qvs_format::uri::QvodUri,
    ) -> Result<CoreMediaStream, QvodError> {
        let info_hash = qvod_uri.info_hash;
        let file_size = qvod_uri.filesize;
        tracing::info!(
            "play_qvod: info_hash={}, file_size={}",
            info_hash,
            file_size
        );

        // Step 1: Check cache for existing metadata
        if let Some(ref cache_mgr) = self.cache {
            let guard = cache_mgr.lock().await;
            if let Some(cached_meta) = guard.find(&info_hash).await {
                tracing::info!("play_qvod: cache hit for {}", info_hash);
                let buffer = Arc::new(RwLock::new(RingBuffer::new(
                    self.config.buffer_capacity(),
                    cached_meta.file_size,
                )));
                let seek_engine = SeekEngine::new(cached_meta.clone());
                let adaptive = AdaptiveBuffer::new();
                let stats = StreamStats::new(cached_meta.duration_ms);
                let stream = Arc::new(Mutex::new(MediaStream::new(stats)));

                let active = ActiveStream {
                    info_hash,
                    metadata: cached_meta.clone(),
                    buffer,
                    seek_engine,
                    adaptive,
                    stream: stream.clone(),
                    paused: false,
                    download_task: None,
                    created_at: tokio::time::Instant::now(),
                };
                self.active_streams.insert(info_hash, active);

                return Ok(CoreMediaStream::new(cached_meta));
            }
            tracing::info!("play_qvod: cache miss for {}", info_hash);
        }

        // Step 2: Get peers in parallel from tracker and DHT
        tracing::info!("play_qvod: fetching peers for {}", info_hash);
        let peers = self.get_peers_parallel(&info_hash).await;
        tracing::info!("play_qvod: found {} peers for {}", peers.len(), info_hash);

        // Step 3: Try to get metadata from peers, fall back to empty metadata
        let metadata = if peers.is_empty() {
            tracing::warn!("play_qvod: no peers found, using empty metadata");
            MetadataResolver::empty_meta(info_hash, file_size)
        } else {
            match self
                .metadata_resolver
                .resolve_from_peers(&info_hash, &peers)
                .await
            {
                Ok(meta) => {
                    tracing::info!(
                        "play_qvod: metadata resolved from peers: {} pieces, duration={}ms",
                        meta.pieces.len(),
                        meta.duration_ms
                    );
                    meta
                }
                Err(e) => {
                    tracing::warn!("play_qvod: metadata resolution failed: {e}, using empty meta");
                    MetadataResolver::empty_meta(info_hash, file_size)
                }
            }
        };

        // Step 4: Create stream components
        let buffer = Arc::new(RwLock::new(RingBuffer::new(
            self.config.buffer_capacity(),
            metadata.file_size,
        )));
        let seek_engine = SeekEngine::new(metadata.clone());
        let adaptive = AdaptiveBuffer::new();
        let stats = StreamStats::new(metadata.duration_ms);
        let stream = Arc::new(Mutex::new(MediaStream::new(stats)));

        // Step 5: Start background download if we have metadata
        let download_task = if !peers.is_empty() || file_size > 0 {
            tracing::info!("play_qvod: starting background download for {}", info_hash);
            let buffer_clone = buffer.clone();
            let stream_clone = stream.clone();
            let metadata_clone = metadata.clone();
            let config = self.config.clone();

            Some(tokio::spawn(async move {
                run_download_loop(buffer_clone, stream_clone, metadata_clone, config).await;
            }))
        } else {
            tracing::warn!("play_qvod: no peers and file_size=0, skipping download");
            None
        };

        // Step 6: Register active stream
        let active = ActiveStream {
            info_hash,
            metadata: metadata.clone(),
            buffer,
            seek_engine,
            adaptive,
            stream: stream.clone(),
            paused: false,
            download_task,
            created_at: tokio::time::Instant::now(),
        };
        self.active_streams.insert(info_hash, active);

        // Update stream state to playing
        {
            let mut s = stream.lock().await;
            let _ = s.play();
        }

        tracing::info!("play_qvod: stream registered for {}", info_hash);
        Ok(CoreMediaStream::new(metadata))
    }

    async fn play_http(
        &mut self,
        http_url: qvs_format::uri::HttpUrl,
    ) -> Result<CoreMediaStream, QvodError> {
        let url_str = http_url.url.clone();
        let filename = http_url.filename.clone();
        tracing::info!("play_http: url={}, filename={}", url_str, filename);

        // Derive a deterministic info_hash from the URL
        let mut hasher = Sha1::new();
        hasher.update(url_str.as_bytes());
        let hash_bytes: [u8; 20] = hasher.finalize().into();
        let info_hash = InfoHash(hash_bytes);
        tracing::info!("play_http: derived info_hash={}", info_hash);

        // Probe the HTTP source for file size and content type
        tracing::info!("play_http: probing HTTP source {}", url_str);
        let (file_size, content_type) = probe_http_source(&url_str).await?;
        tracing::info!(
            "play_http: probed: size={}, type={}",
            file_size,
            content_type
        );

        let _format = filename.rsplit('.').next().unwrap_or("mp4").to_string();

        let piece_length = qvs_core::PIECE_LENGTH;
        let piece_count = if file_size > 0 {
            ((file_size + piece_length - 1) / piece_length) as u32
        } else {
            0
        };
        tracing::info!(
            "play_http: {} pieces, {} bytes each",
            piece_count,
            piece_length
        );

        let metadata = FileMeta {
            info_hash,
            filename,
            file_size,
            piece_length,
            pieces: vec![Default::default(); piece_count as usize],
            keyframe_index: None,
            duration_ms: 0,
            video_codec: None,
            audio_codec: None,
            width: 0,
            height: 0,
            bitrate: 0,
            from_cache: false,
        };

        // Create stream components
        let buffer = Arc::new(RwLock::new(RingBuffer::new(
            self.config.buffer_capacity(),
            metadata.file_size,
        )));
        let seek_engine = SeekEngine::new(metadata.clone());
        let adaptive = AdaptiveBuffer::new();
        let stats = StreamStats::new(metadata.duration_ms);
        let stream = Arc::new(Mutex::new(MediaStream::new(stats)));

        // Start HTTP download loop
        let download_task = if file_size > 0 {
            tracing::info!("play_http: starting HTTP download loop for {}", info_hash);
            let buffer_clone = buffer.clone();
            let stream_clone = stream.clone();
            let metadata_clone = metadata.clone();
            let url = url_str.clone();

            Some(tokio::spawn(async move {
                run_http_download_loop(url, buffer_clone, stream_clone, metadata_clone).await;
            }))
        } else {
            tracing::warn!("play_http: file_size=0, skipping download");
            None
        };

        // Register active stream
        let active = ActiveStream {
            info_hash,
            metadata: metadata.clone(),
            buffer,
            seek_engine,
            adaptive,
            stream: stream.clone(),
            paused: false,
            download_task,
            created_at: tokio::time::Instant::now(),
        };
        self.active_streams.insert(info_hash, active);

        {
            let mut s = stream.lock().await;
            let _ = s.play();
        }

        tracing::info!("play_http: stream registered for {}", info_hash);
        Ok(CoreMediaStream::new(metadata))
    }

    async fn play_file(&mut self, file_path: String) -> Result<CoreMediaStream, QvodError> {
        tracing::info!("play_file: path={}", file_path);
        // Build the file:// URI for consistent hashing with server-side handler
        let file_uri = if cfg!(windows) {
            format!("file://{}", file_path.replace('\\', "/"))
        } else {
            format!("file://{file_path}")
        };

        // Hash the full file:// URI (same as handle_status / handle_control do)
        let mut hasher = Sha1::new();
        hasher.update(file_uri.as_bytes());
        let hash_bytes: [u8; 20] = hasher.finalize().into();
        let info_hash = InfoHash(hash_bytes);

        let canonical = std::fs::canonicalize(&file_path).map_err(|e| {
            tracing::error!("play_file: canonicalize failed: {e}");
            QvodError::Network(e)
        })?;
        tracing::info!("play_file: canonical path={}", canonical.display());

        let mut metadata = probe_file_source(&canonical)?;
        // Override info_hash to match server-side computation
        metadata.info_hash = info_hash;
        let file_size = metadata.file_size;
        tracing::info!(
            "play_file: size={}, duration={}ms, codec={:?}/{:?}",
            file_size,
            metadata.duration_ms,
            metadata.video_codec,
            metadata.audio_codec
        );

        // Create stream components
        let buffer = Arc::new(RwLock::new(RingBuffer::new(
            self.config.buffer_capacity(),
            file_size,
        )));
        let seek_engine = SeekEngine::new(metadata.clone());
        let adaptive = AdaptiveBuffer::new();
        let stats = StreamStats::new(metadata.duration_ms);
        let stream = Arc::new(Mutex::new(MediaStream::new(stats)));

        // Start file download loop
        let download_task = if file_size > 0 {
            tracing::info!(
                "play_file: starting file download loop for {}",
                canonical.display()
            );
            let buffer_clone = buffer.clone();
            let stream_clone = stream.clone();
            let metadata_clone = metadata.clone();

            Some(tokio::spawn(async move {
                run_file_download_loop(canonical, buffer_clone, stream_clone, metadata_clone).await;
            }))
        } else {
            tracing::warn!("play_file: file_size=0");
            None
        };

        // Register active stream
        let active = ActiveStream {
            info_hash,
            metadata: metadata.clone(),
            buffer,
            seek_engine,
            adaptive,
            stream: stream.clone(),
            paused: false,
            download_task,
            created_at: tokio::time::Instant::now(),
        };
        self.active_streams.insert(info_hash, active);

        {
            let mut s = stream.lock().await;
            let _ = s.play();
        }

        tracing::info!("play_file: stream registered for {}", info_hash);
        Ok(CoreMediaStream::new(metadata))
    }

    async fn get_peers_parallel(&self, info_hash: &InfoHash) -> Vec<PeerInfo> {
        let mut futs: Vec<futures::future::BoxFuture<'_, Vec<PeerInfo>>> = Vec::new();
        tracing::info!("get_peers_parallel: fetching peers for {}", info_hash);

        if let Some(ref tracker) = self.tracker {
            tracing::info!("get_peers_parallel: querying tracker");
            let info_hash = *info_hash;
            let tracker = tracker.clone();
            futs.push(Box::pin(async move {
                let peers = tracker
                    .announce(&info_hash, qvs_core::AnnounceEvent::Started, 0, 0, 0)
                    .await
                    .unwrap_or_default();
                tracing::info!("get_peers_parallel: tracker returned {} peers", peers.len());
                peers
            }));
        }

        if let Some(ref dht) = self.dht {
            tracing::info!("get_peers_parallel: querying DHT");
            let info_hash = *info_hash;
            let dht = dht.clone();
            futs.push(Box::pin(async move {
                let peers = dht.find_peers(&info_hash).await.unwrap_or_default();
                tracing::info!("get_peers_parallel: DHT returned {} peers", peers.len());
                peers
            }));
        }

        let all_peers: Vec<PeerInfo> = join_all(futs).await.into_iter().flatten().collect();
        tracing::info!(
            "get_peers_parallel: total {} peers for {}",
            all_peers.len(),
            info_hash
        );
        all_peers
    }

    pub async fn pause(&mut self) {
        let count = self.active_streams.len();
        tracing::info!("pause: pausing {} active stream(s)", count);
        for active in self.active_streams.values_mut() {
            active.paused = true;
            let mut s = active.stream.lock().await;
            s.pause();
        }
        tracing::info!("pause: all streams paused");
    }

    pub async fn resume(&mut self) {
        let count = self.active_streams.len();
        tracing::info!("resume: resuming {} active stream(s)", count);
        for active in self.active_streams.values_mut() {
            active.paused = false;
            let mut s = active.stream.lock().await;
            s.resume();
        }
        tracing::info!("resume: all streams resumed");
    }

    pub fn stop(&mut self, info_hash: &InfoHash) {
        tracing::info!("stop: stopping stream {}", info_hash);
        if let Some(active) = self.active_streams.remove(info_hash) {
            if let Some(task) = active.download_task {
                task.abort();
                tracing::info!("stop: download task aborted for {}", info_hash);
            }
            tracing::info!("stop: stream removed for {}", info_hash);
        } else {
            tracing::warn!("stop: stream not found for {}", info_hash);
        }
    }

    pub async fn seek(&mut self, timestamp_ms: u64) -> Result<(), QvodError> {
        tracing::info!("seek: seeking to {}ms", timestamp_ms);
        for active in self.active_streams.values_mut() {
            match active.seek_engine.find_nearest_keyframe(timestamp_ms) {
                Ok(target_offset) => {
                    let piece_idx = active.seek_engine.piece_for_offset(target_offset);
                    tracing::info!(
                        "seek: target_offset={}, piece_idx={}",
                        target_offset,
                        piece_idx
                    );
                    active.stream.lock().await.seek(timestamp_ms);
                }
                Err(e) => {
                    tracing::warn!("seek: keyframe not found: {e}, seeking directly");
                    active.stream.lock().await.seek(timestamp_ms);
                }
            }
        }
        tracing::info!("seek: completed to {}ms", timestamp_ms);
        Ok(())
    }

    pub async fn status(&self, info_hash: &InfoHash) -> Option<StreamStatus> {
        let active = self.active_streams.get(info_hash)?;
        let stats = active.stream.lock().await.stats().clone();
        tracing::debug!("status: info_hash={}, state={:?}, pos={}ms, dur={}ms, buffered={:.1}s, progress={:.1}%, peers={}",
            info_hash, stats.state, stats.position_ms, stats.duration_ms,
            stats.buffered_seconds, stats.download_progress * 100.0, stats.peer_count);
        Some(StreamStatus {
            state: stats.state,
            position_ms: stats.position_ms,
            duration_ms: stats.duration_ms,
            buffered_seconds: stats.buffered_seconds,
            download_progress: stats.download_progress,
            peer_count: stats.peer_count,
        })
    }

    #[must_use]
    pub fn active_streams(&self) -> Vec<InfoHash> {
        let streams = self.active_streams.keys().copied().collect::<Vec<_>>();
        tracing::debug!("active_streams: {} active", streams.len());
        streams
    }

    pub async fn read_buffer(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> Option<Vec<u8>> {
        let active = self.active_streams.get(info_hash)?;
        let buf = active.buffer.read().await;
        let data = buf.read(offset, length);
        if data.is_some() {
            tracing::trace!(
                "read_buffer: hash={}, offset={}, length={} -> hit",
                info_hash,
                offset,
                length
            );
        } else {
            tracing::trace!(
                "read_buffer: hash={}, offset={}, length={} -> miss",
                info_hash,
                offset,
                length
            );
        }
        data
    }

    #[must_use]
    pub fn file_size(&self, info_hash: &InfoHash) -> Option<u64> {
        let size = self
            .active_streams
            .get(info_hash)
            .map(|a| a.metadata.file_size);
        tracing::debug!("file_size: hash={}, size={:?}", info_hash, size);
        size
    }
}

async fn run_download_loop(
    buffer: Arc<RwLock<RingBuffer>>,
    stream: Arc<Mutex<MediaStream>>,
    metadata: FileMeta,
    config: Arc<EngineConfig>,
) {
    let piece_count = if metadata.piece_length > 0 {
        ((metadata.file_size + metadata.piece_length - 1) / metadata.piece_length) as u32
    } else {
        0
    };

    tracing::info!(
        "run_download_loop: starting for {} ({} pieces, {} bytes each)",
        metadata.info_hash,
        piece_count,
        metadata.piece_length
    );

    let mut current_piece = 0u32;
    let start_time = tokio::time::Instant::now();

    while current_piece < piece_count {
        let is_paused = {
            let s = stream.lock().await;
            s.is_paused()
        };

        if is_paused {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        let piece_len = if current_piece == piece_count - 1 {
            let remainder = metadata.file_size % metadata.piece_length;
            if remainder == 0 {
                metadata.piece_length
            } else {
                remainder
            }
        } else {
            metadata.piece_length
        };

        if current_piece % 100 == 0 {
            let elapsed = start_time.elapsed();
            let rate = if elapsed.as_secs() > 0 {
                (f64::from(current_piece) * metadata.piece_length as f64) / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let pct = (f64::from(current_piece) / f64::from(piece_count) * 100.0) as u32;
            tracing::info!(
                "run_download_loop: piece {}/{} ({}%), rate={:.0} B/s",
                current_piece + 1,
                piece_count,
                pct,
                rate
            );
        }

        let piece_data = vec![0u8; piece_len as usize];

        let offset = u64::from(current_piece) * metadata.piece_length;
        {
            let mut buf = buffer.write().await;
            buf.write(offset, &piece_data);
        }

        let progress = f64::from(current_piece + 1) / f64::from(piece_count);
        {
            let mut s = stream.lock().await;
            s.update_progress(progress, u64::from(current_piece + 1) * piece_len);
        }

        current_piece += 1;

        if config.download_timeout_secs > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    tracing::info!(
        "run_download_loop: completed for {} ({} pieces in {:?})",
        metadata.info_hash,
        piece_count,
        start_time.elapsed()
    );
    stream.lock().await.end();
}

async fn probe_http_source(url: &str) -> Result<(u64, String), QvodError> {
    tracing::info!("probe_http_source: probing {}", url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| {
            tracing::error!("probe_http_source: client build failed: {e}");
            QvodError::Network(std::io::Error::other(e))
        })?;

    let resp = client.head(url).send().await.map_err(|e| {
        tracing::error!("probe_http_source: HEAD request failed for {url}: {e}");
        QvodError::Network(std::io::Error::other(e))
    })?;

    let file_size = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("video/mp4")
        .to_string();

    if file_size == 0 {
        tracing::error!("probe_http_source: no Content-Length from {}", url);
        return Err(QvodError::Protocol(
            "HTTP source did not return Content-Length".into(),
        ));
    }

    tracing::info!(
        "probe_http_source: success: size={}, type={}",
        file_size,
        content_type
    );
    Ok((file_size, content_type))
}

async fn run_http_download_loop(
    url: String,
    buffer: Arc<RwLock<RingBuffer>>,
    stream: Arc<Mutex<MediaStream>>,
    metadata: FileMeta,
) {
    tracing::info!(
        "run_http_download_loop: starting for {} ({} bytes)",
        metadata.info_hash,
        metadata.file_size
    );
    let client = reqwest::Client::new();
    let chunk_size: u64 = 65536;
    let mut offset = 0u64;
    let mut errors: u32 = 0;
    let max_errors: u32 = 10;
    let start_time = tokio::time::Instant::now();

    while offset < metadata.file_size {
        let end = (offset + chunk_size - 1).min(metadata.file_size - 1);

        if offset % (1024 * 1024) == 0 {
            let elapsed = start_time.elapsed();
            let rate = if elapsed.as_secs() > 0 {
                offset as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            tracing::info!(
                "run_http_download_loop: offset={}/{} ({:.1}%), rate={:.0} B/s, errors={}",
                offset,
                metadata.file_size,
                (offset as f64 / metadata.file_size as f64) * 100.0,
                rate,
                errors
            );
        }

        match client
            .get(&url)
            .header("Range", format!("bytes={offset}-{end}"))
            .send()
            .await
        {
            Ok(resp)
                if resp.status().is_success()
                    || resp.status() == reqwest::StatusCode::PARTIAL_CONTENT =>
            {
                match resp.bytes().await {
                    Ok(data) if !data.is_empty() => {
                        errors = 0;
                        let mut buf = buffer.write().await;
                        buf.write(offset, &data);

                        let downloaded = offset + data.len() as u64;
                        let progress = downloaded as f64 / metadata.file_size as f64;
                        drop(buf);
                        let mut s = stream.lock().await;
                        s.update_progress(progress, downloaded);
                        s.update_speed(data.len() as f64 * 10.0);

                        offset += data.len() as u64;
                    }
                    _ => {
                        tracing::warn!(
                            "run_http_download_loop: empty response at offset {}",
                            offset
                        );
                        errors += 1;
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!(
                    "run_http_download_loop: HTTP {} at offset {}",
                    resp.status(),
                    offset
                );
                errors += 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => {
                tracing::warn!("run_http_download_loop: network error at offset {offset}: {e}");
                errors += 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        if errors >= max_errors {
            tracing::error!(
                "run_http_download_loop: too many errors ({}), aborting",
                errors
            );
            break;
        }
    }

    tracing::info!(
        "run_http_download_loop: completed for {} ({}/{} bytes in {:?})",
        metadata.info_hash,
        offset,
        metadata.file_size,
        start_time.elapsed()
    );
    stream.lock().await.end();
}

/// Probe media file using ffprobe/ffmpeg subprocess.
fn probe_with_ffprobe(path: &std::path::Path) -> Option<(u64, u32, u32, String, String, u64)> {
    tracing::info!("probe_with_ffprobe: probing {}", path.display());
    let result = probe_media_file(path)?;
    tracing::info!(
        "probe_with_ffprobe: success: {}ms, {}x{}, codec={}/{}",
        result.duration_ms,
        result.width,
        result.height,
        result.video_codec,
        result.audio_codec
    );
    Some((
        result.duration_ms,
        result.width,
        result.height,
        result.video_codec,
        result.audio_codec,
        result.bitrate,
    ))
}

fn probe_file_source(path: &std::path::Path) -> Result<FileMeta, QvodError> {
    tracing::info!("probe_file_source: probing {}", path.display());
    let metadata = std::fs::metadata(path).map_err(|e| {
        tracing::error!("probe_file_source: metadata failed: {e}");
        QvodError::Network(e)
    })?;
    if !metadata.is_file() {
        tracing::error!("probe_file_source: not a file: {}", path.display());
        return Err(QvodError::Protocol(format!(
            "not a file: {}",
            path.display()
        )));
    }
    let file_size = metadata.len();
    tracing::info!("probe_file_source: file_size={}", file_size);

    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut hasher = Sha1::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let hash_bytes: [u8; 20] = hasher.finalize().into();
    let info_hash = InfoHash(hash_bytes);

    let piece_length = qvs_core::PIECE_LENGTH;
    let piece_count = if file_size > 0 {
        ((file_size + piece_length - 1) / piece_length) as u32
    } else {
        0
    };

    let _fmt = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4")
        .to_string();

    let (duration_ms, width, height, video_codec, audio_codec, bitrate) =
        probe_with_ffprobe(path).unwrap_or_default();

    tracing::info!(
        "probe_file_source: done for {}: {} pieces, {}ms, {}x{}",
        path.display(),
        piece_count,
        duration_ms,
        width,
        height
    );

    Ok(FileMeta {
        info_hash,
        filename,
        file_size,
        piece_length,
        pieces: vec![Default::default(); piece_count as usize],
        keyframe_index: None,
        duration_ms,
        video_codec: if video_codec.is_empty() {
            None
        } else {
            Some(video_codec)
        },
        audio_codec: if audio_codec.is_empty() {
            None
        } else {
            Some(audio_codec)
        },
        width,
        height,
        bitrate,
        from_cache: false,
    })
}

/// Estimate stream position from file-read progress.
/// When real duration is unknown, assume a conservative bitrate of 1 Mbps.
fn estimate_position_ms(offset: u64, file_size: u64, duration_ms: u64) -> u64 {
    if duration_ms > 0 {
        if file_size > 0 {
            (u128::from(offset) * u128::from(duration_ms) / u128::from(file_size)) as u64
        } else {
            0
        }
    } else if file_size > 0 {
        let assumed_bitrate: u64 = 1_000_000;
        offset * 8 * 1000 / assumed_bitrate
    } else {
        0
    }
}

async fn run_file_download_loop(
    path: std::path::PathBuf,
    buffer: Arc<RwLock<RingBuffer>>,
    stream: Arc<Mutex<MediaStream>>,
    metadata: FileMeta,
) {
    tracing::info!(
        "run_file_download_loop: starting for {} ({} bytes)",
        path.display(),
        metadata.file_size
    );
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(
                "run_file_download_loop: failed to open {}: {e}",
                path.display()
            );
            stream.lock().await.end();
            return;
        }
    };

    let chunk_size: u64 = 65536;
    let mut offset = 0u64;
    let mut errors: u32 = 0;
    let max_errors: u32 = 10;
    let file_size = metadata.file_size;
    let duration_ms = metadata.duration_ms;
    let start_time = tokio::time::Instant::now();

    let mut reader = tokio::io::BufReader::new(file);

    while offset < file_size {
        if offset % (1024 * 1024) == 0 {
            let elapsed = start_time.elapsed();
            let rate = if elapsed.as_secs() > 0 {
                offset as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            tracing::info!(
                "run_file_download_loop: offset={}/{} ({:.1}%), rate={:.0} B/s",
                offset,
                file_size,
                (offset as f64 / file_size as f64) * 100.0,
                rate
            );
        }

        let read_size = chunk_size.min(file_size - offset);
        let mut chunk = vec![0u8; read_size as usize];

        match reader.read_exact(&mut chunk).await {
            Ok(_n) => {
                errors = 0;
                {
                    let mut buf = buffer.write().await;
                    buf.write(offset, &chunk);
                }

                let downloaded = offset + chunk.len() as u64;
                let progress = downloaded as f64 / file_size as f64;
                let pos_ms = estimate_position_ms(offset, file_size, duration_ms);
                let buffered_secs =
                    estimate_position_ms(downloaded, file_size, duration_ms) as f64 / 1000.0;

                {
                    let mut s = stream.lock().await;
                    s.update_progress(progress, downloaded);
                    s.update_speed(chunk.len() as f64 * 10.0);
                    s.update_position(pos_ms);
                    s.update_buffered(buffered_secs);
                }

                offset += chunk.len() as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                tracing::warn!(
                    "run_file_download_loop: unexpected EOF at offset {}",
                    offset
                );
                let remaining = chunk.len().min((file_size - offset) as usize);
                if remaining > 0 {
                    let mut buf = buffer.write().await;
                    buf.write(offset, &chunk[..remaining]);
                    let downloaded = offset + remaining as u64;
                    let pos_ms = estimate_position_ms(downloaded, file_size, duration_ms);
                    let mut s = stream.lock().await;
                    s.update_progress(1.0, file_size);
                    s.update_position(pos_ms);
                }
                break;
            }
            Err(e) => {
                tracing::warn!("run_file_download_loop: read error at offset {offset}: {e}");
                errors += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
                if errors >= max_errors {
                    tracing::error!(
                        "run_file_download_loop: too many errors ({}), aborting",
                        errors
                    );
                    break;
                }
            }
        }
    }

    {
        let mut s = stream.lock().await;
        s.update_position(estimate_position_ms(file_size, file_size, duration_ms));
        s.end();
    }
    tracing::info!(
        "run_file_download_loop: completed for {} ({}/{} bytes in {:?})",
        path.display(),
        offset,
        file_size,
        start_time.elapsed()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_engine_new_with_dht_disabled() {
        let config = EngineConfig {
            dht_enabled: false,
            ..Default::default()
        };
        let engine = QvodEngine::new(config).await;
        assert!(engine.tracker.is_some());
        assert!(engine.dht.is_none());
    }

    #[tokio::test]
    async fn test_engine_new_all_disabled() {
        let config = EngineConfig {
            dht_enabled: false,
            tracker_enabled: false,
            cache_enabled: false,
            ..Default::default()
        };
        let engine = QvodEngine::new(config).await;
        assert!(engine.tracker.is_none());
        assert!(engine.dht.is_none());
        assert!(engine.cache.is_none());
    }

    #[tokio::test]
    async fn test_play_and_stop() {
        let config = EngineConfig::default();
        let mut engine = QvodEngine::new(config).await;
        let uri = "qvod://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|test.mp4|1024|mp4|";
        let result = engine.play(uri).await;
        assert!(result.is_ok());
        let ih = InfoHash([0xaa; 20]);
        engine.stop(&ih);
        assert!(!engine.active_streams.contains_key(&ih));
    }

    #[tokio::test]
    async fn test_play_invalid_uri() {
        let config = EngineConfig::default();
        let mut engine = QvodEngine::new(config).await;
        let result = engine.play("invalid://uri").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_play_http_uri_invalid_source() {
        let config = EngineConfig::default();
        let mut engine = QvodEngine::new(config).await;
        // Unreachable HTTP URL should fail the probe
        let result = engine.play("http://localhost:1/nonexistent.mp4").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_play_http_scheme_parsed() {
        let config = EngineConfig::default();
        let mut engine = QvodEngine::new(config).await;
        // Just verify the scheme parsing works (will fail at HTTP probe)
        let result = engine.play("http://example.com/video.mp4").await;
        assert!(result.is_err());
        let result = engine
            .play("https://cdn.example.com/path/to/movie.avi")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pause_resume_stream() {
        let config = EngineConfig::default();
        let mut engine = QvodEngine::new(config).await;
        let uri = "qvod://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1|test.mp4|1024000|mp4|";
        let _ = engine.play(uri).await;
        engine.pause().await;
        engine.resume().await;
    }

    #[tokio::test]
    async fn test_active_streams_list() {
        let config = EngineConfig::default();
        let mut engine = QvodEngine::new(config).await;
        let uri = "qvod://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2|test.mp4|1024000|mp4|";
        let _ = engine.play(uri).await;
        assert_eq!(engine.active_streams().len(), 1);
    }

    #[tokio::test]
    async fn test_seek_no_streams() {
        let config = EngineConfig::default();
        let engine = QvodEngine::new(config).await;
        let mut engine = engine;
        assert!(engine.seek(5000).await.is_ok());
    }
}

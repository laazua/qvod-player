use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use futures::future::join_all;
use sha1::Digest;
use sha1::Sha1;
use tokio::sync::{Mutex, RwLock};

use qvs_core::MediaStream as CoreMediaStream;
use qvs_core::{DhtEngine, FileMeta, InfoHash, PeerInfo, QvodError};

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
        let metadata_resolver = MetadataResolver::new(config.clone());

        let dht = if config.dht_enabled {
            let dht_config = DhtConfig {
                listen_port: config.udp_port,
                seed_nodes: config.dht_seed_nodes.clone(),
                ..Default::default()
            };
            match DhtNode::new(dht_config).await {
                Ok(node) => {
                    let node = Arc::new(node);
                    let _handle = node.start().await;
                    let bootstrap_node = node.clone();
                    let seed_nodes = config.dht_seed_nodes.clone();
                    tokio::spawn(async move {
                        if !seed_nodes.is_empty() {
                            if let Err(e) = bootstrap_node.bootstrap(&seed_nodes).await {
                                tracing::warn!(
                                    "DHT bootstrap failed (no compatible seed nodes?): {e}"
                                );
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
            None
        };

        let tracker = if config.tracker_enabled && !config.tracker_urls.is_empty() {
            let tracker_config = TrackerConfig {
                tracker_urls: config.tracker_urls.clone(),
                peer_id: qvs_core::generate_peer_id(),
                port: config.listen_port,
                compact: true,
            };
            Some(Arc::new(TrackerClient::new(tracker_config)))
        } else {
            None
        };

        let cache = if config.cache_enabled {
            let cache_config = CacheConfig {
                cache_dir: config.cache_dir.clone(),
                max_size: (config.buffer_capacity() * 10).max(1024 * 1024 * 1024),
                max_files: 1000,
            };
            let cm = CacheManager::new(cache_config).await;
            Some(Arc::new(Mutex::new(cm)))
        } else {
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
        let media_uri: MediaUri = uri.parse()?;

        match media_uri {
            MediaUri::Qvod(qvod_uri) => self.play_qvod(qvod_uri).await,
            MediaUri::Http(http_url) => self.play_http(http_url).await,
            MediaUri::File(path) => self.play_file(path).await,
        }
    }

    async fn play_qvod(
        &mut self,
        qvod_uri: qvs_format::uri::QvodUri,
    ) -> Result<CoreMediaStream, QvodError> {
        let info_hash = qvod_uri.info_hash;
        let file_size = qvod_uri.filesize;

        // Step 1: Check cache for existing metadata
        if let Some(ref cache_mgr) = self.cache {
            let guard = cache_mgr.lock().await;
            if let Some(cached_meta) = guard.find(&info_hash).await {
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
        }

        // Step 2: Get peers in parallel from tracker and DHT
        let peers = self.get_peers_parallel(&info_hash).await;

        // Step 3: Try to get metadata from peers, fall back to empty metadata
        let metadata = if peers.is_empty() {
            MetadataResolver::empty_meta(info_hash, file_size)
        } else {
            self.metadata_resolver
                .resolve_from_peers(&info_hash, &peers)
                .await
                .unwrap_or_else(|_| MetadataResolver::empty_meta(info_hash, file_size))
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
            let buffer_clone = buffer.clone();
            let stream_clone = stream.clone();
            let metadata_clone = metadata.clone();
            let config = self.config.clone();

            Some(tokio::spawn(async move {
                run_download_loop(buffer_clone, stream_clone, metadata_clone, config).await;
            }))
        } else {
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

        Ok(CoreMediaStream::new(metadata))
    }

    async fn play_http(
        &mut self,
        http_url: qvs_format::uri::HttpUrl,
    ) -> Result<CoreMediaStream, QvodError> {
        let url_str = http_url.url.clone();
        let filename = http_url.filename.clone();

        // Derive a deterministic info_hash from the URL
        let mut hasher = Sha1::new();
        hasher.update(url_str.as_bytes());
        let hash_bytes: [u8; 20] = hasher.finalize().into();
        let info_hash = InfoHash(hash_bytes);

        // Probe the HTTP source for file size and content type
        let (file_size, _content_type) = probe_http_source(&url_str).await?;

        let _format = filename.rsplit('.').next().unwrap_or("mp4").to_string();

        let piece_length = qvs_core::PIECE_LENGTH;
        let piece_count = if file_size > 0 {
            ((file_size + piece_length - 1) / piece_length) as u32
        } else {
            0
        };

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
            let buffer_clone = buffer.clone();
            let stream_clone = stream.clone();
            let metadata_clone = metadata.clone();
            let url = url_str.clone();

            Some(tokio::spawn(async move {
                run_http_download_loop(url, buffer_clone, stream_clone, metadata_clone).await;
            }))
        } else {
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

        Ok(CoreMediaStream::new(metadata))
    }

    async fn play_file(&mut self, file_path: String) -> Result<CoreMediaStream, QvodError> {
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

        let canonical = std::fs::canonicalize(&file_path).map_err(QvodError::Network)?;

        let mut metadata = probe_file_source(&canonical)?;
        // Override info_hash to match server-side computation
        metadata.info_hash = info_hash;
        let file_size = metadata.file_size;

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
            let buffer_clone = buffer.clone();
            let stream_clone = stream.clone();
            let metadata_clone = metadata.clone();

            Some(tokio::spawn(async move {
                run_file_download_loop(canonical, buffer_clone, stream_clone, metadata_clone).await;
            }))
        } else {
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

        Ok(CoreMediaStream::new(metadata))
    }

    async fn get_peers_parallel(&self, info_hash: &InfoHash) -> Vec<PeerInfo> {
        let mut futs: Vec<futures::future::BoxFuture<'_, Vec<PeerInfo>>> = Vec::new();

        if let Some(ref tracker) = self.tracker {
            let info_hash = *info_hash;
            let tracker = tracker.clone();
            futs.push(Box::pin(async move {
                tracker
                    .announce(&info_hash, qvs_core::AnnounceEvent::Started, 0, 0, 0)
                    .await
                    .unwrap_or_default()
            }));
        }

        if let Some(ref dht) = self.dht {
            let info_hash = *info_hash;
            let dht = dht.clone();
            futs.push(Box::pin(async move {
                dht.find_peers(&info_hash).await.unwrap_or_default()
            }));
        }

        join_all(futs).await.into_iter().flatten().collect()
    }

    pub async fn pause(&mut self) {
        for active in self.active_streams.values_mut() {
            active.paused = true;
            let mut s = active.stream.lock().await;
            s.pause();
        }
    }

    pub async fn resume(&mut self) {
        for active in self.active_streams.values_mut() {
            active.paused = false;
            let mut s = active.stream.lock().await;
            s.resume();
        }
    }

    pub fn stop(&mut self, info_hash: &InfoHash) {
        if let Some(active) = self.active_streams.remove(info_hash) {
            if let Some(task) = active.download_task {
                task.abort();
            }
        }
    }

    pub async fn seek(&mut self, timestamp_ms: u64) -> Result<(), QvodError> {
        for active in self.active_streams.values_mut() {
            let target_offset = active.seek_engine.find_nearest_keyframe(timestamp_ms)?;
            let _piece_idx = active.seek_engine.piece_for_offset(target_offset);
            active.stream.lock().await.seek(timestamp_ms);
        }
        Ok(())
    }

    pub async fn status(&self, info_hash: &InfoHash) -> Option<StreamStatus> {
        let active = self.active_streams.get(info_hash)?;
        let stats = active.stream.lock().await.stats().clone();
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
        self.active_streams.keys().copied().collect()
    }

    pub async fn read_buffer(
        &self,
        info_hash: &InfoHash,
        offset: u64,
        length: u64,
    ) -> Option<Vec<u8>> {
        let active = self.active_streams.get(info_hash)?;
        let buf = active.buffer.read().await;
        buf.read(offset, length)
    }

    #[must_use]
    pub fn file_size(&self, info_hash: &InfoHash) -> Option<u64> {
        let active = self.active_streams.get(info_hash)?;
        Some(active.metadata.file_size)
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

    let mut current_piece = 0u32;

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

    stream.lock().await.end();
}

async fn probe_http_source(url: &str) -> Result<(u64, String), QvodError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| QvodError::Network(std::io::Error::other(e)))?;

    let resp = client
        .head(url)
        .send()
        .await
        .map_err(|e| QvodError::Network(std::io::Error::other(e)))?;

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
        return Err(QvodError::Protocol(
            "HTTP source did not return Content-Length".into(),
        ));
    }

    Ok((file_size, content_type))
}

async fn run_http_download_loop(
    url: String,
    buffer: Arc<RwLock<RingBuffer>>,
    stream: Arc<Mutex<MediaStream>>,
    metadata: FileMeta,
) {
    let client = reqwest::Client::new();
    let chunk_size: u64 = 65536;
    let mut offset = 0u64;
    let mut errors: u32 = 0;
    let max_errors: u32 = 10;

    while offset < metadata.file_size {
        let end = (offset + chunk_size - 1).min(metadata.file_size - 1);

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
                        errors += 1;
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!("HTTP {} at offset {}", resp.status(), offset);
                errors += 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => {
                tracing::warn!("HTTP download error at offset {offset}: {e}");
                errors += 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        if errors >= max_errors {
            tracing::error!("Too many HTTP download errors, aborting");
            break;
        }
    }

    stream.lock().await.end();
}

/// Probe media file using ffprobe subprocess.
/// Returns (duration_ms, width, height, video_codec, audio_codec, bitrate).
fn probe_with_ffprobe(path: &std::path::Path) -> Option<(u64, u32, u32, String, String, u64)> {
    use std::process::Command;

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path.as_os_str())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;

    let duration_ms = json["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0) as u64)
        .unwrap_or(0);

    let bitrate = json["format"]["bit_rate"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut width = 0u32;
    let mut height = 0u32;
    let mut video_codec = String::new();
    let mut audio_codec = String::new();

    if let Some(streams) = json["streams"].as_array() {
        for stream in streams {
            let codec_type = stream["codec_type"].as_str().unwrap_or("");
            let codec_name = stream["codec_name"].as_str().unwrap_or("").to_string();
            match codec_type {
                "video" => {
                    width = stream["width"].as_u64().unwrap_or(0) as u32;
                    height = stream["height"].as_u64().unwrap_or(0) as u32;
                    video_codec = codec_name;
                }
                "audio" => {
                    if audio_codec.is_empty() {
                        audio_codec = codec_name;
                    }
                }
                _ => {}
            }
        }
    }

    Some((
        duration_ms,
        width,
        height,
        video_codec,
        audio_codec,
        bitrate,
    ))
}

fn probe_file_source(path: &std::path::Path) -> Result<FileMeta, QvodError> {
    let metadata = std::fs::metadata(path).map_err(QvodError::Network)?;
    if !metadata.is_file() {
        return Err(QvodError::Protocol(format!(
            "not a file: {}",
            path.display()
        )));
    }
    let file_size = metadata.len();

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
            (offset as u128 * duration_ms as u128 / file_size as u128) as u64
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
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Failed to open local file {}: {e}", path.display());
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

    let mut reader = tokio::io::BufReader::new(file);

    while offset < file_size {
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
                tracing::warn!("File read error at offset {offset}: {e}");
                errors += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
                if errors >= max_errors {
                    tracing::error!("Too many file read errors, aborting");
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

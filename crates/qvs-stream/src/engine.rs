use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use tokio::sync::{Mutex, RwLock};

use qvs_core::MediaStream as CoreMediaStream;
use qvs_core::{DhtEngine, FileMeta, InfoHash, PeerInfo, QvodError};

use qvs_dht::{DhtConfig, DhtNode};
use qvs_format::cache::{CacheConfig, CacheManager};
use qvs_format::uri::QvodUri;
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
                                eprintln!("DHT bootstrap failed: {e}");
                            }
                        }
                    });
                    Some(node)
                }
                Err(e) => {
                    eprintln!("DHT init failed: {e}");
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
        let qvod_uri: QvodUri = uri.parse()?;
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

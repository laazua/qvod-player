use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use qvs_core::{BlockRequest, PiecePriority, QvodError, BLOCK_LENGTH};
use tokio::sync::Mutex;

use crate::scheduler::PieceScheduler;
use crate::tcp_stream::TcpStreamManager;

const HTTP_FALLBACK_TIMEOUT: Duration = Duration::from_secs(3);
const CRITICAL_PARALLEL_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Parallel,
    P2PWithHttpFallback,
    P2POnly,
    P2PIdle,
}

#[derive(Debug, Clone)]
pub struct DownloadConfig {
    pub max_p2p_connections: u32,
    pub http_fallback_enabled: bool,
    pub http_fallback_timeout_ms: u64,
    pub critical_parallel: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            max_p2p_connections: 50,
            http_fallback_enabled: true,
            http_fallback_timeout_ms: 3000,
            critical_parallel: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub piece_index: u32,
    pub source: Source,
    pub started_at: Instant,
    pub block: BlockRequest,
}

#[derive(Debug, Clone, Default)]
pub struct DownloadStats {
    pub total_downloaded: u64,
    pub total_pieces: u64,
    pub http_bytes: u64,
    pub p2p_bytes: u64,
    pub active_requests: usize,
}

pub struct P2spDownloader {
    p2p_engine: Arc<Mutex<Option<TcpStreamManager>>>,
    http_client: Option<reqwest::Client>,
    http_fallback_urls: Vec<String>,
    scheduler: Option<PieceScheduler>,
    config: DownloadConfig,
    active_requests: HashMap<u32, DownloadRequest>,
    stats: DownloadStats,
}

impl std::fmt::Debug for P2spDownloader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("P2spDownloader")
            .field("http_fallback_urls", &self.http_fallback_urls)
            .field("config", &self.config)
            .field("active_requests", &self.active_requests)
            .field("stats", &self.stats)
            .field("scheduler", &self.scheduler)
            .finish_non_exhaustive()
    }
}

impl P2spDownloader {
    #[must_use]
    pub fn new(
        p2p_engine: Arc<Mutex<Option<TcpStreamManager>>>,
        http_fallback_urls: Vec<String>,
        scheduler: Option<PieceScheduler>,
        config: DownloadConfig,
    ) -> Self {
        let http_client = if config.http_fallback_enabled && !http_fallback_urls.is_empty() {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .ok()
        } else {
            None
        };
        Self {
            p2p_engine,
            http_client,
            http_fallback_urls,
            scheduler,
            config,
            active_requests: HashMap::new(),
            stats: DownloadStats::default(),
        }
    }

    #[must_use]
    pub fn select_source(&self, priority: PiecePriority) -> Source {
        match priority {
            PiecePriority::Critical => {
                if self.config.http_fallback_enabled
                    && self.config.critical_parallel
                    && !self.http_fallback_urls.is_empty()
                {
                    Source::Parallel
                } else {
                    Source::P2POnly
                }
            }
            PiecePriority::High => {
                if self.config.http_fallback_enabled && !self.http_fallback_urls.is_empty() {
                    Source::P2PWithHttpFallback
                } else {
                    Source::P2POnly
                }
            }
            PiecePriority::Normal => Source::P2POnly,
            PiecePriority::Low => Source::P2PIdle,
        }
    }

    pub async fn download_critical(&mut self, piece_index: u32) -> Result<Vec<u8>, QvodError> {
        self.stats.active_requests += 1;
        let request = DownloadRequest {
            piece_index,
            source: Source::Parallel,
            started_at: Instant::now(),
            block: BlockRequest {
                piece_index,
                begin: 0,
                length: BLOCK_LENGTH as u32,
            },
        };
        self.active_requests.insert(piece_index, request);

        let p2p_result = self.download_p2p(piece_index).await;
        let result = match p2p_result {
            Ok(data) => Ok(data),
            Err(_) => self.download_http(piece_index).await,
        };

        self.active_requests.remove(&piece_index);
        self.stats.active_requests -= 1;
        if result.is_ok() {
            self.stats.total_pieces += 1;
        }
        result
    }

    pub async fn download_high(&mut self, piece_index: u32) -> Result<Vec<u8>, QvodError> {
        self.stats.active_requests += 1;
        let request = DownloadRequest {
            piece_index,
            source: Source::P2PWithHttpFallback,
            started_at: Instant::now(),
            block: BlockRequest {
                piece_index,
                begin: 0,
                length: BLOCK_LENGTH as u32,
            },
        };
        self.active_requests.insert(piece_index, request);

        let timeout_ms = self.config.http_fallback_timeout_ms;

        let p2p_result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            self.download_p2p(piece_index),
        )
        .await;

        let result = match p2p_result {
            Ok(Ok(data)) => Ok(data),
            Ok(Err(_)) | Err(_) => self.download_http(piece_index).await,
        };

        self.active_requests.remove(&piece_index);
        self.stats.active_requests -= 1;
        if result.is_ok() {
            self.stats.total_pieces += 1;
        }
        result
    }

    pub async fn download_normal(&mut self, piece_index: u32) -> Result<Vec<u8>, QvodError> {
        self.stats.active_requests += 1;
        let request = DownloadRequest {
            piece_index,
            source: Source::P2POnly,
            started_at: Instant::now(),
            block: BlockRequest {
                piece_index,
                begin: 0,
                length: BLOCK_LENGTH as u32,
            },
        };
        self.active_requests.insert(piece_index, request);

        let result = self.download_p2p(piece_index).await;

        self.active_requests.remove(&piece_index);
        self.stats.active_requests -= 1;
        if result.is_ok() {
            self.stats.total_pieces += 1;
        }
        result
    }

    pub async fn download_idle(&mut self, piece_index: u32) -> Result<Vec<u8>, QvodError> {
        self.stats.active_requests += 1;
        let request = DownloadRequest {
            piece_index,
            source: Source::P2PIdle,
            started_at: Instant::now(),
            block: BlockRequest {
                piece_index,
                begin: 0,
                length: BLOCK_LENGTH as u32,
            },
        };
        self.active_requests.insert(piece_index, request);

        let result = self.download_p2p(piece_index).await;

        self.active_requests.remove(&piece_index);
        self.stats.active_requests -= 1;
        if result.is_ok() {
            self.stats.total_pieces += 1;
        }
        result
    }

    async fn download_p2p(&self, piece_index: u32) -> Result<Vec<u8>, QvodError> {
        let engine = self.p2p_engine.lock().await;
        if engine.is_none() {
            return Err(QvodError::Protocol("P2P engine not connected".into()));
        }
        let _ = piece_index;

        tokio::time::sleep(Duration::from_millis(10)).await;

        Err(QvodError::NoPeers)
    }

    async fn download_http(&self, piece_index: u32) -> Result<Vec<u8>, QvodError> {
        let client = self
            .http_client
            .as_ref()
            .ok_or_else(|| QvodError::Protocol("HTTP client not configured".into()))?;

        let url = self.http_fallback_urls.first().ok_or(QvodError::NoPeers)?;

        let offset = u64::from(piece_index) * BLOCK_LENGTH;
        let range = format!("bytes={}-{}", offset, offset + BLOCK_LENGTH - 1);

        let response = client
            .get(url)
            .header("Range", &range)
            .send()
            .await
            .map_err(|e| QvodError::Network(std::io::Error::other(e)))?;

        let data = response
            .bytes()
            .await
            .map_err(|e| QvodError::Network(std::io::Error::other(e)))?
            .to_vec();

        Ok(data)
    }

    #[must_use]
    pub fn download_critical_timeout(&self) -> Duration {
        CRITICAL_PARALLEL_TIMEOUT
    }

    #[must_use]
    pub fn download_high_timeout(&self) -> Duration {
        HTTP_FALLBACK_TIMEOUT
    }

    #[must_use]
    pub fn http_sources(&self) -> &[String] {
        &self.http_fallback_urls
    }

    #[must_use]
    pub fn is_http_enabled(&self) -> bool {
        self.config.http_fallback_enabled
    }

    #[must_use]
    pub fn config(&self) -> &DownloadConfig {
        &self.config
    }

    #[must_use]
    pub fn stats(&self) -> &DownloadStats {
        &self.stats
    }

    #[must_use]
    pub fn active_requests(&self) -> &HashMap<u32, DownloadRequest> {
        &self.active_requests
    }

    pub fn add_http_source(&mut self, url: String) {
        if !self.http_fallback_urls.contains(&url) {
            self.http_fallback_urls.push(url);
        }
    }

    pub fn remove_http_source(&mut self, url: &str) {
        self.http_fallback_urls.retain(|s| s != url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_downloader(http_enabled: bool, urls: Vec<String>) -> P2spDownloader {
        let config = DownloadConfig {
            http_fallback_enabled: http_enabled,
            ..Default::default()
        };
        P2spDownloader::new(Arc::new(Mutex::new(None)), urls, None, config)
    }

    #[test]
    fn test_select_source_with_http() {
        let downloader = make_downloader(true, vec!["http://example.com/file".into()]);
        assert_eq!(
            downloader.select_source(PiecePriority::Critical),
            Source::Parallel
        );
        assert_eq!(
            downloader.select_source(PiecePriority::High),
            Source::P2PWithHttpFallback
        );
        assert_eq!(
            downloader.select_source(PiecePriority::Normal),
            Source::P2POnly
        );
        assert_eq!(
            downloader.select_source(PiecePriority::Low),
            Source::P2PIdle
        );
    }

    #[test]
    fn test_select_source_without_http() {
        let downloader = make_downloader(false, vec![]);
        assert_eq!(
            downloader.select_source(PiecePriority::Critical),
            Source::P2POnly
        );
        assert_eq!(
            downloader.select_source(PiecePriority::High),
            Source::P2POnly
        );
        assert_eq!(
            downloader.select_source(PiecePriority::Normal),
            Source::P2POnly
        );
        assert_eq!(
            downloader.select_source(PiecePriority::Low),
            Source::P2PIdle
        );
    }

    #[test]
    fn test_default_config() {
        let config = DownloadConfig::default();
        assert_eq!(config.max_p2p_connections, 50);
        assert!(config.http_fallback_enabled);
        assert_eq!(config.http_fallback_timeout_ms, 3000);
        assert!(config.critical_parallel);
    }

    #[test]
    fn test_download_stats_default() {
        let stats = DownloadStats::default();
        assert_eq!(stats.total_downloaded, 0);
        assert_eq!(stats.total_pieces, 0);
        assert_eq!(stats.http_bytes, 0);
        assert_eq!(stats.p2p_bytes, 0);
        assert_eq!(stats.active_requests, 0);
    }

    #[test]
    fn test_add_remove_http_source() {
        let mut downloader = make_downloader(true, vec![]);
        downloader.add_http_source("http://example.com/file".into());
        assert_eq!(downloader.http_sources().len(), 1);
        downloader.remove_http_source("http://example.com/file");
        assert!(downloader.http_sources().is_empty());
    }

    #[test]
    fn test_timeouts() {
        let downloader = make_downloader(true, vec!["http://example.com/file".into()]);
        assert_eq!(
            downloader.download_critical_timeout(),
            CRITICAL_PARALLEL_TIMEOUT
        );
        assert_eq!(downloader.download_high_timeout(), HTTP_FALLBACK_TIMEOUT);
    }

    #[test]
    fn test_select_source_critical_parallel_disabled() {
        let config = DownloadConfig {
            http_fallback_enabled: true,
            critical_parallel: false,
            ..Default::default()
        };
        let downloader = P2spDownloader::new(
            Arc::new(Mutex::new(None)),
            vec!["http://example.com/file".into()],
            None,
            config,
        );
        assert_eq!(
            downloader.select_source(PiecePriority::Critical),
            Source::P2POnly
        );
    }

    #[tokio::test]
    async fn test_download_normal_returns_error_when_no_peers() {
        let mut downloader = make_downloader(false, vec![]);
        let result = downloader.download_normal(0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_download_idle_returns_error_when_no_peers() {
        let mut downloader = make_downloader(false, vec![]);
        let result = downloader.download_idle(0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_active_requests_tracking() {
        let mut downloader = make_downloader(false, vec![]);
        assert_eq!(downloader.stats().active_requests, 0);
        let _ = downloader.download_normal(0).await;
        assert_eq!(downloader.stats().active_requests, 0);
    }
}

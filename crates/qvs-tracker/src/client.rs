use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use qvs_core::{AnnounceEvent, InfoHash, PeerInfo, QvodError, SwarmStatus};

use crate::protocol::{AnnounceParams, AnnounceResponse};
use crate::scraper::{build_scrape_url, parse_scrape_response};

use rand::seq::SliceRandom;

pub struct TrackerConfig {
    pub tracker_urls: Vec<String>,
    pub peer_id: [u8; 20],
    pub port: u16,
    pub compact: bool,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            tracker_urls: vec!["http://tracker.qvod.com:6969/announce".into()],
            peer_id: [0u8; 20],
            port: qvs_core::DEFAULT_PORT,
            compact: true,
        }
    }
}

pub struct TrackerClient {
    config: TrackerConfig,
    client: reqwest::Client,
}

impl TrackerClient {
    #[allow(clippy::missing_panics_doc, clippy::expect_used)]
    #[must_use]
    pub fn new(config: TrackerConfig) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("failed to create HTTP client");
        Self { config, client }
    }

    fn get_tracker_urls(&self) -> Vec<String> {
        let mut urls = self.config.tracker_urls.clone();
        urls.shuffle(&mut rand::thread_rng());
        urls
    }

    #[allow(clippy::missing_errors_doc)]
    async fn announce_single(
        &self,
        url: &str,
        info_hash: &InfoHash,
        event: AnnounceEvent,
        uploaded: u64,
        downloaded: u64,
        left: u64,
    ) -> Result<Vec<PeerInfo>, QvodError> {
        let params = AnnounceParams {
            info_hash: *info_hash,
            peer_id: self.config.peer_id,
            port: self.config.port,
            uploaded,
            downloaded,
            left,
            event,
            compact: self.config.compact,
        };
        let announce_url = format!("{}?{}", url.trim_end_matches('/'), params.to_query());
        let response = self
            .client
            .get(&announce_url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| QvodError::TrackerProtocol(format!("request failed: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(QvodError::TrackerProtocol(format!(
                "tracker returned {status}"
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| QvodError::TrackerProtocol(format!("read response: {e}")))?;
        let announce_resp = AnnounceResponse::from_bencode(&bytes)?;
        let peers: Vec<PeerInfo> = announce_resp
            .peers
            .into_iter()
            .map(|(ip_bytes, port)| {
                let ip = if ip_bytes.len() == 4 {
                    IpAddr::V4(Ipv4Addr::new(
                        ip_bytes[0],
                        ip_bytes[1],
                        ip_bytes[2],
                        ip_bytes[3],
                    ))
                } else {
                    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
                };
                PeerInfo {
                    peer_id: [0u8; 20],
                    addr: SocketAddr::new(ip, port),
                    is_firewalled: false,
                    bw_up: 0,
                    bw_down: 0,
                    location: None,
                    latency: Duration::default(),
                }
            })
            .collect();
        Ok(peers)
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn announce(
        &self,
        info_hash: &InfoHash,
        event: AnnounceEvent,
        uploaded: u64,
        downloaded: u64,
        left: u64,
    ) -> Result<Vec<PeerInfo>, QvodError> {
        let urls = self.get_tracker_urls();
        let mut last_err = None;
        for url in &urls {
            for attempt in 0..3 {
                match self
                    .announce_single(url, info_hash, event, uploaded, downloaded, left)
                    .await
                {
                    Ok(peers) => return Ok(peers),
                    Err(e) => {
                        last_err = Some(e);
                        if attempt < 2 {
                            let delay = Duration::from_secs(1 << attempt);
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
            }
            if let Some(ref e) = last_err {
                tracing::warn!("Tracker {url} failed after retries: {e}");
            }
        }
        Err(last_err.unwrap_or(QvodError::TrackerTimeout))
    }

    #[allow(clippy::missing_errors_doc)]
    async fn scrape_single(
        &self,
        url: &str,
        info_hashes: &[InfoHash],
    ) -> Result<Vec<(InfoHash, SwarmStatus)>, QvodError> {
        let scrape_url = build_scrape_url(url, info_hashes);
        let response = self
            .client
            .get(&scrape_url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| QvodError::TrackerProtocol(format!("scrape request failed: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(QvodError::TrackerProtocol(format!(
                "scrape returned {status}"
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| QvodError::TrackerProtocol(format!("read scrape response: {e}")))?;
        parse_scrape_response(&bytes)
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn scrape(
        &self,
        info_hashes: &[InfoHash],
    ) -> Result<Vec<(InfoHash, SwarmStatus)>, QvodError> {
        let urls = self.get_tracker_urls();
        let mut last_err = None;
        for url in &urls {
            for attempt in 0..3 {
                match self.scrape_single(url, info_hashes).await {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        last_err = Some(e);
                        if attempt < 2 {
                            let delay = Duration::from_secs(1 << attempt);
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
            }
            if let Some(ref e) = last_err {
                tracing::warn!("Tracker scrape {url} failed after retries: {e}");
            }
        }
        Err(last_err.unwrap_or(QvodError::TrackerTimeout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_config_default() {
        let config = TrackerConfig::default();
        assert_eq!(config.port, 8621);
        assert!(config.compact);
    }

    #[test]
    fn test_tracker_url_shuffling() {
        let config = TrackerConfig {
            tracker_urls: vec![
                "http://tracker1:6969/announce".into(),
                "http://tracker2:6969/announce".into(),
                "http://tracker3:6969/announce".into(),
            ],
            ..Default::default()
        };
        let client = TrackerClient::new(config);
        let urls = client.get_tracker_urls();
        assert_eq!(urls.len(), 3);
        assert!(urls.contains(&"http://tracker1:6969/announce".into()));
        assert!(urls.contains(&"http://tracker2:6969/announce".into()));
        assert!(urls.contains(&"http://tracker3:6969/announce".into()));
    }

    #[tokio::test]
    async fn test_announce_all_trackers_fail() {
        let config = TrackerConfig {
            tracker_urls: vec![
                "http://127.0.0.1:1/announce".into(),
                "http://127.0.0.1:2/announce".into(),
            ],
            ..Default::default()
        };
        let client = TrackerClient::new(config);
        let result = client
            .announce(&InfoHash([0u8; 20]), AnnounceEvent::Started, 0, 0, 1000)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scrape_all_trackers_fail() {
        let config = TrackerConfig {
            tracker_urls: vec![
                "http://127.0.0.1:1/announce".into(),
                "http://127.0.0.1:2/announce".into(),
            ],
            ..Default::default()
        };
        let client = TrackerClient::new(config);
        let result = client.scrape(&[InfoHash([0xAB; 20])]).await;
        assert!(result.is_err());
    }
}

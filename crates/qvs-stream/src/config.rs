use std::path::PathBuf;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub listen_port: u16,
    pub udp_port: u16,
    pub max_connections: u32,
    pub buffer_capacity_mb: u32,
    pub cache_dir: PathBuf,
    pub tracker_urls: Vec<String>,
    pub dht_seed_nodes: Vec<String>,
    pub http_fallback: bool,
    pub dht_enabled: bool,
    pub tracker_enabled: bool,
    pub cache_enabled: bool,
    pub max_peers_per_stream: u32,
    pub download_timeout_secs: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            listen_port: 8621,
            udp_port: 8621,
            max_connections: 50,
            buffer_capacity_mb: 64,
            cache_dir: PathBuf::from("/tmp/qvs-cache"),
            tracker_urls: vec!["http://tracker.qvod.com:8621/announce".into()],
            dht_seed_nodes: vec!["router.bittorrent.com:6881".into()],
            http_fallback: true,
            dht_enabled: true,
            tracker_enabled: true,
            cache_enabled: true,
            max_peers_per_stream: 50,
            download_timeout_secs: 30,
        }
    }
}

impl EngineConfig {
    #[must_use]
    pub fn load(_path: &str) -> Result<Self, qvs_core::QvodError> {
        Ok(Self::default())
    }

    #[must_use]
    pub fn buffer_capacity(&self) -> u64 {
        u64::from(self.buffer_capacity_mb) * 1024 * 1024
    }
}

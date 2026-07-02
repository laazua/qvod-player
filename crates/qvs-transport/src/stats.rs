use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct TransportStats {
    pub total_connections: u64,
    pub active_connections: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub average_rtt: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub total_peers: usize,
    pub connected_peers: usize,
    pub upload_peers: usize,
    pub download_peers: usize,
    pub idle_peers: usize,
}

#[derive(Debug, Clone)]
pub struct PeerConnectionStats {
    pub speed_down: f64,
    pub speed_up: f64,
    pub rtt: Duration,
    pub loss_rate: f64,
    pub total_downloaded: u64,
    pub total_uploaded: u64,
    pub connected_at: std::time::Instant,
    pub last_active: std::time::Instant,
}

impl Default for PeerConnectionStats {
    fn default() -> Self {
        Self {
            speed_down: 0.0,
            speed_up: 0.0,
            rtt: Duration::from_millis(100),
            loss_rate: 0.0,
            total_downloaded: 0,
            total_uploaded: 0,
            connected_at: std::time::Instant::now(),
            last_active: std::time::Instant::now(),
        }
    }
}
